# Collaborative World Engine: Architecture Specification (v2)

Gen's collaboration is built around one idea: **a world is a shared
document, and every change to it is an op.** People and AI agents author
ops; one authority checks and orders them; every client renders the same
document. This replaces v1's plan (a LAN listen server replicating Bevy ECS
state, scaling up to a cloud MMO). The mechanisms v1 built are kept where they
still fit; see [What carries over](#what-carries-over).

## Why collaboration, and what it has to be

Blender with an MCP server makes better assets than Gen ever will, and its
renderer beats Bevy's on looks. Gen's value is somewhere else: a *place*,
not an asset. It's walkable, animated, lit, audible and simulated, and you
build it by talking, together, in real time. Collaboration is what turns Gen
from a tool into a place, and it's how new users arrive: someone builds,
sends a link, and a friend is standing in the world a few seconds later.

The competition for "build a world together" is Roblox Studio and Minecraft,
not Blender. Gen's edge over those is that it's open source, runs on your
machine, works with the model you choose, and saves to open formats.

So the second person's first minute is the product:

1. They get a link.
2. It opens in a browser. Nothing to install.
3. They see the host's avatar and the same world the host sees.
4. They type "add a lighthouse on that cliff," and it appears for everyone.

v1 couldn't do any of this. Joining needed the same LAN and an installed
binary (lightyear over UDP); guests had no avatars and couldn't see each
other; and terrain, water, foliage, sky, signs and HUD, NPCs, glTF assets and
physics didn't replicate, so guests saw a different world than the host.

## Principles

1. **Sync the document, not the engine.** The wire carries
   `localgpt-world-types` data. Nothing on the wire names a Bevy entity, so
   any renderer that reads the world format can join: Gen (world-bevy), a
   browser (world-export's three.js viewer), later others.
2. **Every change is an op.** The vocabulary is world-types'
   [`EditOp`](../../../crates/world-types/src/history.rs): spawn, delete,
   modify (an `EntityPatch`), set environment, set camera, ambience, audio
   emitters, and an all-or-nothing `Batch`. Tool calls, human edits,
   imports, undo and redo all end up as ops.
3. **One authority per room.** It checks, orders, stores and fans out ops.
   There's always exactly one, so no CRDTs are needed.
4. **Anything a world can save, a session can sync.** The session carries
   exactly what the format carries. A feature that doesn't sync is a feature
   missing from world-types, and it's missing from saved worlds too.
5. **Agents are authors like anyone else.** An AI emits ops under the
   identity of the person who asked, and its ops are checked like anyone's.
6. **Presence is separate from the document.** Avatars, gaze and selections
   are high-rate, lossy and never stored.

## Architecture

```
 clients (render + presence)              agents (emit ops, never mutate state)
 Gen desktop · browser (three.js)         host's model · guest's model · cloud worker
        └──── ops + presence ────┬──── ops, as the requesting user ────┘
                                 ▼
      authority  (localgpt-world-sync: pure Rust, no Bevy, no I/O)
        check: role, schema, limits, expected_revision
        order → op log + snapshots → fan out
        presence relay · prompt job queue
                                 │
 runs in:  the host's Gen (local room) · a headless Gen on a server
           (persistent room) · a SpacetimeDB module (cloud rooms)
 reached:  LAN directly · a relay for internet and browsers
```

### The authority

`crates/world-sync` (`localgpt-world-sync`) holds everything that doesn't
need a renderer: the document, op application and validation, the diff that
turns a scene into ops, the wire protocol, and the authority's bookkeeping
(peers, roles, revisions, presence, the op log). It has no Bevy, tokio or
network code, so the same logic runs inside Gen, in a headless server, or
compiled to wasm.

Work that needs the scene (ground snapping, navmesh queries, collision-aware
placement, screenshots for self-review) happens in the *agent's* copy of the
world, which is a Gen instance (windowed or headless). The authority doesn't
need any of it.

### Capturing ops from Gen (phase 1)

Gen's ~100 tools mutate the Bevy world directly, and only some record
`EditOp`s in the undo stack. Rather than instrument every tool, the host
**projects** its scene into world-types (the same `snapshot_entity` used by
undo and save) a few times a second, and diffs that projection against the
document. The diff becomes ops. This covers every tool, the inspector and
undo/redo at once. Entities with behaviors are projected at their *base*
transform (behaviors animate on every client from the shared definitions),
so animation never generates ops.

Later phases let tools emit ops directly; the projection then becomes a
consistency check.

### Transport

**WebSocket is the baseline**, carrying JSON of world-types. Browsers, relays
and NATs all handle it, and `world.schema.json` already describes the payload
for typed clients. Building together doesn't need UDP. Lightyear's component
replication can't serve a browser client, and a Bevy-to-wasm client would be
a download of tens of megabytes. Lightyear stays only while native `--join`
still uses it (phase 7 retires it), and returns only if shared,
client-predicted physics becomes a feature.

### Browser guests

The world-export viewer joins a session. A guest can watch, walk, chat and
prompt; direct editing comes in phase 6. Phones and tablets are covered by
the browser, which replaces v1's plan for a native mobile renderer.

### Hosting modes

| Mode | Authority | Reach | Survives host leaving |
|------|-----------|-------|-----------------------|
| Local room (default) | Inside the host's Gen | LAN; internet via relay | No |
| Persistent room | Headless Gen on a server | Direct or relay | Yes |
| Cloud room | SpacetimeDB module | Internet | Yes |

The relay only forwards frames between the host and guests by room code; it
never holds world state. It's small enough to run as one Durable Object per
room.

## Wire protocol (version 1)

JSON text frames over one WebSocket (`GET /session` on the session port).
Every message is an object with a `type` field. Ops use world-types' serde
form. Coordinates are world units, Y up.

### Client → server

| `type` | Fields | Who |
|--------|--------|-----|
| `hello` | `protocol`, `name`, `token?`, `client` (`web`/`gen`) | everyone, first message |
| `presence` | `position`, `look_at`, `selected?` | everyone, ≤10 Hz |
| `prompt` | `request_id`, `text`, `anchor?` | guests and up |
| `chat` | `text` | everyone |
| `submit` | `client_seq`, `expected_revision?`, `ops` | editors and up (phase 6) |
| `resync` | — | a client that saw a revision gap |
| `ping` | `t` | anyone |

### Server → client

| `type` | Fields |
|--------|--------|
| `welcome` | `protocol`, `peer_id`, `role`, `session`, `revision`, `world` (a `WorldManifest`), `peers`, `jobs`, `asset_base?` |
| `ops` | `revision`, `author` (`peer_id?`, `name`), `ops`, `client_seq?` (echoed to the submitter) |
| `reject` | `client_seq`, `reason` |
| `snapshot` | `revision`, `world` (answer to `resync`) |
| `peer_joined` / `peer_left` | `peer` / `peer_id` |
| `presence` | `peer_id`, `presence` |
| `job` | `job_id`, `request_id?`, `requester?`, `prompt`, `anchor?`, `state` |
| `chat` | `from` (`peer_id?`, `name`), `text`, `kind` (`human`/`agent`/`system`) |
| `error` | `reason` (the server closes the socket after it) |
| `pong` | `t` |

`revision` increases by one per `ops` message. A client applies `ops` whose
revision is exactly one more than its own; on a gap it sends `resync`.

### Roles

| Role | Presence, chat | Prompt the room's AI | Submit ops |
|------|----------------|----------------------|------------|
| `guest` (default) | yes | yes | no |
| `editor` | yes | yes | yes |
| `host` | yes | yes | yes |

### Conflicts

- The authority's order is the truth.
- `modify` patches merge per field; for the same field, the last write wins.
- A delete wins: a later `modify` or `delete` of a missing entity is
  rejected.
- `Batch` applies all or nothing. With `expected_revision` set, a batch
  planned against an older revision is rejected (Gen's batch tools already do
  this locally since `476353a`).

### Limits

1 MiB per frame; 256 ops per `submit`; world size capped by world-types'
`WorldLimits`; 4 queued prompts per guest and 32 per room (v1's job queue);
presence coalesced to 10 Hz per peer.

## Security

- **Browser joining is opt-in** (`--web`). Without it the session's
  security posture is unchanged from v1.
- **Invite links are bearer tokens.** A PIN session's link carries a random
  128-bit token that the page sends in `hello`. On a LAN the page is plain
  HTTP, so anyone who can read the network traffic can read the token. Use
  `--web` on networks you trust; the relay (phase 4) adds TLS. Browser PIN
  pairing (SPAKE2 compiled to wasm) is a follow-up. An `--open` session
  needs no token.
- **Cross-origin sockets are refused.** The upgrade must come from the page
  the host served (the `Origin` must match the `Host`), or from a native
  client (no `Origin`), so a malicious website can't drive a LAN session from
  a visitor's browser.
- **Guests can't edit the document directly.** Their prompts run on v1's
  scoped remote agent (scene tools only, no shell, files or memory).
- **The authority checks every op**, whoever sent it: role, schema, limits,
  revision.

## What carries over

| v1 mechanism | v2 |
|--------------|----|
| PIN pairing (SPAKE2 + AEAD) | Kept for native clients; browser pairing via wasm is a follow-up |
| mDNS discovery | Kept; advertises the session port |
| Session HTTP (assets + pairing) | Kept; also serves `/session` and the join page |
| Prompt job queue + scaffolds | Kept; browser guests' prompts enter the same queue |
| Scoped remote agent | Kept |
| Chunk interest management | Becomes a subscription filter on ops (later) |
| HLOD summaries, client mesh baking | Kept as client-side rendering optimizations |
| Content-addressed mesh streaming | Kept; browsers fetch the same blobs (later) |
| Batch `expected_revision` | Becomes the protocol's conflict check |
| SpacetimeDB tables and job queue | The cloud-room authority (phase 8) |
| Lightyear component replication | Retired once native `--join` uses ops (phase 7) |

## Scope

Gen's open-source tier covers **rooms**: small groups building together,
hosted locally, relayed, or self-hosted persistently. Planet-scale,
geo-partitioned persistent worlds are out of scope for this roadmap. The
deep-dive documents below stay as research.

## Phases

Steps 1–4 add up to the first demo worth showing.

| # | Phase | Done when |
|---|-------|-----------|
| 1 | **Ops and the authority in the host.** `world-sync` crate; the host projects its scene into ops and serves `/session` over WebSocket. | A client connected to a hosting Gen receives the world and every later change, including undo, as `ops`. |
| 2 | **Presence.** Avatars, names and gaze for every peer, in Gen and the browser. | Host and guests see each other move. |
| 3 | **Browser guests.** The host serves a join page; the world-export viewer applies ops live; guests chat and prompt. | A browser on the LAN joins with the invite link, sees the world update live, and a prompt from it builds for everyone. |
| 4 | **Relay and invite links.** Room codes through a relay with TLS. | A guest on another network joins from a link. |
| 5 | **Persistence and history.** Op log in the world folder; replay; per-user undo; time-lapse. | A room reopened later has its history; a build can be replayed. |
| 6 | **Guest editing.** Editor role; direct manipulation; guests bringing their own model. | A guest moves an object and a desktop guest's own agent builds, both under their names. (Browser editing is Done via `--web-edit`; BYO-model guests arrive with phase 7.) |
| 7 | **Native clients on ops.** `--join` uses the op protocol; lightyear removed. | One protocol for every client. |
| 8 | **Cloud rooms.** The SpacetimeDB module as an authority. | A room lives without any host running. |

Format gaps that block full sync, and are lost on save today too: terrain,
water, foliage, sky, in-world UI (signs, HUD, labels) and NPC bodies need
world-types representations.

## Implementation status

| Item | Status | Where |
|------|--------|-------|
| `localgpt-world-sync`: document with atomic, validated `EditOp` application | Done (9 tests) | `crates/world-sync/src/doc.rs` |
| `EntityPatch.modulations`, so every per-entity field can be patched | Done | `crates/world-types/src/entity.rs` |
| `localgpt-world-sync`: scene diff, wire protocol, room authority | Done (24 tests) | `crates/world-sync/src/{diff,protocol,authority}.rs` |
| Host session endpoint (`--web`): join page, WebSocket, invite tokens | Done | `crates/gen/src/net/web.rs` |
| Scene projection → ops (covers every tool, inspector, undo) | Done | `web::web_projection_sync` |
| Browser join page and live viewer (ops applied live; guests chat + prompt) | Done | `crates/world-export/js/` |
| Browser prompts through the room's job queue and scoped agent | Done | `web.rs`, `net/jobs.rs` |
| Peer avatars in the browser; joins/leaves/presence fan-out | Done | `session-client.js` |
| Peer avatars in the host's own window | Done | `crates/gen/src/net/guest_avatars.rs` |
| Op log persisted per session; `--resume` replays it | Done | `web.rs`, `world-sync/oplog.rs` |
| Per-user undo (own edits + builds the AI made for you) | Done | `world-sync/undo.rs`, `authority::undo` |
| Ops applied doc→scene (undo/edits stick) | Done | `gen3d/ops_apply.rs` |
| Time-lapse replay (`--replay ops.jsonl`) | Done | `gen3d/replay.rs` |
| Fully offline join page (vendored three.js) | Done | `world-export/js/vendor/` |
| Relay | Planned | — |
| Guest editing UI (select/drag/rotate/scale/delete; `--web-edit`) | Done | `session-client.js` |

Phase 1 and the core of phase 3 are verified end to end: a browser joined a
hosting Gen over the invite link, received the world (revision 1), watched a
guest join/leave, exchanged presence and chat, was refused a direct edit, and
prompted the room's AI — the build streamed back as ops (`ws-test-cube`)
while the job states advanced `queued → running`.


## Related documents

- [docs/gen/multiplayer.md](../../gen/multiplayer.md) — how to host and join today
- [SpacetimeDB Integration Design](../../architecture/spacetimedb-integration-design.md) — the cloud-room authority (phase 8)
- Research, not roadmap: [Massively Multiplayer Persistent World](massively-multiplayer-persistent-world.md),
  [Massively Multiplayer Co-Creation](massively-multiplayer-co-creation.md),
  [Multi-Scale 3D Universe](multi-scale-3d-universe.md)
