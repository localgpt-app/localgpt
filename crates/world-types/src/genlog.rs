//! Generation log — append-only record of tool invocations during world generation.
//!
//! Each entry records one tool call with its arguments and result hash,
//! enabling replay, auditing, and diff-based iteration.

use serde::{Deserialize, Serialize};

/// A single generation log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct GenLogEntry {
    /// Monotonically increasing sequence number.
    pub seq: u32,
    /// Tool name that was invoked (e.g., "gen_apply_blockout").
    pub tool: String,
    /// Tool arguments as a JSON value.
    pub args: serde_json::Value,
    /// Hash of the result (for change detection).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_hash: Option<String>,
    /// Pipeline phase (e.g., "blockout", "populate", "audio").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    /// ISO 8601 timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genlog_entry_roundtrip_json() {
        let entry = GenLogEntry {
            seq: 1,
            tool: "gen_apply_blockout".to_string(),
            args: serde_json::json!({
                "regions": [
                    {"name": "courtyard", "center": [0, 0, 0], "size": [20, 1, 20]}
                ]
            }),
            result_hash: Some("sha256:abc123".to_string()),
            phase: Some("blockout".to_string()),
            timestamp: Some("2026-03-21T10:00:00Z".to_string()),
        };
        let json = serde_json::to_string_pretty(&entry).unwrap();
        let back: GenLogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.seq, 1);
        assert_eq!(back.tool, "gen_apply_blockout");
        assert_eq!(back.result_hash.as_deref(), Some("sha256:abc123"));
    }

    #[test]
    fn genlog_entry_roundtrip_ron() {
        let entry = GenLogEntry {
            seq: 0,
            tool: "gen_spawn_entity".to_string(),
            args: serde_json::json!({"name": "cube"}),
            result_hash: None,
            phase: None,
            timestamp: None,
        };
        let ron_str = ron::to_string(&entry).unwrap();
        let back: GenLogEntry = ron::from_str(&ron_str).unwrap();
        assert_eq!(back.seq, 0);
        assert_eq!(back.tool, "gen_spawn_entity");
    }

    #[test]
    fn genlog_entry_minimal() {
        // Minimal entry with only required fields
        let json = r#"{"seq":5,"tool":"gen_clear_scene","args":null}"#;
        let entry: GenLogEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.seq, 5);
        assert!(entry.result_hash.is_none());
        assert!(entry.phase.is_none());
        assert!(entry.timestamp.is_none());
    }
}
