# RFC: SpacetimeDB Server-Side Data Model for 3D Primitives and Audio

**Status:** Archived 2026-11-01 — a draft for the SpacetimeDB module, which was built as `crates/spacetime`, never joined from a client, and removed with every other model and service in the tree. Collaborative rooms run on `localgpt-world-sync` over a host or a relay (`docs/rfcs/multiplayer/collaborative-world-engine-architecture.md`).  
**Status when written:** Draft  
**Author:** LocalGPT Team  
**Date:** 2026-02-19  
**Target crates:** `localgpt-world-types`, `localgpt-world-server`  
**Depends on:** Monorepo restructure (crates/world-types, world-server, world-client)

---

## 1. Problem Statement

LocalGPT Gen currently generates 3D primitives and audio locally via in-process Bevy tools (`gen_spawn_primitive`, ABC notation pipeline). To support MMO-scale multiplayer co-creation, these representations must be stored as SpacetimeDB tables — the single source of truth — and streamed to all connected Bevy clients via subscription queries.

The server must:

- Represent every visual and audio object as SpacetimeDB table rows (not Bevy ECS components)
- Validate creations server-side (size, overlap, budget) before committing
- Stream spatial subsets to each client via chunk-based subscriptions
- Support full undo/history for every mutation
- Keep representations compact — SpacetimeDB holds all data in memory

This spec defines the exact Rust structs, reducers, and validation logic for `world-types` and `world-server`.

---

## 2. Design Principles

**P1 — Server stores intent, client renders.** The server stores `PrimitiveShape::Cuboid { x: 4.0, y: 3.0, z: 5.0 }`. The client turns that into a Bevy `Mesh3d` + `MeshMaterial3d`. The server never touches GPU types.

**P2 — Primitives, not meshes.** Following the proven approach (69.3% exact match on CAD benchmarks, blender-mcp's 16.8K stars), the server stores parametric primitives — not vertex buffers. This keeps rows small (~200 bytes per object) and enables server-side bounding box math without geometry processing.

**P3 — ABC notation is the music wire format.** ABC strings are the most compact LLM-friendly music representation (~1 token per note, 800K+ training tunes). The server stores ABC text verbatim. Clients parse and synthesize locally via FunDSP/RustySynth.

**P4 — Synthesis params ride alongside, not inside, ABC.** ABC can't express timbre, spatial positioning, or effects chains. A separate `SynthParams` struct (JSON-serialized) travels with each audio source. This matches the hybrid architecture from the ABC analysis.

**P5 — One table per update frequency.** SpacetimeDB evaluates subscription deltas per row change. High-frequency data (transforms of moving objects) must be in separate tables from low-frequency data (shape definitions, materials, audio sources).

---

## 3. Crate: `localgpt-world-types`

This crate contains **only** `serde`-derivable Rust types. Zero dependencies on SpacetimeDB or Bevy. Compiles for every target (native, WASM, iOS, Android).

### 3.1 Spatial Primitives

```rust
// crates/world-types/src/spatial.rs

/// Chunk coordinate utilities.
/// Chunk size: 64×64 world units in the horizontal plane.
pub const CHUNK_SIZE: f32 = 64.0;

/// Pack (chunk_x, chunk_y) into a single i64 key for SpacetimeDB indexing.
/// Uses upper 32 bits for x, lower 32 bits for y.
pub fn pack_chunk_key(chunk_x: i32, chunk_y: i32) -> i64 {
    ((chunk_x as i64) << 32) | (chunk_y as u32 as i64)
}

pub fn unpack_chunk_key(key: i64) -> (i32, i32) {
    let x = (key >> 32) as i32;
    let y = key as i32;
    (x, y)
}

/// Convert world position to chunk coordinates.
pub fn world_to_chunk(x: f32, z: f32) -> (i32, i32) {
    (
        (x / CHUNK_SIZE).floor() as i32,
        (z / CHUNK_SIZE).floor() as i32,
    )
}

/// Axis-aligned bounding box in world space.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Aabb {
    pub min_x: f32, pub min_y: f32, pub min_z: f32,
    pub max_x: f32, pub max_y: f32, pub max_z: f32,
}

impl Aabb {
    pub fn half_extents(&self) -> (f32, f32, f32) {
        (
            (self.max_x - self.min_x) / 2.0,
            (self.max_y - self.min_y) / 2.0,
            (self.max_z - self.min_z) / 2.0,
        )
    }

    pub fn overlaps(&self, other: &Aabb) -> bool {
        self.min_x < other.max_x && self.max_x > other.min_x
            && self.min_y < other.max_y && self.max_y > other.min_y
            && self.min_z < other.max_z && self.max_z > other.min_z
    }

    pub fn volume(&self) -> f32 {
        (self.max_x - self.min_x)
            * (self.max_y - self.min_y)
            * (self.max_z - self.min_z)
    }
}
```

### 3.2 Primitive Shape Enum

Maps 1:1 to the existing `gen_spawn_primitive` tool's `shape` parameter and Bevy's built-in mesh primitives.

```rust
// crates/world-types/src/primitives.rs

/// Every shape the LLM can spawn. Each variant carries its own dimensions.
/// Matches gen_spawn_primitive's "shape" + "dimensions" fields.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PrimitiveShape {
    /// Bevy: Cuboid::new(x, y, z)
    Cuboid { x: f32, y: f32, z: f32 },
    /// Bevy: Sphere::new(radius)
    Sphere { radius: f32 },
    /// Bevy: Cylinder::new(radius, height)
    Cylinder { radius: f32, height: f32 },
    /// Bevy: Cone::new(radius, height)
    Cone { radius: f32, height: f32 },
    /// Bevy: Capsule3d::new(radius, length)
    Capsule { radius: f32, length: f32 },
    /// Bevy: Torus::new(minor_radius, major_radius)
    Torus { minor_radius: f32, major_radius: f32 },
    /// Bevy: Plane3d::default().mesh().size(width, depth)
    Plane { width: f32, depth: f32 },
    /// Bevy: ConicalFrustum { top, bottom, height }
    ConicalFrustum { top_radius: f32, bottom_radius: f32, height: f32 },
}

impl PrimitiveShape {
    /// Compute the local-space AABB for this shape (centered at origin).
    pub fn local_aabb(&self) -> Aabb {
        match self {
            Self::Cuboid { x, y, z } => Aabb {
                min_x: -x / 2.0, min_y: -y / 2.0, min_z: -z / 2.0,
                max_x:  x / 2.0, max_y:  y / 2.0, max_z:  z / 2.0,
            },
            Self::Sphere { radius } => Aabb {
                min_x: -radius, min_y: -radius, min_z: -radius,
                max_x:  *radius, max_y:  *radius, max_z:  *radius,
            },
            Self::Cylinder { radius, height } => Aabb {
                min_x: -radius, min_y: -height / 2.0, min_z: -radius,
                max_x:  *radius, max_y:  height / 2.0, max_z:  *radius,
            },
            Self::Cone { radius, height } => Aabb {
                min_x: -radius, min_y: 0.0, min_z: -radius,
                max_x:  *radius, max_y: *height, max_z:  *radius,
            },
            Self::Capsule { radius, length } => Aabb {
                min_x: -radius, min_y: -(length / 2.0 + radius), min_z: -radius,
                max_x:  *radius, max_y:  length / 2.0 + radius, max_z:  *radius,
            },
            Self::Torus { minor_radius, major_radius } => Aabb {
                min_x: -(major_radius + minor_radius),
                min_y: -minor_radius,
                min_z: -(major_radius + minor_radius),
                max_x: major_radius + minor_radius,
                max_y: *minor_radius,
                max_z: major_radius + minor_radius,
            },
            Self::Plane { width, depth } => Aabb {
                min_x: -width / 2.0, min_y: 0.0, min_z: -depth / 2.0,
                max_x:  width / 2.0, max_y: 0.0, max_z:  depth / 2.0,
            },
            Self::ConicalFrustum { top_radius, bottom_radius, height } => {
                let r = top_radius.max(*bottom_radius);
                Aabb {
                    min_x: -r, min_y: 0.0, min_z: -r,
                    max_x:  r, max_y: *height, max_z:  r,
                }
            }
        }
    }

    /// Estimated triangle count for complexity budgeting.
    /// Based on Bevy's default tessellation settings.
    pub fn estimated_triangles(&self) -> u32 {
        match self {
            Self::Cuboid { .. } => 12,
            Self::Sphere { .. } => 2880,     // 36 sectors × 18 stacks × ~4-5
            Self::Cylinder { .. } => 288,     // 36 sectors × 2 caps + sides
            Self::Cone { .. } => 180,
            Self::Capsule { .. } => 3168,
            Self::Torus { .. } => 5184,       // 36 × 36 × 4
            Self::Plane { .. } => 2,
            Self::ConicalFrustum { .. } => 288,
        }
    }
}
```

### 3.3 Material Definition

```rust
// crates/world-types/src/material.rs

/// PBR material stored server-side. Maps to Bevy's StandardMaterial.
/// All fields have defaults matching gen_spawn_primitive's defaults.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PbrMaterial {
    /// RGBA, 0.0–1.0. Default: [0.8, 0.8, 0.8, 1.0]
    #[serde(default = "default_color")]
    pub color: [f32; 4],
    /// 0.0 (dielectric) – 1.0 (metal). Default: 0.0
    #[serde(default)]
    pub metallic: f32,
    /// 0.0 (mirror) – 1.0 (diffuse). Default: 0.5
    #[serde(default = "default_roughness")]
    pub roughness: f32,
    /// Emissive RGBA. Default: [0, 0, 0, 0]
    #[serde(default)]
    pub emissive: [f32; 4],
}

fn default_color() -> [f32; 4] { [0.8, 0.8, 0.8, 1.0] }
fn default_roughness() -> f32 { 0.5 }

impl Default for PbrMaterial {
    fn default() -> Self {
        Self {
            color: default_color(),
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 4],
        }
    }
}

impl PbrMaterial {
    /// Byte size estimate for memory budgeting (4 floats × 3 fields = 48 bytes).
    pub const SIZE_BYTES: usize = 48;
}
```

### 3.4 Transform

```rust
// crates/world-types/src/transform.rs

/// World transform stored server-side.
/// Position is in world-space. Rotation is Euler degrees (matching LLM tool interface).
/// The client converts to Bevy's Transform (quaternion) on receipt.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct WorldTransform {
    pub x: f32, pub y: f32, pub z: f32,
    /// Euler angles in degrees: (pitch, yaw, roll)
    #[serde(default)]
    pub rotation_deg: [f32; 3],
    #[serde(default = "default_scale")]
    pub scale: [f32; 3],
}

fn default_scale() -> [f32; 3] { [1.0, 1.0, 1.0] }

impl Default for WorldTransform {
    fn default() -> Self {
        Self {
            x: 0.0, y: 0.0, z: 0.0,
            rotation_deg: [0.0; 3],
            scale: [1.0, 1.0, 1.0],
        }
    }
}
```

### 3.5 Light Types

```rust
// crates/world-types/src/light.rs

/// Light source stored server-side. Maps to gen_set_light tool.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum LightKind {
    Directional {
        /// Lux. Default: 10000.0
        intensity: f32,
        /// Direction vector [x, y, z].
        direction: [f32; 3],
    },
    Point {
        /// Lumens. Default: 1000.0
        intensity: f32,
    },
    Spot {
        /// Lumens.
        intensity: f32,
        /// Direction vector.
        direction: [f32; 3],
        /// Inner/outer cone angles in degrees.
        inner_angle: f32,
        outer_angle: f32,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LightDef {
    pub kind: LightKind,
    /// RGBA color, 0.0–1.0.
    #[serde(default = "super::material::default_color")]
    pub color: [f32; 4],
    #[serde(default = "default_shadows")]
    pub shadows: bool,
}

fn default_shadows() -> bool { true }
```

### 3.6 Audio Types

```rust
// crates/world-types/src/audio.rs

/// What kind of audio source this is.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum AudioKind {
    /// Musical content: ABC notation string synthesized client-side.
    /// ABC is the LLM output format (1 token/note, 800K+ training corpus).
    Music {
        /// Raw ABC notation string. Example:
        /// "X:1\nT:Forest Theme\nM:3/4\nL:1/8\nK:Dmaj\n|:D2 FA AF|..."
        abc: String,
    },
    /// Ambient/environmental sound: synthesized procedurally on the client.
    /// Uses a DSP graph description (FunDSP-style JSON, not raw code).
    Ambient {
        /// Preset identifier for client-side DSP graph.
        /// Examples: "wind", "rain", "fire_crackle", "water_stream",
        ///           "forest_birds", "cave_drip", "crowd_murmur"
        preset: String,
        /// Preset parameter overrides. Keys depend on preset.
        /// Example for "wind": {"speed": 0.7, "gust_frequency": 0.3}
        #[serde(default)]
        params: std::collections::HashMap<String, f32>,
    },
    /// Sound effect: triggered by events, not looping.
    /// Short procedural synthesis via FunDSP graph description.
    Sfx {
        /// DSP graph as structured JSON (not raw code — safe to evaluate).
        /// Schema: { "nodes": [...], "connections": [...] }
        /// Validated server-side against an allowlist of node types.
        graph_json: String,
    },
}

/// Synthesis parameters that ABC notation cannot express.
/// Travels alongside AudioKind to control timbre and effects.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SynthParams {
    /// Instrument timbre. The client maps this to a SoundFont program
    /// or FunDSP oscillator type.
    /// Examples: "piano", "strings", "flute", "synth_pad", "music_box"
    #[serde(default = "default_instrument")]
    pub instrument: String,
    /// Master volume 0.0–1.0.
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Reverb send level 0.0–1.0. 0 = dry.
    #[serde(default)]
    pub reverb: f32,
    /// Low-pass filter cutoff in Hz. 0 = disabled.
    #[serde(default)]
    pub lpf_cutoff: f32,
    /// Playback tempo override in BPM. 0 = use ABC's Q: field.
    #[serde(default)]
    pub tempo_bpm: f32,
}

fn default_instrument() -> String { "piano".into() }
fn default_volume() -> f32 { 0.7 }

/// Spatial audio behavior.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SpatialMode {
    /// Non-positional (background music, UI sounds).
    Global,
    /// 3D positioned audio with distance attenuation.
    Positional {
        /// Max audible distance in world units.
        max_distance: f32,
        /// Attenuation rolloff: "linear" | "inverse" | "exponential"
        #[serde(default = "default_rolloff")]
        rolloff: String,
    },
    /// Attached to a world object — moves with it.
    Attached {
        /// ID of the WorldObject this audio source follows.
        object_id: u64,
        max_distance: f32,
        #[serde(default = "default_rolloff")]
        rolloff: String,
    },
}

fn default_rolloff() -> String { "inverse".into() }

impl Default for SpatialMode {
    fn default() -> Self {
        Self::Positional { max_distance: 50.0, rolloff: "inverse".into() }
    }
}
```

### 3.7 Semantic Categories and Size Limits

```rust
// crates/world-types/src/categories.rs

/// Semantic category for size normalization.
/// The LLM includes this in its output; the server validates bounds.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub enum SemanticCategory {
    SmallProp,      // chair, lamp, barrel: max 2×2×3m
    LargeProp,      // table, cart, fountain: max 5×5×5m
    Vegetation,     // tree, bush, flower: max 15×15×25m
    SmallStructure, // shed, kiosk, well: max 10×10×8m
    MediumStructure,// cottage, shop, barn: max 20×20×12m
    LargeStructure, // castle, manor, warehouse: max 80×80×40m
    Terrain,        // ground, hill, cliff: max 64×256×64m (one chunk)
    Custom,         // user-specified bounds, requires explicit bbox
}

impl SemanticCategory {
    /// Maximum bounding box half-extents (x, y, z) in world units.
    pub fn max_half_extents(&self) -> (f32, f32, f32) {
        match self {
            Self::SmallProp       => (1.0,   1.5,  1.0),
            Self::LargeProp       => (2.5,   2.5,  2.5),
            Self::Vegetation      => (7.5,   12.5, 7.5),
            Self::SmallStructure  => (5.0,   4.0,  5.0),
            Self::MediumStructure => (10.0,  6.0,  10.0),
            Self::LargeStructure  => (40.0,  20.0, 40.0),
            Self::Terrain         => (32.0,  128.0, 32.0),
            Self::Custom          => (f32::MAX, f32::MAX, f32::MAX),
        }
    }

    /// Minimum setback from chunk edge or neighbor, in world units.
    /// Prevents objects from being flush against boundaries.
    pub fn min_setback(&self) -> f32 {
        match self {
            Self::SmallProp | Self::LargeProp => 0.5,
            Self::Vegetation => 1.0,
            Self::SmallStructure => 2.0,
            Self::MediumStructure => 3.0,
            Self::LargeStructure => 5.0,
            Self::Terrain => 0.0,
            Self::Custom => 0.0,
        }
    }
}
```

### 3.8 Creation Spec (Compound Object)

A "creation" is what the LLM outputs for one user request — potentially multiple primitives forming a single logical object (e.g., a cabin = floor + walls + roof).

```rust
// crates/world-types/src/creation.rs

/// A complete creation submitted by the LLM via a reducer call.
/// This is the unit of undo/redo — one creation = one undo step.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CreationSpec {
    /// Human-readable name (e.g., "stone_cabin", "oak_tree_cluster").
    pub name: String,
    /// Semantic category for size validation.
    pub category: SemanticCategory,
    /// Root transform in world space.
    pub transform: WorldTransform,
    /// The primitives that compose this creation.
    /// Positions are relative to the root transform.
    pub primitives: Vec<PrimitiveDef>,
    /// Optional audio sources attached to this creation.
    #[serde(default)]
    pub audio_sources: Vec<AudioSourceDef>,
    /// Optional light sources.
    #[serde(default)]
    pub lights: Vec<LightPlacement>,
}

/// A single primitive within a creation.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PrimitiveDef {
    /// Name within the creation (e.g., "wall_north", "roof").
    pub name: String,
    pub shape: PrimitiveShape,
    pub material: PbrMaterial,
    /// Transform relative to the creation root.
    #[serde(default)]
    pub local_transform: WorldTransform,
    /// Optional parent name within this creation (for hierarchy).
    #[serde(default)]
    pub parent: Option<String>,
}

/// An audio source placed within a creation.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AudioSourceDef {
    pub name: String,
    pub kind: AudioKind,
    pub synth: SynthParams,
    pub spatial: SpatialMode,
    /// Transform relative to the creation root.
    #[serde(default)]
    pub local_transform: WorldTransform,
    /// Whether this source starts playing immediately on load.
    #[serde(default = "default_autoplay")]
    pub autoplay: bool,
    /// Whether playback loops.
    #[serde(default = "default_loop")]
    pub looping: bool,
}

fn default_autoplay() -> bool { true }
fn default_loop() -> bool { true }

/// A light placed within a creation.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LightPlacement {
    pub name: String,
    pub light: LightDef,
    #[serde(default)]
    pub local_transform: WorldTransform,
}
```

### 3.9 Event Types (for undo/history)

```rust
// crates/world-types/src/events.rs

/// The type of world mutation, stored in the event log.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
#[repr(u16)]
pub enum EventType {
    /// A new creation was placed.
    Place = 1,
    /// A creation was modified (transform, material, etc.).
    Modify = 2,
    /// A creation was deleted.
    Delete = 3,
    /// A creation was resized.
    Resize = 4,
    /// An audio source was added/modified.
    AudioChange = 5,
    /// An undo was performed.
    Undo = 10,
    /// A redo was performed.
    Redo = 11,
}
```

---

## 4. Crate: `localgpt-world-server` (SpacetimeDB WASM Module)

### 4.1 Table Definitions

These are the actual SpacetimeDB tables. They reference types from `world-types` but must use `spacetimedb` attributes.

```rust
// crates/world-server/src/tables.rs

use spacetimedb::{table, Identity, Timestamp};

// ──────────────────────────────────────────────────
// CHUNK METADATA (low frequency, queried for LOD)
// ──────────────────────────────────────────────────

#[table(name = chunk_meta, public)]
pub struct ChunkMeta {
    #[primary_key]
    pub chunk_key: i64,           // pack_chunk_key(x, y)
    pub biome: u8,
    pub object_count: u32,
    pub audio_source_count: u16,
    pub triangle_budget_used: u32,
    pub triangle_budget_max: u32,  // default: 500_000 per chunk
    pub last_modified: Timestamp,
}

// ──────────────────────────────────────────────────
// WORLD OBJECTS (3D primitives — moderate frequency)
// ──────────────────────────────────────────────────

/// Each row is one primitive in the world.
/// A "creation" (cabin = 10 primitives) becomes 10 rows
/// linked by creation_id.
#[table(name = world_object, public)]
pub struct WorldObject {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    /// Groups primitives into logical creations for undo.
    #[index(btree)]
    pub creation_id: u64,
    /// Spatial partition key for subscription queries.
    #[index(btree)]
    pub chunk_key: i64,
    /// Name within the creation (e.g., "wall_north").
    pub name: String,
    /// Serialized PrimitiveShape enum (serde_json bytes).
    /// ~50–100 bytes per shape.
    pub shape_data: Vec<u8>,
    /// Serialized PbrMaterial (48 bytes fixed).
    pub material_data: Vec<u8>,
    /// World-space AABB for overlap checks.
    /// Computed server-side from shape + transform.
    pub aabb_min_x: f32, pub aabb_min_y: f32, pub aabb_min_z: f32,
    pub aabb_max_x: f32, pub aabb_max_y: f32, pub aabb_max_z: f32,
    /// Estimated triangle count for budget tracking.
    pub triangle_count: u32,
    /// Parent object ID within this creation (0 = root).
    pub parent_id: u64,
    pub creator: Identity,
    pub created_at: Timestamp,
}

// ──────────────────────────────────────────────────
// TRANSFORMS (high frequency — separate for perf)
// ──────────────────────────────────────────────────

/// Separated from WorldObject because moving objects update
/// transforms at high frequency. Subscription deltas only
/// fire for changed rows — keeping transforms separate means
/// material/shape changes don't trigger transform subscriptions
/// and vice versa.
#[table(name = object_transform, public)]
pub struct ObjectTransform {
    #[primary_key]
    pub object_id: u64,
    #[index(btree)]
    pub chunk_key: i64,
    pub x: f32, pub y: f32, pub z: f32,
    pub rot_pitch: f32, pub rot_yaw: f32, pub rot_roll: f32,
    pub scale_x: f32, pub scale_y: f32, pub scale_z: f32,
}

// ──────────────────────────────────────────────────
// CREATIONS (logical grouping for undo/history)
// ──────────────────────────────────────────────────

/// One row per logical creation (e.g., "stone_cabin").
/// The objects table has N rows pointing to this creation_id.
#[table(name = creation, public)]
pub struct Creation {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub chunk_key: i64,
    pub name: String,
    pub category: u8,             // SemanticCategory as u8
    pub object_count: u32,
    pub total_triangles: u32,
    pub creator: Identity,
    pub created_at: Timestamp,
    pub modified_at: Timestamp,
}

// ──────────────────────────────────────────────────
// AUDIO SOURCES
// ──────────────────────────────────────────────────

/// Each audio source in the world. Can be standalone
/// (ambient zone) or attached to a creation.
#[table(name = audio_source, public)]
pub struct AudioSource {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub chunk_key: i64,
    /// 0 = standalone (ambient zone), >0 = attached to creation.
    #[index(btree)]
    pub creation_id: u64,
    pub name: String,
    /// Serialized AudioKind enum.
    /// For Music: contains the ABC string (~200–2000 bytes).
    /// For Ambient: preset name + params (~50–200 bytes).
    /// For Sfx: DSP graph JSON (~100–500 bytes).
    pub kind_data: Vec<u8>,
    /// Serialized SynthParams (~80 bytes).
    pub synth_data: Vec<u8>,
    /// Serialized SpatialMode enum (~20 bytes).
    pub spatial_data: Vec<u8>,
    pub autoplay: bool,
    pub looping: bool,
    pub creator: Identity,
    pub created_at: Timestamp,
}

/// Separate table for audio source positions (same pattern
/// as object_transform — high frequency if attached to
/// moving objects).
#[table(name = audio_transform, public)]
pub struct AudioTransform {
    #[primary_key]
    pub audio_id: u64,
    #[index(btree)]
    pub chunk_key: i64,
    pub x: f32, pub y: f32, pub z: f32,
}

// ──────────────────────────────────────────────────
// EVENT LOG (undo/history)
// ──────────────────────────────────────────────────

#[table(name = world_event, public)]
pub struct WorldEvent {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub chunk_key: i64,
    #[index(btree)]
    pub creation_id: u64,
    pub editor: Identity,
    pub event_type: u16,          // EventType as u16
    /// Serialized forward operation (for redo).
    pub forward_data: Vec<u8>,
    /// Serialized inverse operation (for undo).
    pub inverse_data: Vec<u8>,
    pub is_undone: bool,
    pub timestamp: Timestamp,
}

// ──────────────────────────────────────────────────
// PLAYER STATE (interest management)
// ──────────────────────────────────────────────────

#[table(name = player_state, public)]
pub struct PlayerState {
    #[primary_key]
    pub identity: Identity,
    pub current_chunk_key: i64,
    pub x: f32, pub y: f32, pub z: f32,
    /// Edits per minute (rolling window for rate limiting).
    pub edits_this_minute: u16,
    pub minute_start: Timestamp,
    /// Reputation score (higher = more editing freedom).
    pub reputation: u32,
    pub last_seen: Timestamp,
}
```

### 4.2 Core Reducers

```rust
// crates/world-server/src/reducers.rs

use spacetimedb::{reducer, ReducerContext, Timestamp};
use localgpt_world_types::*;

/// Maximum objects per chunk before rejecting new creations.
const MAX_OBJECTS_PER_CHUNK: u32 = 2000;
/// Maximum triangles per chunk.
const MAX_TRIANGLES_PER_CHUNK: u32 = 500_000;
/// Maximum audio sources per chunk.
const MAX_AUDIO_PER_CHUNK: u16 = 100;
/// Maximum ABC string length (bytes).
const MAX_ABC_LENGTH: usize = 8192;
/// New user edit rate limit (per minute).
const RATE_LIMIT_NEW: u16 = 10;
/// Trusted user edit rate limit (per minute).
const RATE_LIMIT_TRUSTED: u16 = 100;
/// Reputation threshold for "trusted" status.
const TRUSTED_REPUTATION: u32 = 50;

// ── Place a new creation ──────────────────────────

#[reducer]
pub fn place_creation(
    ctx: &ReducerContext,
    spec_json: String,   // Serialized CreationSpec
) -> Result<(), String> {
    // 1. Deserialize and validate the spec
    let spec: CreationSpec = serde_json::from_str(&spec_json)
        .map_err(|e| format!("Invalid creation spec: {e}"))?;

    // 2. Rate limit check
    check_rate_limit(ctx)?;

    // 3. Validate primitive dimensions against category limits
    validate_category_bounds(&spec)?;

    // 4. Validate audio sources
    for audio in &spec.audio_sources {
        validate_audio(&audio.kind)?;
    }

    // 5. Compute world-space AABB for entire creation
    let creation_aabb = compute_creation_aabb(&spec);
    let (chunk_x, chunk_y) = world_to_chunk(spec.transform.x, spec.transform.z);
    let chunk_key = pack_chunk_key(chunk_x, chunk_y);

    // 6. Check chunk budgets
    let meta = ctx.db.chunk_meta().chunk_key().find(chunk_key);
    if let Some(ref m) = meta {
        let new_objects = m.object_count + spec.primitives.len() as u32;
        if new_objects > MAX_OBJECTS_PER_CHUNK {
            return Err(format!(
                "Chunk at capacity: {}/{MAX_OBJECTS_PER_CHUNK} objects",
                m.object_count
            ));
        }
        let new_tris: u32 = spec.primitives.iter()
            .map(|p| p.shape.estimated_triangles())
            .sum();
        if m.triangle_budget_used + new_tris > MAX_TRIANGLES_PER_CHUNK {
            return Err(format!(
                "Chunk triangle budget exceeded: {}/{}",
                m.triangle_budget_used + new_tris,
                MAX_TRIANGLES_PER_CHUNK
            ));
        }
        if m.audio_source_count + spec.audio_sources.len() as u16
            > MAX_AUDIO_PER_CHUNK
        {
            return Err("Chunk audio source limit reached".into());
        }
    }

    // 7. Check overlap with existing objects in this + adjacent chunks
    check_overlap(ctx, chunk_key, &creation_aabb, &spec)?;

    // 8. All checks pass — insert creation record
    let creation = ctx.db.creation().insert(Creation {
        id: 0, // auto_inc
        chunk_key,
        name: spec.name.clone(),
        category: spec.category as u8,
        object_count: spec.primitives.len() as u32,
        total_triangles: spec.primitives.iter()
            .map(|p| p.shape.estimated_triangles()).sum(),
        creator: ctx.sender,
        created_at: Timestamp::now(),
        modified_at: Timestamp::now(),
    });
    let creation_id = creation.id;

    // 9. Insert each primitive as a WorldObject + ObjectTransform
    let mut name_to_id: std::collections::HashMap<String, u64> = Default::default();
    for prim in &spec.primitives {
        let world_pos = compose_transform(&spec.transform, &prim.local_transform);
        let aabb = compute_world_aabb(&prim.shape, &world_pos);

        let obj = ctx.db.world_object().insert(WorldObject {
            id: 0,
            creation_id,
            chunk_key,
            name: prim.name.clone(),
            shape_data: serde_json::to_vec(&prim.shape).unwrap(),
            material_data: serde_json::to_vec(&prim.material).unwrap(),
            aabb_min_x: aabb.min_x, aabb_min_y: aabb.min_y, aabb_min_z: aabb.min_z,
            aabb_max_x: aabb.max_x, aabb_max_y: aabb.max_y, aabb_max_z: aabb.max_z,
            triangle_count: prim.shape.estimated_triangles(),
            parent_id: prim.parent.as_ref()
                .and_then(|p| name_to_id.get(p))
                .copied()
                .unwrap_or(0),
            creator: ctx.sender,
            created_at: Timestamp::now(),
        });

        ctx.db.object_transform().insert(ObjectTransform {
            object_id: obj.id,
            chunk_key,
            x: world_pos.x, y: world_pos.y, z: world_pos.z,
            rot_pitch: world_pos.rotation_deg[0],
            rot_yaw: world_pos.rotation_deg[1],
            rot_roll: world_pos.rotation_deg[2],
            scale_x: world_pos.scale[0],
            scale_y: world_pos.scale[1],
            scale_z: world_pos.scale[2],
        });

        name_to_id.insert(prim.name.clone(), obj.id);
    }

    // 10. Insert audio sources
    for audio_def in &spec.audio_sources {
        let world_pos = compose_transform(&spec.transform, &audio_def.local_transform);
        let audio = ctx.db.audio_source().insert(AudioSource {
            id: 0,
            chunk_key,
            creation_id,
            name: audio_def.name.clone(),
            kind_data: serde_json::to_vec(&audio_def.kind).unwrap(),
            synth_data: serde_json::to_vec(&audio_def.synth).unwrap(),
            spatial_data: serde_json::to_vec(&audio_def.spatial).unwrap(),
            autoplay: audio_def.autoplay,
            looping: audio_def.looping,
            creator: ctx.sender,
            created_at: Timestamp::now(),
        });
        ctx.db.audio_transform().insert(AudioTransform {
            audio_id: audio.id,
            chunk_key,
            x: world_pos.x, y: world_pos.y, z: world_pos.z,
        });
    }

    // 11. Insert lights (stored as special world objects with LightKind)
    for light in &spec.lights {
        let world_pos = compose_transform(&spec.transform, &light.local_transform);
        let obj = ctx.db.world_object().insert(WorldObject {
            id: 0,
            creation_id,
            chunk_key,
            name: light.name.clone(),
            shape_data: serde_json::to_vec(&light.light).unwrap(),
            material_data: Vec::new(), // lights have no material
            aabb_min_x: world_pos.x, aabb_min_y: world_pos.y, aabb_min_z: world_pos.z,
            aabb_max_x: world_pos.x, aabb_max_y: world_pos.y, aabb_max_z: world_pos.z,
            triangle_count: 0,
            parent_id: 0,
            creator: ctx.sender,
            created_at: Timestamp::now(),
        });
        ctx.db.object_transform().insert(ObjectTransform {
            object_id: obj.id,
            chunk_key,
            x: world_pos.x, y: world_pos.y, z: world_pos.z,
            rot_pitch: 0.0, rot_yaw: 0.0, rot_roll: 0.0,
            scale_x: 1.0, scale_y: 1.0, scale_z: 1.0,
        });
    }

    // 12. Update chunk metadata
    update_chunk_meta(ctx, chunk_key);

    // 13. Record event for undo
    ctx.db.world_event().insert(WorldEvent {
        id: 0,
        chunk_key,
        creation_id,
        editor: ctx.sender,
        event_type: EventType::Place as u16,
        forward_data: spec_json.into_bytes(),
        inverse_data: creation_id.to_le_bytes().to_vec(), // delete by id
        is_undone: false,
        timestamp: Timestamp::now(),
    });

    Ok(())
}

// ── Delete a creation (all its objects + audio) ───

#[reducer]
pub fn delete_creation(
    ctx: &ReducerContext,
    creation_id: u64,
) -> Result<(), String> {
    check_rate_limit(ctx)?;

    let creation = ctx.db.creation().id().find(creation_id)
        .ok_or("Creation not found")?;

    // Capture full state for undo before deleting
    let snapshot = snapshot_creation(ctx, creation_id);

    // Delete all objects in this creation
    for obj in ctx.db.world_object().creation_id().filter(creation_id) {
        ctx.db.object_transform().object_id().delete(obj.id);
        ctx.db.world_object().id().delete(obj.id);
    }

    // Delete all audio sources
    for audio in ctx.db.audio_source().creation_id().filter(creation_id) {
        ctx.db.audio_transform().audio_id().delete(audio.id);
        ctx.db.audio_source().id().delete(audio.id);
    }

    // Delete creation record
    ctx.db.creation().id().delete(creation_id);

    // Update chunk metadata
    update_chunk_meta(ctx, creation.chunk_key);

    // Record event for undo (inverse = re-place the creation)
    ctx.db.world_event().insert(WorldEvent {
        id: 0,
        chunk_key: creation.chunk_key,
        creation_id,
        editor: ctx.sender,
        event_type: EventType::Delete as u16,
        forward_data: creation_id.to_le_bytes().to_vec(),
        inverse_data: snapshot,
        is_undone: false,
        timestamp: Timestamp::now(),
    });

    Ok(())
}

// ── Undo last action by this user ─────────────────

#[reducer]
pub fn undo(ctx: &ReducerContext) -> Result<(), String> {
    // Find this user's most recent non-undone event
    let events: Vec<_> = ctx.db.world_event()
        .editor().filter(ctx.sender)
        .collect();

    let event = events.iter()
        .filter(|e| !e.is_undone)
        .max_by_key(|e| e.id)
        .ok_or("Nothing to undo")?;

    // Apply inverse operation
    match event.event_type {
        t if t == EventType::Place as u16 => {
            // Inverse of Place = Delete
            let creation_id = u64::from_le_bytes(
                event.inverse_data[..8].try_into().unwrap()
            );
            // Delete without recording a new event
            delete_creation_internal(ctx, creation_id)?;
        }
        t if t == EventType::Delete as u16 => {
            // Inverse of Delete = Re-place from snapshot
            let spec_json = String::from_utf8(event.inverse_data.clone())
                .map_err(|_| "Corrupted undo data")?;
            place_creation_internal(ctx, &spec_json)?;
        }
        _ => return Err("Unsupported undo operation".into()),
    }

    // Mark event as undone
    let mut evt = event.clone();
    evt.is_undone = true;
    ctx.db.world_event().id().update(evt);

    Ok(())
}

// ── Update player position (drives subscriptions) ─

#[reducer]
pub fn update_player_position(
    ctx: &ReducerContext,
    x: f32, y: f32, z: f32,
) -> Result<(), String> {
    let chunk_key = pack_chunk_key(
        world_to_chunk(x, z).0,
        world_to_chunk(x, z).1,
    );

    if let Some(mut state) = ctx.db.player_state()
        .identity().find(ctx.sender)
    {
        state.current_chunk_key = chunk_key;
        state.x = x; state.y = y; state.z = z;
        state.last_seen = Timestamp::now();
        ctx.db.player_state().identity().update(state);
    } else {
        ctx.db.player_state().insert(PlayerState {
            identity: ctx.sender,
            current_chunk_key: chunk_key,
            x, y, z,
            edits_this_minute: 0,
            minute_start: Timestamp::now(),
            reputation: 0,
            last_seen: Timestamp::now(),
        });
    }

    Ok(())
}
```

### 4.3 Validation Functions

```rust
// crates/world-server/src/validation.rs

use localgpt_world_types::*;

/// Validate that a creation's aggregate AABB fits its category limits.
pub fn validate_category_bounds(spec: &CreationSpec) -> Result<(), String> {
    let aabb = compute_creation_aabb(spec);
    let (hx, hy, hz) = aabb.half_extents();
    let (max_hx, max_hy, max_hz) = spec.category.max_half_extents();

    if hx > max_hx || hy > max_hy || hz > max_hz {
        return Err(format!(
            "Creation '{}' ({:?}) exceeds size limits: \
             actual ({:.1}×{:.1}×{:.1}m) > max ({:.1}×{:.1}×{:.1}m)",
            spec.name, spec.category,
            hx * 2.0, hy * 2.0, hz * 2.0,
            max_hx * 2.0, max_hy * 2.0, max_hz * 2.0,
        ));
    }
    Ok(())
}

/// Validate audio source content.
pub fn validate_audio(kind: &AudioKind) -> Result<(), String> {
    match kind {
        AudioKind::Music { abc } => {
            if abc.len() > MAX_ABC_LENGTH {
                return Err(format!(
                    "ABC notation too long: {} bytes (max {MAX_ABC_LENGTH})",
                    abc.len()
                ));
            }
            // Basic structural validation: must have X: and K: headers
            if !abc.contains("X:") || !abc.contains("K:") {
                return Err(
                    "Invalid ABC: missing required X: (index) or K: (key) header"
                    .into()
                );
            }
            Ok(())
        }
        AudioKind::Ambient { preset, params } => {
            // Allowlist of known presets
            const ALLOWED: &[&str] = &[
                "wind", "rain", "fire_crackle", "water_stream",
                "forest_birds", "cave_drip", "crowd_murmur",
                "ocean_waves", "thunder", "insects_night",
            ];
            if !ALLOWED.contains(&preset.as_str()) {
                return Err(format!("Unknown ambient preset: '{preset}'"));
            }
            // Validate param ranges
            for (key, val) in params {
                if *val < 0.0 || *val > 1.0 {
                    return Err(format!(
                        "Ambient param '{key}' out of range: {val} (must be 0.0–1.0)"
                    ));
                }
            }
            Ok(())
        }
        AudioKind::Sfx { graph_json } => {
            // Parse and validate DSP graph against node allowlist
            let graph: serde_json::Value = serde_json::from_str(graph_json)
                .map_err(|e| format!("Invalid SFX graph JSON: {e}"))?;

            const ALLOWED_NODES: &[&str] = &[
                "sin", "saw", "square", "triangle", "noise",
                "lpf", "hpf", "bpf",
                "adsr", "envelope",
                "mul", "add", "mix",
                "delay", "reverb",
            ];

            if let Some(nodes) = graph.get("nodes").and_then(|n| n.as_array()) {
                for node in nodes {
                    if let Some(node_type) = node.get("type").and_then(|t| t.as_str()) {
                        if !ALLOWED_NODES.contains(&node_type) {
                            return Err(format!(
                                "Disallowed SFX node type: '{node_type}'"
                            ));
                        }
                    }
                }
            }
            Ok(())
        }
    }
}

/// Check overlap with existing objects in this and adjacent chunks.
pub fn check_overlap(
    ctx: &ReducerContext,
    chunk_key: i64,
    creation_aabb: &Aabb,
    spec: &CreationSpec,
) -> Result<(), String> {
    let setback = spec.category.min_setback();
    let expanded = Aabb {
        min_x: creation_aabb.min_x - setback,
        min_y: creation_aabb.min_y - setback,
        min_z: creation_aabb.min_z - setback,
        max_x: creation_aabb.max_x + setback,
        max_y: creation_aabb.max_y + setback,
        max_z: creation_aabb.max_z + setback,
    };

    let (cx, cy) = unpack_chunk_key(chunk_key);
    // Check 3×3 grid of chunks
    for dx in -1..=1 {
        for dy in -1..=1 {
            let neighbor_key = pack_chunk_key(cx + dx, cy + dy);
            for obj in ctx.db.world_object().chunk_key().filter(neighbor_key) {
                let obj_aabb = Aabb {
                    min_x: obj.aabb_min_x, min_y: obj.aabb_min_y,
                    min_z: obj.aabb_min_z,
                    max_x: obj.aabb_max_x, max_y: obj.aabb_max_y,
                    max_z: obj.aabb_max_z,
                };
                if expanded.overlaps(&obj_aabb) {
                    return Err(format!(
                        "Would overlap with '{}' (setback: {setback}m). \
                         Try a different position or reduce size.",
                        obj.name
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Rate limit check using token bucket per user.
pub fn check_rate_limit(ctx: &ReducerContext) -> Result<(), String> {
    let now = Timestamp::now();
    if let Some(mut state) = ctx.db.player_state()
        .identity().find(ctx.sender)
    {
        // Reset counter if minute has elapsed
        let elapsed_ms = now.to_duration_from_epoch().as_millis()
            - state.minute_start.to_duration_from_epoch().as_millis();
        if elapsed_ms > 60_000 {
            state.edits_this_minute = 0;
            state.minute_start = now;
        }

        let limit = if state.reputation >= TRUSTED_REPUTATION {
            RATE_LIMIT_TRUSTED
        } else {
            RATE_LIMIT_NEW
        };

        if state.edits_this_minute >= limit {
            return Err(format!(
                "Rate limited: {limit} edits/minute. \
                 Build reputation to increase your limit."
            ));
        }

        state.edits_this_minute += 1;
        ctx.db.player_state().identity().update(state);
        Ok(())
    } else {
        Err("Player not registered — call update_player_position first".into())
    }
}
```

---

## 5. Client-Side Subscription Queries

The Bevy client subscribes to chunk-scoped data. SpacetimeDB evaluates these server-side and pushes deltas.

```rust
// crates/world-client/src/subscriptions.rs
// Pseudocode — actual syntax depends on spacetimedb-sdk version

/// Subscribe to the 3×3 chunk grid around the player.
/// Called on initial connect and whenever the player crosses
/// a chunk boundary (with hysteresis: >16 units into new chunk).
fn subscribe_to_area(conn: &DbConnection, center_chunk: i64) {
    let (cx, cy) = unpack_chunk_key(center_chunk);
    let mut chunk_keys = Vec::with_capacity(9);
    for dx in -1..=1 {
        for dy in -1..=1 {
            chunk_keys.push(pack_chunk_key(cx + dx, cy + dy));
        }
    }

    // Objects + transforms in nearby chunks
    conn.subscribe(&[
        format!(
            "SELECT * FROM world_object WHERE chunk_key IN ({})",
            chunk_keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",")
        ),
        format!(
            "SELECT * FROM object_transform WHERE chunk_key IN ({})",
            chunk_keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",")
        ),
        // Audio sources in nearby chunks
        format!(
            "SELECT * FROM audio_source WHERE chunk_key IN ({})",
            chunk_keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",")
        ),
        format!(
            "SELECT * FROM audio_transform WHERE chunk_key IN ({})",
            chunk_keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",")
        ),
        // Chunk metadata for LOD (wider radius)
        "SELECT * FROM chunk_meta".to_string(),
        // Creations for undo UI
        format!(
            "SELECT * FROM creation WHERE chunk_key IN ({})",
            chunk_keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",")
        ),
    ]);
}
```

---

## 6. Client-Side Bevy Integration

### 6.1 SpacetimeDB Row → Bevy Entity

```rust
// crates/world-client/src/sync.rs

/// Called when SpacetimeDB pushes a new WorldObject row.
fn on_world_object_insert(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    obj: &WorldObject,
    transform: &ObjectTransform,
) {
    let shape: PrimitiveShape = serde_json::from_slice(&obj.shape_data).unwrap();
    let mat: PbrMaterial = serde_json::from_slice(&obj.material_data).unwrap();

    let mesh_handle = meshes.add(shape_to_bevy_mesh(&shape));
    let material_handle = materials.add(StandardMaterial {
        base_color: Color::linear_rgba(mat.color[0], mat.color[1], mat.color[2], mat.color[3]),
        metallic: mat.metallic,
        perceptual_roughness: mat.roughness,
        emissive: LinearRgba::new(
            mat.emissive[0], mat.emissive[1], mat.emissive[2], mat.emissive[3]
        ),
        ..default()
    });

    commands.spawn((
        Mesh3d(mesh_handle),
        MeshMaterial3d(material_handle),
        Transform {
            translation: Vec3::new(transform.x, transform.y, transform.z),
            rotation: Quat::from_euler(
                EulerRot::XYZ,
                transform.rot_pitch.to_radians(),
                transform.rot_yaw.to_radians(),
                transform.rot_roll.to_radians(),
            ),
            scale: Vec3::new(transform.scale_x, transform.scale_y, transform.scale_z),
        },
        // Tag component linking Bevy entity to SpacetimeDB row
        ServerObjectId(obj.id),
        ServerCreationId(obj.creation_id),
    ));
}

fn shape_to_bevy_mesh(shape: &PrimitiveShape) -> Mesh {
    match shape {
        PrimitiveShape::Cuboid { x, y, z } =>
            Cuboid::new(*x, *y, *z).into(),
        PrimitiveShape::Sphere { radius } =>
            Sphere::new(*radius).into(),
        PrimitiveShape::Cylinder { radius, height } =>
            Cylinder::new(*radius, *height).into(),
        PrimitiveShape::Cone { radius, height } =>
            Cone { radius: *radius, height: *height }.into(),
        PrimitiveShape::Capsule { radius, length } =>
            Capsule3d::new(*radius, *length).into(),
        PrimitiveShape::Torus { minor_radius, major_radius } =>
            Torus::new(*minor_radius, *major_radius).into(),
        PrimitiveShape::Plane { width, depth } =>
            Plane3d::default().mesh().size(*width, *depth).into(),
        PrimitiveShape::ConicalFrustum { top_radius, bottom_radius, height } =>
            ConicalFrustum {
                radius_top: *top_radius,
                radius_bottom: *bottom_radius,
                height: *height,
            }.into(),
    }
}
```

### 6.2 Audio Source → Bevy Playback

```rust
// crates/world-client/src/audio_sync.rs

/// Called when SpacetimeDB pushes a new AudioSource row.
fn on_audio_source_insert(
    commands: &mut Commands,
    audio: &AudioSource,
    transform: &AudioTransform,
    abc_renderer: &AbcRenderer,         // custom ABC→PCM pipeline
    ambient_engine: &AmbientEngine,     // preset-based synth
) {
    let kind: AudioKind = serde_json::from_slice(&audio.kind_data).unwrap();
    let synth: SynthParams = serde_json::from_slice(&audio.synth_data).unwrap();
    let spatial: SpatialMode = serde_json::from_slice(&audio.spatial_data).unwrap();

    match kind {
        AudioKind::Music { abc } => {
            // Parse ABC → MIDI events → render to PCM buffer
            let pcm = abc_renderer.render(&abc, &synth);
            let source = AudioSourceBundle::from_pcm(pcm, audio.looping);
            let mut entity = commands.spawn((
                source,
                Transform::from_xyz(transform.x, transform.y, transform.z),
                ServerAudioId(audio.id),
            ));
            if audio.autoplay {
                entity.insert(PlaybackSettings::LOOP.with_spatial(
                    matches!(spatial, SpatialMode::Positional { .. }
                                    | SpatialMode::Attached { .. })
                ));
            }
        }
        AudioKind::Ambient { preset, params } => {
            // Start procedural ambient synth (FunDSP/Glicol)
            let synth_node = ambient_engine.create_preset(&preset, &params);
            commands.spawn((
                synth_node,
                Transform::from_xyz(transform.x, transform.y, transform.z),
                ServerAudioId(audio.id),
            ));
        }
        AudioKind::Sfx { graph_json } => {
            // Register SFX graph — will be triggered by events, not autoplay
            // Store for later triggering
            commands.spawn((
                SfxTemplate { graph_json, synth },
                Transform::from_xyz(transform.x, transform.y, transform.z),
                ServerAudioId(audio.id),
            ));
        }
    }
}
```

---

## 7. Memory Budget Analysis

Per-object server memory with SpacetimeDB (all in-memory):

| Component | Bytes per object | Notes |
|-----------|-----------------|-------|
| WorldObject row | ~220 | shape_data ~80, material_data ~48, rest fixed |
| ObjectTransform row | ~44 | 9 floats + object_id + chunk_key |
| Creation row (shared) | ~120 / N objects | amortized across creation's objects |
| BTree index entries | ~64 | chunk_key + id indexes |
| **Total per primitive** | **~330** | |
| AudioSource row | ~500–2500 | ABC strings vary; ambient ~200 |
| AudioTransform row | ~20 | 3 floats + audio_id + chunk_key |
| **Total per audio** | **~520–2520** | |

At 2000 objects per chunk and 64×64m chunks, a 1km × 1km area (256 chunks) holds ~512K objects consuming ~170MB. A single SpacetimeDB instance with 32GB RAM supports roughly a 3km × 3km active world before requiring sharding.

---

## 8. LLM-to-Reducer Pipeline

The client-side LLM agent constructs a `CreationSpec` JSON from the user's natural language prompt. The flow:

```
User: "Build a small cabin with a fireplace that plays cozy music"
                    │
                    ▼
┌─────────────────────────────────────────────────┐
│  LLM (function calling mode)                     │
│  System prompt includes:                         │
│  - CreationSpec JSON schema                      │
│  - SemanticCategory size limits                  │
│  - Nearby objects from subscription cache        │
│  - Available ambient presets                     │
│  - ABC notation basics + few-shot examples       │
│                                                  │
│  Output: CreationSpec JSON with:                 │
│  - 8 primitives (floor, 4 walls, roof, chimney, │
│    fireplace_opening)                            │
│  - 1 audio source (Music, ABC: cozy melody)      │
│  - 1 audio source (Ambient, "fire_crackle")      │
│  - 1 light (Point, warm orange, inside fireplace)│
└──────────────────┬──────────────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────────────┐
│  Client-side pre-validation                       │
│  - Category bounds check (fast, no server call)   │
│  - ABC structural check (X:, K: headers present)  │
│  - Triangle budget estimate                       │
└──────────────────┬──────────────────────────────┘
                   │ if valid
                   ▼
┌──────────────────────────────────────────────────┐
│  SpacetimeDB reducer call                         │
│  conn.call("place_creation", spec_json)           │
│  Server validates + commits atomically            │
└──────────────────┬──────────────────────────────┘
                   │ subscription delta
                   ▼
┌──────────────────────────────────────────────────┐
│  All connected clients in range receive:          │
│  - 8 WorldObject inserts                          │
│  - 8 ObjectTransform inserts                      │
│  - 2 AudioSource inserts                          │
│  - 2 AudioTransform inserts                       │
│  - 1 Creation insert                              │
│                                                   │
│  Each client's Bevy sync system:                  │
│  - Spawns meshes + materials                      │
│  - Starts ABC → PCM render for music              │
│  - Starts FunDSP fire_crackle ambient             │
│  - Spawns point light in fireplace                │
└──────────────────────────────────────────────────┘
```

---

## 9. Implementation Checklist

### Phase 1: world-types crate (Week 1)

- [ ] Create `crates/world-types/Cargo.toml` with only `serde`, `serde_json`
- [ ] Implement `spatial.rs`: `CHUNK_SIZE`, `pack/unpack_chunk_key`, `world_to_chunk`, `Aabb`
- [ ] Implement `primitives.rs`: `PrimitiveShape` enum with `local_aabb()` and `estimated_triangles()`
- [ ] Implement `material.rs`: `PbrMaterial` with defaults matching `gen_spawn_primitive`
- [ ] Implement `transform.rs`: `WorldTransform` (Euler degrees, matching LLM tool interface)
- [ ] Implement `light.rs`: `LightKind`, `LightDef`
- [ ] Implement `audio.rs`: `AudioKind`, `SynthParams`, `SpatialMode`
- [ ] Implement `categories.rs`: `SemanticCategory` with size limits and setbacks
- [ ] Implement `creation.rs`: `CreationSpec`, `PrimitiveDef`, `AudioSourceDef`, `LightPlacement`
- [ ] Implement `events.rs`: `EventType` enum
- [ ] Unit tests: AABB math, chunk key packing, category bounds, shape AABB computation
- [ ] Verify: `cargo check -p localgpt-world-types --target wasm32-unknown-unknown`

### Phase 2: world-server crate (Week 2–3)

- [ ] Create `crates/world-server/Cargo.toml` with `spacetimedb`, `localgpt-world-types`
- [ ] Implement `tables.rs`: all 8 SpacetimeDB tables
- [ ] Implement `reducers.rs`: `place_creation`, `delete_creation`, `undo`, `update_player_position`
- [ ] Implement `validation.rs`: category bounds, audio validation, overlap check, rate limiting
- [ ] Add `modify_creation` reducer (change transform, material, add/remove primitives)
- [ ] Add `modify_audio` reducer (update ABC, change preset, adjust synth params)
- [ ] Build: `cd crates/world-server && spacetime build`
- [ ] Deploy to local SpacetimeDB: `spacetime publish localgpt-world-dev`
- [ ] Integration test: place creation via CLI → verify subscription delta

### Phase 3: world-client crate (Week 3–4)

- [ ] Create `crates/world-client/Cargo.toml` with `spacetimedb-sdk`, `bevy`, `localgpt-world-types`
- [ ] Implement `subscriptions.rs`: chunk-based subscribe/unsubscribe with hysteresis
- [ ] Implement `sync.rs`: `on_world_object_insert/update/delete` → Bevy entity spawn/update/despawn
- [ ] Implement `audio_sync.rs`: `on_audio_source_insert` → ABC render + ambient synth
- [ ] Wire into `localgpt-gen` behind `--features multiplayer`
- [ ] Test: two Bevy clients see each other's creations in real time

### Phase 4: LLM integration (Week 4–5)

- [ ] Extend `gen_spawn_primitive` to output `CreationSpec` JSON instead of local Bevy commands
- [ ] Add `gen_spawn_audio` tool for the LLM to create audio sources
- [ ] Inject nearby object context from subscription cache into LLM system prompt
- [ ] Add ABC few-shot examples to system prompt
- [ ] End-to-end test: user says "build a cabin with music" → both clients see + hear it

---

## 10. Open Questions

**Q1: Voxel support.** The existing spec mentions `gen_spawn_voxel`. Voxels are fundamentally different from primitives — they're dense grid data, not parametric shapes. Should voxels be a separate table with chunk-aligned `Vec<u8>` occupancy grids, or flattened into `WorldObject` rows? Recommendation: separate `VoxelChunk` table with RLE-compressed data per chunk, Phase 2.

**Q2: Custom meshes.** `gen_spawn_mesh` allows raw vertex/index data. Storing arbitrary mesh data in SpacetimeDB rows is expensive (potentially MB per mesh). Options: (a) store mesh hash + CDN URL, (b) decompose into primitives server-side, (c) defer to Phase 2. Recommendation: (a) for now, with server validating bounding box only.

**Q3: Audio source limits per creation.** Should there be a hard limit? Recommendation: 4 audio sources per creation, 100 per chunk. Each ABC string under 8KB. This keeps memory bounded.

**Q4: Cross-chunk creations.** A large castle may span chunk boundaries. Options: (a) reject — force fit within one chunk, (b) duplicate rows in each overlapping chunk, (c) assign to primary chunk with boundary flag. Recommendation: (a) for Phase 1, (b) for Phase 2 with `multi_chunk: bool` flag on Creation.
