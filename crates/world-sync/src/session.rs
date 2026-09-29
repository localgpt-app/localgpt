//! The session log's op vocabulary — what a line in `ops.jsonl` can hold.
//!
//! An op log entry is a committed batch by one author. Its `ops` are
//! [`SessionOp`]s, of which only [`SessionOp::Edit`] changes the shared
//! document; the others are history that folds to nothing — the model's
//! tool calls, a visitor's sampled input, host game state, a performance
//! clock. See `docs/rfcs/multiplayer/session-package-format.md`.
//!
//! `SessionOp` is untagged with `Edit` first, so a log written before the
//! other kinds existed (plain `EditOp` JSON) parses unchanged, and an
//! `Edit` written today serializes exactly as the old format did.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::doc::{ApplyError, WorldDoc};
use crate::oplog::OpLogEntry;
use localgpt_world_types::EditOp;

/// The session package format's version (`session.json`'s `format_version`).
pub const SESSION_FORMAT_VERSION: u32 = 1;

/// One op in a session log entry. Only `Edit` changes the document; the
/// rest is history that folds to nothing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionOp {
    /// A world edit, exactly as the room has always committed. Boxed so
    /// the history variants don't pay for an entity's size.
    Edit(Box<EditOp>),
    /// A tool call that ran during the session — the model's (or a
    /// pipeline's) intent, recorded next to the edits it caused.
    Tool(ToolRecord),
    /// A visitor's sampled state, for playthrough replay.
    Input(InputRecord),
    /// Host game state the document doesn't hold (score, inventory, …).
    State(StateRecord),
    /// A performance clock: Verse's song transport, a tour's clock.
    Clock(ClockRecord),
}

/// A tool invocation, as the old generation log recorded it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecord {
    /// Tool name (e.g., `gen_spawn_primitive`).
    pub tool: String,
    /// The arguments the caller passed, verbatim.
    pub args: serde_json::Value,
    /// Hash of the result, for change detection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_hash: Option<String>,
    /// Pipeline phase (e.g., "blockout", "populate").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// Milliseconds since the Unix epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u64>,
}

/// A visitor's sampled state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputRecord {
    /// Who was sampled (a visitor id).
    pub input: InputSample,
}

/// One input sample: where the visitor was, what they looked at, what they
/// clicked. Position and look are sampled (~10 Hz); a click is discrete.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputSample {
    /// The visitor this sample belongs to.
    pub actor: String,
    /// Position in world space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    /// Yaw and pitch, degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<[f32; 2]>,
    /// The entity a click hit, by id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click: Option<u64>,
}

/// Host game state, as a flat map (`"score.chest": 10`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateRecord {
    /// The state that changed, keyed by name.
    pub state: BTreeMap<String, serde_json::Value>,
}

/// A clock event: transport for a timed performance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockRecord {
    /// The clock's state at the event.
    pub clock: ClockState,
}

/// Where a performance clock is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockState {
    /// Whether the clock is running.
    pub playing: bool,
    /// The clock's position, seconds.
    pub position_s: f64,
}

/// `session.json` — the package's metadata and integrity record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    /// The session package format version ([`SESSION_FORMAT_VERSION`]).
    pub format_version: u32,
    /// The session's name.
    pub name: String,
    /// Which app wrote the package ("gen", "md", "verse").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// The revision `world.ron` holds.
    pub base_revision: u64,
    /// The newest revision the log reaches.
    pub head_revision: u64,
    /// The session's seed, for deterministic replay. Reserved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// SHA-256 of `world.ron`, hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_sha256: Option<String>,
    /// SHA-256 of `ops.jsonl` as of `head_revision`, hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_sha256: Option<String>,
    /// When the package was last written, milliseconds since the epoch.
    #[serde(default)]
    pub updated_ms: u64,
}

impl SessionMeta {
    /// A fresh meta for a base at revision 0.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            format_version: SESSION_FORMAT_VERSION,
            name: name.into(),
            app: None,
            base_revision: 0,
            head_revision: 0,
            seed: None,
            world_sha256: None,
            log_sha256: None,
            updated_ms: 0,
        }
    }
}

impl SessionOp {
    /// The edit this op carries, if it is one.
    pub fn as_edit(&self) -> Option<&EditOp> {
        match self {
            SessionOp::Edit(op) => Some(op),
            _ => None,
        }
    }
}

impl OpLogEntry {
    /// The entry's edits, in order — the ops that change a document.
    /// Tool, input, state and clock records are skipped.
    pub fn edit_ops(&self) -> Vec<EditOp> {
        self.ops
            .iter()
            .filter_map(|op| match op {
                SessionOp::Edit(edit) => Some((**edit).clone()),
                _ => None,
            })
            .collect()
    }

    /// True when the entry carries no edits (history only).
    pub fn is_history_only(&self) -> bool {
        self.ops.iter().all(|op| !matches!(op, SessionOp::Edit(_)))
    }
}

/// Fold log entries onto a base document: the state at the last entry.
///
/// Each entry's edits apply atomically, as the room applied them; the fold
/// stops at the first entry that no longer applies.
pub fn fold_log(base: &WorldDoc, entries: &[OpLogEntry]) -> Result<WorldDoc, ApplyError> {
    let mut doc = base.clone();
    for entry in entries {
        doc.apply_all(&entry.edit_ops())?;
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Author;
    use localgpt_world_types as wt;

    fn entity(id: u64, name: &str) -> wt::WorldEntity {
        let mut e = wt::WorldEntity::new(id, name);
        e.transform.position = [id as f32, 0.0, 0.0];
        e
    }

    #[test]
    fn edit_serializes_as_the_old_format_did() {
        let op = SessionOp::Edit(Box::new(EditOp::spawn(entity(1, "lighthouse"))));
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.starts_with("{\"SpawnEntity\""));
        let back: SessionOp = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
    }

    #[test]
    fn old_log_line_parses_as_edits() {
        // The shape world-sync wrote before session ops existed.
        let line = r#"{"revision":7,"author":{"peer":3,"name":"maya"},
            "ops":[{"SpawnEntity":{"entity":{"id":1,"name":"lighthouse"}}}],
            "timestamp_ms":1700000000000}"#;
        let entry: OpLogEntry = serde_json::from_str(line).unwrap();
        assert_eq!(entry.revision, 7);
        assert_eq!(entry.edit_ops().len(), 1);
        assert!(!entry.is_history_only());
    }

    #[test]
    fn tool_input_state_clock_roundtrip_and_skip() {
        let entry = OpLogEntry {
            revision: 42,
            author: Author {
                peer: None,
                name: "llm".into(),
            },
            ops: vec![
                SessionOp::Tool(ToolRecord {
                    tool: "gen_spawn_primitive".into(),
                    args: serde_json::json!({"name": "cube"}),
                    result_hash: Some("sha256:ab".into()),
                    phase: Some("blockout".into()),
                    timestamp_ms: Some(7),
                }),
                SessionOp::Input(InputRecord {
                    input: InputSample {
                        actor: "visitor-7".into(),
                        position: Some([3.0, 1.8, -2.0]),
                        look: None,
                        click: Some(17),
                    },
                }),
                SessionOp::State(StateRecord {
                    state: BTreeMap::from([("score.chest".into(), serde_json::json!(10))]),
                }),
                SessionOp::Clock(ClockRecord {
                    clock: ClockState {
                        playing: true,
                        position_s: 41.5,
                    },
                }),
            ],
            timestamp_ms: 9,
        };
        let line = crate::encode_line(&entry).unwrap();
        let back: OpLogEntry = crate::decode_line(&line).unwrap();
        assert_eq!(back.revision, 42);
        assert_eq!(back.ops.len(), 4);
        assert!(back.is_history_only());
        assert!(entry.is_history_only());
        assert!(entry.edit_ops().is_empty());
    }

    #[test]
    fn fold_log_applies_edits_and_skips_history() {
        let base = WorldDoc::new("base");
        let entries = vec![
            OpLogEntry {
                revision: 1,
                author: Author {
                    peer: None,
                    name: "llm".into(),
                },
                ops: vec![SessionOp::Edit(Box::new(EditOp::spawn(entity(1, "a"))))],
                timestamp_ms: 0,
            },
            OpLogEntry {
                revision: 1,
                author: Author {
                    peer: None,
                    name: "llm".into(),
                },
                ops: vec![SessionOp::Tool(ToolRecord {
                    tool: "gen_modify_entity".into(),
                    args: serde_json::json!({"entity": "a"}),
                    result_hash: None,
                    phase: None,
                    timestamp_ms: None,
                })],
                timestamp_ms: 1,
            },
            OpLogEntry {
                revision: 2,
                author: Author {
                    peer: None,
                    name: "maya".into(),
                },
                ops: vec![SessionOp::Edit(Box::new(EditOp::spawn(entity(2, "b"))))],
                timestamp_ms: 2,
            },
        ];
        let doc = fold_log(&base, &entries).unwrap();
        assert_eq!(doc.len(), 2);
        assert!(doc.contains(2));

        let broken = vec![OpLogEntry {
            revision: 3,
            author: Author {
                peer: None,
                name: "x".into(),
            },
            ops: vec![SessionOp::Edit(Box::new(EditOp::delete(wt::EntityId(99))))],
            timestamp_ms: 3,
        }];
        assert!(fold_log(&base, &broken).is_err());
    }
}
