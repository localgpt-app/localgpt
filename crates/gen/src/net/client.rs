//! Collaborative session client (§1): a read-only viewer that renders the
//! host's replicated world and forwards natural-language prompts to the
//! host's agent.
//!
//! The client is a slim Bevy app — no gen command processing, no local
//! agent, no inspector. Replicated world-model components are composed back
//! into meshes/materials/lights through the same conversion helpers the
//! world-load path uses, so host and client render the same scene.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;

use bevy::input::mouse::{AccumulatedMouseMotion, MouseWheel};
use bevy::prelude::*;
use lightyear::connection::client::Connected;
use lightyear::netcode::NetcodeClient;
use lightyear::netcode::auth::Authentication;
use lightyear::prelude::client::*;
use lightyear::prelude::*;
use tokio::sync::mpsc;

use super::protocol::{
    ClientPrompt, HostChat, NetEntityKind, NetKind, NetLight, NetMaterial, NetName, NetParentId,
    NetProtocolPlugin, NetShape, NetTransform, NetWorldId, NetWorldMeta, PromptChannel,
};
use crate::gen3d::plugin::{insert_light_component, material_def_to_standard, shape_to_mesh};

/// Client options, built by the CLI when `--join` is given.
pub struct NetClientOptions {
    /// Host address to connect to.
    pub server_addr: SocketAddr,
    /// Prompts typed into the local REPL, drained and sent each frame.
    pub prompt_rx: mpsc::UnboundedReceiver<String>,
}

/// Resource carrying the prompt channel from the REPL thread.
#[derive(Resource)]
struct PromptOutbox {
    rx: Mutex<mpsc::UnboundedReceiver<String>>,
}

/// Marker on client entities that have local visuals built.
#[derive(Component)]
struct NetVisual;

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

        app.insert_resource(ClientSession { server_addr })
            .insert_resource(PromptOutbox {
                rx: Mutex::new(prompt_rx),
            })
            .init_resource::<ClientEntityMap>()
            .init_resource::<ClientStreamStats>()
            .add_systems(Startup, (connect_to_host, spawn_client_camera))
            .add_observer(on_connected)
            .add_systems(Update, client_apply_transforms)
            .add_systems(Update, client_spawn_visuals)
            .add_systems(Update, client_update_visuals)
            .add_systems(Update, client_resolve_parents)
            .add_systems(Update, client_apply_meta)
            .add_systems(Update, (client_send_prompts, client_receive_chat))
            .add_systems(Update, (client_fly_move, client_fly_look, client_fly_speed));
    }
}

/// Resource carrying the resolved host address.
#[derive(Resource)]
struct ClientSession {
    server_addr: SocketAddr,
}

/// Spawn the client link entity and initiate the connection.
fn connect_to_host(mut commands: Commands, session: Res<ClientSession>) {
    let auth = Authentication::Manual {
        server_addr: session.server_addr,
        client_id: rand::random::<u64>(),
        private_key: super::PRIVATE_KEY,
        protocol_id: super::PROTOCOL_ID,
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
            MessageReceiver::<HostChat>::default(),
        ))
        .id();
    commands.trigger(Connect { entity: client });
    info!("Connecting to {} …", session.server_addr);
}

fn on_connected(_trigger: On<Add, Connected>) {
    println!("\n[net] Connected to host — the world will stream in shortly.");
    println!("[net] Type a prompt and press Enter to ask the host's agent.\n");
}

// ---------------------------------------------------------------------------
// Camera — a compact free-fly camera (WASD + Q/E, right-drag look, wheel speed)
// ---------------------------------------------------------------------------

#[derive(Component)]
struct ClientFlyCam {
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
fn client_apply_meta(mut commands: Commands, meta: Query<&NetWorldMeta, Changed<NetWorldMeta>>) {
    for meta in &meta {
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

/// Send REPL prompts to the host.
fn client_send_prompts(
    outbox: Res<PromptOutbox>,
    mut senders: Query<&mut MessageSender<ClientPrompt>>,
) {
    let Ok(mut rx) = outbox.rx.lock() else {
        return;
    };
    while let Ok(text) = rx.try_recv() {
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        for mut sender in &mut senders {
            sender.send::<PromptChannel>(ClientPrompt { text: text.clone() });
        }
    }
}

/// Print host chat events to the local console.
fn client_receive_chat(mut receivers: Query<&mut MessageReceiver<HostChat>>) {
    for mut receiver in &mut receivers {
        for chat in receiver.receive() {
            println!("\n[{}] {}\n", chat.speaker, chat.text);
        }
    }
}
