//! What changed between two documents, as ordered ops.
//!
//! This is the contract between the editing model and whatever draws. A
//! renderer never re-reads a document: it is handed the ops that move it from
//! what it is showing to what it should show, which is the difference between
//! scrubbing a timeline and rebuilding a world sixty times a second.
//!
//! Entity diffing — including the ordering that makes it applicable, spawns
//! parents-first and deletes children-first — is
//! [`localgpt_world_sync::diff_entities`], reused rather than reimplemented:
//! the ordering rules are subtle and already tested, and a second copy of
//! them is exactly the kind of drift this workspace has paid for before.
//! What this module adds is the scene-wide half, which an entity diff does
//! not cover and a seek very much does: an environment, a camera, an avatar,
//! tours, a soundtrack, ambience and creations all change when you move
//! between tips.

use openworldformat::doc::WorldDoc;
use openworldformat::history::EditOp;
use openworldformat::world_patch::WorldPatch;

/// The ops that move a renderer from one document to another.
///
/// Scene-wide changes come first, so a client repaints its environment before
/// entities arrive; entity ops follow in applicable order.
#[derive(Debug, Clone, Default)]
pub struct Delta {
    /// The ops, in the order they must be applied.
    pub ops: Vec<EditOp>,
}

impl Delta {
    /// True when the two documents agree and nothing has to be drawn.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// How many ops the move costs.
    pub fn len(&self) -> usize {
        self.ops.len()
    }
}

/// The ops that turn `from` into `to`.
///
/// The result is a *description of a difference*, not a history: it is what a
/// renderer applies, never what gets appended to a log. An editor's log entry
/// is the batch the author submitted; a delta is derived and disposable.
pub fn delta_between(from: &WorldDoc, to: &WorldDoc) -> Delta {
    let mut ops = Vec::new();

    if let Some(patch) = world_patch_between(from, to) {
        ops.push(EditOp::ModifyWorld {
            patch: Box::new(patch),
        });
    }

    let projection: Vec<_> = to.entities().cloned().collect();
    ops.extend(localgpt_world_sync::diff_entities(from, &projection));

    Delta { ops }
}

/// The scene-wide patch that turns `from`'s fields into `to`'s, or `None`
/// when they already agree.
///
/// Every field is replaced whole, because that is what the op means: a
/// `WorldPatch` field left absent is untouched, `Some(None)` clears it and
/// `Some(Some(v))` sets it. Partial merging belongs at ingestion, where the
/// format puts it — a delta is computed between two known documents, so
/// there is nothing to merge.
fn world_patch_between(from: &WorldDoc, to: &WorldDoc) -> Option<WorldPatch> {
    let mut patch = WorldPatch::default();

    if from.meta() != to.meta() {
        patch.meta = Some(to.meta());
    }
    if from.environment != to.environment {
        patch.environment = Some(to.environment.clone());
    }
    if from.camera != to.camera {
        patch.camera = Some(to.camera.clone());
    }
    if from.avatar != to.avatar {
        patch.avatar = Some(to.avatar.clone());
    }
    if from.tours != to.tours {
        patch.tours = Some(to.tours.clone());
    }
    if from.soundtrack != to.soundtrack {
        patch.soundtrack = Some(to.soundtrack.clone());
    }
    if from.ambience != to.ambience {
        patch.ambience = Some(to.ambience.clone());
    }
    if from.creations != to.creations {
        patch.creations = Some(to.creations.clone());
    }

    (!patch.is_empty()).then_some(patch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{cube, world_with_cube};

    #[test]
    fn identical_documents_have_an_empty_delta() {
        let doc = world_with_cube();
        assert!(delta_between(&doc, &doc).is_empty());
    }

    #[test]
    fn a_spawn_appears_as_one_op() {
        let from = world_with_cube();
        let mut to = from.clone();
        to.apply(&EditOp::SpawnEntity {
            entity: cube(2, "second"),
        })
        .unwrap();

        let delta = delta_between(&from, &to);
        assert_eq!(delta.len(), 1);
        assert!(matches!(delta.ops[0], EditOp::SpawnEntity { .. }));
    }

    #[test]
    fn a_delta_applied_to_from_produces_to() {
        let from = world_with_cube();
        let mut to = from.clone();
        to.apply(&EditOp::SpawnEntity {
            entity: cube(2, "second"),
        })
        .unwrap();
        to.apply(&EditOp::SpawnEntity {
            entity: cube(3, "third"),
        })
        .unwrap();
        to.apply(&EditOp::DeleteEntity {
            id: openworldformat::EntityId(1),
        })
        .unwrap();

        let delta = delta_between(&from, &to);
        let mut moved = from.clone();
        moved.apply_entry(&delta.ops).expect("the delta applies");

        assert_eq!(moved.len(), to.len());
        for entity in to.entities() {
            assert!(moved.get(entity.id.0).is_some(), "{} missing", entity.id);
        }
    }

    #[test]
    fn a_delta_is_symmetric_enough_to_go_back() {
        let from = world_with_cube();
        let mut to = from.clone();
        to.apply(&EditOp::SpawnEntity {
            entity: cube(2, "second"),
        })
        .unwrap();

        let back = delta_between(&to, &from);
        let mut moved = to.clone();
        moved.apply_entry(&back.ops).expect("the reverse applies");
        assert_eq!(moved.len(), from.len());
    }

    #[test]
    fn scene_wide_changes_ride_in_front_of_entity_ops() {
        let from = world_with_cube();
        let mut to = from.clone();
        to.environment = Some(Default::default());
        to.apply(&EditOp::SpawnEntity {
            entity: cube(2, "second"),
        })
        .unwrap();

        let delta = delta_between(&from, &to);
        assert!(
            matches!(delta.ops.first(), Some(EditOp::ModifyWorld { .. })),
            "expected the world patch first, got {:?}",
            delta.ops.first()
        );
        assert!(delta.ops.len() >= 2);
    }

    #[test]
    fn a_cleared_field_is_a_patch_not_an_omission() {
        let mut from = world_with_cube();
        from.environment = Some(Default::default());
        let mut to = from.clone();
        to.environment = None;

        let delta = delta_between(&from, &to);
        match delta.ops.first() {
            Some(EditOp::ModifyWorld { patch }) => {
                assert_eq!(patch.environment, Some(None), "clearing must be explicit");
            }
            other => panic!("expected a world patch, got {other:?}"),
        }
    }
}
