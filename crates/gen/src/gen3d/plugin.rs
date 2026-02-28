//! Bevy GenPlugin — command processing, default scene, screenshot capture, glTF loading.

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::scene::SceneRoot;

use std::ffi::OsStr;
use std::path::PathBuf;

use super::GenChannels;
use super::audio::{self, SpatialAudioListener};
use super::commands::*;
use super::registry::*;

/// Bevy resource holding the workspace path for default export locations.
#[derive(Resource, Clone)]
pub struct GenWorkspace {
    pub path: PathBuf,
}

/// Bevy resource wrapping the channel endpoints.
#[derive(Resource)]
pub struct GenChannelRes {
    channels: GenChannels,
}

impl GenChannelRes {
    pub fn new(channels: GenChannels) -> Self {
        Self { channels }
    }
}

/// Pending screenshot requests that need to wait N frames.
#[derive(Resource, Default)]
pub struct PendingScreenshots {
    queue: Vec<PendingScreenshot>,
}

#[allow(dead_code)]
struct PendingScreenshot {
    frames_remaining: u32,
    width: u32,
    height: u32,
    path: Option<String>,
}

/// Initial glTF scene to load at startup.
#[derive(Resource)]
pub struct GenInitialScene {
    pub path: Option<PathBuf>,
}

/// A glTF scene that is currently being loaded.
struct PendingGltfLoad {
    handle: Handle<Scene>,
    name: String,
    path: String,
    send_response: bool,
}

/// Queue of pending glTF loads waiting for asset server to finish loading.
#[derive(Resource, Default)]
struct PendingGltfLoads {
    queue: Vec<PendingGltfLoad>,
}

/// Marker component for the interactive fly camera.
#[derive(Component)]
struct FlyCam;

/// Configuration for the fly camera controller.
#[derive(Resource)]
struct FlyCamConfig {
    move_speed: f32,
    look_sensitivity: f32,
}

impl Default for FlyCamConfig {
    fn default() -> Self {
        Self {
            move_speed: 5.0,
            look_sensitivity: 0.003,
        }
    }
}

/// Plugin that sets up the Gen 3D environment.
#[allow(dead_code)]
pub struct GenPlugin {
    pub channels: GenChannels,
}

impl Plugin for GenPlugin {
    fn build(&self, _app: &mut App) {
        // We can't move channels out of &self in build(), so we use a
        // workaround: store channels in a temporary and take them in a
        // startup system. See `setup_channels` below.
    }
}

/// Initialize the Gen world: channels, default scene, systems.
///
/// Call this instead of using Plugin::build since we need to move the channels.
pub fn setup_gen_app(
    app: &mut App,
    channels: GenChannels,
    workspace: PathBuf,
    initial_scene: Option<PathBuf>,
) {
    app.insert_resource(GenChannelRes::new(channels))
        .insert_resource(GenWorkspace { path: workspace })
        .insert_resource(GenInitialScene {
            path: initial_scene,
        })
        .init_resource::<NameRegistry>()
        .init_resource::<PendingScreenshots>()
        .init_resource::<PendingGltfLoads>()
        .init_resource::<FlyCamConfig>()
        .add_systems(
            Startup,
            (
                setup_default_scene,
                load_initial_scene,
                audio::init_audio_engine,
            ),
        )
        .add_systems(Update, process_gen_commands)
        .add_systems(Update, process_pending_screenshots)
        .add_systems(Update, process_pending_gltf_loads)
        .add_systems(Update, audio::spatial_audio_update)
        .add_systems(Update, audio::auto_infer_audio)
        .add_systems(Update, fly_cam_movement)
        .add_systems(Update, fly_cam_look)
        .add_systems(Update, fly_cam_scroll_speed);
}

/// Default scene: ground plane, camera, directional light, ambient light.
fn setup_default_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<NameRegistry>,
) {
    // Ground plane — 20×20 gray
    let ground = commands
        .spawn((
            Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::new(10.0, 10.0)))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgba(0.3, 0.3, 0.3, 1.0),
                metallic: 0.0,
                perceptual_roughness: 0.8,
                ..default()
            })),
            Transform::from_translation(Vec3::ZERO),
            Name::new("ground_plane"),
            GenEntity {
                entity_type: GenEntityType::Primitive,
            },
        ))
        .id();
    registry.insert("ground_plane".into(), ground);

    // Camera at (5, 5, 5) looking at origin
    let camera = commands
        .spawn((
            Camera3d::default(),
            Transform::from_translation(Vec3::new(5.0, 5.0, 5.0)).looking_at(Vec3::ZERO, Vec3::Y),
            Name::new("main_camera"),
            FlyCam,
            SpatialAudioListener,
            GenEntity {
                entity_type: GenEntityType::Camera,
            },
        ))
        .id();
    registry.insert("main_camera".into(), camera);

    // Directional light — warm white, shadows
    let light = commands
        .spawn((
            DirectionalLight {
                illuminance: 10000.0,
                shadows_enabled: true,
                color: Color::srgba(1.0, 0.95, 0.9, 1.0),
                ..default()
            },
            Transform::from_translation(Vec3::new(4.0, 8.0, 4.0)).looking_at(Vec3::ZERO, Vec3::Y),
            Name::new("main_light"),
            GenEntity {
                entity_type: GenEntityType::Light,
            },
        ))
        .id();
    registry.insert("main_light".into(), light);
}

/// Load the initial scene file if provided.
fn load_initial_scene(
    initial_scene: Res<GenInitialScene>,
    asset_server: Res<AssetServer>,
    mut pending: ResMut<PendingGltfLoads>,
) {
    let Some(ref path) = initial_scene.path else {
        return;
    };

    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "scene".to_string());

    let asset_path = path.to_string_lossy().trim_start_matches('/').to_string();
    let handle = asset_server.load::<Scene>(format!("{}#Scene0", asset_path));

    pending.queue.push(PendingGltfLoad {
        handle,
        name,
        path: path.to_string_lossy().into_owned(),
        send_response: false,
    });
}

/// Poll the command channel each frame and dispatch.
#[allow(clippy::too_many_arguments)]
fn process_gen_commands(
    mut channel_res: ResMut<GenChannelRes>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<NameRegistry>,
    mut pending_screenshots: ResMut<PendingScreenshots>,
    mut pending_gltf: ResMut<PendingGltfLoads>,
    mut audio_engine: ResMut<audio::AudioEngine>,
    asset_server: Res<AssetServer>,
    workspace: Res<GenWorkspace>,
    transforms: Query<&Transform>,
    gen_entities: Query<&GenEntity>,
    names_query: Query<&Name>,
    children_query: Query<&Children>,
    parent_query: Query<&ChildOf>,
    visibility_query: Query<&Visibility>,
    material_handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mesh_handles: Query<&Mesh3d>,
) {
    while let Ok(cmd) = channel_res.channels.cmd_rx.try_recv() {
        let response = match cmd {
            GenCommand::SceneInfo => handle_scene_info(
                &registry,
                &transforms,
                &gen_entities,
                &material_handles,
                &materials,
            ),
            GenCommand::EntityInfo { name } => handle_entity_info(
                &name,
                &registry,
                &transforms,
                &gen_entities,
                &names_query,
                &children_query,
                &parent_query,
                &visibility_query,
                &material_handles,
                &materials,
            ),
            GenCommand::Screenshot {
                width,
                height,
                wait_frames,
            } => {
                pending_screenshots.queue.push(PendingScreenshot {
                    frames_remaining: wait_frames,
                    width,
                    height,
                    path: None,
                });
                // Response will be sent by process_pending_screenshots
                continue;
            }
            GenCommand::SpawnPrimitive(cmd) => handle_spawn_primitive(
                cmd,
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut registry,
            ),
            GenCommand::ModifyEntity(cmd) => handle_modify_entity(
                cmd,
                &mut commands,
                &registry,
                &mut materials,
                &material_handles,
                &transforms,
            ),
            GenCommand::DeleteEntity { name } => {
                handle_delete_entity(&name, &mut commands, &mut registry)
            }
            GenCommand::SetCamera(cmd) => handle_set_camera(cmd, &mut commands, &registry),
            GenCommand::SetLight(cmd) => handle_set_light(cmd, &mut commands, &mut registry),
            GenCommand::SetEnvironment(cmd) => handle_set_environment(cmd, &mut commands),
            GenCommand::SpawnMesh(cmd) => handle_spawn_mesh(
                cmd,
                &mut commands,
                &mut meshes,
                &mut materials,
                &mut registry,
            ),
            GenCommand::ExportScreenshot {
                path,
                width,
                height,
            } => {
                pending_screenshots.queue.push(PendingScreenshot {
                    frames_remaining: 3,
                    width,
                    height,
                    path: Some(path),
                });
                continue;
            }
            GenCommand::ExportGltf { path } => handle_export_gltf(
                path.as_deref(),
                &workspace,
                &registry,
                &transforms,
                &gen_entities,
                &parent_query,
                &material_handles,
                &materials,
                &mesh_handles,
                &meshes,
            ),
            GenCommand::LoadGltf { path } => {
                if let Some(resolved) = resolve_gltf_path(&path, &workspace.path) {
                    let name = resolved
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "imported_scene".to_string());

                    let asset_path = resolved
                        .to_string_lossy()
                        .trim_start_matches('/')
                        .to_string();
                    let handle = asset_server.load::<Scene>(format!("{}#Scene0", asset_path));

                    pending_gltf.queue.push(PendingGltfLoad {
                        handle,
                        name,
                        path: resolved.to_string_lossy().into_owned(),
                        send_response: true,
                    });
                } else {
                    let response = GenResponse::Error {
                        message: format!("Failed to resolve path: {}", path),
                    };
                    let _ = channel_res.channels.resp_tx.send(response);
                }
                // Response for successful loads is sent by process_pending_gltf_loads
                continue;
            }

            // Audio commands
            GenCommand::SetAmbience(cmd) => audio::handle_set_ambience(cmd, &mut audio_engine),
            GenCommand::SpawnAudioEmitter(cmd) => {
                audio::handle_spawn_audio_emitter(cmd, &mut audio_engine, &mut commands, &registry)
            }
            GenCommand::ModifyAudioEmitter(cmd) => {
                audio::handle_modify_audio_emitter(cmd, &mut audio_engine)
            }
            GenCommand::RemoveAudioEmitter { name } => {
                audio::handle_remove_audio_emitter(&name, &mut audio_engine)
            }
            GenCommand::AudioInfo => audio::handle_audio_info(&audio_engine),
        };

        let _ = channel_res.channels.resp_tx.send(response);
    }
}

/// Process pending screenshots that need frame delays.
fn process_pending_screenshots(
    channel_res: ResMut<GenChannelRes>,
    mut pending: ResMut<PendingScreenshots>,
) {
    let mut completed = Vec::new();

    for (i, screenshot) in pending.queue.iter_mut().enumerate() {
        if screenshot.frames_remaining > 0 {
            screenshot.frames_remaining -= 1;
        } else {
            completed.push(i);
        }
    }

    // Process completed screenshots in reverse order to preserve indices
    for i in completed.into_iter().rev() {
        let screenshot = pending.queue.remove(i);

        // Determine output path
        let path = screenshot.path.unwrap_or_else(|| {
            let tmp = std::env::temp_dir().join(format!(
                "localgpt_gen_screenshot_{}.png",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ));
            tmp.to_string_lossy().into_owned()
        });

        // TODO: Actual Bevy screenshot capture requires camera entity access
        // and render-to-texture. For now, we create a placeholder and report
        // the path. Full implementation needs Bevy's Screenshot observer or
        // render target approach.
        //
        // In a full implementation:
        //   commands.entity(camera).trigger(Screenshot::to_disk(path));
        let response = GenResponse::Screenshot {
            image_path: path.clone(),
        };
        let _ = channel_res.channels.resp_tx.send(response);
    }
}

/// Process pending glTF loads that are waiting for the asset server.
fn process_pending_gltf_loads(
    channel_res: Res<GenChannelRes>,
    asset_server: Res<AssetServer>,
    mut pending: ResMut<PendingGltfLoads>,
    mut commands: Commands,
    mut registry: ResMut<NameRegistry>,
) {
    let mut completed = Vec::new();

    for (i, load) in pending.queue.iter().enumerate() {
        if asset_server.is_loaded_with_dependencies(&load.handle) {
            completed.push(i);
        }
    }

    // Process completed loads in reverse order to preserve indices
    for i in completed.into_iter().rev() {
        let load = pending.queue.remove(i);

        // Spawn the scene
        let entity = commands.spawn(SceneRoot(load.handle.clone())).id();

        // Register in the name registry
        registry.insert(load.name.clone(), entity);

        // Send response if this was a tool request (not a startup load)
        if load.send_response {
            let response = GenResponse::GltfLoaded {
                name: load.name,
                path: load.path,
            };
            let _ = channel_res.channels.resp_tx.send(response);
        }
    }
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

fn handle_scene_info(
    registry: &NameRegistry,
    transforms: &Query<&Transform>,
    gen_entities: &Query<&GenEntity>,
    material_handles: &Query<&MeshMaterial3d<StandardMaterial>>,
    material_assets: &Assets<StandardMaterial>,
) -> GenResponse {
    let mut entities = Vec::new();

    for (name, entity) in registry.all_names() {
        let position = transforms
            .get(entity)
            .map(|t| t.translation.to_array())
            .unwrap_or_default();
        let scale = transforms
            .get(entity)
            .map(|t| t.scale.to_array())
            .unwrap_or([1.0, 1.0, 1.0]);
        let entity_type = gen_entities
            .get(entity)
            .map(|g| g.entity_type.as_str().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let color = material_handles
            .get(entity)
            .ok()
            .and_then(|h| material_assets.get(&h.0))
            .map(|mat| {
                let c = mat.base_color.to_srgba();
                [c.red, c.green, c.blue, c.alpha]
            });

        entities.push(EntitySummary {
            name: name.to_string(),
            entity_type,
            position,
            scale,
            color,
        });
    }

    GenResponse::SceneInfo(SceneInfoData {
        entity_count: entities.len(),
        entities,
    })
}

#[allow(clippy::too_many_arguments)]
fn handle_entity_info(
    name: &str,
    registry: &NameRegistry,
    transforms: &Query<&Transform>,
    gen_entities: &Query<&GenEntity>,
    names_query: &Query<&Name>,
    children_query: &Query<&Children>,
    parent_query: &Query<&ChildOf>,
    visibility_query: &Query<&Visibility>,
    material_handles: &Query<&MeshMaterial3d<StandardMaterial>>,
    material_assets: &Assets<StandardMaterial>,
) -> GenResponse {
    let Some(entity) = registry.get_entity(name) else {
        return GenResponse::Error {
            message: format!("Entity '{}' not found", name),
        };
    };

    let transform = transforms.get(entity).copied().unwrap_or_default();
    let euler = transform.rotation.to_euler(EulerRot::XYZ);

    let entity_type = gen_entities
        .get(entity)
        .map(|g| g.entity_type.as_str().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let visible = visibility_query
        .get(entity)
        .map(|v| *v != Visibility::Hidden)
        .unwrap_or(true);

    let (color, metallic, roughness) = material_handles
        .get(entity)
        .ok()
        .and_then(|h| material_assets.get(&h.0))
        .map(|mat| {
            let c = mat.base_color.to_srgba();
            (
                Some([c.red, c.green, c.blue, c.alpha]),
                Some(mat.metallic),
                Some(mat.perceptual_roughness),
            )
        })
        .unwrap_or((None, None, None));

    let children: Vec<String> = children_query
        .get(entity)
        .map(|ch| {
            ch.iter()
                .filter_map(|c| {
                    registry
                        .get_name(c)
                        .map(|s| s.to_string())
                        .or_else(|| names_query.get(c).ok().map(|n| n.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    let parent = parent_query
        .get(entity)
        .ok()
        .and_then(|p| registry.get_name(p.parent()).map(|s| s.to_string()));

    GenResponse::EntityInfo(EntityInfoData {
        name: name.to_string(),
        entity_id: entity.to_bits(),
        entity_type,
        position: transform.translation.to_array(),
        rotation_degrees: [
            euler.0.to_degrees(),
            euler.1.to_degrees(),
            euler.2.to_degrees(),
        ],
        scale: transform.scale.to_array(),
        color,
        metallic,
        roughness,
        visible,
        children,
        parent,
    })
}

fn handle_spawn_primitive(
    cmd: SpawnPrimitiveCmd,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<NameRegistry>,
) -> GenResponse {
    if registry.contains_name(&cmd.name) {
        return GenResponse::Error {
            message: format!("Entity '{}' already exists", cmd.name),
        };
    }

    let mesh = match cmd.shape {
        PrimitiveShape::Cuboid => {
            let x = cmd.dimensions.get("x").copied().unwrap_or(1.0);
            let y = cmd.dimensions.get("y").copied().unwrap_or(1.0);
            let z = cmd.dimensions.get("z").copied().unwrap_or(1.0);
            meshes.add(Cuboid::new(x, y, z))
        }
        PrimitiveShape::Sphere => {
            let radius = cmd.dimensions.get("radius").copied().unwrap_or(0.5);
            meshes.add(Sphere::new(radius).mesh().uv(32, 18))
        }
        PrimitiveShape::Cylinder => {
            let radius = cmd.dimensions.get("radius").copied().unwrap_or(0.5);
            let height = cmd.dimensions.get("height").copied().unwrap_or(1.0);
            meshes.add(Cylinder::new(radius, height))
        }
        PrimitiveShape::Cone => {
            let radius = cmd.dimensions.get("radius").copied().unwrap_or(0.5);
            let height = cmd.dimensions.get("height").copied().unwrap_or(1.0);
            meshes.add(Cone { radius, height })
        }
        PrimitiveShape::Capsule => {
            let radius = cmd.dimensions.get("radius").copied().unwrap_or(0.5);
            let half_length = cmd.dimensions.get("half_length").copied().unwrap_or(0.5);
            meshes.add(Capsule3d::new(radius, half_length * 2.0))
        }
        PrimitiveShape::Torus => {
            let major = cmd.dimensions.get("major_radius").copied().unwrap_or(1.0);
            let minor = cmd.dimensions.get("minor_radius").copied().unwrap_or(0.25);
            meshes.add(Torus::new(minor, major))
        }
        PrimitiveShape::Plane => {
            let x = cmd.dimensions.get("x").copied().unwrap_or(1.0);
            let z = cmd.dimensions.get("z").copied().unwrap_or(1.0);
            meshes.add(Plane3d::new(Vec3::Y, Vec2::new(x / 2.0, z / 2.0)))
        }
    };

    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(cmd.color[0], cmd.color[1], cmd.color[2], cmd.color[3]),
        metallic: cmd.metallic,
        perceptual_roughness: cmd.roughness,
        emissive: bevy::color::LinearRgba::new(
            cmd.emissive[0],
            cmd.emissive[1],
            cmd.emissive[2],
            cmd.emissive[3],
        ),
        ..default()
    });

    let rotation = Quat::from_euler(
        EulerRot::XYZ,
        cmd.rotation_degrees[0].to_radians(),
        cmd.rotation_degrees[1].to_radians(),
        cmd.rotation_degrees[2].to_radians(),
    );

    let transform = Transform {
        translation: Vec3::from_array(cmd.position),
        rotation,
        scale: Vec3::from_array(cmd.scale),
    };

    let entity = commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            Name::new(cmd.name.clone()),
            GenEntity {
                entity_type: GenEntityType::Primitive,
            },
        ))
        .id();

    // Handle parenting
    if let Some(ref parent_name) = cmd.parent
        && let Some(parent_entity) = registry.get_entity(parent_name)
    {
        commands.entity(entity).set_parent_in_place(parent_entity);
    }

    let entity_id = entity.to_bits();
    registry.insert(cmd.name.clone(), entity);

    GenResponse::Spawned {
        name: cmd.name,
        entity_id,
    }
}

fn handle_modify_entity(
    cmd: ModifyEntityCmd,
    commands: &mut Commands,
    registry: &NameRegistry,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    material_handles: &Query<&MeshMaterial3d<StandardMaterial>>,
    transforms: &Query<&Transform>,
) -> GenResponse {
    let Some(entity) = registry.get_entity(&cmd.name) else {
        return GenResponse::Error {
            message: format!("Entity '{}' not found", cmd.name),
        };
    };

    let mut entity_commands = commands.entity(entity);

    // Update transform
    if cmd.position.is_some() || cmd.rotation_degrees.is_some() || cmd.scale.is_some() {
        let current = transforms.get(entity).copied().unwrap_or_default();
        let new_transform = Transform {
            translation: cmd
                .position
                .map(Vec3::from_array)
                .unwrap_or(current.translation),
            rotation: cmd
                .rotation_degrees
                .map(|r| {
                    Quat::from_euler(
                        EulerRot::XYZ,
                        r[0].to_radians(),
                        r[1].to_radians(),
                        r[2].to_radians(),
                    )
                })
                .unwrap_or(current.rotation),
            scale: cmd.scale.map(Vec3::from_array).unwrap_or(current.scale),
        };
        entity_commands.insert(new_transform);
    }

    // Update material if any material properties changed
    if cmd.color.is_some()
        || cmd.metallic.is_some()
        || cmd.roughness.is_some()
        || cmd.emissive.is_some()
    {
        // Get current material properties as defaults
        let current_mat = material_handles
            .get(entity)
            .ok()
            .and_then(|h| materials.get(&h.0))
            .cloned();

        let base = current_mat.unwrap_or_default();

        let new_material = materials.add(StandardMaterial {
            base_color: cmd
                .color
                .map(|c| Color::srgba(c[0], c[1], c[2], c[3]))
                .unwrap_or(base.base_color),
            metallic: cmd.metallic.unwrap_or(base.metallic),
            perceptual_roughness: cmd.roughness.unwrap_or(base.perceptual_roughness),
            emissive: cmd
                .emissive
                .map(|e| bevy::color::LinearRgba::new(e[0], e[1], e[2], e[3]))
                .unwrap_or(base.emissive),
            ..base
        });
        entity_commands.insert(MeshMaterial3d(new_material));
    }

    // Update visibility
    if let Some(visible) = cmd.visible {
        entity_commands.insert(if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }

    // Update parent
    if let Some(parent_opt) = cmd.parent {
        match parent_opt {
            Some(parent_name) => {
                if let Some(parent_entity) = registry.get_entity(&parent_name) {
                    commands.entity(entity).set_parent_in_place(parent_entity);
                }
            }
            None => {
                commands.entity(entity).remove_parent_in_place();
            }
        }
    }

    GenResponse::Modified { name: cmd.name }
}

fn handle_delete_entity(
    name: &str,
    commands: &mut Commands,
    registry: &mut ResMut<NameRegistry>,
) -> GenResponse {
    let Some(entity) = registry.remove_by_name(name) else {
        return GenResponse::Error {
            message: format!("Entity '{}' not found", name),
        };
    };

    commands.entity(entity).despawn();

    GenResponse::Deleted {
        name: name.to_string(),
    }
}

fn handle_set_camera(
    cmd: CameraCmd,
    commands: &mut Commands,
    registry: &NameRegistry,
) -> GenResponse {
    let Some(camera_entity) = registry.get_entity("main_camera") else {
        return GenResponse::Error {
            message: "main_camera not found in registry".to_string(),
        };
    };

    let transform = Transform::from_translation(Vec3::from_array(cmd.position))
        .looking_at(Vec3::from_array(cmd.look_at), Vec3::Y);

    commands.entity(camera_entity).insert(transform);

    // Update projection FOV
    let projection = Projection::Perspective(PerspectiveProjection {
        fov: cmd.fov_degrees.to_radians(),
        ..default()
    });
    commands.entity(camera_entity).insert(projection);

    GenResponse::CameraSet
}

fn handle_set_light(
    cmd: SetLightCmd,
    commands: &mut Commands,
    registry: &mut ResMut<NameRegistry>,
) -> GenResponse {
    let color = Color::srgba(cmd.color[0], cmd.color[1], cmd.color[2], cmd.color[3]);

    // If light already exists, update it
    if let Some(entity) = registry.get_entity(&cmd.name) {
        commands.entity(entity).despawn();
        registry.remove_by_name(&cmd.name);
    }

    let entity = match cmd.light_type {
        LightType::Directional => {
            let dir = cmd.direction.unwrap_or([0.0, -1.0, -0.5]);
            let transform = Transform::from_translation(Vec3::new(0.0, 10.0, 0.0))
                .looking_at(Vec3::new(0.0, 10.0, 0.0) + Vec3::from_array(dir), Vec3::Y);
            commands
                .spawn((
                    DirectionalLight {
                        illuminance: cmd.intensity,
                        shadows_enabled: cmd.shadows,
                        color,
                        ..default()
                    },
                    transform,
                    Name::new(cmd.name.clone()),
                    GenEntity {
                        entity_type: GenEntityType::Light,
                    },
                ))
                .id()
        }
        LightType::Point => {
            let pos = cmd.position.unwrap_or([0.0, 5.0, 0.0]);
            commands
                .spawn((
                    PointLight {
                        intensity: cmd.intensity,
                        shadows_enabled: cmd.shadows,
                        color,
                        ..default()
                    },
                    Transform::from_translation(Vec3::from_array(pos)),
                    Name::new(cmd.name.clone()),
                    GenEntity {
                        entity_type: GenEntityType::Light,
                    },
                ))
                .id()
        }
        LightType::Spot => {
            let pos = cmd.position.unwrap_or([0.0, 5.0, 0.0]);
            let dir = cmd.direction.unwrap_or([0.0, -1.0, 0.0]);
            let transform = Transform::from_translation(Vec3::from_array(pos))
                .looking_at(Vec3::from_array(pos) + Vec3::from_array(dir), Vec3::Y);
            commands
                .spawn((
                    SpotLight {
                        intensity: cmd.intensity,
                        shadows_enabled: cmd.shadows,
                        color,
                        ..default()
                    },
                    transform,
                    Name::new(cmd.name.clone()),
                    GenEntity {
                        entity_type: GenEntityType::Light,
                    },
                ))
                .id()
        }
    };

    registry.insert(cmd.name.clone(), entity);

    GenResponse::LightSet { name: cmd.name }
}

fn handle_set_environment(cmd: EnvironmentCmd, commands: &mut Commands) -> GenResponse {
    if let Some(color) = cmd.background_color {
        commands.insert_resource(ClearColor(Color::srgba(
            color[0], color[1], color[2], color[3],
        )));
    }

    if let Some(intensity) = cmd.ambient_light {
        let color = cmd
            .ambient_color
            .map(|c| Color::srgba(c[0], c[1], c[2], c[3]))
            .unwrap_or(Color::WHITE);
        commands.insert_resource(GlobalAmbientLight {
            color,
            brightness: intensity,
            affects_lightmapped_meshes: true,
        });
    }

    GenResponse::EnvironmentSet
}

fn handle_spawn_mesh(
    cmd: RawMeshCmd,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<NameRegistry>,
) -> GenResponse {
    if registry.contains_name(&cmd.name) {
        return GenResponse::Error {
            message: format!("Entity '{}' already exists", cmd.name),
        };
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );

    // Positions
    let positions: Vec<[f32; 3]> = cmd.vertices.clone();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);

    // Indices
    mesh.insert_indices(Indices::U32(cmd.indices));

    // Normals — use provided or compute flat normals
    if let Some(normals) = cmd.normals {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    } else {
        mesh.compute_flat_normals();
    }

    // UVs
    if let Some(uvs) = cmd.uvs {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }

    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(cmd.color[0], cmd.color[1], cmd.color[2], cmd.color[3]),
        metallic: cmd.metallic,
        perceptual_roughness: cmd.roughness,
        ..default()
    });

    let entity = commands
        .spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform::from_translation(Vec3::from_array(cmd.position)),
            Name::new(cmd.name.clone()),
            GenEntity {
                entity_type: GenEntityType::Mesh,
            },
        ))
        .id();

    let entity_id = entity.to_bits();
    registry.insert(cmd.name.clone(), entity);

    GenResponse::Spawned {
        name: cmd.name,
        entity_id,
    }
}

// ---------------------------------------------------------------------------
// glTF/GLB export
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_export_gltf(
    path: Option<&str>,
    workspace: &GenWorkspace,
    registry: &NameRegistry,
    transforms: &Query<&Transform>,
    gen_entities: &Query<&GenEntity>,
    parent_query: &Query<&ChildOf>,
    material_handles: &Query<&MeshMaterial3d<StandardMaterial>>,
    material_assets: &Assets<StandardMaterial>,
    mesh_handles: &Query<&Mesh3d>,
    mesh_assets: &Assets<Mesh>,
) -> GenResponse {
    use gltf_json::validation::Checked::Valid;
    use gltf_json::validation::USize64;

    // Resolve output path: use provided path or default to {workspace}/exports/{timestamp}.glb
    let output_path = match path {
        Some(p) if !p.is_empty() => {
            // Ensure path has .glb/.gltf extension
            if std::path::Path::new(p).extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("glb") || ext.eq_ignore_ascii_case("gltf")
            }) {
                p.to_string()
            } else {
                format!("{}.glb", p)
            }
        }
        _ => {
            // Default: {workspace}/exports/{timestamp}.glb
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let exports_dir = workspace.path.join("exports");
            exports_dir
                .join(format!("{}.glb", timestamp))
                .to_string_lossy()
                .into_owned()
        }
    };

    let mut root = gltf_json::Root::default();
    let mut bin_data: Vec<u8> = Vec::new();

    // Collect exportable entities (those with GenEntity + Mesh3d + material + transform)
    // Map entity → node index
    let mut entity_to_node: std::collections::HashMap<Entity, u32> =
        std::collections::HashMap::new();

    // First pass: create nodes + meshes for all exportable entities
    for (name, entity) in registry.all_names() {
        // Skip non-mesh entities (cameras, lights)
        let Ok(gen_ent) = gen_entities.get(entity) else {
            continue;
        };
        match gen_ent.entity_type {
            GenEntityType::Primitive | GenEntityType::Mesh => {}
            _ => continue,
        }

        // Must have a mesh handle
        let Ok(mesh3d) = mesh_handles.get(entity) else {
            continue;
        };
        let Some(mesh) = mesh_assets.get(&mesh3d.0) else {
            continue;
        };

        // Extract mesh data
        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
            _ => continue,
        };
        if positions.is_empty() {
            continue;
        }

        let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(v)) => Some(v.clone()),
            _ => None,
        };

        let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(v)) => Some(v.clone()),
            _ => None,
        };

        let indices: Option<Vec<u32>> = mesh.indices().map(|idx| match idx {
            Indices::U16(v) => v.iter().map(|i| *i as u32).collect(),
            Indices::U32(v) => v.clone(),
        });

        // Compute bounding box for positions accessor
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for p in &positions {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }

        // --- Write position data to buffer ---
        let pos_offset = bin_data.len();
        for p in &positions {
            for &v in p {
                bin_data.extend_from_slice(&v.to_le_bytes());
            }
        }
        let pos_length = bin_data.len() - pos_offset;

        // Pad to 4-byte alignment
        while !bin_data.len().is_multiple_of(4) {
            bin_data.push(0);
        }

        let pos_view_idx = root.buffer_views.len() as u32;
        root.buffer_views.push(gltf_json::buffer::View {
            buffer: gltf_json::Index::new(0),
            byte_offset: Some(USize64(pos_offset as u64)),
            byte_length: USize64(pos_length as u64),
            byte_stride: None,
            target: Some(Valid(gltf_json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        let pos_accessor_idx = root.accessors.len() as u32;
        root.accessors.push(gltf_json::Accessor {
            buffer_view: Some(gltf_json::Index::new(pos_view_idx)),
            byte_offset: None,
            count: USize64(positions.len() as u64),
            component_type: Valid(gltf_json::accessor::GenericComponentType(
                gltf_json::accessor::ComponentType::F32,
            )),
            type_: Valid(gltf_json::accessor::Type::Vec3),
            min: Some(gltf_json::Value::from(vec![
                serde_json::Number::from_f64(min[0] as f64).unwrap_or(serde_json::Number::from(0)),
                serde_json::Number::from_f64(min[1] as f64).unwrap_or(serde_json::Number::from(0)),
                serde_json::Number::from_f64(min[2] as f64).unwrap_or(serde_json::Number::from(0)),
            ])),
            max: Some(gltf_json::Value::from(vec![
                serde_json::Number::from_f64(max[0] as f64).unwrap_or(serde_json::Number::from(0)),
                serde_json::Number::from_f64(max[1] as f64).unwrap_or(serde_json::Number::from(0)),
                serde_json::Number::from_f64(max[2] as f64).unwrap_or(serde_json::Number::from(0)),
            ])),
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        // --- Normals ---
        let normal_accessor_idx = if let Some(ref normals) = normals {
            let offset = bin_data.len();
            for n in normals {
                for &v in n {
                    bin_data.extend_from_slice(&v.to_le_bytes());
                }
            }
            let length = bin_data.len() - offset;
            while !bin_data.len().is_multiple_of(4) {
                bin_data.push(0);
            }

            let view_idx = root.buffer_views.len() as u32;
            root.buffer_views.push(gltf_json::buffer::View {
                buffer: gltf_json::Index::new(0),
                byte_offset: Some(USize64(offset as u64)),
                byte_length: USize64(length as u64),
                byte_stride: None,
                target: Some(Valid(gltf_json::buffer::Target::ArrayBuffer)),
                name: None,
                extensions: Default::default(),
                extras: Default::default(),
            });

            let acc_idx = root.accessors.len() as u32;
            root.accessors.push(gltf_json::Accessor {
                buffer_view: Some(gltf_json::Index::new(view_idx)),
                byte_offset: None,
                count: USize64(normals.len() as u64),
                component_type: Valid(gltf_json::accessor::GenericComponentType(
                    gltf_json::accessor::ComponentType::F32,
                )),
                type_: Valid(gltf_json::accessor::Type::Vec3),
                min: None,
                max: None,
                name: None,
                normalized: false,
                sparse: None,
                extensions: Default::default(),
                extras: Default::default(),
            });
            Some(acc_idx)
        } else {
            None
        };

        // --- UVs ---
        let uv_accessor_idx = if let Some(ref uvs) = uvs {
            let offset = bin_data.len();
            for uv in uvs {
                for &v in uv {
                    bin_data.extend_from_slice(&v.to_le_bytes());
                }
            }
            let length = bin_data.len() - offset;
            while !bin_data.len().is_multiple_of(4) {
                bin_data.push(0);
            }

            let view_idx = root.buffer_views.len() as u32;
            root.buffer_views.push(gltf_json::buffer::View {
                buffer: gltf_json::Index::new(0),
                byte_offset: Some(USize64(offset as u64)),
                byte_length: USize64(length as u64),
                byte_stride: None,
                target: Some(Valid(gltf_json::buffer::Target::ArrayBuffer)),
                name: None,
                extensions: Default::default(),
                extras: Default::default(),
            });

            let acc_idx = root.accessors.len() as u32;
            root.accessors.push(gltf_json::Accessor {
                buffer_view: Some(gltf_json::Index::new(view_idx)),
                byte_offset: None,
                count: USize64(uvs.len() as u64),
                component_type: Valid(gltf_json::accessor::GenericComponentType(
                    gltf_json::accessor::ComponentType::F32,
                )),
                type_: Valid(gltf_json::accessor::Type::Vec2),
                min: None,
                max: None,
                name: None,
                normalized: false,
                sparse: None,
                extensions: Default::default(),
                extras: Default::default(),
            });
            Some(acc_idx)
        } else {
            None
        };

        // --- Indices ---
        let index_accessor_idx = if let Some(ref indices) = indices {
            let offset = bin_data.len();
            for &idx in indices {
                bin_data.extend_from_slice(&idx.to_le_bytes());
            }
            let length = bin_data.len() - offset;
            while !bin_data.len().is_multiple_of(4) {
                bin_data.push(0);
            }

            let view_idx = root.buffer_views.len() as u32;
            root.buffer_views.push(gltf_json::buffer::View {
                buffer: gltf_json::Index::new(0),
                byte_offset: Some(USize64(offset as u64)),
                byte_length: USize64(length as u64),
                byte_stride: None,
                target: Some(Valid(gltf_json::buffer::Target::ElementArrayBuffer)),
                name: None,
                extensions: Default::default(),
                extras: Default::default(),
            });

            let acc_idx = root.accessors.len() as u32;
            root.accessors.push(gltf_json::Accessor {
                buffer_view: Some(gltf_json::Index::new(view_idx)),
                byte_offset: None,
                count: USize64(indices.len() as u64),
                component_type: Valid(gltf_json::accessor::GenericComponentType(
                    gltf_json::accessor::ComponentType::U32,
                )),
                type_: Valid(gltf_json::accessor::Type::Scalar),
                min: None,
                max: None,
                name: None,
                normalized: false,
                sparse: None,
                extensions: Default::default(),
                extras: Default::default(),
            });
            Some(acc_idx)
        } else {
            None
        };

        // --- Material ---
        let material_idx = {
            let (base_color, metallic, roughness) = material_handles
                .get(entity)
                .ok()
                .and_then(|h| material_assets.get(&h.0))
                .map(|mat| {
                    let c = mat.base_color.to_srgba();
                    (
                        [c.red, c.green, c.blue, c.alpha],
                        mat.metallic,
                        mat.perceptual_roughness,
                    )
                })
                .unwrap_or(([0.8, 0.8, 0.8, 1.0], 0.0, 0.5));

            let mat_idx = root.materials.len() as u32;
            root.materials.push(gltf_json::Material {
                name: Some(format!("{}_material", name)),
                pbr_metallic_roughness: gltf_json::material::PbrMetallicRoughness {
                    base_color_factor: gltf_json::material::PbrBaseColorFactor(base_color),
                    metallic_factor: gltf_json::material::StrengthFactor(metallic),
                    roughness_factor: gltf_json::material::StrengthFactor(roughness),
                    base_color_texture: None,
                    metallic_roughness_texture: None,
                    extensions: Default::default(),
                    extras: Default::default(),
                },
                alpha_cutoff: None,
                alpha_mode: Valid(gltf_json::material::AlphaMode::Opaque),
                double_sided: false,
                normal_texture: None,
                occlusion_texture: None,
                emissive_texture: None,
                emissive_factor: gltf_json::material::EmissiveFactor([0.0, 0.0, 0.0]),
                extensions: Default::default(),
                extras: Default::default(),
            });
            mat_idx
        };

        // --- Mesh primitive ---
        let mut attributes = std::collections::BTreeMap::new();
        attributes.insert(
            Valid(gltf_json::mesh::Semantic::Positions),
            gltf_json::Index::new(pos_accessor_idx),
        );
        if let Some(idx) = normal_accessor_idx {
            attributes.insert(
                Valid(gltf_json::mesh::Semantic::Normals),
                gltf_json::Index::new(idx),
            );
        }
        if let Some(idx) = uv_accessor_idx {
            attributes.insert(
                Valid(gltf_json::mesh::Semantic::TexCoords(0)),
                gltf_json::Index::new(idx),
            );
        }

        let mesh_idx = root.meshes.len() as u32;
        root.meshes.push(gltf_json::Mesh {
            name: Some(format!("{}_mesh", name)),
            primitives: vec![gltf_json::mesh::Primitive {
                attributes,
                indices: index_accessor_idx.map(gltf_json::Index::new),
                material: Some(gltf_json::Index::new(material_idx)),
                mode: Valid(gltf_json::mesh::Mode::Triangles),
                targets: None,
                extensions: Default::default(),
                extras: Default::default(),
            }],
            weights: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        // --- Node ---
        let transform = transforms.get(entity).copied().unwrap_or_default();
        let (axis, angle) = transform.rotation.to_axis_angle();
        let quat = if angle.abs() < f32::EPSILON {
            gltf_json::scene::UnitQuaternion([0.0, 0.0, 0.0, 1.0])
        } else {
            let q = Quat::from_axis_angle(axis, angle);
            gltf_json::scene::UnitQuaternion([q.x, q.y, q.z, q.w])
        };

        let node_idx = root.nodes.len() as u32;
        root.nodes.push(gltf_json::Node {
            name: Some(name.to_string()),
            mesh: Some(gltf_json::Index::new(mesh_idx)),
            translation: Some(transform.translation.to_array()),
            rotation: Some(quat),
            scale: Some(transform.scale.to_array()),
            camera: None,
            children: None,
            skin: None,
            matrix: None,
            weights: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        entity_to_node.insert(entity, node_idx);
    }

    if entity_to_node.is_empty() {
        return GenResponse::Error {
            message: "No exportable entities in scene (need entities with meshes)".to_string(),
        };
    }

    // Second pass: set up parent-child hierarchy
    let mut root_nodes = Vec::new();
    for (name, entity) in registry.all_names() {
        let Some(&node_idx) = entity_to_node.get(&entity) else {
            continue;
        };

        let parent_entity = parent_query.get(entity).ok().map(|p| p.parent());
        let parent_is_gen = parent_entity
            .and_then(|pe| entity_to_node.get(&pe))
            .copied();

        if let Some(parent_node_idx) = parent_is_gen {
            // Add as child of parent node
            let parent_node = &mut root.nodes[parent_node_idx as usize];
            let children = parent_node.children.get_or_insert_with(Vec::new);
            children.push(gltf_json::Index::new(node_idx));
        } else {
            root_nodes.push(gltf_json::Index::new(node_idx));
        }

        // Suppress unused variable warning
        let _ = name;
    }

    // Scene
    root.scenes.push(gltf_json::Scene {
        name: Some("Scene".to_string()),
        nodes: root_nodes,
        extensions: Default::default(),
        extras: Default::default(),
    });
    root.scene = Some(gltf_json::Index::new(0));

    // Buffer (will be embedded in GLB)
    root.buffers.push(gltf_json::Buffer {
        byte_length: USize64(bin_data.len() as u64),
        uri: None,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    // Serialize JSON
    let json_string = match serde_json::to_string(&root) {
        Ok(s) => s,
        Err(e) => {
            return GenResponse::Error {
                message: format!("Failed to serialize glTF JSON: {}", e),
            };
        }
    };
    let mut json_bytes = json_string.into_bytes();
    // Pad JSON to 4-byte alignment with spaces
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }

    // Pad binary buffer to 4-byte alignment with zeros
    while !bin_data.len().is_multiple_of(4) {
        bin_data.push(0);
    }

    // GLB: 12-byte header + JSON chunk (8+N) + BIN chunk (8+M)
    let total_length = 12 + 8 + json_bytes.len() + 8 + bin_data.len();

    let mut glb = Vec::with_capacity(total_length);

    // Header
    glb.extend_from_slice(b"glTF"); // magic
    glb.extend_from_slice(&2u32.to_le_bytes()); // version
    glb.extend_from_slice(&(total_length as u32).to_le_bytes());

    // JSON chunk
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4E4F534Au32.to_le_bytes()); // "JSON"
    glb.extend_from_slice(&json_bytes);

    // BIN chunk
    glb.extend_from_slice(&(bin_data.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004E4942u32.to_le_bytes()); // "BIN\0"
    glb.extend_from_slice(&bin_data);

    // Write to file
    if let Some(parent) = std::path::Path::new(&output_path).parent()
        && !parent.exists()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return GenResponse::Error {
            message: format!("Failed to create directory: {}", e),
        };
    }

    match std::fs::write(&output_path, &glb) {
        Ok(()) => GenResponse::Exported { path: output_path },
        Err(e) => GenResponse::Error {
            message: format!("Failed to write GLB file: {}", e),
        },
    }
}

// ---------------------------------------------------------------------------
// glTF path resolution
// ---------------------------------------------------------------------------

/// Resolve a glTF file path with the following fallback logic:
/// 1. Expand `~` and try as-is
/// 2. Try `{workspace}/{path}`
/// 3. Try `{workspace}/exports/{path}`
/// 4. Walk workspace directory tree looking for a file whose name matches the basename
/// 5. Return None if nothing found
pub fn resolve_gltf_path(path: &str, workspace: &PathBuf) -> Option<PathBuf> {
    // 1. Expand ~ and try as-is
    let expanded = shellexpand::tilde(path).into_owned();
    let p = std::path::Path::new(&expanded);
    if p.exists() {
        return p.canonicalize().ok();
    }

    // 2. {workspace}/{path}
    let wp = workspace.join(&expanded);
    if wp.exists() {
        return wp.canonicalize().ok();
    }

    // 3. {workspace}/exports/{path}
    let ep = workspace.join("exports").join(&expanded);
    if ep.exists() {
        return ep.canonicalize().ok();
    }

    // 4. Walk workspace for matching basename
    let needle = std::path::Path::new(&expanded).file_name()?;
    find_in_dir(workspace, needle)
}

fn find_in_dir(dir: &PathBuf, needle: &OsStr) -> Option<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_in_dir(&path, needle) {
                return Some(found);
            }
        } else if path.file_name() == Some(needle) {
            return path.canonicalize().ok();
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Fly camera systems
// ---------------------------------------------------------------------------

/// WASD + Space/Shift movement relative to camera orientation.
fn fly_cam_movement(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    config: Res<FlyCamConfig>,
    mut query: Query<&mut Transform, With<FlyCam>>,
) {
    let Ok(mut transform) = query.single_mut() else {
        return;
    };

    let forward = transform.forward().as_vec3();
    let right = transform.right().as_vec3();

    let mut velocity = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        velocity += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        velocity -= forward;
    }
    if keys.pressed(KeyCode::KeyA) {
        velocity -= right;
    }
    if keys.pressed(KeyCode::KeyD) {
        velocity += right;
    }
    if keys.pressed(KeyCode::Space) {
        velocity += Vec3::Y;
    }
    if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        velocity -= Vec3::Y;
    }

    if velocity != Vec3::ZERO {
        transform.translation += velocity.normalize() * config.move_speed * time.delta_secs();
    }
}

/// Right-click + mouse drag to rotate the camera (yaw and pitch).
fn fly_cam_look(
    mouse: Res<ButtonInput<MouseButton>>,
    config: Res<FlyCamConfig>,
    mut motion_reader: MessageReader<MouseMotion>,
    mut query: Query<&mut Transform, With<FlyCam>>,
) {
    let delta: Vec2 = motion_reader.read().map(|e| e.delta).sum();
    if delta == Vec2::ZERO || !mouse.pressed(MouseButton::Right) {
        return;
    }

    let Ok(mut transform) = query.single_mut() else {
        return;
    };

    let yaw = -delta.x * config.look_sensitivity;
    let pitch = -delta.y * config.look_sensitivity;

    // Apply yaw (rotate around global Y axis)
    transform.rotate_y(yaw);

    // Apply pitch (rotate around local X axis) with clamping
    let right = transform.right().as_vec3();
    let new_rotation = Quat::from_axis_angle(right, pitch) * transform.rotation;

    // Clamp pitch: check the angle between the camera's forward and the horizontal plane
    let new_forward = new_rotation * Vec3::NEG_Z;
    let pitch_angle = new_forward.y.asin();
    let max_pitch = 89.0_f32.to_radians();

    if pitch_angle.abs() < max_pitch {
        transform.rotation = new_rotation;
    }
}

/// Scroll wheel adjusts movement speed.
fn fly_cam_scroll_speed(
    mut scroll_reader: MessageReader<MouseWheel>,
    mut config: ResMut<FlyCamConfig>,
) {
    for event in scroll_reader.read() {
        config.move_speed = (config.move_speed * (1.0 + event.y * 0.1)).clamp(0.5, 100.0);
    }
}
