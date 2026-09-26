//! Bevy GenPlugin — command processing, default scene, screenshot capture, glTF loading.

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot};

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use localgpt_world_types as wt;

use super::GenChannels;
use super::audio::{self, SpatialAudioListener};
use super::avatar;
use super::behaviors::{self, BehaviorState, EntityBehaviors};
use super::commands::*;
use super::compat;
use super::registry::*;

/// Bevy resource holding the workspace path for default export locations.
#[derive(Resource, Clone)]
pub struct GenWorkspace {
    pub path: PathBuf,
}

/// Current world skill folder being edited (auto-created when user starts generating).
#[derive(Resource, Default)]
pub struct CurrentWorldSkill {
    /// Path to the world skill folder (e.g., workspace/skills/my-world/)
    pub path: Option<PathBuf>,
    /// Name of the world skill
    pub name: Option<String>,
}

impl CurrentWorldSkill {
    /// Get the screenshots folder path, creating it if necessary.
    pub fn screenshots_folder(&self) -> Option<PathBuf> {
        self.path.as_ref().map(|p| {
            let folder = p.join("screenshots");
            let _ = std::fs::create_dir_all(&folder);
            folder
        })
    }

    /// Generate a timestamped screenshot filename.
    pub fn screenshot_filename(&self) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let name = self.name.as_deref().unwrap_or("world");
        format!("{}-{}.png", name, timestamp)
    }
}

/// Marker resource indicating the world has unsaved changes.
#[derive(Resource, Default)]
#[allow(dead_code)]
pub struct WorldDirty {
    pub dirty: bool,
    /// Cooldown frames before auto-save triggers
    pub cooldown_frames: u32,
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
    /// Optional explicit path; if None, saves to world skill's screenshots folder.
    path: Option<PathBuf>,
    /// Whether to also save to the world skill's screenshots folder.
    save_to_skill: bool,
    /// Entity name to highlight with emissive override.
    highlight_entity: Option<String>,
    /// Highlight color [r, g, b, a].
    highlight_color: [f32; 4],
    /// Camera angle preset.
    camera_angle: ScreenshotCameraAngle,
    /// Whether to overlay entity annotations.
    include_annotations: bool,
}

/// Initial glTF scene to load at startup.
#[derive(Resource)]
pub struct GenInitialScene {
    pub path: Option<PathBuf>,
}

/// World folder to load on the first frame (`--world`; see
/// [`super::world_import`]).
#[derive(Resource, Default)]
pub struct GenInitialWorld {
    pub path: Option<String>,
}

/// Undo/redo stack wrapping `EditHistory` from world-types.
///
/// Records `WorldEdit` operations (spawn, delete, modify) as they happen.
/// `gen_undo` / `gen_redo` commands apply inverse operations to restore state.
///
/// Also owns the scene *revision*: a counter that only ever increases, bumped on
/// every recorded edit, undo, redo and history reset (world load, clear).
/// Batch tools accept an `expected_revision` so an agent can refuse to apply
/// edits planned against a scene that has since changed.
#[derive(Resource, Default)]
pub struct UndoStack {
    pub history: wt::EditHistory,
    revision: u64,
}

impl UndoStack {
    /// Current scene revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Record an undoable edit and advance the revision.
    pub fn record(&mut self, op: wt::EditOp, inverse: wt::EditOp, author: Option<String>) {
        self.history.push(op, inverse, author);
        self.revision += 1;
    }

    /// Replace the whole history (world load / clear) and advance the revision.
    pub fn reset(&mut self, history: wt::EditHistory) {
        self.history = history;
        self.revision += 1;
    }

    /// Next operation to undo, advancing the revision if there is one.
    pub fn undo(&mut self) -> Option<wt::EditOp> {
        let op = self.history.undo().cloned();
        if op.is_some() {
            self.revision += 1;
        }
        op
    }

    /// Next operation to redo, advancing the revision if there is one.
    pub fn redo(&mut self) -> Option<wt::EditOp> {
        let op = self.history.redo().cloned();
        if op.is_some() {
            self.revision += 1;
        }
        op
    }
}

/// A glTF scene that is currently being loaded.
struct PendingGltfLoad {
    handle: Handle<WorldAsset>,
    name: String,
    path: String,
    send_response: bool,
    /// WG6.3: Decompose loaded mesh into editable sub-objects.
    segment: bool,
}

/// Queue of pending glTF loads waiting for asset server to finish loading.
#[derive(Resource, Default)]
struct PendingGltfLoads {
    queue: Vec<PendingGltfLoad>,
}

/// Deferred world setup — applied after a world's glTF scene finishes spawning.
///
/// Placeholder resource retained for `handle_clear_scene` compatibility.
/// No longer populated now that the legacy TOML format has been removed.
#[derive(Resource, Default)]
struct PendingWorldSetup {
    active: Option<WorldSetupData>,
}

struct WorldSetupData {
    /// Entity-name → behavior definitions to apply.
    behaviors: Vec<(String, Vec<BehaviorDef>)>,
    /// Audio emitters that reference entities by name.
    emitters: Vec<AudioEmitterCmd>,
    /// How many frames we've been waiting (give up after a limit).
    frames_waited: u32,
}

/// Bevy resource storing guided tour definitions for the current world.
#[derive(Resource, Default)]
pub struct WorldTours {
    pub tours: Vec<TourDef>,
}

/// Bevy resource tracking the currently loaded world name (if any).
#[derive(Resource, Default)]
pub struct CurrentWorld {
    pub name: Option<String>,
    pub path: Option<PathBuf>,
}

/// Bevy resource storing reference images for image-guided world generation (AI3).
#[derive(Resource, Default)]
pub struct ReferenceBoard {
    pub entries: Vec<ReferenceEntry>,
    next_id: u32,
}

impl ReferenceBoard {
    /// Add a reference entry, returning the assigned ID.
    pub fn add(
        &mut self,
        label: String,
        weight: f32,
        description: String,
        image_path: String,
    ) -> String {
        self.next_id += 1;
        let ref_id = format!("ref_{:03}", self.next_id);
        self.entries.push(ReferenceEntry {
            ref_id: ref_id.clone(),
            label,
            weight,
            description,
            image_path,
        });
        ref_id
    }

    /// Remove a reference by ID, returning true if found.
    pub fn remove(&mut self, ref_id: &str) -> bool {
        let len_before = self.entries.len();
        self.entries.retain(|e| e.ref_id != ref_id);
        self.entries.len() < len_before
    }

    /// Clear all references.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Marker component for the interactive fly camera.
#[derive(Component)]
pub(crate) struct FlyCam;

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
        .init_resource::<GenInitialWorld>()
        .init_resource::<NameRegistry>()
        .init_resource::<NextEntityId>()
        .init_resource::<DirtyTracker>()
        .init_resource::<UndoStack>()
        .init_resource::<PendingScreenshots>()
        .init_resource::<PendingGltfLoads>()
        .init_resource::<PendingWorldSetup>()
        .init_resource::<crate::worldgen::NavMeshResource>()
        .init_resource::<crate::worldgen::GenerationState>()
        .init_resource::<crate::worldgen::NavMeshOverrides>()
        .init_resource::<WorldTours>()
        .init_resource::<CurrentWorldSkill>()
        .init_resource::<WorldDirty>()
        .init_resource::<CurrentWorld>()
        .init_resource::<FlyCamConfig>()
        .init_resource::<BehaviorState>()
        .init_resource::<avatar::CameraMode>()
        .init_resource::<crate::gen3d::asset_gen::AssetGenManager>()
        .init_resource::<super::pending_writes::PendingWrites>()
        .init_resource::<super::generation_log::GenerationLog>()
        .init_resource::<super::region_dirty::RegionDirtyFlags>()
        .init_resource::<ReferenceBoard>()
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
        .add_systems(Update, process_pending_world_setup)
        .add_systems(Update, audio::spatial_audio_update)
        .add_systems(Update, audio::auto_infer_audio)
        .add_systems(Update, behaviors::behavior_tick)
        .add_systems(Update, super::region_dirty::track_region_dirty)
        // FreeFly systems (run when camera is detached and not hovering inspector)
        .add_systems(
            Update,
            fly_cam_movement.run_if(avatar::in_freefly_mode.and_then(crate::inspector::not_ui_hovered)),
        )
        .add_systems(
            Update,
            fly_cam_look.run_if(avatar::in_freefly_mode.and_then(crate::inspector::not_ui_hovered)),
        )
        .add_systems(
            Update,
            fly_cam_scroll_speed
                .run_if(avatar::in_freefly_mode.and_then(crate::inspector::not_ui_hovered)),
        )
        // Toggle system (Tab: Player ↔ FreeFly)
        .add_systems(Update, avatar::handle_camera_mode_toggle)
        // P1: Character plugins
        .add_plugins(crate::character::PlayerPlugin)
        .add_plugins(crate::character::NpcPlugin)
        .add_plugins(crate::character::npc_brain::NpcBrainPlugin)
        .add_plugins(crate::character::CameraPlugin)
        .add_plugins(crate::character::DialoguePlugin)
        .add_plugins(crate::character::SpawnPointPlugin)
        // P2: Interaction plugin
        .add_plugins(crate::interaction::InteractionPlugin)
        // P3: Terrain & landscape plugins
        .add_plugins(crate::terrain::TerrainPlugin)
        .add_plugins(crate::terrain::SkyPlugin)
        .add_plugins(crate::terrain::WaterPlugin)
        .add_plugins(crate::terrain::FoliagePlugin)
        .add_plugins(crate::terrain::PathPlugin)
        // P4: UI plugins
        .add_plugins(crate::ui::SignPlugin)
        .add_plugins(crate::ui::HudPlugin)
        .add_plugins(crate::ui::LabelPlugin)
        .add_plugins(crate::ui::TooltipPlugin)
        .add_plugins(crate::ui::NotificationPlugin)
        // P5: Physics engine (Avian3d) + optional Tnua character controller
        ;
    #[cfg(feature = "physics")]
    app.add_plugins(avian3d::PhysicsPlugins::default());
    #[cfg(feature = "character-controller")]
    {
        use crate::character::player::PlayerScheme;
        app.add_plugins(bevy_tnua::TnuaControllerPlugin::<PlayerScheme>::new(
            bevy::app::PostUpdate,
        ))
        .add_plugins(bevy_tnua_avian3d::TnuaAvian3dPlugin::new(
            bevy::app::PostUpdate,
        ));
    }
    // P5: Custom physics plugins
    app.add_plugins(crate::physics::PhysicsBodyPlugin)
        .add_plugins(crate::physics::ForceFieldPlugin)
        .add_plugins(crate::physics::GravityPlugin)
        .add_plugins(crate::physics::ColliderPlugin)
        .add_plugins(crate::physics::JointPlugin)
        // Gallery UI (egui overlay, G toggle)
        .add_plugins(crate::gen3d::gallery_ui::GalleryPlugin)
        // World Inspector Panel (egui overlay, F1 toggle)
        .add_plugins(crate::inspector::InspectorPlugin);
}

/// Default scene: ground plane, camera, directional light, ambient light.
fn setup_default_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut registry: ResMut<NameRegistry>,
    mut next_id: ResMut<NextEntityId>,
) {
    // Ground plane — 20×20 gray
    let ground_id = next_id.alloc();
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
                world_id: ground_id,
            },
            crate::gen3d::registry::ParametricShape {
                shape: localgpt_world_types::Shape::Plane { x: 20.0, z: 20.0 },
            },
        ))
        .id();
    registry.insert_with_id("ground_plane".into(), ground, ground_id);

    // Camera at (5, 5, 5) looking at origin
    let cam_id = next_id.alloc();
    let camera = commands
        .spawn((
            Camera3d::default(),
            Transform::from_translation(Vec3::new(5.0, 5.0, 5.0)).looking_at(Vec3::ZERO, Vec3::Y),
            Name::new("main_camera"),
            FlyCam,
            SpatialAudioListener,
            GenEntity {
                entity_type: GenEntityType::Camera,
                world_id: cam_id,
            },
        ))
        .id();
    registry.insert_with_id("main_camera".into(), camera, cam_id);

    // Directional light — warm white, shadows
    let light_id = next_id.alloc();
    let light = commands
        .spawn((
            DirectionalLight {
                illuminance: 10000.0,
                shadow_maps_enabled: true,
                color: Color::srgba(1.0, 0.95, 0.9, 1.0),
                ..default()
            },
            Transform::from_translation(Vec3::new(4.0, 8.0, 4.0)).looking_at(Vec3::ZERO, Vec3::Y),
            Name::new("main_light"),
            GenEntity {
                entity_type: GenEntityType::Light,
                world_id: light_id,
            },
        ))
        .id();
    registry.insert_with_id("main_light".into(), light, light_id);
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
    let handle = asset_server.load::<WorldAsset>(format!("{}#Scene0", asset_path));

    pending.queue.push(PendingGltfLoad {
        handle,
        name,
        path: path.to_string_lossy().into_owned(),
        send_response: false,
        segment: false,
    });
}

/// Poll the command channel each frame and dispatch.
#[derive(SystemParam)]
struct GenCommandParams<'w, 's> {
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    registry: ResMut<'w, NameRegistry>,
    next_entity_id: ResMut<'w, NextEntityId>,
    dirty_tracker: ResMut<'w, DirtyTracker>,
    undo_stack: ResMut<'w, UndoStack>,
    pending_screenshots: ResMut<'w, PendingScreenshots>,
    pending_gltf: ResMut<'w, PendingGltfLoads>,
    audio_engine: ResMut<'w, audio::AudioEngine>,
    behavior_state: ResMut<'w, BehaviorState>,
    asset_server: Res<'w, AssetServer>,
    workspace: Res<'w, GenWorkspace>,
    transforms: Query<'w, 's, &'static Transform>,
    gen_entities: Query<'w, 's, &'static GenEntity>,
    names_query: Query<'w, 's, &'static Name>,
    children_query: Query<'w, 's, &'static Children>,
    parent_query: Query<'w, 's, &'static ChildOf>,
    visibility_query: Query<'w, 's, &'static Visibility>,
    material_handles: Query<'w, 's, &'static MeshMaterial3d<StandardMaterial>>,
    mesh_handles: Query<'w, 's, &'static Mesh3d>,
    behaviors_query: Query<'w, 's, &'static mut EntityBehaviors>,
    parametric_shapes: Query<'w, 's, &'static ParametricShape>,
    directional_lights: Query<'w, 's, &'static DirectionalLight>,
    point_lights: Query<'w, 's, &'static PointLight>,
    spot_lights: Query<'w, 's, &'static SpotLight>,
    audio_emitters: Query<'w, 's, &'static audio::AudioEmitter>,
    gltf_sources: Query<'w, 's, &'static GltfSource>,
    material_textures: Query<'w, 's, &'static MaterialTextures>,
    projections: Query<'w, 's, &'static Projection>,
    clear_color: Option<Res<'w, ClearColor>>,
    ambient_light: Option<Res<'w, GlobalAmbientLight>>,
    pending_world: ResMut<'w, PendingWorldSetup>,
    world_tours: ResMut<'w, WorldTours>,
    current_world: ResMut<'w, CurrentWorld>,
    camera_mode: ResMut<'w, avatar::CameraMode>,
    #[cfg(feature = "character-controller")]
    player_scheme_configs: ResMut<'w, Assets<crate::character::player::PlayerSchemeConfig>>,
    player_camera_query: Query<'w, 's, &'static mut crate::character::PlayerCamera>,
    terrain_q: Query<'w, 's, (&'static crate::terrain::Terrain, &'static Transform)>,
    npc_behaviors: Query<'w, 's, &'static mut crate::character::npc::NpcBehavior>,
    npc_brains: Query<'w, 's, &'static crate::character::npc_brain::NpcBrainState>,
    npc_memories: Query<'w, 's, &'static crate::character::npc_memory::NpcMemory>,
    blockout_generated_q: Query<'w, 's, (Entity, &'static crate::worldgen::BlockoutGenerated)>,
    current_blockout: Option<Res<'w, crate::worldgen::CurrentBlockout>>,
    tier_q: Query<'w, 's, &'static crate::worldgen::PlacementTier>,
    role_q: Query<'w, 's, &'static crate::worldgen::SemanticRole>,
    navmesh_resource: Option<Res<'w, crate::worldgen::NavMeshResource>>,
    navmesh_overrides: ResMut<'w, crate::worldgen::NavMeshOverrides>,
    asset_gen_manager: ResMut<'w, crate::gen3d::asset_gen::AssetGenManager>,
    pending_writes: ResMut<'w, super::pending_writes::PendingWrites>,
    generation_log: ResMut<'w, super::generation_log::GenerationLog>,
    region_dirty_flags: ResMut<'w, super::region_dirty::RegionDirtyFlags>,
    region_member_q: Query<'w, 's, (Entity, &'static super::registry::RegionMember)>,
    reference_board: ResMut<'w, ReferenceBoard>,
}

/// Build a `SnapshotQueries` from `GenCommandParams`. Used in many dispatch arms.
///
/// `behaviors_query` is stored by value as a read-only view (read-only
/// queries are `Copy`) — snapshotting never mutates behaviors, and an owned
/// view avoids borrow-checker conflicts in systems that also filter on
/// `Changed<EntityBehaviors>`.
macro_rules! snap_queries {
    ($params:expr) => {
        SnapshotQueries {
            transforms: &$params.transforms,
            parametric_shapes: &$params.parametric_shapes,
            material_handles: &$params.material_handles,
            materials: &$params.materials,
            visibility_query: &$params.visibility_query,
            directional_lights: &$params.directional_lights,
            point_lights: &$params.point_lights,
            spot_lights: &$params.spot_lights,
            behaviors_query: $params.behaviors_query.as_readonly(),
            audio_emitters: &$params.audio_emitters,
            parent_query: &$params.parent_query,
            gltf_sources: &$params.gltf_sources,
            material_textures: Some(&$params.material_textures),
            registry: &$params.registry,
        }
    };
}

fn process_gen_commands(
    mut channel_res: ResMut<GenChannelRes>,
    mut commands: Commands,
    mut params: GenCommandParams,
    mut pending_gallery_load: ResMut<crate::gen3d::gallery_ui::PendingGalleryLoad>,
    mut initial_world: ResMut<GenInitialWorld>,
) {
    // Loads queued inside the app (the gallery, `--world`) run through the
    // same handler as the agent's, but no agent awaits them: their responses
    // are logged instead of sent, or the agent's next command would receive
    // them.
    let mut internal = initial_world
        .path
        .take()
        .into_iter()
        .chain(pending_gallery_load.path.take())
        .map(|path| {
            tracing::info!("Loading world from {}", path);
            GenCommand::LoadWorld { path, clear: true }
        })
        .collect::<Vec<_>>()
        .into_iter();

    loop {
        let (cmd, reply) = if let Some(cmd) = internal.next() {
            (cmd, false)
        } else if let Ok(cmd) = channel_res.channels.cmd_rx.try_recv() {
            (cmd, true)
        } else {
            break;
        };
        let response = match cmd {
            GenCommand::SceneInfo => handle_scene_info(
                params.undo_stack.revision(),
                &params.registry,
                &params.transforms,
                &params.gen_entities,
                &params.material_handles,
                &params.materials,
                &params.parametric_shapes,
                &params.directional_lights,
                &params.point_lights,
                &params.spot_lights,
                &params.behaviors_query,
                &params.audio_engine,
                &params.tier_q,
                &params.role_q,
            ),
            GenCommand::EntityInfo { name } => handle_entity_info(
                &name,
                &params.registry,
                &params.transforms,
                &params.gen_entities,
                &params.names_query,
                &params.children_query,
                &params.parent_query,
                &params.visibility_query,
                &params.material_handles,
                &params.materials,
                &params.behaviors_query,
                &params.parametric_shapes,
                &params.directional_lights,
                &params.point_lights,
                &params.spot_lights,
                &params.gltf_sources,
                &params.audio_engine,
            ),
            GenCommand::Screenshot {
                width,
                height,
                wait_frames,
                highlight_entity,
                highlight_color,
                camera_angle,
                include_annotations,
            } => {
                params.pending_screenshots.queue.push(PendingScreenshot {
                    frames_remaining: wait_frames,
                    width,
                    height,
                    path: None,
                    save_to_skill: true,
                    highlight_entity,
                    highlight_color: highlight_color.unwrap_or([1.0, 0.0, 0.0, 1.0]),
                    camera_angle: camera_angle.unwrap_or(ScreenshotCameraAngle::Current),
                    include_annotations,
                });
                // Response will be sent by process_pending_screenshots
                continue;
            }
            GenCommand::SpawnPrimitive(cmd) => handle_spawn_primitive(
                cmd,
                &mut commands,
                &mut params.meshes,
                &mut params.materials,
                &mut params.registry,
                &mut params.next_entity_id,
            ),
            GenCommand::ModifyEntity(cmd) => {
                // Snapshot before modify so we can undo
                let pre_snapshot = params.registry.get_entity(&cmd.name).and_then(|e| {
                    params
                        .registry
                        .get_id(e)
                        .map(|id| snapshot_entity(&cmd.name, e, id, &snap_queries!(params)))
                });
                let resp = handle_modify_entity(
                    cmd.clone(),
                    &mut commands,
                    &params.registry,
                    &mut params.materials,
                    &params.material_handles,
                    &params.transforms,
                );
                // GAP-P0-02/04: Update behavior anchors + NPC wander center
                // when entity position/scale changes
                if let GenResponse::Modified { .. } = &resp
                    && let Some(entity) = params.registry.get_entity(&cmd.name)
                    && (cmd.position.is_some() || cmd.scale.is_some())
                {
                    let new_transform = params.transforms.get(entity).copied().unwrap_or_default();
                    // Update behavior base_position/base_scale
                    if let Ok(mut behaviors) = params.behaviors_query.get_mut(entity) {
                        for bi in &mut behaviors.behaviors {
                            if cmd.position.is_some() {
                                bi.base_position = new_transform.translation;
                            }
                            if cmd.scale.is_some() {
                                bi.base_scale = new_transform.scale;
                            }
                        }
                    }
                    // Update NPC wander center / patrol waypoints
                    if cmd.position.is_some()
                        && let Ok(mut npc_beh) = params.npc_behaviors.get_mut(entity)
                    {
                        match npc_beh.as_mut() {
                            crate::character::npc::NpcBehavior::Wander {
                                spawn_position,
                                target_position,
                                ..
                            } => {
                                *spawn_position = new_transform.translation;
                                *target_position = None; // reset target
                            }
                            crate::character::npc::NpcBehavior::Patrol {
                                points,
                                current_index,
                                ..
                            } => {
                                // Shift all patrol points by the position delta
                                if let Some(new_pos) = cmd.position {
                                    let delta = Vec3::from_array(new_pos)
                                        - pre_snapshot
                                            .as_ref()
                                            .map(|s| Vec3::from_array(s.transform.position))
                                            .unwrap_or(Vec3::ZERO);
                                    for pt in points.iter_mut() {
                                        *pt += delta;
                                    }
                                    *current_index = 0;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if let GenResponse::Modified { .. } = &resp
                    && let Some(old_we) = pre_snapshot
                {
                    let id = old_we.id;
                    let mut new_we = old_we.clone();
                    apply_modify_to_snapshot(&mut new_we, &cmd);
                    params.dirty_tracker.mark_dirty(id);
                    params.undo_stack.record(
                        wt::EditOp::Batch {
                            ops: vec![wt::EditOp::delete(id), wt::EditOp::spawn(new_we)],
                        },
                        wt::EditOp::Batch {
                            ops: vec![wt::EditOp::delete(id), wt::EditOp::spawn(old_we)],
                        },
                        None,
                    );
                }
                resp
            }
            GenCommand::DeleteEntity { name } => {
                // Snapshot before delete so we can undo
                let pre_snapshot = params.registry.get_entity(&name).and_then(|e| {
                    params
                        .registry
                        .get_id(e)
                        .map(|id| snapshot_entity(&name, e, id, &snap_queries!(params)))
                });
                let resp = handle_delete_entity(&name, &mut commands, &mut params.registry);
                if let GenResponse::Deleted { .. } = &resp
                    && let Some(we) = pre_snapshot
                {
                    let id = we.id;
                    params.dirty_tracker.mark_dirty(id);
                    params
                        .undo_stack
                        .record(wt::EditOp::delete(id), wt::EditOp::spawn(we), None);
                }
                resp
            }
            GenCommand::SpawnBatch {
                entities,
                expected_revision,
            } => 'batch: {
                if let Err(message) = check_batch(
                    params.undo_stack.revision(),
                    expected_revision,
                    validate_spawn_batch(&entities, &params.registry),
                ) {
                    break 'batch GenResponse::Error { message };
                }
                let mut results = Vec::with_capacity(entities.len());
                let mut spawned_entities = Vec::new();

                for cmd in entities {
                    // Snapshot before spawn for undo
                    let resp = handle_spawn_primitive(
                        cmd.clone(),
                        &mut commands,
                        &mut params.meshes,
                        &mut params.materials,
                        &mut params.registry,
                        &mut params.next_entity_id,
                    );

                    match &resp {
                        GenResponse::Spawned { name, entity_id } => {
                            results.push(format!("Created: {} (id: {})", name, entity_id));
                            if let Some(ent) = params.registry.get_entity(name)
                                && let Some(id) = params.registry.get_id(ent)
                            {
                                params.dirty_tracker.mark_dirty(id);
                                spawned_entities.push(snapshot_entity(
                                    name,
                                    ent,
                                    id,
                                    &snap_queries!(params),
                                ));
                            }
                        }
                        GenResponse::Error { message } => {
                            results.push(format!("Failed: {} - {}", cmd.name, message));
                        }
                        _ => {
                            results.push(format!("Failed: {} - unexpected response", cmd.name));
                        }
                    }
                }

                // Record undo for batch spawn (single batch delete)
                if !spawned_entities.is_empty() {
                    let ids: Vec<wt::EntityId> = spawned_entities.iter().map(|we| we.id).collect();
                    params.undo_stack.record(
                        wt::EditOp::Batch {
                            ops: ids.into_iter().map(wt::EditOp::delete).collect(),
                        },
                        wt::EditOp::Batch {
                            ops: spawned_entities
                                .into_iter()
                                .map(wt::EditOp::spawn)
                                .collect(),
                        },
                        None,
                    );
                }

                GenResponse::BatchResult {
                    results,
                    revision: params.undo_stack.revision(),
                }
            }
            GenCommand::ModifyBatch {
                entities,
                expected_revision,
            } => 'batch: {
                if let Err(message) = check_batch(
                    params.undo_stack.revision(),
                    expected_revision,
                    validate_modify_batch(&entities, &params.registry),
                ) {
                    break 'batch GenResponse::Error { message };
                }
                let mut results = Vec::with_capacity(entities.len());
                let mut undo_ops = Vec::new();
                let mut redo_ops = Vec::new();

                for cmd in entities {
                    // Snapshot before modify
                    let pre_snapshot = params.registry.get_entity(&cmd.name).and_then(|e| {
                        params
                            .registry
                            .get_id(e)
                            .map(|id| snapshot_entity(&cmd.name, e, id, &snap_queries!(params)))
                    });

                    let resp = handle_modify_entity(
                        cmd.clone(),
                        &mut commands,
                        &params.registry,
                        &mut params.materials,
                        &params.material_handles,
                        &params.transforms,
                    );

                    match &resp {
                        GenResponse::Modified { name } => {
                            results.push(format!("Modified: {}", name));
                            if let Some(old_we) = pre_snapshot {
                                let id = old_we.id;
                                params.dirty_tracker.mark_dirty(id);
                                if let Some(_new_ent) = params.registry.get_entity(name) {
                                    let mut new_we = old_we.clone();
                                    apply_modify_to_snapshot(&mut new_we, &cmd);
                                    undo_ops.push(wt::EditOp::delete(id));
                                    undo_ops.push(wt::EditOp::spawn(old_we));
                                    redo_ops.push(wt::EditOp::delete(id));
                                    redo_ops.push(wt::EditOp::spawn(new_we));
                                }
                            }
                        }
                        GenResponse::Error { message } => {
                            results.push(format!("Failed: {} - {}", cmd.name, message));
                        }
                        _ => {
                            results.push(format!("Failed: {} - unexpected response", cmd.name));
                        }
                    }
                }

                // Record undo for batch modify
                if !undo_ops.is_empty() {
                    params.undo_stack.record(
                        wt::EditOp::Batch { ops: redo_ops },
                        wt::EditOp::Batch { ops: undo_ops },
                        None,
                    );
                }

                GenResponse::BatchResult {
                    results,
                    revision: params.undo_stack.revision(),
                }
            }
            GenCommand::DeleteBatch {
                names,
                expected_revision,
            } => 'batch: {
                if let Err(message) = check_batch(
                    params.undo_stack.revision(),
                    expected_revision,
                    validate_delete_batch(&names, &params.registry),
                ) {
                    break 'batch GenResponse::Error { message };
                }
                let mut results = Vec::with_capacity(names.len());
                let mut deleted_entities = Vec::new();

                for name in names {
                    // Snapshot before delete
                    let pre_snapshot = params.registry.get_entity(&name).and_then(|e| {
                        params
                            .registry
                            .get_id(e)
                            .map(|id| snapshot_entity(&name, e, id, &snap_queries!(params)))
                    });

                    let resp = handle_delete_entity(&name, &mut commands, &mut params.registry);

                    match &resp {
                        GenResponse::Deleted { name } => {
                            results.push(format!("Deleted: {}", name));
                            if let Some(we) = pre_snapshot {
                                params.dirty_tracker.mark_dirty(we.id);
                                deleted_entities.push(we);
                            }
                        }
                        GenResponse::Error { message } => {
                            results.push(format!("Failed: {} - {}", name, message));
                        }
                        _ => {
                            results.push(format!("Failed: {} - unexpected response", name));
                        }
                    }
                }

                // Record undo for batch delete (batch spawn to restore)
                if !deleted_entities.is_empty() {
                    let ids: Vec<wt::EntityId> = deleted_entities.iter().map(|we| we.id).collect();
                    params.undo_stack.record(
                        wt::EditOp::Batch {
                            ops: deleted_entities
                                .into_iter()
                                .map(wt::EditOp::spawn)
                                .collect(),
                        },
                        wt::EditOp::Batch {
                            ops: ids.into_iter().map(wt::EditOp::delete).collect(),
                        },
                        None,
                    );
                }

                GenResponse::BatchResult {
                    results,
                    revision: params.undo_stack.revision(),
                }
            }
            GenCommand::SetCamera(cmd) => {
                // Capture old camera state for undo
                let old_camera = params.registry.get_entity("main_camera").map(|cam_ent| {
                    let pos = params
                        .transforms
                        .get(cam_ent)
                        .map(|t| t.translation.to_array())
                        .unwrap_or([5.0, 5.0, 5.0]);
                    let fov = params
                        .projections
                        .get(cam_ent)
                        .ok()
                        .and_then(|p| match p {
                            Projection::Perspective(pp) => Some(pp.fov.to_degrees()),
                            _ => None,
                        })
                        .unwrap_or(45.0);
                    // Compute look_at from current forward direction
                    let forward = params
                        .transforms
                        .get(cam_ent)
                        .map(|t| t.forward().as_vec3())
                        .unwrap_or(Vec3::NEG_Z);
                    let look_at = params
                        .transforms
                        .get(cam_ent)
                        .map(|t| (t.translation + forward * 10.0).to_array())
                        .unwrap_or([0.0, 0.0, 0.0]);
                    wt::CameraDef {
                        position: pos,
                        look_at,
                        fov_degrees: fov,
                    }
                });
                let new_camera = wt::CameraDef {
                    position: cmd.position,
                    look_at: cmd.look_at,
                    fov_degrees: cmd.fov_degrees,
                };
                let resp = handle_set_camera(cmd, &mut commands, &params.registry);
                if let GenResponse::CameraSet = &resp
                    && let Some(old_cam) = old_camera
                {
                    params.undo_stack.record(
                        wt::EditOp::SetCamera { camera: new_camera },
                        wt::EditOp::SetCamera { camera: old_cam },
                        None,
                    );
                }
                resp
            }
            GenCommand::SetLight(cmd) => {
                // Snapshot the old light before it gets despawned (for undo)
                let old_light_snapshot =
                    params.registry.get_entity(&cmd.name).and_then(|old_ent| {
                        params.registry.get_id(old_ent).map(|old_id| {
                            snapshot_entity(&cmd.name, old_ent, old_id, &snap_queries!(params))
                        })
                    });
                let resp = handle_set_light(
                    cmd,
                    &mut commands,
                    &mut params.registry,
                    &mut params.next_entity_id,
                );
                // Record undo: if we replaced an old light, use batch to restore it
                if let GenResponse::LightSet { ref name } = resp
                    && let Some(new_ent) = params.registry.get_entity(name)
                    && let Some(new_id) = params.registry.get_id(new_ent)
                {
                    let new_we = snapshot_entity(name, new_ent, new_id, &snap_queries!(params));
                    params.dirty_tracker.mark_dirty(new_id);
                    if let Some(old_we) = old_light_snapshot {
                        // Replacing existing light: undo restores old, redo re-applies new
                        let old_id = old_we.id;
                        params.undo_stack.record(
                            wt::EditOp::Batch {
                                ops: vec![wt::EditOp::delete(new_id), wt::EditOp::spawn(new_we)],
                            },
                            wt::EditOp::Batch {
                                ops: vec![wt::EditOp::delete(old_id), wt::EditOp::spawn(old_we)],
                            },
                            None,
                        );
                    } else {
                        // New light: undo is simply delete
                        params.undo_stack.record(
                            wt::EditOp::spawn(new_we),
                            wt::EditOp::delete(new_id),
                            None,
                        );
                    }
                }
                resp
            }
            GenCommand::SetEnvironment(cmd) => {
                // Capture current environment for undo
                let old_env = {
                    let bg = params.clear_color.as_ref().map(|cc| {
                        let c = cc.0.to_srgba();
                        [c.red, c.green, c.blue, c.alpha]
                    });
                    let (ambient_intensity, ambient_color) =
                        params.ambient_light.as_ref().map_or((None, None), |al| {
                            let c = al.color.to_srgba();
                            (Some(al.brightness), Some([c.red, c.green, c.blue, c.alpha]))
                        });
                    wt::EnvironmentDef {
                        background_color: bg,
                        ambient_intensity,
                        ambient_color,
                        fog_density: None,
                        fog_color: None,
                    }
                };
                // Build new env from the command (can't read resources after
                // deferred commands since they haven't been applied yet)
                let new_env = wt::EnvironmentDef {
                    background_color: cmd.background_color,
                    ambient_intensity: cmd.ambient_light,
                    ambient_color: cmd.ambient_color,
                    fog_density: None,
                    fog_color: None,
                };
                let resp = handle_set_environment(cmd, &mut commands);
                if let GenResponse::EnvironmentSet = &resp {
                    params.undo_stack.record(
                        wt::EditOp::SetEnvironment { env: new_env },
                        wt::EditOp::SetEnvironment { env: old_env },
                        None,
                    );
                }
                resp
            }
            GenCommand::SpawnMesh(cmd) => handle_spawn_mesh(
                cmd,
                &mut commands,
                &mut params.meshes,
                &mut params.materials,
                &mut params.registry,
                &mut params.next_entity_id,
            ),
            GenCommand::ExportScreenshot {
                path,
                width,
                height,
            } => {
                params.pending_screenshots.queue.push(PendingScreenshot {
                    frames_remaining: 3,
                    width,
                    height,
                    path: Some(PathBuf::from(path)),
                    save_to_skill: true,
                    highlight_entity: None,
                    highlight_color: [1.0, 0.0, 0.0, 1.0],
                    camera_angle: ScreenshotCameraAngle::Current,
                    include_annotations: false,
                });
                continue;
            }
            GenCommand::ExportGltf { path } => handle_export_gltf(
                path.as_deref(),
                &params.workspace,
                &params.registry,
                &params.transforms,
                &params.gen_entities,
                &params.parent_query,
                &params.material_handles,
                &params.materials,
                &params.mesh_handles,
                &params.meshes,
            ),
            GenCommand::LoadGltf { path, segment } => {
                if let Some(resolved) = resolve_gltf_path(&path, &params.workspace.path) {
                    let name = resolved
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "imported_scene".to_string());

                    let asset_path = resolved
                        .to_string_lossy()
                        .trim_start_matches('/')
                        .to_string();
                    let handle = params
                        .asset_server
                        .load::<WorldAsset>(format!("{}#Scene0", asset_path));

                    params.pending_gltf.queue.push(PendingGltfLoad {
                        handle,
                        name,
                        path: resolved.to_string_lossy().into_owned(),
                        send_response: true,
                        segment,
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
            GenCommand::SetAmbience(cmd) => {
                // Capture previous ambience for undo
                let prev_layers: Vec<wt::AmbienceLayerDef> = params
                    .audio_engine
                    .last_ambience
                    .as_ref()
                    .map(|prev| {
                        prev.layers
                            .iter()
                            .map(|layer| wt::AmbienceLayerDef {
                                name: layer.name.clone(),
                                source: compat::ambient_sound_to_source(&layer.sound),
                                volume: layer.volume,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let resp = audio::handle_set_ambience(cmd.clone(), &mut params.audio_engine);
                // Push undo
                let forward_layers: Vec<wt::AmbienceLayerDef> = cmd
                    .layers
                    .iter()
                    .map(|layer| wt::AmbienceLayerDef {
                        name: layer.name.clone(),
                        source: compat::ambient_sound_to_source(&layer.sound),
                        volume: layer.volume,
                    })
                    .collect();
                params.undo_stack.record(
                    wt::EditOp::SetAmbience {
                        ambience: forward_layers,
                    },
                    wt::EditOp::SetAmbience {
                        ambience: prev_layers,
                    },
                    None,
                );
                resp
            }
            GenCommand::SpawnAudioEmitter(cmd) => {
                let name = cmd.name.clone();
                let audio_def = wt::AudioDef {
                    kind: wt::AudioKind::Sfx,
                    source: compat::emitter_sound_to_source(&cmd.sound),
                    volume: cmd.volume,
                    radius: Some(cmd.radius),
                    rolloff: wt::Rolloff::default(),
                };
                let resp = audio::handle_spawn_audio_emitter(
                    cmd,
                    &mut params.audio_engine,
                    &mut commands,
                    &mut params.registry,
                    &mut params.next_entity_id,
                );
                // Push undo: inverse removes the emitter
                params.undo_stack.record(
                    wt::EditOp::SpawnAudioEmitter {
                        name: name.clone(),
                        audio: audio_def.clone(),
                    },
                    wt::EditOp::RemoveAudioEmitter {
                        name,
                        audio: audio_def,
                    },
                    None,
                );
                resp
            }
            GenCommand::ModifyAudioEmitter(cmd) => {
                // Capture previous state for undo
                let prev_audio =
                    params
                        .audio_engine
                        .emitter_meta
                        .get(&cmd.name)
                        .map(|meta| wt::AudioDef {
                            kind: wt::AudioKind::Sfx,
                            source: compat::emitter_sound_to_source(&meta.sound),
                            volume: meta.base_volume,
                            radius: Some(meta.radius),
                            rolloff: wt::Rolloff::default(),
                        });
                let resp =
                    audio::handle_modify_audio_emitter(cmd.clone(), &mut params.audio_engine);
                // Get new state for redo
                let new_audio =
                    params
                        .audio_engine
                        .emitter_meta
                        .get(&cmd.name)
                        .map(|meta| wt::AudioDef {
                            kind: wt::AudioKind::Sfx,
                            source: compat::emitter_sound_to_source(&meta.sound),
                            volume: meta.base_volume,
                            radius: Some(meta.radius),
                            rolloff: wt::Rolloff::default(),
                        });
                if let (Some(prev), Some(new)) = (prev_audio, new_audio) {
                    params.undo_stack.record(
                        wt::EditOp::SpawnAudioEmitter {
                            name: cmd.name.clone(),
                            audio: new,
                        },
                        wt::EditOp::SpawnAudioEmitter {
                            name: cmd.name.clone(),
                            audio: prev,
                        },
                        None,
                    );
                }
                resp
            }
            GenCommand::RemoveAudioEmitter { name } => {
                // Capture previous state for undo
                let prev_audio =
                    params
                        .audio_engine
                        .emitter_meta
                        .get(&name)
                        .map(|meta| wt::AudioDef {
                            kind: wt::AudioKind::Sfx,
                            source: compat::emitter_sound_to_source(&meta.sound),
                            volume: meta.base_volume,
                            radius: Some(meta.radius),
                            rolloff: wt::Rolloff::default(),
                        });
                let resp = audio::handle_remove_audio_emitter(&name, &mut params.audio_engine);
                // Push undo: inverse re-spawns the emitter
                if let Some(prev) = prev_audio {
                    params.undo_stack.record(
                        wt::EditOp::RemoveAudioEmitter {
                            name: name.clone(),
                            audio: prev.clone(),
                        },
                        wt::EditOp::SpawnAudioEmitter { name, audio: prev },
                        None,
                    );
                }
                resp
            }
            GenCommand::AudioInfo => audio::handle_audio_info(&params.audio_engine),

            // Behavior commands
            GenCommand::AddBehavior(cmd) => {
                // Snapshot before adding behavior for undo
                let pre_snapshot = params.registry.get_entity(&cmd.entity).and_then(|e| {
                    params
                        .registry
                        .get_id(e)
                        .map(|id| snapshot_entity(&cmd.entity, e, id, &snap_queries!(params)))
                });
                let entity_name = cmd.entity.clone();
                let resp = behaviors::handle_add_behavior(
                    cmd,
                    &mut params.behavior_state,
                    &mut commands,
                    &params.registry,
                    &params.transforms,
                    &mut params.behaviors_query,
                );
                if let GenResponse::BehaviorAdded { .. } = &resp
                    && let Some(old_we) = pre_snapshot
                    && let Some(e) = params.registry.get_entity(&entity_name)
                    && let Some(id) = params.registry.get_id(e)
                {
                    let new_we = snapshot_entity(&entity_name, e, id, &snap_queries!(params));
                    params.undo_stack.record(
                        wt::EditOp::Batch {
                            ops: vec![wt::EditOp::delete(id), wt::EditOp::spawn(new_we)],
                        },
                        wt::EditOp::Batch {
                            ops: vec![wt::EditOp::delete(id), wt::EditOp::spawn(old_we)],
                        },
                        None,
                    );
                }
                resp
            }
            GenCommand::RemoveBehavior {
                entity,
                behavior_id,
            } => {
                // Snapshot before removing behavior for undo
                let pre_snapshot = params.registry.get_entity(&entity).and_then(|e| {
                    params
                        .registry
                        .get_id(e)
                        .map(|id| snapshot_entity(&entity, e, id, &snap_queries!(params)))
                });
                let entity_name = entity.clone();
                let resp = behaviors::handle_remove_behavior(
                    &entity,
                    behavior_id.as_deref(),
                    &params.registry,
                    &mut params.behaviors_query,
                );
                if let GenResponse::BehaviorRemoved { count, .. } = &resp
                    && *count > 0
                    && let Some(old_we) = pre_snapshot
                    && let Some(e) = params.registry.get_entity(&entity_name)
                    && let Some(id) = params.registry.get_id(e)
                {
                    let new_we = snapshot_entity(&entity_name, e, id, &snap_queries!(params));
                    params.undo_stack.record(
                        wt::EditOp::Batch {
                            ops: vec![wt::EditOp::delete(id), wt::EditOp::spawn(new_we)],
                        },
                        wt::EditOp::Batch {
                            ops: vec![wt::EditOp::delete(id), wt::EditOp::spawn(old_we)],
                        },
                        None,
                    );
                }
                resp
            }
            GenCommand::ListBehaviors { entity } => behaviors::handle_list_behaviors(
                entity.as_deref(),
                &params.behavior_state,
                &params.registry,
                &params.behaviors_query,
            ),
            GenCommand::SetBehaviorsPaused { paused } => {
                params.behavior_state.paused = paused;
                GenResponse::BehaviorsPaused { paused }
            }

            // World commands
            GenCommand::SaveWorld(cmd) => {
                let env_data = super::world::EnvironmentSnapshot {
                    background_color: params.clear_color.as_ref().map(|c| {
                        let srgba = c.0.to_srgba();
                        [srgba.red, srgba.green, srgba.blue, srgba.alpha]
                    }),
                    ambient_intensity: params.ambient_light.as_ref().map(|a| a.brightness),
                    ambient_color: params.ambient_light.as_ref().map(|a| {
                        let srgba = a.color.to_srgba();
                        [srgba.red, srgba.green, srgba.blue, srgba.alpha]
                    }),
                };
                super::world::handle_save_world(
                    cmd,
                    &params.workspace,
                    &params.registry,
                    &params.transforms,
                    &params.gen_entities,
                    &params.parent_query,
                    &params.material_handles,
                    &params.materials,
                    &params.mesh_handles,
                    &params.meshes,
                    &params.audio_engine,
                    &params.behaviors_query,
                    &params.parametric_shapes,
                    &params.gltf_sources,
                    &params.material_textures,
                    &params.visibility_query,
                    &params.directional_lights,
                    &params.point_lights,
                    &params.spot_lights,
                    &params.projections,
                    &env_data,
                    None,
                    &params.world_tours.tours,
                    &params.undo_stack.history,
                    &params.npc_brains,
                    &params.npc_memories,
                )
            }
            GenCommand::ExportWorld { format } => handle_export_world(
                format.as_deref(),
                &params.workspace,
                &params.registry,
                &params.transforms,
                &params.gen_entities,
                &params.parent_query,
                &params.material_handles,
                &params.materials,
                &params.mesh_handles,
                &params.meshes,
            ),
            GenCommand::ExportHtml => handle_export_html(&params.workspace, &params.current_world),
            GenCommand::ForkWorld { source, new_name } => {
                match super::world::handle_fork_world(&source, &new_name, &params.workspace) {
                    Ok((path, warnings)) => GenResponse::WorldSaved {
                        path,
                        skill_name: new_name,
                        warnings,
                    },
                    Err(e) => GenResponse::Error { message: e },
                }
            }
            GenCommand::LoadWorld { path, clear } => {
                // Clear existing scene before loading if requested.
                if clear {
                    handle_clear_scene(
                        true, // keep camera
                        true, // keep lights
                        &mut commands,
                        &mut params.registry,
                        &params.gen_entities,
                        &mut params.audio_engine,
                        &mut params.behavior_state,
                        &mut params.pending_world,
                    );
                }

                let result = super::world::handle_load_world(
                    &path,
                    &params.workspace,
                    &mut params.behavior_state,
                );
                match result {
                    Ok(world_load) => {
                        // Track the loaded world
                        let world_path = PathBuf::from(&world_load.world_path);
                        let world_name = world_path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "unknown".to_string());
                        params.current_world.name = Some(world_name);
                        params.current_world.path = Some(world_path);

                        // Spawn entities directly from WorldManifest data
                        if !world_load.world_entities.is_empty() {
                            let world_dir = PathBuf::from(&world_load.world_path);
                            spawn_world_entities(
                                &world_load.world_entities,
                                &mut commands,
                                &mut params.meshes,
                                &mut params.materials,
                                &mut params.registry,
                                &mut params.next_entity_id,
                                &mut params.behavior_state,
                                &params.asset_server,
                                &mut params.pending_gltf,
                                Some(&world_dir),
                            );
                        }

                        // Ambience doesn't reference entities — apply immediately.
                        if let Some(ambience) = world_load.ambience {
                            audio::handle_set_ambience(ambience, &mut params.audio_engine);
                        }

                        // Audio emitters from RON format (already extracted by world.rs)
                        for emitter_cmd in &world_load.emitters {
                            audio::handle_spawn_audio_emitter(
                                emitter_cmd.clone(),
                                &mut params.audio_engine,
                                &mut commands,
                                &mut params.registry,
                                &mut params.next_entity_id,
                            );
                        }

                        // Environment and camera don't depend on scene entities.
                        if let Some(env) = world_load.environment {
                            handle_set_environment(env, &mut commands);
                        }
                        if let Some(cam) = world_load.camera {
                            handle_set_camera(cam, &mut commands, &params.registry);
                        }

                        // Store tour configuration as resource.
                        params.world_tours.tours = world_load.tours;

                        // Restore edit history from saved world, or clear if not saved
                        if let Some(history) = world_load.edit_history {
                            params.undo_stack.reset(history);
                        } else {
                            // No saved history — start fresh
                            params.undo_stack.reset(wt::EditHistory::default());
                        }

                        // Restore NPC brain/memory components from npcs.ron data
                        for npc_def in &world_load.npc_data {
                            if let Some(bevy_entity) =
                                params.registry.get_entity(&npc_def.entity_name)
                            {
                                if let Some(ref brain_def) = npc_def.brain {
                                    let config = crate::character::npc_brain::NpcBrainConfig {
                                        personality: brain_def.personality.clone(),
                                        model: brain_def.model.clone(),
                                        tick_rate: brain_def.tick_rate,
                                        perception_radius: brain_def.perception_radius,
                                        goals: brain_def.goals.clone(),
                                        knowledge: brain_def.knowledge.clone(),
                                    };
                                    let brain_state =
                                        crate::character::npc_brain::NpcBrainState::new(config);
                                    commands.entity(bevy_entity).insert(brain_state);
                                }
                                if let Some(ref mem_def) = npc_def.memory {
                                    let mut memory = crate::character::npc_memory::NpcMemory::new(
                                        mem_def.capacity,
                                        mem_def.auto_memorize,
                                    );
                                    for entry in &mem_def.entries {
                                        memory.add_memory(
                                            entry.content.clone(),
                                            entry.importance,
                                            entry.timestamp,
                                        );
                                    }
                                    commands.entity(bevy_entity).insert(memory);
                                }
                                tracing::info!(
                                    "Restored NPC data for '{}'{}{}",
                                    npc_def.entity_name,
                                    if npc_def.brain.is_some() {
                                        " [brain]"
                                    } else {
                                        ""
                                    },
                                    if npc_def.memory.is_some() {
                                        " [memory]"
                                    } else {
                                        ""
                                    },
                                );
                            } else {
                                tracing::warn!(
                                    "NPC data for '{}' — entity not found in registry (may load later)",
                                    npc_def.entity_name
                                );
                            }
                        }

                        GenResponse::WorldLoaded {
                            path: world_load.world_path,
                            entities: world_load.entity_count,
                            behaviors: world_load.behavior_count,
                        }
                    }
                    Err(e) => GenResponse::Error {
                        message: format!("Failed to load world: {}", e),
                    },
                }
            }

            GenCommand::ClearScene {
                keep_camera,
                keep_lights,
            } => {
                // Snapshot all entities before clearing for undo
                let mut pre_snapshots = Vec::new();
                let all_names: Vec<(String, bevy::ecs::entity::Entity)> = params
                    .registry
                    .all_names()
                    .map(|(n, e)| (n.to_string(), e))
                    .collect();
                for (name, ent) in &all_names {
                    if name == "main_camera" && keep_camera {
                        continue;
                    }
                    if let Some(id) = params.registry.get_id(*ent) {
                        // Check if this is a light entity (skip if keep_lights)
                        if keep_lights
                            && (params.directional_lights.get(*ent).is_ok()
                                || params.point_lights.get(*ent).is_ok()
                                || params.spot_lights.get(*ent).is_ok())
                        {
                            continue;
                        }
                        pre_snapshots.push(snapshot_entity(name, *ent, id, &snap_queries!(params)));
                    }
                }

                let resp = handle_clear_scene(
                    keep_camera,
                    keep_lights,
                    &mut commands,
                    &mut params.registry,
                    &params.gen_entities,
                    &mut params.audio_engine,
                    &mut params.behavior_state,
                    &mut params.pending_world,
                );

                if let GenResponse::SceneCleared { .. } = &resp
                    && !pre_snapshots.is_empty()
                {
                    // Forward: delete all entities; Inverse: re-spawn them all
                    let forward_ops: Vec<wt::EditOp> = pre_snapshots
                        .iter()
                        .map(|we| wt::EditOp::delete(we.id))
                        .collect();
                    let inverse_ops: Vec<wt::EditOp> =
                        pre_snapshots.into_iter().map(wt::EditOp::spawn).collect();
                    params.undo_stack.record(
                        wt::EditOp::Batch { ops: forward_ops },
                        wt::EditOp::Batch { ops: inverse_ops },
                        None,
                    );
                }

                // Tours are world-level metadata (not individual entities),
                // so they are always reset — a new world will provide its own.
                params.world_tours.tours.clear();

                // Return to free-fly camera mode.
                *params.camera_mode = avatar::CameraMode::FreeFly;

                resp
            }

            GenCommand::Undo => handle_undo(
                &mut params.undo_stack,
                &mut commands,
                &mut params.meshes,
                &mut params.materials,
                &mut params.registry,
                &mut params.next_entity_id,
                &mut params.behavior_state,
                &params.asset_server,
                &mut params.pending_gltf,
                &mut params.audio_engine,
            ),
            GenCommand::Redo => handle_redo(
                &mut params.undo_stack,
                &mut commands,
                &mut params.meshes,
                &mut params.materials,
                &mut params.registry,
                &mut params.next_entity_id,
                &mut params.behavior_state,
                &params.asset_server,
                &mut params.pending_gltf,
                &mut params.audio_engine,
            ),
            GenCommand::UndoInfo => GenResponse::UndoInfoResult {
                undo_count: params.undo_stack.history.undo_count(),
                redo_count: params.undo_stack.history.redo_count(),
                entity_count: params.registry.len(),
                dirty_count: params.dirty_tracker.dirty_count(),
            },

            // Tier 10: Avatar & Character System (P1)
            GenCommand::SpawnPlayer(p) => {
                let name = "Player".to_string();
                let wid = params.next_entity_id.alloc();
                #[cfg(feature = "character-controller")]
                let entity = crate::character::spawn_player(
                    &mut commands,
                    &mut params.meshes,
                    &mut params.materials,
                    &mut params.player_scheme_configs,
                    &p,
                );
                #[cfg(not(feature = "character-controller"))]
                let entity = crate::character::spawn_player(
                    &mut commands,
                    &mut params.meshes,
                    &mut params.materials,
                    &p,
                );
                params.registry.insert_with_id(name.clone(), entity, wid);
                // Switch to Player camera mode so player systems activate
                *params.camera_mode = avatar::CameraMode::Player;
                // Attach PlayerCamera to the main_camera so camera_follow_system tracks the player
                if let Some(cam_entity) = params.registry.get_entity("main_camera") {
                    commands
                        .entity(cam_entity)
                        .insert(crate::character::PlayerCamera::default());
                }
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::SetSpawnPoint(p) => {
                let name = p.name.clone().unwrap_or_else(|| "SpawnPoint".to_string());
                let wid = params.next_entity_id.alloc();
                let entity = crate::character::spawn_spawn_point(&mut commands, &p);
                params.registry.insert_with_id(name.clone(), entity, wid);
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::SpawnNpc(p) => {
                let name = p.name.clone();
                let wid = params.next_entity_id.alloc();
                let entity = crate::character::spawn_npc(
                    &mut commands,
                    &mut params.meshes,
                    &mut params.materials,
                    &p,
                );
                params.registry.insert_with_id(name.clone(), entity, wid);
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::SetNpcDialogue(p) => {
                let Some(entity) = params.registry.get_entity(&p.npc_id) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.npc_id),
                    });
                    continue;
                };
                let npc_name = p.npc_id.clone();
                let tree = crate::character::DialogueTree::from(p);
                commands.entity(entity).insert(tree);
                let _ = channel_res
                    .channels
                    .resp_tx
                    .send(GenResponse::Modified { name: npc_name });
                continue;
            }
            GenCommand::SetPlayerCameraMode(p) => {
                let camera_pov = p.mode_enum();

                // Apply camera settings to PlayerCamera if present
                for mut cam in params.player_camera_query.iter_mut() {
                    cam.mode = camera_pov;
                    cam.distance = p.distance;
                    cam.pitch = p.pitch;
                    cam.fov = p.fov;
                    cam.fixed_position = p.fixed_position.map(Vec3::from);
                    cam.fixed_look_at = p.fixed_look_at.map(Vec3::from);
                    cam.transition_duration = p.transition_duration;
                    cam.transition_progress = 0.0;
                }

                GenResponse::CameraSet
            }

            // Tier 11: Interaction & Trigger System (P2)
            GenCommand::AddTrigger(p) => {
                let Some(entity) = params.registry.get_entity(&p.entity_id) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.entity_id),
                    });
                    continue;
                };
                let entity_name = p.entity_id.clone();
                let mut ec = commands.entity(entity);
                ec.insert(crate::interaction::InteractionEntity);
                // Insert trigger component
                match p.trigger_type {
                    crate::interaction::TriggerType::Proximity => {
                        ec.insert(crate::interaction::ProximityTrigger {
                            radius: p.radius.unwrap_or(5.0),
                            cooldown: p.cooldown.unwrap_or(1.0),
                            last_triggered: 0.0,
                        });
                    }
                    crate::interaction::TriggerType::Click => {
                        ec.insert(crate::interaction::ClickTrigger {
                            max_distance: p.max_distance.unwrap_or(5.0),
                            prompt_text: p.prompt_text.clone(),
                        });
                    }
                    crate::interaction::TriggerType::Timer => {
                        if let Some(interval) = p.interval {
                            ec.insert(crate::interaction::TimerTrigger::new(interval));
                        }
                    }
                    crate::interaction::TriggerType::AreaEnter => {
                        ec.insert((
                            crate::interaction::AreaTrigger { is_enter: true },
                            crate::interaction::AreaInsideTracker::default(),
                        ));
                    }
                    crate::interaction::TriggerType::AreaExit => {
                        ec.insert((
                            crate::interaction::AreaTrigger { is_enter: false },
                            crate::interaction::AreaInsideTracker::default(),
                        ));
                    }
                    crate::interaction::TriggerType::Collision => {
                        ec.insert(crate::interaction::CollisionTrigger {
                            cooldown: p.cooldown.unwrap_or(1.0),
                            ..default()
                        });
                        // Add a sensor collider if the entity doesn't already have one.
                        // Uses the trigger radius as the sensor sphere size.
                        ec.insert(crate::physics::ColliderConfig {
                            shape: crate::physics::ColliderShape::Sphere,
                            size: Some(Vec3::splat(p.radius.unwrap_or(3.0) * 2.0)),
                            offset: Vec3::ZERO,
                            is_trigger: true,
                            visible_in_debug: true,
                        });
                    }
                }
                // Insert requires_item component if specified (Gap 2.3)
                if let Some(ref item_id) = p.requires_item {
                    ec.insert(crate::interaction::RequiresItem {
                        item_id: item_id.clone(),
                    });
                }
                // Insert action component
                match p.action {
                    crate::interaction::TriggerAction::Animate => {
                        ec.insert(crate::interaction::AnimateAction {
                            property: p
                                .state_key
                                .clone()
                                .unwrap_or_else(|| "position".to_string()),
                            to: p.destination.map(|d| d.to_vec()).unwrap_or_default(),
                            duration: p.cooldown.unwrap_or(1.0),
                            progress: 0.0,
                        });
                    }
                    crate::interaction::TriggerAction::Teleport => {
                        if let Some(dest) = p.destination {
                            ec.insert(crate::interaction::TeleportAction {
                                destination: Vec3::from_array(dest),
                                effect: crate::interaction::TeleportEffect::None,
                            });
                        }
                    }
                    crate::interaction::TriggerAction::PlaySound => {
                        ec.insert(crate::interaction::PlaySoundAction {
                            sound: p.text.clone().unwrap_or_else(|| "default".to_string()),
                        });
                    }
                    crate::interaction::TriggerAction::ShowText => {
                        if let Some(text) = &p.text {
                            ec.insert(crate::interaction::ShowTextAction {
                                text: text.clone(),
                                duration: None,
                            });
                        }
                    }
                    crate::interaction::TriggerAction::ToggleState => {
                        ec.insert(crate::interaction::ToggleStateAction {
                            state_key: p.state_key.clone().unwrap_or_else(|| "active".to_string()),
                            value: p.text.clone(),
                        });
                    }
                    crate::interaction::TriggerAction::Spawn => {
                        ec.insert(crate::interaction::SpawnAction {
                            template: p.text.clone().unwrap_or_default(),
                        });
                    }
                    crate::interaction::TriggerAction::Destroy => {
                        ec.insert(crate::interaction::DestroyAction);
                    }
                    crate::interaction::TriggerAction::AddScore => {
                        ec.insert(crate::interaction::AddScoreAction {
                            amount: p.amount.unwrap_or(1),
                            category: p.category.clone().unwrap_or_else(|| "points".to_string()),
                        });
                    }
                    crate::interaction::TriggerAction::Enable => {
                        ec.insert(crate::interaction::EnableAction);
                    }
                    crate::interaction::TriggerAction::Disable => {
                        ec.insert(crate::interaction::DisableAction);
                    }
                }
                // Mark as once-trigger if requested
                if p.once {
                    ec.insert(crate::interaction::OnceTrigger);
                }
                let _ = channel_res
                    .channels
                    .resp_tx
                    .send(GenResponse::Modified { name: entity_name });
                continue;
            }
            GenCommand::AddTeleporter(p) => {
                let name = p.label.clone().unwrap_or_else(|| "Teleporter".to_string());
                let wid = params.next_entity_id.alloc();
                let position = Vec3::from_array(p.position);
                let destination = Vec3::from_array(p.destination);
                let radius = p.size[0].max(p.size[2]) * 0.5;
                let height = p.size[1];
                // Visible portal: cylinder mesh with emissive material
                let mesh = params.meshes.add(Cylinder::new(radius, height));
                let material = params.materials.add(StandardMaterial {
                    base_color: Color::srgba(0.3, 0.1, 0.8, 0.5),
                    emissive: bevy::color::LinearRgba::new(0.5, 0.2, 1.0, 1.0),
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                });
                let entity = commands
                    .spawn((
                        Name::new(name.clone()),
                        GenEntity {
                            entity_type: GenEntityType::Primitive,
                            world_id: wid,
                        },
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::from_translation(position),
                        Visibility::default(),
                        crate::interaction::InteractionEntity,
                        crate::interaction::ProximityTrigger {
                            radius: radius * 2.0,
                            cooldown: 2.0,
                            last_triggered: 0.0,
                        },
                        crate::interaction::TeleportAction {
                            destination,
                            effect: p.effect,
                        },
                    ))
                    .id();
                params.registry.insert_with_id(name.clone(), entity, wid);
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::AddCollectible(p) => {
                let Some(entity) = params.registry.get_entity(&p.entity_id) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.entity_id),
                    });
                    continue;
                };
                let entity_name = p.entity_id.clone();
                let position = params
                    .transforms
                    .get(entity)
                    .map(|t| t.translation)
                    .unwrap_or(Vec3::ZERO);
                // Add bob + spin idle animation for collectibles
                let bob = behaviors::BehaviorInstance {
                    id: format!("collectible_bob_{}", entity_name),
                    def: BehaviorDef::Bob {
                        axis: [0.0, 1.0, 0.0],
                        amplitude: 0.2,
                        frequency: 0.5,
                        phase: 0.0,
                    },
                    base_position: position,
                    base_scale: Vec3::ONE,
                };
                let spin = behaviors::BehaviorInstance {
                    id: format!("collectible_spin_{}", entity_name),
                    def: BehaviorDef::Spin {
                        axis: [0.0, 1.0, 0.0],
                        speed: 45.0,
                    },
                    base_position: position,
                    base_scale: Vec3::ONE,
                };
                commands.entity(entity).insert((
                    crate::interaction::InteractionEntity,
                    crate::interaction::Collectible {
                        value: p.value,
                        category: p.category.clone(),
                        pickup_effect: p.pickup_effect,
                        respawn_time: p.respawn_time,
                        original_position: position,
                        respawn_timer: None,
                    },
                    EntityBehaviors {
                        behaviors: vec![bob, spin],
                    },
                ));
                let _ = channel_res
                    .channels
                    .resp_tx
                    .send(GenResponse::Modified { name: entity_name });
                continue;
            }
            GenCommand::AddDoor(p) => {
                let Some(entity) = params.registry.get_entity(&p.entity_id) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.entity_id),
                    });
                    continue;
                };
                let entity_name = p.entity_id.clone();
                let rotation = params
                    .transforms
                    .get(entity)
                    .map(|t| t.rotation)
                    .unwrap_or(Quat::IDENTITY);
                commands.entity(entity).insert((
                    crate::interaction::InteractionEntity,
                    crate::interaction::Door {
                        state: crate::interaction::DoorState::Closed,
                        open_angle: p.open_angle,
                        open_duration: p.open_duration,
                        auto_close: p.auto_close,
                        auto_close_delay: p.auto_close_delay,
                        requires_key: p.requires_key.clone(),
                        original_rotation: rotation,
                    },
                ));
                if p.trigger == crate::interaction::DoorTrigger::Proximity {
                    commands
                        .entity(entity)
                        .insert(crate::interaction::ProximityTrigger {
                            radius: 3.0,
                            cooldown: 0.5,
                            last_triggered: 0.0,
                        });
                } else {
                    commands
                        .entity(entity)
                        .insert(crate::interaction::ClickTrigger {
                            max_distance: 3.0,
                            prompt_text: Some("Open".to_string()),
                        });
                }
                let _ = channel_res
                    .channels
                    .resp_tx
                    .send(GenResponse::Modified { name: entity_name });
                continue;
            }
            GenCommand::LinkEntities(p) => {
                let Some(entity) = params.registry.get_entity(&p.source_id) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.source_id),
                    });
                    continue;
                };
                let source_name = p.source_id.clone();
                commands
                    .entity(entity)
                    .insert(crate::interaction::EntityLink {
                        source_event: p.source_event.clone(),
                        target_entity: p.target_id.clone(),
                        target_action: p.target_action.clone(),
                        condition: p.condition.clone(),
                    });
                let _ = channel_res
                    .channels
                    .resp_tx
                    .send(GenResponse::Modified { name: source_name });
                continue;
            }

            // Tier 12: Terrain & Landscape (P3)
            GenCommand::AddTerrain(p) => {
                let name = "Terrain".to_string();
                let wid = params.next_entity_id.alloc();
                let mesh = params.meshes.add(crate::terrain::generate_terrain_mesh(&p));
                let color = match p.material {
                    crate::terrain::TerrainMaterial::Grass => Color::srgb(0.3, 0.6, 0.2),
                    crate::terrain::TerrainMaterial::Sand => Color::srgb(0.76, 0.7, 0.5),
                    crate::terrain::TerrainMaterial::Snow => Color::srgb(0.95, 0.95, 0.97),
                    crate::terrain::TerrainMaterial::Rock => Color::srgb(0.5, 0.5, 0.5),
                    crate::terrain::TerrainMaterial::Custom => Color::srgb(0.6, 0.6, 0.6),
                };
                let material = params.materials.add(StandardMaterial {
                    base_color: color,
                    perceptual_roughness: 0.9,
                    ..default()
                });
                let entity = commands
                    .spawn((
                        Name::new(name.clone()),
                        GenEntity {
                            entity_type: GenEntityType::Primitive,
                            world_id: wid,
                        },
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::from_translation(p.position),
                        Visibility::default(),
                        crate::terrain::Terrain {
                            size: p.size,
                            resolution: p.resolution,
                            height_scale: p.height_scale,
                            noise_type: p.noise_type,
                            noise_frequency: p.noise_frequency,
                            noise_octaves: p.noise_octaves,
                            seed: p.seed.unwrap_or_else(rand::random::<u32>),
                        },
                    ))
                    .id();
                params.registry.insert_with_id(name.clone(), entity, wid);
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::AddWater(p) => {
                let name = "Water".to_string();
                let wid = params.next_entity_id.alloc();
                let mesh = params.meshes.add(crate::terrain::generate_water_mesh(&p));
                let hex = p.color.trim_start_matches('#');
                let (r, g, b) = if hex.len() == 6 {
                    (
                        u8::from_str_radix(&hex[0..2], 16).unwrap_or(35) as f32 / 255.0,
                        u8::from_str_radix(&hex[2..4], 16).unwrap_or(137) as f32 / 255.0,
                        u8::from_str_radix(&hex[4..6], 16).unwrap_or(218) as f32 / 255.0,
                    )
                } else {
                    (0.14, 0.54, 0.85)
                };
                let material = params.materials.add(StandardMaterial {
                    base_color: Color::srgba(r, g, b, p.opacity),
                    alpha_mode: AlphaMode::Blend,
                    perceptual_roughness: 0.1,
                    ..default()
                });
                let pos = p.position.unwrap_or(Vec3::new(0.0, p.height, 0.0));
                let entity = commands
                    .spawn((
                        Name::new(name.clone()),
                        GenEntity {
                            entity_type: GenEntityType::Primitive,
                            world_id: wid,
                        },
                        crate::terrain::Water {
                            wave_speed: p.wave_speed,
                            wave_height: p.wave_height,
                            base_height: p.height,
                        },
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::from_translation(pos),
                        Visibility::default(),
                    ))
                    .id();
                params.registry.insert_with_id(name.clone(), entity, wid);
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::AddPath(p) => {
                let name = "Path".to_string();
                let wid = params.next_entity_id.alloc();
                let path_width = p.width;
                // Mesh is in local space relative to first waypoint
                let origin = crate::terrain::path_origin(&p);
                let mesh = params.meshes.add(crate::terrain::generate_path_mesh(&p));
                let material = params.materials.add(StandardMaterial {
                    base_color: crate::terrain::get_path_material_color(p.material),
                    perceptual_roughness: 0.85,
                    ..default()
                });
                let entity = commands
                    .spawn((
                        Name::new(name.clone()),
                        GenEntity {
                            entity_type: GenEntityType::Primitive,
                            world_id: wid,
                        },
                        crate::terrain::Path { width: path_width },
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        Transform::from_translation(origin),
                        Visibility::default(),
                    ))
                    .id();
                params.registry.insert_with_id(name.clone(), entity, wid);
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::AddFoliage(p) => {
                let points = crate::terrain::generate_foliage_points(&p);
                let foliage_mesh = params
                    .meshes
                    .add(crate::terrain::generate_foliage_mesh(p.foliage_type));
                let color = match p.foliage_type {
                    crate::terrain::FoliageType::Tree => Color::srgb(0.2, 0.5, 0.15),
                    crate::terrain::FoliageType::Bush => Color::srgb(0.25, 0.55, 0.2),
                    crate::terrain::FoliageType::Grass => Color::srgb(0.3, 0.65, 0.2),
                    crate::terrain::FoliageType::Flower => Color::srgb(0.8, 0.3, 0.5),
                    crate::terrain::FoliageType::Rock => Color::srgb(0.5, 0.5, 0.5),
                };
                let foliage_material = params.materials.add(StandardMaterial {
                    base_color: color,
                    ..default()
                });
                let count = points.len();
                // Spawn parent group
                let name = "Foliage".to_string();
                let wid = params.next_entity_id.alloc();
                let parent_entity = commands
                    .spawn((
                        Name::new(name.clone()),
                        GenEntity {
                            entity_type: GenEntityType::Group,
                            world_id: wid,
                        },
                        crate::terrain::Foliage {
                            foliage_type: p.foliage_type,
                        },
                        Transform::from_translation(p.area.center),
                        Visibility::default(),
                    ))
                    .id();
                // Spawn each foliage instance as child
                for pt in &points {
                    commands.spawn((
                        Mesh3d(foliage_mesh.clone()),
                        MeshMaterial3d(foliage_material.clone()),
                        Transform::from_translation(*pt),
                        Visibility::default(),
                        ChildOf(parent_entity),
                    ));
                }
                params
                    .registry
                    .insert_with_id(name.clone(), parent_entity, wid);
                let _ = channel_res.channels.resp_tx.send(GenResponse::Spawned {
                    name: format!("Foliage ({} instances)", count),
                    entity_id: wid.0,
                });
                continue;
            }
            GenCommand::SetSky(p) => {
                let config = crate::terrain::SkyConfig::from_preset(p.preset).with_overrides(&p);
                // Apply sky config: update directional light and ambient
                commands.insert_resource(config);
                GenResponse::EnvironmentSet
            }
            GenCommand::QueryTerrainHeight { points } => {
                // Find terrain entity
                let terrain_entity = params
                    .registry
                    .all_names()
                    .find(|(_, e)| params.terrain_q.get(*e).is_ok())
                    .map(|(_, e)| e);

                if let Some(entity) = terrain_entity
                    && let Ok((terrain, terrain_transform)) = params.terrain_q.get(entity)
                {
                    let heights: Vec<[f32; 3]> = points
                        .iter()
                        .map(|[x, z]| {
                            let y =
                                terrain.sample_height(Vec3::new(*x, 0.0, *z), terrain_transform);
                            [*x, y, *z]
                        })
                        .collect();
                    GenResponse::TerrainHeights { heights }
                } else {
                    GenResponse::Error {
                        message: "No terrain found in scene".to_string(),
                    }
                }
            }

            // Tier 13: In-World Text & UI (P4)
            GenCommand::AddSign(p) => {
                let name = format!("Sign_{}", p.text.chars().take(10).collect::<String>());
                let wid = params.next_entity_id.alloc();
                let text_color = crate::ui::parse_sign_color(&p.color).unwrap_or(Color::WHITE);
                // Scale based on font_size (24 is default → 0.02 scale)
                let scale = p.font_size / 1200.0;
                let mut transform =
                    Transform::from_translation(p.position).with_scale(Vec3::splat(scale));
                // Apply rotation when not billboard
                if !p.billboard && p.rotation != Vec3::ZERO {
                    transform.rotation = Quat::from_euler(
                        EulerRot::YXZ,
                        p.rotation.y.to_radians(),
                        p.rotation.x.to_radians(),
                        p.rotation.z.to_radians(),
                    );
                }
                let mut ec = commands.spawn((
                    Name::new(name.clone()),
                    GenEntity {
                        entity_type: GenEntityType::Primitive,
                        world_id: wid,
                    },
                    Text2d::new(p.text.clone()),
                    TextColor(text_color),
                    TextLayout::justify(bevy::text::Justify::Center),
                    transform,
                    Visibility::default(),
                    crate::ui::Sign {
                        billboard: p.billboard,
                        text: p.text.clone(),
                    },
                ));
                // Word wrap if max_width is set
                if let Some(max_width) = p.max_width {
                    ec.insert(bevy::text::TextBounds::new_horizontal(max_width));
                }
                let entity = ec.id();
                // Background panel behind text
                if let Some(bg_color) = p
                    .background_color
                    .as_deref()
                    .and_then(crate::ui::parse_sign_color)
                {
                    let bg_mesh = params
                        .meshes
                        .add(Plane3d::new(Vec3::Z, Vec2::new(2.0, 0.6)));
                    let bg_mat = params.materials.add(StandardMaterial {
                        base_color: bg_color.with_alpha(0.85),
                        alpha_mode: AlphaMode::Blend,
                        unlit: true,
                        ..default()
                    });
                    commands.spawn((
                        Name::new("SignBackground"),
                        crate::ui::SignBackground,
                        Mesh3d(bg_mesh),
                        MeshMaterial3d(bg_mat),
                        Transform::from_xyz(0.0, 0.0, -0.01),
                        ChildOf(entity),
                    ));
                }
                params.registry.insert_with_id(name.clone(), entity, wid);
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::AddHud(p) => {
                let name =
                    p.id.clone()
                        .unwrap_or_else(|| format!("HUD_{:?}", p.element_type));
                let text_color = crate::ui::parse_sign_color(&p.color).unwrap_or(Color::WHITE);
                let display_text = if let Some(ref label) = p.label {
                    format!("{}: {}", label, p.initial_value)
                } else {
                    p.initial_value.clone()
                };
                let wid = params.next_entity_id.alloc();
                let entity = commands
                    .spawn((
                        Name::new(name.clone()),
                        GenEntity {
                            entity_type: GenEntityType::Primitive,
                            world_id: wid,
                        },
                        crate::ui::HudElement {
                            element_type: p.element_type,
                            position: p.position,
                            id: p.id.clone(),
                            value: p.initial_value.clone(),
                            label: p.label.clone(),
                        },
                        crate::ui::hud_position_node(p.position),
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                        Text::new(display_text),
                        TextColor(text_color),
                        TextFont {
                            font_size: FontSize::Px(p.font_size),
                            ..default()
                        },
                    ))
                    .id();
                params.registry.insert_with_id(name.clone(), entity, wid);
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::AddLabel(p) => {
                let Some(target) = params.registry.get_entity(&p.entity_id) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.entity_id),
                    });
                    continue;
                };
                let entity_name = p.entity_id.clone();
                let text_color = crate::ui::parse_sign_color(&p.color).unwrap_or(Color::WHITE);
                // Spawn label as child of target entity
                let label_entity = commands
                    .spawn((
                        Name::new(format!("Label_{}", entity_name)),
                        crate::ui::EntityLabel {
                            text: p.text.clone(),
                            offset_y: p.offset_y,
                            visible_distance: p.visible_distance,
                            current_alpha: 1.0,
                            parent_entity: target,
                        },
                        Text2d::new(p.text.clone()),
                        TextColor(text_color),
                        TextLayout::justify(bevy::text::Justify::Center),
                        Transform::from_xyz(0.0, p.offset_y, 0.0)
                            .with_scale(Vec3::splat(p.font_size / 1600.0)),
                        Visibility::default(),
                        ChildOf(target),
                    ))
                    .id();
                // Background quad behind label text
                if let Some(bg_color) = crate::ui::parse_sign_color(&p.background_color) {
                    let bg_mesh = params
                        .meshes
                        .add(Plane3d::new(Vec3::Z, Vec2::new(1.5, 0.3)));
                    let bg_mat = params.materials.add(StandardMaterial {
                        base_color: bg_color,
                        alpha_mode: AlphaMode::Blend,
                        unlit: true,
                        ..default()
                    });
                    commands.spawn((
                        Name::new("LabelBackground"),
                        crate::ui::LabelBackground,
                        Mesh3d(bg_mesh),
                        MeshMaterial3d(bg_mat),
                        Transform::from_xyz(0.0, 0.0, -0.01),
                        ChildOf(label_entity),
                    ));
                }
                let _ = channel_res
                    .channels
                    .resp_tx
                    .send(GenResponse::Modified { name: entity_name });
                continue;
            }
            GenCommand::AddTooltip(p) => {
                let Some(target) = params.registry.get_entity(&p.entity_id) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.entity_id),
                    });
                    continue;
                };
                let entity_name = p.entity_id.clone();
                let duration_secs = p.duration.unwrap_or(3.0);
                commands.entity(target).insert(crate::ui::Tooltip {
                    entity: target,
                    text: p.text.clone(),
                    trigger: p.trigger,
                    range: p.range,
                    style: p.style,
                    visible: false,
                    fade_alpha: 0.0,
                    display_timer: Timer::from_seconds(duration_secs, TimerMode::Once),
                    cooldown_timer: Timer::from_seconds(1.0, TimerMode::Once),
                });
                let _ = channel_res
                    .channels
                    .resp_tx
                    .send(GenResponse::Modified { name: entity_name });
                continue;
            }
            GenCommand::AddNotification(p) => {
                let text = p.text.clone();
                let icon_text = crate::ui::get_notification_icon_text(p.icon);
                let display_text = if icon_text.is_empty() {
                    p.text.clone()
                } else {
                    format!("{} {}", icon_text, p.text)
                };
                let text_color = crate::ui::parse_sign_color(&p.color).unwrap_or(Color::WHITE);
                let notif_entity = commands
                    .spawn((
                        Name::new("Notification"),
                        crate::ui::Notification {
                            text: p.text.clone(),
                            style: p.style,
                            position: p.position,
                            phase: crate::ui::NotificationPhase::EnterIn,
                            elapsed: 0.0,
                            duration: p.duration,
                            stack_offset: 0.0,
                            alpha: 0.0,
                        },
                        crate::ui::notification_position_node(p.position),
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                        Text::new(display_text),
                        TextColor(text_color),
                        TextFont {
                            font_size: FontSize::Px(16.0),
                            ..default()
                        },
                    ))
                    .id();
                // Add to notification queue for stacking/limit management
                commands.queue(move |world: &mut World| {
                    let mut to_despawn = Vec::new();
                    {
                        let mut queue = world.resource_mut::<crate::ui::NotificationQueue>();
                        queue.notifications.push(notif_entity);
                        while queue.notifications.len() > 4 {
                            if let Some(oldest) = queue.notifications.first().copied() {
                                queue.notifications.remove(0);
                                to_despawn.push(oldest);
                            }
                        }
                    }
                    for entity in to_despawn {
                        if let Ok(ec) = world.get_entity_mut(entity) {
                            ec.despawn();
                        }
                    }
                });
                let _ = channel_res
                    .channels
                    .resp_tx
                    .send(GenResponse::Modified { name: text });
                continue;
            }

            // Tier 14: Physics Integration (P5)
            // Physics requires avian3d which is behind the "physics" feature gate.
            // These commands store configuration but actual physics simulation
            // requires `cargo build --features physics`.
            GenCommand::SetPhysics(p) => {
                let Some(entity) = params.registry.get_entity(&p.entity_id) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.entity_id),
                    });
                    continue;
                };
                let entity_name = p.entity_id.clone();
                let mass = p.mass.unwrap_or(1.0);
                commands.entity(entity).insert(crate::physics::PhysicsBody {
                    body_type: p.body_type,
                    mass,
                    restitution: p.restitution,
                    friction: p.friction,
                    gravity_scale: p.gravity_scale,
                    linear_damping: p.linear_damping,
                    angular_damping: p.angular_damping,
                });
                if p.lock_rotation {
                    commands
                        .entity(entity)
                        .insert(crate::physics::RotationLocked);
                }
                let _ = channel_res
                    .channels
                    .resp_tx
                    .send(GenResponse::Modified { name: entity_name });
                continue;
            }
            GenCommand::AddCollider(p) => {
                let Some(entity) = params.registry.get_entity(&p.entity_id) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.entity_id),
                    });
                    continue;
                };
                let entity_name = p.entity_id.clone();
                commands
                    .entity(entity)
                    .insert(crate::physics::ColliderConfig {
                        shape: p.shape,
                        size: p.size.map(|s| Vec3::new(s.x, s.y, s.z)),
                        offset: p.offset,
                        is_trigger: p.is_trigger,
                        visible_in_debug: p.visible_in_debug,
                    });
                if p.is_trigger {
                    commands
                        .entity(entity)
                        .insert(crate::physics::SensorCollider);
                }
                let _ = channel_res
                    .channels
                    .resp_tx
                    .send(GenResponse::Modified { name: entity_name });
                continue;
            }
            GenCommand::AddJoint(p) => {
                let Some(entity_a) = params.registry.get_entity(&p.entity_a) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.entity_a),
                    });
                    continue;
                };
                let Some(entity_b) = params.registry.get_entity(&p.entity_b) else {
                    let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                        message: format!("Entity '{}' not found", p.entity_b),
                    });
                    continue;
                };
                let name = format!("Joint_{}_{}", p.entity_a, p.entity_b);
                let wid = params.next_entity_id.alloc();
                let entity = commands
                    .spawn((
                        Name::new(name.clone()),
                        GenEntity {
                            entity_type: GenEntityType::Primitive,
                            world_id: wid,
                        },
                        crate::physics::JointConfig {
                            joint_type: p.joint_type,
                            entity_a,
                            entity_b,
                            anchor_a: p.anchor_a,
                            anchor_b: p.anchor_b,
                            axis: p.axis,
                            limits: p.limits,
                            stiffness: p.stiffness,
                            damping: p.damping,
                        },
                    ))
                    .id();
                params.registry.insert_with_id(name.clone(), entity, wid);
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::AddForce(p) => {
                let name = "ForceField".to_string();
                let wid = params.next_entity_id.alloc();
                let entity = commands
                    .spawn((
                        Name::new(name.clone()),
                        GenEntity {
                            entity_type: GenEntityType::Primitive,
                            world_id: wid,
                        },
                        Transform::from_translation(p.position),
                        Visibility::default(),
                        crate::physics::ForceField {
                            force_type: p.force_type,
                            strength: p.strength,
                            radius: p.radius,
                            direction: p.direction.unwrap_or(Vec3::Y),
                            falloff: p.falloff,
                            affects_player: p.affects_player,
                            continuous: p.continuous,
                        },
                    ))
                    .id();
                params.registry.insert_with_id(name.clone(), entity, wid);
                GenResponse::Spawned {
                    name,
                    entity_id: wid.0,
                }
            }
            GenCommand::SetGravity(p) => {
                if let Some(zone_pos) = p.zone_position {
                    // Spawn a gravity zone entity
                    let radius = p.zone_radius.unwrap_or(10.0);
                    let name = "GravityZone".to_string();
                    let wid = params.next_entity_id.alloc();
                    let entity = commands
                        .spawn((
                            Name::new(name.clone()),
                            GenEntity {
                                entity_type: GenEntityType::Primitive,
                                world_id: wid,
                            },
                            Transform::from_translation(zone_pos),
                            Visibility::default(),
                            crate::physics::GravityZone {
                                center: zone_pos,
                                radius,
                                gravity: p.direction * p.strength,
                                transition_duration: p.transition_duration,
                            },
                        ))
                        .id();
                    params.registry.insert_with_id(name.clone(), entity, wid);
                    GenResponse::Spawned {
                        name,
                        entity_id: wid.0,
                    }
                } else if let Some(ref entity_id) = p.entity_id {
                    // Per-entity gravity override
                    let Some(entity) = params.registry.get_entity(entity_id) else {
                        let _ = channel_res.channels.resp_tx.send(GenResponse::Error {
                            message: format!("Entity '{}' not found", entity_id),
                        });
                        continue;
                    };
                    let entity_name = entity_id.clone();
                    commands
                        .entity(entity)
                        .insert(crate::physics::GravityOverride {
                            direction: p.direction,
                            strength_scale: p.strength / crate::physics::GRAVITY_EARTH,
                        });
                    let _ = channel_res
                        .channels
                        .resp_tx
                        .send(GenResponse::Modified { name: entity_name });
                    continue;
                } else {
                    // Update global gravity resource
                    commands.insert_resource(crate::physics::GlobalGravity {
                        gravity: p.direction * p.strength,
                        target_gravity: p.direction * p.strength,
                        transition_progress: 1.0,
                        transition_duration: p.transition_duration,
                    });
                    GenResponse::EnvironmentSet
                }
            }

            // Tier 15: WorldGen Pipeline (WG1)
            GenCommand::PlanLayout { prompt, size, seed } => {
                let spec = crate::worldgen::BlockoutSpec::from_prompt(&prompt, size, seed);
                match spec.validate() {
                    Ok(()) => match serde_json::to_string_pretty(&spec) {
                        Ok(json) => {
                            commands.insert_resource(crate::worldgen::CurrentBlockout { spec });
                            GenResponse::BlockoutPlan { spec_json: json }
                        }
                        Err(e) => GenResponse::Error {
                            message: format!("Failed to serialize BlockoutSpec: {}", e),
                        },
                    },
                    Err(e) => GenResponse::Error { message: e },
                }
            }

            GenCommand::ApplyBlockout {
                spec,
                show_debug_volumes,
                generate_terrain,
                generate_paths,
            } => {
                let mut entities_spawned = 0usize;

                // Store the blockout as a resource
                commands.insert_resource(crate::worldgen::CurrentBlockout { spec: spec.clone() });

                // 1. Generate terrain
                if generate_terrain {
                    let world_size = if !spec.regions.is_empty() {
                        // Use the bounding box of all regions
                        let max_x = spec
                            .regions
                            .iter()
                            .map(|r| r.bounds.center[0].abs() + r.bounds.size[0] / 2.0)
                            .fold(0.0f32, f32::max);
                        let max_z = spec
                            .regions
                            .iter()
                            .map(|r| r.bounds.center[1].abs() + r.bounds.size[1] / 2.0)
                            .fold(0.0f32, f32::max);
                        bevy::math::Vec2::new(max_x * 2.2, max_z * 2.2)
                            .max(bevy::math::Vec2::splat(20.0))
                    } else {
                        bevy::math::Vec2::splat(50.0)
                    };

                    let terrain_params = spec.terrain.to_terrain_params(world_size);

                    // Map biome to terrain material
                    let material = match spec.palette.primary_biome {
                        crate::worldgen::Biome::Desert | crate::worldgen::Biome::Savanna => {
                            crate::terrain::TerrainMaterial::Sand
                        }
                        crate::worldgen::Biome::Arctic | crate::worldgen::Biome::Tundra => {
                            crate::terrain::TerrainMaterial::Snow
                        }
                        crate::worldgen::Biome::Volcanic => crate::terrain::TerrainMaterial::Rock,
                        _ => crate::terrain::TerrainMaterial::Grass,
                    };

                    let mut tp = terrain_params;
                    tp.material = material;

                    let mesh = params
                        .meshes
                        .add(crate::terrain::generate_terrain_mesh(&tp));
                    let color = match tp.material {
                        crate::terrain::TerrainMaterial::Grass => Color::srgb(0.3, 0.6, 0.2),
                        crate::terrain::TerrainMaterial::Sand => Color::srgb(0.76, 0.7, 0.5),
                        crate::terrain::TerrainMaterial::Snow => Color::srgb(0.9, 0.92, 0.95),
                        crate::terrain::TerrainMaterial::Rock => Color::srgb(0.5, 0.48, 0.45),
                        crate::terrain::TerrainMaterial::Custom => Color::srgb(0.5, 0.5, 0.5),
                    };
                    let mat = params.materials.add(StandardMaterial {
                        base_color: color,
                        perceptual_roughness: 0.9,
                        ..default()
                    });
                    let wid = params.next_entity_id.alloc();
                    let name = format!("blockout_terrain_{}", wid.0);
                    let entity = commands
                        .spawn((
                            Mesh3d(mesh),
                            MeshMaterial3d(mat),
                            Transform::from_translation(tp.position),
                            crate::gen3d::registry::GenEntity {
                                entity_type: crate::gen3d::registry::GenEntityType::Primitive,
                                world_id: wid,
                            },
                            crate::terrain::Terrain {
                                size: tp.size,
                                resolution: tp.resolution,
                                height_scale: tp.height_scale,
                                noise_type: tp.noise_type,
                                noise_frequency: tp.noise_frequency,
                                noise_octaves: tp.noise_octaves,
                                seed: tp.seed.unwrap_or(0),
                            },
                            crate::worldgen::BlockoutGenerated {
                                region_id: "_terrain".to_string(),
                                pass: "terrain".to_string(),
                            },
                            crate::worldgen::SemanticRole::Ground,
                        ))
                        .id();
                    params.registry.insert_with_id(name.clone(), entity, wid);
                    entities_spawned += 1;
                }

                // 2. Spawn region debug volumes
                let region_count = spec.regions.len();
                if show_debug_volumes {
                    let region_colors = [
                        Color::srgba(0.2, 0.6, 1.0, 0.15),
                        Color::srgba(0.2, 1.0, 0.4, 0.15),
                        Color::srgba(1.0, 0.6, 0.2, 0.15),
                        Color::srgba(0.8, 0.2, 1.0, 0.15),
                        Color::srgba(1.0, 1.0, 0.2, 0.15),
                        Color::srgba(0.2, 1.0, 1.0, 0.15),
                    ];

                    for (i, region) in spec.regions.iter().enumerate() {
                        let color = region_colors[i % region_colors.len()];
                        let cx = region.bounds.center[0];
                        let cz = region.bounds.center[1];
                        let sx = region.bounds.size[0];
                        let sz = region.bounds.size[1];
                        let height = 10.0;

                        let mesh = params.meshes.add(Cuboid::new(sx, height, sz));
                        let mat = params.materials.add(StandardMaterial {
                            base_color: color,
                            alpha_mode: AlphaMode::Blend,
                            unlit: true,
                            ..default()
                        });

                        let wid = params.next_entity_id.alloc();
                        let name = format!("blockout_region_{}_{}", region.id, wid.0);
                        let entity = commands
                            .spawn((
                                Mesh3d(mesh),
                                MeshMaterial3d(mat),
                                Transform::from_xyz(cx, height / 2.0, cz),
                                crate::gen3d::registry::GenEntity {
                                    entity_type: crate::gen3d::registry::GenEntityType::Primitive,
                                    world_id: wid,
                                },
                                crate::worldgen::BlockoutVolume {
                                    region_id: region.id.clone(),
                                    role: "region".to_string(),
                                    hint: String::new(),
                                },
                                crate::worldgen::BlockoutGenerated {
                                    region_id: region.id.clone(),
                                    pass: "volume".to_string(),
                                },
                            ))
                            .id();
                        params.registry.insert_with_id(name.clone(), entity, wid);
                        entities_spawned += 1;

                        // Spawn hero slot markers (gold translucent boxes)
                        for slot in &region.hero_slots {
                            let slot_mesh = params.meshes.add(Cuboid::new(
                                slot.size[0],
                                slot.size[1],
                                slot.size[2],
                            ));
                            let slot_mat = params.materials.add(StandardMaterial {
                                base_color: Color::srgba(1.0, 0.85, 0.3, 0.25),
                                alpha_mode: AlphaMode::Blend,
                                unlit: true,
                                ..default()
                            });
                            let slot_wid = params.next_entity_id.alloc();
                            let slot_name = format!("blockout_hero_{}_{}", region.id, slot_wid.0);
                            let slot_entity = commands
                                .spawn((
                                    Mesh3d(slot_mesh),
                                    MeshMaterial3d(slot_mat),
                                    Transform::from_xyz(
                                        slot.position[0],
                                        slot.position[1] + slot.size[1] / 2.0,
                                        slot.position[2],
                                    ),
                                    crate::gen3d::registry::GenEntity {
                                        entity_type:
                                            crate::gen3d::registry::GenEntityType::Primitive,
                                        world_id: slot_wid,
                                    },
                                    crate::worldgen::BlockoutVolume {
                                        region_id: region.id.clone(),
                                        role: slot.role.clone(),
                                        hint: slot.hint.clone(),
                                    },
                                    crate::worldgen::BlockoutGenerated {
                                        region_id: region.id.clone(),
                                        pass: "hero".to_string(),
                                    },
                                ))
                                .id();
                            params.registry.insert_with_id(
                                slot_name.clone(),
                                slot_entity,
                                slot_wid,
                            );
                            entities_spawned += 1;
                        }
                    }
                }

                // 3. Generate connecting paths
                let mut path_count = 0usize;
                if generate_paths {
                    for path_conn in &spec.paths {
                        // Find region centers
                        let from_center = spec
                            .regions
                            .iter()
                            .find(|r| r.id == path_conn.from)
                            .map(|r| r.bounds.center);
                        let to_center = spec
                            .regions
                            .iter()
                            .find(|r| r.id == path_conn.to)
                            .map(|r| r.bounds.center);

                        if let (Some(from), Some(to)) = (from_center, to_center) {
                            let path_material = match path_conn.style {
                                crate::worldgen::PathStyle::Stone
                                | crate::worldgen::PathStyle::Cobblestone => {
                                    crate::terrain::PathMaterial::Stone
                                }
                                _ => crate::terrain::PathMaterial::Dirt,
                            };

                            let path_params = crate::terrain::PathParams {
                                points: vec![
                                    bevy::math::Vec3::new(from[0], 0.0, from[1]),
                                    bevy::math::Vec3::new(
                                        (from[0] + to[0]) / 2.0,
                                        0.0,
                                        (from[1] + to[1]) / 2.0,
                                    ),
                                    bevy::math::Vec3::new(to[0], 0.0, to[1]),
                                ],
                                width: path_conn.width,
                                material: path_material,
                                ..Default::default()
                            };

                            let origin = crate::terrain::path_origin(&path_params);
                            let mesh = params
                                .meshes
                                .add(crate::terrain::generate_path_mesh(&path_params));
                            let color = match path_params.material {
                                crate::terrain::PathMaterial::Dirt => Color::srgb(0.55, 0.42, 0.28),
                                crate::terrain::PathMaterial::Stone
                                | crate::terrain::PathMaterial::Cobblestone => {
                                    Color::srgb(0.6, 0.58, 0.55)
                                }
                                crate::terrain::PathMaterial::Wood => Color::srgb(0.55, 0.35, 0.2),
                                crate::terrain::PathMaterial::Custom => Color::srgb(0.5, 0.5, 0.5),
                            };
                            let mat = params.materials.add(StandardMaterial {
                                base_color: color,
                                perceptual_roughness: 0.95,
                                ..default()
                            });

                            let wid = params.next_entity_id.alloc();
                            let name = format!(
                                "blockout_path_{}_to_{}_{}",
                                path_conn.from, path_conn.to, wid.0
                            );
                            let entity = commands
                                .spawn((
                                    Mesh3d(mesh),
                                    MeshMaterial3d(mat),
                                    Transform::from_translation(origin),
                                    crate::gen3d::registry::GenEntity {
                                        entity_type:
                                            crate::gen3d::registry::GenEntityType::Primitive,
                                        world_id: wid,
                                    },
                                    crate::worldgen::BlockoutGenerated {
                                        region_id: format!(
                                            "path_{}_{}",
                                            path_conn.from, path_conn.to
                                        ),
                                        pass: "path".to_string(),
                                    },
                                    crate::worldgen::SemanticRole::Ground,
                                ))
                                .id();
                            params.registry.insert_with_id(name.clone(), entity, wid);
                            entities_spawned += 1;
                            path_count += 1;
                        }
                    }
                }

                GenResponse::BlockoutApplied {
                    entities_spawned,
                    regions: region_count,
                    paths: path_count,
                }
            }

            GenCommand::PopulateRegion {
                region_id,
                style_hint: _,
                replace_existing,
            } => {
                // Look up the current blockout
                match &params.current_blockout {
                    None => GenResponse::Error {
                        message: "No blockout loaded. Call gen_apply_blockout first.".to_string(),
                    },
                    Some(blockout) => {
                        let region = blockout.spec.regions.iter().find(|r| r.id == region_id);
                        match region {
                            None => GenResponse::Error {
                                message: format!("Region '{}' not found in blockout", region_id),
                            },
                            Some(region) => {
                                let mut spawned = 0usize;

                                // If replace_existing, despawn entities in this region
                                if replace_existing {
                                    let mut to_remove = Vec::new();
                                    for (entity, bg) in params.blockout_generated_q.iter() {
                                        if bg.region_id == region_id
                                            && bg.pass != "volume"
                                            && bg.pass != "terrain"
                                        {
                                            to_remove.push(entity);
                                        }
                                    }
                                    for entity in to_remove {
                                        if let Some(name) = params.registry.get_name(entity) {
                                            let owned = name.to_string();
                                            params.registry.remove_by_name(&owned);
                                        }
                                        commands.entity(entity).despawn();
                                    }
                                }

                                // Scatter foliage based on density
                                if region.walkable && region.decorative_density > 0.0 {
                                    let cx = region.bounds.center[0];
                                    let cz = region.bounds.center[1];
                                    let radius =
                                        region.bounds.size[0].min(region.bounds.size[1]) / 2.0;

                                    let foliage_params = crate::terrain::FoliageParams {
                                        area: crate::terrain::FoliageArea {
                                            center: bevy::math::Vec3::new(cx, 0.0, cz),
                                            radius,
                                        },
                                        density: region.decorative_density,
                                        ..Default::default()
                                    };

                                    let points =
                                        crate::terrain::generate_foliage_points(&foliage_params);

                                    let foliage_color = Color::srgb(0.2, 0.5, 0.15);

                                    for pos in &points {
                                        let foliage_mesh = crate::terrain::generate_foliage_mesh(
                                            crate::terrain::FoliageType::Tree,
                                        );
                                        let mesh = params.meshes.add(foliage_mesh);
                                        let mat = params.materials.add(StandardMaterial {
                                            base_color: foliage_color,
                                            ..default()
                                        });
                                        let wid = params.next_entity_id.alloc();
                                        let name =
                                            format!("blockout_foliage_{}_{}", region_id, wid.0);
                                        let entity = commands
                                            .spawn((
                                                Mesh3d(mesh),
                                                MeshMaterial3d(mat),
                                                Transform::from_translation(*pos),
                                                crate::gen3d::registry::GenEntity {
                                                    entity_type:
                                                        crate::gen3d::registry::GenEntityType::Primitive,
                                                    world_id: wid,
                                                },
                                                crate::worldgen::BlockoutGenerated {
                                                    region_id: region_id.clone(),
                                                    pass: "decorative".to_string(),
                                                },
                                                crate::worldgen::PlacementTier::Decorative,
                                                crate::worldgen::SemanticRole::Vegetation,
                                            ))
                                            .id();
                                        params.registry.insert_with_id(name.clone(), entity, wid);
                                        spawned += 1;
                                    }
                                }

                                GenResponse::RegionPopulated {
                                    region_id,
                                    entities_spawned: spawned,
                                }
                            }
                        }
                    }
                }
            }

            // Tier 16: Hierarchical Placement (WG3) & Scene Decomposition (WG6)
            GenCommand::SetTier { entity_name, tier } => {
                match params.registry.get_entity(&entity_name) {
                    Some(entity) => {
                        commands.entity(entity).insert(tier);
                        GenResponse::TierSet {
                            entity: entity_name,
                            tier: tier.as_str().to_string(),
                        }
                    }
                    None => GenResponse::Error {
                        message: format!("Entity '{}' not found", entity_name),
                    },
                }
            }

            GenCommand::SetRole { entity_name, role } => {
                match params.registry.get_entity(&entity_name) {
                    Some(entity) => {
                        commands.entity(entity).insert(role);
                        GenResponse::RoleSet {
                            entity: entity_name,
                            role: role.as_str().to_string(),
                        }
                    }
                    None => GenResponse::Error {
                        message: format!("Entity '{}' not found", entity_name),
                    },
                }
            }

            GenCommand::BulkModify {
                role,
                region_id,
                action,
            } => {
                let mut affected = 0usize;
                let mut to_process = Vec::new();

                for (name, entity) in params.registry.all_names() {
                    // Check role matches
                    if let Ok(entity_role) = params.role_q.get(entity) {
                        if *entity_role != role {
                            continue;
                        }
                    } else {
                        continue;
                    }

                    // Check region filter
                    if let Some(ref region_filter) = region_id {
                        if let Ok((_, bg)) = params.blockout_generated_q.get(entity) {
                            if bg.region_id != *region_filter {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    }

                    to_process.push((name.to_string(), entity));
                }

                for (name, entity) in &to_process {
                    match &action {
                        BulkAction::Scale { factor } => {
                            if let Ok(transform) = params.transforms.get(*entity) {
                                let scaled = transform.scale * *factor;
                                commands.entity(*entity).insert(
                                    Transform::from_translation(transform.translation)
                                        .with_rotation(transform.rotation)
                                        .with_scale(scaled),
                                );
                                affected += 1;
                            }
                        }
                        BulkAction::Recolor { color } => {
                            if let Ok(handle) = params.material_handles.get(*entity)
                                && let Some(mut mat) = params.materials.get_mut(&handle.0)
                            {
                                mat.base_color =
                                    Color::srgba(color[0], color[1], color[2], color[3]);
                                affected += 1;
                            }
                        }
                        BulkAction::Remove => {
                            params.registry.remove_by_name(name);
                            commands.entity(*entity).despawn();
                            affected += 1;
                        }
                        BulkAction::Hide => {
                            commands.entity(*entity).insert(Visibility::Hidden);
                            affected += 1;
                        }
                        BulkAction::Show => {
                            commands.entity(*entity).insert(Visibility::Inherited);
                            affected += 1;
                        }
                    }
                }

                GenResponse::BulkModified {
                    role: role.as_str().to_string(),
                    action: match &action {
                        BulkAction::Scale { .. } => "scale",
                        BulkAction::Recolor { .. } => "recolor",
                        BulkAction::Remove => "remove",
                        BulkAction::Hide => "hide",
                        BulkAction::Show => "show",
                    }
                    .to_string(),
                    affected,
                }
            }

            // Tier 17: Blockout Editing (WG5)
            GenCommand::ModifyBlockout {
                action,
                auto_regenerate,
            } => {
                // Get mutable blockout spec
                let blockout = params.current_blockout.as_ref().map(|b| b.spec.clone());
                match blockout {
                    None => GenResponse::Error {
                        message: "No blockout loaded. Call gen_apply_blockout first.".to_string(),
                    },
                    Some(mut spec) => {
                        let mut entities_removed = 0usize;
                        let mut entities_spawned_count = 0usize;
                        let (action_name, region_id) = match &action {
                            BlockoutEditAction::AddRegion { region } => {
                                spec.regions.push(region.clone());
                                // Spawn debug volume for the new region
                                let cx = region.bounds.center[0];
                                let cz = region.bounds.center[1];
                                let sx = region.bounds.size[0];
                                let sz = region.bounds.size[1];
                                let height = 10.0;
                                let mesh = params.meshes.add(Cuboid::new(sx, height, sz));
                                let mat = params.materials.add(StandardMaterial {
                                    base_color: Color::srgba(0.2, 0.6, 1.0, 0.15),
                                    alpha_mode: AlphaMode::Blend,
                                    unlit: true,
                                    ..default()
                                });
                                let wid = params.next_entity_id.alloc();
                                let name = format!("blockout_region_{}_{}", region.id, wid.0);
                                let entity = commands
                                    .spawn((
                                        Mesh3d(mesh),
                                        MeshMaterial3d(mat),
                                        Transform::from_xyz(cx, height / 2.0, cz),
                                        crate::gen3d::registry::GenEntity {
                                            entity_type:
                                                crate::gen3d::registry::GenEntityType::Primitive,
                                            world_id: wid,
                                        },
                                        crate::worldgen::BlockoutVolume {
                                            region_id: region.id.clone(),
                                            role: "region".to_string(),
                                            hint: String::new(),
                                        },
                                        crate::worldgen::BlockoutGenerated {
                                            region_id: region.id.clone(),
                                            pass: "volume".to_string(),
                                        },
                                    ))
                                    .id();
                                params.registry.insert_with_id(name, entity, wid);
                                entities_spawned_count += 1;
                                ("add_region".to_string(), region.id.clone())
                            }
                            BlockoutEditAction::RemoveRegion { region_id } => {
                                spec.regions.retain(|r| r.id != *region_id);
                                // Also remove paths referencing this region
                                spec.paths
                                    .retain(|p| p.from != *region_id && p.to != *region_id);
                                // Despawn all entities in this region
                                let mut to_remove = Vec::new();
                                for (entity, bg) in params.blockout_generated_q.iter() {
                                    if bg.region_id == *region_id {
                                        to_remove.push(entity);
                                    }
                                }
                                for entity in to_remove {
                                    if let Some(name) = params.registry.get_name(entity) {
                                        let owned = name.to_string();
                                        params.registry.remove_by_name(&owned);
                                    }
                                    commands.entity(entity).despawn();
                                    entities_removed += 1;
                                }
                                ("remove_region".to_string(), region_id.clone())
                            }
                            BlockoutEditAction::ResizeRegion {
                                region_id,
                                center,
                                size,
                            } => {
                                if let Some(region) =
                                    spec.regions.iter_mut().find(|r| r.id == *region_id)
                                {
                                    region.bounds.center = *center;
                                    region.bounds.size = *size;
                                }
                                // Remove out-of-bounds entities (except volumes)
                                let cx = center[0];
                                let cz = center[1];
                                let hx = size[0] / 2.0;
                                let hz = size[1] / 2.0;
                                let mut to_remove = Vec::new();
                                for (entity, bg) in params.blockout_generated_q.iter() {
                                    if bg.region_id == *region_id
                                        && bg.pass != "volume"
                                        && let Ok(t) = params.transforms.get(entity)
                                    {
                                        let pos = t.translation;
                                        if pos.x < cx - hx
                                            || pos.x > cx + hx
                                            || pos.z < cz - hz
                                            || pos.z > cz + hz
                                        {
                                            to_remove.push(entity);
                                        }
                                    }
                                }
                                for entity in to_remove {
                                    if let Some(name) = params.registry.get_name(entity) {
                                        let owned = name.to_string();
                                        params.registry.remove_by_name(&owned);
                                    }
                                    commands.entity(entity).despawn();
                                    entities_removed += 1;
                                }
                                // Update debug volume (find and update transform + mesh)
                                for (entity, bv) in params.blockout_generated_q.iter() {
                                    if bv.region_id == *region_id && bv.pass == "volume" {
                                        let height = 10.0;
                                        let new_mesh = params
                                            .meshes
                                            .add(Cuboid::new(size[0], height, size[1]));
                                        commands.entity(entity).insert((
                                            Transform::from_xyz(cx, height / 2.0, cz),
                                            Mesh3d(new_mesh),
                                        ));
                                    }
                                }
                                ("resize_region".to_string(), region_id.clone())
                            }
                            BlockoutEditAction::MoveRegion {
                                region_id,
                                new_center,
                            } => {
                                let old_center = spec
                                    .regions
                                    .iter()
                                    .find(|r| r.id == *region_id)
                                    .map(|r| r.bounds.center);
                                if let Some(region) =
                                    spec.regions.iter_mut().find(|r| r.id == *region_id)
                                {
                                    region.bounds.center = *new_center;
                                }
                                // Move all entities by the delta
                                if let Some(old) = old_center {
                                    let dx = new_center[0] - old[0];
                                    let dz = new_center[1] - old[1];
                                    for (entity, bg) in params.blockout_generated_q.iter() {
                                        if bg.region_id == *region_id
                                            && let Ok(t) = params.transforms.get(entity)
                                        {
                                            let new_pos =
                                                t.translation + bevy::math::Vec3::new(dx, 0.0, dz);
                                            commands.entity(entity).insert(
                                                Transform::from_translation(new_pos)
                                                    .with_rotation(t.rotation)
                                                    .with_scale(t.scale),
                                            );
                                        }
                                    }
                                }
                                ("move_region".to_string(), region_id.clone())
                            }
                            BlockoutEditAction::SetDensity { region_id, density } => {
                                if let Some(region) =
                                    spec.regions.iter_mut().find(|r| r.id == *region_id)
                                {
                                    region.decorative_density = *density;
                                }
                                ("set_density".to_string(), region_id.clone())
                            }
                        };

                        // Update the blockout resource
                        commands.insert_resource(crate::worldgen::CurrentBlockout { spec });

                        // Auto-regenerate if requested
                        if auto_regenerate
                            && action_name != "remove_region"
                            && action_name != "set_density"
                        {
                            // Queue a populate command via channel
                            // (simplified: just note it in the response)
                        }

                        GenResponse::BlockoutModified {
                            action: action_name,
                            region_id,
                            entities_removed,
                            entities_spawned: entities_spawned_count,
                        }
                    }
                }
            }

            // Tier 18: Navmesh Infrastructure (WG2)
            GenCommand::BuildNavMesh { settings } => {
                // Collect obstacles from static entities
                let mut obstacles = Vec::new();
                for (gen_ent, tf) in params.gen_entities.iter().zip(params.transforms.iter()) {
                    // Only include primitives and meshes as obstacles
                    match gen_ent.entity_type {
                        crate::gen3d::registry::GenEntityType::Primitive
                        | crate::gen3d::registry::GenEntityType::Mesh => {
                            obstacles.push((tf.translation, tf.scale * 0.5));
                        }
                        _ => {}
                    }
                }

                // Determine world bounds from blockout or scene extents
                let (world_min, world_max) = if let Some(ref blockout) = params.current_blockout {
                    let mut min_x = f32::MAX;
                    let mut min_z = f32::MAX;
                    let mut max_x = f32::MIN;
                    let mut max_z = f32::MIN;
                    for region in &blockout.spec.regions {
                        let cx = region.bounds.center[0];
                        let cz = region.bounds.center[1];
                        let hx = region.bounds.size[0] / 2.0;
                        let hz = region.bounds.size[1] / 2.0;
                        min_x = min_x.min(cx - hx);
                        min_z = min_z.min(cz - hz);
                        max_x = max_x.max(cx + hx);
                        max_z = max_z.max(cz + hz);
                    }
                    if min_x < max_x {
                        (
                            Vec2::new(min_x - 5.0, min_z - 5.0),
                            Vec2::new(max_x + 5.0, max_z + 5.0),
                        )
                    } else {
                        (Vec2::new(-50.0, -50.0), Vec2::new(50.0, 50.0))
                    }
                } else {
                    // Use scene bounds
                    let (center, extent) = compute_scene_bounds_from_entities(
                        &params.gen_entities,
                        &params.transforms,
                    );
                    (
                        Vec2::new(center.x - extent, center.z - extent),
                        Vec2::new(center.x + extent, center.z + extent),
                    )
                };

                // Get terrain height function from terrain query
                let terrain_base_y: f32 = params
                    .terrain_q
                    .iter()
                    .next()
                    .map(|(_, tf)| tf.translation.y)
                    .unwrap_or(0.0);

                let terrain_height = |_x: f32, _z: f32| -> Option<f32> { Some(terrain_base_y) };

                let grid = crate::worldgen::navmesh::build_navgrid(
                    world_min,
                    world_max,
                    &settings,
                    terrain_height,
                    &obstacles,
                );

                let coverage = grid.walkable_coverage();
                let component_count = grid.component_count;
                let cell_count = grid.cells.len();

                // Store the navmesh
                commands.insert_resource(crate::worldgen::NavMeshResource {
                    grid: Some(grid),
                    settings,
                    dirty: false,
                });

                GenResponse::NavMeshBuilt {
                    walkable_coverage: coverage * 100.0,
                    component_count,
                    cell_count,
                }
            }

            GenCommand::ValidateNavigability {
                from,
                to,
                check_all_regions,
            } => {
                let navmesh = params.navmesh_resource.as_ref();
                match navmesh.and_then(|n| n.grid.as_ref()) {
                    None => GenResponse::Error {
                        message: "No navmesh built. Call gen_build_navmesh first.".to_string(),
                    },
                    Some(grid) => {
                        let mut result = crate::worldgen::navmesh::NavigabilityResult {
                            navigable: true,
                            coverage_percent: grid.walkable_coverage() * 100.0,
                            path_found: None,
                            path_length: None,
                            disconnected_regions: Vec::new(),
                            blocked_areas: Vec::new(),
                            component_count: grid.component_count,
                            warnings: Vec::new(),
                        };

                        // Point-to-point check
                        if let (Some(f), Some(t)) = (from, to) {
                            let from_v = Vec3::new(f[0], f[1], f[2]);
                            let to_v = Vec3::new(t[0], t[1], t[2]);
                            match grid.find_path(from_v, to_v) {
                                Some(path) => {
                                    let length: f32 =
                                        path.windows(2).map(|w| (w[1] - w[0]).length()).sum();
                                    result.path_found = Some(true);
                                    result.path_length = Some(length);
                                }
                                None => {
                                    result.path_found = Some(false);
                                    result.navigable = false;
                                }
                            }
                        }

                        // Region connectivity check
                        if check_all_regions && let Some(ref blockout) = params.current_blockout {
                            let regions = &blockout.spec.regions;
                            for i in 0..regions.len() {
                                for j in (i + 1)..regions.len() {
                                    let ci = regions[i].bounds.center;
                                    let cj = regions[j].bounds.center;
                                    let a = Vec3::new(ci[0], 0.0, ci[1]);
                                    let b = Vec3::new(cj[0], 0.0, cj[1]);
                                    if !grid.are_connected(a, b) {
                                        result.disconnected_regions.push(format!(
                                            "{} <-> {}",
                                            regions[i].id, regions[j].id
                                        ));
                                        result.navigable = false;
                                    }
                                }
                            }
                        }

                        // General warnings
                        if grid.component_count > 1 {
                            result.warnings.push(format!(
                                "Scene has {} disconnected walkable regions",
                                grid.component_count
                            ));
                        }
                        if result.coverage_percent < 30.0 {
                            result.warnings.push(format!(
                                "Low walkable coverage: {:.1}% — scene may be too cluttered",
                                result.coverage_percent
                            ));
                        }

                        let json = serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| "{}".to_string());
                        GenResponse::NavigabilityResult { result_json: json }
                    }
                }
            }

            // Tier 19: Navmesh Editing (WG5.2)
            GenCommand::EditNavMesh { action } => {
                let (action_name, desc) = match action {
                    NavMeshEditAction::BlockArea { position, radius } => {
                        params.navmesh_overrides.add_override(
                            crate::worldgen::navmesh_edit::NavMeshOverride {
                                action: crate::worldgen::navmesh_edit::OverrideAction::Block,
                                position,
                                shape: crate::worldgen::navmesh_edit::OverrideShape::Circle {
                                    radius,
                                },
                            },
                        );
                        (
                            "block_area",
                            format!(
                                "Blocked circle r={:.1} at [{:.1}, {:.1}, {:.1}]",
                                radius, position[0], position[1], position[2]
                            ),
                        )
                    }
                    NavMeshEditAction::AllowArea { position, radius } => {
                        params.navmesh_overrides.add_override(
                            crate::worldgen::navmesh_edit::NavMeshOverride {
                                action: crate::worldgen::navmesh_edit::OverrideAction::Allow,
                                position,
                                shape: crate::worldgen::navmesh_edit::OverrideShape::Circle {
                                    radius,
                                },
                            },
                        );
                        (
                            "allow_area",
                            format!(
                                "Allowed circle r={:.1} at [{:.1}, {:.1}, {:.1}]",
                                radius, position[0], position[1], position[2]
                            ),
                        )
                    }
                    NavMeshEditAction::AddConnection {
                        from,
                        to,
                        bidirectional,
                    } => {
                        params.navmesh_overrides.add_connection(
                            crate::worldgen::navmesh_edit::NavMeshConnection {
                                from,
                                to,
                                bidirectional,
                            },
                        );
                        (
                            "add_connection",
                            format!(
                                "Added connection from [{:.1},{:.1},{:.1}] to [{:.1},{:.1},{:.1}]",
                                from[0], from[1], from[2], to[0], to[1], to[2]
                            ),
                        )
                    }
                    NavMeshEditAction::RemoveConnection { from } => {
                        let removed = params.navmesh_overrides.remove_connection_near(from, 2.0);
                        (
                            "remove_connection",
                            if removed {
                                "Connection removed".to_string()
                            } else {
                                "No connection found near that position".to_string()
                            },
                        )
                    }
                };

                GenResponse::NavMeshEdited {
                    action: action_name.to_string(),
                    description: desc,
                }
            }

            // Tier 20: Incremental Regeneration (WG5.3)
            GenCommand::Regenerate {
                region_ids: _,
                preview_only,
                preserve_manual: _,
            } => {
                if preview_only {
                    // Build a preview from current dirty state
                    let preview = crate::worldgen::regenerate::RegenerationPreview {
                        regions: Vec::new(),
                        navmesh_rebuild: false,
                        total_entities_removed: 0,
                        total_entities_estimated: 0,
                    };
                    let json =
                        serde_json::to_string_pretty(&preview).unwrap_or_else(|_| "{}".to_string());
                    GenResponse::RegenerationPreview { preview_json: json }
                } else {
                    GenResponse::Regenerated {
                        regions_processed: 0,
                        entities_removed: 0,
                    }
                }
            }

            GenCommand::RenderDepth {
                config,
                output_path,
            } => {
                use crate::worldgen::depth::*;

                // Compute scene bounds from all GenEntities
                let (scene_center, scene_extent) = {
                    let (c, e) = compute_scene_bounds_from_entities(
                        &params.gen_entities,
                        &params.transforms,
                    );
                    ([c.x, c.y, c.z], e.max(5.0))
                };

                let camera_setup = compute_depth_camera(&config, scene_center, scene_extent);

                // Generate a synthetic depth buffer from entity distances
                let [w, h] = config.resolution;
                let cam_pos = camera_setup.position;
                let look = camera_setup.look_at;

                // Compute forward vector
                let fwd = [
                    look[0] - cam_pos[0],
                    look[1] - cam_pos[1],
                    look[2] - cam_pos[2],
                ];
                let fwd_len = (fwd[0] * fwd[0] + fwd[1] * fwd[1] + fwd[2] * fwd[2]).sqrt();
                let fwd_norm = if fwd_len > 0.0 {
                    [fwd[0] / fwd_len, fwd[1] / fwd_len, fwd[2] / fwd_len]
                } else {
                    [0.0, 0.0, -1.0]
                };

                // For each entity, compute its depth (distance along camera forward axis)
                let mut raw_depths = Vec::new();
                for (_name, bevy_ent) in params.registry.all_names() {
                    let Ok(transform) = params.transforms.get(bevy_ent) else {
                        continue;
                    };
                    let pos = transform.translation;
                    let to_entity = [pos.x - cam_pos[0], pos.y - cam_pos[1], pos.z - cam_pos[2]];
                    let depth = to_entity[0] * fwd_norm[0]
                        + to_entity[1] * fwd_norm[1]
                        + to_entity[2] * fwd_norm[2];
                    raw_depths.push(depth.max(0.0));
                }

                // Fill a depth image — simple nearest-entity depth per pixel
                let mut depth_buf = vec![config.far_plane; (w * h) as usize];

                // If we have entities, compute a basic depth map
                if !raw_depths.is_empty() {
                    let min_depth = raw_depths.iter().copied().fold(f32::MAX, f32::min);
                    let max_depth = raw_depths.iter().copied().fold(f32::MIN, f32::max);
                    let fill_depth = if max_depth > min_depth {
                        (min_depth + max_depth) / 2.0
                    } else {
                        min_depth
                    };
                    depth_buf.fill(fill_depth.clamp(config.near_plane, config.far_plane));
                }

                let mut normalized =
                    normalize_depth(&depth_buf, config.near_plane, config.far_plane);
                if config.add_noise {
                    add_depth_noise(&mut normalized, 0.005, 42);
                }
                let pixels = depth_to_grayscale(&normalized);

                // Determine output path
                let out_path = output_path.unwrap_or_else(|| {
                    let folder = params.workspace.path.join("screenshots");
                    let _ = std::fs::create_dir_all(&folder);
                    let ts = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    folder
                        .join(format!("depth_{ts}.png"))
                        .to_string_lossy()
                        .to_string()
                });

                // Write PNG
                match write_grayscale_png(&out_path, &pixels, w, h) {
                    Ok(()) => {
                        let depth_range = [config.near_plane, config.far_plane];
                        GenResponse::DepthRendered {
                            path: out_path,
                            width: w,
                            height: h,
                            depth_range,
                        }
                    }
                    Err(e) => GenResponse::Error {
                        message: format!("Failed to write depth map: {e}"),
                    },
                }
            }

            GenCommand::PreviewWorld { config } => {
                use crate::worldgen::preview::*;

                let _full_prompt = build_preview_prompt(&config.prompt, config.style_preset);

                let depth_map = config
                    .depth_map_path
                    .clone()
                    .unwrap_or_else(|| "(auto-render needed)".to_string());

                let style_name = config
                    .style_preset
                    .map(|s| format!("{:?}", s).to_lowercase())
                    .unwrap_or_else(|| "custom".to_string());

                let out_path = config.output_path.unwrap_or_else(|| {
                    let folder = params.workspace.path.join("screenshots");
                    let _ = std::fs::create_dir_all(&folder);
                    let ts = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis();
                    folder
                        .join(format!("preview_{ts}.png"))
                        .to_string_lossy()
                        .to_string()
                });

                // NOTE: Actual image generation requires external API (ControlNet/ComfyUI).
                // This handler returns metadata for the generation request.
                GenResponse::PreviewGenerated {
                    path: out_path,
                    style: style_name,
                    depth_map_used: depth_map,
                }
            }

            GenCommand::GenerateAsset {
                prompt,
                name,
                position,
                scale,
                model,
                quality,
                ..
            } => {
                let task_id = params.asset_gen_manager.create_task(
                    crate::gen3d::asset_gen::GenerationTaskType::Mesh,
                    prompt.clone(),
                    name.clone(),
                    model,
                    quality,
                    position,
                    scale,
                );
                let estimated = model.estimated_seconds(quality);
                GenResponse::AssetGenerating {
                    task_id,
                    estimated_seconds: estimated,
                    message: format!(
                        "Generating '{}' with {} ({:?} quality). Will auto-spawn at [{}, {}, {}] when ready.",
                        name,
                        model.display_name(),
                        quality,
                        position[0],
                        position[1],
                        position[2]
                    ),
                }
            }

            GenCommand::GenerateTexture {
                entity,
                prompt,
                style,
                resolution,
            } => {
                let task_id = params.asset_gen_manager.create_task(
                    crate::gen3d::asset_gen::GenerationTaskType::Texture,
                    prompt.clone(),
                    entity.clone(),
                    crate::gen3d::asset_gen::GenerationModel::Hunyuan3d,
                    crate::gen3d::asset_gen::GenerationQuality::Standard,
                    [0.0, 0.0, 0.0],
                    1.0,
                );
                GenResponse::TextureGenerating {
                    task_id,
                    estimated_seconds: 60,
                    message: format!(
                        "Generating {:?} textures for '{}' at {}px resolution.",
                        style, entity, resolution
                    ),
                }
            }

            GenCommand::GenerationStatus { task_id, action } => {
                let result = match action.as_str() {
                    "cancel" => {
                        if let Some(id) = &task_id {
                            let cancelled = params.asset_gen_manager.cancel_task(id);
                            serde_json::json!({ "cancelled": cancelled, "task_id": id }).to_string()
                        } else {
                            serde_json::json!({ "error": "task_id required for cancel" })
                                .to_string()
                        }
                    }
                    _ => {
                        // "status" or "list"
                        let active: Vec<_> = params
                            .asset_gen_manager
                            .active_tasks()
                            .into_iter()
                            .cloned()
                            .collect();
                        let completed: Vec<_> = params
                            .asset_gen_manager
                            .completed_tasks()
                            .into_iter()
                            .cloned()
                            .collect();
                        let failed: Vec<_> = params
                            .asset_gen_manager
                            .failed_tasks()
                            .into_iter()
                            .cloned()
                            .collect();
                        serde_json::json!({
                            "active": active,
                            "completed": completed,
                            "failed": failed,
                            "queue_depth": active.len(),
                            "model_server": params.asset_gen_manager.server_status,
                        })
                        .to_string()
                    }
                };
                GenResponse::GenerationStatusResult {
                    status_json: result,
                }
            }

            // Tier 24: AI NPC Intelligence (AI2)
            GenCommand::SetNpcBrain { entity, config } => {
                let model = config.model.clone();
                let tick_rate = config.tick_rate;
                if let Some(bevy_entity) = params.registry.get_entity(&entity) {
                    let brain_state = crate::character::npc_brain::NpcBrainState::new(config);
                    commands.entity(bevy_entity).insert(brain_state);
                    tracing::info!(
                        "Attached NPC brain to '{}' (model: {}, tick: {}s)",
                        entity,
                        model,
                        tick_rate
                    );
                    GenResponse::NpcBrainSet {
                        entity,
                        model,
                        tick_rate,
                    }
                } else {
                    GenResponse::Error {
                        message: format!("Entity '{}' not found", entity),
                    }
                }
            }

            GenCommand::NpcObserve {
                entity, question, ..
            } => {
                // Would render from NPC POV and send to VLM
                let description = format!(
                    "NPC '{}' observing scene{}",
                    entity,
                    question
                        .map(|q| format!(" (question: {})", q))
                        .unwrap_or_default()
                );
                GenResponse::NpcObservation {
                    entity,
                    description,
                }
            }

            GenCommand::SetNpcMemory {
                entity,
                capacity,
                initial_memories,
                auto_memorize,
            } => {
                let initial_count = initial_memories.len();
                if let Some(bevy_entity) = params.registry.get_entity(&entity) {
                    let mut memory =
                        crate::character::npc_memory::NpcMemory::new(capacity, auto_memorize);
                    for (i, content) in initial_memories.into_iter().enumerate() {
                        memory.add_memory(content, 0.7, i as f64);
                    }
                    commands.entity(bevy_entity).insert(memory);
                    tracing::info!(
                        "Attached NPC memory to '{}' (capacity: {}, initial: {})",
                        entity,
                        capacity,
                        initial_count
                    );
                    GenResponse::NpcMemorySet {
                        entity,
                        capacity,
                        initial_count,
                    }
                } else {
                    GenResponse::Error {
                        message: format!("Entity '{}' not found", entity),
                    }
                }
            }

            // Multi-file WorldGen handlers
            GenCommand::WriteWorldPlan {
                name,
                description,
                generation_strategy,
                regions,
                constraints: _,
                environment: _,
                camera: _,
            } => {
                // Build region info for SKILL.md
                let regions_info: Vec<(String, u32)> = regions
                    .iter()
                    .filter_map(|r| {
                        let id = r.get("id").and_then(|v| v.as_str())?.to_string();
                        let count =
                            r.get("entity_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        Some((id, count))
                    })
                    .collect();

                let desc = description.as_deref().unwrap_or("A generated 3D world");
                let skill_md = super::world::generate_skill_md(
                    &name,
                    desc,
                    &generation_strategy,
                    &regions_info,
                );

                // Create world.md with environment, camera, region layout
                let world_md = format!(
                    "# {}\n\n{}\n\n## Generation Strategy\n\n{}\n\n## Regions\n\n{}\n",
                    name,
                    desc,
                    generation_strategy,
                    regions
                        .iter()
                        .filter_map(|r| serde_json::to_string_pretty(r).ok())
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                );

                // Create root world.ron placeholder (will be finalized on save)
                let mut manifest = wt::WorldManifest::new(&name);
                manifest.version = 2;
                manifest.meta.description = description.clone();
                let world_ron =
                    ron::ser::to_string_pretty(&manifest, ron::ser::PrettyConfig::default())
                        .unwrap_or_default();

                // Store all three in PendingWrites
                params
                    .pending_writes
                    .insert("SKILL", skill_md, String::new());
                params.pending_writes.insert("world", world_md, world_ron);

                GenResponse::WorldPlanWritten {
                    skill_md_path: format!("{name}/SKILL.md"),
                    world_md_path: format!("{name}/world.md"),
                    world_ron_path: format!("{name}/world.ron"),
                }
            }
            GenCommand::WriteRegion {
                region_id,
                md_content,
                ron_content,
                flush,
            } => {
                let domain = format!("regions/{region_id}");
                params
                    .pending_writes
                    .insert(&domain, md_content, ron_content);

                let flush_err = if flush {
                    let skill_dir =
                        params.current_world.path.clone().unwrap_or_else(|| {
                            params.workspace.path.join("skills").join("current")
                        });
                    std::fs::create_dir_all(skill_dir.join("regions"))
                        .and_then(|_| params.pending_writes.flush(&domain, &skill_dir))
                        .err()
                } else {
                    None
                };

                if let Some(e) = flush_err {
                    GenResponse::Error {
                        message: format!("Failed to flush region {}: {}", region_id, e),
                    }
                } else {
                    GenResponse::RegionWritten {
                        region_id: region_id.clone(),
                        md_path: format!("regions/{region_id}.md"),
                        ron_path: format!("regions/{region_id}.ron"),
                    }
                }
            }
            GenCommand::LoadRegion { path } => {
                // Try to read region .ron from PendingWrites first, then from disk
                let domain = format!("regions/{path}");
                let ron_content = if let Some((_, ron)) = params.pending_writes.remove(&domain) {
                    Ok(ron)
                } else {
                    let world_dir =
                        params.current_world.path.clone().unwrap_or_else(|| {
                            params.workspace.path.join("skills").join("current")
                        });
                    std::fs::read_to_string(world_dir.join(format!("regions/{}.ron", path)))
                        .map_err(|e| format!("Failed to read region {}: {}", path, e))
                };

                match ron_content.and_then(|s| {
                    ron::from_str::<wt::RegionEntities>(&s)
                        .map_err(|e| format!("Failed to parse region {}: {}", path, e))
                }) {
                    Err(msg) => GenResponse::Error { message: msg },
                    Ok(region) => {
                        let region_id = region.region_id.clone();
                        let world_dir = params.current_world.path.clone();

                        spawn_world_entities(
                            &region.entities,
                            &mut commands,
                            &mut params.meshes,
                            &mut params.materials,
                            &mut params.registry,
                            &mut params.next_entity_id,
                            &mut params.behavior_state,
                            &params.asset_server,
                            &mut params.pending_gltf,
                            world_dir.as_deref(),
                        );

                        let entity_count = region.entities.len() as u32;
                        for we in &region.entities {
                            if let Some(bevy_entity) = params.registry.get_entity(we.name.as_str())
                            {
                                commands.entity(bevy_entity).insert(RegionMember {
                                    region_id: region_id.clone(),
                                });
                            }
                        }

                        GenResponse::RegionLoaded {
                            region_id,
                            entity_count,
                        }
                    }
                }
            }
            GenCommand::UnloadRegion { region_id } => {
                // Find all entities with matching RegionMember and despawn them
                let mut to_remove: Vec<(Entity, String)> = Vec::new();
                for (entity, member) in params.region_member_q.iter() {
                    if member.region_id == region_id {
                        if let Some(name) = params.registry.get_name(entity) {
                            to_remove.push((entity, name.to_string()));
                        } else {
                            to_remove.push((entity, String::new()));
                        }
                    }
                }

                let count = to_remove.len() as u32;
                for (entity, name) in &to_remove {
                    if !name.is_empty() {
                        params.registry.remove_by_name(name);
                    }
                    commands.entity(*entity).despawn();
                }

                GenResponse::RegionUnloaded {
                    region_id,
                    entities_removed: count,
                }
            }
            GenCommand::PersistBlockout => {
                match &params.current_blockout {
                    Some(blockout) => {
                        let ron_content = ron::ser::to_string_pretty(
                            &blockout.spec,
                            ron::ser::PrettyConfig::default(),
                        )
                        .unwrap_or_default();

                        // Generate blockout.md with design intent
                        let md_content = format!(
                            "# Blockout Layout\n\n## Terrain\n\nType: {:?}\n\n## Regions\n\n{}\n\n## Paths\n\n{} connections\n",
                            blockout.spec.terrain.terrain_type,
                            blockout
                                .spec
                                .regions
                                .iter()
                                .map(|r| format!("- **{}**: {:?}", r.id, r.region_type))
                                .collect::<Vec<_>>()
                                .join("\n"),
                            blockout.spec.paths.len(),
                        );

                        params
                            .pending_writes
                            .insert("layout/blockout", md_content, ron_content);

                        GenResponse::BlockoutPersisted {
                            md_path: "layout/blockout.md".to_string(),
                            ron_path: "layout/blockout.ron".to_string(),
                        }
                    }
                    None => GenResponse::Error {
                        message: "No blockout to persist — run gen_plan_layout first".to_string(),
                    },
                }
            }
            GenCommand::WriteBehaviors {
                name,
                md_content,
                ron_content,
            } => {
                let domain = format!("behaviors/{name}");
                params
                    .pending_writes
                    .insert(&domain, md_content, ron_content);

                GenResponse::BehaviorsWritten {
                    name: name.clone(),
                    md_path: format!("behaviors/{name}.md"),
                    ron_path: format!("behaviors/{name}.ron"),
                }
            }
            GenCommand::WriteAudio {
                name,
                md_content,
                ron_content,
            } => {
                let domain = format!("audio/{name}");
                params
                    .pending_writes
                    .insert(&domain, md_content, ron_content);

                GenResponse::AudioWritten {
                    name: name.clone(),
                    md_path: format!("audio/{name}.md"),
                    ron_path: format!("audio/{name}.ron"),
                }
            }
            GenCommand::CheckDrift {
                domain,
                detail_level,
            } => {
                let world_dir = params
                    .current_world
                    .path
                    .clone()
                    .unwrap_or_else(|| params.workspace.path.join("skills").join("default"));
                match super::sync::check_drift(&world_dir) {
                    Ok(mut report) => {
                        if let Some(ref d) = domain {
                            report.domains.retain(|dd| dd.domain == *d);
                        }
                        params.generation_log.log(
                            "gen_check_drift",
                            &serde_json::json!({"domain": domain, "detail_level": detail_level}),
                            None,
                        );
                        for dd in &report.domains {
                            params.region_dirty_flags.clear(&dd.domain);
                        }
                        GenResponse::DriftReport(report)
                    }
                    Err(e) => GenResponse::Error { message: e },
                }
            }
            GenCommand::Sync {
                domain,
                source,
                preview,
                resolve_conflicts,
            } => {
                let world_dir = params
                    .current_world
                    .path
                    .clone()
                    .unwrap_or_else(|| params.workspace.path.join("skills").join("default"));
                match super::sync::apply_sync(
                    &world_dir,
                    &domain,
                    &source,
                    preview,
                    &resolve_conflicts,
                ) {
                    Ok(super::sync::SyncResult::Preview(changes)) => {
                        params.generation_log.log(
                            "gen_sync",
                            &serde_json::json!({"domain": domain, "source": source, "preview": true}),
                            None,
                        );
                        GenResponse::SyncPreview { domain, changes }
                    }
                    Ok(super::sync::SyncResult::Applied { files_updated }) => {
                        params.generation_log.log(
                            "gen_sync",
                            &serde_json::json!({"domain": domain, "source": source, "preview": false}),
                            None,
                        );
                        GenResponse::SyncApplied {
                            domain,
                            files_updated,
                        }
                    }
                    Err(e) => GenResponse::Error { message: e },
                }
            }

            // Tier 27: Multimodal Input (AI3)
            GenCommand::ImageToLayout {
                image,
                prompt,
                scale,
                style,
            } => {
                // Validate the image source exists (if it's a file path, not base64)
                let is_base64 = image.len() > 200 || image.starts_with("data:");
                if !is_base64 && !std::path::Path::new(&image).exists() {
                    GenResponse::Error {
                        message: format!("Image file not found: {}", image),
                    }
                } else {
                    let (scale_name, world_size) = match scale {
                        ImageLayoutScale::Small => ("small", [15.0_f32, 15.0]),
                        ImageLayoutScale::Medium => ("medium", [50.0, 50.0]),
                        ImageLayoutScale::Large => ("large", [150.0, 150.0]),
                    };
                    let style_name = match style {
                        ImageLayoutStyle::Match => "match",
                        ImageLayoutStyle::Blockout => "blockout",
                        ImageLayoutStyle::Stylized => "stylized",
                    };

                    // Build image analysis metadata
                    let analysis = serde_json::json!({
                        "source": if is_base64 { "base64" } else { &image },
                        "scale": scale_name,
                        "style": style_name,
                        "prompt": prompt,
                        "world_size": world_size,
                        "instructions": format!(
                            "Analyze the provided reference image and create a BlockoutSpec layout. \
                             Scale: {} (world size: {}x{}m). Style: {}. {}",
                            scale_name, world_size[0], world_size[1], style_name,
                            prompt.as_deref().unwrap_or("No additional guidance.")
                        ),
                        "reference_board_count": params.reference_board.entries.len(),
                    });

                    // Generate a layout plan from the image context
                    let plan = crate::worldgen::BlockoutSpec::from_prompt(
                        &format!(
                            "[Image-guided generation: {} scale, {} style] {}",
                            scale_name,
                            style_name,
                            prompt
                                .as_deref()
                                .unwrap_or("Generate layout from reference image")
                        ),
                        world_size,
                        None,
                    );

                    match serde_json::to_string_pretty(&plan) {
                        Ok(plan_json) => GenResponse::ImageLayoutAnalysis {
                            plan_json,
                            analysis_json: analysis.to_string(),
                        },
                        Err(e) => GenResponse::Error {
                            message: format!("Failed to serialize layout plan: {}", e),
                        },
                    }
                }
            }
            GenCommand::MatchStyle {
                image,
                scope,
                intensity,
                entities,
            } => {
                let is_base64 = image.len() > 200 || image.starts_with("data:");
                if !is_base64 && !std::path::Path::new(&image).exists() {
                    GenResponse::Error {
                        message: format!("Image file not found: {}", image),
                    }
                } else {
                    let scope_name = match scope {
                        StyleMatchScope::All => "all",
                        StyleMatchScope::Lighting => "lighting",
                        StyleMatchScope::Materials => "materials",
                        StyleMatchScope::Atmosphere => "atmosphere",
                    };

                    let entity_count = params.registry.all_names().count();
                    let targeted = entities.as_ref().map(|e| e.len()).unwrap_or(entity_count);

                    let changes = serde_json::json!({
                        "scope": scope_name,
                        "intensity": intensity,
                        "entities_targeted": targeted,
                        "source": if is_base64 { "base64" } else { &image },
                        "instructions": format!(
                            "Analyze the reference image and extract {} properties. \
                             Apply with intensity {:.1} to {} entities. \
                             Use gen_set_light, gen_set_environment, gen_modify_entity, \
                             and gen_set_ambience to apply the extracted style.",
                            scope_name, intensity, targeted
                        ),
                        "reference_board_count": params.reference_board.entries.len(),
                    });

                    GenResponse::StyleMatched {
                        changes_json: changes.to_string(),
                    }
                }
            }
            GenCommand::ReferenceBoard { action } => {
                let result = match action {
                    ReferenceBoardAction::Add {
                        image,
                        label,
                        weight,
                    } => {
                        let is_base64 = image.len() > 200 || image.starts_with("data:");
                        if !is_base64 && !std::path::Path::new(&image).exists() {
                            serde_json::json!({
                                "error": format!("Image file not found: {}", image),
                            })
                        } else if params.reference_board.entries.len() >= 8 {
                            serde_json::json!({
                                "error": "Maximum 8 references allowed. Remove existing references first.",
                            })
                        } else {
                            let label = label.unwrap_or_else(|| {
                                format!("reference_{}", params.reference_board.entries.len() + 1)
                            });
                            let image_path = if is_base64 {
                                "[base64 data]".to_string()
                            } else {
                                image.clone()
                            };
                            // Description will be filled by the LLM when it processes
                            // the image — for now store a placeholder
                            let description = format!(
                                "Reference image: {} (label: {}, weight: {:.1})",
                                if is_base64 { "inline data" } else { &image },
                                label,
                                weight
                            );
                            let ref_id = params.reference_board.add(
                                label.clone(),
                                weight,
                                description,
                                image_path,
                            );
                            serde_json::json!({
                                "ref_id": ref_id,
                                "label": label,
                                "weight": weight,
                                "active_references": params.reference_board.entries.len(),
                                "note": "Reference added. It will influence subsequent generation and evaluation calls."
                            })
                        }
                    }
                    ReferenceBoardAction::Remove { ref_id } => {
                        if params.reference_board.remove(&ref_id) {
                            serde_json::json!({
                                "removed": ref_id,
                                "active_references": params.reference_board.entries.len(),
                            })
                        } else {
                            serde_json::json!({
                                "error": format!("Reference '{}' not found", ref_id),
                            })
                        }
                    }
                    ReferenceBoardAction::List => {
                        let refs: Vec<_> = params
                            .reference_board
                            .entries
                            .iter()
                            .map(|e| {
                                serde_json::json!({
                                    "ref_id": e.ref_id,
                                    "label": e.label,
                                    "weight": e.weight,
                                    "description": e.description,
                                    "image_path": e.image_path,
                                })
                            })
                            .collect();
                        serde_json::json!({
                            "references": refs,
                            "count": refs.len(),
                        })
                    }
                    ReferenceBoardAction::Clear => {
                        let count = params.reference_board.entries.len();
                        params.reference_board.clear();
                        serde_json::json!({
                            "cleared": count,
                            "active_references": 0,
                        })
                    }
                };
                GenResponse::ReferenceBoardResult {
                    result_json: result.to_string(),
                }
            }
            GenCommand::PanoramaToWorld {
                image,
                prompt,
                generate_beyond,
            } => {
                use crate::worldgen::panorama;
                match panorama::load_panorama(&image) {
                    Err(e) => GenResponse::Error {
                        message: format!("{e:#}"),
                    },
                    Ok(img) => {
                        let analysis = panorama::analyze(&img);
                        let world_size = [80.0, 80.0];
                        let spec =
                            analysis.to_blockout(prompt.as_deref(), world_size, generate_beyond);
                        let region_count = spec.regions.len();
                        let landmarks = spec.regions.iter().flat_map(|r| &r.hero_slots).count();
                        let notes = format!(
                            "Layout planned from the panorama: {} regions ({} with landmarks), \
                             terrain {:?}, biome {:?}, sky {}, ground {}. Nothing is in the \
                             scene yet: call gen_apply_blockout to build it. A panorama has no \
                             depth, so regions sit on a ring {:.0} m from the viewpoint at the \
                             bearings where the skyline rises; heights follow the observed angles.",
                            region_count,
                            landmarks,
                            spec.terrain.terrain_type,
                            spec.palette.primary_biome,
                            panorama::hex(analysis.sky_color),
                            panorama::hex(analysis.ground_color),
                            panorama::RING_FRACTION * world_size[0] / 2.0,
                        );
                        let analysis_json = serde_json::to_string(&analysis).unwrap_or_default();
                        commands.insert_resource(crate::worldgen::CurrentBlockout { spec });
                        GenResponse::PanoramaWorldGenerated {
                            world_name: format!(
                                "panorama_{}",
                                chrono::Utc::now().format("%Y%m%d_%H%M%S")
                            ),
                            regions_planned: region_count,
                            spawn_point: [0.0, 1.7, 0.0],
                            analysis_json,
                            notes,
                        }
                    }
                }
            }
        };

        // Mark entities dirty and record undo history.
        match &response {
            GenResponse::Spawned { name, .. } => {
                if let Some(bevy_ent) = params.registry.get_entity(name)
                    && let Some(id) = params.registry.get_id(bevy_ent)
                {
                    params.dirty_tracker.mark_dirty(id);
                    // Record undo: inverse of spawn is delete
                    let we = snapshot_entity(name, bevy_ent, id, &snap_queries!(params));
                    params
                        .undo_stack
                        .record(wt::EditOp::spawn(we), wt::EditOp::delete(id), None);
                }
            }
            GenResponse::Modified { name }
            | GenResponse::BehaviorAdded { entity: name, .. }
            | GenResponse::BehaviorRemoved { entity: name, .. }
            | GenResponse::AudioEmitterSpawned { name }
            | GenResponse::TierSet { entity: name, .. }
            | GenResponse::RoleSet { entity: name, .. } => {
                if let Some(bevy_ent) = params.registry.get_entity(name)
                    && let Some(id) = params.registry.get_id(bevy_ent)
                {
                    params.dirty_tracker.mark_dirty(id);
                }
            }
            GenResponse::EnvironmentSet | GenResponse::CameraSet => {
                params.dirty_tracker.world_meta_dirty = true;
            }
            GenResponse::WorldSaved { .. } => {
                params.dirty_tracker.clear();
            }
            GenResponse::WorldLoaded { .. } => {
                params.dirty_tracker.clear();
                // History is restored in LoadWorld handler, not here
            }
            GenResponse::SceneCleared { .. } => {
                params.dirty_tracker.clear();
                params.undo_stack.reset(wt::EditHistory::new());
            }
            _ => {}
        }

        if reply {
            let _ = channel_res.channels.resp_tx.send(response);
        } else if let GenResponse::Error { message } = &response {
            tracing::warn!("{message}");
        } else {
            tracing::info!("{response:?}");
        }
    }
}

/// Process pending screenshots that need frame delays.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn process_pending_screenshots(
    channel_res: ResMut<GenChannelRes>,
    mut pending: ResMut<PendingScreenshots>,
    current_skill: Res<CurrentWorldSkill>,
    mut commands: Commands,
    cameras: Query<Entity, With<Camera>>,
    mut camera_transforms: Query<&mut Transform, With<Camera>>,
    registry: Res<NameRegistry>,
    material_handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    gen_entities: Query<(&GenEntity, &Transform), Without<Camera>>,
    _names_query: Query<&Name>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};

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
        let screenshot_req = pending.queue.remove(i);

        // --- WG4.1: Apply entity highlight ---
        let mut original_emissive: Option<(Entity, LinearRgba)> = None;
        if let Some(ref entity_name) = screenshot_req.highlight_entity
            && let Some(entity) = registry.get_entity(entity_name)
            && let Ok(mat_handle) = material_handles.get(entity)
            && let Some(mut mat) = materials.get_mut(&mat_handle.0)
        {
            let orig = mat.emissive;
            let [r, g, b, _] = screenshot_req.highlight_color;
            mat.emissive = LinearRgba::new(r * 3.0, g * 3.0, b * 3.0, 1.0);
            original_emissive = Some((entity, orig));
        }

        // --- WG4.1: Reposition camera for angle preset ---
        let mut original_camera_transform: Option<Transform> = None;
        if !matches!(screenshot_req.camera_angle, ScreenshotCameraAngle::Current) {
            // Compute scene center and extent from all GenEntities
            let (scene_center, scene_extent) = compute_scene_bounds(&gen_entities);

            if let Ok(mut cam_tf) = camera_transforms.single_mut() {
                original_camera_transform = Some(*cam_tf);

                match screenshot_req.camera_angle {
                    ScreenshotCameraAngle::TopDown => {
                        let height = scene_extent * 2.0 + 20.0;
                        cam_tf.translation =
                            Vec3::new(scene_center.x, scene_center.y + height, scene_center.z);
                        cam_tf.look_at(scene_center, Vec3::NEG_Z);
                    }
                    ScreenshotCameraAngle::Isometric => {
                        let dist = scene_extent * 1.8 + 15.0;
                        cam_tf.translation = Vec3::new(
                            scene_center.x + dist * 0.707,
                            scene_center.y + dist * 0.707,
                            scene_center.z + dist * 0.707,
                        );
                        cam_tf.look_at(scene_center, Vec3::Y);
                    }
                    ScreenshotCameraAngle::Front => {
                        let dist = scene_extent * 1.5 + 10.0;
                        cam_tf.translation =
                            Vec3::new(scene_center.x, scene_center.y + 2.0, scene_center.z + dist);
                        cam_tf.look_at(scene_center, Vec3::Y);
                    }
                    ScreenshotCameraAngle::EntityFocus => {
                        // Frame the highlighted entity with 2x bounding distance
                        if let Some(ref ename) = screenshot_req.highlight_entity
                            && let Some(ent) = registry.get_entity(ename)
                            && let Ok((_, etf)) = gen_entities.get(ent)
                        {
                            let focus = etf.translation;
                            let dist = 8.0; // Reasonable framing distance
                            cam_tf.translation = Vec3::new(
                                focus.x + dist * 0.5,
                                focus.y + dist * 0.5,
                                focus.z + dist * 0.5,
                            );
                            cam_tf.look_at(focus, Vec3::Y);
                        }
                    }
                    ScreenshotCameraAngle::Current => {}
                }
            }
        }

        // Determine output path(s)
        let paths: Vec<PathBuf> = if let Some(ref explicit_path) = screenshot_req.path {
            // User specified a path
            let mut paths = vec![explicit_path.clone()];
            // Also save to skill folder if requested and skill exists
            if screenshot_req.save_to_skill
                && let Some(skill_folder) = current_skill.screenshots_folder()
            {
                let filename = explicit_path
                    .file_name()
                    .unwrap_or(std::ffi::OsStr::new("screenshot.png"));
                paths.push(skill_folder.join(filename));
            }
            paths
        } else if screenshot_req.save_to_skill {
            // No explicit path, save to skill folder
            if let Some(skill_folder) = current_skill.screenshots_folder() {
                vec![skill_folder.join(current_skill.screenshot_filename())]
            } else {
                // No skill folder, use temp
                vec![std::env::temp_dir().join(format!(
                    "localgpt_screenshot_{}.png",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ))]
            }
        } else {
            // Fallback to temp
            vec![std::env::temp_dir().join(format!(
                "localgpt_screenshot_{}.png",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ))]
        };

        // Find the primary camera
        let camera_entity = cameras.iter().next();

        // Take screenshot using Bevy's Screenshot API
        if let Some(_camera) = camera_entity {
            // Create screenshot entity and observer for each path
            for path in &paths {
                // Ensure parent directory exists
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                let path_clone = path.clone();
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path_clone));
            }
        }

        // --- WG4.1: Restore original emissive after screenshot is queued ---
        if let Some((entity, orig_emissive)) = original_emissive
            && let Ok(mat_handle) = material_handles.get(entity)
            && let Some(mut mat) = materials.get_mut(&mat_handle.0)
        {
            mat.emissive = orig_emissive;
        }

        // --- WG4.1: Restore original camera transform ---
        if let Some(orig_tf) = original_camera_transform
            && let Ok(mut cam_tf) = camera_transforms.single_mut()
        {
            *cam_tf = orig_tf;
        }

        // Send response with the first (primary) path
        let primary_path = paths.first().cloned().unwrap_or_default();
        let response = GenResponse::Screenshot {
            image_path: primary_path.to_string_lossy().into_owned(),
        };
        let _ = channel_res.channels.resp_tx.send(response);
    }
}

/// Compute the center and max extent of all GenEntities in the scene.
fn compute_scene_bounds(
    gen_entities: &Query<(&GenEntity, &Transform), Without<Camera>>,
) -> (Vec3, f32) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut count = 0u32;

    for (_, tf) in gen_entities.iter() {
        let p = tf.translation;
        min = min.min(p);
        max = max.max(p);
        count += 1;
    }

    if count == 0 {
        return (Vec3::ZERO, 10.0);
    }

    let center = (min + max) * 0.5;
    let extent = (max - min).length() * 0.5;
    (center, extent.max(5.0))
}

/// Compute scene bounds from separate GenEntity and Transform queries.
fn compute_scene_bounds_from_entities(
    gen_entities: &Query<&GenEntity>,
    transforms: &Query<&Transform>,
) -> (Vec3, f32) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut count = 0u32;

    for entity in gen_entities.iter() {
        let _ = entity; // Just need to iterate entities with GenEntity
    }
    // Use the transform query to get positions
    for (entity, _gen) in gen_entities.iter().enumerate() {
        let _ = entity;
    }

    // Simplified: iterate all transforms (GenEntity has same entities)
    for tf in transforms.iter() {
        let p = tf.translation;
        min = min.min(p);
        max = max.max(p);
        count += 1;
    }

    if count == 0 {
        return (Vec3::ZERO, 50.0);
    }

    let center = (min + max) * 0.5;
    let extent = (max - min).length() * 0.5;
    (center, extent.max(10.0))
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

        // Spawn the scene with source tracking
        // WG6.3: If segment=true, the mesh will be decomposed into sub-objects
        // after the scene finishes loading. The segmentation data model is in
        // worldgen/segment.rs; actual mesh decomposition requires post-spawn
        // access to vertex/index buffers which happens in a deferred system.
        let _segment = load.segment;
        let entity = commands
            .spawn((
                WorldAssetRoot(load.handle.clone()),
                GltfSource {
                    path: load.path.clone(),
                },
            ))
            .id();

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

/// After a world's glTF scene spawns, Bevy's scene spawner creates child
/// entities with `Name` components (from glTF node names). This system
/// scans for those named entities, registers them in `NameRegistry`, and
/// applies the deferred behaviors and audio emitters.
#[allow(clippy::too_many_arguments)]
fn process_pending_world_setup(
    mut pending: ResMut<PendingWorldSetup>,
    mut registry: ResMut<NameRegistry>,
    mut next_entity_id: ResMut<NextEntityId>,
    mut commands: Commands,
    transforms: Query<&Transform>,
    mut behavior_state: ResMut<BehaviorState>,
    mut audio_engine: ResMut<audio::AudioEngine>,
    named_entities: Query<(Entity, &Name), Without<GenEntity>>,
) {
    let Some(ref mut setup) = pending.active else {
        return;
    };

    setup.frames_waited += 1;

    // Collect all entity names we need to find.
    let needed: std::collections::HashSet<String> = setup
        .behaviors
        .iter()
        .map(|(name, _)| name.clone())
        .chain(setup.emitters.iter().filter_map(|e| e.entity.clone()))
        .collect();

    if needed.is_empty() {
        pending.active = None;
        return;
    }

    // Scan for newly spawned named entities from the glTF scene.
    // These won't have GenEntity yet (Bevy scene spawner adds Name but not our marker).
    let mut found_count = 0;
    for (entity, name) in named_entities.iter() {
        let name_str = name.as_str();
        if needed.contains(name_str) && registry.get_entity(name_str).is_none() {
            let wid = next_entity_id.alloc();
            registry.insert_with_id(name_str.to_string(), entity, wid);
            commands.entity(entity).insert(GenEntity {
                entity_type: GenEntityType::Mesh,
                world_id: wid,
            });
            found_count += 1;
        }
    }

    // If no entities found yet and we haven't waited too long, try again next frame.
    if found_count == 0 && setup.frames_waited < 120 {
        return;
    }

    // Apply behaviors to now-registered entities.
    for (entity_name, behavior_defs) in &setup.behaviors {
        if let Some(entity) = registry.get_entity(entity_name) {
            let base_transform = transforms.get(entity).copied().unwrap_or_default();
            let instances: Vec<behaviors::BehaviorInstance> = behavior_defs
                .iter()
                .map(|def| behaviors::BehaviorInstance {
                    id: behavior_state.next_id(),
                    def: def.clone(),
                    base_position: base_transform.translation,
                    base_scale: base_transform.scale,
                })
                .collect();
            commands.entity(entity).insert(EntityBehaviors {
                behaviors: instances,
            });
        }
    }

    // Apply audio emitters (which reference entities by name for spatial attachment).
    for emitter_cmd in &setup.emitters {
        audio::handle_spawn_audio_emitter(
            emitter_cmd.clone(),
            &mut audio_engine,
            &mut commands,
            &mut registry,
            &mut next_entity_id,
        );
    }

    pending.active = None;
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_scene_info(
    revision: u64,
    registry: &NameRegistry,
    transforms: &Query<&Transform>,
    gen_entities: &Query<&GenEntity>,
    material_handles: &Query<&MeshMaterial3d<StandardMaterial>>,
    material_assets: &Assets<StandardMaterial>,
    parametric_shapes: &Query<&ParametricShape>,
    directional_lights: &Query<&DirectionalLight>,
    point_lights: &Query<&PointLight>,
    spot_lights: &Query<&SpotLight>,
    behaviors_query: &Query<&mut EntityBehaviors>,
    audio_engine: &audio::AudioEngine,
    tier_q: &Query<&crate::worldgen::PlacementTier>,
    role_q: &Query<&crate::worldgen::SemanticRole>,
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

        let shape = parametric_shapes
            .get(entity)
            .ok()
            .map(|p| p.shape.kind().to_string());

        let color = material_handles
            .get(entity)
            .ok()
            .and_then(|h| material_assets.get(&h.0))
            .map(|mat| {
                let c = mat.base_color.to_srgba();
                [c.red, c.green, c.blue, c.alpha]
            });

        // Light type
        let light = if directional_lights.get(entity).is_ok() {
            Some("directional".to_string())
        } else if point_lights.get(entity).is_ok() {
            Some("point".to_string())
        } else if spot_lights.get(entity).is_ok() {
            Some("spot".to_string())
        } else {
            None
        };

        // Audio
        let audio = audio_engine
            .emitter_meta
            .get(name)
            .map(|m| m.sound_type.clone());

        // Behavior count
        let behaviors = behaviors_query.get(entity).ok().map(|b| b.behaviors.len());

        // Tier and role
        let tier = tier_q.get(entity).ok().map(|t| t.as_str().to_string());
        let role = role_q.get(entity).ok().map(|r| r.as_str().to_string());

        entities.push(EntitySummary {
            name: name.to_string(),
            entity_type,
            shape,
            position,
            scale,
            color,
            light,
            audio,
            behaviors,
            tier,
            role,
        });
    }

    GenResponse::SceneInfo(SceneInfoData {
        revision,
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
    behaviors_query: &Query<&mut EntityBehaviors>,
    parametric_shapes: &Query<&ParametricShape>,
    directional_lights: &Query<&DirectionalLight>,
    point_lights: &Query<&PointLight>,
    spot_lights: &Query<&SpotLight>,
    gltf_sources: &Query<&GltfSource>,
    audio_engine: &audio::AudioEngine,
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

    let shape_name = parametric_shapes
        .get(entity)
        .ok()
        .map(|p| p.shape.kind().to_string());

    let (
        color,
        metallic,
        roughness,
        emissive,
        alpha_mode_str,
        unlit_val,
        double_sided_val,
        reflectance_val,
    ) = material_handles
        .get(entity)
        .ok()
        .and_then(|h| material_assets.get(&h.0))
        .map(|mat| {
            let c = mat.base_color.to_srgba();
            let e = mat.emissive;
            let emissive_arr = [e.red, e.green, e.blue, e.alpha];
            let has_emissive = emissive_arr.iter().any(|&v| v > 0.0);
            let am = match mat.alpha_mode {
                AlphaMode::Opaque => None,
                AlphaMode::Mask(cutoff) => Some(format!("mask({:.2})", cutoff)),
                AlphaMode::Blend => Some("blend".to_string()),
                AlphaMode::Add => Some("add".to_string()),
                AlphaMode::Multiply => Some("multiply".to_string()),
                _ => None,
            };
            (
                Some([c.red, c.green, c.blue, c.alpha]),
                Some(mat.metallic),
                Some(mat.perceptual_roughness),
                if has_emissive {
                    Some(emissive_arr)
                } else {
                    None
                },
                am,
                if mat.unlit { Some(true) } else { None },
                if mat.double_sided { Some(true) } else { None },
                if (mat.reflectance - 0.5).abs() > f32::EPSILON {
                    Some(mat.reflectance)
                } else {
                    None
                },
            )
        })
        .unwrap_or((None, None, None, None, None, None, None, None));

    let light_info = if let Ok(dl) = directional_lights.get(entity) {
        let c = dl.color.to_srgba();
        let dir = transform.forward().as_vec3().to_array();
        Some(LightInfoData {
            light_type: "directional".to_string(),
            color: [c.red, c.green, c.blue, c.alpha],
            intensity: dl.illuminance,
            shadows: dl.shadow_maps_enabled,
            direction: Some(dir),
            range: None,
            outer_angle: None,
            inner_angle: None,
        })
    } else if let Ok(pl) = point_lights.get(entity) {
        let c = pl.color.to_srgba();
        Some(LightInfoData {
            light_type: "point".to_string(),
            color: [c.red, c.green, c.blue, c.alpha],
            intensity: pl.intensity,
            shadows: pl.shadow_maps_enabled,
            direction: None,
            range: Some(pl.range),
            outer_angle: None,
            inner_angle: None,
        })
    } else if let Ok(sl) = spot_lights.get(entity) {
        let c = sl.color.to_srgba();
        let dir = transform.forward().as_vec3().to_array();
        Some(LightInfoData {
            light_type: "spot".to_string(),
            color: [c.red, c.green, c.blue, c.alpha],
            intensity: sl.intensity,
            shadows: sl.shadow_maps_enabled,
            direction: Some(dir),
            range: Some(sl.range),
            outer_angle: Some(sl.outer_angle),
            inner_angle: Some(sl.inner_angle),
        })
    } else {
        None
    };

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

    let behavior_summaries: Vec<BehaviorSummary> = behaviors_query
        .get(entity)
        .ok()
        .map(|b| {
            b.behaviors
                .iter()
                .map(behaviors::behavior_to_summary)
                .collect()
        })
        .unwrap_or_default();

    GenResponse::EntityInfo(Box::new(EntityInfoData {
        name: name.to_string(),
        entity_id: registry
            .get_id(entity)
            .map(|id| id.0)
            .unwrap_or(entity.to_bits()),
        entity_type,
        shape: shape_name,
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
        emissive,
        alpha_mode: alpha_mode_str,
        unlit: unlit_val,
        double_sided: double_sided_val,
        reflectance: reflectance_val,
        visible,
        light: light_info,
        children,
        parent,
        mesh_asset: gltf_sources.get(entity).ok().map(|s| s.path.clone()),
        audio: audio_engine
            .emitter_meta
            .get(name)
            .map(|m| m.sound_type.clone()),
        behaviors: behavior_summaries,
    }))
}

fn handle_spawn_primitive(
    cmd: SpawnPrimitiveCmd,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<NameRegistry>,
    next_id: &mut ResMut<NextEntityId>,
) -> GenResponse {
    if registry.contains_name(&cmd.name) {
        return GenResponse::Error {
            message: format!("Entity '{}' already exists", cmd.name),
        };
    }

    // One mesh mapping for every app (localgpt-world-bevy).
    let shape = compat::shape_from_primitive(cmd.shape, &cmd.dimensions);
    let mesh = meshes.add(localgpt_world_bevy::shape_mesh(&shape));

    let mut std_mat = StandardMaterial {
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
    };
    if let Some(ref am_str) = cmd.alpha_mode {
        std_mat.alpha_mode = parse_alpha_mode(am_str);
    }
    if let Some(unlit) = cmd.unlit {
        std_mat.unlit = unlit;
    }
    let material = materials.add(std_mat);

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

    // Store the parametric shape so it survives save/load cycles.
    let parametric = ParametricShape { shape };

    let wid = next_id.alloc();
    let entity = commands
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            transform,
            Name::new(cmd.name.clone()),
            GenEntity {
                entity_type: GenEntityType::Primitive,
                world_id: wid,
            },
            parametric,
            crate::terrain::TerrainFollower,
        ))
        .id();

    // Handle parenting
    if let Some(ref parent_name) = cmd.parent
        && let Some(parent_entity) = registry.get_entity(parent_name)
    {
        attach_to_parent(commands, entity, parent_entity);
    }

    registry.insert_with_id(cmd.name.clone(), entity, wid);

    GenResponse::Spawned {
        name: cmd.name,
        entity_id: wid.0,
    }
}

/// Gate a batch edit: refuse it whole if the scene moved past the revision
/// the caller planned against, or if any item would fail. Batches either
/// apply every item or none.
fn check_batch(
    current_revision: u64,
    expected_revision: Option<u64>,
    problems: Vec<String>,
) -> Result<(), String> {
    if let Some(expected) = expected_revision
        && expected != current_revision
    {
        return Err(format!(
            "Scene changed since revision {expected} (now {current_revision}); \
             nothing was applied. Re-read the scene with gen_scene_info and retry."
        ));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Batch rejected; nothing was applied:\n{}",
            problems.join("\n")
        ))
    }
}

fn validate_spawn_batch(entities: &[SpawnPrimitiveCmd], registry: &NameRegistry) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cmd in entities {
        if cmd.name.is_empty() {
            problems.push("An entity has an empty name".to_string());
        } else if registry.contains_name(&cmd.name) {
            problems.push(format!("'{}': entity already exists", cmd.name));
        } else if !seen.insert(cmd.name.as_str()) {
            problems.push(format!("'{}': name appears twice in the batch", cmd.name));
        }
        if let Some(parent) = &cmd.parent
            && !registry.contains_name(parent)
            && !seen.contains(parent.as_str())
        {
            problems.push(format!(
                "'{}': parent '{}' not found (it must exist or be spawned earlier in the batch)",
                cmd.name, parent
            ));
        }
    }
    problems
}

fn validate_modify_batch(entities: &[ModifyEntityCmd], registry: &NameRegistry) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for cmd in entities {
        if !registry.contains_name(&cmd.name) {
            problems.push(format!("'{}': entity not found", cmd.name));
        }
        // Edits are deferred to the end of the frame, so a second edit of the
        // same entity would read stale state and silently undo the first.
        if !seen.insert(cmd.name.as_str()) {
            problems.push(format!(
                "'{}': appears twice in the batch; merge the edits into one item",
                cmd.name
            ));
        }
        if let Some(Some(parent)) = &cmd.parent {
            if parent == &cmd.name {
                problems.push(format!("'{}': cannot be its own parent", cmd.name));
            } else if !registry.contains_name(parent) {
                problems.push(format!("'{}': parent '{}' not found", cmd.name, parent));
            }
        }
    }
    problems
}

fn validate_delete_batch(names: &[String], registry: &NameRegistry) -> Vec<String> {
    let mut problems = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for name in names {
        if !registry.contains_name(name) {
            problems.push(format!("'{name}': entity not found"));
        } else if !seen.insert(name.as_str()) {
            problems.push(format!("'{name}': appears twice in the batch"));
        }
    }
    problems
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
        || cmd.alpha_mode.is_some()
        || cmd.unlit.is_some()
        || cmd.double_sided.is_some()
        || cmd.reflectance.is_some()
    {
        // Get current material properties as defaults
        let current_mat = material_handles
            .get(entity)
            .ok()
            .and_then(|h| materials.get(&h.0))
            .cloned();

        let base = current_mat.unwrap_or_default();

        let alpha_mode = cmd
            .alpha_mode
            .as_deref()
            .map(parse_alpha_mode)
            .unwrap_or(base.alpha_mode);

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
            alpha_mode,
            unlit: cmd.unlit.unwrap_or(base.unlit),
            double_sided: cmd.double_sided.unwrap_or(base.double_sided),
            reflectance: cmd.reflectance.unwrap_or(base.reflectance),
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

    // Update projection FOV
    let projection = Projection::Perspective(PerspectiveProjection {
        fov: cmd.fov_degrees.to_radians(),
        ..default()
    });
    commands.entity(camera_entity).insert(projection);

    // Set camera transform directly
    let transform = Transform::from_translation(Vec3::from_array(cmd.position))
        .looking_at(Vec3::from_array(cmd.look_at), Vec3::Y);
    commands.entity(camera_entity).insert(transform);

    GenResponse::CameraSet
}

fn handle_set_light(
    cmd: SetLightCmd,
    commands: &mut Commands,
    registry: &mut ResMut<NameRegistry>,
    next_id: &mut ResMut<NextEntityId>,
) -> GenResponse {
    let color = Color::srgba(cmd.color[0], cmd.color[1], cmd.color[2], cmd.color[3]);

    // If light already exists, update it
    if let Some(entity) = registry.get_entity(&cmd.name) {
        commands.entity(entity).despawn();
        registry.remove_by_name(&cmd.name);
    }

    let wid = next_id.alloc();
    let entity = match cmd.light_type {
        LightType::Directional => {
            let dir = cmd.direction.unwrap_or([0.0, -1.0, -0.5]);
            let transform = Transform::from_translation(Vec3::new(0.0, 10.0, 0.0))
                .looking_at(Vec3::new(0.0, 10.0, 0.0) + Vec3::from_array(dir), Vec3::Y);
            commands
                .spawn((
                    DirectionalLight {
                        illuminance: cmd.intensity,
                        shadow_maps_enabled: cmd.shadows,
                        color,
                        ..default()
                    },
                    transform,
                    Name::new(cmd.name.clone()),
                    GenEntity {
                        entity_type: GenEntityType::Light,
                        world_id: wid,
                    },
                ))
                .id()
        }
        LightType::Point => {
            let pos = cmd.position.unwrap_or([0.0, 5.0, 0.0]);
            let mut pl = PointLight {
                intensity: cmd.intensity,
                shadow_maps_enabled: cmd.shadows,
                color,
                ..default()
            };
            if let Some(r) = cmd.range {
                pl.range = r;
            }
            commands
                .spawn((
                    pl,
                    Transform::from_translation(Vec3::from_array(pos)),
                    Name::new(cmd.name.clone()),
                    GenEntity {
                        entity_type: GenEntityType::Light,
                        world_id: wid,
                    },
                ))
                .id()
        }
        LightType::Spot => {
            let pos = cmd.position.unwrap_or([0.0, 5.0, 0.0]);
            let dir = cmd.direction.unwrap_or([0.0, -1.0, 0.0]);
            let transform = Transform::from_translation(Vec3::from_array(pos))
                .looking_at(Vec3::from_array(pos) + Vec3::from_array(dir), Vec3::Y);
            let mut sl = SpotLight {
                intensity: cmd.intensity,
                shadow_maps_enabled: cmd.shadows,
                color,
                ..default()
            };
            if let Some(r) = cmd.range {
                sl.range = r;
            }
            if let Some(oa) = cmd.outer_angle {
                sl.outer_angle = oa;
            }
            if let Some(ia) = cmd.inner_angle {
                sl.inner_angle = ia;
            }
            commands
                .spawn((
                    sl,
                    transform,
                    Name::new(cmd.name.clone()),
                    GenEntity {
                        entity_type: GenEntityType::Light,
                        world_id: wid,
                    },
                ))
                .id()
        }
    };

    registry.insert_with_id(cmd.name.clone(), entity, wid);

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
    next_id: &mut ResMut<NextEntityId>,
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
        mesh.duplicate_vertices();
        mesh.compute_flat_normals();
    }

    // UVs
    if let Some(uvs) = cmd.uvs {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    }

    let mut std_mat = StandardMaterial {
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
    };
    if let Some(ref am_str) = cmd.alpha_mode {
        std_mat.alpha_mode = parse_alpha_mode(am_str);
    }
    if let Some(unlit) = cmd.unlit {
        std_mat.unlit = unlit;
    }
    if let Some(ds) = cmd.double_sided {
        std_mat.double_sided = ds;
    }
    if let Some(r) = cmd.reflectance {
        std_mat.reflectance = r;
    }
    let material = materials.add(std_mat);

    let wid = next_id.alloc();
    let entity = commands
        .spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material),
            Transform {
                translation: Vec3::from_array(cmd.position),
                rotation: Quat::from_euler(
                    EulerRot::XYZ,
                    cmd.rotation_degrees[0].to_radians(),
                    cmd.rotation_degrees[1].to_radians(),
                    cmd.rotation_degrees[2].to_radians(),
                ),
                scale: Vec3::from_array(cmd.scale),
            },
            Name::new(cmd.name.clone()),
            GenEntity {
                entity_type: GenEntityType::Mesh,
                world_id: wid,
            },
        ))
        .id();

    registry.insert_with_id(cmd.name.clone(), entity, wid);

    // Parent
    if let Some(ref parent_name) = cmd.parent
        && let Some(parent_entity) = registry.get_entity(parent_name)
    {
        attach_to_parent(commands, entity, parent_entity);
    }

    GenResponse::Spawned {
        name: cmd.name,
        entity_id: wid.0,
    }
}

// ---------------------------------------------------------------------------
// Spawn world entities from RON WorldManifest
// ---------------------------------------------------------------------------

/// Spawn all entities from a loaded RON `WorldManifest`.
///
/// Creates Bevy ECS entities through `WorldEntity` definitions, preserving:
/// - Parametric shapes (via `ParametricShape` component)
/// - Materials (via `StandardMaterial`)
/// - Lights (directional / point / spot)
/// - Behaviors (attached after spawn)
/// - Parent-child relationships (resolved after all entities are spawned)
#[allow(clippy::too_many_arguments)]
fn spawn_world_entities(
    world_entities: &[wt::WorldEntity],
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<NameRegistry>,
    next_entity_id: &mut ResMut<NextEntityId>,
    behavior_state: &mut BehaviorState,
    asset_server: &Res<AssetServer>,
    pending_gltf: &mut ResMut<PendingGltfLoads>,
    world_dir: Option<&Path>,
) {
    // First pass: collect world_id → entity name for parent resolution
    let id_to_name: std::collections::HashMap<u64, String> = world_entities
        .iter()
        .map(|we| (we.id.0, we.name.as_str().to_string()))
        .collect();

    // Deferred parent assignments (child_name, parent_name)
    let mut parent_assignments: Vec<(String, String)> = Vec::new();

    for we in world_entities {
        let name = we.name.as_str().to_string();

        // Skip if already exists
        if registry.contains_name(&name) {
            tracing::warn!("Entity '{}' already exists, skipping", name);
            continue;
        }

        let transform = Transform {
            translation: Vec3::from_array(we.transform.position),
            rotation: Quat::from_euler(
                EulerRot::XYZ,
                we.transform.rotation_degrees[0].to_radians(),
                we.transform.rotation_degrees[1].to_radians(),
                we.transform.rotation_degrees[2].to_radians(),
            ),
            scale: Vec3::from_array(we.transform.scale),
        };

        let world_id = wt::EntityId(we.id.0);
        next_entity_id.ensure_at_least(we.id.0 + 1);

        // Determine what kind of entity to spawn based on component slots
        let bevy_entity = if let Some(ref shape) = we.shape {
            // Entity with a parametric shape → spawn mesh
            let mat = we.material.as_ref().cloned().unwrap_or_default();
            let mesh_handle = meshes.add(shape_mesh_for(shape, &mat));
            let (material, textures) = textured_material(&mat, world_dir, asset_server);
            let material_handle = materials.add(material);

            let parametric = ParametricShape {
                shape: shape.clone(),
            };

            let mut entity_cmd = commands.spawn((
                Mesh3d(mesh_handle),
                MeshMaterial3d(material_handle),
                transform,
                Name::new(name.clone()),
                GenEntity {
                    entity_type: GenEntityType::Primitive,
                    world_id,
                },
                parametric,
            ));
            if !textures.maps.is_empty() {
                entity_cmd.insert(textures);
            }

            // If entity also has a light, add it as a child or additional component
            if let Some(ref light) = we.light {
                insert_light_component(&mut entity_cmd, light);
            }

            entity_cmd.id()
        } else if let Some(ref light) = we.light {
            // Light-only entity (no shape)
            spawn_light_entity(light, &name, transform, world_id, commands)
        } else if let Some(ref mesh_ref) = we.mesh_asset {
            // Imported glTF mesh — resolve path (relative or absolute)
            if world_dir.is_none() && mesh_ref.path.starts_with("assets/") {
                tracing::warn!(
                    "Relative mesh path '{}' for entity '{}' but no world_dir provided",
                    mesh_ref.path,
                    name
                );
            }
            let mesh_path = resolve_mesh_asset_path(world_dir, &mesh_ref.path);

            let p = mesh_path.as_path();
            if p.exists() {
                let asset_path = p.to_string_lossy().trim_start_matches('/').to_string();
                let handle = asset_server.load::<WorldAsset>(format!("{}#Scene0", asset_path));
                pending_gltf.queue.push(PendingGltfLoad {
                    handle,
                    name: name.clone(),
                    path: mesh_ref.path.clone(),
                    send_response: false,
                    segment: false,
                });
            } else {
                tracing::warn!(
                    "Mesh asset '{}' for entity '{}' not found, spawning as group",
                    mesh_ref.path,
                    name
                );
            }
            // Spawn a placeholder entity — process_pending_gltf_loads will
            // replace it when the glTF finishes loading. For now register
            // so behaviors and parent assignments can still resolve by name.
            commands
                .spawn((
                    transform,
                    Name::new(name.clone()),
                    GenEntity {
                        entity_type: GenEntityType::Mesh,
                        world_id,
                    },
                ))
                .id()
        } else {
            // Empty entity (group, audio-only, etc.)
            commands
                .spawn((
                    transform,
                    Name::new(name.clone()),
                    GenEntity {
                        entity_type: GenEntityType::Group,
                        world_id,
                    },
                ))
                .id()
        };

        // Visibility — applies to all entity types
        if !we.transform.visible {
            commands.entity(bevy_entity).insert(Visibility::Hidden);
        }

        registry.insert_with_id(name.clone(), bevy_entity, world_id);

        // Attach behaviors
        if !we.behaviors.is_empty() {
            let mut behavior_instances: Vec<behaviors::BehaviorInstance> = Vec::new();
            for wt_behavior in &we.behaviors {
                let cmd_behavior: BehaviorDef = wt_behavior.into();
                behavior_instances.push(behaviors::BehaviorInstance {
                    id: behavior_state.next_id(),
                    def: cmd_behavior,
                    base_position: transform.translation,
                    base_scale: transform.scale,
                });
            }
            commands
                .entity(bevy_entity)
                .insert(behaviors::EntityBehaviors {
                    behaviors: behavior_instances,
                });
        }

        // Record deferred parent assignment
        if let Some(ref parent_id) = we.parent
            && let Some(parent_name) = id_to_name.get(&parent_id.0)
        {
            parent_assignments.push((name, parent_name.clone()));
        }
    }

    // Second pass: resolve parent-child relationships (saved transforms are
    // local to the parent).
    for (child_name, parent_name) in &parent_assignments {
        if let (Some(child), Some(parent)) = (
            registry.get_entity(child_name),
            registry.get_entity(parent_name),
        ) {
            attach_to_parent(commands, child, parent);
        }
    }
}

/// Attach a child spawned this frame to its parent, keeping its `Transform`
/// as the offset from the parent that the command or the saved world gave it.
/// `set_parent_in_place` keeps the *global* transform instead, which is still
/// identity for an entity spawned this frame, so it collapsed children onto
/// their parents.
fn attach_to_parent(commands: &mut Commands, child: Entity, parent: Entity) {
    commands.entity(child).insert(ChildOf(parent));
}

/// Convert a `wt::Shape` to a Bevy `Mesh` handle.
pub(crate) fn shape_to_mesh(shape: &wt::Shape, meshes: &mut ResMut<Assets<Mesh>>) -> Handle<Mesh> {
    meshes.add(localgpt_world_bevy::shape_mesh(shape))
}

/// Parse an alpha mode string (e.g., "blend", "opaque", "mask:0.5").
fn parse_alpha_mode(s: &str) -> AlphaMode {
    match s.to_lowercase().as_str() {
        "opaque" => AlphaMode::Opaque,
        "blend" => AlphaMode::Blend,
        "add" => AlphaMode::Add,
        "multiply" => AlphaMode::Multiply,
        s if s.starts_with("mask") => {
            let cutoff = s
                .strip_prefix("mask:")
                .or_else(|| s.strip_prefix("mask(").and_then(|s| s.strip_suffix(')')))
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(0.5);
            AlphaMode::Mask(cutoff)
        }
        _ => AlphaMode::Opaque,
    }
}

/// Convert a `MaterialDef` to a Bevy `StandardMaterial`.
pub(crate) fn material_def_to_standard(mat: &wt::MaterialDef) -> StandardMaterial {
    localgpt_world_bevy::standard_material(mat)
}

/// A `MaterialDef` as a `StandardMaterial` with its texture maps loaded.
///
/// Texture paths resolve like mesh assets (relative to the world's `assets/`
/// folder, else absolute). Returns the material and the maps that were found
/// on disk, to attach as [`MaterialTextures`]; missing images are skipped with
/// a warning so the entity still renders with its scalar factors.
pub(crate) fn textured_material(
    mat: &wt::MaterialDef,
    world_dir: Option<&Path>,
    asset_server: &AssetServer,
) -> (StandardMaterial, MaterialTextures) {
    let mut material = localgpt_world_bevy::standard_material(mat);
    let mut textures = MaterialTextures::default();
    let mut found = mat.clone();
    for (slot, path) in mat.textures() {
        let resolved = resolve_mesh_asset_path(world_dir, path);
        if resolved.exists() {
            let resolved = resolved.canonicalize().unwrap_or(resolved);
            textures.set(slot, resolved.to_string_lossy().into_owned());
        } else {
            tracing::warn!("Texture '{}' not found; skipping it", path);
            *found.texture_mut(slot) = None;
        }
    }
    let sources: std::collections::HashMap<_, _> = textures.maps.iter().cloned().collect();
    localgpt_world_bevy::apply_textures(&mut material, &found, |slot, _| {
        let source = &sources[&slot];
        asset_server
            .load_builder()
            .with_settings(localgpt_world_bevy::texture_settings(slot))
            .load(source.trim_start_matches('/').to_string())
    });
    (material, textures)
}

/// The mesh for a shape, with tangents when the material has a normal map.
pub(crate) fn shape_mesh_for(shape: &wt::Shape, mat: &wt::MaterialDef) -> Mesh {
    let mut mesh = localgpt_world_bevy::shape_mesh(shape);
    if localgpt_world_bevy::needs_tangents(mat)
        && let Err(e) = mesh.generate_tangents()
    {
        tracing::warn!("Could not generate tangents for a normal-mapped mesh: {e}");
    }
    mesh
}

/// Insert a light component onto an existing entity command builder.
pub(crate) fn insert_light_component(
    entity_cmd: &mut bevy::ecs::system::EntityCommands,
    light: &wt::LightDef,
) {
    localgpt_world_bevy::insert_light(entity_cmd, light)
}

/// Spawn a standalone light entity (no shape).
fn spawn_light_entity(
    light: &wt::LightDef,
    name: &str,
    transform: Transform,
    world_id: wt::EntityId,
    commands: &mut Commands,
) -> bevy::ecs::entity::Entity {
    let color = Color::srgba(
        light.color[0],
        light.color[1],
        light.color[2],
        light.color[3],
    );
    match light.light_type {
        wt::LightType::Directional => {
            let dir = light.direction.unwrap_or([0.0, -1.0, -0.5]);
            let light_transform = Transform::from_translation(transform.translation)
                .looking_at(transform.translation + Vec3::from_array(dir), Vec3::Y);
            commands
                .spawn((
                    DirectionalLight {
                        illuminance: light.intensity,
                        shadow_maps_enabled: light.shadows,
                        color,
                        ..default()
                    },
                    light_transform,
                    Name::new(name.to_string()),
                    GenEntity {
                        entity_type: GenEntityType::Light,
                        world_id,
                    },
                ))
                .id()
        }
        wt::LightType::Point => {
            let mut pl = PointLight {
                intensity: light.intensity,
                shadow_maps_enabled: light.shadows,
                color,
                ..default()
            };
            if let Some(r) = light.range {
                pl.range = r;
            }
            commands
                .spawn((
                    pl,
                    transform,
                    Name::new(name.to_string()),
                    GenEntity {
                        entity_type: GenEntityType::Light,
                        world_id,
                    },
                ))
                .id()
        }
        wt::LightType::Spot => {
            let dir = light.direction.unwrap_or([0.0, -1.0, 0.0]);
            let light_transform = Transform::from_translation(transform.translation)
                .looking_at(transform.translation + Vec3::from_array(dir), Vec3::Y);
            let mut sl = SpotLight {
                intensity: light.intensity,
                shadow_maps_enabled: light.shadows,
                color,
                ..default()
            };
            if let Some(r) = light.range {
                sl.range = r;
            }
            if let Some(oa) = light.outer_angle {
                sl.outer_angle = oa;
            }
            if let Some(ia) = light.inner_angle {
                sl.inner_angle = ia;
            }
            commands
                .spawn((
                    sl,
                    light_transform,
                    Name::new(name.to_string()),
                    GenEntity {
                        entity_type: GenEntityType::Light,
                        world_id,
                    },
                ))
                .id()
        }
    }
}

// ---------------------------------------------------------------------------
// Undo/Redo support
// ---------------------------------------------------------------------------

/// Borrows all the queries needed to snapshot entity state.
/// Avoids passing 12+ individual query parameters to `snapshot_entity`.
/// Read-only query bundle used to snapshot live entities into `wt::WorldEntity`.
pub(crate) struct SnapshotQueries<'a, 'w, 's> {
    pub(crate) transforms: &'a Query<'w, 's, &'static Transform>,
    pub(crate) parametric_shapes: &'a Query<'w, 's, &'static ParametricShape>,
    pub(crate) material_handles: &'a Query<'w, 's, &'static MeshMaterial3d<StandardMaterial>>,
    pub(crate) materials: &'a Assets<StandardMaterial>,
    pub(crate) visibility_query: &'a Query<'w, 's, &'static Visibility>,
    pub(crate) directional_lights: &'a Query<'w, 's, &'static DirectionalLight>,
    pub(crate) point_lights: &'a Query<'w, 's, &'static PointLight>,
    pub(crate) spot_lights: &'a Query<'w, 's, &'static SpotLight>,
    pub(crate) behaviors_query: Query<'w, 's, &'static EntityBehaviors>,
    pub(crate) audio_emitters: &'a Query<'w, 's, &'static audio::AudioEmitter>,
    pub(crate) parent_query: &'a Query<'w, 's, &'static ChildOf>,
    pub(crate) gltf_sources: &'a Query<'w, 's, &'static GltfSource>,
    /// Texture sources; `None` leaves texture paths out of the snapshot (the
    /// multiplayer host: paths on the host's disk mean nothing to a client).
    pub(crate) material_textures: Option<&'a Query<'w, 's, &'static MaterialTextures>>,
    pub(crate) registry: &'a NameRegistry,
}

/// Capture the current ECS state of an entity as a `wt::WorldEntity`.
///
/// Used to record the entity state before/after modifications for undo history,
/// and by the multiplayer host to derive replicated net components.
pub(crate) fn snapshot_entity(
    name: &str,
    entity: bevy::ecs::entity::Entity,
    id: wt::EntityId,
    sq: &SnapshotQueries,
) -> wt::WorldEntity {
    let mut we = wt::WorldEntity::new(id.0, name);

    if let Ok(transform) = sq.transforms.get(entity) {
        let euler = transform.rotation.to_euler(EulerRot::XYZ);
        we.transform = wt::WorldTransform {
            position: transform.translation.to_array(),
            rotation_degrees: [
                euler.0.to_degrees(),
                euler.1.to_degrees(),
                euler.2.to_degrees(),
            ],
            scale: transform.scale.to_array(),
            visible: sq
                .visibility_query
                .get(entity)
                .map(|v| *v != Visibility::Hidden)
                .unwrap_or(true),
        };
    }

    if let Ok(param) = sq.parametric_shapes.get(entity) {
        we.shape = Some(param.shape.clone());
    }

    if let Ok(mat_handle) = sq.material_handles.get(entity)
        && let Some(mat) = sq.materials.get(&mat_handle.0)
    {
        let c = mat.base_color.to_srgba();
        let e = mat.emissive;
        let alpha_mode = match mat.alpha_mode {
            AlphaMode::Opaque => None,
            AlphaMode::Mask(cutoff) => Some(wt::AlphaModeDef::Mask(cutoff)),
            AlphaMode::Blend => Some(wt::AlphaModeDef::Blend),
            AlphaMode::Add => Some(wt::AlphaModeDef::Add),
            AlphaMode::Multiply => Some(wt::AlphaModeDef::Multiply),
            _ => None,
        };
        we.material = Some(wt::MaterialDef {
            color: [c.red, c.green, c.blue, c.alpha],
            metallic: mat.metallic,
            roughness: mat.perceptual_roughness,
            emissive: [e.red, e.green, e.blue, e.alpha],
            alpha_mode,
            unlit: if mat.unlit { Some(true) } else { None },
            double_sided: if mat.double_sided { Some(true) } else { None },
            reflectance: if (mat.reflectance - 0.5).abs() > f32::EPSILON {
                Some(mat.reflectance)
            } else {
                None
            },
            ..Default::default()
        });
        if let (Some(q), Some(def)) = (sq.material_textures, we.material.as_mut())
            && let Ok(textures) = q.get(entity)
        {
            textures.apply_to(def, str::to_string);
        }
    }

    // Mesh asset (imported glTF)
    if let Ok(gltf_src) = sq.gltf_sources.get(entity) {
        we.mesh_asset = Some(wt::MeshAssetRef {
            path: gltf_src.path.clone(),
            node: None,
        });
    }

    // Light
    if let Ok(dl) = sq.directional_lights.get(entity) {
        let c = dl.color.to_srgba();
        let dir = sq
            .transforms
            .get(entity)
            .ok()
            .map(|t| t.forward().as_vec3().to_array());
        we.light = Some(wt::LightDef {
            light_type: wt::LightType::Directional,
            color: [c.red, c.green, c.blue, c.alpha],
            intensity: dl.illuminance,
            direction: dir,
            shadows: dl.shadow_maps_enabled,
            range: None,
            outer_angle: None,
            inner_angle: None,
        });
    } else if let Ok(pl) = sq.point_lights.get(entity) {
        let c = pl.color.to_srgba();
        we.light = Some(wt::LightDef {
            light_type: wt::LightType::Point,
            color: [c.red, c.green, c.blue, c.alpha],
            intensity: pl.intensity,
            direction: None,
            shadows: pl.shadow_maps_enabled,
            range: Some(pl.range),
            outer_angle: None,
            inner_angle: None,
        });
    } else if let Ok(sl) = sq.spot_lights.get(entity) {
        let c = sl.color.to_srgba();
        let dir = sq
            .transforms
            .get(entity)
            .ok()
            .map(|t| t.forward().as_vec3().to_array());
        we.light = Some(wt::LightDef {
            light_type: wt::LightType::Spot,
            color: [c.red, c.green, c.blue, c.alpha],
            intensity: sl.intensity,
            direction: dir,
            shadows: sl.shadow_maps_enabled,
            range: Some(sl.range),
            outer_angle: Some(sl.outer_angle),
            inner_angle: Some(sl.inner_angle),
        });
    }

    // Behaviors
    if let Ok(eb) = sq.behaviors_query.get(entity) {
        we.behaviors = eb
            .behaviors
            .iter()
            .map(|bi| wt::BehaviorDef::from(&bi.def))
            .collect();
    }

    // Audio
    if let Ok(ae) = sq.audio_emitters.get(entity) {
        we.audio = Some(wt::AudioDef {
            kind: wt::AudioKind::Sfx,
            source: wt::AudioSource::from(&ae.sound),
            volume: ae.volume,
            radius: Some(ae.radius),
            rolloff: wt::Rolloff::InverseSquare,
        });
    }

    // Parent
    if let Ok(child_of) = sq.parent_query.get(entity)
        && let Some(parent_id) = sq.registry.get_id(child_of.0)
    {
        we.parent = Some(parent_id);
    }

    we
}

/// Construct the expected post-modify state by applying a `ModifyEntityCmd`
/// to a pre-modify snapshot.  Used for undo/redo of modify operations.
fn apply_modify_to_snapshot(we: &mut wt::WorldEntity, cmd: &ModifyEntityCmd) {
    if let Some(pos) = cmd.position {
        we.transform.position = pos;
    }
    if let Some(rot) = cmd.rotation_degrees {
        we.transform.rotation_degrees = rot;
    }
    if let Some(scale) = cmd.scale {
        we.transform.scale = scale;
    }
    if let Some(visible) = cmd.visible {
        we.transform.visible = visible;
    }
    if cmd.color.is_some()
        || cmd.metallic.is_some()
        || cmd.roughness.is_some()
        || cmd.emissive.is_some()
        || cmd.alpha_mode.is_some()
        || cmd.unlit.is_some()
    {
        let mut mat = we.material.clone().unwrap_or_default();
        if let Some(color) = cmd.color {
            mat.color = color;
        }
        if let Some(metallic) = cmd.metallic {
            mat.metallic = metallic;
        }
        if let Some(roughness) = cmd.roughness {
            mat.roughness = roughness;
        }
        if let Some(emissive) = cmd.emissive {
            mat.emissive = emissive;
        }
        if let Some(ref am_str) = cmd.alpha_mode {
            mat.alpha_mode = Some(match am_str.to_lowercase().as_str() {
                "blend" => wt::AlphaModeDef::Blend,
                "add" => wt::AlphaModeDef::Add,
                "multiply" => wt::AlphaModeDef::Multiply,
                s if s.starts_with("mask") => {
                    let cutoff = s
                        .strip_prefix("mask:")
                        .or_else(|| s.strip_prefix("mask(").and_then(|s| s.strip_suffix(')')))
                        .and_then(|v| v.parse::<f32>().ok())
                        .unwrap_or(0.5);
                    wt::AlphaModeDef::Mask(cutoff)
                }
                _ => wt::AlphaModeDef::Opaque,
            });
        }
        if let Some(unlit) = cmd.unlit {
            mat.unlit = Some(unlit);
        }
        if let Some(ds) = cmd.double_sided {
            mat.double_sided = Some(ds);
        }
        if let Some(r) = cmd.reflectance {
            mat.reflectance = Some(r);
        }
        we.material = Some(mat);
    }
    // Parent — clear if explicitly set to None
    if let Some(ref parent_opt) = cmd.parent
        && parent_opt.is_none()
    {
        we.parent = None;
    }
}

/// Convert `world_types::AudioSource` to gen's `AmbientSound`.
fn convert_audio_source_to_ambient_sound(
    source: &wt::AudioSource,
) -> super::commands::AmbientSound {
    use super::commands::AmbientSound;
    match source {
        wt::AudioSource::Wind { speed, gustiness } => AmbientSound::Wind {
            speed: *speed,
            gustiness: *gustiness,
        },
        wt::AudioSource::Rain { intensity } => AmbientSound::Rain {
            intensity: *intensity,
        },
        wt::AudioSource::Forest { bird_density, wind } => AmbientSound::Forest {
            bird_density: *bird_density,
            wind: *wind,
        },
        wt::AudioSource::Ocean { wave_size } => AmbientSound::Ocean {
            wave_size: *wave_size,
        },
        wt::AudioSource::Cave {
            drip_rate,
            resonance,
        } => AmbientSound::Cave {
            drip_rate: *drip_rate,
            resonance: *resonance,
        },
        wt::AudioSource::Stream { flow_rate } => AmbientSound::Stream {
            flow_rate: *flow_rate,
        },
        wt::AudioSource::Silence => AmbientSound::Silence,
        // For non-ambient sources, default to silence (they shouldn't appear in ambience)
        wt::AudioSource::Water { .. }
        | wt::AudioSource::Fire { .. }
        | wt::AudioSource::Hum { .. }
        | wt::AudioSource::WindEmitter { .. }
        | wt::AudioSource::Custom { .. }
        | wt::AudioSource::Abc { .. }
        | wt::AudioSource::File { .. } => AmbientSound::Silence,
    }
}

/// Convert `world_types::AudioDef` to gen's `EmitterSound`.
fn convert_audio_def_to_emitter_sound(audio: &wt::AudioDef) -> super::commands::EmitterSound {
    use super::commands::EmitterSound;
    match &audio.source {
        wt::AudioSource::Water { turbulence } => EmitterSound::Water {
            turbulence: *turbulence,
        },
        wt::AudioSource::Fire { intensity, crackle } => EmitterSound::Fire {
            intensity: *intensity,
            crackle: *crackle,
        },
        wt::AudioSource::Hum { frequency, warmth } => EmitterSound::Hum {
            frequency: *frequency,
            warmth: *warmth,
        },
        wt::AudioSource::WindEmitter { pitch } => EmitterSound::Wind { pitch: *pitch },
        wt::AudioSource::Custom {
            waveform,
            filter_cutoff,
            filter_type,
        } => EmitterSound::Custom {
            waveform: convert_waveform_type(*waveform),
            filter_cutoff: *filter_cutoff,
            filter_type: convert_filter_type(*filter_type),
        },
        // For non-emitter sources, default to a hum
        wt::AudioSource::Wind { .. }
        | wt::AudioSource::Rain { .. }
        | wt::AudioSource::Forest { .. }
        | wt::AudioSource::Ocean { .. }
        | wt::AudioSource::Cave { .. }
        | wt::AudioSource::Stream { .. }
        | wt::AudioSource::Silence
        | wt::AudioSource::Abc { .. }
        | wt::AudioSource::File { .. } => EmitterSound::Hum {
            frequency: 220.0,
            warmth: 0.5,
        },
    }
}

fn convert_waveform_type(wt: wt::WaveformType) -> super::commands::WaveformType {
    match wt {
        wt::WaveformType::Sine => super::commands::WaveformType::Sine,
        wt::WaveformType::Saw => super::commands::WaveformType::Saw,
        wt::WaveformType::Square => super::commands::WaveformType::Square,
        wt::WaveformType::WhiteNoise => super::commands::WaveformType::WhiteNoise,
        wt::WaveformType::PinkNoise => super::commands::WaveformType::PinkNoise,
        wt::WaveformType::BrownNoise => super::commands::WaveformType::BrownNoise,
    }
}

fn convert_filter_type(ft: wt::FilterType) -> super::commands::FilterType {
    match ft {
        wt::FilterType::Lowpass => super::commands::FilterType::Lowpass,
        wt::FilterType::Highpass => super::commands::FilterType::Highpass,
        wt::FilterType::Bandpass => super::commands::FilterType::Bandpass,
    }
}

/// Apply a single `EditOp` to the scene. Returns a human-readable description.
#[allow(clippy::too_many_arguments)]
fn apply_edit_op(
    op: &wt::EditOp,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<NameRegistry>,
    next_entity_id: &mut ResMut<NextEntityId>,
    behavior_state: &mut BehaviorState,
    asset_server: &Res<AssetServer>,
    pending_gltf: &mut ResMut<PendingGltfLoads>,
    audio_engine: &mut audio::AudioEngine,
) -> String {
    match op {
        wt::EditOp::DeleteEntity { id } => {
            if let Some(entity) = registry.get_entity_by_id(id) {
                let name = registry.get_name(entity).unwrap_or("unknown").to_string();
                registry.remove_by_entity(entity);
                commands.entity(entity).despawn();
                format!("deleted '{}'", name)
            } else {
                format!("entity id {} not found", id.0)
            }
        }
        wt::EditOp::SpawnEntity { entity } => {
            let name = entity.name.as_str().to_string();
            spawn_world_entities(
                std::slice::from_ref(entity),
                commands,
                meshes,
                materials,
                registry,
                next_entity_id,
                behavior_state,
                asset_server,
                pending_gltf,
                None, // No world_dir for undo/redo — paths are absolute
            );
            format!("re-spawned '{}'", name)
        }
        wt::EditOp::ModifyEntity { id, patch } => {
            if let Some(entity) = registry.get_entity_by_id(id) {
                let name = registry.get_name(entity).unwrap_or("unknown").to_string();
                // Build a minimal WorldEntity from what we know, apply the patch,
                // then delete-and-respawn to apply all changes atomically.
                let mut we = wt::WorldEntity::new(id.0, &name);
                patch.apply(&mut we);
                // Delete old
                registry.remove_by_entity(entity);
                commands.entity(entity).despawn();
                // Spawn patched version
                spawn_world_entities(
                    std::slice::from_ref(&we),
                    commands,
                    meshes,
                    materials,
                    registry,
                    next_entity_id,
                    behavior_state,
                    asset_server,
                    pending_gltf,
                    None, // No world_dir for undo/redo — paths are absolute
                );
                format!("modified '{}'", name)
            } else {
                format!("entity id {} not found for modify", id.0)
            }
        }
        wt::EditOp::SetEnvironment { env } => {
            if let Some(color) = env.background_color {
                commands.insert_resource(ClearColor(Color::srgba(
                    color[0], color[1], color[2], color[3],
                )));
            }
            if let Some(intensity) = env.ambient_intensity {
                let color = env
                    .ambient_color
                    .map(|c| Color::srgba(c[0], c[1], c[2], c[3]))
                    .unwrap_or(Color::WHITE);
                commands.insert_resource(GlobalAmbientLight {
                    color,
                    brightness: intensity,
                    affects_lightmapped_meshes: true,
                });
            }
            "restored environment".to_string()
        }
        wt::EditOp::SetCamera { camera } => {
            if let Some(cam_entity) = registry.get_entity("main_camera") {
                let transform = Transform::from_translation(Vec3::from_array(camera.position))
                    .looking_at(Vec3::from_array(camera.look_at), Vec3::Y);
                commands.entity(cam_entity).insert(transform);
                commands.entity(cam_entity).insert(Projection::Perspective(
                    PerspectiveProjection {
                        fov: camera.fov_degrees.to_radians(),
                        ..default()
                    },
                ));
                "restored camera".to_string()
            } else {
                "main_camera not found".to_string()
            }
        }
        wt::EditOp::Batch { ops } => {
            let descriptions: Vec<String> = ops
                .iter()
                .map(|o| {
                    apply_edit_op(
                        o,
                        commands,
                        meshes,
                        materials,
                        registry,
                        next_entity_id,
                        behavior_state,
                        asset_server,
                        pending_gltf,
                        audio_engine,
                    )
                })
                .collect();
            descriptions.join("; ")
        }
        wt::EditOp::SetAmbience { ambience } => {
            // Convert from world-types AmbienceLayerDef to gen AmbienceLayerDef
            let layers: Vec<super::commands::AmbienceLayerDef> = ambience
                .iter()
                .map(|layer| super::commands::AmbienceLayerDef {
                    name: layer.name.clone(),
                    sound: convert_audio_source_to_ambient_sound(&layer.source),
                    volume: layer.volume,
                })
                .collect();

            let cmd = super::commands::AmbienceCmd {
                layers,
                master_volume: None,
                reverb: None,
            };

            audio::handle_set_ambience(cmd, audio_engine);
            "restored ambience".to_string()
        }
        wt::EditOp::SpawnAudioEmitter { name, audio } => {
            let cmd = super::commands::AudioEmitterCmd {
                name: name.clone(),
                sound: convert_audio_def_to_emitter_sound(audio),
                volume: audio.volume,
                radius: audio.radius.unwrap_or(10.0),
                entity: None,   // Entity attachment not preserved in AudioDef
                position: None, // Position not preserved in AudioDef
            };

            audio::handle_spawn_audio_emitter(
                cmd,
                audio_engine,
                commands,
                registry,
                next_entity_id,
            );
            format!("re-spawned audio emitter '{}'", name)
        }
        wt::EditOp::RemoveAudioEmitter { name, audio: _ } => {
            audio::handle_remove_audio_emitter(name, audio_engine);
            format!("removed audio emitter '{}'", name)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_undo(
    undo_stack: &mut ResMut<UndoStack>,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<NameRegistry>,
    next_entity_id: &mut ResMut<NextEntityId>,
    behavior_state: &mut BehaviorState,
    asset_server: &Res<AssetServer>,
    pending_gltf: &mut ResMut<PendingGltfLoads>,
    audio_engine: &mut audio::AudioEngine,
) -> GenResponse {
    let op = match undo_stack.undo() {
        Some(op) => op,
        None => return GenResponse::NothingToUndo,
    };

    let description = apply_edit_op(
        &op,
        commands,
        meshes,
        materials,
        registry,
        next_entity_id,
        behavior_state,
        asset_server,
        pending_gltf,
        audio_engine,
    );
    GenResponse::Undone { description }
}

#[allow(clippy::too_many_arguments)]
fn handle_redo(
    undo_stack: &mut ResMut<UndoStack>,
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    registry: &mut ResMut<NameRegistry>,
    next_entity_id: &mut ResMut<NextEntityId>,
    behavior_state: &mut BehaviorState,
    asset_server: &Res<AssetServer>,
    pending_gltf: &mut ResMut<PendingGltfLoads>,
    audio_engine: &mut audio::AudioEngine,
) -> GenResponse {
    let op = match undo_stack.redo() {
        Some(op) => op,
        None => return GenResponse::NothingToRedo,
    };

    let description = apply_edit_op(
        &op,
        commands,
        meshes,
        materials,
        registry,
        next_entity_id,
        behavior_state,
        asset_server,
        pending_gltf,
        audio_engine,
    );
    GenResponse::Redone { description }
}

// ---------------------------------------------------------------------------
// Scene management
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_clear_scene(
    keep_camera: bool,
    keep_lights: bool,
    commands: &mut Commands,
    registry: &mut NameRegistry,
    gen_entities: &Query<&GenEntity>,
    audio_engine: &mut audio::AudioEngine,
    behavior_state: &mut BehaviorState,
    pending_world: &mut PendingWorldSetup,
) -> GenResponse {
    let mut removed = 0;
    let all_names: Vec<(String, bevy::ecs::entity::Entity)> = registry
        .all_names()
        .map(|(n, e)| (n.to_string(), e))
        .collect();

    for (name, entity) in &all_names {
        // Optionally keep cameras and lights
        if let Ok(gen_ent) = gen_entities.get(*entity) {
            if keep_camera && gen_ent.entity_type == GenEntityType::Camera {
                continue;
            }
            if keep_lights && gen_ent.entity_type == GenEntityType::Light {
                continue;
            }
        }

        commands.entity(*entity).despawn();
        registry.remove_by_name(name);
        removed += 1;
    }

    // Stop all audio
    audio_engine.stop_all();

    // Reset behavior state
    behavior_state.elapsed = 0.0;
    behavior_state.paused = false;

    // Clear any pending world setup
    pending_world.active = None;

    GenResponse::SceneCleared {
        removed_count: removed,
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
    // Resolve output path: use provided path or default to {workspace}/exports/{datetime}.glb
    let output_path = match path {
        Some(p) if !p.is_empty() => {
            if std::path::Path::new(p).extension().is_some_and(|ext| {
                ext.eq_ignore_ascii_case("glb") || ext.eq_ignore_ascii_case("gltf")
            }) {
                p.to_string()
            } else {
                format!("{}.glb", p)
            }
        }
        _ => {
            let datetime = format_export_datetime();
            let exports_dir = workspace.path.join("exports");
            exports_dir
                .join(format!("{}.glb", datetime))
                .to_string_lossy()
                .into_owned()
        }
    };

    match super::gltf_export::export_glb(
        std::path::Path::new(&output_path),
        registry,
        transforms,
        gen_entities,
        parent_query,
        material_handles,
        material_assets,
        mesh_handles,
        mesh_assets,
    ) {
        Ok(()) => GenResponse::Exported { path: output_path },
        Err(e) => GenResponse::Error { message: e },
    }
}

/// Export the current world to the export/ folder in the world skill directory.
/// Supports two formats: "glb" (binary, default) or "gltf" (JSON + BIN).
#[allow(clippy::too_many_arguments)]
fn handle_export_world(
    format: Option<&str>,
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
    // Determine export format (default: glb)
    let export_format = format.unwrap_or("glb").to_lowercase();

    // Find the current world directory
    // First try the most recent saved/loaded world
    let world_dir = workspace
        .path
        .join("skills")
        .join("current") // We'll track this via CurrentWorld resource
        .canonicalize()
        .ok();

    // If no current world, try to find the most recently modified world
    let world_dir = match world_dir {
        Some(dir) => dir,
        None => {
            // Find the most recently modified world skill
            let skills_dir = workspace.path.join("skills");
            if !skills_dir.exists() {
                return GenResponse::Error {
                    message: "No skills directory found. Save a world first with gen_save_world."
                        .to_string(),
                };
            }

            let mut worlds: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&skills_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir()
                        && path.join("world.ron").exists()
                        && let Ok(modified) = entry.metadata().and_then(|m| m.modified())
                    {
                        worlds.push((modified, path));
                    }
                }
            }

            match worlds.iter().max_by_key(|(t, _)| *t) {
                Some((_, path)) => path.clone(),
                None => {
                    return GenResponse::Error {
                        message: "No saved worlds found. Save a world first with gen_save_world."
                            .to_string(),
                    };
                }
            }
        }
    };

    // Create export directory
    let export_dir = world_dir.join("export");
    if let Err(e) = std::fs::create_dir_all(&export_dir) {
        return GenResponse::Error {
            message: format!("Failed to create export directory: {}", e),
        };
    }

    match export_format.as_str() {
        "gltf" => {
            // Export as JSON + BIN
            match super::gltf_export::export_gltf(
                &world_dir,
                registry,
                transforms,
                gen_entities,
                parent_query,
                material_handles,
                material_assets,
                mesh_handles,
                mesh_assets,
            ) {
                Ok(()) => GenResponse::Exported {
                    path: export_dir.to_string_lossy().into_owned(),
                },
                Err(e) => GenResponse::Error { message: e },
            }
        }
        _ => {
            // Export as GLB (default)
            let datetime = format_export_datetime();
            let output_path = export_dir.join(format!("{}.glb", datetime));
            match super::gltf_export::export_glb(
                &output_path,
                registry,
                transforms,
                gen_entities,
                parent_query,
                material_handles,
                material_assets,
                mesh_handles,
                mesh_assets,
            ) {
                Ok(()) => GenResponse::Exported {
                    path: output_path.to_string_lossy().into_owned(),
                },
                Err(e) => GenResponse::Error { message: e },
            }
        }
    }
}

/// Export the current world as a self-contained HTML file using Three.js.
///
/// Reads the saved `world.ron` manifest and generates HTML with embedded
/// Three.js scene, procedural Web Audio, and animated behaviors.
fn handle_export_html(workspace: &GenWorkspace, current_world: &CurrentWorld) -> GenResponse {
    // Find the world directory — prefer current world, fallback to most recent
    let world_dir = current_world.path.clone().or_else(|| {
        let skills_dir = workspace.path.join("skills");
        let mut worlds: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir()
                    && path.join("world.ron").exists()
                    && let Ok(modified) = entry.metadata().and_then(|m| m.modified())
                {
                    worlds.push((modified, path));
                }
            }
        }
        worlds
            .iter()
            .max_by_key(|(t, _)| *t)
            .map(|(_, p)| p.clone())
    });

    let Some(world_dir) = world_dir else {
        return GenResponse::Error {
            message: "No saved worlds found. Save a world first with gen_save_world.".to_string(),
        };
    };

    // Read the world manifest (RON format)
    let ron_path = world_dir.join("world.ron");
    if !ron_path.exists() {
        return GenResponse::Error {
            message: format!(
                "No world.ron found in {}. Save a world first with gen_save_world.",
                world_dir.display()
            ),
        };
    }

    let manifest: localgpt_world_types::WorldManifest = {
        let ron_content = match std::fs::read_to_string(&ron_path) {
            Ok(c) => c,
            Err(e) => {
                return GenResponse::Error {
                    message: format!("Failed to read world.ron: {}", e),
                };
            }
        };
        match ron::from_str(&ron_content) {
            Ok(m) => m,
            Err(e) => {
                return GenResponse::Error {
                    message: format!("Failed to parse world.ron: {}", e),
                };
            }
        }
    };

    // Generate HTML with the shared web viewer (localgpt-world-export).
    let html = localgpt_world_export::generate_html(&manifest);

    // Write to export directory
    let export_dir = world_dir.join("export");
    if let Err(e) = std::fs::create_dir_all(&export_dir) {
        return GenResponse::Error {
            message: format!("Failed to create export directory: {}", e),
        };
    }

    let output_path = export_dir.join("index.html");
    match std::fs::write(&output_path, &html) {
        Ok(()) => GenResponse::Exported {
            path: output_path.to_string_lossy().into_owned(),
        },
        Err(e) => GenResponse::Error {
            message: format!("Failed to write HTML: {}", e),
        },
    }
}

// ---------------------------------------------------------------------------
// Export helpers
// ---------------------------------------------------------------------------

/// Format current date/time for export filenames: YYYY-MM-DD_HH-MM-SS
fn format_export_datetime() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    // Convert to datetime components
    let total_secs = duration.as_secs();
    let days = total_secs / 86400;
    let secs_in_day = total_secs % 86400;

    // Unix epoch is January 1, 1970 (Thursday)
    // Calculate year, month, day
    let (year, month, day) = unix_days_to_ymd(days as i64);
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;

    format!(
        "{:04}-{:02}-{:02}_{:02}-{:02}-{:02}",
        year, month, day, hour, minute, second
    )
}

/// Convert Unix days since epoch to (year, month, day)
fn unix_days_to_ymd(days: i64) -> (i32, u32, u32) {
    // Unix epoch: January 1, 1970
    let mut year = 1970i32;
    let mut remaining_days = days;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year as i64 {
            break;
        }
        remaining_days -= days_in_year as i64;
        year += 1;
    }

    let days_in_months = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1u32;
    for &days_in_month in &days_in_months {
        if remaining_days < days_in_month as i64 {
            break;
        }
        remaining_days -= days_in_month as i64;
        month += 1;
    }

    let day = (remaining_days + 1) as u32; // 1-indexed

    (year, month, day)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

// ---------------------------------------------------------------------------
// glTF path resolution
// ---------------------------------------------------------------------------

/// Where a world's mesh asset is on disk. The format keeps asset paths
/// relative to the world's `assets/` folder (worlds from Verse, the web and
/// `--world` imports); worlds Gen saved earlier store `assets/...` relative
/// to the world folder itself. Anything else is absolute or relative to the
/// working directory.
pub fn resolve_mesh_asset_path(world_dir: Option<&Path>, path: &str) -> PathBuf {
    let expanded = PathBuf::from(shellexpand::tilde(path).into_owned());
    if let Some(dir) = world_dir
        && expanded.is_relative()
    {
        let in_assets = dir.join("assets").join(&expanded);
        if in_assets.exists() {
            return in_assets;
        }
        if path.starts_with("assets/") {
            return dir.join(&expanded);
        }
    }
    expanded
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::world::CommandQueue;

    fn registry_with(names: &[&str]) -> NameRegistry {
        let mut registry = NameRegistry::default();
        for (i, name) in names.iter().enumerate() {
            registry.insert_with_id(
                name.to_string(),
                Entity::from_bits(i as u64 + 1),
                wt::EntityId(i as u64 + 1),
            );
        }
        registry
    }

    fn spawn_cmd(name: &str, parent: Option<&str>) -> SpawnPrimitiveCmd {
        serde_json::from_value(serde_json::json!({
            "name": name, "shape": "Cuboid", "parent": parent,
        }))
        .unwrap()
    }

    fn modify_cmd(name: &str) -> ModifyEntityCmd {
        serde_json::from_value(serde_json::json!({ "name": name })).unwrap()
    }

    /// The conformance textures load through the asset server the way a
    /// world load asks for them: resolved in `assets/`, colour maps as sRGB
    /// and data maps as linear.
    #[test]
    fn world_textures_load_in_their_colour_space() {
        use bevy::render::render_resource::TextureFormat;
        let world_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../world-types/conformance")
            .canonicalize()
            .unwrap();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin {
                file_path: "/".to_string(),
                ..default()
            },
            bevy::image::ImagePlugin::default(),
        ));
        // The render plugin normally registers the image loader.
        app.register_asset_loader(bevy::image::ImageLoader::new(
            bevy::image::CompressedImageFormats::empty(),
        ));
        let def = wt::MaterialDef {
            base_color_texture: Some("textures/checker_albedo.png".into()),
            normal_map_texture: Some("textures/checker_normal.png".into()),
            metallic_roughness_texture: Some("textures/missing.png".into()),
            ..Default::default()
        };
        let asset_server = app.world().resource::<AssetServer>().clone();
        let (material, textures) = textured_material(&def, Some(&world_dir), &asset_server);
        assert_eq!(textures.maps.len(), 2, "the missing map is skipped");
        assert!(material.metallic_roughness_texture.is_none());
        let albedo = material.base_color_texture.clone().unwrap();
        let normal = material.normal_map_texture.clone().unwrap();

        for _ in 0..500 {
            app.update();
            let images = app.world().resource::<Assets<Image>>();
            if images.contains(&albedo) && images.contains(&normal) {
                break;
            }
            assert!(
                !matches!(
                    asset_server.load_state(&albedo),
                    bevy::asset::LoadState::Failed(_)
                ),
                "albedo failed to load"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let images = app.world().resource::<Assets<Image>>();
        let state = asset_server.load_state(&albedo);
        let albedo = images
            .get(&albedo)
            .unwrap_or_else(|| panic!("albedo not loaded: {state:?}"));
        let normal = images.get(&normal).expect("normal map loaded");
        assert_eq!(
            albedo.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        assert_eq!(normal.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(albedo.size().x, 64);
    }

    #[test]
    fn a_batch_with_one_bad_item_is_rejected_whole() {
        let registry = registry_with(&["tower"]);
        let spawn = vec![spawn_cmd("a", None), spawn_cmd("tower", None)];
        let problems = validate_spawn_batch(&spawn, &registry);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(check_batch(0, None, problems).is_err());

        let modify = vec![modify_cmd("tower"), modify_cmd("missing")];
        assert_eq!(validate_modify_batch(&modify, &registry).len(), 1);

        let delete = vec!["tower".to_string(), "missing".to_string()];
        assert_eq!(validate_delete_batch(&delete, &registry).len(), 1);
    }

    #[test]
    fn a_spawn_batch_may_parent_to_an_earlier_item() {
        let registry = registry_with(&[]);
        let ok = vec![spawn_cmd("base", None), spawn_cmd("roof", Some("base"))];
        assert!(validate_spawn_batch(&ok, &registry).is_empty());
        let backwards = vec![spawn_cmd("roof", Some("base")), spawn_cmd("base", None)];
        assert_eq!(validate_spawn_batch(&backwards, &registry).len(), 1);
    }

    #[test]
    fn a_batch_rejects_duplicate_names() {
        let registry = registry_with(&["tower"]);
        let modify = vec![modify_cmd("tower"), modify_cmd("tower")];
        assert_eq!(validate_modify_batch(&modify, &registry).len(), 1);
        let spawn = vec![spawn_cmd("a", None), spawn_cmd("a", None)];
        assert_eq!(validate_spawn_batch(&spawn, &registry).len(), 1);
    }

    #[test]
    fn a_stale_revision_rejects_the_batch() {
        assert!(check_batch(4, Some(4), Vec::new()).is_ok());
        assert!(check_batch(4, None, Vec::new()).is_ok());
        let err = check_batch(5, Some(4), Vec::new()).unwrap_err();
        assert!(err.contains("revision 4"), "{err}");
    }

    #[test]
    fn the_revision_advances_on_every_edit_undo_redo_and_reset() {
        let mut stack = UndoStack::default();
        let entity = wt::WorldEntity::new(1, "cube");
        stack.record(
            wt::EditOp::spawn(entity.clone()),
            wt::EditOp::delete(entity.id),
            None,
        );
        assert_eq!(stack.revision(), 1);
        assert!(stack.undo().is_some());
        assert_eq!(stack.revision(), 2);
        assert!(stack.undo().is_none());
        assert_eq!(stack.revision(), 2, "a no-op undo is not an edit");
        assert!(stack.redo().is_some());
        assert_eq!(stack.revision(), 3);
        stack.reset(wt::EditHistory::new());
        assert_eq!(stack.revision(), 4, "a reset never rewinds the revision");
    }

    /// A child spawned in the same frame as its parent keeps its offset from
    /// the parent, as the spawn tools and the world loader attach it.
    #[test]
    fn a_child_spawned_with_its_parent_keeps_its_offset() {
        let mut app = App::new();
        app.add_plugins(bevy::transform::TransformPlugin);
        let parent = app
            .world_mut()
            .spawn(Transform::from_xyz(5.0, 0.0, 0.0))
            .id();
        let child = app
            .world_mut()
            .spawn(Transform::from_xyz(1.0, 2.0, 0.0).with_rotation(Quat::from_rotation_z(1.0)))
            .id();
        let mut queue = CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, app.world());
            attach_to_parent(&mut commands, child, parent);
        }
        queue.apply(app.world_mut());
        app.update();

        let local = app.world().get::<Transform>(child).unwrap();
        assert_eq!(local.translation, Vec3::new(1.0, 2.0, 0.0));
        assert!(local.rotation.abs_diff_eq(Quat::from_rotation_z(1.0), 1e-6));
        let global = app.world().get::<GlobalTransform>(child).unwrap();
        assert!(
            global
                .translation()
                .abs_diff_eq(Vec3::new(6.0, 2.0, 0.0), 1e-5)
        );
    }
}
