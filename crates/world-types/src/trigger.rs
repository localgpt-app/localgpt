//! Triggers — what happens when a visitor clicks, comes near, or waits.
//!
//! [`BehaviorDef`](crate::BehaviorDef)s run from the moment a world loads;
//! a [`TriggerDef`] waits for an event and then runs one action. The
//! vocabulary is Gen's interaction runtime (`gen_add_trigger`), so a Gen
//! world keeps its triggers when it is saved and the web viewer can run them.
//!
//! It is kept small so it can map onto glTF's `KHR_interactivity` behavior
//! graphs when runtimes support them:
//!
//! | Here | Nearest `KHR_interactivity` construct |
//! |---|---|
//! | `click` | `event/onSelect` (with `KHR_node_selectability`) |
//! | `timer` | `event/onTick` accumulating time |
//! | `proximity`, `area_enter`, `area_exit`, `collision` | none in the core set; an extension event |
//! | `once`, `cooldown` | flow control between the event and the action |
//! | `enable`, `disable`, `destroy` | `pointer/set` on `KHR_node_visibility`'s `visible` |
//! | `animate` | `pointer/set` on the node's transform, interpolated |
//! | `teleport`, `show_text`, `play_sound`, `toggle_state`, `add_score`, `spawn` | no core equivalent; host actions |
//!
//! Renderers that can't run an action skip it; the Gen runtime runs all of
//! them, the web viewer runs `click`, `proximity`, `area_enter`, `area_exit`
//! and `timer` with `show_text`, `enable`, `disable`, `destroy` and
//! `teleport`.

use serde::{Deserialize, Serialize};

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
    /// Seconds before the trigger can fire again (proximity and collision).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooldown: Option<f32>,
    /// An inventory item the visitor must hold for the trigger to fire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_item: Option<String>,
}

/// The event that fires a trigger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TriggerEvent {
    /// The visitor clicks the entity from at most `max_distance` away.
    Click {
        #[serde(default = "default_click_distance")]
        max_distance: f32,
        /// Hint shown while the entity is in reach ("Open").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
    },
    /// The visitor comes within `radius` of the entity.
    Proximity {
        #[serde(default = "default_radius")]
        radius: f32,
    },
    /// The visitor enters the entity's bounds.
    AreaEnter,
    /// The visitor leaves the entity's bounds.
    AreaExit,
    /// Something overlaps a sensor sphere of `radius` around the entity.
    Collision {
        #[serde(default = "default_collision_radius")]
        radius: f32,
    },
    /// Every `interval` seconds.
    Timer { interval: f32 },
}

/// The action a trigger runs on its own entity (or on the visitor, for
/// `teleport`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum TriggerActionDef {
    /// Animate a transform property (`position`, `rotation`, `scale`) to `to`
    /// over `duration` seconds.
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
    /// Play a named sound.
    PlaySound { sound: String },
    /// Show `text` to the visitor.
    ShowText { text: String },
    /// Flip a named state on the entity (a door's `open`, a lamp's `on`).
    ToggleState {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<String>,
    },
    /// Spawn a copy of a named template entity.
    Spawn { template: String },
    /// Remove the entity.
    Destroy,
    /// Add `amount` to the visitor's score in `category`.
    AddScore {
        amount: i32,
        #[serde(default = "default_category")]
        category: String,
    },
    /// Show the entity.
    Enable,
    /// Hide the entity.
    Disable,
}

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
fn default_category() -> String {
    "points".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_flat_and_tagged() {
        let t = TriggerDef {
            on: TriggerEvent::Click {
                max_distance: 4.0,
                prompt: Some("Open".into()),
            },
            action: TriggerActionDef::ShowText {
                text: "It creaks open.".into(),
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
            serde_json::from_str(r#"{"on":{"event":"proximity"},"action":{"action":"disable"}}"#)
                .unwrap();
        assert_eq!(t.on, TriggerEvent::Proximity { radius: 5.0 });
        assert!(!t.once);
    }
}
