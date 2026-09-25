//! # localgpt-world-bevy
//!
//! The one Bevy mapping of the LocalGPT world format
//! ([`localgpt_world_types`]). Gen, MD and Verse all build their scenes
//! through these functions, so a manifest looks the same in every app, and
//! the web viewer (`localgpt-world-export`) mirrors the same rules:
//!
//! - colours are linear RGBA arrays; `color` and light colours go through
//!   [`srgba`] (Bevy's `Color::srgba`, as the Gen tools have always done),
//!   `emissive` stays linear ([`linear_rgba`]);
//! - rotations are XYZ Euler angles in degrees ([`transform`]);
//! - directional light intensity is lux, point and spot lights are lumens,
//!   spot angles are radians ([`insert_light`]);
//! - directional and spot lights aim along `direction`, defaulting to
//!   Gen's `[0, -1, -0.5]` and `[0, -1, 0]` ([`light_transform`]);
//! - every parametric [`Shape`](wt::Shape) is centered on its origin and
//!   fits the bounds `Shape::local_aabb_half` declares ([`shape_mesh`]).
//!
//! Reference scenes for eyeballing all of this live in
//! `crates/world-types/conformance/`.

use std::f32::consts::FRAC_PI_4;

use bevy::asset::RenderAssetUsages;
use bevy::ecs::system::EntityCommands;
use bevy::light::{DirectionalLight, GlobalAmbientLight, PointLight, SpotLight};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::pbr::{DistanceFog, FogFalloff, StandardMaterial};
use bevy::prelude::*;
use localgpt_world_types as wt;

/// Default direction of a directional light without one (Gen's sun).
pub const DEFAULT_DIRECTIONAL_DIRECTION: [f32; 3] = [0.0, -1.0, -0.5];
/// Default direction of a spot light without one (straight down).
pub const DEFAULT_SPOT_DIRECTION: [f32; 3] = [0.0, -1.0, 0.0];

/// A manifest colour as Bevy's `Color::srgba` (base colours, light colours,
/// background, fog).
pub fn srgba([r, g, b, a]: [f32; 4]) -> Color {
    Color::srgba(r, g, b, a)
}

/// A manifest colour as linear RGBA (emissive).
pub fn linear_rgba([r, g, b, a]: [f32; 4]) -> LinearRgba {
    LinearRgba::new(r, g, b, a)
}

/// The Bevy transform of a manifest transform: XYZ Euler degrees.
pub fn transform(t: &wt::WorldTransform) -> Transform {
    let [rx, ry, rz] = t.rotation_degrees.map(f32::to_radians);
    Transform {
        translation: Vec3::from_array(t.position),
        rotation: Quat::from_euler(EulerRot::XYZ, rx, ry, rz),
        scale: Vec3::from_array(t.scale),
    }
}

/// The transform back as manifest fields (XYZ Euler degrees).
pub fn world_transform(transform: &Transform, visible: bool) -> wt::WorldTransform {
    let (rx, ry, rz) = transform.rotation.to_euler(EulerRot::XYZ);
    wt::WorldTransform {
        position: transform.translation.to_array(),
        rotation_degrees: [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()],
        scale: transform.scale.to_array(),
        visible,
    }
}

/// `Visibility` for a manifest transform's `visible` flag.
pub fn visibility(t: &wt::WorldTransform) -> Visibility {
    if t.visible {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// Aim a light's transform along its `direction` (directional and spot
/// lights); point lights keep the transform as is.
pub fn light_transform(light: &wt::LightDef, mut transform: Transform) -> Transform {
    let default_direction = match light.light_type {
        wt::LightType::Directional => Some(DEFAULT_DIRECTIONAL_DIRECTION),
        wt::LightType::Spot => Some(DEFAULT_SPOT_DIRECTION),
        wt::LightType::Point => None,
    };
    if let Some(direction) = light.direction.or(default_direction) {
        let direction = Vec3::from_array(direction);
        if direction.length_squared() > 0.0 {
            transform.look_to(direction, Vec3::Y);
        }
    }
    transform
}

/// Insert the Bevy light component for a manifest light.
pub fn insert_light(entity: &mut EntityCommands, light: &wt::LightDef) {
    let color = srgba(light.color);
    match light.light_type {
        wt::LightType::Directional => {
            entity.insert(DirectionalLight {
                illuminance: light.intensity,
                shadow_maps_enabled: light.shadows,
                color,
                ..default()
            });
        }
        wt::LightType::Point => {
            let mut point = PointLight {
                intensity: light.intensity,
                shadow_maps_enabled: light.shadows,
                color,
                ..default()
            };
            if let Some(range) = light.range {
                point.range = range;
            }
            entity.insert(point);
        }
        wt::LightType::Spot => {
            let mut spot = SpotLight {
                intensity: light.intensity,
                shadow_maps_enabled: light.shadows,
                color,
                ..default()
            };
            if let Some(range) = light.range {
                spot.range = range;
            }
            if let Some(outer) = light.outer_angle {
                spot.outer_angle = outer;
            }
            if let Some(inner) = light.inner_angle {
                spot.inner_angle = inner;
            }
            entity.insert(spot);
        }
    }
}

/// The `StandardMaterial` for a manifest material.
pub fn standard_material(def: &wt::MaterialDef) -> StandardMaterial {
    let mut material = StandardMaterial {
        base_color: srgba(def.color),
        metallic: def.metallic,
        perceptual_roughness: def.roughness,
        emissive: linear_rgba(def.emissive),
        ..default()
    };
    if let Some(alpha) = def.alpha_mode {
        material.alpha_mode = alpha_mode(alpha);
    }
    if let Some(unlit) = def.unlit {
        material.unlit = unlit;
    }
    if let Some(double_sided) = def.double_sided {
        material.double_sided = double_sided;
    }
    if let Some(reflectance) = def.reflectance {
        material.reflectance = reflectance;
    }
    material
}

/// Bevy's `AlphaMode` for a manifest alpha mode.
pub fn alpha_mode(alpha: wt::AlphaModeDef) -> AlphaMode {
    match alpha {
        wt::AlphaModeDef::Opaque => AlphaMode::Opaque,
        wt::AlphaModeDef::Mask(cutoff) => AlphaMode::Mask(cutoff),
        wt::AlphaModeDef::Blend => AlphaMode::Blend,
        wt::AlphaModeDef::Add => AlphaMode::Add,
        wt::AlphaModeDef::Multiply => AlphaMode::Multiply,
    }
}

/// The manifest alpha mode for Bevy's `AlphaMode` (modes the format has no
/// name for map to `Opaque`).
pub fn alpha_mode_def(alpha: AlphaMode) -> wt::AlphaModeDef {
    match alpha {
        AlphaMode::Opaque => wt::AlphaModeDef::Opaque,
        AlphaMode::Mask(cutoff) => wt::AlphaModeDef::Mask(cutoff),
        AlphaMode::Blend => wt::AlphaModeDef::Blend,
        AlphaMode::Add => wt::AlphaModeDef::Add,
        AlphaMode::Multiply => wt::AlphaModeDef::Multiply,
        _ => wt::AlphaModeDef::Opaque,
    }
}

/// The scene-wide ambient light for a manifest environment (Bevy's
/// defaults when the environment leaves a field unset).
pub fn ambient_light(env: Option<&wt::EnvironmentDef>) -> GlobalAmbientLight {
    let mut ambient = GlobalAmbientLight::default();
    if let Some(env) = env {
        if let Some(color) = env.ambient_color {
            ambient.color = srgba(color);
        }
        if let Some(brightness) = env.ambient_intensity {
            ambient.brightness = brightness;
        }
    }
    ambient
}

/// The clear colour for a manifest environment.
pub fn clear_color(env: Option<&wt::EnvironmentDef>) -> ClearColor {
    env.and_then(|e| e.background_color)
        .map_or_else(ClearColor::default, |c| ClearColor(srgba(c)))
}

/// The camera's distance fog for a manifest environment: exponential in
/// `fog_density`, coloured by `fog_color`, else the background, else white.
/// `None` when the environment has no fog.
pub fn distance_fog(env: Option<&wt::EnvironmentDef>) -> Option<DistanceFog> {
    let env = env?;
    let density = env.fog_density.filter(|d| *d > 0.0)?;
    let color = env.fog_color.or(env.background_color);
    Some(DistanceFog {
        color: color.map_or(Color::WHITE, srgba),
        falloff: FogFalloff::Exponential { density },
        ..default()
    })
}

/// The camera projection for a manifest camera (45° when unset, the
/// format's default).
pub fn perspective(camera: Option<&wt::CameraDef>) -> PerspectiveProjection {
    let fov = camera.map_or(wt::CameraDef::default().fov_degrees, |c| c.fov_degrees);
    PerspectiveProjection {
        fov: fov.to_radians(),
        ..default()
    }
}

/// The camera transform for a manifest camera, falling back to the avatar
/// spawn, then the format's default.
pub fn camera_transform(manifest: &wt::WorldManifest) -> Transform {
    let (position, look_at) = match (&manifest.camera, &manifest.avatar) {
        (Some(camera), _) => (camera.position, camera.look_at),
        (None, Some(avatar)) => (avatar.spawn_position, avatar.spawn_look_at),
        (None, None) => {
            let camera = wt::CameraDef::default();
            (camera.position, camera.look_at)
        }
    };
    Transform::from_translation(Vec3::from_array(position))
        .looking_at(Vec3::from_array(look_at), Vec3::Y)
}

/// A mesh for every [`Shape`](wt::Shape), centered on its origin.
///
/// Bevy has no pyramid or wedge primitive: the pyramid is built flat-shaded
/// with its base at `-height/2`, the wedge is a right-triangle profile in XY
/// (vertical face at `-x`, slope down toward `+x`) extruded along Z.
pub fn shape_mesh(shape: &wt::Shape) -> Mesh {
    match *shape {
        wt::Shape::Cuboid { x, y, z } => Cuboid::new(x, y, z).into(),
        wt::Shape::Sphere { radius } => Sphere::new(radius).mesh().uv(32, 18),
        wt::Shape::Cylinder { radius, height } => Cylinder::new(radius, height).into(),
        wt::Shape::Cone { radius, height } => Cone::new(radius, height).into(),
        wt::Shape::Capsule {
            radius,
            half_length,
        } => Capsule3d::new(radius, half_length * 2.0).into(),
        wt::Shape::Torus {
            major_radius,
            minor_radius,
        } => Torus {
            minor_radius,
            major_radius,
        }
        .into(),
        wt::Shape::Plane { x, z } => Plane3d::default().mesh().size(x, z).into(),
        wt::Shape::Pyramid {
            base_x,
            base_z,
            height,
        } => pyramid_mesh(base_x, base_z, height),
        // Bevy's default tetrahedron has a circumradius of √0.75.
        wt::Shape::Tetrahedron { radius } => {
            Mesh::from(Tetrahedron::default()).scaled_by(Vec3::splat(radius / 0.75_f32.sqrt()))
        }
        wt::Shape::Icosahedron { radius } => Sphere::new(radius)
            .mesh()
            .ico(0)
            .unwrap_or_else(|_| Sphere::new(radius).mesh().uv(8, 6)),
        wt::Shape::Wedge { x, y, z } => {
            let (hx, hy) = (x / 2.0, y / 2.0);
            let profile =
                Triangle2d::new(Vec2::new(-hx, -hy), Vec2::new(hx, -hy), Vec2::new(-hx, hy));
            Extrusion::new(profile, z).into()
        }
    }
}

/// A flat-shaded triangle-list mesh from explicit triangles.
fn flat_mesh(triangles: &[[[f32; 3]; 3]]) -> Mesh {
    let mut positions = Vec::with_capacity(triangles.len() * 3);
    let mut normals = Vec::with_capacity(triangles.len() * 3);
    let mut uvs = Vec::with_capacity(triangles.len() * 3);
    let mut indices = Vec::with_capacity(triangles.len() * 3);
    for tri in triangles {
        let [a, b, c] = *tri;
        let e1 = Vec3::from_array(b) - Vec3::from_array(a);
        let e2 = Vec3::from_array(c) - Vec3::from_array(a);
        let normal = e1.cross(e2).normalize_or(Vec3::Y).to_array();
        let base = positions.len() as u32;
        positions.extend([a, b, c]);
        normals.extend([normal; 3]);
        uvs.extend([[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]]);
        indices.extend([base, base + 1, base + 2]);
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// A rectangular pyramid centered on the origin: base at `-height/2`, apex
/// at `+height/2`.
fn pyramid_mesh(base_x: f32, base_z: f32, height: f32) -> Mesh {
    let (hx, hz, hy) = (base_x / 2.0, base_z / 2.0, height / 2.0);
    let a = [-hx, -hy, -hz];
    let b = [hx, -hy, -hz];
    let c = [hx, -hy, hz];
    let d = [-hx, -hy, hz];
    let apex = [0.0, hy, 0.0];
    flat_mesh(&[
        [a, b, apex],
        [b, c, apex],
        [c, d, apex],
        [d, a, apex],
        [a, d, b],
        [b, d, c],
    ])
}

/// Rotate a pyramid so its base edges align with the axes when it is built
/// from a four-sided cone instead (kept for callers that want Bevy's cone).
pub fn pyramid_from_cone(base_x: f32, base_z: f32, height: f32) -> Mesh {
    Mesh::from(
        Cone::new(std::f32::consts::FRAC_1_SQRT_2, height)
            .mesh()
            .resolution(4),
    )
    .rotated_by(Quat::from_rotation_y(FRAC_PI_4))
    .scaled_by(Vec3::new(base_x, 1.0, base_z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::primitives::MeshAabb;

    fn half_extents(mesh: &Mesh) -> [f32; 3] {
        let aabb = mesh.compute_aabb().expect("positions");
        aabb.half_extents.to_array()
    }

    fn shapes() -> Vec<wt::Shape> {
        vec![
            wt::Shape::Cuboid {
                x: 1.5,
                y: 2.0,
                z: 0.5,
            },
            wt::Shape::Sphere { radius: 0.75 },
            wt::Shape::Cylinder {
                radius: 0.6,
                height: 1.5,
            },
            wt::Shape::Cone {
                radius: 0.7,
                height: 1.5,
            },
            wt::Shape::Capsule {
                radius: 0.4,
                half_length: 0.45,
            },
            wt::Shape::Torus {
                major_radius: 0.6,
                minor_radius: 0.2,
            },
            wt::Shape::Plane { x: 4.0, z: 2.0 },
            wt::Shape::Pyramid {
                base_x: 1.5,
                base_z: 1.0,
                height: 2.0,
            },
            wt::Shape::Tetrahedron { radius: 0.9 },
            wt::Shape::Icosahedron { radius: 0.8 },
            wt::Shape::Wedge {
                x: 1.5,
                y: 1.0,
                z: 2.0,
            },
        ]
    }

    /// Every mesh is centered and fits the bounds the format declares, and
    /// the box-like shapes fill them exactly.
    #[test]
    fn meshes_match_declared_bounds() {
        for shape in shapes() {
            let mesh = shape_mesh(&shape);
            let aabb = mesh.compute_aabb().unwrap();
            let center = aabb.center.to_array();
            let half = half_extents(&mesh);
            let declared = shape.local_aabb_half();
            let tight = !matches!(
                shape,
                wt::Shape::Tetrahedron { .. } | wt::Shape::Icosahedron { .. }
            );
            for axis in 0..3 {
                assert!(
                    center[axis].abs() < 1e-4,
                    "{shape:?}: not centered on axis {axis}: {center:?}"
                );
                assert!(
                    half[axis] <= declared[axis] + 1e-3,
                    "{shape:?}: axis {axis} extent {} exceeds declared {}",
                    half[axis],
                    declared[axis]
                );
                if tight {
                    assert!(
                        (half[axis] - declared[axis]).abs() < 1e-3,
                        "{shape:?}: axis {axis} extent {} != declared {}",
                        half[axis],
                        declared[axis]
                    );
                }
            }
        }
    }

    #[test]
    fn pyramid_and_wedge_topology() {
        let pyramid = shape_mesh(&wt::Shape::Pyramid {
            base_x: 1.0,
            base_z: 1.0,
            height: 1.0,
        });
        assert_eq!(pyramid.indices().unwrap().len(), 6 * 3);
        let wedge = shape_mesh(&wt::Shape::Wedge {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        });
        assert!(wedge.count_vertices() > 0);
    }

    #[test]
    fn transform_round_trips_euler_degrees() {
        let t = wt::WorldTransform {
            position: [1.0, 2.0, 3.0],
            rotation_degrees: [10.0, 20.0, 30.0],
            scale: [2.0, 2.0, 2.0],
            visible: true,
        };
        let back = world_transform(&transform(&t), true);
        for axis in 0..3 {
            assert!((back.position[axis] - t.position[axis]).abs() < 1e-5);
            assert!((back.rotation_degrees[axis] - t.rotation_degrees[axis]).abs() < 1e-3);
            assert!((back.scale[axis] - t.scale[axis]).abs() < 1e-5);
        }
        assert_eq!(visibility(&t), Visibility::Inherited);
    }

    #[test]
    fn lights_aim_along_direction() {
        let light = wt::LightDef {
            light_type: wt::LightType::Spot,
            direction: Some([1.0, 0.0, 0.0]),
            ..Default::default()
        };
        let t = light_transform(&light, Transform::from_xyz(0.0, 5.0, 0.0));
        assert!((t.forward().as_vec3() - Vec3::X).length() < 1e-5);
        assert_eq!(t.translation, Vec3::new(0.0, 5.0, 0.0));

        let point = wt::LightDef {
            light_type: wt::LightType::Point,
            ..Default::default()
        };
        let t = light_transform(&point, Transform::from_xyz(1.0, 1.0, 1.0));
        assert_eq!(t.rotation, Quat::IDENTITY);
    }

    #[test]
    fn material_mapping() {
        let def = wt::MaterialDef {
            color: [0.5, 0.25, 0.125, 0.5],
            metallic: 0.7,
            roughness: 0.2,
            emissive: [1.0, 2.0, 3.0, 1.0],
            alpha_mode: Some(wt::AlphaModeDef::Mask(0.3)),
            unlit: Some(true),
            double_sided: Some(true),
            reflectance: Some(0.9),
        };
        let m = standard_material(&def);
        assert_eq!(m.base_color, Color::srgba(0.5, 0.25, 0.125, 0.5));
        assert_eq!(m.metallic, 0.7);
        assert_eq!(m.perceptual_roughness, 0.2);
        assert_eq!(m.emissive, LinearRgba::new(1.0, 2.0, 3.0, 1.0));
        assert_eq!(m.alpha_mode, AlphaMode::Mask(0.3));
        assert!(m.unlit);
        assert!(m.double_sided);
        assert_eq!(m.reflectance, 0.9);
        assert_eq!(alpha_mode_def(AlphaMode::Add), wt::AlphaModeDef::Add);
    }

    #[test]
    fn environment_mapping() {
        let env = wt::EnvironmentDef {
            background_color: Some([0.1, 0.2, 0.3, 1.0]),
            ambient_intensity: Some(350.0),
            ambient_color: Some([0.9, 0.9, 1.0, 1.0]),
            fog_density: Some(0.02),
            fog_color: None,
        };
        let ambient = ambient_light(Some(&env));
        assert_eq!(ambient.brightness, 350.0);
        assert_eq!(clear_color(Some(&env)).0, Color::srgba(0.1, 0.2, 0.3, 1.0));
        let fog = distance_fog(Some(&env)).unwrap();
        assert_eq!(fog.color, Color::srgba(0.1, 0.2, 0.3, 1.0));
        assert!(distance_fog(None).is_none());
        assert!((perspective(None).fov - 45.0_f32.to_radians()).abs() < 1e-6);
    }
}
