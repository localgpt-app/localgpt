//! The `ext-cinematography` math, kept here (the crate that re-exports
//! the format) until the pinned `openworldformat` ships a typed
//! cinematography module — the format's spec repo already has one, so
//! when the workspace bumps to it this module's surface delegates to
//! the crate's and the paths keep working. This is the normative
//! surface of `openworldformat/spec/extensions/cinematography.md` —
//! the crop math (Unreal's crop-to-aspect rule: cropping never widens
//! the frame past the sensor, it trims), the aim look-at (+Y up), and
//! a pinhole projection — matching the reference implementations
//! (`rust/src/cinematography.rs`, `js/src/cinematography.js`) so the
//! numbers agree. Cameras ride `WorldEntity::extra` as plain JSON.
//!
//! Moved here from `localgpt-previs` (`crates/previs`) so every
//! renderer of a camera entity — the previs board, Gen's live canvas,
//! anything next — derives the same numbers from one place.

use serde::{Deserialize, Serialize};

use crate as wt;

/// The extension key under `WorldEntity::extra`.
pub const EXTENSION_NAME: &str = "ext-cinematography";

/// The default sensor (filmback): Super 35, `[w, h]` in mm.
pub const DEFAULT_SENSOR: [f64; 2] = [24.89, 18.66];

/// The default focal length, in mm.
pub const DEFAULT_FOCAL_LENGTH: f64 = 35.0;

/// One camera's `ext-cinematography.camera` component with every default
/// applied (the spec's `{}` is a 35 mm on Super 35).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraComponent {
    /// Sensor (filmback) `[w, h]`, mm.
    pub sensor_mm: [f64; 2],
    /// The lens, mm.
    pub focal_length_mm: f64,
    /// Crop to this frame aspect (w/h); absent keeps the desqueezed
    /// sensor's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<f64>,
    /// Anamorphic desqueeze.
    pub squeeze: f64,
    /// Look at this world point, +Y up; absent looks down the entity's
    /// local −Z.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aim: Option<[f64; 3]>,
}

impl Default for CameraComponent {
    fn default() -> Self {
        Self {
            sensor_mm: DEFAULT_SENSOR,
            focal_length_mm: DEFAULT_FOCAL_LENGTH,
            aspect_ratio: None,
            squeeze: 1.0,
            aim: None,
        }
    }
}

impl CameraComponent {
    /// Parse the component from an entity's `extra` map, defaults per
    /// absent field. `None` when the entity declares no camera.
    pub fn of(entity: &wt::WorldEntity) -> Option<Self> {
        let camera = entity
            .extra
            .get(EXTENSION_NAME)?
            .get("camera")?
            .as_object()?;
        let num = |k: &str, d: f64| {
            camera
                .get(k)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(d)
        };
        let vec2 = |k: &str| -> Option<[f64; 2]> {
            let a: Vec<f64> = camera
                .get(k)?
                .as_array()?
                .iter()
                .filter_map(serde_json::Value::as_f64)
                .collect();
            (a.len() == 2).then_some([a[0], a[1]])
        };
        let vec3 = |k: &str| -> Option<[f64; 3]> {
            let a: Vec<f64> = camera
                .get(k)?
                .as_array()?
                .iter()
                .filter_map(serde_json::Value::as_f64)
                .collect();
            (a.len() == 3).then_some([a[0], a[1], a[2]])
        };
        Some(CameraComponent {
            sensor_mm: vec2("sensor_mm").unwrap_or(DEFAULT_SENSOR),
            focal_length_mm: num("focal_length_mm", DEFAULT_FOCAL_LENGTH),
            aspect_ratio: camera
                .get("aspect_ratio")
                .and_then(serde_json::Value::as_f64),
            squeeze: num("squeeze", 1.0),
            aim: vec3("aim"),
        })
    }
}

/// A camera's frame: the largest rectangle of the aspect inside the
/// desqueezed sensor, centred, and the fields of view it gives the lens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraFrame {
    /// Frame width (desqueezed), mm.
    pub width_mm: f64,
    /// Frame height, mm.
    pub height_mm: f64,
    /// Horizontal field of view, degrees.
    pub hfov_degrees: f64,
    /// Vertical field of view, degrees.
    pub vfov_degrees: f64,
    /// Frame width / height.
    pub aspect: f64,
}

/// The normative crop math (spec "Derived values"): with sensor `w × h`,
/// squeeze `s`, focal length `f` and aspect `a` — desqueezed sensor
/// aspect `A = w·s / h`; frame `W = w·s · min(1, a/A)`,
/// `H = h · min(1, A/a)` (no aspect: the whole desqueezed sensor); FOVs
/// `2·atan(W / 2f)`, `2·atan(H / 2f)`.
pub fn frame_of(camera: &CameraComponent) -> CameraFrame {
    let [w, h] = camera.sensor_mm;
    let s = camera.squeeze;
    let f = camera.focal_length_mm;
    let a = w * s / h;
    let (width_mm, height_mm) = match camera.aspect_ratio {
        Some(aspect) if aspect != 0.0 => (w * s * (aspect / a).min(1.0), h * (a / aspect).min(1.0)),
        _ => (w * s, h),
    };
    let deg = |rad: f64| rad * 180.0 / std::f64::consts::PI;
    CameraFrame {
        width_mm,
        height_mm,
        hfov_degrees: deg(2.0 * (width_mm / (2.0 * f)).atan()),
        vfov_degrees: deg(2.0 * (height_mm / (2.0 * f)).atan()),
        aspect: width_mm / height_mm,
    }
}

/// A camera's view: where it stands and which way it looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraView {
    /// World point.
    pub position: [f64; 3],
    /// Unit vector, the look direction.
    pub forward: [f64; 3],
    /// Unit vector.
    pub right: [f64; 3],
    /// Unit vector.
    pub up: [f64; 3],
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = dot(v, v).sqrt();
    [v[0] / l, v[1] / l, v[2] / l]
}

/// A camera's view from a position and a component: with `aim`, the
/// camera looks at the world point, +Y up; without it, it looks down
/// the entity's local −Z (not needed by the draft — every staged camera
/// carries an aim — but part of the extension's contract).
pub fn view_of(position: [f64; 3], camera: &CameraComponent) -> CameraView {
    let aim = camera.aim.expect("a staged camera carries an aim");
    let forward = norm(sub(aim, position));
    let right = norm(cross(forward, [0.0, 1.0, 0.0]));
    let up = cross(right, forward);
    CameraView {
        position,
        forward,
        right,
        up,
    }
}

/// A world point in a camera's normalized frame (see [`project`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    /// Frame-half units; inside when `|x| <= 1`.
    pub x: f64,
    /// Frame-half units; inside when `|y| <= 1`.
    pub y: f64,
    /// Distance along the look direction; in front when positive.
    pub z: f64,
}

impl Projection {
    /// The extension's `in_frame` predicate.
    pub fn in_frame(&self) -> bool {
        self.z > 0.0 && self.x.abs() <= 1.0 && self.y.abs() <= 1.0
    }
}

/// Project a world point through a camera: `x` and `y` in frame-half
/// units (inside when `|x| <= 1` and `|y| <= 1`), `z` the distance along
/// the look direction (in front when positive).
pub fn project(
    view: &CameraView,
    frame: &CameraFrame,
    camera: &CameraComponent,
    point: [f64; 3],
) -> Projection {
    let d = sub(point, view.position);
    let z = dot(d, view.forward);
    let scale = camera.focal_length_mm / z;
    Projection {
        x: (dot(d, view.right) * scale) / (frame.width_mm / 2.0),
        y: (dot(d, view.up) * scale) / (frame.height_mm / 2.0),
        z,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_normative_crop_math_trims_never_widens() {
        // Super 35 with no aspect is the whole sensor.
        let full = frame_of(&CameraComponent::default());
        assert!((full.aspect - 24.89 / 18.66).abs() < 1e-9);
        // A 2.39 crop on Super 35 trims the height only.
        let cropped = frame_of(&CameraComponent {
            focal_length_mm: 24.0,
            aspect_ratio: Some(2.39),
            ..CameraComponent::default()
        });
        assert_eq!(cropped.width_mm, 24.89);
        assert!(cropped.height_mm < 18.66);
        assert!((cropped.aspect - 2.39).abs() < 1e-9);
        // The 2× anamorphic desqueezes, then crops the width.
        let ana = frame_of(&CameraComponent {
            aspect_ratio: Some(2.39),
            squeeze: 2.0,
            ..CameraComponent::default()
        });
        assert!(ana.width_mm < 24.89 * 2.0);
        assert_eq!(ana.height_mm, 18.66);
        assert!((ana.aspect - 2.39).abs() < 1e-9);
    }

    #[test]
    fn the_aim_point_projects_to_the_frame_center() {
        let camera = CameraComponent {
            focal_length_mm: 50.0,
            aspect_ratio: Some(2.39),
            aim: Some([0.7, 1.6, 0.0]),
            ..CameraComponent::default()
        };
        let view = view_of([0.7, 1.6, 2.0], &camera);
        let frame = frame_of(&camera);
        let p = project(&view, &frame, &camera, [0.7, 1.6, 0.0]);
        assert!(p.x.abs() < 1e-9 && p.y.abs() < 1e-9 && p.z > 0.0);
        assert!(p.in_frame());
    }
}
