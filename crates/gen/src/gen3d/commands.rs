//! GenCommand / GenResponse protocol between agent and Bevy.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Import from crate root for P1/P2/P3/P4/P5 types
use crate::character;
use crate::interaction;
use crate::physics;
use crate::terrain;
use crate::ui;
use crate::worldgen;

// ---------------------------------------------------------------------------
// Commands (agent → Bevy)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum GenCommand {
    // Tier 1: Perceive
    SceneInfo,
    Screenshot {
        width: u32,
        height: u32,
        wait_frames: u32,
        /// Entity name to highlight with emissive override.
        highlight_entity: Option<String>,
        /// Highlight color as [r, g, b, a] (default red).
        highlight_color: Option<[f32; 4]>,
        /// Camera angle preset for the screenshot.
        camera_angle: Option<ScreenshotCameraAngle>,
        /// Overlay entity names and bounding boxes.
        include_annotations: bool,
    },
    EntityInfo {
        name: String,
    },

    // Tier 2: Mutate
    SpawnPrimitive(SpawnPrimitiveCmd),
    ModifyEntity(ModifyEntityCmd),
    DeleteEntity {
        name: String,
    },

    // Tier 2b: Batch mutations
    // A batch applies every item or none. `expected_revision`, when set,
    // rejects the batch if the scene revision has moved on.
    SpawnBatch {
        entities: Vec<SpawnPrimitiveCmd>,
        expected_revision: Option<u64>,
    },
    ModifyBatch {
        entities: Vec<ModifyEntityCmd>,
        expected_revision: Option<u64>,
    },
    DeleteBatch {
        names: Vec<String>,
        expected_revision: Option<u64>,
    },
    SetCamera(CameraCmd),
    SetLight(SetLightCmd),
    SetEnvironment(EnvironmentCmd),

    // Tier 3: Advanced
    SpawnMesh(RawMeshCmd),

    // Tier 4: Export
    ExportScreenshot {
        path: String,
        width: u32,
        height: u32,
    },
    ExportGltf {
        path: Option<String>,
    },

    // Tier 3b: Import
    LoadGltf {
        path: String,
        /// Decompose mesh into individually editable sub-objects (WG6.3).
        segment: bool,
    },

    // Tier 5: Audio
    SetAmbience(AmbienceCmd),
    SpawnAudioEmitter(AudioEmitterCmd),
    ModifyAudioEmitter(ModifyAudioEmitterCmd),
    RemoveAudioEmitter {
        name: String,
    },
    AudioInfo,

    // Tier 6: Behaviors
    AddBehavior(AddBehaviorCmd),
    RemoveBehavior {
        entity: String,
        behavior_id: Option<String>,
    },
    ListBehaviors {
        entity: Option<String>,
    },
    SetBehaviorsPaused {
        paused: bool,
    },

    // Tier 7: World skills
    SaveWorld(SaveWorldCmd),
    LoadWorld {
        path: String,
        /// Clear existing scene before loading (default: true).
        clear: bool,
    },
    ExportWorld {
        /// Export format: "glb" (binary) or "gltf" (JSON + BIN)
        format: Option<String>,
    },
    ExportHtml,
    ForkWorld {
        /// Source world path or skill name.
        source: String,
        /// New world name.
        new_name: String,
    },
    // Tier 8: Scene management
    ClearScene {
        keep_camera: bool,
        keep_lights: bool,
    },

    // Tier 9: Undo/Redo
    Undo,
    Redo,
    UndoInfo,

    // Tier 10: Avatar & Character System (P1)
    SpawnPlayer(character::SpawnPlayerParams),
    SetSpawnPoint(character::SpawnPointParams),
    SpawnNpc(character::SpawnNpcParams),
    SetNpcDialogue(character::SetDialogueParams),
    SetPlayerCameraMode(character::SetCameraModeParams),

    // Tier 11: Interaction & Trigger System (P2)
    AddTrigger(interaction::AddTriggerParams),
    AddTeleporter(interaction::TeleporterParams),
    AddCollectible(interaction::CollectibleParams),
    AddDoor(interaction::DoorParams),
    LinkEntities(interaction::LinkEntitiesParams),

    // Tier 12: Terrain & Landscape (P3)
    AddTerrain(terrain::TerrainParams),
    AddWater(terrain::WaterParams),
    AddPath(terrain::PathParams),
    AddFoliage(terrain::FoliageParams),
    SetSky(terrain::SkyParams),
    QueryTerrainHeight {
        /// Points to query: [[x, z], ...]
        points: Vec<[f32; 2]>,
    },

    // Tier 13: In-World Text & UI (P4)
    AddSign(ui::SignParams),
    AddHud(ui::HudParams),
    AddLabel(ui::LabelParams),
    AddTooltip(ui::TooltipParams),
    AddNotification(ui::NotificationParams),

    // Tier 14: Physics Integration (P5)
    SetPhysics(physics::PhysicsParams),
    AddCollider(physics::ColliderParams),
    AddJoint(physics::JointParams),
    AddForce(physics::ForceParams),
    SetGravity(physics::GravityParams),

    // Tier 15: WorldGen Pipeline (WG1)
    PlanLayout {
        prompt: String,
        size: [f32; 2],
        seed: Option<u32>,
    },
    ApplyBlockout {
        spec: worldgen::BlockoutSpec,
        show_debug_volumes: bool,
        generate_terrain: bool,
        generate_paths: bool,
    },
    PopulateRegion {
        region_id: String,
        style_hint: Option<String>,
        replace_existing: bool,
    },

    // Tier 16: Hierarchical Placement (WG3) & Scene Decomposition (WG6)
    SetTier {
        entity_name: String,
        tier: worldgen::PlacementTier,
    },
    SetRole {
        entity_name: String,
        role: worldgen::SemanticRole,
    },
    BulkModify {
        role: worldgen::SemanticRole,
        region_id: Option<String>,
        action: BulkAction,
    },

    // Tier 17: Blockout Editing (WG5)
    ModifyBlockout {
        action: BlockoutEditAction,
        auto_regenerate: bool,
    },

    // Tier 18: Navmesh Infrastructure (WG2)
    BuildNavMesh {
        settings: worldgen::NavMeshSettings,
    },
    ValidateNavigability {
        from: Option<[f32; 3]>,
        to: Option<[f32; 3]>,
        check_all_regions: bool,
    },

    // Tier 19: Navmesh Editing (WG5.2)
    EditNavMesh {
        action: NavMeshEditAction,
    },

    // Tier 20: Incremental Regeneration (WG5.3)
    Regenerate {
        region_ids: Option<Vec<String>>,
        preview_only: bool,
        preserve_manual: bool,
    },

    // Tier 21: Depth Map Rendering (WG7.1)
    RenderDepth {
        config: worldgen::DepthRenderConfig,
        output_path: Option<String>,
    },

    // Tier 22: Styled Preview (WG7.2) runs in the gen_preview_world tool
    // itself (ComfyUI over HTTP), using RenderDepth for the depth map.

    // Tier 23: AI Asset Generation (AI1)
    GenerateAsset {
        prompt: String,
        name: String,
        reference_image: Option<String>,
        position: [f32; 3],
        scale: f32,
        model: crate::gen3d::asset_gen::GenerationModel,
        quality: crate::gen3d::asset_gen::GenerationQuality,
        pbr: bool,
    },
    GenerateTexture {
        entity: String,
        prompt: String,
        style: crate::gen3d::asset_gen::TextureStyle,
        resolution: u32,
    },
    GenerationStatus {
        task_id: Option<String>,
        action: String, // "status", "cancel", "list"
    },

    // Tier 24: AI NPC Intelligence (AI2)
    SetNpcBrain {
        entity: String,
        config: character::npc_brain::NpcBrainConfig,
    },
    /// What an entity can perceive: named entities inside its view cone
    /// (`fov` degrees around its forward) and perception radius.
    NpcObserve {
        entity: String,
        fov: f32,
    },
    SetNpcMemory {
        entity: String,
        capacity: usize,
        initial_memories: Vec<String>,
        auto_memorize: bool,
    },

    // Tier 25: Multi-file WorldGen
    WriteWorldPlan {
        name: String,
        description: Option<String>,
        generation_strategy: String,
        regions: Vec<serde_json::Value>,
        constraints: Option<serde_json::Value>,
        environment: Option<serde_json::Value>,
        camera: Option<serde_json::Value>,
    },
    WriteRegion {
        region_id: String,
        md_content: String,
        ron_content: String,
        flush: bool,
    },
    LoadRegion {
        path: String,
    },
    UnloadRegion {
        region_id: String,
    },
    PersistBlockout,
    WriteBehaviors {
        name: String,
        md_content: String,
        ron_content: String,
    },
    WriteAudio {
        name: String,
        md_content: String,
        ron_content: String,
    },

    // Tier 26: Sync & Drift Detection
    CheckDrift {
        domain: Option<String>,
        detail_level: String,
    },
    Sync {
        domain: String,
        source: String,
        preview: bool,
        resolve_conflicts: Option<HashMap<String, String>>,
    },

    // Tier 27: Multimodal Input (AI3)
    ImageToLayout {
        /// Path to reference image or base64-encoded image data.
        image: String,
        /// Additional text guidance to supplement the image.
        prompt: Option<String>,
        /// Target world scale: small, medium, large.
        scale: ImageLayoutScale,
        /// Style mode: match, blockout, stylized.
        style: ImageLayoutStyle,
    },
    MatchStyle {
        /// Path to style reference image or base64 data.
        image: String,
        /// Which aspects to adjust.
        scope: StyleMatchScope,
        /// How strongly to match (0.0–1.0).
        intensity: f32,
        /// Specific entity IDs to restyle (None = all).
        entities: Option<Vec<String>>,
    },
    ReferenceBoard {
        action: ReferenceBoardAction,
    },
    PanoramaToWorld {
        /// Path to equirectangular panorama image or base64 data.
        image: String,
        /// Additional guidance: layout style, density, biome.
        prompt: Option<String>,
        /// Also plan sparse regions in directions where the panorama shows
        /// open horizon, so the world continues past what it shows.
        generate_beyond: bool,
    },
}

/// Actions for editing the navmesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum NavMeshEditAction {
    /// Mark an area as non-walkable.
    BlockArea { position: [f32; 3], radius: f32 },
    /// Force an area to be walkable.
    AllowArea { position: [f32; 3], radius: f32 },
    /// Create a navigable connection between two points.
    AddConnection {
        from: [f32; 3],
        to: [f32; 3],
        bidirectional: bool,
    },
    /// Remove a connection near a point.
    RemoveConnection { from: [f32; 3] },
}

// ---------------------------------------------------------------------------
// AI3: Multimodal input data structures
// ---------------------------------------------------------------------------

/// Target world scale for image-to-layout conversion.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageLayoutScale {
    /// Single room (~10m²).
    Small,
    /// Building/courtyard (~100m²).
    Medium,
    /// Landscape (~1000m²).
    Large,
}

/// Style interpretation mode for image-to-layout.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageLayoutStyle {
    /// Reproduce the image's style as closely as possible.
    Match,
    /// Gray-box blockout only, ignore visual style.
    Blockout,
    /// Freely interpret the image's mood and structure.
    Stylized,
}

/// Which aspects of the scene to adjust during style matching.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleMatchScope {
    /// Adjust lighting, materials, and atmosphere.
    All,
    /// Adjust lighting only.
    Lighting,
    /// Adjust materials only.
    Materials,
    /// Adjust atmosphere (fog, sky) only.
    Atmosphere,
}

/// Actions for the reference board (mood board) management.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ReferenceBoardAction {
    /// Add a new reference image.
    Add {
        image: String,
        label: Option<String>,
        weight: f32,
    },
    /// Remove a reference by ID.
    Remove { ref_id: String },
    /// List all active references.
    List,
    /// Clear all references.
    Clear,
}

/// A single reference image entry stored in the reference board.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceEntry {
    pub ref_id: String,
    pub label: String,
    pub weight: f32,
    /// VLM-generated text description (cached at add time).
    pub description: String,
    /// Path to the stored (resized) image file.
    pub image_path: String,
}

/// Actions for editing the blockout layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BlockoutEditAction {
    /// Add a new region to the blockout.
    AddRegion {
        region: worldgen::blockout::RegionDef,
    },
    /// Remove a region and all its generated entities.
    RemoveRegion { region_id: String },
    /// Resize a region's bounds.
    ResizeRegion {
        region_id: String,
        center: [f32; 2],
        size: [f32; 2],
    },
    /// Move a region by changing its center.
    MoveRegion {
        region_id: String,
        new_center: [f32; 2],
    },
    /// Change a region's decorative density.
    SetDensity { region_id: String, density: f32 },
}

/// Actions for bulk modification of entities by semantic role.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BulkAction {
    /// Scale all matching entities by a factor.
    Scale { factor: f32 },
    /// Recolor all matching entities.
    Recolor { color: [f32; 4] },
    /// Remove all matching entities.
    Remove,
    /// Hide all matching entities.
    Hide,
    /// Show all matching entities.
    Show,
}

/// Camera angle presets for screenshots.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotCameraAngle {
    /// Use the current camera position and orientation.
    Current,
    /// Position camera directly above the scene center, looking down.
    TopDown,
    /// Position camera at ~45 degree angle from above (isometric perspective).
    Isometric,
    /// Position camera at ground level facing the scene from the north.
    Front,
    /// Frame the highlighted entity with surrounding context visible.
    EntityFocus,
}

// ---------------------------------------------------------------------------
// Command data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnPrimitiveCmd {
    pub name: String,
    pub shape: PrimitiveShape,
    #[serde(default)]
    pub dimensions: HashMap<String, f32>,
    #[serde(default = "default_position")]
    pub position: [f32; 3],
    #[serde(default)]
    pub rotation_degrees: [f32; 3],
    #[serde(default = "default_scale")]
    pub scale: [f32; 3],
    #[serde(default = "default_color")]
    pub color: [f32; 4],
    #[serde(default)]
    pub metallic: f32,
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    #[serde(default)]
    pub emissive: [f32; 4],
    pub alpha_mode: Option<String>,
    pub unlit: Option<bool>,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PrimitiveShape {
    Cuboid,
    Sphere,
    Cylinder,
    Cone,
    Capsule,
    Torus,
    Plane,
    Pyramid,
    Tetrahedron,
    Icosahedron,
    Wedge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifyEntityCmd {
    pub name: String,
    pub position: Option<[f32; 3]>,
    pub rotation_degrees: Option<[f32; 3]>,
    pub scale: Option<[f32; 3]>,
    pub color: Option<[f32; 4]>,
    pub metallic: Option<f32>,
    pub roughness: Option<f32>,
    pub emissive: Option<[f32; 4]>,
    pub alpha_mode: Option<String>,
    pub unlit: Option<bool>,
    pub double_sided: Option<bool>,
    pub reflectance: Option<f32>,
    pub visible: Option<bool>,
    pub parent: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraCmd {
    #[serde(default = "default_camera_pos")]
    pub position: [f32; 3],
    #[serde(default)]
    pub look_at: [f32; 3],
    #[serde(default = "default_fov")]
    pub fov_degrees: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetLightCmd {
    pub name: String,
    #[serde(default = "default_light_type")]
    pub light_type: LightType,
    #[serde(default = "default_white")]
    pub color: [f32; 4],
    #[serde(default = "default_intensity")]
    pub intensity: f32,
    pub position: Option<[f32; 3]>,
    pub direction: Option<[f32; 3]>,
    #[serde(default = "default_true")]
    pub shadows: bool,
    /// Maximum range for point/spot lights (world units). None = engine default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<f32>,
    /// Outer cone angle in radians (spot lights only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outer_angle: Option<f32>,
    /// Inner cone angle in radians (spot lights only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inner_angle: Option<f32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LightType {
    Directional,
    Point,
    Spot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentCmd {
    pub background_color: Option<[f32; 4]>,
    pub ambient_light: Option<f32>,
    pub ambient_color: Option<[f32; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMeshCmd {
    pub name: String,
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub uvs: Option<Vec<[f32; 2]>>,
    #[serde(default = "default_color")]
    pub color: [f32; 4],
    #[serde(default)]
    pub metallic: f32,
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    #[serde(default)]
    pub position: [f32; 3],
    #[serde(default)]
    pub rotation_degrees: [f32; 3],
    #[serde(default = "default_scale")]
    pub scale: [f32; 3],
    pub parent: Option<String>,
    #[serde(default)]
    pub emissive: [f32; 4],
    pub alpha_mode: Option<String>,
    pub unlit: Option<bool>,
    pub double_sided: Option<bool>,
    pub reflectance: Option<f32>,
}

// ---------------------------------------------------------------------------
// Audio command data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbienceCmd {
    pub layers: Vec<AmbienceLayerDef>,
    pub master_volume: Option<f32>,
    pub reverb: Option<ReverbParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbienceLayerDef {
    pub name: String,
    pub sound: AmbientSound,
    pub volume: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AmbientSound {
    Wind { speed: f32, gustiness: f32 },
    Rain { intensity: f32 },
    Forest { bird_density: f32, wind: f32 },
    Ocean { wave_size: f32 },
    Cave { drip_rate: f32, resonance: f32 },
    Stream { flow_rate: f32 },
    Silence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioEmitterCmd {
    pub name: String,
    pub entity: Option<String>,
    pub position: Option<[f32; 3]>,
    pub sound: EmitterSound,
    pub radius: f32,
    pub volume: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EmitterSound {
    Water {
        turbulence: f32,
    },
    Fire {
        intensity: f32,
        crackle: f32,
    },
    Hum {
        frequency: f32,
        warmth: f32,
    },
    Wind {
        pitch: f32,
    },
    Custom {
        waveform: WaveformType,
        filter_cutoff: f32,
        filter_type: FilterType,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveformType {
    Sine,
    Saw,
    Square,
    WhiteNoise,
    PinkNoise,
    BrownNoise,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterType {
    Lowpass,
    Highpass,
    Bandpass,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverbParams {
    pub room_size: f32,
    pub damping: f32,
    pub wet: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModifyAudioEmitterCmd {
    pub name: String,
    pub volume: Option<f32>,
    pub radius: Option<f32>,
    pub sound: Option<EmitterSound>,
}

// ---------------------------------------------------------------------------
// Behavior command data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddBehaviorCmd {
    pub entity: String,
    #[serde(default)]
    pub behavior_id: Option<String>,
    pub behavior: BehaviorDef,
}

/// Declarative behavior definition — data, not code.
/// Each variant fully describes a continuous animation that the tick system evaluates.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BehaviorDef {
    /// Orbit around a center entity or point.
    Orbit {
        /// Name of entity to orbit around (mutually exclusive with `center_point`).
        #[serde(default)]
        center: Option<String>,
        /// Fixed point to orbit around [x,y,z] (used if `center` is None).
        #[serde(default)]
        center_point: Option<[f32; 3]>,
        /// Orbit radius.
        #[serde(default = "default_orbit_radius")]
        radius: f32,
        /// Orbital speed in degrees per second.
        #[serde(default = "default_orbit_speed")]
        speed: f32,
        /// Orbit axis (normalized). Default: Y-up.
        #[serde(default = "default_y_axis")]
        axis: [f32; 3],
        /// Initial phase angle in degrees.
        #[serde(default)]
        phase: f32,
        /// Orbit tilt in degrees (inclination from the axis plane).
        #[serde(default)]
        tilt: f32,
    },
    /// Spin (rotate) around a local axis.
    Spin {
        /// Local axis to spin around.
        #[serde(default = "default_y_axis")]
        axis: [f32; 3],
        /// Rotation speed in degrees per second.
        #[serde(default = "default_spin_speed")]
        speed: f32,
    },
    /// Bob up and down (sinusoidal oscillation along an axis).
    Bob {
        /// Axis of oscillation.
        #[serde(default = "default_y_axis")]
        axis: [f32; 3],
        /// Amplitude (distance from center in each direction).
        #[serde(default = "default_bob_amplitude")]
        amplitude: f32,
        /// Oscillation frequency in Hz.
        #[serde(default = "default_bob_frequency")]
        frequency: f32,
        /// Phase offset in degrees.
        #[serde(default)]
        phase: f32,
    },
    /// Continuously look at / follow another entity.
    LookAt {
        /// Name of entity to look at.
        target: String,
    },
    /// Scale pulsation (breathing effect).
    Pulse {
        /// Minimum scale multiplier.
        #[serde(default = "default_pulse_min")]
        min_scale: f32,
        /// Maximum scale multiplier.
        #[serde(default = "default_pulse_max")]
        max_scale: f32,
        /// Pulse frequency in Hz.
        #[serde(default = "default_bob_frequency")]
        frequency: f32,
    },
    /// Follow a path of waypoints in sequence.
    PathFollow {
        /// Ordered waypoints [[x,y,z], ...].
        waypoints: Vec<[f32; 3]>,
        /// Movement speed in units per second.
        #[serde(default = "default_path_speed")]
        speed: f32,
        /// Loop mode: "loop" wraps back to start, "ping_pong" reverses.
        #[serde(default = "default_path_mode")]
        mode: PathMode,
        /// Smoothly interpolate rotation toward movement direction.
        #[serde(default)]
        orient_to_path: bool,
    },
    /// Bouncing on a surface with gravity.
    Bounce {
        /// Height of initial/max bounce.
        #[serde(default = "default_bounce_height")]
        height: f32,
        /// Gravity acceleration (units/s^2).
        #[serde(default = "default_bounce_gravity")]
        gravity: f32,
        /// Energy retained per bounce (0.0-1.0).
        #[serde(default = "default_bounce_damping")]
        damping: f32,
        /// Surface Y level to bounce on.
        #[serde(default)]
        surface_y: f32,
    },
}

fn default_orbit_radius() -> f32 {
    5.0
}
fn default_orbit_speed() -> f32 {
    36.0
}
fn default_y_axis() -> [f32; 3] {
    [0.0, 1.0, 0.0]
}
fn default_spin_speed() -> f32 {
    90.0
}
fn default_bob_amplitude() -> f32 {
    0.5
}
fn default_bob_frequency() -> f32 {
    0.5
}
fn default_pulse_min() -> f32 {
    0.9
}
fn default_pulse_max() -> f32 {
    1.1
}
fn default_path_speed() -> f32 {
    2.0
}
fn default_path_mode() -> PathMode {
    PathMode::Loop
}
fn default_bounce_height() -> f32 {
    3.0
}
fn default_bounce_gravity() -> f32 {
    9.8
}
fn default_bounce_damping() -> f32 {
    0.7
}

/// Path follow loop mode.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PathMode {
    Loop,
    PingPong,
    Once,
}

// ---------------------------------------------------------------------------
// World save/load command data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveWorldCmd {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

// ---------------------------------------------------------------------------
// Avatar & tour data structures
// ---------------------------------------------------------------------------

/// Point of view mode for the avatar / camera.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PointOfView {
    /// Camera placed at avatar eye level; avatar model is not visible.
    FirstPerson,
    /// Camera orbits behind/above the avatar; avatar model is visible.
    #[default]
    ThirdPerson,
}

/// Avatar configuration describing the user/explorer presence in a world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarDef {
    /// Where the avatar spawns in the world.
    #[serde(default = "default_avatar_spawn")]
    pub spawn_position: [f32; 3],
    /// Initial look direction.
    #[serde(default = "default_avatar_look_at")]
    pub spawn_look_at: [f32; 3],
    /// Camera point-of-view mode.
    #[serde(default)]
    pub pov: PointOfView,
    /// Movement speed in units per second.
    #[serde(default = "default_avatar_speed")]
    pub movement_speed: f32,
    /// Avatar eye-height above ground (used for first-person eye level).
    #[serde(default = "default_avatar_height")]
    pub height: f32,
    /// Entity name of the 3D model representing the avatar (3rd-person).
    /// When `None`, the world has no visible avatar model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_entity: Option<String>,
}

impl Default for AvatarDef {
    fn default() -> Self {
        Self {
            spawn_position: default_avatar_spawn(),
            spawn_look_at: default_avatar_look_at(),
            pov: PointOfView::default(),
            movement_speed: default_avatar_speed(),
            height: default_avatar_height(),
            model_entity: None,
        }
    }
}

fn default_avatar_spawn() -> [f32; 3] {
    [0.0, 0.0, 5.0]
}
fn default_avatar_look_at() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}
fn default_avatar_speed() -> f32 {
    5.0
}
fn default_avatar_height() -> f32 {
    1.8
}
fn default_tour_speed() -> f32 {
    3.0
}

/// How the camera/avatar moves between tour waypoints.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TourMode {
    /// Ground-level movement respecting gravity / terrain.
    #[default]
    Walk,
    /// Free-flying camera interpolation.
    Fly,
    /// Instant teleport between waypoints (cut, no interpolation).
    Teleport,
}

/// A single stop along a guided tour.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TourWaypoint {
    /// Camera / avatar position at this stop.
    pub position: [f32; 3],
    /// Where the camera looks at this stop.
    pub look_at: [f32; 3],
    /// Narrative text shown to the user at this stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// How long to pause at this waypoint (seconds) before moving on.
    #[serde(default)]
    pub pause_duration: f32,
}

/// A guided tour — a named, ordered sequence of waypoints through the world.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TourDef {
    /// Human-readable tour name (e.g. "grand_tour", "scenic_overlook").
    pub name: String,
    /// Brief description of the tour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Ordered stops along the tour.
    pub waypoints: Vec<TourWaypoint>,
    /// Movement speed between waypoints (units/sec).
    #[serde(default = "default_tour_speed")]
    pub speed: f32,
    /// Movement mode between waypoints.
    #[serde(default)]
    pub mode: TourMode,
    /// If true, this tour starts automatically when the world is loaded.
    #[serde(default)]
    pub autostart: bool,
    /// If true, the tour loops back to the first waypoint after the last.
    #[serde(default)]
    pub loop_tour: bool,
    /// Optional PoV override for this tour (falls back to avatar default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pov: Option<PointOfView>,
}

// ---------------------------------------------------------------------------
// Responses (Bevy → agent)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum GenResponse {
    SceneInfo(SceneInfoData),
    Screenshot {
        image_path: String,
    },
    EntityInfo(Box<EntityInfoData>),
    Spawned {
        name: String,
        entity_id: u64,
    },
    Modified {
        name: String,
    },
    Deleted {
        name: String,
    },

    // Batch results
    BatchResult {
        results: Vec<String>,
        /// Scene revision after the batch was applied.
        revision: u64,
    },
    CameraSet,
    LightSet {
        name: String,
    },
    EnvironmentSet,
    Exported {
        path: String,
    },
    GltfLoaded {
        name: String,
        path: String,
    },

    // Audio responses
    AmbienceSet,
    AudioEmitterSpawned {
        name: String,
    },
    AudioEmitterModified {
        name: String,
    },
    AudioEmitterRemoved {
        name: String,
    },
    AudioInfoData(AudioInfoResponse),

    // Behavior responses
    BehaviorAdded {
        entity: String,
        behavior_id: String,
    },
    BehaviorRemoved {
        entity: String,
        count: usize,
    },
    BehaviorList(BehaviorListResponse),
    BehaviorsPaused {
        paused: bool,
    },

    // World responses
    WorldSaved {
        path: String,
        skill_name: String,
        warnings: Vec<String>,
    },
    WorldLoaded {
        path: String,
        entities: usize,
        behaviors: usize,
    },

    // Scene management
    SceneCleared {
        removed_count: usize,
    },

    // Undo/Redo
    Undone {
        description: String,
    },
    Redone {
        description: String,
    },
    NothingToUndo,
    NothingToRedo,
    UndoInfoResult {
        undo_count: usize,
        redo_count: usize,
        entity_count: usize,
        dirty_count: usize,
    },

    // Terrain query
    TerrainHeights {
        /// Results: [[x, y, z], ...] with y = sampled height
        heights: Vec<[f32; 3]>,
    },

    // WorldGen responses
    BlockoutPlan {
        spec_json: String,
    },
    BlockoutApplied {
        entities_spawned: usize,
        regions: usize,
        paths: usize,
    },
    RegionPopulated {
        region_id: String,
        entities_spawned: usize,
    },

    // Tier/Role responses
    TierSet {
        entity: String,
        tier: String,
    },
    RoleSet {
        entity: String,
        role: String,
    },
    BulkModified {
        role: String,
        action: String,
        affected: usize,
    },

    // Blockout edit response
    BlockoutModified {
        action: String,
        region_id: String,
        entities_removed: usize,
        entities_spawned: usize,
    },

    // Navmesh responses
    NavMeshBuilt {
        walkable_coverage: f32,
        component_count: u32,
        cell_count: usize,
    },
    NavigabilityResult {
        result_json: String,
    },

    // Navmesh edit responses
    NavMeshEdited {
        action: String,
        description: String,
    },

    // Regeneration responses
    RegenerationPreview {
        preview_json: String,
    },
    Regenerated {
        regions_processed: usize,
        entities_removed: usize,
    },

    // Depth map responses
    DepthRendered {
        path: String,
        width: u32,
        height: u32,
        depth_range: [f32; 2],
    },

    // AI asset generation responses
    AssetGenerating {
        task_id: String,
        estimated_seconds: u32,
        message: String,
    },
    TextureGenerating {
        task_id: String,
        estimated_seconds: u32,
        message: String,
    },
    GenerationStatusResult {
        status_json: String,
    },

    // AI NPC responses
    NpcBrainSet {
        entity: String,
        model: String,
        tick_rate: f32,
    },
    NpcObservation {
        entity: String,
        position: [f32; 3],
        radius: f32,
        /// JSON array of `{name, distance, bearing, position}`, nearest first.
        perceived_json: String,
        /// The entity's brain model, if it has a brain.
        model: Option<String>,
    },
    NpcMemorySet {
        entity: String,
        capacity: usize,
        initial_count: usize,
    },

    // Multi-file WorldGen responses
    WorldPlanWritten {
        skill_md_path: String,
        world_md_path: String,
        world_ron_path: String,
    },
    RegionWritten {
        region_id: String,
        md_path: String,
        ron_path: String,
    },
    RegionLoaded {
        region_id: String,
        entity_count: u32,
    },
    RegionUnloaded {
        region_id: String,
        entities_removed: u32,
    },
    BlockoutPersisted {
        md_path: String,
        ron_path: String,
    },
    BehaviorsWritten {
        name: String,
        md_path: String,
        ron_path: String,
    },
    AudioWritten {
        name: String,
        md_path: String,
        ron_path: String,
    },

    // Sync & Drift responses
    DriftReport(localgpt_world_types::DriftReport),
    SyncPreview {
        domain: String,
        changes: String,
    },
    SyncApplied {
        domain: String,
        files_updated: Vec<String>,
    },

    // AI3: Multimodal input responses
    ImageLayoutAnalysis {
        /// BlockoutSpec-compatible JSON plan.
        plan_json: String,
        /// Image analysis metadata (detected elements, palette, etc.).
        analysis_json: String,
    },
    StyleMatched {
        /// JSON summary of changes applied.
        changes_json: String,
    },
    ReferenceBoardResult {
        /// JSON response (varies by action: add/remove/list/clear).
        result_json: String,
    },
    /// A blockout plan read from a panorama (not yet applied to the scene).
    PanoramaWorldGenerated {
        world_name: String,
        regions_planned: usize,
        spawn_point: [f32; 3],
        /// What was read from the image (sky, ground, skyline per bearing).
        analysis_json: String,
        notes: String,
    },

    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneInfoData {
    /// Scene revision; pass it as `expected_revision` to batch tools.
    pub revision: u64,
    pub entity_count: usize,
    pub entities: Vec<EntitySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySummary {
    pub name: String,
    pub entity_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    pub position: [f32; 3],
    pub scale: [f32; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub behaviors: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityInfoData {
    pub name: String,
    pub entity_id: u64,
    pub entity_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    pub position: [f32; 3],
    pub rotation_degrees: [f32; 3],
    pub scale: [f32; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emissive: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub double_sided: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reflectance: Option<f32>,
    pub visible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<LightInfoData>,
    pub children: Vec<String>,
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mesh_asset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub behaviors: Vec<BehaviorSummary>,
}

/// Light component info returned by `gen_entity_info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightInfoData {
    pub light_type: String,
    pub color: [f32; 4],
    pub intensity: f32,
    pub shadows: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outer_angle: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inner_angle: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioInfoResponse {
    pub active: bool,
    pub ambience_layers: Vec<String>,
    pub emitters: Vec<AudioEmitterSummary>,
    pub master_volume: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioEmitterSummary {
    pub name: String,
    pub sound_type: String,
    pub volume: f32,
    pub radius: f32,
    pub position: Option<[f32; 3]>,
    pub attached_to: Option<String>,
}

// ---------------------------------------------------------------------------
// Behavior response data structures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorListResponse {
    pub paused: bool,
    pub entities: Vec<EntityBehaviorsSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityBehaviorsSummary {
    pub entity: String,
    pub behaviors: Vec<BehaviorSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorSummary {
    pub id: String,
    pub behavior_type: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// Default helpers
// ---------------------------------------------------------------------------

fn default_position() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}
fn default_scale() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}
fn default_color() -> [f32; 4] {
    [0.8, 0.8, 0.8, 1.0]
}
fn default_roughness() -> f32 {
    0.5
}
fn default_camera_pos() -> [f32; 3] {
    [5.0, 5.0, 5.0]
}
fn default_fov() -> f32 {
    45.0
}
fn default_light_type() -> LightType {
    LightType::Directional
}
fn default_white() -> [f32; 4] {
    [1.0, 1.0, 1.0, 1.0]
}
fn default_intensity() -> f32 {
    1000.0
}
fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spawn_primitive_serde_defaults() {
        let json = r#"{"name": "Box1", "shape": "Cuboid"}"#;
        let cmd: SpawnPrimitiveCmd = serde_json::from_str(json).unwrap();
        assert_eq!(cmd.name, "Box1");
        assert_eq!(cmd.position, [0.0, 0.0, 0.0]);
        assert_eq!(cmd.scale, [1.0, 1.0, 1.0]);
        assert_eq!(cmd.color, [0.8, 0.8, 0.8, 1.0]);
        assert_eq!(cmd.roughness, 0.5);
        assert_eq!(cmd.metallic, 0.0);
    }

    #[test]
    fn test_camera_cmd_serde_defaults() {
        let json = r#"{}"#;
        let cmd: CameraCmd = serde_json::from_str(json).unwrap();
        assert_eq!(cmd.position, [5.0, 5.0, 5.0]);
        assert_eq!(cmd.look_at, [0.0, 0.0, 0.0]);
        assert_eq!(cmd.fov_degrees, 45.0);
    }

    #[test]
    fn test_set_light_cmd_serde_defaults() {
        let json = r#"{"name": "Sun"}"#;
        let cmd: SetLightCmd = serde_json::from_str(json).unwrap();
        assert!(matches!(cmd.light_type, LightType::Directional));
        assert_eq!(cmd.intensity, 1000.0);
        assert!(cmd.shadows);
    }

    #[test]
    fn test_behavior_def_orbit_serde() {
        let json = r#"{"type": "orbit", "radius": 3.0, "speed": 45.0}"#;
        let def: BehaviorDef = serde_json::from_str(json).unwrap();
        assert!(
            matches!(def, BehaviorDef::Orbit { radius, speed, .. } if radius == 3.0 && speed == 45.0)
        );
    }

    #[test]
    fn test_behavior_def_spin_serde() {
        let json = r#"{"type": "spin"}"#;
        let def: BehaviorDef = serde_json::from_str(json).unwrap();
        assert!(matches!(def, BehaviorDef::Spin { speed, .. } if speed == 90.0));
    }

    #[test]
    fn test_behavior_def_bob_serde() {
        let json = r#"{"type": "bob", "amplitude": 1.0}"#;
        let def: BehaviorDef = serde_json::from_str(json).unwrap();
        assert!(
            matches!(def, BehaviorDef::Bob { amplitude, frequency, .. } if amplitude == 1.0 && frequency == 0.5)
        );
    }

    #[test]
    fn test_behavior_def_path_follow_serde() {
        let json = r#"{"type": "path_follow", "waypoints": [[0,0,0],[1,0,0]]}"#;
        let def: BehaviorDef = serde_json::from_str(json).unwrap();
        match &def {
            BehaviorDef::PathFollow {
                waypoints,
                speed,
                mode,
                ..
            } => {
                assert_eq!(waypoints.len(), 2);
                assert_eq!(*speed, 2.0);
                assert_eq!(*mode, PathMode::Loop);
            }
            other => unreachable!("Expected PathFollow, got {:?}", other),
        }
    }

    #[test]
    fn test_ambient_sound_serde() {
        let json = r#"{"type": "wind", "speed": 5.0, "gustiness": 0.3}"#;
        let sound: AmbientSound = serde_json::from_str(json).unwrap();
        assert!(
            matches!(sound, AmbientSound::Wind { speed, gustiness } if speed == 5.0 && gustiness == 0.3)
        );
    }

    #[test]
    fn test_emitter_sound_fire_serde() {
        let json = r#"{"type": "fire", "intensity": 0.8, "crackle": 0.5}"#;
        let sound: EmitterSound = serde_json::from_str(json).unwrap();
        assert!(
            matches!(sound, EmitterSound::Fire { intensity, crackle } if intensity == 0.8 && crackle == 0.5)
        );
    }

    #[test]
    fn test_avatar_def_defaults() {
        let def = AvatarDef::default();
        assert_eq!(def.spawn_position, [0.0, 0.0, 5.0]);
        assert_eq!(def.pov, PointOfView::ThirdPerson);
        assert_eq!(def.movement_speed, 5.0);
        assert_eq!(def.height, 1.8);
        assert!(def.model_entity.is_none());
    }

    #[test]
    fn test_tour_def_serde() {
        let json = r#"{
            "name": "grand_tour",
            "waypoints": [
                {"position": [0,0,0], "look_at": [1,0,0], "pause_duration": 2.0}
            ],
            "mode": "fly",
            "autostart": true
        }"#;
        let tour: TourDef = serde_json::from_str(json).unwrap();
        assert_eq!(tour.name, "grand_tour");
        assert_eq!(tour.mode, TourMode::Fly);
        assert!(tour.autostart);
        assert_eq!(tour.speed, 3.0); // default
        assert_eq!(tour.waypoints.len(), 1);
        assert_eq!(tour.waypoints[0].pause_duration, 2.0);
    }

    #[test]
    fn test_modify_entity_cmd_serde() {
        let json = r#"{"name": "Box1", "position": [1.0, 2.0, 3.0], "visible": false}"#;
        let cmd: ModifyEntityCmd = serde_json::from_str(json).unwrap();
        assert_eq!(cmd.name, "Box1");
        assert_eq!(cmd.position, Some([1.0, 2.0, 3.0]));
        assert_eq!(cmd.visible, Some(false));
        assert!(cmd.color.is_none());
        assert!(cmd.scale.is_none());
    }
}
