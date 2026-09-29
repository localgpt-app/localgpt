//! Triggers — what happens when a visitor clicks, comes near, or waits.
//!
//! [`BehaviorDef`](crate::BehaviorDef)s run from the moment a world loads;
//! a [`TriggerDef`] waits for an event and then runs one action on its own
//! entity (or, for `teleport`, on the visitor). The *visitor* is whoever
//! walks the world: the player when there is one, else the camera.
//!
//! Every renderer runs the same core with the same semantics — Bevy through
//! `localgpt-world-bevy`'s trigger runtime, three.js through the web viewer —
//! and the conformance world `triggers.json` holds them to it. Actions only
//! a host app understands (a score, an inventory, named sounds) are
//! [`TriggerActionDef::Host`] actions, which other renderers skip.
//!
//! The core is small so it maps onto glTF's `KHR_interactivity` behavior
//! graphs:
//!
//! | Here | Nearest `KHR_interactivity` construct |
//! |---|---|
//! | `start` | `event/onStart` |
//! | `click` | `event/onSelect` (with `KHR_node_selectability`) |
//! | `timer` | `event/onTick` accumulating time |
//! | `proximity`, `area_enter`, `area_exit`, `collision` | none in the core set; an extension event |
//! | `once`, `cooldown` | flow control between the event and the action |
//! | `show`, `hide`, `toggle`, `remove` | `pointer/set` on `KHR_node_visibility`'s `visible` |
//! | `animate` | `pointer/set` on the node's transform, interpolated |
//! | `teleport`, `show_text`, `host` | no core equivalent; host actions |
//!
//! ## Areas
//!
//! An area is an explicit volume in the entity's own frame (so it moves,
//! turns and scales with the entity), independent of what the entity looks
//! like — the model of Unity's trigger colliders, Unreal's trigger volumes,
//! Godot's `Area3D` with a collision shape, and OMI's physics triggers.
//! Without one, the area is the box of the entity's parametric shape, or a
//! sphere of radius 3 when it has none ([`TriggerDef::area`]).

use serde::{Deserialize, Serialize};

use crate::entity::WorldEntity;

/// One event and the action it runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TriggerDef {
    /// The event that fires the trigger.
    pub on: TriggerEvent,
    /// What the trigger does.
    pub action: TriggerActionDef,
    /// Fire at most once.
    #[serde(default, skip_serializing_if = "is_false")]
    pub once: bool,
    /// Seconds before the trigger can fire again. Default: 1 for
    /// `proximity` (which fires while the visitor stays near), 0 otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown: Option<f32>,
    /// An item the visitor must hold. Renderers without an inventory never
    /// fire a trigger that needs one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_item: Option<String>,
}

/// The event that fires a trigger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TriggerEvent {
    /// Once, when the world starts.
    Start,
    /// The visitor clicks (or uses) the entity from at most `max_distance`
    /// away, measured to the entity's origin.
    Click {
        #[serde(default = "default_click_distance")]
        max_distance: f32,
        /// Hint shown while the entity is in reach ("Open").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
    },
    /// The visitor is within `radius` of the entity's origin; fires again
    /// every `cooldown` while they stay.
    Proximity {
        #[serde(default = "default_radius")]
        radius: f32,
    },
    /// The visitor enters the entity's area.
    AreaEnter {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        volume: Option<TriggerVolume>,
    },
    /// The visitor leaves the entity's area.
    AreaExit {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        volume: Option<TriggerVolume>,
    },
    /// A body starts overlapping a sphere of `radius` around the entity.
    /// Renderers without physics treat the visitor as the only body.
    Collision {
        #[serde(default = "default_collision_radius")]
        radius: f32,
    },
    /// Every `interval` seconds.
    Timer { interval: f32 },
}

/// A trigger area, in the entity's own frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "shape", rename_all = "snake_case")]
pub enum TriggerVolume {
    /// A box centred on the entity's origin.
    Box { half_extents: [f32; 3] },
    /// A sphere centred on the entity's origin.
    Sphere { radius: f32 },
}

impl TriggerVolume {
    /// Whether a point in the entity's own frame is inside.
    pub fn contains_local(&self, p: [f32; 3]) -> bool {
        match self {
            TriggerVolume::Box { half_extents: h } => {
                p[0].abs() <= h[0] && p[1].abs() <= h[1] && p[2].abs() <= h[2]
            }
            TriggerVolume::Sphere { radius } => {
                p[0] * p[0] + p[1] * p[1] + p[2] * p[2] <= radius * radius
            }
        }
    }
}

/// The action a trigger runs on its own entity (or, for `teleport`, on
/// the visitor).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TriggerActionDef {
    /// Show `text` to the visitor for `seconds`.
    ShowText {
        text: String,
        #[serde(default = "default_text_seconds")]
        seconds: f32,
    },
    /// Make the entity visible.
    Show,
    /// Make the entity invisible (it stays in the world and can be shown).
    Hide,
    /// Flip the entity's visibility.
    Toggle,
    /// Remove the entity from the running world (a saved world keeps it).
    Remove,
    /// Move a transform property (`position`, `rotation` in XYZ Euler
    /// degrees, or `scale`; one number scales uniformly) from its current
    /// value to `to`, linearly over `duration` seconds.
    Animate {
        #[serde(default = "default_property")]
        property: String,
        #[serde(default)]
        to: Vec<f32>,
        #[serde(default = "default_duration")]
        duration: f32,
    },
    /// Move the visitor to `destination`.
    Teleport { destination: [f32; 3] },
    /// An action the host app defines (Gen: `add_score`, `play_sound`,
    /// `set_state`, `spawn`), with its arguments. Renderers that don't know
    /// `name` skip it.
    Host {
        name: String,
        #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
        args: serde_json::Map<String, serde_json::Value>,
    },
}

impl TriggerDef {
    /// The trigger's area on `entity`, for `area_enter` and `area_exit`: the
    /// event's volume, else the box of the entity's shape, else a sphere of
    /// radius 3. `None` for other events.
    pub fn area(&self, entity: &WorldEntity) -> Option<TriggerVolume> {
        let volume = match &self.on {
            TriggerEvent::AreaEnter { volume } | TriggerEvent::AreaExit { volume } => volume,
            _ => return None,
        };
        Some(volume.clone().unwrap_or_else(|| match &entity.shape {
            Some(shape) => TriggerVolume::Box {
                half_extents: shape.local_aabb_half(),
            },
            None => TriggerVolume::Sphere {
                radius: DEFAULT_AREA_RADIUS,
            },
        }))
    }

    /// The cooldown in seconds, with the event's default.
    pub fn cooldown_secs(&self) -> f32 {
        self.cooldown.unwrap_or(match self.on {
            TriggerEvent::Proximity { .. } => 1.0,
            _ => 0.0,
        })
    }
}

/// Radius of the area of an entity with no shape and no volume.
pub const DEFAULT_AREA_RADIUS: f32 = 3.0;

fn is_false(v: &bool) -> bool {
    !*v
}
fn default_click_distance() -> f32 {
    5.0
}
fn default_radius() -> f32 {
    5.0
}
fn default_collision_radius() -> f32 {
    3.0
}
fn default_property() -> String {
    "position".to_string()
}
fn default_duration() -> f32 {
    1.0
}
fn default_text_seconds() -> f32 {
    4.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shape::Shape;

    #[test]
    fn json_is_flat_and_tagged() {
        let t = TriggerDef {
            on: TriggerEvent::Click {
                max_distance: 4.0,
                prompt: Some("Open".into()),
            },
            action: TriggerActionDef::ShowText {
                text: "It creaks open.".into(),
                seconds: 4.0,
            },
            once: true,
            cooldown: None,
            requires_item: None,
        };
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(json["on"]["event"], "click");
        assert_eq!(json["action"]["action"], "show_text");
        assert_eq!(json["once"], true);
        let back: TriggerDef = serde_json::from_value(json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn defaults_fill_missing_numbers() {
        let t: TriggerDef =
            serde_json::from_str(r#"{"on":{"event":"proximity"},"action":{"action":"hide"}}"#)
                .unwrap();
        assert_eq!(t.on, TriggerEvent::Proximity { radius: 5.0 });
        assert!(!t.once);
        assert_eq!(t.cooldown_secs(), 1.0);
    }

    #[test]
    fn host_actions_carry_their_arguments() {
        let t: TriggerDef = serde_json::from_str(
            r#"{"on":{"event":"click"},"action":{"action":"host","name":"add_score","args":{"amount":10}}}"#,
        )
        .unwrap();
        let TriggerActionDef::Host { name, args } = &t.action else {
            panic!("{t:?}");
        };
        assert_eq!(name, "add_score");
        assert_eq!(args["amount"], 10);
    }

    #[test]
    fn areas_default_to_the_shape_box_or_a_sphere() {
        let area = |volume| TriggerDef {
            on: TriggerEvent::AreaEnter { volume },
            action: TriggerActionDef::Show,
            once: false,
            cooldown: None,
            requires_item: None,
        };
        let crate_ = WorldEntity::new(1, "crate").with_shape(Shape::Cuboid {
            x: 2.0,
            y: 1.0,
            z: 4.0,
        });
        let empty = WorldEntity::new(2, "spot");
        assert_eq!(
            area(None).area(&crate_),
            Some(TriggerVolume::Box {
                half_extents: [1.0, 0.5, 2.0]
            })
        );
        assert_eq!(
            area(None).area(&empty),
            Some(TriggerVolume::Sphere { radius: 3.0 })
        );
        let sphere = TriggerVolume::Sphere { radius: 2.0 };
        assert_eq!(area(Some(sphere.clone())).area(&crate_), Some(sphere));

        let b = TriggerVolume::Box {
            half_extents: [1.0, 0.5, 2.0],
        };
        assert!(b.contains_local([0.9, -0.4, 1.9]));
        assert!(!b.contains_local([0.9, 0.6, 0.0]));
        assert!(TriggerVolume::Sphere { radius: 1.0 }.contains_local([0.6, 0.6, 0.0]));
        assert!(!TriggerVolume::Sphere { radius: 1.0 }.contains_local([0.8, 0.8, 0.0]));
    }
}
