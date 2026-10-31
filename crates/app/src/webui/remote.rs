//! A paired device's page: the app's own page, served on the local network,
//! which sees and drives what the window does — the agents' terminals, the
//! history and its playback, the clock, a proposal waiting, and the world as
//! a frame read back from the window (`docs/rfcs/authoring/mobile-remote.md`).
//!
//! Off until it is turned on (the page's Phone button, or
//! `LOCALGPT_APP_REMOTE=on`), and remembered after. On, the app listens on
//! port 9880 of every interface: the built page over HTTP, and a WebSocket
//! that carries the page's own messages — `FromPage` in, `ToPage` out, as
//! `window.ipc` and `__lg.recv` carry them in the window. Whatever the window
//! is sent, the devices are sent too; a device that arrives late is given the
//! newest of each snapshot first, then each tab's screen when it shows it
//! (`term.rs`).
//!
//! A device pairs by trading a one-time code, shown as a QR code for ten
//! minutes, for a key of its own; the app keeps the key's hash
//! (`<state>/app-remote.json`, readable by the owner alone), lists the
//! devices, forgets any of them, and shows which are attached. It is plain
//! HTTP, for a network the person trusts or a VPN they run: whoever can read
//! the traffic can read the terminals, and a paired device that can type
//! into them is a shell on this computer.

use std::future::IntoFuture;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Json;
use axum::extract::State;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::window::PrimaryWindow;
use futures::{SinkExt, StreamExt};
use localgpt_world_agent::session::sha256_hex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::Hole;

/// `on` starts the listener at launch and shows a pairing code in the
/// terminal the app was started from; `off` keeps it off, whatever was
/// remembered.
pub const LOCALGPT_APP_REMOTE: &str = "LOCALGPT_APP_REMOTE";
/// Another port than 9880, for a second app or a test.
pub const LOCALGPT_APP_REMOTE_PORT: &str = "LOCALGPT_APP_REMOTE_PORT";

/// Beside Gen's rooms (9879). Taken, any free port serves.
const PORT: u16 = 9880;
/// How long a pairing code lasts.
const PAIRING: Duration = Duration::from_secs(600);
/// A device has this long to say who it is.
const HELLO: Duration = Duration::from_secs(10);
/// The largest message a device may send: a pasted prompt, not a file.
const MAX_MESSAGE: usize = 1 << 20;
/// The frame is cut down to this width: a phone's screen in device pixels.
const FRAME_WIDTH: u32 = 1280;
const VERSION: u32 = 1;

/// What is remembered: whether it was on, and the paired devices.
#[derive(Serialize, Deserialize, Default)]
struct Saved {
    version: u32,
    on: bool,
    devices: Vec<Device>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct Device {
    id: String,
    name: String,
    /// The key's hash: the key itself stays on the device.
    key_sha256: String,
    paired_ms: u64,
    seen_ms: u64,
}

/// What the listener's thread and the app share.
#[derive(Default)]
struct Shared {
    on: bool,
    devices: Vec<Device>,
    /// The code on show: its hash, and until when.
    pairing: Option<(String, Instant)>,
    /// The connections attached, each with its device's id.
    attached: Vec<(u64, String)>,
    /// Bumped with every change the window's page shows.
    rev: u64,
    path: Option<PathBuf>,
}

impl Shared {
    fn changed(&mut self) {
        self.rev += 1;
    }

    /// Writes what is remembered: by the owner alone, all at once.
    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let saved = Saved {
            version: VERSION,
            on: self.on,
            devices: self.devices.clone(),
        };
        if let Err(e) = write_private(path, &saved) {
            tracing::warn!("remote: couldn't remember the paired devices: {e:#}");
        }
    }
}

fn write_private(path: &std::path::Path, saved: &Saved) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&tmp)?;
    std::io::Write::write_all(&mut file, serde_json::to_string_pretty(saved)?.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn load(path: &std::path::Path) -> Saved {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Saved>(&text).ok())
        .filter(|saved| saved.version == VERSION)
        .unwrap_or_default()
}

/// What a device's connection tells the app.
pub enum Inbound {
    /// One of the page's messages, as the page wrote it.
    Message(u64, String),
    /// The connection closed.
    Gone(u64),
}

/// What the app tells the hub, which writes to the devices' sockets.
enum HubIn {
    /// A device said who it is; nothing is sent to it until its page is
    /// ready.
    Join(u64, UnboundedSender<String>),
    Leave(u64),
    /// Sent to the window: the newest of its kind is kept, and the devices
    /// that are ready get it.
    Broadcast(String),
    /// For one device alone.
    To(u64, String),
    /// The device's page is ready: the snapshots kept, then `catch_up`, then
    /// everything as it comes.
    Ready(u64, Vec<String>),
}

/// Where the page's messages go besides the window: the hub, while the
/// listener runs.
#[derive(Clone, Default)]
pub struct Mirror(Arc<Mutex<Option<UnboundedSender<HubIn>>>>);

impl Mirror {
    fn send(&self, message: impl FnOnce() -> HubIn) {
        if let Ok(hub) = self.0.lock()
            && let Some(hub) = hub.as_ref()
        {
            let _ = hub.send(message());
        }
    }

    /// What the window was sent, for the devices as well.
    pub fn broadcast(&self, json: &str) {
        self.send(|| HubIn::Broadcast(json.to_string()));
    }

    pub fn to(&self, conn: u64, json: String) {
        self.send(|| HubIn::To(conn, json));
    }

    fn set(&self, hub: Option<UnboundedSender<HubIn>>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = hub;
        }
    }
}

/// The listener's thread, stopped when this is dropped.
struct Server {
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// The listener, its devices, and what the window's page shows of them.
#[derive(Resource)]
pub struct Remote {
    shared: Arc<Mutex<Shared>>,
    inbox: Mutex<Receiver<Inbound>>,
    inbox_tx: Sender<Inbound>,
    mirror: Mirror,
    server: Option<Server>,
    /// Every interface; loopback in a test.
    host: std::net::Ipv4Addr,
    port: u16,
    /// The page's address on this network, while on.
    url: Option<String>,
    /// The pairing code in the clear, while it lasts: for the QR code.
    code: Option<String>,
    error: Option<String>,
    next_conn: Arc<AtomicU64>,
}

impl Remote {
    /// What was remembered, started when it was on — or as
    /// `LOCALGPT_APP_REMOTE` says.
    pub fn from_env() -> Self {
        let path = localgpt_core::paths::Paths::resolve()
            .ok()
            .map(|paths| paths.state_dir.join("app-remote.json"));
        let port = std::env::var(LOCALGPT_APP_REMOTE_PORT)
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(PORT);
        let mut remote = Self::at(path, port);
        let asked = std::env::var(LOCALGPT_APP_REMOTE).ok();
        let on = match asked.as_deref() {
            Some("on" | "1") => true,
            Some("off" | "0") => false,
            _ => remote.shared().on,
        };
        if on {
            remote.start();
            if asked.is_some()
                && remote.server.is_some()
                && let Some(url) = remote.pair()
            {
                eprintln!("localgpt-app: pair a phone at {url} (ten minutes)");
            }
        }
        remote
    }

    /// What `path` remembers, not listening yet.
    fn at(path: Option<PathBuf>, port: u16) -> Self {
        let saved = path.as_deref().map(load).unwrap_or_default();
        let (inbox_tx, inbox) = channel();
        Self {
            shared: Arc::new(Mutex::new(Shared {
                on: saved.on,
                devices: saved.devices,
                path,
                ..Default::default()
            })),
            inbox: Mutex::new(inbox),
            inbox_tx,
            mirror: Mirror::default(),
            server: None,
            host: std::net::Ipv4Addr::UNSPECIFIED,
            port,
            url: None,
            code: None,
            error: None,
            next_conn: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn mirror(&self) -> Mirror {
        self.mirror.clone()
    }

    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_on(&self) -> bool {
        self.server.is_some()
    }

    /// Turns the listener on or off, and remembers it. Turned on with no
    /// device paired, it shows a pairing code at once.
    pub fn set_on(&mut self, on: bool) {
        if on {
            self.start();
            if self.server.is_some() && self.shared().devices.is_empty() {
                self.pair();
            }
        } else {
            self.stop();
        }
        let mut shared = self.shared();
        shared.on = self.server.is_some();
        shared.changed();
        shared.save();
    }

    fn start(&mut self) {
        if self.server.is_some() {
            return;
        }
        match self.listen() {
            Ok((server, port)) => {
                self.server = Some(server);
                self.url = lan_ip().map(|ip| format!("http://{ip}:{port}/"));
                self.error = None;
            }
            Err(e) => {
                tracing::warn!("remote: couldn't listen: {e:#}");
                self.error = Some(format!("{e:#}"));
            }
        }
        self.shared().changed();
    }

    fn listen(&self) -> anyhow::Result<(Server, u16)> {
        let listener = std::net::TcpListener::bind((self.host, self.port))
            .or_else(|_| std::net::TcpListener::bind((self.host, 0)))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let (hub_tx, hub_rx) = unbounded_channel();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let ctx = Ctx {
            shared: Arc::clone(&self.shared),
            inbox: self.inbox_tx.clone(),
            hub: hub_tx.clone(),
            next_conn: Arc::clone(&self.next_conn),
        };
        std::thread::Builder::new()
            .name("remote".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(e) => {
                        tracing::warn!("remote: {e}");
                        return;
                    }
                };
                // Stopping drops the runtime: the hub and every socket with it.
                runtime.block_on(async move {
                    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                        return;
                    };
                    tokio::spawn(hub(hub_rx));
                    tokio::select! {
                        served = axum::serve(listener, router(ctx)).into_future() => {
                            if let Err(e) = served {
                                tracing::warn!("remote: the listener stopped: {e}");
                            }
                        }
                        _ = stopped => {}
                    }
                });
            })?;
        self.mirror.set(Some(hub_tx));
        Ok((Server { stop: Some(stop) }, port))
    }

    fn stop(&mut self) {
        self.mirror.set(None);
        self.server = None;
        self.url = None;
        self.code = None;
        let mut shared = self.shared();
        shared.pairing = None;
        shared.attached.clear();
        shared.changed();
    }

    /// A fresh one-time code; the address that pairs with it.
    pub fn pair(&mut self) -> Option<String> {
        let url = self.url.clone()?;
        let code = hex(&rand::random::<[u8; 16]>());
        {
            let mut shared = self.shared();
            shared.pairing = Some((sha256_hex(code.as_bytes()), Instant::now() + PAIRING));
            shared.changed();
        }
        let link = format!("{url}#pair={code}");
        self.code = Some(code);
        Some(link)
    }

    /// Forgets a device; its connections close.
    pub fn forget(&mut self, id: &str) {
        let mut shared = self.shared();
        shared.devices.retain(|d| d.id != id);
        let kicked: Vec<u64> = shared
            .attached
            .iter()
            .filter(|(_, device)| device == id)
            .map(|(conn, _)| *conn)
            .collect();
        shared.changed();
        shared.save();
        drop(shared);
        for conn in kicked {
            // Its sender dropped, the socket's writer ends and closes it.
            self.mirror.send(|| HubIn::Leave(conn));
        }
    }

    /// Everything the devices said since the last frame.
    pub fn take_inbox(&self) -> Vec<Inbound> {
        self.inbox
            .lock()
            .map(|inbox| inbox.try_iter().collect())
            .unwrap_or_default()
    }

    /// The device's page is ready: it gets what the window has, then
    /// `catch_up`, then everything as it comes.
    pub fn ready(&self, conn: u64, catch_up: Vec<String>) {
        self.mirror.send(|| HubIn::Ready(conn, catch_up));
    }

    pub fn rev(&self) -> u64 {
        self.shared().rev
    }

    /// The code expires: it stops showing.
    fn tick(&mut self) {
        let mut shared = self.shared();
        let expired = shared
            .pairing
            .as_ref()
            .is_some_and(|(_, until)| Instant::now() >= *until);
        if expired {
            shared.pairing = None;
            shared.changed();
        }
        // Used, or expired: the code in the clear goes too.
        let gone = shared.pairing.is_none() && self.code.is_some();
        drop(shared);
        if gone {
            self.code = None;
        }
    }

    pub fn view(&self) -> RemoteView {
        let shared = self.shared();
        let pairing = match (&self.code, &shared.pairing, &self.url) {
            (Some(code), Some((_, until)), Some(url)) => {
                let link = format!("{url}#pair={code}");
                Some(PairingView {
                    qr: qr_svg(&link),
                    url: link,
                    left_s: until.saturating_duration_since(Instant::now()).as_secs(),
                })
            }
            _ => None,
        };
        RemoteView {
            on: self.server.is_some(),
            url: self.url.clone(),
            pairing,
            devices: shared
                .devices
                .iter()
                .map(|d| DeviceView {
                    id: d.id.clone(),
                    name: d.name.clone(),
                    paired_ms: d.paired_ms,
                    seen_ms: d.seen_ms,
                    attached: shared.attached.iter().any(|(_, id)| *id == d.id),
                })
                .collect(),
            error: self.error.clone(),
        }
    }
}

/// The listener as the window's page shows it.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct RemoteView {
    pub on: bool,
    /// The page's address on this network.
    pub url: Option<String>,
    /// While a code lasts: the address that pairs with it, as a QR code too.
    pub pairing: Option<PairingView>,
    pub devices: Vec<DeviceView>,
    pub error: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct PairingView {
    pub url: String,
    /// An SVG image of `url`.
    pub qr: Option<String>,
    /// Seconds it still lasts, when this was sent.
    pub left_s: u64,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub paired_ms: u64,
    pub seen_ms: u64,
    pub attached: bool,
}

fn qr_svg(text: &str) -> Option<String> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(text.as_bytes()).ok()?;
    Some(
        code.render::<svg::Color<'_>>()
            .min_dimensions(200, 200)
            .dark_color(svg::Color("#0f1218"))
            .light_color(svg::Color("#ffffff"))
            .build(),
    )
}

/// This computer's address on the local network: the interface a packet
/// out would leave by (no packet is sent).
fn lan_ip() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(("8.8.8.8", 80)).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The listener: the page, pairing, and the devices' sockets
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Ctx {
    shared: Arc<Mutex<Shared>>,
    inbox: Sender<Inbound>,
    hub: UnboundedSender<HubIn>,
    next_conn: Arc<AtomicU64>,
}

impl Ctx {
    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn router(ctx: Ctx) -> axum::Router {
    axum::Router::new()
        .route("/pair", post(pair))
        .route("/remote", get(socket))
        .fallback(get(page))
        .with_state(ctx)
}

/// The built page, marked as a device's, so it opens its line here.
async fn page(uri: Uri) -> Response {
    let Some((data, kind)) = super::dist::file(uri.path()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if kind != "text/html" {
        return ([(header::CONTENT_TYPE, kind)], data.into_owned()).into_response();
    }
    let html = String::from_utf8_lossy(&data).replacen(
        "<head>",
        "<head><meta name=\"lg-remote\" content=\"1\">",
        1,
    );
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        html,
    )
        .into_response()
}

/// A browser's request from another site's page, which must not pair or
/// connect: a page served here sends its own origin, and a tool sends none.
fn foreign(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    origin
        .split("://")
        .nth(1)
        .unwrap_or(origin)
        .trim_end_matches('/')
        != host
}

#[derive(Deserialize)]
struct PairAsk {
    code: String,
    #[serde(default)]
    name: String,
}

/// The code on show, traded once for a key of the device's own.
async fn pair(State(ctx): State<Ctx>, headers: HeaderMap, Json(ask): Json<PairAsk>) -> Response {
    if foreign(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut shared = ctx.shared();
    let current = shared.pairing.as_ref().is_some_and(|(hash, until)| {
        *hash == sha256_hex(ask.code.as_bytes()) && Instant::now() < *until
    });
    if !current {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error": "That code is used or expired: show a new one on the computer."})),
        )
            .into_response();
    }
    shared.pairing = None;
    let key = hex(&rand::random::<[u8; 32]>());
    let name: String = ask
        .name
        .chars()
        .filter(|c| !c.is_control())
        .take(40)
        .collect();
    let device = Device {
        id: hex(&rand::random::<[u8; 4]>()),
        name: if name.trim().is_empty() {
            "A device".into()
        } else {
            name.trim().to_string()
        },
        key_sha256: sha256_hex(key.as_bytes()),
        paired_ms: now_ms(),
        seen_ms: now_ms(),
    };
    let reply = json!({"device": device.id, "name": device.name, "key": key});
    shared.devices.push(device);
    shared.changed();
    shared.save();
    Json(reply).into_response()
}

#[derive(Deserialize)]
#[serde(tag = "t")]
enum Hello {
    #[serde(rename = "hello")]
    Hello { key: String },
}

async fn socket(ws: WebSocketUpgrade, State(ctx): State<Ctx>, headers: HeaderMap) -> Response {
    if foreign(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    ws.max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket| connection(socket, ctx))
}

/// One device's socket: it says who it is, then carries the page's
/// messages both ways until either side closes.
async fn connection(socket: WebSocket, ctx: Ctx) {
    let (mut sink, mut stream) = socket.split();
    let key = match tokio::time::timeout(HELLO, stream.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => match serde_json::from_str::<Hello>(&text) {
            Ok(Hello::Hello { key }) => Some(key),
            Err(_) => None,
        },
        _ => None,
    };
    let conn = ctx.next_conn.fetch_add(1, Ordering::Relaxed) + 1;
    let device = key.and_then(|key| {
        let hash = sha256_hex(key.as_bytes());
        let mut shared = ctx.shared();
        let device = shared.devices.iter_mut().find(|d| d.key_sha256 == hash)?;
        device.seen_ms = now_ms();
        let (id, name) = (device.id.clone(), device.name.clone());
        shared.attached.push((conn, id.clone()));
        shared.changed();
        shared.save();
        Some((id, name))
    });
    let Some((id, name)) = device else {
        let _ = sink
            .send(WsMessage::Text(
                json!({"t": "remote.denied", "why": "This device isn't paired: scan the code the computer shows under its Phone button."})
                    .to_string()
                    .into(),
            ))
            .await;
        let _ = sink.close().await;
        return;
    };
    let (tx, mut rx) = unbounded_channel::<String>();
    let _ = ctx.hub.send(HubIn::Join(conn, tx));
    let welcome = json!({"t": "remote.welcome", "device": id, "name": name}).to_string();
    let writer = tokio::spawn(async move {
        if sink.send(WsMessage::Text(welcome.into())).await.is_err() {
            return;
        }
        while let Some(text) = rx.recv().await {
            if sink.send(WsMessage::Text(text.into())).await.is_err() {
                return;
            }
        }
        // Forgotten, or the app stopped listening.
        let _ = sink.close().await;
    });
    while let Some(Ok(message)) = stream.next().await {
        match message {
            WsMessage::Text(text) => {
                let _ = ctx.inbox.send(Inbound::Message(conn, text.to_string()));
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }
    let _ = ctx.hub.send(HubIn::Leave(conn));
    let _ = ctx.inbox.send(Inbound::Gone(conn));
    {
        let mut shared = ctx.shared();
        shared.attached.retain(|(c, _)| *c != conn);
        shared.changed();
    }
    writer.abort();
}

/// Writes to the devices' sockets, in the order the app sent: the snapshots
/// it keeps for a page that arrives late are always the newest.
async fn hub(mut inbox: UnboundedReceiver<HubIn>) {
    let mut conns: std::collections::HashMap<u64, (UnboundedSender<String>, bool)> =
        Default::default();
    let mut kept = Kept::default();
    while let Some(message) = inbox.recv().await {
        match message {
            HubIn::Join(conn, tx) => {
                conns.insert(conn, (tx, false));
            }
            HubIn::Leave(conn) => {
                conns.remove(&conn);
            }
            HubIn::Broadcast(json) => {
                for (tx, ready) in conns.values() {
                    if *ready {
                        let _ = tx.send(json.clone());
                    }
                }
                kept.keep(json);
            }
            HubIn::To(conn, json) => {
                if let Some((tx, _)) = conns.get(&conn) {
                    let _ = tx.send(json);
                }
            }
            HubIn::Ready(conn, catch_up) => {
                if let Some((tx, ready)) = conns.get_mut(&conn) {
                    for json in kept.newest() {
                        let _ = tx.send(json.clone());
                    }
                    for json in catch_up {
                        let _ = tx.send(json);
                    }
                    *ready = true;
                }
            }
        }
    }
}

/// The newest of each snapshot the window was sent, in the order they last
/// changed: what a device's page needs to draw when it arrives late.
#[derive(Default)]
struct Kept(Vec<(String, String)>);

impl Kept {
    fn keep(&mut self, json: String) {
        let Some(key) = kept_as(&json) else {
            return;
        };
        self.0.retain(|(k, _)| *k != key);
        self.0.push((key, json));
    }

    fn newest(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(_, json)| json)
    }
}

/// What a message is the newest of: its kind, or a shot's thumbnail by its
/// id. A terminal's stream and an event are not kept. (Serde writes an
/// internally tagged enum's tag first.)
fn kept_as(json: &str) -> Option<String> {
    let t = json.strip_prefix(r#"{"t":""#)?.split('"').next()?;
    match t {
        "doc.reveal" | "frame" => None,
        t if t.starts_with("term.") || t.starts_with("remote") => None,
        "thumb" => serde_json::from_str::<serde_json::Value>(json)
            .ok()?
            .get("id")?
            .as_str()
            .map(|id| format!("thumb:{id}")),
        t => Some(t.to_string()),
    }
}

/// The listener as the window's page shows it, when it changed (or the page
/// is new: its `sent` starts empty).
pub fn show(page: Option<NonSendMut<super::Page>>, remote: Res<Remote>) {
    let Some(mut page) = page else {
        return;
    };
    let rev = remote.rev();
    if !page.ready || page.host.is_none() || page.sent.remote_rev == Some(rev) {
        return;
    }
    page.sent.remote_rev = Some(rev);
    page.send_window(&super::ToPage::Remote {
        view: &remote.view(),
    });
}

// ---------------------------------------------------------------------------
// Frames: the world as the window shows it, for a device's page
// ---------------------------------------------------------------------------

/// The devices that asked for a frame, and the one being read back.
#[derive(Resource, Default)]
pub struct Frames {
    wanted: Vec<u64>,
    busy: Arc<AtomicBool>,
    /// When the frame being read back was asked for: a window that draws no
    /// frame (hidden, or the Mac locked) never answers.
    since: Option<f32>,
}

impl Frames {
    pub fn want(&mut self, conn: u64) {
        if !self.wanted.contains(&conn) {
            self.wanted.push(conn);
        }
    }

    pub fn forget(&mut self, conn: u64) {
        self.wanted.retain(|c| *c != conn);
    }
}

/// The pairing code's clock, and the frames asked for: the window read back
/// once for all who asked, cut to the world's part of it, and encoded on a
/// thread of its own.
pub fn tick(
    mut commands: Commands,
    remote: Option<ResMut<Remote>>,
    mut frames: ResMut<Frames>,
    time: Res<Time>,
    hole: Res<Hole>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Some(mut remote) = remote else {
        return;
    };
    remote.tick();
    let now = time.elapsed_secs();
    if frames.busy.load(Ordering::Acquire) {
        if frames.since.is_some_and(|since| now - since < 5.0) {
            return;
        }
        frames.busy.store(false, Ordering::Release);
    }
    if frames.wanted.is_empty() || !remote.is_on() {
        return;
    }
    let to = std::mem::take(&mut frames.wanted);
    frames.busy.store(true, Ordering::Release);
    frames.since = Some(now);
    let busy = Arc::clone(&frames.busy);
    let mirror = remote.mirror();
    let crop = windows.single().ok().and_then(|window| {
        let [x, y, w, h] = hole.0?;
        let scale = window.scale_factor() as f64;
        Some([x * scale, y * scale, w * scale, h * scale].map(|v| v.round().max(0.0) as u32))
    });
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>, mut commands: Commands| {
            commands.entity(captured.entity).despawn();
            let image = captured.image.clone();
            let (to, busy, mirror) = (to.clone(), Arc::clone(&busy), mirror.clone());
            let _ = std::thread::Builder::new()
                .name("remote-frame".into())
                .spawn(move || {
                    if let Some(url) = frame_url(image, crop) {
                        let json = json!({"t": "frame", "url": url}).to_string();
                        for conn in to {
                            mirror.to(conn, json.clone());
                        }
                    }
                    busy.store(false, Ordering::Release);
                });
        },
    );
}

/// The window's frame as a JPEG data URL: the world's part of it, no wider
/// than a phone's screen.
fn frame_url(image: Image, crop: Option<[u32; 4]>) -> Option<String> {
    let mut frame = image.try_into_dynamic().ok()?.to_rgb8();
    if let Some([x, y, w, h]) = crop {
        let (fw, fh) = frame.dimensions();
        let (x, y) = (x.min(fw.saturating_sub(1)), y.min(fh.saturating_sub(1)));
        let (w, h) = (w.min(fw - x), h.min(fh - y));
        if w > 0 && h > 0 {
            frame = image::imageops::crop_imm(&frame, x, y, w, h).to_image();
        }
    }
    if frame.width() > FRAME_WIDTH {
        let height = (frame.height() as u64 * FRAME_WIDTH as u64 / frame.width() as u64) as u32;
        frame = image::imageops::resize(
            &frame,
            FRAME_WIDTH,
            height.max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 72)
        .encode_image(&frame)
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_late_page_gets_the_newest_of_each_snapshot_in_the_order_they_changed() {
        let mut kept = Kept::default();
        kept.keep(r#"{"t":"mode","history":true,"document":false}"#.into());
        kept.keep(r#"{"t":"history","rows":[],"current":null}"#.into());
        kept.keep(r#"{"t":"term.data","id":"t1","b64":"aGk="}"#.into());
        kept.keep(r#"{"t":"thumb","id":"7","url":"data:a"}"#.into());
        kept.keep(r#"{"t":"thumb","id":"8","url":"data:b"}"#.into());
        kept.keep(r#"{"t":"doc.reveal","line":3}"#.into());
        kept.keep(r#"{"t":"mode","history":true,"document":true}"#.into());
        kept.keep(r#"{"t":"thumb","id":"7","url":"data:c"}"#.into());
        let newest: Vec<&str> = kept.newest().map(String::as_str).collect();
        assert_eq!(
            newest,
            [
                r#"{"t":"history","rows":[],"current":null}"#,
                r#"{"t":"thumb","id":"8","url":"data:b"}"#,
                r#"{"t":"mode","history":true,"document":true}"#,
                r#"{"t":"thumb","id":"7","url":"data:c"}"#,
            ]
        );
    }

    #[test]
    fn another_sites_page_may_not_pair_or_connect() {
        let headers = |origin: Option<&str>| {
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, "192.168.1.5:9880".parse().unwrap());
            if let Some(origin) = origin {
                headers.insert(header::ORIGIN, origin.parse().unwrap());
            }
            headers
        };
        assert!(!foreign(&headers(Some("http://192.168.1.5:9880"))));
        assert!(!foreign(&headers(None)), "a tool, not a browser");
        assert!(foreign(&headers(Some("https://example.com"))));
        assert!(foreign(&headers(Some("http://192.168.1.5:9881"))));
    }

    /// The Phone button's asks: on, with a code at once when nothing is
    /// paired; a device forgotten; off — each remembered.
    #[test]
    fn the_window_turns_it_on_pairs_forgets_and_turns_it_off() {
        let dir = std::env::temp_dir().join(format!("lg-remote-on-{}", std::process::id()));
        let path = dir.join("app-remote.json");
        let device = |id: &str| Device {
            id: id.into(),
            name: format!("phone {id}"),
            key_sha256: sha256_hex(id.as_bytes()),
            paired_ms: 1,
            seen_ms: 2,
        };
        write_private(
            &path,
            &Saved {
                version: VERSION,
                on: false,
                devices: vec![device("a"), device("b")],
            },
        )
        .unwrap();
        // Any free port on loopback: a running app's 9880 is left alone, and
        // the firewall has nothing to ask.
        let mut remote = Remote::at(Some(path.clone()), 0);
        remote.host = std::net::Ipv4Addr::LOCALHOST;
        assert!(!remote.is_on());
        assert_eq!(remote.view().devices.len(), 2);

        let before = remote.rev();
        remote.set_on(true);
        assert!(remote.is_on(), "{:?}", remote.view().error);
        assert!(remote.rev() > before, "the window's page is told");
        assert!(load(&path).on, "remembered");
        // Devices are paired already: no code until one is asked for. (A
        // computer with no network has no address to pair with.)
        assert!(remote.view().pairing.is_none());
        if remote.view().url.is_some() {
            let link = remote.pair().expect("a pairing link");
            let pairing = remote.view().pairing.expect("the code shows");
            assert_eq!(pairing.url, link);
            assert!(
                pairing
                    .qr
                    .as_deref()
                    .is_some_and(|svg| svg.starts_with("<?xml") || svg.contains("<svg"))
            );
            assert!(pairing.left_s > 500);
        }

        remote.forget("a");
        let names: Vec<String> = remote.view().devices.into_iter().map(|d| d.id).collect();
        assert_eq!(names, ["b"]);
        assert_eq!(load(&path).devices.len(), 1, "forgotten on disk too");

        remote.set_on(false);
        assert!(!remote.is_on());
        assert!(
            remote.view().pairing.is_none(),
            "a code goes with the listener"
        );
        assert!(!load(&path).on);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn what_is_remembered_is_the_owners_alone() {
        let dir = std::env::temp_dir().join(format!("lg-remote-{}", std::process::id()));
        let path = dir.join("app-remote.json");
        let saved = Saved {
            version: VERSION,
            on: true,
            devices: vec![Device {
                id: "ab12cd34".into(),
                name: "iPhone".into(),
                key_sha256: sha256_hex(b"key"),
                paired_ms: 1,
                seen_ms: 2,
            }],
        };
        write_private(&path, &saved).unwrap();
        let back = load(&path);
        assert!(back.on);
        assert_eq!(back.devices[0].key_sha256, sha256_hex(b"key"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
