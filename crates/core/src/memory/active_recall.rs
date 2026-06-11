//! Active Memory Recall — automatically search memory before generating replies
//!
//! When enabled, the agent searches its memory using the user's message as a query
//! before the LLM call, injecting any relevant recalled context into the conversation.
//! This ensures user preferences, past decisions, and important facts are surfaced
//! without requiring the agent to explicitly call memory_search.

use super::MemoryChunk;
use crate::text::prefix_chars_cow;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Configuration for active memory recall
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ActiveMemoryConfig {
    /// Enable active memory recall before replies. Default: false.
    pub enabled: bool,
    /// How to build the search query: "message" (user message only) or "recent" (include recent turns)
    pub query_mode: QueryMode,
    /// Number of recent turns to include when query_mode is "recent". Default: 4.
    pub max_recent_turns: usize,
    /// Maximum number of memory chunks to recall. Default: 3.
    pub max_results: usize,
    /// Maximum total characters of recalled context to inject. Default: 500.
    pub max_chars: usize,
    /// Minimum relevance score to include a result (0.0-1.0). Default: 0.1.
    pub min_score: f64,
    /// Cache TTL in milliseconds. Prevents redundant searches for repeated queries. Default: 15000.
    pub cache_ttl_ms: u64,
}

impl Default for ActiveMemoryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            query_mode: QueryMode::Message,
            max_recent_turns: 4,
            max_results: 3,
            max_chars: 500,
            min_score: 0.1,
            cache_ttl_ms: 15_000,
        }
    }
}

/// How to construct the memory search query
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum QueryMode {
    /// Use only the current user message as the query
    #[default]
    Message,
    /// Include recent conversation turns for better context
    Recent,
}

/// Result of an active memory recall attempt
#[derive(Debug)]
pub enum RecallResult {
    /// Relevant context was recalled
    Recalled(String),
    /// No relevant results found
    Empty,
    /// Feature is disabled
    Disabled,
    /// Cache hit (returning cached result)
    CacheHit(String),
}

/// In-memory cache for recall results to avoid redundant searches
pub struct RecallCache {
    entries: HashMap<u64, CacheEntry>,
    ttl: Duration,
}

struct CacheEntry {
    result: Option<String>,
    created_at: Instant,
}

impl RecallCache {
    pub fn new(ttl_ms: u64) -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_millis(ttl_ms),
        }
    }

    pub fn get(&self, query_hash: u64) -> Option<&Option<String>> {
        self.entries.get(&query_hash).and_then(|entry| {
            if entry.created_at.elapsed() < self.ttl {
                Some(&entry.result)
            } else {
                None
            }
        })
    }

    pub fn put(&mut self, query_hash: u64, result: Option<String>) {
        // Evict expired entries periodically
        if self.entries.len() > 100 {
            self.entries
                .retain(|_, entry| entry.created_at.elapsed() < self.ttl);
        }
        self.entries.insert(
            query_hash,
            CacheEntry {
                result,
                created_at: Instant::now(),
            },
        );
    }
}

/// Build a search query from the user's message and optionally recent conversation turns
pub fn build_query(
    user_message: &str,
    recent_messages: &[(String, String)], // (role, content) pairs
    config: &ActiveMemoryConfig,
) -> String {
    match config.query_mode {
        QueryMode::Message => user_message.to_string(),
        QueryMode::Recent => {
            let mut parts = Vec::new();
            let start = recent_messages
                .len()
                .saturating_sub(config.max_recent_turns);
            for (role, content) in &recent_messages[start..] {
                // Truncate each turn to avoid excessive query length
                let truncated = prefix_chars_cow(content, 200);
                parts.push(format!("{}: {}", role, truncated));
            }
            parts.push(format!("user: {}", user_message));
            parts.join("\n")
        }
    }
}

/// Format recalled memory chunks into a context string for injection
pub fn format_recalled_context(chunks: &[MemoryChunk], max_chars: usize) -> Option<String> {
    if chunks.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    let mut total_chars = 0;

    for chunk in chunks {
        let entry = chunk.content.trim();
        if entry.is_empty() {
            continue;
        }

        let entry_chars = entry.chars().count();
        if total_chars + entry_chars > max_chars {
            // Include partial last entry if we have room
            let remaining = max_chars.saturating_sub(total_chars);
            if remaining > 50 {
                let prefix = prefix_chars_cow(entry, remaining);
                parts.push(format!("- {prefix}..."));
            }
            break;
        }

        parts.push(format!("- {}", entry));
        total_chars += entry_chars;
    }

    if parts.is_empty() {
        return None;
    }

    Some(format!(
        "<recalled_context>\nThe following was automatically recalled from memory and may be relevant:\n{}\n</recalled_context>",
        parts.join("\n")
    ))
}

/// Compute a simple hash for cache keying
pub fn query_hash(query: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    query.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_query_message_mode() {
        let config = ActiveMemoryConfig {
            query_mode: QueryMode::Message,
            ..Default::default()
        };
        let query = build_query("What color do I prefer?", &[], &config);
        assert_eq!(query, "What color do I prefer?");
    }

    #[test]
    fn test_build_query_recent_mode() {
        let config = ActiveMemoryConfig {
            query_mode: QueryMode::Recent,
            max_recent_turns: 2,
            ..Default::default()
        };
        let recent = vec![
            ("user".to_string(), "Hello".to_string()),
            ("assistant".to_string(), "Hi there!".to_string()),
            ("user".to_string(), "Tell me about colors".to_string()),
            ("assistant".to_string(), "Sure, what colors?".to_string()),
        ];
        let query = build_query("What color do I prefer?", &recent, &config);
        assert!(query.contains("Tell me about colors"));
        assert!(query.contains("Sure, what colors?"));
        assert!(query.contains("What color do I prefer?"));
        // Should NOT contain the first turn (only last 2)
        assert!(!query.contains("Hello"));
    }

    #[test]
    fn test_build_query_recent_mode_truncates_multibyte_chars() {
        let config = ActiveMemoryConfig {
            query_mode: QueryMode::Recent,
            max_recent_turns: 1,
            ..Default::default()
        };
        let recent = vec![("user".to_string(), "✅".repeat(201))];

        let query = build_query("current", &recent, &config);

        assert_eq!(query.matches('✅').count(), 200);
        assert!(query.contains("user: current"));
    }

    #[test]
    fn test_format_recalled_context_empty() {
        assert!(format_recalled_context(&[], 500).is_none());
    }

    #[test]
    fn test_format_recalled_context_basic() {
        let chunks = vec![
            MemoryChunk {
                file: "test.md".to_string(),
                line_start: 1,
                line_end: 1,
                content: "User prefers dark mode".to_string(),
                score: 0.9,
                updated_at: 0,
            },
            MemoryChunk {
                file: "test.md".to_string(),
                line_start: 2,
                line_end: 2,
                content: "User works at Acme Corp".to_string(),
                score: 0.7,
                updated_at: 0,
            },
        ];

        let result = format_recalled_context(&chunks, 500).unwrap();
        assert!(result.contains("<recalled_context>"));
        assert!(result.contains("User prefers dark mode"));
        assert!(result.contains("User works at Acme Corp"));
    }

    #[test]
    fn test_format_recalled_context_truncation() {
        let chunks = vec![MemoryChunk {
            file: "test.md".to_string(),
            line_start: 1,
            line_end: 1,
            content: "A".repeat(600),
            score: 0.9,
            updated_at: 0,
        }];

        let result = format_recalled_context(&chunks, 100).unwrap();
        // Should be truncated
        assert!(result.len() < 600);
        assert!(result.contains("..."));
    }

    #[test]
    fn test_format_recalled_context_truncates_multibyte_chars() {
        let chunks = vec![MemoryChunk {
            file: "test.md".to_string(),
            line_start: 1,
            line_end: 1,
            content: "✅".repeat(200),
            score: 0.9,
            updated_at: 0,
        }];

        let result = format_recalled_context(&chunks, 100).unwrap();

        assert_eq!(result.matches('✅').count(), 100);
        assert!(result.contains("- ✅"));
        assert!(result.contains("..."));
    }

    #[test]
    fn test_recall_cache() {
        let mut cache = RecallCache::new(60_000); // 60s TTL

        // Miss
        assert!(cache.get(123).is_none());

        // Put and hit
        cache.put(123, Some("recalled text".to_string()));
        let hit = cache.get(123).unwrap();
        assert_eq!(hit.as_deref(), Some("recalled text"));

        // Empty result cache
        cache.put(456, None);
        let hit = cache.get(456).unwrap();
        assert!(hit.is_none());
    }

    #[test]
    fn test_recall_cache_expired() {
        let mut cache = RecallCache::new(0); // 0ms TTL = immediate expiry
        cache.put(123, Some("text".to_string()));

        // Should be expired
        std::thread::sleep(std::time::Duration::from_millis(1));
        assert!(cache.get(123).is_none());
    }

    #[test]
    fn test_query_hash_deterministic() {
        let h1 = query_hash("test query");
        let h2 = query_hash("test query");
        let h3 = query_hash("different query");

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_default_config_disabled() {
        let config = ActiveMemoryConfig::default();
        assert!(!config.enabled);
    }
}
