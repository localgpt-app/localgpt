//! Sync and drift detection types for three-way consistency.
//!
//! Tracks hashes and timestamps across `.md`, `.ron`, and live scene
//! representations, detecting when any layer drifts out of sync.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Top-level sync manifest — tracks consistency state across all domains.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SyncManifest {
    /// ISO 8601 timestamp of last sync check.
    pub updated_at: String,
    /// Per-domain sync records keyed by domain name.
    pub domains: HashMap<String, SyncRecord>,
    /// Hash of the root markdown file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_md_hash: Option<String>,
    /// Hash of the root RON file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_ron_hash: Option<String>,
    /// Hash of the live scene state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root_scene_hash: Option<String>,
}

/// Sync state for a single domain (e.g., "layout", "audio", "behaviors").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SyncRecord {
    /// Hash of the markdown representation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md_hash: Option<String>,
    /// Hash of the RON representation.
    pub ron_hash: String,
    /// Hash of the live scene representation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_hash: Option<String>,
    /// Last-modified time of the markdown file (ISO 8601).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md_mtime: Option<String>,
    /// Last-modified time of the RON file (ISO 8601).
    pub ron_mtime: String,
    /// ISO 8601 timestamp of last successful sync.
    pub last_sync: String,
    /// Direction of the last sync operation.
    pub sync_direction: SyncDirection,
    /// Current sync status.
    pub status: SyncStatus,
}

/// Sync status for a domain or overall.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SyncStatus {
    /// All representations match.
    Clean,
    /// Markdown has changes not yet in RON/scene.
    MdAhead,
    /// RON has changes not yet in markdown/scene.
    RonAhead,
    /// Scene has changes not yet in markdown/RON.
    SceneAhead,
    /// Multiple representations changed — manual resolution needed.
    Conflict,
    /// Status could not be determined.
    Unknown,
}

/// Direction of a sync operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum SyncDirection {
    MdToRon,
    RonToScene,
    SceneToRon,
    MdToRonToScene,
    SceneToRonToMd,
}

/// Summary report of drift across all domains.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DriftReport {
    /// Overall sync status (worst of all domains).
    pub overall_status: SyncStatus,
    /// Per-domain drift details.
    pub domains: Vec<DomainDrift>,
}

/// Drift details for a single domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DomainDrift {
    /// Domain name (e.g., "layout", "entities", "audio").
    pub domain: String,
    /// Sync status for this domain.
    pub status: SyncStatus,
    /// Human-readable detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Specific structural differences found.
    #[serde(default)]
    pub structural_diffs: Vec<StructuralDiff>,
    /// Suggested resolution action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

/// A single structural difference between representations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StructuralDiff {
    /// Entity name involved (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    /// Type of difference.
    pub diff_type: DiffType,
    /// Field that differs (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Value in the markdown representation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub md_value: Option<serde_json::Value>,
    /// Value in the RON representation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ron_value: Option<serde_json::Value>,
    /// Value in the live scene.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_value: Option<serde_json::Value>,
}

/// Type of structural difference.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DiffType {
    Added,
    Removed,
    Modified,
}

/// A claim extracted from markdown about world structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StructuralClaim {
    /// Entity name mentioned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_name: Option<String>,
    /// Claimed entity count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Claimed position [x, y, z].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    /// Claimed dimensions [x, y, z].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<[f32; 3]>,
    /// Tier hint (e.g., "hero", "medium", "decorative").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// Material hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material_hint: Option<String>,
    /// Behavior hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior_hint: Option<String>,
}

/// Error when extracting structural claims from markdown.
#[derive(Debug, Clone)]
pub enum ClaimExtractionError {
    /// No entity groups section found.
    MissingEntityGroups,
    /// Entity groups section is malformed.
    MalformedEntityGroups(String),
}

impl std::fmt::Display for ClaimExtractionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaimExtractionError::MissingEntityGroups => {
                write!(f, "missing entity groups section in markdown")
            }
            ClaimExtractionError::MalformedEntityGroups(detail) => {
                write!(f, "malformed entity groups: {}", detail)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_sync_manifest() -> SyncManifest {
        let mut domains = HashMap::new();
        domains.insert(
            "layout".to_string(),
            SyncRecord {
                md_hash: Some("abc123".to_string()),
                ron_hash: "def456".to_string(),
                scene_hash: None,
                md_mtime: Some("2026-03-21T10:00:00Z".to_string()),
                ron_mtime: "2026-03-21T10:01:00Z".to_string(),
                last_sync: "2026-03-21T10:01:00Z".to_string(),
                sync_direction: SyncDirection::MdToRon,
                status: SyncStatus::Clean,
            },
        );
        SyncManifest {
            updated_at: "2026-03-21T10:01:00Z".to_string(),
            domains,
            root_md_hash: Some("root_md".to_string()),
            root_ron_hash: Some("root_ron".to_string()),
            root_scene_hash: None,
        }
    }

    #[test]
    fn sync_manifest_roundtrip_json() {
        let manifest = sample_sync_manifest();
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let back: SyncManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.updated_at, "2026-03-21T10:01:00Z");
        assert!(back.domains.contains_key("layout"));
        assert_eq!(back.domains["layout"].status, SyncStatus::Clean);
    }

    #[test]
    fn sync_manifest_roundtrip_ron() {
        let manifest = sample_sync_manifest();
        let ron_str = ron::to_string(&manifest).unwrap();
        let back: SyncManifest = ron::from_str(&ron_str).unwrap();
        assert_eq!(back.updated_at, manifest.updated_at);
    }

    #[test]
    fn drift_report_roundtrip_json() {
        let report = DriftReport {
            overall_status: SyncStatus::Conflict,
            domains: vec![DomainDrift {
                domain: "entities".to_string(),
                status: SyncStatus::MdAhead,
                detail: Some("3 entities added in markdown".to_string()),
                structural_diffs: vec![StructuralDiff {
                    entity: Some("tower".to_string()),
                    diff_type: DiffType::Added,
                    field: None,
                    md_value: Some(serde_json::json!({"name": "tower"})),
                    ron_value: None,
                    scene_value: None,
                }],
                suggestion: Some("Run md-to-ron sync".to_string()),
            }],
        };
        let json = serde_json::to_string_pretty(&report).unwrap();
        let back: DriftReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.overall_status, SyncStatus::Conflict);
        assert_eq!(back.domains.len(), 1);
        assert_eq!(
            back.domains[0].structural_diffs[0].diff_type,
            DiffType::Added
        );
    }

    #[test]
    fn sync_status_all_variants_roundtrip() {
        let statuses = vec![
            SyncStatus::Clean,
            SyncStatus::MdAhead,
            SyncStatus::RonAhead,
            SyncStatus::SceneAhead,
            SyncStatus::Conflict,
            SyncStatus::Unknown,
        ];
        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let back: SyncStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(*status, back);
        }
    }

    #[test]
    fn sync_direction_all_variants_roundtrip() {
        let directions = vec![
            SyncDirection::MdToRon,
            SyncDirection::RonToScene,
            SyncDirection::SceneToRon,
            SyncDirection::MdToRonToScene,
            SyncDirection::SceneToRonToMd,
        ];
        for dir in &directions {
            let json = serde_json::to_string(dir).unwrap();
            let back: SyncDirection = serde_json::from_str(&json).unwrap();
            assert_eq!(*dir, back);
        }
    }

    #[test]
    fn structural_claim_roundtrip_json() {
        let claim = StructuralClaim {
            entity_name: Some("castle_wall".to_string()),
            count: Some(4),
            position: Some([0.0, 0.0, 0.0]),
            dimensions: Some([10.0, 5.0, 1.0]),
            tier: Some("hero".to_string()),
            material_hint: Some("stone".to_string()),
            behavior_hint: None,
        };
        let json = serde_json::to_string_pretty(&claim).unwrap();
        let back: StructuralClaim = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entity_name.as_deref(), Some("castle_wall"));
        assert_eq!(back.count, Some(4));
        assert_eq!(back.tier.as_deref(), Some("hero"));
    }

    #[test]
    fn diff_type_all_variants_roundtrip() {
        for dt in [DiffType::Added, DiffType::Removed, DiffType::Modified] {
            let json = serde_json::to_string(&dt).unwrap();
            let back: DiffType = serde_json::from_str(&json).unwrap();
            assert_eq!(dt, back);
        }
    }

    #[test]
    fn claim_extraction_error_display() {
        let err = ClaimExtractionError::MissingEntityGroups;
        assert!(err.to_string().contains("missing entity groups"));

        let err = ClaimExtractionError::MalformedEntityGroups("bad format".to_string());
        assert!(err.to_string().contains("bad format"));
    }
}
