//! Query expansion for FTS5 memory search.
//!
//! Converts conversational queries ("that Rust error from yesterday") into
//! FTS5-friendly keyword queries ("rust OR error") by stripping stop words,
//! handling CJK n-grams, and optionally using an LLM for keyword extraction.

use std::collections::HashSet;

/// Result of expanding a user query for FTS5 search.
#[derive(Debug, Clone)]
pub struct ExpandedQuery {
    /// Original query, unchanged.
    pub original: String,
    /// FTS5-ready query: `"keyword1" OR "keyword2" OR ...`
    /// Falls back to original if no keywords extracted.
    pub fts_query: String,
    /// Individual extracted keywords.
    pub keywords: Vec<String>,
}

/// Expand a conversational query into FTS5-friendly keywords.
///
/// This is the fast local path — no LLM call, no network. Suitable for
/// use as a fallback when LLM expansion is unavailable.
pub fn expand_query_local(query: &str) -> ExpandedQuery {
    let lang = detect_language(query);
    let stop_words = stop_words_for(lang);

    let tokens = tokenize(query, lang);
    let mut seen = HashSet::new();
    let keywords: Vec<String> = tokens
        .into_iter()
        .filter(|t| t.chars().count() >= 2 && !stop_words.contains(t.as_str()))
        .filter(|t| seen.insert(t.clone()))
        .collect();

    let fts_query = if keywords.is_empty() {
        query.to_string()
    } else {
        // Quote each keyword for FTS5 safety, join with OR
        keywords
            .iter()
            .map(|k| format!("\"{}\"", k.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" OR ")
    };

    ExpandedQuery {
        original: query.to_string(),
        fts_query,
        keywords,
    }
}

/// Language detected from script heuristics.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum Language {
    English,
    Spanish,
    Portuguese,
    Chinese,
    Korean,
    Japanese,
    Arabic,
    Other,
}

fn detect_language(text: &str) -> Language {
    let mut cjk = 0usize;
    let mut hangul = 0usize;
    let mut hiragana_katakana = 0usize;
    let mut arabic = 0usize;

    for c in text.chars() {
        match c as u32 {
            0x4E00..=0x9FFF | 0x3400..=0x4DBF => cjk += 1,
            0xAC00..=0xD7AF | 0x1100..=0x11FF => hangul += 1,
            0x3040..=0x30FF => hiragana_katakana += 1,
            0x0600..=0x06FF => arabic += 1,
            _ => {}
        }
    }

    let total = text.chars().count().max(1);
    if hangul as f64 / total as f64 > 0.2 {
        return Language::Korean;
    }
    if hiragana_katakana as f64 / total as f64 > 0.2 {
        return Language::Japanese;
    }
    if cjk as f64 / total as f64 > 0.2 {
        return Language::Chinese;
    }
    if arabic as f64 / total as f64 > 0.2 {
        return Language::Arabic;
    }

    // Heuristic: Spanish/Portuguese presence markers
    let lower = text.to_lowercase();
    let es_markers = ["ñ", "¿", "¡", " el ", " la ", " de ", " en ", " que "];
    let pt_markers = ["ão", "ã", "ç", " de ", " da ", " do "];

    let es_score: usize = es_markers.iter().filter(|m| lower.contains(*m)).count();
    let pt_score: usize = pt_markers.iter().filter(|m| lower.contains(*m)).count();

    if es_score > pt_score && es_score > 1 {
        Language::Spanish
    } else if pt_score > 1 {
        Language::Portuguese
    } else {
        Language::English
    }
}

fn tokenize(text: &str, lang: Language) -> Vec<String> {
    match lang {
        Language::Chinese | Language::Japanese => {
            // CJK: 2-character n-grams from CJK characters only
            let cjk_chars: Vec<char> = text
                .chars()
                .filter(|c| {
                    let u = *c as u32;
                    (0x4E00..=0x9FFF).contains(&u)
                        || (0x3400..=0x4DBF).contains(&u)
                        || (0x3040..=0x309F).contains(&u)
                        || (0x30A0..=0x30FF).contains(&u)
                })
                .collect();
            if cjk_chars.len() < 2 {
                return cjk_chars.iter().map(|c| c.to_string()).collect();
            }
            cjk_chars.windows(2).map(|w| w.iter().collect()).collect()
        }
        Language::Korean => {
            // Korean: split on whitespace, strip common particles
            text.split_whitespace()
                .map(|w| strip_korean_particle(w).to_lowercase())
                .filter(|w| !w.is_empty())
                .collect()
        }
        _ => {
            // Latin scripts: split on non-alphanumeric, lowercase
            text.split(|c: char| !c.is_alphanumeric())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_lowercase())
                .collect()
        }
    }
}

fn strip_korean_particle(word: &str) -> String {
    // Strip common Korean postpositions (longest first to avoid partial matches)
    let particles = [
        "에서", "으로", "은", "는", "이", "가", "을", "를", "의", "에", "로",
    ];
    for p in &particles {
        if word.ends_with(p) && word.len() > p.len() {
            return word[..word.len() - p.len()].to_string();
        }
    }
    word.to_string()
}

fn stop_words_for(lang: Language) -> HashSet<&'static str> {
    match lang {
        Language::English => STOP_WORDS_EN.iter().copied().collect(),
        Language::Spanish => STOP_WORDS_ES.iter().copied().collect(),
        Language::Portuguese => STOP_WORDS_PT.iter().copied().collect(),
        _ => HashSet::new(),
    }
}

static STOP_WORDS_EN: &[&str] = &[
    "a",
    "an",
    "the",
    "this",
    "that",
    "these",
    "those",
    "i",
    "me",
    "my",
    "we",
    "our",
    "you",
    "your",
    "he",
    "she",
    "it",
    "they",
    "them",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "have",
    "has",
    "had",
    "do",
    "does",
    "did",
    "will",
    "would",
    "could",
    "should",
    "can",
    "may",
    "might",
    "in",
    "on",
    "at",
    "to",
    "for",
    "of",
    "with",
    "by",
    "from",
    "about",
    "into",
    "through",
    "during",
    "before",
    "after",
    "above",
    "below",
    "between",
    "and",
    "or",
    "but",
    "if",
    "then",
    "because",
    "as",
    "while",
    "when",
    "where",
    "what",
    "which",
    "who",
    "how",
    "why",
    "yesterday",
    "today",
    "tomorrow",
    "earlier",
    "later",
    "recently",
    "ago",
    "just",
    "now",
    "thing",
    "things",
    "stuff",
    "something",
    "anything",
    "everything",
    "nothing",
    "please",
    "help",
    "find",
    "show",
    "get",
    "tell",
    "give",
];

static STOP_WORDS_ES: &[&str] = &[
    "el",
    "la",
    "los",
    "las",
    "un",
    "una",
    "unos",
    "unas",
    "este",
    "esta",
    "ese",
    "esa",
    "yo",
    "me",
    "mi",
    "nosotros",
    "tu",
    "usted",
    "ustedes",
    "ellos",
    "ellas",
    "de",
    "del",
    "a",
    "en",
    "con",
    "por",
    "para",
    "sobre",
    "entre",
    "y",
    "o",
    "pero",
    "si",
    "es",
    "son",
    "fue",
    "fueron",
    "ser",
    "estar",
    "haber",
    "tener",
    "hacer",
    "ayer",
    "hoy",
    "antes",
    "ahora",
    "recientemente",
    "que",
    "como",
    "cuando",
    "donde",
    "favor",
    "ayuda",
];

static STOP_WORDS_PT: &[&str] = &[
    "o",
    "a",
    "os",
    "as",
    "um",
    "uma",
    "uns",
    "umas",
    "este",
    "essa",
    "eu",
    "me",
    "meu",
    "nós",
    "você",
    "vocês",
    "eles",
    "elas",
    "de",
    "do",
    "da",
    "em",
    "com",
    "por",
    "para",
    "sobre",
    "entre",
    "e",
    "ou",
    "mas",
    "se",
    "é",
    "são",
    "foi",
    "foram",
    "ser",
    "estar",
    "ter",
    "fazer",
    "ontem",
    "hoje",
    "antes",
    "agora",
    "recentemente",
    "que",
    "como",
    "quando",
    "onde",
    "favor",
    "ajuda",
];

/// Prompt template for LLM-based keyword extraction.
pub const EXPAND_PROMPT: &str = r#"Extract 5-10 specific search keywords from this query for a full-text search engine.
Return ONLY a comma-separated list of keywords, no explanation.
Focus on nouns, technical terms, and specific concepts. Omit stop words.

Query: {query}

Keywords:"#;

/// Parse an LLM response into an ExpandedQuery.
/// Used by the agent layer to complete LLM-based expansion.
pub fn parse_llm_keywords(query: &str, llm_response: &str) -> ExpandedQuery {
    let lang = detect_language(query);
    let stop_words = stop_words_for(lang);
    let mut seen = HashSet::new();
    let keywords: Vec<String> = llm_response
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty() && s.chars().count() >= 2)
        .filter(|s| !stop_words.contains(s.as_str()))
        .filter(|s| seen.insert(s.clone()))
        .collect();

    if keywords.is_empty() {
        return expand_query_local(query);
    }

    let fts_query = keywords
        .iter()
        .map(|k| format!("\"{}\"", k.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ");

    ExpandedQuery {
        original: query.to_string(),
        fts_query,
        keywords,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_english_stop_words_stripped() {
        let result = expand_query_local("show me the Rust error from yesterday");
        assert!(result.keywords.contains(&"rust".to_string()));
        assert!(result.keywords.contains(&"error".to_string()));
        assert!(!result.keywords.contains(&"the".to_string()));
        assert!(!result.keywords.contains(&"show".to_string()));
        assert!(!result.keywords.contains(&"me".to_string()));
        assert!(!result.keywords.contains(&"from".to_string()));
        assert!(!result.keywords.contains(&"yesterday".to_string()));
        assert!(result.fts_query.contains("OR"));
    }

    #[test]
    fn test_cjk_bigrams() {
        let result = expand_query_local("数据库错误");
        // 4 CJK chars → 3 bigrams: 数据, 据库, 库错, 错误  — wait, 5 chars → 4 bigrams
        assert_eq!(result.keywords.len(), 4);
        assert!(result.keywords.contains(&"数据".to_string()));
        assert!(result.keywords.contains(&"错误".to_string()));
    }

    #[test]
    fn test_korean_particles_stripped() {
        let result = expand_query_local("데이터베이스에서 오류가 발생");
        // "데이터베이스에서" → strip "에서" → "데이터베이스"
        // "오류가" → strip "가" → "오류"
        assert!(result.keywords.contains(&"데이터베이스".to_string()));
        assert!(result.keywords.contains(&"오류".to_string()));
    }

    #[test]
    fn test_spanish_detection_and_stop_words() {
        let result = expand_query_local("¿cómo puedo encontrar el error en la base de datos?");
        assert_eq!(
            detect_language("¿cómo puedo encontrar el error en la base de datos?"),
            Language::Spanish
        );
        assert!(!result.keywords.contains(&"el".to_string()));
        assert!(!result.keywords.contains(&"la".to_string()));
        assert!(!result.keywords.contains(&"de".to_string()));
    }

    #[test]
    fn test_portuguese_detection() {
        let result = expand_query_local("encontrar a configuração da conexão do banco");
        assert_eq!(
            detect_language("encontrar a configuração da conexão do banco"),
            Language::Portuguese
        );
        assert!(!result.keywords.contains(&"da".to_string()));
        assert!(!result.keywords.contains(&"do".to_string()));
        assert!(result.keywords.contains(&"banco".to_string()));
    }

    #[test]
    fn test_empty_query_fallback() {
        let result = expand_query_local("");
        assert!(result.keywords.is_empty());
        assert_eq!(result.fts_query, "");
    }

    #[test]
    fn test_single_word_query() {
        let result = expand_query_local("async");
        assert_eq!(result.keywords, vec!["async".to_string()]);
        assert_eq!(result.fts_query, "\"async\"");
    }

    #[test]
    fn test_single_multibyte_char_query_falls_back_to_original() {
        let result = expand_query_local("é");
        assert!(result.keywords.is_empty());
        assert_eq!(result.fts_query, "é");
    }

    #[test]
    fn test_fts_query_format() {
        let result = expand_query_local("Rust async runtime error handling");
        // Should produce quoted OR-joined keywords
        assert!(result.fts_query.contains("OR"));
        for kw in &result.keywords {
            assert!(result.fts_query.contains(&format!("\"{}\"", kw)));
        }
    }

    #[test]
    fn test_duplicate_keywords_removed() {
        let result = expand_query_local("error error error");
        assert_eq!(result.keywords, vec!["error".to_string()]);
    }

    #[test]
    fn test_parse_llm_keywords() {
        let result = parse_llm_keywords(
            "show me the API key error",
            "API, key, error, authentication, credentials",
        );
        assert_eq!(result.keywords.len(), 5);
        assert!(result.keywords.contains(&"api".to_string()));
        assert!(result.keywords.contains(&"credentials".to_string()));
    }

    #[test]
    fn test_parse_llm_keywords_deduplicates_normalized_terms() {
        let result = parse_llm_keywords("show me API errors", "API, api, Error, error");

        assert_eq!(
            result.keywords,
            vec!["api".to_string(), "error".to_string()]
        );
        assert_eq!(result.fts_query, "\"api\" OR \"error\"");
    }

    #[test]
    fn test_parse_llm_keywords_filters_query_language_stop_words() {
        let result = parse_llm_keywords(
            "show me the Rust error from yesterday",
            "the, rust, from, error, show",
        );

        assert_eq!(
            result.keywords,
            vec!["rust".to_string(), "error".to_string()]
        );
        assert_eq!(result.fts_query, "\"rust\" OR \"error\"");
    }

    #[test]
    fn test_parse_llm_keywords_filters_single_multibyte_chars() {
        let result = parse_llm_keywords("show me authentication errors", "é, 中, api");

        assert_eq!(result.keywords, vec!["api".to_string()]);
    }

    #[test]
    fn test_parse_llm_keywords_empty_fallback() {
        let result = parse_llm_keywords("show me the Rust error", "");
        // Should fall back to local expansion
        assert!(result.keywords.contains(&"rust".to_string()));
        assert!(result.keywords.contains(&"error".to_string()));
    }

    #[test]
    fn test_arabic_detection() {
        let lang = detect_language("بحث عن خطأ في قاعدة البيانات");
        assert_eq!(lang, Language::Arabic);
    }

    #[test]
    fn test_japanese_detection() {
        let lang = detect_language("データベースのエラーを探す");
        assert_eq!(lang, Language::Japanese);
        let result = expand_query_local("データベースのエラーを探す");
        // Should produce bigrams from kana/kanji
        assert!(!result.keywords.is_empty());
    }
}
