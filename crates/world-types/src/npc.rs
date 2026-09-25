//! NPC brain and memory serialization types.
//!
//! These types are used to persist NPC brain configuration and memories
//! across save/load. Stored in `npcs.ron` alongside `world.ron`.

use serde::{Deserialize, Serialize};

/// Persisted NPC brain + memory data for a single NPC entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcDef {
    /// Entity name this NPC data belongs to.
    pub entity_name: String,
    /// Brain configuration (if attached).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brain: Option<NpcBrainDef>,
    /// Persisted memories (if attached).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<NpcMemoryDef>,
}

/// Serializable brain configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcBrainDef {
    pub personality: String,
    pub model: String,
    pub tick_rate: f32,
    pub perception_radius: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub goals: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge: Vec<String>,
}

/// Serializable memory store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcMemoryDef {
    pub capacity: usize,
    pub auto_memorize: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<NpcMemoryEntryDef>,
}

/// A single serialized memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcMemoryEntryDef {
    pub timestamp: f64,
    pub content: String,
    pub importance: f32,
}

/// Collection of all NPC data in a world, for `npcs.ron`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NpcDataCollection {
    /// NPC definitions, one per entity with brain/memory.
    pub npcs: Vec<NpcDef>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_npc_def_roundtrip() {
        let npc = NpcDef {
            entity_name: "guard".to_string(),
            brain: Some(NpcBrainDef {
                personality: "a stern guard".to_string(),
                model: "llama3.2:3b".to_string(),
                tick_rate: 5.0,
                perception_radius: 15.0,
                goals: vec!["protect the gate".to_string()],
                knowledge: vec!["the king is away".to_string()],
            }),
            memory: Some(NpcMemoryDef {
                capacity: 50,
                auto_memorize: true,
                entries: vec![NpcMemoryEntryDef {
                    timestamp: 100.0,
                    content: "Met a traveler at dawn".to_string(),
                    importance: 0.8,
                }],
            }),
        };

        let json = serde_json::to_string(&npc).unwrap();
        let roundtrip: NpcDef = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.entity_name, "guard");
        assert!(roundtrip.brain.is_some());
        assert!(roundtrip.memory.is_some());
        assert_eq!(roundtrip.memory.unwrap().entries.len(), 1);
    }

    #[test]
    fn test_empty_collection() {
        let col = NpcDataCollection::default();
        assert!(col.npcs.is_empty());
    }
}
