# SpacetimeDB Integration Design for LocalGPT Gen

## Executive Summary

Integrate [SpacetimeDB](https://spacetimedb.com/) into `localgpt-gen` to enable **real-time collaborative 3D world building** — multiple users (each with their own AI agent) can co-create, modify, and explore the same 3D scene simultaneously. SpacetimeDB replaces the local-only `world.ron` save format with a persistent, synchronized, server-authoritative world database while preserving the existing single-player experience as the default.

---

## 1. Why SpacetimeDB

### Current Architecture (Single-Player)

```
User → stdin → Agent → GenCommand (mpsc) → Bevy → Render
                                              ↓
                                         world.ron (local file)
```

- One user, one agent, one Bevy instance
- World state lives in Bevy ECS + RON file snapshots
- No networking, no shared state

### What SpacetimeDB Provides

| Capability | How It Helps Gen |
|-----------|-----------------|
| **Database + server in one** | No separate backend to deploy; world state is the database |
| **WASM module execution** | Game logic (validation, permissions, physics rules) runs server-side in Rust, compiled to WASM |
| **Real-time subscriptions** | Clients subscribe to entity tables; deltas pushed automatically on change |
| **ACID transactions** | Entity spawns/modifications are atomic — no half-applied batches |
| **Reducers** | Named server-side functions that mutate state transactionally |
| **Identity system** | Built-in per-connection identity for ownership/authorship |
| **Commit log + snapshots** | Automatic persistence with crash recovery |
| **Bevy plugin exists** | [`bevy_spacetimedb`](https://crates.io/crates/bevy_spacetimedb) provides `StdbConnection` resource, table event messages |

### What It Replaces vs. What It Complements

| Component | Current | With SpacetimeDB |
|-----------|---------|-------------------|
| World persistence | `world.ron` file | SpacetimeDB tables (+ export to RON) |
| Edit history | In-memory `EditHistory` | SpacetimeDB commit log + `edit_history` table |
| Entity registry | `NameRegistry` (local HashMap) | `entities` table with unique index on name |
| Multiplayer | Not supported | Real-time via subscriptions |
| Auth/ownership | N/A | SpacetimeDB `Identity` per connection |
| Local single-player | Direct Bevy ECS | **Unchanged** — SpacetimeDB is opt-in |

---

## 2. Architecture Overview

### Deployment Topology

```
┌──────────────────────────────────────────────────────────┐
│                    SpacetimeDB Instance                    │
│  (self-hosted or cloud)                                   │
│                                                           │
│  ┌─────────────────────────────────────────────────────┐  │
│  │              WASM Module (Rust)                      │  │
│  │  Tables: worlds, entities, behaviors, audio,        │  │
│  │          environment, cameras, avatars,              │  │
│  │          edit_history, players                       │  │
│  │  Reducers: spawn_entity, modify_entity,             │  │
│  │           delete_entity, set_environment, ...       │  │
│  └─────────────────────────────────────────────────────┘  │
└─────────────┬──────────────────────┬──────────────────────┘
              │ WebSocket            │ WebSocket
              ▼                      ▼
┌─────────────────────┐  ┌─────────────────────┐
│   Client A          │  │   Client B          │
│  ┌───────────┐      │  │  ┌───────────┐      │
│  │ Bevy +    │      │  │  │ Bevy +    │      │
│  │ StdbPlugin│      │  │  │ StdbPlugin│      │
│  └───────────┘      │  │  └───────────┘      │
│  ┌───────────┐      │  │  ┌───────────┐      │
│  │ AI Agent  │      │  │  │ AI Agent  │      │
│  └───────────┘      │  │  └───────────┘      │
└─────────────────────┘  └─────────────────────┘
```

### Mode Selection

```
# Single-player (default, no change)
cargo run -p localgpt-gen -- "build a castle"

# Multiplayer: host a new world
cargo run -p localgpt-gen -- --multiplayer --host "my_world"

# Multiplayer: join an existing world
cargo run -p localgpt-gen -- --multiplayer --join "ws://host:3000" --world "my_world"
```

---

## 3. New Crate: `localgpt-stdb-module`

A standalone Rust crate compiled to WASM and published to SpacetimeDB. This is the **server-side module** — all authoritative world state lives here.

### 3.1 Table Definitions

```rust
use spacetimedb::{table, Identity, Timestamp};

// ── World ────────────────────────────────────────────────

#[table(name = worlds, public)]
pub struct World {
    #[primary_key]
    pub world_id: u64,
    pub name: String,
    pub description: Option<String>,
    pub biome: Option<String>,
    pub time_of_day: Option<f32>,
    pub schema_version: u32,
    pub next_entity_id: u64,
    pub owner: Identity,
    pub created_at: Timestamp,
}

// ── Entity ───────────────────────────────────────────────

#[table(name = entities, public)]
pub struct Entity {
    #[primary_key]
    #[auto_inc]
    pub row_id: u64,

    pub world_id: u64,
    pub entity_id: u64,        // Stable EntityId from world-types
    pub name: String,           // Unique within world

    // Transform
    pub pos_x: f32,
    pub pos_y: f32,
    pub pos_z: f32,
    pub rot_x: f32,            // Euler degrees
    pub rot_y: f32,
    pub rot_z: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub scale_z: f32,
    pub visible: bool,

    // Parent hierarchy
    pub parent_entity_id: Option<u64>,
    pub chunk_x: Option<i32>,
    pub chunk_z: Option<i32>,
    pub creation_id: Option<u64>,

    // Components stored as JSON blobs (flexible, avoids
    // excessive table joins for composable components)
    pub shape_json: Option<String>,      // Shape serialized
    pub material_json: Option<String>,   // MaterialDef serialized
    pub light_json: Option<String>,      // LightDef serialized
    pub audio_json: Option<String>,      // AudioDef serialized
    pub mesh_asset_json: Option<String>, // MeshAssetRef serialized

    // Metadata
    pub author: Identity,
    pub updated_at: Timestamp,
}

// ── Behaviors (separate table: many-to-one with entity) ──

#[table(name = behaviors, public)]
pub struct Behavior {
    #[primary_key]
    #[auto_inc]
    pub row_id: u64,
    pub world_id: u64,
    pub entity_id: u64,
    pub behavior_id: String,   // "b1", "b2", etc.
    pub def_json: String,      // BehaviorDef serialized
}

// ── Environment (one per world) ──────────────────────────

#[table(name = environments, public)]
pub struct Environment {
    #[primary_key]
    pub world_id: u64,
    pub background_color: Option<String>,  // "[r,g,b,a]"
    pub ambient_intensity: Option<f32>,
    pub ambient_color: Option<String>,
    pub fog_density: Option<f32>,
    pub fog_color: Option<String>,
}

// ── Camera (per-world default) ───────────────────────────

#[table(name = cameras, public)]
pub struct Camera {
    #[primary_key]
    pub world_id: u64,
    pub pos_x: f32,
    pub pos_y: f32,
    pub pos_z: f32,
    pub look_x: f32,
    pub look_y: f32,
    pub look_z: f32,
    pub fov_degrees: f32,
}

// ── Players (connected users / agents) ───────────────────

#[table(name = players, public)]
pub struct Player {
    #[primary_key]
    pub identity: Identity,
    pub world_id: u64,
    pub display_name: String,
    pub avatar_pos_x: f32,
    pub avatar_pos_y: f32,
    pub avatar_pos_z: f32,
    pub avatar_rot_y: f32,     // Yaw only for network efficiency
    pub is_agent: bool,        // true = AI agent, false = human
    pub connected_at: Timestamp,
}

// ── Edit History (append-only log) ───────────────────────

#[table(name = edit_history, public)]
pub struct EditRecord {
    #[primary_key]
    #[auto_inc]
    pub seq: u64,
    pub world_id: u64,
    pub op_json: String,       // EditOp serialized
    pub inverse_json: String,  // Inverse EditOp serialized
    pub author: Identity,
    pub timestamp: Timestamp,
}

// ── Ambience (per-world, multiple layers) ────────────────

#[table(name = ambience_layers, public)]
pub struct AmbienceLayer {
    #[primary_key]
    #[auto_inc]
    pub row_id: u64,
    pub world_id: u64,
    pub layer_name: String,
    pub source_json: String,   // AudioSource serialized
    pub volume: f32,
}
```

### 3.2 Key Reducers

```rust
use spacetimedb::{reducer, ReducerContext, Identity};

#[reducer]
pub fn create_world(ctx: &ReducerContext, name: String, description: Option<String>) {
    // Allocate world_id, insert into worlds table
    // Set owner = ctx.sender
    // Insert default environment, camera
}

#[reducer]
pub fn spawn_entity(ctx: &ReducerContext, world_id: u64, entity_json: String) {
    // Deserialize WorldEntity from JSON
    // Validate: name uniqueness, entity count limits, triangle budget
    // Allocate entity_id from world.next_entity_id
    // Insert into entities table
    // Insert any behaviors into behaviors table
    // Record in edit_history
}

#[reducer]
pub fn modify_entity(ctx: &ReducerContext, world_id: u64, entity_id: u64, patch_json: String) {
    // Deserialize EntityPatch
    // Find existing entity row
    // Apply patch, validate constraints
    // Update entity row
    // Update/insert/delete behaviors if patch.behaviors is Some
    // Record in edit_history (with inverse computed from previous state)
}

#[reducer]
pub fn delete_entity(ctx: &ReducerContext, world_id: u64, entity_id: u64) {
    // Snapshot entity for inverse (undo)
    // Delete from entities table
    // Delete associated behaviors
    // Record in edit_history
}

#[reducer]
pub fn batch_edit(ctx: &ReducerContext, world_id: u64, ops_json: String) {
    // Deserialize Vec<EditOp>
    // Execute each op in sequence within single transaction
    // All-or-nothing: if any op fails, entire batch rolls back
}

#[reducer]
pub fn set_environment(ctx: &ReducerContext, world_id: u64, env_json: String) {
    // Upsert environment row
    // Record in edit_history
}

#[reducer]
pub fn set_camera(ctx: &ReducerContext, world_id: u64, camera_json: String) {
    // Upsert camera row
}

#[reducer]
pub fn update_player_position(ctx: &ReducerContext, world_id: u64, x: f32, y: f32, z: f32, yaw: f32) {
    // Update player's avatar position
    // High-frequency; no edit_history recording
}

#[reducer]
pub fn set_ambience(ctx: &ReducerContext, world_id: u64, layers_json: String) {
    // Delete existing layers for world
    // Insert new layers
    // Record in edit_history
}

// Lifecycle reducers
#[reducer(client_connected)]
pub fn on_connect(ctx: &ReducerContext) {
    // Insert player row with default position
}

#[reducer(client_disconnected)]
pub fn on_disconnect(ctx: &ReducerContext) {
    // Remove player row
}
```

### 3.3 Design Decisions

**JSON blobs for components vs. normalized tables:**
Components like `Shape`, `MaterialDef`, `LightDef` are stored as JSON strings rather than fully normalized columns because:
1. `localgpt-world-types` already defines rich enum types (20+ shape variants, nested material properties) — normalizing them creates massive table sprawl
2. Components are always read/written together with their entity — no need for per-field queries
3. The JSON is the exact same serde format already used in `world.ron` — zero conversion cost
4. Behaviors get their own table because they're many-to-one with entities and need individual add/remove

**Entity ID allocation:**
The `world.next_entity_id` counter is atomically incremented in the `spawn_entity` reducer. Since reducers are transactional, this is race-free even with concurrent spawns.

---

## 4. Client-Side Integration

### 4.1 New Crate: `localgpt-stdb-client`

A thin adapter layer that bridges between `localgpt-world-types` and the SpacetimeDB Rust Client SDK.

```
crates/
├── stdb-module/    # Server-side WASM module (spacetimedb crate)
├── stdb-client/    # Client-side adapter (spacetimedb-sdk crate)
└── world-types/    # Unchanged — shared types, serde-only
```

**Dependency graph addition:**

```
localgpt-world-types  (serde-only, no new deps)
        │
        ├──────────────────────────┐
        ▼                          ▼
localgpt-stdb-module          localgpt-stdb-client
(spacetimedb, WASM target)    (spacetimedb-sdk, bevy_spacetimedb)
                                       │
                                       ▼
                               localgpt-gen (Bevy)
                               (feature = "multiplayer")
```

### 4.2 Adapter Functions

```rust
// crates/stdb-client/src/lib.rs

/// Convert a SpacetimeDB Entity row → WorldEntity (world-types)
pub fn row_to_world_entity(row: &EntityRow) -> WorldEntity { ... }

/// Convert a WorldEntity → fields for spawn_entity reducer
pub fn world_entity_to_spawn_args(entity: &WorldEntity) -> String { ... }

/// Convert an EntityPatch → JSON for modify_entity reducer
pub fn entity_patch_to_args(patch: &EntityPatch) -> String { ... }

/// Convert a SpacetimeDB Environment row → EnvironmentDef
pub fn row_to_environment(row: &EnvironmentRow) -> EnvironmentDef { ... }
```

### 4.3 Bevy Plugin Integration

The existing `GenPlugin` gains an optional `StdbSyncPlugin` that runs alongside it:

```rust
// In crates/gen/src/multiplayer/mod.rs (new module, behind feature flag)

pub struct StdbSyncPlugin {
    pub uri: String,
    pub module_name: String,
    pub world_name: String,
}

impl Plugin for StdbSyncPlugin {
    fn build(&self, app: &mut App) {
        app
            // bevy_spacetimedb connection
            .add_plugins(
                StdbPlugin::default()
                    .with_uri(&self.uri)
                    .with_module_name(&self.module_name)
                    .with_run_fn(DbConnection::run_threaded)
                    .add_table(RemoteTables::entities)
                    .add_table(RemoteTables::behaviors)
                    .add_table(RemoteTables::environments)
                    .add_table(RemoteTables::players)
                    .add_table(RemoteTables::ambience_layers)
                    .add_reducer::<SpawnEntity>()
                    .add_reducer::<ModifyEntity>()
                    .add_reducer::<DeleteEntity>()
                    .add_reducer::<SetEnvironment>()
                    .add_reducer::<BatchEdit>()
                    .add_reducer::<UpdatePlayerPosition>()
            )
            // Sync systems
            .add_systems(Update, (
                sync_entity_inserts,    // DB insert → Bevy spawn
                sync_entity_updates,    // DB update → Bevy modify
                sync_entity_deletes,    // DB delete → Bevy despawn
                sync_environment,       // DB env change → Bevy env
                sync_player_avatars,    // DB player positions → avatar meshes
                broadcast_local_avatar, // Local avatar → DB position
            ))
            // Intercept local GenCommands and route through SpacetimeDB
            .add_systems(
                Update,
                intercept_gen_commands
                    .before(process_gen_commands)
            );
    }
}
```

### 4.4 Command Flow (Multiplayer Mode)

In multiplayer mode, the gen tool commands are **intercepted** before reaching the local Bevy handler. Instead of directly mutating the ECS, they call SpacetimeDB reducers. The mutations then arrive back via subscription updates:

```
Agent tool call: gen_spawn_primitive
       │
       ▼
GenCommand::SpawnPrimitive ──┐
       │                     │
       │ (single-player)     │ (multiplayer)
       │                     │
       ▼                     ▼
process_gen_commands    intercept_gen_commands
  │                        │
  ▼                        ▼
Bevy ECS direct       stdb.reducers().spawn_entity(json)
  │                        │
  ▼                        ▼
Immediate render      SpacetimeDB server
                           │
                           ▼ (subscription update)
                      sync_entity_inserts
                           │
                           ▼
                      Bevy ECS spawn
                           │
                           ▼
                      Render (all clients)
```

**Latency consideration:** In multiplayer mode, there's one network round-trip before the entity appears. For local single-player, behavior is unchanged (direct ECS mutation, zero latency).

### 4.5 Remote Player Rendering

When other players/agents are connected:

```rust
fn sync_player_avatars(
    mut messages: ReadInsertUpdateMessage<Player>,
    stdb: Res<StdbConnection<DbConnection>>,
    mut commands: Commands,
    mut existing: Query<(Entity, &RemotePlayer, &mut Transform)>,
) {
    // For each player update from SpacetimeDB:
    // - If new player: spawn a colored capsule with nametag
    // - If existing: update transform (interpolated)
    // - Mark AI agents with a different color/icon
}
```

---

## 5. Feature Flag & Workspace Changes

### 5.1 `localgpt-gen` Cargo.toml

```toml
[features]
default = []
multiplayer = ["dep:spacetimedb-sdk", "dep:bevy_spacetimedb", "dep:localgpt-stdb-client"]

[dependencies]
localgpt-stdb-client = { path = "../stdb-client", optional = true }
spacetimedb-sdk = { version = "2.0", optional = true }
bevy_spacetimedb = { version = "1.0", optional = true }
```

### 5.2 New Workspace Members

```toml
# Root Cargo.toml
[workspace]
members = [
    # ... existing ...
    "crates/stdb-module",
    "crates/stdb-client",
]
```

### 5.3 `localgpt-world-types` — No Changes

The world-types crate remains **serde-only** with zero SpacetimeDB dependencies. The adapter layer in `stdb-client` handles conversion. This preserves the critical constraint that `world-types` compiles for WASM, iOS, and Android.

---

## 6. Data Flow Diagrams

### 6.1 World Creation (Host)

```
User: "cargo run -p localgpt-gen -- --multiplayer --host my_castle"

1. Start local SpacetimeDB instance (or connect to remote)
2. Publish WASM module if not already deployed
3. Call create_world reducer → World row created
4. Subscribe to all tables WHERE world_id = X
5. Launch Bevy with StdbSyncPlugin
6. Agent enters interactive loop (same as single-player)
7. Tool calls → reducers → subscription updates → Bevy render
```

### 6.2 World Join (Client)

```
User: "cargo run -p localgpt-gen -- --multiplayer --join ws://host:3000 --world my_castle"

1. Connect to SpacetimeDB at given URI
2. Subscribe to all tables WHERE world_id = X
3. Initial sync: all existing entities materialized in Bevy
4. Launch Bevy with StdbSyncPlugin
5. See existing world + other players' avatars
6. Agent can modify (tool calls → reducers → sync to all)
```

### 6.3 Edit Conflict Resolution

SpacetimeDB reducers are serialized — no two reducers run concurrently on the same database. This means:

- **No conflicts possible** at the database level
- If two users modify the same entity "simultaneously," one reducer runs first, the other runs second (both succeed, last write wins)
- The `edit_history` table preserves the full sequence for undo

For higher-level semantic conflicts (e.g., two users building in the same area), the system relies on:
1. **Spatial awareness:** `gen_scene_info` shows entity positions, agents can coordinate
2. **Ownership hints:** `author` field on entities lets agents know who created what
3. **Future enhancement:** optional per-entity locks via a `locks` table

---

## 7. Migration Path

### Phase 1: Module + Client Crates (Foundation)

- [ ] Create `crates/stdb-module/` with all table and reducer definitions
- [ ] Create `crates/stdb-client/` with `world-types ↔ SpacetimeDB` adapters
- [ ] Unit tests: round-trip `WorldEntity → Entity row → WorldEntity`
- [ ] Build and publish module to local SpacetimeDB for testing

### Phase 2: Bevy Sync Plugin

- [ ] Implement `StdbSyncPlugin` with entity insert/update/delete sync
- [ ] Implement `intercept_gen_commands` system for multiplayer routing
- [ ] Test: single client creating entities via reducers, seeing them rendered
- [ ] Implement environment + ambience sync

### Phase 3: Multi-Client

- [ ] Implement player avatar sync (position, nametag)
- [ ] Test: two Bevy clients connected to same world
- [ ] Add `--multiplayer`, `--host`, `--join` CLI flags
- [ ] Handle connection/disconnection gracefully (reconnect logic)

### Phase 4: Agent Collaboration

- [ ] Multiple AI agents in same world (each with own identity)
- [ ] Agent sees other agents' changes in real-time via `gen_scene_info`
- [ ] Authorship tracking in edit history
- [ ] Export multiplayer world to `world.ron` (snapshot current state)

### Phase 5: Polish

- [ ] Spatial voice/text chat between connected users
- [ ] Per-entity ownership/lock system
- [ ] World browser (list available worlds on a SpacetimeDB instance)
- [ ] Web viewer client (WASM Bevy in browser connecting to same DB)

---

## 8. Key Design Constraints

1. **`localgpt-world-types` stays serde-only.** No SpacetimeDB dependency. Adapter layer handles conversion.

2. **`localgpt-core` stays platform-agnostic.** No SpacetimeDB dependency in core. The multiplayer feature is entirely within `gen` and the two new crates.

3. **Single-player is the default.** Multiplayer is opt-in via feature flag + CLI flags. Zero behavioral change without `--multiplayer`.

4. **Reducers mirror the existing tool set.** Every `GenCommand` variant maps to a reducer. The agent's tool interface is unchanged — it still calls `gen_spawn_primitive`, `gen_modify_entity`, etc. The routing layer decides whether to go local or networked.

5. **Edit history is preserved.** The `edit_history` table in SpacetimeDB mirrors the local `EditHistory` struct. Undo/redo works in multiplayer (per-user undo stack, computed client-side from `edit_history` rows filtered by author).

6. **SpacetimeDB is self-hostable.** Users can run `spacetime start` locally for LAN multiplayer, or use SpacetimeDB cloud for internet-scale.

---

## 9. Open Questions

1. **Avatar physics in multiplayer:** Should collision detection run server-side (in reducers) or client-side with position reconciliation?

2. **Large worlds:** For worlds with 10,000+ entities, should we use SpacetimeDB's subscription queries to only sync nearby chunks (`SELECT * FROM entities WHERE chunk_x BETWEEN ? AND ?`)?

3. **Asset sharing:** When one client loads a `.glb` model, how do other clients get the mesh data? Options: shared asset server, SpacetimeDB blob table, or out-of-band file sync.

4. **Tick rate:** Player avatar position updates need ~20Hz for smooth movement. Should this bypass the reducer system and use a lighter-weight channel, or is SpacetimeDB fast enough?

5. **Web client:** Should the browser WASM client use the SpacetimeDB TypeScript SDK or the Rust SDK compiled to WASM?
