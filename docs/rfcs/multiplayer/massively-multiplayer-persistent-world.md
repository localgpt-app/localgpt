# Architecture for a massively multiplayer AI-driven persistent world platform

> **Parent spec:** [3D Collaborative World Engine: Architecture Specification](collaborative-world-engine-architecture.md) — this document is the deep-dive on MMO-tier schema, spatial ownership, and governance (§2).

**SpacetimeDB can serve as the backbone of a multiplayer AI-driven world co-creation platform, but the existing schema covers only about 30% of what a persistent, governed, scalable world requires.** The gaps are concrete: no spatial ownership model, no content quality scoring, no permission tiers, no content versioning, no decay mechanics, and no asset distribution pipeline. This document synthesizes research from BitCraft (the flagship SpacetimeDB MMO), Second Life, Minecraft claim systems, Wikipedia governance, Figma collaboration, and dozens of other production systems into a buildable architecture. SpacetimeDB 2.0's new View Functions and Row-Level Security (experimental) unlock patterns that were impossible at 1.0, particularly identity-parameterized data visibility—the foundation for a permission system. BitCraft's proven pattern of 9 spatial regions on separate SpacetimeDB instances, each handling several hundred concurrent users, provides the scaling blueprint.

---

## A. SpacetimeDB table definitions the platform still needs

The existing tables (Player, WorldEntityRow, ChatMessage, WorldInfo) handle basic entity placement and chat. The following tables fill every identified gap, organized by subsystem. All table definitions use SpacetimeDB 2.0 conventions with Rust syntax.

### Spatial ownership subsystem

```rust
#[spacetimedb::table(public)]
struct Region {
    #[primary_key]
    #[auto_inc]
    region_id: u64,
    name: String,
    owner_identity: Identity,       // creator/owner
    chunk_x_min: i32,
    chunk_y_min: i32,
    chunk_x_max: i32,
    chunk_y_max: i32,
    parent_region_id: Option<u64>,  // hierarchical: World → District → Plot
    region_type: u8,                // 0=wilderness, 1=claimed, 2=community_commons, 3=protected
    created_at: Timestamp,
    last_active_at: Timestamp,      // updated on any edit or visit
    claim_block_cost: u32,          // claim blocks consumed
    protection_level: u8,           // 0=open, 1=semi, 2=full, 3=admin_only
}

#[spacetimedb::table(public)]
#[index(btree, columns = [chunk_x, chunk_y])]
struct ChunkOwnership {
    #[primary_key]
    chunk_x: i32,
    #[primary_key]
    chunk_y: i32,
    region_id: u64,
    generation_status: u8,    // 0=ungenerated, 1=procedural, 2=modified, 3=published
    generated_seed: u64,      // deterministic seed for regeneration
    modification_count: u32,
    last_visited_at: Timestamp,
    visit_count: u64,
}
```

**Design rationale:** Chunk-aligned ownership (the Towny model) gives **O(1) lookup** by chunk coordinates via BTree index—the same pattern BitCraft uses for spatial filtering with RLS. Hierarchical regions enable World → District → Plot → Subdivision nesting, matching Second Life's Estate → Region → Parcel pattern. The `generation_status` field tracks the lazy-generation lifecycle.

### Permission and trust subsystem

```rust
#[spacetimedb::table(public)]
struct RegionPermission {
    #[primary_key]
    #[auto_inc]
    permission_id: u64,
    region_id: u64,
    grantee_identity: Option<Identity>,  // None = applies to everyone
    grantee_trust_tier: Option<u8>,      // alternative: tier-based grants
    permission_flags: u32,               // bitmask: VIEW=1, REACT=2, MODIFY=4, 
                                         // CREATE=8, GOVERN=16, MANAGE_PERMS=32
    granted_by: Identity,
    granted_at: Timestamp,
    expires_at: Option<Timestamp>,
}

#[spacetimedb::table(public)]
struct PlayerTrust {
    #[primary_key]
    player_identity: Identity,
    trust_tier: u8,              // 0=viewer, 1=reactor, 2=modifier, 3=creator, 4=governor
    total_playtime_seconds: u64,
    constructive_actions: u64,   // edits, builds, positive contributions
    quality_score_sum: f64,      // sum of ratings received on content
    quality_score_count: u32,
    reports_received: u32,
    reports_upheld: u32,         // confirmed violations
    claim_blocks_accrued: u32,   // grows with playtime (GriefPrevention pattern)
    claim_blocks_used: u32,
    last_promotion_at: Timestamp,
    tier_locked_until: Option<Timestamp>,  // demotion cooldown
}
```

**Trust tier thresholds** follow the Wikipedia/Stack Overflow progressive unlock model. Tier 0 (View) requires only account creation. Tier 1 (React) requires **1 hour of playtime**. Tier 2 (Modify) requires **10 hours + 20 constructive actions + no upheld reports**. Tier 3 (Create) requires **50 hours + positive quality score + 5 approved contributions**. Tier 4 (Govern) requires **community nomination or algorithmic threshold of 200+ hours, quality score above 90th percentile, and zero upheld reports in 90 days**. These thresholds are configurable via a scheduled reducer that re-evaluates promotions hourly.

### Content quality and versioning subsystem

```rust
#[spacetimedb::table(public)]
struct EntityVersion {
    #[primary_key]
    #[auto_inc]
    version_id: u64,
    entity_id: u64,              // references WorldEntityRow
    version_number: u32,
    author_identity: Identity,
    created_at: Timestamp,
    snapshot_data: Vec<u8>,      // BSATN-serialized entity state
    change_description: String,
    parent_version_id: Option<u64>,  // for branching/forking
}

#[spacetimedb::table(public)]
struct ContentRating {
    #[primary_key]
    rater_identity: Identity,
    #[primary_key]
    entity_id: u64,
    score: i8,                   // -1, 0, +1 (simple ternary)
    rated_at: Timestamp,
}

#[spacetimedb::table(public)]
struct ContentQuality {
    #[primary_key]
    entity_id: u64,
    net_score: i32,              // sum of ratings
    rating_count: u32,
    visit_count: u64,
    interaction_count: u64,      // clicks, inspections, time-near
    decay_score: f64,            // computed quality × recency weight
    last_interaction_at: Timestamp,
    quality_tier: u8,            // 0=unrated, 1=low, 2=medium, 3=high, 4=featured
    moderation_status: u8,       // 0=unreviewed, 1=approved, 2=flagged, 3=hidden
    is_decaying: bool,           // true when no interactions for decay_threshold
}

#[spacetimedb::table(public)]
struct ContentFlag {
    #[primary_key]
    #[auto_inc]
    flag_id: u64,
    entity_id: u64,
    reporter_identity: Identity,
    flag_type: u8,               // 0=low_quality, 1=offensive, 2=griefing, 3=copyright
    description: String,
    created_at: Timestamp,
    resolution: Option<u8>,      // 0=dismissed, 1=upheld, 2=escalated
    resolved_by: Option<Identity>,
}
```

### Content decay and persistence subsystem

```rust
#[spacetimedb::table(public)]
struct DecaySchedule {
    #[primary_key]
    #[auto_inc]
    id: u64,
    scheduled_at: ScheduleAt,    // SpacetimeDB scheduled reducer trigger
}

// The decay reducer runs on schedule and:
// 1. Queries ContentQuality WHERE last_interaction_at < NOW - 30 days
// 2. Sets is_decaying = true, reduces decay_score by 10%
// 3. At decay_score < 0.1: entity becomes translucent (visual fade)
// 4. At decay_score < 0.01 AND net_score < 0: entity archived
// 5. Protected entities (in protected regions, high quality_tier) exempt
```

### Asset distribution subsystem

```rust
#[spacetimedb::table(public)]
struct AssetReference {
    #[primary_key]
    content_hash: String,        // SHA-256 of asset bytes
    asset_type: u8,              // 0=mesh, 1=texture, 2=audio, 3=animation, 4=script
    file_size_bytes: u64,
    cdn_url: String,             // CDN endpoint for retrieval
    creator_identity: Identity,
    created_at: Timestamp,
    lod_levels: u8,              // number of LOD variants available
    license_type: u8,            // 0=all_rights_reserved, 1=cc_by, 2=cc_by_sa, 3=public_domain
    moderation_status: u8,
}

#[spacetimedb::table(public)]
struct EntityAssetBinding {
    #[primary_key]
    entity_id: u64,
    #[primary_key]
    asset_slot: String,          // "mesh", "texture_diffuse", "audio_ambient", etc.
    content_hash: String,        // references AssetReference
    lod_level: u8,               // which LOD to use at which distance
}
```

### Governance subsystem

```rust
#[spacetimedb::table(public)]
struct GovernanceProposal {
    #[primary_key]
    #[auto_inc]
    proposal_id: u64,
    region_id: u64,
    proposer_identity: Identity,
    proposal_type: u8,           // 0=protection_change, 1=ownership_transfer, 2=content_removal,
                                 // 3=rule_change, 4=governor_nomination
    description: String,
    created_at: Timestamp,
    closes_at: Timestamp,
    status: u8,                  // 0=open, 1=passed, 2=rejected, 3=enacted
    votes_for: u32,
    votes_against: u32,
    quorum_required: u32,
}

#[spacetimedb::table(public)]
struct GovernanceVote {
    #[primary_key]
    proposal_id: u64,
    #[primary_key]
    voter_identity: Identity,
    vote: i8,                    // -1=against, +1=for
    weight: u32,                 // trust-tier-weighted or quadratic
    voted_at: Timestamp,
}
```

### Edit review subsystem (the "wiki layer")

```rust
#[spacetimedb::table(public)]
struct EditReviewQueue {
    #[primary_key]
    #[auto_inc]
    review_id: u64,
    entity_id: u64,
    editor_identity: Identity,
    edit_type: u8,               // 0=create, 1=modify, 2=delete
    snapshot_before: Vec<u8>,    // entity state before edit
    snapshot_after: Vec<u8>,     // entity state after edit
    submitted_at: Timestamp,
    status: u8,                  // 0=pending, 1=approved, 2=rejected, 3=auto_approved
    reviewer_identity: Option<Identity>,
    reviewed_at: Option<Timestamp>,
    review_notes: Option<String>,
}
```

**Total new tables: 14.** Combined with the existing 4 tables, this gives **18 tables** covering spatial ownership, permissions, trust, content quality, versioning, decay, asset distribution, governance, and edit review. SpacetimeDB 2.0's View Functions should be used to create identity-parameterized views over these tables, so each client receives only data relevant to their location and permission level.

---

## B. The governance and persistence system design

The governance system must solve the central tension of persistent UGC worlds: **unlimited persistence without quality pressure creates digital ruins** (Second Life's mainland is 13-14% abandoned land), while **aggressive deletion destroys creative investment**. The design below creates a pressure gradient that naturally surfaces quality.

### Three-layer persistence model

**Layer 1: Protected content (permanent).** Content in protected regions, content with quality_tier ≥ 3 (high), or content explicitly preserved by a governor. This content never decays. It represents the "featured articles" of the world—community-validated work that defines the platform's identity. Governors can mark content as protected via GovernanceProposal with community vote. Protected status requires **net_score > 0 and rating_count > 10**.

**Layer 2: Active content (conditionally persistent).** Content with recent interactions (visited, rated, or modified within 30 days). This is the vast majority of the world at any point. Active content persists normally and faces no decay pressure. The `last_interaction_at` timestamp in ContentQuality resets on any meaningful interaction—a player spending more than 5 seconds within 20 meters counts as a visit.

**Layer 3: Decaying content (fading).** Content with no interactions for 30+ days AND net_score ≤ 0. A scheduled reducer runs every 6 hours, computing `decay_score = base_quality × 0.9^(days_since_interaction / 30)`. **At decay_score < 0.1**, the Bevy client renders the entity with increasing translucency—a visual "fading" that signals neglect and invites maintenance. **At decay_score < 0.01**, the entity moves to an archive table (invisible in-world but recoverable). **At 180 days archived with no recovery request**, the entity is permanently deleted and its chunk's modification_count decremented.

This mirrors the spectrum from Decentraland (permanent, leads to speculation/abandonment) through GriefPrevention (time-based expiration) to Towny (economic pressure). The visual fading is a novel mechanic—it turns decay into a gameplay signal. Players who value a build can interact with it to reset its decay clock, creating **emergent curation** where the community collectively decides what persists through attention rather than votes.

### Content quality scoring algorithm

Quality assessment combines four signals, weighted to resist gaming:

**Visit density** (30% weight): `visit_count / days_since_creation`, normalized against the world median. This captures genuine interest without rewarding age alone. **Rating score** (25% weight): `net_score / sqrt(rating_count)`, a Wilson-score-inspired metric that penalizes content with few ratings. A single +1 from 1 rater scores lower than +8 from 10 raters. **Interaction depth** (25% weight): `interaction_count / visit_count`—the ratio of meaningful interactions (inspect, comment, rate, modify) to passive visits. High interaction depth signals engaging content. **Creator reputation** (20% weight): the author's `quality_score_sum / quality_score_count` from PlayerTrust, providing a Bayesian prior for new content from established creators.

The combined `decay_score` determines `quality_tier`: unrated (0), low (<25th percentile), medium (25-75th), high (75-95th), featured (>95th). Featured content gets algorithmic promotion in discovery and permanent persistence. This avoids Reddit's failure mode of pure popularity voting by incorporating interaction depth and creator reputation.

### Governance model

Governance operates at the region level, not the world level. Each claimed region has an owner and optionally a set of governors (trust tier 4 users). **For community commons regions** (region_type=2), governance follows the Wikipedia model: proposals require a quorum of 30% of active region participants, pass with simple majority, and enact after a 48-hour objection window. Governor nominations follow Stack Overflow's community-elected moderator pattern—candidates need endorsement from 3 existing governors and a 2/3 majority vote.

Regions can set their own rules (build height limits, material restrictions, theme requirements) via a `region_rules` JSON field. This mirrors Second Life's covenant system where estate owners set aesthetic guidelines. Rule enforcement is semi-automated: MCP tools can flag content that appears to violate region rules, but only governors can remove it.

---

## C. The interaction permission model

The five-tier permission model (View → React → Modify → Create → Govern) intersects with three spatial contexts (owned regions, community commons, wilderness) to produce a **15-cell permission matrix**.

### Permission matrix

| Action | Wilderness | Community Commons | Owned Region (non-member) | Owned Region (member) |
|--------|-----------|-------------------|---------------------------|----------------------|
| **View (Tier 0)** | ✅ Navigate, observe | ✅ Navigate, observe | ✅ Navigate, observe | ✅ Navigate, observe |
| **React (Tier 1)** | ✅ Rate, comment, flag | ✅ Rate, comment, flag | ✅ Rate, comment, flag | ✅ Rate, comment, flag |
| **Modify (Tier 2)** | ✅ Edit unclaimed entities | ⚠️ Edit → review queue | ❌ No access | ✅ Edit per granted permissions |
| **Create (Tier 3)** | ✅ Build freely, claim land | ⚠️ Build → review queue | ❌ No access | ✅ Build per granted permissions |
| **Govern (Tier 4)** | ✅ Protect areas, rollback | ✅ Approve/reject edits | ❌ Owner controls | ✅ Full moderation tools |

The review queue for community commons is the Wikipedia-derived mechanism. Edits from Tier 2-3 users in commons regions are applied immediately but flagged in EditReviewQueue. If a Tier 4 governor rejects the edit within 24 hours, it's rolled back using the `snapshot_before` data. If no review occurs, the edit auto-approves. This balances creative flow (no blocking wait) with quality control (revert capability).

### Permission enforcement via SpacetimeDB View Functions

SpacetimeDB 2.0's View Functions provide the ideal mechanism. Each reducer that mutates world state begins with a permission check:

```rust
#[spacetimedb::reducer]
fn place_entity(ctx: &ReducerContext, chunk_x: i32, chunk_y: i32, /* ... */) {
    let player = ctx.db.player_trust().player_identity().find(ctx.sender)
        .expect("Player not found");
    let chunk = ctx.db.chunk_ownership().find(chunk_x, chunk_y);
    
    // Check trust tier >= Create (3)
    assert!(player.trust_tier >= 3, "Insufficient trust tier");
    
    // Check region permissions
    if let Some(region_id) = chunk.map(|c| c.region_id) {
        let region = ctx.db.region().region_id().find(region_id).unwrap();
        match region.region_type {
            2 => { /* community commons: create entity + add to review queue */ }
            1 => {
                // owned region: check explicit permission
                let perm = ctx.db.region_permission()
                    .filter(|p| p.region_id == region_id && 
                            p.grantee_identity == Some(ctx.sender) &&
                            p.permission_flags & 8 != 0); // CREATE flag
                assert!(perm.is_some(), "No create permission in this region");
            }
            _ => { /* wilderness: tier check sufficient */ }
        }
    }
    // ... proceed with entity creation
}
```

Row-Level Security filters ensure clients only receive entity data for chunks near their position—the exact pattern BitCraft uses: `SELECT * FROM WorldEntityRow WHERE chunk_x IN (SELECT chunk_x FROM Player WHERE identity = :sender AND abs(chunk_x - WorldEntityRow.chunk_x) < view_distance)`.

### Conflict resolution for AI-generated content spanning regions

When an AI generation via MCP tools produces content spanning multiple chunks, the reducer must check permissions for **every affected chunk**. If the generation would place entities in a chunk where the user lacks permission, the reducer truncates the generation at the boundary and returns a partial result with a message indicating which chunks were excluded. This mirrors Second Life's auto-return behavior for objects crossing parcel boundaries. The MCP tool should pre-check boundaries and warn the user before generation.

---

## D. The progressive construction architecture

### Lazy chunk generation with persistent deltas

The world uses a **hybrid Minecraft/No Man's Sky model**: procedural generation from a deterministic seed on first visit, with full state persistence after any modification. This avoids No Man's Sky's 15,000-edit eviction problem while keeping storage efficient for unexplored territory.

**Chunk lifecycle:**
1. **Ungenerated**: No data exists. ChunkOwnership row has `generation_status = 0`. Storage cost: ~40 bytes (the row itself).
2. **Procedural**: First player visits → scheduled reducer generates terrain from `world_seed XOR hash(chunk_x, chunk_y)`. WorldEntityRow entries created for terrain features, flora, structures. `generation_status = 1`. The seed is stored so regeneration is possible if needed.
3. **Modified**: Player or AI places/edits an entity. `generation_status = 2`, `modification_count` incremented. An EntityVersion snapshot captures the change. From this point, the chunk is fully persisted—no regeneration.
4. **Published**: Owner or governor marks content as reviewed/stable. `generation_status = 3`. Content enters the quality scoring pipeline.

**The critical design decision**: unlike No Man's Sky's delta overlay, modified chunks store **complete state** (the Minecraft model). This eliminates the eviction/terrain-regrowth problem entirely. The cost is higher storage for modified chunks, but SpacetimeDB's in-memory model handles this well—a chunk with 100 entities at ~500 bytes each is only 50KB, and the Maincloud supports up to 256GB RAM.

### Procedural generation via MCP tools

The 57+ existing MCP tools already handle single-player generation. For the persistent multiplayer world, the generation pipeline adds a **validation layer**:

1. MCP tool receives generation request (natural language prompt + target chunk coordinates)
2. MCP tool generates entity definitions (geometry descriptors, not raw mesh bytes)
3. **Validation reducer** checks: permission (does user have Create tier for these chunks?), budget (does user have enough claim blocks?), content policy (does the LLM-generated content pass safety filters?)
4. **Placement reducer** creates WorldEntityRow entries transactionally (ACID guarantees no partial generations)
5. **Asset pipeline**: 3D mesh generation happens asynchronously. The entity is placed with a placeholder bounding box immediately; the full mesh uploads to CDN and AssetReference updates with the content_hash. Other clients fetch the mesh from CDN by content_hash.

### Schema evolution without world wipes

SpacetimeDB 2.0's automatic migration supports adding new tables and adding columns with defaults—the two most common evolution needs. For the WorldEntityRow specifically, the architecture uses a **component-bag pattern**:

```rust
#[spacetimedb::table(public)]
struct WorldEntityRow {
    #[primary_key]
    entity_id: u64,
    chunk_x: i32,
    chunk_y: i32,
    entity_type: u16,
    position_x: f32,
    position_y: f32,
    position_z: f32,
    rotation_x: f32,
    rotation_y: f32,
    rotation_z: f32,
    rotation_w: f32,
    scale: f32,
    creator_identity: Identity,
    created_at: Timestamp,
    // Extension: new component columns added at end with defaults
    // This is always a safe SpacetimeDB migration
    #[spacetimedb(default = 0)]
    health: i32,
    #[spacetimedb(default = "")]
    behavior_script_hash: String,
}
```

For more radical schema changes (e.g., splitting WorldEntityRow into a proper ECS with separate component tables), the **incremental migration pattern** applies: create new tables alongside old ones, add a reducer that lazily migrates entities on access, run dual-write during the transition. BitCraft uses this same pattern. The `schema_version` field on each entity enables load-time migration logic.

**New entity types** (NPCs, vehicles, interactive objects) are added as new `entity_type` values. The Bevy client uses the entity_type to select the correct rendering/behavior system. Unknown entity_types render as placeholder cubes with a "content update available" indicator, gracefully degrading for clients that haven't updated yet.

### Module upgrade strategy

Following BitCraft's production pattern: `spacetime publish` hot-swaps the WASM module while clients remain connected. The deployment process:

1. New module version tested on staging instance with snapshot of production data
2. `spacetime publish world-production` deploys to production
3. Automatic migration runs (adding new tables/columns)
4. `__update__` lifecycle reducer runs any data migration logic
5. Scheduled reducers (game loop, decay cycle) restart
6. Clients continue with existing subscriptions; new features activate as clients update

Backward-compatible client updates ship via the Bevy client's auto-update mechanism. For breaking changes, a `min_client_version` field in WorldInfo gates connection—old clients see a "please update" message.

---

## E. Phased implementation roadmap

### Phase 0: Foundation hardening (weeks 1-4)

**Goal**: Extend the existing single-player prototype to a solid multiplayer foundation.

Add the ChunkOwnership and Region tables with basic wilderness-only generation. Implement the lazy chunk generation pipeline: seed-based procedural generation on first visit via MCP tools, full persistence of modified chunks. Deploy a single SpacetimeDB instance with RLS filtering so each client only receives entities in nearby chunks. Add the AssetReference table and a basic CDN pipeline (S3 + CloudFront) for AI-generated meshes. **The existing 57+ MCP tools continue to work**, but now generate into persistent shared state instead of ephemeral single-player state.

**Key deliverable**: Two players can connect, explore procedurally generated terrain, and see each other's AI-generated creations persist across sessions.

### Phase 1: Co-creation with ownership (weeks 5-12)

**Goal**: Real-time collaborative building with spatial claims and conflict resolution.

Add PlayerTrust, RegionPermission, EntityVersion, and EditReviewQueue tables. Implement the trust tier system with automatic promotion. Implement claim blocks (accruing with playtime, consumed by claiming chunks). Build the permission check layer into all mutation reducers. Add edit history with undo/redo using EntityVersion snapshots—this extends the existing `edit_history` system to multiplayer with per-user undo stacks.

**Conflict resolution** follows Figma's model: **last-writer-wins at the entity-property level**. Two users can edit different properties of the same entity simultaneously (one moves it, another recolors it). If both change the same property, the server timestamps resolve the conflict. The losing edit is preserved in EntityVersion for recovery. Cursor/presence awareness uses SpacetimeDB's Event Tables—ephemeral rows broadcast each player's selection state without persisting it.

**Key deliverable**: A small group (5-20 people) can collaboratively build in a shared world with claims, permissions, and real-time presence awareness.

### Phase 2: Quality persistence and governance (weeks 13-20)

**Goal**: Content quality scoring, decay mechanics, and community governance.

Add ContentRating, ContentQuality, ContentFlag, GovernanceProposal, and GovernanceVote tables. Implement the three-layer persistence model (protected → active → decaying). Deploy the scheduled decay reducer running every 6 hours. Build the Bevy client's visual fade rendering for decaying content. Implement the voting UI (rate content, flag content, vote on proposals). Add the governance workflow for community commons regions.

**Content moderation**: Integrate an LLM-based content policy checker into the MCP generation pipeline. The LLM reviews generated content descriptions against community guidelines before placement. For existing content, implement a report → review → action workflow using ContentFlag and the EditReviewQueue. Governors (Tier 4) get moderation tools: rollback a user's recent edits, hide flagged content, protect high-quality areas.

**Key deliverable**: A world where quality content naturally persists and low-quality content gracefully fades, with community-elected governors maintaining shared spaces.

### Phase 3: Scale architecture (weeks 21-32)

**Goal**: Support hundreds of concurrent users across a large world.

Implement BitCraft's multi-region sharding pattern: split the world into spatial regions, each running on a separate SpacetimeDB instance. Build the orchestration layer that routes player connections to the correct region instance based on their position. Implement cross-region player handoff (when a player crosses a region boundary, their PlayerTrust and inventory data transfers to the new instance). Each region supports several hundred concurrent users, matching BitCraft's proven capacity.

**Interest management**: Implement grid-based AOI with tiered update rates. Entities within **5 chunks** of the player sync at full rate (subscription-based, via RLS filtering). Entities **5-15 chunks** away sync at reduced rate (every 5th transaction). Beyond 15 chunks, no sync—entities load from persistent state when the player approaches. This follows the production pattern from EVE Online and Saga of Ryzom, achieving **6x bandwidth reduction** versus broadcast-all.

**Asset distribution optimization**: Implement progressive mesh loading. When a player first encounters an AI-generated entity, the client receives a low-poly proxy immediately (bounding box + dominant color from entity metadata stored in SpacetimeDB). The full mesh loads asynchronously from CDN by content_hash. The client caches assets locally on disk; content-addressed hashing provides automatic deduplication (the Roblox model that serves 77.7M daily active users).

**Key deliverable**: A world spanning multiple SpacetimeDB instances supporting 500+ concurrent users with sub-second entity sync and efficient asset streaming.

### Phase 4: Wiki-world and emergent ecosystem (weeks 33-44)

**Goal**: The layered wiki model where different regions operate in different modes.

Enable all three interaction phases to coexist spatially. **Private workshops** (Phase 1 co-creation): players create personal or group-owned regions with invited collaborators. These are isolated creative spaces. **Published galleries** (Phase 2 persistent gallery): completed regions can be "published," making them read-only for visitors. Published regions enter the quality scoring pipeline and appear in discovery. **Community commons** (Phase 3 layered wiki): regions designated as commons allow wiki-style editing with the review queue.

The transitions between phases are explicit reducer calls: `publish_region(region_id)` makes it read-only and enters it into discovery. `open_for_contribution(region_id)` converts a published region into a community commons. `fork_region(region_id)` creates a private workshop copy of any published or commons region—the Dreams remixing model applied to spatial content.

Implement spatial version control inspired by Project Beckett (Ink & Switch): visual 3D diffs showing what changed between versions, with per-entity attribution. This is the Wikipedia "View History" page rendered spatially—governors can scrub through time and selectively revert individual entities or entire time ranges. CoreProtect's Minecraft model (per-user, per-area, per-timeframe rollback) is the direct inspiration for the reducer API: `rollback(region_id, author: Option<Identity>, since: Timestamp, radius: Option<u32>)`.

**Key deliverable**: A world with diverse zones—some collaborative workshops, some curated galleries, some wiki-like evolving commons—all in a single persistent universe.

### Phase 5: Autonomous world (weeks 45+)

**Goal**: AI-driven world evolution, NPC ecosystems, and self-sustaining content generation.

Deploy scheduled MCP procedures that autonomously generate content in unexplored regions based on world lore, biome rules, and player activity patterns. NPCs with LLM-driven behavior (using SpacetimeDB 2.0 Procedures for HTTP calls to AI inference endpoints) inhabit the world and respond to player actions. The Avian physics system enables physically simulated interactions.

Content decay becomes **content evolution**: decaying entities don't just fade—the AI can transform them. An abandoned building develops cracks and vegetation. An untended garden grows wild. This creates a living world where neglect produces interesting emergent aesthetics rather than empty voids.

**Estimated architecture at full scale**: 10-20 SpacetimeDB regional instances, each handling 200-500 concurrent users, with a CDN-backed asset layer serving thousands of unique AI-generated meshes, governed by community-elected moderators operating through a spatial wiki model. Total concurrent capacity: **2,000-10,000 users** in a single persistent world, limited primarily by the orchestration layer's cross-region handoff speed and CDN throughput for novel assets.

---

## Why this architecture works and where the real risks hide

The design leverages SpacetimeDB's unique strengths—ACID reducers eliminate duplication bugs and race conditions that plague traditional MMO architectures, and the in-memory model delivers sub-microsecond lookups that make permission checks essentially free. **BitCraft has already proven the core pattern** of spatial regions on separate instances with several hundred concurrent users each.

The three highest-risk areas are **content-addressed asset distribution** (no game has shipped AI-generated 3D meshes to thousands of concurrent users at low latency—this is genuinely novel territory), **the review queue's social dynamics** (Wikipedia's model works for text but has never been tested for spatial 3D content, where "what constitutes a good edit" is far more subjective), and **cross-region entity handoff** (BitCraft handles player transfer but not AI-generated content that spans boundaries). Each of these risks has a fallback: assets can use standard CDN patterns proven by Roblox, the review queue can start fully permissive and tighten as community norms develop, and cross-region entities can be prohibited initially (content clips at region boundaries, like Second Life's auto-return).

The architecture deliberately avoids the SpatialOS trap of building distributed spatial simulation from scratch—Improbable spent $700M+ and never shipped a commercial game on that model. Instead, it uses SpacetimeDB's single-instance performance (up to **170,000 TPS**) as the unit of scale and composes multiple instances externally, which is exactly the approach that BitCraft has validated in production.