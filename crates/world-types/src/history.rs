//! Undo/redo history — edit operations and their inverses.

use serde::{Deserialize, Serialize};

use crate::audio::AudioDef;
use crate::entity::{EntityPatch, WorldEntity};
use crate::identity::EntityId;
use crate::world::{CameraDef, EnvironmentDef};

/// A recorded world edit with its inverse for undo support.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorldEdit {
    /// Monotonically increasing sequence number.
    pub seq: u64,
    /// The operation that was performed.
    pub op: EditOp,
    /// The inverse operation (for undo).
    pub inverse: EditOp,
    /// Timestamp in milliseconds since epoch.
    pub timestamp_ms: u64,
    /// Who performed the edit (e.g., "user", "llm", agent ID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

/// An atomic edit operation on the world.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum EditOp {
    /// Spawn a new entity.
    SpawnEntity { entity: WorldEntity },
    /// Delete an entity by ID.
    DeleteEntity { id: EntityId },
    /// Modify an entity with a patch.
    ModifyEntity { id: EntityId, patch: EntityPatch },
    /// Set environment (background color, ambient light, fog).
    SetEnvironment { env: EnvironmentDef },
    /// Set camera position, look-at target, and FOV.
    SetCamera { camera: CameraDef },
    /// Set ambient soundscape (replaces all layers).
    SetAmbience { ambience: Vec<AmbienceLayerDef> },
    /// Spawn an audio emitter attached to an entity or position.
    SpawnAudioEmitter { name: String, audio: AudioDef },
    /// Remove an audio emitter by name.
    RemoveAudioEmitter { name: String, audio: AudioDef },
    /// A batch of atomic operations (all-or-nothing).
    Batch { ops: Vec<EditOp> },
}

/// A single ambient audio layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AmbienceLayerDef {
    /// Layer name (e.g., "wind", "rain").
    pub name: String,
    /// The sound source.
    pub source: crate::audio::AudioSource,
    /// Volume (0.0–1.0).
    pub volume: f32,
}

impl EditOp {
    /// Compute the inverse of this operation.
    ///
    /// For `SpawnEntity`, the inverse is `DeleteEntity`.
    /// For `DeleteEntity`, the caller must provide the entity state to restore.
    /// For `ModifyEntity`, the caller must provide the previous state.
    /// For `Batch`, the inverse is a batch of inverses in reverse order.
    pub fn compute_inverse_spawn(entity: &WorldEntity) -> EditOp {
        EditOp::DeleteEntity { id: entity.id }
    }

    /// Create a spawn operation for an entity.
    pub fn spawn(entity: WorldEntity) -> EditOp {
        EditOp::SpawnEntity { entity }
    }

    /// Create a delete operation.
    pub fn delete(id: EntityId) -> EditOp {
        EditOp::DeleteEntity { id }
    }

    /// Create a modify operation.
    pub fn modify(id: EntityId, patch: EntityPatch) -> EditOp {
        EditOp::ModifyEntity { id, patch }
    }
}

/// Edit history — append-only log of world edits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EditHistory {
    /// All edits in chronological order.
    pub edits: Vec<WorldEdit>,
    /// Current position in the edit log (for undo/redo).
    /// Points to the next edit to be undone.
    pub cursor: usize,
}

impl EditHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a new edit. Truncates any redo history.
    pub fn push(&mut self, op: EditOp, inverse: EditOp, author: Option<String>) {
        // Truncate redo history
        self.edits.truncate(self.cursor);

        let seq = self.edits.len() as u64;
        self.edits.push(WorldEdit {
            seq,
            op,
            inverse,
            timestamp_ms: 0, // Caller should set this
            author,
        });
        self.cursor = self.edits.len();
    }

    /// Get the next operation to undo, if any.
    pub fn undo(&mut self) -> Option<&EditOp> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        Some(&self.edits[self.cursor].inverse)
    }

    /// Get the next operation to redo, if any.
    pub fn redo(&mut self) -> Option<&EditOp> {
        if self.cursor >= self.edits.len() {
            return None;
        }
        let op = &self.edits[self.cursor].op;
        self.cursor += 1;
        Some(op)
    }

    /// Number of operations that can be undone.
    pub fn undo_count(&self) -> usize {
        self.cursor
    }

    /// Number of operations that can be redone.
    pub fn redo_count(&self) -> usize {
        self.edits.len() - self.cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::WorldEntity;

    #[test]
    fn undo_redo_basic() {
        let mut history = EditHistory::new();

        let entity = WorldEntity::new(1, "cube");
        let op = EditOp::spawn(entity.clone());
        let inverse = EditOp::delete(entity.id);

        history.push(op, inverse, None);
        assert_eq!(history.undo_count(), 1);
        assert_eq!(history.redo_count(), 0);

        // Undo
        let undo_op = history.undo().unwrap();
        assert!(matches!(undo_op, EditOp::DeleteEntity { .. }));
        assert_eq!(history.undo_count(), 0);
        assert_eq!(history.redo_count(), 1);

        // Redo
        let redo_op = history.redo().unwrap();
        assert!(matches!(redo_op, EditOp::SpawnEntity { .. }));
        assert_eq!(history.undo_count(), 1);
        assert_eq!(history.redo_count(), 0);
    }

    #[test]
    fn new_edit_truncates_redo() {
        let mut history = EditHistory::new();

        // Push two edits
        for i in 0..2 {
            let entity = WorldEntity::new(i, format!("e{i}"));
            history.push(
                EditOp::spawn(entity.clone()),
                EditOp::delete(entity.id),
                None,
            );
        }

        // Undo one
        history.undo();
        assert_eq!(history.redo_count(), 1);

        // Push a new edit — should truncate redo
        let entity = WorldEntity::new(99, "new");
        history.push(
            EditOp::spawn(entity.clone()),
            EditOp::delete(entity.id),
            None,
        );
        assert_eq!(history.redo_count(), 0);
        assert_eq!(history.undo_count(), 2); // first + new
    }

    #[test]
    fn edit_op_set_environment_roundtrip() {
        let op = EditOp::SetEnvironment {
            env: crate::world::EnvironmentDef {
                background_color: Some([0.1, 0.2, 0.3, 1.0]),
                ambient_intensity: Some(0.5),
                ambient_color: Some([1.0, 1.0, 0.9, 1.0]),
                fog_density: Some(0.02),
                fog_color: None,
            },
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: EditOp = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, EditOp::SetEnvironment { .. }));
    }

    #[test]
    fn edit_op_set_camera_roundtrip() {
        let op = EditOp::SetCamera {
            camera: crate::world::CameraDef {
                position: [10.0, 5.0, 10.0],
                look_at: [0.0, 0.0, 0.0],
                fov_degrees: 60.0,
            },
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: EditOp = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, EditOp::SetCamera { .. }));
    }

    #[test]
    fn edit_op_batch_roundtrip() {
        let entity = WorldEntity::new(5, "test_batch");
        let op = EditOp::Batch {
            ops: vec![
                EditOp::delete(EntityId(1)),
                EditOp::spawn(entity),
                EditOp::SetEnvironment {
                    env: crate::world::EnvironmentDef {
                        background_color: Some([0.0, 0.0, 0.0, 1.0]),
                        ambient_intensity: None,
                        ambient_color: None,
                        fog_density: None,
                        fog_color: None,
                    },
                },
            ],
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: EditOp = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, EditOp::Batch { ops } if ops.len() == 3));
    }

    #[test]
    fn edit_op_modify_entity_roundtrip() {
        let op = EditOp::ModifyEntity {
            id: EntityId(42),
            patch: EntityPatch {
                name: Some(crate::identity::EntityName::new("renamed")),
                shape: Some(Some(crate::shape::Shape::Sphere { radius: 3.0 })),
                light: Some(None), // Clearing the light
                ..Default::default()
            },
        };
        let json = serde_json::to_string(&op).unwrap();
        let back: EditOp = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, EditOp::ModifyEntity { id, .. } if id.0 == 42));
    }
}
