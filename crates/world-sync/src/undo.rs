//! Computing the inverse of a committed batch, for per-author undo.
//!
//! The document computes inverses against its state *before* the batch
//! applied, so call this before [`WorldDoc::apply_all`]. Inverses are
//! themselves batches: undoing is just applying more ops, which means undo
//! travels over the wire and into the op log like any other change.

use localgpt_world_types as wt;
use wt::{EditOp, EntityPatch};

use crate::doc::WorldDoc;

/// The ops that undo `ops`, in application order (inner batches keep their
/// atomicity; the top-level sequence is reversed).
pub fn compute_inverse(doc: &WorldDoc, ops: &[EditOp]) -> Vec<EditOp> {
    ops.iter()
        .rev()
        .flat_map(|op| inverse_of(doc, op))
        .collect()
}

fn inverse_of(doc: &WorldDoc, op: &EditOp) -> Vec<EditOp> {
    match op {
        EditOp::SpawnEntity { entity } => vec![EditOp::delete(entity.id)],
        EditOp::DeleteEntity { id } => {
            // The delete took the whole subtree; restoring means spawning it
            // back parents-first (the doc still holds it pre-apply).
            doc.subtree_entities_parent_first(id.0)
                .into_iter()
                .map(|e| EditOp::spawn(e.clone()))
                .collect()
        }
        EditOp::ModifyEntity { id, patch } => doc
            .get(id.0)
            .map(|old| vec![EditOp::modify(*id, invert_patch(old, patch))])
            .unwrap_or_default(),
        EditOp::SetEnvironment { .. } => vec![EditOp::SetEnvironment {
            env: doc.environment.clone().unwrap_or_default(),
        }],
        EditOp::SetCamera { .. } => doc
            .camera
            .clone()
            .map(|camera| EditOp::SetCamera { camera })
            .into_iter()
            .collect(),
        EditOp::SetAmbience { .. } => vec![EditOp::SetAmbience {
            ambience: doc.ambience.clone(),
        }],
        EditOp::SpawnAudioEmitter { name, .. } => doc
            .get_by_name(name)
            .and_then(|e| e.audio.clone())
            .map(|audio| {
                vec![EditOp::RemoveAudioEmitter {
                    name: name.clone(),
                    audio,
                }]
            })
            .unwrap_or_default(),
        EditOp::RemoveAudioEmitter { name, .. } => doc
            .get_by_name(name)
            .and_then(|e| e.audio.clone())
            .map(|audio| {
                vec![EditOp::SpawnAudioEmitter {
                    name: name.clone(),
                    audio,
                }]
            })
            .unwrap_or_default(),
        EditOp::Batch { ops } => vec![EditOp::Batch {
            ops: compute_inverse(doc, ops),
        }],
    }
}

/// The patch that undoes `patch`, field by field, from the pre-apply state.
fn invert_patch(old: &wt::WorldEntity, patch: &EntityPatch) -> EntityPatch {
    let mut inverse = EntityPatch::default();
    if patch.name.is_some() {
        inverse.name = Some(old.name.clone());
    }
    if patch.transform.is_some() {
        inverse.transform = Some(old.transform.clone());
    }
    if patch.parent.is_some() {
        inverse.parent = Some(old.parent);
    }
    if patch.shape.is_some() {
        inverse.shape = Some(old.shape.clone());
    }
    if patch.material.is_some() {
        inverse.material = Some(old.material.clone());
    }
    if patch.light.is_some() {
        inverse.light = Some(old.light.clone());
    }
    if patch.behaviors.is_some() {
        inverse.behaviors = Some(old.behaviors.clone());
    }
    if patch.audio.is_some() {
        inverse.audio = Some(old.audio.clone());
    }
    if patch.mesh_asset.is_some() {
        inverse.mesh_asset = Some(old.mesh_asset.clone());
    }
    if patch.modulations.is_some() {
        inverse.modulations = Some(old.modulations.clone());
    }
    inverse
}

#[cfg(test)]
mod tests {
    use super::*;
    use wt::EntityId;

    fn entity(id: u64, name: &str, parent: Option<u64>) -> wt::WorldEntity {
        let mut e = wt::WorldEntity::new(id, name);
        e.parent = parent.map(EntityId);
        e
    }

    fn doc_with(entities: Vec<wt::WorldEntity>) -> WorldDoc {
        WorldDoc::from_entities("w", &entities, None).unwrap()
    }

    #[test]
    fn spawn_inverts_to_delete() {
        let doc = WorldDoc::new("w");
        let ops = vec![EditOp::spawn(entity(1, "a", None))];
        let inverse = compute_inverse(&doc, &ops);
        assert!(matches!(inverse[0], EditOp::DeleteEntity { .. }));
    }

    #[test]
    fn delete_inverts_to_respawning_the_subtree_parents_first() {
        let doc = doc_with(vec![
            entity(1, "house", None),
            entity(2, "roof", Some(1)),
            entity(3, "chimney", Some(2)),
            entity(4, "tree", None),
        ]);
        let ops = vec![EditOp::delete(EntityId(1))];
        let inverse = compute_inverse(&doc, &ops);
        let names: Vec<&str> = inverse
            .iter()
            .map(|op| match op {
                EditOp::SpawnEntity { entity } => entity.name.as_str(),
                other => panic!("expected spawns, got {other:?}"),
            })
            .collect();
        assert_eq!(names, ["house", "roof", "chimney"]);

        // Apply the delete, then its inverse: the world comes back.
        let mut next = doc.clone();
        next.apply_all(&ops).unwrap();
        next.apply_all(&inverse).unwrap();
        assert_eq!(next.len(), 4);
        assert_eq!(next.get(3).unwrap().parent, Some(EntityId(2)));
    }

    #[test]
    fn modify_inverts_field_by_field() {
        let old = entity(1, "cube", None);
        let doc = doc_with(vec![old.clone()]);
        let patch = EntityPatch {
            name: Some(wt::EntityName::new("big-cube")),
            transform: Some(wt::WorldTransform {
                position: [9.0, 0.0, 0.0],
                ..Default::default()
            }),
            ..Default::default()
        };
        let ops = vec![EditOp::modify(EntityId(1), patch)];
        let inverse = compute_inverse(&doc, &ops);
        let mut next = doc.clone();
        next.apply_all(&ops).unwrap();
        next.apply_all(&inverse).unwrap();
        let restored = next.get(1).unwrap();
        assert_eq!(restored.name.as_str(), "cube");
        assert_eq!(restored.transform.position, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn batch_inverse_is_atomic_and_applies() {
        let doc = doc_with(vec![entity(1, "a", None)]);
        let ops = vec![EditOp::Batch {
            ops: vec![
                EditOp::delete(EntityId(1)),
                EditOp::spawn(entity(2, "b", None)),
            ],
        }];
        let inverse = compute_inverse(&doc, &ops);
        let mut next = doc.clone();
        next.apply_all(&ops).unwrap();
        assert!(next.get_by_name("b").is_some());
        next.apply_all(&inverse).unwrap();
        assert!(next.get_by_name("a").is_some());
        assert!(next.get_by_name("b").is_none());
    }

    #[test]
    fn environment_inverts_to_previous() {
        let mut doc = WorldDoc::new("w");
        let env1 = wt::EnvironmentDef {
            background_color: Some([0.1, 0.1, 0.1, 1.0]),
            ..Default::default()
        };
        doc.apply(&EditOp::SetEnvironment { env: env1.clone() })
            .unwrap();
        let env2 = wt::EnvironmentDef {
            background_color: Some([0.9, 0.9, 0.9, 1.0]),
            ..Default::default()
        };
        let ops = vec![EditOp::SetEnvironment { env: env2 }];
        let inverse = compute_inverse(&doc, &ops);
        let mut next = doc.clone();
        next.apply_all(&ops).unwrap();
        next.apply_all(&inverse).unwrap();
        assert_eq!(
            next.environment.unwrap().background_color,
            env1.background_color
        );
    }
}
