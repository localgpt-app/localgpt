//! File-hash-based media cache for transcription and document extraction results.
//!
//! Avoids re-processing the same files by caching results keyed by SHA-256 content hash.

use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tracing::debug;

/// Media cache backed by filesystem.
#[derive(Debug, Clone)]
pub struct MediaCache {
    cache_dir: PathBuf,
    max_bytes: u64,
}

impl MediaCache {
    /// Create a new media cache at the given directory.
    pub fn new(cache_dir: PathBuf, max_mb: u64) -> Self {
        Self {
            cache_dir,
            max_bytes: max_mb * 1024 * 1024,
        }
    }

    /// Get cached result for a file. Returns None if not cached.
    pub fn get(&self, file_path: &Path) -> Option<String> {
        let hash = self.hash_file(file_path)?;
        let cache_path = self.cache_dir.join(format!("{}.txt", hash));

        if cache_path.exists() {
            match std::fs::read_to_string(&cache_path) {
                Ok(content) => {
                    debug!("Media cache hit: {} -> {}", file_path.display(), hash);
                    Some(content)
                }
                Err(_) => None,
            }
        } else {
            None
        }
    }

    /// Store a result in the cache.
    pub fn put(&self, file_path: &Path, result: &str) -> Result<()> {
        let hash = match self.hash_file(file_path) {
            Some(h) => h,
            None => return Ok(()), // Can't hash = can't cache
        };

        std::fs::create_dir_all(&self.cache_dir)?;

        // Check cache size before writing
        if self.max_bytes > 0 {
            let current_size = self.total_size();
            if current_size + result.len() as u64 > self.max_bytes {
                debug!(
                    "Media cache full ({} bytes, max {}), evicting oldest",
                    current_size, self.max_bytes
                );
                self.evict_oldest(result.len() as u64)?;
            }
        }

        let cache_path = self.cache_dir.join(format!("{}.txt", hash));
        std::fs::write(&cache_path, result)?;
        debug!("Media cache store: {} -> {}", file_path.display(), hash);
        Ok(())
    }

    /// Hash file content with SHA-256.
    fn hash_file(&self, path: &Path) -> Option<String> {
        let data = std::fs::read(path).ok()?;
        let mut hasher = Sha256::new();
        hasher.update(&data);
        Some(format!("{:x}", hasher.finalize()))
    }

    /// Calculate total cache size in bytes.
    fn total_size(&self) -> u64 {
        if !self.cache_dir.exists() {
            return 0;
        }
        std::fs::read_dir(&self.cache_dir)
            .ok()
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| e.metadata().ok())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Evict oldest cache entries to free at least `needed` bytes.
    fn evict_oldest(&self, needed: u64) -> Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(&self.cache_dir)?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let meta = e.metadata().ok()?;
                let modified = meta.modified().ok()?;
                Some((e.path(), meta.len(), modified))
            })
            .collect();

        // Sort by modification time (oldest first)
        entries.sort_by_key(|(_, _, modified)| *modified);

        let mut freed = 0u64;
        for (path, size, _) in &entries {
            if freed >= needed {
                break;
            }
            let _ = std::fs::remove_file(path);
            freed += size;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_miss_returns_none() {
        let tmp = TempDir::new().unwrap();
        let cache = MediaCache::new(tmp.path().join("cache"), 100);

        let file = tmp.path().join("test.txt");
        std::fs::write(&file, "content").unwrap();

        assert!(cache.get(&file).is_none());
    }

    #[test]
    fn test_cache_put_and_get() {
        let tmp = TempDir::new().unwrap();
        let cache = MediaCache::new(tmp.path().join("cache"), 100);

        let file = tmp.path().join("test.pdf");
        std::fs::write(&file, "pdf content").unwrap();

        cache.put(&file, "extracted text").unwrap();
        assert_eq!(cache.get(&file).unwrap(), "extracted text");
    }

    #[test]
    fn test_cache_different_content_different_key() {
        let tmp = TempDir::new().unwrap();
        let cache = MediaCache::new(tmp.path().join("cache"), 100);

        let file1 = tmp.path().join("a.pdf");
        let file2 = tmp.path().join("b.pdf");
        std::fs::write(&file1, "content A").unwrap();
        std::fs::write(&file2, "content B").unwrap();

        cache.put(&file1, "text A").unwrap();
        cache.put(&file2, "text B").unwrap();

        assert_eq!(cache.get(&file1).unwrap(), "text A");
        assert_eq!(cache.get(&file2).unwrap(), "text B");
    }

    #[test]
    fn test_cache_eviction_on_full() {
        let tmp = TempDir::new().unwrap();
        let cache_dir = tmp.path().join("cache");
        let cache = MediaCache::new(cache_dir, 1); // 1 MB

        let file = tmp.path().join("test.txt");
        std::fs::write(&file, "data").unwrap();
        cache.put(&file, "result").unwrap();

        assert!(cache.get(&file).is_some());
    }

    #[test]
    fn test_cache_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let cache = MediaCache::new(tmp.path().join("cache"), 100);

        assert!(cache.get(Path::new("/nonexistent/file.pdf")).is_none());
    }
}
