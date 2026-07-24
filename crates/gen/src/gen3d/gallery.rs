//! World gallery — filesystem scanner for browsing generated worlds.
//!
//! Scans `workspace/skills/` for world skill directories and builds
//! gallery entries from their `world.ron` metadata. No database required.

use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// A single entry in the world gallery.
#[derive(Debug, Clone)]
pub struct WorldGalleryEntry {
    /// World name from the manifest.
    pub name: String,
    /// Path to the world skill directory.
    pub path: PathBuf,
    /// Human-readable description.
    pub description: Option<String>,
    /// Number of entities in the world.
    pub entity_count: usize,
    /// When the world was created (from filesystem metadata).
    pub created_at: Option<DateTime<Utc>>,
    /// Path to the best thumbnail image.
    pub thumbnail_path: Option<PathBuf>,
    /// Free-form style tags for filtering.
    pub style_tags: Vec<String>,
    /// Variation group ID (if part of a variation experiment).
    pub variation_group: Option<String>,
    /// Generation source: "interactive", "headless", "experiment", "mcp".
    pub source: String,
    /// Original prompt used to generate (if available).
    pub prompt: Option<String>,
}

/// Scan workspace/skills/ for world skills and build gallery entries.
pub fn scan_world_gallery(workspace: &Path) -> Vec<WorldGalleryEntry> {
    let skills_dir = workspace.join("skills");
    let mut entries = Vec::new();

    if !skills_dir.exists() {
        return entries;
    }

    let read_dir = match std::fs::read_dir(&skills_dir) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::warn!("Failed to read skills directory: {}", e);
            return entries;
        }
    };

    for dir_entry in read_dir.flatten() {
        let path = dir_entry.path();
        if !path.is_dir() {
            continue;
        }

        // Must have world.ron to be a world skill
        let ron_path = path.join("world.ron");
        if !ron_path.exists() {
            continue;
        }

        // Parse world.ron for metadata
        let ron_str = match std::fs::read_to_string(&ron_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let manifest = match ron::from_str::<localgpt_world_types::WorldManifest>(&ron_str) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!("Failed to parse {}: {}", ron_path.display(), e);
                continue;
            }
        };

        let thumbnail_path = find_thumbnail(&path);
        let created_at = std::fs::metadata(&ron_path)
            .ok()
            .and_then(|m| m.created().ok().or_else(|| m.modified().ok()))
            .map(DateTime::<Utc>::from);

        entries.push(WorldGalleryEntry {
            name: manifest.meta.name.clone(),
            path: path.clone(),
            description: manifest.meta.description.clone(),
            entity_count: manifest.entities.len(),
            created_at,
            thumbnail_path,
            style_tags: manifest.meta.tags.clone().unwrap_or_default(),
            variation_group: manifest.meta.variation_group.clone(),
            source: manifest
                .meta
                .source
                .clone()
                .unwrap_or_else(|| "interactive".to_string()),
            prompt: manifest.meta.prompt.clone(),
        });
    }

    // Sort by creation date, newest first
    entries.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    entries
}

/// Find the best thumbnail for a world skill folder.
///
/// Looks in the `screenshots/` subdirectory and returns the most recent image.
fn find_thumbnail(world_path: &Path) -> Option<PathBuf> {
    let screenshots_dir = world_path.join("screenshots");
    if !screenshots_dir.exists() {
        return None;
    }

    let mut screenshots: Vec<PathBuf> = std::fs::read_dir(&screenshots_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext == "png" || ext == "jpg" || ext == "jpeg")
        })
        .collect();

    // Sort by name (timestamps in filenames) to get the most recent
    screenshots.sort();
    screenshots.last().cloned()
}

/// Get a summary of the gallery for display.
pub fn gallery_summary(workspace: &Path) -> String {
    let entries = scan_world_gallery(workspace);
    if entries.is_empty() {
        return "No worlds found in skills/".to_string();
    }

    let mut summary = format!("{} worlds found:\n", entries.len());
    for entry in &entries {
        let date = entry
            .created_at
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let thumb = if entry.thumbnail_path.is_some() {
            " [thumb]"
        } else {
            ""
        };
        summary.push_str(&format!(
            "  {} — {} entities, {} ({}){}\n",
            entry.name, entry.entity_count, entry.source, date, thumb
        ));
    }
    summary
}
