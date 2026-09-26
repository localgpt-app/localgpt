//! The cloud room authority (spec phase 8): committed ops applied to the
//! `world_entity` table through the same `localgpt-world-sync` document and
//! validation the LAN host uses.
//!
//! Everything here is plain functions over rows and ops — the reducers in
//! `lib.rs` are thin wrappers, so the logic is unit-testable on the host
//! without a running SpacetimeDB.

use std::collections::BTreeMap;

use localgpt_world_sync as sync;
use localgpt_world_types as wt;
use spacetimedb::{Identity, Timestamp};

use crate::WorldEntityRow;

/// Convert a table row back into a format entity.
pub fn entity_from_row(row: &WorldEntityRow) -> Option<wt::WorldEntity> {
    let mut entity = wt::WorldEntity::new(row.id, row.name.clone());
    entity.transform = wt::WorldTransform {
        position: [row.x, row.y, row.z],
        rotation_degrees: [row.rot_pitch, row.rot_yaw, row.rot_roll],
        scale: [row.scale, row.scale, row.scale],
        visible: row.visible,
    };
    entity.parent = row.parent_id.map(wt::EntityId);
    entity.shape = if row.shape_json.is_empty() {
        None
    } else {
        serde_json::from_str(&row.shape_json).ok()?
    };
    entity.material = row
        .material_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok());
    entity.light = row
        .light_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok());
    entity.behaviors = serde_json::from_str(&row.behaviors_json).unwrap_or_default();
    entity.audio = row
        .audio_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok());
    entity.mesh_asset = row
        .mesh_asset_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok());
    entity.modulations = row
        .modulations_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_default();
    entity.chunk = Some(wt::ChunkCoord {
        x: row.chunk_x,
        y: row.chunk_y,
    });
    Some(entity)
}

/// Decompose a format entity into a table row.
pub fn row_from_entity(
    entity: &wt::WorldEntity,
    owner: Option<Identity>,
    now: Timestamp,
) -> WorldEntityRow {
    let chunk = entity.chunk.unwrap_or(wt::ChunkCoord { x: 0, y: 0 });
    WorldEntityRow {
        id: entity.id.0,
        name: entity.name.0.clone(),
        entity_type: entity_kind(entity).to_string(),
        x: entity.transform.position[0],
        y: entity.transform.position[1],
        z: entity.transform.position[2],
        rot_pitch: entity.transform.rotation_degrees[0],
        rot_yaw: entity.transform.rotation_degrees[1],
        rot_roll: entity.transform.rotation_degrees[2],
        scale: entity.transform.scale[0],
        shape_json: entity
            .shape
            .as_ref()
            .map(|s| serde_json::to_string(s).unwrap_or_default())
            .unwrap_or_default(),
        material_json: entity
            .material
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default()),
        light_json: entity
            .light
            .as_ref()
            .map(|l| serde_json::to_string(l).unwrap_or_default()),
        behaviors_json: serde_json::to_string(&entity.behaviors).unwrap_or_default(),
        audio_json: entity
            .audio
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default()),
        mesh_asset_json: entity
            .mesh_asset
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap_or_default()),
        modulations_json: if entity.modulations.is_empty() {
            None
        } else {
            serde_json::to_string(&entity.modulations).ok()
        },
        parent_id: entity.parent.map(|p| p.0),
        visible: entity.transform.visible,
        chunk_x: chunk.x,
        chunk_y: chunk.y,
        owner,
        created_at: now,
    }
}

fn entity_kind(entity: &wt::WorldEntity) -> &'static str {
    if entity.shape.is_some() {
        "primitive"
    } else if entity.light.is_some() {
        "light"
    } else if entity.mesh_asset.is_some() {
        "mesh"
    } else if entity.audio.is_some() {
        "audio_emitter"
    } else {
        "group"
    }
}

/// Rebuild the shared document from the table's rows.
pub fn doc_from_rows(
    rows: impl Iterator<Item = WorldEntityRow>,
    name: &str,
) -> sync::WorldDoc {
    // from_manifest-style lenient load: dangling parents become roots.
    let entities: Vec<wt::WorldEntity> = rows.filter_map(|r| entity_from_row(&r)).collect();
    sync::WorldDoc::from_entities(name, &entities, None)
        .unwrap_or_else(|_| sync::WorldDoc::new(name))
}

/// Row writes needed to make the table match the document after a commit.
#[derive(Default)]
pub struct RowChanges {
    pub upserts: Vec<WorldEntityRow>,
    pub deletes: Vec<u64>,
}

/// The result of validating and applying a submission.
pub enum SubmitOutcome {
    Applied {
        revision: u64,
        changes: RowChanges,
        /// The batch's inverse (for per-author undo), as JSON.
        inverse_json: String,
        /// New environment, if it changed.
        env_json: Option<String>,
    },
    Rejected(String),
}

/// Validate and apply a batch against the current rows. Pure: returns the
/// row writes instead of performing them.
pub fn submit_ops(
    rows: Vec<WorldEntityRow>,
    world_name: &str,
    revision: u64,
    expected_revision: Option<u64>,
    ops: Vec<wt::EditOp>,
    owner: Option<Identity>,
    now: Timestamp,
) -> SubmitOutcome {
    if let Some(expected) = expected_revision {
        if expected != revision {
            return SubmitOutcome::Rejected(format!(
                "the world moved on (at revision {revision}, you planned against {expected}) — resync and retry"
            ));
        }
    }
    if ops.is_empty() {
        return SubmitOutcome::Rejected("empty submission".into());
    }
    if ops.len() > 256 {
        return SubmitOutcome::Rejected("too many ops (max 256)".into());
    }

    let mut doc = doc_from_rows(rows.into_iter(), world_name);
    let before: BTreeMap<u64, wt::WorldEntity> =
        doc.entities().map(|e| (e.id.0, e.clone())).collect();
    let inverse = sync::compute_inverse(&doc, &ops);
    if let Err(e) = doc.apply_all(&ops) {
        return SubmitOutcome::Rejected(e.to_string());
    }

    // Diff the new document against the old rows.
    let mut changes = RowChanges::default();
    for entity in doc.entities() {
        let changed = before.get(&entity.id.0) != Some(entity);
        if changed {
            changes.upserts.push(row_from_entity(entity, owner, now));
        }
    }
    for id in before.keys() {
        if !doc.contains(*id) {
            changes.deletes.push(*id);
        }
    }

    let inverse_json = serde_json::to_string(&inverse).unwrap_or_default();
    let env_json = doc
        .environment
        .as_ref()
        .and_then(|e| serde_json::to_string(e).ok());
    SubmitOutcome::Applied {
        revision: revision + 1,
        changes,
        inverse_json,
        env_json,
    }
}

/// Undo the batch `inverse_json` (computed by an earlier submit) — same
/// path as a normal submit, attributed to the undoing author.
pub fn apply_undo(
    rows: Vec<WorldEntityRow>,
    world_name: &str,
    revision: u64,
    inverse: Vec<wt::EditOp>,
    owner: Option<Identity>,
    now: Timestamp,
) -> SubmitOutcome {
    submit_ops(rows, world_name, revision, None, inverse, owner, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(id: u64, name: &str, parent: Option<u64>) -> wt::WorldEntity {
        let mut e = wt::WorldEntity::new(id, name);
        e.parent = parent.map(wt::EntityId);
        e
    }

    fn now() -> Timestamp {
        Timestamp::UNIX_EPOCH
    }

    fn rows_of(entities: &[wt::WorldEntity]) -> Vec<WorldEntityRow> {
        entities
            .iter()
            .map(|e| row_from_entity(e, None, now()))
            .collect()
    }

    #[test]
    fn row_round_trip_keeps_the_entity() {
        let mut e = entity(7, "lantern", Some(3));
        e.shape = Some(wt::Shape::Sphere { radius: 0.5 });
        e.transform.visible = false;
        let row = row_from_entity(&e, None, now());
        let back = entity_from_row(&row).unwrap();
        assert_eq!(back.id, e.id);
        assert_eq!(back.name, e.name);
        assert_eq!(back.parent, e.parent);
        assert_eq!(back.shape, e.shape);
        assert!(!back.transform.visible);
    }

    #[test]
    fn submit_applies_and_reports_row_changes() {
        let rows = rows_of(&[entity(1, "oak", None)]);
        let outcome = submit_ops(
            rows,
            "cloud",
            0,
            None,
            vec![
                wt::EditOp::spawn(entity(2, "stone", None)),
                wt::EditOp::delete(wt::EntityId(1)),
            ],
            None,
            now(),
        );
        match outcome {
            SubmitOutcome::Applied {
                revision, changes, ..
            } => {
                assert_eq!(revision, 1);
                assert_eq!(changes.upserts.len(), 1);
                assert_eq!(changes.upserts[0].name, "stone");
                assert_eq!(changes.deletes, vec![1]);
            }
            SubmitOutcome::Rejected(reason) => panic!("rejected: {reason}"),
        }
    }

    #[test]
    fn stale_expected_revision_rejects() {
        let rows = rows_of(&[]);
        let outcome = submit_ops(
            rows,
            "cloud",
            5,
            Some(3),
            vec![wt::EditOp::spawn(entity(1, "a", None))],
            None,
            now(),
        );
        assert!(matches!(outcome, SubmitOutcome::Rejected(_)));
    }

    #[test]
    fn invalid_batch_rejects_without_changes() {
        let rows = rows_of(&[entity(1, "oak", None)]);
        let outcome = submit_ops(
            rows,
            "cloud",
            0,
            None,
            vec![wt::EditOp::Batch {
                ops: vec![
                    wt::EditOp::spawn(entity(2, "ok", None)),
                    wt::EditOp::delete(wt::EntityId(99)),
                ],
            }],
            None,
            now(),
        );
        assert!(matches!(outcome, SubmitOutcome::Rejected(_)));
    }

    #[test]
    fn undo_restores_a_delete() {
        let rows = rows_of(&[entity(1, "house", None), entity(2, "roof", Some(1))]);
        let outcome = submit_ops(
            rows,
            "cloud",
            0,
            None,
            vec![wt::EditOp::delete(wt::EntityId(1))],
            None,
            now(),
        );
        let SubmitOutcome::Applied {
            inverse_json,
            changes,
            ..
        } = outcome
        else {
            panic!("expected applied");
        };
        assert_eq!(changes.deletes.len(), 2);

        // Undo with the rows as they now stand (both entities gone).
        let inverse: Vec<wt::EditOp> = serde_json::from_str(&inverse_json).unwrap();
        let outcome = apply_undo(vec![], "cloud", 1, inverse, None, now());
        let SubmitOutcome::Applied { changes, .. } = outcome else {
            panic!("expected applied");
        };
        let names: Vec<&str> = changes.upserts.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["house", "roof"]);
    }
}
