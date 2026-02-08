use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::embeddings::{cosine_similarity, deserialize_embedding, serialize_embedding};
use super::search::MemoryChunk;

#[derive(Clone)]
pub struct MemoryIndex {
    conn: Arc<Mutex<Connection>>,
    workspace: PathBuf,
    db_path: PathBuf,
    /// Whether sqlite-vec extension is loaded for fast vector search
    has_vec_extension: bool,
    /// Token count per chunk (default: 400)
    chunk_size: usize,
    /// Token overlap between chunks (default: 80)
    chunk_overlap: usize,
}

#[derive(Debug)]
pub struct ReindexStats {
    pub files_processed: usize,
    pub files_updated: usize,
    pub chunks_indexed: usize,
    pub duration: Duration,
}

impl MemoryIndex {
    /// Create a new memory index with database at the specified path
    pub fn new_with_db_path(workspace: &Path, db_path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(db_path)?;

        // Check if we need to migrate from old schema
        let needs_migration = Self::needs_schema_migration(&conn)?;
        if needs_migration {
            info!("Migrating database schema to OpenClaw-compatible format");
            Self::migrate_to_openclaw_schema(&conn)?;
        }

        // Initialize OpenClaw-compatible schema
        conn.execute_batch(
            r#"
            -- Metadata key/value store
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- File tracking (OpenClaw-compatible)
            CREATE TABLE IF NOT EXISTS files (
                path TEXT PRIMARY KEY,
                source TEXT NOT NULL DEFAULT 'memory',
                hash TEXT NOT NULL,
                mtime INTEGER NOT NULL,
                size INTEGER NOT NULL
            );

            -- Chunked content (OpenClaw-compatible)
            CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'memory',
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                hash TEXT NOT NULL,
                model TEXT NOT NULL DEFAULT '',
                text TEXT NOT NULL,
                embedding TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL
            );

            -- Embedding cache (OpenClaw-compatible)
            CREATE TABLE IF NOT EXISTS embedding_cache (
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                provider_key TEXT NOT NULL,
                hash TEXT NOT NULL,
                embedding TEXT NOT NULL,
                dims INTEGER,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (provider, model, provider_key, hash)
            );

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_chunks_path ON chunks(path);
            CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(source);
            CREATE INDEX IF NOT EXISTS idx_embedding_cache_updated_at ON embedding_cache(updated_at);
            "#,
        )?;

        // Create FTS5 table (OpenClaw-compatible with UNINDEXED columns)
        Self::ensure_fts_table(&conn)?;

        // Ensure source column exists on older tables
        Self::ensure_column(&conn, "files", "source", "TEXT NOT NULL DEFAULT 'memory'")?;
        Self::ensure_column(&conn, "chunks", "source", "TEXT NOT NULL DEFAULT 'memory'")?;

        // Try to load sqlite-vec extension for fast vector search
        let has_vec_extension = Self::try_load_sqlite_vec(&conn);
        if has_vec_extension {
            debug!("sqlite-vec extension loaded successfully");
            Self::ensure_vec_table(&conn)?;
        } else {
            debug!("sqlite-vec extension not available, using in-memory vector search");
        }

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            workspace: workspace.to_path_buf(),
            db_path: db_path.to_path_buf(),
            has_vec_extension,
            chunk_size: 400,
            chunk_overlap: 80,
        })
    }

    /// Set chunk size and overlap (builder pattern)
    pub fn with_chunk_config(mut self, chunk_size: usize, chunk_overlap: usize) -> Self {
        self.chunk_size = chunk_size;
        self.chunk_overlap = chunk_overlap;
        self
    }

    /// Try to load sqlite-vec extension
    #[allow(unsafe_code)]
    fn try_load_sqlite_vec(conn: &Connection) -> bool {
        // sqlite-vec provides the extension as a loadable module
        // Try to load it - this requires the extension to be installed on the system

        // SAFETY: load_extension is unsafe because it loads arbitrary native code
        // We only load from known, trusted paths (sqlite-vec)
        if unsafe { conn.load_extension_enable() }.is_err() {
            return false;
        }

        // Try loading from common locations
        let ext_paths = [
            "vec0", // If in LD_LIBRARY_PATH
            "./vec0",
            "/usr/local/lib/vec0",
            "/usr/lib/vec0",
        ];

        for path in ext_paths {
            // SAFETY: Loading sqlite-vec extension from trusted path
            if unsafe { conn.load_extension(path, None) }.is_ok() {
                let _ = conn.load_extension_disable();
                return true;
            }
        }

        let _ = conn.load_extension_disable();
        false
    }

    /// Create virtual table for vector search (requires sqlite-vec)
    fn ensure_vec_table(conn: &Connection) -> Result<()> {
        // Create chunks_vec virtual table if sqlite-vec is available
        let result = conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(id TEXT PRIMARY KEY, embedding float[1536])",
            [],
        );
        match result {
            Ok(_) => debug!("chunks_vec table created/verified"),
            Err(e) => debug!("chunks_vec table creation skipped: {}", e),
        }
        Ok(())
    }

    /// Create a new memory index with database in workspace (legacy path)
    pub fn new(workspace: &Path) -> Result<Self> {
        let db_path = workspace.join("memory.sqlite");
        Self::new_with_db_path(workspace, &db_path)
    }

    /// Index a file, returning true if it was updated
    pub fn index_file(&self, path: &Path, force: bool) -> Result<bool> {
        let content = fs::read_to_string(path)?;
        let file_hash = hash_content(&content);
        let metadata = fs::metadata(path)?;
        let mtime = metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;
        let size = metadata.len() as i64;

        let relative_path = path
            .strip_prefix(&self.workspace)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        // Check if file has changed
        if !force {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT hash FROM files WHERE path = ?1",
                    params![&relative_path],
                    |row| row.get(0),
                )
                .ok();

            if existing.as_deref() == Some(&file_hash) {
                debug!("File unchanged, skipping: {}", relative_path);
                return Ok(false);
            }
        }

        debug!("Indexing file: {}", relative_path);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        // Update file record (OpenClaw-compatible columns)
        conn.execute(
            "INSERT OR REPLACE INTO files (path, source, hash, mtime, size) VALUES (?1, 'memory', ?2, ?3, ?4)",
            params![&relative_path, &file_hash, mtime, size],
        )?;

        // Delete existing chunks and their FTS entries
        Self::delete_chunks_for_path(&conn, &relative_path)?;

        // Create new chunks (OpenClaw-compatible)
        let chunks = chunk_text(&content, self.chunk_size, self.chunk_overlap);

        for chunk in chunks.iter() {
            let chunk_id = Uuid::new_v4().to_string();
            let chunk_hash = hash_content(&chunk.content);

            conn.execute(
                r#"INSERT INTO chunks (id, path, source, start_line, end_line, hash, model, text, embedding, updated_at)
                   VALUES (?1, ?2, 'memory', ?3, ?4, ?5, '', ?6, '', ?7)"#,
                params![&chunk_id, &relative_path, chunk.line_start, chunk.line_end, &chunk_hash, &chunk.content, now],
            )?;

            // Insert into FTS
            Self::insert_fts(
                &conn,
                &chunk_id,
                &relative_path,
                "memory",
                "",
                chunk.line_start,
                chunk.line_end,
                &chunk.content,
            )?;
        }

        Ok(true)
    }

    /// Delete chunks for a path and their FTS entries
    fn delete_chunks_for_path(conn: &Connection, path: &str) -> Result<()> {
        // Delete from FTS first (get chunk IDs)
        let mut stmt = conn.prepare("SELECT id FROM chunks WHERE path = ?1")?;
        let chunk_ids: Vec<String> = stmt
            .query_map(params![path], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        for chunk_id in chunk_ids {
            let _ = conn.execute("DELETE FROM chunks_fts WHERE id = ?1", params![&chunk_id]);
        }

        // Delete chunks
        conn.execute("DELETE FROM chunks WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// Remove a file and its chunks from the index (for deleted files)
    pub fn remove_file(&self, relative_path: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        Self::delete_chunks_for_path(&conn, relative_path)?;
        conn.execute("DELETE FROM files WHERE path = ?1", params![relative_path])?;

        debug!("Removed deleted file from index: {}", relative_path);
        Ok(())
    }

    /// Get all indexed file paths
    pub fn indexed_files(&self) -> Result<Vec<String>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let mut stmt = conn.prepare("SELECT path FROM files")?;
        let rows = stmt.query_map([], |row| row.get(0))?;

        let mut paths = Vec::new();
        for row in rows {
            paths.push(row?);
        }
        Ok(paths)
    }

    /// Insert into FTS table
    #[allow(clippy::too_many_arguments)]
    fn insert_fts(
        conn: &Connection,
        id: &str,
        path: &str,
        source: &str,
        model: &str,
        start_line: i32,
        end_line: i32,
        text: &str,
    ) -> Result<()> {
        let _ = conn.execute(
            "INSERT INTO chunks_fts (text, id, path, source, model, start_line, end_line) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![text, id, path, source, model, start_line, end_line],
        );
        Ok(())
    }

    /// Search using FTS5
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryChunk>> {
        let fts_query = match build_fts_query(query) {
            Some(q) => q,
            None => return Ok(Vec::new()),
        };

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        // OpenClaw-compatible: use 'path', 'start_line', 'end_line', 'text' columns
        let mut stmt = conn.prepare(
            r#"
            SELECT fts.path, fts.start_line, fts.end_line, fts.text, bm25(chunks_fts) as score
            FROM chunks_fts fts
            WHERE chunks_fts MATCH ?1
            ORDER BY score
            LIMIT ?2
            "#,
        )?;

        let rows = stmt.query_map(params![&fts_query, limit as i64], |row| {
            Ok(MemoryChunk {
                file: row.get(0)?,
                line_start: row.get(1)?,
                line_end: row.get(2)?,
                content: row.get(3)?,
                score: row.get::<_, f64>(4)?.abs(), // BM25 returns negative scores
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }

    /// Get total chunk count
    pub fn chunk_count(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Get chunk count for a specific file
    pub fn file_chunk_count(&self, path: &Path) -> Result<usize> {
        let relative_path = path
            .strip_prefix(&self.workspace)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chunks WHERE path = ?1",
            params![&relative_path],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Get database size in bytes
    pub fn size_bytes(&self) -> Result<u64> {
        if self.db_path.exists() {
            Ok(fs::metadata(&self.db_path)?.len())
        } else {
            Ok(0)
        }
    }

    /// Get the database path
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Check if we need to migrate from old LocalGPT schema to OpenClaw schema
    fn needs_schema_migration(conn: &Connection) -> Result<bool> {
        // Check for old schema indicators:
        // 1. chunks table has 'file_path' column instead of 'path'
        // 2. chunks table has 'content' column instead of 'text'
        // 3. chunks.id is INTEGER instead of TEXT
        let result: rusqlite::Result<String> =
            conn.query_row("PRAGMA table_info(chunks)", [], |row| row.get(1));

        if result.is_err() {
            // chunks table doesn't exist yet, no migration needed
            return Ok(false);
        }

        // Check column names
        let has_file_path: bool = conn.prepare("SELECT file_path FROM chunks LIMIT 0").is_ok();
        let has_content: bool = conn.prepare("SELECT content FROM chunks LIMIT 0").is_ok();

        // Old schema has file_path and content columns
        Ok(has_file_path || has_content)
    }

    /// Migrate from old LocalGPT schema to OpenClaw-compatible schema
    fn migrate_to_openclaw_schema(conn: &Connection) -> Result<()> {
        // Start transaction
        conn.execute("BEGIN TRANSACTION", [])?;

        // 1. Rename old tables
        let _ = conn.execute("ALTER TABLE chunks RENAME TO chunks_old", []);
        let _ = conn.execute("ALTER TABLE files RENAME TO files_old", []);

        // 2. Drop old FTS and triggers
        let _ = conn.execute("DROP TABLE IF EXISTS chunks_fts", []);
        let _ = conn.execute("DROP TRIGGER IF EXISTS chunks_ai", []);
        let _ = conn.execute("DROP TRIGGER IF EXISTS chunks_ad", []);
        let _ = conn.execute("DROP TRIGGER IF EXISTS chunks_au", []);

        // 3. Create new tables with OpenClaw schema
        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS files (
                path TEXT PRIMARY KEY,
                source TEXT NOT NULL DEFAULT 'memory',
                hash TEXT NOT NULL,
                mtime INTEGER NOT NULL,
                size INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        conn.execute(
            r#"
            CREATE TABLE IF NOT EXISTS chunks (
                id TEXT PRIMARY KEY,
                path TEXT NOT NULL,
                source TEXT NOT NULL DEFAULT 'memory',
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                hash TEXT NOT NULL,
                model TEXT NOT NULL DEFAULT '',
                text TEXT NOT NULL,
                embedding TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL
            )
            "#,
            [],
        )?;

        // 4. Migrate data from old tables
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        // Migrate files
        let _ = conn.execute(
            r#"
            INSERT INTO files (path, source, hash, mtime, size)
            SELECT path, 'memory', hash, mtime, size FROM files_old
            "#,
            [],
        );

        // Migrate chunks - generate new TEXT UUIDs for each row
        // Check if old schema has embedding columns
        let has_embedding_cols = conn
            .prepare("SELECT embedding FROM chunks_old LIMIT 0")
            .is_ok();

        // Read old data and insert with new UUIDs
        if has_embedding_cols {
            // Old schema has embedding columns
            let mut stmt = conn.prepare(
                "SELECT file_path, line_start, line_end, content, embedding, embedding_model FROM chunks_old",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })?;

            for row in rows {
                let (file_path, line_start, line_end, content, embedding, model) = row?;
                let new_id = Uuid::new_v4().to_string();
                let hash = hash_content(&content);
                let model = model.unwrap_or_default();
                let embedding = embedding.unwrap_or_default();

                conn.execute(
                    r#"
                    INSERT INTO chunks (id, path, source, start_line, end_line, hash, model, text, embedding, updated_at)
                    VALUES (?1, ?2, 'memory', ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                    "#,
                    params![&new_id, &file_path, line_start, line_end, &hash, &model, &content, &embedding, now],
                )?;
            }
        } else {
            // Old schema without embedding columns
            let mut stmt =
                conn.prepare("SELECT file_path, line_start, line_end, content FROM chunks_old")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i32>(1)?,
                    row.get::<_, i32>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;

            for row in rows {
                let (file_path, line_start, line_end, content) = row?;
                let new_id = Uuid::new_v4().to_string();
                let hash = hash_content(&content);

                conn.execute(
                    r#"
                    INSERT INTO chunks (id, path, source, start_line, end_line, hash, model, text, embedding, updated_at)
                    VALUES (?1, ?2, 'memory', ?3, ?4, ?5, '', ?6, '', ?7)
                    "#,
                    params![&new_id, &file_path, line_start, line_end, &hash, &content, now],
                )?;
            }
        }

        // 5. Drop old tables
        let _ = conn.execute("DROP TABLE IF EXISTS chunks_old", []);
        let _ = conn.execute("DROP TABLE IF EXISTS files_old", []);

        conn.execute("COMMIT", [])?;
        info!("Schema migration completed successfully");
        Ok(())
    }

    /// Create FTS5 table with OpenClaw-compatible structure
    fn ensure_fts_table(conn: &Connection) -> Result<()> {
        // OpenClaw uses UNINDEXED columns for metadata
        let result = conn.execute(
            r#"
            CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                text,
                id UNINDEXED,
                path UNINDEXED,
                source UNINDEXED,
                model UNINDEXED,
                start_line UNINDEXED,
                end_line UNINDEXED
            )
            "#,
            [],
        );

        match result {
            Ok(_) => debug!("FTS5 table created/verified"),
            Err(e) => debug!("FTS5 table creation skipped: {}", e),
        }

        Ok(())
    }

    /// Ensure a column exists on a table (for migrations)
    fn ensure_column(conn: &Connection, table: &str, column: &str, definition: &str) -> Result<()> {
        let sql = format!("SELECT {} FROM {} LIMIT 0", column, table);
        if conn.prepare(&sql).is_err() {
            let alter = format!("ALTER TABLE {} ADD COLUMN {} {}", table, column, definition);
            conn.execute(&alter, [])?;
            debug!("Added column {} to table {}", column, table);
        }
        Ok(())
    }

    /// Get chunks that need embeddings (OpenClaw-compatible: id is TEXT, text column)
    pub fn chunks_without_embeddings(&self, limit: usize) -> Result<Vec<(String, String)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT id, text FROM chunks WHERE embedding = '' OR embedding IS NULL LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }

    /// Store embedding for a chunk (OpenClaw-compatible: id is TEXT, model column)
    pub fn store_embedding(&self, chunk_id: &str, embedding: &[f32], model: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let embedding_json = serialize_embedding(embedding);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "UPDATE chunks SET embedding = ?1, model = ?2, updated_at = ?3 WHERE id = ?4",
            params![&embedding_json, model, now, chunk_id],
        )?;

        // Also store in vec table if sqlite-vec is available
        if self.has_vec_extension {
            let embedding_blob = embedding_to_blob(embedding);
            let _ = conn.execute(
                "INSERT OR REPLACE INTO chunks_vec (id, embedding) VALUES (?1, ?2)",
                params![chunk_id, &embedding_blob],
            );
        }

        Ok(())
    }

    // ========================================================================
    // Embedding Cache (OpenClaw-compatible)
    // ========================================================================

    /// Get cached embedding by content hash
    pub fn get_cached_embedding(
        &self,
        provider: &str,
        model: &str,
        text_hash: &str,
    ) -> Result<Option<Vec<f32>>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let result: Option<String> = conn
            .query_row(
                "SELECT embedding FROM embedding_cache WHERE provider = ?1 AND model = ?2 AND hash = ?3",
                params![provider, model, text_hash],
                |row| row.get(0),
            )
            .ok();

        Ok(result.map(|json| deserialize_embedding(&json)))
    }

    /// Store embedding in cache
    pub fn cache_embedding(
        &self,
        provider: &str,
        model: &str,
        provider_key: &str,
        text_hash: &str,
        embedding: &[f32],
    ) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let embedding_json = serialize_embedding(embedding);
        let dims = embedding.len() as i32;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "INSERT OR REPLACE INTO embedding_cache (provider, model, provider_key, hash, embedding, dims, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![provider, model, provider_key, text_hash, &embedding_json, dims, now],
        )?;

        Ok(())
    }

    /// Check if sqlite-vec is available
    pub fn has_vec_extension(&self) -> bool {
        self.has_vec_extension
    }

    /// Vector search using embeddings (OpenClaw-compatible columns)
    /// Uses sqlite-vec if available for fast search, otherwise falls back to in-memory scan
    pub fn search_vector(
        &self,
        query_embedding: &[f32],
        model: &str,
        limit: usize,
    ) -> Result<Vec<MemoryChunk>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        // Try sqlite-vec fast path if available
        if self.has_vec_extension {
            if let Ok(results) = self.search_vector_fast(&conn, query_embedding, model, limit) {
                return Ok(results);
            }
            warn!("sqlite-vec search failed, falling back to in-memory scan");
        }

        // Fallback: in-memory scan (slower but always works)
        self.search_vector_scan(&conn, query_embedding, model, limit)
    }

    /// Fast vector search using sqlite-vec extension
    fn search_vector_fast(
        &self,
        conn: &Connection,
        query_embedding: &[f32],
        model: &str,
        limit: usize,
    ) -> Result<Vec<MemoryChunk>> {
        let query_blob = embedding_to_blob(query_embedding);

        // sqlite-vec uses vec_distance_cosine for cosine distance (1 - similarity)
        let mut stmt = conn.prepare(
            r#"
            SELECT c.path, c.start_line, c.end_line, c.text,
                   1.0 - vec_distance_cosine(v.embedding, ?1) AS score
            FROM chunks_vec v
            JOIN chunks c ON c.id = v.id
            WHERE c.model = ?2
            ORDER BY score DESC
            LIMIT ?3
            "#,
        )?;

        let rows = stmt.query_map(params![&query_blob, model, limit as i64], |row| {
            Ok(MemoryChunk {
                file: row.get(0)?,
                line_start: row.get(1)?,
                line_end: row.get(2)?,
                content: row.get(3)?,
                score: row.get(4)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// In-memory vector scan (fallback when sqlite-vec not available)
    fn search_vector_scan(
        &self,
        conn: &Connection,
        query_embedding: &[f32],
        model: &str,
        limit: usize,
    ) -> Result<Vec<MemoryChunk>> {
        let mut stmt = conn.prepare(
            "SELECT id, path, start_line, end_line, text, embedding
             FROM chunks
             WHERE embedding != '' AND embedding IS NOT NULL AND model = ?1",
        )?;

        let rows = stmt.query_map(params![model], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i32>(2)?,
                row.get::<_, i32>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;

        // Compute similarities and sort
        let mut scored: Vec<(f32, MemoryChunk)> = Vec::new();

        for row in rows {
            let (_, path, start_line, end_line, text, embedding_json) = row?;
            let embedding = deserialize_embedding(&embedding_json);

            if embedding.len() == query_embedding.len() {
                let similarity = cosine_similarity(query_embedding, &embedding);
                scored.push((
                    similarity,
                    MemoryChunk {
                        file: path,
                        line_start: start_line,
                        line_end: end_line,
                        content: text,
                        score: similarity as f64,
                    },
                ));
            }
        }

        // Sort by similarity (descending)
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Take top results
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(_, chunk)| chunk)
            .collect())
    }

    /// Hybrid search: combine FTS and vector results
    pub fn search_hybrid(
        &self,
        query: &str,
        query_embedding: Option<&[f32]>,
        model: &str,
        limit: usize,
        text_weight: f32,
        vector_weight: f32,
    ) -> Result<Vec<MemoryChunk>> {
        // Get FTS results
        let fts_results = self.search(query, limit * 2)?;

        // Get vector results if embedding provided
        let vector_results = if let Some(embedding) = query_embedding {
            self.search_vector(embedding, model, limit * 2)?
        } else {
            Vec::new()
        };

        // Merge results using rank-based scoring (OpenClaw-compatible)
        let mut merged: std::collections::HashMap<String, (f32, MemoryChunk)> =
            std::collections::HashMap::new();

        // Add FTS results using rank-based scoring (OpenClaw-compatible)
        // BM25 results are already ordered by relevance (best first)
        for (rank, result) in fts_results.into_iter().enumerate() {
            let key = format!("{}:{}:{}", result.file, result.line_start, result.line_end);
            let rank_score = 1.0 / (1.0 + rank as f32); // rank 0 → 1.0, rank 1 → 0.5, rank 9 → 0.1
            let weighted_score = rank_score * text_weight;
            merged.insert(key, (weighted_score, result));
        }

        // Add/merge vector results using rank-based scoring
        for (rank, result) in vector_results.into_iter().enumerate() {
            let key = format!("{}:{}:{}", result.file, result.line_start, result.line_end);
            let rank_score = 1.0 / (1.0 + rank as f32);
            let weighted_score = rank_score * vector_weight;

            if let Some((existing_score, existing_chunk)) = merged.get_mut(&key) {
                *existing_score += weighted_score;
                existing_chunk.score = *existing_score as f64;
            } else {
                let mut chunk = result;
                chunk.score = weighted_score as f64;
                merged.insert(key, (weighted_score, chunk));
            }
        }

        // Sort by combined score and take top results
        let mut results: Vec<_> = merged.into_values().collect();
        results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        Ok(results
            .into_iter()
            .take(limit)
            .map(|(_, chunk)| chunk)
            .collect())
    }

    /// Count chunks with embeddings (OpenClaw-compatible: model column)
    pub fn embedded_chunk_count(&self, model: &str) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow!("Lock poisoned: {}", e))?;

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chunks WHERE embedding != '' AND embedding IS NOT NULL AND model = ?1",
            params![model],
            |row| row.get(0),
        )?;

        Ok(count as usize)
    }
}

fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Convert f32 embedding to binary blob for sqlite-vec
fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(embedding.len() * 4);
    for &val in embedding {
        blob.extend_from_slice(&val.to_le_bytes());
    }
    blob
}

/// Build FTS5 query from raw input (OpenClaw-compatible)
/// Tokenizes input and joins with AND so all terms must appear (in any order)
fn build_fts_query(raw: &str) -> Option<String> {
    let tokens: Vec<&str> = raw
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.is_empty() {
        return None;
    }

    // Quote each token individually, join with AND
    let quoted: Vec<String> = tokens
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();

    Some(quoted.join(" AND "))
}

struct ChunkInfo {
    line_start: i32,
    line_end: i32,
    content: String,
}

fn chunk_text(text: &str, target_tokens: usize, overlap_tokens: usize) -> Vec<ChunkInfo> {
    let lines: Vec<&str> = text.lines().collect();
    let mut chunks = Vec::new();

    if lines.is_empty() {
        return chunks;
    }

    // Rough estimate: 4 chars per token
    let target_chars = target_tokens * 4;
    let overlap_chars = overlap_tokens * 4;

    let mut start_line = 0;
    let mut current_chars = 0;
    let mut chunk_lines = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        chunk_lines.push(*line);
        current_chars += line.len() + 1; // +1 for newline

        if current_chars >= target_chars || i == lines.len() - 1 {
            // Create chunk
            chunks.push(ChunkInfo {
                line_start: (start_line + 1) as i32,
                line_end: (i + 1) as i32,
                content: chunk_lines.join("\n"),
            });

            // Calculate overlap for next chunk
            let mut overlap_len = 0;
            let mut overlap_start = chunk_lines.len();

            for (j, line) in chunk_lines.iter().enumerate().rev() {
                overlap_len += line.len() + 1;
                if overlap_len >= overlap_chars {
                    overlap_start = j;
                    break;
                }
            }

            // Prepare for next chunk
            if overlap_start < chunk_lines.len() {
                start_line += overlap_start;
                chunk_lines = chunk_lines[overlap_start..].to_vec();
                current_chars = chunk_lines.iter().map(|l| l.len() + 1).sum();
            } else {
                start_line = i + 1;
                chunk_lines.clear();
                current_chars = 0;
            }
        }
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_chunk_text() {
        let text = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5";
        let chunks = chunk_text(text, 10, 2); // Small chunks for testing

        assert!(!chunks.is_empty());
        assert_eq!(chunks[0].line_start, 1);
    }

    #[test]
    fn test_memory_index() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let workspace = temp_dir.path();

        // Create a test file
        let test_file = workspace.join("test.md");
        fs::write(
            &test_file,
            "# Test\n\nThis is a test document.\n\nWith multiple lines.",
        )?;

        let index = MemoryIndex::new(workspace)?;
        index.index_file(&test_file, false)?;

        assert!(index.chunk_count()? > 0);

        let results = index.search("test document", 10)?;
        assert!(!results.is_empty());

        Ok(())
    }
}
