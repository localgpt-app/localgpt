//! Browser guests for hosted sessions (spec phases 1–3).
//!
//! A hosting Gen serves a join page and a WebSocket endpoint on the session
//! HTTP port (the same axum server as pairing and assets). A browser that
//! opens the invite link becomes a guest: it receives the world, sees every
//! later change as ops, shows up in presence, chats, and can prompt the
//! room's AI through the shared job queue.
//!
//! The room's state lives in a [`localgpt_world_sync::Authority`] (the
//! document, peers, revisions and jobs); this module is only the transport:
//! sockets on the session-HTTP thread, channels into Bevy, and a projection
//! that turns the live scene into ops (so every tool, the inspector and
//! undo/redo sync without per-tool instrumentation).

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use bevy::prelude::*;
use futures::{SinkExt, StreamExt};
use localgpt_world_sync as sync;
use localgpt_world_sync::{
    Authority, AuthorityEvent, ClientMsg, Limits, Outbound, PeerId, Recipients, Role, ServerMsg,
};
use localgpt_world_types as wt;
use tokio::sync::mpsc;

use crate::gen3d::audio::AudioEmitter;
use crate::gen3d::behaviors::EntityBehaviors;
use crate::gen3d::plugin::{SnapshotQueries, snapshot_entity};
use crate::gen3d::registry::{GenEntity, GenEntityType, GltfSource, NameRegistry, ParametricShape};

/// WebSocket endpoint path on the session HTTP server.
pub const SESSION_ROUTE: &str = "/session";

/// How often the live scene is projected into the document and diffed.
const PROJECTION_INTERVAL: Duration = Duration::from_millis(250);

/// Committed revisions between package snapshots (keyframes for seeking).
const SNAPSHOT_EVERY: u64 = 500;

/// Largest accepted WebSocket message (submit caps at 256 ops; the authority
/// rejects anything bigger at the document level anyway).
const MAX_WS_MESSAGE_BYTES: usize = 1024 * 1024;

/// Wrong PINs a session takes before it stops taking the PIN at all: six
/// digits can be guessed, and a connection costs nothing. The invite token
/// (128 bits) stays good, and a new session gets a new PIN.
const PIN_GUESSES: u32 = 5;

/// What a hello with the PIN is told once the session no longer takes it.
const PIN_LOCKED: &str = "the PIN is no longer taken: too many wrong ones";

// ---------------------------------------------------------------------------
// The bridge between the session-HTTP thread and Bevy
// ---------------------------------------------------------------------------

/// One frame to write to a connection's socket.
pub(crate) struct OutboundFrame {
    pub(crate) text: String,
    /// Close after sending (protocol errors).
    pub(crate) close: bool,
}

struct ConnTx {
    tx: mpsc::UnboundedSender<OutboundFrame>,
    /// Came through `localgpt-relay`, where anyone with the room code can
    /// open one: it is let in by the invite token alone, never the PIN.
    via_relay: bool,
}

/// Events from socket tasks into Bevy.
pub enum InboundEvent {
    Connected { id: PeerId },
    Disconnected { id: PeerId },
    Message { id: PeerId, msg: ClientMsg },
}

/// Shared with the axum handlers on the session-HTTP thread. Cheap to clone.
#[derive(Clone)]
pub struct WebBridge {
    inbound: mpsc::UnboundedSender<InboundEvent>,
    conns: Arc<StdMutex<HashMap<PeerId, ConnTx>>>,
    next_conn: Arc<AtomicU64>,
    /// Invite-link bearer token; `None` for open sessions.
    token: Option<String>,
    /// Session name, for `/session-info`.
    session_name: String,
}

impl WebBridge {
    /// A bridge plus the receiver Bevy drains.
    pub fn new(
        token: Option<String>,
        session_name: &str,
    ) -> (Self, mpsc::UnboundedReceiver<InboundEvent>) {
        let (inbound, rx) = mpsc::unbounded_channel();
        (
            Self {
                inbound,
                conns: Arc::new(StdMutex::new(HashMap::new())),
                next_conn: Arc::new(AtomicU64::new(0)),
                token,
                session_name: session_name.to_string(),
            },
            rx,
        )
    }
}

impl WebBridge {
    /// Allocate a peer id for a connection arriving via the relay.
    pub(crate) fn alloc_peer_id(&self) -> PeerId {
        self.next_conn.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Register a relay guest's frame channel as a connection.
    pub(crate) fn insert_conn(&self, id: PeerId, tx: mpsc::UnboundedSender<OutboundFrame>) {
        self.conns.lock().expect("conns lock poisoned").insert(
            id,
            ConnTx {
                tx,
                via_relay: true,
            },
        );
    }

    /// Whether a connection came through the relay.
    fn via_relay(&self, id: PeerId) -> bool {
        self.conns
            .lock()
            .expect("conns lock poisoned")
            .get(&id)
            .is_some_and(|conn| conn.via_relay)
    }

    /// Drop a relay guest's connection.
    pub(crate) fn remove_conn(&self, id: PeerId) {
        self.conns.lock().expect("conns lock poisoned").remove(&id);
    }

    /// Feed an inbound event (used by the relay adapter).
    pub(crate) fn send_inbound(
        &self,
        event: InboundEvent,
    ) -> Result<(), mpsc::error::SendError<InboundEvent>> {
        self.inbound.send(event)
    }
}

/// The room, as a Bevy resource. Inserted when a session starts with web
/// enabled; the systems below run only while it exists.
#[derive(Resource)]
pub struct WebRoom {
    pub authority: Authority,
    pub bridge: WebBridge,
    inbound_rx: StdMutex<mpsc::UnboundedReceiver<InboundEvent>>,
    projection_timer: Timer,
    warned_bad_projection: bool,
    /// The room's build history, appended on every committed batch.
    op_log: Option<OpLog>,
    /// The session's directory (the package), when its history persists.
    session_dir: Option<PathBuf>,
    /// The revision of the newest snapshot written for this session.
    last_snapshot_rev: u64,
    /// The score state as last recorded in the log.
    last_scores: BTreeMap<String, i32>,
    /// A replayed op log is waiting to be spawned into the scene.
    scene_rebuild_pending: bool,
    /// Browser guests join as editors (direct edits) instead of guests.
    pub web_edit: bool,
    /// The session PIN, when this isn't an open session. Native guests
    /// authenticate with it (browsers use the invite token).
    session_pin: Option<String>,
    /// Wrong PINs so far; at [`PIN_GUESSES`] the PIN is no longer taken.
    wrong_pins: u32,
}

impl WebRoom {
    pub fn new(
        session_name: &str,
        bridge: WebBridge,
        inbound_rx: mpsc::UnboundedReceiver<InboundEvent>,
        workspace: &std::path::Path,
        resume: Option<&str>,
        web_edit: bool,
        session_pin: Option<String>,
    ) -> Self {
        let mut room = Self {
            authority: Authority::new(session_name, Limits::default()),
            bridge,
            inbound_rx: StdMutex::new(inbound_rx),
            projection_timer: Timer::new(PROJECTION_INTERVAL, TimerMode::Repeating),
            warned_bad_projection: false,
            op_log: None,
            session_dir: None,
            last_snapshot_rev: 0,
            last_scores: BTreeMap::new(),
            scene_rebuild_pending: false,
            web_edit,
            session_pin,
            wrong_pins: 0,
        };
        // History first: replay what a previous session left behind, then
        // open the log so this session's ops keep appending to it.
        if let Some(resume) = resume {
            let path = resolve_op_log_path(workspace, resume);
            room.replay_log(&path);
        }
        match OpLog::open(&session_op_log_path(workspace, session_name)) {
            Ok(log) => room.op_log = Some(log),
            Err(e) => eprintln!("web session: op log unavailable ({e}) — history won't persist"),
        }
        // The session is a package: a base world, metadata and snapshots
        // next to the log (docs/rfcs/multiplayer/session-package-format.md).
        room.session_dir = Some(session_dir_path(workspace, session_name));
        room.ensure_package();
        room
    }

    /// The PIN has been guessed at too often to be taken any more.
    fn pin_locked(&self) -> bool {
        self.wrong_pins >= PIN_GUESSES
    }

    /// A hello with the PIN joins: the session has one and it isn't locked.
    fn pin_taken(&self) -> bool {
        self.session_pin.is_some() && !self.pin_locked()
    }

    /// Make the session directory a package: write the base and metadata
    /// when they're missing, then refresh the metadata's head revision.
    ///
    /// A fresh directory gets the current document as its base. A legacy
    /// directory (a log, no metadata) keeps its log and gets an empty base
    /// at revision 0 — exactly what those sessions folded from.
    fn ensure_package(&mut self) {
        let Some(dir) = self.session_dir.clone() else {
            return;
        };
        match localgpt_world_agent::session::read_meta(&dir) {
            Ok(meta) => {
                self.last_snapshot_rev = meta.base_revision;
            }
            Err(_) => {
                let fresh = !dir.join(localgpt_world_agent::session::LOG_FILE).exists();
                let base = if fresh {
                    self.authority.doc().to_manifest()
                } else {
                    // History exists; the base is what it started from.
                    wt::WorldManifest::new(self.authority.doc().name.clone())
                };
                let revision = if fresh { self.authority.revision() } else { 0 };
                match localgpt_world_agent::session::write_base(&dir, &base, "gen", None, revision)
                {
                    Ok(_) => {
                        self.last_snapshot_rev = revision;
                        eprintln!(
                            "web session: package base written at revision {revision} ({}), dir {}",
                            if fresh { "fresh" } else { "legacy log" },
                            dir.display()
                        );
                    }
                    Err(e) => {
                        eprintln!("web session: package base unavailable ({e}) — continuing")
                    }
                }
            }
        }
        if let Err(e) = localgpt_world_agent::session::refresh_meta(&dir, self.authority.revision())
        {
            eprintln!("web session: session metadata unavailable ({e})");
        }
    }

    /// Append one history op (a tool call, a state change) to the session
    /// log at the current revision. History never bumps the revision.
    fn record_history(&self, op: sync::SessionOp, author: &str) {
        let Some(log) = &self.op_log else { return };
        let _ = log.append(&sync::OpLogEntry {
            revision: self.authority.revision(),
            author: sync::Author {
                peer: None,
                name: author.into(),
                source: None,
            },
            ops: vec![op],
            timestamp_ms: now_ms(),
            id: None,
            parent: None,
            message: None,
        });
    }

    /// Replay a previous session into the authority: a `.world` archive,
    /// a package directory (base + log, folded by the reader), a legacy
    /// session directory (a log, no metadata), or a bare `ops.jsonl`.
    fn replay_log(&mut self, path: &std::path::Path) {
        // A .world archive: extract to a temp dir and read it as a package.
        if path.is_file() && path.extension().is_some_and(|e| e == "world") {
            match localgpt_world_agent::session::extract_zip(path) {
                Ok(temp) => self.replay_log(temp.path()),
                Err(e) => eprintln!("web session: {e} — starting fresh"),
            }
            return;
        }
        let mut path = path.to_path_buf();
        if path.is_dir() {
            match localgpt_world_agent::session::read_package(&path, None) {
                Ok(pkg) => {
                    let head = pkg
                        .entries
                        .last()
                        .map_or(pkg.base_revision, |e| e.revision.max(pkg.base_revision));
                    let entities = pkg.doc.len();
                    if pkg.skipped_lines > 0 {
                        eprintln!(
                            "web session: skipped {} unreadable log lines",
                            pkg.skipped_lines
                        );
                    }
                    self.authority.load_base(pkg.doc, head);
                    self.scene_rebuild_pending = true;
                    eprintln!(
                        "web session: resumed package {} — base revision {}, head revision {}, {} entities",
                        path.display(),
                        pkg.base_revision,
                        head,
                        entities
                    );
                    return;
                }
                Err(localgpt_world_agent::session::PackageError::MissingMeta) => {
                    // A legacy session directory: a log, no metadata. Read
                    // the log directly, the way the room always did.
                    path = path.join(localgpt_world_agent::session::LOG_FILE);
                }
                Err(e) => eprintln!(
                    "web session: package at {} unreadable ({e}) — starting fresh",
                    path.display()
                ),
            }
            if path.is_dir() {
                return;
            }
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            eprintln!(
                "web session: no op log at {} — starting fresh",
                path.display()
            );
            return;
        };
        let mut applied = 0usize;
        for (n, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match sync::decode_line(line) {
                Ok(entry) => {
                    if let Err(e) = self.authority.apply_replay(&entry.edit_ops()) {
                        eprintln!(
                            "web session: op log line {} no longer applies ({e}) — replay stops here",
                            n + 1
                        );
                        break;
                    }
                    applied += 1;
                }
                Err(e) => eprintln!("web session: skipping unreadable log line {}: {e}", n + 1),
            }
        }
        if applied > 0 {
            self.scene_rebuild_pending = true;
            eprintln!(
                "web session: restored {} batches from {} — revision {}, {} entities",
                applied,
                path.display(),
                self.authority.revision(),
                self.authority.doc().len()
            );
        }
    }
}

/// Ops committed by the authority that the scene hasn't seen yet — undo,
/// guest edits. The projection covers scene→doc; this queue covers doc→scene.
#[derive(Resource, Default)]
pub struct PendingSceneOps(Vec<wt::EditOp>);

/// The room's op log: `<workspace>/sessions/<slug>/ops.jsonl`, one JSON
/// entry per committed batch (spec phase 5).
struct OpLog {
    file: std::fs::File,
}

impl OpLog {
    fn open(path: &std::path::Path) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self { file })
    }

    fn append(&self, entry: &sync::OpLogEntry) -> std::io::Result<()> {
        use std::io::Write as _;
        let mut line = sync::encode_line(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        line.push('\n');
        (&self.file).write_all(line.as_bytes())
    }
}

/// A filesystem-safe slug for a session name.
fn session_slug(name: &str) -> String {
    let slug = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug: String = slug.chars().take(48).collect();
    if slug.is_empty() {
        "session".to_string()
    } else {
        slug
    }
}

/// Where a session's package lives.
fn session_dir_path(workspace: &std::path::Path, session_name: &str) -> std::path::PathBuf {
    workspace.join("sessions").join(session_slug(session_name))
}

/// Where a session's op log lives.
fn session_op_log_path(workspace: &std::path::Path, session_name: &str) -> std::path::PathBuf {
    session_dir_path(workspace, session_name).join("ops.jsonl")
}

/// Resolve `--resume` / `--replay`: a session name (its standard directory),
/// a package directory, or an explicit path to an `ops.jsonl`.
pub fn resolve_op_log_path(workspace: &std::path::Path, resume: &str) -> std::path::PathBuf {
    if resume.contains('/') || resume.ends_with(".jsonl") || resume.ends_with(".world") {
        std::path::PathBuf::from(shellexpand::tilde(resume).as_ref())
    } else {
        session_dir_path(workspace, resume)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A fresh invite token (128 bits, hex). The join page carries it in the
/// URL fragment, which browsers never send in the HTTP request.
pub fn generate_web_token() -> String {
    let bytes: [u8; 16] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The host's primary LAN address, for printing the invite link. (The
/// connect trick picks the right interface without sending traffic.)
pub fn primary_lan_ip() -> Option<Ipv4Addr> {
    let sock = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    sock.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    match sock.local_addr().ok()?.ip() {
        IpAddr::V4(v4) => Some(v4),
        IpAddr::V6(_) => None,
    }
}

// ---------------------------------------------------------------------------
// HTTP routes (join page, static JS, the WebSocket endpoint)
// ---------------------------------------------------------------------------

/// The web routes, merged into the session HTTP server.
pub fn web_router(bridge: WebBridge) -> axum::Router {
    axum::Router::new()
        .route("/", get(join_page))
        .route("/world-viewer.js", get(viewer_js))
        .route("/cinematography.js", get(cinematography_js))
        .route("/session-client.js", get(client_js))
        .route("/session-info", get(session_info))
        .route("/vendor/three.module.js", get(vendor_three))
        .route(
            "/vendor/three/addons/controls/OrbitControls.js",
            get(vendor_orbit),
        )
        .route(
            "/vendor/three/addons/loaders/GLTFLoader.js",
            get(vendor_gltf),
        )
        .route(
            "/vendor/three/addons/utils/BufferGeometryUtils.js",
            get(vendor_bgu),
        )
        .route(SESSION_ROUTE, get(session_ws))
        .with_state(bridge)
}

/// Serve the session's HTTP routes on `addr` from a background thread.
/// Binds synchronously so the caller learns about port conflicts.
pub fn spawn_session_http(router: axum::Router, addr: std::net::SocketAddr) -> std::io::Result<()> {
    let listener = std::net::TcpListener::bind(addr)?;
    listener.set_nonblocking(true)?;
    std::thread::Builder::new()
        .name("gen-session-http".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Session HTTP runtime failed: {e}");
                    return;
                }
            };
            rt.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                    return;
                };
                if let Err(e) = axum::serve(listener, router).await {
                    eprintln!("Session HTTP server stopped: {e}");
                }
            });
        })?;
    Ok(())
}

async fn join_page() -> impl IntoResponse {
    Html(localgpt_world_export::html::SESSION_JOIN_PAGE_HTML)
}

async fn viewer_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        localgpt_world_export::html::WORLD_VIEWER_JS,
    )
}

/// The `./cinematography.js` the viewer imports, next to it.
async fn cinematography_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        localgpt_world_export::html::CINEMATOGRAPHY_JS,
    )
}

/// What a joiner needs to know before saying hello (protocol version,
/// session name, whether a secret is required).
async fn session_info(State(bridge): State<WebBridge>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "protocol": sync::PROTOCOL_VERSION,
        "name": bridge.session_name,
        "secret_required": bridge.token.is_some(),
    }))
}

async fn client_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        localgpt_world_export::html::SESSION_CLIENT_JS,
    )
}

/// Vendored three.js, served so the join page works with no internet.
async fn vendor_three() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        localgpt_world_export::html::THREE_MODULE_JS,
    )
}

async fn vendor_orbit() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        localgpt_world_export::html::ORBIT_CONTROLS_JS,
    )
}

async fn vendor_gltf() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        localgpt_world_export::html::GLTF_LOADER_JS,
    )
}

async fn vendor_bgu() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        localgpt_world_export::html::BUFFER_GEOMETRY_UTILS_JS,
    )
}

async fn session_ws(
    ws: WebSocketUpgrade,
    State(bridge): State<WebBridge>,
    headers: HeaderMap,
) -> Response {
    // Refuse cross-origin browser sockets: a page must come from this host's
    // own join page (or not be a browser at all), so a malicious site can't
    // drive a LAN session from a visitor's browser.
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let origin_authority = origin
            .split("://")
            .nth(1)
            .unwrap_or(origin)
            .trim_end_matches('/');
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if origin_authority != host {
            return (StatusCode::FORBIDDEN, "cross-origin sessions are refused").into_response();
        }
    }
    ws.max_message_size(MAX_WS_MESSAGE_BYTES)
        .on_upgrade(move |socket| handle_conn(socket, bridge))
}

async fn handle_conn(socket: WebSocket, bridge: WebBridge) {
    let id = bridge.next_conn.fetch_add(1, Ordering::Relaxed) + 1;
    let (tx, mut rx) = mpsc::unbounded_channel::<OutboundFrame>();
    bridge.conns.lock().expect("conns lock poisoned").insert(
        id,
        ConnTx {
            tx: tx.clone(),
            via_relay: false,
        },
    );
    let _ = bridge.inbound.send(InboundEvent::Connected { id });

    let (mut sink, mut stream) = socket.split();
    let writer = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            let close = frame.close;
            if sink.send(WsMessage::Text(frame.text.into())).await.is_err() {
                break;
            }
            if close {
                let _ = sink.close().await;
                break;
            }
        }
    });

    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            WsMessage::Text(text) => match serde_json::from_str::<ClientMsg>(&text) {
                Ok(msg) => {
                    let _ = bridge.inbound.send(InboundEvent::Message { id, msg });
                }
                Err(e) => {
                    let frame = OutboundFrame {
                        text: serde_json::to_string(&ServerMsg::Error {
                            reason: format!("bad message: {e}"),
                        })
                        .unwrap_or_default(),
                        close: true,
                    };
                    let _ = tx.send(frame);
                    break;
                }
            },
            WsMessage::Close(_) => break,
            _ => {}
        }
    }

    bridge
        .conns
        .lock()
        .expect("conns lock poisoned")
        .remove(&id);
    let _ = bridge.inbound.send(InboundEvent::Disconnected { id });
    writer.abort();
}

// ---------------------------------------------------------------------------
// Delivery: authority outbound messages → sockets
// ---------------------------------------------------------------------------

/// Send everything the authority produced. `Error` frames close the socket
/// after delivery.
pub(crate) fn deliver(room: &WebRoom, out: Vec<Outbound>) {
    if out.is_empty() {
        return;
    }
    let conns = room.bridge.conns.lock().expect("conns lock poisoned");
    for o in out {
        // Every committed batch lands in the op log — the room's history.
        if let ServerMsg::Ops {
            revision,
            author,
            ops,
            ..
        } = &o.msg
            && let Some(log) = &room.op_log
        {
            let _ = log.append(&sync::OpLogEntry {
                revision: *revision,
                author: author.clone(),
                ops: ops
                    .iter()
                    .cloned()
                    .map(|op| sync::SessionOp::Edit(Box::new(op)))
                    .collect(),
                timestamp_ms: now_ms(),
                id: None,
                parent: None,
                message: None,
            });
        }
        let close = matches!(o.msg, ServerMsg::Error { .. });
        let Ok(text) = serde_json::to_string(&o.msg) else {
            continue;
        };
        let targets: Vec<PeerId> = match o.to {
            Recipients::All => room.authority.peers().map(|p| p.id).collect(),
            Recipients::AllExcept(skip) => room
                .authority
                .peers()
                .map(|p| p.id)
                .filter(|p| *p != skip)
                .collect(),
            Recipients::One(id) => vec![id],
        };
        for id in targets {
            if let Some(conn) = conns.get(&id) {
                let _ = conn.tx.send(OutboundFrame {
                    text: text.clone(),
                    close,
                });
            }
        }
    }
}

fn error_to(id: PeerId, reason: impl Into<String>) -> Vec<Outbound> {
    vec![Outbound {
        to: Recipients::One(id),
        msg: ServerMsg::Error {
            reason: reason.into(),
        },
    }]
}

// ---------------------------------------------------------------------------
// Bevy systems
// ---------------------------------------------------------------------------

/// Drain socket events: joins, leaves, and client messages.
pub(crate) fn web_drain_inbound(mut room: ResMut<WebRoom>, mut pending: ResMut<PendingSceneOps>) {
    let events: Vec<InboundEvent> = {
        let Ok(mut rx) = room.inbound_rx.lock() else {
            return;
        };
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    };
    for event in events {
        match event {
            InboundEvent::Connected { .. } => {}
            InboundEvent::Disconnected { id } => {
                let out = room.authority.leave(id);
                deliver(&room, out);
            }
            InboundEvent::Message { id, msg } => {
                handle_client_msg(&mut room, &mut pending, id, msg)
            }
        }
    }
}

/// Record the session's non-edit history: the model's tool calls, host
/// state changes, and package snapshots when the revisions pile up.
/// History entries carry the current revision; they never bump it.
pub(crate) fn web_session_history(
    mut room: ResMut<WebRoom>,
    mut generation_log: ResMut<crate::gen3d::generation_log::GenerationLog>,
    score: Res<crate::interaction::ScoreBoard>,
) {
    for record in generation_log.take_session_records() {
        room.record_history(sync::SessionOp::Tool(record), "llm");
    }

    let changed: BTreeMap<String, serde_json::Value> = score
        .scores
        .iter()
        .filter(|(k, v)| room.last_scores.get(*k) != Some(v))
        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
        .collect();
    if !changed.is_empty() {
        room.last_scores = score.scores.iter().map(|(k, v)| (k.clone(), *v)).collect();
        room.record_history(
            sync::SessionOp::State(sync::StateRecord { state: changed }),
            "host",
        );
    }

    let revision = room.authority.revision();
    if revision >= room.last_snapshot_rev + SNAPSHOT_EVERY {
        let Some(dir) = room.session_dir.clone() else {
            return;
        };
        let manifest = room.authority.doc().to_manifest();
        match localgpt_world_agent::session::write_snapshot(&dir, revision, &manifest) {
            Ok(()) => {
                room.last_snapshot_rev = revision;
                if let Err(e) = localgpt_world_agent::session::refresh_meta(&dir, revision) {
                    eprintln!("web session: session metadata unavailable ({e})");
                }
            }
            Err(e) => eprintln!("web session: snapshot unavailable ({e})"),
        }
    }
}

fn handle_client_msg(
    room: &mut WebRoom,
    pending: &mut PendingSceneOps,
    id: PeerId,
    msg: ClientMsg,
) {
    match msg {
        ClientMsg::Hello {
            protocol,
            name,
            token,
            client,
        } => {
            if room.authority.peer(id).is_some() {
                return;
            }
            if protocol != sync::PROTOCOL_VERSION {
                deliver(
                    room,
                    error_to(
                        id,
                        format!(
                            "protocol {protocol} not supported (this session speaks {})",
                            sync::PROTOCOL_VERSION
                        ),
                    ),
                );
                return;
            }
            // Two bearer secrets open the room: the invite token (browser
            // links) and the session PIN (native --join). Both arrive in
            // the clear, on a LAN and through a relay, which reads them.
            // A connection through the relay is anyone's with the room
            // code, so it is let in by the 128-bit token alone, never the
            // six-digit PIN; and after a few wrong PINs the session stops
            // taking the PIN, so it can't be guessed on a LAN either. A
            // refused hello closes the connection, a relayed one included.
            let presented = token.as_deref().unwrap_or("");
            let via_relay = room.bridge.via_relay(id);
            let ok = match (&room.bridge.token, &room.session_pin) {
                (None, None) => true,
                (t, p) => {
                    t.as_deref() == Some(presented)
                        || (!via_relay && room.pin_taken() && p.as_deref() == Some(presented))
                }
            };
            if !ok {
                let pin_shaped =
                    presented.len() == 6 && presented.bytes().all(|b| b.is_ascii_digit());
                if !via_relay && pin_shaped && room.pin_taken() {
                    room.wrong_pins += 1;
                    if room.pin_locked() {
                        eprintln!(
                            "web session: {PIN_GUESSES} wrong PINs — the PIN is no longer taken \
                             (the invite link still is; a new session gets a new PIN)"
                        );
                    }
                }
                let reason = if via_relay {
                    "bad invite token"
                } else if pin_shaped && room.pin_locked() {
                    PIN_LOCKED
                } else {
                    "bad invite token or PIN"
                };
                deliver(room, error_to(id, reason));
                return;
            }
            let role = if room.web_edit {
                Role::Editor
            } else {
                Role::Guest
            };
            let out = room.authority.join(id, &name, role, client);
            deliver(room, out);
        }
        _ if room.authority.peer(id).is_none() => {
            deliver(room, error_to(id, "say hello first"));
        }
        ClientMsg::Presence(presence) => {
            let out = room.authority.presence(id, presence);
            deliver(room, out);
        }
        ClientMsg::Chat { text } => {
            let out = room.authority.chat(id, &text);
            deliver(room, out);
        }
        ClientMsg::Prompt {
            request_id,
            text,
            anchor,
        } => {
            // No model runs for the room, so a prompt is taken by the
            // authority (every peer sees it queued, as the protocol says)
            // and failed at once with the reason. A guest edits the world
            // through ops; an agent through the world's API.
            let (out, event) = room.authority.prompt(id, &request_id, &text, anchor);
            deliver(room, out);
            if let Some(AuthorityEvent::PromptAccepted { job_id, .. }) = event {
                let out = room.authority.job_finished(
                    job_id,
                    Some(
                        "this room runs no AI: edit the world directly, or with an agent \
                         through the world's API"
                            .to_string(),
                    ),
                );
                deliver(room, out);
            }
        }
        ClientMsg::Submit {
            client_seq,
            expected_revision,
            ops,
        } => {
            let out = room
                .authority
                .submit(id, client_seq, expected_revision, ops);
            queue_committed(&out, pending);
            deliver(room, out);
        }
        ClientMsg::Resync => {
            let out = room.authority.resync(id);
            deliver(room, out);
        }
        ClientMsg::Undo => {
            let out = room.authority.undo(id);
            queue_committed(&out, pending);
            deliver(room, out);
        }
        ClientMsg::Ping { t } => {
            deliver(
                room,
                vec![Outbound {
                    to: Recipients::One(id),
                    msg: ServerMsg::Pong { t },
                }],
            );
        }
    }
}

/// Ops that committed through the authority must also land in the scene,
/// or the next projection would restore the pre-commit state.
fn queue_committed(out: &[Outbound], pending: &mut PendingSceneOps) {
    for o in out {
        if let ServerMsg::Ops { ops, .. } = &o.msg {
            pending.0.extend(ops.iter().cloned());
        }
    }
}

/// Apply queued authority ops to the scene, and rebuild the scene after an
/// op-log replay.
pub(crate) fn web_apply_scene_ops(
    mut pending: ResMut<PendingSceneOps>,
    mut room: ResMut<WebRoom>,
    mut applier: crate::gen3d::ops_apply::OpsApplier,
) {
    if room.scene_rebuild_pending {
        room.scene_rebuild_pending = false;
        let entities: Vec<wt::WorldEntity> = room.authority.doc().entities().cloned().collect();
        if !entities.is_empty() {
            applier.rebuild_scene(&entities);
            eprintln!(
                "web session: rebuilt {} entities from the op log",
                entities.len()
            );
        }
        if let Some(env) = room.authority.doc().environment.clone() {
            applier.apply_ops(&[wt::EditOp::SetEnvironment { env }]);
        }
    }
    if pending.0.is_empty() {
        return;
    }
    let ops = std::mem::take(&mut pending.0);
    applier.apply_ops(&ops);
}

/// Read-only ECS access for projecting the scene into world-types.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct ProjectionQueries<'w, 's> {
    transforms: Query<'w, 's, &'static Transform>,
    parametric_shapes: Query<'w, 's, &'static ParametricShape>,
    material_handles: Query<'w, 's, &'static MeshMaterial3d<StandardMaterial>>,
    materials: Res<'w, Assets<StandardMaterial>>,
    visibility_query: Query<'w, 's, &'static Visibility>,
    directional_lights: Query<'w, 's, &'static DirectionalLight>,
    point_lights: Query<'w, 's, &'static PointLight>,
    spot_lights: Query<'w, 's, &'static SpotLight>,
    behaviors_query: Query<'w, 's, &'static EntityBehaviors>,
    audio_emitters: Query<'w, 's, &'static AudioEmitter>,
    parent_query: Query<'w, 's, &'static ChildOf>,
    gltf_sources: Query<'w, 's, &'static GltfSource>,
    gen_entities: Query<'w, 's, &'static GenEntity>,
    registry: Res<'w, NameRegistry>,
}

/// Project the live scene into the document and broadcast the diff as ops.
///
/// Entities with behaviors are projected at their *base* transform (their
/// animation runs on every client from the shared behavior definitions), so
/// a spinning windmill doesn't generate 4 ops per second. Rotation can't be
/// recovered from behavior state, so behavior entities keep the document's
/// rotation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn web_projection_sync(
    time: Res<Time>,
    mut room: ResMut<WebRoom>,
    clear_color: Option<Res<ClearColor>>,
    ambient_light: Option<Res<GlobalAmbientLight>>,
    params: ProjectionQueries,
) {
    if !room.projection_timer.tick(time.delta()).just_finished() {
        return;
    }
    let ProjectionQueries {
        transforms,
        parametric_shapes,
        material_handles,
        materials,
        visibility_query,
        directional_lights,
        point_lights,
        spot_lights,
        behaviors_query,
        audio_emitters,
        parent_query,
        gltf_sources,
        gen_entities,
        registry,
    } = params;
    let sq = SnapshotQueries {
        transforms: &transforms,
        parametric_shapes: &parametric_shapes,
        material_handles: &material_handles,
        materials: &materials,
        visibility_query: &visibility_query,
        directional_lights: &directional_lights,
        point_lights: &point_lights,
        spot_lights: &spot_lights,
        behaviors_query,
        audio_emitters: &audio_emitters,
        parent_query: &parent_query,
        gltf_sources: &gltf_sources,
        material_textures: None,
        authored: None,
        registry: &registry,
    };

    let mut projection = Vec::new();
    for (name, bevy_entity) in registry.all_names() {
        let Ok(gen_ent) = gen_entities.get(bevy_entity) else {
            continue;
        };
        if gen_ent.entity_type == GenEntityType::Camera {
            continue;
        }
        let mut we = snapshot_entity(name, bevy_entity, gen_ent.world_id, &sq);
        if let Ok(eb) = sq.behaviors_query.get(bevy_entity)
            && let Some(base) = eb.behaviors.first()
        {
            we.transform.position = base.base_position.to_array();
            we.transform.scale = base.base_scale.to_array();
            if let Some(old) = room.authority.doc().get(we.id.0) {
                we.transform.rotation_degrees = old.transform.rotation_degrees;
            }
        }
        projection.push(we);
    }

    // Background and ambient light come from the live render resources
    // (the same sources gen_save_world reads); fog isn't captured anywhere
    // live, so keep whatever the document already has.
    let doc_env = room.authority.doc().environment.clone();
    let env = wt::EnvironmentDef {
        background_color: clear_color.as_ref().map(|c| {
            let srgba = c.0.to_srgba();
            [srgba.red, srgba.green, srgba.blue, srgba.alpha]
        }),
        ambient_intensity: ambient_light.as_ref().map(|a| a.brightness),
        ambient_color: ambient_light.as_ref().map(|a| {
            let srgba = a.color.to_srgba();
            [srgba.red, srgba.green, srgba.blue, srgba.alpha]
        }),
        fog_density: doc_env.as_ref().and_then(|e| e.fog_density),
        fog_color: doc_env.and_then(|e| e.fog_color),

        extra: ::std::collections::BTreeMap::new(),
    };

    let ops = sync::diff_scene(room.authority.doc(), &projection, Some(&env));
    if ops.is_empty() {
        return;
    }
    match room.authority.record_local_ops("host", ops) {
        Ok(out) => deliver(&room, out),
        Err(e) => {
            // A projection should always apply; if one doesn't, skip this
            // cycle rather than corrupt the room (the next diff retries).
            if !room.warned_bad_projection {
                room.warned_bad_projection = true;
                tracing::warn!("web sync: projection didn't apply ({e}); will keep retrying");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The join page
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_128_bits_of_hex() {
        let token = generate_web_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(token, generate_web_token());
    }

    #[test]
    fn bridge_carries_events_and_frames() {
        let (bridge, mut rx) = WebBridge::new(Some("tok".into()), "test");
        let (tx, _frame_rx) = mpsc::unbounded_channel::<OutboundFrame>();
        let id = 7;
        bridge.conns.lock().unwrap().insert(
            id,
            ConnTx {
                tx,
                via_relay: false,
            },
        );
        bridge
            .inbound
            .send(InboundEvent::Message {
                id,
                msg: ClientMsg::Ping { t: 1.0 },
            })
            .unwrap();
        match rx.try_recv().unwrap() {
            InboundEvent::Message { id: got, .. } => assert_eq!(got, 7),
            _ => panic!("expected message"),
        }
        // The connection's frame channel is reachable from the bridge.
        assert!(bridge.conns.lock().unwrap().contains_key(&id));
    }

    /// A PIN session with the invite token `tok`, its package in `workspace`.
    fn pin_room(pin: &str, workspace: &std::path::Path) -> WebRoom {
        let (bridge, rx) = WebBridge::new(Some("tok".into()), "test");
        WebRoom::new("test", bridge, rx, workspace, None, false, Some(pin.into()))
    }

    /// A connection to the room: its peer id and the frames sent to it.
    fn connect(
        room: &WebRoom,
        via_relay: bool,
    ) -> (PeerId, mpsc::UnboundedReceiver<OutboundFrame>) {
        let id = room.bridge.alloc_peer_id();
        let (tx, rx) = mpsc::unbounded_channel();
        room.bridge
            .conns
            .lock()
            .unwrap()
            .insert(id, ConnTx { tx, via_relay });
        (id, rx)
    }

    fn hello(secret: &str) -> ClientMsg {
        ClientMsg::Hello {
            protocol: sync::PROTOCOL_VERSION,
            name: "guest".into(),
            token: Some(secret.into()),
            client: sync::ClientKind::Gen,
        }
    }

    /// Why the room refused a hello, and whether the refusal closes the
    /// connection.
    fn refusal(rx: &mut mpsc::UnboundedReceiver<OutboundFrame>) -> Option<(String, bool)> {
        let frame = rx.try_recv().ok()?;
        match serde_json::from_str::<ServerMsg>(&frame.text) {
            Ok(ServerMsg::Error { reason }) => Some((reason, frame.close)),
            _ => None,
        }
    }

    #[test]
    fn a_relayed_hello_is_let_in_by_the_token_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut room = pin_room("482913", dir.path());
        let mut pending = PendingSceneOps::default();

        let (direct, _rx) = connect(&room, false);
        handle_client_msg(&mut room, &mut pending, direct, hello("482913"));
        assert!(
            room.authority.peer(direct).is_some(),
            "the PIN joins directly"
        );

        let (relayed, mut rx) = connect(&room, true);
        handle_client_msg(&mut room, &mut pending, relayed, hello("482913"));
        assert!(room.authority.peer(relayed).is_none());
        assert_eq!(refusal(&mut rx), Some(("bad invite token".into(), true)));

        let (relayed, _rx) = connect(&room, true);
        handle_client_msg(&mut room, &mut pending, relayed, hello("tok"));
        assert!(
            room.authority.peer(relayed).is_some(),
            "the token joins through the relay"
        );
    }

    #[test]
    fn wrong_pins_stop_the_pin_being_taken() {
        let dir = tempfile::tempdir().unwrap();
        let mut room = pin_room("482913", dir.path());
        let mut pending = PendingSceneOps::default();

        for wrong in ["000000", "111111", "222222", "333333"] {
            let (id, mut rx) = connect(&room, false);
            handle_client_msg(&mut room, &mut pending, id, hello(wrong));
            assert_eq!(
                refusal(&mut rx),
                Some(("bad invite token or PIN".into(), true))
            );
            assert!(room.pin_taken());
        }
        // The fifth wrong one locks the PIN, and is told so.
        let (id, mut rx) = connect(&room, false);
        handle_client_msg(&mut room, &mut pending, id, hello("444444"));
        assert_eq!(refusal(&mut rx), Some((PIN_LOCKED.into(), true)));
        assert!(!room.pin_taken());

        let (id, mut rx) = connect(&room, false);
        handle_client_msg(&mut room, &mut pending, id, hello("482913"));
        assert!(
            room.authority.peer(id).is_none(),
            "the right PIN no longer joins"
        );
        assert_eq!(refusal(&mut rx), Some((PIN_LOCKED.into(), true)));

        let (id, _rx) = connect(&room, false);
        handle_client_msg(&mut room, &mut pending, id, hello("tok"));
        assert!(
            room.authority.peer(id).is_some(),
            "the invite token still joins"
        );
    }

    #[test]
    fn only_a_pin_shaped_guess_on_a_direct_connection_counts() {
        let dir = tempfile::tempdir().unwrap();
        let mut room = pin_room("482913", dir.path());
        let mut pending = PendingSceneOps::default();

        for _ in 0..PIN_GUESSES {
            // Through the relay the PIN isn't taken, so a guess there isn't one.
            let (id, _rx) = connect(&room, true);
            handle_client_msg(&mut room, &mut pending, id, hello("000000"));
            // A wrong invite token isn't a PIN guess either.
            let (id, _rx) = connect(&room, false);
            handle_client_msg(&mut room, &mut pending, id, hello("not-the-token"));
        }
        assert_eq!(room.wrong_pins, 0);
        assert!(room.pin_taken());
    }
}
