# Gen Multiplayer — One Protocol, Every Client

Gen's collaboration is built around one idea: **a world is a shared
document, and every change to it is an op.** The host runs the room
authority (`localgpt-world-sync`); native and browser clients connect over
one WebSocket and render the same document. See
`docs/rfcs/multiplayer/collaborative-world-engine-architecture.md` for the
full design.

## Quick start

```bash
localgpt-gen --host --web --mcp-relay      # a room, a browser invite link, and your agent building in it over MCP
localgpt-gen --join 192.168.1.5 --pin 482913
```

Every flag is in the user docs (`website/content/docs/gen/multiplayer.md`)
and parsed in `crates/gen/src/main.rs`. What each side is:

- **Host:** the normal interactive gen window plus a session HTTP server on
  the session port (join page + WebSocket ops endpoint) and an mDNS
  announcement (`_localgpt-world._udp.local.`). The console shows the PIN.
- **Native client (`--join`):** the full gen scene — camera, behaviors,
  audio — driven by the room instead of tools. You see every guest as a
  colored capsule, and anything typed at the terminal is said to the room
  as chat. `/stats` shows the room; `/quit` exits.
- **Browser guests:** the world-export three.js viewer, live over the same
  protocol — see [Browser guests](#browser-guests---web).
- **Building:** no model runs for the room. The host's own agent builds —
  outside the window, over MCP (`--mcp-relay` beside `--host`) — and its
  edits reach every guest as the host's ops. A live canvas is not a way in:
  `--live` and `--replay` are modes of their own, and given with `--host`
  they run and no room is hosted.
- **Not from the desktop app:** `localgpt-app` hosts no room. Rooms in the
  app, over the `.world` it has open, are
  `docs/rfcs/authoring/rooms-and-the-channel.md`'s.

## Architecture

```
 host Gen ── projection (~4 Hz): scene → ops ─┐
                                              ▼
        localgpt-world-sync Authority  (the room: document, peers,
        check, order, op log, fan-out)   revisions, presence, jobs)
                                              │ ops, in commit order
            ┌──────────────────┬──────────────┴───────────────┐
            ▼                  ▼                              ▼
     native --join      browser guests                 the host's own
     (full gen scene)   (three.js viewer)              window (ops apply
                                                        doc→scene)
```

- **The document is the wire.** Messages carry world-types `EditOp`s
  (spawn/delete/modify/environment), never engine state — any renderer that
  reads the world format can join.
- **The host projects its scene into ops** a few times a second (the same
  snapshot the undo stack and save use), so every tool, the inspector and
  undo/redo sync without per-tool work. Behavior-animated entities project
  at their base transform; clients animate from the shared definitions.
- **Committed ops apply back into the host's scene** (undo, guest edits) —
  the projection can't heal them away.
- **Every committed batch lands in the op log**
  (`<workspace>/sessions/<name>/ops.jsonl`): resume a room later, undo per
  author, replay as a time-lapse.
- **A prompt from any client** is accepted by the authority (every peer sees
  it queued, as the protocol says) and failed at once with the reason: this
  room runs no AI. The prompt and job messages stay in the protocol for an
  agent of a guest's own, later.
- **Presence** (position and gaze) is sent on the same socket at ~5 Hz,
  fanned out to the other peers and never stored. The protocol has a
  `selected` field that neither client sends.

## Browser guests (`--web`)

Every hosted session serves a **join page** at `http://<host>:<port>/`;
what `--web` adds is the printed invite link, which carries the token unless
the session is `--open`. Guests can watch, walk and chat, and with
`--web-edit` they edit directly (the page's controls are in the user docs).
`--web-edit` gives every joiner the editor role, but only the page has
editing controls. Everything is served by the host (three.js is vendored):
a guest needs no install and no internet.

## History, undo & replay

Every session is a package: a base world (`world.ron`), the one log
(`ops.jsonl`), metadata (`session.json`) and keyframe snapshots
(`snapshots/rev-<N>.ron`), under `<workspace>/sessions/<name>/`. The log
holds every committed batch and some history beside it, which folds to
nothing because only edits change the document: score changes as host
state, and tool records from the generation log, which today records only
`gen_sync` and `gen_check_drift` calls (author `llm`). See
`docs/rfcs/multiplayer/session-package-format.md`.

`--resume`, `--replay` and `--export-session` are in the user docs. What
they leave out:

- **Undo** is the browser page's `/undo`. The authority applies the inverse
  of that peer's newest batch as a new commit, so undoing an undo redoes
  it. It keeps a peer's last 64 batches and drops them when the peer
  leaves. A native joiner's `/undo` goes out as chat.
- **Resume** (`--resume`, which requires `--web`) only chooses what is
  replayed before guests join. The room always appends to the log of its
  own session name (`--session-name`, default `<user>'s world`), resumed or
  not.
- **`<name>`** in the path is the session name in lowercase, with every run
  of other characters a single `-`, cut at 48 characters.
- **Snapshots** are written every 500 revisions.

## Internet guests (`--relay`)

A host behind NAT reaches guests through a `localgpt-relay` (`crates/relay`)
run anywhere with a public address; the commands are in the user docs. The
host connects out: it registers a room (`POST /rooms`), holds one WebSocket
to it, and prints an invite link with the room's code,
`<relay>/r/<code>/#t=<token>`.

The relay forwards frames by room code and keeps no world state, but it
encrypts nothing. The room's messages pass through it in the clear (a
guest's hello with the invite token, every op, chat line and prompt), and it
serves the join page itself, so whoever runs a relay can read its rooms and
could serve a page of its own. A TLS terminator in front of it protects the
hops to the relay, not the room from it: run your own. The host still checks
the invite token. Local LAN guests and relay guests share the same room.

## Trust model

- Rooms are guarded by **bearer secrets**: the PIN (native joiners) and the
  invite token (browser links) are checked at hello; `--open` skips both.
  Every guest presents a secret they all share and picks their own name, and
  the WebSocket is plaintext, on a LAN and through a relay alike. On a
  network you don't trust, treat the room as public.
- Through a relay, a hello is let in by the invite token alone: the PIN is
  not taken on a relayed connection, which anyone with the room's code can
  open, and a refused hello ends the connection. The room still crosses the
  relay in the clear, so relay only a world you would show anyone.
- A key per person and end-to-end encryption, for rooms in the desktop app
  and through the relay, are designed in
  `docs/rfcs/authoring/rooms-and-the-channel.md`.
- Browser sockets must come from the host's own page (Origin check) and
  guests can't edit the document directly unless they're editors. Nothing a
  guest sends runs a model or a tool on the host: a prompt is refused, and
  only ops change the world.
- The authority validates every op from anyone: roles, schema, limits,
  revision. A full room (16 guests) declines joins.

## Limitations

- glTF mesh assets do not travel. A browser guest sees a placeholder; a
  native guest's entity stays empty unless the file is at the same path on
  its machine.
- The projection reads the registered entities, the background and the
  ambient light. The host's ambience layers, fog and camera are not
  projected, so they do not reach guests.
- Guests sync the whole document (no per-chunk interest management yet) —
  fine at room scale.
- Not in the world format (so neither saved nor synced): terrain, water,
  foliage, sky, in-world UI (signs/HUD/labels), NPC bodies.
- The PIN is six digits. After five wrong ones the session stops taking it
  (the invite link still works; a new session gets a new PIN), so it can't
  be guessed, but anyone on the network reads it as a guest sends it. Don't
  host sensitive worlds on untrusted networks or through a relay.
- A guest's *own* agent building through the room — rather than through the
  host's window — is a follow-up the protocol is shaped for: the prompt and
  job messages are kept for it, and the room refuses them until then.

## Feature flag

`multiplayer` (default on): `--host`/`--join`, the ops room, mDNS discovery,
and the browser guest page. It is not an off switch today: `main.rs` and
`gen3d/{replay,generation_log,live}.rs` use `localgpt-world-sync` and
`net::web` outside the gate, and both come only with the feature.
