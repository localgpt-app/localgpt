//! The camera-*entity* view: a camera carried by an entity
//! (`ext-cinematography.camera`, see [`wt::cinematography`]) turned
//! into a Bevy camera — where the scene-wide [`crate::perspective`]
//! maps the one `CameraDef` a manifest starts with, this maps any
//! number of setups: "12A, 24 mm on Super 35, cropped to 2.39".
//!
//! [`shot_camera`] is the pure mapping, so apps (Gen's live canvas
//! look-through, thumbnails) and tests share it; [`letterbox`] is the
//! viewport math for looking through a frame whose aspect is not the
//! window's, the way the extension's JS reference letterboxes the
//! canvas. The projection's vertical FOV and the frame aspect come
//! from the extension's derived frame, so every renderer agrees on
//! the numbers; Bevy's `camera_system` re-derives the projection's
//! aspect from the letterboxed viewport each frame.

use bevy::prelude::*;
use localgpt_world_types as wt;
use wt::cinematography::{self, CameraFrame};

/// A camera entity as a Bevy camera: the transform to place, the
/// projection to look through, and the frame it came from.
#[derive(Debug, Clone)]
pub struct ShotCamera {
    /// Where the camera stands and which way it looks (the entity's
    /// transform; `aim` overrides which way).
    pub transform: Transform,
    /// Vertical FOV from the extension's derived frame. The aspect is
    /// the frame's, but Bevy's `camera_system` overwrites it from the
    /// render target each frame — letterbox the viewport to
    /// [`Self::frame`]'s aspect to hold it.
    pub projection: PerspectiveProjection,
    /// The derived frame (crop-to-aspect already applied).
    pub frame: CameraFrame,
}

/// The Bevy camera for a camera entity: `None` when the entity carries
/// no `ext-cinematography.camera`. Placed by the entity's transform —
/// or, when the component carries an `aim`, looking at that world
/// point, +Y up — with the vertical FOV and frame aspect of the
/// extension's derived frame.
pub fn shot_camera(entity: &wt::WorldEntity) -> Option<ShotCamera> {
    let component = cinematography::CameraComponent::of(entity)?;
    let frame = cinematography::frame_of(&component);
    let mut transform = crate::transform(&entity.transform);
    if let Some(aim) = component.aim {
        transform.look_at(Vec3::from_array(aim.map(|v| v as f32)), Vec3::Y);
    }
    Some(ShotCamera {
        transform,
        projection: PerspectiveProjection {
            fov: (frame.vfov_degrees as f32).to_radians(),
            aspect_ratio: frame.aspect as f32,
            ..default()
        },
        frame,
    })
}

/// The viewport rect — physical position and size — that letterboxes a
/// frame of `frame_aspect` inside a target of `target` pixels: the
/// largest rect of the frame's aspect, centered, black bars on the
/// sides it doesn't fill. `None` for a degenerate target or frame, or
/// when the frame fills the target already (no viewport to set).
pub fn letterbox(target: UVec2, frame_aspect: f32) -> Option<(UVec2, UVec2)> {
    if target.x == 0 || target.y == 0 || frame_aspect.is_nan() || frame_aspect <= 0.0 {
        return None;
    }
    let target_aspect = target.x as f32 / target.y as f32;
    if (target_aspect - frame_aspect).abs() < 1e-3 {
        return None;
    }
    let (w, h) = if target_aspect > frame_aspect {
        // Too wide: bars left and right.
        ((target.y as f32 * frame_aspect).round() as u32, target.y)
    } else {
        // Too tall: bars top and bottom.
        (target.x, (target.x as f32 / frame_aspect).round() as u32)
    };
    if w == 0 || h == 0 {
        return None;
    }
    Some((
        UVec2::new((target.x - w) / 2, (target.y - h) / 2),
        UVec2::new(w, h),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A camera entity: `transform` as written, `ext` the
    /// ext-cinematography block.
    fn entity(
        name: &str,
        position: [f32; 3],
        rotation: [f32; 3],
        ext: serde_json::Value,
    ) -> wt::WorldEntity {
        let mut entity = wt::WorldEntity::new(1, name);
        entity.transform = wt::WorldTransform {
            position,
            rotation_degrees: rotation,
            ..Default::default()
        };
        entity
            .extra
            .insert(cinematography::EXTENSION_NAME.to_string(), ext);
        entity
    }

    #[test]
    fn a_non_camera_entity_has_no_shot_camera() {
        let entity = wt::WorldEntity::new(1, "crate");
        assert!(shot_camera(&entity).is_none());
    }

    #[test]
    fn a_24_on_super_35_cropped_to_239_has_the_conformance_numbers() {
        // The extension's worked case: hfov ≈ 54.8172°, vfov ≈ 24.4827°.
        let entity = entity(
            "12A",
            [0.0, 1.6, 6.0],
            [0.0, 0.0, 0.0],
            json!({"camera": {"focal_length_mm": 24, "aspect_ratio": 2.39}}),
        );
        let shot = shot_camera(&entity).expect("a camera entity");
        assert!((shot.frame.hfov_degrees - 54.8172).abs() < 1e-3);
        assert!((shot.frame.vfov_degrees - 24.4827).abs() < 1e-3);
        assert!((shot.frame.aspect - 2.39).abs() < 1e-6);
        assert!((shot.projection.fov.to_degrees() - 24.4827).abs() < 1e-2);
        // Placed by the entity's transform; no aim, so it keeps the
        // entity's rotation (looking down local −Z here).
        assert_eq!(shot.transform.translation, Vec3::new(0.0, 1.6, 6.0));
        assert!(shot.transform.forward().dot(Vec3::NEG_Z) > 0.9999);
    }

    #[test]
    fn cropping_trims_and_never_widens_the_frame() {
        // No aspect: the whole sensor, and the widest frame.
        let bare = entity("a", [0.0; 3], [0.0; 3], json!({"camera": {}}));
        let bare = shot_camera(&bare).unwrap();
        assert!((bare.frame.width_mm - 24.89).abs() < 1e-9);
        // A 2.39 crop is shorter, never wider; a taller-than-sensor
        // aspect is narrower, never taller.
        let cropped = entity(
            "b",
            [0.0; 3],
            [0.0; 3],
            json!({"camera": {"aspect_ratio": 2.39}}),
        );
        let cropped = shot_camera(&cropped).unwrap();
        assert_eq!(cropped.frame.width_mm, bare.frame.width_mm);
        assert!(cropped.frame.height_mm < bare.frame.height_mm);
        let tall = entity(
            "c",
            [0.0; 3],
            [0.0; 3],
            json!({"camera": {"aspect_ratio": 1.0}}),
        );
        let tall = shot_camera(&tall).unwrap();
        assert_eq!(tall.frame.height_mm, bare.frame.height_mm);
        assert!(tall.frame.width_mm < bare.frame.width_mm);
    }

    #[test]
    fn the_aim_overrides_the_entity_rotation_and_centers_the_view() {
        let aim = [0.7, 1.6, 0.0];
        let entity = entity(
            "1B",
            [0.7, 1.6, 2.0],
            // A written rotation the aim must beat.
            [0.0, 90.0, 0.0],
            json!({"camera": {"focal_length_mm": 50, "aim": aim}}),
        );
        let shot = shot_camera(&entity).unwrap();
        let forward = shot.transform.forward().as_vec3();
        let want = (Vec3::from_array(aim) - Vec3::new(0.7, 1.6, 2.0)).normalize();
        assert!(
            (forward - want).length() < 1e-5,
            "forward {forward:?} != aim direction {want:?}"
        );
        // +Y up: no roll.
        assert!(shot.transform.up().dot(Vec3::Y) > 0.9999);
    }

    #[test]
    fn letterboxing_centers_the_largest_rect_of_the_frame_aspect() {
        // A 2.39 frame in a 16:9 window: bars top and bottom.
        let (pos, size) = letterbox(UVec2::new(1920, 1080), 2.39).unwrap();
        assert_eq!(size.x, 1920);
        assert_eq!(size.y, (1920.0_f32 / 2.39).round() as u32);
        assert_eq!(pos.x, 0);
        assert_eq!(pos.y, (1080 - size.y) / 2);
        // A square frame in the same window: bars left and right.
        let (pos, size) = letterbox(UVec2::new(1920, 1080), 1.0).unwrap();
        assert_eq!(size, UVec2::new(1080, 1080));
        assert_eq!(pos, UVec2::new((1920 - 1080) / 2, 0));
        // A matching aspect needs no viewport; a degenerate one gets none.
        assert!(letterbox(UVec2::new(2390, 1000), 2.39).is_none());
        assert!(letterbox(UVec2::new(0, 1080), 2.39).is_none());
        assert!(letterbox(UVec2::new(1920, 1080), 0.0).is_none());
        assert!(letterbox(UVec2::new(1920, 1080), -1.0).is_none());
    }
}
