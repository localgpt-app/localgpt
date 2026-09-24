//! Tour definitions — guided camera/avatar paths through the world.

use serde::{Deserialize, Serialize};

use crate::avatar::PointOfView;

/// How the camera/avatar moves between tour waypoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TourMode {
    /// Ground-level movement respecting gravity / terrain.
    #[default]
    Walk,
    /// Free-flying camera interpolation.
    Fly,
    /// Instant teleport between waypoints (cut, no interpolation).
    Teleport,
}

/// A single stop along a guided tour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TourWaypoint {
    /// Camera / avatar position at this stop.
    pub position: [f32; 3],
    /// Where the camera looks at this stop.
    pub look_at: [f32; 3],
    /// Narrative text shown to the user at this stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// How long to pause at this waypoint (seconds) before moving on.
    #[serde(default)]
    pub pause_duration: f32,
}

/// A guided tour — a named, ordered sequence of waypoints through the world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TourDef {
    /// Human-readable tour name (e.g., "grand_tour", "scenic_overlook").
    pub name: String,
    /// Brief description of the tour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Ordered stops along the tour.
    pub waypoints: Vec<TourWaypoint>,
    /// Movement speed between waypoints (units/sec).
    #[serde(default = "default_tour_speed")]
    pub speed: f32,
    /// Movement mode between waypoints.
    #[serde(default)]
    pub mode: TourMode,
    /// If true, this tour starts automatically when the world is loaded.
    #[serde(default)]
    pub autostart: bool,
    /// If true, the tour loops back to the first waypoint after the last.
    #[serde(default)]
    pub loop_tour: bool,
    /// Optional PoV override for this tour (falls back to avatar default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pov: Option<PointOfView>,
}

fn default_tour_speed() -> f32 {
    3.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tour_roundtrip() {
        let tour = TourDef {
            name: "grand_tour".to_string(),
            description: Some("A scenic tour".to_string()),
            waypoints: vec![
                TourWaypoint {
                    position: [0.0, 1.0, 5.0],
                    look_at: [0.0, 1.5, 0.0],
                    description: Some("Start".to_string()),
                    pause_duration: 3.0,
                },
                TourWaypoint {
                    position: [10.0, 1.0, 0.0],
                    look_at: [5.0, 2.0, -5.0],
                    description: None,
                    pause_duration: 0.0,
                },
            ],
            speed: 2.0,
            mode: TourMode::Walk,
            autostart: false,
            loop_tour: true,
            pov: Some(PointOfView::ThirdPerson),
        };

        let json = serde_json::to_string_pretty(&tour).unwrap();
        let back: TourDef = serde_json::from_str(&json).unwrap();
        assert_eq!(tour, back);
    }

    #[test]
    fn tour_modes() {
        for mode in [TourMode::Walk, TourMode::Fly, TourMode::Teleport] {
            let json = serde_json::to_string(&mode).unwrap();
            let back: TourMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, back);
        }
    }
}
