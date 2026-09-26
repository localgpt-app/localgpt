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
localgpt-gen --host --web --web-edit       # browser guests may edit directly, not just prompt
localgpt-gen --host --web --resume "Castle Build"   # restore a previous session from its op log
localgpt-gen --host --remote-tools full    # let guests' prompts use ALL host tools (shell!)

# Join from another machine (you'll be asked for the PIN if it has one):
localgpt-gen --join                        # browse mDNS, join the first session found
localgpt-gen --join 192.168.1.5            # bare host, default port 9879
localgpt-gen --join 192.168.1.5:9879 --pin 482913

# Browser guests (nothing to install):
#   send the invite link the host prints — http://<host>:<port>/#t=<token>

# History:
localgpt-gen --replay ~/.local/share/localgpt/workspace/sessions/<name>/ops.jsonl
```

- **Host:** the normal interactive gen window plus a session HTTP server on
  the session port (join page + WebSocket ops endpoint) and an mDNS
  announcement (`_localgpt-world._udp.local.`). The console shows the PIN.
- **Native client (`--join`):** the full gen scene — camera, behaviors,
  audio — driven by the room instead of tools. You see every guest as a
  labeled capsule, chat, and anything typed at the REPL goes to the room's
  build queue (scaffolds mark running builds). `/stats` shows the room;
  `/quit` exits.
- **Browser guests:** the world-export three.js viewer, live over the same
  protocol — see [Browser guests](#browser-guests---web).
- **From the window:** the prompt panel's Collaborate section starts hosting
  or joins without flags.

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
- **Prompts from any client** enter one FIFO queue (scaffolds mark running
  builds), run on a scene-only agent unless `--remote-tools full`, and are
  attributed to the asker — so `/undo` undoes *your* builds.
- **Presence** (positions, gaze, selections) rides a lossy channel at ~5 Hz
  and is never stored.

## Browser guests (`--web`)

The session HTTP server also serves a **join page** at `http://<host>:<port>/`
and prints an invite link. Guests can watch, walk, chat, prompt the room's
AI, and — with `--web-edit` — edit directly: click to select, drag to move,
`Q`/`E` rotate, `+`/`-` scale, `Delete` remove, `/undo` to step back through
their own changes. Everything is served by the host (three.js is vendored):
a guest needs no install and no internet.

## History, undo & replay

Every session appends committed batches to
`<workspace>/sessions/<name>/ops.jsonl`.

- **Undo:** `/undo` undoes your most recent batch — your own edits and
  builds the room's AI made for you (prompt builds are attributed to the
  asker). Undoing an undo redoes it.
- **Resume:** `--host --web --resume <session>` replays a session's log
  before guests join, restoring the world and its revision.
- **Time-lapse:** `localgpt-gen --replay <ops.jsonl> [--replay-speed N]`
  opens a no-agent window that rebuilds the world batch by batch.

## Trust model

- Rooms are **LAN rooms with bearer secrets**: the PIN (native joiners) and
  the invite token (browser links) are checked at hello; `--open` skips both.
  On a hostile network, treat the room as public — secrets ride plaintext
  WebSocket. The internet relay (with TLS) is phase 4.
- Browser sockets must come from the host's own page (Origin check) and
  guests can't edit the document directly unless they're editors — their
  prompts run on a scene-only agent (no shell, files, memory, or web).
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
- The PIN is six digits with no attempt limit; don't host sensitive worlds
  on untrusted networks.
- A guest running its *own* model against the room (BYO-agent) is a
  follow-up the protocol is shaped for.

## Feature flag

`multiplayer` (default on): `--host`/`--join`, the ops room, mDNS discovery,
and the browser guest page. Build without it for a single-player binary.
