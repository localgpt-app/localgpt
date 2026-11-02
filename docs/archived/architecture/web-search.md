# Web Search

**Status:** Archived 2026-11-01 — the assistant's `web_search` and `web_fetch` tools went with `localgpt-core`; the coding agents in the app's terminals bring their own web tools.

LocalGPT supported hybrid web search:

- Native provider search when the active model supports it (for example Anthropic and xAI)
- Client-side `web_search` tool fallback with configurable providers

## Quick Start with SearXNG (Recommended)

SearXNG gives you private, self-hosted web search without API costs.

1. Start SearXNG:

```bash
docker run -d --name searxng -p 8080:8080 \
  -e SEARXNG_SECRET="$(openssl rand -hex 32)" \
  searxng/searxng:latest
```

2. Configure LocalGPT (`~/.localgpt/config.toml`):

```toml
[tools.web_search]
provider = "searxng"
cache_enabled = true
cache_ttl = 900
max_results = 5
prefer_native = true

[tools.web_search.searxng]
base_url = "http://localhost:8080"
categories = "general"
language = "en"
time_range = ""
```

3. Test:

```bash
localgpt search test "latest rust release"
```

## Other Providers

Set `tools.web_search.provider` to one of:

- `brave`
- `tavily`
- `perplexity`

Then add the matching API key section:

```toml
[tools.web_search.brave]
api_key = "${BRAVE_API_KEY}"
```

```toml
[tools.web_search.tavily]
api_key = "${TAVILY_API_KEY}"
search_depth = "basic"
include_answer = true
```

```toml
[tools.web_search.perplexity]
api_key = "${PERPLEXITY_API_KEY}"
model = "sonar"
```

## Native Search Behavior

When `prefer_native = true` and the active LLM provider supports native search:

- LocalGPT hides the client-side `web_search` tool schema
- Native provider search tools are passed through in API requests

This avoids duplicate tool surfaces and uses fresher provider-native search indexes.

## Commands

```bash
# Validate search configuration with a live query
localgpt search test "rust async runtime"

# Show cumulative search usage and estimated spend
localgpt search stats
```
