//! Collision shapes.
//!
//! Implements Spec 5.2: `gen_add_collider` — Add collision shapes.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "physics")]
use avian3d::prelude::*;

/// Collider shape types.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Reflect)]
#[serde(rename_all = "snake_case")]
pub enum ColliderShape {
    #[default]
    Box,
    Sphere,
    Capsule,
    Cylinder,
    Mesh,
}

/// Parameters for collider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColliderParams {
    /// Target entity ID.
    pub entity_id: String,
    /// Shape type.
    #[serde(default)]
    pub shape: ColliderShape,
    /// Dimensions (optional, auto-fit to mesh).
    #[serde(default)]
    pub size: Option<Vec3>,
    /// Offset from entity origin.
    #[serde(default)]
    pub offset: Vec3,
    /// Is this a sensor (trigger only).
    #[serde(default)]
    pub is_trigger: bool,
    /// Show in debug view.
    #[serde(default = "default_visible_in_debug")]
    pub visible_in_debug: bool,
}

fn default_visible_in_debug() -> bool {
    true
}

impl Default for ColliderParams {
    fn default() -> Self {
        Self {
            entity_id: String::new(),
            shape: ColliderShape::default(),
            size: None,
            offset: Vec3::ZERO,
            is_trigger: false,
            visible_in_debug: true,
        }
    }
}

/// Component for collider configuration.
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct ColliderConfig {
    /// Shape type.
    pub shape: ColliderShape,
    /// Explicit dimensions (auto-sized from transform scale if None).
    pub size: Option<Vec3>,
    /// Offset from entity origin.
    pub offset: Vec3,
    /// Is sensor.
    pub is_trigger: bool,
    /// Debug visibility.
    pub visible_in_debug: bool,
}

/// Component for sensor colliders.
#[derive(Component, Default)]
pub struct SensorCollider;

/// System to convert ColliderConfig into Avian Collider components.
///
/// Uses explicit size if provided, otherwise derives from entity transform scale.
/// Box uses half-extents, sphere uses max scale axis as radius, etc.
#[cfg(feature = "physics")]
pub fn collider_setup_system(
    mut commands: Commands,
    query: Query<(Entity, &ColliderConfig, &Transform), Added<ColliderConfig>>,
) {
    for (entity, config, transform) in query.iter() {
        let scale = transform.scale;
        let collider = match config.shape {
            ColliderShape::Box => {
                let half = config.size.unwrap_or(scale) * 0.5;
                Collider::cuboid(half.x, half.y, half.z)
            }
            ColliderShape::Sphere => {
                let radius = config
                    .size
                    .map(|s| s.x * 0.5)
                    .unwrap_or(scale.max_element() * 0.5);
                Collider::sphere(radius)
            }
            ColliderShape::Capsule => {
                let s = config.size.unwrap_or(scale);
                let radius = s.x * 0.5;
                let height = s.y - radius * 2.0;
                Collider::capsule(radius, height.max(0.01))
            }
            ColliderShape::Cylinder => {
                let s = config.size.unwrap_or(scale);
                let radius = s.x * 0.5;
                let height = s.y;
                Collider::cylinder(radius, height)
            }
            ColliderShape::Mesh => {
                // Mesh colliders require actual mesh data; fall back to box
                let half = config.size.unwrap_or(scale) * 0.5;
                Collider::cuboid(half.x, half.y, half.z)
            }
        };

        commands.entity(entity).insert(collider);

        if config.is_trigger {
            commands.entity(entity).insert(Sensor);
        }
    }
}

/// Auto-attaches colliders to newly spawned parametric shapes.
///
/// Mirrors `terrain_collider_system` pattern: reacts to `Added<ParametricShape>`
/// and inserts a matching Avian `Collider` so physics bodies don't fall through.
/// Complex shapes (torus, pyramid, tetrahedron, wedge) use AABB bounding box fallback.
#[cfg(feature = "physics")]
#[allow(clippy::type_complexity)]
pub fn auto_collider_system(
    mut commands: Commands,
    query: Query<
        (Entity, &crate::gen3d::registry::ParametricShape),
        (
            Added<crate::gen3d::registry::ParametricShape>,
            Without<Collider>,
        ),
    >,
) {
    use localgpt_world_types::Shape;

    for (entity, param) in query.iter() {
        let collider = match &param.shape {
            Shape::Cuboid { x, y, z } => Collider::cuboid(x / 2.0, y / 2.0, z / 2.0),
            Shape::Sphere { radius } => Collider::sphere(*radius),
            Shape::Cylinder { radius, height } => Collider::cylinder(*radius, height / 2.0),
            Shape::Cone { radius, height } => {
                // Avian has no cone collider; approximate with cylinder
                Collider::cylinder(*radius, height / 2.0)
            }
            Shape::Capsule {
                radius,
                half_length,
            } => Collider::capsule(*radius, *half_length),
            Shape::Icosahedron { radius } => Collider::sphere(*radius),
            Shape::Plane { x, z } => Collider::cuboid(x / 2.0, 0.005, z / 2.0),
            // Complex shapes: use AABB bounding box
            Shape::Torus { .. }
            | Shape::Pyramid { .. }
            | Shape::Tetrahedron { .. }
            | Shape::Wedge { .. } => {
                let h = param.shape.local_aabb_half();
                Collider::cuboid(h[0], h[1].max(0.005), h[2])
            }
        };
        // The entity may be despawned by a command queued earlier this frame
        // (a world load clearing the scene), so don't panic if it is gone.
        commands
            .entity(entity)
            .try_insert((RigidBody::Static, collider));
    }
}

/// Auto-attaches trimesh colliders to glTF-loaded meshes.
///
/// glTF scenes load asynchronously — child entities with `Mesh3d` appear after
/// the scene is ready. This system catches newly added meshes that lack colliders
/// and generates trimesh colliders from vertex/index data, identical to the
/// terrain collider approach.
#[cfg(feature = "physics")]
#[allow(clippy::type_complexity)]
pub fn gltf_mesh_collider_system(
    mut commands: Commands,
    query: Query<
        (Entity, &Mesh3d),
        (
            Added<Mesh3d>,
            Without<Collider>,
            Without<crate::gen3d::registry::ParametricShape>,
        ),
    >,
    meshes: Res<Assets<Mesh>>,
) {
    use bevy::mesh::Indices;

    for (entity, mesh_handle) in query.iter() {
        let Some(mesh) = meshes.get(&mesh_handle.0) else {
            continue;
        };
        let Some(positions) = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|attr| attr.as_float3())
        else {
            continue;
        };
        let vertices: Vec<Vec3> = positions.iter().map(|p| Vec3::from_array(*p)).collect();
        let Some(indices) = mesh.indices() else {
            continue;
        };
        let tri_indices: Vec<[u32; 3]> = match indices {
            Indices::U32(idx) => idx.chunks(3).map(|c| [c[0], c[1], c[2]]).collect(),
            Indices::U16(idx) => idx
                .chunks(3)
                .map(|c| [c[0] as u32, c[1] as u32, c[2] as u32])
                .collect(),
        };
        if !vertices.is_empty() && !tri_indices.is_empty() {
            commands
                .entity(entity)
                .insert((RigidBody::Static, Collider::trimesh(vertices, tri_indices)));
        }
    }
}

/// Plugin for collider systems.
pub struct ColliderPlugin;

impl Plugin for ColliderPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(feature = "physics")]
        {
            app.add_systems(Update, collider_setup_system);
            app.add_systems(Update, auto_collider_system);
            app.add_systems(Update, gltf_mesh_collider_system);
        }

        let _ = app;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collider_params() {
        let params = ColliderParams {
            entity_id: "test".to_string(),
            shape: ColliderShape::Sphere,
            is_trigger: true,
            ..default()
        };
        assert!(params.is_trigger);
    }

    #[test]
    fn test_collider_params_default() {
        let params = ColliderParams::default();
        assert!(matches!(params.shape, ColliderShape::Box));
        assert!(!params.is_trigger);
        assert!(params.visible_in_debug);
        assert!(params.size.is_none());
        assert_eq!(params.offset, Vec3::ZERO);
    }

    #[test]
    fn test_collider_shape_default_is_box() {
        assert!(matches!(ColliderShape::default(), ColliderShape::Box));
    }

    #[test]
    fn test_collider_shape_variants() {
        assert!(!matches!(ColliderShape::Sphere, ColliderShape::Box));
        assert!(!matches!(ColliderShape::Capsule, ColliderShape::Cylinder));
        assert!(!matches!(ColliderShape::Mesh, ColliderShape::Sphere));
    }

    #[test]
    fn test_collider_config_component() {
        let config = ColliderConfig {
            shape: ColliderShape::Capsule,
            size: None,
            offset: Vec3::ZERO,
            is_trigger: true,
            visible_in_debug: false,
        };
        assert!(matches!(config.shape, ColliderShape::Capsule));
        assert!(config.is_trigger);
        assert!(!config.visible_in_debug);
    }

    #[test]
    fn test_collider_params_with_size() {
        let params = ColliderParams {
            entity_id: "box1".to_string(),
            shape: ColliderShape::Box,
            size: Some(Vec3::new(2.0, 3.0, 4.0)),
            offset: Vec3::new(0.0, 1.5, 0.0),
            is_trigger: false,
            visible_in_debug: true,
        };
        assert_eq!(params.size, Some(Vec3::new(2.0, 3.0, 4.0)));
        assert_eq!(params.offset, Vec3::new(0.0, 1.5, 0.0));
    }
}
