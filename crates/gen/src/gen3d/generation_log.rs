//! Generation log — records tool invocations during worldgen for replay and debugging.
//!
//! The same invocations also queue as [`ToolRecord`]s for the session
//! package's log (`ops.jsonl`), where the room's web system drains them
//! next to the edits they caused. The in-memory entries still write the
//! skill dir's `meta/generation-log.jsonl` on save; the session log is the
//! live, unified history.

use bevy::prelude::*;
use localgpt_world_sync::ToolRecord;
use localgpt_world_types::GenLogEntry;
use serde_json::Value;
use std::io;
use std::path::Path;

/// Tracks all tool invocations during a generation session.
#[derive(Resource, Default)]
pub struct GenerationLog {
    pub entries: Vec<GenLogEntry>,
    pub current_phase: Option<String>,
    seq_counter: u32,
    /// Tool calls waiting to land in the session package's log.
    pending_session_records: Vec<ToolRecord>,
}

#[allow(dead_code)]
impl GenerationLog {
    pub fn log(&mut self, tool: &str, args: &Value, result_hash: Option<String>) {
        self.seq_counter += 1;
        self.entries.push(GenLogEntry {
            seq: self.seq_counter,
            tool: tool.to_string(),
            args: args.clone(),
            result_hash: result_hash.clone(),
            phase: self.current_phase.clone(),
            timestamp: None,
        });
        self.pending_session_records.push(ToolRecord {
            tool: tool.to_string(),
            args: args.clone(),
            result_hash,
            phase: self.current_phase.clone(),
            timestamp_ms: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
            ),
        });
    }

    pub fn set_phase(&mut self, phase: &str) {
        self.current_phase = Some(phase.to_string());
    }

    pub fn clear_phase(&mut self) {
        self.current_phase = None;
    }

    /// Take the tool calls recorded since the last drain, for the session
    /// package's log.
    pub fn take_session_records(&mut self) -> Vec<ToolRecord> {
        std::mem::take(&mut self.pending_session_records)
    }

    pub fn write_jsonl(&self, path: &Path) -> io::Result<()> {
        let lines: Vec<String> = self
            .entries
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect();
        std::fs::write(path, lines.join("\n"))
    }

    pub fn has_phases(&self) -> bool {
        self.entries.iter().any(|e| e.phase.is_some())
    }
}
