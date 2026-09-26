//! # localgpt-world-types
//!
//! Unified world data model for LocalGPT Gen.
//!
//! This crate defines the canonical types for 3D worlds: entities, shapes,
//! materials, lights, behaviors, audio, tours, and more.  It has **zero
//! dependencies on Bevy or SpacetimeDB** — only `serde` and `serde_json`.
//!
//! The same types serialize to RON files for local saves and can be mapped
//! to SpacetimeDB rows for multiplayer via a thin adapter layer.
//!
//! ## Core Types
//!
//! - [`WorldManifest`] — top-level world definition with schema versioning
//! - [`WorldEntity`] — composable entity with optional shape, material,
//!   light, behaviors, and audio components
//! - [`Shape`] — parametric shapes that never lose dimension info
//! - [`BehaviorDef`] — all 7 declarative animation types
//! - [`AudioDef`] + [`AudioSource`] — unified audio taxonomy
//! - [`EditHistory`] — undo/redo support
//!
//! ## Design Principles
//!
//! 1. **Parametric shapes** — `Shape::Cuboid { x: 4, y: 3, z: 5 }` never
//!    degrades to raw triangles
//! 2. **Composable entities** — a campfire is one entity with shape + light +
//!    audio + pulse behavior
//! 3. **Dual identity** — stable `EntityId` for storage, human-readable
//!    `EntityName` for LLM interaction
//! 4. **Serde-only** — compiles for native, WASM, iOS, Android

pub mod asset;
pub mod audio;
pub mod avatar;
pub mod behavior;
pub mod creation;
pub mod entity;
pub mod genlog;
pub mod history;
pub mod identity;
pub mod library;
pub mod light;
pub mod material;
pub mod modulation;
pub mod npc;
pub mod region;
pub mod shape;
pub mod soundtrack;
pub mod spatial;
pub mod sync;
pub mod tour;
pub mod validation;
pub mod world;

// Re-exports for convenience
pub use asset::MeshAssetRef;
pub use audio::{AudioDef, AudioKind, AudioSource, FilterType, Rolloff, WaveformType};
pub use avatar::{AvatarDef, PointOfView};
pub use behavior::{BehaviorDef, PathMode};
pub use creation::{CreationDef, SemanticCategory};
pub use entity::{EntityPatch, WorldEntity, WorldTransform};
pub use history::{AmbienceLayerDef, EditHistory, EditOp, WorldEdit};
pub use identity::{CreationId, EntityId, EntityName, EntityRef};
pub use light::{LightDef, LightType};
pub use material::{AlphaModeDef, MaterialDef, TextureSlot};
pub use modulation::{ModulationDef, ModulationTarget, SignalSource, StemKind};
pub use npc::{NpcBrainDef, NpcDataCollection, NpcDef, NpcMemoryDef, NpcMemoryEntryDef};
pub use shape::{PrimitiveShapeKind, Shape};
pub use soundtrack::{SoundtrackDef, StemCurves, curve_at};
pub use spatial::ChunkCoord;
pub use tour::{TourDef, TourMode, TourWaypoint};
pub use validation::{
    Severity, ValidationIssue, WorldLimits, validate_entities, validate_manifest,
};
pub use world::{CameraDef, ComplianceMeta, EnvironmentDef, WorldManifest, WorldMeta};

// Multi-file world types (v2)
pub use genlog::GenLogEntry;
pub use library::{AudioEmitterSpec, AudioSpec, BehaviorLibrary};
pub use region::{RegionBounds, RegionEntities};
pub use sync::{
    ClaimExtractionError, DiffType, DomainDrift, DriftReport, StructuralClaim, StructuralDiff,
    SyncDirection, SyncManifest, SyncRecord, SyncStatus,
};
