# Collaborative World Engine: Architecture Specification (v2)

**Status:** implemented in part. Phases 1–5 and 7 are built and are what
`localgpt-gen --host` and `--join` run; phase 6 is half built; phase 8's
SpacetimeDB module was removed on 2026-11-01, and a cloud authority is out
of scope. The [phase table](#phases) says what is built and where. No model
runs for a room: a guest's prompt is accepted and failed with the reason,
and the host's own agent builds through the host's window over MCP. The
next design for rooms is
[rooms-and-the-channel.md](../authoring/rooms-and-the-channel.md), which
keeps this wire protocol and moves the room onto the desktop app's
authority.

Gen's collaboration is built around one idea: **a world is a shared
document, and every change to it is an op.** People and agents author ops;
one authority checks and orders them; every client renders the same
document. The requirement behind it is the second person's first minute:
they get a link, it opens in a browser with nothing to install, and they
see the same world the host sees. This is the second design (v2); the first
replicated Bevy ECS state from a LAN listen server, which a browser cannot
join.

## Principles

1. **Sync the document, not the engine.** The wire carries the world
   format's data (`localgpt-world-types`, which re-exports the
   `openworldformat` crate). Nothing on the wire names a Bevy entity, so
   any renderer that reads the world format can join: Gen (world-bevy), a
   browser (world-export's three.js viewer), later others.
2. **Every change is an op.** The vocabulary is the format's
   [`EditOp`](../../../crates/world-types/src/lib.rs): spawn, delete,
   modify (an `EntityPatch`), set environment, set camera, ambience, audio
   emitters, modify world (the scene-wide fields), and an all-or-nothing
   `Batch`. Tool calls, human edits, imports, undo and redo all end up as
   ops.
3. **One authority per room.** It checks, orders and fans out ops. There's
   always exactly one, so no CRDTs are needed.
4. **Anything a world can save, a session can sync.** The session carries
   exactly what the format carries. A feature that doesn't sync is a feature
   missing from the format, and it's missing from saved worlds too.
5. **Agents are authors like anyone else.** An agent's ops carry an author
   and are checked like anyone's. No model runs for a room today: the
   host's agent drives the host's window over MCP, and its work reaches the
   room as the host's ops. `prompt` and `job` stay in the protocol for a
   guest's own agent (phase 6).
6. **Presence is separate from the document.** Avatars, gaze and selections
   are high-rate, lossy and never stored.

## Architecture

```
 guests (render + presence)             the host's window
 Gen --join · browser (three.js)        Gen's tools, an agent over MCP,
        │                               the inspector, undo and redo
        │ ops, presence, chat                  │ projection, 4 Hz:
        │                                      │ scene → ops
        └───────────────┬──────────────────────┘
                        ▼
      authority  (localgpt-world-sync: pure Rust, no Bevy, no I/O)
        check: role, limits, expected_revision, then apply atomically
        order → revision → fan out · presence relay · prompt jobs
                        │
 runs in:  the host's Gen (--host)
 reached:  LAN directly · localgpt-relay for the internet
 stored:   the transport appends every committed batch to the op log
```

### The authority

`crates/world-sync` (`localgpt-world-sync`) is the room around the fold:
the authority (peers, roles, revisions, presence, prompt jobs and the
per-author undo stacks), the diff that turns a scene into ops, the inverse
of a batch, and the wire protocol. The document, op application and the op
log entry are the Open World Format's, re-exported from the
`openworldformat` crate (`doc`, `oplog`, `session`). The crate depends on
no Bevy, async runtime or socket code, so a transport feeds it messages and
delivers what comes out. Gen's session server (`crates/gen/src/net/web.rs`)
is the only transport today, and it owns what the authority does not touch:
the sockets, the op log file and the snapshots.

Work that needs the scene (ground snapping, navmesh queries, collision-aware
placement, screenshots) happens in the host's Gen, where the tools run. The
authority doesn't need any of it.

### Capturing ops from Gen

Gen's tools mutate the Bevy world directly, and only some record `EditOp`s
in the undo stack. Rather than instrument every tool, the host **projects**
its scene into the format's types (the same `snapshot_entity` used by undo
and save) four times a second, and diffs that projection against the
document (`web::web_projection_sync`, `world-sync`'s `diff_scene`). The diff
becomes ops, authored as `host`. This covers every tool, the inspector and
undo/redo at once. Entities with behaviors are projected at their *base*
position and scale and keep the document's rotation (behaviors animate on
every client from the shared definitions), so animation never generates
ops. Background and ambient light are read from the render resources; fog
is not captured live.

The other direction has to exist too: a batch the authority commits that
did not come from the scene (a guest's edit, an undo) is applied back into
the host's scene (`gen3d/ops_apply.rs`), or the next projection would
restore the scene from before it.

The projection makes the scene a co-author of the document. A live room
survives that; a history that can be scrubbed does not, and
[world-editor-stack.md](../authoring/world-editor-stack.md) has the rule
that replaces it and where the projection still stands.

### Transport

**WebSocket is the baseline**, carrying JSON: `GET /session` on the session
port (TCP 9879 by default, `--port`). Browsers, relays and NATs all handle
it, and building together doesn't need UDP. Replicating engine state cannot
serve a browser client, and a Bevy-to-wasm client would be a download of
tens of megabytes. A replication transport returns only if shared,
client-predicted physics becomes a feature.

The manifest in `welcome` and `snapshot` is the format's, which
`world.schema.json` describes; ops are the format's `EditOp` in serde's
externally tagged form (`{"SpawnEntity": {…}}`).

### Browser guests

The host serves a join page at `/` on the session port, with the viewer,
the session client and a vendored three.js beside it, so a guest needs no
install and no internet. A guest watches, walks and chats; `/undo` in the
chat box takes back their own newest batch. With `--web-edit` they join as
editors and select, drag, rotate, scale and delete
(`crates/world-export/js/session-client.js`). The page still has a prompt
box; the room fails what it sends, with the reason. Phones and tablets are
covered by the browser.

### Hosting modes

| Mode | Authority | Reach | Survives host leaving | Built |
|------|-----------|-------|-----------------------|-------|
| Local room | Inside the host's Gen (`--host`) | LAN; internet via the relay | No | Yes |
| Persistent room | A Gen without a window, on a server | Direct or relay | Yes | No: `--host` runs only with the window |

On a LAN the host announces the session over mDNS
(`_localgpt-world._udp.local.`, `net/mdns.rs`), and `--join` with no address
browses for it.

`localgpt-relay` (`crates/relay`) only forwards frames between the host and
guests by room code, in a small envelope, and serves the join page; it
never parses the frames or holds world state. The host connects out to it
(`--relay <url>`, `net/relay_client.rs`), so nothing is port-forwarded.
It's small enough to run as one Durable Object per room.

## Wire protocol (version 1)

JSON text frames over one WebSocket (`GET /session` on the session port).
Every message is an object with a `type` field. Ops use the format's serde
form. Coordinates are world units, Y up. The types are
`crates/world-sync/src/protocol.rs`.

### Client → server

| `type` | Fields | Who |
|--------|--------|-----|
| `hello` | `protocol`, `name`, `token?` (the invite token, or the PIN from a native client), `client` (`web`/`gen`) | everyone, first message |
| `presence` | `position`, `look_at`, `selected?` | everyone; both clients send at 5 Hz |
| `prompt` | `request_id`, `text`, `anchor?` | everyone; the room fails it |
| `chat` | `text` | everyone |
| `submit` | `client_seq`, `expected_revision?`, `ops` | editors and up |
| `undo` | — | everyone; takes back the sender's newest batch |
| `resync` | — | a client that saw a revision gap |
| `ping` | `t` | anyone |

### Server → client

| `type` | Fields |
|--------|--------|
| `welcome` | `protocol`, `peer_id`, `role`, `session` (`name`), `revision`, `world` (a `WorldManifest`), `peers`, `jobs`, `asset_base?` |
| `ops` | `revision`, `author`, `ops`, `client_seq?` (the submitter's, on a submitted batch) |
| `reject` | `client_seq`, `reason` |
| `snapshot` | `revision`, `world` (answer to `resync`) |
| `peer_joined` / `peer_left` | `peer` / `peer_id` |
| `presence` | `peer_id`, `presence` |
| `job` | `job` (`job_id`, `requester?`, `prompt`, `anchor?`, `state`), `request_id?` |
| `chat` | `from`, `text`, `kind` (`human`/`agent`/`system`) |
| `error` | `reason` (the server closes the socket after it) |
| `pong` | `t` |

`author` and `from` are the log's `Author` (`peer?`, `name`, `source?`).
`job.state` is `{"state": "queued", "position": n}`, `running`, `done`,
`failed` or `rejected`, the last two with a `reason`. An `ops` message goes
to every peer as one frame, `client_seq` included. `asset_base` is never
set today: the host serves no asset files to guests.

`revision` increases by one per `ops` message. A client applies `ops` whose
revision is exactly one more than its own; on a gap it sends `resync`.

### Roles

| Role | Presence, chat, `prompt`, `undo` | `submit` |
|------|----------------------------------|----------|
| `guest` (default) | yes | no |
| `editor` | yes | yes |
| `host` | yes | yes |

The transport gives the role at `hello`: `guest`, or `editor` for every
joiner when the host passed `--web-edit`. Nothing gives `host`: the hosting
window is not a peer, and its changes enter through the projection.

### Conflicts

- The authority's order is the truth.
- `modify` patches merge per field; for the same field, the last write wins.
- A delete wins: a later `modify` or `delete` of a missing entity is
  rejected.
- `Batch` applies all or nothing. With `expected_revision` set, a batch
  planned against an older revision is rejected (Gen's batch tools make the
  same check locally).

### Limits

1 MiB per frame; 16 peers per room; 256 ops per `submit`; chat lines cut at
2,000 bytes and prompts at 4,096; names at 32 characters; 4 queued prompts
per peer and 32 per room; 64 undoable batches per author. Presence is not
rate-limited by the host; both clients send it at 5 Hz. The room does not
run the format's `WorldLimits` validation, so nothing caps the document's
size.

## Security

[docs/gen/multiplayer.md](../../gen/multiplayer.md) states the trust model
for someone hosting. What the protocol does:

- **Two bearer secrets open a room, both sent in `hello.token` in the
  clear.** A PIN session makes a six-digit PIN, which a native `--join`
  sends, and a random 128-bit invite token, which the browser link carries
  in its fragment. `--web` prints that link; the join page and `/session`
  are served by every hosted session. An `--open` session has neither.
- **Through the relay, only the token opens the room.** Anyone with a room
  code can open a relayed connection, so the PIN is not taken there, and a
  refused hello ends the connection. On a LAN, five wrong PINs stop the
  session taking the PIN; the token still joins.
- **The relay reads everything.** TLS in front of it ends at the relay,
  which sees the token and the room as they pass and serves the join page.
  A key per person and end-to-end encryption are
  [rooms-and-the-channel.md](../authoring/rooms-and-the-channel.md)'s.
- **Cross-origin sockets are refused.** The upgrade must come from the page
  the host served (the `Origin` must match the `Host`), or from a native
  client (no `Origin`), so a malicious website can't drive a LAN session from
  a visitor's browser.
- **Nothing a guest sends runs a model or a tool on the host.** A guest's
  `submit` is rejected, a prompt is failed, and only ops change the world.
- **The authority checks every submitted batch**: role, size,
  `expected_revision`, and that it applies. The host's own projection
  enters without the role and revision checks, since it describes the scene
  the room is hosting.

## Scope

Rooms: small groups building together, hosted from a desktop and reached on
a LAN or through a relay. A cloud authority and planet-scale persistent
worlds are out of scope ([world-strategy.md](../../world-strategy.md)).

## Phases

| # | Phase | Status |
|---|-------|--------|
| 1 | **Ops and the authority in the host.** The host projects its scene into ops and serves `/session` over WebSocket. | Built: `crates/world-sync`; `crates/gen/src/net/web.rs` (the room, the socket, `web_projection_sync`); `gen3d/ops_apply.rs` for committed ops back into the scene. |
| 2 | **Presence.** Avatars, names and gaze for every peer, in Gen and the browser. | Built for guests: the host's window and a native client draw peers (`net/guest_avatars.rs`), the browser draws them (`session-client.js`). The hosting window is not a peer, so guests do not see the host. |
| 3 | **Browser guests.** The host serves a join page; the world-export viewer applies ops live; guests chat. | Built: `crates/world-export/js/` (`session-client.js`, `world-viewer.js`'s `applyOps`, three.js under `vendor/`). A prompt from the page is failed. |
| 4 | **Relay and invite links.** Room codes through a relay. | Built: `crates/relay`, `net/relay_client.rs` (`--relay`). The relay encrypts nothing; see [Security](#security). |
| 5 | **Persistence and history.** The op log, resume, replay, per-author undo. | Built: the session package ([session-package-format.md](session-package-format.md)), `--resume`, `--replay` (`gen3d/replay.rs`), `undo` (`world-sync/src/undo.rs`, `Authority::undo`). |
| 6 | **Guest editing.** Editor role; direct manipulation; a guest's own agent. | Half built: browser editors select, drag, rotate, scale and delete (`--web-edit`). A native guest has no editing, and no guest's agent builds through the room. |
| 7 | **Native clients on ops.** `--join` speaks the op protocol. | Built: `net/ops_client.rs`. The replication transport of the first design is gone. |
| 8 | **Cloud rooms.** | Not built, and out of scope: the SpacetimeDB module was removed on 2026-11-01. A room that outlives its host would be the persistent room above. |

Also not built: asset delivery to guests (`asset_base`), interest management
(every guest syncs the whole document, which is fine at room scale), and
the format gaps that block full sync and are lost on save too: terrain,
water, foliage, sky, in-world UI (signs, HUD, labels) and NPC bodies have
no representation in the format.

## Related documents

- [docs/gen/multiplayer.md](../../gen/multiplayer.md) — how to host and join today
- [session-package-format.md](session-package-format.md) — the session artifact
- [../authoring/rooms-and-the-channel.md](../authoring/rooms-and-the-channel.md) — the proposed identity and encrypted-channel layer, and rooms in the desktop app
- [../authoring/world-editor-stack.md](../authoring/world-editor-stack.md) — the one-direction rule the projection gives way to
