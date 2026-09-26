//! The native ops client (`--join`): a full Gen scene driven by the room.
//!
//! One WebSocket to the host's `/session`: the welcome brings the whole
//! world (spawned through the same `OpsApplier` undo and resume use), ops
//! stream in live, presence and chat flow both ways, and prompts enter the
//! room's job queue. Because the client runs the full gen3d app, behaviors
//! tick and audio plays exactly as on the host — no second scene runtime.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::Duration;

use bevy::prelude::*;
use localgpt_world_sync as sync;
use localgpt_world_sync::{ClientMsg, PeerId, PeerInfo, Presence, ServerMsg, WorldDoc};
use localgpt_world_types as wt;
use tokio::sync::mpsc;

use crate::gen3d::ops_apply::OpsApplier;
use crate::gen3d::registry::{GenEntity, GenEntityType};

/// Options for the client plugin, built by `run_join_mode`.
pub struct OpsClientOptions {
    pub server_addr: SocketAddr,
    /// The session PIN (None for open sessions).
    pub pin: Option<String>,
    /// Display name shown to the room.
    pub name: String,
    /// Lines typed at the client REPL (or the desktop panel).
    pub prompt_rx: mpsc::UnboundedReceiver<String>,
}

/// One line in the client panel's log.
#[derive(Debug, Clone)]
pub enum ClientEntry {
    You(String),
    Chat { speaker: String, text: String },
    Status(String),
    Error(String),
}

/// The client panel's scrolling log (bounded).
#[derive(Resource, Default)]
pub struct ClientPanelLog {
    pub entries: std::collections::VecDeque<ClientEntry>,
}

impl ClientPanelLog {
    pub fn push(&mut self, entry: ClientEntry) {
        const MAX_ENTRIES: usize = 200;
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.pop_front();
        }
        // The REPL client has no panel: print there too.
        match &entry {
            ClientEntry::You(text) => eprintln!("You: {text}"),
            ClientEntry::Chat { speaker, text } => eprintln!("[{speaker}] {text}"),
            ClientEntry::Status(text) => eprintln!("{text}"),
            ClientEntry::Error(text) => eprintln!("error: {text}"),
        }
        self.entries.push_back(entry);
    }
}

/// Everything the Bevy side knows about the connection.
#[derive(Resource)]
pub struct OpsClient {
    doc: WorldDoc,
    revision: u64,
    peer_id: Option<PeerId>,
    peers: HashMap<PeerId, PeerInfo>,
    /// WS task → Bevy.
    events: Mutex<mpsc::UnboundedReceiver<ClientEvent>>,
    /// Bevy → WS task.
    outbound: mpsc::UnboundedSender<ClientMsg>,
    prompt_rx: Mutex<mpsc::UnboundedReceiver<String>>,
    presence_timer: Timer,
    request_seq: u64,
    /// A fresh world to spawn next frame (welcome/snapshot).
    pending_world: bool,
    /// Ops to apply to the scene next frame.
    pending_ops: Vec<wt::EditOp>,
    /// Scaffold entity per running job.
    scaffolds: HashMap<u64, Entity>,
}

/// Lifecycle + messages from the WS task.
pub enum ClientEvent {
    Connected,
    Failed(String),
    Closed(String),
    Msg(ServerMsg),
}

pub struct OpsClientPlugin {
    pub options: Mutex<Option<OpsClientOptions>>,
}

impl Plugin for OpsClientPlugin {
    fn build(&self, app: &mut App) {
        let options = self
            .options
            .lock()
            .expect("OpsClientPlugin options lock poisoned")
            .take()
            .expect("OpsClientPlugin options consumed twice");

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel();
        spawn_ws_task(
            options.server_addr,
            options.pin,
            options.name,
            event_tx,
            outbound_rx,
        );

        app.insert_resource(OpsClient {
            doc: WorldDoc::new("client"),
            revision: 0,
            peer_id: None,
            peers: HashMap::new(),
            events: Mutex::new(event_rx),
            outbound: outbound_tx,
            prompt_rx: Mutex::new(options.prompt_rx),
            presence_timer: Timer::new(Duration::from_millis(200), TimerMode::Repeating),
            request_seq: 0,
            pending_world: false,
            pending_ops: Vec::new(),
            scaffolds: HashMap::new(),
        })
        .init_resource::<crate::net::guest_avatars::GuestAvatars>()
        .init_resource::<ClientPanelLog>()
        .add_systems(
            Update,
            (
                ops_inbound,
                ops_apply,
                ops_prompts,
                ops_presence,
                ops_avatars,
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// The WebSocket task (its own thread + current-thread runtime)
// ---------------------------------------------------------------------------

fn spawn_ws_task(
    addr: SocketAddr,
    pin: Option<String>,
    name: String,
    events: mpsc::UnboundedSender<ClientEvent>,
    mut outbound: mpsc::UnboundedReceiver<ClientMsg>,
) {
    std::thread::Builder::new()
        .name("gen-ops-client".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = events.send(ClientEvent::Failed(format!("runtime: {e}")));
                    return;
                }
            };
            rt.block_on(async move {
                use futures::{SinkExt, StreamExt};
                let url = format!("ws://{addr}/session");
                let (mut ws, _) = match tokio_tungstenite::connect_async(&url).await {
                    Ok(ok) => ok,
                    Err(e) => {
                        let _ = events.send(ClientEvent::Failed(format!(
                            "couldn't connect to {url} ({e})"
                        )));
                        return;
                    }
                };
                let hello = ClientMsg::Hello {
                    protocol: sync::PROTOCOL_VERSION,
                    name,
                    token: pin,
                    client: sync::ClientKind::Gen,
                };
                let Ok(text) = serde_json::to_string(&hello) else {
                    return;
                };
                if ws.send(text.into()).await.is_err() {
                    return;
                }
                let _ = events.send(ClientEvent::Connected);
                loop {
                    tokio::select! {
                        msg = ws.next() => {
                            match msg {
                                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                    match serde_json::from_str::<ServerMsg>(&text) {
                                        Ok(m) => {
                                            if events.send(ClientEvent::Msg(m)).is_err() {
                                                return;
                                            }
                                        }
                                        Err(e) => {
                                            let _ = events.send(ClientEvent::Failed(format!("bad message: {e}")));
                                        }
                                    }
                                }
                                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) | None => {
                                    let _ = events.send(ClientEvent::Closed("session closed".into()));
                                    return;
                                }
                                Some(Err(e)) => {
                                    let _ = events.send(ClientEvent::Closed(format!("socket error: {e}")));
                                    return;
                                }
                                Some(Ok(_)) => {}
                            }
                        }
                        out = outbound.recv() => {
                            let Some(msg) = out else { return };
                            let Ok(text) = serde_json::to_string(&msg) else { continue };
                            if ws.send(text.into()).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            });
        })
        .expect("spawn ops client thread");
}

// ---------------------------------------------------------------------------
// Inbound: server messages → local document + scene
// ---------------------------------------------------------------------------

fn ops_inbound(mut client: ResMut<OpsClient>, mut log: ResMut<ClientPanelLog>) {
    let events: Vec<ClientEvent> = {
        let Ok(mut rx) = client.events.lock() else {
            return;
        };
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    };
    for event in events {
        match event {
            ClientEvent::Connected => log.push(ClientEntry::Status("connected".into())),
            ClientEvent::Failed(reason) => log.push(ClientEntry::Error(reason)),
            ClientEvent::Closed(reason) => {
                log.push(ClientEntry::Status(format!("disconnected ({reason})")))
            }
            ClientEvent::Msg(msg) => handle_server_msg(&mut client, &mut log, msg),
        }
    }
}

fn handle_server_msg(client: &mut OpsClient, log: &mut ClientPanelLog, msg: ServerMsg) {
    match msg {
        ServerMsg::Welcome {
            peer_id,
            session,
            revision,
            world,
            peers,
            ..
        } => {
            client.peer_id = Some(peer_id);
            client.revision = revision;
            client.peers = peers.into_iter().map(|p| (p.id, p)).collect();
            let count = world.entities.len();
            match WorldDoc::from_manifest(&world) {
                Ok(doc) => {
                    client.doc = doc;
                    client.pending_world = true;
                }
                Err(e) => eprintln!("welcome world didn't load ({e})"),
            }
            log.push(ClientEntry::Status(format!(
                "joined '{}' — {} entities, rev {}, {} other guest(s)",
                session.name,
                count,
                revision,
                client.peers.len().saturating_sub(1)
            )));
        }
        ServerMsg::Snapshot { revision, world } => match WorldDoc::from_manifest(&world) {
            Ok(doc) => {
                client.doc = doc;
                client.revision = revision;
                client.pending_world = true;
            }
            Err(e) => eprintln!("snapshot didn't load ({e})"),
        },
        ServerMsg::Ops {
            revision,
            ops,
            author,
            ..
        } => {
            if revision != client.revision + 1 {
                let _ = client.outbound.send(ClientMsg::Resync);
                return;
            }
            if let Err(e) = client.doc.apply_all(&ops) {
                eprintln!("ops didn't apply locally ({e}) — resyncing");
                let _ = client.outbound.send(ClientMsg::Resync);
                return;
            }
            client.revision = revision;
            let _ = author;
            client.pending_ops.extend(ops);
        }
        ServerMsg::PeerJoined { peer } => {
            log.push(ClientEntry::Status(format!("{} joined", peer.name)));
            client.peers.insert(peer.id, peer);
        }
        ServerMsg::PeerLeft { peer_id } => {
            if let Some(peer) = client.peers.remove(&peer_id) {
                log.push(ClientEntry::Status(format!("{} left", peer.name)));
            }
        }
        ServerMsg::Presence { peer_id, presence } => {
            if let Some(peer) = client.peers.get_mut(&peer_id) {
                peer.presence = Some(presence);
            }
        }
        ServerMsg::Job { job, .. } => {
            match &job.state {
                sync::JobState::Queued { position } => log.push(ClientEntry::Status(format!(
                    "queued (#{position}): {}",
                    job.prompt
                ))),
                sync::JobState::Running => {
                    log.push(ClientEntry::Status(format!("building: {}", job.prompt)))
                }
                sync::JobState::Done => {
                    log.push(ClientEntry::Status(format!("done: {}", job.prompt)))
                }
                sync::JobState::Failed { reason } => {
                    log.push(ClientEntry::Error(format!("build failed: {reason}")))
                }
                sync::JobState::Rejected { reason } => {
                    log.push(ClientEntry::Error(format!("prompt rejected: {reason}")))
                }
            }
            // Scaffold markers ride the job lifecycle.
            match job.state {
                sync::JobState::Queued { .. } | sync::JobState::Running => {
                    if let Some(anchor) = job.anchor {
                        client
                            .scaffolds
                            .entry(job.job_id)
                            .or_insert(Entity::PLACEHOLDER);
                        let _ = anchor;
                    }
                }
                _ => {
                    client.scaffolds.remove(&job.job_id);
                }
            }
        }
        ServerMsg::Chat { from, text, .. } => log.push(ClientEntry::Chat {
            speaker: from.name,
            text,
        }),
        ServerMsg::Reject { reason, .. } => {
            log.push(ClientEntry::Error(format!("edit rejected: {reason}")))
        }
        ServerMsg::Error { reason } => log.push(ClientEntry::Error(reason)),
        ServerMsg::Pong { .. } => {}
    }
}

/// Apply a fresh world or streamed ops to the scene.
fn ops_apply(mut client: ResMut<OpsClient>, mut applier: OpsApplier) {
    if client.pending_world {
        client.pending_world = false;
        let entities: Vec<wt::WorldEntity> = client.doc.entities().cloned().collect();
        applier.rebuild_scene(&entities);
        if let Some(env) = client.doc.environment.clone() {
            applier.apply_ops(&[wt::EditOp::SetEnvironment { env }]);
        }
    }
    if client.pending_ops.is_empty() {
        return;
    }
    let ops = std::mem::take(&mut client.pending_ops);
    applier.apply_ops(&ops);
}

// ---------------------------------------------------------------------------
// Outbound: REPL prompts, presence, editing (later)
// ---------------------------------------------------------------------------

/// The gen3d primary camera's transform, for presence and prompt anchors.
fn primary_camera_transform(
    cameras: &Query<(&Transform, &GenEntity), With<Camera>>,
) -> Option<Transform> {
    cameras
        .iter()
        .find(|(_, g)| g.entity_type == GenEntityType::Camera)
        .map(|(t, _)| *t)
}

fn ops_prompts(
    mut client: ResMut<OpsClient>,
    mut log: ResMut<ClientPanelLog>,
    cameras: Query<(&Transform, &GenEntity), With<Camera>>,
) {
    let lines: Vec<String> = {
        let Ok(mut rx) = client.prompt_rx.lock() else {
            return;
        };
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    };
    for line in lines {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        if line == "/stats" {
            log.push(ClientEntry::Status(format!(
                "revision {} · {} entities · {} guest(s)",
                client.revision,
                client.doc.len(),
                client.peers.len()
            )));
            continue;
        }
        if line == "/quit" || line == "/exit" {
            std::process::exit(0);
        }
        if let Some(text) = line.strip_prefix("/chat ") {
            let _ = client.outbound.send(ClientMsg::Chat {
                text: text.to_string(),
            });
            continue;
        }
        // Anchor builds where the camera is looking, like the old client.
        let anchor = primary_camera_transform(&cameras).map(|t| {
            let forward = t.forward();
            let point = t.translation + forward * 12.0;
            [point.x, 0.0, point.z]
        });
        client.request_seq += 1;
        log.push(ClientEntry::You(line.clone()));
        let _ = client.outbound.send(ClientMsg::Prompt {
            request_id: format!("c{}", client.request_seq),
            text: line,
            anchor,
        });
    }
}

fn ops_presence(
    time: Res<Time>,
    mut client: ResMut<OpsClient>,
    cameras: Query<(&Transform, &GenEntity), With<Camera>>,
) {
    if !client.presence_timer.tick(time.delta()).just_finished() {
        return;
    }
    let Some(t) = primary_camera_transform(&cameras) else {
        return;
    };
    let look = t.translation + t.forward() * 12.0;
    let _ = client.outbound.send(ClientMsg::Presence(Presence {
        position: t.translation.to_array(),
        look_at: look.to_array(),
        selected: None,
    }));
}

// ---------------------------------------------------------------------------
// Peer avatars (same visuals as the host's guest_avatars)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn ops_avatars(
    mut commands: Commands,
    client: Res<OpsClient>,
    mut avatars: ResMut<crate::net::guest_avatars::GuestAvatars>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut targets: Query<&mut crate::net::guest_avatars::GuestAvatarTarget>,
    mut transforms: Query<&mut Transform>,
    time: Res<Time>,
) {
    crate::net::guest_avatars::sync_avatars(
        &mut commands,
        client
            .peers
            .values()
            .filter(|p| Some(p.id) != client.peer_id)
            .filter_map(|p| p.presence.clone().map(|pr| (p.id, p.name.clone(), pr))),
        &mut avatars.entities,
        &mut meshes,
        &mut materials,
        &mut targets,
        &mut transforms,
        &time,
    );
}
