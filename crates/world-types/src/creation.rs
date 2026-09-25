//! Creation definitions — compound objects made of multiple entities.

use serde::{Deserialize, Serialize};

use crate::identity::{CreationId, EntityId};

/// Semantic category for a creation (aids LLM reasoning and search).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SemanticCategory {
    Building,
    Vegetation,
    Furniture,
    Vehicle,
    Character,
    Terrain,
    Decoration,
    Light,
    Audio,
    Custom(String),
}

/// A compound creation — a named group of entities that form a logical object.
///
/// For example, a "house" creation might consist of walls, roof, door, and
/// chimney entities, with optional audio (fireplace) and lights (windows).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CreationDef {
    /// Unique creation identifier.
    pub id: CreationId,
    /// Human-readable name (e.g., "oak_tree", "stone_bridge").
    pub name: String,
    /// Semantic category for search and classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_category: Option<SemanticCategory>,
    /// Bounding box half-extents [x, y, z] for the entire creation.
    #[serde(default)]
    pub bbox_half: [f32; 3],
    /// Entity IDs that make up this creation.
    #[serde(default)]
    pub entities: Vec<EntityId>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_roundtrip() {
        let c = CreationDef {
            id: CreationId(1),
            name: "campfire_setup".to_string(),
            semantic_category: Some(SemanticCategory::Decoration),
            bbox_half: [2.0, 1.5, 2.0],
            entities: vec![EntityId(10), EntityId(11), EntityId(12)],
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: CreationDef = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn custom_category() {
        let c = SemanticCategory::Custom("portal".to_string());
        let json = serde_json::to_string(&c).unwrap();
        let back: SemanticCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }
}
