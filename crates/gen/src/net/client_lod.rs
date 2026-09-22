//! Client-side level-of-detail for streamed worlds (§2 "Asset Streaming &
//! Memory Management").
//!
//! - **HLOD impostors:** the host replicates one [`NetChunkSummary`] per
//!   occupied chunk to everyone. Chunks outside this client's view window
//!   (whose full-detail entities the host no longer streams to us) render as
//!   a single low-poly box in the chunk's average color, so the horizon
//!   isn't empty.
//! - **Mesh baking:** once a chunk has been static for
//!   [`BAKE_QUIET_SECS`], its static primitives are merged per material into
//!   combined meshes and the originals stop drawing (their `Mesh3d` is
//!   parked in [`BakedAway`]). Any change in the chunk un-bakes it.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bevy::prelude::*;
use localgpt_world_types::ChunkCoord;

use super::bake::{
    BAKE_MIN_ENTITIES, BAKE_QUIET_SECS, BakeTracker, layout_signature, merge_meshes,
};
use super::client::{ClientViewState, NetVisual};
use super::protocol::{
    NetBehaviors, NetChunkSummary, NetEntityKind, NetKind, NetLight, NetMaterial, NetShape,
};

/// Adds HLOD impostors and (optionally) static mesh baking to the client.
pub struct ClientLodPlugin {
    pub bake: bool,
}

impl Plugin for ClientLodPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (lod_spawn_impostors, lod_impostor_visibility)
                .chain()
                .after(TransformSystems::Propagate),
        );
        if self.bake {
            app.init_resource::<BakeState>()
                .add_observer(on_bake_track_removed)
                .add_systems(
                    PostUpdate,
                    (bake_track_changes, bake_quiet_chunks)
                        .chain()
                        .after(TransformSystems::Propagate),
                );
        }
    }
}

// ---------------------------------------------------------------------------
// HLOD impostors
// ---------------------------------------------------------------------------

/// Marker on a replicated chunk summary that has an impostor visual.
#[derive(Component)]
pub(crate) struct Impostor {
    chunk: ChunkCoord,
}

fn impostor_transform(summary: &NetChunkSummary) -> Transform {
    let min = Vec3::from_array(summary.0.min);
    let max = Vec3::from_array(summary.0.max);
    Transform::from_translation((min + max) * 0.5).with_scale((max - min).max(Vec3::splat(0.5)))
}

fn impostor_material(summary: &NetChunkSummary) -> StandardMaterial {
    let [r, g, b, a] = summary.0.color;
    StandardMaterial {
        base_color: Color::srgba(r, g, b, a),
        alpha_mode: if a < 0.999 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        perceptual_roughness: 1.0,
        ..default()
    }
}

/// Build/refresh impostor visuals for replicated chunk summaries.
#[allow(clippy::type_complexity)]
fn lod_spawn_impostors(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut unit_cube: Local<Option<Handle<Mesh>>>,
    added: Query<(Entity, &NetChunkSummary), Added<NetChunkSummary>>,
    mut changed: Query<
        (
            &NetChunkSummary,
            &mut Transform,
            &MeshMaterial3d<StandardMaterial>,
        ),
        (Changed<NetChunkSummary>, With<Impostor>),
    >,
) {
    let cube = unit_cube
        .get_or_insert_with(|| meshes.add(Cuboid::new(1.0, 1.0, 1.0)))
        .clone();
    for (entity, summary) in &added {
        commands.entity(entity).insert((
            Name::new(format!("Impostor {}", summary.0.coord)),
            Impostor {
                chunk: summary.0.coord,
            },
            Mesh3d(cube.clone()),
            MeshMaterial3d(materials.add(impostor_material(summary))),
            impostor_transform(summary),
            Visibility::Hidden,
        ));
    }
    for (summary, mut transform, material) in &mut changed {
        *transform = impostor_transform(summary);
        if let Some(mut mat) = materials.get_mut(&material.0) {
            *mat = impostor_material(summary);
        }
    }
}

/// Impostors show only for chunks outside our view window — inside it the
/// host streams full detail.
fn lod_impostor_visibility(
    view: Res<ClientViewState>,
    mut impostors: Query<(&Impostor, &mut Visibility)>,
) {
    for (impostor, mut visibility) in &mut impostors {
        let wanted = if view.window.contains(impostor.chunk) {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

// ---------------------------------------------------------------------------
// Mesh baking
// ---------------------------------------------------------------------------

/// Parked mesh of an entity whose geometry is currently part of a baked
/// chunk mesh.
#[derive(Component)]
pub(crate) struct BakedAway(pub Handle<Mesh>);

/// Combined mesh produced by baking a chunk.
#[derive(Component)]
struct BakedChunkMesh;

/// Per-entity change tracking for baking.
#[derive(Component)]
struct BakeTrack {
    chunk: ChunkCoord,
    global: GlobalTransform,
    eligible: bool,
    material_key: u64,
}

#[derive(Resource, Default)]
pub(crate) struct BakeState {
    tracker: BakeTracker,
    /// Combined mesh entities per baked chunk.
    baked_meshes: HashMap<ChunkCoord, Vec<Entity>>,
    /// Entities whose meshes were parked, per baked chunk.
    members: HashMap<ChunkCoord, Vec<Entity>>,
}

impl BakeState {
    /// `(baked chunks, combined meshes, entities folded into them)`.
    pub(crate) fn stats(&self) -> (usize, usize, usize) {
        (
            self.baked_meshes.len(),
            self.baked_meshes.values().map(Vec::len).sum(),
            self.members.values().map(Vec::len).sum(),
        )
    }
}

/// Visible impostor count (for `/stats`).
pub(crate) fn visible_impostors(impostors: &Query<(&Impostor, &Visibility)>) -> usize {
    impostors
        .iter()
        .filter(|(_, v)| **v != Visibility::Hidden)
        .count()
}

fn material_key(material: Option<&NetMaterial>) -> u64 {
    let mut h = DefaultHasher::new();
    match material {
        Some(m) => serde_json::to_string(&m.0).unwrap_or_default().hash(&mut h),
        None => "default".hash(&mut h),
    }
    h.finish()
}

fn globals_differ(a: &GlobalTransform, b: &GlobalTransform) -> bool {
    let (a, b) = (a.affine(), b.affine());
    !a.translation.abs_diff_eq(b.translation, 1e-3) || !a.matrix3.abs_diff_eq(b.matrix3, 1e-4)
}

/// Undo a chunk's bake: drop combined meshes, give members their meshes back.
fn unbake(commands: &mut Commands, state: &mut BakeState, chunk: ChunkCoord) {
    for baked in state.baked_meshes.remove(&chunk).unwrap_or_default() {
        commands.entity(baked).try_despawn();
    }
    for member in state.members.remove(&chunk).unwrap_or_default() {
        commands.queue(move |world: &mut World| {
            let Ok(mut entity) = world.get_entity_mut(member) else {
                return;
            };
            if let Some(parked) = entity.take::<BakedAway>()
                && !entity.contains::<Mesh3d>()
            {
                // (A shape change may already have installed a new mesh.)
                entity.insert(Mesh3d(parked.0));
            }
        });
    }
}

fn mark_dirty(commands: &mut Commands, state: &mut BakeState, chunk: ChunkCoord, now: f64) {
    if state.tracker.mark_dirty(chunk, now) {
        unbake(commands, state, chunk);
    }
}

/// Detect changes that invalidate a chunk's bake.
///
/// Only *eligible* (static) primitives mark chunks dirty — an animated
/// windmill must not keep its whole chunk from ever baking. Global
/// transforms are compared by value because interpolation rewrites
/// replicated transforms every frame even when nothing moved.
#[allow(clippy::type_complexity)]
fn bake_track_changes(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<BakeState>,
    mut visuals: Query<
        (
            Entity,
            &GlobalTransform,
            &NetEntityKind,
            Has<NetShape>,
            Has<NetLight>,
            Option<&NetMaterial>,
            Option<&mut BakeTrack>,
        ),
        With<NetVisual>,
    >,
    structural: Query<
        (),
        Or<(
            Changed<NetShape>,
            Changed<NetMaterial>,
            Changed<NetLight>,
            Changed<NetBehaviors>,
        )>,
    >,
    animated: Query<(), With<NetBehaviors>>,
    parents: Query<&ChildOf>,
) {
    let now = time.elapsed_secs_f64();
    for (entity, global, kind, has_shape, has_light, material, track) in &mut visuals {
        let under_behavior = animated.contains(entity)
            || parents
                .iter_ancestors(entity)
                .any(|ancestor| animated.contains(ancestor));
        let eligible = kind.0 == NetKind::Primitive && has_shape && !has_light && !under_behavior;
        let translation = global.translation();
        let chunk = ChunkCoord::from_world_pos(translation.x, translation.z);
        let key = material_key(material);

        let Some(mut track) = track else {
            commands.entity(entity).insert(BakeTrack {
                chunk,
                global: *global,
                eligible,
                material_key: key,
            });
            if eligible {
                mark_dirty(&mut commands, &mut state, chunk, now);
            }
            continue;
        };

        let changed = track.eligible != eligible
            || structural.contains(entity)
            || (eligible
                && (track.chunk != chunk
                    || track.material_key != key
                    || globals_differ(&track.global, global)));
        if changed {
            let old = track.chunk;
            mark_dirty(&mut commands, &mut state, old, now);
            if chunk != old {
                mark_dirty(&mut commands, &mut state, chunk, now);
            }
            track.chunk = chunk;
            track.global = *global;
            track.eligible = eligible;
            track.material_key = key;
        }
    }
}

/// A tracked entity despawned (deleted on the host or left our view).
fn on_bake_track_removed(
    trigger: On<Remove, BakeTrack>,
    tracks: Query<&BakeTrack>,
    time: Res<Time>,
    mut state: ResMut<BakeState>,
    mut commands: Commands,
) {
    if let Ok(track) = tracks.get(trigger.entity) {
        let chunk = track.chunk;
        mark_dirty(&mut commands, &mut state, chunk, time.elapsed_secs_f64());
    }
}

/// Bake chunks that have been quiet long enough.
#[allow(clippy::type_complexity)]
fn bake_quiet_chunks(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<BakeState>,
    mut meshes: ResMut<Assets<Mesh>>,
    candidates: Query<(
        Entity,
        &BakeTrack,
        &GlobalTransform,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let ready = state
        .tracker
        .ready(time.elapsed_secs_f64(), BAKE_QUIET_SECS);
    if ready.is_empty() {
        return;
    }

    for chunk in ready {
        // Members: eligible primitives in this chunk that still draw.
        let members: Vec<_> = candidates
            .iter()
            .filter(|(_, track, ..)| track.eligible && track.chunk == chunk)
            .collect();
        // Even when not worth baking, record the chunk as settled so we
        // don't re-scan it every frame until something changes.
        state.tracker.set_baked(chunk);
        if members.len() < BAKE_MIN_ENTITIES {
            continue;
        }

        // Group by material + vertex layout.
        let mut groups: HashMap<(u64, u64), Vec<usize>> = HashMap::new();
        for (i, (_, track, _, mesh, _)) in members.iter().enumerate() {
            let Some(mesh) = meshes.get(&mesh.0) else {
                continue;
            };
            groups
                .entry((track.material_key, layout_signature(mesh)))
                .or_default()
                .push(i);
        }

        let mut baked_entities = Vec::new();
        let mut parked = Vec::new();
        for indices in groups.into_values() {
            if indices.len() < 2 {
                continue;
            }
            let merged = {
                let parts: Vec<(&Mesh, Transform)> = indices
                    .iter()
                    .filter_map(|&i| {
                        let (_, _, global, mesh, _) = members[i];
                        meshes.get(&mesh.0).map(|m| (m, global.compute_transform()))
                    })
                    .collect();
                if parts.len() != indices.len() {
                    continue;
                }
                match merge_meshes(parts) {
                    Ok(mesh) => mesh,
                    Err(_) => continue,
                }
            };
            let material = members[indices[0]].4.0.clone();
            let baked = commands
                .spawn((
                    Name::new(format!("Baked {chunk}")),
                    BakedChunkMesh,
                    Mesh3d(meshes.add(merged)),
                    MeshMaterial3d(material),
                    Transform::IDENTITY,
                ))
                .id();
            baked_entities.push(baked);
            for &i in &indices {
                let (entity, _, _, mesh, _) = members[i];
                commands
                    .entity(entity)
                    .try_insert(BakedAway(mesh.0.clone()))
                    .try_remove::<Mesh3d>();
                parked.push(entity);
            }
        }
        if !baked_entities.is_empty() {
            state.baked_meshes.insert(chunk, baked_entities);
            state.members.insert(chunk, parked);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use localgpt_world_types as wt;

    #[test]
    fn material_key_is_stable_and_distinguishes() {
        let a = NetMaterial(wt::MaterialDef {
            color: [1.0, 0.0, 0.0, 1.0],
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 4],
            alpha_mode: None,
            unlit: None,
            double_sided: None,
            reflectance: None,
        });
        let mut b = a.clone();
        assert_eq!(material_key(Some(&a)), material_key(Some(&b)));
        b.0.color[1] = 1.0;
        assert_ne!(material_key(Some(&a)), material_key(Some(&b)));
        assert_ne!(material_key(Some(&a)), material_key(None));
    }

    #[test]
    fn globals_differ_threshold() {
        let a = GlobalTransform::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let b = GlobalTransform::from_translation(Vec3::new(1.0, 2.0, 3.0005));
        let c = GlobalTransform::from_translation(Vec3::new(1.0, 2.0, 3.1));
        assert!(!globals_differ(&a, &b));
        assert!(globals_differ(&a, &c));
    }
}
