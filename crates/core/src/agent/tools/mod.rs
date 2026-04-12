pub mod spawn_agent;
pub mod ssrf;
pub mod web_search;

use anyhow::Result;
use async_trait::async_trait;
use once_cell::sync::Lazy;
use readability::extractor;
use regex::Regex;
use serde_json::{Value, json};
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;

use super::providers::ToolSchema;
use crate::config::{Config, SearchProviderType};
use crate::memory::MemoryManager;

use spawn_agent::{SpawnAgentTool, SpawnContext};
use web_search::{SearchRouter, WebSearchTool};

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub call_id: String,
    pub output: String,
}

/// Permission level required to execute a tool.
///
/// Tools default to `Safe`. CLI dangerous tools (bash, file write, etc.) override
/// to `Elevated`. Admin tools (config edit, key rotation) override to `Admin`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    /// Read-only tools: memory search, web fetch, etc.
    Safe = 0,
    /// File write, shell exec, browser automation.
    Elevated = 1,
    /// Config changes, daemon control, encryption key rotation.
    Admin = 2,
}

impl Default for PermissionLevel {
    fn default() -> Self {
        Self::Safe
    }
}

impl std::fmt::Display for PermissionLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Safe => f.write_str("safe"),
            Self::Elevated => f.write_str("elevated"),
            Self::Admin => f.write_str("admin"),
        }
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, arguments: &str) -> Result<String>;

    /// Permission level required to execute this tool. Default: Safe.
    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Safe
    }

    /// MCP tool annotations (readOnlyHint, destructiveHint, etc.).
    ///
    /// Returns `None` by default. Override to provide annotations per the MCP spec.
    fn annotations(&self) -> Option<Value> {
        None
    }
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
        Box::new(MemoryGetTool::new(workspace.clone())),
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

    // Document loader tool (always available — uses shell commands for extraction)
    tools.push(Box::new(DocumentLoadTool::new(workspace, &config.tools)));

    // Wiki tools (structured knowledge management)
    if config.memory.wiki_enabled
        && let Some(ref mem) = memory
    {
        match crate::memory::wiki::WikiStore::new(
            mem.db_path(),
            config.memory.wiki_fresh_days,
            config.memory.wiki_stale_days,
        ) {
            Ok(store) => {
                let store = Arc::new(store);
                tools.push(Box::new(WikiAddTool::new(Arc::clone(&store))));
                tools.push(Box::new(WikiSearchTool::new(Arc::clone(&store))));
                tools.push(Box::new(WikiStatusTool::new(store)));
            }
            Err(e) => tracing::warn!("Wiki store init failed: {e}"),
        }
    }

    // Audio transcription tool (only if STT providers are configured)
    if let Some(ref stt_config) = config.tools.stt {
        let env_vars: std::collections::HashMap<String, String> = std::env::vars().collect();
        let registry = crate::media::SttRegistry::from_config(stt_config, &env_vars);
        if registry.has_providers() {
            let audio_cache = if config.tools.media_cache_enabled {
                Some(crate::media::cache::MediaCache::new(
                    config.workspace_path().join(".cache").join("media"),
                    config.tools.media_cache_max_mb,
                ))
            } else {
                None
            };
            tools.push(Box::new(AudioTranscribeTool::new(
                Arc::new(registry),
                config.workspace_path(),
                audio_cache,
            )));
        } else {
            tracing::debug!("STT configured but no providers available (missing API keys?)");
        }
    }

    Ok(tools)
}

/// Create spawn_agent tool for hierarchical delegation.
///
/// This tool allows an agent to spawn specialist subagents for tasks like
/// exploration, planning, implementation, or analysis.
///
/// # Arguments
/// * `config` - Application configuration (cloned)
/// * `memory` - Memory manager (shared with parent agent, required)
///
/// # Returns
/// A boxed spawn_agent tool
pub fn create_spawn_agent_tool(config: Config, memory: Arc<MemoryManager>) -> Box<dyn Tool> {
    Box::new(SpawnAgentTool::from_config(config, memory))
}

/// Create spawn_agent tool with custom depth (for subagents).
///
/// Subagents get spawn_agent tool only if they're below the max depth.
pub fn create_spawn_agent_tool_at_depth(
    config: Config,
    memory: Arc<MemoryManager>,
    depth: u8,
) -> Option<Box<dyn Tool>> {
    let max_depth = config.agent.max_spawn_depth.unwrap_or(1);

    if depth >= max_depth {
        // At or past max depth, don't provide spawn_agent
        return None;
    }

    let tool = SpawnAgentTool::new(SpawnContext {
        depth,
        config,
        memory,
        model: None,
        max_depth,
    });

    Some(Box::new(tool))
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

        // Format results with citation-style references
        let formatted: Vec<String> = results
            .iter()
            .enumerate()
            .map(|(i, chunk)| {
                let preview: String = chunk.content.chars().take(200).collect();
                let preview = preview.replace('\n', " ");
                format!(
                    "{}. [{}:{}-{}] (score: {:.3})\n   {}{}",
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

    /// Validate that a resolved path stays within the workspace directory.
    /// Checks the parent directory's canonical path if the file doesn't exist yet.
    fn is_within_workspace(&self, resolved: &std::path::Path) -> bool {
        let workspace_canonical = match self.workspace.canonicalize() {
            Ok(p) => p,
            Err(_) => return false,
        };
        // Try canonicalizing the file itself first
        if let Ok(canonical) = resolved.canonicalize() {
            return canonical.starts_with(&workspace_canonical);
        }
        // File doesn't exist — check the parent directory instead
        if let Some(parent) = resolved.parent()
            && let Ok(parent_canonical) = parent.canonicalize()
        {
            return parent_canonical.starts_with(&workspace_canonical);
        }
        false
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

        // Reject null bytes in raw input
        if path.contains('\0') {
            anyhow::bail!("Invalid path: null bytes not allowed");
        }

        let from = args["from"].as_u64().unwrap_or(1).max(1) as usize;
        let lines_count = (args["lines"].as_u64().unwrap_or(50) as usize).min(10_000);

        let resolved_path = self.resolve_path(path);

        // Check for path traversal on the resolved path (catches .. after tilde expansion)
        if resolved_path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            anyhow::bail!("Invalid path: path traversal not allowed");
        }

        // Verify resolved path stays within workspace
        if !self.is_within_workspace(&resolved_path) {
            anyhow::bail!("Access denied: path is outside workspace");
        }

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

// Document Load Tool — extracts text from PDF, DOCX, EPUB, HTML via shell commands
pub struct DocumentLoadTool {
    loaders: crate::media::DocumentLoaders,
    workspace: PathBuf,
    max_bytes: usize,
    output_max_chars: usize,
    cache: Option<crate::media::cache::MediaCache>,
}

impl DocumentLoadTool {
    pub fn new(workspace: PathBuf, config: &crate::config::ToolsConfig) -> Self {
        let loaders = match config.document_loaders {
            Some(ref custom) => crate::media::DocumentLoaders::with_custom(custom),
            None => crate::media::DocumentLoaders::new(),
        };
        let cache = if config.media_cache_enabled {
            Some(crate::media::cache::MediaCache::new(
                workspace.join(".cache").join("media"),
                config.media_cache_max_mb,
            ))
        } else {
            None
        };
        Self {
            loaders,
            workspace,
            max_bytes: config.document_max_bytes,
            output_max_chars: config.tool_output_max_chars,
            cache,
        }
    }

    fn validate_path(&self, path_str: &str) -> Result<PathBuf> {
        if path_str.contains('\0') {
            anyhow::bail!("Invalid path: null bytes not allowed");
        }
        let expanded = shellexpand::tilde(path_str).to_string();
        let resolved = if std::path::Path::new(&expanded).is_absolute() {
            PathBuf::from(expanded)
        } else {
            self.workspace.join(expanded)
        };
        if resolved
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            anyhow::bail!("Invalid path: path traversal not allowed");
        }
        Ok(resolved)
    }
}

#[async_trait]
impl Tool for DocumentLoadTool {
    fn name(&self) -> &str {
        "document_load"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "document_load".to_string(),
            description: "Extract text content from PDF, DOCX, EPUB, or HTML documents. Returns the document text.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the document file (relative to workspace or absolute)"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;

        let resolved = self.validate_path(path_str)?;

        if !resolved.exists() {
            anyhow::bail!("File not found: {}", path_str);
        }

        let metadata = fs::metadata(&resolved)?;
        if metadata.len() as usize > self.max_bytes {
            anyhow::bail!(
                "File too large: {} bytes (max: {} bytes / {}MB)",
                metadata.len(),
                self.max_bytes,
                self.max_bytes / 1_048_576
            );
        }

        let ext = resolved.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !self.loaders.has_loader(ext) {
            let supported = self.loaders.supported_extensions().join(", ");
            anyhow::bail!("Unsupported format: .{}. Supported: {}", ext, supported);
        }

        // Check cache
        if let Some(ref cache) = self.cache
            && let Some(cached) = cache.get(&resolved)
        {
            return Ok(cached);
        }

        debug!("Loading document: {} ({})", resolved.display(), ext);
        let text = self.loaders.extract_text(&resolved)?;

        if let Some(ref cache) = self.cache {
            let _ = cache.put(&resolved, &text);
        }

        if self.output_max_chars > 0 && text.len() > self.output_max_chars {
            let truncated = truncate_on_char_boundary(&text, self.output_max_chars);
            Ok(format!(
                "{}\n\n[Truncated, {} chars total]",
                truncated,
                text.len()
            ))
        } else {
            Ok(text)
        }
    }
}

// Audio Transcribe Tool — transcribes audio files via Groq/OpenAI/CLI
pub struct AudioTranscribeTool {
    registry: Arc<crate::media::SttRegistry>,
    workspace: PathBuf,
    cache: Option<crate::media::cache::MediaCache>,
}

impl AudioTranscribeTool {
    pub fn new(
        registry: Arc<crate::media::SttRegistry>,
        workspace: PathBuf,
        cache: Option<crate::media::cache::MediaCache>,
    ) -> Self {
        Self {
            registry,
            workspace,
            cache,
        }
    }

    fn validate_path(&self, path_str: &str) -> Result<PathBuf> {
        if path_str.contains('\0') {
            anyhow::bail!("Invalid path: null bytes not allowed");
        }
        let expanded = shellexpand::tilde(path_str).to_string();
        let resolved = if std::path::Path::new(&expanded).is_absolute() {
            PathBuf::from(expanded)
        } else {
            self.workspace.join(expanded)
        };
        if resolved
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            anyhow::bail!("Invalid path: path traversal not allowed");
        }
        Ok(resolved)
    }
}

#[async_trait]
impl Tool for AudioTranscribeTool {
    fn name(&self) -> &str {
        "transcribe_audio"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "transcribe_audio".to_string(),
            description: "Transcribe audio files (MP3, M4A, WAV, OGG, FLAC, WEBM) to text using speech-to-text.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the audio file"
                    },
                    "language": {
                        "type": "string",
                        "description": "Language hint (ISO 639-1, e.g., 'en', 'zh', 'ja'). Default: 'en'"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let path_str = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;

        let resolved = self.validate_path(path_str)?;

        if !resolved.exists() {
            anyhow::bail!("File not found: {}", path_str);
        }

        let mime_type = crate::media::audio::mime_type_from_path(&resolved);
        if mime_type == "audio/octet-stream" {
            let ext = resolved.extension().and_then(|e| e.to_str()).unwrap_or("?");
            anyhow::bail!(
                "Unsupported audio format: .{}. Supported: ogg, opus, mp3, m4a, wav, webm, flac",
                ext
            );
        }

        // Check cache
        if let Some(ref cache) = self.cache
            && let Some(cached) = cache.get(&resolved)
        {
            return Ok(cached);
        }

        let audio_data = fs::read(&resolved)?;
        debug!(
            "Transcribing audio: {} ({} bytes, {})",
            resolved.display(),
            audio_data.len(),
            mime_type
        );

        let text = self.registry.transcribe(&audio_data, mime_type).await?;

        if let Some(ref cache) = self.cache {
            let _ = cache.put(&resolved, &text);
        }

        Ok(text)
    }
}

fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    &s[..s.floor_char_boundary(max_bytes)]
}

/// Delegates to [`ssrf::validate_url`] — validates URL for SSRF safety before
/// any HTTP request is made (scheme, hostname, IP range, DNS pinning).
async fn validate_web_fetch_url(url: &str) -> Result<reqwest::Url> {
    ssrf::validate_url(url).await
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

        // Limit download size to prevent memory exhaustion from malicious servers.
        // Allow up to 2x max_bytes raw download since extraction often shrinks content.
        let download_limit = self.max_bytes * 2;

        // Fast reject via Content-Length header when available
        if let Some(content_length) = response.content_length()
            && content_length as usize > download_limit
        {
            anyhow::bail!(
                "Response too large ({} bytes, limit {})",
                content_length,
                download_limit
            );
        }

        // Stream response body with size cap (handles chunked/missing Content-Length)
        let mut body_bytes = Vec::new();
        let mut stream = response.bytes_stream();
        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            body_bytes.extend_from_slice(&chunk);
            if body_bytes.len() > download_limit {
                anyhow::bail!(
                    "Response too large (>{} bytes), download aborted",
                    download_limit
                );
            }
        }
        let body = String::from_utf8_lossy(&body_bytes).to_string();
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
        "edit_file" | "write_file" | "read_file" | "replace" => args
            .get("path")
            .or_else(|| args.get("file_path"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "bash" | "run_shell_command" => args.get("command").and_then(|v| v.as_str()).map(|s| {
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
            .or_else(|| args.get("prompt"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "web_search" | "google_web_search" => args
            .get("query")
            .and_then(|v| v.as_str())
            .map(|s| format!("\"{}\"", s)),
        "grep_search" | "glob" => args
            .get("pattern")
            .and_then(|v| v.as_str())
            .map(|s| format!("\"{}\"", s)),
        "list_directory" => args
            .get("dir_path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "codebase_investigator" => args
            .get("objective")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "document_load" => args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "transcribe_audio" => args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),

        // Gen tools - 3D scene manipulation
        "gen_spawn_primitive" => {
            let name = args.get("name").and_then(|v| v.as_str());
            let shape = args.get("shape").and_then(|v| v.as_str()).unwrap_or("?");
            name.map(|n| format!("{} ({})", n, shape))
        }
        "gen_spawn_batch" => args
            .get("entities")
            .and_then(|v| v.as_array())
            .map(|arr| format!("{} entities", arr.len())),
        "gen_modify_batch" => args
            .get("entities")
            .and_then(|v| v.as_array())
            .map(|arr| format!("{} entities", arr.len())),
        "gen_delete_batch" => args
            .get("names")
            .and_then(|v| v.as_array())
            .map(|arr| format!("{} entities", arr.len())),
        "gen_spawn_mesh" => args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_modify_entity" => args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_delete_entity" => args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_entity_info" => args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_set_light" => args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_load_gltf" => args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_export_screenshot" => args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_export_gltf" => args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_save_world" => args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| format!("'{}'", s)),
        "gen_load_world" => args
            .get("path")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_export_world" => args
            .get("format")
            .and_then(|v| v.as_str())
            .map(|f| format!("format: {}", f)),

        // Gen tools - audio
        "gen_audio_emitter" => args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_modify_audio" => args
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),

        // Gen tools - behaviors
        "gen_add_behavior" => {
            let entity = args.get("entity").and_then(|v| v.as_str());
            let behavior_type = args
                .get("behavior")
                .and_then(|b| b.get("type"))
                .and_then(|v| v.as_str());
            match (entity, behavior_type) {
                (Some(e), Some(t)) => Some(format!("{} [{}]", e, t)),
                (Some(e), None) => Some(e.to_string()),
                _ => None,
            }
        }
        "gen_remove_behavior" => args
            .get("entity")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "gen_list_behaviors" => args
            .get("entity")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),

        // Gen tools with no meaningful detail
        "gen_scene_info"
        | "gen_screenshot"
        | "gen_set_camera"
        | "gen_set_environment"
        | "gen_set_ambience"
        | "gen_audio_info"
        | "gen_pause_behaviors"
        | "gen_clear_scene" => None,

        _ => None,
    }
}

// ── Wiki Tools ──────────────────────────────────────────────────────────

/// wiki_add — Add or update a structured knowledge claim with evidence.
pub struct WikiAddTool {
    store: Arc<crate::memory::wiki::WikiStore>,
}

impl WikiAddTool {
    pub fn new(store: Arc<crate::memory::wiki::WikiStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for WikiAddTool {
    fn name(&self) -> &str {
        "wiki_add"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "wiki_add".to_string(),
            description: "Add or update a structured knowledge claim with optional evidence. Deduplicates similar claims automatically.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The claim text"
                    },
                    "category": {
                        "type": "string",
                        "enum": ["fact", "preference", "decision", "question"],
                        "description": "Claim category (default: fact)"
                    },
                    "confidence": {
                        "type": "number",
                        "description": "Confidence score 0.0-1.0 (default: 0.8)"
                    },
                    "evidence_source": {
                        "type": "string",
                        "description": "Source of evidence (file path, URL, session ID)"
                    },
                    "evidence_excerpt": {
                        "type": "string",
                        "description": "Relevant text excerpt from the source"
                    }
                },
                "required": ["text"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let text = args["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing text"))?;

        let category = args["category"]
            .as_str()
            .map(crate::memory::wiki::ClaimCategory::parse)
            .transpose()?
            .unwrap_or(crate::memory::wiki::ClaimCategory::Fact);

        let confidence = args["confidence"].as_f64().unwrap_or(0.8) as f32;
        let evidence_source = args["evidence_source"].as_str();
        let evidence_excerpt = args["evidence_excerpt"].as_str();

        let id = self.store.add_claim(
            text,
            category,
            confidence,
            evidence_source,
            evidence_excerpt,
        )?;

        Ok(format!("Claim stored (id: {}, category: {})", id, category))
    }
}

/// wiki_search — Search structured knowledge claims.
pub struct WikiSearchTool {
    store: Arc<crate::memory::wiki::WikiStore>,
}

impl WikiSearchTool {
    pub fn new(store: Arc<crate::memory::wiki::WikiStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for WikiSearchTool {
    fn name(&self) -> &str {
        "wiki_search"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "wiki_search".to_string(),
            description: "Search structured knowledge claims by text, category, or freshness."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "category": {
                        "type": "string",
                        "enum": ["fact", "preference", "decision", "question"],
                        "description": "Filter by category (optional)"
                    },
                    "include_stale": {
                        "type": "boolean",
                        "description": "Include stale claims (default: false)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results (default: 10)"
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

        let category = args["category"]
            .as_str()
            .map(crate::memory::wiki::ClaimCategory::parse)
            .transpose()?;

        let include_stale = args["include_stale"].as_bool().unwrap_or(false);
        let limit = args["limit"].as_u64().unwrap_or(10) as usize;

        let claims = self.store.search(query, category, include_stale, limit)?;

        if claims.is_empty() {
            return Ok("No claims found".to_string());
        }

        let formatted: Vec<String> = claims
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let freshness = self.store.freshness(c.updated_at);
                let evidence_summary = if c.evidence.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n   Evidence ({}):\n{}",
                        c.evidence.len(),
                        c.evidence
                            .iter()
                            .take(3)
                            .map(|e| format!(
                                "   - [{}] {}",
                                e.source,
                                e.excerpt.chars().take(80).collect::<String>()
                            ))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                };
                format!(
                    "{}. [{}] ({}, {}, conf: {:.1}) {freshness}\n   {}{}",
                    i + 1,
                    c.id.chars().take(8).collect::<String>(),
                    c.category,
                    c.status,
                    c.confidence,
                    c.text,
                    evidence_summary,
                    freshness = freshness,
                )
            })
            .collect();

        Ok(formatted.join("\n\n"))
    }
}

/// wiki_status — Knowledge base health overview.
pub struct WikiStatusTool {
    store: Arc<crate::memory::wiki::WikiStore>,
}

impl WikiStatusTool {
    pub fn new(store: Arc<crate::memory::wiki::WikiStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for WikiStatusTool {
    fn name(&self) -> &str {
        "wiki_status"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "wiki_status".to_string(),
            description: "Get knowledge base health overview: total claims, breakdown by category/status/freshness, top stale claims.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        }
    }

    async fn execute(&self, _arguments: &str) -> Result<String> {
        let status = self.store.status()?;

        let mut out = format!(
            "## Knowledge Base Status\n\nTotal claims: {}\n",
            status.total_claims
        );

        if !status.by_category.is_empty() {
            out.push_str("\n**By category:**\n");
            for (cat, count) in &status.by_category {
                out.push_str(&format!("- {}: {}\n", cat, count));
            }
        }

        if !status.by_status.is_empty() {
            out.push_str("\n**By status:**\n");
            for (st, count) in &status.by_status {
                out.push_str(&format!("- {}: {}\n", st, count));
            }
        }

        out.push_str("\n**By freshness:**\n");
        for (freshness, count) in &status.by_freshness {
            out.push_str(&format!("- {}: {}\n", freshness, count));
        }

        if !status.top_stale.is_empty() {
            out.push_str("\n**Top stale claims:**\n");
            for c in &status.top_stale {
                out.push_str(&format!(
                    "- [{}] {}\n",
                    c.id.chars().take(8).collect::<String>(),
                    c.text
                ));
            }
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SSRF unit tests for is_private_ip and is_blocked_hostname are in ssrf.rs.
    // These integration tests verify the redirect validation path delegates correctly.

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
        assert!(
            msg.contains("private/reserved IP"),
            "expected SSRF block message, got: {msg}"
        );
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

    #[tokio::test]
    async fn test_memory_get_rejects_path_traversal() {
        let workspace = std::env::temp_dir().join("localgpt_test_workspace");
        let _ = std::fs::create_dir_all(&workspace);
        let tool = MemoryGetTool::new(workspace);

        // Path with .. should be rejected
        let args = r#"{"path": "memory/../../../etc/passwd"}"#;
        let result = tool.execute(args).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("path traversal"));
    }

    #[tokio::test]
    async fn test_memory_get_rejects_null_bytes() {
        let workspace = std::env::temp_dir().join("localgpt_test_workspace");
        let _ = std::fs::create_dir_all(&workspace);
        let tool = MemoryGetTool::new(workspace);

        let args = r#"{"path": "memory/\u0000evil.md"}"#;
        let result = tool.execute(args).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_memory_get_caps_lines_parameter() {
        let workspace = std::env::temp_dir().join("localgpt_test_mg_lines");
        let _ = std::fs::create_dir_all(workspace.join("memory"));
        // Create a small test file
        std::fs::write(workspace.join("MEMORY.md"), "line1\nline2\nline3\n").unwrap();
        let tool = MemoryGetTool::new(workspace.clone());

        // Even with a huge lines value, it should be capped and work normally
        let args = r#"{"path": "MEMORY.md", "lines": 999999999}"#;
        let result = tool.execute(args).await.unwrap();
        assert!(result.contains("line1"));
        // Cleanup
        let _ = std::fs::remove_dir_all(&workspace);
    }

    // --- DocumentLoadTool tests ---

    fn test_tools_config() -> crate::config::ToolsConfig {
        crate::config::ToolsConfig::default()
    }

    #[test]
    fn test_document_load_tool_schema() {
        let workspace = std::env::temp_dir().join("localgpt_test_doc_schema");
        let tool = DocumentLoadTool::new(workspace, &test_tools_config());
        assert_eq!(tool.name(), "document_load");
        let schema = tool.schema();
        assert_eq!(schema.name, "document_load");
        let params = &schema.parameters;
        assert!(params["properties"]["path"].is_object());
        assert_eq!(params["required"][0], "path");
    }

    #[tokio::test]
    async fn test_document_load_rejects_path_traversal() {
        let workspace = std::env::temp_dir().join("localgpt_test_doc_traversal");
        let _ = std::fs::create_dir_all(&workspace);
        let tool = DocumentLoadTool::new(workspace, &test_tools_config());

        let args = r#"{"path": "../../../etc/passwd"}"#;
        let result = tool.execute(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path traversal"));
    }

    #[tokio::test]
    async fn test_document_load_rejects_unsupported_format() {
        let workspace = std::env::temp_dir().join("localgpt_test_doc_format");
        let _ = std::fs::create_dir_all(&workspace);
        std::fs::write(workspace.join("test.xyz"), "content").unwrap();
        let tool = DocumentLoadTool::new(workspace.clone(), &test_tools_config());

        let args = r#"{"path": "test.xyz"}"#;
        let result = tool.execute(args).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("Unsupported format"));
        assert!(msg.contains("pdf"));
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[tokio::test]
    async fn test_document_load_rejects_too_large() {
        let workspace = std::env::temp_dir().join("localgpt_test_doc_large");
        let _ = std::fs::create_dir_all(&workspace);
        std::fs::write(workspace.join("big.pdf"), vec![0u8; 100]).unwrap();

        let mut config = test_tools_config();
        config.document_max_bytes = 50; // 50 bytes limit
        let tool = DocumentLoadTool::new(workspace.clone(), &config);

        let args = r#"{"path": "big.pdf"}"#;
        let result = tool.execute(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[tokio::test]
    async fn test_document_load_file_not_found() {
        let workspace = std::env::temp_dir().join("localgpt_test_doc_notfound");
        let _ = std::fs::create_dir_all(&workspace);
        let tool = DocumentLoadTool::new(workspace, &test_tools_config());

        let args = r#"{"path": "nonexistent.pdf"}"#;
        let result = tool.execute(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    // --- AudioTranscribeTool tests ---

    #[test]
    fn test_audio_transcribe_tool_schema() {
        let workspace = std::env::temp_dir().join("localgpt_test_audio_schema");
        let registry = Arc::new(crate::media::SttRegistry::new(
            crate::media::SttConfig::default(),
        ));
        let tool = AudioTranscribeTool::new(registry, workspace, None);
        assert_eq!(tool.name(), "transcribe_audio");
        let schema = tool.schema();
        assert_eq!(schema.name, "transcribe_audio");
        let params = &schema.parameters;
        assert!(params["properties"]["path"].is_object());
        assert!(params["properties"]["language"].is_object());
        assert_eq!(params["required"][0], "path");
    }

    #[tokio::test]
    async fn test_audio_transcribe_rejects_path_traversal() {
        let workspace = std::env::temp_dir().join("localgpt_test_audio_traversal");
        let _ = std::fs::create_dir_all(&workspace);
        let registry = Arc::new(crate::media::SttRegistry::new(
            crate::media::SttConfig::default(),
        ));
        let tool = AudioTranscribeTool::new(registry, workspace, None);

        let args = r#"{"path": "../../../etc/passwd.mp3"}"#;
        let result = tool.execute(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path traversal"));
    }

    #[tokio::test]
    async fn test_audio_transcribe_rejects_unsupported_format() {
        let workspace = std::env::temp_dir().join("localgpt_test_audio_format");
        let _ = std::fs::create_dir_all(&workspace);
        std::fs::write(workspace.join("test.txt"), "not audio").unwrap();
        let registry = Arc::new(crate::media::SttRegistry::new(
            crate::media::SttConfig::default(),
        ));
        let tool = AudioTranscribeTool::new(registry, workspace.clone(), None);

        let args = r#"{"path": "test.txt"}"#;
        let result = tool.execute(args).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unsupported audio")
        );
        let _ = std::fs::remove_dir_all(&workspace);
    }

    #[tokio::test]
    async fn test_audio_transcribe_file_not_found() {
        let workspace = std::env::temp_dir().join("localgpt_test_audio_notfound");
        let _ = std::fs::create_dir_all(&workspace);
        let registry = Arc::new(crate::media::SttRegistry::new(
            crate::media::SttConfig::default(),
        ));
        let tool = AudioTranscribeTool::new(registry, workspace, None);

        let args = r#"{"path": "nonexistent.mp3"}"#;
        let result = tool.execute(args).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }
}
