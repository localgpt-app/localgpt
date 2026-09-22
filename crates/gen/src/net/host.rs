//! Listen-server host: authoritative ECS + rendering client in one process
//! (§1 of the collaborative world engine spec), plus the §2 mechanisms that
//! scale it past a handful of LAN peers.
//!
//! The host is the existing single-player gen app plus these additions:
//!
//! 1. A lightyear UDP server (netcode-authenticated) that replicates the
//!    world-model components from [`super::protocol`] to connected clients.
//!    Entities are attached to replication as they spawn and re-synced
//!    whenever their live components change — the existing `GenCommand`
//!    handlers are untouched.
//! 2. An mDNS announcer so LAN clients can discover the session.
//! 3. **Spatial interest management** (§2 AoI): clients report their camera
//!    position and only receive entities in chunks around them, via
//!    lightyear's per-link visibility. Chunks outside their window are
//!    represented by replicated [`NetChunkSummary`] impostors (§2 HLOD).
//! 4. **Asynchronous inference queue** (§2): client prompts become jobs in a
//!    [`JobQueue`], each shown to everyone as a replicated [`NetScaffold`]
//!    until the agent finishes it. The agent loop is the (single) worker; it
//!    is handed one job at a time and reports start/finish back.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::camera::primitives::Aabb;
use bevy::prelude::*;
use lightyear::connection::client_of::ClientOf;
use lightyear::connection::server::Start;
use lightyear::netcode::NetcodeServer;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use localgpt_world_types as wt;
use localgpt_world_types::ChunkCoord;
use tokio::sync::mpsc;

use super::assets::{AssetStore, MAX_BLOB_BYTES, asset_router, encode_mesh, spawn_session_http};
use super::interest::{
    ChunkSummaryBuilder, Relevance, ViewWindow, VisibilityCache, VisibilityChange, summaries_differ,
};
use super::jobs::{JobId, JobQueue, JobState};
use super::mdns::SessionAnnouncer;
use super::pairing::{PairingHost, format_pin, generate_pin, generate_private_key, pairing_router};
use super::protocol::{
    ChatChannel, ClientPrompt, ClientView, HostChat, JobStatus, NetChunkSummary, NetKind,
    NetMeshAsset, NetProtocolPlugin, NetScaffold, NetTransform, NetWorldMeta, apply_net_components,
    transforms_differ,
};
use crate::gen3d::audio::AudioEmitter;
use crate::gen3d::behaviors::EntityBehaviors;
use crate::gen3d::plugin::{CurrentWorld, SnapshotQueries, snapshot_entity};
use crate::gen3d::registry::{GenEntity, GenEntityType, GltfSource, NameRegistry, ParametricShape};

/// How often replicated component deltas are sampled and sent.
const REPLICATION_SEND_INTERVAL: Duration = Duration::from_millis(50);

/// Maximum simultaneous clients for a LAN session.
const MAX_CLIENTS: usize = 8;

/// How often chunk summaries (HLOD impostors) are recomputed.
const SUMMARY_INTERVAL_SECS: f32 = 1.0;

/// A client prompt handed to the agent loop as a job.
#[derive(Debug, Clone)]
pub struct RemoteJob {
    pub job_id: JobId,
    /// Full text for the agent (prompt + spatial context).
    pub agent_prompt: String,
    /// What the user typed (for console/chat echo).
    pub display: String,
}

/// Worker → host progress reports for a [`RemoteJob`].
#[derive(Debug, Clone)]
pub enum JobEvent {
    Started(JobId),
    Finished {
        job_id: JobId,
        error: Option<String>,
    },
}

/// Host-side options, built by the CLI when `--host` is given.
pub struct NetHostOptions {
    /// Session name shown in mDNS discovery and to joining clients.
    pub session_name: String,
    /// UDP port to listen on.
    pub port: u16,
    /// Sender half of the job channel into the agent loop.
    pub job_tx: mpsc::UnboundedSender<RemoteJob>,
    /// Receiver half of the chat channel out of the agent loop.
    pub chat_rx: mpsc::UnboundedReceiver<HostChat>,
    /// Receiver half of worker progress events.
    pub job_events_rx: mpsc::UnboundedReceiver<JobEvent>,
    /// Skip PIN pairing and use the public open-session key (trusted LAN /
    /// development only).
    pub open: bool,
}

/// What joiners need to know about the running session, for in-window
/// display (the desktop prompt panel). The PIN is read live because it
/// rotates after too many wrong guesses.
#[derive(Resource, Clone)]
pub struct HostSessionInfo {
    pub session_name: String,
    pub port: u16,
    pairing: Option<Arc<PairingHost>>,
}

impl HostSessionInfo {
    /// The current session PIN, formatted like the console shows it, or
    /// `None` for an open session.
    pub fn pin(&self) -> Option<String> {
        self.pairing
            .as_ref()
            .map(|pairing| format_pin(&pairing.pin()))
    }

    /// One line for a status display.
    pub fn summary(&self) -> String {
        match self.pin() {
            Some(pin) => format!("Hosting '{}' · PIN {pin}", self.session_name),
            None => format!("Hosting '{}' · open session (no PIN)", self.session_name),
        }
    }
}

/// Agent-loop side of the job/chat bridge.
pub struct AgentNetHooks {
    /// Jobs dispatched from the host's prompt queue (one at a time).
    pub job_rx: mpsc::UnboundedReceiver<RemoteJob>,
    /// Outbound chat events shown to connected clients.
    pub chat_tx: mpsc::UnboundedSender<HostChat>,
    /// Progress reports for dispatched jobs.
    pub job_events_tx: mpsc::UnboundedSender<JobEvent>,
    /// Run remote prompts on the host's own agent with all of its tools
    /// (`--remote-tools full`). Default: a scene-only agent
    /// (see [`super::remote_scope`]).
    pub remote_full_access: bool,
}

/// Create a matched (options, hooks) pair for the host.
pub fn create_host_channels() -> (NetHostOptions, AgentNetHooks) {
    let (job_tx, job_rx) = mpsc::unbounded_channel();
    let (chat_tx, chat_rx) = mpsc::unbounded_channel();
    let (job_events_tx, job_events_rx) = mpsc::unbounded_channel();
    (
        NetHostOptions {
            session_name: String::new(),
            port: super::DEFAULT_PORT,
            job_tx,
            chat_rx,
            job_events_rx,
            open: false,
        },
        AgentNetHooks {
            job_rx,
            chat_tx,
            job_events_tx,
            remote_full_access: false,
        },
    )
}

/// Resource carrying host session configuration.
#[derive(Resource)]
struct NetHostState {
    port: u16,
    /// Netcode private key (random per session unless `--open`).
    private_key: [u8; 32],
}

/// Resource: the §2 prompt queue plus the channels to/from its worker.
#[derive(Resource)]
struct HostJobs {
    queue: JobQueue,
    /// Replicated scaffold entity per job.
    scaffolds: HashMap<JobId, Entity>,
    /// Link entity of each job's requester (for targeted status messages).
    requester_links: HashMap<JobId, Entity>,
    /// A job has been handed to the worker and not yet finished.
    dispatched: Option<JobId>,
    job_tx: mpsc::UnboundedSender<RemoteJob>,
    events_rx: Mutex<mpsc::UnboundedReceiver<JobEvent>>,
}

/// Resource: chat events published by the agent loop, drained and broadcast
/// each frame.
#[derive(Resource)]
struct HostChatOutbox {
    rx: Mutex<mpsc::UnboundedReceiver<HostChat>>,
}

/// Resource owning the mDNS announcer (unregisters on drop).
#[derive(Resource)]
struct HostAnnouncer {
    _announcer: SessionAnnouncer,
}

/// Resource: each client link's area of interest and visibility cache.
#[derive(Resource, Default)]
struct InterestState {
    views: HashMap<Entity, (ViewWindow, [f32; 3])>,
    cache: VisibilityCache<Entity, Entity>,
}

/// Resource: replicated HLOD impostor entity per chunk.
#[derive(Resource, Default)]
struct ChunkSummaryEntities {
    by_chunk: HashMap<ChunkCoord, Entity>,
}

/// Resource: the content-addressed asset store and its server port.
#[derive(Resource)]
struct HostAssets {
    store: AssetStore,
    /// `None` when the asset server failed to start.
    port: Option<u16>,
}

/// Marker for replicated entities that bypass interest management (session
/// metadata, chunk summaries).
#[derive(Component)]
struct AlwaysRelevant;

/// The listen-server plugin. Requires the full gen app (it replicates
/// entities created by the gen systems).
pub struct NetHostPlugin {
    /// Consumed on build (contains non-clonable channel halves).
    pub options: std::sync::Mutex<Option<NetHostOptions>>,
}

impl Plugin for NetHostPlugin {
    fn build(&self, app: &mut App) {
        let NetHostOptions {
            session_name,
            port,
            job_tx,
            chat_rx,
            job_events_rx,
            open,
        } = self
            .options
            .lock()
            .expect("NetHostPlugin options lock poisoned")
            .take()
            .expect("NetHostPlugin options consumed twice");

        app.add_plugins(ServerPlugins {
            tick_duration: Duration::from_secs_f32(1.0 / 60.0),
        });
        app.add_plugins(NetProtocolPlugin);

        // Session secrets: a random netcode key that never leaves this
        // process, and a PIN joiners must know to be issued a connect token.
        let private_key = if open {
            super::OPEN_SESSION_KEY
        } else {
            generate_private_key()
        };
        let pairing = (!open).then(|| {
            let pin = generate_pin();
            Arc::new(PairingHost::new(
                private_key,
                super::PROTOCOL_ID,
                pin,
                |new_pin| {
                    eprintln!(
                        "\n[net] Too many wrong PINs — new session PIN: {}\n",
                        format_pin(new_pin)
                    );
                },
            ))
        });

        // Session HTTP (TCP, same port number as the UDP session): pairing
        // + content-addressed assets.
        let store = AssetStore::default();
        let http_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
        let router =
            asset_router(store.clone()).merge(pairing_router(pairing.clone(), super::PROTOCOL_ID));
        let asset_port = match spawn_session_http(router, http_addr) {
            Ok(()) => {
                eprintln!("Session HTTP (pairing + assets) on tcp://{http_addr}");
                Some(port)
            }
            Err(e) => {
                if open {
                    eprintln!(
                        "Session HTTP failed to start ({e}) — clients will show mesh placeholders"
                    );
                } else {
                    eprintln!(
                        "Session HTTP failed to start ({e}) — clients cannot pair; \
                         free TCP port {port} or pass --port"
                    );
                }
                None
            }
        };
        match &pairing {
            Some(pairing) => eprintln!(
                "\n  Session PIN: {}   (joiners: localgpt-gen --join <this-host> --pin <PIN>)\n",
                format_pin(&pairing.pin())
            ),
            None => eprintln!(
                "\n  OPEN session: no PIN — anyone on the LAN with localgpt-gen can join\n"
            ),
        }

        app.insert_resource(HostSessionInfo {
            session_name: session_name.clone(),
            port,
            pairing: pairing.clone(),
        })
        .insert_resource(ReplicationMetadata::new(REPLICATION_SEND_INTERVAL))
        .insert_resource(HostAssets {
            store,
            port: asset_port,
        })
        .insert_resource(NetHostState { port, private_key })
        .insert_resource(HostJobs {
            queue: JobQueue::default(),
            scaffolds: HashMap::new(),
            requester_links: HashMap::new(),
            dispatched: None,
            job_tx,
            events_rx: Mutex::new(job_events_rx),
        })
        .insert_resource(HostChatOutbox {
            rx: Mutex::new(chat_rx),
        })
        .init_resource::<InterestState>()
        .init_resource::<ChunkSummaryEntities>()
        .add_systems(Startup, start_listen_server)
        .add_systems(Startup, spawn_world_meta_entity.after(start_listen_server))
        .add_observer(on_client_link_connected)
        .add_systems(PreUpdate, net_attach_new_entities)
        .add_systems(
            PostUpdate,
            (
                net_sync_changes,
                net_sync_meta,
                net_update_chunk_summaries,
                net_update_interest
                    .after(TransformSystems::Propagate)
                    .after(net_update_chunk_summaries)
                    .before(ReplicationSystems::Send),
            ),
        )
        .add_systems(
            Update,
            (
                net_prompt_intake,
                net_publish_mesh_assets,
                net_job_events,
                net_job_dispatch
                    .after(net_prompt_intake)
                    .after(net_job_events),
                net_view_intake,
                net_chat_broadcast,
                net_client_lifecycle,
            ),
        );

        match SessionAnnouncer::start(&session_name, port, super::PROTOCOL_ID) {
            Ok(announcer) => {
                eprintln!(
                    "mDNS: session '{}' discoverable on the LAN ({}), port {port}",
                    session_name,
                    super::mdns::SERVICE_TYPE
                );
                app.insert_resource(HostAnnouncer {
                    _announcer: announcer,
                });
            }
            Err(e) => {
                eprintln!("mDNS announcement failed ({e}) — clients must connect by address");
            }
        }
    }
}

/// Marker on the dedicated singleton entity carrying [`NetWorldMeta`].
#[derive(Component)]
struct WorldMetaEntity;

/// Spawn the server link entity and start listening.
fn start_listen_server(mut commands: Commands, state: Res<NetHostState>) {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), state.port);

    let server_config = lightyear::netcode::server_plugin::NetcodeConfig {
        protocol_id: super::PROTOCOL_ID,
        private_key: state.private_key,
        max_clients: MAX_CLIENTS,
        // We bind 0.0.0.0 while clients connect via any of the host's LAN
        // addresses, so token-address validation would always mismatch.
        // Tokens are still bound to this session's private key, which only
        // the pairing endpoint uses.
        server_addr_check: false,
        ..default()
    };

    let server = commands
        .spawn((
            Name::new("World Host"),
            Server::new(None),
            NetcodeServer::new(server_config),
            LocalAddr(addr),
            ServerUdpIo::default(),
        ))
        .id();
    commands.trigger(Start { entity: server });
    // eprintln, not info!: the gen app runs with a warn-level tracing filter,
    // so info! lines would be invisible (same reasoning as the MCP relay).
    eprintln!("Collaborative session listening on {addr}");
}

/// Spawn the singleton entity that replicates session metadata.
fn spawn_world_meta_entity(mut commands: Commands) {
    commands.spawn((
        Name::new("World Meta"),
        WorldMetaEntity,
        AlwaysRelevant,
        NetWorldMeta {
            name: String::new(),
            environment: default_environment(),
            asset_port: None,
        },
        Replicate::to_clients(NetworkTarget::All),
    ));
}

fn default_environment() -> wt::EnvironmentDef {
    wt::EnvironmentDef {
        background_color: None,
        ambient_intensity: None,
        ambient_color: None,
        fog_density: None,
        fog_color: None,
    }
}

/// Enable replication for every newly spawned gen entity.
///
/// Cameras are skipped — each client renders with its own camera. Which
/// clients actually receive an entity is decided by [`net_update_interest`].
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn net_attach_new_entities(
    mut commands: Commands,
    new_entities: Query<(Entity, &GenEntity, &Name), (Added<GenEntity>, Without<Replicate>)>,
    transforms: Query<&'static Transform>,
    parametric_shapes: Query<&'static ParametricShape>,
    material_handles: Query<&'static MeshMaterial3d<StandardMaterial>>,
    materials: Res<Assets<StandardMaterial>>,
    visibility_query: Query<&'static Visibility>,
    directional_lights: Query<&'static DirectionalLight>,
    point_lights: Query<&'static PointLight>,
    spot_lights: Query<&'static SpotLight>,
    behaviors_query: Query<&'static EntityBehaviors>,
    audio_emitters: Query<&'static AudioEmitter>,
    parent_query: Query<&'static ChildOf>,
    gltf_sources: Query<&'static GltfSource>,
    registry: Res<NameRegistry>,
) {
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
        registry: &registry,
    };

    for (entity, gen_entity, name) in new_entities.iter() {
        let Some(kind) = net_kind(gen_entity.entity_type) else {
            continue;
        };
        let we = snapshot_entity(name.as_ref(), entity, gen_entity.world_id, &sq);
        commands
            .entity(entity)
            .insert(Replicate::to_clients(NetworkTarget::All));
        apply_net_components(&mut commands.entity(entity), &we, kind);
    }
}

fn net_kind(entity_type: GenEntityType) -> Option<NetKind> {
    match entity_type {
        GenEntityType::Camera => None,
        GenEntityType::Primitive => Some(NetKind::Primitive),
        GenEntityType::Light => Some(NetKind::Light),
        GenEntityType::Mesh => Some(NetKind::Mesh),
        GenEntityType::Group => Some(NetKind::Group),
        GenEntityType::AudioEmitter => Some(NetKind::AudioEmitter),
    }
}

/// Push live ECS state into the net components whenever it changes.
///
/// Two paths keep bandwidth sane:
/// - Transforms are epsilon-compared — only moved entities re-send.
/// - Structural changes (shape/material/light/behaviors/parent) trigger a
///   full re-snapshot of that entity.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn net_sync_changes(
    mut commands: Commands,
    moving: Query<(&Transform, &Visibility, &mut NetTransform), (With<Replicate>, With<GenEntity>)>,
    structural: Query<
        (Entity, &GenEntity, &Name),
        (
            With<Replicate>,
            Or<(
                Changed<ParametricShape>,
                Changed<MeshMaterial3d<StandardMaterial>>,
                Changed<EntityBehaviors>,
                Changed<DirectionalLight>,
                Changed<PointLight>,
                Changed<SpotLight>,
                Changed<ChildOf>,
            )>,
        ),
    >,
    transforms: Query<&'static Transform>,
    parametric_shapes: Query<&'static ParametricShape>,
    material_handles: Query<&'static MeshMaterial3d<StandardMaterial>>,
    materials: Res<Assets<StandardMaterial>>,
    visibility_query: Query<&'static Visibility>,
    directional_lights: Query<&'static DirectionalLight>,
    point_lights: Query<&'static PointLight>,
    spot_lights: Query<&'static SpotLight>,
    behaviors_query: Query<&'static EntityBehaviors>,
    audio_emitters: Query<&'static AudioEmitter>,
    parent_query: Query<&'static ChildOf>,
    gltf_sources: Query<&'static GltfSource>,
    registry: Res<NameRegistry>,
) {
    // Fast path: transforms (behavior-animated entities move every frame).
    for (transform, visibility, mut net_transform) in moving {
        let euler = transform.rotation.to_euler(EulerRot::XYZ);
        let current = wt::WorldTransform {
            position: transform.translation.to_array(),
            rotation_degrees: [
                euler.0.to_degrees(),
                euler.1.to_degrees(),
                euler.2.to_degrees(),
            ],
            scale: transform.scale.to_array(),
            visible: *visibility != Visibility::Hidden,
        };
        if transforms_differ(&net_transform.0, &current, 1e-4) {
            net_transform.0 = current;
        }
    }

    if structural.is_empty() {
        return;
    }
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
        registry: &registry,
    };
    for (entity, gen_entity, name) in structural {
        let Some(kind) = net_kind(gen_entity.entity_type) else {
            continue;
        };
        let we = snapshot_entity(name.as_ref(), entity, gen_entity.world_id, &sq);
        apply_net_components(&mut commands.entity(entity), &we, kind);
    }
}

/// Keep the singleton [`NetWorldMeta`] in step with world resources.
fn net_sync_meta(
    current_world: Res<CurrentWorld>,
    assets: Res<HostAssets>,
    clear_color: Option<Res<ClearColor>>,
    ambient: Option<Res<GlobalAmbientLight>>,
    mut meta: Query<&mut NetWorldMeta, With<WorldMetaEntity>>,
) {
    let Ok(mut meta) = meta.single_mut() else {
        return;
    };
    let name = current_world.name.clone().unwrap_or_default();
    let mut environment = default_environment();
    if let Some(clear) = &clear_color {
        let c = clear.0.to_srgba();
        environment.background_color = Some([c.red, c.green, c.blue, c.alpha]);
    }
    if let Some(ambient) = &ambient {
        let c = ambient.color.to_srgba();
        environment.ambient_intensity = Some(ambient.brightness);
        environment.ambient_color = Some([c.red, c.green, c.blue, c.alpha]);
    }
    if meta.name != name || meta.environment != environment || meta.asset_port != assets.port {
        meta.name = name;
        meta.environment = environment;
        meta.asset_port = assets.port;
    }
}

/// Publish custom mesh geometry to the asset store and replicate its digest
/// (§2 asset streaming) — the geometry itself never rides replication.
#[allow(clippy::type_complexity)]
fn net_publish_mesh_assets(
    mut commands: Commands,
    assets: Res<HostAssets>,
    meshes: Res<Assets<Mesh>>,
    changed: Query<
        (Entity, &GenEntity, &Mesh3d, Option<&NetMeshAsset>),
        (With<Replicate>, Or<(Added<Replicate>, Changed<Mesh3d>)>),
    >,
) {
    if assets.port.is_none() {
        return;
    }
    for (entity, gen_entity, mesh3d, current) in &changed {
        if gen_entity.entity_type != GenEntityType::Mesh {
            continue;
        }
        let Some(blob) = meshes.get(&mesh3d.0).and_then(encode_mesh) else {
            continue;
        };
        if blob.len() > MAX_BLOB_BYTES {
            continue;
        }
        let bytes = blob.len() as u32;
        let digest = assets.store.publish(blob);
        if current.is_none_or(|c| c.digest != digest) {
            commands
                .entity(entity)
                .insert(NetMeshAsset { digest, bytes });
        }
    }
}

// ---------------------------------------------------------------------------
// §2 Spatial interest management
// ---------------------------------------------------------------------------

/// Record each client's reported camera position.
fn net_view_intake(
    mut links: Query<(Entity, &mut MessageReceiver<ClientView>), With<ClientOf>>,
    mut interest: ResMut<InterestState>,
) {
    for (link, mut receiver) in &mut links {
        // Sequenced channel: keep only the newest view.
        if let Some(view) = receiver.receive().last() {
            if !view.position.iter().all(|v| v.is_finite()) {
                continue;
            }
            interest.views.insert(
                link,
                (ViewWindow::at(view.position, view.radius), view.position),
            );
        }
    }
}

/// Decide, per client link, which replicated entities it should receive.
///
/// Entities are placed by their hierarchy *root's* world position so a
/// child's (parent-relative) transform always arrives together with its
/// parent. Directional lights, session metadata, and chunk summaries are
/// global. Only visibility transitions are sent to lightyear.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn net_update_interest(
    mut commands: Commands,
    links: Query<Entity, (With<ClientOf>, With<ReplicationSender>)>,
    replicated: Query<
        (Entity, &GlobalTransform, Has<DirectionalLight>),
        (With<Replicate>, Without<AlwaysRelevant>),
    >,
    parents: Query<&ChildOf>,
    globals: Query<&GlobalTransform>,
    mut interest: ResMut<InterestState>,
    mut prune_timer: Local<f32>,
    time: Res<Time>,
) {
    let link_set: HashSet<Entity> = links.iter().collect();
    if link_set.is_empty() {
        return;
    }

    let InterestState { views, cache } = &mut *interest;
    for (entity, global, is_sun) in &replicated {
        let relevance = if is_sun {
            Relevance::Global
        } else {
            let root = parents.root_ancestor(entity);
            let position = globals
                .get(root)
                .map(|g| g.translation())
                .unwrap_or_else(|_| global.translation());
            Relevance::at(position.to_array())
        };
        for &link in &link_set {
            let view = views.get(&link).map(|(v, _)| *v).unwrap_or_default();
            match cache.update(link, entity, relevance.visible_in(&view)) {
                Some(VisibilityChange::Gain { link, entity }) => {
                    commands.gain_visibility(entity, link);
                }
                Some(VisibilityChange::Lose { link, entity }) => {
                    commands.lose_visibility(entity, link);
                }
                None => {}
            }
        }
    }

    // Prune state for departed links / despawned entities now and then.
    *prune_timer += time.delta_secs();
    if *prune_timer > 5.0 {
        *prune_timer = 0.0;
        let entities: HashSet<Entity> = replicated.iter().map(|(e, _, _)| e).collect();
        cache.retain(&link_set, &entities);
        views.retain(|link, _| link_set.contains(link));
    }
}

/// Recompute per-chunk HLOD summaries and replicate them to every client.
#[allow(clippy::type_complexity)]
fn net_update_chunk_summaries(
    mut commands: Commands,
    visuals: Query<
        (
            &GlobalTransform,
            &Aabb,
            Option<&MeshMaterial3d<StandardMaterial>>,
            &InheritedVisibility,
        ),
        (With<GenEntity>, With<Mesh3d>),
    >,
    materials: Res<Assets<StandardMaterial>>,
    mut summaries: ResMut<ChunkSummaryEntities>,
    mut existing: Query<&mut NetChunkSummary>,
    mut timer: Local<f32>,
    time: Res<Time>,
) {
    *timer += time.delta_secs();
    if *timer < SUMMARY_INTERVAL_SECS {
        return;
    }
    *timer = 0.0;

    let mut builder = ChunkSummaryBuilder::new();
    for (global, aabb, material, visibility) in &visuals {
        if !visibility.get() {
            continue;
        }
        let (min, max) = world_aabb(global, aabb);
        let color = material
            .and_then(|m| materials.get(&m.0))
            .map(|m| m.base_color.to_srgba().to_f32_array())
            .unwrap_or([0.7, 0.7, 0.7, 1.0]);
        builder.add(min.to_array(), max.to_array(), color);
    }
    let fresh = builder.build();

    // Despawn summaries for chunks that emptied out.
    summaries.by_chunk.retain(|chunk, entity| {
        let keep = fresh.contains_key(chunk);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
    for (chunk, summary) in fresh {
        match summaries.by_chunk.get(&chunk) {
            Some(&entity) => {
                if let Ok(mut current) = existing.get_mut(entity)
                    && summaries_differ(&current.0, &summary)
                {
                    current.0 = summary;
                }
            }
            None => {
                let entity = commands
                    .spawn((
                        Name::new(format!("Chunk Summary {chunk}")),
                        AlwaysRelevant,
                        NetChunkSummary(summary),
                        Replicate::to_clients(NetworkTarget::All),
                    ))
                    .id();
                summaries.by_chunk.insert(chunk, entity);
            }
        }
    }
}

/// World-space AABB of a local AABB under a global transform.
fn world_aabb(global: &GlobalTransform, aabb: &Aabb) -> (Vec3, Vec3) {
    let affine = global.affine();
    let center = affine.transform_point3(Vec3::from(aabb.center));
    let m = affine.matrix3;
    let he = Vec3::from(aabb.half_extents);
    let extent = Vec3::new(
        m.x_axis.x.abs() * he.x + m.y_axis.x.abs() * he.y + m.z_axis.x.abs() * he.z,
        m.x_axis.y.abs() * he.x + m.y_axis.y.abs() * he.y + m.z_axis.y.abs() * he.z,
        m.x_axis.z.abs() * he.x + m.y_axis.z.abs() * he.y + m.z_axis.z.abs() * he.z,
    );
    (center - extent, center + extent)
}

// ---------------------------------------------------------------------------
// Links, prompts, and the §2 inference queue
// ---------------------------------------------------------------------------

/// Insert replication + message components on each client link entity.
///
/// Netcode spawns a `ClientOf` entity per accepted connection; replication
/// itself only starts once the link is also `Connected`, which leaves
/// [`net_update_interest`] a few frames to hide out-of-view entities before
/// the first snapshot goes out.
fn on_client_link_connected(trigger: On<Add, ClientOf>, mut commands: Commands) {
    commands.entity(trigger.entity).insert((
        ReplicationSender,
        Name::new("Client Link"),
        MessageReceiver::<ClientPrompt>::default(),
        MessageReceiver::<ClientView>::default(),
        MessageSender::<JobStatus>::default(),
    ));
}

/// Turn client prompts into queued jobs with replicated scaffolds.
fn net_prompt_intake(
    mut commands: Commands,
    mut links: Query<(Entity, &mut MessageReceiver<ClientPrompt>), With<ClientOf>>,
    mut status_senders: Query<&mut MessageSender<JobStatus>>,
    server: Query<&Server>,
    mut sender: ServerMultiMessageSender,
    mut jobs: ResMut<HostJobs>,
    interest: Res<InterestState>,
) {
    let Some(server) = server.iter().next() else {
        return;
    };
    for (link, mut receiver) in &mut links {
        for prompt in receiver.receive() {
            let requester = link.to_bits();
            let result =
                jobs.queue
                    .enqueue(requester, prompt.request_id, &prompt.text, prompt.anchor);
            let (job, position) = match result {
                Ok(accepted) => accepted,
                Err(e) => {
                    if let Ok(mut status) = status_senders.get_mut(link) {
                        status.send::<ChatChannel>(JobStatus {
                            request_id: prompt.request_id,
                            job_id: 0,
                            state: JobState::Rejected {
                                reason: e.to_string(),
                            },
                        });
                    }
                    continue;
                }
            };
            eprintln!(
                "[net] Prompt queued as job #{} (position {position}): {}",
                job.id, job.prompt
            );

            // Scaffold at the anchor, else at the requester's camera, else
            // the origin.
            let at = job
                .anchor
                .or_else(|| interest.views.get(&link).map(|(_, p)| *p))
                .unwrap_or([0.0; 3]);
            let scaffold = commands
                .spawn((
                    Name::new(format!("Scaffold #{}", job.id)),
                    Transform::from_translation(Vec3::from_array(at)),
                    NetTransform(wt::WorldTransform {
                        position: at,
                        ..default()
                    }),
                    NetScaffold {
                        job_id: job.id,
                        request_id: job.request_id,
                        prompt: job.prompt.clone(),
                        running: false,
                    },
                    Replicate::to_clients(NetworkTarget::All),
                ))
                .id();
            jobs.scaffolds.insert(job.id, scaffold);
            jobs.requester_links.insert(job.id, link);

            if let Ok(mut status) = status_senders.get_mut(link) {
                status.send::<ChatChannel>(JobStatus {
                    request_id: job.request_id,
                    job_id: job.id,
                    state: JobState::Queued { position },
                });
            }
            let _ = sender.send::<HostChat, ChatChannel>(
                &HostChat {
                    speaker: "client".to_string(),
                    text: job.prompt,
                },
                server,
                &NetworkTarget::All,
            );
        }
    }
}

/// Hand the next queued job to the worker once it is idle.
fn net_job_dispatch(
    mut commands: Commands,
    mut jobs: ResMut<HostJobs>,
    mut status_senders: Query<&mut MessageSender<JobStatus>>,
) {
    if jobs.dispatched.is_some() {
        return;
    }
    let Some(job) = jobs.queue.start_next() else {
        return;
    };
    let remote = RemoteJob {
        job_id: job.id,
        agent_prompt: job.agent_prompt(),
        display: job.prompt.clone(),
    };
    if jobs.job_tx.send(remote).is_err() {
        // Worker gone (agent loop exited) — drop the job and its scaffold.
        jobs.queue.finish(job.id);
        jobs.requester_links.remove(&job.id);
        if let Some(scaffold) = jobs.scaffolds.remove(&job.id) {
            commands.entity(scaffold).despawn();
        }
        return;
    }
    jobs.dispatched = Some(job.id);

    // Everyone still waiting moved up one place.
    let positions = jobs.queue.positions();
    for (job_id, position) in positions {
        let Some(&link) = jobs.requester_links.get(&job_id) else {
            continue;
        };
        let Some(request_id) = jobs.queue.get(job_id).map(|j| j.request_id) else {
            continue;
        };
        if let Ok(mut status) = status_senders.get_mut(link) {
            status.send::<ChatChannel>(JobStatus {
                request_id,
                job_id,
                state: JobState::Queued { position },
            });
        }
    }
}

/// Apply worker progress: mark scaffolds running, retire finished jobs.
fn net_job_events(
    mut commands: Commands,
    mut jobs: ResMut<HostJobs>,
    mut scaffolds: Query<&mut NetScaffold>,
    mut status_senders: Query<&mut MessageSender<JobStatus>>,
) {
    let events: Vec<JobEvent> = {
        let Ok(mut rx) = jobs.events_rx.lock() else {
            return;
        };
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    };
    for event in events {
        let (job_id, state) = match event {
            JobEvent::Started(job_id) => {
                if let Some(mut scaffold) = jobs
                    .scaffolds
                    .get(&job_id)
                    .and_then(|e| scaffolds.get_mut(*e).ok())
                {
                    scaffold.running = true;
                }
                (job_id, JobState::Running)
            }
            JobEvent::Finished { job_id, error } => {
                let request = jobs.queue.finish(job_id);
                if jobs.dispatched == Some(job_id) {
                    jobs.dispatched = None;
                }
                if let Some(entity) = jobs.scaffolds.remove(&job_id) {
                    commands.entity(entity).despawn();
                }
                let state = match error {
                    None => JobState::Done,
                    Some(reason) => JobState::Failed { reason },
                };
                let link = jobs.requester_links.remove(&job_id);
                if let (Some(request), Some(link)) = (request, link)
                    && let Ok(mut status) = status_senders.get_mut(link)
                {
                    status.send::<ChatChannel>(JobStatus {
                        request_id: request.request_id,
                        job_id,
                        state,
                    });
                }
                continue;
            }
        };
        let link = jobs.requester_links.get(&job_id).copied();
        let request_id = jobs.queue.get(job_id).map(|j| j.request_id);
        if let (Some(link), Some(request_id)) = (link, request_id)
            && let Ok(mut status) = status_senders.get_mut(link)
        {
            status.send::<ChatChannel>(JobStatus {
                request_id,
                job_id,
                state,
            });
        }
    }
}

/// Broadcast agent-loop chat events to every client.
fn net_chat_broadcast(
    outbox: Res<HostChatOutbox>,
    server: Query<&Server>,
    mut sender: ServerMultiMessageSender,
) {
    let Some(server) = server.iter().next() else {
        return;
    };
    let Ok(mut rx) = outbox.rx.lock() else {
        return;
    };
    while let Ok(chat) = rx.try_recv() {
        let _ = sender.send::<HostChat, ChatChannel>(&chat, server, &NetworkTarget::All);
    }
}

/// Announce client joins/leaves; drop a departed client's pending jobs.
fn net_client_lifecycle(
    mut commands: Commands,
    links: Query<Entity, With<ClientOf>>,
    server: Query<&Server>,
    mut sender: ServerMultiMessageSender,
    mut jobs: ResMut<HostJobs>,
    mut seen: Local<HashSet<Entity>>,
) {
    let Some(server) = server.iter().next() else {
        return;
    };
    for entity in &links {
        if seen.insert(entity) {
            eprintln!("Client connected");
            let _ = sender.send::<HostChat, ChatChannel>(
                &HostChat {
                    speaker: "host".to_string(),
                    text: "client joined the session".to_string(),
                },
                server,
                &NetworkTarget::All,
            );
        }
    }
    let current: HashSet<Entity> = links.iter().collect();
    let left: Vec<Entity> = seen
        .iter()
        .copied()
        .filter(|e| !current.contains(e))
        .collect();
    for entity in left {
        seen.remove(&entity);
        let cancelled = jobs.queue.cancel_requester(entity.to_bits());
        for job in &cancelled {
            jobs.requester_links.remove(&job.id);
            if let Some(scaffold) = jobs.scaffolds.remove(&job.id) {
                commands.entity(scaffold).despawn();
            }
        }
        if cancelled.is_empty() {
            eprintln!("Client disconnected");
        } else {
            eprintln!(
                "Client disconnected ({} queued prompt(s) cancelled)",
                cancelled.len()
            );
        }
        let _ = sender.send::<HostChat, ChatChannel>(
            &HostChat {
                speaker: "host".to_string(),
                text: "client left the session".to_string(),
            },
            server,
            &NetworkTarget::All,
        );
    }
}
