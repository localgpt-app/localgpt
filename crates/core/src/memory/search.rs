//! Memory search types and utilities

use serde::{Deserialize, Serialize};

/// A chunk of memory content returned from search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChunk {
    /// File path relative to workspace
    pub file: String,

    /// Starting line number (1-indexed)
    pub line_start: i32,

    /// Ending line number (1-indexed)
    pub line_end: i32,

    /// The actual content
    pub content: String,

    /// Relevance score (higher is better)
    pub score: f64,
}

impl MemoryChunk {
    /// Create a new memory chunk
    pub fn new(file: String, line_start: i32, line_end: i32, content: String, score: f64) -> Self {
        Self {
            file,
            line_start,
            line_end,
            content,
            score,
        }
    }

    /// Get a preview of the content (first N characters)
    pub fn preview(&self, max_len: usize) -> String {
        if self.content.len() <= max_len {
            self.content.clone()
        } else {
            format!(
                "{}...",
                &self.content[..self.content.floor_char_boundary(max_len)]
            )
        }
    }

    /// Get the location string (file:line)
    pub fn location(&self) -> String {
        if self.line_start == self.line_end {
            format!("{}:{}", self.file, self.line_start)
        } else {
            format!("{}:{}-{}", self.file, self.line_start, self.line_end)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_chunk_preview() {
        let chunk = MemoryChunk::new(
            "test.md".to_string(),
            1,
            5,
            "This is a long content string that should be truncated".to_string(),
            0.9,
        );

        assert_eq!(chunk.preview(20), "This is a long conte...");
        assert_eq!(chunk.location(), "test.md:1-5");
    }

    #[test]
    fn test_memory_chunk_single_line_location() {
        let chunk = MemoryChunk::new(
            "test.md".to_string(),
            10,
            10,
            "Single line".to_string(),
            0.5,
        );

        assert_eq!(chunk.location(), "test.md:10");
    }

    #[test]
    fn test_memory_chunk_preview_multibyte() {
        // Emoji are 4 bytes each in UTF-8
        let chunk = MemoryChunk::new(
            "test.md".to_string(),
            1,
            1,
            "Hello 🌍🌎🌏 world".to_string(),
            1.0,
        );

        // max_len=8 lands inside the first emoji (bytes 6-9), should not panic
        let preview = chunk.preview(8);
        assert!(preview.ends_with("..."));
        // Should truncate to "Hello " (6 bytes) since byte 8 is mid-emoji
        assert_eq!(preview, "Hello ...");
    }

    #[test]
    fn test_memory_chunk_preview_emdash() {
        // Em-dash (—) is 3 bytes in UTF-8
        let chunk = MemoryChunk::new(
            "test.md".to_string(),
            1,
            1,
            "one—two—three—four—five".to_string(),
            1.0,
        );

        // "one—" is 3 + 3 = 6 bytes; max_len=5 lands mid-emdash
        let preview = chunk.preview(5);
        assert!(preview.ends_with("..."));
        assert_eq!(preview, "one...");
    }
}
