//! Avatar definition — user/explorer presence in a world.

use serde::{Deserialize, Serialize};

use crate::identity::EntityRef;

/// Camera point-of-view mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum PointOfView {
    /// Camera at avatar eye level; avatar model not visible.
    FirstPerson,
    /// Camera orbits behind/above the avatar; avatar model visible.
    #[default]
    ThirdPerson,
}

/// Avatar configuration describing the user/explorer presence in a world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AvatarDef {
    /// Where the avatar spawns in the world.
    #[serde(default = "default_avatar_spawn")]
    pub spawn_position: [f32; 3],
    /// Initial look direction.
    #[serde(default = "default_avatar_look_at")]
    pub spawn_look_at: [f32; 3],
    /// Camera point-of-view mode.
    #[serde(default)]
    pub pov: PointOfView,
    /// Movement speed in units per second.
    #[serde(default = "default_avatar_speed")]
    pub movement_speed: f32,
    /// Avatar eye-height above ground (used for first-person eye level).
    #[serde(default = "default_avatar_height")]
    pub height: f32,
    /// Entity reference of the 3D model representing the avatar (3rd-person).
    /// When `None`, the world has no visible avatar model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_entity: Option<EntityRef>,
}

impl Default for AvatarDef {
    fn default() -> Self {
        Self {
            spawn_position: default_avatar_spawn(),
            spawn_look_at: default_avatar_look_at(),
            pov: PointOfView::default(),
            movement_speed: default_avatar_speed(),
            height: default_avatar_height(),
            model_entity: None,
        }
    }
}

fn default_avatar_spawn() -> [f32; 3] {
    [0.0, 0.0, 5.0]
}
fn default_avatar_look_at() -> [f32; 3] {
    [0.0, 0.0, 0.0]
}
fn default_avatar_speed() -> f32 {
    5.0
}
fn default_avatar_height() -> f32 {
    1.8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_avatar() {
        let a = AvatarDef::default();
        assert_eq!(a.pov, PointOfView::ThirdPerson);
        assert_eq!(a.movement_speed, 5.0);
        assert_eq!(a.height, 1.8);
        assert!(a.model_entity.is_none());
    }

    #[test]
    fn avatar_roundtrip() {
        let a = AvatarDef {
            spawn_position: [1.0, 1.8, 5.0],
            spawn_look_at: [0.0, 1.5, 0.0],
            pov: PointOfView::FirstPerson,
            movement_speed: 3.0,
            height: 1.8,
            model_entity: Some(EntityRef::name("player_model")),
        };
        let json = serde_json::to_string(&a).unwrap();
        let back: AvatarDef = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }
}
