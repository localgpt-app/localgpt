//! Bevy side: spawn the current `WorldManifest`, and respawn it whenever the
//! document changes.
//!
//! Every mapping from the manifest to Bevy (meshes, materials, lights,
//! environment, camera) comes from `localgpt-world-bevy`, the crate Gen and
//! Verse render with too, so a manifest looks the same in every LocalGPT app
//! and in the web viewer. Mesh assets (placed GLBs) load through Bevy's
//! gltf pipeline relative to the assets root pinned in `main`. Behaviors,
//! audio, NPCs, and modulations are not rendered yet.

use std::collections::HashMap;

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::gltf::GltfAssetLabel;
use bevy::pbr::DistanceFog;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use localgpt_world_bevy as wb;
use localgpt_world_types as wt;

use localgpt_md::doc::Doc;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera)
            .add_systems(Update, rebuild.run_if(resource_changed::<CurrentWorld>));
    }
}

/// The document on screen and the world compiled from it.
#[derive(Resource)]
pub struct CurrentWorld {
    pub doc: Doc,
    pub manifest: wt::WorldManifest,
}

/// The single camera; [`crate::tour`] flies it between waypoints.
#[derive(Component)]
pub struct TourCamera;

/// Everything spawned from the manifest, despawned on rebuild.
#[derive(Component)]
struct FromManifest;

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        TourCamera,
        Camera3d::default(),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        Transform::from_xyz(0.0, 4.0, 14.0).looking_at(Vec3::new(0.0, 2.0, 0.0), Vec3::Y),
    ));
}

fn rebuild(
    mut commands: Commands,
    world: Res<CurrentWorld>,
    old: Query<Entity, (With<FromManifest>, Without<ChildOf>)>,
    camera: Query<Entity, With<TourCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    // Despawning a root takes its children with it.
    for entity in &old {
        commands.entity(entity).despawn();
    }
    let manifest = &world.manifest;
    if let Ok(camera) = camera.single() {
        apply_camera(&mut commands, camera, manifest);
    }
    let env = manifest.environment.as_ref();
    commands.insert_resource(wb::clear_color(env));
    commands.insert_resource(wb::ambient_light(env));

    let mut spawned = HashMap::new();
    for def in &manifest.entities {
        let entity = spawn_entity(
            &mut commands,
            def,
            &mut meshes,
            &mut materials,
            &asset_server,
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

fn apply_camera(commands: &mut Commands, camera: Entity, manifest: &wt::WorldManifest) {
    let mut camera = commands.entity(camera);
    camera.insert(Projection::Perspective(wb::perspective(
        manifest.camera.as_ref(),
    )));
    match wb::distance_fog(manifest.environment.as_ref()) {
        Some(fog) => {
            camera.insert(fog);
        }
        None => {
            camera.remove::<DistanceFog>();
        }
    }
}

fn spawn_entity(
    commands: &mut Commands,
    def: &wt::WorldEntity,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
) -> Entity {
    let mut transform = wb::transform(&def.transform);
    if let Some(light) = &def.light {
        transform = wb::light_transform(light, transform);
    }
    let mut entity = commands.spawn((
        FromManifest,
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
    // A placed GLB: spawn the gltf scene as this entity's subtree (Verse's
    // direct pattern — the scene appears when the async load completes).
    // The path is `models/<file>` relative to the assets root pinned in
    // `main`'s `AssetPlugin`.
    if let Some(mesh) = &def.mesh_asset {
        let handle: Handle<_> =
            asset_server.load(GltfAssetLabel::Scene(0).from_asset(mesh.path.clone()));
        entity.insert(WorldAssetRoot(handle));
    }
    if let Some(light) = &def.light {
        wb::insert_light(&mut entity, light);
    }
    entity.id()
}
