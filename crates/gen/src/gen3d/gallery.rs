//! World gallery — filesystem scanner for browsing generated worlds.
//!
//! Scans `workspace/skills/` for world skill directories and builds
//! gallery entries from their `world.ron` metadata. No database required.

use jiff::Timestamp;
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
    /// When the world was created (from filesystem metadata), as a UTC
    /// instant. `None` when the filesystem reports no time for the file or
    /// the reported time is outside the range jiff represents (years
    /// -9999..=9999) — such a time is refused, never clamped, and the
    /// gallery shows the entry as created at an unknown date.
    pub created_at: Option<Timestamp>,
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
            .and_then(|time| metadata_timestamp(time, &ron_path));

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
            prompt: manifest
                .meta
                .ext_provenance
                .as_ref()
                .and_then(|p| p.prompt.clone()),
        });
    }

    // Sort by creation date, newest first
    sort_newest_first(&mut entries);
    entries
}

/// Order gallery entries newest first; an entry whose creation time is
/// unknown sorts last.
fn sort_newest_first(entries: &mut [WorldGalleryEntry]) {
    entries.sort_by_key(|b| std::cmp::Reverse(b.created_at));
}

/// Convert a file's metadata time to a UTC instant. `Timestamp`'s
/// `TryFrom<SystemTime>` is fallible: a filesystem time outside the range
/// jiff represents is an error, and returning `None` here (the entry's
/// "unknown" date) with the refusal logged is the deliberate handling —
/// never a clamp to jiff's minimum or maximum.
fn metadata_timestamp(time: std::time::SystemTime, source: &Path) -> Option<Timestamp> {
    match Timestamp::try_from(time) {
        Ok(ts) => Some(ts),
        Err(e) => {
            tracing::warn!(
                "Failed to convert {} metadata time to a timestamp: {}",
                source.display(),
                e
            );
            None
        }
    }
}

/// An entry's creation date, in UTC — the date `chrono`'s
/// `DateTime<Utc>::format("%Y-%m-%d")` used to print, never the local
/// zone's.
fn utc_date(ts: Timestamp) -> String {
    ts.strftime("%Y-%m-%d").to_string()
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
        summary.push_str(&summary_line(entry));
        summary.push('\n');
    }
    summary
}

/// One gallery-summary line for an entry: name, entities, source, UTC date
/// (or "unknown" when the creation time is missing).
fn summary_line(entry: &WorldGalleryEntry) -> String {
    let date = entry
        .created_at
        .map(utc_date)
        .unwrap_or_else(|| "unknown".to_string());
    let thumb = if entry.thumbnail_path.is_some() {
        " [thumb]"
    } else {
        ""
    };
    format!(
        "  {} — {} entities, {} ({}){}",
        entry.name, entry.entity_count, entry.source, date, thumb
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 2023-11-14T22:13:20Z, the fixed instant the time tests format.
    const NOV_14_22_13_20Z: i64 = 1_700_000_000;

    fn ts(second: i64) -> Timestamp {
        Timestamp::from_second(second).unwrap()
    }

    fn entry(created_at: Option<Timestamp>) -> WorldGalleryEntry {
        WorldGalleryEntry {
            name: "demo".to_string(),
            path: PathBuf::from("skills/demo"),
            description: None,
            entity_count: 0,
            created_at,
            thumbnail_path: None,
            style_tags: vec![],
            variation_group: None,
            source: "interactive".to_string(),
            prompt: None,
        }
    }

    #[test]
    fn gallery_dates_are_utc_not_local() {
        // 22:13 UTC is already November 15 on a UTC+13 clock, so a local
        // date would print 2023-11-15 here.
        assert_eq!(utc_date(ts(NOV_14_22_13_20Z)), "2023-11-14");
        // The UTC day boundary: 2023-11-14T23:59:59Z, then 2023-11-15T00:00:00Z.
        assert_eq!(utc_date(ts(1_700_006_399)), "2023-11-14");
        assert_eq!(utc_date(ts(1_700_006_400)), "2023-11-15");
    }

    #[test]
    fn entries_sort_newest_first_with_unknown_time_last() {
        let mut entries = vec![
            entry(None),
            entry(Some(ts(100))),
            entry(Some(ts(300))),
            entry(Some(ts(200))),
        ];
        sort_newest_first(&mut entries);
        let times: Vec<Option<i64>> = entries
            .iter()
            .map(|e| e.created_at.map(|t| t.as_second()))
            .collect();
        assert_eq!(times, vec![Some(300), Some(200), Some(100), None]);
    }

    #[test]
    fn summary_shows_utc_date_and_unknown_for_missing_time() {
        let known = entry(Some(ts(NOV_14_22_13_20Z)));
        assert_eq!(
            summary_line(&known),
            "  demo — 0 entities, interactive (2023-11-14)"
        );
        let missing = entry(None);
        assert_eq!(
            summary_line(&missing),
            "  demo — 0 entities, interactive (unknown)"
        );
    }

    #[test]
    fn out_of_range_metadata_times_are_refused_not_clamped() {
        // Jiff represents instants up to year 9999 (253402207200 seconds
        // after the epoch); beyond that `TryFrom` fails and the entry keeps
        // an unknown date instead of being pinned to jiff's maximum.
        let far_future = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(300_000_000_000);
        assert!(Timestamp::try_from(far_future).is_err());
        assert_eq!(metadata_timestamp(far_future, Path::new("world.ron")), None);

        // An in-range filesystem time converts exactly.
        let real = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(NOV_14_22_13_20Z as u64);
        assert_eq!(
            metadata_timestamp(real, Path::new("world.ron")),
            Some(ts(NOV_14_22_13_20Z))
        );
    }

    #[test]
    fn scan_finds_world_skills_and_skips_other_folders() {
        let dir = tempfile::tempdir().unwrap();
        let world = dir.path().join("skills").join("demo");
        std::fs::create_dir_all(&world).unwrap();
        let manifest = localgpt_world_types::WorldManifest::new("demo");
        std::fs::write(world.join("world.ron"), ron::to_string(&manifest).unwrap()).unwrap();
        std::fs::create_dir_all(dir.path().join("skills").join("not-a-world")).unwrap();

        let entries = scan_world_gallery(dir.path());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "demo");
        assert!(entries[0].created_at.is_some());

        // The summary line shows the entry's own creation date.
        let summary = gallery_summary(dir.path());
        let expected = summary_line(&entries[0]);
        assert!(
            summary.contains(&expected),
            "no {expected:?} in {summary:?}"
        );
    }
}
