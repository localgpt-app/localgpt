//! Relay client (`--relay`): reach guests beyond the LAN through a
//! `localgpt-relay`.
//!
//! The host POSTs `/rooms`, connects OUT to the relay (no port forwarding),
//! and every relay guest becomes a peer in the same [`WebBridge`] local
//! sockets use — frames ride the relay's envelope and the room protocol
//! inside is unchanged. A relayed guest is let in by the invite token
//! alone, never the session PIN (anyone with the room code can open a
//! relayed connection), and a refused hello ends its connection as it ends
//! a direct one. The token and everything else still cross the relay in
//! the clear, so the relay can read them. Local LAN guests keep working
//! alongside relay guests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use localgpt_world_sync::{ClientMsg, PeerId};
use tokio::sync::mpsc;

use super::web::{InboundEvent, OutboundFrame, WebBridge};

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

    /// Ends a guest's connection at the relay.
    fn close(conn: u64) -> String {
        serde_json::to_string(&Envelope {
            kind: "close".into(),
            conn,
            data: None,
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
    let peers: Arc<StdMutex<HashMap<u64, PeerId>>> = Arc::new(StdMutex::new(HashMap::new()));
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
                                let (frame_tx, frame_rx) =
                                    mpsc::unbounded_channel::<OutboundFrame>();
                                bridge.insert_conn(peer, frame_tx);
                                peers.lock().expect("peers").insert(env.conn, peer);
                                let _ = bridge.send_inbound(InboundEvent::Connected { id: peer });
                                tokio::spawn(forward_guest(
                                    env.conn,
                                    peer,
                                    frame_rx,
                                    relay_out_tx.clone(),
                                    peers.clone(),
                                    bridge.clone(),
                                ));
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

/// Forward one relay guest's outbound frames to the relay. A frame that
/// closes the connection (a refused hello) is followed by a close envelope,
/// so the relay ends the guest's socket as the host ends a direct one; and
/// the guest is forgotten on this side at once, so a relay that ignores the
/// close can't keep it talking to the room.
async fn forward_guest(
    conn: u64,
    peer: PeerId,
    mut frames: mpsc::UnboundedReceiver<OutboundFrame>,
    relay_out: mpsc::UnboundedSender<String>,
    peers: Arc<StdMutex<HashMap<u64, PeerId>>>,
    bridge: WebBridge,
) {
    while let Some(frame) = frames.recv().await {
        let _ = relay_out.send(Envelope::frame(conn, frame.text));
        if frame.close {
            let _ = relay_out.send(Envelope::close(conn));
            peers.lock().expect("peers").remove(&conn);
            bridge.remove_conn(peer);
            let _ = bridge.send_inbound(InboundEvent::Disconnected { id: peer });
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closing_frame_ends_the_relayed_connection_on_both_sides() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let (bridge, mut inbound) = WebBridge::new(None, "test");
            let peer = bridge.alloc_peer_id();
            let (frame_tx, frame_rx) = mpsc::unbounded_channel();
            bridge.insert_conn(peer, frame_tx.clone());
            let peers = Arc::new(StdMutex::new(HashMap::from([(3, peer)])));
            let (relay_out, mut to_relay) = mpsc::unbounded_channel();

            frame_tx
                .send(OutboundFrame {
                    text: "{\"type\":\"error\"}".into(),
                    close: true,
                })
                .unwrap();
            forward_guest(3, peer, frame_rx, relay_out, peers.clone(), bridge.clone()).await;

            let sent: Vec<Envelope> = std::iter::from_fn(|| to_relay.try_recv().ok())
                .map(|text| serde_json::from_str(&text).unwrap())
                .collect();
            let sent: Vec<(&str, u64, Option<&str>)> = sent
                .iter()
                .map(|e| (e.kind.as_str(), e.conn, e.data.as_deref()))
                .collect();
            assert_eq!(
                sent,
                [
                    ("frame", 3, Some("{\"type\":\"error\"}")),
                    ("close", 3, None)
                ]
            );
            assert!(peers.lock().unwrap().is_empty());
            assert!(matches!(
                inbound.try_recv(),
                Ok(InboundEvent::Disconnected { id }) if id == peer
            ));
        });
    }
}
