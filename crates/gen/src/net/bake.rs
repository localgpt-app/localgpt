//! Static mesh baking (§2 "Asset Streaming & Memory Management → Mesh
//! Baking").
//!
//! AI-generated scenes are hundreds of tiny primitives, each its own draw
//! call. Once a chunk has been static for a while, its primitives are merged
//! per material into one combined mesh and the originals are hidden; any
//! change in the chunk un-bakes it again. This keeps draw calls (and, on
//! mobile, memory for per-entity render state) proportional to the number of
//! distinct materials instead of the number of entities.
//!
//! Baking runs on the viewer side, so it works unchanged whether the world
//! arrives from a listen server today or from a sharded cloud tier later.
//! This module holds the pure pieces: the quiet-period tracker and the mesh
//! merge.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use bevy::mesh::{Mesh, PrimitiveTopology};
use bevy::prelude::Transform;
use localgpt_world_types::ChunkCoord;

/// Seconds a chunk must be unchanged before it is baked.
pub const BAKE_QUIET_SECS: f64 = 3.0;

/// Minimum number of static primitives in a chunk before baking pays off.
pub const BAKE_MIN_ENTITIES: usize = 8;

/// Tracks per-chunk change times and which chunks are currently baked.
#[derive(Debug, Default)]
pub struct BakeTracker {
    last_change: HashMap<ChunkCoord, f64>,
    baked: HashSet<ChunkCoord>,
}

impl BakeTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a change in `chunk` at time `now`. Returns `true` if the chunk
    /// was baked and must be un-baked.
    pub fn mark_dirty(&mut self, chunk: ChunkCoord, now: f64) -> bool {
        self.last_change.insert(chunk, now);
        self.baked.remove(&chunk)
    }

    /// Chunks that have been quiet for `quiet` seconds and are not yet baked.
    pub fn ready(&self, now: f64, quiet: f64) -> Vec<ChunkCoord> {
        self.last_change
            .iter()
            .filter(|(chunk, t)| now - **t >= quiet && !self.baked.contains(chunk))
            .map(|(chunk, _)| *chunk)
            .collect()
    }

    /// Record that `chunk` was baked (or deliberately left unbaked — e.g. too
    /// few entities — so it isn't retried every frame until it changes).
    pub fn set_baked(&mut self, chunk: ChunkCoord) {
        self.baked.insert(chunk);
    }

    pub fn is_baked(&self, chunk: ChunkCoord) -> bool {
        self.baked.contains(&chunk)
    }
}

/// Why a set of meshes could not be merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BakeError {
    Empty,
    /// Only triangle lists are merged.
    Topology,
    /// Parts disagree on vertex attributes or index presence.
    IncompatibleLayout,
    /// Mesh data is no longer on the CPU (render-world only asset).
    Unavailable,
}

/// Order-independent signature of a mesh's vertex layout, used to group
/// merge-compatible meshes (merging meshes with different attribute sets
/// would misalign vertex streams).
pub fn layout_signature(mesh: &Mesh) -> u64 {
    let mut sig = 0u64;
    for (attribute, _) in mesh.attributes() {
        let mut h = DefaultHasher::new();
        attribute.id.hash(&mut h);
        sig ^= h.finish();
    }
    // Fold index presence in so indexed/non-indexed never group together.
    if mesh.indices().is_some() {
        sig = sig.rotate_left(1) ^ 0x9e37_79b9_7f4a_7c15;
    }
    sig
}

/// Merge meshes into one, each baked into world space by its transform.
pub fn merge_meshes<'a>(
    parts: impl IntoIterator<Item = (&'a Mesh, Transform)>,
) -> Result<Mesh, BakeError> {
    let mut parts = parts.into_iter();
    let (first, first_tf) = parts.next().ok_or(BakeError::Empty)?;
    if first.primitive_topology() != PrimitiveTopology::TriangleList {
        return Err(BakeError::Topology);
    }
    let signature = layout_signature(first);
    let mut merged = first
        .clone()
        .try_transformed_by(first_tf)
        .map_err(|_| BakeError::Unavailable)?;
    for (mesh, tf) in parts {
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            return Err(BakeError::Topology);
        }
        if layout_signature(mesh) != signature {
            return Err(BakeError::IncompatibleLayout);
        }
        let part = mesh
            .clone()
            .try_transformed_by(tf)
            .map_err(|_| BakeError::Unavailable)?;
        merged
            .merge(&part)
            .map_err(|_| BakeError::IncompatibleLayout)?;
    }
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::primitives::{Cuboid, Sphere};
    use bevy::mesh::{MeshBuilder, Meshable, VertexAttributeValues};
    use bevy::prelude::Vec3;

    #[test]
    fn tracker_quiet_period() {
        let mut t = BakeTracker::new();
        let c = ChunkCoord { x: 0, y: 0 };
        assert!(!t.mark_dirty(c, 1.0));
        assert!(t.ready(2.0, 3.0).is_empty());
        assert_eq!(t.ready(4.5, 3.0), vec![c]);
        t.set_baked(c);
        assert!(t.ready(10.0, 3.0).is_empty());
        // A change un-bakes.
        assert!(t.mark_dirty(c, 11.0));
        assert!(!t.is_baked(c));
    }

    #[test]
    fn merge_combines_vertices_and_indices() {
        let a = Cuboid::new(1.0, 1.0, 1.0).mesh().build();
        let b = Cuboid::new(2.0, 2.0, 2.0).mesh().build();
        let va = a.count_vertices();
        let ia = a.indices().unwrap().len();
        let merged = merge_meshes([
            (&a, Transform::IDENTITY),
            (&b, Transform::from_translation(Vec3::new(10.0, 0.0, 0.0))),
        ])
        .unwrap();
        assert_eq!(merged.count_vertices(), va * 2);
        assert_eq!(merged.indices().unwrap().len(), ia * 2);
        // Second cube was translated into world space.
        let Some(VertexAttributeValues::Float32x3(pos)) =
            merged.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions missing");
        };
        let max_x = pos.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
        assert!((max_x - 11.0).abs() < 1e-4);
        // Indices of the second part point past the first part's vertices.
        let max_index = merged.indices().unwrap().iter().max().unwrap();
        assert_eq!(max_index, va * 2 - 1);
    }

    #[test]
    fn merge_shapes_with_same_layout() {
        let cube = Cuboid::new(1.0, 1.0, 1.0).mesh().build();
        let ball = Sphere::new(0.5).mesh().uv(8, 6);
        assert_eq!(layout_signature(&cube), layout_signature(&ball));
        assert!(merge_meshes([(&cube, Transform::IDENTITY), (&ball, Transform::IDENTITY)]).is_ok());
    }

    #[test]
    fn merge_rejects_mismatched_layout() {
        let cube = Cuboid::new(1.0, 1.0, 1.0).mesh().build();
        let mut bare = cube.clone();
        bare.remove_attribute(Mesh::ATTRIBUTE_UV_0);
        assert_ne!(layout_signature(&cube), layout_signature(&bare));
        assert_eq!(
            merge_meshes([(&cube, Transform::IDENTITY), (&bare, Transform::IDENTITY)]).err(),
            Some(BakeError::IncompatibleLayout)
        );
        assert_eq!(
            merge_meshes(std::iter::empty()).err(),
            Some(BakeError::Empty)
        );
    }
}
