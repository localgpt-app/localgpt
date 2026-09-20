# Architecture for a massively multiplayer AI-driven 3D world co-creation platform

> **Parent spec:** [3D Collaborative World Engine: Architecture Specification](collaborative-world-engine-architecture.md) — this document is the deep-dive on Spatial Interest Management and the Asynchronous AI Inference Pool (§2).

**SpacetimeDB combined with Bevy and LLM-powered content generation can deliver an MMO-scale co-creation platform, but the architecture requires careful spatial sharding, a multi-stage AI pipeline, and Figma-style conflict resolution rather than full CRDTs.** The critical insight from BitCraft Online — the flagship SpacetimeDB MMO — is that infinite-world scale demands multiple SpacetimeDB instances partitioned spatially, since each instance holds all data in memory on a single machine. This report provides concrete data models, pipeline designs, and trade-off analysis across all six architectural areas, grounded in how SpacetimeDB actually works (reducer pattern, subscription queries, WASM modules) and what real systems like BitCraft, Figma, Roblox, and SpatialOS have proven at scale.

---

## SpacetimeDB's architecture shapes everything

SpacetimeDB is a **relational database that doubles as the application server**. Game logic compiles to WebAssembly and runs inside the database. Clients connect directly via WebSocket — there is no separate server layer. Three primitives define the programming model:

**Tables** are annotated Rust structs stored in-memory with automatic disk persistence via write-ahead log. They support BTree indexes, primary keys, auto-increment, and unique constraints. Supported column types include all numeric types, strings, `Vec<T>`, `Option<T>`, custom structs, and `Identity` (unique per-client cryptographic identifier).

**Reducers** are the sole mechanism for state mutation. Each reducer call executes as an **ACID transaction** — atomic, isolated, and durable. If a reducer returns an error or panics, all changes roll back. Clients invoke reducers via WebSocket; reducers never return data directly. State flows to clients exclusively through subscriptions.

**Subscription queries** let clients subscribe to SQL `SELECT * FROM table WHERE ...` queries. SpacetimeDB sends all matching rows initially, then pushes **incremental deltas** as rows change. Delta evaluation is efficient: the WHERE clause applies only to changed rows, not a full table scan. Subscriptions support `=`, `<`, `>`, `<=`, `>=`, `AND`, `OR` in WHERE clauses, but no arithmetic expressions, no aggregations, and no LIMIT.

```rust
#[spacetimedb::table(name = entity_position, public)]
pub struct EntityPosition {
    #[primary_key]
    entity_id: u64,
    #[index(btree)]
    chunk_index: u32,   // spatial partition key
    x: f32, y: f32, z: f32,
}

#[reducer]
fn move_entity(ctx: &ReducerContext, entity_id: u64, new_x: f32, new_y: f32, new_z: f32) -> Result<(), String> {
    let mut pos = ctx.db.entity_position().entity_id().find(entity_id)
        .ok_or("Entity not found")?;
    pos.x = new_x; pos.y = new_y; pos.z = new_z;
    pos.chunk_index = compute_chunk(new_x, new_y, new_z);
    ctx.db.entity_position().entity_id().update(pos);
    Ok(())
}
```

A critical finding from SpacetimeDB's own codebase: **Row Level Security (RLS) filters** can automatically scope each client's subscriptions to spatially relevant data, using a server-defined SQL filter that references the caller's identity:

```rust
#[client_visibility_filter]
const NEARBY_ENTITIES: Filter = Filter::Sql("
    SELECT * FROM entity_position WHERE chunk_index IN (
        SELECT chunk_index FROM player_visible_chunks WHERE player_identity = :sender
    )
");
```

This means the server controls what each client sees without the client needing to manually manage subscriptions — a pattern BitCraft uses for its MMO.

**BitCraft Online's scaling approach** is the most important reference. The Clockwork Labs team (who built both SpacetimeDB and BitCraft) confirmed that each SpacetimeDB instance is limited to a single machine's memory. BitCraft achieves infinite-world scale by running **multiple SpacetimeDB instances, each handling a spatial partition**. This application-layer sharding is not a hack — it is the intended architecture for worlds that exceed single-node capacity. The BitCraft server code was open-sourced in January 2026 at `github.com/clockworklabs/BitCraftPublic`.

---

## Spatial partitioning: chunk grids with spatial hash indexing

For an infinite world with variable-density AI-generated content, **fixed-size chunk grids** are the clear winner over octrees, spatial hashing alone, or hexagonal grids. The reasoning:

**Octrees require a predefined bounding volume** — fatal for infinite worlds. Expanding the root node requires reshuffling the entire tree. Octrees also produce unbalanced traversal times for batched queries and are harder to parallelize. **Hexagonal grids** offer uniform neighbor distances but extend poorly to 3D and have more complex coordinate math. **Pure spatial hashing** gives O(1) lookups but lacks natural streaming boundaries. **Chunk grids** combine O(1) position-to-chunk mapping (`chunk_x = floor(world_x / CHUNK_SIZE)`) with natural streaming units, no bounding volume requirement, and direct mapping to SpacetimeDB subscription queries.

The recommended chunk size for an entity-based (not voxel) world is **64×64 world units** in the horizontal plane. Minecraft uses 16×16 blocks because its content is voxel-granular; for AI-generated meshes, larger chunks reduce subscription churn while maintaining reasonable streaming granularity. The chunk size should be roughly **2-4× the typical player interaction radius**.

### SpacetimeDB table design for spatial data

```rust
// Core world object table — decomposed for subscription efficiency
#[spacetimedb::table(name = world_object, public)]
pub struct WorldObject {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub chunk_x: i32,
    #[index(btree)]  
    pub chunk_y: i32,
    pub local_x: f32, pub local_y: f32, pub local_z: f32,
    pub object_type: u32,
    pub asset_hash: String,      // content-addressable GLB reference
    pub bbox_half_x: f32,        // bounding box half-extents
    pub bbox_half_y: f32,
    pub bbox_half_z: f32,
    pub creator: Identity,
    pub created_at: Timestamp,
}

// Separate high-frequency position updates (for moveable objects)
#[spacetimedb::table(name = object_transform, public)]
pub struct ObjectTransform {
    #[primary_key]
    pub object_id: u64,
    #[index(btree)]
    pub chunk_x: i32,
    pub chunk_y: i32,
    pub x: f32, pub y: f32, pub z: f32,
    pub rotation_y: f32,
    pub scale: f32,
}

// Chunk metadata for lazy generation
#[spacetimedb::table(name = chunk_meta, public)]
pub struct ChunkMeta {
    #[primary_key]
    pub chunk_key: i64,  // pack (chunk_x, chunk_y) into single key
    pub biome: u8,
    pub terrain_heightmap: Vec<f32>,
    pub generation_seed: u64,
    pub object_count: u32,
    pub last_modified: Timestamp,
}

// Player visibility tracking — drives RLS filter
#[spacetimedb::table(name = player_visible_chunk, public)]
pub struct PlayerVisibleChunk {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    pub player: Identity,
    #[index(btree)]
    pub chunk_key: i64,
}
```

SpacetimeDB recommends **splitting tables by update frequency** because every row change triggers subscription delta evaluation. Position data that updates at 60Hz should live in a separate table from rarely-changing metadata. In-memory joins complete in nanoseconds, so this decomposition costs almost nothing for reads while dramatically reducing subscription bandwidth.

### Interest management via subscriptions

Clients subscribe to entities in their **current chunk plus adjacent chunks** (a 3×3 grid minimum). When a player crosses into a new chunk, the client unsubscribes from old queries and subscribes to new ones. SpacetimeDB handles the diff automatically — entities leaving the subscription trigger `onDelete` callbacks, new ones trigger `onInsert`.

The **boundary problem** (player at chunk edge needs to see across) is solved naturally by subscribing to the ring of adjacent chunks. Adding **hysteresis** — only changing subscriptions when the player is >25% into the new chunk — prevents rapid subscription churn at boundaries.

For **LOD streaming**, use multiple concurrent subscriptions at different granularities:

- **Near (1-2 chunks)**: Full object data, high update rate
- **Medium (3-5 chunks)**: Position + type + bounding box only (via a separate lightweight table)
- **Far (6+ chunks)**: Chunk-level summary only (biome, object count, heightmap)

SpatialOS pioneered this approach with component-level interest management: nearby entities get all components streamed, while distant entities get only Position and visual type. The SpatialOS worker model (server-workers with spatial authority, client-workers with cross-boundary read access, and a central Runtime computing deltas) validated that spatial sharding with overlapping interest regions works at MMO scale. Improbable's Project Morpheus claimed **10,000+ concurrent players** in a single area using this architecture.

### Scaling beyond one node: the BitCraft pattern

For thousands of concurrent creators, the architecture should follow BitCraft's approach:

1. Multiple SpacetimeDB instances, each owning a rectangular region of chunks
2. A lightweight **routing layer** that directs client connections to the correct instance based on player position
3. Players near region boundaries subscribe to **both** adjacent instances
4. Cross-instance operations (structures spanning boundaries) use eventual consistency or are rejected at the boundary

This is conceptually identical to SpatialOS's worker model but implemented at the database level rather than the game server level.

---

## Content size normalization through semantic ontology and zoning

When an LLM generates "a castle" versus "a cottage," the system needs a **semantic size ontology** mapping object categories to physically plausible dimensions. Research from Stanford (Savva et al., "On Being the Right Scale") shows that real-world object sizes follow **log-normal distributions** within categories. The platform should maintain a lookup table:

| Category | Footprint (mean) | Height (mean) | Min viable | Max allowed |
|----------|-------------------|---------------|------------|-------------|
| Tree (oak) | 4×4m | 8m | 2×2×3m | 10×10×20m |
| Cottage | 8×10m | 5m | 6×6×3m | 12×12×8m |
| Castle | 40×60m | 25m | 20×20×10m | 80×80×40m |
| Shop | 10×15m | 4m | 6×8×3m | 20×20×8m |
| Skyscraper | 30×30m | 150m | 15×15×30m | 50×50×300m |

The LLM's function-calling output includes a `semantic_category` field. The validation layer looks up the category's size distribution and constrains the generation's bounding box accordingly.

### A tiered spatial allocation system

The recommended approach blends three proven UGC management systems:

**Second Life's Land Impact model** provides the concept of a **complexity budget per area unit**. Each chunk gets a resource budget (polygon count, texture memory, object count). AI-generated content consumes budget proportional to its actual complexity. A castle consuming 50K polygons costs more "Land Impact" than a cottage at 5K polygons.

**Dreams' thermometer system** extends this with **multi-dimensional resource tracking**: geometry budget, texture budget, and interaction/script budget tracked separately. Each plot displays real-time resource usage that the LLM respects during generation.

**Real urban planning** provides zoning constraints that prevent chaotic density:

```
Zone Types:
├── Residential-Low:  max height 10m, max density 40%, min green space 30%
├── Commercial:       max height 50m, max density 70%
├── Park/Nature:      max built area 5%, min green 80%
├── Landmark:         height unlimited, requires community vote
└── Mixed-Use:        blended constraints
```

**Setback requirements** scale with building height: `setback = max(2m, height/4)`. This prevents canyon-like corridors and ensures breathing room between creations.

### Handling "make this bigger" conflicts

When expansion would overlap a neighbor's creation, the system follows a priority hierarchy:

1. **Check if adjacent plots are unclaimed** → auto-expand the claim
2. **Check if the same creator owns adjacent plots** → merge and expand
3. **If a different creator owns adjacent space** → notify the user of the constraint, suggest vertical expansion ("build upward instead"), or offer to send a collaboration request to the neighbor
4. **Hard boundary enforcement**: creations physically cannot extend beyond their plot's bounding volume — the SpacetimeDB reducer rejects the mutation

For **neighbor-aware generation**, Wave Function Collapse (WFC) principles apply. Each plot boundary has an "edge type" (wall, garden, path, water). New generations must produce compatible edges. Townscaper demonstrated this beautifully — buildings automatically adapt their forms to match adjacent structures through WFC constraint propagation. The LLM's context window receives neighboring object types, styles, and bounding boxes, and the system prompt instructs it to generate contextually appropriate content.

---

## Conflict resolution leverages SpacetimeDB's transactional model

The platform should **not** use full peer-to-peer CRDTs. SpacetimeDB is inherently server-authoritative with ACID transactions — there is no distributed state to reconcile. Instead, use **CRDT-inspired server-authoritative Last-Writer-Wins (LWW) per property**, exactly as Figma does.

Figma's document model — `Map<ObjectID, Map<Property, Value>>` with per-property LWW — is the closest analogue. Two clients changing unrelated properties on the same object never conflict. The server defines ordering via transaction sequence numbers, eliminating the need for vector clocks or Lamport timestamps. SpacetimeDB's monotonically increasing `TxOffset` provides this ordering natively.

For object existence, use **OR-Set semantics** (add wins over remove) to prevent accidental deletion of content created by others during concurrent edits. This means if one user deletes a tree while another user modifies that same tree, the modification wins and the tree survives. This is a deliberate design choice favoring content preservation.

### Event sourcing is built into SpacetimeDB

SpacetimeDB's commit log is already an append-only event store. Each reducer invocation = one transaction = one event with a monotonic `TxOffset`. Snapshots are taken every ~1,000,000 transactions for recovery. This means **every world modification is already durably recorded** at the infrastructure level.

The platform adds a semantic event layer on top for undo/history UI:

```rust
#[spacetimedb::table(name = world_event, public)]
pub struct WorldEvent {
    #[primary_key]
    #[auto_inc]
    pub id: u64,
    #[index(btree)]
    pub chunk_key: i64,
    pub editor: Identity,
    pub event_type: u16,      // place, modify, delete, resize
    pub target_id: u64,       // affected world_object ID
    pub forward_data: Vec<u8>, // serialized operation
    pub inverse_data: Vec<u8>, // serialized undo operation
    pub is_undone: bool,
    pub timestamp: Timestamp,
}
```

**Undo is per-user** (never undo someone else's work), following the command pattern with stored inverse operations. When a user undoes, the reducer pops their latest non-undone event, applies the inverse operation as a new transaction, and pushes to a redo stack. If the target state has been modified by another user since the original edit, the system applies a "best-effort" inverse or notifies the user that the undo cannot be cleanly applied.

### Storage compaction prevents unbounded growth

At **500K events/second** (estimated for 10,000 active editors at ~50 edits/sec each), raw event storage accumulates ~100 MB/sec. The compaction strategy:

- **SpacetimeDB snapshots** every 1M transactions provide the baseline recovery mechanism
- **Per-chunk archival**: event history for inactive world regions compresses and moves to cold storage (S3)
- **Semantic event compaction**: after N days, collapse sequential edits to the same object into a single delta (keep only the net change)
- **Tiered retention**: full per-event history for the last 7 days, daily snapshots for the last 90 days, weekly snapshots beyond that
- **Per-chunk content-addressed snapshots** enable Git-like deduplication: unchanged chunks share storage across versions

### Anti-griefing through layered defense

In a fully open editing system, griefing is inevitable. The defense draws from Wikipedia's anti-vandalism infrastructure and Minecraft server plugins:

**Rate limiting**: new users get 10 edits/minute, trusted users 100/minute. Token bucket algorithm with per-user buckets that refill at a constant rate. Rate limit tier stored in the `EditorReputation` table and checked inside every reducer.

**Anomaly detection**: statistical triggers fire on mass deletion (>50 objects/minute), large-area fill operations, or pattern destruction. When triggered, the system **automatically rolls back that user's recent edits** by replaying inverse operations from the `WorldEvent` table.

**Reputation gating**: new users edit only in designated "sandbox" zones. Reputation score increases as edits survive without being reverted. At threshold scores, users unlock editing in more areas. Wikipedia's WikiTrust system validates this approach — content-driven reputation based on edit survival is more robust than simple seniority.

**Spatial permissions**: plot owners can designate who can edit their regions. The `RegionPermission` table stores per-user ACLs, checked in every mutation reducer.

---

## The LLM-to-world pipeline is a seven-stage process

The text-to-3D generation landscape as of early 2026 has matured significantly. **Commercial APIs like Meshy and Tripo produce production-quality PBR meshes in GLB format in 30-60 seconds**, making them immediately deployable. Research systems like Turbo3D achieve sub-second generation but output Gaussian splats requiring conversion. The practical pipeline:

### Stage 1: LLM intent parsing

The user's natural language ("build a castle here") goes to an LLM (GPT-4o or Claude) in function-calling mode. The system prompt includes the world style guide, spatial constraints, and a JSON output schema. Before calling the LLM, the server injects **spatial context** — all objects within a 50m radius, their types, styles, bounding boxes, and the terrain profile:

```json
{
  "name": "generate_3d_asset",
  "parameters": {
    "object_description": "medieval stone castle with two towers, weathered walls",
    "semantic_category": "castle",
    "bounding_box": {"min": [80,0,190], "max": [120,25,220]},
    "style_tags": ["medieval", "stone", "weathered"],
    "max_polygons": 50000,
    "pbr_materials": true
  }
}
```

### Stage 2: Pre-validation

Before spending GPU time, validate: Is there space in the target plot? Does the user have generation credits? Does the bounding box fit the zone's height and density constraints? Is the prompt safe (text-level content moderation)?

### Stage 3: Cache lookup

Hash the normalized prompt + style tags + LOD level into a cache key. **Exact matches** serve the cached GLB instantly. **Semantic similarity** (cosine > 0.95 on prompt embeddings) offers near-matches: "A similar castle was recently generated — use this or generate new?" Pre-generated template libraries for common objects (trees, rocks, basic buildings) serve instantly with minor texture variations. Expected cache hit rates: **30-50% for common objects**, <10% for unique requests.

### Stage 4: 3D generation

A priority queue (Redis-backed) feeds GPU workers running Meshy or Tripo API calls. Per-user concurrency limit of 3 simultaneous generations. Credit costs scale with complexity: simple object = 1 credit, complex structure = 3, multi-object scene = 5-10. A Kubernetes-managed GPU worker pool autoscales based on queue depth.

### Stage 5: Post-validation

The generated GLB undergoes geometry checks (bounding box compliance, mesh manifoldness), performance checks (polygon count ≤ 50K, texture resolution ≤ 2048²), and content moderation. For 3D content moderation, Roblox's approach applies: render the mesh from multiple viewpoints and run 2D classifiers on the renders, since off-the-shelf 3D mesh classifiers don't yet exist at Roblox's scale. Style consistency scoring uses CLIP embeddings of rendered views compared against neighbors.

### Stage 6: Storage and LOD generation

The validated GLB stores in S3 with content-addressable naming (SHA-256 hash). The system generates **three LOD tiers**: high-detail (original, ~500KB), medium (~50KB, decimated mesh), and low (~5KB, bounding-box placeholder with dominant color). All three push to CDN.

### Stage 7: Client integration via Bevy

SpacetimeDB pushes a subscription update to all clients in the area: a new `WorldObject` row appears with `asset_hash` pointing to the CDN URL. The Bevy client immediately renders a colored bounding-box placeholder, then background-downloads the appropriate LOD tier based on camera distance. Bevy's native `bevy_gltf` crate loads GLB directly:

```rust
commands.spawn((
    SceneRoot(asset_server.load(
        GltfAssetLabel::Scene(0).from_asset("https://cdn.example.com/assets/{hash}.glb")
    )),
    Transform::from_xyz(100.0, 0.0, 200.0),
));
```

The key Bevy consideration: **GLB is the universal interchange format**. All major AI 3D generators support it, Bevy loads it natively, and it's compact for network transfer. Standardize the entire pipeline on `.glb`.

---

## What's achievable today versus what requires R&D

**Production-ready today**: text-to-3D single objects via Meshy/Tripo API (30-60s, GLB with PBR), LLM intent parsing via function calling, SpacetimeDB subscription-based interest management, token-bucket rate limiting, GLB loading in Bevy, content-addressable asset caching.

**Requires 3-6 months of engineering**: context-aware generation (injecting spatial context into prompts), style consistency scoring (CLIP-based), 3D content moderation pipeline (multi-view rendering + 2D classifiers), progressive LOD streaming, and multi-instance SpacetimeDB routing for cross-region subscriptions.

**Requires 6-12+ months of R&D**: real-time sub-second generation with mesh output (not Gaussian splats), coherent multi-object scene generation from single prompts, interactive editing of existing 3D assets ("make the castle taller"), and global style coherence enforcement across entire neighborhoods. Meta's WorldGen and NVIDIA's LLaMA-Mesh point toward these capabilities but are not production-ready.

---

## Conclusion: the architecture converges on four key decisions

The most consequential architectural decision is **spatial sharding across multiple SpacetimeDB instances**, following BitCraft's proven pattern. Each instance owns a rectangular world region; a routing layer directs clients to the correct instance. This is non-negotiable for MMO scale — SpacetimeDB's single-node memory constraint makes it inevitable for worlds with millions of entities.

The second decision is **chunk-based spatial indexing with SpacetimeDB subscription queries as the interest management system**. The `chunk_index` column with BTree index, combined with RLS filters, means the database itself handles what data each client sees. No separate interest management service is needed. This is the most elegant architectural property of the SpacetimeDB approach.

Third, **server-authoritative LWW conflict resolution** (not full CRDTs) is correct because SpacetimeDB's centralized transaction model already provides strong consistency. Adding CRDT overhead would be architecturally redundant. Per-property LWW with OR-Set existence semantics, exactly as Figma implements, handles all concurrent editing scenarios.

Fourth, **commercial AI 3D generation APIs** (Meshy or Tripo) should be the initial generation backend, with the full seven-stage pipeline handling context injection, validation, caching, and Bevy integration. The validation and caching layers are more architecturally important than the generation model itself — they ensure world coherence regardless of which AI backend produces the mesh. As self-hosted models like Turbo3D mature toward sub-second mesh generation, the pipeline's modular design allows swapping backends without architectural changes.