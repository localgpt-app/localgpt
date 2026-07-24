//! Vegetation scattering across terrain.
//!
//! Places trees, bushes, grass, flowers, and rocks using Poisson disk sampling.

use bevy::mesh::{Mesh, PrimitiveTopology};
use bevy::prelude::*;
use rand::{RngExt, SeedableRng};
use serde::{Deserialize, Serialize};

/// Type of foliage to spawn.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, Reflect)]
#[serde(rename_all = "lowercase")]
pub enum FoliageType {
    #[default]
    Tree,
    Bush,
    Grass,
    Flower,
    Rock,
}

/// Area definition for foliage placement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoliageArea {
    /// Center of the area.
    #[serde(default)]
    pub center: Vec3,
    /// Radius of the circular area.
    #[serde(default = "default_radius")]
    pub radius: f32,
}

fn default_radius() -> f32 {
    30.0
}

impl Default for FoliageArea {
    fn default() -> Self {
        Self {
            center: Vec3::ZERO,
            radius: default_radius(),
        }
    }
}

/// Parameters for foliage generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FoliageParams {
    /// Type of foliage.
    pub foliage_type: FoliageType,
    /// Area to place foliage in.
    #[serde(default)]
    pub area: FoliageArea,
    /// Density (0.0-1.0, items per square meter scaled).
    #[serde(default = "default_density")]
    pub density: f32,
    /// Min/max random scale multiplier.
    #[serde(default = "default_scale_range")]
    pub scale_range: Vec2,
    /// Random seed.
    #[serde(default)]
    pub seed: Option<u64>,
    /// Avoid placing on paths.
    #[serde(default = "default_avoid_paths")]
    pub avoid_paths: bool,
    /// Avoid placing in water.
    #[serde(default = "default_avoid_water")]
    pub avoid_water: bool,
    /// Maximum terrain slope for placement (degrees).
    #[serde(default = "default_max_slope")]
    pub max_slope: f32,
}

fn default_density() -> f32 {
    0.5
}
fn default_scale_range() -> Vec2 {
    Vec2::new(0.8, 1.2)
}
fn default_avoid_paths() -> bool {
    true
}
fn default_avoid_water() -> bool {
    true
}
fn default_max_slope() -> f32 {
    30.0
}

impl Default for FoliageParams {
    fn default() -> Self {
        Self {
            foliage_type: FoliageType::Tree,
            area: FoliageArea::default(),
            density: default_density(),
            scale_range: default_scale_range(),
            seed: None,
            avoid_paths: default_avoid_paths(),
            avoid_water: default_avoid_water(),
            max_slope: default_max_slope(),
        }
    }
}

/// Marker component for foliage entities.
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct Foliage {
    /// Type of foliage.
    pub foliage_type: FoliageType,
}

/// Generate placement points using Poisson disk sampling.
pub fn generate_foliage_points(params: &FoliageParams) -> Vec<Vec3> {
    let seed = params.seed.unwrap_or_else(rand::random::<u64>);
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);

    // Calculate minimum distance based on density
    // density 1.0 = 1 item per 2m², density 0.1 = 1 item per 20m²
    let min_distance = (2.0 / params.density).sqrt() * 2.0;

    let area = params.area.radius * params.area.radius * std::f32::consts::PI;
    let expected_count = (area * params.density / 2.0) as usize;

    let mut points = Vec::with_capacity(expected_count);
    let mut active = Vec::new();

    // Start with center point
    let center = params.area.center;
    points.push(center);
    active.push(0);

    while let Some(active_idx) = active.pop() {
        let point = points[active_idx];
        let mut found = false;

        // Try up to 30 candidates
        for _ in 0..30 {
            let angle = rng.random::<f32>() * std::f32::consts::TAU;
            let distance = min_distance + rng.random::<f32>() * min_distance;

            let candidate = Vec3::new(
                point.x + angle.cos() * distance,
                point.y,
                point.z + angle.sin() * distance,
            );

            // Check if within area
            let dist_from_center = (candidate - center).length();
            if dist_from_center > params.area.radius {
                continue;
            }

            // Check minimum distance from all existing points
            let too_close = points.iter().any(|&p| {
                (p.x - candidate.x).powi(2) + (p.z - candidate.z).powi(2) < min_distance.powi(2)
            });

            if !too_close {
                points.push(candidate);
                active.push(points.len() - 1);
                found = true;
            }
        }

        if found {
            active.push(active_idx);
        }
    }

    points
}

/// Generate procedural mesh for foliage type.
pub fn generate_foliage_mesh(foliage_type: FoliageType) -> Mesh {
    match foliage_type {
        FoliageType::Tree => generate_tree_mesh(),
        FoliageType::Bush => generate_bush_mesh(),
        FoliageType::Grass => generate_grass_mesh(),
        FoliageType::Flower => generate_flower_mesh(),
        FoliageType::Rock => generate_rock_mesh(),
    }
}

/// Generate tree mesh (cylinder trunk + cone canopy).
fn generate_tree_mesh() -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // Trunk (cylinder, 8 segments)
    let trunk_radius = 0.2;
    let trunk_height = 2.0;
    let segments = 8u32;

    for i in 0..segments {
        let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let next_angle = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;

        let x = angle.cos() * trunk_radius;
        let z = angle.sin() * trunk_radius;
        let nx = angle.cos();
        let nz = angle.sin();

        let nx2 = next_angle.cos();
        let nz2 = next_angle.sin();
        let x2 = next_angle.cos() * trunk_radius;
        let z2 = next_angle.sin() * trunk_radius;

        let base = positions.len() as u32;

        // Four vertices for this segment
        positions.push([x, 0.0, z]);
        positions.push([x, trunk_height, z]);
        positions.push([x2, 0.0, z2]);
        positions.push([x2, trunk_height, z2]);

        normals.push([nx, 0.0, nz]);
        normals.push([nx, 0.0, nz]);
        normals.push([nx2, 0.0, nz2]);
        normals.push([nx2, 0.0, nz2]);

        uvs.push([0.0, 0.0]);
        uvs.push([0.0, 1.0]);
        uvs.push([1.0, 0.0]);
        uvs.push([1.0, 1.0]);

        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 2);

        indices.push(base + 2);
        indices.push(base + 1);
        indices.push(base + 3);
    }

    // Canopy (cone, 8 segments)
    let canopy_radius = 1.5;
    let canopy_height = 3.0;
    let canopy_base_y = trunk_height;

    let apex_idx = positions.len() as u32;
    positions.push([0.0, canopy_base_y + canopy_height, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    uvs.push([0.5, 1.0]);

    for i in 0..segments {
        let angle = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let next_angle = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;

        let x = angle.cos() * canopy_radius;
        let z = angle.sin() * canopy_radius;
        let x2 = next_angle.cos() * canopy_radius;
        let z2 = next_angle.sin() * canopy_radius;

        let base = positions.len() as u32;

        positions.push([x, canopy_base_y, z]);
        positions.push([x2, canopy_base_y, z2]);

        // Approximate cone normals
        let n = Vec3::new(x, canopy_height, z).normalize();
        let n2 = Vec3::new(x2, canopy_height, z2).normalize();

        normals.push([n.x, n.y, n.z]);
        normals.push([n2.x, n2.y, n2.z]);

        uvs.push([0.0, 0.0]);
        uvs.push([1.0, 0.0]);

        indices.push(apex_idx);
        indices.push(base);
        indices.push(base + 1);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

/// Generate bush mesh (flattened sphere).
fn generate_bush_mesh() -> Mesh {
    // Simple icosphere approximation with 12 vertices
    let phi = (1.0 + 5.0f32.sqrt()) / 2.0;
    let scale = 0.6;

    let base_verts: [[f32; 3]; 12] = [
        [-1.0, phi, 0.0],
        [1.0, phi, 0.0],
        [-1.0, -phi, 0.0],
        [1.0, -phi, 0.0],
        [0.0, -1.0, phi],
        [0.0, 1.0, phi],
        [0.0, -1.0, -phi],
        [0.0, 1.0, -phi],
        [phi, 0.0, -1.0],
        [phi, 0.0, 1.0],
        [-phi, 0.0, -1.0],
        [-phi, 0.0, 1.0],
    ];
    let positions: Vec<[f32; 3]> = base_verts
        .iter()
        .map(|v| [v[0] * scale * 0.8, v[1] * scale * 0.5, v[2] * scale * 0.8])
        .collect();

    let normals = positions.clone();
    let uvs: Vec<[f32; 2]> = (0..12).map(|_| [0.5, 0.5]).collect();

    let indices: Vec<u32> = vec![
        0, 11, 5, 0, 5, 1, 0, 1, 7, 0, 7, 10, 0, 10, 11, 1, 5, 9, 5, 11, 4, 11, 10, 2, 10, 7, 6, 7,
        1, 8, 3, 9, 4, 3, 4, 2, 3, 2, 6, 3, 6, 8, 3, 8, 9, 4, 9, 5, 2, 4, 11, 6, 2, 10, 8, 6, 7, 9,
        8, 1,
    ];

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

/// Generate grass mesh (cluster of quads).
fn generate_grass_mesh() -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // 6 grass blades
    for i in 0..6u32 {
        let _angle = (i as f32 / 6.0) * std::f32::consts::TAU;
        let base = positions.len() as u32;

        // Simple quad for each blade
        let w = 0.05;
        let h = 0.3 + (i as f32 * 0.02);

        positions.push([-w, 0.0, 0.0]);
        positions.push([w, 0.0, 0.0]);
        positions.push([-w * 0.5, h, 0.0]);
        positions.push([w * 0.5, h, 0.0]);

        normals.push([0.0, 0.0, 1.0]);
        normals.push([0.0, 0.0, 1.0]);
        normals.push([0.0, 0.0, 1.0]);
        normals.push([0.0, 0.0, 1.0]);

        uvs.push([0.0, 0.0]);
        uvs.push([1.0, 0.0]);
        uvs.push([0.0, 1.0]);
        uvs.push([1.0, 1.0]);

        indices.push(base);
        indices.push(base + 2);
        indices.push(base + 1);

        indices.push(base + 1);
        indices.push(base + 2);
        indices.push(base + 3);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

/// Generate flower mesh (stem + colored sphere).
fn generate_flower_mesh() -> Mesh {
    // Simple: thin cylinder stem + small sphere on top
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // Stem (thin cylinder)
    let stem_height = 0.3;

    // Stem vertices
    positions.push([0.0, 0.0, 0.0]);
    positions.push([0.0, stem_height, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    normals.push([0.0, 1.0, 0.0]);
    uvs.push([0.5, 0.0]);
    uvs.push([0.5, 1.0]);

    // Flower head (simple tetrahedron approximation)
    let head_base = positions.len() as u32;
    let head_y = stem_height;
    let head_size = 0.1;

    positions.push([0.0, head_y + head_size, 0.0]); // top
    positions.push([head_size, head_y, 0.0]);
    positions.push([-head_size, head_y, 0.0]);
    positions.push([0.0, head_y, head_size]);
    positions.push([0.0, head_y, -head_size]);

    for _ in 0..5 {
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([0.5, 0.5]);
    }

    indices.push(head_base);
    indices.push(head_base + 1);
    indices.push(head_base + 2);

    indices.push(head_base);
    indices.push(head_base + 2);
    indices.push(head_base + 3);

    indices.push(head_base);
    indices.push(head_base + 3);
    indices.push(head_base + 4);

    indices.push(head_base);
    indices.push(head_base + 4);
    indices.push(head_base + 1);

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

/// Generate rock mesh (deformed sphere).
fn generate_rock_mesh() -> Mesh {
    // Simple dodecahedron approximation
    let phi = (1.0 + 5.0f32.sqrt()) / 2.0;
    let scale = 0.4;

    let base_verts: [[f32; 3]; 12] = [
        [-1.0, phi, 0.0],
        [1.0, phi, 0.0],
        [-1.0, -phi, 0.0],
        [1.0, -phi, 0.0],
        [0.0, -1.0, phi],
        [0.0, 1.0, phi],
        [0.0, -1.0, -phi],
        [0.0, 1.0, -phi],
        [phi, 0.0, -1.0],
        [phi, 0.0, 1.0],
        [-phi, 0.0, -1.0],
        [-phi, 0.0, 1.0],
    ];
    let positions: Vec<[f32; 3]> = base_verts
        .iter()
        .map(|v| [v[0] * scale, v[1] * scale * 0.6, v[2] * scale])
        .collect();

    let normals = positions.clone();
    let uvs: Vec<[f32; 2]> = (0..12).map(|_| [0.5, 0.5]).collect();

    let indices: Vec<u32> = vec![
        0, 11, 5, 0, 5, 1, 0, 1, 7, 0, 7, 10, 0, 10, 11, 1, 5, 9, 5, 11, 4, 11, 10, 2, 10, 7, 6, 7,
        1, 8, 3, 9, 4, 3, 4, 2, 3, 2, 6, 3, 6, 8, 3, 8, 9, 4, 9, 5, 2, 4, 11, 6, 2, 10, 8, 6, 7, 9,
        8, 1,
    ];

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));
    mesh
}

/// Get material color for foliage type.
pub fn get_foliage_color(foliage_type: FoliageType) -> Color {
    match foliage_type {
        FoliageType::Tree => Color::srgb(0.15, 0.5, 0.15), // Green canopy
        FoliageType::Bush => Color::srgb(0.2, 0.55, 0.2),
        FoliageType::Grass => Color::srgb(0.25, 0.6, 0.2),
        FoliageType::Flower => Color::srgb(0.9, 0.3, 0.4), // Pink/red
        FoliageType::Rock => Color::srgb(0.5, 0.5, 0.5),
    }
}

/// Plugin for foliage systems.
pub struct FoliagePlugin;

impl Plugin for FoliagePlugin {
    fn build(&self, _app: &mut App) {
        // Foliage is generated on demand
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_foliage_points() {
        let params = FoliageParams {
            area: FoliageArea {
                radius: 10.0,
                ..default()
            },
            density: 0.3,
            seed: Some(42),
            ..default()
        };
        let points = generate_foliage_points(&params);
        // Should have some points
        assert!(!points.is_empty());
        // All points should be within radius
        for p in &points {
            let dist = (*p - params.area.center).length();
            assert!(dist <= params.area.radius);
        }
    }

    #[test]
    fn test_generate_tree_mesh() {
        let mesh = generate_foliage_mesh(FoliageType::Tree);
        assert!(mesh.count_vertices() > 0);
    }

    #[test]
    fn test_generate_bush_mesh() {
        let mesh = generate_foliage_mesh(FoliageType::Bush);
        assert_eq!(mesh.count_vertices(), 12);
    }

    #[test]
    fn test_generate_grass_mesh() {
        let mesh = generate_foliage_mesh(FoliageType::Grass);
        // 6 blades * 4 vertices each = 24
        assert_eq!(mesh.count_vertices(), 24);
    }

    #[test]
    fn test_generate_flower_mesh() {
        let mesh = generate_foliage_mesh(FoliageType::Flower);
        // 2 stem vertices + 5 head vertices = 7
        assert_eq!(mesh.count_vertices(), 7);
    }

    #[test]
    fn test_generate_rock_mesh() {
        let mesh = generate_foliage_mesh(FoliageType::Rock);
        assert_eq!(mesh.count_vertices(), 12);
    }

    #[test]
    fn test_foliage_params_default() {
        let params = FoliageParams::default();
        assert!(matches!(params.foliage_type, FoliageType::Tree));
        assert_eq!(params.density, 0.5);
        assert_eq!(params.scale_range, Vec2::new(0.8, 1.2));
        assert!(params.seed.is_none());
        assert!(params.avoid_paths);
        assert!(params.avoid_water);
        assert_eq!(params.max_slope, 30.0);
    }

    #[test]
    fn test_foliage_area_default() {
        let area = FoliageArea::default();
        assert_eq!(area.center, Vec3::ZERO);
        assert_eq!(area.radius, 30.0);
    }

    #[test]
    fn test_foliage_colors_distinct() {
        let tree = get_foliage_color(FoliageType::Tree);
        let bush = get_foliage_color(FoliageType::Bush);
        let flower = get_foliage_color(FoliageType::Flower);
        let rock = get_foliage_color(FoliageType::Rock);
        assert_ne!(tree, flower);
        assert_ne!(bush, rock);
    }

    #[test]
    fn test_deterministic_points() {
        let params = FoliageParams {
            seed: Some(123),
            area: FoliageArea {
                radius: 10.0,
                ..default()
            },
            density: 0.3,
            ..default()
        };
        let points_a = generate_foliage_points(&params);
        let points_b = generate_foliage_points(&params);
        assert_eq!(points_a.len(), points_b.len());
        for (a, b) in points_a.iter().zip(points_b.iter()) {
            assert_eq!(*a, *b);
        }
    }
}
