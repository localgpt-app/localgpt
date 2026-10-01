//! # localgpt-world-types
//!
//! The format's document types, re-exported from the `openworldformat`
//! crate — the Rust reference extracted from this very code and now
//! published on crates.io. What stays here is LocalGPT's own surface:
//! NPCs, the generation log, behavior libraries, regions, and the sync
//! drift machinery — the app-flavored leaves that were never part of
//! the format's schema.
//!
//! Downstream crates see no difference: `wt::Shape`, `wt::entity::WorldEntity`,
//! `wt::WorldManifest` all resolve, now to the crate's types. When the
//! format changes, the crate releases first and this workspace bumps —
//! the spec leads, the apps follow, which is the point of the spin-out.
//!
//! The same types serialize to RON for local saves and to JSON for the
//! web viewer; `schema_for!(WorldManifest)` (feature `schema`) still
//! generates `world.schema.json`, byte-identical to the published one.

// The format's document: modules and the flat re-export surface, both
// from the crate, so module paths (`wt::entity::…`) and root paths
// (`wt::WorldEntity`) keep working.
pub use openworldformat::validation::{
    Severity, ValidationIssue, WorldLimits, validate_entities, validate_manifest,
};
pub use openworldformat::world::WORLD_SCHEMA_VERSION;
pub use openworldformat::{
    AlphaModeDef, AmbienceLayerDef, AudioDef, AudioKind, AudioSource, AvatarDef, BehaviorDef,
    CameraDef, ChunkCoord, ComplianceMeta, CreationDef, CreationId, EditHistory, EditOp, EntityId,
    EntityName, EntityPatch, EntityRef, EnvironmentDef, FilterType, InstanceOf, LightDef,
    LightType, MaterialDef, MeshAssetRef, ModulationDef, ModulationTarget, NodeOverride, PartLink,
    PartOverride, PathMode, PointOfView, PrimitiveShapeKind, Rolloff, SemanticCategory, Shape,
    SignalSource, SoundtrackDef, StemCurves, StemKind, TextureSlot, TourDef, TourMode,
    TourWaypoint, TriggerActionDef, TriggerDef, TriggerEvent, TriggerVolume, WaveformType,
    WorldEdit, WorldEntity, WorldManifest, WorldMeta, WorldTransform, curve_at, expand_instances,
    part_links, validate_instances, values_close,
};
pub use openworldformat::{
    asset, audio, avatar, behavior, creation, entity, history, identity, instance, light, material,
    modulation, shape, soundtrack, spatial, tour, trigger, validation, world,
};

// LocalGPT's own leaves — never part of the format's schema.
pub mod genlog;
pub mod library;
pub mod npc;
pub mod region;
pub mod sync;

pub use genlog::GenLogEntry;
pub use library::{AudioEmitterSpec, AudioSpec, BehaviorLibrary};
pub use npc::{NpcBrainDef, NpcDataCollection, NpcDef, NpcMemoryDef, NpcMemoryEntryDef};
pub use region::{RegionBounds, RegionEntities};
pub use sync::{
    ClaimExtractionError, DiffType, DomainDrift, DriftReport, StructuralClaim, StructuralDiff,
    SyncDirection, SyncManifest, SyncRecord, SyncStatus,
};
