//! localgpt-relay — internet reach for LocalGPT collaborative rooms.
//!
//! A host runs behind NAT and connects OUT to the relay; guests connect to
//! the relay with a room code from the invite link. The relay forwards
//! WebSocket frames between them, wrapped in a tiny envelope — it never
//! inspects or holds world state, so the room's secrets stay end-to-end.
//!
//! Endpoints:
//! - `POST /rooms` → create a room: `{code, host_secret}`
//! - `GET  /rooms/<code>/host?secret=…` (WS) — the host's one connection
//! - `GET  /r/<code>/` — the join page (same client a host serves)
//! - `GET  /r/<code>/session` (WS) — a guest connection
//!
//! Envelope on the host connection (JSON text frames):
//! `{"type":"open"|"close","conn":N}` and `{"type":"frame","conn":N,"data":"…"}`
//! where `data` is the room protocol's raw message, opaque to the relay.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, RwLock};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use futures::{SinkExt, StreamExt};
use localgpt_world_export::html as export_html;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// Largest frame the relay forwards.
const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// Most guests per room.
const MAX_GUESTS: usize = 64;
/// Most live rooms.
const MAX_ROOMS: usize = 256;

/// One live room.
struct Room {
    host: mpsc::UnboundedSender<String>,
    host_secret: String,
    guests: HashMap<u64, mpsc::UnboundedSender<String>>,
    next_conn: u64,
}

#[derive(Clone, Default)]
struct RelayState {
    rooms: Arc<RwLock<HashMap<String, Room>>>,
}

#[derive(Serialize)]
struct CreateRoomResponse {
    code: String,
    host_secret: String,
}

/// The host-connection envelope.
#[derive(Serialize, Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: String,
    conn: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<String>,
}

impl Envelope {
    fn open(conn: u64) -> String {
        serde_json::to_string(&Envelope {
            kind: "open".into(),
            conn,
            data: None,
        })
        .expect("envelope")
    }
    fn close(conn: u64) -> String {
        serde_json::to_string(&Envelope {
            kind: "close".into(),
            conn,
            data: None,
        })
        .expect("envelope")
    }
    fn frame(conn: u64, data: String) -> String {
        serde_json::to_string(&Envelope {
            kind: "frame".into(),
            conn,
            data: Some(data),
        })
        .expect("envelope")
    }
}

fn room_code() -> String {
    // Unambiguous alphabet (no 0/O, 1/I/L).
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let bytes: [u8; 6] = rand::random();
    bytes
        .iter()
        .map(|b| ALPHABET[(b % ALPHABET.len() as u8) as usize] as char)
        .collect()
}

fn host_secret() -> String {
    let bytes: [u8; 16] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "localgpt_relay=info".into()),
        )
        .init();

    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(9911);

    let app = axum::Router::new()
        .route("/rooms", post(create_room))
        .route("/rooms/{code}/host", get(host_ws))
        .route("/r/{code}/", get(join_page))
        .route("/r/{code}", get(join_page))
        .route("/r/{code}/session", get(guest_ws))
        .route("/r/{code}/world-viewer.js", get(viewer_js))
        .route("/r/{code}/session-client.js", get(client_js))
        .route("/r/{code}/vendor/three.module.js", get(vendor_three))
        .route(
            "/r/{code}/vendor/three/addons/controls/OrbitControls.js",
            get(vendor_orbit),
        )
        .route(
            "/r/{code}/vendor/three/addons/loaders/GLTFLoader.js",
            get(vendor_gltf),
        )
        .route(
            "/r/{code}/vendor/three/addons/utils/BufferGeometryUtils.js",
            get(vendor_bgu),
        )
        .route("/healthz", get(|| async { "ok" }))
        .with_state(RelayState::default());

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("relay bind");
    tracing::info!(
        "localgpt-relay listening on {addr} — hosts POST /rooms, guests open /r/<code>/"
    );
    axum::serve(listener, app).await.expect("relay serve");
}

async fn create_room(State(state): State<RelayState>) -> impl IntoResponse {
    let mut rooms = state.rooms.write().expect("rooms lock");
    if rooms.len() >= MAX_ROOMS {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "too many rooms — try again later",
        )
            .into_response();
    }
    let (code, host_secret) = loop {
        let code = room_code();
        if !rooms.contains_key(&code) {
            break (code, host_secret());
        }
    };
    let (host_tx, _host_rx) = mpsc::unbounded_channel::<String>();
    rooms.insert(
        code.clone(),
        Room {
            host: host_tx.clone(),
            host_secret: host_secret.clone(),
            guests: HashMap::new(),
            next_conn: 1,
        },
    );
    tracing::info!("room {code} created");
    // The sender half is replaced by the real host connection; this one
    // only holds the slot until then.
    drop(host_tx);
    axum::Json(CreateRoomResponse { code, host_secret }).into_response()
}

#[derive(Deserialize)]
struct HostAuth {
    secret: Option<String>,
}

async fn host_ws(
    ws: WebSocketUpgrade,
    State(state): State<RelayState>,
    Path(code): Path<String>,
    Query(auth): Query<HostAuth>,
) -> Response {
    let Some(host_secret) = ({
        let rooms = state.rooms.read().expect("rooms lock");
        rooms.get(&code).map(|r| r.host_secret.clone())
    }) else {
        return (StatusCode::NOT_FOUND, "no such room").into_response();
    };
    if auth.secret.as_deref() != Some(host_secret.as_str()) {
        return (StatusCode::FORBIDDEN, "bad host secret").into_response();
    }
    ws.max_message_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| run_host(code, socket, state))
}

/// The host's one connection: guests open/close/frame through here.
async fn run_host(code: String, socket: WebSocket, state: RelayState) {
    let (host_tx, mut host_rx) = mpsc::unbounded_channel::<String>();
    // Install the real host sender, closing every guest from a previous
    // host connection (reconnect = a fresh room).
    let guest_conns: Vec<u64> = {
        let mut rooms = state.rooms.write().expect("rooms lock");
        let Some(room) = rooms.get_mut(&code) else {
            return;
        };
        room.host = host_tx.clone();
        room.guests.keys().copied().collect()
    };
    {
        let mut rooms = state.rooms.write().expect("rooms lock");
        if let Some(room) = rooms.get_mut(&code) {
            room.guests.clear();
        }
    }
    tracing::info!("room {code}: host connected");

    let (mut sink, mut stream) = socket.split();
    let writer = tokio::spawn(async move {
        while let Some(text) = host_rx.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = stream.next().await {
        let Message::Text(text) = msg else {
            continue;
        };
        let Ok(env) = serde_json::from_str::<Envelope>(&text) else {
            continue;
        };
        if env.kind != "frame" {
            continue;
        }
        let Some(data) = env.data else {
            continue;
        };
        let guest_tx = {
            let rooms = state.rooms.read().expect("rooms lock");
            rooms
                .get(&code)
                .and_then(|room| room.guests.get(&env.conn).cloned())
        };
        if let Some(tx) = guest_tx {
            let _ = tx.send(data);
        }
    }

    // Host gone: close the room and every guest.
    tracing::info!("room {code}: host disconnected — closing room");
    let guests = {
        let mut rooms = state.rooms.write().expect("rooms lock");
        rooms.remove(&code).map(|room| {
            room.guests
                .into_values()
                .collect::<Vec<mpsc::UnboundedSender<String>>>()
        })
    };
    if let Some(guests) = guests {
        for tx in guests {
            drop(tx);
        }
    }
    writer.abort();
    let _ = guest_conns;
}

async fn guest_ws(
    ws: WebSocketUpgrade,
    State(state): State<RelayState>,
    Path(code): Path<String>,
) -> Response {
    let (host_tx, conn_id) = {
        let mut rooms = state.rooms.write().expect("rooms lock");
        match rooms.get_mut(&code) {
            Some(room) if room.guests.len() < MAX_GUESTS => {
                let id = room.next_conn;
                room.next_conn += 1;
                (room.host.clone(), id)
            }
            _ => return (StatusCode::NOT_FOUND, "no such room (or it is full)").into_response(),
        }
    };
    ws.max_message_size(MAX_FRAME_BYTES)
        .on_upgrade(move |socket| run_guest(code, conn_id, socket, state, host_tx))
}

async fn run_guest(
    code: String,
    conn: u64,
    socket: WebSocket,
    state: RelayState,
    host: mpsc::UnboundedSender<String>,
) {
    let (guest_tx, mut guest_rx) = mpsc::unbounded_channel::<String>();
    {
        let mut rooms = state.rooms.write().expect("rooms lock");
        let Some(room) = rooms.get_mut(&code) else {
            return;
        };
        room.guests.insert(conn, guest_tx);
    }
    let _ = host.send(Envelope::open(conn));
    tracing::debug!("room {code}: guest {conn} connected");

    let (mut sink, mut stream) = socket.split();
    let writer = tokio::spawn(async move {
        while let Some(text) = guest_rx.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Text(text) => {
                let _ = host.send(Envelope::frame(conn, text.to_string()));
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    {
        let mut rooms = state.rooms.write().expect("rooms lock");
        if let Some(room) = rooms.get_mut(&code) {
            room.guests.remove(&conn);
        }
    }
    let _ = host.send(Envelope::close(conn));
    tracing::debug!("room {code}: guest {conn} disconnected");
    writer.abort();
}

// ---------------------------------------------------------------------------
// The join page + statics (the same client a host serves itself)
// ---------------------------------------------------------------------------

async fn join_page() -> impl IntoResponse {
    Html(export_html::SESSION_JOIN_PAGE_HTML)
}

async fn viewer_js() -> impl IntoResponse {
    js(export_html::WORLD_VIEWER_JS)
}

async fn client_js() -> impl IntoResponse {
    js(export_html::SESSION_CLIENT_JS)
}

async fn vendor_three() -> impl IntoResponse {
    js(export_html::THREE_MODULE_JS)
}

async fn vendor_orbit() -> impl IntoResponse {
    js(export_html::ORBIT_CONTROLS_JS)
}

async fn vendor_gltf() -> impl IntoResponse {
    js(export_html::GLTF_LOADER_JS)
}

async fn vendor_bgu() -> impl IntoResponse {
    js(export_html::BUFFER_GEOMETRY_UTILS_JS)
}

fn js(body: &'static str) -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/javascript; charset=utf-8",
        )],
        body,
    )
}
