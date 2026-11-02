# LocalGPT Web Search Spec

> **Status:** Archived 2026-11-01 — built for the assistant, which went with `localgpt-core`; the agents bring their own web tools  
> **Status when written:** Draft  
> **Author:** Yi  
> **Date:** 2026-02-15  
> **Milestone:** v0.3.0  
> **Effort:** ~3 weeks (Phase 1: 2 weeks, Phase 2: 1 week)

---

## 1. Problem Statement

LocalGPT currently has a `web_fetch` tool (HTTP GET + naive text extraction) but **no web search tool**. Users cannot ask their agent to research topics, check current information, or find answers online.

OpenClaw implements search as a client-side tool wrapping Brave, Perplexity, and Grok APIs. This works but has known pain points the community has flagged:

- **No native search passthrough** — models like Grok with built-in server-side search feel "artificially handicapped" (GitHub Issue #6872)
- **No self-hosted option** — SearXNG support is community-requested (Issue #15068) but only available via a third-party plugin
- **No cost visibility** — Brave charges $5/1,000 queries; Perplexity charges per-call; users have no spending insight
- **Brave dropped free tier** — the default provider now requires payment (Issue #16629)

LocalGPT can leapfrog by shipping a **hybrid search architecture** from day one: use native provider search when available, fall back to user-configured client-side search, and include SearXNG as a first-class zero-cost option.

---

## 2. Design Principles

1. **Hybrid native-first** — If the LLM provider offers server-side search, use it. It's faster, fresher, and free (included in API cost).
2. **Provider-agnostic fallback** — Client-side search tool works identically regardless of LLM backend.
3. **Self-hosted by default** — SearXNG is the recommended default: zero cost, zero tracking, fully local.
4. **Cost-transparent** — Track and surface every search query's cost alongside LLM token costs.
5. **Single binary** — No bundled SearXNG. LocalGPT is an HTTP client; the user runs SearXNG separately (or not).
6. **Permissive licenses only** — SearXNG is GPL but we only call its HTTP API. LocalGPT ships zero GPL code.

---

## 3. Architecture Overview

```
User Query
  │
  ▼
Agent Turn (LLM call)
  │
  ├─ Provider supports native search? ──► YES ──► Pass native tool definition
  │   (e.g., xAI Responses API)                   LLM calls it server-side
  │                                                Results returned in response
  │
  └─ NO (Anthropic, OpenAI, Ollama, etc.)
      │
      ▼
    Expose `web_search` as client-side tool
      │
      ▼
    LLM invokes `web_search(query, ...)`
      │
      ▼
    SearchRouter dispatches to configured provider
      │
      ├─ SearXNG  (self-hosted, free)
      ├─ Brave    (API key required, $5/1k queries)
      ├─ Tavily   (API key required, free tier available)
      └─ Perplexity Sonar (API key required)
      │
      ▼
    Results formatted + cached (15 min TTL)
      │
      ▼
    Cost recorded in session stats
      │
      ▼
    Returned to LLM as tool result
```

---

## 4. Configuration

### 4.1 TOML Schema

Add to `config.toml`:

```toml
#──────────────────────────────────────────────────────────────────────────────
# Web Search
#──────────────────────────────────────────────────────────────────────────────

[tools.web_search]
# Search provider: "searxng" | "brave" | "tavily" | "perplexity" | "none"
# Default: "none" (disabled until user configures)
provider = "searxng"

# Enable result caching (avoids duplicate queries within TTL)
cache_enabled = true

# Cache TTL in seconds (default: 900 = 15 minutes)
cache_ttl = 900

# Maximum results to return per query (default: 5, max: 10)
max_results = 5

# When true AND provider supports native search (e.g., xAI), prefer native.
# When false, always use client-side tool regardless of provider capability.
prefer_native = true

[tools.web_search.searxng]
# SearXNG instance URL (user-hosted)
# Common: http://localhost:8080 or https://searx.example.com
base_url = "http://localhost:8080"

# Optional: categories to search (comma-separated)
# Options: general, images, news, science, files, it, social media
categories = "general"

# Optional: search language (BCP 47)
language = "en"

# Optional: time range filter: "day" | "week" | "month" | "year" | ""
time_range = ""

[tools.web_search.brave]
api_key = "${BRAVE_API_KEY}"
# Optional: country code for result localization
country = ""
# Optional: freshness filter: "pd" (past day) | "pw" (past week) | "pm" (past month)
freshness = ""

[tools.web_search.tavily]
api_key = "${TAVILY_API_KEY}"
# Search depth: "basic" | "advanced"
search_depth = "basic"
# Include AI-generated answer summary
include_answer = true

[tools.web_search.perplexity]
api_key = "${PERPLEXITY_API_KEY}"
# Model: "sonar" | "sonar-pro" | "sonar-reasoning-pro"
model = "sonar"
```

### 4.2 Config Struct

```rust
// src/config/mod.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    pub web_search: Option<WebSearchConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchConfig {
    pub provider: SearchProvider,
    #[serde(default = "default_true")]
    pub cache_enabled: bool,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl: u64,       // seconds
    #[serde(default = "default_max_results")]
    pub max_results: u8,      // 1-10
    #[serde(default = "default_true")]
    pub prefer_native: bool,

    pub searxng: Option<SearxngConfig>,
    pub brave: Option<BraveConfig>,
    pub tavily: Option<TavilyConfig>,
    pub perplexity: Option<PerplexityConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchProvider {
    Searxng,
    Brave,
    Tavily,
    Perplexity,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearxngConfig {
    pub base_url: String,
    #[serde(default)]
    pub categories: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub time_range: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraveConfig {
    pub api_key: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub freshness: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TavilyConfig {
    pub api_key: String,
    #[serde(default = "default_basic")]
    pub search_depth: String,
    #[serde(default = "default_true")]
    pub include_answer: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerplexityConfig {
    pub api_key: String,
    #[serde(default = "default_sonar")]
    pub model: String,
}
```

---

## 5. Search Provider Trait

### 5.1 Core Trait

```rust
// src/agent/search/mod.rs

pub mod brave;
pub mod cache;
pub mod perplexity;
pub mod searxng;
pub mod tavily;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// A single search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// Provider-specific score (0.0-1.0) if available
    pub score: Option<f64>,
    /// ISO 8601 publish date if available
    pub published_date: Option<String>,
}

/// Metadata about the search execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMeta {
    pub provider: String,
    pub query: String,
    pub result_count: usize,
    pub latency_ms: u64,
    /// Estimated cost in USD (0.0 for free providers)
    pub estimated_cost_usd: f64,
    /// AI-synthesized answer (Perplexity/Tavily only)
    pub answer: Option<String>,
    pub cached: bool,
}

/// Response combining results and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
    pub meta: SearchMeta,
}

#[async_trait]
pub trait SearchProvider: Send + Sync {
    /// Provider name for logging and cost tracking
    fn name(&self) -> &str;

    /// Execute a search query
    async fn search(
        &self,
        query: &str,
        max_results: u8,
    ) -> anyhow::Result<SearchResponse>;

    /// Estimated cost per query in USD
    fn cost_per_query(&self) -> f64;
}
```

### 5.2 SearXNG Provider

```rust
// src/agent/search/searxng.rs

use super::*;
use reqwest::Client;

pub struct SearxngProvider {
    client: Client,
    config: SearxngConfig,
}

impl SearxngProvider {
    pub fn new(config: SearxngConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }
}

#[async_trait]
impl SearchProvider for SearxngProvider {
    fn name(&self) -> &str { "searxng" }

    async fn search(
        &self,
        query: &str,
        max_results: u8,
    ) -> anyhow::Result<SearchResponse> {
        let start = std::time::Instant::now();

        // SearXNG JSON API: GET /search?q=...&format=json
        let mut url = format!("{}/search", self.config.base_url.trim_end_matches('/'));
        let mut params = vec![
            ("q", query.to_string()),
            ("format", "json".to_string()),
            ("pageno", "1".to_string()),
        ];

        if !self.config.categories.is_empty() {
            params.push(("categories", self.config.categories.clone()));
        }
        if !self.config.language.is_empty() {
            params.push(("language", self.config.language.clone()));
        }
        if !self.config.time_range.is_empty() {
            params.push(("time_range", self.config.time_range.clone()));
        }

        let resp = self.client
            .get(&url)
            .query(&params)
            .header("Accept", "application/json")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("SearXNG returned HTTP {}", status);
        }

        let body: serde_json::Value = resp.json().await?;
        let latency = start.elapsed().as_millis() as u64;

        // Parse SearXNG JSON response
        let results = body["results"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .take(max_results as usize)
            .filter_map(|r| {
                Some(SearchResult {
                    title: r["title"].as_str()?.to_string(),
                    url: r["url"].as_str()?.to_string(),
                    snippet: r["content"].as_str().unwrap_or("").to_string(),
                    score: r["score"].as_f64(),
                    published_date: r["publishedDate"]
                        .as_str()
                        .map(|s| s.to_string()),
                })
            })
            .collect::<Vec<_>>();

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

    fn cost_per_query(&self) -> f64 { 0.0 }
}
```

### 5.3 Brave Provider

```rust
// src/agent/search/brave.rs

pub struct BraveProvider {
    client: Client,
    config: BraveConfig,
}

#[async_trait]
impl SearchProvider for BraveProvider {
    fn name(&self) -> &str { "brave" }

    async fn search(
        &self,
        query: &str,
        max_results: u8,
    ) -> anyhow::Result<SearchResponse> {
        let start = std::time::Instant::now();

        let mut params = vec![
            ("q", query.to_string()),
            ("count", max_results.to_string()),
        ];
        if !self.config.country.is_empty() {
            params.push(("country", self.config.country.clone()));
        }
        if !self.config.freshness.is_empty() {
            params.push(("freshness", self.config.freshness.clone()));
        }

        let resp = self.client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("X-Subscription-Token", &self.config.api_key)
            .header("Accept", "application/json")
            .query(&params)
            .send()
            .await?;

        if !resp.status().is_success() {
            anyhow::bail!("Brave Search API returned HTTP {}", resp.status());
        }

        let body: serde_json::Value = resp.json().await?;
        let latency = start.elapsed().as_millis() as u64;

        let results = body["web"]["results"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .filter_map(|r| {
                Some(SearchResult {
                    title: r["title"].as_str()?.to_string(),
                    url: r["url"].as_str()?.to_string(),
                    snippet: r["description"].as_str().unwrap_or("").to_string(),
                    score: None,
                    published_date: r["age"].as_str().map(|s| s.to_string()),
                })
            })
            .collect::<Vec<_>>();

        Ok(SearchResponse {
            meta: SearchMeta {
                provider: "brave".to_string(),
                query: query.to_string(),
                result_count: results.len(),
                latency_ms: latency,
                estimated_cost_usd: 0.005, // $5 per 1,000 queries
                answer: None,
                cached: false,
            },
            results,
        })
    }

    fn cost_per_query(&self) -> f64 { 0.005 }
}
```

### 5.4 Tavily Provider (sketch)

Tavily is included as a strong alternative to Brave: it has a free tier (1,000 queries/month), returns AI-synthesized answers, and is popular in the LangChain/agent ecosystem.

```rust
// src/agent/search/tavily.rs
// POST https://api.tavily.com/search
// Body: { "api_key": "...", "query": "...", "max_results": 5,
//         "search_depth": "basic", "include_answer": true }
// Cost: Free tier 1,000/month, then $0.005/query (same as Brave)
```

### 5.5 Perplexity Sonar Provider (sketch)

```rust
// src/agent/search/perplexity.rs
// POST https://api.perplexity.ai/chat/completions
// Body: { "model": "sonar", "messages": [{"role": "user", "content": query}] }
// Returns AI-synthesized answer with citations, not raw results.
// Cost: ~$0.001-$0.005/query depending on model tier
```

---

## 6. Result Cache

```rust
// src/agent/search/cache.rs

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub struct SearchCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

struct CacheEntry {
    response: SearchResponse,
    inserted_at: Instant,
}

impl SearchCache {
    pub fn new(ttl_seconds: u64) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_seconds),
        }
    }

    /// Cache key = "{provider}:{query_lowercase}"
    fn cache_key(provider: &str, query: &str) -> String {
        format!("{}:{}", provider, query.to_lowercase().trim())
    }

    pub async fn get(&self, provider: &str, query: &str) -> Option<SearchResponse> {
        let key = Self::cache_key(provider, query);
        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(&key) {
            if entry.inserted_at.elapsed() < self.ttl {
                let mut response = entry.response.clone();
                response.meta.cached = true;
                response.meta.estimated_cost_usd = 0.0; // cached = free
                return Some(response);
            }
        }
        None
    }

    pub async fn put(&self, provider: &str, query: &str, response: SearchResponse) {
        let key = Self::cache_key(provider, query);
        let mut entries = self.entries.write().await;
        entries.insert(key, CacheEntry {
            response,
            inserted_at: Instant::now(),
        });

        // Evict expired entries (lazy cleanup)
        entries.retain(|_, e| e.inserted_at.elapsed() < self.ttl);
    }
}
```

---

## 7. Search Router

The router selects the provider and handles cache:

```rust
// src/agent/search/router.rs

pub struct SearchRouter {
    provider: Box<dyn SearchProvider>,
    cache: SearchCache,
    max_results: u8,
}

impl SearchRouter {
    pub fn from_config(config: &WebSearchConfig) -> anyhow::Result<Self> {
        let provider: Box<dyn SearchProvider> = match config.provider {
            SearchProvider::Searxng => {
                let c = config.searxng.as_ref()
                    .ok_or_else(|| anyhow::anyhow!(
                        "tools.web_search.searxng config required when provider = 'searxng'"
                    ))?;
                Box::new(SearxngProvider::new(c.clone()))
            }
            SearchProvider::Brave => {
                let c = config.brave.as_ref()
                    .ok_or_else(|| anyhow::anyhow!(
                        "tools.web_search.brave config required when provider = 'brave'"
                    ))?;
                Box::new(BraveProvider::new(c.clone()))
            }
            SearchProvider::Tavily => {
                let c = config.tavily.as_ref()
                    .ok_or_else(|| anyhow::anyhow!(
                        "tools.web_search.tavily config required when provider = 'tavily'"
                    ))?;
                Box::new(TavilyProvider::new(c.clone()))
            }
            SearchProvider::Perplexity => {
                let c = config.perplexity.as_ref()
                    .ok_or_else(|| anyhow::anyhow!(
                        "tools.web_search.perplexity config required when provider = 'perplexity'"
                    ))?;
                Box::new(PerplexityProvider::new(c.clone()))
            }
            SearchProvider::None => {
                anyhow::bail!("Web search is disabled (provider = 'none')")
            }
        };

        let cache = SearchCache::new(
            if config.cache_enabled { config.cache_ttl } else { 0 }
        );

        Ok(Self {
            provider,
            cache,
            max_results: config.max_results.clamp(1, 10),
        })
    }

    pub async fn search(&self, query: &str) -> anyhow::Result<SearchResponse> {
        // Check cache first
        if let Some(cached) = self.cache.get(self.provider.name(), query).await {
            return Ok(cached);
        }

        // Execute search
        let response = self.provider.search(query, self.max_results).await?;

        // Cache result
        self.cache.put(self.provider.name(), query, response.clone()).await;

        Ok(response)
    }
}
```

---

## 8. Native Search Passthrough

### 8.1 Problem

When LLM providers expose server-side search (e.g., xAI's Responses API with `tools: [{type: "web_search"}]`), using it is strictly better: it's faster (parallel server-side queries), accesses fresher indexes, and is free (included in API cost). OpenClaw doesn't support this; LocalGPT should.

### 8.2 Detection

The LLM provider module is the right place to detect native search capability:

```rust
// src/agent/providers.rs (addition to LlmProvider trait)

#[async_trait]
pub trait LlmProvider: Send + Sync {
    // ... existing methods ...

    /// Does this provider support native server-side web search?
    /// If true, the agent should include native search tool definitions
    /// in the API call instead of the client-side web_search tool.
    fn supports_native_search(&self) -> bool { false }

    /// Returns native tool definitions for the provider's built-in tools.
    /// Only called when supports_native_search() returns true.
    fn native_tool_definitions(&self) -> Vec<serde_json::Value> { vec![] }
}
```

### 8.3 Provider Implementation

For xAI/Grok (the first provider with native search):

```rust
// src/agent/providers/xai.rs (new file)

impl LlmProvider for XaiProvider {
    fn supports_native_search(&self) -> bool { true }

    fn native_tool_definitions(&self) -> Vec<serde_json::Value> {
        vec![json!({ "type": "web_search" })]
    }

    async fn chat(&self, messages: &[Message], tools: &[Tool]) -> Result<Response> {
        // POST https://api.x.ai/v1/responses
        // Include native tools alongside custom tools
        let mut all_tools = self.native_tool_definitions();
        all_tools.extend(tools.iter().map(|t| t.to_json()));

        let body = json!({
            "model": self.model,
            "input": messages,
            "tools": all_tools,
        });

        // ... standard request handling ...
    }
}
```

### 8.4 Agent Tool Selection Logic

```rust
// src/agent/mod.rs (in tool registration)

fn register_tools(&mut self) {
    // Always register core tools
    self.register_tool(BashTool::new(/* ... */));
    self.register_tool(ReadFileTool::new(/* ... */));
    // ... other core tools ...

    // Web search: prefer native when available and configured
    let use_native = self.config.tools.web_search
        .as_ref()
        .map(|ws| ws.prefer_native)
        .unwrap_or(true)
        && self.provider.supports_native_search();

    if use_native {
        // Don't register client-side web_search tool.
        // Native search is passed through in the provider's tool definitions.
        info!("Using native search from provider: {}", self.provider.name());
    } else if let Some(ref ws_config) = self.config.tools.web_search {
        if !matches!(ws_config.provider, SearchProvider::None) {
            match SearchRouter::from_config(ws_config) {
                Ok(router) => {
                    self.register_tool(WebSearchTool::new(Arc::new(router)));
                    info!("Using client-side search provider: {:?}", ws_config.provider);
                }
                Err(e) => {
                    warn!("Failed to initialize web search: {}. Search disabled.", e);
                }
            }
        }
    }
}
```

### 8.5 Future Native Search Providers

| Provider | Native Search | Status | Notes |
|----------|--------------|--------|-------|
| xAI/Grok | `web_search` in Responses API | Ready to implement | Also has `x_search` for Twitter |
| Anthropic | `web_search_20250305` tool type | Available now | Pass `{type: "web_search_20250305", name: "web_search"}` in tools |
| OpenAI | Not available via API | Monitor | Only available in ChatGPT web interface |
| Google Gemini | Grounding with Google Search | Partially available | Requires `google_search_retrieval` tool config |
| Ollama / local | Never | N/A | Always use client-side fallback |

**Implementation priority:** Anthropic native search first (our most popular provider), then xAI.

---

## 9. The `web_search` Tool Definition

### 9.1 Tool Schema (exposed to LLM)

```json
{
    "name": "web_search",
    "description": "Search the web for current information. Use this when you need up-to-date facts, recent events, or information not in your training data. Returns titles, URLs, and snippets from top results.",
    "parameters": {
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
    }
}
```

### 9.2 Tool Implementation

```rust
// src/agent/tools/web_search.rs

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
    fn name(&self) -> &str { "web_search" }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_search".to_string(),
            description: "Search the web for current information...".to_string(),
            parameters: json!({ /* schema above */ }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: serde_json::Value = serde_json::from_str(arguments)?;
        let query = args["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing query parameter"))?;

        let count = args["count"].as_u64().unwrap_or(5) as u8;

        let response = self.router.search(query).await?;

        // Format results for the LLM
        let mut output = String::new();

        // Include AI answer if available (Perplexity/Tavily)
        if let Some(ref answer) = response.meta.answer {
            output.push_str(&format!("**AI Summary:**\n{}\n\n", answer));
        }

        output.push_str(&format!(
            "**Search results for:** {}\n",
            response.meta.query
        ));
        output.push_str(&format!(
            "*Provider: {} | {} results | {}ms{}*\n\n",
            response.meta.provider,
            response.meta.result_count,
            response.meta.latency_ms,
            if response.meta.cached { " | cached" } else { "" },
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

        Ok(output)
    }
}
```

---

## 10. Cost Tracking

### 10.1 Session-Level Tracking

Extend the existing session stats to include search costs:

```rust
// src/agent/session.rs (extend existing SessionStats)

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStats {
    // ... existing fields ...
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tool_calls: u64,

    // NEW: search-specific stats
    pub search_queries: u64,
    pub search_cached_hits: u64,
    pub search_cost_usd: f64,

    // NEW: combined cost estimate
    pub total_estimated_cost_usd: f64,
}
```

### 10.2 Cost Recording

After each search execution, record the cost:

```rust
// In WebSearchTool::execute(), after getting response:
if let Some(stats) = self.session_stats.as_ref() {
    let mut stats = stats.lock().await;
    stats.search_queries += 1;
    if response.meta.cached {
        stats.search_cached_hits += 1;
    }
    stats.search_cost_usd += response.meta.estimated_cost_usd;
    stats.total_estimated_cost_usd += response.meta.estimated_cost_usd;
}
```

### 10.3 Cost Display

Surface in `/status` slash command and HTTP API:

```
Session Stats:
  Messages: 12 (6 user, 6 assistant)
  Tokens:   4,230 in / 2,891 out
  Tools:    8 calls (3 bash, 2 web_search, 2 read_file, 1 memory_search)
  Search:   2 queries (1 cached) · $0.005 · Provider: brave
  Est Cost: $0.047 (LLM: $0.042 + Search: $0.005)
```

---

## 11. Improved `web_fetch`

Upgrade the existing `web_fetch` tool alongside the new `web_search`:

### 11.1 Readability Extraction

Replace naive text extraction with proper article parsing:

```rust
// Current: naive reqwest::get() + body text
// Improved: use `readability` crate for Mozilla Readability-style extraction

// Cargo.toml addition:
// readability = "0.3"  (MIT license)

use readability::extractor;

async fn fetch_readable(url: &str, max_chars: usize) -> Result<String> {
    let resp = self.client.get(url).send().await?;
    let html = resp.text().await?;

    // Extract main content using Readability algorithm
    let product = extractor::extract_from_str(&html, url)?;

    let mut content = format!("# {}\n\n{}", product.title, product.text);

    // Truncate to max chars
    if content.len() > max_chars {
        content.truncate(max_chars);
        content.push_str("\n\n[Content truncated]");
    }

    Ok(content)
}
```

### 11.2 SSRF Protection

Add private IP blocking (currently a gap vs. OpenClaw):

```rust
// src/agent/tools/web_fetch.rs

fn is_private_ip(addr: &std::net::IpAddr) -> bool {
    match addr {
        std::net::IpAddr::V4(ip) => {
            ip.is_loopback()           // 127.0.0.0/8
            || ip.is_private()          // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
            || ip.is_link_local()       // 169.254.0.0/16
            || ip.is_unspecified()      // 0.0.0.0
        }
        std::net::IpAddr::V6(ip) => {
            ip.is_loopback()            // ::1
            || ip.is_unspecified()      // ::
            // fe80::/10 (link-local), fc00::/7 (unique local)
            || (ip.segments()[0] & 0xffc0) == 0xfe80
            || (ip.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

fn is_blocked_hostname(host: &str) -> bool {
    let blocked = [
        "localhost",
        "metadata.google.internal",
        "169.254.169.254", // AWS/GCP metadata
    ];
    let blocked_tlds = [".local", ".internal", ".localhost"];

    blocked.contains(&host)
        || blocked_tlds.iter().any(|tld| host.ends_with(tld))
}

/// Resolve hostname and validate all IPs before connecting
async fn validate_url(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url)?;
    let host = parsed.host_str()
        .ok_or_else(|| anyhow::anyhow!("No host in URL"))?;

    if is_blocked_hostname(host) {
        anyhow::bail!("Blocked hostname: {}", host);
    }

    // DNS resolution and IP validation
    let addrs = tokio::net::lookup_host(format!(
        "{}:{}",
        host,
        parsed.port_or_known_default().unwrap_or(443)
    )).await?;

    for addr in addrs {
        if is_private_ip(&addr.ip()) {
            anyhow::bail!(
                "URL {} resolves to private IP {} — blocked for security",
                url, addr.ip()
            );
        }
    }

    Ok(())
}
```

---

## 12. File Layout

```
src/agent/
├── search/
│   ├── mod.rs          # SearchProvider trait, SearchResult, SearchResponse
│   ├── router.rs       # SearchRouter (provider selection + cache)
│   ├── cache.rs        # SearchCache (in-memory TTL cache)
│   ├── searxng.rs      # SearXNG provider
│   ├── brave.rs        # Brave Search provider
│   ├── tavily.rs       # Tavily provider
│   └── perplexity.rs   # Perplexity Sonar provider
├── tools/
│   ├── mod.rs          # existing tool registry
│   ├── web_search.rs   # WebSearchTool (wraps SearchRouter)
│   ├── web_fetch.rs    # improved web_fetch with Readability + SSRF
│   └── ...             # existing tools
└── providers/
    ├── xai.rs          # NEW: xAI provider with native search
    └── ...             # existing providers
```

---

## 13. CLI Additions

### 13.1 Search Test Command

```bash
# Test search configuration
localgpt search test "rust async runtime"

# Output:
# ✓ Search provider: searxng (http://localhost:8080)
# ✓ 5 results in 234ms (cost: $0.000)
#
# 1. Tokio - An Asynchronous Rust Runtime
#    https://tokio.rs
#    Tokio is an event-driven, non-blocking I/O platform...
# ...
```

### 13.2 Search Stats Command

```bash
# Show cumulative search usage
localgpt search stats

# Output:
# Search Statistics (since 2026-02-01):
#   Provider: brave
#   Total queries: 147
#   Cached hits: 43 (29%)
#   Estimated cost: $0.52
```

### 13.3 Clap Registration

```rust
// src/cli/mod.rs

#[derive(Subcommand)]
pub enum Commands {
    // ... existing ...

    /// Test and manage web search
    Search {
        #[command(subcommand)]
        action: SearchAction,
    },
}

#[derive(Subcommand)]
pub enum SearchAction {
    /// Test search with a query
    Test {
        /// Search query
        query: String,
    },
    /// Show search usage statistics
    Stats,
}
```

---

## 14. SearXNG Quick Start Documentation

Add to docs/web-search.md:

```markdown
## Quick Start with SearXNG (Recommended)

SearXNG gives you free, private web search with zero API keys.

### 1. Start SearXNG (one command)

    docker run -d --name searxng -p 8080:8080 \
      -e SEARXNG_SECRET=$(openssl rand -hex 32) \
      searxng/searxng:latest

### 2. Configure LocalGPT

    [tools.web_search]
    provider = "searxng"

    [tools.web_search.searxng]
    base_url = "http://localhost:8080"

### 3. Test it

    localgpt search test "latest rust release"

That's it. Your agent now has web search — fully local, zero cost, zero tracking.
```

---

## 15. Rollout Plan

### Phase 1: Core Search (2 weeks)

| Week | Task | Deliverable |
|------|------|-------------|
| 1.1 | `SearchProvider` trait + `SearchResult` types | `src/agent/search/mod.rs` |
| 1.2 | `SearxngProvider` implementation | `src/agent/search/searxng.rs` |
| 1.3 | `BraveProvider` implementation | `src/agent/search/brave.rs` |
| 1.4 | `SearchCache` with TTL | `src/agent/search/cache.rs` |
| 1.5 | `SearchRouter` (config → provider + cache) | `src/agent/search/router.rs` |
| 2.1 | `WebSearchTool` (Tool trait impl) | `src/agent/tools/web_search.rs` |
| 2.2 | Config structs + TOML parsing | `src/config/mod.rs` additions |
| 2.3 | Agent tool registration logic | `src/agent/mod.rs` changes |
| 2.4 | `web_fetch` SSRF protection | `src/agent/tools/web_fetch.rs` |
| 2.5 | `web_fetch` Readability extraction | `src/agent/tools/web_fetch.rs` |
| 2.6 | CLI `search test` + `search stats` | `src/cli/search.rs` |
| 2.7 | Cost tracking in session stats | `src/agent/session.rs` |
| 2.8 | Integration tests | `tests/search_*.rs` |

### Phase 2: Native Search + Polish (1 week)

| Day | Task | Deliverable |
|-----|------|-------------|
| 3.1 | `supports_native_search()` trait method | Provider trait extension |
| 3.2 | Anthropic native search passthrough | `src/agent/providers/anthropic.rs` |
| 3.3 | xAI provider + native search | `src/agent/providers/xai.rs` (new) |
| 3.4 | `TavilyProvider` + `PerplexityProvider` | Remaining providers |
| 3.5 | Documentation (web-search.md) | `docs/web-search.md` |
| 3.6 | Config template update | `config.example.toml` |
| 3.7 | CHANGELOG + release notes | `CHANGELOG.md` |

### Phase 3: Future (backlog)

- Google Gemini native grounding support
- Search result indexing into memory (agent can save useful findings)
- Multi-query parallel search (agent issues N queries simultaneously)
- Image search (SearXNG supports `categories=images`)
- MCP web_search tool (expose search via MCP protocol)

---

## 16. Dependencies

| Crate | License | Purpose | New? |
|-------|---------|---------|------|
| `reqwest` | MIT/Apache-2.0 | HTTP client (already used) | No |
| `serde` | MIT/Apache-2.0 | Serialization (already used) | No |
| `serde_json` | MIT/Apache-2.0 | JSON parsing (already used) | No |
| `url` | MIT/Apache-2.0 | URL parsing (already used) | No |
| `readability` | MIT | Article extraction | **Yes** |
| `tokio` | MIT | Async runtime (already used) | No |

Only one new dependency (`readability`), MIT-licensed. `cargo-deny` stays clean.

---

## 17. Testing Strategy

### Unit Tests

- Each provider: mock HTTP responses, verify parsing
- Cache: TTL expiration, key normalization, eviction
- Router: config validation, provider selection
- SSRF: private IP detection (IPv4, IPv6, mapped addresses)

### Integration Tests

```rust
#[tokio::test]
#[ignore] // requires live SearXNG instance
async fn test_searxng_live() {
    let config = SearxngConfig {
        base_url: "http://localhost:8080".to_string(),
        ..Default::default()
    };
    let provider = SearxngProvider::new(config);
    let results = provider.search("rust programming", 3).await.unwrap();
    assert!(!results.results.is_empty());
    assert_eq!(results.meta.estimated_cost_usd, 0.0);
}
```

### CI

- Unit tests run on every PR
- Integration tests run nightly with a SearXNG container in CI
- `cargo deny check licenses` verifies no GPL contamination

---

## 18. Success Metrics

| Metric | Target | How to Measure |
|--------|--------|----------------|
| Search available out-of-box | SearXNG working in <2 min setup | Manual test |
| Provider switching | Change one TOML line, restart, works | Manual test |
| Cache hit rate | >25% in typical sessions | Session stats |
| Latency overhead | <50ms added vs. direct API call | Benchmarks |
| Cost accuracy | Within 10% of actual provider billing | Compare stats vs. invoices |
| Native search | Anthropic + xAI passthrough working | Integration tests |
| Zero GPL code | `cargo deny` passes | CI |

---

## 19. OpenClaw Comparison (Post-Implementation)

| Feature | OpenClaw | LocalGPT (after this spec) |
|---------|----------|--------------------------|
| Search providers | 3 (Brave, Perplexity, Grok) | 4 (SearXNG, Brave, Tavily, Perplexity) |
| Self-hosted search | Community plugin only | First-class SearXNG support |
| Native search passthrough | No | Yes (Anthropic, xAI) |
| Cost tracking | No | Per-query + session aggregate |
| Result caching | 15 min TTL | Configurable TTL |
| SSRF protection | Full | Full (parity) |
| Zero-cost option | No (Brave dropped free tier) | Yes (SearXNG) |
| Setup friction | API key required | `docker run` + 2 TOML lines |

**Status after implementation: AHEAD of OpenClaw.**
