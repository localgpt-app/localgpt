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

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
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

use super::host::HostJobs;
use crate::gen3d::audio::AudioEmitter;
use crate::gen3d::behaviors::EntityBehaviors;
use crate::gen3d::plugin::{SnapshotQueries, snapshot_entity};
use crate::gen3d::registry::{GenEntity, GenEntityType, GltfSource, NameRegistry, ParametricShape};

/// WebSocket endpoint path on the session HTTP server.
pub const SESSION_ROUTE: &str = "/session";

/// Queue `requester` key base for web peers, so they never collide with
/// native clients (whose requester key is a Bevy link's bits).
pub(crate) const WEB_REQUESTER_OFFSET: u64 = 1 << 40;

/// How often the live scene is projected into the document and diffed.
const PROJECTION_INTERVAL: Duration = Duration::from_millis(250);

/// Largest accepted WebSocket message (submit caps at 256 ops; the authority
/// rejects anything bigger at the document level anyway).
const MAX_WS_MESSAGE_BYTES: usize = 1024 * 1024;

// ---------------------------------------------------------------------------
// The bridge between the session-HTTP thread and Bevy
// ---------------------------------------------------------------------------

/// One frame to write to a connection's socket.
struct OutboundFrame {
    text: String,
    /// Close after sending (protocol errors).
    close: bool,
}

struct ConnTx {
    tx: mpsc::UnboundedSender<OutboundFrame>,
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
}

impl WebBridge {
    /// A bridge plus the receiver Bevy drains.
    pub fn new(token: Option<String>) -> (Self, mpsc::UnboundedReceiver<InboundEvent>) {
        let (inbound, rx) = mpsc::unbounded_channel();
        (
            Self {
                inbound,
                conns: Arc::new(StdMutex::new(HashMap::new())),
                next_conn: Arc::new(AtomicU64::new(0)),
                token,
            },
            rx,
        )
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
    /// A replayed op log is waiting to be spawned into the scene.
    scene_rebuild_pending: bool,
}

impl WebRoom {
    pub fn new(
        session_name: &str,
        bridge: WebBridge,
        inbound_rx: mpsc::UnboundedReceiver<InboundEvent>,
        workspace: &std::path::Path,
        resume: Option<&str>,
    ) -> Self {
        let mut room = Self {
            authority: Authority::new(session_name, Limits::default()),
            bridge,
            inbound_rx: StdMutex::new(inbound_rx),
            projection_timer: Timer::new(PROJECTION_INTERVAL, TimerMode::Repeating),
            warned_bad_projection: false,
            op_log: None,
            scene_rebuild_pending: false,
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
        room
    }

    /// Replay a previous session's op log into the authority.
    fn replay_log(&mut self, path: &std::path::Path) {
        let Ok(content) = std::fs::read_to_string(path) else {
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
                    if let Err(e) = self.authority.apply_replay(&entry.ops) {
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

/// Where a session's op log lives.
fn session_op_log_path(workspace: &std::path::Path, session_name: &str) -> std::path::PathBuf {
    workspace
        .join("sessions")
        .join(session_slug(session_name))
        .join("ops.jsonl")
}

/// Resolve `--resume`: a session name (its standard log path) or an explicit
/// path to an `ops.jsonl`.
pub(crate) fn resolve_op_log_path(workspace: &std::path::Path, resume: &str) -> std::path::PathBuf {
    if resume.ends_with(".jsonl") || resume.contains('/') {
        std::path::PathBuf::from(shellexpand::tilde(resume).as_ref())
    } else {
        session_op_log_path(workspace, resume)
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
        .route("/session-client.js", get(client_js))
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

async fn join_page() -> impl IntoResponse {
    Html(JOIN_PAGE_HTML)
}

async fn viewer_js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        localgpt_world_export::html::WORLD_VIEWER_JS,
    )
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
    bridge
        .conns
        .lock()
        .expect("conns lock poisoned")
        .insert(id, ConnTx { tx: tx.clone() });
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
                ops: ops.clone(),
                timestamp_ms: now_ms(),
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
pub(crate) fn web_drain_inbound(
    mut room: ResMut<WebRoom>,
    mut jobs: ResMut<HostJobs>,
    mut pending: ResMut<PendingSceneOps>,
) {
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
                // The authority already failed the peer's queued prompts;
                // drop them from the worker queue too.
                jobs.cancel_web_requester(WEB_REQUESTER_OFFSET + id);
            }
            InboundEvent::Message { id, msg } => {
                handle_client_msg(&mut room, &mut jobs, &mut pending, id, msg)
            }
        }
    }
}

fn handle_client_msg(
    room: &mut WebRoom,
    jobs: &mut HostJobs,
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
            // Invite links are bearer tokens. `==` on the hex strings is
            // fine on a LAN; the relay phase adds TLS.
            if let Some(expected) = &room.bridge.token
                && token.as_deref() != Some(expected.as_str())
            {
                deliver(room, error_to(id, "bad invite token"));
                return;
            }
            let out = room.authority.join(id, &name, Role::Guest, client);
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
            let (out, event) = room.authority.prompt(id, &request_id, &text, anchor);
            deliver(room, out);
            if let Some(AuthorityEvent::PromptAccepted {
                job_id,
                peer,
                text,
                anchor,
                ..
            }) = event
            {
                // The room's worker (the scoped remote agent) picks it up
                // through the same queue native prompts use.
                let result =
                    jobs.enqueue_web_prompt(job_id, WEB_REQUESTER_OFFSET + peer, &text, anchor);
                if let Err(e) = result {
                    let out = room.authority.job_finished(job_id, Some(e));
                    deliver(room, out);
                }
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
    jobs: Option<Res<HostJobs>>,
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
    };

    let ops = sync::diff_scene(room.authority.doc(), &projection, Some(&env));
    if ops.is_empty() {
        return;
    }
    // While the worker builds a guest's prompt, the ops are theirs:
    // attributed to them and undoable by them.
    let owner = jobs
        .as_deref()
        .and_then(HostJobs::running_web_requester)
        .filter(|p| room.authority.peer(*p).is_some());
    let result = match owner {
        Some(peer) => room.authority.record_local_ops_for_peer(peer, ops),
        None => room.authority.record_local_ops("host", ops),
    };
    match result {
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

/// Worker progress for web guests' jobs. Called from the native job-event
/// system, which owns the worker channel.
pub(crate) fn tee_job_event(room: &mut WebRoom, event: &super::host::JobEvent) {
    let out = match event {
        super::host::JobEvent::Started(job_id) => room.authority.job_started(*job_id),
        super::host::JobEvent::Finished { job_id, error } => {
            room.authority.job_finished(*job_id, error.clone())
        }
    };
    deliver(room, out);
}

// ---------------------------------------------------------------------------
// The join page
// ---------------------------------------------------------------------------

/// The page served at `/` while hosting with web enabled. Logic lives in
/// `/session-client.js`; three.js comes from the same CDN the HTML export
/// uses.
const JOIN_PAGE_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Join a LocalGPT world</title>
<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
html, body { width: 100%; height: 100%; overflow: hidden; background: #0b0e14; color: #e6e9ef;
  font: 14px/1.5 system-ui, sans-serif; }
#scene { width: 100%; height: 100%; }
#join-overlay { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center;
  background: radial-gradient(ellipse at center, #141a26 0%, #0b0e14 100%); z-index: 10; }
.card { background: #161c28; border: 1px solid #2a3347; border-radius: 12px; padding: 28px;
  width: 320px; box-shadow: 0 12px 40px rgba(0,0,0,0.5); }
.card h1 { font-size: 18px; margin-bottom: 4px; }
.card p { color: #8b95a9; margin-bottom: 16px; }
.card input { width: 100%; padding: 10px 12px; border-radius: 8px; border: 1px solid #2a3347;
  background: #0e1219; color: #e6e9ef; font-size: 15px; margin-bottom: 12px; }
.card button { width: 100%; padding: 10px; border-radius: 8px; border: none; background: #4f7cff;
  color: white; font-size: 15px; font-weight: 600; cursor: pointer; }
.card button:disabled { opacity: 0.5; }
#join-error { color: #ff7a7a; min-height: 18px; margin-top: 8px; }
#hud { position: absolute; top: 10px; left: 10px; background: rgba(0,0,0,0.5); padding: 8px 12px;
  border-radius: 8px; pointer-events: none; }
#hud-session { font-weight: 600; }
#hud-peers { color: #aab4c8; font-size: 12px; }
#status { position: absolute; top: 12px; left: 50%; transform: translateX(-50%);
  background: rgba(0,0,0,0.6); padding: 6px 14px; border-radius: 999px; display: none; }
#chat { position: absolute; left: 10px; bottom: 10px; width: 280px; display: flex;
  flex-direction: column; gap: 6px; }
#chat-log { max-height: 160px; overflow-y: auto; background: rgba(0,0,0,0.45); border-radius: 8px;
  padding: 8px; font-size: 13px; }
#chat-log:empty { display: none; }
.chat-line b { color: #8fb3ff; margin-right: 4px; }
.chat-agent b { color: #7be0a3; }
.chat-system b { color: #d9b45f; }
#chat input, #prompt-bar input { width: 100%; padding: 8px 12px; border-radius: 8px;
  border: 1px solid #2a3347; background: rgba(14,18,25,0.9); color: #e6e9ef; }
#prompt-bar { position: absolute; bottom: 10px; left: 50%; transform: translateX(-50%);
  width: min(520px, 60vw); }
</style>
<script type="importmap">
{
  "imports": {
    "three": "/vendor/three.module.js",
    "three/addons/": "/vendor/three/addons/"
  }
}
</script>
</head>
<body>
<div id="scene"></div>
<div id="hud"><div id="hud-session"></div><div id="hud-peers"></div></div>
<div id="status"></div>
<div id="chat"><div id="chat-log"></div><input id="chat-input" placeholder="Chat… (/undo undoes your last build)" autocomplete="off"></div>
<div id="prompt-bar"><input id="prompt-input" placeholder="Ask the AI to build something…" autocomplete="off"></div>
<div id="join-overlay">
  <div class="card">
    <h1>Join this world</h1>
    <p>A friend is hosting a LocalGPT session. Pick a name and step in.</p>
    <input id="name-input" placeholder="Your name" maxlength="32" autocomplete="off">
    <button id="join-btn">Join</button>
    <div id="join-error"></div>
  </div>
</div>
<script type="module">
import { startSessionClient } from '/session-client.js';
startSessionClient();
</script>
</body>
</html>
"#;

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
        let (bridge, mut rx) = WebBridge::new(Some("tok".into()));
        let (tx, _frame_rx) = mpsc::unbounded_channel::<OutboundFrame>();
        let id = 7;
        bridge.conns.lock().unwrap().insert(id, ConnTx { tx });
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
}
