---
title: "CLI Commands"
---

# CLI Commands

Two binaries have a command line: `localgpt`, a `.world` package's, and `localgpt-gen`, the world builder's. Neither takes a model, a key or an agent's settings. The `.world` package is the [Open World Format](https://openworldformat.org)'s: the folder's layout, the log and the fold that `verify` checks are the format's rules, so a package made here opens in any viewer of the format.

## `localgpt`

```bash
localgpt <COMMAND>

Commands:
  world       Work on a .world package: commit batches, read the log, verify, serve its tools over MCP
  gen         Drive Gen's 3D world builder (`localgpt-app gen` / `localgpt-gen`): MCP server, live canvas, sessions
  previs      Stage a screenplay (.fountain, .fdx, .pdf, or .md) as per-scene .world packages, shotlist.csv, and board.html
  audio       The optional audio models: list, install and remove them, run them, serve them to an agent
  completion  Generate shell completion scripts
  help        Print help information

Options:
  -v, --verbose   Enable verbose logging (to stderr; stdout is the transport while `world mcp` serves)
```

## `localgpt world` {#world}

A `.world` package is a folder — `manifest.json` (the world now), `ops.jsonl` (every change), `assets/` — and `localgpt world` is its command line, whoever holds it. While an app has the world open, every command is a call to that app's localhost API: the app commits, and what only a window knows — what's on screen, the selection, a screenshot, a batch waiting for review — comes from it. With nothing open, the commands are the authority themselves, the same one the [live canvas](/docs/gen/live-canvas/) uses, for scripts and CI. One authority at a time, either way, and a commit prints the same receipt whichever made it.

This is how an agent in a terminal changes a world: no token or JSON quoting on its command line, short lines unless `--json`, and one command prefix to allow.

```bash
localgpt world init <dir> [--git]             # open a folder as a package, creating one if needed; --git: every batch a commit
localgpt world submit [<dir>] [<batch> | -]   # commit a batch of edit ops — whole, or not at all; stdin by default
    --author <name> --message <why>           #   attribution ($LOCALGPT_WORLD_AUTHOR; "cli" by default) and the commit message
    --at <entry-id>                           #   build on an earlier entry: a branch; the head stays where it is
    --expected-revision <n>                   #   refused if the world moved since you read it
localgpt world undo [<dir>] [--own] [--at <id>]   # take back the newest edit by appending its inverse
localgpt world redo [<dir>] [--own] [--at <id>]   # take the newest undo back
localgpt world log [<dir>] [--last <n>]       # every committed batch with ids, the head marked
localgpt world tips [<dir>]                   # every branch end, the head marked
localgpt world show [<dir>] [--at <entry>]    # the world in a line per entity; --at current, <id> or base
localgpt world get [<dir>] <entity>…          # entities by name or id, as JSON: what a patch changes
localgpt world verify [<dir>]                 # the fold of the log still equals the head; assets match their hashes
localgpt world lint [<dir>] [--at <entry>] [--strict]   # what each camera sees, in numbers, and what is probably a mistake
localgpt world find [<dir>] <phrase…> [--camera <entity>]   # which entities "the red chair on the left" means
localgpt world link [<dir>] [--at <entry>]    # the world as a link that opens in a browser, nothing uploaded
localgpt world screenshot [<dir>]             # the open app's view now, as a PNG path
localgpt world selection [<dir>]              # what the person has selected in the open app
localgpt world goto [<dir>] <entry>           # show an entry in the open app's window; nothing is written
localgpt world proposals [<dir>]              # with review on, the batches waiting for the person
localgpt world history [<dir>] [<rev>]        # the git history, when the world is in git
localgpt world mcp [<dir>]                    # serve the open canvas's tools over stdio (submit, undo, log, tips, goto, verify, lint, find, link, screenshot, selection)
```

The folder is the first argument when it's given; else `$LOCALGPT_WORLD_DIR`, which the desktop app sets in its terminals; else the folder you're in, or the nearest one above it that is a world. A batch is JSON: `[op, …]` or `{"ops": [op, …]}`, inline, a file, or stdin, with the op kinds `SpawnEntity`, `ModifyEntity`, `DeleteEntity`, `SetEnvironment`, `SetCamera`, `ModifyWorld` (the world's scene-wide fields: its song, tours, environment) and `Batch`. Names bind wherever the format marks a reference and commit as ids; a refusal names the op and the reason, on stderr with exit status 1. The folder's `AGENTS.md` describes the same rules for an agent.

```bash
localgpt world submit -m "a lamp by the crate" <<'EOF'
[{"SpawnEntity": {"entity": {"name": "lamp", "transform": {"position": [2, 0.5, 1]}}}}]
EOF
claude mcp add world -- localgpt world mcp          # or: Claude Code edits the open world over MCP
```

A refusal says where: `op 0: /ModifyEntity/patch/material/color: invalid type: string "#ff0000", expected an array of length 4`, or `… /patch/materail is not a field of the format`. Every refused batch is also kept as it was sent, with its reasons, in the folder's `.live/rejected.jsonl`. A receipt's warnings are what did not stop the batch and is worth knowing; one that ends `(carried-not-run)` says the world kept something this app will not run — a trigger's host action it does not have, an extension nothing here reads, a misspelt `ext-…` key among them.

### `world lint`: what the cameras see {#lint}

`verify` asks whether a package agrees with itself. `lint` asks whether the world is the one its author meant. It is arithmetic over the document — the shapes' boxes, the transforms, the cinematography extension's own projection — so it needs no window, no renderer and no app, and it is quick enough to run after every batch:

```bash
$ localgpt world lint scene-1.world
1A    WS      24 mm  sees KAI 5% · MAYA 5% · floor 24% · wall-left 6% (left) · +2 more
1B    MCU     50 mm  on MAYA.head 14% (centre) · KAI 22% (right) · MAYA 30% · wall-left 100% (centre)
1C    MCU     50 mm  on KAI.head 14% (centre) · MAYA 22% (left) · KAI 30% · wall-right 100% (centre)
ok: nothing to flag in 14 entities
```

That is a drafted scene. Put 1B straight behind KAI and MAYA at the height of her feet, and it says so:

```
1B    MCU     50 mm  on MAYA.head 13% (centre) · KAI 100% (centre) · MAYA 13% (centre) · wall-left 100% (centre)
warning[subject-hidden]: KAI is between 1B and MAYA.head, 0.64 m from the lens
    move 1B or KAI until the line from the lens to MAYA.head is clear
note[sunk]: MAYA is 0.85 m into floor
    a position is the shape's centre: y 0.85 stands it on floor
1 warning, 1 note in 14 entities
```

A line per camera says what the camera is on, when it aims at an entity, then what is in its frame from the nearest back, with the share of the frame each covers — an entity and its parts (`MAYA`, `MAYA.head`, `MAYA.face`) as one figure. `--json` has every entity with its share, its height in the frame, its rectangle and its distance. Then a line per finding, with what would change it under it. A share is the projected box's, not a silhouette's, and nothing is hidden behind anything: it is what stands in the frame's pyramid, which is why a wall behind everything reads 100%. A mesh asset, whose size the document does not state, is counted and not measured. `--strict` exits 1 when anything is a warning; notes never fail.

| Code | | What it means |
|---|---|---|
| `camera-inside` | warning | a camera, or the view a visitor opens on, stands inside a solid shape |
| `empty-frame` | warning | a camera, or the opening view, has no shape in its frame |
| `subject-hidden` | warning | something solid stands between a camera and the entity it aims at or focuses on |
| `focus-out-of-frame` | warning | a camera focuses on an entity that is outside its frame |
| `dangling-target` | warning | a camera's `aim_entity` or `focus_entity` is not in the world, so the fallback applies |
| `line-crossed` | warning | a shot and its reverse are on opposite sides of the line between their subjects |
| `shot-timing` | warning | a shot ends at or before the second it starts |
| `duplicate` | warning | two entities are the same shape in the same place |
| `tiny-world` | warning | the whole world is smaller than 5 cm: probably not authored in metres |
| `sunk` | note | a shape's centre is at the top of the ground under it, so half of it is below: a position is a shape's centre |
| `carried-not-run` | note | content the format carries and this app does not run: it is kept, and nothing happens |
| `nothing-to-draw` | note | no entity has a shape or a mesh |

A code is never renamed. The cinematography rules read the same fields the [previs](/docs/gen/previs/) draft writes, and a draft passes them.

### `world find`: which one {#find}

A person looking at a world says "the actor on the left"; `find` resolves the phrase to entities by rule: a whole name or a word of one (singular or plural), shape and colour words (a stand-in capsule is an actor), left, right, front and back as a view has them, height and size, a count, an ordinal, lists and "except".

```bash
$ localgpt world find the actor on the left --camera 1A
#6 MAYA
sides and distance as seen from shot 1A
$ localgpt world find the wall
Which one: wall-back, wall-left or wall-right?
$ localgpt world find the thing behind KAI
not understood: behind — settle it yourself (`localgpt world show` has every entity's place)
```

Left and right are the person's while the app has the world open; with it closed, the world's opening view; `--camera 1A` reads them through a shot instead. When a phrase could mean several it asks which, and a phrase it only half understands — a relation like "behind", a pointer like "this", a word it has no rule for where the noun should be — picks nothing and says which word. Both exit 1: a guess would be an edit to the wrong thing. `selection` is what the person clicked; this is what they said.

### `world link`: a world in a link {#link}

```bash
$ localgpt world link scene-1.world
https://localgpt.world/world.html#world=z:1ZjvbuI4EMBfBfkziUggIfCt1zvtVXtdndSeTq…
```

The whole manifest rides in the link's fragment — compact JSON, raw DEFLATE, base64url — and a browser never sends a fragment to a server, so opening the link uploads nothing and stores nothing: the page reads the world out of its own address. A previs scene is about 1,600 characters. A world that needs files (meshes, textures, a recording) is refused, naming them: publish its package instead. `--viewer <url>` writes the link for another copy of the viewer page.

## `localgpt previs` {#previs}

Stages a screenplay — Fountain (`.fountain`), Final Draft (`.fdx`), PDF (`.pdf`), or Markdown script pages (`.md`) — deterministically, with no model and no network, using the same function [localgpt.md](https://localgpt.md) runs in the browser:

```bash
localgpt previs <script.fountain | script.fdx | script.pdf | pages.md> --out <dir>
# <dir>/scene-1.world/  <dir>/scene-2.world/  <dir>/shotlist.csv  <dir>/board.html
```

See [Previs](/docs/gen/previs/).

## `localgpt audio` {#audio}

The optional [audio models](/docs/audio-models/): nothing is installed until you install it, and an agent gets an installed model's tool in `localgpt world mcp`.

```bash
localgpt audio list                       # each model, its download and licence, installed or not
localgpt audio install <id>... | --all    # stems, notes, tags, match; each file checked against its pin
localgpt audio remove <id>...
localgpt audio mcp                        # the installed models' tools alone, over stdio

localgpt audio stems <file> [--out DIR]   # drums, bass, vocals and other WAVs
localgpt audio notes <file> [--json]      # the notes played (--start, --end, thresholds)
localgpt audio tags <file> [--json]       # AudioSet labels, overall and every 10 s
localgpt audio match <file> <description>...
```

`--no-default-features` builds `localgpt` without it, and without ONNX Runtime.

## `localgpt completion`

```bash
localgpt completion bash > /etc/bash_completion.d/localgpt
localgpt completion zsh > "${fpath[1]}/_localgpt"
localgpt completion fish > ~/.config/fish/completions/localgpt.fish
```

## `localgpt gen` / `localgpt-gen`

The world builder's window, driven from outside. `localgpt gen …` forwards to `localgpt-app gen …` when the desktop app is installed, or falls back to `localgpt-gen` (`localgpt-app mcp-server` also works directly). No prompt is typed into Gen itself.

```bash
localgpt gen                               # the window alone (nothing drives it: pass one of the flags below)
localgpt gen --mcp-relay                   # serve the MCP relay beside the window; an MCP client reaches it through `mcp-server --connect`
localgpt gen mcp-server                    # the window as an MCP client's own process, tools over stdio
localgpt gen mcp-server --mcp-http 8766    # and the streamable HTTP transport at http://127.0.0.1:8766/mcp
localgpt gen mcp-server --headless         # no window: CI and batch work over MCP
localgpt gen mcp-server --connect [PORT]   # the stdio end of a running window's relay (port auto-discovered)
localgpt gen --live scene-1.world [--review]   # a .world package as a live canvas; --review holds each batch for Enter or Esc
localgpt gen --world <PATH|URL> | --scene <glb> # open a world, or a glTF scene, at start
localgpt gen --replay <session|dir|ops.jsonl|.world> [--replay-speed 4]   # a time-lapse of a history
localgpt gen --export-session <name|dir> [--out x.world]                  # zip a session package
localgpt gen --host [--session-name n] [--port 9879] [--open] [--web] [--web-edit] [--relay URL] [--resume x]
localgpt gen --join [host[:port]] [--pin PIN]   # join a session as a viewer; lines typed are chat
```

Invoking `localgpt-gen …` directly takes the exact same flags and subcommands.

See [MCP Server](/docs/gen/mcp-server/), [MCP Relay](/docs/gen/cli-mode/), [Live canvas](/docs/gen/live-canvas/) and [Collaborative Sessions](/docs/gen/multiplayer/).

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | An error, named on stderr |
