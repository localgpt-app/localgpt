//! Listen-server host: authoritative ECS + rendering client in one process
//! (§1 of the collaborative world engine spec).
//!
//! The host is the existing single-player gen app plus three additions:
//!
//! 1. A lightyear UDP server (netcode-authenticated) that replicates the
//!    world-model components from [`super::protocol`] to every connected
//!    client. Entities are attached to replication as they spawn and
//!    re-synced whenever their live components change — the existing
//!    `GenCommand` handlers are untouched.
//! 2. An mDNS announcer so LAN clients can discover the session.
//! 3. A prompt/chat bridge: client prompts flow to the agent loop through a
//!    channel, and the agent's replies are broadcast back to all clients.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;
use std::time::Duration;

use bevy::prelude::*;
use lightyear::connection::client_of::ClientOf;
use lightyear::connection::server::Start;
use lightyear::netcode::NetcodeServer;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use localgpt_world_types as wt;
use tokio::sync::mpsc;

use super::mdns::SessionAnnouncer;
use super::protocol::{
    ChatChannel, ClientPrompt, HostChat, NetKind, NetProtocolPlugin, NetTransform, NetWorldMeta,
    apply_net_components, transforms_differ,
};
use crate::gen3d::audio::AudioEmitter;
use crate::gen3d::behaviors::EntityBehaviors;
use crate::gen3d::plugin::{CurrentWorld, SnapshotQueries, snapshot_entity};
use crate::gen3d::registry::{GenEntity, GenEntityType, GltfSource, NameRegistry, ParametricShape};

/// How often replicated component deltas are sampled and sent.
const REPLICATION_SEND_INTERVAL: Duration = Duration::from_millis(50);

/// Maximum simultaneous clients for a LAN session.
const MAX_CLIENTS: usize = 8;

/// Host-side options, built by the CLI when `--host` is given.
pub struct NetHostOptions {
    /// Session name shown in mDNS discovery and to joining clients.
    pub session_name: String,
    /// UDP port to listen on.
    pub port: u16,
    /// Sender half of the prompt channel into the agent loop.
    pub prompt_tx: mpsc::UnboundedSender<ClientPrompt>,
    /// Receiver half of the chat channel out of the agent loop.
    pub chat_rx: mpsc::UnboundedReceiver<HostChat>,
}

/// Agent-loop side of the prompt/chat bridge.
pub struct AgentNetHooks {
    /// Prompts received from connected clients.
    pub prompt_rx: mpsc::UnboundedReceiver<ClientPrompt>,
    /// Outbound chat events shown to connected clients.
    pub chat_tx: mpsc::UnboundedSender<HostChat>,
}

/// Create a matched (options, hooks) pair for the host.
pub fn create_host_channels() -> (NetHostOptions, AgentNetHooks) {
    let (prompt_tx, prompt_rx) = mpsc::unbounded_channel();
    let (chat_tx, chat_rx) = mpsc::unbounded_channel();
    (
        NetHostOptions {
            session_name: String::new(),
            port: super::DEFAULT_PORT,
            prompt_tx,
            chat_rx,
        },
        AgentNetHooks { prompt_rx, chat_tx },
    )
}

/// Resource carrying host session configuration.
#[derive(Resource)]
struct NetHostState {
    port: u16,
}

/// Resource: forward prompts from connected clients to the agent loop.
#[derive(Resource)]
struct RemotePrompts {
    tx: mpsc::UnboundedSender<ClientPrompt>,
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
            prompt_tx,
            chat_rx,
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

        app.insert_resource(ReplicationMetadata::new(REPLICATION_SEND_INTERVAL))
            .insert_resource(NetHostState { port })
            .insert_resource(RemotePrompts { tx: prompt_tx })
            .insert_resource(HostChatOutbox {
                rx: Mutex::new(chat_rx),
            })
            .add_systems(Startup, start_listen_server)
            .add_systems(Startup, spawn_world_meta_entity.after(start_listen_server))
            .add_observer(on_client_link_connected)
            .add_systems(PreUpdate, net_attach_new_entities)
            .add_systems(PostUpdate, (net_sync_changes, net_sync_meta))
            .add_systems(
                Update,
                (net_prompt_intake, net_chat_broadcast, net_client_lifecycle),
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
        private_key: super::PRIVATE_KEY,
        max_clients: MAX_CLIENTS,
        // We bind 0.0.0.0 while clients connect via any of the host's LAN
        // addresses, so token-address validation would always mismatch.
        // Safe to skip under the Phase-1 trust model (see mod.rs): the
        // private key is a compile-time constant anyway.
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
        NetWorldMeta {
            name: String::new(),
            environment: default_environment(),
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
/// Cameras are skipped — each client renders with its own camera.
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
        let kind = match gen_entity.entity_type {
            GenEntityType::Camera => continue,
            GenEntityType::Primitive => NetKind::Primitive,
            GenEntityType::Light => NetKind::Light,
            GenEntityType::Mesh => NetKind::Mesh,
            GenEntityType::Group => NetKind::Group,
            GenEntityType::AudioEmitter => NetKind::AudioEmitter,
        };
        let we = snapshot_entity(name.as_ref(), entity, gen_entity.world_id, &sq);
        commands
            .entity(entity)
            .insert(Replicate::to_clients(NetworkTarget::All));
        apply_net_components(&mut commands.entity(entity), &we, kind);
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
        let kind = match gen_entity.entity_type {
            GenEntityType::Camera => continue,
            GenEntityType::Primitive => NetKind::Primitive,
            GenEntityType::Light => NetKind::Light,
            GenEntityType::Mesh => NetKind::Mesh,
            GenEntityType::Group => NetKind::Group,
            GenEntityType::AudioEmitter => NetKind::AudioEmitter,
        };
        let we = snapshot_entity(name.as_ref(), entity, gen_entity.world_id, &sq);
        apply_net_components(&mut commands.entity(entity), &we, kind);
    }
}

/// Keep the singleton [`NetWorldMeta`] in step with world resources.
fn net_sync_meta(
    current_world: Res<CurrentWorld>,
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
    if meta.name != name || meta.environment != environment {
        meta.name = name;
        meta.environment = environment;
    }
}

/// Insert replication + prompt reception on each client link entity.
///
/// Netcode spawns a `ClientOf` entity per accepted connection; replication
/// itself only starts once the link is also `Connected`.
fn on_client_link_connected(trigger: On<Add, ClientOf>, mut commands: Commands) {
    commands.entity(trigger.entity).insert((
        ReplicationSender,
        Name::new("Client Link"),
        MessageReceiver::<ClientPrompt>::default(),
    ));
}

/// Forward client prompts to the agent loop and echo them to all clients.
fn net_prompt_intake(
    mut receivers: Query<&mut MessageReceiver<ClientPrompt>>,
    server: Query<&Server>,
    mut sender: ServerMultiMessageSender,
    prompts: Res<RemotePrompts>,
) {
    let Some(server) = server.iter().next() else {
        return;
    };
    for mut receiver in &mut receivers {
        for prompt in receiver.receive() {
            info!("Prompt from client: {}", prompt.text);
            let _ = prompts.tx.send(ClientPrompt {
                text: prompt.text.clone(),
            });
            let _ = sender.send::<HostChat, ChatChannel>(
                &HostChat {
                    speaker: "client".to_string(),
                    text: prompt.text,
                },
                server,
                &NetworkTarget::All,
            );
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

/// Announce client joins/leaves on the host console and to clients.
fn net_client_lifecycle(
    links: Query<Entity, With<ClientOf>>,
    server: Query<&Server>,
    mut sender: ServerMultiMessageSender,
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
        eprintln!("Client disconnected");
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
