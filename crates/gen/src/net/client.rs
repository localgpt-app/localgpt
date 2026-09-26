//! Collaborative session client (§1): a read-only viewer that renders the
//! host's replicated world and forwards natural-language prompts to the
//! host's agent.
//!
//! The client is a slim Bevy app — no gen command processing, no local
//! agent, no inspector. Replicated world-model components are composed back
//! into meshes/materials/lights through the same conversion helpers the
//! world-load path uses, so host and client render the same scene.
//!
//! §2 behaviour on the client side:
//! - **View reports** — the camera position is sent to the host a few times
//!   per second so it can scope replication to nearby chunks (AoI).
//! - **Scaffolds** — sending a prompt immediately spawns a translucent local
//!   scaffold where the camera is looking; it hands over to the host's
//!   replicated scaffold, which disappears once the agent has built the
//!   real geometry.
//! - **HLOD + baking** — see [`super::client_lod`].
//! - **Asset streaming** — custom mesh geometry arrives as a digest; the
//!   blob is fetched from the host's asset server on demand and cached on
//!   disk by digest (see [`super::assets`]).

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;

use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::prelude::*;
use lightyear::connection::client::Connected;
use lightyear::netcode::auth::Authentication;
use lightyear::netcode::{ConnectToken, NetcodeClient};
use lightyear::prelude::client::*;
use lightyear::prelude::*;
use tokio::sync::mpsc;

use super::assets::{decode_mesh, default_cache_dir, fetch_blob, read_cached, write_cached};
use super::client_lod::{BakeState, ClientLodPlugin, Impostor, visible_impostors};
use super::interest::ViewWindow;
use super::jobs::JobState;
use super::protocol::{
    ClientPrompt, ClientView, HostChat, JobStatus, NetEntityKind, NetKind, NetLight, NetMaterial,
    NetMeshAsset, NetName, NetParentId, NetProtocolPlugin, NetScaffold, NetShape, NetTransform,
    NetWorldId, NetWorldMeta, PromptChannel, ViewChannel,
};
use crate::gen3d::plugin::{insert_light_component, material_def_to_standard, shape_to_mesh};

/// Client options, built by the CLI when `--join` is given.
pub struct NetClientOptions {
    /// Host address to connect to.
    pub server_addr: SocketAddr,
    /// Prompts typed into the local REPL, drained and sent each frame.
    pub prompt_rx: mpsc::UnboundedReceiver<String>,
    /// Requested area-of-interest radius in chunks (§2 AoI).
    pub view_radius: u8,
    /// Merge static primitives into per-material meshes (§2 mesh baking).
    pub bake: bool,
    /// Connect token issued by the host's pairing endpoint (serialized).
    /// `None` joins an `--open` session with the public key.
    pub connect_token: Option<Vec<u8>>,
}

/// How often the camera position is reported to the host.
const VIEW_REPORT_SECS: f32 = 0.25;

/// Resource carrying the prompt channel from the REPL thread.
#[derive(Resource)]
struct PromptOutbox {
    rx: Mutex<mpsc::UnboundedReceiver<String>>,
}

/// One line of the in-window client panel's log (desktop join mode):
/// prompts, host chat, and queue progress, mirroring the console output.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientEntry {
    You(String),
    Chat { speaker: String, text: String },
    Status(String),
    Error(String),
}

/// Log shared between the net systems (which append) and the in-window
/// client panel (which displays it when there's no terminal). Bounded; the
/// console still prints every line.
#[derive(Resource, Default)]
pub struct ClientPanelLog {
    pub entries: Vec<ClientEntry>,
}

impl ClientPanelLog {
    pub fn push(&mut self, entry: ClientEntry) {
        self.entries.push(entry);
        let excess = self.entries.len().saturating_sub(400);
        self.entries.drain(..excess);
    }
}

/// Marker on client entities that have local visuals built.
#[derive(Component)]
pub(crate) struct NetVisual;

/// Streamed mesh assets: in-memory handles, pending entities, fetch results.
#[derive(Resource)]
struct ClientAssets {
    /// `http://host:port`, known once session metadata arrives.
    base_url: Option<String>,
    cache_dir: std::path::PathBuf,
    loaded: std::collections::HashMap<String, Handle<Mesh>>,
    waiting: std::collections::HashMap<String, Vec<Entity>>,
    in_flight: std::collections::HashSet<String>,
    failed: std::collections::HashSet<String>,
    downloaded: usize,
    tx: std::sync::mpsc::Sender<AssetFetch>,
    rx: Mutex<std::sync::mpsc::Receiver<AssetFetch>>,
}

/// Result of a disk-cache read or network fetch.
struct AssetFetch {
    digest: String,
    result: Result<Vec<u8>, String>,
    /// Came from the network (so it should be written to the disk cache).
    from_network: bool,
}

impl Default for ClientAssets {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            base_url: None,
            cache_dir: default_cache_dir(),
            loaded: default(),
            waiting: default(),
            in_flight: default(),
            failed: default(),
            downloaded: 0,
            tx,
            rx: Mutex::new(rx),
        }
    }
}

/// The client's own area of interest (mirrors what it reports to the host).
#[derive(Resource, Clone, Copy)]
pub(crate) struct ClientViewState {
    pub radius: u8,
    pub window: ViewWindow,
}

/// Resource mapping stable world ids → local entities (for parent resolution).
#[derive(Resource, Default)]
struct ClientEntityMap {
    id_to_entity: std::collections::HashMap<u64, Entity>,
    /// Children waiting for their parent entity to arrive.
    pending_parents: Vec<(Entity, u64)>,
}

/// Progress counter for the streamed world (console feedback).
#[derive(Resource, Default)]
struct ClientStreamStats {
    spawned: usize,
    announced: usize,
}

/// The client plugin. Renders replicated state and sends prompts.
pub struct NetClientPlugin {
    /// Consumed on build (contains non-clonable channel halves).
    pub options: std::sync::Mutex<Option<NetClientOptions>>,
}

impl Plugin for NetClientPlugin {
    fn build(&self, app: &mut App) {
        let NetClientOptions {
            server_addr,
            prompt_rx,
            view_radius,
            bake,
            connect_token,
        } = self
            .options
            .lock()
            .expect("NetClientPlugin options lock poisoned")
            .take()
            .expect("NetClientPlugin options consumed twice");

        app.add_plugins(ClientPlugins {
            tick_duration: std::time::Duration::from_secs_f32(1.0 / 60.0),
        });
        app.add_plugins(NetProtocolPlugin);
        app.add_plugins(ClientLodPlugin { bake });

        app.insert_resource(ClientSession {
            server_addr,
            connect_token,
        })
        .insert_resource(ClientViewState {
            radius: view_radius,
            window: ViewWindow {
                radius: view_radius,
                ..default()
            },
        })
        .insert_resource(PromptOutbox {
            rx: Mutex::new(prompt_rx),
        })
        .init_resource::<ClientEntityMap>()
        .init_resource::<LocalCommands>()
        .init_resource::<ClientAssets>()
        .init_resource::<ClientStreamStats>()
        .init_resource::<ClientPanelLog>()
        .add_systems(Startup, (connect_to_host, spawn_client_camera))
        .add_observer(on_connected)
        .add_observer(on_replicated_despawn)
        .add_systems(Update, client_apply_transforms)
        .add_systems(Update, client_spawn_visuals)
        .add_systems(Update, client_update_visuals)
        .add_systems(Update, client_resolve_parents)
        .add_systems(Update, client_apply_meta)
        .add_systems(Update, (client_send_prompts, client_receive_chat))
        .add_systems(
            Update,
            (
                client_report_view,
                client_local_commands.after(client_send_prompts),
                client_request_mesh_assets.after(client_spawn_visuals),
                client_receive_mesh_assets.after(client_request_mesh_assets),
                client_receive_job_status,
                client_spawn_scaffolds,
                client_animate_scaffolds,
            ),
        )
        .add_systems(Update, (client_fly_move, client_fly_look, client_fly_speed));
    }
}

/// Resource carrying the resolved host address.
#[derive(Resource)]
struct ClientSession {
    server_addr: SocketAddr,
    connect_token: Option<Vec<u8>>,
}

/// Spawn the client link entity and initiate the connection.
fn connect_to_host(mut commands: Commands, session: Res<ClientSession>) {
    let auth = match &session.connect_token {
        Some(bytes) => match ConnectToken::try_from_bytes(bytes) {
            Ok(token) => Authentication::Token(token),
            Err(e) => {
                error!("Invalid connect token from pairing: {e:?}");
                return;
            }
        },
        None => Authentication::Manual {
            server_addr: session.server_addr,
            client_id: rand::random::<u64>(),
            private_key: super::OPEN_SESSION_KEY,
            protocol_id: super::PROTOCOL_ID,
        },
    };
    let netcode = match NetcodeClient::new(
        auth,
        lightyear::netcode::client_plugin::NetcodeConfig {
            token_expire_secs: -1,
            ..default()
        },
    ) {
        Ok(netcode) => netcode,
        Err(e) => {
            error!("Failed to build netcode client: {e}");
            return;
        }
    };

    let client = commands
        .spawn((
            Name::new("World Client"),
            Client,
            ReplicationReceiver,
            Link::default(),
            LocalAddr(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)),
            PeerAddr(session.server_addr),
            UdpIo::default(),
            netcode,
            MessageSender::<ClientPrompt>::default(),
            MessageSender::<ClientView>::default(),
            MessageReceiver::<HostChat>::default(),
            MessageReceiver::<JobStatus>::default(),
        ))
        .id();
    commands.trigger(Connect { entity: client });
    info!("Connecting to {} …", session.server_addr);
}

fn on_connected(_trigger: On<Add, Connected>, mut log: ResMut<ClientPanelLog>) {
    println!("\n[net] Connected to host — the world will stream in shortly.");
    println!("[net] Type a prompt and press Enter to ask the host's agent.\n");
    log.push(ClientEntry::Status(
        "Connected — the world will stream in shortly.".into(),
    ));
}

// ---------------------------------------------------------------------------
// Camera — a compact free-fly camera (WASD + Q/E, right-drag look, wheel speed)
// ---------------------------------------------------------------------------

#[derive(Component)]
pub(crate) struct ClientFlyCam {
    speed: f32,
}

fn spawn_client_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(6.0, 5.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        ClientFlyCam { speed: 8.0 },
    ));
}

fn client_fly_move(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut query: Query<(&ClientFlyCam, &mut Transform), With<ClientFlyCam>>,
) {
    for (cam, mut transform) in &mut query {
        let mut direction = Vec3::ZERO;
        if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
            direction += *transform.forward();
        }
        if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
            direction += *transform.back();
        }
        if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
            direction += *transform.left();
        }
        if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
            direction += *transform.right();
        }
        if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::Space) {
            direction += Vec3::Y;
        }
        if keys.pressed(KeyCode::KeyQ) || keys.pressed(KeyCode::ShiftLeft) {
            direction += Vec3::NEG_Y;
        }
        if direction != Vec3::ZERO {
            transform.translation += direction.normalize() * cam.speed * time.delta_secs();
        }
    }
}

fn client_fly_look(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    mut query: Query<&mut Transform, With<ClientFlyCam>>,
) {
    if !buttons.pressed(MouseButton::Right) {
        return;
    }
    for mut transform in &mut query {
        let yaw = -motion.delta.x * 0.005;
        let pitch = -motion.delta.y * 0.005;
        transform.rotate_local_x(pitch);
        transform.rotate_y(yaw);
    }
}

fn client_fly_speed(mut wheel: MessageReader<MouseWheel>, mut query: Query<&mut ClientFlyCam>) {
    for event in wheel.read() {
        for mut cam in &mut query {
            cam.speed = (cam.speed * (1.0 + event.y * 0.1)).clamp(0.5, 100.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Visuals from replicated state
// ---------------------------------------------------------------------------

/// Write replicated (interpolated) transforms into local Bevy transforms.
#[allow(clippy::type_complexity)]
fn client_apply_transforms(
    query: Query<
        (&NetTransform, &mut Transform, Option<&mut Visibility>),
        (With<NetVisual>, Without<ClientFlyCam>),
    >,
) {
    for (net, mut transform, visibility) in query {
        let wt = &net.0;
        transform.translation = Vec3::from_array(wt.position);
        transform.rotation = Quat::from_euler(
            EulerRot::XYZ,
            wt.rotation_degrees[0].to_radians(),
            wt.rotation_degrees[1].to_radians(),
            wt.rotation_degrees[2].to_radians(),
        );
        transform.scale = Vec3::from_array(wt.scale);
        if let Some(mut visibility) = visibility {
            *visibility = if wt.visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
}

/// Build local visuals for newly replicated entities.
#[allow(clippy::type_complexity)]
fn client_spawn_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_entities: Query<
        (
            Entity,
            &NetWorldId,
            &NetName,
            &NetEntityKind,
            &NetTransform,
            Option<&NetShape>,
            Option<&NetMaterial>,
            Option<&NetLight>,
        ),
        (Added<NetWorldId>, Without<NetVisual>),
    >,
    mut map: ResMut<ClientEntityMap>,
    mut stats: ResMut<ClientStreamStats>,
) {
    for (entity, id, name, kind, transform, shape, material, light) in new_entities.iter() {
        stats.spawned += 1;
        let mut entity_cmd = commands.entity(entity);
        entity_cmd.insert((
            Name::new(name.0.clone()),
            NetVisual,
            Transform {
                translation: Vec3::from_array(transform.0.position),
                rotation: Quat::from_euler(
                    EulerRot::XYZ,
                    transform.0.rotation_degrees[0].to_radians(),
                    transform.0.rotation_degrees[1].to_radians(),
                    transform.0.rotation_degrees[2].to_radians(),
                ),
                scale: Vec3::from_array(transform.0.scale),
            },
        ));
        if !transform.0.visible {
            entity_cmd.insert(Visibility::Hidden);
        }

        match kind.0 {
            NetKind::Primitive => {
                if let Some(shape) = shape {
                    entity_cmd.insert(Mesh3d(shape_to_mesh(&shape.0, &mut meshes)));
                    let mat = material
                        .map(|m| m.0.clone())
                        .unwrap_or_else(wt_default_material);
                    entity_cmd.insert(MeshMaterial3d(
                        materials.add(material_def_to_standard(&mat)),
                    ));
                }
                if let Some(light) = light {
                    insert_light_component(&mut entity_cmd, &light.0);
                }
            }
            NetKind::Light => {
                if let Some(light) = light {
                    insert_light_component(&mut entity_cmd, &light.0);
                }
            }
            NetKind::Mesh => {
                // Placeholder until mesh assets can be served to clients.
                entity_cmd.insert(Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))));
                entity_cmd.insert(MeshMaterial3d(materials.add(placeholder_material())));
                if let Some(shape) = shape {
                    // gen_spawn_mesh entities carry a bounding shape — use it.
                    entity_cmd.insert(Mesh3d(shape_to_mesh(&shape.0, &mut meshes)));
                }
            }
            NetKind::Group | NetKind::AudioEmitter => {
                // Nothing visual — transform + name are enough.
            }
        }

        map.id_to_entity.insert(id.0, entity);
    }

    // Console feedback at coarse milestones (first entity, then every 25).
    if stats.spawned > 0 && (stats.spawned == 1 || stats.spawned / 25 > stats.announced / 25) {
        println!("[net] World streaming: {} entities", stats.spawned);
        stats.announced = stats.spawned;
    }
}

fn wt_default_material() -> localgpt_world_types::MaterialDef {
    localgpt_world_types::MaterialDef {
        color: [0.8, 0.8, 0.8, 1.0],
        metallic: 0.0,
        roughness: 0.9,
        emissive: [0.0; 4],
        alpha_mode: None,
        unlit: None,
        double_sided: None,
        reflectance: None,
        ..Default::default()
    }
}

fn placeholder_material() -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgba(0.6, 0.7, 0.9, 0.4),
        alpha_mode: AlphaMode::Blend,
        ..default()
    }
}

/// Update visuals when replicated shape/material/light components change.
#[allow(clippy::type_complexity)]
fn client_update_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    changed_shapes: Query<(Entity, &NetShape), (Changed<NetShape>, With<NetVisual>)>,
    changed_materials: Query<(Entity, &NetMaterial), (Changed<NetMaterial>, With<NetVisual>)>,
    changed_lights: Query<(Entity, &NetLight), (Changed<NetLight>, With<NetVisual>)>,
) {
    for (entity, shape) in &changed_shapes {
        commands
            .entity(entity)
            .insert(Mesh3d(shape_to_mesh(&shape.0, &mut meshes)));
    }

    for (entity, material) in &changed_materials {
        commands.entity(entity).insert(MeshMaterial3d(
            materials.add(material_def_to_standard(&material.0)),
        ));
    }
    for (entity, light) in &changed_lights {
        let mut entity_cmd = commands.entity(entity);
        entity_cmd.remove::<(DirectionalLight, PointLight, SpotLight)>();
        insert_light_component(&mut entity_cmd, &light.0);
    }
}

/// Forget world ids whose entity left this client's view (or was deleted on
/// the host), so a later re-entry maps to the fresh entity.
fn on_replicated_despawn(
    trigger: On<Remove, NetWorldId>,
    ids: Query<&NetWorldId>,
    mut map: ResMut<ClientEntityMap>,
) {
    let entity = trigger.entity;
    if let Ok(id) = ids.get(entity)
        && map.id_to_entity.get(&id.0) == Some(&entity)
    {
        map.id_to_entity.remove(&id.0);
    }
    map.pending_parents.retain(|(child, _)| *child != entity);
}

/// Resolve replicated parent links once both entities exist locally.
fn client_resolve_parents(
    mut commands: Commands,
    children: Query<(Entity, &NetParentId), Added<NetParentId>>,
    mut map: ResMut<ClientEntityMap>,
) {
    for (child, parent) in &children {
        map.pending_parents.push((child, parent.0));
    }
    let mut still_pending = Vec::new();
    for (child, parent_id) in std::mem::take(&mut map.pending_parents) {
        if let Some(parent) = map.id_to_entity.get(&parent_id).copied() {
            if child != parent {
                // Inserting the relationship directly keeps the (already
                // replicated) local transform intact.
                commands.entity(child).insert(ChildOf(parent));
            }
        } else {
            still_pending.push((child, parent_id));
        }
    }
    map.pending_parents = still_pending;
}

/// Apply session metadata (background + ambient) to rendering resources.
fn client_apply_meta(
    mut commands: Commands,
    meta: Query<&NetWorldMeta, Changed<NetWorldMeta>>,
    session: Res<ClientSession>,
    mut assets: ResMut<ClientAssets>,
) {
    for meta in &meta {
        let base_url = meta
            .asset_port
            .map(|port| format!("http://{}", SocketAddr::new(session.server_addr.ip(), port)));
        if assets.base_url != base_url {
            assets.base_url = base_url;
        }
        if let Some(bg) = meta.environment.background_color {
            commands.insert_resource(ClearColor(Color::srgba(bg[0], bg[1], bg[2], bg[3])));
        }
        if let Some(intensity) = meta.environment.ambient_intensity {
            let color = meta
                .environment
                .ambient_color
                .map(|c| Color::srgba(c[0], c[1], c[2], c[3]))
                .unwrap_or(Color::WHITE);
            commands.insert_resource(GlobalAmbientLight {
                color,
                brightness: intensity,
                affects_lightmapped_meshes: true,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Chat / prompts
// ---------------------------------------------------------------------------

/// Send REPL prompts to the host, spawning a zero-latency local scaffold
/// where the camera is looking.
#[allow(clippy::too_many_arguments)]
fn client_send_prompts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    outbox: Res<PromptOutbox>,
    mut senders: Query<&mut MessageSender<ClientPrompt>>,
    camera: Query<&Transform, With<ClientFlyCam>>,
    mut local: ResMut<LocalCommands>,
    mut log: ResMut<ClientPanelLog>,
) {
    let Ok(mut rx) = outbox.rx.lock() else {
        return;
    };
    while let Ok(text) = rx.try_recv() {
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if text.starts_with('/') {
            local.0.push(text);
            continue;
        }
        log.push(ClientEntry::You(text.clone()));
        let anchor = camera.single().ok().map(|cam| {
            prompt_anchor(
                cam.translation.to_array(),
                cam.forward().as_vec3().to_array(),
            )
        });
        let request_id = rand::random::<u64>();
        for mut sender in &mut senders {
            sender.send::<PromptChannel>(ClientPrompt {
                text: text.clone(),
                request_id,
                anchor,
            });
        }
        commands.spawn((
            Name::new("Scaffold (local)"),
            LocalScaffold { request_id },
            ScaffoldVisual { running: false },
            Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))),
            MeshMaterial3d(materials.add(scaffold_material(false))),
            Transform::from_translation(Vec3::from_array(anchor.unwrap_or([0.0; 3]))),
        ));
    }
}

/// REPL lines starting with `/` — handled locally, never sent to the host.
#[derive(Resource, Default)]
struct LocalCommands(Vec<String>);

/// Run local slash commands: `/stats` (what this client is receiving) and
/// `/goto x y z` (teleport the camera — handy for exercising streaming).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn client_local_commands(
    mut local: ResMut<LocalCommands>,
    mut camera: Query<&mut Transform, With<ClientFlyCam>>,
    replicated: Query<(), With<NetWorldId>>,
    scaffolds: Query<(), With<NetScaffold>>,
    local_scaffolds: Query<(), With<LocalScaffold>>,
    impostors: Query<(&Impostor, &Visibility)>,
    bake: Option<Res<BakeState>>,
    view: Res<ClientViewState>,
    assets: Res<ClientAssets>,
    mut log: ResMut<ClientPanelLog>,
) {
    for command in std::mem::take(&mut local.0) {
        let mut parts = command.split_whitespace();
        let line = match parts.next() {
            Some("/stats") => {
                let (chunks, meshes, folded) = bake.as_ref().map(|b| b.stats()).unwrap_or_default();
                let cam = camera
                    .single()
                    .map(|t| t.translation.to_array())
                    .unwrap_or_default();
                format!(
                    "[stats] camera=({:.1}, {:.1}, {:.1}) view={} r={} entities={} scaffolds={} \
                     local_scaffolds={} impostors_visible={} baked_chunks={chunks} \
                     baked_meshes={meshes} baked_entities={folded} meshes_streamed={} \
                     meshes_downloaded={}",
                    cam[0],
                    cam[1],
                    cam[2],
                    view.window.center,
                    view.window.radius,
                    replicated.iter().count(),
                    scaffolds.iter().count(),
                    local_scaffolds.iter().count(),
                    visible_impostors(&impostors),
                    assets.loaded.len(),
                    assets.downloaded,
                )
            }
            Some("/goto") => {
                let coords: Vec<f32> = parts.filter_map(|p| p.parse().ok()).collect();
                if let ([x, y, z], Ok(mut cam)) = (coords.as_slice(), camera.single_mut()) {
                    cam.translation = Vec3::new(*x, *y, *z);
                    format!("[goto] camera moved to ({x}, {y}, {z})")
                } else {
                    "usage: /goto <x> <y> <z>".to_string()
                }
            }
            _ => "local commands: /stats, /goto <x> <y> <z>".to_string(),
        };
        println!("{line}");
        log.push(ClientEntry::Status(line));
    }
}

/// Where a prompt should land: the ground point under the camera's gaze if
/// it's within reach, else a point a fixed distance ahead.
pub(crate) fn prompt_anchor(position: [f32; 3], forward: [f32; 3]) -> [f32; 3] {
    const MIN_DIST: f32 = 2.0;
    const MAX_DIST: f32 = 60.0;
    const FALLBACK_DIST: f32 = 12.0;
    let pos = Vec3::from_array(position);
    let dir = Vec3::from_array(forward).normalize_or(Vec3::NEG_Z);
    if dir.y < -0.05 && pos.y > 0.0 {
        let t = -pos.y / dir.y;
        if t <= MAX_DIST {
            let hit = pos + dir * t.max(MIN_DIST);
            return [hit.x, hit.y.max(0.0), hit.z];
        }
    }
    (pos + dir * FALLBACK_DIST).to_array()
}

/// Report the camera position to the host (drives its interest management)
/// and keep the local view window in step for HLOD decisions.
fn client_report_view(
    time: Res<Time>,
    mut timer: Local<f32>,
    camera: Query<&Transform, With<ClientFlyCam>>,
    mut senders: Query<&mut MessageSender<ClientView>>,
    mut view: ResMut<ClientViewState>,
) {
    let Ok(cam) = camera.single() else {
        return;
    };
    let position = cam.translation.to_array();
    let window = ViewWindow::at(position, view.radius);
    if window != view.window {
        view.window = window;
    }
    *timer += time.delta_secs();
    if *timer < VIEW_REPORT_SECS {
        return;
    }
    *timer = 0.0;
    for mut sender in &mut senders {
        sender.send::<ViewChannel>(ClientView {
            position,
            radius: view.radius,
        });
    }
}

// ---------------------------------------------------------------------------
// Asset streaming (§2)
// ---------------------------------------------------------------------------

/// Queue streamed meshes for entities whose digest arrived or changed, and
/// start fetches: memory → disk cache → host asset server.
fn client_request_mesh_assets(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    changed: Query<(Entity, &NetMeshAsset, Option<&NetMaterial>), Changed<NetMeshAsset>>,
    mut assets: ResMut<ClientAssets>,
) {
    for (entity, asset, material) in &changed {
        if let Some(handle) = assets.loaded.get(&asset.digest).cloned() {
            apply_streamed_mesh(&mut commands, &mut materials, entity, handle, material);
        } else {
            assets
                .waiting
                .entry(asset.digest.clone())
                .or_default()
                .push(entity);
        }
    }

    let ClientAssets {
        base_url,
        cache_dir,
        waiting,
        in_flight,
        failed,
        tx,
        ..
    } = &mut *assets;
    for digest in waiting.keys() {
        if in_flight.contains(digest) || failed.contains(digest) {
            continue;
        }
        if let Some(bytes) = read_cached(cache_dir, digest) {
            in_flight.insert(digest.clone());
            let _ = tx.send(AssetFetch {
                digest: digest.clone(),
                result: Ok(bytes),
                from_network: false,
            });
            continue;
        }
        // Not cached: fetch once the host's asset server is known.
        let Some(base_url) = base_url.clone() else {
            continue;
        };
        in_flight.insert(digest.clone());
        let (digest, tx) = (digest.clone(), tx.clone());
        std::thread::spawn(move || {
            let result = fetch_blob(&base_url, &digest);
            let _ = tx.send(AssetFetch {
                digest,
                result,
                from_network: true,
            });
        });
    }
}

/// Decode fetched blobs and swap placeholders for the real geometry.
fn client_receive_mesh_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    net_materials: Query<Option<&NetMaterial>>,
    mut assets: ResMut<ClientAssets>,
) {
    let fetched: Vec<AssetFetch> = {
        let Ok(rx) = assets.rx.lock() else {
            return;
        };
        rx.try_iter().collect()
    };
    for fetch in fetched {
        assets.in_flight.remove(&fetch.digest);
        let mesh = fetch.result.and_then(|bytes| {
            decode_mesh(&bytes)
                .map(|m| (m, bytes))
                .map_err(|e| e.to_string())
        });
        let (mesh, bytes) = match mesh {
            Ok(ok) => ok,
            Err(e) => {
                eprintln!("[net] mesh asset {} unavailable: {e}", &fetch.digest[..12]);
                assets.failed.insert(fetch.digest);
                continue;
            }
        };
        if fetch.from_network {
            write_cached(&assets.cache_dir, &fetch.digest, &bytes);
            assets.downloaded += 1;
        }
        let handle = meshes.add(mesh);
        assets.loaded.insert(fetch.digest.clone(), handle.clone());
        for entity in assets.waiting.remove(&fetch.digest).unwrap_or_default() {
            let Ok(material) = net_materials.get(entity) else {
                continue; // despawned while we were fetching
            };
            apply_streamed_mesh(
                &mut commands,
                &mut materials,
                entity,
                handle.clone(),
                material,
            );
        }
    }
}

fn apply_streamed_mesh(
    commands: &mut Commands,
    materials: &mut Assets<StandardMaterial>,
    entity: Entity,
    handle: Handle<Mesh>,
    material: Option<&NetMaterial>,
) {
    let mat = material
        .map(|m| m.0.clone())
        .unwrap_or_else(wt_default_material);
    commands.entity(entity).try_insert((
        Mesh3d(handle),
        MeshMaterial3d(materials.add(material_def_to_standard(&mat))),
    ));
}

// ---------------------------------------------------------------------------
// Scaffolds (§2 scaffold-then-replace)
// ---------------------------------------------------------------------------

/// Client-predicted scaffold spawned the moment a prompt is sent.
#[derive(Component)]
struct LocalScaffold {
    request_id: u64,
}

/// Visual state shared by local and replicated scaffolds.
#[derive(Component)]
struct ScaffoldVisual {
    running: bool,
}

fn scaffold_material(running: bool) -> StandardMaterial {
    let (base, glow) = if running {
        (
            Color::srgba(1.0, 0.75, 0.3, 0.30),
            LinearRgba::rgb(0.8, 0.5, 0.1),
        )
    } else {
        (
            Color::srgba(0.4, 0.7, 1.0, 0.25),
            LinearRgba::rgb(0.1, 0.3, 0.6),
        )
    };
    StandardMaterial {
        base_color: base,
        emissive: glow,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    }
}

/// Give replicated scaffolds a visual and retire the matching local one.
#[allow(clippy::type_complexity)]
fn client_spawn_scaffolds(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    added: Query<(Entity, &NetScaffold, Option<&NetTransform>), Added<NetScaffold>>,
    mut changed: Query<
        (
            &NetScaffold,
            &mut ScaffoldVisual,
            &MeshMaterial3d<StandardMaterial>,
        ),
        Changed<NetScaffold>,
    >,
    locals: Query<(Entity, &LocalScaffold)>,
) {
    for (entity, scaffold, transform) in &added {
        let position = transform.map(|t| t.0.position).unwrap_or([0.0; 3]);
        commands.entity(entity).insert((
            Name::new(format!("Scaffold #{}", scaffold.job_id)),
            ScaffoldVisual {
                running: scaffold.running,
            },
            Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))),
            MeshMaterial3d(materials.add(scaffold_material(scaffold.running))),
            Transform::from_translation(Vec3::from_array(position)),
        ));
        // Hand-off: the authoritative scaffold replaces our prediction.
        for (local, own) in &locals {
            if own.request_id == scaffold.request_id {
                commands.entity(local).despawn();
            }
        }
    }
    for (scaffold, mut visual, material) in &mut changed {
        if visual.running != scaffold.running {
            visual.running = scaffold.running;
            if let Some(mut mat) = materials.get_mut(&material.0) {
                *mat = scaffold_material(scaffold.running);
            }
        }
    }
}

/// Breathe scaffolds so they read as "in progress".
fn client_animate_scaffolds(time: Res<Time>, mut query: Query<(&ScaffoldVisual, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (visual, mut transform) in &mut query {
        let speed = if visual.running { 6.0 } else { 2.0 };
        let s = 1.0 + 0.08 * (t * speed).sin();
        transform.scale = Vec3::splat(s);
        if visual.running {
            transform.rotate_y(time.delta_secs() * 0.8);
        }
    }
}

/// Show queue progress for our prompts; drop local scaffolds on terminal
/// states (normally the replicated scaffold already replaced them).
fn client_receive_job_status(
    mut commands: Commands,
    mut receivers: Query<&mut MessageReceiver<JobStatus>>,
    locals: Query<(Entity, &LocalScaffold)>,
    mut log: ResMut<ClientPanelLog>,
) {
    for mut receiver in &mut receivers {
        for status in receiver.receive() {
            let line = match &status.state {
                JobState::Queued { position: 0 } => {
                    format!("[queue] job #{} is next up", status.job_id)
                }
                JobState::Queued { position } => {
                    format!(
                        "[queue] job #{} queued — {position} ahead of it",
                        status.job_id
                    )
                }
                JobState::Running => format!("[queue] job #{} is being built…", status.job_id),
                JobState::Done => format!("[queue] job #{} done", status.job_id),
                JobState::Failed { reason } => {
                    format!("[queue] job #{} failed: {reason}", status.job_id)
                }
                JobState::Rejected { reason } => format!("[queue] prompt rejected: {reason}"),
            };
            println!("{line}");
            let entry = if matches!(
                &status.state,
                JobState::Failed { .. } | JobState::Rejected { .. }
            ) {
                ClientEntry::Error(line)
            } else {
                ClientEntry::Status(line)
            };
            log.push(entry);
            if status.state.is_terminal() {
                for (entity, local) in &locals {
                    if local.request_id == status.request_id {
                        commands.entity(entity).despawn();
                    }
                }
            }
        }
    }
}

/// Print host chat events to the local console and the client panel.
fn client_receive_chat(
    mut receivers: Query<&mut MessageReceiver<HostChat>>,
    mut log: ResMut<ClientPanelLog>,
) {
    for mut receiver in &mut receivers {
        for chat in receiver.receive() {
            println!("\n[{}] {}\n", chat.speaker, chat.text);
            log.push(ClientEntry::Chat {
                speaker: chat.speaker.clone(),
                text: chat.text.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_log_stays_bounded() {
        let mut log = ClientPanelLog::default();
        for i in 0..500 {
            log.push(ClientEntry::Status(format!("line {i}")));
        }
        assert_eq!(log.entries.len(), 400);
        assert_eq!(
            log.entries.first(),
            Some(&ClientEntry::Status("line 100".into()))
        );
    }
}
