//! Session store - manages sessions.json metadata file
//!
//! Matches OpenClaw's session store format for compatibility:
//! ~/.local/state/localgpt/agents/<agentId>/sessions/sessions.json

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::debug;

use super::session::{DEFAULT_AGENT_ID, get_sessions_dir_for_agent};

/// Session entry in sessions.json (matches OpenClaw's SessionEntry)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    /// Internal session ID (UUID)
    pub session_id: String,

    /// Last update timestamp (milliseconds since epoch)
    pub updated_at: u64,

    /// CLI session IDs per provider (e.g., "claude-cli" -> "session-uuid")
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cli_session_ids: HashMap<String, String>,

    /// Legacy field for backward compatibility with OpenClaw
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_cli_session_id: Option<String>,

    /// Token usage tracking
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,

    /// Compaction count
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_count: Option<u32>,

    /// Memory flush tracking (which compaction cycle flush ran for)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_flush_compaction_count: Option<u32>,

    /// Estimated cumulative cost in USD
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,

    /// Last heartbeat text that was delivered (for deduplication)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_text: Option<String>,

    /// Timestamp when last heartbeat was sent (milliseconds since epoch)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_heartbeat_sent_at: Option<u64>,
}

impl SessionEntry {
    pub fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_string(),
            updated_at: chrono::Utc::now().timestamp_millis() as u64,
            ..Default::default()
        }
    }

    /// Get CLI session ID for a provider
    pub fn get_cli_session_id(&self, provider: &str) -> Option<&str> {
        // Try the map first
        if let Some(id) = self.cli_session_ids.get(provider)
            && !id.is_empty()
        {
            return Some(id.as_str());
        }

        // Fallback to legacy field for claude-cli
        if provider == "claude-cli"
            && let Some(ref id) = self.claude_cli_session_id
            && !id.is_empty()
        {
            return Some(id.as_str());
        }

        None
    }

    /// Set CLI session ID for a provider
    pub fn set_cli_session_id(&mut self, provider: &str, session_id: &str) {
        self.cli_session_ids
            .insert(provider.to_string(), session_id.to_string());

        // Also set legacy field for claude-cli compatibility
        if provider == "claude-cli" {
            self.claude_cli_session_id = Some(session_id.to_string());
        }

        self.updated_at = chrono::Utc::now().timestamp_millis() as u64;
    }

    /// Clear CLI session IDs (called on `/new` to force a fresh CLI session)
    pub fn clear_cli_session_ids(&mut self) {
        self.cli_session_ids.clear();
        self.claude_cli_session_id = None;
        self.updated_at = chrono::Utc::now().timestamp_millis() as u64;
    }

    /// Check if a heartbeat would be a duplicate (same text within 24 hours)
    pub fn is_duplicate_heartbeat(&self, text: &str) -> bool {
        const DEDUP_WINDOW_MS: u64 = 24 * 60 * 60 * 1000; // 24 hours

        if let (Some(last_text), Some(last_sent_at)) =
            (&self.last_heartbeat_text, self.last_heartbeat_sent_at)
        {
            let now = chrono::Utc::now().timestamp_millis() as u64;
            let within_window = now.saturating_sub(last_sent_at) < DEDUP_WINDOW_MS;
            let same_text = last_text == text;

            return within_window && same_text;
        }

        false
    }

    /// Record that a heartbeat was sent
    pub fn record_heartbeat(&mut self, text: &str) {
        self.last_heartbeat_text = Some(text.to_string());
        self.last_heartbeat_sent_at = Some(chrono::Utc::now().timestamp_millis() as u64);
        self.updated_at = chrono::Utc::now().timestamp_millis() as u64;
    }
}

/// Session store - manages the sessions.json file
pub struct SessionStore {
    path: PathBuf,
    entries: HashMap<String, SessionEntry>,
}

impl SessionStore {
    /// Load session store for the default agent
    pub fn load() -> Result<Self> {
        Self::load_for_agent(DEFAULT_AGENT_ID)
    }

    /// Load session store for a specific agent
    pub fn load_for_agent(agent_id: &str) -> Result<Self> {
        let sessions_dir = get_sessions_dir_for_agent(agent_id)?;
        let path = sessions_dir.join("sessions.json");

        let entries = if path.exists() {
            let content = fs::read_to_string(&path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            HashMap::new()
        };

        debug!(
            "Loaded session store from {:?} ({} entries)",
            path,
            entries.len()
        );

        Ok(Self { path, entries })
    }

    /// Save session store to disk using atomic write (temp file + rename).
    pub fn save(&self) -> Result<()> {
        // Ensure directory exists
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(&self.entries)?;

        // Write to a unique temp file then atomically rename
        let tmp_path = self.path.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            uuid::Uuid::new_v4().as_simple()
        ));

        fs::write(&tmp_path, &content)?;
        fs::rename(&tmp_path, &self.path)?;

        // Restrict permissions: session metadata may contain sensitive info
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            fs::set_permissions(&self.path, perms)?;
        }

        debug!("Saved session store to {:?}", self.path);
        Ok(())
    }

    /// Get a session entry by key (typically "main" for the default session)
    pub fn get(&self, session_key: &str) -> Option<&SessionEntry> {
        self.entries.get(session_key)
    }

    /// Get or create a session entry
    pub fn get_or_create(&mut self, session_key: &str, session_id: &str) -> &mut SessionEntry {
        self.entries
            .entry(session_key.to_string())
            .or_insert_with(|| SessionEntry::new(session_id))
    }

    /// Update a session entry
    pub fn update<F>(&mut self, session_key: &str, session_id: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut SessionEntry),
    {
        let entry = self.get_or_create(session_key, session_id);
        f(entry);
        entry.updated_at = chrono::Utc::now().timestamp_millis() as u64;
        self.save()
    }

    /// Re-read the store from disk, apply a mutation, and save atomically.
    ///
    /// This prevents clobbering when multiple processes or tasks update
    /// the same sessions.json concurrently (e.g., heartbeat dedup).
    pub fn load_and_update<F>(&mut self, session_key: &str, session_id: &str, f: F) -> Result<()>
    where
        F: FnOnce(&mut SessionEntry),
    {
        // Re-read from disk to pick up changes from other processes
        if self.path.exists() {
            let content = fs::read_to_string(&self.path)?;
            self.entries = serde_json::from_str(&content).unwrap_or_default();
        }

        let entry = self.get_or_create(session_key, session_id);
        f(entry);
        entry.updated_at = chrono::Utc::now().timestamp_millis() as u64;
        self.save()
    }

    /// Get CLI session ID for a session and provider
    pub fn get_cli_session_id(&self, session_key: &str, provider: &str) -> Option<String> {
        self.get(session_key)
            .and_then(|e| e.get_cli_session_id(provider))
            .map(|s| s.to_string())
    }

    /// Set CLI session ID for a session and provider
    pub fn set_cli_session_id(
        &mut self,
        session_key: &str,
        session_id: &str,
        provider: &str,
        cli_session_id: &str,
    ) -> Result<()> {
        self.update(session_key, session_id, |entry| {
            entry.set_cli_session_id(provider, cli_session_id);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_entry_cli_session() {
        let mut entry = SessionEntry::new("test-session");

        // Initially empty
        assert!(entry.get_cli_session_id("claude-cli").is_none());

        // Set and get
        entry.set_cli_session_id("claude-cli", "cli-123");
        assert_eq!(entry.get_cli_session_id("claude-cli"), Some("cli-123"));

        // Legacy field should also be set
        assert_eq!(entry.claude_cli_session_id, Some("cli-123".to_string()));
    }

    #[test]
    fn test_session_entry_token_tracking() {
        let mut entry = SessionEntry::new("test-session");

        // Initially None
        assert!(entry.input_tokens.is_none());
        assert!(entry.output_tokens.is_none());
        assert!(entry.total_tokens.is_none());

        // Set token values
        entry.input_tokens = Some(100);
        entry.output_tokens = Some(50);
        entry.total_tokens = Some(150);

        // Verify they serialize/deserialize correctly
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: SessionEntry = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.input_tokens, Some(100));
        assert_eq!(deserialized.output_tokens, Some(50));
        assert_eq!(deserialized.total_tokens, Some(150));
    }

    #[test]
    fn test_session_entry_defaults() {
        // Empty JSON should deserialize with defaults
        let json = r#"{"sessionId": "test", "updatedAt": 0}"#;
        let entry: SessionEntry = serde_json::from_str(json).unwrap();

        assert_eq!(entry.session_id, "test");
        assert!(entry.cli_session_ids.is_empty());
        assert!(entry.input_tokens.is_none());
        assert!(entry.compaction_count.is_none());
    }

    #[test]
    fn test_atomic_save_no_tmp_files() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sessions.json");

        let mut store = SessionStore {
            path: path.clone(),
            entries: HashMap::new(),
        };

        store.get_or_create("main", "test-session-1");
        store.save().unwrap();

        // Verify the file exists and is valid JSON
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        let parsed: HashMap<String, SessionEntry> = serde_json::from_str(&content).unwrap();
        assert!(parsed.contains_key("main"));

        // Verify no .tmp files remain
        let tmp_files: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "tmp")
                    .unwrap_or(false)
            })
            .collect();
        assert!(
            tmp_files.is_empty(),
            "No .tmp files should remain after save, found: {:?}",
            tmp_files
        );
    }

    #[test]
    fn test_atomic_save_produces_valid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sessions.json");

        let mut store = SessionStore {
            path: path.clone(),
            entries: HashMap::new(),
        };

        let entry = store.get_or_create("main", "session-abc");
        entry.input_tokens = Some(1000);
        entry.output_tokens = Some(500);

        store.save().unwrap();

        // Re-read and verify
        let content = fs::read_to_string(&path).unwrap();
        let parsed: HashMap<String, SessionEntry> = serde_json::from_str(&content).unwrap();
        let main = parsed.get("main").unwrap();
        assert_eq!(main.session_id, "session-abc");
        assert_eq!(main.input_tokens, Some(1000));
        assert_eq!(main.output_tokens, Some(500));
    }
}
