//! Declarative behavior definitions — data-driven entity animations.
//!
//! All 7 behavior types from the current gen crate are preserved exactly.
//! The only structural change is that entity references use [`EntityRef`]
//! instead of raw `String`.

use serde::{Deserialize, Serialize};

use crate::identity::EntityRef;

/// Declarative behavior definition — data, not code.
/// Each variant fully describes a continuous animation that the tick system evaluates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BehaviorDef {
    /// Orbit around a center entity or point.
    Orbit {
        /// Entity to orbit around (mutually exclusive with `center_point`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        center: Option<EntityRef>,
        /// Fixed point to orbit around [x,y,z] (used if `center` is None).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        center_point: Option<[f32; 3]>,
        /// Orbit radius.
        #[serde(default = "default_orbit_radius")]
        radius: f32,
        /// Orbital speed in degrees per second.
        #[serde(default = "default_orbit_speed")]
        speed: f32,
        /// Orbit axis (normalized). Default: Y-up.
        #[serde(default = "default_y_axis")]
        axis: [f32; 3],
        /// Initial phase angle in degrees.
        #[serde(default)]
        phase: f32,
        /// Orbit tilt in degrees (inclination from the axis plane).
        #[serde(default)]
        tilt: f32,
    },
    /// Spin (rotate) around a local axis.
    Spin {
        /// Local axis to spin around.
        #[serde(default = "default_y_axis")]
        axis: [f32; 3],
        /// Rotation speed in degrees per second.
        #[serde(default = "default_spin_speed")]
        speed: f32,
    },
    /// Bob up and down (sinusoidal oscillation along an axis).
    Bob {
        /// Axis of oscillation.
        #[serde(default = "default_y_axis")]
        axis: [f32; 3],
        /// Amplitude (distance from center in each direction).
        #[serde(default = "default_bob_amplitude")]
        amplitude: f32,
        /// Oscillation frequency in Hz.
        #[serde(default = "default_bob_frequency")]
        frequency: f32,
        /// Phase offset in degrees.
        #[serde(default)]
        phase: f32,
    },
    /// Continuously look at / follow another entity.
    LookAt {
        /// Entity to look at.
        target: EntityRef,
    },
    /// Scale pulsation (breathing effect).
    Pulse {
        /// Minimum scale multiplier.
        #[serde(default = "default_pulse_min")]
        min_scale: f32,
        /// Maximum scale multiplier.
        #[serde(default = "default_pulse_max")]
        max_scale: f32,
        /// Pulse frequency in Hz.
        #[serde(default = "default_bob_frequency")]
        frequency: f32,
    },
    /// Follow a path of waypoints in sequence.
    PathFollow {
        /// Ordered waypoints [[x,y,z], ...].
        waypoints: Vec<[f32; 3]>,
        /// Movement speed in units per second.
        #[serde(default = "default_path_speed")]
        speed: f32,
        /// Loop mode: "loop" wraps back to start, "ping_pong" reverses.
        #[serde(default = "default_path_mode")]
        mode: PathMode,
        /// Smoothly interpolate rotation toward movement direction.
        #[serde(default)]
        orient_to_path: bool,
    },
    /// Bouncing on a surface with gravity.
    Bounce {
        /// Height of initial/max bounce.
        #[serde(default = "default_bounce_height")]
        height: f32,
        /// Gravity acceleration (units/s^2).
        #[serde(default = "default_bounce_gravity")]
        gravity: f32,
        /// Energy retained per bounce (0.0-1.0).
        #[serde(default = "default_bounce_damping")]
        damping: f32,
        /// Surface Y level to bounce on.
        #[serde(default)]
        surface_y: f32,
    },
}

/// Path follow loop mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathMode {
    #[default]
    Loop,
    PingPong,
    Once,
}

// ---- Default value helpers (match current gen crate exactly) ----

pub(crate) fn default_orbit_radius() -> f32 {
    5.0
}
pub(crate) fn default_orbit_speed() -> f32 {
    36.0
}
pub(crate) fn default_y_axis() -> [f32; 3] {
    [0.0, 1.0, 0.0]
}
pub(crate) fn default_spin_speed() -> f32 {
    90.0
}
pub(crate) fn default_bob_amplitude() -> f32 {
    0.5
}
pub(crate) fn default_bob_frequency() -> f32 {
    0.5
}
pub(crate) fn default_pulse_min() -> f32 {
    0.9
}
pub(crate) fn default_pulse_max() -> f32 {
    1.1
}
pub(crate) fn default_path_speed() -> f32 {
    2.0
}
pub(crate) fn default_path_mode() -> PathMode {
    PathMode::Loop
}
pub(crate) fn default_bounce_height() -> f32 {
    3.0
}
pub(crate) fn default_bounce_gravity() -> f32 {
    9.8
}
pub(crate) fn default_bounce_damping() -> f32 {
    0.7
}

impl BehaviorDef {
    /// Returns the behavior kind as a string.
    pub fn kind(&self) -> &'static str {
        match self {
            BehaviorDef::Orbit { .. } => "orbit",
            BehaviorDef::Spin { .. } => "spin",
            BehaviorDef::Bob { .. } => "bob",
            BehaviorDef::LookAt { .. } => "look_at",
            BehaviorDef::Pulse { .. } => "pulse",
            BehaviorDef::PathFollow { .. } => "path_follow",
            BehaviorDef::Bounce { .. } => "bounce",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behavior_orbit_roundtrip() {
        let b = BehaviorDef::Orbit {
            center: Some(EntityRef::name("sun")),
            center_point: None,
            radius: 10.0,
            speed: 45.0,
            axis: [0.0, 1.0, 0.0],
            phase: 0.0,
            tilt: 15.0,
        };
        let json = serde_json::to_string(&b).unwrap();
        let back: BehaviorDef = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }

    #[test]
    fn behavior_look_at_with_entity_ref() {
        let b = BehaviorDef::LookAt {
            target: EntityRef::name("player"),
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("player"));
        let back: BehaviorDef = serde_json::from_str(&json).unwrap();
        assert_eq!(b, back);
    }

    #[test]
    fn all_behavior_kinds() {
        let behaviors = [
            BehaviorDef::Orbit {
                center: None,
                center_point: Some([0.0, 0.0, 0.0]),
                radius: 5.0,
                speed: 36.0,
                axis: [0.0, 1.0, 0.0],
                phase: 0.0,
                tilt: 0.0,
            },
            BehaviorDef::Spin {
                axis: [0.0, 1.0, 0.0],
                speed: 90.0,
            },
            BehaviorDef::Bob {
                axis: [0.0, 1.0, 0.0],
                amplitude: 0.5,
                frequency: 0.5,
                phase: 0.0,
            },
            BehaviorDef::LookAt {
                target: EntityRef::name("cam"),
            },
            BehaviorDef::Pulse {
                min_scale: 0.9,
                max_scale: 1.1,
                frequency: 0.5,
            },
            BehaviorDef::PathFollow {
                waypoints: vec![[0.0, 0.0, 0.0], [5.0, 0.0, 0.0]],
                speed: 2.0,
                mode: PathMode::PingPong,
                orient_to_path: true,
            },
            BehaviorDef::Bounce {
                height: 3.0,
                gravity: 9.8,
                damping: 0.7,
                surface_y: 0.0,
            },
        ];
        let kinds: Vec<&str> = behaviors.iter().map(|b| b.kind()).collect();
        assert_eq!(
            kinds,
            vec![
                "orbit",
                "spin",
                "bob",
                "look_at",
                "pulse",
                "path_follow",
                "bounce"
            ]
        );
    }

    #[test]
    fn path_mode_roundtrip() {
        for mode in [PathMode::Loop, PathMode::PingPong, PathMode::Once] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: PathMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }
}
