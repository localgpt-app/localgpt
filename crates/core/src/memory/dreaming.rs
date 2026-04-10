//! Dreaming — background memory consolidation
//!
//! Scans completed session transcripts, extracts durable knowledge signals
//! (preferences, facts, decisions, tasks), and appends them to workspace
//! memory files for future recall.
//!
//! Inspired by OpenClaw's three-phase dreaming system, simplified to:
//! 1. Scan unprocessed sessions (idempotent via processed log)
//! 2. Extract signals via regex patterns
//! 3. Append to memory/dreaming/YYYY-MM-DD.md

use anyhow::{Context, Result};
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tracing::{debug, info};

/// Configuration for the dreaming system
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DreamingConfig {
    /// Enable dreaming. Default: false.
    pub enabled: bool,
    /// Maximum sessions to process per sweep. Default: 10.
    pub max_sessions_per_sweep: usize,
    /// Maximum messages to extract per session. Default: 100.
    pub max_messages_per_session: usize,
    /// Minimum session age in seconds before processing (avoid active sessions). Default: 3600 (1h).
    pub min_session_age_secs: u64,
}

impl Default for DreamingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_sessions_per_sweep: 10,
            max_messages_per_session: 100,
            min_session_age_secs: 3600,
        }
    }
}

/// A knowledge signal extracted from a session
#[derive(Debug, Clone, Serialize)]
pub struct Signal {
    pub signal_type: SignalType,
    pub content: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SignalType {
    Preference,
    Fact,
    Decision,
    Task,
}

impl std::fmt::Display for SignalType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignalType::Preference => write!(f, "preference"),
            SignalType::Fact => write!(f, "fact"),
            SignalType::Decision => write!(f, "decision"),
            SignalType::Task => write!(f, "task"),
        }
    }
}

/// Tracks which sessions have been processed to prevent re-processing
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessedLog {
    pub processed_sessions: HashMap<String, ProcessedEntry>,
    pub last_sweep: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedEntry {
    pub processed_at: String,
    pub signal_count: usize,
}

impl ProcessedLog {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn is_processed(&self, session_id: &str) -> bool {
        self.processed_sessions.contains_key(session_id)
    }

    pub fn mark_processed(&mut self, session_id: &str, signal_count: usize) {
        self.processed_sessions.insert(
            session_id.to_string(),
            ProcessedEntry {
                processed_at: Utc::now().to_rfc3339(),
                signal_count,
            },
        );
        self.last_sweep = Some(Utc::now().to_rfc3339());
    }
}

/// Signal classifier using regex patterns
pub struct SignalClassifier {
    preference_patterns: Vec<Regex>,
    fact_patterns: Vec<Regex>,
    decision_patterns: Vec<Regex>,
    task_patterns: Vec<Regex>,
}

impl SignalClassifier {
    pub fn new() -> Self {
        Self {
            preference_patterns: vec![
                Regex::new(r"(?i)\b(i\s+prefer|i\s+like|i\s+want|i\s+always|my\s+favorite|i\s+don't\s+like|i\s+hate|i\s+love)\b").unwrap(),
                Regex::new(r"(?i)\b(please\s+always|please\s+never|from\s+now\s+on|going\s+forward)\b").unwrap(),
            ],
            fact_patterns: vec![
                Regex::new(r"(?i)\b(my\s+name\s+is|i\s+work\s+at|i\s+am\s+a|i\s+live\s+in|my\s+email|my\s+company)\b").unwrap(),
                Regex::new(r"(?i)\b(we\s+use|our\s+team|our\s+stack|our\s+project|our\s+codebase)\b").unwrap(),
            ],
            decision_patterns: vec![
                Regex::new(r"(?i)\b(let's\s+go\s+with|decided\s+to|we\s+chose|i'll\s+use|settled\s+on)\b").unwrap(),
                Regex::new(r"(?i)\b(the\s+plan\s+is|we're\s+going\s+to|i'm\s+going\s+to)\b").unwrap(),
            ],
            task_patterns: vec![
                Regex::new(r"(?i)\b(todo|need\s+to|remember\s+to|don't\s+forget|make\s+sure\s+to)\b").unwrap(),
                Regex::new(r"(?i)\b(next\s+step|action\s+item|follow\s+up)\b").unwrap(),
            ],
        }
    }

    /// Classify a message and return any detected signals
    pub fn classify(&self, content: &str, session_id: &str) -> Vec<Signal> {
        let mut signals = Vec::new();

        // Only classify user messages (they contain the user's intent/preferences)
        // Skip very short messages
        if content.len() < 15 {
            return signals;
        }

        // Truncate very long messages to first 500 chars for pattern matching
        let text = if content.len() > 500 {
            &content[..500]
        } else {
            content
        };

        for pattern in &self.preference_patterns {
            if pattern.is_match(text) {
                signals.push(Signal {
                    signal_type: SignalType::Preference,
                    content: truncate_signal(content),
                    session_id: session_id.to_string(),
                });
                break; // One signal per type per message
            }
        }

        for pattern in &self.fact_patterns {
            if pattern.is_match(text) {
                signals.push(Signal {
                    signal_type: SignalType::Fact,
                    content: truncate_signal(content),
                    session_id: session_id.to_string(),
                });
                break;
            }
        }

        for pattern in &self.decision_patterns {
            if pattern.is_match(text) {
                signals.push(Signal {
                    signal_type: SignalType::Decision,
                    content: truncate_signal(content),
                    session_id: session_id.to_string(),
                });
                break;
            }
        }

        for pattern in &self.task_patterns {
            if pattern.is_match(text) {
                signals.push(Signal {
                    signal_type: SignalType::Task,
                    content: truncate_signal(content),
                    session_id: session_id.to_string(),
                });
                break;
            }
        }

        signals
    }
}

impl Default for SignalClassifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Truncate a signal content to a reasonable length
fn truncate_signal(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.len() <= 200 {
        trimmed.to_string()
    } else {
        format!("{}...", &trimmed[..197])
    }
}

/// Run a single dreaming sweep: scan sessions, extract signals, write to memory
pub fn run_sweep(
    config: &DreamingConfig,
    sessions_dir: &Path,
    workspace: &Path,
    log_path: &Path,
) -> Result<SweepResult> {
    if !config.enabled {
        return Ok(SweepResult::default());
    }

    let mut log = ProcessedLog::load(log_path)?;
    let classifier = SignalClassifier::new();
    let now = Utc::now();
    let min_age = std::time::Duration::from_secs(config.min_session_age_secs);

    // Find unprocessed session files
    let mut session_files = Vec::new();
    if sessions_dir.exists() {
        for entry in fs::read_dir(sessions_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();

                // Skip non-UUID filenames (e.g., sessions.json)
                if stem.len() < 32 {
                    continue;
                }

                // Skip already processed
                if log.is_processed(&stem) {
                    continue;
                }

                // Skip recently modified (active sessions)
                if let Ok(metadata) = entry.metadata()
                    && let Ok(modified) = metadata.modified()
                    && let Ok(age) = modified.elapsed()
                    && age < min_age
                {
                    debug!("Skipping recent session: {}", stem);
                    continue;
                }

                session_files.push((stem.to_string(), path));
            }
        }
    }

    // Limit sessions per sweep
    session_files.truncate(config.max_sessions_per_sweep);

    if session_files.is_empty() {
        debug!("Dreaming: no unprocessed sessions found");
        log.last_sweep = Some(now.to_rfc3339());
        log.save(log_path)?;
        return Ok(SweepResult::default());
    }

    info!("Dreaming: processing {} session(s)", session_files.len());

    let mut all_signals = Vec::new();

    for (session_id, path) in &session_files {
        let signals = extract_signals_from_session(
            path,
            session_id,
            &classifier,
            config.max_messages_per_session,
        )?;

        let count = signals.len();
        all_signals.extend(signals);
        log.mark_processed(session_id, count);
    }

    // Write signals to memory
    let signals_written = if !all_signals.is_empty() {
        write_signals_to_memory(&all_signals, workspace, &now.format("%Y-%m-%d").to_string())?
    } else {
        0
    };

    log.last_sweep = Some(now.to_rfc3339());
    log.save(log_path)?;

    let result = SweepResult {
        sessions_processed: session_files.len(),
        signals_extracted: all_signals.len(),
        signals_written,
    };

    info!(
        "Dreaming sweep complete: {} sessions, {} signals extracted, {} written",
        result.sessions_processed, result.signals_extracted, result.signals_written
    );

    Ok(result)
}

/// Extract signals from a single session JSONL file
fn extract_signals_from_session(
    path: &Path,
    session_id: &str,
    classifier: &SignalClassifier,
    max_messages: usize,
) -> Result<Vec<Signal>> {
    let file = fs::File::open(path)
        .with_context(|| format!("Failed to open session: {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut signals = Vec::new();
    let mut message_count = 0;

    for line in reader.lines() {
        let line = line?;
        if line.is_empty() {
            continue;
        }

        // Parse JSONL entry
        let entry: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Only process message entries
        if entry.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }

        // Only classify user messages
        let role = entry
            .pointer("/message/role")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if role != "user" {
            continue;
        }

        let content = entry
            .pointer("/message/content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if content.is_empty() {
            continue;
        }

        message_count += 1;
        if message_count > max_messages {
            break;
        }

        signals.extend(classifier.classify(content, session_id));
    }

    Ok(signals)
}

/// Write extracted signals to a dreaming memory file
fn write_signals_to_memory(signals: &[Signal], workspace: &Path, date: &str) -> Result<usize> {
    let dreaming_dir = workspace.join("memory").join("dreaming");
    fs::create_dir_all(&dreaming_dir)?;

    let file_path = dreaming_dir.join(format!("{}.md", date));

    // Group signals by type
    let mut by_type: HashMap<String, Vec<&Signal>> = HashMap::new();
    for signal in signals {
        by_type
            .entry(signal.signal_type.to_string())
            .or_default()
            .push(signal);
    }

    let mut content = String::new();

    // If file exists, append; otherwise create with header
    if file_path.exists() {
        content = fs::read_to_string(&file_path)?;
        content.push_str("\n\n");
    } else {
        content.push_str(&format!("# Dreaming — {}\n\n", date));
        content.push_str("Automatically extracted knowledge signals from session transcripts.\n\n");
    }

    let sweep_header = format!("## Sweep at {}\n\n", Utc::now().format("%H:%M:%S UTC"));
    content.push_str(&sweep_header);

    let mut written = 0;
    for (signal_type, group) in &by_type {
        content.push_str(&format!("### {}\n\n", capitalize(signal_type)));
        for signal in group {
            content.push_str(&format!("- {}\n", signal.content));
            written += 1;
        }
        content.push('\n');
    }

    fs::write(&file_path, content)?;

    info!(
        "Dreaming: wrote {} signals to {}",
        written,
        file_path.display()
    );

    Ok(written)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Result of a dreaming sweep
#[derive(Debug, Default)]
pub struct SweepResult {
    pub sessions_processed: usize,
    pub signals_extracted: usize,
    pub signals_written: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write_test_session(dir: &Path, session_id: &str, messages: &[(&str, &str)]) -> PathBuf {
        let path = dir.join(format!("{}.jsonl", session_id));
        let mut file = fs::File::create(&path).unwrap();

        writeln!(
            file,
            r#"{{"type":"session","version":1,"id":"{}","timestamp":"2026-04-01T00:00:00Z","cwd":"."}}"#,
            session_id
        )
        .unwrap();

        for (role, content) in messages {
            writeln!(
                file,
                r#"{{"type":"message","message":{{"role":"{}","content":"{}"}},"timestamp":1000}}"#,
                role, content
            )
            .unwrap();
        }

        // Set modified time to 2 hours ago so it passes min_age
        let two_hours_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
        filetime::set_file_mtime(&path, filetime::FileTime::from_system_time(two_hours_ago))
            .unwrap_or_default();

        path
    }

    #[test]
    fn test_signal_classifier_preferences() {
        let classifier = SignalClassifier::new();
        let signals = classifier.classify("I prefer dark mode for all editors", "s1");
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].signal_type, SignalType::Preference);
    }

    #[test]
    fn test_signal_classifier_facts() {
        let classifier = SignalClassifier::new();
        let signals = classifier.classify("My name is Alice and I work at Acme", "s1");
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].signal_type, SignalType::Fact);
    }

    #[test]
    fn test_signal_classifier_decisions() {
        let classifier = SignalClassifier::new();
        let signals = classifier.classify("Let's go with the PostgreSQL backend for this", "s1");
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].signal_type, SignalType::Decision);
    }

    #[test]
    fn test_signal_classifier_tasks() {
        let classifier = SignalClassifier::new();
        let signals = classifier.classify("Remember to update the CI config after merging", "s1");
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].signal_type, SignalType::Task);
    }

    #[test]
    fn test_signal_classifier_short_message_skipped() {
        let classifier = SignalClassifier::new();
        let signals = classifier.classify("yes", "s1");
        assert!(signals.is_empty());
    }

    #[test]
    fn test_signal_classifier_no_match() {
        let classifier = SignalClassifier::new();
        let signals = classifier.classify("Can you explain how async works in Rust?", "s1");
        assert!(signals.is_empty());
    }

    #[test]
    fn test_processed_log_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("dreaming-log.json");

        let mut log = ProcessedLog::default();
        assert!(!log.is_processed("session-1"));

        log.mark_processed("session-1", 3);
        assert!(log.is_processed("session-1"));

        log.save(&log_path).unwrap();

        let loaded = ProcessedLog::load(&log_path).unwrap();
        assert!(loaded.is_processed("session-1"));
        assert!(!loaded.is_processed("session-2"));
        assert_eq!(loaded.processed_sessions["session-1"].signal_count, 3);
    }

    #[test]
    fn test_run_sweep_disabled() {
        let config = DreamingConfig::default(); // disabled
        let result = run_sweep(
            &config,
            Path::new("/nonexistent"),
            Path::new("/nonexistent"),
            Path::new("/nonexistent"),
        )
        .unwrap();
        assert_eq!(result.sessions_processed, 0);
    }

    #[test]
    fn test_run_sweep_extracts_signals() {
        let tmp = TempDir::new().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let workspace = tmp.path().join("workspace");
        let log_path = tmp.path().join("dreaming-log.json");
        fs::create_dir_all(&sessions_dir).unwrap();
        fs::create_dir_all(&workspace).unwrap();

        write_test_session(
            &sessions_dir,
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            &[
                ("user", "I prefer using Rust for all backend services"),
                ("assistant", "Great choice! Rust provides memory safety."),
                ("user", "My name is Alice and I work at Acme Corp"),
                ("assistant", "Nice to meet you, Alice!"),
            ],
        );

        let config = DreamingConfig {
            enabled: true,
            min_session_age_secs: 0, // Don't skip recent sessions in test
            ..Default::default()
        };

        let result = run_sweep(&config, &sessions_dir, &workspace, &log_path).unwrap();

        assert_eq!(result.sessions_processed, 1);
        assert!(result.signals_extracted >= 2); // preference + fact
        assert!(result.signals_written >= 2);

        // Verify memory file was created
        let dreaming_dir = workspace.join("memory").join("dreaming");
        assert!(dreaming_dir.exists());
        let files: Vec<_> = fs::read_dir(&dreaming_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(files.len(), 1);

        let content = fs::read_to_string(files[0].path()).unwrap();
        assert!(content.contains("Preference"));
        assert!(content.contains("dark mode") || content.contains("Rust"));

        // Verify idempotency — second sweep should skip
        let result2 = run_sweep(&config, &sessions_dir, &workspace, &log_path).unwrap();
        assert_eq!(result2.sessions_processed, 0);
    }

    #[test]
    fn test_truncate_signal() {
        let short = "short message";
        assert_eq!(truncate_signal(short), "short message");

        let long = "a".repeat(300);
        let truncated = truncate_signal(&long);
        assert!(truncated.len() <= 200 + 3); // 200 chars + "..."
        assert!(truncated.ends_with("..."));
    }
}
