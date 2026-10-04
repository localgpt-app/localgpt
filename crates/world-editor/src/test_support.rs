//! Fixtures shared by this crate's tests. Not compiled into a release build.

use openworldformat::author::Author;
use openworldformat::doc::WorldDoc;
use openworldformat::entity::{EntityPatch, WorldEntity};
use openworldformat::history::EditOp;
use openworldformat::identity::EntityId;
use openworldformat::oplog::OpLogEntry;
use openworldformat::session::SessionOp;

/// A minimal entity.
pub fn cube(id: u64, name: &str) -> WorldEntity {
    WorldEntity::new(id, name)
}

/// A base document holding one entity, id 1.
pub fn world_with_cube() -> WorldDoc {
    let mut doc = WorldDoc::new("test");
    doc.apply(&EditOp::SpawnEntity {
        entity: cube(1, "cube"),
    })
    .expect("spawning into an empty document");
    doc
}

/// An entry spawning one entity, with an explicit id and parent.
pub fn spawn_entry(id: &str, parent: Option<&str>, revision: u64) -> OpLogEntry {
    OpLogEntry {
        revision,
        author: Author {
            peer: None,
            name: "test".into(),
        },
        ops: vec![SessionOp::Edit(Box::new(EditOp::SpawnEntity {
            entity: cube(1_000 + revision, &format!("spawned-{revision}")),
        }))],
        timestamp_ms: 0,
        id: Some(id.to_string()),
        parent: parent.map(str::to_string),
        message: None,
    }
}

/// An entry moving an entity that may not exist — for testing a fold that
/// cannot reach its tip.
pub fn move_entry(target: u64, id: Option<&str>, revision: u64) -> OpLogEntry {
    let patch = EntityPatch {
        name: Some(openworldformat::identity::EntityName::new("moved")),
        ..Default::default()
    };
    OpLogEntry {
        revision,
        author: Author {
            peer: None,
            name: "test".into(),
        },
        ops: vec![SessionOp::Edit(Box::new(EditOp::ModifyEntity {
            id: EntityId(target),
            patch,
        }))],
        timestamp_ms: 0,
        id: id.map(str::to_string),
        parent: None,
        message: None,
    }
}

/// A linear log of `n` spawn entries, ids `e1`..`en`, each the child of the
/// one before.
pub fn linear_log(n: u64) -> Vec<OpLogEntry> {
    (1..=n)
        .map(|i| {
            let parent = (i > 1).then(|| format!("e{}", i - 1));
            spawn_entry(&format!("e{i}"), parent.as_deref(), i)
        })
        .collect()
}
