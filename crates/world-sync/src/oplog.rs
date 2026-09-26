//! The op log: one JSON object per line, `ops.jsonl` — the room's durable,
//! replayable build history (spec phase 5).
//!
//! world-sync stays I/O-free: this module is only the serde shape and
//! line codec. The host appends every committed `ops` message; a later
//! session replays the file to rebuild the document, and a time-lapse
//! player steps through it.

use serde::{Deserialize, Serialize};

use localgpt_world_types::EditOp;

use crate::protocol::Author;

/// One committed batch, in the order it committed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpLogEntry {
    /// The document revision after these ops applied.
    pub revision: u64,
    pub author: Author,
    pub ops: Vec<EditOp>,
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
            ops: vec![EditOp::spawn(wt::WorldEntity::new(1, "lighthouse"))],
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
