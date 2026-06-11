//! Session transcript indexing for memory search.
//!
//! Parses completed session JSONL files and indexes user+assistant message pairs
//! into the same FTS5 search index used for workspace memory files. This enables
//! `memory_search` to find prior conversation context.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use super::backend::MemoryBackend;

/// Maximum exchanges per chunk (user + assistant = 1 exchange).
const EXCHANGES_PER_CHUNK: usize = 3;

/// Maximum characters from each user or assistant message in an indexed chunk.
const MAX_INDEXED_MESSAGE_CHARS: usize = 500;

/// State file tracking which sessions have been indexed.
const STATE_FILE: &str = "indexed_sessions.json";

/// Session indexer — indexes completed session transcripts into memory search.
pub struct SessionIndexer<'a> {
    backend: &'a dyn MemoryBackend,
    state_path: PathBuf,
    state: IndexState,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct IndexState {
    /// Map of "agent_id/session_id" → last-indexed timestamp (ISO 8601)
    indexed: HashMap<String, String>,
}

impl<'a> SessionIndexer<'a> {
    /// Create a new session indexer.
    ///
    /// `cache_dir` is where the indexing state file is stored
    /// (e.g., `~/.cache/localgpt/memory/`).
    pub fn new(backend: &'a dyn MemoryBackend, cache_dir: &Path) -> Result<Self> {
        let state_path = cache_dir.join(STATE_FILE);
        let state = if state_path.exists() {
            let content = std::fs::read_to_string(&state_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            IndexState::default()
        };
        Ok(Self {
            backend,
            state_path,
            state,
        })
    }

    /// Index all unindexed sessions for an agent.
    /// Returns the number of newly indexed sessions.
    pub fn index_agent_sessions(&mut self, agent_id: &str, sessions_dir: &Path) -> Result<usize> {
        if !sessions_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;

        let entries: Vec<_> = std::fs::read_dir(sessions_dir)?
            .filter_map(|e| e.ok())
            .collect();

        for entry in entries {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());

            // Only process .jsonl files (skip .jsonl.enc for now)
            if ext != Some("jsonl") {
                continue;
            }

            let session_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

            let state_key = format!("{}/{}", agent_id, session_id);

            if self.state.indexed.contains_key(&state_key) {
                continue; // Already indexed
            }

            match self.index_session_file(&path, agent_id, session_id) {
                Ok(chunks) => {
                    if chunks > 0 {
                        debug!(
                            "Indexed session {}/{}: {} chunks",
                            agent_id, session_id, chunks
                        );
                        count += 1;
                    }
                    // Mark as indexed even if 0 chunks (empty session)
                    let now = chrono::Utc::now().to_rfc3339();
                    self.state.indexed.insert(state_key, now);
                }
                Err(e) => {
                    warn!("Failed to index session {}/{}: {}", agent_id, session_id, e);
                }
            }
        }

        if count > 0 {
            self.save_state()?;
            info!("Indexed {} new sessions for agent {}", count, agent_id);
        }

        Ok(count)
    }

    /// Index a single session file into the memory index.
    fn index_session_file(&self, path: &Path, agent_id: &str, session_id: &str) -> Result<usize> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read session: {}", path.display()))?;

        let exchanges = extract_exchanges(&content);
        if exchanges.is_empty() {
            return Ok(0);
        }

        let chunks = group_into_chunks(&exchanges, agent_id, session_id);

        let virtual_path = format!("sessions/{}/{}.jsonl", agent_id, session_id);
        let mut indexed = 0;

        for (i, chunk_text) in chunks.iter().enumerate() {
            let line_start = i * EXCHANGES_PER_CHUNK * 2 + 1;
            let line_end = line_start + EXCHANGES_PER_CHUNK * 2;

            if let Err(e) =
                self.backend
                    .insert_chunk(&virtual_path, chunk_text, line_start, line_end)
            {
                warn!("Failed to insert session chunk: {}", e);
                continue;
            }
            indexed += 1;
        }

        Ok(indexed)
    }

    fn save_state(&self) -> Result<()> {
        if let Some(parent) = self.state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.state)?;
        std::fs::write(&self.state_path, json)?;
        Ok(())
    }
}

/// A single user→assistant exchange extracted from a session.
#[derive(Debug)]
pub(crate) struct Exchange {
    user: String,
    assistant: String,
    timestamp: Option<String>,
}

/// Parse session JSONL and extract user+assistant exchanges.
/// Skips system messages, tool calls, and tool results.
pub(crate) fn extract_exchanges(content: &str) -> Vec<Exchange> {
    let mut exchanges = Vec::new();
    let mut pending_user: Option<String> = None;
    let mut pending_ts: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let entry: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if entry["type"].as_str() != Some("message") {
            continue;
        }

        let msg = match entry.get("message") {
            Some(m) => m,
            None => continue,
        };

        let role = msg["role"].as_str().unwrap_or("");
        let content_text = extract_message_text(msg);

        if content_text.trim().is_empty() {
            continue;
        }

        match role {
            "user" => {
                pending_user = Some(content_text);
                pending_ts = msg
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .map(String::from);
            }
            "assistant" => {
                if let Some(user_msg) = pending_user.take() {
                    exchanges.push(Exchange {
                        user: user_msg,
                        assistant: content_text,
                        timestamp: pending_ts.take(),
                    });
                }
            }
            // Skip system, toolResult, tool
            _ => {}
        }
    }

    exchanges
}

/// Extract text content from a Pi-format message object.
fn extract_message_text(msg: &serde_json::Value) -> String {
    // Try content array first (Pi format)
    if let Some(content_arr) = msg["content"].as_array() {
        let texts: Vec<&str> = content_arr
            .iter()
            .filter_map(|part| {
                if part["type"].as_str() == Some("text") {
                    part["text"].as_str()
                } else {
                    None
                }
            })
            .collect();
        if !texts.is_empty() {
            return texts.join("\n");
        }
    }
    // Fallback: content as string
    msg["content"].as_str().unwrap_or("").to_string()
}

/// Group exchanges into chunks of EXCHANGES_PER_CHUNK.
fn group_into_chunks(exchanges: &[Exchange], agent_id: &str, session_id: &str) -> Vec<String> {
    exchanges
        .chunks(EXCHANGES_PER_CHUNK)
        .map(|group| {
            let date = group
                .first()
                .and_then(|e| e.timestamp.as_deref())
                .and_then(|ts| ts.split('T').next())
                .unwrap_or("unknown");

            let mut chunk = format!("[Session: {}/{}, {}]\n\n", agent_id, session_id, date);

            for exchange in group {
                chunk.push_str("User: ");
                push_message_preview(&mut chunk, &exchange.user, MAX_INDEXED_MESSAGE_CHARS);
                chunk.push_str("\n\nAssistant: ");
                push_message_preview(&mut chunk, &exchange.assistant, MAX_INDEXED_MESSAGE_CHARS);
                chunk.push_str("\n\n");
            }

            chunk
        })
        .collect()
}

fn push_message_preview(chunk: &mut String, message: &str, max_chars: usize) {
    let mut chars = message.chars();
    chunk.extend(chars.by_ref().take(max_chars));
    if chars.next().is_some() {
        chunk.push_str("...");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_session_jsonl(messages: &[(&str, &str)]) -> String {
        let mut content = String::new();
        content.push_str(r#"{"type":"session","version":1,"id":"test-session","timestamp":"2026-03-17T10:00:00Z","cwd":"."}"#);
        content.push('\n');

        for (role, text) in messages {
            let entry = serde_json::json!({
                "type": "message",
                "message": {
                    "role": role,
                    "content": [{"type": "text", "text": text}],
                    "timestamp": "2026-03-17T10:00:00Z"
                }
            });
            content.push_str(&serde_json::to_string(&entry).unwrap());
            content.push('\n');
        }
        content
    }

    #[test]
    fn test_extract_user_assistant_pairs() {
        let jsonl = make_session_jsonl(&[
            ("user", "Hello"),
            ("assistant", "Hi there!"),
            ("user", "How are you?"),
            ("assistant", "I'm great!"),
        ]);

        let exchanges = extract_exchanges(&jsonl);
        assert_eq!(exchanges.len(), 2);
        assert_eq!(exchanges[0].user, "Hello");
        assert_eq!(exchanges[0].assistant, "Hi there!");
        assert_eq!(exchanges[1].user, "How are you?");
        assert_eq!(exchanges[1].assistant, "I'm great!");
    }

    #[test]
    fn test_skip_system_and_tool_messages() {
        let jsonl = make_session_jsonl(&[
            ("system", "You are a helpful assistant"),
            ("user", "Hello"),
            ("assistant", "Hi!"),
            ("toolResult", "tool output here"),
            ("user", "Thanks"),
            ("assistant", "Welcome!"),
        ]);

        let exchanges = extract_exchanges(&jsonl);
        assert_eq!(exchanges.len(), 2);
        assert_eq!(exchanges[0].user, "Hello");
        assert_eq!(exchanges[1].user, "Thanks");
    }

    #[test]
    fn test_unpaired_user_message_skipped() {
        let jsonl = make_session_jsonl(&[
            ("user", "Hello"),
            ("user", "Another question"), // Overrides previous
            ("assistant", "Answer"),
        ]);

        let exchanges = extract_exchanges(&jsonl);
        assert_eq!(exchanges.len(), 1);
        assert_eq!(exchanges[0].user, "Another question"); // Last user wins
    }

    #[test]
    fn test_group_into_chunks() {
        let exchanges = vec![
            Exchange {
                user: "Q1".into(),
                assistant: "A1".into(),
                timestamp: Some("2026-03-17T10:00:00Z".into()),
            },
            Exchange {
                user: "Q2".into(),
                assistant: "A2".into(),
                timestamp: None,
            },
            Exchange {
                user: "Q3".into(),
                assistant: "A3".into(),
                timestamp: None,
            },
            Exchange {
                user: "Q4".into(),
                assistant: "A4".into(),
                timestamp: None,
            },
        ];

        let chunks = group_into_chunks(&exchanges, "main", "test-session");

        // 4 exchanges / 3 per chunk = 2 chunks
        assert_eq!(chunks.len(), 2);

        assert!(chunks[0].contains("[Session: main/test-session, 2026-03-17]"));
        assert!(chunks[0].contains("User: Q1"));
        assert!(chunks[0].contains("Assistant: A1"));
        assert!(chunks[0].contains("User: Q3"));
        assert!(!chunks[0].contains("User: Q4")); // In second chunk

        assert!(chunks[1].contains("User: Q4"));
        assert!(chunks[1].contains("Assistant: A4"));
    }

    #[test]
    fn test_long_messages_truncated_in_chunks() {
        let long_text = "x".repeat(1000);
        let exchanges = vec![Exchange {
            user: long_text.clone(),
            assistant: long_text,
            timestamp: None,
        }];

        let chunks = group_into_chunks(&exchanges, "main", "test");
        assert!(chunks[0].contains("..."));
        assert!(chunks[0].len() < 1500); // Should be truncated
    }

    #[test]
    fn test_long_multibyte_messages_truncated_in_chunks() {
        let long_text = "✅".repeat(1000);
        let exchanges = vec![Exchange {
            user: long_text.clone(),
            assistant: long_text,
            timestamp: None,
        }];

        let chunks = group_into_chunks(&exchanges, "main", "test");

        assert_eq!(chunks[0].matches("...").count(), 2);
        assert_eq!(chunks[0].chars().filter(|&c| c == '✅').count(), 1000);
    }

    #[test]
    fn test_empty_session() {
        let jsonl = r#"{"type":"session","version":1,"id":"empty","timestamp":"2026-03-17T10:00:00Z","cwd":"."}"#;
        let exchanges = extract_exchanges(jsonl);
        assert!(exchanges.is_empty());
    }

    #[test]
    fn test_extract_message_text_pi_format() {
        let msg = serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": " world"}
            ]
        });
        assert_eq!(extract_message_text(&msg), "Hello\n world");
    }

    #[test]
    fn test_extract_message_text_string_format() {
        let msg = serde_json::json!({
            "role": "user",
            "content": "Simple string"
        });
        assert_eq!(extract_message_text(&msg), "Simple string");
    }
}
