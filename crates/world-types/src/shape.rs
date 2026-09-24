//! Parametric shape definitions — type-safe, never loses dimension info.
//!
//! Unlike the current `PrimitiveShape` + `HashMap<String, f32>` approach,
//! dimensions are encoded directly in each variant.  This survives
//! serialization roundtrips (unlike glTF export which flattens to triangles).

use serde::{Deserialize, Serialize};

/// A parametric primitive shape with inline dimensions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum Shape {
    Cuboid {
        #[serde(default = "default_one")]
        x: f32,
        #[serde(default = "default_one")]
        y: f32,
        #[serde(default = "default_one")]
        z: f32,
    },
    Sphere {
        #[serde(default = "default_half")]
        radius: f32,
    },
    Cylinder {
        #[serde(default = "default_half")]
        radius: f32,
        #[serde(default = "default_one")]
        height: f32,
    },
    Cone {
        #[serde(default = "default_half")]
        radius: f32,
        #[serde(default = "default_one")]
        height: f32,
    },
    Capsule {
        #[serde(default = "default_half")]
        radius: f32,
        #[serde(default = "default_half")]
        half_length: f32,
    },
    Torus {
        #[serde(default = "default_one")]
        major_radius: f32,
        #[serde(default = "default_quarter")]
        minor_radius: f32,
    },
    Plane {
        #[serde(default = "default_ten")]
        x: f32,
        #[serde(default = "default_ten")]
        z: f32,
    },
    Pyramid {
        #[serde(default = "default_one")]
        base_x: f32,
        #[serde(default = "default_one")]
        base_z: f32,
        #[serde(default = "default_one")]
        height: f32,
    },
    Tetrahedron {
        #[serde(default = "default_one")]
        radius: f32,
    },
    Icosahedron {
        #[serde(default = "default_one")]
        radius: f32,
    },
    Wedge {
        #[serde(default = "default_one")]
        x: f32,
        #[serde(default = "default_one")]
        y: f32,
        #[serde(default = "default_one")]
        z: f32,
    },
}

fn default_one() -> f32 {
    1.0
}
fn default_half() -> f32 {
    0.5
}
fn default_quarter() -> f32 {
    0.25
}
fn default_ten() -> f32 {
    10.0
}

impl Shape {
    /// Rough triangle count estimate for budget tracking.
    pub fn estimate_triangles(&self) -> usize {
        match self {
            Shape::Cuboid { .. } => 12,
            Shape::Sphere { .. } => 760,   // 20x20 UV sphere
            Shape::Cylinder { .. } => 124, // 32 segments
            Shape::Cone { .. } => 64,      // 32 segments
            Shape::Capsule { .. } => 1520, // 2 hemispheres + cylinder
            Shape::Torus { .. } => 2048,   // 32x32 segments
            Shape::Plane { .. } => 2,
            Shape::Pyramid { .. } => 6, // 4 triangular sides + 2 for base quad
            Shape::Tetrahedron { .. } => 4, // 4 triangular faces
            Shape::Icosahedron { .. } => 20, // 20 triangular faces
            Shape::Wedge { .. } => 8,   // 2 end caps + 3 sides (as triangles)
        }
    }

    /// Axis-aligned bounding box half-extents in local space.
    pub fn local_aabb_half(&self) -> [f32; 3] {
        match self {
            Shape::Cuboid { x, y, z } => [x / 2.0, y / 2.0, z / 2.0],
            Shape::Sphere { radius } => [*radius, *radius, *radius],
            Shape::Cylinder { radius, height } => [*radius, height / 2.0, *radius],
            Shape::Cone { radius, height } => [*radius, height / 2.0, *radius],
            Shape::Capsule {
                radius,
                half_length,
            } => [*radius, half_length + radius, *radius],
            Shape::Torus {
                major_radius,
                minor_radius,
            } => [
                major_radius + minor_radius,
                *minor_radius,
                major_radius + minor_radius,
            ],
            Shape::Plane { x, z } => [x / 2.0, 0.0, z / 2.0],
            Shape::Pyramid {
                base_x,
                base_z,
                height,
            } => [base_x / 2.0, height / 2.0, base_z / 2.0],
            Shape::Tetrahedron { radius } => [*radius, *radius, *radius],
            Shape::Icosahedron { radius } => [*radius, *radius, *radius],
            Shape::Wedge { x, y, z } => [x / 2.0, y / 2.0, z / 2.0],
        }
    }

    /// Returns the shape kind as a string (for display/logging).
    pub fn kind(&self) -> &'static str {
        match self {
            Shape::Cuboid { .. } => "cuboid",
            Shape::Sphere { .. } => "sphere",
            Shape::Cylinder { .. } => "cylinder",
            Shape::Cone { .. } => "cone",
            Shape::Capsule { .. } => "capsule",
            Shape::Torus { .. } => "torus",
            Shape::Plane { .. } => "plane",
            Shape::Pyramid { .. } => "pyramid",
            Shape::Tetrahedron { .. } => "tetrahedron",
            Shape::Icosahedron { .. } => "icosahedron",
            Shape::Wedge { .. } => "wedge",
        }
    }
}

/// Maps to the legacy `PrimitiveShape` enum for compatibility with gen commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum PrimitiveShapeKind {
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

impl Shape {
    /// Extract the shape kind without dimensions.
    pub fn primitive_kind(&self) -> PrimitiveShapeKind {
        match self {
            Shape::Cuboid { .. } => PrimitiveShapeKind::Cuboid,
            Shape::Sphere { .. } => PrimitiveShapeKind::Sphere,
            Shape::Cylinder { .. } => PrimitiveShapeKind::Cylinder,
            Shape::Cone { .. } => PrimitiveShapeKind::Cone,
            Shape::Capsule { .. } => PrimitiveShapeKind::Capsule,
            Shape::Torus { .. } => PrimitiveShapeKind::Torus,
            Shape::Plane { .. } => PrimitiveShapeKind::Plane,
            Shape::Pyramid { .. } => PrimitiveShapeKind::Pyramid,
            Shape::Tetrahedron { .. } => PrimitiveShapeKind::Tetrahedron,
            Shape::Icosahedron { .. } => PrimitiveShapeKind::Icosahedron,
            Shape::Wedge { .. } => PrimitiveShapeKind::Wedge,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_roundtrip_json() {
        let shapes = vec![
            Shape::Cuboid {
                x: 4.0,
                y: 3.0,
                z: 5.0,
            },
            Shape::Sphere { radius: 2.0 },
            Shape::Torus {
                major_radius: 3.0,
                minor_radius: 0.5,
            },
            Shape::Plane { x: 20.0, z: 20.0 },
            Shape::Pyramid {
                base_x: 4.0,
                base_z: 4.0,
                height: 3.0,
            },
            Shape::Tetrahedron { radius: 1.5 },
            Shape::Icosahedron { radius: 2.0 },
            Shape::Wedge {
                x: 2.0,
                y: 1.0,
                z: 3.0,
            },
        ];

        for shape in &shapes {
            let json = serde_json::to_string(shape).unwrap();
            let back: Shape = serde_json::from_str(&json).unwrap();
            assert_eq!(*shape, back, "roundtrip failed for {}", shape.kind());
        }
    }

    #[test]
    fn cuboid_aabb() {
        let s = Shape::Cuboid {
            x: 4.0,
            y: 6.0,
            z: 2.0,
        };
        assert_eq!(s.local_aabb_half(), [2.0, 3.0, 1.0]);
    }

    #[test]
    fn triangle_estimates_are_nonzero() {
        let shapes = [
            Shape::Cuboid {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            Shape::Sphere { radius: 1.0 },
            Shape::Cylinder {
                radius: 1.0,
                height: 1.0,
            },
            Shape::Cone {
                radius: 1.0,
                height: 1.0,
            },
            Shape::Capsule {
                radius: 1.0,
                half_length: 1.0,
            },
            Shape::Torus {
                major_radius: 1.0,
                minor_radius: 0.25,
            },
            Shape::Plane { x: 1.0, z: 1.0 },
            Shape::Pyramid {
                base_x: 4.0,
                base_z: 4.0,
                height: 3.0,
            },
            Shape::Tetrahedron { radius: 1.0 },
            Shape::Icosahedron { radius: 1.0 },
            Shape::Wedge {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        ];
        for s in &shapes {
            assert!(s.estimate_triangles() > 0, "{} has 0 triangles", s.kind());
        }
    }
}
