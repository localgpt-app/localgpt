//! WebSocket inspector server — bridges Bevy ECS state to remote clients.
//!
//! Runs an Axum HTTP/WebSocket server on a background tokio task.
//! Bevy systems push state via channels; the server broadcasts to clients.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use bevy::prelude::*;
use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use tokio::sync::{Mutex, broadcast, mpsc};

use super::protocol::*;

// ---------------------------------------------------------------------------
// Bevy ↔ Server bridge channels
// ---------------------------------------------------------------------------

/// Messages from Bevy → WS server (outbound to all clients).
pub type BevyToWsTx = mpsc::UnboundedSender<ServerMessage>;
pub type BevyToWsRx = mpsc::UnboundedReceiver<ServerMessage>;

/// Messages from WS clients → Bevy (inbound from any client).
pub type WsToBevyTx = mpsc::UnboundedSender<ClientMessage>;
pub type WsToBevyRx = mpsc::UnboundedReceiver<ClientMessage>;

/// Bevy resource: the send-side channels for pushing state to remote clients.
#[derive(Resource)]
pub struct InspectorWsBridge {
    /// Send server messages to be broadcast to all connected clients.
    pub tx: BevyToWsTx,
    /// Receive client messages from remote inspector clients.
    pub rx: Arc<Mutex<WsToBevyRx>>,
    /// Send raw binary data (e.g. GLB snapshots) to all connected clients.
    binary_tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl InspectorWsBridge {
    /// Send binary data (GLB scene snapshot) to all connected clients.
    pub fn send_binary(&self, data: Vec<u8>) {
        let _ = self.binary_tx.send(data);
    }
}

// ---------------------------------------------------------------------------
// Server state
// ---------------------------------------------------------------------------

/// Payload that can be broadcast to clients.
#[derive(Clone)]
enum BroadcastPayload {
    Text(String),
    Binary(Vec<u8>),
}

/// Shared state for the Axum WebSocket server.
#[derive(Clone)]
struct ServerState {
    /// Broadcast channel for server → client messages.
    broadcast_tx: broadcast::Sender<BroadcastPayload>,
    /// Channel for client → Bevy messages.
    client_to_bevy: WsToBevyTx,
}

// ---------------------------------------------------------------------------
// Bevy plugin + systems
// ---------------------------------------------------------------------------

/// Default port for the inspector WebSocket server. `LOCALGPT_INSPECTOR_PORT`
/// overrides it, so a second canvas on the same desk (a smoke run beside a
/// person's window) has its own.
const DEFAULT_PORT: u16 = 9877;

fn port() -> u16 {
    std::env::var("LOCALGPT_INSPECTOR_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// Starts the WebSocket server and inserts bridge resources into the Bevy app.
pub fn start_ws_server(app: &mut App) {
    let (bevy_to_ws_tx, mut bevy_to_ws_rx) = mpsc::unbounded_channel::<ServerMessage>();
    let (ws_to_bevy_tx, ws_to_bevy_rx) = mpsc::unbounded_channel::<ClientMessage>();
    let (binary_tx, mut binary_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // Broadcast channel for fan-out to all connected clients
    let (broadcast_tx, _) = broadcast::channel::<BroadcastPayload>(256);
    let broadcast_tx_clone = broadcast_tx.clone();
    let broadcast_tx_binary = broadcast_tx.clone();

    let state = ServerState {
        broadcast_tx: broadcast_tx.clone(),
        client_to_bevy: ws_to_bevy_tx,
    };

    // Spawn the Axum server on the tokio runtime
    // Gen mode spawns tokio on a background thread, so we use tokio::spawn
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for inspector WS");

        rt.block_on(async move {
            // Bridge: forward bevy_to_ws JSON messages to broadcast channel
            let bridge_broadcast_tx = broadcast_tx_clone;
            tokio::spawn(async move {
                while let Some(msg) = bevy_to_ws_rx.recv().await {
                    if let Ok(json) = serde_json::to_string(&msg) {
                        let _ = bridge_broadcast_tx.send(BroadcastPayload::Text(json));
                    }
                }
            });

            // Bridge: forward binary data (GLB snapshots) to broadcast channel
            tokio::spawn(async move {
                while let Some(data) = binary_rx.recv().await {
                    let _ = broadcast_tx_binary.send(BroadcastPayload::Binary(data));
                }
            });

            let router = Router::new()
                .route("/ws", get(ws_handler))
                .route("/health", get(health_handler))
                .with_state(state);

            let port = port();
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            info!("Inspector WebSocket server listening on ws://{}/ws", addr);

            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    warn!("Inspector WS server failed to bind port {}: {}", port, e);
                    return;
                }
            };

            if let Err(e) = axum::serve(listener, router).await {
                warn!("Inspector WS server error: {}", e);
            }
        });
    });

    app.insert_resource(InspectorWsBridge {
        tx: bevy_to_ws_tx,
        rx: Arc::new(Mutex::new(ws_to_bevy_rx)),
        binary_tx,
    });
}

// ---------------------------------------------------------------------------
// HTTP handlers
// ---------------------------------------------------------------------------

async fn health_handler() -> &'static str {
    "inspector ok"
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<ServerState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws_connection(socket, state))
}

// ---------------------------------------------------------------------------
// WebSocket connection handler
// ---------------------------------------------------------------------------

async fn handle_ws_connection(socket: WebSocket, state: ServerState) {
    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Auto-request scene tree + world info so the client gets initial state
    let _ = state.client_to_bevy.send(ClientMessage::RequestSceneTree);
    let _ = state.client_to_bevy.send(ClientMessage::RequestWorldInfo);

    // Subscribe to broadcast channel for server→client messages
    let mut broadcast_rx = state.broadcast_tx.subscribe();

    // Track this client's subscribed topics
    let topics: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

    // Task: forward broadcast messages to this client (filtered by topics)
    let sender_clone = sender.clone();
    let topics_clone = topics.clone();
    let send_task = tokio::spawn(async move {
        while let Ok(payload) = broadcast_rx.recv().await {
            match payload {
                BroadcastPayload::Text(json) => {
                    if should_send_to_client(&json, &topics_clone).await
                        && send_text(&sender_clone, &json).await.is_err()
                    {
                        break;
                    }
                }
                BroadcastPayload::Binary(data) => {
                    if send_binary(&sender_clone, data).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Task: receive client messages and forward to Bevy
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                        // Handle subscribe locally (update topic filter)
                        if let ClientMessage::Subscribe { topics: new_topics } = &client_msg {
                            let mut t = topics.lock().await;
                            t.extend(new_topics.iter().cloned());
                        }
                        // Forward all messages to Bevy
                        let _ = state.client_to_bevy.send(client_msg);
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to finish (client disconnect)
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Check if a JSON server message should be sent to a client based on topics.
async fn should_send_to_client(json: &str, topics: &Arc<Mutex<HashSet<String>>>) -> bool {
    let t = topics.lock().await;
    if t.is_empty() {
        // No topic filter = receive everything
        return true;
    }

    // Extract message type from JSON for topic matching
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(json)
        && let Some(msg_type) = value.get("type").and_then(|v| v.as_str())
    {
        return t.contains(msg_type);
    }
    true
}

async fn send_text(
    sender: &Arc<Mutex<SplitSink<WebSocket, Message>>>,
    text: &str,
) -> Result<(), axum::Error> {
    let mut s = sender.lock().await;
    s.send(Message::text(text))
        .await
        .map_err(|_| axum::Error::new("send failed"))
}

async fn send_binary(
    sender: &Arc<Mutex<SplitSink<WebSocket, Message>>>,
    data: Vec<u8>,
) -> Result<(), axum::Error> {
    let mut s = sender.lock().await;
    s.send(Message::binary(data))
        .await
        .map_err(|_| axum::Error::new("send failed"))
}
