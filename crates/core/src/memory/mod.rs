pub mod active_recall;
pub mod backend;
mod backend_markdown;
mod backend_none;
mod backend_sqlite;
mod embeddings;
mod index;
pub mod query_expansion;
mod search;
pub mod session_index;
mod watcher;
mod workspace;

pub use backend::{MemoryBackend, MemoryBackendKind};
pub use backend_markdown::MarkdownBackend;
pub use backend_none::NoneBackend;
pub use backend_sqlite::SqliteBackend;
#[cfg(feature = "embeddings-local")]
pub use embeddings::FastEmbedProvider;
#[cfg(feature = "gguf")]
pub use embeddings::LlamaCppProvider;
pub use embeddings::{
    EmbeddingProvider, GeminiEmbeddingProvider, OpenAIEmbeddingProvider, hash_text,
};
pub use index::{MemoryIndex, ReindexStats};
pub use query_expansion::{EXPAND_PROMPT, ExpandedQuery, expand_query_local, parse_llm_keywords};
pub use search::MemoryChunk;
pub use watcher::MemoryWatcher;
pub use workspace::{init_state_dir, init_workspace};

use anyhow::Result;
use chrono::Local;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use tracing::{debug, info, warn};

use crate::config::{Config, MemoryConfig};

#[derive(Clone)]
pub struct MemoryManager {
    workspace: PathBuf,
    db_path: PathBuf,
    backend: Arc<dyn MemoryBackend>,
    config: MemoryConfig,
    /// Optional embedding provider for semantic search
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// True if this was a brand new workspace (first run)
    is_brand_new: bool,
}

/// Aggregate statistics about the memory index for a workspace.
#[derive(Debug)]
pub struct MemoryStats {
    /// Absolute path of the indexed workspace.
    pub workspace: String,
    /// Number of distinct files in the index.
    pub total_files: usize,
    /// Total number of text chunks across all files.
    pub total_chunks: usize,
    /// On-disk size of the index database in kilobytes.
    pub index_size_kb: u64,
    /// Per-file breakdown of indexing statistics.
    pub files: Vec<FileStats>,
}

/// Per-file statistics within the memory index.
#[derive(Debug)]
pub struct FileStats {
    /// File name (relative to workspace root).
    pub name: String,
    /// Number of indexed chunks for this file.
    pub chunks: usize,
    /// Total line count of the source file.
    pub lines: usize,
}

/// A recently modified memory entry, used for the `memory recent` display.
#[derive(Debug)]
pub struct RecentEntry {
    /// ISO-8601 timestamp of the last modification.
    pub timestamp: String,
    /// File path (relative to workspace root).
    pub file: String,
    /// Short text preview of the entry content.
    pub preview: String,
}

impl MemoryManager {
    /// Create a new MemoryManager with the default agent ID ("main")
    pub fn new(config: &MemoryConfig) -> Result<Self> {
        Self::new_with_agent(config, "main")
    }

    /// Create a new MemoryManager for a specific agent ID (OpenClaw-compatible)
    pub fn new_with_agent(config: &MemoryConfig, agent_id: &str) -> Result<Self> {
        Self::new_with_full_config(config, None, agent_id)
    }

    /// Create a new MemoryManager with full config (for OpenAI embedding provider)
    pub fn new_with_full_config(
        memory_config: &MemoryConfig,
        app_config: Option<&Config>,
        agent_id: &str,
    ) -> Result<Self> {
        // Use paths from app_config if available, otherwise resolve fresh
        let paths = if let Some(config) = app_config {
            config.paths.clone()
        } else {
            crate::paths::Paths::resolve()?
        };

        let workspace = paths.workspace.clone();

        // Initialize workspace with templates if needed, returns true if brand new
        let is_brand_new = init_workspace(&workspace, &paths)?;

        // Database goes in cache_dir/memory/{agentId}.sqlite (XDG cache — regenerable)
        let db_path = paths.search_index(agent_id);
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Create the search backend based on config
        let backend: Arc<dyn MemoryBackend> = match memory_config.backend {
            MemoryBackendKind::Sqlite => {
                let index = MemoryIndex::new_with_db_path(&workspace, &db_path)?
                    .with_chunk_config(memory_config.chunk_size, memory_config.chunk_overlap);
                Arc::new(SqliteBackend::new(index))
            }
            MemoryBackendKind::Markdown => Arc::new(MarkdownBackend::new(workspace.clone())),
            MemoryBackendKind::None => Arc::new(NoneBackend::new()),
        };

        // Create embedding provider based on config
        let embedding_provider: Option<Arc<dyn EmbeddingProvider>> = match memory_config
            .embedding_provider
            .as_str()
        {
            "local" => {
                #[cfg(feature = "embeddings-local")]
                {
                    let model_name = if memory_config.embedding_model.is_empty()
                        || memory_config.embedding_model == "text-embedding-3-small"
                    {
                        None // Use default local model
                    } else {
                        Some(memory_config.embedding_model.as_str())
                    };
                    let cache_dir = if memory_config.embedding_cache_dir.is_empty() {
                        None
                    } else {
                        Some(memory_config.embedding_cache_dir.as_str())
                    };
                    match FastEmbedProvider::new_with_cache_dir(model_name, cache_dir) {
                        Ok(provider) => {
                            info!("Using local embedding provider: {}", provider.model());
                            Some(Arc::new(provider))
                        }
                        Err(e) => {
                            warn!(
                                "Failed to initialize local embeddings: {}. Falling back to FTS-only search.",
                                e
                            );
                            None
                        }
                    }
                }
                #[cfg(not(feature = "embeddings-local"))]
                {
                    warn!(
                        "Local embeddings requested but `embeddings-local` feature is disabled. Falling back to FTS-only search."
                    );
                    None
                }
            }
            "openai" => {
                // Need OpenAI config for API key
                if let Some(config) = app_config {
                    if let Some(ref openai) = config.providers.openai {
                        match OpenAIEmbeddingProvider::new(
                            &openai.api_key,
                            &openai.base_url,
                            &memory_config.embedding_model,
                        ) {
                            Ok(provider) => {
                                info!("Using OpenAI embedding provider: {}", provider.model());
                                Some(Arc::new(provider))
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to initialize OpenAI embeddings: {}. Falling back to FTS-only search.",
                                    e
                                );
                                None
                            }
                        }
                    } else {
                        warn!(
                            "OpenAI embedding provider requested but no OpenAI config found. Falling back to FTS-only search."
                        );
                        None
                    }
                } else {
                    warn!(
                        "OpenAI embedding provider requested but no app config provided. Falling back to FTS-only search."
                    );
                    None
                }
            }
            "gemini" => {
                let api_key = memory_config.gemini_api_key.as_deref().or_else(|| {
                    app_config
                        .and_then(|c| c.providers.gemini.as_ref())
                        .map(|g| g.api_key.as_str())
                });

                if let Some(key) = api_key {
                    let model = if memory_config.embedding_model.is_empty()
                        || memory_config.embedding_model == "text-embedding-3-small"
                    {
                        None
                    } else {
                        Some(memory_config.embedding_model.as_str())
                    };
                    let provider = GeminiEmbeddingProvider::new(key, model);
                    info!("Using Gemini embedding provider: {}", provider.model());
                    Some(Arc::new(provider))
                } else {
                    warn!(
                        "Gemini embedding provider requested but no API key configured. \
                         Set memory.gemini_api_key or providers.gemini.api_key. Falling back to FTS-only search."
                    );
                    None
                }
            }
            #[cfg(feature = "gguf")]
            "gguf" => {
                let cache_dir = if memory_config.embedding_cache_dir.is_empty() {
                    None
                } else {
                    Some(memory_config.embedding_cache_dir.as_str())
                };
                match LlamaCppProvider::new(&memory_config.embedding_model, cache_dir) {
                    Ok(provider) => {
                        info!("Using GGUF embedding provider: {}", provider.model());
                        Some(Arc::new(provider))
                    }
                    Err(e) => {
                        warn!(
                            "Failed to initialize GGUF embeddings: {}. Falling back to FTS-only search.",
                            e
                        );
                        None
                    }
                }
            }
            #[cfg(not(feature = "gguf"))]
            "gguf" => {
                warn!(
                    "GGUF embedding provider requested but 'gguf' feature is not enabled. Build with --features gguf. Falling back to FTS-only search."
                );
                None
            }
            "none" => {
                debug!("Embeddings disabled, using FTS-only search");
                None
            }
            other => {
                warn!(
                    "Unknown embedding provider '{}'. Falling back to FTS-only search.",
                    other
                );
                None
            }
        };

        Ok(Self {
            workspace,
            db_path,
            backend,
            config: memory_config.clone(),
            embedding_provider,
            is_brand_new,
        })
    }

    /// Set embedding provider for semantic search (requires OpenAI API key)
    pub fn with_embedding_provider(mut self, provider: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedding_provider = Some(provider);
        self
    }

    /// Check if semantic search is available
    pub fn has_embeddings(&self) -> bool {
        self.embedding_provider.is_some()
    }

    pub fn workspace(&self) -> &PathBuf {
        &self.workspace
    }

    /// Get a reference to the active memory backend.
    pub fn backend(&self) -> &dyn MemoryBackend {
        self.backend.as_ref()
    }

    /// Read the main MEMORY.md file
    pub fn read_memory_file(&self) -> Result<String> {
        let path = self.workspace.join("MEMORY.md");
        if path.exists() {
            Ok(fs::read_to_string(&path)?)
        } else {
            Ok(String::new())
        }
    }

    /// Read the HEARTBEAT.md file
    pub fn read_heartbeat_file(&self) -> Result<String> {
        let path = self.workspace.join("HEARTBEAT.md");
        if path.exists() {
            Ok(fs::read_to_string(&path)?)
        } else {
            Ok(String::new())
        }
    }

    /// Read the SOUL.md file (persona/tone guidance)
    pub fn read_soul_file(&self) -> Result<String> {
        let path = self.workspace.join("SOUL.md");
        if path.exists() {
            Ok(fs::read_to_string(&path)?)
        } else {
            Ok(String::new())
        }
    }

    /// Read the USER.md file (OpenClaw-compatible: user info)
    pub fn read_user_file(&self) -> Result<String> {
        let path = self.workspace.join("USER.md");
        if path.exists() {
            Ok(fs::read_to_string(&path)?)
        } else {
            Ok(String::new())
        }
    }

    /// Read the IDENTITY.md file (OpenClaw-compatible: agent identity context)
    pub fn read_identity_file(&self) -> Result<String> {
        let path = self.workspace.join("IDENTITY.md");
        if path.exists() {
            Ok(fs::read_to_string(&path)?)
        } else {
            Ok(String::new())
        }
    }

    /// Read the AGENTS.md file (OpenClaw-compatible: list of agents)
    pub fn read_agents_file(&self) -> Result<String> {
        let path = self.workspace.join("AGENTS.md");
        if path.exists() {
            Ok(fs::read_to_string(&path)?)
        } else {
            Ok(String::new())
        }
    }

    /// Check if this is a brand new workspace (first run)
    pub fn is_brand_new(&self) -> bool {
        self.is_brand_new
    }

    /// Read the TOOLS.md file (OpenClaw-compatible: local tool notes)
    pub fn read_tools_file(&self) -> Result<String> {
        let path = self.workspace.join("TOOLS.md");
        if path.exists() {
            Ok(fs::read_to_string(&path)?)
        } else {
            Ok(String::new())
        }
    }

    /// Read recent daily log files
    pub fn read_recent_daily_logs(&self, days: usize) -> Result<String> {
        let memory_dir = self.workspace.join("memory");
        if !memory_dir.exists() {
            return Ok(String::new());
        }

        let today = Local::now().date_naive();
        let mut content = String::new();

        for i in 0..days {
            let date = today - chrono::Duration::days(i as i64);
            let filename = format!("{}.md", date.format("%Y-%m-%d"));
            let path = memory_dir.join(&filename);

            if path.exists()
                && let Ok(file_content) = fs::read_to_string(&path)
            {
                if !content.is_empty() {
                    content.push_str("\n---\n\n");
                }
                content.push_str(&format!("## {}\n\n", filename));
                content.push_str(&file_content);
            }
        }

        Ok(content)
    }

    /// Search memory using hybrid search (FTS + semantic if available)
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryChunk>> {
        let mut results = self.search_raw(query, limit)?;

        // Apply temporal decay if configured
        if self.config.temporal_decay_lambda > 0.0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            for chunk in &mut results {
                chunk.apply_temporal_decay(self.config.temporal_decay_lambda, now);
            }
            // Re-sort after decay
            results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        Ok(results)
    }

    /// Search memory without temporal decay (internal use)
    fn search_raw(&self, query: &str, limit: usize) -> Result<Vec<MemoryChunk>> {
        // Expand query for better FTS5 keyword matching
        let expanded = query_expansion::expand_query_local(query);
        let fts_query = &expanded.fts_query;
        debug!(
            "Query expanded: {:?} -> {} keywords",
            query,
            expanded.keywords.len()
        );

        // If we have an embedding provider, try hybrid search
        if let Some(ref provider) = self.embedding_provider {
            // Try to get query embedding (may fail if no API key, rate limited, etc.)
            if let Ok(handle) = Handle::try_current() {
                let provider = provider.clone();
                let query_string = query.to_string();
                let model = provider.model().to_string();

                // Run embedding in blocking context
                let embedding_result = std::thread::spawn(move || {
                    handle.block_on(async { provider.embed(&query_string).await })
                })
                .join()
                .map_err(|_| anyhow::anyhow!("Thread panicked"))?;

                if let Ok(embedding) = embedding_result {
                    debug!("Using hybrid search with {} dimensions", embedding.len());
                    // Use expanded query for FTS component, original for embeddings
                    return self.backend.search_hybrid(
                        fts_query,
                        Some(&embedding),
                        &model,
                        limit,
                        0.3, // FTS weight
                        0.7, // Vector weight
                    );
                }
            }
        }

        // Fallback to FTS-only search with expanded query
        self.backend.search_fts_raw(fts_query, limit)
    }

    /// Search memory using FTS only (faster, no API calls)
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<MemoryChunk>> {
        self.backend.search(query, limit)
    }

    /// Get total chunk count
    pub fn chunk_count(&self) -> Result<usize> {
        self.backend.chunk_count()
    }

    /// Reindex all memory files
    pub fn reindex(&self, force: bool) -> Result<ReindexStats> {
        let start = std::time::Instant::now();
        let mut stats = ReindexStats {
            files_processed: 0,
            files_updated: 0,
            chunks_indexed: 0,
            duration: Duration::default(),
        };

        // First, clean up deleted files from the index
        let files_removed = self.cleanup_deleted_files()?;
        if files_removed > 0 {
            info!("Removed {} deleted files from index", files_removed);
        }

        // Index all .md files recursively under workspace
        let pattern = format!("{}/**/*.md", self.workspace.display());
        for entry in glob::glob(&pattern)
            .into_iter()
            .flatten()
            .filter_map(|r| r.ok())
        {
            if entry.is_file() {
                stats.files_processed += 1;
                if self.backend.index_file(&entry, force)? {
                    stats.files_updated += 1;
                }
            }
        }

        // Index configured external paths (outside workspace)
        for index_path in &self.config.paths {
            let base_path = if index_path.path.starts_with('~') || index_path.path.starts_with('/')
            {
                PathBuf::from(shellexpand::tilde(&index_path.path).to_string())
            } else {
                self.workspace.join(&index_path.path)
            };

            // Skip paths inside workspace (already covered by recursive glob above)
            if base_path.starts_with(&self.workspace) {
                continue;
            }

            if !base_path.exists() {
                debug!("Skipping non-existent index path: {}", base_path.display());
                continue;
            }

            let pattern = format!("{}/{}", base_path.display(), index_path.pattern);
            debug!("Indexing external path with pattern: {}", pattern);

            for entry in glob::glob(&pattern)
                .into_iter()
                .flatten()
                .filter_map(|r| r.ok())
            {
                if entry.is_file() {
                    stats.files_processed += 1;
                    if self.backend.index_file(&entry, force)? {
                        stats.files_updated += 1;
                    }
                }
            }
        }

        stats.chunks_indexed = self.backend.chunk_count()?;
        stats.duration = start.elapsed();

        info!("Reindex complete: {:?}", stats);
        Ok(stats)
    }

    /// Remove files from index that no longer exist on disk
    fn cleanup_deleted_files(&self) -> Result<usize> {
        let indexed_files = self.backend.indexed_files()?;
        let mut removed = 0;

        for relative_path in indexed_files {
            let full_path = self.workspace.join(&relative_path);
            if !full_path.exists() {
                debug!("Cleaning up deleted file: {}", relative_path);
                self.backend.remove_file(&relative_path)?;
                removed += 1;
            }
        }

        Ok(removed)
    }

    /// Get memory statistics
    pub fn stats(&self) -> Result<MemoryStats> {
        let mut files = Vec::new();
        let mut total_chunks = 0;

        // Get stats for all .md files recursively under workspace
        let pattern = format!("{}/**/*.md", self.workspace.display());
        for entry in glob::glob(&pattern)
            .into_iter()
            .flatten()
            .filter_map(|r| r.ok())
        {
            if entry.is_file() {
                let content = fs::read_to_string(&entry)?;
                let lines = content.lines().count();
                let chunks = self.backend.file_chunk_count(&entry)?;
                total_chunks += chunks;

                let display_name = entry
                    .strip_prefix(&self.workspace)
                    .map(|rel| rel.display().to_string())
                    .unwrap_or_else(|_| entry.display().to_string());

                files.push(FileStats {
                    name: display_name,
                    chunks,
                    lines,
                });
            }
        }

        // Configured external paths (outside workspace)
        for index_path in &self.config.paths {
            let base_path = if index_path.path.starts_with('~') || index_path.path.starts_with('/')
            {
                PathBuf::from(shellexpand::tilde(&index_path.path).to_string())
            } else {
                self.workspace.join(&index_path.path)
            };

            // Skip paths inside workspace (already covered above)
            if base_path.starts_with(&self.workspace) {
                continue;
            }

            if !base_path.exists() {
                continue;
            }

            let pattern = format!("{}/{}", base_path.display(), index_path.pattern);

            for entry in glob::glob(&pattern)
                .into_iter()
                .flatten()
                .filter_map(|r| r.ok())
            {
                if entry.is_file() {
                    let content = fs::read_to_string(&entry)?;
                    let lines = content.lines().count();
                    let chunks = self.backend.file_chunk_count(&entry)?;
                    total_chunks += chunks;

                    let display_name = if let Ok(rel) = entry.strip_prefix(&base_path) {
                        format!("{}/{}", index_path.path, rel.display())
                    } else {
                        entry.display().to_string()
                    };

                    files.push(FileStats {
                        name: display_name,
                        chunks,
                        lines,
                    });
                }
            }
        }

        let index_size = self.backend.size_bytes()? / 1024;

        Ok(MemoryStats {
            workspace: self.workspace.display().to_string(),
            total_files: files.len(),
            total_chunks,
            index_size_kb: index_size,
            files,
        })
    }

    /// Get recent memory entries
    pub fn recent_entries(&self, count: usize) -> Result<Vec<RecentEntry>> {
        let mut entries = Vec::new();

        let memory_dir = self.workspace.join("memory");
        if !memory_dir.exists() {
            return Ok(entries);
        }

        // Get all daily log files sorted by date (newest first)
        let mut files: Vec<_> = fs::read_dir(&memory_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map(|e| e == "md").unwrap_or(false))
            .collect();

        files.sort_by_key(|f| std::cmp::Reverse(f.file_name()));

        for entry in files.into_iter().take(count) {
            let path = entry.path();
            let filename = path.file_name().unwrap().to_string_lossy().to_string();

            if let Ok(content) = fs::read_to_string(&path) {
                // Get last non-empty line as preview
                let preview = content
                    .lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("")
                    .chars()
                    .take(100)
                    .collect();

                entries.push(RecentEntry {
                    timestamp: filename.replace(".md", ""),
                    file: format!("memory/{}", filename),
                    preview,
                });
            }
        }

        Ok(entries)
    }

    /// Start file watcher for automatic reindexing.
    ///
    /// The file watcher is only supported with the SQLite backend because
    /// it creates its own `MemoryIndex` connection for the background thread.
    pub fn start_watcher(&self) -> Result<MemoryWatcher> {
        if self.backend.kind() != MemoryBackendKind::Sqlite {
            return Err(anyhow::anyhow!(
                "File watcher is only supported with the SQLite backend (current: {})",
                self.backend.kind()
            ));
        }
        MemoryWatcher::new(
            self.workspace.clone(),
            self.db_path.clone(),
            self.config.clone(),
        )
    }

    /// Generate embeddings for chunks that don't have them
    /// Returns (chunks_processed, chunks_embedded)
    /// Uses embedding cache to avoid regenerating identical content
    pub async fn generate_embeddings(&self, batch_size: usize) -> Result<(usize, usize)> {
        let provider = match &self.embedding_provider {
            Some(p) => p,
            None => {
                debug!("No embedding provider configured, skipping embedding generation");
                return Ok((0, 0));
            }
        };

        let provider_id = provider.id().to_string();
        let model = provider.model().to_string();
        let mut total_processed = 0;
        let mut total_embedded = 0;
        let mut cache_hits = 0;

        loop {
            // Get chunks without embeddings
            let chunks = self.backend.chunks_without_embeddings(batch_size)?;
            if chunks.is_empty() {
                break;
            }

            total_processed += chunks.len();

            // Separate chunks into cached and uncached
            let mut to_embed: Vec<(String, String, String)> = Vec::new(); // (id, text, hash)
            let mut from_cache: Vec<(String, Vec<f32>)> = Vec::new(); // (id, embedding)

            for (chunk_id, text) in &chunks {
                let text_hash = hash_text(text);

                // Check cache first
                if let Ok(Some(cached)) =
                    self.backend
                        .get_cached_embedding(&provider_id, &model, &text_hash)
                {
                    from_cache.push((chunk_id.clone(), cached));
                    cache_hits += 1;
                } else {
                    to_embed.push((chunk_id.clone(), text.clone(), text_hash));
                }
            }

            // Store cached embeddings
            for (chunk_id, embedding) in from_cache {
                if let Err(e) = self.backend.store_embedding(&chunk_id, &embedding, &model) {
                    warn!(
                        "Failed to store cached embedding for chunk {}: {}",
                        chunk_id, e
                    );
                } else {
                    total_embedded += 1;
                }
            }

            // Generate new embeddings for uncached chunks
            if !to_embed.is_empty() {
                let texts: Vec<String> = to_embed.iter().map(|(_, text, _)| text.clone()).collect();

                match provider.embed_batch(&texts).await {
                    Ok(embeddings) => {
                        for ((chunk_id, _text, text_hash), embedding) in
                            to_embed.iter().zip(embeddings.iter())
                        {
                            // Store in chunk
                            if let Err(e) =
                                self.backend.store_embedding(chunk_id, embedding, &model)
                            {
                                warn!("Failed to store embedding for chunk {}: {}", chunk_id, e);
                            } else {
                                total_embedded += 1;
                            }

                            // Store in cache for future reuse
                            if let Err(e) = self.backend.cache_embedding(
                                &provider_id,
                                &model,
                                "", // provider_key (API key identifier, can be empty)
                                text_hash,
                                embedding,
                            ) {
                                debug!("Failed to cache embedding: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to generate embeddings: {}", e);
                        break;
                    }
                }
            }

            debug!(
                "Generated embeddings: {}/{} chunks ({} from cache)",
                total_embedded, total_processed, cache_hits
            );

            // Break if we processed fewer than batch_size (last batch)
            if chunks.len() < batch_size {
                break;
            }
        }

        info!(
            "Embedding generation complete: {} chunks, {} embedded, {} cache hits",
            total_processed, total_embedded, cache_hits
        );

        Ok((total_processed, total_embedded))
    }

    /// Get count of chunks with embeddings
    pub fn embedded_chunk_count(&self) -> Result<usize> {
        let model = self
            .embedding_provider
            .as_ref()
            .map(|p| p.model().to_string())
            .unwrap_or_default();
        self.backend.embedded_chunk_count(&model)
    }
}
