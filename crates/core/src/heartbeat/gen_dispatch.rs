//! Gen experiment dispatch for the heartbeat runner.
//!
//! Detects gen experiment entries in HEARTBEAT.md, dispatches them by spawning
//! `localgpt-gen headless` as a subprocess, and updates HEARTBEAT.md with results.
//!
//! This module lives in `localgpt-core` (not `localgpt-gen`) so the heartbeat
//! runner can dispatch gen work without pulling Bevy into the CLI binary.
//! The actual generation happens in the separate `localgpt-gen` binary.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::text::prefix_chars_with_ellipsis;

/// Detect if HEARTBEAT.md content contains gen experiment entries.
///
/// Mirrors the heuristic from `localgpt-gen::experiment::has_gen_experiments`
/// without importing the gen crate.
pub fn has_gen_markers(content: &str) -> bool {
    let lower = content.to_lowercase();

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

/// Extract the first unchecked gen experiment line from HEARTBEAT.md content.
///
/// Returns `Some((line_index, prompt_text))` for the first match, or `None`.
pub fn extract_first_gen_experiment(content: &str) -> Option<(usize, String)> {
    let gen_verbs = [
        "build a ",
        "generate a ",
        "create a world",
        "create a scene",
        "variation:",
        "lighting variation",
        "headless gen",
    ];

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if !trimmed.starts_with("- [ ]") {
            continue;
        }

        let lower = trimmed.to_lowercase();
        let is_gen = gen_verbs.iter().any(|v| lower.contains(v));
        if !is_gen {
            continue;
        }

        // Extract the prompt text (strip the "- [ ] " prefix)
        let prompt = trimmed
            .strip_prefix("- [ ]")
            .unwrap_or(trimmed)
            .trim()
            .to_string();

        if !prompt.is_empty() {
            return Some((idx, prompt));
        }
    }

    None
}

/// Result of a gen experiment dispatch.
#[derive(Debug)]
pub struct GenDispatchResult {
    /// The prompt that was dispatched.
    pub prompt: String,
    /// Whether the generation succeeded.
    pub success: bool,
    /// Generation duration.
    pub duration: Duration,
    /// Error message on failure.
    pub error: Option<String>,
    /// Output path (world skill directory) on success.
    pub output_path: Option<String>,
}

/// Dispatch a single gen experiment by spawning `localgpt-gen headless`.
///
/// Returns `Ok(result)` with the dispatch outcome, or `Err` if we couldn't
/// even start the subprocess.
pub fn dispatch_gen_experiment(
    prompt: &str,
    timeout_secs: u64,
    model: Option<&str>,
    style: Option<&str>,
) -> Result<GenDispatchResult> {
    let gen_bin = which::which("localgpt-gen").context(
        "localgpt-gen binary not found in PATH. \
         Install it with: cargo install --path crates/gen",
    )?;

    let mut cmd = Command::new(&gen_bin);
    cmd.arg("headless");
    cmd.arg("--prompt").arg(prompt);
    cmd.arg("--timeout").arg(timeout_secs.to_string());

    if let Some(m) = model {
        cmd.arg("--model").arg(m);
    }
    if let Some(s) = style {
        cmd.arg("--style").arg(s);
    }

    info!(
        name: "Heartbeat",
        "dispatching gen experiment: {:?} (timeout: {}s)",
        prompt, timeout_secs
    );

    let start = Instant::now();

    let output = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .context("failed to spawn localgpt-gen headless")?;

    let duration = start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        // Try to extract world path from stdout (localgpt-gen prints it)
        let output_path = stdout
            .lines()
            .find(|l| l.contains("skills/") || l.contains("world.ron"))
            .map(|l| l.trim().to_string());

        info!(
            name: "Heartbeat",
            "gen experiment completed in {:?}: {:?}", duration, prompt
        );

        Ok(GenDispatchResult {
            prompt: prompt.to_string(),
            success: true,
            duration,
            error: None,
            output_path,
        })
    } else {
        let error_msg = if stderr.is_empty() {
            format!("localgpt-gen exited with status {}", output.status)
        } else {
            // Take last few lines of stderr for the error
            stderr
                .lines()
                .rev()
                .take(5)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n")
        };

        warn!(
            name: "Heartbeat",
            "gen experiment failed in {:?}: {}", duration, error_msg
        );

        Ok(GenDispatchResult {
            prompt: prompt.to_string(),
            success: false,
            duration,
            error: Some(error_msg),
            output_path: None,
        })
    }
}

/// Update HEARTBEAT.md content to check off a completed experiment or mark it as failed.
///
/// Returns the updated content.
pub fn update_heartbeat_entry(
    content: &str,
    line_index: usize,
    result: &GenDispatchResult,
) -> String {
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    if line_index >= lines.len() {
        return content.to_string();
    }

    let original = &lines[line_index];
    if result.success {
        // Check off: "- [ ] Build a castle" → "- [x] Build a castle"
        lines[line_index] = original.replacen("- [ ]", "- [x]", 1);
        debug!(
            name: "Heartbeat",
            "checked off HEARTBEAT.md line {}: {}", line_index, lines[line_index]
        );
    } else {
        // Mark with error: "- [ ] Build a castle" → "- [!] Build a castle (FAILED: ...)"
        let error_note = result
            .error
            .as_deref()
            .unwrap_or("unknown error")
            .lines()
            .next()
            .unwrap_or("unknown error");
        let truncated = error_note_preview(error_note);
        lines[line_index] = original.replacen("- [ ]", "- [!]", 1);
        lines[line_index] = format!("{} (FAILED: {})", lines[line_index], truncated);
        warn!(
            name: "Heartbeat",
            "marked HEARTBEAT.md line {} as failed: {}", line_index, truncated
        );
    }

    lines.join("\n")
}

fn error_note_preview(error_note: &str) -> String {
    prefix_chars_with_ellipsis(error_note, 80)
}

/// Run gen experiment dispatch as a pre-step in the heartbeat tick.
///
/// If HEARTBEAT.md has gen experiments and `localgpt-gen` is available,
/// dispatches ONE experiment and updates HEARTBEAT.md.
///
/// Returns `true` if an experiment was dispatched (caller may want to re-read HEARTBEAT.md).
pub fn try_dispatch_gen_experiment(
    workspace: &Path,
    timeout_secs: u64,
    model: Option<&str>,
) -> bool {
    let heartbeat_path = workspace.join("HEARTBEAT.md");

    let content = match std::fs::read_to_string(&heartbeat_path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    if !has_gen_markers(&content) {
        return false;
    }

    // Check if localgpt-gen is available before parsing
    if which::which("localgpt-gen").is_err() {
        debug!(
            name: "Heartbeat",
            "gen experiments detected but localgpt-gen not in PATH, skipping"
        );
        return false;
    }

    let Some((line_idx, prompt)) = extract_first_gen_experiment(&content) else {
        return false;
    };

    info!(
        name: "Heartbeat",
        "found gen experiment at line {}: {:?}", line_idx, prompt
    );

    // Dispatch the experiment
    match dispatch_gen_experiment(&prompt, timeout_secs, model, None) {
        Ok(result) => {
            // Update HEARTBEAT.md
            let updated = update_heartbeat_entry(&content, line_idx, &result);
            if let Err(e) = std::fs::write(&heartbeat_path, &updated) {
                warn!(
                    name: "Heartbeat",
                    "failed to update HEARTBEAT.md after gen dispatch: {}", e
                );
            }
            true
        }
        Err(e) => {
            warn!(
                name: "Heartbeat",
                "failed to dispatch gen experiment: {}", e
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_gen_markers_section_header() {
        assert!(has_gen_markers("## Gen Experiments\n- [ ] Build a castle"));
        assert!(has_gen_markers("## World Experiments\n- [ ] foo"));
    }

    #[test]
    fn test_has_gen_markers_heuristic() {
        assert!(has_gen_markers("- [ ] Build a cozy cabin in the woods"));
        assert!(has_gen_markers("- [ ] Generate a sci-fi world"));
        assert!(has_gen_markers("- [ ] Create a world with medieval theme"));
        assert!(!has_gen_markers("- [ ] Fix the CI pipeline"));
        assert!(!has_gen_markers("- [x] Build a castle")); // already checked
        assert!(!has_gen_markers("No checkboxes here"));
    }

    #[test]
    fn test_extract_first_gen_experiment() {
        let content = "# Tasks\n- [x] Done task\n- [ ] Fix bug\n- [ ] Build a cozy cabin\n- [ ] Generate a space station";
        let result = extract_first_gen_experiment(content);
        assert_eq!(result, Some((3, "Build a cozy cabin".to_string())));
    }

    #[test]
    fn test_extract_skips_non_gen_items() {
        let content = "- [ ] Fix the tests\n- [ ] Update docs";
        assert_eq!(extract_first_gen_experiment(content), None);
    }

    #[test]
    fn test_extract_skips_checked_items() {
        let content = "- [x] Build a castle\n- [ ] Generate a garden";
        let result = extract_first_gen_experiment(content);
        assert_eq!(result, Some((1, "Generate a garden".to_string())));
    }

    #[test]
    fn test_update_heartbeat_success() {
        let content = "- [ ] Build a castle\n- [ ] Other task";
        let result = GenDispatchResult {
            prompt: "Build a castle".to_string(),
            success: true,
            duration: Duration::from_secs(30),
            error: None,
            output_path: Some("/skills/castle".to_string()),
        };
        let updated = update_heartbeat_entry(content, 0, &result);
        assert!(updated.starts_with("- [x] Build a castle"));
        assert!(updated.contains("- [ ] Other task"));
    }

    #[test]
    fn test_update_heartbeat_failure() {
        let content = "- [ ] Build a castle\n- [ ] Other task";
        let result = GenDispatchResult {
            prompt: "Build a castle".to_string(),
            success: false,
            duration: Duration::from_secs(5),
            error: Some("LLM timeout".to_string()),
            output_path: None,
        };
        let updated = update_heartbeat_entry(content, 0, &result);
        assert!(updated.starts_with("- [!] Build a castle (FAILED: LLM timeout)"));
    }

    #[test]
    fn test_update_heartbeat_failure_truncates_multibyte_error_by_characters() {
        let content = "- [ ] Build a castle";
        let result = GenDispatchResult {
            prompt: "Build a castle".to_string(),
            success: false,
            duration: Duration::from_secs(5),
            error: Some("✅".repeat(81)),
            output_path: None,
        };

        let updated = update_heartbeat_entry(content, 0, &result);

        assert!(updated.starts_with("- [!] Build a castle (FAILED: "));
        assert_eq!(updated.chars().filter(|&c| c == '✅').count(), 80);
        assert!(updated.contains("...)"));
    }

    #[test]
    fn test_error_note_preview_preserves_exact_limit() {
        assert_eq!(error_note_preview(&"x".repeat(80)), "x".repeat(80));
    }

    #[test]
    fn test_update_heartbeat_invalid_index() {
        let content = "- [ ] Build a castle";
        let result = GenDispatchResult {
            prompt: "test".to_string(),
            success: true,
            duration: Duration::from_secs(1),
            error: None,
            output_path: None,
        };
        let updated = update_heartbeat_entry(content, 99, &result);
        assert_eq!(updated, content);
    }
}
