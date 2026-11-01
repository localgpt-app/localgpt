---
slug: "/architecture"
title: "Architecture"
description: "The LocalGPT workspace: the world apps, the world-format crates they share, the dependency graph, the binaries, and the rules that keep the format single and the agents outside."
---

# Architecture

LocalGPT is one Cargo workspace: the world apps (the desktop app, Gen, MD, Verse, previs), the world-format crates they all share, and the CLI. MD and Verse used to be repositories of their own and were folded in so the format could not drift between apps; the format itself then moved out to the [Open World Format](https://openworldformat.org), whose Rust reference this workspace consumes as the `openworldformat` crate.

Nothing in the workspace runs a model as an agent, or holds a key: the coding agents that build worlds run outside, in terminals and MCP clients, and reach a world through its API.

## Workspace Structure

```
crates/
├── cli/          # localgpt — `localgpt world`, a .world package's command line, and `localgpt world mcp`, the shim
├── gen/          # localgpt-gen — the world builder (Bevy): its tools over MCP, the relay, the live canvas, sessions, headless
├── app/          # localgpt-app — the one desktop app: Gen's viewport, a web page over it, terminals for the agents
├── md/           # localgpt-md — a Markdown file as a walkable world (pure lib + Bevy bin)
├── verse/        # localgpt-verse — a 3D world for every song (lib + Bevy bin)
├── previs/       # localgpt-previs — screenplay → per-scene .world with set/cast/coverage, a board, a shot list
├── previs-web/   # localgpt-previs-web — previs's draft as WASM, for the localgpt.md page
├── world-types/  # localgpt-world-types — the world data model (serde only; the format's types re-exported)
├── world-bevy/   # localgpt-world-bevy — the one Bevy mapping of the format (Gen, MD, Verse, the app)
├── world-audio/  # localgpt-world-audio — the one mapping of the format's audio (kira + FunDSP)
├── world-export/ # localgpt-world-export — the three.js viewer and HTML export, no Bevy
├── world-sync/   # localgpt-world-sync — collaborative rooms: shared document, ops, wire protocol, authority
├── world-editor/ # localgpt-world-editor — the non-linear editing model: tips, forks, folds, deltas
├── world-agent/  # localgpt-world-agent — the live authority, packages, the headless commands, the MCP server, the tool-call protocol
├── relay/        # localgpt-relay — internet reach for collaborative rooms
└── spacetime/    # localgpt-spacetime — SpacetimeDB multiplayer module (standalone; wasm toolchain)

apps/
└── app-desktop/  # the LocalGPT.app bundler (Developer ID + notarization when given)

website*/         # one Cloudflare Worker each: localgpt.app (this site), gen., verse., localgpt.md, localgpt.world
scripts/          # fetch-model.sh and fetch-assets.sh — the two downloads every app shares
```

## Dependency Graph

```
                     ┌──────────────────┐
                     │   world-types    │  (serde only; the openworldformat crate re-exported)
                     └────────┬─────────┘
          ┌──────────┬────────┼──────────┬──────────────┐
          ▼          ▼        ▼          ▼              ▼
     world-bevy  world-audio  world-export  world-sync  world-editor
     (Bevy)      (kira)       (three.js)       │            │
          │          │                         ▼            │
          │          │                    world-agent ◄─────┘
          │          │               (the authority, the MCP server)
          │          │                    │        │
          ▼          ▼                    ▼        ▼
         gen ───────────────────────► app        cli (localgpt)
          ▲                            ▲
     previs (deterministic)       md, verse (as libraries)
```

`previs-web` is previs as WASM; `relay` and `spacetime` stand alone.

## Crate Summary

| Crate | Type | Purpose |
|-------|------|---------|
| `localgpt` | bin | `localgpt world` and `localgpt world mcp` |
| `localgpt-gen` | lib+bin | The world builder: Bevy viewport, 95 tools served over MCP (stdio, the relay, HTTP), headless, sessions, the live canvas |
| `localgpt-app` | bin | The one desktop app, assembled from the gen library with MD and Verse as libraries; a web page over the viewport with terminals in it (not published) |
| `localgpt-md` | lib+bin | Markdown → sections → a world; the pure library is what the app compiles documents with |
| `localgpt-verse` | lib+bin | Song analysis → mood → a furnished, performing world; `song_world` is what the app calls |
| `localgpt-previs` | lib+bin | Screenplay → per-scene `.world`, board and shot list; deterministic, no model, no network |
| `localgpt-previs-web` | lib | The same draft as WASM for localgpt.md (not published) |
| `localgpt-world-types` | lib | The data model: the format's types re-exported, plus NPCs, behavior libraries, regions, sync drift. `schema` writes `world.schema.json` |
| `localgpt-world-bevy` | lib | The only place a shape, material, light or environment becomes Bevy components |
| `localgpt-world-audio` | lib | One kira `AudioManager`; FunDSP graphs as sources; ambience, spatial emitters, soundtracks |
| `localgpt-world-export` | lib | The three.js viewer (vendored from the format's npm package) and `generate_html`; no Bevy |
| `localgpt-world-sync` | lib | Shared world document, validated `EditOp`s, the wire protocol, the room authority; no Bevy, no I/O |
| `localgpt-world-editor` | lib | History as a tree: a branch index, a fold cache fast enough to scrub, deltas between tips |
| `localgpt-world-agent` | lib | `LiveWorld` (the live authority), packages, the headless commands behind `localgpt world`, the `Tool` trait and MCP server (`mcp`), the tool-call protocol, the shared directories |
| `localgpt-relay` | bin | Forwards WebSocket frames between a host and its guests by room code; never holds world state (not published) |
| `localgpt-spacetime` | module | SpacetimeDB rooms: the same ops and validation as a LAN host, in the cloud (standalone workspace) |

## The world format is shared

Every app reads and writes one format. `localgpt-world-types` is the source: the [Open World Format](https://openworldformat.org)'s document, session and state types come from the `openworldformat` crate and are re-exported unchanged, and the crate adds LocalGPT's own surface on top. `localgpt-world-bevy` is the only place those types become Bevy components, so Gen, MD, Verse and the app render identically; `localgpt-world-audio` is the one mapping of the format's audio; `localgpt-world-export` holds the only web renderer, the same three.js viewer [localgpt.world](https://localgpt.world) serves and every exported HTML page embeds.

Reference scenes live in `crates/world-types/conformance/` and are rendered by both renderers; the web viewer's headless render check runs in CI. The rule inside the workspace: **add fields to `world-types`, never a parallel format** — proposals that change what the format *means* belong in the format's own repository.

A `.world` package is a folder: `manifest.json` (the head), `ops.jsonl` (the log), `assets/` (content-addressed), `AGENTS.md`. `LiveWorld` in `world-agent` is the one authority that commits to it — from the window ([live canvas](/docs/gen/live-canvas/)) or from `localgpt world` — and `world-editor` is the view that follows: tips, forks, folds and deltas, which the history panel draws.

## The agents are outside

An agent never runs inside a LocalGPT process. Three doors lead in, all to the same authority:

| Door | What it is | Who uses it |
|---|---|---|
| The folder | `AGENTS.md` and `.live/endpoint.json` in a live package; the app's terminals open there with `$LOCALGPT_WORLD_DIR` set | a coding agent in a terminal, with `localgpt world` |
| The shim | `localgpt world mcp`: eight tools over stdio against the open world's API | any MCP client |
| Gen's tools | `localgpt-gen mcp-server` (the agent's own window) or `--mcp-relay` with `mcp-server --connect` (the window you are looking at): the 95 `gen_*` tools | MCP clients and editors, until they move behind the shim |

The `Tool` trait and the MCP server they share live in `world-agent::mcp`. Every change lands as one log entry, attributed to whoever sent it.

## Binaries

### `localgpt` (CLI)

| Command | Purpose |
|---------|---------|
| `world` | a `.world` package's command line: `init`, `submit`, `undo`, `redo`, `log`, `tips`, `show`, `get`, `verify`, `screenshot`, `selection`, `goto`, `proposals`, `history`, and `mcp` — the open canvas's tools over stdio |
| `completion` | shell completion scripts |

### `localgpt-gen`

The world builder: a Bevy window and 95 `gen_*` tools; `mcp-server` (stdio, `--mcp-http`, `--headless`, `--connect` to a running window's relay); `--mcp-relay`; `--host`/`--join`/`--web`/`--relay` collaborative sessions over `world-sync`; `--live` for the live canvas; `--replay`. See [LocalGPT Gen](/docs/gen/).

### `localgpt-app`

[The desktop app](/docs/app/): Gen's viewport plus `--md`, `--song` and `--world`, one web page over it with the history, the outliner and inspector, the document, the timeline and the terminals. Thin by design — everything heavy comes from the gen library, MD's pure library compiles documents, and Verse's library builds song worlds.

### `localgpt-previs`

Script to board, deterministic: Fountain, Final Draft `.fdx`, PDF or Markdown pages in; per scene a `.world` with set, cast and coverage, plus `board.html` and `shotlist.csv`. The same `draft` function runs as WASM on [localgpt.md](https://localgpt.md). See [Previs](/docs/gen/previs/).

### `localgpt-relay`

Internet reach for collaborative rooms: forwards frames between a NAT'd host and its guests by room code.

## Design Principles

1. **One format, one mapping each** — fields go into `world-types`; `world-bevy`, `world-audio` and `world-export` are the only places the format becomes pixels or sound.
2. **One authority per world** — `LiveWorld` commits; the canvas, the editor model and `localgpt world` follow. A direct write to `manifest.json` is put back.
3. **The agents are outside** — no process here runs a model or holds a key, and no agent is known by name: a terminal is a login shell, a client is an MCP client, and the history names whoever commits.
4. **Feature flags for the heavy tiers** — MD's and Verse's local models, the ML mood model, physics and multiplayer are opt-in, and each has a CI job so gated code cannot rot.
5. **Graceful degradation** — a missing asset pack leaves primitives; a missing model leaves the rule-derived draft on screen.
