//! The player: one `.world` package from the app bundle, spawned through
//! `localgpt-world-bevy` and sounded through `localgpt-world-audio`, the way
//! the desktop app's world is (`crates/doc/src/scene.rs` is the same spawn),
//! plus the two behaviors the song worlds use.

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use bevy::asset::LoadState;
use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::gltf::GltfAssetLabel;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::PipelineCache;
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::window::{AppLifecycle, MonitorSelection, WindowMode};
use bevy::world_serialization::{WorldAsset, WorldAssetRoot};
use localgpt_world_audio::Engine;
use localgpt_world_bevy as wb;
use localgpt_world_types as wt;

use crate::camera::{Orbit, OrbitPlugin};
use crate::hud::HudPlugin;

/// Where the bundler puts the package, under the app's `assets/`.
pub const WORLD: &str = "world";

/// The package the player opened.
#[derive(Resource)]
pub struct Package {
    pub manifest: wt::WorldManifest,
}

/// When the player started, and when each step of the first draw finished,
/// in seconds from then; what the app's life cycle last said.
#[derive(Resource)]
pub struct Timeline {
    pub launched: Instant,
    pub first_frame: Option<f32>,
    pub assets_loaded: Option<f32>,
    pub pipelines_ready: Option<f32>,
    pub lifecycle: Option<AppLifecycle>,
    pub suspensions: u32,
    quiet_since: Option<(f32, u32)>,
}

/// The sound: the engine, if a device opened, and how the session and the
/// song went. Kept on the main thread, where it was made.
pub struct Audio {
    pub engine: Option<Engine>,
    pub session: Result<(), String>,
    pub song: Result<f32, String>,
    pub paused: bool,
}

/// How the world is drawn: as the desktop draws it, or without bloom, MSAA
/// and shadows, to measure what those cost on the device.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Full,
    Lite,
}

/// The one camera.
#[derive(Component)]
pub struct WorldCamera;

/// A light's authored shadows, for going back to Full after Lite.
#[derive(Component)]
struct AuthoredShadows(bool);

/// The behaviors the player runs (spin and bob, the ones the song worlds
/// use) and the authored transform each frame starts from, as Gen does.
#[derive(Component)]
struct Behaviors {
    defs: Vec<wt::BehaviorDef>,
    base: Transform,
}

/// Every glTF scene the world places, to know when all have loaded.
#[derive(Resource, Default)]
struct Scenes(Vec<Handle<WorldAsset>>);

/// Pipelines still compiling, and all there are, counted in the render
/// world each frame: a world draws nothing of a mesh until its pipeline is
/// ready, which is what made the desktop's first launch slow.
#[derive(Resource, Clone, Default)]
pub struct Pipelines(Arc<[AtomicUsize; 2]>);

impl Pipelines {
    pub fn waiting(&self) -> usize {
        self.0[0].load(Ordering::Relaxed)
    }

    pub fn total(&self) -> usize {
        self.0[1].load(Ordering::Relaxed)
    }
}

pub fn run() {
    let launched = Instant::now();
    // Before any stream opens: the category decides the silent switch and
    // the locked screen.
    let session = crate::ios::start_playback_session();
    let assets = bundle_assets();
    let manifest = match load_manifest(&assets) {
        Ok(manifest) => manifest,
        Err(e) => {
            eprintln!("player: {e}");
            return;
        }
    };
    let audio = start_audio(&manifest, &assets, session);
    let unsupported = manifest
        .entities
        .iter()
        .flat_map(|e| &e.behaviors)
        .filter(|b| !runs(b))
        .count();
    eprintln!(
        "player: {} entities, {} behaviors it does not run, session {:?}, song {:?}",
        manifest.entities.len(),
        unsupported,
        audio.session,
        audio.song,
    );

    let _ = App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Player".into(),
                // Without it winit makes the UIWindow Bevy's default
                // 1280x720 points, in a corner of the screen.
                mode: WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
                prefers_home_indicator_hidden: true,
                prefers_status_bar_hidden: true,
                ..default()
            }),
            ..default()
        }))
        .add_plugins((
            wb::modulation::ModulationPlugin,
            wb::nodes::NodeOverridesPlugin,
            PipelineWatch,
            OrbitPlugin,
            HudPlugin,
        ))
        .insert_resource(Package { manifest })
        .insert_resource(Timeline {
            launched,
            first_frame: None,
            assets_loaded: None,
            pipelines_ready: None,
            lifecycle: None,
            suspensions: 0,
            quiet_since: None,
        })
        .insert_resource(Look::Full)
        .init_resource::<Scenes>()
        .insert_non_send(audio)
        .add_systems(Startup, spawn_world)
        .add_systems(
            Update,
            (
                behave,
                follow_song,
                lifecycle,
                first_draw,
                look.run_if(resource_changed::<Look>),
            ),
        )
        .run();
}

/// The app's `assets/`, beside the executable in an iOS bundle: Bevy's
/// asset root too.
fn bundle_assets() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("assets")))
        .unwrap_or_else(|| PathBuf::from("assets"))
}

/// The package's head: `manifest.json` is the fold of its log, written down,
/// which is all a player needs.
fn load_manifest(assets: &Path) -> Result<wt::WorldManifest, String> {
    let path = assets.join(WORLD).join("manifest.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn start_audio(manifest: &wt::WorldManifest, assets: &Path, session: Result<(), String>) -> Audio {
    let mut engine = Engine::new();
    let song = match (
        engine.as_mut(),
        manifest.soundtrack.as_ref().and_then(|s| s.path.as_ref()),
    ) {
        (None, _) => Err("no output device".to_string()),
        (Some(_), None) => Err("the world has no song".to_string()),
        (Some(engine), Some(path)) => {
            engine.play_soundtrack(&assets.join(WORLD).join("assets").join(path), 1.0)
        }
    };
    if let Some(engine) = engine.as_mut()
        && !manifest.ambience.is_empty()
    {
        let layers: Vec<_> = manifest
            .ambience
            .iter()
            .map(|layer| (layer.name.clone(), layer.source.clone(), layer.volume))
            .collect();
        engine.set_ambience(&layers);
    }
    Audio {
        engine,
        session,
        song,
        paused: false,
    }
}

fn spawn_world(
    mut commands: Commands,
    package: Res<Package>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    mut soundtrack: ResMut<wb::modulation::Soundtrack>,
    mut scenes: ResMut<Scenes>,
) {
    let manifest = &package.manifest;
    let env = manifest.environment.as_ref();
    commands.insert_resource(wb::clear_color(env));
    commands.insert_resource(wb::ambient_light(env));
    soundtrack.def = manifest.soundtrack.clone();

    // The world's own camera, as the place a finger turns around.
    let (from, at) = match &manifest.camera {
        Some(camera) => (
            Vec3::from_array(camera.position),
            Vec3::from_array(camera.look_at),
        ),
        None => {
            let eye = wb::camera_transform(manifest);
            (eye.translation, eye.translation + eye.forward() * 10.0)
        }
    };
    let orbit = Orbit::looking(from, at);
    // As the desktop's world panel draws (Document's camera is the same).
    let mut camera = commands.spawn((
        WorldCamera,
        Camera3d::default(),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        Msaa::Sample4,
        Projection::Perspective(wb::perspective(manifest.camera.as_ref())),
        orbit.transform(),
        orbit,
    ));
    if let Some(fog) = wb::distance_fog(env) {
        camera.insert(fog);
    }

    let mut spawned = HashMap::new();
    for def in &manifest.entities {
        let entity = spawn_entity(
            &mut commands,
            def,
            &mut meshes,
            &mut materials,
            &asset_server,
            &mut scenes.0,
        );
        spawned.insert(def.id, entity);
    }
    for def in &manifest.entities {
        let parent = def.parent.and_then(|id| spawned.get(&id));
        if let (Some(&parent), Some(&child)) = (parent, spawned.get(&def.id)) {
            commands.entity(child).insert(ChildOf(parent));
        }
    }
}

fn spawn_entity(
    commands: &mut Commands,
    def: &wt::WorldEntity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    scenes: &mut Vec<Handle<WorldAsset>>,
) -> Entity {
    let mut transform = wb::transform(&def.transform);
    if let Some(light) = &def.light {
        transform = wb::light_transform(light, transform);
    }
    let mut entity = commands.spawn((
        Name::new(def.name.0.clone()),
        transform,
        wb::visibility(&def.transform),
    ));
    if let Some(shape) = &def.shape {
        let material = def
            .material
            .as_ref()
            .map(wb::standard_material)
            .unwrap_or_default();
        entity.insert((
            Mesh3d(meshes.add(wb::shape_mesh(shape))),
            MeshMaterial3d(materials.add(material)),
        ));
    }
    // A placed glTF, relative to the package's `assets/`.
    if let Some(mesh) = &def.mesh_asset {
        let handle: Handle<WorldAsset> = asset_server
            .load(GltfAssetLabel::Scene(0).from_asset(format!("{WORLD}/assets/{}", mesh.path)));
        scenes.push(handle.clone());
        entity.insert(WorldAssetRoot(handle));
        if !mesh.node_overrides.is_empty() {
            entity.insert(wb::nodes::NodeOverrides(mesh.node_overrides.clone()));
        }
    }
    if let Some(light) = &def.light {
        wb::insert_light(&mut entity, light);
        entity.insert(AuthoredShadows(light.shadows));
    }
    if let Some(modulated) = wb::modulation::modulated(def) {
        entity.insert(modulated);
    }
    let defs: Vec<_> = def.behaviors.iter().filter(|b| runs(b)).cloned().collect();
    if !defs.is_empty() {
        entity.insert(Behaviors {
            defs,
            base: transform,
        });
    }
    entity.id()
}

/// The behaviors this player runs; the others stay as authored.
fn runs(behavior: &wt::BehaviorDef) -> bool {
    matches!(
        behavior,
        wt::BehaviorDef::Spin { .. } | wt::BehaviorDef::Bob { .. }
    )
}

/// Each frame from the authored transform, every behavior in order, at the
/// world clock: Gen's `behaviors::pose` for the two kinds.
fn behave(time: Res<Time>, mut entities: Query<(&Behaviors, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (behaviors, mut transform) in &mut entities {
        *transform = behaviors.base;
        for def in &behaviors.defs {
            match def {
                wt::BehaviorDef::Spin {
                    axis,
                    speed,
                    phase,
                    pivot,
                } => {
                    let axis = Vec3::from_array(*axis).normalize_or(Vec3::Y);
                    let angle = (speed * t + phase).to_radians();
                    let (q0, s0) = (transform.rotation, transform.scale);
                    transform.rotation = q0 * Quat::from_axis_angle(axis, angle);
                    // Keep the pivot where it stood (Gen's `hold_pivot`).
                    let pivot = Vec3::from_array(*pivot);
                    let before = q0 * (s0 * pivot);
                    let after = transform.rotation * (transform.scale * pivot);
                    transform.translation += before - after;
                }
                wt::BehaviorDef::Bob {
                    axis,
                    amplitude,
                    frequency,
                    phase,
                } => {
                    let axis = Vec3::from_array(*axis).normalize_or(Vec3::Y);
                    let offset = (TAU * frequency * t + phase.to_radians()).sin() * amplitude;
                    transform.translation += axis * offset;
                }
                _ => {}
            }
        }
    }
}

/// The song's position drives the modulations; the camera is the ears.
fn follow_song(
    mut audio: NonSendMut<Audio>,
    mut soundtrack: ResMut<wb::modulation::Soundtrack>,
    camera: Query<&GlobalTransform, With<WorldCamera>>,
) {
    let Some(engine) = audio.engine.as_mut() else {
        return;
    };
    soundtrack.position = engine.soundtrack_position();
    if let Ok(eye) = camera.single() {
        let (_, rotation, translation) = eye.to_scale_rotation_translation();
        engine.set_listener(translation.to_array(), rotation.to_array());
    }
}

fn lifecycle(mut messages: MessageReader<AppLifecycle>, mut timeline: ResMut<Timeline>) {
    for &state in messages.read() {
        eprintln!(
            "player: lifecycle {state:?} at {:.1}s",
            timeline.launched.elapsed().as_secs_f32()
        );
        if state == AppLifecycle::Suspended {
            timeline.suspensions += 1;
        }
        timeline.lifecycle = Some(state);
    }
}

/// The first draw's steps: the first frame, every glTF loaded, and every
/// pipeline compiled (no pipeline waiting for ten frames running).
fn first_draw(
    mut timeline: ResMut<Timeline>,
    scenes: Res<Scenes>,
    asset_server: Res<AssetServer>,
    pipelines: Res<Pipelines>,
) {
    let now = timeline.launched.elapsed().as_secs_f32();
    if timeline.first_frame.is_none() {
        timeline.first_frame = Some(now);
        eprintln!("player: first frame at {now:.2}s");
    }
    if timeline.assets_loaded.is_none() {
        let done = scenes.0.iter().all(|handle| {
            asset_server.is_loaded_with_dependencies(handle)
                || matches!(asset_server.load_state(handle), LoadState::Failed(_))
        });
        if done {
            timeline.assets_loaded = Some(now);
            eprintln!("player: {} glTF scenes loaded at {now:.2}s", scenes.0.len());
        }
        return;
    }
    if timeline.pipelines_ready.is_some() {
        return;
    }
    if pipelines.waiting() == 0 && pipelines.total() > 0 {
        let (since, frames) = timeline.quiet_since.unwrap_or((now, 0));
        if frames + 1 >= 10 {
            timeline.pipelines_ready = Some(since);
            eprintln!(
                "player: {} pipelines ready at {since:.2}s",
                pipelines.total()
            );
        } else {
            timeline.quiet_since = Some((since, frames + 1));
        }
    } else {
        timeline.quiet_since = None;
    }
}

fn look(
    look: Res<Look>,
    mut commands: Commands,
    cameras: Query<Entity, With<WorldCamera>>,
    mut lights: Query<(&mut DirectionalLight, &AuthoredShadows)>,
) {
    for camera in &cameras {
        match *look {
            Look::Full => {
                commands
                    .entity(camera)
                    .insert((Bloom::NATURAL, Msaa::Sample4));
            }
            Look::Lite => {
                commands.entity(camera).remove::<Bloom>().insert(Msaa::Off);
            }
        }
    }
    for (mut light, authored) in &mut lights {
        light.shadow_maps_enabled = authored.0 && *look == Look::Full;
    }
}

/// Counts the render world's pipelines into [`Pipelines`].
struct PipelineWatch;

impl Plugin for PipelineWatch {
    fn build(&self, app: &mut App) {
        app.init_resource::<Pipelines>();
    }

    // The render app exists once the renderer's own `finish` has run, which
    // comes first: DefaultPlugins were added before this.
    fn finish(&self, app: &mut App) {
        let shared = app.world().resource::<Pipelines>().clone();
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(shared)
                .add_systems(Render, count_pipelines.in_set(RenderSystems::Cleanup));
        }
    }
}

fn count_pipelines(cache: Res<PipelineCache>, shared: Res<Pipelines>) {
    shared.0[0].store(cache.waiting_pipelines().count(), Ordering::Relaxed);
    shared.0[1].store(cache.pipelines().count(), Ordering::Relaxed);
}
