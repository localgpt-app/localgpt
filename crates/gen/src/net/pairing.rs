//! PIN pairing for collaborative sessions.
//!
//! Each hosted session generates a random netcode private key that never
//! leaves the host, plus a short PIN shown on the host console. A joining
//! client proves it knows the PIN and receives a host-minted netcode
//! [`ConnectToken`] — the only way to obtain one.
//!
//! The PIN is short, so it is never used directly as a key (a LAN sniffer
//! could brute-force a PIN-derived key offline). Instead both sides run
//! **SPAKE2** (a password-authenticated key exchange) keyed by the PIN:
//!
//! 1. `POST /pair/start {client_msg}` → `{session, host_msg}`. Both sides
//!    now hold a shared key `K` that is only equal if the PINs matched. A
//!    passive observer learns nothing about the PIN or `K`.
//! 2. `POST /pair/finish {session, confirm, server_addr}` — `confirm` proves
//!    the client derived the same `K`. The host checks it (each wrong
//!    `confirm` is one online guess; five rotate the PIN), then returns its
//!    own confirmation plus the connect token sealed with
//!    ChaCha20-Poly1305 under a key derived from `K`.
//! 3. The client verifies the host's confirmation (so a rogue host that
//!    doesn't know the PIN can't impersonate the session) and opens the
//!    token.
//!
//! Tokens expire after [`TOKEN_EXPIRE_SECS`], so a client pairs once per
//! connection attempt.

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use lightyear::netcode::ConnectToken;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spake2::{Ed25519Group, Identity, Password, Spake2};

/// Netcode private key type (32 bytes).
pub type Key = [u8; 32];

/// Connect tokens are only valid for this long after pairing.
pub const TOKEN_EXPIRE_SECS: i32 = 60;
/// Wrong PIN confirmations before the host rotates its PIN.
pub const MAX_FAILURES_PER_PIN: u32 = 5;
/// Pairing starts accepted per rolling minute (all clients combined).
pub const MAX_STARTS_PER_MINUTE: usize = 20;
/// Unfinished exchanges are dropped after this long.
const PENDING_TTL: Duration = Duration::from_secs(60);
/// Most unfinished exchanges held at once.
const MAX_PENDING: usize = 16;

const SPAKE_IDENTITY: &[u8] = b"localgpt-gen/pair/v1";

// ---------------------------------------------------------------------------
// Wire types (JSON over HTTP)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairInfo {
    /// Whether `/pair/*` must be used (false for `--open` sessions).
    pub pairing_required: bool,
    pub protocol_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PairStartRequest {
    pub client_msg: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PairStartResponse {
    pub session: String,
    pub host_msg: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PairFinishRequest {
    pub session: String,
    pub confirm: String,
    /// The address the client will connect to (goes into the token).
    pub server_addr: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PairFinishResponse {
    pub host_confirm: String,
    pub nonce: String,
    pub sealed_token: String,
}

/// Why pairing failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairError {
    WrongPin,
    TooManyAttempts,
    UnknownSession,
    BadRequest(String),
    Internal(String),
    /// Client side: the host's confirmation didn't verify (not the host we
    /// paired with, or a different PIN).
    HostNotAuthenticated,
}

impl std::fmt::Display for PairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongPin => write!(f, "wrong PIN"),
            Self::TooManyAttempts => write!(f, "too many pairing attempts — wait a minute"),
            Self::UnknownSession => write!(f, "pairing exchange expired — try again"),
            Self::BadRequest(m) => write!(f, "bad pairing request: {m}"),
            Self::Internal(m) => write!(f, "pairing failed: {m}"),
            Self::HostNotAuthenticated => {
                write!(f, "the host could not prove it knows the PIN")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared crypto
// ---------------------------------------------------------------------------

/// Keys derived from the SPAKE2 output, bound to both handshake messages.
struct Derived {
    client_confirm: [u8; 32],
    host_confirm: [u8; 32],
    enc_key: [u8; 32],
}

fn derive(k: &[u8], client_msg: &[u8], host_msg: &[u8]) -> Derived {
    let label = |tag: &[u8]| -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"localgpt-gen/pair/v1/");
        h.update(tag);
        h.update((k.len() as u32).to_le_bytes());
        h.update(k);
        h.update((client_msg.len() as u32).to_le_bytes());
        h.update(client_msg);
        h.update((host_msg.len() as u32).to_le_bytes());
        h.update(host_msg);
        h.finalize().into()
    };
    Derived {
        client_confirm: label(b"client-confirm"),
        host_confirm: label(b"host-confirm"),
        enc_key: label(b"enc"),
    }
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn b64d(s: &str) -> Result<Vec<u8>, PairError> {
    B64.decode(s)
        .map_err(|e| PairError::BadRequest(e.to_string()))
}

/// Uniform random 6-digit PIN.
pub fn generate_pin() -> String {
    format!("{:06}", rand::random_range(0..1_000_000u32))
}

/// Fresh random netcode private key for a session.
pub fn generate_private_key() -> Key {
    rand::random()
}

/// `123456` → `123 456` for display.
pub fn format_pin(pin: &str) -> String {
    if pin.len() == 6 {
        format!("{} {}", &pin[..3], &pin[3..])
    } else {
        pin.to_string()
    }
}

/// Accept `123456`, `123 456`, `123-456`.
pub fn normalize_pin(input: &str) -> String {
    input.chars().filter(|c| c.is_ascii_digit()).collect()
}

// ---------------------------------------------------------------------------
// Host side
// ---------------------------------------------------------------------------

struct Pending {
    derived: Derived,
    created: Instant,
}

struct HostInner {
    pin: String,
    failures: u32,
    pending: HashMap<String, Pending>,
    recent_starts: VecDeque<Instant>,
}

/// Host-side pairing state, shared with the HTTP handlers.
pub struct PairingHost {
    private_key: Key,
    protocol_id: u64,
    inner: Mutex<HostInner>,
    /// Called with the new PIN whenever it rotates.
    on_rotate: Box<dyn Fn(&str) + Send + Sync>,
}

impl PairingHost {
    pub fn new(
        private_key: Key,
        protocol_id: u64,
        pin: String,
        on_rotate: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        Self {
            private_key,
            protocol_id,
            inner: Mutex::new(HostInner {
                pin,
                failures: 0,
                pending: HashMap::new(),
                recent_starts: VecDeque::new(),
            }),
            on_rotate: Box::new(on_rotate),
        }
    }

    pub fn pin(&self) -> String {
        self.inner.lock().map(|i| i.pin.clone()).unwrap_or_default()
    }

    /// Step 1: run SPAKE2 against the client's message.
    pub fn start(&self, req: &PairStartRequest) -> Result<PairStartResponse, PairError> {
        let client_msg = b64d(&req.client_msg)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| PairError::Internal("lock poisoned".into()))?;
        let now = Instant::now();
        while inner
            .recent_starts
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60))
        {
            inner.recent_starts.pop_front();
        }
        if inner.recent_starts.len() >= MAX_STARTS_PER_MINUTE {
            return Err(PairError::TooManyAttempts);
        }
        inner.recent_starts.push_back(now);
        inner
            .pending
            .retain(|_, p| now.duration_since(p.created) < PENDING_TTL);
        if inner.pending.len() >= MAX_PENDING {
            return Err(PairError::TooManyAttempts);
        }

        let (spake, host_msg) = Spake2::<Ed25519Group>::start_symmetric(
            &Password::new(inner.pin.as_bytes()),
            &Identity::new(SPAKE_IDENTITY),
        );
        let k = spake
            .finish(&client_msg)
            .map_err(|e| PairError::BadRequest(format!("{e:?}")))?;
        let session = hex::encode(rand::random::<[u8; 16]>());
        inner.pending.insert(
            session.clone(),
            Pending {
                derived: derive(&k, &client_msg, &host_msg),
                created: now,
            },
        );
        Ok(PairStartResponse {
            session,
            host_msg: B64.encode(host_msg),
        })
    }

    /// Step 2: check the client's key confirmation and mint a token.
    pub fn finish(&self, req: &PairFinishRequest) -> Result<PairFinishResponse, PairError> {
        let confirm = b64d(&req.confirm)?;
        let server_addr: SocketAddr = req
            .server_addr
            .parse()
            .map_err(|_| PairError::BadRequest("server_addr".into()))?;

        let pending = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| PairError::Internal("lock poisoned".into()))?;
            let pending = inner
                .pending
                .remove(&req.session)
                .filter(|p| p.created.elapsed() < PENDING_TTL)
                .ok_or(PairError::UnknownSession)?;
            if !ct_eq(&confirm, &pending.derived.client_confirm) {
                inner.failures += 1;
                if inner.failures >= MAX_FAILURES_PER_PIN {
                    inner.failures = 0;
                    inner.pin = generate_pin();
                    inner.pending.clear();
                    (self.on_rotate)(&inner.pin);
                }
                return Err(PairError::WrongPin);
            }
            pending
        };

        let client_id: u64 = rand::random();
        let token = ConnectToken::build(server_addr, self.protocol_id, client_id, self.private_key)
            .expire_seconds(TOKEN_EXPIRE_SECS)
            .generate()
            .map_err(|e| PairError::Internal(e.to_string()))?
            .try_into_bytes()
            .map_err(|e| PairError::Internal(e.to_string()))?;

        let nonce_bytes: [u8; 12] = rand::random();
        let cipher = ChaCha20Poly1305::new((&pending.derived.enc_key).into());
        let sealed = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: &token,
                    aad: req.session.as_bytes(),
                },
            )
            .map_err(|_| PairError::Internal("seal failed".into()))?;

        Ok(PairFinishResponse {
            host_confirm: B64.encode(pending.derived.host_confirm),
            nonce: B64.encode(nonce_bytes),
            sealed_token: B64.encode(sealed),
        })
    }
}

/// HTTP routes for pairing (merged into the session's HTTP server).
pub fn pairing_router(host: Option<Arc<PairingHost>>, protocol_id: u64) -> axum::Router {
    use axum::Json;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};

    type St = (Option<Arc<PairingHost>>, u64);

    fn status(e: &PairError) -> StatusCode {
        match e {
            PairError::WrongPin | PairError::HostNotAuthenticated => StatusCode::FORBIDDEN,
            PairError::TooManyAttempts => StatusCode::TOO_MANY_REQUESTS,
            PairError::UnknownSession => StatusCode::GONE,
            PairError::BadRequest(_) => StatusCode::BAD_REQUEST,
            PairError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    async fn info(State((host, protocol_id)): State<St>) -> Json<PairInfo> {
        Json(PairInfo {
            pairing_required: host.is_some(),
            protocol_id,
        })
    }

    async fn start(State((host, _)): State<St>, Json(req): Json<PairStartRequest>) -> Response {
        let Some(host) = host else {
            return StatusCode::NOT_FOUND.into_response();
        };
        match host.start(&req) {
            Ok(resp) => Json(resp).into_response(),
            Err(e) => (status(&e), e.to_string()).into_response(),
        }
    }

    async fn finish(State((host, _)): State<St>, Json(req): Json<PairFinishRequest>) -> Response {
        let Some(host) = host else {
            return StatusCode::NOT_FOUND.into_response();
        };
        match host.finish(&req) {
            Ok(resp) => {
                eprintln!("[net] Client paired");
                Json(resp).into_response()
            }
            Err(e) => {
                if e == PairError::WrongPin {
                    eprintln!("[net] Pairing attempt with a wrong PIN");
                }
                (status(&e), e.to_string()).into_response()
            }
        }
    }

    axum::Router::new()
        .route("/pair/info", get(info))
        .route("/pair/start", post(start))
        .route("/pair/finish", post(finish))
        .with_state((host, protocol_id))
}

// ---------------------------------------------------------------------------
// Client side
// ---------------------------------------------------------------------------

/// Client half of the exchange, transport-free (unit-testable).
pub struct PairingClient {
    spake: Option<Spake2<Ed25519Group>>,
    client_msg: Vec<u8>,
    derived: Option<Derived>,
    session: String,
}

impl PairingClient {
    pub fn start(pin: &str) -> (Self, PairStartRequest) {
        let (spake, client_msg) = Spake2::<Ed25519Group>::start_symmetric(
            &Password::new(normalize_pin(pin).as_bytes()),
            &Identity::new(SPAKE_IDENTITY),
        );
        let req = PairStartRequest {
            client_msg: B64.encode(&client_msg),
        };
        (
            Self {
                spake: Some(spake),
                client_msg,
                derived: None,
                session: String::new(),
            },
            req,
        )
    }

    pub fn finish_request(
        &mut self,
        resp: &PairStartResponse,
        server_addr: SocketAddr,
    ) -> Result<PairFinishRequest, PairError> {
        let host_msg = b64d(&resp.host_msg)?;
        let k = self
            .spake
            .take()
            .ok_or_else(|| PairError::Internal("exchange already finished".into()))?
            .finish(&host_msg)
            .map_err(|e| PairError::BadRequest(format!("{e:?}")))?;
        let derived = derive(&k, &self.client_msg, &host_msg);
        let confirm = B64.encode(derived.client_confirm);
        self.derived = Some(derived);
        self.session = resp.session.clone();
        Ok(PairFinishRequest {
            session: resp.session.clone(),
            confirm,
            server_addr: server_addr.to_string(),
        })
    }

    /// Verify the host and open the sealed token.
    pub fn open_token(&self, resp: &PairFinishResponse) -> Result<ConnectToken, PairError> {
        let derived = self
            .derived
            .as_ref()
            .ok_or_else(|| PairError::Internal("finish_request not called".into()))?;
        if !ct_eq(&b64d(&resp.host_confirm)?, &derived.host_confirm) {
            return Err(PairError::HostNotAuthenticated);
        }
        let nonce = b64d(&resp.nonce)?;
        if nonce.len() != 12 {
            return Err(PairError::BadRequest("nonce".into()));
        }
        let cipher = ChaCha20Poly1305::new((&derived.enc_key).into());
        let token = cipher
            .decrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: &b64d(&resp.sealed_token)?,
                    aad: self.session.as_bytes(),
                },
            )
            .map_err(|_| PairError::HostNotAuthenticated)?;
        ConnectToken::try_from_bytes(&token).map_err(|e| PairError::Internal(format!("{e:?}")))
    }
}

/// Query whether a host requires pairing. Blocking.
pub fn fetch_info(http_base: &str) -> Result<PairInfo, String> {
    block_on_http(async {
        let resp = reqwest::Client::new()
            .get(format!("{http_base}/pair/info"))
            .timeout(Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        resp.json::<PairInfo>().await.map_err(|e| e.to_string())
    })
}

/// Run the full pairing exchange against a host. Blocking.
pub fn pair_with_host(
    http_base: &str,
    server_addr: SocketAddr,
    pin: &str,
) -> Result<ConnectToken, PairError> {
    let http = |e: reqwest::Error| PairError::Internal(e.to_string());
    let from_status = |status: reqwest::StatusCode, body: String| match status.as_u16() {
        403 => PairError::WrongPin,
        429 => PairError::TooManyAttempts,
        410 => PairError::UnknownSession,
        _ => PairError::Internal(format!("HTTP {status}: {body}")),
    };
    block_on_http(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(http)?;
        let (mut pairing, start_req) = PairingClient::start(pin);

        let resp = client
            .post(format!("{http_base}/pair/start"))
            .json(&start_req)
            .send()
            .await
            .map_err(http)?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(from_status(status, resp.text().await.unwrap_or_default()));
        }
        let start_resp: PairStartResponse = resp.json().await.map_err(http)?;
        let finish_req = pairing.finish_request(&start_resp, server_addr)?;

        let resp = client
            .post(format!("{http_base}/pair/finish"))
            .json(&finish_req)
            .send()
            .await
            .map_err(http)?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(from_status(status, resp.text().await.unwrap_or_default()));
        }
        let finish_resp: PairFinishResponse = resp.json().await.map_err(http)?;
        pairing.open_token(&finish_resp)
    })
}

fn block_on_http<T, E: From<String>>(
    fut: impl std::future::Future<Output = Result<T, E>>,
) -> Result<T, E> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| E::from(e.to_string()))?
        .block_on(fut)
}

impl From<String> for PairError {
    fn from(s: String) -> Self {
        Self::Internal(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(pin: &str) -> PairingHost {
        PairingHost::new(generate_private_key(), 3, pin.to_string(), |_| {})
    }

    fn addr() -> SocketAddr {
        "192.168.1.5:9879".parse().unwrap()
    }

    fn run(host: &PairingHost, pin: &str) -> Result<ConnectToken, PairError> {
        let (mut client, start) = PairingClient::start(pin);
        let start_resp = host.start(&start)?;
        let finish = client.finish_request(&start_resp, addr())?;
        let finish_resp = host.finish(&finish)?;
        client.open_token(&finish_resp)
    }

    #[test]
    fn correct_pin_yields_token() {
        let host = host("482913");
        let token = run(&host, "482 913").expect("pairing should succeed");
        // The token round-trips and is bound to our protocol.
        let bytes = token.try_into_bytes().unwrap();
        assert!(ConnectToken::try_from_bytes(&bytes).is_ok());
    }

    #[test]
    fn wrong_pin_rejected_and_rotates() {
        let rotated = Arc::new(Mutex::new(None::<String>));
        let seen = rotated.clone();
        let host = PairingHost::new(generate_private_key(), 3, "111111".into(), move |p| {
            *seen.lock().unwrap() = Some(p.to_string());
        });
        for _ in 0..MAX_FAILURES_PER_PIN - 1 {
            assert_eq!(run(&host, "222222").err(), Some(PairError::WrongPin));
            assert_eq!(host.pin(), "111111");
        }
        assert_eq!(run(&host, "222222").err(), Some(PairError::WrongPin));
        let new_pin = rotated.lock().unwrap().clone().expect("PIN should rotate");
        assert_eq!(host.pin(), new_pin);
        // The old PIN no longer works (unless the rotation happened to
        // reproduce it, a one-in-a-million event).
        if new_pin != "111111" {
            assert_eq!(run(&host, "111111").err(), Some(PairError::WrongPin));
        }
        assert!(run(&host, &new_pin).is_ok());
    }

    #[test]
    fn session_is_single_use() {
        let host = host("123456");
        let (mut client, start) = PairingClient::start("123456");
        let start_resp = host.start(&start).unwrap();
        let finish = client.finish_request(&start_resp, addr()).unwrap();
        assert!(host.finish(&finish).is_ok());
        assert_eq!(host.finish(&finish).err(), Some(PairError::UnknownSession));
    }

    #[test]
    fn rogue_host_detected() {
        // A host with a different PIN can't produce a valid confirmation,
        // even if it skipped its own confirm check.
        let real = host("123456");
        let rogue = host("654321");
        let (mut client, start) = PairingClient::start("123456");
        let start_resp = rogue.start(&start).unwrap();
        let finish = client.finish_request(&start_resp, addr()).unwrap();
        assert_eq!(rogue.finish(&finish).err(), Some(PairError::WrongPin));
        // Forge a response from the real host's point of view: still fails.
        let (mut c2, s2) = PairingClient::start("123456");
        let r2 = real.start(&s2).unwrap();
        let f2 = c2.finish_request(&r2, addr()).unwrap();
        let mut resp = real.finish(&f2).unwrap();
        resp.host_confirm = B64.encode([0u8; 32]);
        assert_eq!(
            c2.open_token(&resp).err(),
            Some(PairError::HostNotAuthenticated)
        );
    }

    #[test]
    fn start_throttle() {
        let host = host("123456");
        for _ in 0..MAX_PENDING {
            let (_, start) = PairingClient::start("123456");
            host.start(&start).unwrap();
        }
        let (_, start) = PairingClient::start("123456");
        assert_eq!(host.start(&start).err(), Some(PairError::TooManyAttempts));
    }

    #[test]
    fn pin_helpers() {
        let pin = generate_pin();
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(format_pin("123456"), "123 456");
        assert_eq!(normalize_pin(" 123-456 "), "123456");
    }
}
