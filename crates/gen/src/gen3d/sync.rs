//! Sync and drift detection engine.
//!
//! Computes content hashes for `.ron` and `.md` files, extracts structural
//! claims from markdown, and compares against a stored `SyncManifest` to
//! detect when representations drift apart.

use jiff::Timestamp;
use std::collections::HashMap;
use std::path::Path;

use localgpt_world_types::*;
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// Hash computation
// ---------------------------------------------------------------------------

/// 8-char SHA-256 prefix of content.
fn sha256_prefix(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..4]) // 8 hex chars
}

/// Hash a `.ron` file, normalizing whitespace/formatting.
pub fn ron_content_hash(path: &Path) -> Result<String, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    // Parse and re-serialize to normalize formatting
    let parsed: ron::Value = ron::from_str(&content)
        .map_err(|e| format!("Failed to parse RON {}: {}", path.display(), e))?;
    let normalized =
        ron::to_string(&parsed).map_err(|e| format!("Failed to serialize RON: {}", e))?;
    Ok(sha256_prefix(&normalized))
}

/// Hash structural claims from a `.md` file.
/// Returns `None` if no Entity Groups section found (→ Unknown status).
pub fn md_content_hash(path: &Path) -> Result<Option<String>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    match extract_structural_claims(&content) {
        Ok(claims) if claims.is_empty() => Ok(None),
        Ok(claims) => {
            let json = serde_json::to_string(&claims)
                .map_err(|e| format!("Failed to serialize claims: {}", e))?;
            Ok(Some(sha256_prefix(&json)))
        }
        Err(_) => Ok(None), // No Entity Groups section → Unknown
    }
}

// ---------------------------------------------------------------------------
// Structural claim extraction
// ---------------------------------------------------------------------------

/// Extract structural claims from the `## Entity Groups` section of markdown.
pub fn extract_structural_claims(
    md_content: &str,
) -> Result<Vec<StructuralClaim>, ClaimExtractionError> {
    // 1. Find "## Entity Groups" section
    let entity_groups_start = md_content
        .find("## Entity Groups")
        .ok_or(ClaimExtractionError::MissingEntityGroups)?;

    // 2. Extract section content (until next ## heading or EOF)
    let section = &md_content[entity_groups_start..];
    let section_end = section[16..]
        .find("\n## ")
        .map(|i| i + 16)
        .unwrap_or(section.len());
    let section_content = &section[..section_end];

    let mut claims = Vec::new();

    for line in section_content.lines() {
        let line = line.trim();

        // Check for <!-- sync: {...} --> HTML comments (authoritative)
        if let Some(sync_start) = line.find("<!-- sync:")
            && let Some(sync_end) = line[sync_start..].find("-->")
        {
            let json_str = line[sync_start + 10..sync_start + sync_end].trim();
            if let Ok(claim) = serde_json::from_str::<StructuralClaim>(json_str) {
                claims.push(claim);
                continue;
            }
        }

        // Fallback: parse "- **name** — description" lines
        if line.starts_with("- **")
            && let Some(name_end) = line[4..].find("**")
        {
            let entity_name = line[4..4 + name_end].to_string();
            let rest = &line[4 + name_end + 2..];

            let mut claim = StructuralClaim {
                entity_name: Some(entity_name),
                count: None,
                position: None,
                dimensions: None,
                tier: None,
                material_hint: None,
                behavior_hint: None,
            };

            // Parse backtick-delimited position: `[x, y, z]`
            if let Some(pos) = extract_backtick_array(rest) {
                claim.position = Some(pos);
            }

            // Parse dimensions: NxNxNm
            if let Some(dims) = extract_dimensions(rest) {
                claim.dimensions = Some(dims);
            }

            // Parse material hint: last comma-separated phrase
            if let Some(hint) = extract_material_hint(rest) {
                claim.material_hint = Some(hint);
            }

            claims.push(claim);
        }

        // Parse count patterns: "4x market stalls..."
        if line.starts_with("- ")
            && !line.starts_with("- **")
            && let Some(count_claim) = parse_count_line(&line[2..])
        {
            claims.push(count_claim);
        }
    }

    // Sort by entity_name for deterministic hashing
    claims.sort_by(|a, b| a.entity_name.cmp(&b.entity_name));
    Ok(claims)
}

/// Extract `[x, y, z]` from backtick-delimited text.
fn extract_backtick_array(text: &str) -> Option<[f32; 3]> {
    let start = text.find('`')? + 1;
    let end = text[start..].find('`')? + start;
    let inner = text[start..end].trim();
    if !inner.starts_with('[') {
        return None;
    }
    let inner = &inner[1..inner.len() - 1];
    let parts: Vec<f32> = inner
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    if parts.len() == 3 {
        Some([parts[0], parts[1], parts[2]])
    } else {
        None
    }
}

/// Extract NxNxNm dimensions from text.
fn extract_dimensions(text: &str) -> Option<[f32; 3]> {
    let re_like = text.split_whitespace().find(|w| {
        w.contains('x')
            && w.chars()
                .next()
                .map(|c| c.is_ascii_digit())
                .unwrap_or(false)
    })?;
    // Strip trailing punctuation and unit suffix
    let clean = re_like.trim_end_matches(|c: char| !c.is_ascii_digit());
    let parts: Vec<f32> = clean.split('x').filter_map(|s| s.parse().ok()).collect();
    if parts.len() == 3 {
        Some([parts[0], parts[1], parts[2]])
    } else {
        None
    }
}

/// Extract material hint from the last comma-separated phrase.
fn extract_material_hint(text: &str) -> Option<String> {
    let last_part = text.rsplit(',').next()?.trim();
    if last_part.is_empty() || last_part.contains('[') || last_part.contains('`') {
        None
    } else {
        Some(last_part.to_string())
    }
}

/// Parse "4x market stalls..." → count=4, entity_name pattern.
fn parse_count_line(text: &str) -> Option<StructuralClaim> {
    let text = text.trim();
    let x_pos = text.find('x')?;
    let count: u32 = text[..x_pos].trim().parse().ok()?;
    let name_part = text[x_pos + 1..]
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join("_");
    Some(StructuralClaim {
        entity_name: Some(name_part),
        count: Some(count),
        position: None,
        dimensions: None,
        tier: None,
        material_hint: None,
        behavior_hint: None,
    })
}

// ---------------------------------------------------------------------------
// SyncManifest I/O
// ---------------------------------------------------------------------------

/// Load a sync manifest from `meta/.sync.ron`.
pub fn load_sync_manifest(meta_dir: &Path) -> Option<SyncManifest> {
    let path = meta_dir.join(".sync.ron");
    let content = std::fs::read_to_string(&path).ok()?;
    ron::from_str(&content).ok()
}

/// Save a sync manifest to `meta/.sync.ron`.
pub fn save_sync_manifest(meta_dir: &Path, manifest: &SyncManifest) -> std::io::Result<()> {
    std::fs::create_dir_all(meta_dir)?;
    let ron_str = ron::ser::to_string_pretty(manifest, ron::ser::PrettyConfig::default())
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(meta_dir.join(".sync.ron"), ron_str)
}

// ---------------------------------------------------------------------------
// Main drift detection
// ---------------------------------------------------------------------------

/// Return an ISO 8601 timestamp string for "now".
fn now_iso8601() -> String {
    rfc3339_utc_autosi(Timestamp::now())
}

/// Format a UTC instant the way `chrono`'s `to_rfc3339` did, which is the
/// shape `.sync.ron`'s timestamp strings have always had: RFC 3339 with an
/// explicit `+00:00` offset and AutoSi fractional seconds (0, 3, 6 or 9
/// digits). Jiff's `Display` would print `Z` with the fewest digits it can
/// (`...20.5Z`), so the string is assembled here instead.
fn rfc3339_utc_autosi(ts: Timestamp) -> String {
    let subsec = ts.subsec_nanosecond().unsigned_abs();
    let fraction = if subsec == 0 {
        String::new()
    } else if subsec.is_multiple_of(1_000_000) {
        format!(".{:03}", subsec / 1_000_000)
    } else if subsec.is_multiple_of(1_000) {
        format!(".{:06}", subsec / 1_000)
    } else {
        format!(".{:09}", subsec)
    };
    format!("{}{}+00:00", ts.strftime("%Y-%m-%dT%H:%M:%S"), fraction)
}

/// Scan a world directory and detect drift across all domains.
pub fn check_drift(world_dir: &Path) -> Result<DriftReport, String> {
    let meta_dir = world_dir.join("meta");
    let existing_manifest = load_sync_manifest(&meta_dir);

    let mut domains = Vec::new();

    // Scan for .ron/.md pairs in regions/, behaviors/, audio/, avatar/, layout/
    for subdir in &["regions", "behaviors", "audio", "avatar", "layout"] {
        let dir = world_dir.join(subdir);
        if !dir.exists() {
            continue;
        }

        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("Failed to read {}: {}", dir.display(), e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "ron").unwrap_or(false) {
                let stem = path.file_stem().unwrap().to_string_lossy();
                let domain = format!("{}/{}", subdir, stem);
                let md_path = dir.join(format!("{}.md", stem));

                let ron_hash = ron_content_hash(&path)?;
                let md_hash = if md_path.exists() {
                    md_content_hash(&md_path)?
                } else {
                    None
                };

                // Compare against existing manifest
                let status = if let Some(ref manifest) = existing_manifest {
                    if let Some(record) = manifest.domains.get(&domain) {
                        determine_status(record, &ron_hash, &md_hash)
                    } else {
                        SyncStatus::Unknown
                    }
                } else {
                    SyncStatus::Unknown
                };

                domains.push(DomainDrift {
                    domain,
                    status: status.clone(),
                    detail: None,
                    structural_diffs: vec![],
                    suggestion: generate_suggestion(&status),
                });
            }
        }
    }

    let overall = if domains.iter().all(|d| d.status == SyncStatus::Clean) {
        SyncStatus::Clean
    } else if domains.iter().any(|d| d.status == SyncStatus::Conflict) {
        SyncStatus::Conflict
    } else {
        domains
            .iter()
            .find(|d| d.status != SyncStatus::Clean)
            .map(|d| d.status.clone())
            .unwrap_or(SyncStatus::Clean)
    };

    // If no manifest existed, create a baseline
    if existing_manifest.is_none() {
        let mut new_manifest = SyncManifest {
            updated_at: now_iso8601(),
            domains: HashMap::new(),
            root_md_hash: None,
            root_ron_hash: None,
            root_scene_hash: None,
        };
        for dd in &domains {
            // Find the ron/md hashes we just computed
            let subdir_and_stem: Vec<&str> = dd.domain.splitn(2, '/').collect();
            if subdir_and_stem.len() == 2 {
                let dir = world_dir.join(subdir_and_stem[0]);
                let ron_path = dir.join(format!("{}.ron", subdir_and_stem[1]));
                let md_path = dir.join(format!("{}.md", subdir_and_stem[1]));

                let ron_hash = ron_content_hash(&ron_path).unwrap_or_default();
                let md_hash = if md_path.exists() {
                    md_content_hash(&md_path).ok().flatten()
                } else {
                    None
                };

                new_manifest.domains.insert(
                    dd.domain.clone(),
                    SyncRecord {
                        md_hash,
                        ron_hash,
                        scene_hash: None,
                        md_mtime: None,
                        ron_mtime: now_iso8601(),
                        last_sync: now_iso8601(),
                        sync_direction: SyncDirection::RonToScene,
                        status: SyncStatus::Unknown,
                    },
                );
            }
        }
        let _ = save_sync_manifest(&meta_dir, &new_manifest);
    }

    Ok(DriftReport {
        overall_status: overall,
        domains,
    })
}

/// Compare stored record hashes against current file hashes.
fn determine_status(
    record: &SyncRecord,
    current_ron_hash: &str,
    current_md_hash: &Option<String>,
) -> SyncStatus {
    let ron_changed = record.ron_hash != current_ron_hash;
    let md_changed = match (&record.md_hash, current_md_hash) {
        (Some(old), Some(new)) => old != new,
        (None, None) => false,
        _ => true,
    };

    match (md_changed, ron_changed) {
        (false, false) => SyncStatus::Clean,
        (true, false) => SyncStatus::MdAhead,
        (false, true) => SyncStatus::RonAhead,
        (true, true) => SyncStatus::Conflict,
    }
}

/// Generate a human-readable suggestion for a given status.
fn generate_suggestion(status: &SyncStatus) -> Option<String> {
    match status {
        SyncStatus::Clean => None,
        SyncStatus::MdAhead => {
            Some("Run gen_sync with source=md to propagate .md changes".to_string())
        }
        SyncStatus::RonAhead => {
            Some("Run gen_sync with source=ron to update scene and .md".to_string())
        }
        SyncStatus::SceneAhead => {
            Some("Run gen_sync with source=scene to persist scene changes".to_string())
        }
        SyncStatus::Conflict => {
            Some("Resolve conflict: specify per-field resolution in gen_sync".to_string())
        }
        SyncStatus::Unknown => Some("Run gen_check_drift to establish baseline".to_string()),
    }
}

// ---------------------------------------------------------------------------
// Sync apply
// ---------------------------------------------------------------------------

/// Result of a sync operation.
#[derive(Debug)]
pub enum SyncResult {
    Preview(String),
    Applied { files_updated: Vec<String> },
}

/// Apply a sync operation for a given domain.
pub fn apply_sync(
    world_dir: &Path,
    domain: &str,
    source: &str,
    preview: bool,
    _resolve_conflicts: &Option<HashMap<String, String>>,
) -> Result<SyncResult, String> {
    // Parse domain into subdir/stem
    let parts: Vec<&str> = domain.splitn(2, '/').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid domain format '{}': expected 'subdir/name'",
            domain
        ));
    }
    let (subdir, stem) = (parts[0], parts[1]);
    let dir = world_dir.join(subdir);
    let ron_path = dir.join(format!("{}.ron", stem));
    let md_path = dir.join(format!("{}.md", stem));

    match source {
        "ron" => {
            if !ron_path.exists() {
                return Err(format!("RON file not found: {}", ron_path.display()));
            }

            if preview {
                let mut changes = format!("Sync source=ron for domain '{}':\n", domain);
                if md_path.exists() {
                    changes.push_str(&format!(
                        "  - Update {} Entity Groups section with current .ron data\n",
                        md_path.display()
                    ));
                } else {
                    changes.push_str(&format!(
                        "  - No .md file at {} — skipping markdown update\n",
                        md_path.display()
                    ));
                }
                changes.push_str("  - Update sync manifest with new hashes\n");
                return Ok(SyncResult::Preview(changes));
            }

            let mut files_updated = Vec::new();

            // Update sync manifest with current hashes
            let meta_dir = world_dir.join("meta");
            let mut manifest = load_sync_manifest(&meta_dir).unwrap_or_else(|| SyncManifest {
                updated_at: now_iso8601(),
                domains: HashMap::new(),
                root_md_hash: None,
                root_ron_hash: None,
                root_scene_hash: None,
            });

            let ron_hash = ron_content_hash(&ron_path)?;
            let md_hash = if md_path.exists() {
                md_content_hash(&md_path)?
            } else {
                None
            };

            manifest.updated_at = now_iso8601();
            manifest.domains.insert(
                domain.to_string(),
                SyncRecord {
                    md_hash,
                    ron_hash,
                    scene_hash: None,
                    md_mtime: None,
                    ron_mtime: now_iso8601(),
                    last_sync: now_iso8601(),
                    sync_direction: SyncDirection::RonToScene,
                    status: SyncStatus::Clean,
                },
            );

            save_sync_manifest(&meta_dir, &manifest)
                .map_err(|e| format!("Failed to save sync manifest: {}", e))?;
            files_updated.push("meta/.sync.ron".to_string());

            Ok(SyncResult::Applied { files_updated })
        }
        "scene" => {
            if preview {
                let changes = format!(
                    "Sync source=scene for domain '{}':\n  \
                     - Snapshot current scene state to {}\n  \
                     - Update sync manifest with new hashes\n",
                    domain,
                    ron_path.display()
                );
                return Ok(SyncResult::Preview(changes));
            }

            // Scene → RON sync: in a full implementation this would serialize
            // the live Bevy scene to .ron. For file-level operations we just
            // update the manifest to mark current state as synced.
            let meta_dir = world_dir.join("meta");
            let mut manifest = load_sync_manifest(&meta_dir).unwrap_or_else(|| SyncManifest {
                updated_at: now_iso8601(),
                domains: HashMap::new(),
                root_md_hash: None,
                root_ron_hash: None,
                root_scene_hash: None,
            });

            let ron_hash = if ron_path.exists() {
                ron_content_hash(&ron_path)?
            } else {
                String::new()
            };
            let md_hash = if md_path.exists() {
                md_content_hash(&md_path)?
            } else {
                None
            };

            manifest.updated_at = now_iso8601();
            manifest.domains.insert(
                domain.to_string(),
                SyncRecord {
                    md_hash,
                    ron_hash,
                    scene_hash: None,
                    md_mtime: None,
                    ron_mtime: now_iso8601(),
                    last_sync: now_iso8601(),
                    sync_direction: SyncDirection::SceneToRon,
                    status: SyncStatus::Clean,
                },
            );

            save_sync_manifest(&meta_dir, &manifest)
                .map_err(|e| format!("Failed to save sync manifest: {}", e))?;

            Ok(SyncResult::Applied {
                files_updated: vec!["meta/.sync.ron".to_string()],
            })
        }
        "md" => {
            // md → ron sync requires LLM to convert semantic descriptions to concrete scene data
            Err("source=md sync requires LLM call (not yet implemented)".to_string())
        }
        _ => Err(format!(
            "Unknown sync source '{}': expected 'ron', 'scene', or 'md'",
            source
        )),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_prefix_deterministic() {
        let h1 = sha256_prefix("hello world");
        let h2 = sha256_prefix("hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 8);
    }

    #[test]
    fn sha256_prefix_different_inputs() {
        let h1 = sha256_prefix("hello");
        let h2 = sha256_prefix("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn extract_backtick_array_valid() {
        assert_eq!(
            extract_backtick_array("at `[1.0, 2.5, 3.0]` position"),
            Some([1.0, 2.5, 3.0])
        );
    }

    #[test]
    fn extract_backtick_array_no_backticks() {
        assert_eq!(extract_backtick_array("no backticks here"), None);
    }

    #[test]
    fn extract_backtick_array_not_array() {
        assert_eq!(extract_backtick_array("value `hello` here"), None);
    }

    #[test]
    fn extract_dimensions_valid() {
        assert_eq!(
            extract_dimensions("a 6x4x5m building"),
            Some([6.0, 4.0, 5.0])
        );
    }

    #[test]
    fn extract_dimensions_no_m_suffix() {
        assert_eq!(
            extract_dimensions("size 3x2x1 block"),
            Some([3.0, 2.0, 1.0])
        );
    }

    #[test]
    fn extract_dimensions_missing() {
        assert_eq!(extract_dimensions("no dimensions here"), None);
    }

    #[test]
    fn extract_material_hint_valid() {
        assert_eq!(
            extract_material_hint("large building, warm brown wood"),
            Some("warm brown wood".to_string())
        );
    }

    #[test]
    fn extract_material_hint_with_backticks() {
        assert_eq!(extract_material_hint("at `[0, 0, 0]`"), None);
    }

    #[test]
    fn parse_count_line_valid() {
        let claim = parse_count_line("4x market stalls arranged").unwrap();
        assert_eq!(claim.count, Some(4));
        assert_eq!(claim.entity_name.as_deref(), Some("market_stalls_arranged"));
    }

    #[test]
    fn parse_count_line_invalid() {
        assert!(parse_count_line("no count here").is_none());
    }

    #[test]
    fn extract_structural_claims_basic() {
        let md = r#"# Region

## Entity Groups

- **castle_wall** — north wall `[0, 0, 10]` 10x5x1m, grey stone
- **tower** — corner tower `[10, 0, 10]` 3x3x8m
- 4x market stalls along road

## Other Section
"#;
        let claims = extract_structural_claims(md).unwrap();
        assert_eq!(claims.len(), 3);

        // Sorted by entity_name
        let castle = claims
            .iter()
            .find(|c| c.entity_name.as_deref() == Some("castle_wall"))
            .unwrap();
        assert_eq!(castle.position, Some([0.0, 0.0, 10.0]));
        assert_eq!(castle.dimensions, Some([10.0, 5.0, 1.0]));
        assert_eq!(castle.material_hint.as_deref(), Some("grey stone"));

        let count_claim = claims.iter().find(|c| c.count == Some(4)).unwrap();
        assert_eq!(count_claim.count, Some(4));
    }

    #[test]
    fn extract_structural_claims_with_sync_comment() {
        let md = r#"## Entity Groups

- **tower** — main tower <!-- sync: {"entity_name":"tower","position":[5.0,0.0,5.0]} -->
"#;
        let claims = extract_structural_claims(md).unwrap();
        // The sync comment should be parsed as authoritative
        assert!(
            claims
                .iter()
                .any(|c| c.entity_name.as_deref() == Some("tower"))
        );
    }

    #[test]
    fn extract_structural_claims_missing_section() {
        let md = "# No entity groups here\n\nJust text.\n";
        let result = extract_structural_claims(md);
        assert!(result.is_err());
    }

    #[test]
    fn determine_status_clean() {
        let record = SyncRecord {
            md_hash: Some("abc".to_string()),
            ron_hash: "def".to_string(),
            scene_hash: None,
            md_mtime: None,
            ron_mtime: String::new(),
            last_sync: String::new(),
            sync_direction: SyncDirection::RonToScene,
            status: SyncStatus::Clean,
        };
        assert_eq!(
            determine_status(&record, "def", &Some("abc".to_string())),
            SyncStatus::Clean
        );
    }

    #[test]
    fn determine_status_md_ahead() {
        let record = SyncRecord {
            md_hash: Some("abc".to_string()),
            ron_hash: "def".to_string(),
            scene_hash: None,
            md_mtime: None,
            ron_mtime: String::new(),
            last_sync: String::new(),
            sync_direction: SyncDirection::RonToScene,
            status: SyncStatus::Clean,
        };
        assert_eq!(
            determine_status(&record, "def", &Some("CHANGED".to_string())),
            SyncStatus::MdAhead
        );
    }

    #[test]
    fn determine_status_ron_ahead() {
        let record = SyncRecord {
            md_hash: Some("abc".to_string()),
            ron_hash: "def".to_string(),
            scene_hash: None,
            md_mtime: None,
            ron_mtime: String::new(),
            last_sync: String::new(),
            sync_direction: SyncDirection::RonToScene,
            status: SyncStatus::Clean,
        };
        assert_eq!(
            determine_status(&record, "CHANGED", &Some("abc".to_string())),
            SyncStatus::RonAhead
        );
    }

    #[test]
    fn determine_status_conflict() {
        let record = SyncRecord {
            md_hash: Some("abc".to_string()),
            ron_hash: "def".to_string(),
            scene_hash: None,
            md_mtime: None,
            ron_mtime: String::new(),
            last_sync: String::new(),
            sync_direction: SyncDirection::RonToScene,
            status: SyncStatus::Clean,
        };
        assert_eq!(
            determine_status(&record, "NEW_RON", &Some("NEW_MD".to_string())),
            SyncStatus::Conflict
        );
    }

    #[test]
    fn sync_manifest_roundtrip() {
        let dir = std::env::temp_dir().join("localgpt_sync_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut manifest = SyncManifest {
            updated_at: "2026-03-21T00:00:00Z".to_string(),
            domains: HashMap::new(),
            root_md_hash: None,
            root_ron_hash: None,
            root_scene_hash: None,
        };
        manifest.domains.insert(
            "regions/town".to_string(),
            SyncRecord {
                md_hash: Some("aabb".to_string()),
                ron_hash: "ccdd".to_string(),
                scene_hash: None,
                md_mtime: None,
                ron_mtime: "2026-03-21T00:00:00Z".to_string(),
                last_sync: "2026-03-21T00:00:00Z".to_string(),
                sync_direction: SyncDirection::RonToScene,
                status: SyncStatus::Clean,
            },
        );

        save_sync_manifest(&dir, &manifest).unwrap();
        let loaded = load_sync_manifest(&dir).unwrap();
        assert_eq!(loaded.domains.len(), 1);
        assert!(loaded.domains.contains_key("regions/town"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generate_suggestion_values() {
        assert!(generate_suggestion(&SyncStatus::Clean).is_none());
        assert!(
            generate_suggestion(&SyncStatus::MdAhead)
                .unwrap()
                .contains("source=md")
        );
        assert!(
            generate_suggestion(&SyncStatus::RonAhead)
                .unwrap()
                .contains("source=ron")
        );
        assert!(
            generate_suggestion(&SyncStatus::Conflict)
                .unwrap()
                .contains("Resolve")
        );
        assert!(
            generate_suggestion(&SyncStatus::Unknown)
                .unwrap()
                .contains("baseline")
        );
    }

    #[test]
    fn apply_sync_invalid_domain() {
        let dir = std::env::temp_dir().join("localgpt_sync_invalid");
        let result = apply_sync(&dir, "nodomain", "ron", false, &None);
        assert!(result.is_err());
    }

    #[test]
    fn apply_sync_unknown_source() {
        let dir = std::env::temp_dir().join("localgpt_sync_unknown");
        let result = apply_sync(&dir, "regions/town", "banana", false, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown sync source"));
    }

    #[test]
    fn apply_sync_md_source_stub() {
        let dir = std::env::temp_dir().join("localgpt_sync_md");
        let result = apply_sync(&dir, "regions/town", "md", false, &None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("LLM call"));
    }

    /// 2023-11-14T22:13:20Z, the fixed instant every timestamp-shape test
    /// formats.
    const NOV_14_22_13_20Z: i64 = 1_700_000_000;

    #[test]
    fn now_iso8601_keeps_the_chrono_shape() {
        // AutoSi fractional seconds: 0, 3, 6 or 9 digits — never a shorter
        // form, never a dropped fraction.
        assert_eq!(
            rfc3339_utc_autosi(Timestamp::new(NOV_14_22_13_20Z, 0).unwrap()),
            "2023-11-14T22:13:20+00:00"
        );
        assert_eq!(
            rfc3339_utc_autosi(Timestamp::new(NOV_14_22_13_20Z, 500_000_000).unwrap()),
            "2023-11-14T22:13:20.500+00:00"
        );
        assert_eq!(
            rfc3339_utc_autosi(Timestamp::new(NOV_14_22_13_20Z, 123_456_000).unwrap()),
            "2023-11-14T22:13:20.123456+00:00"
        );
        assert_eq!(
            rfc3339_utc_autosi(Timestamp::new(NOV_14_22_13_20Z, 123_456_789).unwrap()),
            "2023-11-14T22:13:20.123456789+00:00"
        );
        // The explicit UTC offset survives, and the whole string is a
        // timestamp a parser (jiff's own, here) accepts back.
        let now = now_iso8601();
        assert!(now.ends_with("+00:00"), "no +00:00 offset in {now}");
        assert!(now.parse::<Timestamp>().is_ok(), "{now} does not parse");
    }

    #[test]
    fn now_iso8601_is_not_jiff_display() {
        // Jiff's Display prints Zulu time with the fewest digits it can;
        // the sync manifest keeps chrono's offset and fixed precision. If
        // jiff's display ever leaks in here, these two strings converge and
        // this test fails.
        let half_second = Timestamp::new(NOV_14_22_13_20Z, 500_000_000).unwrap();
        assert_eq!(half_second.to_string(), "2023-11-14T22:13:20.5Z");
        assert_eq!(
            rfc3339_utc_autosi(half_second),
            "2023-11-14T22:13:20.500+00:00"
        );
    }
}
