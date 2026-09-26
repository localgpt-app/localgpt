//! World manifest — the top-level world definition.
//!
//! Schema-versioned and designed for RON serialization.  Small worlds
//! store entities inline; large worlds split into per-chunk files.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::avatar::AvatarDef;
use crate::creation::CreationDef;
use crate::entity::WorldEntity;
use crate::soundtrack::SoundtrackDef;
use crate::tour::TourDef;

/// Current schema version. Increment when making breaking changes.
pub const WORLD_SCHEMA_VERSION: u32 = 2;

/// Minimum supported version for loading. Update when dropping old format support.
pub const MIN_SUPPORTED_VERSION: u32 = 1;

/// Version compatibility error.
#[derive(Debug, Clone)]
pub enum VersionError {
    /// World file is too old to load.
    TooOld { found: u32, min: u32 },
    /// World file is from a newer version of the software.
    TooNew { found: u32, current: u32 },
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionError::TooOld { found, min } => {
                write!(
                    f,
                    "version {} is too old (minimum supported: {})",
                    found, min
                )
            }
            VersionError::TooNew { found, current } => {
                write!(
                    f,
                    "version {} is from a newer localgpt-gen (current: {})",
                    found, current
                )
            }
        }
    }
}

/// Top-level world manifest — everything needed to save/load a world.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorldManifest {
    /// Schema version for forward/backward migration.
    #[serde(default = "default_version")]
    pub version: u32,
    /// World metadata (name, description, biome, etc.).
    pub meta: WorldMeta,
    /// Environment settings (background, ambient light, fog).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<EnvironmentDef>,
    /// Default camera position/orientation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraDef>,
    /// Avatar configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar: Option<AvatarDef>,
    /// Guided tours.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tours: Vec<TourDef>,
    /// The song this world performs to, with the analysis that drives
    /// entity modulations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soundtrack: Option<SoundtrackDef>,
    // ---- Multi-file references (v2) ----
    /// Path to a separate layout file (blockout regions, spatial graph).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout_file: Option<String>,
    /// Paths to per-region entity files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_files: Option<Vec<String>>,
    /// Paths to behavior library files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behavior_files: Option<Vec<String>>,
    /// Paths to audio spec files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_files: Option<Vec<String>>,
    /// Path to an avatar definition file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_file: Option<String>,

    /// Entities (inline for small worlds).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<WorldEntity>,
    /// Compound creations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub creations: Vec<CreationDef>,
    /// Next entity ID to allocate.
    #[serde(default = "default_next_id")]
    pub next_entity_id: u64,
}

/// World metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorldMeta {
    /// World name (used as skill name / directory name).
    pub name: String,
    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Biome hint for procedural generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub biome: Option<String>,
    /// Time of day (0.0-24.0, for lighting presets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_of_day: Option<f32>,

    // --- Gallery & experiment metadata (added for headless pipeline) ---
    /// Free-form style tags for gallery filtering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    /// Generation source: "interactive", "headless", "experiment", "mcp".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// If part of a variation experiment, the group ID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_group: Option<String>,
    /// Variation axis and value (e.g., ("lighting", "sunset")).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation: Option<(String, String)>,
    /// Original prompt used to generate this world.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// LLM model used for generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Generation duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_duration_ms: Option<u64>,
    /// Style name from memory (if applied).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_ref: Option<String>,
    /// Bevy engine version targeted by this world's generated code.
    #[serde(
        default = "default_bevy_version",
        skip_serializing_if = "Option::is_none"
    )]
    pub bevy_version: Option<String>,

    // --- Regulatory compliance metadata ---
    /// Compliance metadata for distribution and regulatory classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compliance: Option<ComplianceMeta>,
}

/// Regulatory and distribution compliance metadata.
///
/// Records classification signals for storefronts, regulatory frameworks,
/// and content-origin seals so that exported worlds carry machine-readable
/// provenance alongside the creative data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComplianceMeta {
    /// Steam "code tool" exemption flag.
    ///
    /// `true` indicates the output is compilable/editable source code (RON scene
    /// definitions, parametric shapes) rather than pre-made binary assets.  Under
    /// Valve's AI content policy, tools whose output is code that the developer
    /// compiles or modifies are treated as code tools, not AI-generated asset
    /// generators.
    #[serde(default = "default_true")]
    pub steam_code_tool_exempt: bool,

    /// EU AI Act risk-level classification.
    ///
    /// LocalGPT Gen is a code-generation tool: the LLM produces scene definition
    /// code (RON) that the user compiles into 3D geometry via Bevy.  Under the
    /// EU AI Act (Regulation 2024/1689), general-purpose code-generation tools
    /// with a human in the loop fall under the "minimal risk" tier, requiring
    /// only transparency obligations (Art. 52) -- no conformity assessment.
    #[serde(default = "default_risk_level")]
    pub eu_ai_act_risk_level: String,

    /// "No Gen AI" seal compatibility flag.
    ///
    /// `true` indicates the output is human-editable source code (not opaque
    /// binary blobs) and the creative direction is human-driven.  The scene
    /// definition can be fully read, understood, and modified by a person,
    /// making the output compatible with "No Gen AI" asset provenance
    /// requirements that focus on human authorship of the final artifact.
    #[serde(default = "default_true")]
    pub no_gen_ai_compatible: bool,

    /// Tool name and version that produced this world (e.g. "LocalGPT Gen v0.3.5").
    #[serde(default = "default_generation_tool")]
    pub generation_tool: String,

    /// How the output was produced: "code-generation" (LLM writes scene code
    /// compiled by the engine) vs "asset-generation" (LLM directly produces
    /// binary mesh/texture data).
    #[serde(default = "default_generation_method")]
    pub generation_method: String,

    /// Whether the output can be meaningfully edited by a human.
    ///
    /// `true` for LocalGPT Gen because the canonical format is RON text with
    /// parametric shapes -- users can open, read, and modify every dimension,
    /// material, and behavior by hand.
    #[serde(default = "default_true")]
    pub human_modifiable: bool,
}

/// Environment settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EnvironmentDef {
    /// Background/sky color, sRGB-encoded RGBA in `0..=1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<[f32; 4]>,
    /// Ambient light brightness in Bevy's `GlobalAmbientLight` units
    /// (default 80; the web viewer scales it with `AMBIENT_SCALE`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambient_intensity: Option<f32>,
    /// Ambient light color, sRGB-encoded RGBA in `0..=1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ambient_color: Option<[f32; 4]>,
    /// Exponential fog density (0.0 = no fog).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fog_density: Option<f32>,
    /// Fog color, sRGB-encoded RGBA in `0..=1`; the background color when
    /// unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fog_color: Option<[f32; 4]>,
}

/// Camera definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CameraDef {
    /// Camera position [x, y, z].
    #[serde(default = "default_camera_pos")]
    pub position: [f32; 3],
    /// Camera look-at target [x, y, z].
    #[serde(default)]
    pub look_at: [f32; 3],
    /// Vertical field of view in degrees.
    #[serde(default = "default_fov")]
    pub fov_degrees: f32,
}

impl Default for CameraDef {
    fn default() -> Self {
        Self {
            position: default_camera_pos(),
            look_at: [0.0, 0.0, 0.0],
            fov_degrees: default_fov(),
        }
    }
}

fn default_version() -> u32 {
    WORLD_SCHEMA_VERSION
}
fn default_next_id() -> u64 {
    1
}
fn default_camera_pos() -> [f32; 3] {
    [5.0, 5.0, 5.0]
}
fn default_fov() -> f32 {
    45.0
}
fn default_bevy_version() -> Option<String> {
    Some("0.18".to_string())
}
fn default_true() -> bool {
    true
}
fn default_risk_level() -> String {
    "minimal".to_string()
}
fn default_generation_tool() -> String {
    format!("LocalGPT Gen v{}", env!("CARGO_PKG_VERSION"))
}
fn default_generation_method() -> String {
    "code-generation".to_string()
}

impl Default for ComplianceMeta {
    fn default() -> Self {
        Self {
            steam_code_tool_exempt: true,
            eu_ai_act_risk_level: default_risk_level(),
            no_gen_ai_compatible: true,
            generation_tool: default_generation_tool(),
            generation_method: default_generation_method(),
            human_modifiable: true,
        }
    }
}

impl WorldManifest {
    /// Create a new empty world with a given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: default_version(),
            meta: WorldMeta {
                name: name.into(),
                description: None,
                biome: None,
                time_of_day: None,
                tags: None,
                source: None,
                variation_group: None,
                variation: None,
                prompt: None,
                model: None,
                generation_duration_ms: None,
                style_ref: None,
                bevy_version: default_bevy_version(),
                compliance: Some(ComplianceMeta::default()),
            },
            environment: None,
            camera: None,
            avatar: None,
            layout_file: None,
            region_files: None,
            behavior_files: None,
            audio_files: None,
            avatar_file: None,
            tours: Vec::new(),
            soundtrack: None,
            entities: Vec::new(),
            creations: Vec::new(),
            next_entity_id: default_next_id(),
        }
    }

    /// Check if this manifest's version is compatible with current code.
    pub fn check_version(&self) -> Result<(), VersionError> {
        if self.version < MIN_SUPPORTED_VERSION {
            Err(VersionError::TooOld {
                found: self.version,
                min: MIN_SUPPORTED_VERSION,
            })
        } else if self.version > WORLD_SCHEMA_VERSION {
            Err(VersionError::TooNew {
                found: self.version,
                current: WORLD_SCHEMA_VERSION,
            })
        } else {
            Ok(())
        }
    }

    /// Allocate and return the next entity ID, incrementing the counter.
    pub fn alloc_entity_id(&mut self) -> crate::identity::EntityId {
        let id = crate::identity::EntityId(self.next_entity_id);
        self.next_entity_id += 1;
        id
    }

    /// Total entity count.
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Total triangle budget estimate.
    pub fn estimate_triangles(&self) -> usize {
        self.entities
            .iter()
            .filter_map(|e| e.shape.as_ref())
            .map(|s| s.estimate_triangles())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::WorldEntity;
    use crate::shape::Shape;

    #[test]
    fn manifest_new() {
        let m = WorldManifest::new("test_world");
        assert_eq!(m.meta.name, "test_world");
        assert_eq!(m.version, WORLD_SCHEMA_VERSION);
        assert_eq!(m.next_entity_id, 1);
        assert!(m.entities.is_empty());
    }

    #[test]
    fn alloc_entity_id() {
        let mut m = WorldManifest::new("test");
        let id1 = m.alloc_entity_id();
        let id2 = m.alloc_entity_id();
        assert_eq!(id1.0, 1);
        assert_eq!(id2.0, 2);
        assert_eq!(m.next_entity_id, 3);
    }

    #[test]
    fn manifest_roundtrip_json() {
        let mut m = WorldManifest::new("roundtrip_test");
        m.meta.description = Some("A test world".to_string());
        m.environment = Some(EnvironmentDef {
            background_color: Some([0.1, 0.1, 0.2, 1.0]),
            ambient_intensity: Some(0.3),
            ambient_color: None,
            fog_density: None,
            fog_color: None,
        });
        m.camera = Some(CameraDef::default());
        m.avatar = Some(AvatarDef::default());
        m.entities
            .push(WorldEntity::new(1, "cube").with_shape(Shape::Cuboid {
                x: 2.0,
                y: 2.0,
                z: 2.0,
            }));
        m.next_entity_id = 2;

        let json = serde_json::to_string_pretty(&m).unwrap();
        let back: WorldManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn triangle_estimate() {
        let mut m = WorldManifest::new("budget_test");
        m.entities
            .push(WorldEntity::new(1, "cube").with_shape(Shape::Cuboid {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            }));
        m.entities
            .push(WorldEntity::new(2, "sphere").with_shape(Shape::Sphere { radius: 1.0 }));
        assert!(m.estimate_triangles() > 0);
    }

    #[test]
    fn version_check_current() {
        let m = WorldManifest::new("test");
        assert!(m.check_version().is_ok());
    }

    #[test]
    fn version_check_too_old() {
        let mut m = WorldManifest::new("test");
        m.version = 0; // Below MIN_SUPPORTED_VERSION
        let err = m.check_version().unwrap_err();
        match err {
            VersionError::TooOld { found, min } => {
                assert_eq!(found, 0);
                assert_eq!(min, MIN_SUPPORTED_VERSION);
            }
            _ => panic!("Expected TooOld error"),
        }
    }

    #[test]
    fn version_check_too_new() {
        let mut m = WorldManifest::new("test");
        m.version = 99; // Above WORLD_SCHEMA_VERSION
        let err = m.check_version().unwrap_err();
        match err {
            VersionError::TooNew { found, current } => {
                assert_eq!(found, 99);
                assert_eq!(current, WORLD_SCHEMA_VERSION);
            }
            _ => panic!("Expected TooNew error"),
        }
    }

    #[test]
    fn compliance_meta_default() {
        let c = ComplianceMeta::default();
        assert!(c.steam_code_tool_exempt);
        assert_eq!(c.eu_ai_act_risk_level, "minimal");
        assert!(c.no_gen_ai_compatible);
        assert!(c.generation_tool.starts_with("LocalGPT Gen v"));
        assert_eq!(c.generation_method, "code-generation");
        assert!(c.human_modifiable);
    }

    #[test]
    fn compliance_roundtrip_json() {
        let c = ComplianceMeta::default();
        let json = serde_json::to_string_pretty(&c).unwrap();
        let back: ComplianceMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn manifest_new_includes_compliance() {
        let m = WorldManifest::new("compliance_test");
        assert!(m.meta.compliance.is_some());
        let c = m.meta.compliance.unwrap();
        assert!(c.steam_code_tool_exempt);
        assert_eq!(c.eu_ai_act_risk_level, "minimal");
        assert!(c.no_gen_ai_compatible);
        assert!(c.human_modifiable);
        assert_eq!(c.generation_method, "code-generation");
    }
}
