//! Relay client (`--relay`): reach guests beyond the LAN through a
//! `localgpt-relay`.
//!
//! The host POSTs `/rooms`, connects OUT to the relay (no port forwarding),
//! and every relay guest becomes a peer in the same [`WebBridge`] local
//! sockets use — frames ride the relay's envelope, the room protocol inside
//! is unchanged, and the invite token is still checked end-to-end. Local
//! LAN guests keep working alongside relay guests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use localgpt_world_sync::ClientMsg;
use tokio::sync::mpsc;

use super::web::{InboundEvent, WebBridge};

/// The relay's envelope on the host connection (kept in sync with
/// `crates/relay/src/main.rs`).
#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: String,
    conn: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<String>,
}

impl Envelope {
    fn frame(conn: u64, data: String) -> String {
        serde_json::to_string(&Envelope {
            kind: "frame".into(),
            conn,
            data: Some(data),
        })
        .expect("envelope")
    }
}

#[derive(serde::Deserialize)]
struct CreateRoomResponse {
    code: String,
    host_secret: String,
}

/// Start the relay client on a background thread. Prints the internet
/// invite link (the room code inside the page URL plus the invite token).
pub fn start_relay_client(bridge: WebBridge, relay_url: &str, token: Option<String>) {
    let relay_url = relay_url.to_string();
    std::thread::Builder::new()
        .name("gen-relay-client".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("relay: runtime failed: {e}");
                    return;
                }
            };
            rt.block_on(run(bridge, relay_url, token));
        })
        .expect("spawn relay client");
}

async fn run(bridge: WebBridge, relay_url: String, token: Option<String>) {
    use futures::{SinkExt, StreamExt};

    let http_base = relay_url.trim_end_matches('/');
    // Register the room.
    let room: CreateRoomResponse = match reqwest::Client::new()
        .post(format!("{http_base}/rooms"))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) => match resp.json().await {
            Ok(room) => room,
            Err(e) => {
                eprintln!("relay: bad /rooms reply: {e}");
                return;
            }
        },
        Err(e) => {
            eprintln!("relay: couldn't reach {http_base} ({e})");
            return;
        }
    };

    let ws_base = http_base
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let ws_url = format!(
        "{ws_base}/rooms/{}/host?secret={}",
        room.code, room.host_secret
    );
    let (mut ws, _) = match tokio_tungstenite::connect_async(&ws_url).await {
        Ok(ok) => ok,
        Err(e) => {
            eprintln!("relay: host socket failed: {e}");
            return;
        }
    };

    let page_url = format!("{http_base}/r/{}/", room.code);
    match &token {
        Some(token) => eprintln!(
            "
  Internet guests: {page_url}#t={token}
"
        ),
        None => eprintln!(
            "
  Internet guests: {page_url}  (open session)
"
        ),
    }

    // Relay conn id ↔ bridge peer id, and each peer's frame sender.
    let peers: Arc<StdMutex<HashMap<u64, u64>>> = Arc::new(StdMutex::new(HashMap::new()));
    // Frames headed to the relay, wrapped per guest.
    let (relay_out_tx, mut relay_out_rx) = mpsc::unbounded_channel::<String>();

    loop {
        tokio::select! {
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        let Ok(env) = serde_json::from_str::<Envelope>(&text) else {
                            continue;
                        };
                        match env.kind.as_str() {
                            "open" => {
                                let peer = bridge.alloc_peer_id();
                                let (frame_tx, mut frame_rx) =
                                    mpsc::unbounded_channel::<super::web::OutboundFrame>();
                                bridge.insert_conn(peer, frame_tx);
                                peers.lock().expect("peers").insert(env.conn, peer);
                                let _ = bridge.send_inbound(InboundEvent::Connected { id: peer });
                                // Forward this guest's outbound frames to the relay.
                                let relay_out = relay_out_tx.clone();
                                let conn = env.conn;
                                tokio::spawn(async move {
                                    while let Some(frame) = frame_rx.recv().await {
                                        let _ = relay_out.send(Envelope::frame(conn, frame.text));
                                    }
                                });
                            }
                            "frame" => {
                                let Some(data) = env.data else { continue };
                                let Some(peer) = peers.lock().expect("peers").get(&env.conn).copied() else {
                                    continue;
                                };
                                match serde_json::from_str::<ClientMsg>(&data) {
                                    Ok(msg) => {
                                        let _ = bridge.send_inbound(InboundEvent::Message { id: peer, msg });
                                    }
                                    Err(e) => {
                                        tracing::warn!("relay guest frame didn't parse: {e}");
                                    }
                                }
                            }
                            "close" => {
                                if let Some(peer) = peers.lock().expect("peers").remove(&env.conn) {
                                    bridge.remove_conn(peer);
                                    let _ = bridge.send_inbound(InboundEvent::Disconnected { id: peer });
                                }
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => {
                        eprintln!("relay: connection closed — internet guests disconnected");
                        return;
                    }
                    Some(Err(e)) => {
                        eprintln!("relay: socket error: {e}");
                        return;
                    }
                    Some(Ok(_)) => {}
                }
            }
            out = relay_out_rx.recv() => {
                let Some(text) = out else { return };
                if ws.send(tokio_tungstenite::tungstenite::Message::Text(text.into())).await.is_err() {
                    return;
                }
            }
        }
    }
}
