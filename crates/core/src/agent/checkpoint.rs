//! Compaction checkpoints — snapshot session transcripts before compaction
//!
//! Saves a copy of the session JSONL file before compaction truncates messages,
//! allowing users to restore or branch from the pre-compaction state.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Default maximum checkpoints retained per session
const DEFAULT_MAX_CHECKPOINTS: usize = 5;

/// Metadata for a single compaction checkpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionCheckpoint {
    /// Sequential checkpoint number (1, 2, 3, ...)
    pub checkpoint_number: u32,
    /// When the checkpoint was created
    pub created_at: u64,
    /// Reason for compaction
    pub reason: String,
    /// Token count before compaction
    pub tokens_before: usize,
    /// Token count after compaction (filled in after compaction completes)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_after: Option<usize>,
    /// Number of messages at checkpoint time
    pub message_count: usize,
    /// Relative path to the checkpoint file
    pub file_name: String,
}

/// Manages checkpoints for a specific agent's sessions
pub struct CheckpointManager {
    checkpoints_dir: PathBuf,
    max_checkpoints: usize,
}

impl CheckpointManager {
    pub fn new(checkpoints_dir: PathBuf, max_checkpoints: usize) -> Self {
        Self {
            checkpoints_dir,
            max_checkpoints,
        }
    }

    pub fn from_agent(agent_id: &str) -> Result<Self> {
        let paths = crate::paths::Paths::resolve()?;
        let checkpoints_dir = paths
            .state_dir
            .join("agents")
            .join(agent_id)
            .join("checkpoints");
        Ok(Self::new(checkpoints_dir, DEFAULT_MAX_CHECKPOINTS))
    }

    pub fn with_max_checkpoints(mut self, max: usize) -> Self {
        self.max_checkpoints = max;
        self
    }

    /// Save a checkpoint of the current session JSONL file.
    ///
    /// Copies the session file to the checkpoints directory with a sequential number.
    /// Returns the checkpoint metadata.
    pub fn save_checkpoint(
        &self,
        session_id: &str,
        session_file: &Path,
        tokens_before: usize,
        message_count: usize,
        reason: &str,
    ) -> Result<CompactionCheckpoint> {
        fs::create_dir_all(&self.checkpoints_dir)
            .context("Failed to create checkpoints directory")?;

        let existing = self.list_checkpoints(session_id)?;
        let next_number = existing
            .last()
            .map(|c| c.checkpoint_number + 1)
            .unwrap_or(1);

        let file_name = format!("{}.{}.jsonl", session_id, next_number);
        let checkpoint_path = self.checkpoints_dir.join(&file_name);

        // Copy session file to checkpoint location
        fs::copy(session_file, &checkpoint_path).with_context(|| {
            format!(
                "Failed to copy session file to checkpoint: {}",
                checkpoint_path.display()
            )
        })?;

        // Set restrictive permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&checkpoint_path, fs::Permissions::from_mode(0o600));
        }

        let checkpoint = CompactionCheckpoint {
            checkpoint_number: next_number,
            created_at: Utc::now().timestamp_millis() as u64,
            reason: reason.to_string(),
            tokens_before,
            tokens_after: None,
            message_count,
            file_name,
        };

        info!(
            "Saved compaction checkpoint #{} for session {} ({} tokens, {} messages)",
            next_number, session_id, tokens_before, message_count
        );

        // Prune old checkpoints if over limit
        self.prune_checkpoints(session_id)?;

        Ok(checkpoint)
    }

    /// Update the tokens_after field on the most recent checkpoint
    pub fn update_tokens_after(
        &self,
        checkpoints: &mut [CompactionCheckpoint],
        tokens_after: usize,
    ) {
        if let Some(last) = checkpoints.last_mut() {
            last.tokens_after = Some(tokens_after);
        }
    }

    /// List all checkpoints for a session, sorted by checkpoint number
    pub fn list_checkpoints(&self, session_id: &str) -> Result<Vec<CompactionCheckpoint>> {
        if !self.checkpoints_dir.exists() {
            return Ok(Vec::new());
        }

        let prefix = format!("{}.", session_id);
        let mut checkpoints = Vec::new();

        for entry in fs::read_dir(&self.checkpoints_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if !name_str.starts_with(&prefix) || !name_str.ends_with(".jsonl") {
                continue;
            }

            // Parse checkpoint number from filename: {session_id}.{n}.jsonl
            let rest = &name_str[prefix.len()..name_str.len() - ".jsonl".len()];
            if let Ok(num) = rest.parse::<u32>() {
                let metadata = entry.metadata()?;
                let created_at = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);

                checkpoints.push(CompactionCheckpoint {
                    checkpoint_number: num,
                    created_at,
                    reason: "auto".to_string(),
                    tokens_before: 0,
                    tokens_after: None,
                    message_count: 0,
                    file_name: name_str.to_string(),
                });
            }
        }

        checkpoints.sort_by_key(|c| c.checkpoint_number);
        Ok(checkpoints)
    }

    /// Get the file path for a checkpoint
    pub fn checkpoint_path(&self, file_name: &str) -> PathBuf {
        self.checkpoints_dir.join(file_name)
    }

    /// Restore a session from a checkpoint by copying the checkpoint file
    /// back to the sessions directory.
    pub fn restore_checkpoint(
        &self,
        session_id: &str,
        checkpoint_number: u32,
        sessions_dir: &Path,
    ) -> Result<PathBuf> {
        let file_name = format!("{}.{}.jsonl", session_id, checkpoint_number);
        let checkpoint_path = self.checkpoints_dir.join(&file_name);

        if !checkpoint_path.exists() {
            anyhow::bail!(
                "Checkpoint #{} not found for session {}",
                checkpoint_number,
                session_id
            );
        }

        let session_path = sessions_dir.join(format!("{}.jsonl", session_id));

        fs::copy(&checkpoint_path, &session_path).with_context(|| {
            format!(
                "Failed to restore checkpoint to session: {}",
                session_path.display()
            )
        })?;

        info!(
            "Restored session {} from checkpoint #{}",
            session_id, checkpoint_number
        );

        Ok(session_path)
    }

    /// Branch a new session from a checkpoint by loading it and saving
    /// with a new session ID.
    pub fn branch_from_checkpoint(
        &self,
        session_id: &str,
        checkpoint_number: u32,
        agent_id: &str,
    ) -> Result<String> {
        let file_name = format!("{}.{}.jsonl", session_id, checkpoint_number);
        let checkpoint_path = self.checkpoints_dir.join(&file_name);

        if !checkpoint_path.exists() {
            anyhow::bail!(
                "Checkpoint #{} not found for session {}",
                checkpoint_number,
                session_id
            );
        }

        // Copy checkpoint to sessions dir with the original session_id, load it, then branch
        let sessions_dir = super::session::get_sessions_dir_for_agent(agent_id)?;
        let temp_path = sessions_dir.join(format!("{}.jsonl", session_id));
        let had_existing = temp_path.exists();
        let existing_backup = if had_existing {
            let backup = sessions_dir.join(format!("{}.jsonl.bak", session_id));
            fs::copy(&temp_path, &backup)?;
            Some(backup)
        } else {
            None
        };

        fs::copy(&checkpoint_path, &temp_path)?;
        let session = super::Session::load_for_agent(session_id, agent_id)?;
        let branched = session.branch();
        let new_id = branched.id().to_string();
        branched.save_for_agent(agent_id)?;

        // Restore original session file if it existed
        if let Some(backup) = existing_backup {
            fs::rename(&backup, &temp_path)?;
        } else {
            let _ = fs::remove_file(&temp_path);
        }

        info!(
            "Branched session {} from checkpoint #{} of {}",
            new_id, checkpoint_number, session_id
        );

        Ok(new_id)
    }

    /// Remove old checkpoints exceeding max_checkpoints limit
    fn prune_checkpoints(&self, session_id: &str) -> Result<()> {
        let checkpoints = self.list_checkpoints(session_id)?;

        if checkpoints.len() <= self.max_checkpoints {
            return Ok(());
        }

        let to_remove = checkpoints.len() - self.max_checkpoints;
        for checkpoint in checkpoints.iter().take(to_remove) {
            let path = self.checkpoints_dir.join(&checkpoint.file_name);
            if path.exists() {
                debug!(
                    "Pruning old checkpoint: {} (#{}) for session {}",
                    checkpoint.file_name, checkpoint.checkpoint_number, session_id
                );
                let _ = fs::remove_file(&path);
            }
        }

        Ok(())
    }

    /// Remove all checkpoints for a session
    pub fn remove_all_checkpoints(&self, session_id: &str) -> Result<usize> {
        let checkpoints = self.list_checkpoints(session_id)?;
        let count = checkpoints.len();

        for checkpoint in &checkpoints {
            let path = self.checkpoints_dir.join(&checkpoint.file_name);
            let _ = fs::remove_file(&path);
        }

        Ok(count)
    }
}

/// Format a checkpoint created_at timestamp for display
pub fn format_checkpoint_time(millis: u64) -> String {
    DateTime::from_timestamp_millis(millis as i64)
        .map(|dt: DateTime<Utc>| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_session_file(dir: &Path, session_id: &str) -> PathBuf {
        let path = dir.join(format!("{}.jsonl", session_id));
        let mut file = fs::File::create(&path).unwrap();
        writeln!(
            file,
            r#"{{"type":"session","version":1,"id":"{}","timestamp":"2026-04-10T00:00:00Z","cwd":".","compactionCount":0,"memoryFlushCompactionCount":0}}"#,
            session_id
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","message":{{"role":"user","content":"hello"}},"timestamp":1000}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"type":"message","message":{{"role":"assistant","content":"hi there"}},"timestamp":2000}}"#
        )
        .unwrap();
        path
    }

    #[test]
    fn test_save_and_list_checkpoints() {
        let tmp = TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let checkpoints_dir = tmp.path().join("checkpoints");
        fs::create_dir_all(&sessions_dir).unwrap();

        let session_id = "test-session-1";
        let session_file = create_test_session_file(&sessions_dir, session_id);

        let mgr = CheckpointManager::new(checkpoints_dir.clone(), 5);

        let cp = mgr
            .save_checkpoint(session_id, &session_file, 1000, 10, "auto-threshold")
            .unwrap();

        assert_eq!(cp.checkpoint_number, 1);
        assert_eq!(cp.tokens_before, 1000);
        assert_eq!(cp.message_count, 10);
        assert_eq!(cp.reason, "auto-threshold");
        assert!(cp.tokens_after.is_none());

        let listed = mgr.list_checkpoints(session_id).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].checkpoint_number, 1);

        // Save another
        let cp2 = mgr
            .save_checkpoint(session_id, &session_file, 2000, 20, "auto-threshold")
            .unwrap();
        assert_eq!(cp2.checkpoint_number, 2);

        let listed = mgr.list_checkpoints(session_id).unwrap();
        assert_eq!(listed.len(), 2);
    }

    #[test]
    fn test_prune_checkpoints() {
        let tmp = TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let checkpoints_dir = tmp.path().join("checkpoints");
        fs::create_dir_all(&sessions_dir).unwrap();

        let session_id = "test-prune";
        let session_file = create_test_session_file(&sessions_dir, session_id);

        let mgr = CheckpointManager::new(checkpoints_dir.clone(), 2);

        // Save 3 checkpoints; max is 2, so oldest should be pruned
        mgr.save_checkpoint(session_id, &session_file, 100, 5, "auto")
            .unwrap();
        mgr.save_checkpoint(session_id, &session_file, 200, 10, "auto")
            .unwrap();
        mgr.save_checkpoint(session_id, &session_file, 300, 15, "auto")
            .unwrap();

        let listed = mgr.list_checkpoints(session_id).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].checkpoint_number, 2);
        assert_eq!(listed[1].checkpoint_number, 3);
    }

    #[test]
    fn test_restore_checkpoint() {
        let tmp = TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let checkpoints_dir = tmp.path().join("checkpoints");
        fs::create_dir_all(&sessions_dir).unwrap();

        let session_id = "test-restore";
        let session_file = create_test_session_file(&sessions_dir, session_id);
        let original_content = fs::read_to_string(&session_file).unwrap();

        let mgr = CheckpointManager::new(checkpoints_dir.clone(), 5);
        mgr.save_checkpoint(session_id, &session_file, 500, 8, "auto")
            .unwrap();

        // Overwrite the session file (simulating compaction)
        fs::write(&session_file, "compacted content").unwrap();
        assert_ne!(fs::read_to_string(&session_file).unwrap(), original_content);

        // Restore from checkpoint
        mgr.restore_checkpoint(session_id, 1, &sessions_dir)
            .unwrap();
        assert_eq!(fs::read_to_string(&session_file).unwrap(), original_content);
    }

    #[test]
    fn test_restore_nonexistent_checkpoint() {
        let tmp = TempDir::new().unwrap();
        let checkpoints_dir = tmp.path().join("checkpoints");
        let sessions_dir = tmp.path().join("sessions");
        fs::create_dir_all(&sessions_dir).unwrap();

        let mgr = CheckpointManager::new(checkpoints_dir, 5);
        let result = mgr.restore_checkpoint("nonexistent", 1, &sessions_dir);
        assert!(result.is_err());
    }

    #[test]
    fn test_remove_all_checkpoints() {
        let tmp = TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let checkpoints_dir = tmp.path().join("checkpoints");
        fs::create_dir_all(&sessions_dir).unwrap();

        let session_id = "test-remove";
        let session_file = create_test_session_file(&sessions_dir, session_id);

        let mgr = CheckpointManager::new(checkpoints_dir.clone(), 5);
        mgr.save_checkpoint(session_id, &session_file, 100, 5, "auto")
            .unwrap();
        mgr.save_checkpoint(session_id, &session_file, 200, 10, "auto")
            .unwrap();

        let removed = mgr.remove_all_checkpoints(session_id).unwrap();
        assert_eq!(removed, 2);

        let listed = mgr.list_checkpoints(session_id).unwrap();
        assert_eq!(listed.len(), 0);
    }

    #[test]
    fn test_update_tokens_after() {
        let mut checkpoints = vec![CompactionCheckpoint {
            checkpoint_number: 1,
            created_at: 1000,
            reason: "auto".to_string(),
            tokens_before: 5000,
            tokens_after: None,
            message_count: 20,
            file_name: "s.1.jsonl".to_string(),
        }];

        let mgr = CheckpointManager::new(PathBuf::from("/tmp"), 5);
        mgr.update_tokens_after(&mut checkpoints, 1500);

        assert_eq!(checkpoints[0].tokens_after, Some(1500));
    }
}
