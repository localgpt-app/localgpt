//! The op log: one JSON object per line, `ops.jsonl` — the room's durable,
//! replayable history (spec phase 5; the session package's one log — see
//! `docs/rfcs/multiplayer/session-package-format.md`).
//!
//! world-sync stays I/O-free: this module is only the serde shape and
//! line codec. The host appends every committed `ops` message; a later
//! session replays the file to rebuild the document, and a time-lapse
//! player steps through it.
//!
//! An entry's `ops` are [`SessionOp`]s: edits (which change the document)
//! plus tool, input, state and clock records (history that folds to
//! nothing). Logs written before those kinds existed hold plain `EditOp`
//! JSON and parse unchanged.

use serde::{Deserialize, Serialize};

use crate::protocol::Author;
use crate::session::SessionOp;

/// One committed batch, in the order it committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpLogEntry {
    /// The document revision after these ops applied. Only edits bump it;
    /// history-only entries carry the current revision.
    pub revision: u64,
    pub author: Author,
    pub ops: Vec<SessionOp>,
    /// Milliseconds since the Unix epoch (0 when the writer didn't clock).
    #[serde(default)]
    pub timestamp_ms: u64,
}

/// One log line (no trailing newline).
pub fn encode_line(entry: &OpLogEntry) -> Result<String, serde_json::Error> {
    serde_json::to_string(entry)
}

/// Parse one log line. Blank lines are skipped by callers before this.
pub fn decode_line(line: &str) -> Result<OpLogEntry, serde_json::Error> {
    serde_json::from_str(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use localgpt_world_types as wt;

    #[test]
    fn entry_roundtrip() {
        let entry = OpLogEntry {
            revision: 7,
            author: Author {
                peer: Some(3),
                name: "maya".into(),
            },
            ops: vec![SessionOp::Edit(Box::new(wt::EditOp::spawn(
                wt::WorldEntity::new(1, "lighthouse"),
            )))],
            timestamp_ms: 1_700_000_000_000,
        };
        let line = encode_line(&entry).unwrap();
        assert!(!line.contains('\n'));
        let back = decode_line(&line).unwrap();
        assert_eq!(back.revision, 7);
        assert_eq!(back.author.name, "maya");
        assert_eq!(back.ops.len(), 1);
    }
}
