# Unified World Data Model: `localgpt-world-types`

**Status:** implemented, then moved upstream. `localgpt-world-types` shipped as
this RFC describes and is still the workspace's world data model — but the
format itself is now the published `openworldformat` crate, which this
workspace consumes. Change the format there, never here.


## Problem

LocalGPT Gen had two representations of 3D worlds that didn't talk to each other:

1. **Current code** (`commands.rs` + `world.rs`): File-based, TOML manifest + glTF scene. Works for local single-player but has critical flaws:
   - glTF export **destroys parametric shape info** — a `Cuboid(4,3,5)` becomes raw triangles with no way to recover the original dimensions
   - No undo/redo
   - No schema versioning (no migration path)
   - Audio, geometry, lights, and behaviors are completely disconnected systems
   - Entity identity uses raw `String` names (fragile, breaks on rename)
   - `PrimitiveShape` + `HashMap<String, f32>` dimensions (stringly typed)

2. **SpacetimeDB RFC** (`docs/world/spacetime/RFC-SpacetimeDB-3D-Audio-Data-Model.md`): Server-authoritative tables for multiplayer. Fixes some issues but creates others:
   - Has proper parametric shapes and undo via `WorldEvent` table
   - **Completely drops all 7 behavior types** (orbit, spin, bob, look_at, pulse, path_follow, bounce — all gone)
   - No environment settings (background color, ambient light, fog)
   - No avatar or tours
   - Uses `shape_data: Vec<u8>` and `graph_json: String` columns — stringly typed, loses type safety
   - Audio model conflates spatial modes with source types

Neither is complete. Both lose information the other preserves.

## Solution: `localgpt-world-types` Crate

A new crate (`crates/world-types/`) that defines the canonical types for 3D worlds. **Zero dependencies on Bevy or SpacetimeDB** — only `serde` and `serde_json`. The same types serialize to RON/JSON files for local saves and can be mapped to SpacetimeDB rows for multiplayer via a thin adapter layer.

### Design Principles

1. **Parametric shapes** — `Shape::Cuboid { x: 4, y: 3, z: 5 }` never degrades to raw triangles
2. **Composable entities** — a campfire is one `WorldEntity` with shape + light + audio + pulse behavior
3. **Dual identity** — stable `EntityId(u64)` for storage, human-readable `EntityName` for LLM interaction
4. **Serde-only** — compiles for native, WASM, iOS, Android
5. **One data model, two backends** — RON files locally, SpacetimeDB rows for multiplayer

---

## What Was Implemented

### Crate Structure (16 modules, 55+ types, 48 tests, 26 gen tools)

```
crates/world-types/src/
  lib.rs           — Re-exports, crate docs
  identity.rs      — EntityId, EntityName, EntityRef, CreationId
  entity.rs        — WorldEntity, WorldTransform, EntityPatch
  shape.rs         — Shape (7 parametric variants), PrimitiveShapeKind
  material.rs      — MaterialDef (PBR properties)
  light.rs         — LightDef, LightType (Directional/Point/Spot)
  behavior.rs      — BehaviorDef (7 types), PathMode
  audio.rs         — AudioDef, AudioSource (14 variants), AudioKind, Rolloff, WaveformType, FilterType
  creation.rs      — CreationDef, SemanticCategory (10 categories)
  spatial.rs       — ChunkCoord (64x64 world units)
  world.rs         — WorldManifest, WorldMeta, EnvironmentDef, CameraDef
  avatar.rs        — AvatarDef, PointOfView
  tour.rs          — TourDef, TourWaypoint, TourMode
  history.rs       — EditHistory, WorldEdit, EditOp (5 variants)
  validation.rs    — WorldLimits, ValidationIssue, validate_entities()
  asset.rs         — MeshAssetRef (for imported glTF)
```

### Key Types

#### WorldEntity — Composable Component Bag

Instead of separate disconnected systems, one entity can have any combination of components:

```rust
pub struct WorldEntity {
    pub id: EntityId,                      // Stable numeric ID
    pub name: EntityName,                  // Human-readable, LLM-friendly
    pub transform: WorldTransform,         // Position, rotation, scale, visibility
    pub parent: Option<EntityId>,          // Hierarchy
    pub chunk: Option<ChunkCoord>,         // Spatial partitioning
    pub creation_id: Option<CreationId>,   // Compound object membership

    // Component slots — all optional, any combination valid
    pub shape: Option<Shape>,              // Parametric (never loses dimensions)
    pub material: Option<MaterialDef>,     // PBR: base_color, metallic, roughness, emissive
    pub light: Option<LightDef>,           // Can coexist with shape (glowing orb)
    pub behaviors: Vec<BehaviorDef>,       // Multiple stack (orbit + spin + bob)
    pub audio: Option<AudioDef>,           // Spatial or ambient
    pub mesh_asset: Option<MeshAssetRef>,  // For imported glTF (alternative to Shape)
}
```

A campfire entity: `shape: Cone` + `light: Point(warm)` + `audio: Fire(crackle)` + `behaviors: [Pulse(flicker)]`. Previously this required coordinating 4 separate systems.

#### Shape — Type-Safe Parametric

```rust
pub enum Shape {
    Cuboid { x: f32, y: f32, z: f32 },
    Sphere { radius: f32 },
    Cylinder { radius: f32, height: f32 },
    Cone { radius: f32, height: f32 },
    Capsule { radius: f32, half_length: f32 },
    Torus { major_radius: f32, minor_radius: f32 },
    Plane { x: f32, z: f32 },
}
```

With `estimate_triangles()` for budget tracking and `local_aabb_half()` for spatial queries.

#### Audio — Unified Taxonomy (14 Sources)

Merged the old `AmbientSound` (6 variants) and `EmitterSound` (5 variants) into one `AudioSource` enum with 14 variants. Spatial vs. global behavior is determined by `radius: Option<f32>` — `None` = fills the scene, `Some(r)` = spatial with distance attenuation.

New from RFC: `Abc { notation }` for ABC music notation, `File { path, looping }` for audio files, `Silence` for muting.

#### Behaviors — All 7 Types Preserved

All 7 declarative animation types from current code preserved verbatim:
- `Orbit` — around entity or fixed point, with tilt and phase
- `Spin` — local axis rotation
- `Bob` — sinusoidal oscillation
- `LookAt` — track target entity
- `Pulse` — scale breathing effect
- `PathFollow` — waypoint path (loop/ping-pong/once)
- `Bounce` — gravity bounce with damping

Entity references upgraded from raw `String` to `EntityRef` (resolves to stable ID).

#### Undo/Redo — Full Edit History

```rust
pub enum EditOp {
    SpawnEntity { entity: WorldEntity },
    DeleteEntity { id: EntityId },
    ModifyEntity { id: EntityId, patch: EntityPatch },
    SetEnvironment { env: EnvironmentDef },
    Batch { ops: Vec<EditOp> },
}
```

`EditHistory` maintains an append-only log with cursor-based undo/redo. Each `WorldEdit` stores both the forward operation and its inverse. New edits truncate redo history.

`EntityPatch` uses `Option<Option<T>>` semantics: `None` = unchanged, `Some(None)` = cleared, `Some(Some(v))` = set to v. This ensures correct undo for every component slot.

#### Validation

Budget limits enforced per-chunk: max 1000 entities, 500K triangles, 200-unit entity extent, 8 behaviors per entity.

### Gen3D Integration (Bevy Adapter Layer)

The gen crate (`crates/gen/src/gen3d/`) was updated to use `world-types` as its persistence layer:

#### Compat Layer (`compat.rs`)

Bidirectional conversion between `world-types` types and gen `commands.rs` types:
- `impl From<&wt::Shape> for (PrimitiveShape, HashMap<String, f32>)` — for Bevy mesh generation
- `impl From<&wt::BehaviorDef> for gen::BehaviorDef` — and reverse
- `impl From<&wt::MaterialDef> for StandardMaterial` — PBR material mapping

#### Save/Load (`world.rs`)

Rewrote save handler to produce `WorldManifest` with inline `WorldEntity` objects. Each entity is snapshotted from Bevy ECS (transform, parametric shape, material, light, visibility). The manifest serializes to JSON (RON planned).

Camera save now reads actual `PerspectiveProjection::fov` instead of hardcoding 45 degrees.

#### Undo System (`plugin.rs`)

`UndoStack` resource wrapping `wt::EditHistory` integrated into the command dispatch loop:

- **Spawn** (primitives, lights, gltf): Records `SpawnEntity(we)` forward / `DeleteEntity(id)` inverse
- **Delete**: Records `DeleteEntity(id)` forward / `SpawnEntity(snapshot)` inverse
- **Modify** (transform, material, visibility): Pre/post snapshot as `Batch { Delete(id) + Spawn(new) }` / `Batch { Delete(id) + Spawn(old) }`
- **Light replace**: Pre-snapshots old light before despawn, records batch undo with old/new states
- **Add/Remove behavior**: Full entity snapshot before and after
- **Set environment**: Captures old `ClearColor`/`AmbientLight` resources, records `SetEnvironment` with old/new state

`apply_edit_op()` replays operations for undo/redo by converting `WorldEntity` back to Bevy ECS components.

#### Entity Info Enrichment

`gen_entity_info` now returns shape type, emissive color, and light data (type, color, intensity, shadows) alongside existing fields. `gen_scene_info` entity summaries include shape type.

#### Dirty Tracking

`DirtyTracker` resource marks entities as dirty on modification. Enables future incremental saves (only write changed entities).

#### Entity ID Allocation

`NextEntityId` resource provides stable `u64` IDs. `GenEntityId` component stores the world-types ID alongside Bevy's transient `Entity`. `EntityRegistry` maps `name <-> Entity <-> EntityId`.

---

## Comparison: What Each System Has

| Feature | Old Code (TOML+glTF) | SpacetimeDB RFC | world-types |
|---|---|---|---|
| Parametric shapes | Stringly typed | Yes (8 types) | Yes (7 types, type-safe) |
| Shape info survives save | No (glTF flattens) | Yes | Yes |
| Behaviors (7 types) | Yes | **No** | Yes |
| Audio (unified) | 2 disconnected enums | Separate table | 14-variant enum |
| ABC notation | No | Yes | Yes |
| Undo/redo | No | Yes (WorldEvent) | Yes (EditHistory) |
| Entity identity | String name only | u64 only | Dual ID + Name |
| Entity references | Raw String | — | EntityRef (Id or Name) |
| Schema versioning | No | No | Yes (version field) |
| Environment settings | Yes (in code) | No | Yes (EnvironmentDef) |
| Avatar | Yes (in TOML) | No | Yes (AvatarDef) |
| Tours | Yes (in TOML) | No | Yes (TourDef) |
| Creations (compound) | No | Yes | Yes (SemanticCategory) |
| Spatial chunking | No | Yes (64x64) | Yes (ChunkCoord) |
| Validation/budgets | No | Yes (rate limiting) | Yes (WorldLimits) |
| Composable entities | No (separate systems) | Partial | Yes (component slots) |
| PBR materials | Yes | Yes (Vec\<u8\>) | Yes (typed MaterialDef) |
| Light types | 3 types | Stored as shape | 3 types (proper LightDef) |
| Bevy dependency | Yes | N/A | **None** |
| WASM compatible | N/A | Partial | Yes |
| Dirty tracking | No | Implicit | Yes (DirtyTracker) |
| Entity patches | No | No | Yes (Option\<Option\<T\>\>) |

---

## What's Done (Commits)

### Session 1 — Core Implementation
1. **`feat(gen): add localgpt-world-types crate with unified world data model`** — Created all 16 modules with 52+ types and 40 tests
2. **`feat(gen): integrate world-types into gen3d save/load with compat layer`** — Compat conversions, WorldManifest save, ParametricShape component
3. **`feat(gen): add undo/redo system with entity ID tracking`** — UndoStack, NextEntityId, GenEntityId, EntityRegistry, DirtyTracker, undo recording for spawn/delete/modify
4. **`refactor(gen): replace string entity refs in behaviors with stable EntityRef`** — Behaviors use EntityId instead of raw String

### Session 2 — Undo Completeness + Polish
5. **`feat(gen): extend undo to lights, behaviors, and environment`** — Pre-snapshot for light replace, behavior add/remove undo, SetEnvironment undo, snapshot_entity captures behaviors
6. **`fix(gen): save actual camera FOV instead of hardcoded 45 degrees`** — Reads PerspectiveProjection FOV from Bevy
7. **`chore(gen): remove unused functions and silence warnings`** — Removed `collect_all_behaviors`, gated test-only `shape_to_primitive`
8. **`feat(gen): enrich entity_info with shape, emissive, and light data`** — LightInfoData struct, emissive color
9. **`feat(gen): include shape type in scene_info and entity_info summaries`** — Shape kind in EntitySummary

### Session 3 — Undo Coverage + Validation + Refactoring
10. **`feat(gen): extend undo to camera, clear-scene, and implement ModifyEntity replay`** — SetCamera undo, ClearScene snapshots all entities for undo, ModifyEntity in apply_edit_op
11. **`refactor(gen): introduce SnapshotQueries struct and add audio/parent/undo-info`** — Replaced 12-param snapshot_entity with SnapshotQueries + snap_queries! macro, audio and parent captured in snapshots, gen_undo_info tool
12. **`feat(gen): add save validation, enrich undo-info, and add EditOp tests`** — validate_entities() on save with warnings, entity/dirty counts in undo_info, 4 new EditOp roundtrip tests (44 total)
13. **`fix(gen): save light components on any entity type, not just GenEntityType::Light`** — Composable entities (shape + light) now save correctly

### Session 4 — Data Fidelity + History Persistence
14. **`feat(gen): persist light range, outer_angle, and inner_angle through save/load`** — PointLight range and SpotLight cone angles survive save/load roundtrip. Backward-compatible with old saves.
15. **`feat(gen): add range/angle fields to SetLightCmd and light tool schema`** — LLM agents can now specify point light range and spot light cone angles when creating lights.
16. **`feat(gen): extend MaterialDef with alpha_mode, unlit, double_sided, reflectance`** — Material properties beyond basic PBR now preserved. AlphaModeDef enum, material_def_to_standard() helper.
17. **`feat(gen): include light range and spot angles in entity_info response`** — LightInfoData enriched with range/outer_angle/inner_angle.
18. **`feat(gen): persist undo history as history.jsonl alongside world saves`** — Undo/redo stack survives save/load cycles. JSONL format with cursor metadata.
19. **`feat(gen): include alpha_mode and unlit in entity_info response`** — LLM agents can inspect transparency/unlit state.
20. **`feat(gen): add alpha_mode and unlit to gen_modify_entity tool`** — LLM agents can change transparency mode and unlit flag.
21. **`feat(gen): track glTF source path through save/load cycle`** — GltfSource component preserves imported mesh file paths. Save captures mesh_asset, load re-imports.
22. **`feat(gen): add alpha_mode/unlit to spawn, fix redo snapshot, add mesh_asset to entity_info`** — Transparent/unlit at creation time. Fixed apply_modify_to_snapshot for correct redo. Box EntityInfoData.
23. **`feat(gen): add double_sided/reflectance to tools and complete material property coverage`** — All 8 PBR material properties round-trip through every code path.
24. **`feat(gen): add rotation/scale/parent to gen_spawn_mesh tool`** — Custom meshes can be properly positioned and hierarchied at creation.
25. **`feat(gen): include audio emitter type in entity_info response`** — EntityInfoData reports audio emitter sound type.
26. **`fix(gen): apply visibility to all entity types on world load`** — Hidden state now applies to lights, meshes, and groups, not just shapes.
27. **`feat(gen): add full material properties to gen_spawn_mesh tool`** — RawMeshCmd now supports emissive, alpha_mode, unlit, double_sided, reflectance. Custom meshes match primitive capabilities.
28. **`feat(gen): enrich EntitySummary with light, audio, and behavior info`** — gen_scene_info now reports light types, audio emitters, and behavior counts per entity.

---

## Future Work

### High Priority

1. **Undo for audio commands** — Audio undo requires threading AudioEngine through apply_edit_op. The audio system uses a separate 3-thread model with lock-free shared params that doesn't fit the current undo architecture. Needs redesign.

2. ~~**History persistence**~~ — **Done** (Session 4, commit 18). JSONL format with cursor metadata.

### Medium Priority

3. **Per-chunk entity files** — For large worlds, split entities into `entities/chunk_0_0.ron` files instead of inlining all in `WorldManifest`. Use `DirtyTracker` for incremental saves.

4. **SpacetimeDB adapter** — Thin `world-server` crate mapping `WorldEntity` to flat table rows. Transform fields flattened for high-frequency updates. Component slots serialized as `Option<String>` (JSON), not `Vec<u8>`.

5. **Creation grouping** — `CreationDef` types exist but no gen tools create/manage them yet. Add `gen_create_creation` / `gen_dissolve_creation` tools.

6. ~~**WASM cross-compilation**~~ — **Done** (Session 3). Verified `cargo check --target wasm32-unknown-unknown` passes.

### Low Priority

7. **Screenshot capture on save** — Render a thumbnail when saving worlds for visual browsing.

8. ~~**Asset management**~~ — **Done** (Session 5, commit 21). GltfSource component + MeshAssetRef persist imported mesh paths through save/load.

9. **Migration system** — When `WorldManifest.version` changes, run migration functions to update old saves.

10. **Incremental save with dirty tracking** — Only serialize entities marked dirty by `DirtyTracker`, merge with existing chunk files.

14. **Tour playback** — `TourDef` and `TourWaypoint` types exist. Wire up camera interpolation and playback controls.

### Save Format Target

```
world-name/
  SKILL.md              # OpenClaw-compatible (kept from current)
  world.ron             # WorldManifest with inline entities
  entities/             # Per-chunk files for large worlds (future)
    chunk_0_0.ron
  assets/               # Imported meshes, textures, audio files
  history.jsonl         # Undo log (optional, future)
```
