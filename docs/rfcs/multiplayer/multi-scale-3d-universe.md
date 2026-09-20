# Multi-scale 3D universe architecture for Bevy + SpacetimeDB

> **Parent spec:** [3D Collaborative World Engine: Architecture Specification](collaborative-world-engine-architecture.md) — this document is the deep-dive on coordinate systems and spatial partitioning at planetary scale (§2).

**A continuous universe spanning millimeters to light-years is achievable with today's tools.** The core architecture combines `big_space`'s hierarchical integer grids for precision, cube-sphere CDLOD quadtrees for planetary terrain, composite BTree keys in SpacetimeDB for spatial persistence, and a deterministic seed-plus-delta content pipeline where LLMs enrich procedural foundations. Every shipping game that solves this problem — No Man's Sky, Star Citizen, Elite Dangerous, Space Engineers — converges on the same fundamental pattern: **64-bit positions on the CPU, 32-bit camera-relative rendering on the GPU, and hierarchical coordinate frames that keep local math small**. Bevy's ecosystem now has the pieces to implement this, most critically the `big_space` crate (0.12.0, Bevy 0.18) which eliminates the need for custom floating-origin infrastructure.

---

## A. Coordinate system: hierarchical integer grids solve precision forever

The fundamental enemy is IEEE 754 float32 precision decay. At **16,777 km** from origin (roughly Earth's diameter), float32 can only resolve positions to **1 meter** — completely unusable for human-scale gameplay. Even float64 begins losing millimeter precision at ~9 billion km (60 AU). Every shipping space game has learned this the hard way: KSP's infamous "Deep Space Kraken" destroyed ships at high velocities due to float32 catastrophic cancellation, prompting the "Krakensbane" floating-origin system.

### The precision landscape

| Target precision | float32 max distance | float64 max distance |
|---|---|---|
| 0.1 mm | 1.68 km | 900 million km |
| 1 mm | 16.8 km | 9 billion km |
| 1 cm | 168 km | 90 billion km |
| 1 m | 16,777 km | 9 trillion km (~0.95 ly) |

The industry has converged on a clear hierarchy of solutions. **Star Citizen** uses 64-bit CPU positions with camera-relative 32-bit GPU rendering plus nested "zone host" coordinate frames. **Elite Dangerous** packs galaxy-scale addresses into 64-bit integers encoding sector octree layers. **Space Engineers** uses 64-bit global positions with 32-bit Havok physics clusters. **Outerra** uses per-tile local coordinate systems. All render in 32-bit on the GPU — no exceptions.

### big_space: the Bevy solution

The `big_space` crate (334 GitHub stars, actively maintained by Aevyrie) provides a superior approach: **hierarchical integer grids with compile-time precision selection**. Rather than periodically rebasing the origin (which triggers change detection on every entity and causes drift), big_space stores positions as `CellCoord<P>` (integer grid cell) + `Transform` (f32 offset within cell). A `Transform` on an i32 grid gives **64 bits of effective precision**. With i128 grids, you get **160 bits** — sufficient for proton-sized meshes across the observable universe.

The architecture maps directly to the reference-frame hierarchy:

```
BigSpace (universe root)
 └─ Grid<i128>: galaxy scale
     └─ Grid<i64>: solar system (star at origin)
         ├─ Grid<i32>: Planet A (rotating, orbiting parent)
         │   ├─ Terrain chunks
         │   ├─ Surface buildings, NPCs
         │   └─ Player camera ← FloatingOrigin
         ├─ Grid<i32>: Planet B
         └─ Grid<i32>: Space station
             └─ Grid<i16>: ship interior (physics scene)
```

The `FloatingOrigin` marker component designates which entity is the rendering reference point. All `GlobalTransform` values are computed relative to this entity automatically. Crucially, **entities in a nested grid inherit parent motion for free** — a player standing on a planet that orbits a star doesn't need per-frame orbital transform computation. The planet's grid moves; everything inside it moves with it.

### Frame transitions

When an entity leaves a planet's surface for orbit, it transitions between grids:

1. Compute absolute position by walking up the frame hierarchy (child grid → parent grid → ... → root)
2. Express the position in the target frame's local coordinates
3. Transform velocity vectors to account for frame rotation and linear velocity
4. Re-register with the physics engine in the new frame

This is explicit and event-driven — no continuous polling. Star Citizen's zone system works identically: entities store positions relative to their "zone host," and crossing a zone boundary triggers coordinate re-expression.

---

## B. Spatial indexing in SpacetimeDB: composite BTree keys on cube-sphere faces

The chunk system must span multiple celestial bodies, each with spherical geometry, at varying levels of detail, while supporting efficient database queries. The recommended structure is a **cube-sphere mapping** where each planet is a cube projected onto a sphere (6 faces), each face containing a CDLOD quadtree of chunks.

### Chunk key design

The primary key for surface chunks uses SpacetimeDB's multi-column BTree index, which supports **prefix matching** — an index on `(body_id, face_id, lod_level, chunk_x, chunk_y)` efficiently handles queries for "all chunks on body 5" (prefix scan), "all chunks on face 2 of body 5" (compound prefix), and "chunks near (100, 200) at LOD 4" (range scan on tail columns).

```rust
#[table(
    accessor = surface_chunk,
    public,
    index(accessor = spatial_idx, btree(columns = [body_id, face_id, lod_level, chunk_x, chunk_y]))
)]
pub struct SurfaceChunk {
    #[primary_key]
    #[auto_inc]
    pub chunk_id: u64,
    pub body_id: u64,
    pub face_id: u8,        // 0-5 for cube-sphere faces
    pub lod_level: u8,      // 0 = coarsest, 23 = meter-level on Earth
    pub chunk_x: i32,
    pub chunk_y: i32,
    pub terrain_seed: u64,
    pub biome_type: u8,
    pub modified: bool,     // player edits exist as deltas
}
```

This composite key was chosen over Morton-coded u64 keys because SpacetimeDB subscription SQL doesn't support custom encoding functions, making BIGMIN/LITMAX range calculations on Morton codes impractical. The composite key maps directly to readable SQL subscriptions.

### Entity placement across scales

Entities exist at radically different scales and must be queryable uniformly. The solution is a discriminated union for location plus **denormalized system-frame coordinates** for cross-scale proximity queries:

```rust
#[derive(SpacetimeType, Clone)]
pub enum EntityLocation {
    OnSurface { body_id: u64, face_id: u8, latitude: f64, longitude: f64, altitude: f32 },
    InOrbit { parent_body_id: u64, orbit_radius: f64, orbit_angle: f64 },
    InSpace { sys_x: f64, sys_y: f64, sys_z: f64 },  // AU
}
```

The `current_body_id` field (nullable, indexed) enables fast queries like "all entities on/near body 5" regardless of whether they're on the surface, in orbit, or flying past.

### Multi-scale subscription strategy

A player on a planet surface needs simultaneous data at multiple scales with different update frequencies:

- **Nearby surface chunks** (100m radius, high detail): `WHERE body_id = 5 AND face_id = 2 AND lod_level >= 4 AND chunk_x BETWEEN 90 AND 110 AND chunk_y BETWEEN 190 AND 210`
- **Regional chunks** (10km, low LOD): `WHERE body_id = 5 AND lod_level <= 2`
- **Celestial body registry** (all planets, low frequency): `SELECT * FROM celestial_body`
- **Nearby entities** (same body): `WHERE current_body_id = 5`
- **Chunk modifications** (deltas for visible chunks): joined to chunk subscription

When the player transitions to orbit, the client **unsubscribes** from high-LOD surface subscriptions and subscribes to orbital-view ones. SpacetimeDB supports independent subscription handles with dynamic subscribe/unsubscribe. The base terrain is never stored — only the seed (in `terrain_seed`) and player deltas (in a `chunk_modification` table). All clients regenerate identical terrain from the same seed, applying identical deltas.

---

## C. Rendering pipeline: CDLOD quadtrees on cube-sphere with reverse-Z

### Planetary terrain geometry

The cube-sphere with **CDLOD (Continuous Distance-Dependent LOD)** quadtrees is the dominant approach across shipping games. No Man's Sky, Star Citizen, Elite Dangerous, and Outerra all use variants. Each of the 6 cube faces hosts an independent quadtree. For an Earth-sized planet (**6,371 km radius**) with 1-meter surface detail and 64×64 vertex patches per node, the quadtree needs approximately **18-23 levels**:

| Quadtree level | Node size | Use case |
|---|---|---|
| 0 | ~12,742 km | Entire face |
| 5 | ~398 km | Continental view |
| 10 | ~12.4 km | Regional |
| 15 | ~389 m | Village-scale |
| 18 | ~48.6 m | Building-scale |
| 23 | ~1.52 m | Meter-level detail |

CDLOD renders each quadtree node using a **single reusable grid mesh** (e.g., 32×32) repositioned and scaled by the vertex shader, which samples a heightmap texture and applies sphere projection. Geomorphing is built in — vertices at LOD boundaries smoothly interpolate to the coarser grid, eliminating visible pops. Adjacent nodes enforce a maximum 1-level LOD difference to prevent T-junctions.

The tangent-space cube-to-sphere projection (`f(t) = tan(t × π/4)`) reduces area distortion from **5.2× at corners** (standard gnomonic) to **1.22×**, making cell sizes near-uniform across the face. Outerra pioneered this; it's now standard practice.

### When curvature matters

Surface curvature drop follows `d² / (2R)`. At **10 km** on an Earth-sized planet, the drop is **7.85 meters** — clearly visible for distant buildings and terrain. Below 1 km, the drop is under 8 cm: safely flat for gameplay physics and collision. The practical rule: **use a flat local patch for physics within 1 km, spherical rendering for anything beyond 10 km, and a blending zone between**.

### Underground and caves

Heightmaps cannot represent caves, tunnels, or overhangs. The recommended **hybrid approach** layers three systems:

1. **Heightmap terrain** covers 95%+ of the surface cheaply (~2-4 KB per chunk). Most terrain is simple elevation data.
2. **Sparse SDF (Signed Distance Field) overlays** add volumetric features only where caves or overhangs exist. These are stored as density fields in octree nodes (typically 16³ or 32³ voxels per node). The **Transvoxel algorithm** generates crack-free meshes at LOD boundaries — essential for multi-resolution voxel terrain.
3. **Player modifications** are delta-based SDF edits (mining, building) stored in SpacetimeDB's `chunk_modification` table.

No Man's Sky uses full voxel terrain everywhere (7 noise layers including cave data, meshed with a marching-cubes variant). This enables universal cave generation but costs **5-20× more** than heightmap rendering. The hybrid approach pays the voxel cost only where volumetric features exist. For a planet that's 95% simple terrain, this saves enormous compute.

### Depth buffer and extreme ranges

**Bevy already uses reverse-Z with float32 depth and infinite far plane.** This is confirmed in Bevy's source: `Mat4::perspective_infinite_reverse_rh(fov, aspect, near)` with `DepthStencilState { depth_compare: GreaterEqual }` and depth cleared to 0.0. The quasi-logarithmic precision distribution this provides means sub-millimeter depth precision to ~1000m and graceful degradation beyond. No custom depth pipeline is needed.

For atmosphere rendering, **Bevy 0.18 includes built-in physically-based atmospheric scattering** based on Hillaire's 2020 EGSR paper (Rayleigh + Mie scattering, customizable via `ScatteringMedium` assets). This covers surface-level sky rendering. For the space-to-surface view (atmosphere seen from orbit as a glowing limb), a custom atmosphere shell mesh or post-process pass would supplement the built-in system.

### The LOD cascade from surface to space

| Altitude | Representation | What's active |
|---|---|---|
| 0-100m | Full terrain + foliage + objects | Quadtree levels 18-23, physics, NPCs |
| 100m-10km | Terrain mesh, simplified vegetation | Levels 12-18, reduced entities |
| 10-1000km | Coarse terrain + atmosphere shell | Levels 3-10, no surface objects |
| 1000-100,000km | Planet sphere + atmosphere + clouds | Levels 0-5, analytical sphere |
| >100,000km | Textured billboard / point light | Impostor or procedural dot |

---

## D. Bevy implementation: the concrete crate stack

### Core dependencies (Bevy 0.18)

| Layer | Crate | Purpose |
|---|---|---|
| Floating origin | `big_space` 0.12.0 | Hierarchical integer grids, automatic origin rebasing |
| Physics | `avian3d` 0.6 with `f64` feature | ECS-native XPBD physics with double precision |
| Terrain | `bevy_terrain` (git) | UDLOD terrain with experimental spherical support |
| Atmosphere | Built-in `Atmosphere` | Hillaire 2020 scattering, `ScatteringMedium` customization |
| Depth buffer | Built-in reverse-Z | Infinite far plane, float32 depth — already default |
| Voxel terrain | `bevy_voxel_world` | Caves/underground with greedy meshing |

### big_space integration pattern

big_space replaces the need for manual origin rebasing. Entities get a `CellCoord<P>` component (where P is the integer precision type: i32, i64, i128) plus a standard `Transform` for sub-cell positioning. The `FloatingOrigin` marker on the camera entity causes all `GlobalTransform` computation to be relative to that entity's position.

Key architectural decisions:
- **Solar system grid**: `Grid<i64>` — covers ~9.2 × 10¹⁸ units. With 1-meter cells, this spans ~9.7 light-years. With 1-km cells, it spans the observable universe.
- **Planet surface grid**: `Grid<i32>` nested inside the solar system grid. With 1-meter cells, covers ~4.3 billion meters (~4.3 million km) — sufficient for any planet.
- **Ship/station interior**: `Grid<i16>` nested inside a ship entity. Covers ~65 km at meter resolution — plenty for any structure.

Entities auto-migrate between cells when their `Transform` translation exceeds cell boundaries. No manual rebasing code is needed.

### Avian physics at planetary scale

Avian 0.6 supports **compile-time f64 selection** via the `f64` feature flag, doubling numerical precision for the physics solver, collision detection (Parry f64), and constraint solving. This eliminates the float32 physics breakdown that plagues Space Engineers and KSP.

The physics integration strategy:

- **On surface**: Standard constant gravity via Avian's `Gravity` resource. Physics runs only on entities within a few grid cells of the player. Distant entities are "on rails" (scripted or orbital motion, no rigid-body simulation).
- **In space**: Custom gravity system applying `ExternalForce` based on inverse-square law. Orbital mechanics for non-focused vessels use Keplerian analytical solutions (same as KSP's patched conics).
- **Transition zone**: Blend between surface gravity and orbital mechanics based on altitude. Frame transition triggers physics re-registration in the new coordinate grid.

Avian maintains separate `Position`/`Rotation` components from Bevy's `Transform`, synchronized bidirectionally via `PhysicsTransformPlugin`. Since big_space keeps `Transform` values small (always relative to the current cell center), Avian sees well-conditioned coordinates regardless of absolute position.

### System execution order

```
FixedUpdate:
  1. update_orbital_mechanics    // Kepler for celestial bodies
  2. update_entity_positions     // Gameplay movement

PreUpdate:
  3. big_space auto-migration    // CellCoord adjustments
  4. sync_cosmic_to_transform    // High-precision → Transform

FixedPostUpdate:
  5. Avian PhysicsSchedule       // Nearby entities only

PostUpdate:
  6. TransformPropagate          // Bevy computes GlobalTransform
  7. update_lod_levels           // Distance-based LOD switching
  8. manage_chunk_streaming      // Load/unload terrain chunks
  9. update_subscriptions        // Adjust SpacetimeDB subscriptions
```

---

## E. Phased build order: surface first, then expand outward

### Phase 1 — Grounded world (months 1-3)

Start with a single flat terrain patch, no planetary curvature. This validates the core stack.

- Implement `big_space` with a single `Grid<i32>` for the surface
- Flat heightmap terrain using `bevy_terrain` or custom CDLOD on a plane
- Avian physics (f64) for player movement and object interaction
- SpacetimeDB schema: `surface_chunk` table with `(body_id=1, face_id=0, lod_level, chunk_x, chunk_y)` — single body, single face, flat grid
- MCP tools: `place_building()`, `create_npc()`, `define_biome()` — AI generates content on the flat surface
- Chunk streaming: load/unload chunks based on player distance
- **Deliverable**: Player walks around an AI-generated landscape with buildings and NPCs, multiplayer-synced via SpacetimeDB

### Phase 2 — Vertical dimension (months 4-5)

Add underground caves and multi-story buildings.

- Extend chunk key to include depth: `(body_id, face_id, lod_level, chunk_x, chunk_y, chunk_z)`
- Implement hybrid terrain: heightmap surface + sparse SDF overlays for caves
- Transvoxel meshing for volumetric terrain at LOD boundaries
- Building interiors as nested `Grid<i16>` spaces
- SpacetimeDB: `chunk_modification` table for terrain edits (mining, digging)
- MCP tools: `create_cave_system()`, `design_interior()` for AI-generated underground and indoor content
- **Deliverable**: Player can dig underground, enter buildings, explore AI-generated cave systems

### Phase 3 — Planetary (months 6-9)

Wrap the flat surface onto a sphere. This is the biggest architectural leap.

- Convert flat terrain to cube-sphere: 6 faces, each a CDLOD quadtree
- Implement sphere projection in the vertex shader (tangent-space correction)
- Handle cube-face edge seams (1-cell overlap borders, vertex stitching at corners)
- Atmosphere rendering using Bevy's built-in `Atmosphere` + `ScatteringMedium`
- Gravity aligned to planet center (Avian `ExternalForce` toward body center)
- Horizon culling (cull terrain beyond the geometric horizon)
- Nested `Grid<i32>` for the planet inside a `Grid<i64>` solar system grid
- SpacetimeDB: `celestial_body` table with orbital parameters, scheduled reducer updating positions
- **Deliverable**: Player stands on a spherical planet, sees curvature at distance, atmosphere renders correctly

### Phase 4 — Orbital and interplanetary (months 10-13)

Add the ability to leave the surface and travel between planets.

- Surface-to-orbit transition: entity migrates from planet's `Grid<i32>` to solar system's `Grid<i64>`
- LOD cascade: close-up terrain → distant planet sphere → impostor → point light
- Orbital mechanics: Keplerian for on-rails vessels, Avian physics for active flight
- Multiple planets in system, each with procedural terrain generated from unique seeds
- Warp/jump mechanic to solve the "space is empty" problem (see below)
- SpacetimeDB: multi-body subscription management — subscribe to surface detail of current planet, low-LOD positions of other bodies
- MCP tools: `generate_star_system()`, `place_celestial_body()`, planet-scale biome generation
- **Deliverable**: Player flies between planets in a solar system, lands on different worlds

### Phase 5 — Interstellar (months 14-16)

Extend to multiple star systems with galaxy-scale navigation.

- `Grid<i128>` at the universe root for galaxy-scale distances
- Star system procedural generation from galaxy seed (inspired by Elite's Stellar Forge)
- Hyperspace/jump mechanic between systems (loading screen or warp tunnel masks streaming)
- Galaxy map UI showing star positions, system info, routes
- SpacetimeDB: star system registry, inter-system travel state
- MCP tools: `name_star_system()`, `create_faction()`, `generate_system_lore()` — LLM creates narrative context for procedural systems
- **Deliverable**: Full galaxy navigation, procedural + AI-enriched star systems

---

## F. Content generation pipeline: seeds for structure, LLMs for soul

The content pipeline splits cleanly between **deterministic procedural generation** (same on every client, never stored) and **AI-enriched content** (non-deterministic, always persisted in SpacetimeDB).

### What procedural seeds handle (computed client-side, not stored)

- Terrain heightmaps at all LOD levels — fBM noise with ridged multifractal and domain warping
- Biome distribution — temperature (latitude + altitude), moisture (distance from coast + noise), mapped via Whittaker diagram
- Vegetation and rock placement — position hashing from terrain seed
- Star system orbital parameters — deterministic from galaxy seed → sector hash → system seed
- Cave geometry — 3D noise defining density fields within planetary crust

All noise evaluation is deterministic: identical seed + identical position = identical output on every client. The PRNG algorithm must be platform-consistent (avoid platform-dependent floating-point behavior; prefer integer-based noise or explicit rounding).

### What LLMs generate via MCP tools (persisted in SpacetimeDB)

At each scale, the LLM receives procedural context and adds creative intent:

- **Solar system**: Naming, faction history, trade routes, political tensions → `generated_lore` table
- **Planet**: Biome descriptions, point-of-interest placement rationale, narrative hooks → `settlement`, `generated_lore`
- **Settlement**: Culture style, architecture choices, NPC backstories, economic specialization → `settlement`, NPC tables
- **Building/Room**: Interior decoration reflecting occupant personality, environmental storytelling details, quest-relevant items → `chunk_modification`

The MCP tool interface gives the AI world-builder structured actions: `place_building(type, position, style)`, `create_npc(role, personality, backstory)`, `add_lore(target, content)`. The LLM calls these tools; the reducers validate and persist results in SpacetimeDB.

### The generation pipeline

```
Phase 1 (Seeding — offline):
  galaxy_seed → sector_hash(x,y,z) → system_seed → star + planet parameters
  → stored in celestial_body table

Phase 2 (Enrichment — on first player approach):
  LLM receives: {system_params, planet_params, faction_context}
  LLM calls: create_settlement(), add_lore(), create_npc()
  → stored in settlement, generated_lore, entity tables

Phase 3 (Detail — on landing/entry):
  LLM receives: {settlement_params, building_list, npc_list}
  LLM calls: place_building(), design_interior(), add_dialogue()
  → stored as chunk_modification, dialogue tables

Phase 4 (Runtime — during gameplay):
  LLM powers: NPC dialogue, quest progression, dynamic events
  Uses: RAG over stored lore for narrative consistency
```

### What changes for existing MCP tools

Current MCP tools designed for flat 2D chunk placement need these extensions:

- **Coordinate parameters** expand from `(chunk_x, chunk_y)` to `(body_id, face_id, chunk_x, chunk_y, chunk_z)` or equivalently `(body_id, latitude, longitude, altitude)` for surface placement
- **Scale context parameter** tells the AI what scale it's operating at: room, building, settlement, region, planet, system — this constrains which tools are available and what parameters make sense
- **New tool categories** for celestial-scale generation: `generate_star_system()`, `place_celestial_body()`, `define_orbit()`, `set_planet_properties()`
- **Template system** becomes hierarchical: room templates compose into building templates, building templates into settlement templates, settlement distributions into biome templates, biome configurations into planet templates
- **Existing world generation tools continue to work unchanged** for surface-level content — they just gain a `body_id` parameter to specify which planet they're operating on

---

## Conclusion: the architecture is proven, the tools exist

The multi-scale universe problem is solved in production games. No Man's Sky proves voxel planets with caves work. Star Citizen proves nested coordinate frames with seamless transitions work. Elite Dangerous proves galaxy-scale procedural generation works. Space Engineers proves 64-bit positioning with physics clustering works. The novel contribution here isn't inventing new techniques — it's **assembling proven patterns into a Bevy + SpacetimeDB stack** and adding LLM-driven content enrichment.

Three non-obvious insights emerged from this research. First, `big_space`'s integer-grid approach is **strictly superior** to the floating-origin-rebasing used by KSP and Unity games, because it eliminates accumulated drift and avoids triggering change detection on all entities during rebasing. Second, Bevy's rendering pipeline already handles the depth-buffer problem — reverse-Z with infinite far plane is the default, not something you need to implement. Third, the deterministic-seed-plus-stored-delta pattern used by every multiplayer procedural game maps perfectly to SpacetimeDB's subscription model: seeds are client-side constants, deltas are subscribed rows, and the LLM's creative contributions are just a special class of delta that gets persisted exactly once and replicated to all clients.

The riskiest phase is Phase 3 (planetary). Wrapping flat terrain onto a sphere, handling cube-face seams, and implementing the surface-to-orbit frame transition are the hardest engineering challenges. Everything before and after is incremental. Start there in prototyping — build a bare cube-sphere with CDLOD early — and the rest of the architecture will follow.