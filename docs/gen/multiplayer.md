# Gen Multiplayer — Phase 1: Listen Server Prototype

Implementation of **§1 (Prototype & Session-Based Architecture)** of the
[Collaborative World Engine spec](../rfcs/multiplayer/collaborative-world-engine-architecture.md).
One desktop `localgpt-gen` instance acts as the **authoritative host and
rendering client**; secondary clients discover the session over mDNS and join
as read-only viewers that can send natural-language prompts to the host's
agent.

The cloud MMO tier (§2) is **not** built here, but Phase 1 keeps its design
compatible with it — see [§2 compatibility](#2-compatibility).

## Quick start

```bash
# On the host machine (authoritative + rendering):
localgpt-gen --host                        # session named "<user>'s world"
localgpt-gen --host --session-name "Castle Build" --port 9879

# On client machines:
localgpt-gen --join                        # browse mDNS, join the first session found
localgpt-gen --join 192.168.1.5            # bare host, default port 9879
localgpt-gen --join 192.168.1.5:9879       # explicit address
```

- Host: the normal interactive gen REPL + window, plus a lightyear UDP
  server on port **9879** and an mDNS announcement
  (`_localgpt-world._udp.local.`).
- Client: a slim Bevy viewer window with a free-fly camera (WASD/QE, hold
  right-click to look, scroll to change speed) and a REPL. Anything typed at
  the client REPL is sent to the host's agent; replies and scene changes
  stream back.

Both sides must use the same netcode `protocol_id`
(`crates/gen/src/net/mod.rs`, currently `1`). Bump it on any wire-format
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
| `protocol.rs` | Shared wire protocol: replicated component newtypes over `localgpt-world-types`, `ClientPrompt`/`HostChat` messages + channels, decompose/compose helpers. Registered by `NetProtocolPlugin` on both sides. |
| `host.rs` | `NetHostPlugin`: lightyear server entity, replication attachment + change sync, prompt intake, chat broadcast, client lifecycle, mDNS announce. |
| `client.rs` | `NetClientPlugin`: link entity + connect, replicated-state → visuals builder, parent resolution, metadata application, prompt send / chat receive, free-fly camera. |
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
  `GenEntity` (cameras excluded — clients render with their own camera).
  The existing `GenCommand` handlers are untouched.
- **Deltas are change-driven.** A `PostUpdate` sync system
  epsilon-compares transforms (so behavior-animated entities stream at the
  50 ms replication interval but static ones don't resend) and re-snapshots
  entities whose shape/material/light/behaviors/parent changed.
  `NetTransform` uses lightyear's linear interpolation so client motion
  stays smooth between replication ticks.
- **Session metadata** (world name, background color, ambient light) rides
  a singleton replicated entity carrying `NetWorldMeta`.

### Command channel (client → host prompts)

Client REPL lines go out as `ClientPrompt` messages on a reliable ordered
channel. On the host, `net_prompt_intake` forwards them to the agent loop
through a tokio channel (the REPL was restructured onto a blocking thread
feeding a merged event stream, so local and remote prompts interleave) and
echoes them to all clients. The agent's reply text is broadcast as
`HostChat { speaker: "host" }`, as are host-operator prompts
(`"host-user"`) and join/leave notices.

## Trust model (prototype)

- **LAN-only by design.** The netcode private key is a compile-time
  constant shared by all binaries; anyone on the LAN with the binary can
  connect. mDNS announcements are unauthenticated.
- No authorization on prompts: any connected client drives the host's
  agent (which has the host user's full tool access, including CLI tools).
- Per-session keys, a pairing/pin step, and prompt-level authorization are
  prerequisites for anything beyond trusted-LAN use.

## Phase-1 limitations

- Clients are **read-only viewers** — no client-side scene mutation, no
  avatar/player representation, no camera replication.
- Replicated: primitives (shape/material/transform), lights, groups,
  behaviors (data only — not ticked client-side), audio (data only),
  parent links, environment metadata. **Not replicated:** terrain, water,
  foliage, sky, in-world UI (signs/HUD/labels), NPCs/players, custom-mesh
  geometry (shown as a placeholder), glTF assets (placeholder), physics
  state.
- Entity renames are not replicated (names are the stable registry key).
- Euler-angle interpolation is component-wise (no wrap handling).
- mDNS instance name collisions between two hosts with the same session
  name are not resolved (suffix the name manually).

## §2 compatibility

Design choices made specifically so the §2 (cloud MMO / SpacetimeDB) tier
can reuse this work:

- The wire data model is exactly the SpacetimeDB tier's model
  (`localgpt-world-types`); the `crates/spacetime` module maps the same
  types to tables, so replication events translate to reducer calls
  (`spawn_entity` / `modify_entity` / `remove_entity`) one-for-one.
- Mutations flow through a single authoritative path on the host (the
  `GenCommand` funnel), which is the seam a future reducer-validation
  layer or async inference queue (§2's scaffold-then-replace flow) plugs
  into.
- Replication targets are `NetworkTarget` values — flat
  `NetworkTarget::All` broadcast today, per-client interest scopes
  (spatial subscription gateway) later without protocol changes.
- Stable entity ids + `ChunkCoord` support in world-types keep the
  chunked AoI partitioning (64-unit chunks, shared constant with the
  SpacetimeDB module) available for delta scoping.

## Feature flag

The `multiplayer` cargo feature (default on) gates all of this; disable it
to drop the lightyear/mdns-sd dependency tree:

```bash
cargo check -p localgpt-gen --no-default-features
```
