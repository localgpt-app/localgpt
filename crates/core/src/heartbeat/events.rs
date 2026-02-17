//! Heartbeat event tracking for UI status display

use serde::{Deserialize, Serialize};
use std::sync::RwLock;

/// Heartbeat event status
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HeartbeatStatus {
    /// Heartbeat ran and sent a response
    Sent,
    /// Heartbeat ran but nothing needed attention (HEARTBEAT_OK)
    Ok,
    /// Heartbeat was skipped (outside active hours, empty file, etc.)
    Skipped,
    /// Heartbeat was skipped transiently (a soon retry may be useful)
    SkippedMayTry,
    /// Heartbeat failed with an error
    Failed,
}

/// A heartbeat event for tracking/display
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HeartbeatEvent {
    /// Timestamp in milliseconds
    pub ts: u64,
    /// Status of the heartbeat
    pub status: HeartbeatStatus,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Preview of the response (first 200 chars)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    /// Reason for skip/failure
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Global state for last heartbeat event
static LAST_HEARTBEAT: RwLock<Option<HeartbeatEvent>> = RwLock::new(None);

/// Emit a heartbeat event (stores it for later retrieval)
pub fn emit_heartbeat_event(event: HeartbeatEvent) {
    if let Ok(mut guard) = LAST_HEARTBEAT.write() {
        *guard = Some(event);
    }
}

/// Get the last heartbeat event
pub fn get_last_heartbeat_event() -> Option<HeartbeatEvent> {
    LAST_HEARTBEAT.read().ok().and_then(|guard| guard.clone())
}

/// Helper to get current timestamp in milliseconds
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
