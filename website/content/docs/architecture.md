---
slug: "/architecture"
title: "Architecture"
description: "The LocalGPT workspace: the assistant's crates, the world apps, the world-format crates they share, the dependency graph, the binaries, and the rules that keep core portable and the format single."
---

# Architecture

LocalGPT is one Cargo workspace: the assistant, the world apps (the desktop app, Gen, MD, Verse, previs) and the world-format crates they all share. MD and Verse used to be repositories of their own and were folded in so the format could not drift between apps; the format itself then moved out to the [Open World Format](https://openworldformat.org), whose Rust reference this workspace consumes as the `openworldformat` crate.

## Workspace Structure

```
crates/
├── core/         # localgpt-core — the shared library: agent, memory, config, security, MCP, cron, hooks
├── cli/          # localgpt — the main binary: clap CLI, TUI, desktop GUI, daemon, `localgpt world`
├── cli-tools/    # localgpt-cli-tools — the dangerous tools: bash, read/write/edit_file, browser
├── server/       # localgpt-server — HTTP/WS API, OpenAI-compatible API, TLS, BridgeManager
├── sandbox/      # localgpt-sandbox — Landlock/Seatbelt kernel-enforced shell isolation
├── mobile-ffi/   # localgpt-mobile-ffi — UniFFI bindings for iOS/Android
├── gen/          # localgpt-gen — the world builder (Bevy): interactive, MCP server, headless, sessions, live canvas
├── app/          # localgpt-app — the one desktop app: Gen's shell with documents, songs and packages as modes
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
├── world-agent/  # localgpt-world-agent — the live authority, packages, headless commands, the tool-call protocol
├── relay/        # localgpt-relay — internet reach for collaborative rooms
├── bridge/       # localgpt-bridge — secure IPC protocol for bridge daemons
└── spacetime/    # localgpt-spacetime — SpacetimeDB multiplayer module (standalone; wasm toolchain)

bridges/
└── cli/          # localgpt-bridge-cli — a terminal that connects to a running daemon over the IPC socket

apps/
├── apple/        # iOS/macOS (Swift, UniFFI)
├── android/      # Android (Kotlin, UniFFI)
├── app-desktop/  # the LocalGPT.app bundler (Developer ID + notarization when given)
└── web/          # web client

website*/         # one Cloudflare Worker each: localgpt.app (this site), gen., verse., localgpt.md, localgpt.world
scripts/          # fetch-model.sh and fetch-assets.sh — the two downloads every app shares
```

## Dependency Graph

```
                        ┌─────────────────┐
                        │ localgpt-core   │  (no internal deps; compiles for iOS and Android)
                        └────────┬────────┘
         ┌───────────────┬───────┼────────┬──────────────┐
         ▼               ▼       ▼        ▼              ▼
┌──────────────┐ ┌────────────┐ ┌──────┐ ┌────────┐ ┌──────────────┐
│ localgpt-    │ │ localgpt-  │ │ gen  │ │ mobile │ │ world-types  │
│ bridge       │ │ sandbox    │ │      │ │ -ffi   │ │ (serde only) │
│ (no internal)│ └─────┬──────┘ └──────┘ └────────┘ └──────┬───────┘
└──────┬───────┘       │                                   │
       │               ▼                 ┌─────────────────┼──────────────┬───────────┐
       │     ┌──────────────────┐        ▼                 ▼              ▼           ▼
       │     │ localgpt-cli-    │   world-bevy        world-export    world-sync   world-agent
       │     │ tools (core+     │   world-audio       (three.js)      world-editor (the authority)
       │     │  sandbox)        │   (Bevy, kira)                      previs
       │     └────────┬─────────┘        │                 │              │           │
       ▼              │                  └────────┬────────┴──────────────┴───────────┘
┌─────────────────┐   │                           ▼
│ localgpt-server │   │                 gen ─► app (md + verse as libs)
│ (core + bridge) │   │                 md, verse, previs-web
└────────┬────────┘   │
         ▼            ▼
      ┌──────────────────────────┐
      │ localgpt (CLI)           │
      │ core + server + sandbox  │
      │ + cli-tools + world-agent│
      └──────────────────────────┘
```

Bridge daemons depend on core + bridge. Mobile depends on core with `default-features = false` plus local embeddings and sqlite-vec.

## Crate Summary

| Crate | Type | Purpose |
|-------|------|---------|
| `localgpt-core` | lib | Agent, providers, memory, config, security, MCP client and server, cron, hooks. Zero platform-specific dependencies |
| `localgpt` | bin | The assistant's CLI, TUI, desktop GUI and daemon, and `localgpt world` |
| `localgpt-cli-tools` | lib | The dangerous tools (shell, files, browser) the CLI and daemon inject; never in core |
| `localgpt-server` | lib | HTTP/WebSocket API, OpenAI-compatible surface, embedded web UI, TLS, BridgeManager |
| `localgpt-sandbox` | lib | Landlock/seccomp, Seatbelt and container sandboxing for shell commands |
| `localgpt-bridge` | lib | IPC protocol for bridge daemons |
| `localgpt-bridge-cli` | bin | Terminal chat over the daemon's bridge socket |
| `localgpt-mobile-ffi` | lib+bin | UniFFI bindings; `LocalGPTClient` owns a tokio runtime and an `AgentHandle` |
| `localgpt-gen` | lib+bin | The world builder: Bevy viewport, the agent loop, 77 tools, MCP server and relay, headless, sessions, the live canvas |
| `localgpt-app` | bin | The one desktop app, assembled from the gen library with MD and Verse as libraries (not published) |
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
| `localgpt-world-agent` | lib | `LiveWorld` (the live authority), packages, the headless commands behind `localgpt world`, the tool-call protocol, the shared download paths |
| `localgpt-relay` | bin | Forwards WebSocket frames between a host and its guests by room code; never holds world state (not published) |
| `localgpt-spacetime` | module | SpacetimeDB rooms: the same ops and validation as a LAN host, in the cloud (standalone workspace) |

## The world format is shared

Every app reads and writes one format. `localgpt-world-types` is the source: the [Open World Format](https://openworldformat.org)'s document, session and state types come from the `openworldformat` crate and are re-exported unchanged, and the crate adds LocalGPT's own surface on top. `localgpt-world-bevy` is the only place those types become Bevy components, so Gen, MD, Verse and the app render identically; `localgpt-world-audio` is the one mapping of the format's audio; `localgpt-world-export` holds the only web renderer, the same three.js viewer [localgpt.world](https://localgpt.world) serves and every exported HTML page embeds.

Reference scenes live in `crates/world-types/conformance/` and are rendered by both renderers; the web viewer's headless render check runs in CI. The rule inside the workspace: **add fields to `world-types`, never a parallel format** — proposals that change what the format *means* belong in the format's own repository.

A `.world` package is a folder: `manifest.json` (the head), `ops.jsonl` (the log), `assets/` (content-addressed), `AGENTS.md`. `LiveWorld` in `world-agent` is the one authority that commits to it — from the window ([live canvas](/docs/gen/live-canvas/)) or from `localgpt world` — and `world-editor` is the view that follows: tips, forks, folds and deltas, which the history rail and panel draw.

## Core Libraries

### `localgpt-core`

Foundation library with zero platform-specific dependencies, compiled for `aarch64-apple-ios` and `aarch64-linux-android` as well as the desktop:

- **Agent**: the `LLMProvider` trait and its implementations (Anthropic, OpenAI and OpenAI-compatible, xAI, Gemini, Vertex AI, GLM, Ollama, Claude/Gemini/Codex CLIs), model aliases, failover, compaction, checkpoints, tool filters, approval
- **Memory**: SQLite FTS5 + sqlite-vec with local embeddings, Markdown and no-op backends, active recall, dreaming (background consolidation), the wiki
- **Config**: TOML with XDG path resolution, migration and hot reload
- **Security**: `POLICY.md` injection and sanitization, protected files, SSRF guards, the hardcoded security suffix
- **Heartbeat and cron**: autonomous runs, each job in a fresh session, with a durable outbox
- **MCP**: a client for external servers (stdio, HTTP/SSE) and a server exposing memory tools
- **Session**: conversation management with automatic compaction and branching

Safe tools only (`create_safe_tools`): memory, web, documents, audio transcription, wiki, `spawn_agent`. The CLI and daemon inject the dangerous tools from `localgpt-cli-tools`; server agents and bridges never get them.

### `localgpt-bridge`

IPC protocol for daemon-to-bridge communication: tarpc-based async RPC, peer identity verification (Unix UID/GID), credential exchange, Unix sockets and Windows named pipes.

## Desktop Components

### `localgpt-server`

- **Axum HTTP**: REST API, the OpenAI-compatible `/v1/chat/completions` and `/v1/models`, the embedded web UI (RustEmbed), the tool-approval endpoint
- **WebSocket**: real-time chat streaming
- **BridgeManager**: the socket server for bridge daemons
- **TLS** and per-IP rate limiting

### `localgpt-sandbox`

| Platform | Technology |
|----------|------------|
| Linux | Landlock + seccomp |
| macOS | Seatbelt (sandbox-init) |
| Any | Docker / Podman as a backend |
| Windows | Restricted tokens |

Falls back gracefully on unsupported systems.

## Binaries

### `localgpt` (CLI)

| Command | Purpose |
|---------|---------|
| `chat`, `tui`, `ask` | conversation |
| `daemon` | HTTP server + web UI + heartbeat + cron |
| `world` | a `.world` package's command line: `init`, `submit`, `undo`, `log`, `tips`, `verify`, `history`, and `mcp` — the open canvas's tools over stdio |
| `mcp-server` | memory and web tools over stdio for other agents |
| `memory`, `config`, `policy`, `session`, `cron`, `hooks`, `tool` | the workspace and the daemon's services |
| `sandbox`, `cert`, `doctor`, `paths`, `init`, `completion` | setup and diagnostics |
| `gen` | launches `localgpt-gen` |

### `localgpt-gen`

The world builder: a Bevy window with the agent loop and 77 `gen_*` tools in three profiles; `mcp-server` (stdio, `--mcp-http`, `--connect` to a running window's relay); `headless` generation; `--host`/`--join`/`--web`/`--relay` collaborative sessions over `world-sync`; `--live` for the live canvas; `--replay`. See [LocalGPT Gen](/docs/gen/).

### `localgpt-app`

[The desktop app](/docs/app/): Gen's shell plus `--md`, `--song` and `--world`. Thin by design — everything heavy comes from the gen library, MD's pure library compiles documents, Verse's library builds song worlds, and the prompt panel runs Gen's own agent loop.

### `localgpt-previs`

Script to board, deterministic: Fountain, Final Draft `.fdx`, PDF or Markdown pages in; per scene a `.world` with set, cast and coverage, plus `board.html` and `shotlist.csv`. The same `draft` function runs as WASM on [localgpt.md](https://localgpt.md). See [Previs](/docs/gen/previs/).

### `localgpt-relay`

Internet reach for collaborative rooms: forwards frames between a NAT'd host and its guests by room code.

## System Prompt

Every agent in the family shares the base system prompt built by `build_system_prompt()` in `localgpt-core`: identity, safety guidelines, the registered tools' documentation, the workspace and the current time, memory conventions, runtime info. On a new session the agent loads its workspace context (SOUL.md, MEMORY.md, daily logs, HEARTBEAT.md) and appends it.

| | CLI | Gen and the app | Server / Mobile |
|---|---|---|---|
| **Agent creation** | `Agent::new()` — async, connects MCP servers | `Agent::new_with_tools()` — the gen toolbelt at the chosen profile | `Agent::new()` — safe tools only |
| **Tools** | Safe tools + CLI tools (shell, files) + `spawn_agent` | Safe tools + `gen_*` tools; interactive Gen adds the CLI tools at the `full` profile | Safe tools only |
| **Workspace** | the assistant's (`main` agent) | Gen's own (`gen` agent, `gen-settings.json`); no `config.toml` | the assistant's |
| **Extra prompts** | None | creative-memory guidance and the headless experiment prompt, injected as user messages | None |

The world modes keep their own settings and workspace so nothing from the assistant's memory reaches a world or a shared session unless the user puts it there.

## Mobile

`localgpt-mobile-ffi` exposes `LocalGPTClient` (an `Arc<Mutex<Agent>>` with its own tokio runtime) through UniFFI proc-macros. Build outputs: `liblocalgpt_mobile.a` + Swift bindings → an XCFramework for iOS/macOS; `liblocalgpt_mobile.so` + Kotlin bindings → an AAR for Android. Core is built with `embeddings-local` + `sqlite-vec` and none of the subprocess providers or HTTP.

## Bridge Daemon

A standalone binary that connects to the running daemon over the IPC protocol in `localgpt-bridge`; `localgpt-bridge-cli` is the terminal one (rustyline).

## Design Principles

1. **Core stays portable** — `localgpt-core` compiles for iOS and Android with no desktop dependencies; clap, eframe, axum, landlock and the like live in `cli`, `server`, `sandbox`, `cli-tools` or `gen`.
2. **Dangerous tools live in `cli-tools`** — never in core's safe set; server agents and bridges never see them.
3. **One format, one mapping each** — fields go into `world-types`; `world-bevy`, `world-audio` and `world-export` are the only places the format becomes pixels or sound.
4. **One authority per world** — `LiveWorld` commits; the canvas, the editor model and `localgpt world` follow. A direct write to `manifest.json` is put back.
5. **Feature flags for the heavy tiers** — the in-process LLM, the ML mood model, physics and multiplayer are opt-in, and each has a CI job so gated code cannot rot.
6. **Graceful degradation** — the sandbox falls back on unsupported systems; a missing model leaves the rule-derived draft on screen.
