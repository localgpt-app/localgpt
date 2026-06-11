use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use super::Tool;
use crate::agent::providers::ToolSchema;
use crate::config::{
    BraveConfig, PerplexityConfig, SearchProviderType, SearxngConfig, TavilyConfig, WebSearchConfig,
};

/// Percent-encode a string for use in URL query parameters.
fn url_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                let _ = write!(encoded, "%{:02X}", b);
            }
        }
    }
    encoded
}

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub score: Option<f64>,
    pub published_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMeta {
    pub provider: String,
    pub query: String,
    pub result_count: usize,
    pub latency_ms: u64,
    pub estimated_cost_usd: f64,
    pub answer: Option<String>,
    pub cached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub meta: SearchMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchUsageStats {
    pub since: String,
    pub provider: String,
    pub total_queries: u64,
    pub cached_hits: u64,
    pub estimated_cost_usd: f64,
}

impl Default for SearchUsageStats {
    fn default() -> Self {
        Self {
            since: chrono::Utc::now().format("%Y-%m-%d").to_string(),
            provider: "none".to_string(),
            total_queries: 0,
            cached_hits: 0,
            estimated_cost_usd: 0.0,
        }
    }
}

fn search_stats_path() -> Result<PathBuf> {
    let paths = crate::paths::Paths::resolve()?;
    Ok(paths.state_dir.join("search_stats.json"))
}

pub fn read_search_usage_stats() -> Result<SearchUsageStats> {
    let path = search_stats_path()?;
    if !path.exists() {
        return Ok(SearchUsageStats::default());
    }
    let content = fs::read_to_string(path)?;
    let stats: SearchUsageStats = serde_json::from_str(&content)?;
    Ok(stats)
}

pub fn record_search_usage(provider: &str, cached: bool, estimated_cost_usd: f64) -> Result<()> {
    let path = search_stats_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut stats = if path.exists() {
        let content = fs::read_to_string(&path)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        SearchUsageStats::default()
    };

    if stats.provider == "none" {
        stats.provider = provider.to_string();
    } else if stats.provider != provider {
        stats.provider = "mixed".to_string();
    }

    stats.total_queries += 1;
    if cached {
        stats.cached_hits += 1;
    }
    stats.estimated_cost_usd += estimated_cost_usd.max(0.0);

    fs::write(path, serde_json::to_string_pretty(&stats)?)?;
    Ok(())
}

// ── Provider Trait ───────────────────────────────────────────────────────────

#[async_trait]
pub trait SearchProvider: Send + Sync {
    fn name(&self) -> &str;
    async fn search(&self, query: &str, max_results: u8) -> Result<SearchResponse>;
    fn cost_per_query(&self) -> f64;
}

// ── SearXNG Provider ─────────────────────────────────────────────────────────

pub struct SearxngProvider {
    client: reqwest::Client,
    config: SearxngConfig,
}

impl SearxngProvider {
    pub fn new(config: SearxngConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    pub fn parse_response(body: &Value, max_results: u8) -> Vec<SearchResult> {
        let empty = vec![];
        body["results"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .take(max_results as usize)
            .filter_map(|r| {
                Some(SearchResult {
                    title: r["title"].as_str()?.to_string(),
                    url: r["url"].as_str()?.to_string(),
                    snippet: r["content"].as_str().unwrap_or("").to_string(),
                    score: r["score"].as_f64(),
                    published_date: r["publishedDate"].as_str().map(|s| s.to_string()),
                })
            })
            .collect()
    }
}

#[async_trait]
impl SearchProvider for SearxngProvider {
    fn name(&self) -> &str {
        "searxng"
    }

    async fn search(&self, query: &str, max_results: u8) -> Result<SearchResponse> {
        let start = Instant::now();

        let base = self.config.base_url.trim_end_matches('/');
        let mut url = format!(
            "{}/search?q={}&format=json&pageno=1",
            base,
            url_encode(query)
        );

        if !self.config.categories.is_empty() {
            url.push_str(&format!(
                "&categories={}",
                url_encode(&self.config.categories)
            ));
        }
        if !self.config.language.is_empty() {
            url.push_str(&format!("&language={}", url_encode(&self.config.language)));
        }
        if !self.config.time_range.is_empty() {
            url.push_str(&format!(
                "&time_range={}",
                url_encode(&self.config.time_range)
            ));
        }

        let resp = self
            .client
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("SearXNG returned HTTP {}", status);
        }

        let body: Value = resp.json().await?;
        let latency = start.elapsed().as_millis() as u64;
        let results = Self::parse_response(&body, max_results);

        Ok(SearchResponse {
            meta: SearchMeta {
                provider: "searxng".to_string(),
                query: query.to_string(),
                result_count: results.len(),
                latency_ms: latency,
                estimated_cost_usd: 0.0,
                answer: None,
                cached: false,
            },
            results,
        })
    }

    fn cost_per_query(&self) -> f64 {
        0.0
    }
}

// ── Brave Provider ───────────────────────────────────────────────────────────

pub struct BraveProvider {
    client: reqwest::Client,
    config: BraveConfig,
}

impl BraveProvider {
    pub fn new(config: BraveConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    pub fn parse_response(body: &Value, max_results: u8) -> Vec<SearchResult> {
        let empty = vec![];
        body["web"]["results"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .take(max_results as usize)
            .filter_map(|r| {
                Some(SearchResult {
                    title: r["title"].as_str()?.to_string(),
                    url: r["url"].as_str()?.to_string(),
                    snippet: r["description"].as_str().unwrap_or("").to_string(),
                    score: None,
                    published_date: r["age"].as_str().map(|s| s.to_string()),
                })
            })
            .collect()
    }
}

#[async_trait]
impl SearchProvider for BraveProvider {
    fn name(&self) -> &str {
        "brave"
    }

    async fn search(&self, query: &str, max_results: u8) -> Result<SearchResponse> {
        let start = Instant::now();

        let mut url = format!(
            "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
            url_encode(query),
            max_results
        );

        if !self.config.country.is_empty() {
            url.push_str(&format!("&country={}", url_encode(&self.config.country)));
        }
        if !self.config.freshness.is_empty() {
            url.push_str(&format!(
                "&freshness={}",
                url_encode(&self.config.freshness)
            ));
        }

        let resp = self
            .client
            .get(&url)
            .header("X-Subscription-Token", &self.config.api_key)
            .header("Accept", "application/json")
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Brave Search API returned HTTP {}", resp.status());
        }

        let body: Value = resp.json().await?;
        let latency = start.elapsed().as_millis() as u64;
        let results = Self::parse_response(&body, max_results);

        Ok(SearchResponse {
            meta: SearchMeta {
                provider: "brave".to_string(),
                query: query.to_string(),
                result_count: results.len(),
                latency_ms: latency,
                estimated_cost_usd: 0.005,
                answer: None,
                cached: false,
            },
            results,
        })
    }

    fn cost_per_query(&self) -> f64 {
        0.005
    }
}

// ── Tavily Provider ──────────────────────────────────────────────────────────

pub struct TavilyProvider {
    client: reqwest::Client,
    config: TavilyConfig,
}

impl TavilyProvider {
    pub fn new(config: TavilyConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    pub fn parse_response(body: &Value, max_results: u8) -> (Vec<SearchResult>, Option<String>) {
        let empty = vec![];
        let results = body["results"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .take(max_results as usize)
            .filter_map(|r| {
                Some(SearchResult {
                    title: r["title"].as_str()?.to_string(),
                    url: r["url"].as_str()?.to_string(),
                    snippet: r["content"].as_str().unwrap_or("").to_string(),
                    score: r["score"].as_f64(),
                    published_date: r["published_date"]
                        .as_str()
                        .or_else(|| r["publishedDate"].as_str())
                        .map(|s| s.to_string()),
                })
            })
            .collect();

        let answer = body["answer"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        (results, answer)
    }
}

#[async_trait]
impl SearchProvider for TavilyProvider {
    fn name(&self) -> &str {
        "tavily"
    }

    async fn search(&self, query: &str, max_results: u8) -> Result<SearchResponse> {
        let start = Instant::now();

        let resp = self
            .client
            .post("https://api.tavily.com/search")
            .header("Content-Type", "application/json")
            .json(&json!({
                "api_key": self.config.api_key,
                "query": query,
                "max_results": max_results,
                "search_depth": self.config.search_depth,
                "include_answer": self.config.include_answer
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Tavily API returned HTTP {}", resp.status());
        }

        let body: Value = resp.json().await?;
        let latency = start.elapsed().as_millis() as u64;
        let (results, answer) = Self::parse_response(&body, max_results);

        Ok(SearchResponse {
            meta: SearchMeta {
                provider: "tavily".to_string(),
                query: query.to_string(),
                result_count: results.len(),
                latency_ms: latency,
                estimated_cost_usd: 0.005,
                answer,
                cached: false,
            },
            results,
        })
    }

    fn cost_per_query(&self) -> f64 {
        0.005
    }
}

// ── Perplexity Provider ──────────────────────────────────────────────────────

pub struct PerplexityProvider {
    client: reqwest::Client,
    config: PerplexityConfig,
}

impl PerplexityProvider {
    pub fn new(config: PerplexityConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    fn host_from_url(url: &str) -> String {
        url.split('/')
            .nth(2)
            .map(|host| host.to_string())
            .filter(|host| !host.is_empty())
            .unwrap_or_else(|| "Citation".to_string())
    }

    pub fn parse_response(body: &Value, max_results: u8) -> (Vec<SearchResult>, Option<String>) {
        let answer = body["choices"]
            .get(0)
            .and_then(|choice| choice["message"]["content"].as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let mut results = Vec::new();

        if let Some(search_results) = body["search_results"].as_array() {
            for item in search_results.iter().take(max_results as usize) {
                if let Some(url) = item["url"].as_str() {
                    results.push(SearchResult {
                        title: item["title"]
                            .as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| Self::host_from_url(url)),
                        url: url.to_string(),
                        snippet: item["snippet"].as_str().unwrap_or("").to_string(),
                        score: item["score"].as_f64(),
                        published_date: item["date"].as_str().map(|s| s.to_string()),
                    });
                }
            }
        }

        if results.is_empty() {
            let citations = body["citations"]
                .as_array()
                .or_else(|| {
                    body["choices"]
                        .get(0)
                        .and_then(|choice| choice["message"]["citations"].as_array())
                })
                .cloned()
                .unwrap_or_default();

            for citation in citations.iter().take(max_results as usize) {
                if let Some(url) = citation.as_str() {
                    results.push(SearchResult {
                        title: Self::host_from_url(url),
                        url: url.to_string(),
                        snippet: String::new(),
                        score: None,
                        published_date: None,
                    });
                }
            }
        }

        (results, answer)
    }
}

#[async_trait]
impl SearchProvider for PerplexityProvider {
    fn name(&self) -> &str {
        "perplexity"
    }

    async fn search(&self, query: &str, max_results: u8) -> Result<SearchResponse> {
        let start = Instant::now();

        let resp = self
            .client
            .post("https://api.perplexity.ai/chat/completions")
            .header("Authorization", format!("Bearer {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&json!({
                "model": self.config.model,
                "messages": [{
                    "role": "user",
                    "content": query
                }],
                "stream": false
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Perplexity API returned HTTP {}", resp.status());
        }

        let body: Value = resp.json().await?;
        let latency = start.elapsed().as_millis() as u64;
        let (results, answer) = Self::parse_response(&body, max_results);

        Ok(SearchResponse {
            meta: SearchMeta {
                provider: "perplexity".to_string(),
                query: query.to_string(),
                result_count: results.len(),
                latency_ms: latency,
                estimated_cost_usd: 0.003,
                answer,
                cached: false,
            },
            results,
        })
    }

    fn cost_per_query(&self) -> f64 {
        0.003
    }
}

// ── Cache ────────────────────────────────────────────────────────────────────

struct CacheEntry {
    response: SearchResponse,
    inserted_at: Instant,
}

pub struct SearchCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

impl SearchCache {
    pub fn new(ttl_seconds: u64) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_seconds),
        }
    }

    fn cache_key(provider: &str, query: &str, max_results: u8) -> String {
        format!(
            "{}:{}:{}",
            provider,
            query.to_lowercase().trim(),
            max_results
        )
    }

    pub async fn get(
        &self,
        provider: &str,
        query: &str,
        max_results: u8,
    ) -> Option<SearchResponse> {
        let key = Self::cache_key(provider, query, max_results);
        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(&key)
            && entry.inserted_at.elapsed() < self.ttl
        {
            let mut response = entry.response.clone();
            response.meta.cached = true;
            response.meta.estimated_cost_usd = 0.0;
            return Some(response);
        }
        None
    }

    pub async fn put(
        &self,
        provider: &str,
        query: &str,
        max_results: u8,
        response: SearchResponse,
    ) {
        let key = Self::cache_key(provider, query, max_results);
        let mut entries = self.entries.write().await;
        entries.insert(
            key,
            CacheEntry {
                response,
                inserted_at: Instant::now(),
            },
        );
        // Lazy eviction of expired entries
        let ttl = self.ttl;
        entries.retain(|_, e| e.inserted_at.elapsed() < ttl);
    }
}

// ── Router ───────────────────────────────────────────────────────────────────

pub struct SearchRouter {
    provider: Box<dyn SearchProvider>,
    cache: SearchCache,
    max_results: u8,
}

impl std::fmt::Debug for SearchRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchRouter")
            .field("provider", &self.provider.name())
            .field("max_results", &self.max_results)
            .finish()
    }
}

impl SearchRouter {
    pub fn from_config(config: &WebSearchConfig) -> Result<Self> {
        let provider: Box<dyn SearchProvider> = match config.provider {
            SearchProviderType::Searxng => {
                let c = config.searxng.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "tools.web_search.searxng config required when provider = 'searxng'"
                    )
                })?;
                Box::new(SearxngProvider::new(c.clone()))
            }
            SearchProviderType::Brave => {
                let c = config.brave.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "tools.web_search.brave config required when provider = 'brave'"
                    )
                })?;
                Box::new(BraveProvider::new(c.clone()))
            }
            SearchProviderType::Tavily => {
                let c = config.tavily.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "tools.web_search.tavily config required when provider = 'tavily'"
                    )
                })?;
                Box::new(TavilyProvider::new(c.clone()))
            }
            SearchProviderType::Perplexity => {
                let c = config.perplexity.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "tools.web_search.perplexity config required when provider = 'perplexity'"
                    )
                })?;
                Box::new(PerplexityProvider::new(c.clone()))
            }
            SearchProviderType::None => {
                anyhow::bail!("Web search is disabled (provider = 'none')")
            }
        };

        let cache = SearchCache::new(if config.cache_enabled {
            config.cache_ttl
        } else {
            0
        });

        Ok(Self {
            provider,
            cache,
            max_results: config.max_results.clamp(1, 10),
        })
    }

    pub fn provider_name(&self) -> &str {
        self.provider.name()
    }

    pub async fn search(&self, query: &str) -> Result<SearchResponse> {
        self.search_with_count(query, None).await
    }

    pub async fn search_with_count(
        &self,
        query: &str,
        count: Option<u8>,
    ) -> Result<SearchResponse> {
        let requested = count.unwrap_or(self.max_results).clamp(1, 10);

        // Check cache first
        if let Some(cached) = self.cache.get(self.provider.name(), query, requested).await {
            if let Err(e) =
                record_search_usage(self.provider.name(), true, cached.meta.estimated_cost_usd)
            {
                warn!("Failed to record search usage stats: {}", e);
            }
            return Ok(cached);
        }

        let response = self.provider.search(query, requested).await?;

        self.cache
            .put(self.provider.name(), query, requested, response.clone())
            .await;

        if let Err(e) = record_search_usage(
            self.provider.name(),
            response.meta.cached,
            response.meta.estimated_cost_usd,
        ) {
            warn!("Failed to record search usage stats: {}", e);
        }

        Ok(response)
    }
}

// ── WebSearchTool ────────────────────────────────────────────────────────────

pub struct WebSearchTool {
    router: Arc<SearchRouter>,
}

impl WebSearchTool {
    pub fn new(router: Arc<SearchRouter>) -> Self {
        Self { router }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_search".to_string(),
            description: "Search the web for current information. Use this when you need \
                up-to-date facts, recent events, or information not in your training data. \
                Returns titles, URLs, and snippets from top results."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query. Be specific and use keywords for best results."
                    },
                    "count": {
                        "type": "integer",
                        "description": "Number of results to return (1-10, default: 5)",
                        "minimum": 1,
                        "maximum": 10
                    }
                },
                "required": ["query"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let query = args["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing query parameter"))?;
        let count = args["count"].as_u64().map(|n| n.clamp(1, 10) as u8);

        debug!("Web search: {} (count={:?})", query, count);

        let response = self.router.search_with_count(query, count).await?;

        // Include synthesized answer when provider supplies one
        let mut output = String::new();
        if let Some(answer) = response.meta.answer.as_ref() {
            output.push_str(&format!("**AI Summary:**\n{}\n\n", answer));
        }

        output.push_str(&format!(
            "**Search results for:** {}\n*Provider: {} | {} results | {}ms{}*\n\n",
            response.meta.query,
            response.meta.provider,
            response.meta.result_count,
            response.meta.latency_ms,
            if response.meta.cached {
                " | cached"
            } else {
                ""
            },
        ));

        for (i, result) in response.results.iter().enumerate() {
            output.push_str(&format!(
                "{}. **{}**\n   {}\n   {}\n\n",
                i + 1,
                result.title,
                result.url,
                result.snippet,
            ));
        }

        if response.results.is_empty() {
            output.push_str("No results found.\n");
        }

        Ok(output)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_hit() {
        let cache = SearchCache::new(60);
        let response = SearchResponse {
            results: vec![SearchResult {
                title: "Test".to_string(),
                url: "https://example.com".to_string(),
                snippet: "A test result".to_string(),
                score: None,
                published_date: None,
            }],
            meta: SearchMeta {
                provider: "test".to_string(),
                query: "hello".to_string(),
                result_count: 1,
                latency_ms: 100,
                estimated_cost_usd: 0.005,
                answer: None,
                cached: false,
            },
        };

        cache.put("test", "hello", 5, response).await;

        let cached = cache.get("test", "hello", 5).await;
        assert!(cached.is_some());
        let cached = cached.unwrap();
        assert!(cached.meta.cached);
        assert_eq!(cached.meta.estimated_cost_usd, 0.0);
        assert_eq!(cached.results.len(), 1);
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let cache = SearchCache::new(60);
        let result = cache.get("test", "nonexistent", 5).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_ttl_expiry() {
        let cache = SearchCache::new(0); // 0-second TTL
        let response = SearchResponse {
            results: vec![],
            meta: SearchMeta {
                provider: "test".to_string(),
                query: "hello".to_string(),
                result_count: 0,
                latency_ms: 0,
                estimated_cost_usd: 0.0,
                answer: None,
                cached: false,
            },
        };

        cache.put("test", "hello", 5, response).await;

        // With TTL=0, entry should be expired immediately
        tokio::time::sleep(Duration::from_millis(10)).await;
        let result = cache.get("test", "hello", 5).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_key_normalization() {
        let cache = SearchCache::new(60);
        let response = SearchResponse {
            results: vec![],
            meta: SearchMeta {
                provider: "test".to_string(),
                query: "Hello World".to_string(),
                result_count: 0,
                latency_ms: 0,
                estimated_cost_usd: 0.0,
                answer: None,
                cached: false,
            },
        };

        cache.put("test", "Hello World", 5, response).await;

        // Should match case-insensitive
        let cached = cache.get("test", "hello world", 5).await;
        assert!(cached.is_some());

        // Should match with whitespace trimmed
        let cached = cache.get("test", "  hello world  ", 5).await;
        assert!(cached.is_some());
    }

    #[tokio::test]
    async fn test_cache_key_includes_result_count() {
        let cache = SearchCache::new(60);
        let response = SearchResponse {
            results: vec![],
            meta: SearchMeta {
                provider: "test".to_string(),
                query: "hello".to_string(),
                result_count: 0,
                latency_ms: 0,
                estimated_cost_usd: 0.0,
                answer: None,
                cached: false,
            },
        };

        cache.put("test", "hello", 5, response).await;

        assert!(cache.get("test", "hello", 5).await.is_some());
        assert!(cache.get("test", "hello", 3).await.is_none());
    }

    #[test]
    fn test_router_missing_searxng_config() {
        let config = WebSearchConfig {
            provider: SearchProviderType::Searxng,
            cache_enabled: true,
            cache_ttl: 900,
            max_results: 5,
            prefer_native: true,
            searxng: None,
            brave: None,
            tavily: None,
            perplexity: None,
        };
        let result = SearchRouter::from_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("searxng config required")
        );
    }

    #[test]
    fn test_router_missing_brave_config() {
        let config = WebSearchConfig {
            provider: SearchProviderType::Brave,
            cache_enabled: true,
            cache_ttl: 900,
            max_results: 5,
            prefer_native: true,
            searxng: None,
            brave: None,
            tavily: None,
            perplexity: None,
        };
        let result = SearchRouter::from_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("brave config required")
        );
    }

    #[test]
    fn test_router_none_provider() {
        let config = WebSearchConfig {
            provider: SearchProviderType::None,
            cache_enabled: true,
            cache_ttl: 900,
            max_results: 5,
            prefer_native: true,
            searxng: None,
            brave: None,
            tavily: None,
            perplexity: None,
        };
        let result = SearchRouter::from_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("disabled"));
    }

    #[test]
    fn test_router_missing_tavily_config() {
        let config = WebSearchConfig {
            provider: SearchProviderType::Tavily,
            cache_enabled: true,
            cache_ttl: 900,
            max_results: 5,
            prefer_native: true,
            searxng: None,
            brave: None,
            tavily: None,
            perplexity: None,
        };
        let result = SearchRouter::from_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("tavily config required")
        );
    }

    #[test]
    fn test_router_missing_perplexity_config() {
        let config = WebSearchConfig {
            provider: SearchProviderType::Perplexity,
            cache_enabled: true,
            cache_ttl: 900,
            max_results: 5,
            prefer_native: true,
            searxng: None,
            brave: None,
            tavily: None,
            perplexity: None,
        };
        let result = SearchRouter::from_config(&config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("perplexity config required")
        );
    }

    #[test]
    fn test_searxng_parse_response() {
        let body: Value = serde_json::from_str(
            r#"{
                "results": [
                    {
                        "title": "Rust Programming Language",
                        "url": "https://www.rust-lang.org",
                        "content": "A language empowering everyone to build reliable software.",
                        "score": 0.95,
                        "publishedDate": "2024-01-15"
                    },
                    {
                        "title": "Rust by Example",
                        "url": "https://doc.rust-lang.org/rust-by-example/",
                        "content": "Learn Rust by solving small exercises."
                    }
                ]
            }"#,
        )
        .unwrap();

        let results = SearxngProvider::parse_response(&body, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org");
        assert_eq!(results[0].score, Some(0.95));
        assert_eq!(results[0].published_date, Some("2024-01-15".to_string()));
        assert_eq!(results[1].title, "Rust by Example");
        assert!(results[1].score.is_none());
    }

    #[test]
    fn test_searxng_parse_response_max_results() {
        let body: Value = serde_json::from_str(
            r#"{
                "results": [
                    {"title": "A", "url": "https://a.com", "content": ""},
                    {"title": "B", "url": "https://b.com", "content": ""},
                    {"title": "C", "url": "https://c.com", "content": ""}
                ]
            }"#,
        )
        .unwrap();

        let results = SearxngProvider::parse_response(&body, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_brave_parse_response() {
        let body: Value = serde_json::from_str(
            r#"{
                "web": {
                    "results": [
                        {
                            "title": "Tokio - Async Rust Runtime",
                            "url": "https://tokio.rs",
                            "description": "Tokio is an event-driven platform for async I/O.",
                            "age": "2 days ago"
                        },
                        {
                            "title": "Async in Rust",
                            "url": "https://rust-lang.github.io/async-book/",
                            "description": "The async book for Rust."
                        }
                    ]
                }
            }"#,
        )
        .unwrap();

        let results = BraveProvider::parse_response(&body, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Tokio - Async Rust Runtime");
        assert_eq!(results[0].url, "https://tokio.rs");
        assert_eq!(results[0].published_date, Some("2 days ago".to_string()));
        assert!(results[0].score.is_none());
    }

    #[test]
    fn test_brave_parse_response_max_results() {
        let body: Value = serde_json::from_str(
            r#"{
                "web": {
                    "results": [
                        {"title": "A", "url": "https://a.com", "description": ""},
                        {"title": "B", "url": "https://b.com", "description": ""},
                        {"title": "C", "url": "https://c.com", "description": ""}
                    ]
                }
            }"#,
        )
        .unwrap();

        let results = BraveProvider::parse_response(&body, 2);

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "A");
        assert_eq!(results[1].title, "B");
    }

    #[test]
    fn test_tavily_parse_response_with_answer() {
        let body: Value = serde_json::from_str(
            r#"{
                "answer": "Rust is a systems programming language.",
                "results": [
                    {
                        "title": "Rust Language",
                        "url": "https://www.rust-lang.org",
                        "content": "Official Rust website.",
                        "score": 0.98,
                        "published_date": "2025-10-01"
                    }
                ]
            }"#,
        )
        .unwrap();

        let (results, answer) = TavilyProvider::parse_response(&body, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org");
        assert_eq!(
            answer,
            Some("Rust is a systems programming language.".to_string())
        );
    }

    #[test]
    fn test_perplexity_parse_response_with_citations() {
        let body: Value = serde_json::from_str(
            r#"{
                "choices": [{
                    "message": {
                        "content": "Tokio is Rust's async runtime."
                    }
                }],
                "citations": [
                    "https://tokio.rs",
                    "https://doc.rust-lang.org/std/future/"
                ]
            }"#,
        )
        .unwrap();

        let (results, answer) = PerplexityProvider::parse_response(&body, 5);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].url, "https://tokio.rs");
        assert_eq!(answer, Some("Tokio is Rust's async runtime.".to_string()));
    }

    #[test]
    fn test_web_search_tool_schema() {
        let config = WebSearchConfig {
            provider: SearchProviderType::Searxng,
            cache_enabled: true,
            cache_ttl: 900,
            max_results: 5,
            prefer_native: true,
            searxng: Some(SearxngConfig {
                base_url: "http://localhost:8080".to_string(),
                categories: String::new(),
                language: String::new(),
                time_range: String::new(),
            }),
            brave: None,
            tavily: None,
            perplexity: None,
        };
        let router = SearchRouter::from_config(&config).unwrap();
        let tool = WebSearchTool::new(Arc::new(router));

        let schema = tool.schema();
        assert_eq!(schema.name, "web_search");
        assert!(schema.description.contains("Search the web"));

        let params = &schema.parameters;
        assert_eq!(params["required"][0], "query");
        assert_eq!(params["properties"]["query"]["type"], "string");
        assert_eq!(params["properties"]["count"]["type"], "integer");
    }

    #[test]
    fn test_output_formatting() {
        let response = SearchResponse {
            results: vec![
                SearchResult {
                    title: "First Result".to_string(),
                    url: "https://first.com".to_string(),
                    snippet: "First snippet".to_string(),
                    score: None,
                    published_date: None,
                },
                SearchResult {
                    title: "Second Result".to_string(),
                    url: "https://second.com".to_string(),
                    snippet: "Second snippet".to_string(),
                    score: None,
                    published_date: None,
                },
            ],
            meta: SearchMeta {
                provider: "test".to_string(),
                query: "test query".to_string(),
                result_count: 2,
                latency_ms: 150,
                estimated_cost_usd: 0.0,
                answer: None,
                cached: false,
            },
        };

        let mut output = format!(
            "**Search results for:** {}\n*Provider: {} | {} results | {}ms{}*\n\n",
            response.meta.query,
            response.meta.provider,
            response.meta.result_count,
            response.meta.latency_ms,
            if response.meta.cached {
                " | cached"
            } else {
                ""
            },
        );

        for (i, result) in response.results.iter().enumerate() {
            output.push_str(&format!(
                "{}. **{}**\n   {}\n   {}\n\n",
                i + 1,
                result.title,
                result.url,
                result.snippet,
            ));
        }

        assert!(output.contains("**Search results for:** test query"));
        assert!(output.contains("Provider: test"));
        assert!(output.contains("1. **First Result**"));
        assert!(output.contains("2. **Second Result**"));
        assert!(output.contains("https://first.com"));
        assert!(!output.contains("cached"));
    }

    #[tokio::test]
    #[ignore = "requires a live SearXNG instance"]
    async fn test_searxng_live() {
        let base_url = std::env::var(crate::env::LOCALGPT_TEST_SEARXNG_URL)
            .unwrap_or_else(|_| "http://localhost:8080".to_string());
        let provider = SearxngProvider::new(SearxngConfig {
            base_url,
            categories: String::new(),
            language: String::new(),
            time_range: String::new(),
        });

        let response = provider
            .search("rust programming language", 3)
            .await
            .expect("live SearXNG query should succeed");

        assert!(!response.results.is_empty());
        assert_eq!(response.meta.estimated_cost_usd, 0.0);
    }
}
