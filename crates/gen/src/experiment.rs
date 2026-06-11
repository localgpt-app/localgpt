//! Experiment tracking for headless generation.
//!
//! Each experiment is a queued world generation task with status tracking.
//! State is persisted as append-only JSONL in the XDG state directory.

use chrono::{DateTime, Utc};
use localgpt_core::text::prefix_chars_with_ellipsis;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A world generation experiment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    /// Unique experiment ID (e.g., "exp-20260316-143022-enchanted-forest").
    pub id: String,
    /// Original prompt from HEARTBEAT.md or MCP.
    pub prompt: String,
    /// Optional style hint (from memory or inline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Current status.
    pub status: ExperimentStatus,
    /// Output world skill path (set on completion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    /// Screenshot path (set on completion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot_path: Option<String>,
    /// Entity count in final scene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_count: Option<usize>,
    /// Generation duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Error message (set on failure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Timestamp when queued.
    pub queued_at: DateTime<Utc>,
    /// Timestamp when started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    /// Timestamp when completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    /// Variation group ID (if part of a variation set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_group: Option<String>,
    /// Variation axis and value (e.g., "lighting" = "sunset").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation: Option<(String, String)>,
    /// LLM model used for generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl Experiment {
    /// Create a new pending experiment.
    pub fn new(prompt: impl Into<String>, name_slug: &str) -> Self {
        let now = Utc::now();
        let timestamp = now.format("%Y%m%d-%H%M%S").to_string();
        Self {
            id: format!("exp-{}-{}", timestamp, name_slug),
            prompt: prompt.into(),
            style: None,
            status: ExperimentStatus::Pending,
            output_path: None,
            screenshot_path: None,
            entity_count: None,
            duration_ms: None,
            error: None,
            queued_at: now,
            started_at: None,
            completed_at: None,
            variation_group: None,
            variation: None,
            model: None,
        }
    }

    /// Mark as running.
    pub fn mark_running(&mut self) {
        self.status = ExperimentStatus::Running;
        self.started_at = Some(Utc::now());
    }

    /// Mark as completed with results.
    pub fn mark_completed(
        &mut self,
        output_path: String,
        screenshot_path: Option<String>,
        entity_count: usize,
        duration_ms: u64,
    ) {
        self.status = ExperimentStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.output_path = Some(output_path);
        self.screenshot_path = screenshot_path;
        self.entity_count = Some(entity_count);
        self.duration_ms = Some(duration_ms);
    }

    /// Mark as failed with error message.
    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.status = ExperimentStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.error = Some(error.into());
    }
}

/// Experiment status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExperimentStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for ExperimentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExperimentStatus::Pending => write!(f, "pending"),
            ExperimentStatus::Running => write!(f, "running"),
            ExperimentStatus::Completed => write!(f, "completed"),
            ExperimentStatus::Failed => write!(f, "failed"),
            ExperimentStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Append-only experiment log backed by JSONL.
pub struct ExperimentTracker {
    path: PathBuf,
}

impl ExperimentTracker {
    /// Create a tracker at the given XDG state directory.
    pub fn new(state_dir: &Path) -> Self {
        Self {
            path: state_dir.join("gen-experiments.jsonl"),
        }
    }

    /// Append a new or updated experiment record.
    ///
    /// Uses file locking to prevent concurrent writes from corrupting the JSONL.
    pub fn append(&self, experiment: &Experiment) -> anyhow::Result<()> {
        use std::io::Write;

        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        // Lock file for exclusive write access
        lock_file_exclusive(&file)?;

        let mut file = file;
        let line = serde_json::to_string(experiment)?;
        writeln!(file, "{}", line)?;
        // Lock released when file is dropped
        Ok(())
    }

    /// Read all experiments, deduplicating by ID (last entry wins).
    pub fn read_all(&self) -> anyhow::Result<Vec<Experiment>> {
        let content = std::fs::read_to_string(&self.path).unwrap_or_default();
        let mut map: HashMap<String, Experiment> = HashMap::new();
        for line in content.lines() {
            if let Ok(exp) = serde_json::from_str::<Experiment>(line) {
                map.insert(exp.id.clone(), exp);
            }
        }
        let mut experiments: Vec<Experiment> = map.into_values().collect();
        experiments.sort_by_key(|e| e.queued_at);
        Ok(experiments)
    }

    /// Get pending experiments.
    pub fn pending(&self) -> anyhow::Result<Vec<Experiment>> {
        Ok(self
            .read_all()?
            .into_iter()
            .filter(|e| e.status == ExperimentStatus::Pending)
            .collect())
    }

    /// Get experiments by status.
    pub fn by_status(&self, status: &ExperimentStatus) -> anyhow::Result<Vec<Experiment>> {
        Ok(self
            .read_all()?
            .into_iter()
            .filter(|e| &e.status == status)
            .collect())
    }

    /// Get a specific experiment by ID.
    pub fn get(&self, id: &str) -> anyhow::Result<Option<Experiment>> {
        Ok(self.read_all()?.into_iter().find(|e| e.id == id))
    }

    /// Compact the tracker: keep only the last N entries.
    ///
    /// Uses file locking to prevent concurrent writes during compaction.
    pub fn compact(&self, max_entries: usize) -> anyhow::Result<usize> {
        let all = self.read_all()?;
        let total = all.len();
        if total <= max_entries {
            return Ok(0);
        }

        // Keep the most recent entries
        let to_keep: Vec<&Experiment> = all.iter().rev().take(max_entries).collect();

        // Rewrite the file with exclusive lock
        use std::io::Write;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;

        lock_file_exclusive(&file)?;

        let mut file = file;
        for exp in to_keep.into_iter().rev() {
            let line = serde_json::to_string(exp)?;
            writeln!(file, "{}", line)?;
        }

        Ok(total - max_entries)
    }
}

/// Parse variation syntax from a prompt.
///
/// Format: `(variation: <axis> = <v1>, <v2>, ...)`
///
/// Returns `Some((base_prompt, axis, values))` if variation syntax found.
pub fn parse_variation(prompt: &str) -> Option<(String, String, Vec<String>)> {
    let re_start = prompt.find("(variation:")?;
    let re_end = prompt[re_start..].find(')')? + re_start;

    let variation_str = &prompt[re_start + 11..re_end].trim();
    let eq_pos = variation_str.find('=')?;

    let axis = variation_str[..eq_pos].trim().to_string();
    let values: Vec<String> = variation_str[eq_pos + 1..]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if values.is_empty() {
        return None;
    }

    // Base prompt is everything before the variation syntax
    let base = prompt[..re_start].trim().to_string();

    Some((base, axis, values))
}

/// Create a URL-safe slug from a prompt string.
pub fn prompt_to_slug(prompt: &str) -> String {
    let slug: String = prompt
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse multiple dashes and trim
    let mut result = String::new();
    let mut last_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !last_dash && !result.is_empty() {
                result.push('-');
            }
            last_dash = true;
        } else {
            result.push(c);
            last_dash = false;
        }
    }

    // Truncate to reasonable length
    let trimmed = result.trim_end_matches('-');
    let mut chars = trimmed.chars();
    let truncated: String = chars.by_ref().take(40).collect();
    if chars.next().is_some() {
        truncated.trim_end_matches('-').to_string()
    } else {
        trimmed.to_string()
    }
}

/// Build a display preview for an experiment prompt.
pub fn prompt_preview(prompt: &str, max_chars: usize) -> String {
    prefix_chars_with_ellipsis(prompt, max_chars)
}

/// Detect if HEARTBEAT.md content contains gen experiment entries.
pub fn has_gen_experiments(heartbeat_content: &str) -> bool {
    let lower = heartbeat_content.to_lowercase();
    // Explicit section header
    if lower.contains("## gen experiments") || lower.contains("## world experiments") {
        return true;
    }
    // Heuristic: unchecked items with gen-like verbs
    if lower.contains("- [ ]") {
        let gen_verbs = [
            "build a ",
            "generate a ",
            "create a world",
            "create a scene",
            "variation:",
            "lighting variation",
            "headless gen",
        ];
        return gen_verbs.iter().any(|v| lower.contains(v));
    }
    false
}

/// Lock a file for exclusive access (blocking, with timeout via retry).
#[cfg(unix)]
fn lock_file_exclusive(file: &std::fs::File) -> anyhow::Result<()> {
    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    // SAFETY: `fd` is a valid file descriptor obtained from `file` which is borrowed
    // for the duration of this call. flock(LOCK_EX) blocks until the lock is acquired
    // and has no memory safety concerns beyond requiring a valid fd.
    let ret = unsafe { libc::flock(fd, libc::LOCK_EX) };
    if ret != 0 {
        anyhow::bail!("Failed to lock experiment file");
    }
    Ok(())
}

#[cfg(not(unix))]
fn lock_file_exclusive(_file: &std::fs::File) -> anyhow::Result<()> {
    // On non-Unix, skip locking (append mode is mostly safe for single-line writes)
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_variation_basic() {
        let prompt = "Medieval village (variation: lighting = dawn, noon, sunset)";
        let result = parse_variation(prompt);
        assert!(result.is_some());
        let (base, axis, values) = result.unwrap();
        assert_eq!(base, "Medieval village");
        assert_eq!(axis, "lighting");
        assert_eq!(values, vec!["dawn", "noon", "sunset"]);
    }

    #[test]
    fn test_parse_variation_none() {
        assert!(parse_variation("Just a normal prompt").is_none());
    }

    #[test]
    fn test_prompt_to_slug() {
        assert_eq!(prompt_to_slug("Build a cozy cabin"), "build-a-cozy-cabin");
        assert_eq!(prompt_to_slug("Hello, world!"), "hello-world");
        assert_eq!(prompt_to_slug("  spaces  "), "spaces");
    }

    #[test]
    fn test_prompt_to_slug_handles_multibyte_text() {
        let slug = prompt_to_slug(&"中".repeat(20));

        assert_eq!(slug, "中".repeat(20));
    }

    #[test]
    fn test_prompt_to_slug_truncates_multibyte_by_characters() {
        let slug = prompt_to_slug(&"中".repeat(45));

        assert_eq!(slug, "中".repeat(40));
    }

    #[test]
    fn test_prompt_preview_truncates_multibyte_by_characters() {
        let preview = prompt_preview(&"✅".repeat(51), 50);

        assert_eq!(preview.chars().filter(|&c| c == '✅').count(), 50);
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn test_has_gen_experiments_section() {
        assert!(has_gen_experiments(
            "## Gen Experiments\n- [ ] Build a castle"
        ));
        assert!(has_gen_experiments("## World Experiments\n- [ ] foo"));
    }

    #[test]
    fn test_has_gen_experiments_heuristic() {
        assert!(has_gen_experiments("- [ ] Build a cozy cabin in the woods"));
        assert!(has_gen_experiments("- [ ] Generate a sci-fi world"));
        assert!(!has_gen_experiments("- [ ] Fix the CI pipeline"));
        assert!(!has_gen_experiments("- [x] Build a castle")); // already checked
    }

    #[test]
    fn test_experiment_lifecycle() {
        let mut exp = Experiment::new("Build a castle", "castle");
        assert_eq!(exp.status, ExperimentStatus::Pending);
        assert!(exp.id.starts_with("exp-"));
        assert!(exp.id.ends_with("-castle"));

        exp.mark_running();
        assert_eq!(exp.status, ExperimentStatus::Running);
        assert!(exp.started_at.is_some());

        exp.mark_completed("/skills/castle".to_string(), None, 10, 5000);
        assert_eq!(exp.status, ExperimentStatus::Completed);
        assert_eq!(exp.entity_count, Some(10));
        assert!(exp.completed_at.is_some());
    }

    #[test]
    fn test_experiment_failure() {
        let mut exp = Experiment::new("Build a castle", "castle");
        exp.mark_running();
        exp.mark_failed("LLM timeout");
        assert_eq!(exp.status, ExperimentStatus::Failed);
        assert_eq!(exp.error.as_deref(), Some("LLM timeout"));
    }
}
