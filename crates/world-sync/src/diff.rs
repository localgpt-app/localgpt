//! Turn a fresh projection of a live scene into the ops that bring the
//! document to match.
//!
//! Gen's host builds a `Vec<WorldEntity>` from its live ECS (the same
//! `snapshot_entity` used by undo and save) a few times a second and diffs it
//! against the authority's document. The diff is the ops. This is how ~100
//! tools, the inspector and undo/redo all sync without per-tool
//! instrumentation. Entities with behaviors must be projected at their *base*
//! transform, so animation never becomes ops.
//!
//! Op ordering matters for both ends: spawns go parents-first (a child's
//! parent must exist), deletes go children-first (deleting an entity deletes
//! its subtree, so the child must be gone before its parent is named).

use std::collections::{HashMap, HashSet};

use localgpt_world_types as wt;
use wt::{EditOp, EntityPatch, WorldEntity};

use crate::doc::WorldDoc;

/// The ops that turn `doc` into `projection` (plus an environment change).
///
/// `projection` must be the full scene every call, not a delta. An empty
/// `ops` result means the scene matches the document.
pub fn diff_scene(
    doc: &WorldDoc,
    projection: &[WorldEntity],
    environment: Option<&wt::EnvironmentDef>,
) -> Vec<EditOp> {
    let mut ops = diff_entities(doc, projection);
    if let Some(env) = environment
        && doc.environment.as_ref() != Some(env)
    {
        // Environment changes ride in front so clients repaint first.
        ops.insert(0, EditOp::SetEnvironment { env: env.clone() });
    }
    ops
}

/// The entity ops that turn the document's entity set into `projection`.
pub fn diff_entities(doc: &WorldDoc, projection: &[WorldEntity]) -> Vec<EditOp> {
    let new_by_id: HashMap<u64, &WorldEntity> = projection.iter().map(|e| (e.id.0, e)).collect();
    let old_ids: HashSet<u64> = doc.entities().map(|e| e.id.0).collect();
    let new_ids: HashSet<u64> = new_by_id.keys().copied().collect();

    let mut ops = Vec::new();

    // Deletes: ids gone from the projection. Children first (deepest last
    // surviving ancestor is named last), because applying a delete removes
    // the whole subtree.
    let mut deleted: Vec<u64> = old_ids.difference(&new_ids).copied().collect();
    deleted.sort_by_key(|id| std::cmp::Reverse(doc.depth(*id)));
    for id in deleted {
        ops.push(EditOp::delete(wt::EntityId(id)));
    }

    // Spawns: new ids, parents before children. Depth is measured against the
    // projection; ids whose parents are also new sort after them.
    let mut spawned: Vec<&WorldEntity> = projection
        .iter()
        .filter(|e| !old_ids.contains(&e.id.0))
        .collect();
    spawned.sort_by_key(|e| projection_depth(e, &new_by_id));
    for entity in spawned {
        ops.push(EditOp::spawn(entity.clone()));
    }

    // Modifies: ids in both, field by field.
    for new in projection {
        let Some(old) = doc.get(new.id.0) else {
            continue; // already spawned above
        };
        if let Some(patch) = entity_patch(old, new) {
            ops.push(EditOp::modify(new.id, patch));
        }
    }

    ops
}

/// The patch from `old` to `new`, or `None` when they're equal. Only fields
/// `EntityPatch` can express are compared (creation membership and chunk
/// assignment don't sync, as in the undo history).
fn entity_patch(old: &WorldEntity, new: &WorldEntity) -> Option<EntityPatch> {
    let mut patch = EntityPatch::default();
    if old.name != new.name {
        patch.name = Some(new.name.clone());
    }
    if old.transform != new.transform {
        patch.transform = Some(new.transform.clone());
    }
    if old.parent != new.parent {
        patch.parent = Some(new.parent);
    }
    if old.shape != new.shape {
        patch.shape = Some(new.shape.clone());
    }
    if old.material != new.material {
        patch.material = Some(new.material.clone());
    }
    if old.light != new.light {
        patch.light = Some(new.light.clone());
    }
    if old.behaviors != new.behaviors {
        patch.behaviors = Some(new.behaviors.clone());
    }
    if old.audio != new.audio {
        patch.audio = Some(new.audio.clone());
    }
    if old.mesh_asset != new.mesh_asset {
        patch.mesh_asset = Some(new.mesh_asset.clone());
    }
    if old.modulations != new.modulations {
        patch.modulations = Some(new.modulations.clone());
    }
    if patch == EntityPatch::default() {
        None
    } else {
        Some(patch)
    }
}

/// Nesting depth of `entity` within the projection (0 for a root). Cycles
/// (which validation rejects downstream anyway) just stop counting.
fn projection_depth(entity: &WorldEntity, by_id: &HashMap<u64, &WorldEntity>) -> usize {
    let mut depth = 0;
    let mut current = entity.parent;
    while let Some(parent) = current {
        depth += 1;
        if depth > by_id.len() {
            break;
        }
        current = by_id.get(&parent.0).and_then(|p| p.parent);
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::*;
    use wt::{EntityId, EntityName};

    fn entity(id: u64, name: &str, parent: Option<u64>) -> WorldEntity {
        let mut e = WorldEntity::new(id, name);
        e.parent = parent.map(EntityId);
        e
    }

    fn doc_with(entities: Vec<WorldEntity>) -> WorldDoc {
        WorldDoc::from_entities("w", &entities, None).unwrap()
    }

    #[test]
    fn empty_doc_gains_everything_as_spawns_parents_first() {
        let doc = WorldDoc::new("w");
        // Given children-before-parents in the projection…
        let projection = vec![entity(2, "door", Some(5)), entity(5, "house", None)];
        let ops = diff_entities(&doc, &projection);
        assert_eq!(ops.len(), 2);
        // …the spawn ops come out parents-first.
        match &ops[0] {
            EditOp::SpawnEntity { entity } => assert_eq!(entity.name.as_str(), "house"),
            other => panic!("expected spawn, got {other:?}"),
        }
        // And they apply cleanly in order.
        let mut doc = WorldDoc::new("w");
        doc.apply_all(&ops).unwrap();
        assert_eq!(doc.get(2).unwrap().parent, Some(EntityId(5)));
    }

    #[test]
    fn matching_projection_is_no_ops() {
        let doc = doc_with(vec![entity(1, "a", None), entity(2, "b", Some(1))]);
        let projection = vec![entity(1, "a", None), entity(2, "b", Some(1))];
        assert!(diff_entities(&doc, &projection).is_empty());
    }

    #[test]
    fn deletes_go_children_first_and_cover_the_subtree() {
        let doc = doc_with(vec![
            entity(1, "house", None),
            entity(2, "roof", Some(1)),
            entity(3, "chimney", Some(2)),
            entity(4, "tree", None),
        ]);
        // Only the tree survives.
        let projection = vec![entity(4, "tree", None)];
        let ops = diff_entities(&doc, &projection);
        let deleted: Vec<u64> = ops
            .iter()
            .map(|op| match op {
                EditOp::DeleteEntity { id } => id.0,
                other => panic!("expected deletes only, got {other:?}"),
            })
            .collect();
        assert_eq!(deleted, vec![3, 2, 1]);

        // Applying them to a copy leaves exactly the tree.
        let mut next = doc.clone();
        next.apply_all(&ops).unwrap();
        assert_eq!(next.len(), 1);
        assert!(next.get_by_name("tree").is_some());
    }

    #[test]
    fn field_changes_become_a_minimal_patch() {
        let old = entity(1, "cube", None);
        let mut new = old.clone();
        new.transform.position = [3.0, 0.0, 0.0];
        new.name = EntityName::new("big-cube");
        let doc = doc_with(vec![old]);
        let ops = diff_entities(&doc, &[new]);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            EditOp::ModifyEntity { id, patch } => {
                assert_eq!(id.0, 1);
                assert!(patch.name.is_some());
                assert!(patch.transform.is_some());
                assert!(patch.shape.is_none());
                assert!(patch.parent.is_none());
            }
            other => panic!("expected modify, got {other:?}"),
        }
    }

    #[test]
    fn reparent_is_a_modify() {
        let doc = doc_with(vec![
            entity(1, "a", None),
            entity(2, "b", None),
            entity(3, "c", Some(1)),
        ]);
        let projection = vec![
            entity(1, "a", None),
            entity(2, "b", None),
            entity(3, "c", Some(2)),
        ];
        let ops = diff_entities(&doc, &projection);
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            EditOp::ModifyEntity { id, patch } => {
                assert_eq!(id.0, 3);
                assert_eq!(patch.parent, Some(Some(EntityId(2))));
            }
            other => panic!("expected modify, got {other:?}"),
        }
    }

    #[test]
    fn diff_then_apply_is_idempotent() {
        let doc = doc_with(vec![entity(1, "a", None)]);
        let projection = vec![entity(1, "renamed", None), entity(2, "child", Some(1))];
        let ops = diff_scene(&doc, &projection, None);
        let mut next = doc.clone();
        next.apply_all(&ops).unwrap();
        assert!(diff_entities(&next, &projection).is_empty());
    }

    #[test]
    fn environment_change_becomes_set_environment() {
        let doc = WorldDoc::new("w");
        let env = wt::EnvironmentDef {
            background_color: Some([0.5, 0.7, 0.9, 1.0]),
            ambient_intensity: Some(80.0),
            ambient_color: None,
            fog_density: None,
            fog_color: None,
        };
        let ops = diff_scene(&doc, &[], Some(&env));
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], EditOp::SetEnvironment { .. }));
        // Same environment again: no op.
        let mut next = doc.clone();
        next.apply_all(&ops).unwrap();
        assert!(diff_scene(&next, &[], Some(&env)).is_empty());
    }
}
