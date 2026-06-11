//! Memory search types and utilities

use crate::text::prefix_chars_with_ellipsis;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

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

    /// Unix timestamp when the chunk was last updated (for temporal decay)
    #[serde(default)]
    pub updated_at: i64,
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
            updated_at: 0,
        }
    }

    /// Create a new memory chunk with timestamp
    pub fn with_timestamp(mut self, updated_at: i64) -> Self {
        self.updated_at = updated_at;
        self
    }

    /// Apply temporal decay to the score based on age.
    /// decay_factor = exp(-lambda * age_days)
    /// Returns the decayed score.
    pub fn apply_temporal_decay(&mut self, lambda: f64, now_unix: i64) -> f64 {
        if lambda <= 0.0 || self.updated_at <= 0 {
            return self.score;
        }

        let age_secs = (now_unix - self.updated_at).max(0) as f64;
        let age_days = age_secs / (24.0 * 60.0 * 60.0);
        let decay_factor = (-lambda * age_days).exp();

        self.score *= decay_factor;
        self.score
    }

    /// Get a preview of the content (first N characters)
    pub fn preview(&self, max_len: usize) -> String {
        prefix_chars_with_ellipsis(&self.content, max_len)
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

/// MMR (Maximal Marginal Relevance) re-ranking for search results.
///
/// MMR diversifies results by balancing relevance with novelty.
/// Formula: MMR = λ * relevance - (1-λ) * max_similarity_to_selected
///
/// This helps avoid showing multiple very similar chunks in results.
#[allow(dead_code)]
pub struct MmrReranker {
    /// Trade-off between relevance (1.0) and diversity (0.0)
    /// Default: 0.7 (slightly favor relevance)
    lambda: f64,
}

impl Default for MmrReranker {
    fn default() -> Self {
        Self { lambda: 0.7 }
    }
}

impl MmrReranker {
    /// Create a new MMR reranker with custom lambda
    pub fn new(lambda: f64) -> Self {
        Self {
            lambda: lambda.clamp(0.0, 1.0),
        }
    }

    /// Re-rank search results using MMR algorithm.
    ///
    /// # Arguments
    /// * `chunks` - Search results to re-rank (will be modified in place)
    ///
    /// # Returns
    /// The re-ranked chunks in MMR order
    pub fn rerank(&self, chunks: &mut [MemoryChunk]) {
        if chunks.len() <= 1 {
            return;
        }

        // Tokenize all chunks once
        let token_sets: Vec<HashSet<String>> =
            chunks.iter().map(|c| tokenize(&c.content)).collect();

        // Track original scores
        let original_scores: Vec<f64> = chunks.iter().map(|c| c.score).collect();

        // Track which indices have been selected
        let mut selected: Vec<usize> = Vec::with_capacity(chunks.len());
        let mut remaining: Vec<usize> = (0..chunks.len()).collect();

        // Select first item (highest relevance)
        if let Some((best_pos, _best_idx)) =
            remaining.iter().enumerate().max_by(|(_, a), (_, b)| {
                original_scores[**a]
                    .partial_cmp(&original_scores[**b])
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        {
            selected.push(remaining.remove(best_pos));
        }

        // Greedily select remaining items using MMR
        while !remaining.is_empty() {
            let best = remaining
                .iter()
                .enumerate()
                .max_by(|(_pos_a, idx_a), (_pos_b, idx_b)| {
                    let mmr_a =
                        self.compute_mmr(**idx_a, original_scores[**idx_a], &selected, &token_sets);
                    let mmr_b =
                        self.compute_mmr(**idx_b, original_scores[**idx_b], &selected, &token_sets);
                    mmr_a
                        .partial_cmp(&mmr_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

            if let Some((best_pos, best_idx)) = best {
                // Update the score to the MMR value for transparency
                let mmr_score = self.compute_mmr(
                    *best_idx,
                    original_scores[*best_idx],
                    &selected,
                    &token_sets,
                );
                chunks[*best_idx].score = mmr_score;
                selected.push(remaining.remove(best_pos));
            }
        }

        // Reorder chunks by selection order
        let mut reordered: Vec<MemoryChunk> =
            selected.into_iter().map(|i| chunks[i].clone()).collect();
        chunks.swap_with_slice(&mut reordered);
    }

    /// Compute MMR score for a candidate
    fn compute_mmr(
        &self,
        candidate_idx: usize,
        relevance: f64,
        selected: &[usize],
        token_sets: &[HashSet<String>],
    ) -> f64 {
        let max_sim = if selected.is_empty() {
            0.0
        } else {
            selected
                .iter()
                .map(|&sel_idx| {
                    jaccard_similarity(&token_sets[candidate_idx], &token_sets[sel_idx])
                })
                .fold(0.0_f64, f64::max)
        };

        self.lambda * relevance - (1.0 - self.lambda) * max_sim
    }
}

/// Simple whitespace tokenizer with lowercase normalization
#[allow(dead_code)]
fn tokenize(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split_whitespace()
        .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|s| !s.is_empty() && s.chars().count() > 1) // Skip single chars
        .map(|s| s.to_string())
        .collect()
}

/// Compute Jaccard similarity between two token sets
#[allow(dead_code)]
fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let intersection = a.intersection(b).count();
    let union = a.union(b).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Apply MMR re-ranking to search results.
///
/// This is a convenience function that creates a reranker with default lambda (0.7).
#[allow(dead_code)]
pub fn apply_mmr(chunks: &mut [MemoryChunk]) {
    MmrReranker::default().rerank(chunks);
}

/// Apply MMR re-ranking with custom lambda.
#[allow(dead_code)]
pub fn apply_mmr_with_lambda(chunks: &mut [MemoryChunk], lambda: f64) {
    MmrReranker::new(lambda).rerank(chunks);
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

        let preview = chunk.preview(8);
        assert!(preview.ends_with("..."));
        assert_eq!(preview, "Hello 🌍🌎...");
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

        let preview = chunk.preview(5);
        assert!(preview.ends_with("..."));
        assert_eq!(preview, "one—t...");
    }

    #[test]
    fn test_temporal_decay_no_decay() {
        // Lambda = 0 means no decay
        let mut chunk = MemoryChunk::new("test.md".to_string(), 1, 1, "content".to_string(), 1.0);
        chunk.updated_at = 1_700_000_000; // Some old timestamp

        let decayed = chunk.apply_temporal_decay(0.0, 1_710_000_000);
        assert!((decayed - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_temporal_decay_seven_days() {
        // Lambda = 0.1: 7-day old memory should get ~50% penalty
        let mut chunk = MemoryChunk::new("test.md".to_string(), 1, 1, "content".to_string(), 1.0);
        let now = 1_710_000_000i64;
        chunk.updated_at = now - (7 * 24 * 60 * 60); // 7 days ago

        let decayed = chunk.apply_temporal_decay(0.1, now);
        // exp(-0.1 * 7) ≈ 0.496
        assert!((decayed - 0.496).abs() < 0.01);
    }

    #[test]
    fn test_temporal_decay_fresh() {
        // Fresh memory (just updated) should have no penalty
        let mut chunk = MemoryChunk::new("test.md".to_string(), 1, 1, "content".to_string(), 1.0);
        let now = 1_710_000_000i64;
        chunk.updated_at = now;

        let decayed = chunk.apply_temporal_decay(0.1, now);
        assert!((decayed - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_jaccard_similarity() {
        let a: HashSet<String> = ["apple", "banana", "cherry"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let b: HashSet<String> = ["banana", "cherry", "date"]
            .iter()
            .map(|s| s.to_string())
            .collect();

        // Intersection: banana, cherry (2)
        // Union: apple, banana, cherry, date (4)
        let sim = jaccard_similarity(&a, &b);
        assert!((sim - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_jaccard_similarity_empty() {
        let a: HashSet<String> = ["apple"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = HashSet::new();

        assert_eq!(jaccard_similarity(&a, &b), 0.0);
        assert_eq!(jaccard_similarity(&b, &a), 0.0);
    }

    #[test]
    fn test_jaccard_similarity_identical() {
        let a: HashSet<String> = ["apple", "banana"].iter().map(|s| s.to_string()).collect();
        let b: HashSet<String> = ["apple", "banana"].iter().map(|s| s.to_string()).collect();

        assert!((jaccard_similarity(&a, &b) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_mmr_single_item() {
        let mut chunks = vec![MemoryChunk::new(
            "test.md".to_string(),
            1,
            1,
            "content".to_string(),
            0.9,
        )];

        apply_mmr(&mut chunks);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_mmr_diverse_results() {
        // Two very different chunks with same relevance
        let mut chunks = vec![
            MemoryChunk::new(
                "a.md".to_string(),
                1,
                1,
                "apple banana cherry".to_string(),
                0.9,
            ),
            MemoryChunk::new(
                "b.md".to_string(),
                1,
                1,
                "xray yacht zebra".to_string(),
                0.9,
            ),
        ];

        apply_mmr(&mut chunks);

        // Both should be selected (they're diverse), order based on MMR
        assert_eq!(chunks.len(), 2);
        // Files should still be present
        let files: Vec<_> = chunks.iter().map(|c| c.file.clone()).collect();
        assert!(files.contains(&"a.md".to_string()));
        assert!(files.contains(&"b.md".to_string()));
    }

    #[test]
    fn test_mmr_similar_penalized() {
        // High relevance similar vs lower relevance diverse
        let mut chunks = vec![
            MemoryChunk::new(
                "similar1.md".to_string(),
                1,
                1,
                "apple banana".to_string(),
                1.0,
            ),
            MemoryChunk::new(
                "similar2.md".to_string(),
                1,
                1,
                "apple banana cherry".to_string(),
                0.95,
            ),
            MemoryChunk::new(
                "diverse.md".to_string(),
                1,
                1,
                "xray yacht zebra".to_string(),
                0.8,
            ),
        ];

        apply_mmr(&mut chunks);

        // First should be similar1 (highest relevance)
        assert_eq!(chunks[0].file, "similar1.md");

        // Diverse should rank higher than similar2 due to MMR
        let diverse_pos = chunks.iter().position(|c| c.file == "diverse.md").unwrap();
        let similar2_pos = chunks.iter().position(|c| c.file == "similar2.md").unwrap();

        // Diverse should come before the similar duplicate
        assert!(
            diverse_pos < similar2_pos,
            "Diverse result should rank higher than similar duplicate"
        );
    }

    #[test]
    fn test_mmr_lambda_extremes() {
        let mut chunks = vec![
            MemoryChunk::new("high.md".to_string(), 1, 1, "unique alpha".to_string(), 1.0),
            MemoryChunk::new("low.md".to_string(), 1, 1, "unique alpha".to_string(), 0.5),
        ];

        // Lambda = 1.0: pure relevance, should prefer high.md
        apply_mmr_with_lambda(&mut chunks, 1.0);
        assert_eq!(chunks[0].file, "high.md");

        // Reset and test lambda = 0.0: pure diversity (but identical content here)
        let mut chunks2 = vec![
            MemoryChunk::new("high.md".to_string(), 1, 1, "unique alpha".to_string(), 1.0),
            MemoryChunk::new(
                "low.md".to_string(),
                1,
                1,
                "different beta".to_string(),
                0.5,
            ),
        ];

        // With lambda=0, it's purely about diversity
        // First selection picks highest relevance, second gets penalized by similarity
        apply_mmr_with_lambda(&mut chunks2, 0.0);
        assert_eq!(chunks2[0].file, "high.md"); // First always highest relevance
    }

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("Hello World! This is a test.");
        assert!(tokens.contains("hello"));
        assert!(tokens.contains("world"));
        assert!(tokens.contains("this"));
        assert!(tokens.contains("test"));
        // Single char 'a' should be filtered
        assert!(!tokens.contains("a"));
        // Single multibyte chars should be filtered too.
        let multibyte = tokenize("é 中 데이터");
        assert!(!multibyte.contains("é"));
        assert!(!multibyte.contains("中"));
        assert!(multibyte.contains("데이터"));
    }
}
