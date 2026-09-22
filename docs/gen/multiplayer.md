# Gen Multiplayer — Listen Server + §2 Scaling Mechanisms

Implementation of the
[Collaborative World Engine spec](../rfcs/multiplayer/collaborative-world-engine-architecture.md):

- **§1 (Prototype & Session-Based Architecture):** one desktop
  `localgpt-gen` instance acts as the **authoritative host and rendering
  client**; secondary clients discover the session over mDNS and join as
  viewers that can send natural-language prompts to the host's agent.
- **§2 (Massive Scale Architecture)**, the parts that apply to a single
  authoritative host: spatial interest management, the asynchronous
  inference queue with scaffolds, HLOD impostors, mesh baking, and
  content-addressed asset streaming — see [§2 mechanisms](#2-mechanisms).
  The SpacetimeDB module (`crates/spacetime`) carries the cloud-tier
  version of the inference queue.

## Quick start

```bash
# On the host machine (authoritative + rendering):
localgpt-gen --host                        # session named "<user>'s world"
localgpt-gen --host --session-name "Castle Build" --port 9879

# On client machines:
localgpt-gen --join                        # browse mDNS, join the first session found
localgpt-gen --join 192.168.1.5            # bare host, default port 9879
localgpt-gen --join 192.168.1.5:9879       # explicit address
localgpt-gen --join --view-radius 4        # stream 4 chunks around the camera (default 2, max 8)
localgpt-gen --join --no-bake              # disable client-side static mesh baking
```

- Host: the normal interactive gen REPL + window, plus a lightyear UDP
  server on port **9879** and an mDNS announcement
  (`_localgpt-world._udp.local.`).
- Client: a slim Bevy viewer window with a free-fly camera (WASD/QE, hold
  right-click to look, scroll to change speed) and a REPL. Anything typed at
  the client REPL is sent to the host's agent as a queued job; a translucent
  scaffold appears where you are looking until the agent finishes, and
  replies and scene changes stream back.
- Client REPL lines starting with `/` stay local: `/stats` prints what the
  client is receiving (entities, scaffolds, visible impostors, baked chunks,
  streamed meshes); `/goto x y z` teleports the camera (handy for exercising
  chunk streaming).

Both sides must use the same netcode `protocol_id`
(`crates/gen/src/net/mod.rs`, currently `2`). Bump it on any wire-format
change; mDNS browsing filters mismatches via the `proto` TXT key.

## Architecture

```
                ┌────────────────────────────── Host (localgpt-gen --host) ─────────────────────────────┐
                │                                                                                     │
 User REPL ──┐  │  ┌──────────────┐   GenCommand mpsc   ┌─────────────┐                                 │
             ├──┼─▶│ Agent (tokio)│ ──────────────────▶ │ Bevy ECS    │──▶ Render (host window)         │
             │  │  │ (LLM tools)  │ ◀────────────────── │ (authorit.) │                                 │
 Client      │  │  └──────┬───────┘    GenResponse       └──────┬──────┘                                 │
 prompts ────┘  │         │ prompt/chat channels               │ net_attach / net_sync (world-model   │
 (via netcode)  │         ▼                                    │ shadow components)                   │
                │  ┌──────────────┐                            ▼                                      │
                │  │ lightyear    │◀── replication deltas ── Replicate::to_clients(All)                 │
                │  │ UDP :9879    │                                                                  │
                │  │ + netcode    │  mDNS announce: _localgpt-world._udp.local.                       │
                │  └──────┬───────┘                                                                  │
                └─────────┼─────────────────────────────────────────────────────────────────────────┘
                          │  reliable UDP (netcode-authenticated)
              ┌───────────┴───────────┐
              ▼                       ▼
      ┌──────────────┐        ┌──────────────┐
      │ Client       │        │ Client       │   slim Bevy viewer + REPL
      │ (read-only   │        │ (read-only   │   sends ClientPrompt messages
      │  viewer)     │        │  viewer)     │   receives HostChat messages
      └──────────────┘        └──────────────┘
```

### Code layout (`crates/gen/src/net/`, feature `multiplayer`, default on)

| File | Role |
|------|------|
| `protocol.rs` | Shared wire protocol: replicated component newtypes over `localgpt-world-types`, messages (`ClientPrompt`, `ClientView`, `HostChat`, `JobStatus`) + channels, decompose/compose helpers. Registered by `NetProtocolPlugin` on both sides. |
| `host.rs` | `NetHostPlugin`: lightyear server entity, replication attachment + change sync, per-link interest visibility, chunk summaries, prompt job queue + scaffolds, mesh asset publishing, chat broadcast, client lifecycle, mDNS announce. |
| `client.rs` | `NetClientPlugin`: link entity + connect, replicated-state → visuals builder, parent resolution, metadata application, view reports, prompt send + local scaffolds, job status, streamed mesh fetch, `/stats` + `/goto`, free-fly camera. |
| `client_lod.rs` | `ClientLodPlugin`: HLOD impostors for out-of-view chunks, static mesh baking. |
| `interest.rs` | Pure AoI policy: `ViewWindow`, `Relevance`, `VisibilityCache`, `ChunkSummaryBuilder`. |
| `jobs.rs` | Pure inference-queue bookkeeping: `JobQueue`, `JobState`, intake limits. |
| `bake.rs` | Pure baking pieces: quiet-period `BakeTracker`, layout-checked `merge_meshes`. |
| `assets.rs` | `LGM1` mesh blob codec, SHA-256 content addressing, host HTTP asset server, client fetch + disk cache. |
| `mdns.rs` | `SessionAnnouncer` (register, unregisters on drop) + `browse_sessions` (2–3 s window). |

### Replication model

- **Wire vocabulary = world types.** The host snapshots live ECS entities
  into `wt::WorldEntity` (reusing `snapshot_entity`, the same function the
  undo system uses) and decomposes it into `Net*` components
  (`NetWorldId`, `NetName`, `NetEntityKind`, `NetTransform`, `NetShape`,
  `NetMaterial`, `NetLight`, `NetBehaviors`, `NetAudio`, `NetMeshRef`,
  `NetParentId`). The client composes them back into meshes/materials/lights
  via the same conversion helpers the world-load path uses
  (`shape_to_mesh`, `material_def_to_standard`, `insert_light_component`).
- **Wire format:** replicon's compact default (postcard) for the simple
  components; **JSON replication rules** for Option-heavy world-types
  payloads (`MaterialDef`, `LightDef`, `BehaviorDef`, `AudioDef`,
  `MeshAssetRef`, `EnvironmentDef`). Those types rely on
  `#[serde(skip_serializing_if)]`, which is symmetric in self-describing
  formats but misaligns field-order formats like postcard (a skipped
  `Option` writes nothing while the reader expects a presence tag). See
  `json_rule_fns` in `crates/gen/src/net/protocol.rs`.
- **Entities are identified by stable host-assigned `wt::EntityId`s.** No
  Bevy `Entity` crosses the wire.
- **Attachment is automatic.** A `PreUpdate` system attaches
  `Replicate::to_clients(NetworkTarget::All)` to every newly spawned
  `GenEntity` (cameras excluded — clients render with their own camera);
  interest management then narrows it per client (see §2 below). The
  existing `GenCommand` handlers are untouched.
- **Deltas are change-driven.** A `PostUpdate` sync system
  epsilon-compares transforms (so behavior-animated entities stream at the
  50 ms replication interval but static ones don't resend) and re-snapshots
  entities whose shape/material/light/behaviors/parent changed.
  `NetTransform` uses lightyear's linear interpolation so client motion
  stays smooth between replication ticks.
- **Session metadata** (world name, background color, ambient light) rides
  a singleton replicated entity carrying `NetWorldMeta`.

### Command channel (client → host prompts)

Client REPL lines go out as `ClientPrompt { text, request_id, anchor }`
messages on a reliable ordered channel. On the host, `net_prompt_intake`
enqueues them as jobs (see [inference queue](#asynchronous-inference-queue--scaffolds))
and echoes them to all clients; `net_job_dispatch` hands one job at a time
to the agent loop (the REPL runs on a blocking thread feeding a merged
event stream, so local and remote prompts interleave). The agent's reply
text is broadcast as `HostChat { speaker: "host" }`, as are host-operator
prompts (`"host-user"`) and join/leave notices.

## Trust model (prototype)

- **LAN-only by design.** The netcode private key is a compile-time
  constant shared by all binaries; anyone on the LAN with the binary can
  connect. mDNS announcements are unauthenticated.
- No authorization on prompts: any connected client drives the host's
  agent (which has the host user's full tool access, including CLI tools).
  The job queue only rate-limits (4 queued prompts per client, 32 total).
- The asset server listens on all interfaces (TCP, same port number as the
  session) and serves any *published* mesh blob to anyone who knows its
  digest — the same data every connected client already receives. It
  never maps requests onto the filesystem.
- Interest management is a bandwidth optimisation, not access control: a
  client may report any camera position.
- Per-session keys, a pairing/pin step, and prompt-level authorization are
  prerequisites for anything beyond trusted-LAN use.

## Limitations

- Clients are **read-only viewers** — no client-side scene mutation, no
  avatar/player representation, no camera replication (the host only
  learns each client's camera position for interest management).
- Replicated: primitives (shape/material/transform), lights, groups,
  behaviors (data only — not ticked client-side), audio (data only),
  parent links, environment metadata, custom-mesh geometry (streamed as
  content-addressed blobs). **Not replicated:** terrain, water, foliage,
  sky, in-world UI (signs/HUD/labels), NPCs/players, glTF assets (glTF
  roots are not `GenEntity`s on the host; world-loaded mesh refs show a
  placeholder), physics state.
- Entity renames are not replicated (names are the stable registry key).
- Euler-angle interpolation is component-wise (no wrap handling).
- mDNS instance name collisions between two hosts with the same session
  name are not resolved (suffix the name manually).

## §2 mechanisms

The spec's §2 replaces the flat broadcast, synchronous inference, single
ECS, and pre-bundled assets of §1. Everything below runs inside the
listen-server session today and is shaped so the cloud tier can take it
over without protocol changes.

### Spatial interest management (AoI)

- The world is partitioned into the shared 64-unit `ChunkCoord` grid
  (`localgpt-world-types`, same constant as the SpacetimeDB module).
- Each client sends `ClientView { position, radius }` four times a second
  on a sequenced-unreliable channel (`--view-radius`, default 2 → a 5×5
  chunk window, clamped to 8).
- Every frame, `net_update_interest` places each replicated entity by its
  **hierarchy root's** world position (so a child's parent-relative
  transform always arrives with its parent) and calls lightyear's
  `gain_visibility` / `lose_visibility` per link — only on transitions,
  tracked by `VisibilityCache`. Directional lights, session metadata, and
  chunk summaries are global. Entities leaving a window despawn on that
  client; re-entering respawns them.
- It runs in `PostUpdate` after transform propagation and before
  lightyear's send, so no out-of-window entity is ever sent: a new link is
  hidden-from before its netcode handshake completes.

Verified end-to-end: with 12 entities at the origin and 5 at x≈500, a client
at the origin receives 14 (12 + ground + sun); after `/goto 505 10 520` it
holds 6 (the far group + sun) and the origin entities are gone.

### HLOD chunk impostors

The host folds every visible mesh's world AABB and base color into one
`NetChunkSummary` per occupied chunk (recomputed each second, re-sent only
when bounds/colour/count move) and replicates those to everyone. Clients
render each summary as a single box in the chunk's average colour, shown
only for chunks **outside** their own view window — so distant building
sites read as mass on the horizon while their detail isn't streamed.

### Asynchronous inference queue + scaffolds

1. The client computes an **anchor** (ground point under its gaze, else
   12 units ahead), sends the prompt with a random `request_id`, and
   immediately spawns a translucent **local scaffold** there (zero latency).
2. The host's `JobQueue` accepts it (FIFO; 32-job backlog, 4 per client),
   spawns a replicated `NetScaffold` at the anchor — everyone in range sees
   it — and replies `JobStatus::Queued { position }`. When the requester
   receives the replicated scaffold carrying its `request_id`, its local
   prediction is despawned (hand-off).
3. `net_job_dispatch` hands the next job to the agent loop only when the
   worker is idle. The agent gets the prompt prefixed with the anchor
   ("a connected user is looking at (x, y, z) …") so builds land where the
   requester is looking. The loop reports `Started` (scaffold turns amber
   and spins) and `Finished { error }`; the scaffold despawns, the real
   geometry is already replicating, and the requester gets
   `Done` / `Failed { reason }`. Queue positions are re-sent as jobs ahead
   finish. A disconnecting client's queued jobs are cancelled.

The queue is transport-agnostic bookkeeping; the SpacetimeDB module
(`crates/spacetime/src/jobs.rs`) implements the same lifecycle for the cloud
tier with the database as the queue: `submit_prompt` (clients),
`register_worker` (admin — the publisher, seeded at `init`),
`claim_job` / `worker_heartbeat` / `complete_job` (registered workers
only; completion only by the claimant), `cancel_prompt`, stale-claim
requeue after 120 s without a heartbeat, and pruning of terminal jobs. The
public `prompt_job` table doubles as the scaffold feed.

### Static mesh baking

Clients track every static primitive (no light, no behaviour on itself or
an ancestor) by chunk. Once a chunk has had no changes for 3 s and holds at
least 8 of them, they are grouped by material and vertex layout, merged in
world space into one mesh per group, and the originals' `Mesh3d`s are
parked. Any change in the chunk — a moved/added/removed entity, a new
material — un-bakes it immediately. Animated entities never mark chunks
dirty, so one spinning windmill doesn't block its neighbours. (In the
end-to-end test, 12 cubes collapse into one draw call.) Disable with
`--no-bake`.

### Asset streaming (content-addressed)

Custom meshes (`gen_spawn_mesh`) don't ride replication. The host encodes
each into an `LGM1` blob, publishes it in a content-addressed store keyed
by SHA-256, and replicates only `NetMeshAsset { digest, bytes }`. A small
HTTP server on the session's port number (TCP) serves
`GET /assets/{digest}` with `Cache-Control: immutable`; only published
digests resolve — nothing maps onto the host filesystem. Clients look in
memory, then the disk cache (`<cache>/gen-assets/<digest>.lgm`,
digest-verified on read), then fetch over HTTP on a worker thread, verify
the digest, and swap the placeholder for the real geometry. Identical
meshes share one blob, and a machine downloads each blob once across
sessions. Because URLs are content hashes, the host's server can be
replaced by a real CDN without protocol changes.

### Still §2-only (not built)

- A geo-partitioned headless server mesh with dynamic authority transfer
  (the SpacetimeDB tier is the intended route; the listen server has one
  authority).
- An embedded on-device LLM on the host (llama.cpp/candle); today the host
  uses any configured provider, including local Ollama.
- Mobile rendering via Filament/Metal, KTX2 textures, glTF streaming.

### Design seams kept for the cloud tier

- The wire data model is the SpacetimeDB tier's model
  (`localgpt-world-types`); replication events translate to reducer calls
  (`spawn_entity` / `modify_entity` / `remove_entity`) one-for-one.
- Mutations flow through a single authoritative path on the host (the
  `GenCommand` funnel), the seam a reducer-validation layer plugs into.
- Interest decisions are pure functions over chunk coordinates, matching
  the module's `chunk_subscription` table.

## Feature flag

The `multiplayer` cargo feature (default on) gates all of this; disable it
to drop the lightyear/mdns-sd dependency tree:

```bash
cargo check -p localgpt-gen --no-default-features
```
