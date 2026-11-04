# Gen Multiplayer — One Protocol, Every Client

Gen's collaboration is built around one idea: **a world is a shared
document, and every change to it is an op.** The host runs the room
authority (`localgpt-world-sync`); native and browser clients connect over
one WebSocket and render the same document. See
`docs/rfcs/multiplayer/collaborative-world-engine-architecture.md` for the
full design.

## Quick start

```bash
# On the host machine:
localgpt-gen --host                        # session named "<user>'s world"; prints a 6-digit PIN
localgpt-gen --host --session-name "Castle Build" --port 9879
localgpt-gen --host --open                 # no PIN — trusted networks only
localgpt-gen --host --web                  # also print a browser invite link (token in the URL)
localgpt-gen --host --web --web-edit       # browser guests may edit directly, not just watch and chat
localgpt-gen --host --web --resume "Castle Build"   # restore a previous session from its op log
localgpt-gen --host --mcp-relay            # and let your agent build in the hosted world over MCP

# Join from another machine (you'll be asked for the PIN if it has one):
localgpt-gen --join                        # browse mDNS, join the first session found
localgpt-gen --join 192.168.1.5            # bare host, default port 9879
localgpt-gen --join 192.168.1.5:9879 --pin 482913

# Browser guests (nothing to install):
#   send the invite link the host prints — http://<host>:<port>/#t=<token>

# History:
localgpt-gen --replay ~/.local/share/localgpt/gen-workspace/sessions/<name>/ops.jsonl
```

- **Host:** the normal interactive gen window plus a session HTTP server on
  the session port (join page + WebSocket ops endpoint) and an mDNS
  announcement (`_localgpt-world._udp.local.`). The console shows the PIN.
- **Native client (`--join`):** the full gen scene — camera, behaviors,
  audio — driven by the room instead of tools. You see every guest as a
  labeled capsule, and anything typed at the REPL is said to the room as
  chat. `/stats` shows the room; `/quit` exits.
- **Browser guests:** the world-export three.js viewer, live over the same
  protocol — see [Browser guests](#browser-guests---web).
- **Building:** no model runs for the room. The host's own agent builds —
  outside the window, over MCP (`--mcp-relay` beside `--host`) or through a
  live package's API — and its edits reach every guest as the host's ops.
- **Not from the desktop app:** its Collaborate section went with the
  prompt panel. Rooms in the app, over the `.world` it has open, are
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
- **Presence** (positions, gaze, selections) rides a lossy channel at ~5 Hz
  and is never stored.

## Browser guests (`--web`)

The session HTTP server also serves a **join page** at `http://<host>:<port>/`
and prints an invite link. Guests can watch, walk, chat, and — with
`--web-edit` — edit directly: click to select, drag to move,
`Q`/`E` rotate, `+`/`-` scale, `Delete` remove, `/undo` to step back through
their own changes. Everything is served by the host (three.js is vendored):
a guest needs no install and no internet.

## History, undo & replay

Every session is a package: a base world (`world.ron`), the one log
(`ops.jsonl`), metadata (`session.json`) and keyframe snapshots
(`snapshots/rev-<N>.ron`), under `<workspace>/sessions/<name>/`. The log
holds every committed batch and the history around it — the tool calls
the host's agent made, host state such as score changes — because only
edits change the document; everything else folds to nothing. See
`docs/rfcs/multiplayer/session-package-format.md`.

- **Undo:** `/undo` undoes your most recent batch — your own edits.
  Undoing an undo redoes it.
- **Resume:** `--host --web --resume <session>` replays a session's log
  before guests join, restoring the world and its revision. `<session>`
  is a session name, a package directory, or a bare `ops.jsonl`.
- **Time-lapse:** `localgpt-gen --replay <session> [--replay-speed N]`
  opens a no-agent window that rebuilds the world batch by batch; it takes
  the same paths, and a package replays its base world first.
- **Transport:** `localgpt-gen --export-session <name> [--out x.world]`
  zips a session's package into the one-file form other software reads
  (the [Open World Format](https://openworldformat.org)); `--resume` and
  `--replay` accept those `.world` archives back.

## Internet guests (`--relay`)

The host runs behind NAT? Run a relay anywhere with a public address:

```bash
localgpt-relay 9911        # put it behind any TLS terminator for wss
```

and host with it:

```bash
localgpt-gen --host --relay https://relay.example.com
# prints: Internet guests: https://relay.example.com/r/Q8QJVD/#t=<token>
```

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

- glTF mesh assets render as placeholders for guests (content-addressed
  streaming returns in a later phase). Everything else in the world format
  syncs.
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
and the browser guest page. Build without it for a single-player binary.
