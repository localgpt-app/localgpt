//! The `ext-cinematography` extension's typed surface: the
//! `openworldformat` crate's `cinematography` module — the normative
//! crop math, the aim look-at, the pinhole projection, the shot list
//! and the conformance outcome runner — re-exported so
//! `wt::cinematography` keeps working, plus what LocalGPT's *writers*
//! need that a reader's reference doesn't carry: a default component to
//! build on, the component as the JSON it rides `WorldEntity::extra`
//! as, and a view from a bare position.
//!
//! Until the workspace pinned `openworldformat` 0.3.4 this module held
//! a copy of the math; the copy is gone, so every renderer of a camera
//! entity — the previs board, Gen's live canvas, the spec's own
//! references — derives the same numbers from one place.

use serde_json::json;

use crate as wt;

pub use openworldformat::cinematography::*;

/// A camera with every default applied — the spec's `{}`: a 35 mm on
/// Super 35, no crop, no squeeze, no aim. Build on it with struct
/// update syntax: `CameraComponent { focal_length_mm: 24.0,
/// ..default_camera() }`.
pub fn default_camera() -> CameraComponent {
    CameraComponent {
        sensor_mm: DEFAULT_SENSOR,
        focal_length_mm: DEFAULT_FOCAL_LENGTH,
        aspect_ratio: None,
        squeeze: 1.0,
        aim: None,
        focus_distance_m: None,
        f_stop: None,
    }
}

/// The component as the JSON an entity's `ext-cinematography.camera`
/// carries: the fields with defaults always written, the optional ones
/// only when set. [`CameraComponent::of`] reads it back equal.
pub fn camera_json(camera: &CameraComponent) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("sensor_mm".into(), json!(camera.sensor_mm));
    map.insert("focal_length_mm".into(), json!(camera.focal_length_mm));
    map.insert("squeeze".into(), json!(camera.squeeze));
    if let Some(aspect) = camera.aspect_ratio {
        map.insert("aspect_ratio".into(), json!(aspect));
    }
    if let Some(aim) = camera.aim {
        map.insert("aim".into(), json!(aim));
    }
    if let Some(distance) = camera.focus_distance_m {
        map.insert("focus_distance_m".into(), json!(distance));
    }
    if let Some(f_stop) = camera.f_stop {
        map.insert("f_stop".into(), json!(f_stop));
    }
    serde_json::Value::Object(map)
}

/// Whether a projected point is in frame: in front of the camera and
/// inside both frame halves — the `in_frame` assertion's predicate,
/// as a function on [`Projection`] (the crate's is a plain record).
pub fn in_frame(p: &Projection) -> bool {
    p.z > 0.0 && p.x.abs() <= 1.0 && p.y.abs() <= 1.0
}

/// The view from a bare position — the staging loop places a camera
/// before it is an entity. With an `aim` the look-at is the entity's;
/// without one the camera looks down −Z (an unrotated entity). The
/// position goes through the format's `f32`, as it will once written.
pub fn view_from(position: [f64; 3], camera: &CameraComponent) -> CameraView {
    let mut scratch = wt::WorldEntity::new(0, "camera");
    scratch.transform.position = position.map(|p| p as f32);
    view_of(&scratch, camera)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity_with(camera: &CameraComponent, position: [f32; 3]) -> wt::WorldEntity {
        let mut entity = wt::WorldEntity::new(7, "12A");
        entity.transform.position = position;
        entity.extra.insert(
            EXTENSION_NAME.to_string(),
            json!({ "camera": camera_json(camera) }),
        );
        entity
    }

    #[test]
    fn the_default_is_the_specs_empty_component() {
        let bare = wt::WorldEntity::new(1, "bare");
        let mut bare = bare;
        bare.extra
            .insert(EXTENSION_NAME.to_string(), json!({ "camera": {} }));
        assert_eq!(CameraComponent::of(&bare).unwrap(), default_camera());
    }

    #[test]
    fn camera_json_reads_back_equal_with_and_without_the_optionals() {
        let plain = default_camera();
        assert_eq!(
            CameraComponent::of(&entity_with(&plain, [0.0; 3])).unwrap(),
            plain
        );
        let json = camera_json(&plain);
        assert!(json.get("aim").is_none() && json.get("aspect_ratio").is_none());

        let full = CameraComponent {
            focal_length_mm: 24.0,
            aspect_ratio: Some(2.39),
            aim: Some([0.0, 1.5, 0.0]),
            focus_distance_m: Some(6.0),
            f_stop: Some(2.8),
            ..default_camera()
        };
        assert_eq!(
            CameraComponent::of(&entity_with(&full, [0.0; 3])).unwrap(),
            full
        );
    }

    #[test]
    fn in_frame_wants_the_point_in_front_and_inside_both_halves() {
        let inside = Projection { x: 0.5, y: -0.9, z: 3.0 };
        assert!(in_frame(&inside));
        assert!(!in_frame(&Projection { x: 1.01, ..inside }));
        assert!(!in_frame(&Projection { y: -1.01, ..inside }));
        assert!(!in_frame(&Projection { z: -3.0, ..inside }));
        assert!(in_frame(&Projection { x: 1.0, y: 1.0, z: 0.01 }));
    }

    #[test]
    fn a_view_from_a_position_is_the_entitys_view() {
        let camera = CameraComponent {
            aim: Some([0.0, 1.5, 0.0]),
            ..default_camera()
        };
        let position = [1.25, 1.6, 6.5];
        let entity = entity_with(&camera, position);
        let from = view_from(position.map(|p| p as f64), &camera);
        let of = view_of(&entity, &camera);
        assert_eq!(from.position, of.position);
        assert_eq!(from.forward, of.forward);
        assert_eq!(from.right, of.right);
        assert_eq!(from.up, of.up);
    }
}
