pub mod web_search;

use anyhow::Result;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use readability::extractor;
use regex::Regex;
use serde_json::{Value, json};
use std::fs;
use std::io::Cursor;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;

use super::providers::ToolSchema;
use crate::config::{Config, SearchProviderType};
use crate::memory::MemoryManager;

use web_search::{SearchRouter, WebSearchTool};

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub call_id: String,
    pub output: String,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, arguments: &str) -> Result<String>;
}

/// Create the safe (mobile-compatible) tools: memory search, memory get, web fetch, web search.
///
/// Dangerous tools (bash, read_file, write_file, edit_file) are provided by the CLI crate.
/// Use `Agent::new_with_tools()` to supply the full tool set.
pub fn create_safe_tools(
    config: &Config,
    memory: Option<Arc<MemoryManager>>,
) -> Result<Vec<Box<dyn Tool>>> {
    use super::hardcoded_filters;
    use super::tool_filters::CompiledToolFilter;

    let workspace = config.workspace_path();

    // Use indexed memory search if MemoryManager is provided, otherwise fallback to grep-based
    let memory_search_tool: Box<dyn Tool> = if let Some(ref mem) = memory {
        Box::new(MemorySearchToolWithIndex::new(Arc::clone(mem)))
    } else {
        Box::new(MemorySearchTool::new(workspace.clone()))
    };

    // Compile web_fetch filter from user config and merge small hardcoded
    // fail-fast deny rules (authoritative SSRF protection is still handled by
    // validate_web_fetch_url() with host parsing + DNS/IP checks).
    let web_fetch_filter = config
        .tools
        .filters
        .get("web_fetch")
        .map(CompiledToolFilter::compile)
        .unwrap_or_else(|| Ok(CompiledToolFilter::permissive()))?
        .merge_hardcoded(
            hardcoded_filters::WEB_FETCH_DENY_SUBSTRINGS,
            hardcoded_filters::WEB_FETCH_DENY_PATTERNS,
        )?;

    let mut tools: Vec<Box<dyn Tool>> = vec![
        memory_search_tool,
        Box::new(MemoryGetTool::new(workspace)),
        Box::new(WebFetchTool::new(
            config.tools.web_fetch_max_bytes,
            web_fetch_filter,
        )?),
    ];

    // Conditionally add web search tool
    if let Some(ref ws_config) = config.tools.web_search
        && !matches!(ws_config.provider, SearchProviderType::None)
    {
        match SearchRouter::from_config(ws_config) {
            Ok(router) => tools.push(Box::new(WebSearchTool::new(Arc::new(router)))),
            Err(e) => tracing::warn!("Web search init failed: {e}"),
        }
    }

    Ok(tools)
}

// Memory Search Tool
pub struct MemorySearchTool {
    workspace: PathBuf,
}

impl MemorySearchTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "memory_search".to_string(),
            description: "Search the memory index for relevant information".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 5)"
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
            .ok_or_else(|| anyhow::anyhow!("Missing query"))?;
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;

        debug!("Memory search: {} (limit: {})", query, limit);

        // Simple grep-based search for now
        // TODO: Use proper memory index
        let mut results = Vec::new();

        let memory_file = self.workspace.join("MEMORY.md");
        if memory_file.exists()
            && let Ok(content) = fs::read_to_string(&memory_file)
        {
            for (i, line) in content.lines().enumerate() {
                if line.to_lowercase().contains(&query.to_lowercase()) {
                    results.push(format!("MEMORY.md:{}: {}", i + 1, line));
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }

        // Search daily logs
        let memory_dir = self.workspace.join("memory");
        if memory_dir.exists()
            && let Ok(entries) = fs::read_dir(&memory_dir)
        {
            for entry in entries.filter_map(|e| e.ok()) {
                if results.len() >= limit {
                    break;
                }

                let path = entry.path();
                if path.extension().map(|e| e == "md").unwrap_or(false)
                    && let Ok(content) = fs::read_to_string(&path)
                {
                    let filename = path.file_name().unwrap().to_string_lossy();
                    for (i, line) in content.lines().enumerate() {
                        if line.to_lowercase().contains(&query.to_lowercase()) {
                            results.push(format!("memory/{}:{}: {}", filename, i + 1, line));
                            if results.len() >= limit {
                                break;
                            }
                        }
                    }
                }
            }
        }

        if results.is_empty() {
            Ok("No results found".to_string())
        } else {
            Ok(results.join("\n"))
        }
    }
}

// Memory Search Tool with Index - uses MemoryManager for hybrid FTS+vector search
pub struct MemorySearchToolWithIndex {
    memory: Arc<MemoryManager>,
}

impl MemorySearchToolWithIndex {
    pub fn new(memory: Arc<MemoryManager>) -> Self {
        Self { memory }
    }
}

#[async_trait]
impl Tool for MemorySearchToolWithIndex {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn schema(&self) -> ToolSchema {
        let description = if self.memory.has_embeddings() {
            "Search the memory index using hybrid semantic + keyword search for relevant information"
        } else {
            "Search the memory index for relevant information"
        };

        ToolSchema {
            name: "memory_search".to_string(),
            description: description.to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 5)"
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
            .ok_or_else(|| anyhow::anyhow!("Missing query"))?;
        let limit = args["limit"].as_u64().unwrap_or(5) as usize;

        let search_type = if self.memory.has_embeddings() {
            "hybrid"
        } else {
            "FTS"
        };
        debug!(
            "Memory search ({}): {} (limit: {})",
            search_type, query, limit
        );

        let results = self.memory.search(query, limit)?;

        if results.is_empty() {
            return Ok("No results found".to_string());
        }

        // Format results with relevance scores
        let formatted: Vec<String> = results
            .iter()
            .enumerate()
            .map(|(i, chunk)| {
                let preview: String = chunk.content.chars().take(200).collect();
                let preview = preview.replace('\n', " ");
                format!(
                    "{}. {} (lines {}-{}, score: {:.3})\n   {}{}",
                    i + 1,
                    chunk.file,
                    chunk.line_start,
                    chunk.line_end,
                    chunk.score,
                    preview,
                    if chunk.content.len() > 200 { "..." } else { "" }
                )
            })
            .collect();

        Ok(formatted.join("\n\n"))
    }
}

// Memory Get Tool - efficient snippet fetching after memory_search
pub struct MemoryGetTool {
    workspace: PathBuf,
}

impl MemoryGetTool {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    fn resolve_path(&self, path: &str) -> PathBuf {
        // Handle paths relative to workspace
        if path.starts_with("memory/") || path == "MEMORY.md" || path == "HEARTBEAT.md" {
            self.workspace.join(path)
        } else {
            PathBuf::from(shellexpand::tilde(path).to_string())
        }
    }
}

#[async_trait]
impl Tool for MemoryGetTool {
    fn name(&self) -> &str {
        "memory_get"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "memory_get".to_string(),
            description: "Safe snippet read from MEMORY.md or memory/*.md with optional line range; use after memory_search to pull only the needed lines and keep context small.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file (e.g., 'MEMORY.md' or 'memory/2024-01-15.md')"
                    },
                    "from": {
                        "type": "integer",
                        "description": "Starting line number (1-indexed, default: 1)"
                    },
                    "lines": {
                        "type": "integer",
                        "description": "Number of lines to read (default: 50)"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;

        let from = args["from"].as_u64().unwrap_or(1).max(1) as usize;
        let lines_count = args["lines"].as_u64().unwrap_or(50) as usize;

        let resolved_path = self.resolve_path(path);

        debug!(
            "Memory get: {} (from: {}, lines: {})",
            resolved_path.display(),
            from,
            lines_count
        );

        if !resolved_path.exists() {
            return Ok(format!("File not found: {}", path));
        }

        let content = fs::read_to_string(&resolved_path)?;
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // Convert from 1-indexed to 0-indexed
        let start = (from - 1).min(total_lines);
        let end = (start + lines_count).min(total_lines);

        if start >= total_lines {
            return Ok(format!(
                "Line {} is past end of file ({} lines)",
                from, total_lines
            ));
        }

        let selected: Vec<String> = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:4}\t{}", start + i + 1, line))
            .collect();

        let header = format!(
            "# {} (lines {}-{} of {})\n",
            path,
            start + 1,
            end,
            total_lines
        );
        Ok(header + &selected.join("\n"))
    }
}

fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    &s[..s.floor_char_boundary(max_bytes)]
}

fn is_private_ip(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(ip) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || (ip.segments()[0] & 0xffc0) == 0xfe80
                || (ip.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

fn is_blocked_hostname(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    let blocked = ["localhost", "metadata.google.internal", "169.254.169.254"];
    let blocked_tlds = [".local", ".internal", ".localhost"];

    blocked.contains(&host.as_str()) || blocked_tlds.iter().any(|tld| host.ends_with(tld))
}

async fn validate_web_fetch_url(url: &str) -> Result<reqwest::Url> {
    let parsed = reqwest::Url::parse(url)?;

    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("Only http/https URLs are allowed");
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("No host in URL"))?;

    if is_blocked_hostname(host) {
        anyhow::bail!("Blocked hostname: {}", host);
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            anyhow::bail!("URL resolves to private IP {} — blocked for security", ip);
        }
        return Ok(parsed);
    }

    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs = tokio::net::lookup_host((host, port)).await?;
    for addr in addrs {
        if is_private_ip(&addr.ip()) {
            anyhow::bail!(
                "URL {} resolves to private IP {} — blocked for security",
                url,
                addr.ip()
            );
        }
    }

    Ok(parsed)
}

const MAX_WEB_FETCH_REDIRECTS: usize = 10;

fn should_follow_redirect(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::MOVED_PERMANENTLY
            | reqwest::StatusCode::FOUND
            | reqwest::StatusCode::SEE_OTHER
            | reqwest::StatusCode::TEMPORARY_REDIRECT
            | reqwest::StatusCode::PERMANENT_REDIRECT
    )
}

async fn resolve_and_validate_redirect_target(
    current: &reqwest::Url,
    location: &str,
) -> Result<reqwest::Url> {
    let candidate = current
        .join(location)
        .map_err(|e| anyhow::anyhow!("Invalid redirect target '{}': {}", location, e))?;
    validate_web_fetch_url(candidate.as_str()).await
}

fn extract_fallback_text(html: &str) -> String {
    static SCRIPT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<script[^>]*>.*?</script>").expect("valid script regex"));
    static STYLE_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<style[^>]*>.*?</style>").expect("valid style regex"));
    static TAG_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?is)<[^>]+>").expect("valid tag regex"));
    static WS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").expect("valid whitespace regex"));

    let no_scripts = SCRIPT_RE.replace_all(html, " ");
    let no_styles = STYLE_RE.replace_all(&no_scripts, " ");
    let no_tags = TAG_RE.replace_all(&no_styles, " ");
    WS_RE.replace_all(no_tags.trim(), " ").to_string()
}

fn extract_readable_text(html: &str, url: &reqwest::Url) -> String {
    let mut cursor = Cursor::new(html.as_bytes());
    match extractor::extract(&mut cursor, url) {
        Ok(product) => {
            let text = product.text.trim();
            if text.is_empty() {
                return extract_fallback_text(html);
            }

            let title = product.title.trim();
            if title.is_empty() {
                text.to_string()
            } else {
                format!("# {}\n\n{}", title, text)
            }
        }
        Err(e) => {
            debug!("Readability extraction failed for {}: {}", url, e);
            extract_fallback_text(html)
        }
    }
}

// Web Fetch Tool
pub struct WebFetchTool {
    client: reqwest::Client,
    max_bytes: usize,
    filter: super::tool_filters::CompiledToolFilter,
}

impl WebFetchTool {
    pub fn new(max_bytes: usize, filter: super::tool_filters::CompiledToolFilter) -> Result<Self> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;

        Ok(Self {
            client,
            max_bytes,
            filter,
        })
    }

    async fn fetch_with_validated_redirects(
        &self,
        mut current_url: reqwest::Url,
    ) -> Result<(reqwest::Response, reqwest::Url)> {
        for redirect_count in 0..=MAX_WEB_FETCH_REDIRECTS {
            let response = self
                .client
                .get(current_url.clone())
                .header("User-Agent", "LocalGPT/0.1")
                .send()
                .await?;

            if !should_follow_redirect(response.status()) {
                return Ok((response, current_url));
            }

            if redirect_count == MAX_WEB_FETCH_REDIRECTS {
                anyhow::bail!(
                    "Too many redirects (>{}) while fetching {}",
                    MAX_WEB_FETCH_REDIRECTS,
                    current_url
                );
            }

            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Redirect response {} missing Location header",
                        response.status()
                    )
                })?
                .to_str()
                .map_err(|_| anyhow::anyhow!("Redirect Location header is not valid UTF-8"))?;

            let next_url = resolve_and_validate_redirect_target(&current_url, location).await?;
            debug!(
                "Following redirect {}: {} -> {}",
                redirect_count + 1,
                current_url,
                next_url
            );
            current_url = next_url;
        }

        unreachable!("redirect loop should return or bail")
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "web_fetch".to_string(),
            description: "Fetch content from a URL".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "The URL to fetch"
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let url = args["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing url"))?;

        // Check URL against SSRF deny filters (fast, static patterns)
        self.filter.check(url, "web_fetch", "url")?;

        let parsed_url = validate_web_fetch_url(url).await?;
        debug!("Fetching URL: {}", parsed_url);

        let (response, final_url) = self.fetch_with_validated_redirects(parsed_url).await?;

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = response.text().await?;
        let extracted =
            if content_type.contains("text/html") || content_type.contains("application/xhtml") {
                extract_readable_text(&body, &final_url)
            } else {
                body
            };

        // Truncate if too long
        let truncated = if extracted.len() > self.max_bytes {
            let prefix = truncate_on_char_boundary(&extracted, self.max_bytes);
            format!(
                "{}...\n\n[Truncated, {} bytes total]",
                prefix,
                extracted.len()
            )
        } else {
            extracted
        };

        Ok(format!(
            "Status: {}\nURL: {}\nContent-Type: {}\n\n{}",
            status, final_url, content_type, truncated
        ))
    }
}

/// Extract relevant detail from tool arguments for display.
/// Returns a human-readable summary of the key argument (file path, command, query, URL).
pub fn extract_tool_detail(tool_name: &str, arguments: &str) -> Option<String> {
    let args: Value = serde_json::from_str(arguments).ok()?;

    match tool_name {
        "edit_file" | "write_file" | "read_file" => args
            .get("path")
            .or_else(|| args.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "bash" => args.get("command").and_then(|v| v.as_str()).map(|s| {
            if s.len() > 60 {
                format!("{}...", &s[..57])
            } else {
                s.to_string()
            }
        }),
        "memory_search" => args
            .get("query")
            .and_then(|v| v.as_str())
            .map(|s| format!("\"{}\"", s)),
        "web_fetch" => args
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "web_search" => args
            .get("query")
            .and_then(|v| v.as_str())
            .map(|s| format!("\"{}\"", s)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_private_ip_v4_ranges() {
        assert!(is_private_ip(&"127.0.0.1".parse().unwrap()));
        assert!(is_private_ip(&"10.0.0.5".parse().unwrap()));
        assert!(is_private_ip(&"192.168.1.1".parse().unwrap()));
        assert!(is_private_ip(&"169.254.0.10".parse().unwrap()));
        assert!(!is_private_ip(&"8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_v6_ranges() {
        assert!(is_private_ip(&"::1".parse().unwrap()));
        assert!(is_private_ip(&"fe80::1".parse().unwrap()));
        assert!(is_private_ip(&"fc00::1".parse().unwrap()));
        assert!(!is_private_ip(&"2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn test_is_blocked_hostname() {
        assert!(is_blocked_hostname("localhost"));
        assert!(is_blocked_hostname("metadata.google.internal"));
        assert!(is_blocked_hostname("my-service.internal"));
        assert!(is_blocked_hostname("printer.local"));
        assert!(!is_blocked_hostname("example.com"));
    }

    #[test]
    fn test_extract_readable_text_removes_html() {
        let html = r#"
            <html><head><style>.x{display:none}</style></head>
            <body><script>alert(1)</script><h1>Title</h1><p>Hello <b>world</b>.</p></body></html>
        "#;
        let url = reqwest::Url::parse("https://example.com/test").unwrap();
        let text = extract_readable_text(html, &url);
        assert!(text.contains("Hello world"));
        assert!(!text.contains("alert(1)"));
    }

    #[tokio::test]
    async fn test_redirect_target_validation_blocks_private_ip() {
        let current = reqwest::Url::parse("https://93.184.216.34/start").unwrap();
        let err = resolve_and_validate_redirect_target(&current, "http://127.0.0.1/admin").await;
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("private IP"));
    }

    #[tokio::test]
    async fn test_redirect_target_validation_allows_relative_public_ip_target() {
        let current = reqwest::Url::parse("https://93.184.216.34/start").unwrap();
        let next = resolve_and_validate_redirect_target(&current, "/next")
            .await
            .unwrap();
        assert_eq!(next.as_str(), "https://93.184.216.34/next");
    }

    #[tokio::test]
    async fn test_redirect_target_validation_blocks_non_http_scheme() {
        let current = reqwest::Url::parse("https://93.184.216.34/start").unwrap();
        let err = resolve_and_validate_redirect_target(&current, "file:///etc/passwd").await;
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("Only http/https"));
    }
}
