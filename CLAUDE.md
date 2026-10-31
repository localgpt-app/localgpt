# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

`bin/lg` is the one entry point for building and releasing — it only routes
to scripts in `bin/{web,build,ops}/`, `apps/` and `scripts/`, each of which
also runs directly:

```bash
bin/lg build app gen                    # cargo build, by name: cli app gen previs md verse all
bin/lg build bundle --dmg               # LocalGPT.app for macOS
bin/lg verify                           # the CI checks on this machine (--rust --bevy --features --web --deps)
bin/lg web deploy world --dry-run       # the family's sites: app gen verse md world, or all
bin/lg fetch model|assets               # the two shared downloads
```

```bash
# Build
cargo build                     # Debug build (default-members = crates/cli)
cargo build --release           # Release build (LTO, stripped — slow)
cargo build --workspace         # Build all workspace crates

# Run
cargo run -- world init demo.world   # a .world package's command line (init, submit,
                                     # undo, log, tips, verify, history)
cargo run -- world mcp               # the shim: a stdio MCP server over an app's open canvas

# Test
cargo test --workspace          # All tests
cargo test -p localgpt-core     # Single crate
cargo test -- --nocapture       # Show stdout

# Lint (required before commits)
cargo clippy --workspace -- -D warnings
cargo fmt --check

# Gen (3D scene generation with Bevy)
cargo run -p localgpt-gen                          # Interactive mode
cargo run -p localgpt-gen -- "build a castle"      # With initial prompt
cargo run -p localgpt-gen -- -s model.glb          # Load existing scene
cargo run -p localgpt-gen -- -v                    # Verbose logging
cargo run -p localgpt-gen -- --host                # Host a collaborative session (mDNS, UDP 9879)
cargo run -p localgpt-gen -- --join 192.168.1.5    # Join a session (or --join to browse)

# The one desktop app — Gen's world, one web page around it (crates/app/ui:
# React, Tailwind, CodeMirror, xterm.js) with the agents' own terminals in it
(cd crates/app/ui && npm ci && npm run build)      # the page, once; the binary carries it
cargo run -p localgpt-app                          # reopens what was opened last
cargo run -p localgpt-app -- --md notes.md         # edit a document, the world rebuilds
cargo run -p localgpt-app -- --song track.mp3      # a song as a world that performs it
cargo run -p localgpt-app -- --world harbour.world  # a .world package opens as a live canvas:
                                                   # agents edit through its API ("at" an
                                                   # earlier entry forks), the history panel
                                                   # draws the branches and plays them
cargo run -p localgpt-app --features local-llm-metal -- --md doc.md   # in-process GGUF
(cd crates/app/ui && npm run dev)                  # the page with reload; run the app with
                                                   # LOCALGPT_APP_UI_URL=http://localhost:5174
(cd crates/app/ui && npm run design)               # the design sheets: new interface is drawn
                                                   # there and reviewed before it is wired
LOCALGPT_APP_REMOTE=on cargo run -p localgpt-app   # and on a phone on this network: prints a
                                                   # pairing link (webui/remote.rs; port 9880,
                                                   # LOCALGPT_APP_REMOTE_PORT for another)

# MD — a Markdown file as a walkable world
cargo run -p localgpt-md                           # samples/hello.md
cargo run -p localgpt-md -- path/to/doc.md         # any file (rebuilds on save)
cargo run -p localgpt-md --features llm-metal -- doc.md   # LLM-authored scenery

# Verse — a 3D world for every song
cargo run -p localgpt-verse
cargo run -p localgpt-verse --features llm-metal   # LLM recipes, Apple GPU

# The two shared downloads (once per machine, not per app)
scripts/fetch-model.sh                             # ~5 GB GGUF
scripts/fetch-assets.sh                            # 522 MB CC0 pack, once;
                                                   # symlinks it into md's and
                                                   # verse's asset roots
```

### Releasing

cargo-release with one shared version (`[workspace.metadata.release]`).
Pass `--workspace` to every step that selects packages (`version`,
`publish`, `tag`, `push`): the root's `default-members` is the CLI alone,
so without it cargo-release selects only `localgpt`. `commit` takes no
package flags and always commits the whole workspace. Edit
`CHANGELOG.md` (Unreleased becomes `## [X.Y.Z] - date`, plus the links at
the bottom) in its own commit first.

```bash
cargo release version X.Y.Z --workspace --execute   # bumps every crate and the internal requirements
cargo release commit --execute                      # the "chore: Release" commit
git tag -s vX.Y.Z -m vX.Y.Z                         # signed, like every v tag
git push origin main vX.Y.Z                         # .github/workflows/release.yml publishes
```

The tag push runs `.github/workflows/release.yml`: it checks the tag
against the version, asks crates.io which crates are already up
(`scripts/release-plan.py`, so a rerun finishes a partial publish), builds
the rest from their packages and uploads them in dependency order, through
Trusted Publishing. A crate published for the first time needs the
`CARGO_REGISTRY_TOKEN` secret, since crates.io can only trust a workflow
for a crate that exists; crates.io also limits how many new crates go up
at once, and a rerun picks up the rest. Members with `publish = false`
(app, md, verse, relay, previs-web) stay off crates.io; core publishes no
version after 0.4.2.
`cargo publish --workspace --dry-run` is the local check.

The desktop app releases on its own tag: `desktop-vX.Y.Z` (the same
version) runs `.github/workflows/desktop.yml`, which builds `LocalGPT.app`
for Apple silicon and Intel with `apps/app-desktop/macos/build-app.sh`
and drafts a GitHub release with the two `.dmg` files and the Homebrew
cask; `apps/app-desktop/README.md` has the secrets and the steps after
the draft.

## Architecture

LocalGPT is the desktop app where a prompt, a document, a song or a
screenplay becomes a walkable `.world`, with the coding agents in its page's
terminals; the `localgpt` CLI, a `.world` package's command line and the MCP
shim; Gen, MD, Verse and Previs; and the format crates. The assistant (chat,
daemon, memory, heartbeat) had its last release in 0.4.2: its daemon, server,
bridges, sandbox, shell tools and mobile apps are gone from the tree, and what
is left of it is `localgpt-core`, the agent loop Gen's own chat still runs on
(`docs/rfcs/authoring/the-harness-goes.md`).

### Workspace

`Cargo.toml` defines 16 members. **MD and Verse are members**, not separate
repositories — they were folded in so the world format cannot drift between
apps (see `docs/world-strategy.md` §13.6). `crates/spacetime` is a **standalone** crate
(its own `[workspace]`) excluded from the main build because it targets
SpacetimeDB's wasm/module toolchain.

```
crates/
├── core/         # localgpt-core — the assistant's agent loop, memory, config, providers, MCP (goes in 0.5.0)
├── cli/          # localgpt — `localgpt world` (a .world package's command line) and `world mcp` (the shim)
├── gen/          # localgpt-gen — Bevy 3D scene generation binary
├── app/          # localgpt-app — the one desktop app: Gen's world, documents and songs as modes; its interface is the web page in app/ui (docs/rfcs/authoring/the-page.md)
├── md/           # localgpt-md — a Markdown file as a walkable world (pure lib + Bevy bin)
├── verse/        # localgpt-verse — a 3D world for every song (lib + Bevy bin; `song_world`)
├── previs/       # localgpt-previs — screenplay (Fountain, Final Draft .fdx, PDF or Markdown pages) → per-scene .world with set/cast/coverage + board (deterministic)
├── previs-web/   # localgpt-previs-web — previs's draft as WASM for the localgpt.md page
├── world-types/  # localgpt-world-types — serde-only world data model (no Bevy/SpacetimeDB)
├── world-bevy/   # localgpt-world-bevy — the one Bevy mapping of the format (Gen, MD, Verse)
├── world-audio/  # localgpt-world-audio — the one mapping of the format's audio (kira + FunDSP)
├── world-export/ # localgpt-world-export — web viewer (three.js) + HTML export, no Bevy
├── world-sync/   # localgpt-world-sync — room authority: shared doc, EditOps, presence
├── world-editor/ # localgpt-world-editor — the non-linear editing model: tips, forks, folds, deltas (no engine, no I/O)
├── world-agent/  # localgpt-world-agent — tool-call protocol, interpreter, shared paths
├── relay/        # localgpt-relay — internet reach for collaborative rooms
└── spacetime/    # localgpt-spacetime — SpacetimeDB multiplayer world server (standalone)

apps/
└── app-desktop/  # LocalGPT.app bundler for the one app (Developer ID + notarization when given; replaced gen-desktop)

scripts/          # The shared downloads, fetched once for every app
├── fetch-model.sh   # the ~5 GB GGUF -> ~/.local/share/localgpt/models/llm
└── fetch-assets.sh  # the 522 MB CC0 pack -> ~/.local/share/localgpt/models/pack

website*/         # One Cloudflare Worker each, all deployed by hand
├── website/      # localgpt.app — Zola, the family hub and only docs site (deployed by website/deploy.sh)
├── website-gen/  # gen.localgpt.app — static landing
├── website-md/   # localgpt.md — drop a screenplay (PDF, Final Draft, Fountain or Markdown), get a previs board in the tab (WASM draft)
├── website-verse/# verse.localgpt.app — static landing
└── website-world/# localgpt.world — the gallery and the one web viewer
```

### Dependency Graph

```
localgpt (CLI)  → core (no default features), world-agent, world-editor
localgpt-gen    → core (agent loop, config, MCP server), previs, world-*
localgpt-app    → core (config, paths, providers for document authoring),
                  gen, md, verse, previs, world-agent, world-bevy, world-types
md, verse, previs, relay, world-*  → no core

World format: world-types → world-bevy (bevy, no windowing) and world-export
  (three.js viewer + HTML, no Bevy); gen depends on both. LocalGPT MD and
  Verse depend on these three crates from this repository too.
```

**The world format is shared across apps.** `localgpt-world-types` is the
source format (serde-only; `schema` feature writes `world.schema.json`);
`localgpt-world-bevy` is the only place a `Shape`, `MaterialDef`, `LightDef`
or `EnvironmentDef` becomes Bevy components — Gen, MD and Verse all call it;
`localgpt-world-export` holds the only web renderer
(`js/world-viewer.js` and the `js/cinematography.js` it imports, vendored
snapshots of the `openworldformat` npm package — `scripts/sync-viewer.sh`)
and `generate_html`. Reference scenes live in
`crates/world-types/conformance/`; render them before and after changing a
mapping. Add fields to world-types, never a parallel format.

**`localgpt-core` takes nothing new.** It goes in 0.5.0, after its stdio MCP
server and `Tool` trait move to `world-agent`; new code goes in the crate that
outlives it. It has no shell or file tools: the coding agents in the app's
terminals do that work with their own.

### Feature Flags (`localgpt-core`)

Default: `claude-cli`, `gemini-cli`, `codex-cli`, `image-resize`. Memory
searches by keyword (SQLite FTS5), with vectors from an embeddings API when
`memory.embedding_provider` is `openai` or `gemini`; the in-process models
(fastembed over ONNX Runtime, llama.cpp) and sqlite-vec are gone.

| Feature | Default | Purpose |
|---------|---------|---------|
| `claude-cli` | yes | ClaudeCliProvider (a subprocess) |
| `gemini-cli` | yes | GeminiCliProvider (a subprocess) |
| `codex-cli` | yes | CodexCliProvider (a subprocess) |
| `image-resize` | yes | Resize images before sending to vision models |
| `mcp-http` | no | MCP streamable HTTP transport (adds axum + tower-http) |

### Key Patterns

**Safe tools only:** `Agent::new()` registers `tools::create_safe_tools()`
(memory_search, memory_get, web_fetch, web_search, document_load, wiki,
audio_transcribe, spawn_agent). There are no bash or file tools any more.

**Custom tool sets:** `Agent::new_with_tools()` replaces all tools — used by Gen
mode for its own Bevy/world tools (spawn_entity, modify_entity, etc.).

**Thread safety:** Agent is not `Send+Sync` (SQLite); keep one on the task
that made it.

**Bevy main thread:** Bevy must own the main thread (macOS windowing/GPU). Gen
mode spawns tokio on a background thread.

**Session compaction:** Approaching context limits triggers a memory flush first
(the LLM saves important context to MEMORY.md before older messages are
truncated). See `agent/compaction.rs`.

**Provider failover:** `agent/failover.rs` can fall back across configured
providers when a primary errors out.

**Memory context:** New sessions auto-load `MEMORY.md` and recent daily logs.
Active recall (`memory/active_recall.rs`) can search memory before each reply
and inject results.

**Audio:** `localgpt-world-audio` is the one mapping from the format's audio
types to sound, the counterpart to `world-bevy` for meshes. **kira owns the
device, FunDSP is a source** — they are different layers, not alternatives, so
the crate is one `AudioManager` with the synthesis graphs feeding it through
`FundspSound`. `graphs::build` takes `wt::AudioSource` (already the union of
Gen's `AmbientSound` and `EmitterSound`) and is pure, so it tests without an
audio device. Gen's `gen3d/audio.rs` engine and Verse's `audio.rs` still run
their own; migrating them onto this crate is what lets one process play a
document world's ambience and a song world's soundtrack, which Worlds needs.

**Shared downloads:** the ~5 GB GGUF and the 522 MB CC0 asset pack are
resolved by `localgpt-world-agent`'s `paths` module — `shared_llm_dir()` and
`world_pack_dir()` — so one download serves Gen, MD and Verse. It lives there
because all three depend on that crate and none of them can depend on
`localgpt-core`. Never re-implement the XDG rule in an app; `LOCALGPT_LLM_DIR`
and `LOCALGPT_WORLD_ASSETS` override.

**Path expansion & safety:** Tools use `shellexpand::tilde()` for `~`; path
handling and traversal guards live in `agent/path_utils.rs`.

**Provider routing:** Prefer the explicit `provider/model` form
(OpenClaw-compatible). Bare model names are routed by prefix in
`create_provider()`:

| Model prefix / scheme | Provider |
|-----------------------|----------|
| `claude-cli/*` | Claude CLI (subprocess) |
| `gemini-cli/*` | Gemini CLI (subprocess) |
| `codex-cli/*` | Codex CLI (subprocess) |
| `anthropic/*`, `claude-*` | Anthropic API |
| `openai/*`, `gpt-*`, `o1*` | OpenAI |
| `xai/*`, `grok-*` | xAI |
| `gemini/*`, `gemini-*` | Gemini API key |
| `vertex/*` | Vertex AI |
| `glm/*`, `glm-*` | GLM (Z.AI) |
| `ollama/*`, `local/*` | Ollama |
| `openrouter/*`, `openai-compat/*` | OpenAI-compatible endpoints |

Aliases resolve first (e.g. `opus` → `anthropic/claude-opus-4-6`, `sonnet`,
`gpt`, `glm`, `grok`, `codex`). Default `agent.default_model` is `claude-cli/opus`.

### Core Modules (`crates/core/src/`)

- **agent/providers.rs** — `LLMProvider` trait + implementations: OpenAI,
  OpenAI-compatible (OpenRouter etc.), xAI, Anthropic, Ollama, ClaudeCli,
  GeminiCli, CodexCli, Gemini (API key), Vertex AI, GLM. Includes model-alias
  resolution and per-model pricing.
- **agent/session.rs**, **compaction.rs**, **checkpoint.rs** — conversation
  state, automatic compaction, checkpoint/restore.
- **agent/session_store.rs**, **session_pruning.rs** — session metadata
  persistence and pruning.
- **agent/system_prompt.rs** — system prompt builder (identity, safety,
  workspace, tools, skills).
- **agent/skills.rs** — SKILL.md loading from `workspace/skills/`.
- **agent/tool_filters.rs**, **hardcoded_filters.rs**, **sanitize.rs**,
  **ssrf.rs** — tool input filtering, output sanitization, SSRF protection.
- **agent/approval.rs** — human-in-the-loop tool approval (HTTP approve flow).
- **agent/tools/** — safe tool set: `spawn_agent` (hierarchical delegation),
  `web_search`, plus memory/web/document/wiki/audio tools.
- **memory/** — SQLite FTS5 (`backend_sqlite.rs`, `backend_markdown.rs`,
  `backend_none.rs`), file watcher, an embeddings API when one is configured,
  query expansion, session index, **active_recall**, and **wiki**
  (claims/evidence with staleness tracking).
- **mcp/** — MCP client (stdio + HTTP/SSE transports) that exposes external MCP
  server tools as LocalGPT tools, plus the stdio MCP **server** the shim runs.
- **media/** — document loading, audio transcription, TTS, image optimization.
- **config/** — the TOML schema (`mod.rs`); `Config::load()`. A config written
  for the assistant keeps its `[heartbeat]`, `[server]`, `[sandbox]`, `[cron]`
  and `[hooks]` tables, which are ignored.
- **paths.rs** — XDG dirs, `Paths::resolve()`. **env.rs** centralizes all
  `LOCALGPT_*` env var names.
- **commands.rs** — the slash commands of Gen's chat.
- **security/** — POLICY.md policy loading (with legacy `LocalGPT.md`
  fallback) and injection sanitization, and the hardcoded security suffix.

### CLI Subcommands (`crates/cli/src/cli/`)

`world` (a `.world` package's command
line — init, submit (`--at <id>` forks), undo, log, tips, verify, history — and
`world mcp`, a stdio MCP server over an app's open canvas;
`crates/cli/src/cli/world.rs` over `world-agent::headless`, which is shared with
the `live` example). The head is `refs.main` (else the last entry): read it
with `LiveWorld::main_tip`, never as "the last line" — a branch commit is
always the last line.

### Gen (3D Scene Generation with Audio + Multiplayer)

**Binary:** `localgpt-gen` — Bevy-based 3D scene generation with procedural
environmental audio. World data uses **`localgpt-world-types`** (serde-only,
zero Bevy/SpacetimeDB deps) so the same types serialize to RON for local saves
and map to SpacetimeDB rows for multiplayer via `crates/spacetime`. Meshes,
materials, lights and environment come from `localgpt-world-bevy`;
`gen_export_html` embeds `localgpt-world-export`'s viewer.

**Multiplayer (`multiplayer` feature, default on):** collaborative rooms
over the ops protocol — `localgpt-gen --host` runs the room authority
(`localgpt-world-sync`, `crates/world-sync`: shared document, validated
`EditOp`s, revisions, presence, jobs) plus a session HTTP server (join page
+ WebSocket `/session`, TCP 9879) and an mDNS announcement
(`_localgpt-world._udp.local.`). `localgpt-gen --join [addr] [--pin PIN]`
runs the native client (`net/ops_client.rs` — the full gen scene driven by
the room); `--web` adds browser guests via an invite link (`--web-edit`
gives them the editor role); guests prompt a scene-only agent unless
`--remote-tools full`. The host projects its scene into ops (~4 Hz), so
every tool syncs; committed ops apply back into its scene. Sessions are
packages (`<workspace>/sessions/<name>/`: base `world.ron`, the one log
`ops.jsonl` — edits plus tool/state history — `session.json`, keyframe
snapshots; see `docs/rfcs/multiplayer/session-package-format.md`) with
per-user `/undo`, `--resume`, and `--replay` time-lapse. See
`docs/gen/multiplayer.md` and
`crates/gen/src/net/`; the SpacetimeDB module carries the cloud-tier
inference queue (`crates/spacetime/src/jobs.rs`).

**Audio System:** `localgpt-world-audio` — one kira `AudioManager` with the
FunDSP graphs feeding it as sources. The Bevy side (`gen3d/audio.rs`) keeps
metadata and forwards listener/emitter transforms to a dedicated thread that
owns the engine; emitters are kira spatial tracks, so distance attenuation and
panning come from positions rather than per-frame math. Ambient
(Wind/Rain/Forest/Ocean/Cave/Stream/Silence) and spatial emitters
(Water/Fire/Hum/Wind/Custom). Auto-inference from entity names ("campfire",
"waterfall"). Tools: `gen_set_ambience`, `gen_audio_emitter`,
`gen_modify_audio`, `gen_audio_info`. See `docs/gen/gen-audio.md`.

**Behavior System:** Declarative, data-driven animations (all 7 types in
`world-types`): `orbit`, `spin`, `bob`, `look_at`, `pulse`, `path_follow`
(loop/ping-pong/once), `bounce`. Composable (stack on one entity). Tools:
`gen_add_behavior`, `gen_remove_behavior`, `gen_list_behaviors`,
`gen_pause_behaviors`.

**World Skills:** Save/load complete worlds as skill directories — `SKILL.md` +
`world.ron` (entities, shapes, materials, behaviors, audio, avatar, tours
inline). Tools: `gen_save_world`, `gen_load_world` (auto-clears by default),
`gen_clear_scene`. Avatar section defines user presence (spawn, PoV mode, speed,
height, optional model). Tours define guided named waypoint sequences
(walk/fly/teleport).

**WorldGen Pipeline (WG1–WG7):** Blockout-first workflow
(`gen_plan_layout` → `gen_apply_blockout` → `gen_populate_region`), grid navmesh
with A* pathfinding + slope/erosion, hierarchical hero/medium/decorative
placement with collision-aware ground snap, screenshot-based evaluation loop,
incremental blockout editing, semantic scene decomposition, depth/2D preview.
**40+ MCP tools** across P0–P5, WG1–WG7, AI1–AI2. See
`docs/gen/external-services.md` for external services (Ollama, ComfyUI, model
server).

### A phone or a tablet

No native app: the desktop app serves its own page to a paired device's
browser (`crates/app/src/webui/remote.rs`, `crates/app/ui/src/phone/`;
`docs/rfcs/authoring/mobile-remote.md`). `scripts/remote-smoke.mjs` plays a
device against a running app.

## Configuration

Config: `~/.config/localgpt/config.toml` (auto-created on first run, see
`config.example.toml`).

Key settings:
- `agent.default_model` — determines provider. Default: `claude-cli/opus`.
- `agent.subagent_model` — model for spawned subagents (defaults to
  `default_model`).
- `providers.claude_cli.effort` — `low` | `medium` | `high` | `max` (default
  `max`); switch at runtime with the `/effort` slash command.
- `providers.*` — per-provider API keys/endpoints (anthropic, openai, xai,
  gemini, vertex, glm, ollama, openrouter, openai-compat). Supports
  `${ENV_VAR}` interpolation.
- `memory.workspace` — workspace dir. Default
  `~/.local/share/localgpt/workspace`.
- `memory.embedding_provider` — `"none"` (default, keyword search),
  `"openai"` or `"gemini"`.

Workspace path resolution: `LOCALGPT_WORKSPACE` env > `LOCALGPT_PROFILE` env >
`memory.workspace` config > `~/.local/share/localgpt/workspace`. All
`LOCALGPT_*` env vars are defined centrally in `crates/core/src/env.rs`.

## Runtime Directory Structure (XDG Base Directory Specification)

```
~/.config/localgpt/                      # XDG_CONFIG_HOME
├── config.toml
~/.local/share/localgpt/                 # XDG_DATA_HOME
├── workspace/                           # Memory workspace
│   ├── MEMORY.md                        # Long-term curated memory
│   ├── SOUL.md                          # Persona/tone
│   ├── memory/YYYY-MM-DD.md             # Daily logs
│   ├── knowledge/                       # Knowledge repository
│   └── skills/*/SKILL.md                # Custom skills
└── skills/                              # Managed skills
~/.local/state/localgpt/                 # XDG_STATE_HOME
├── agents/{agent_id}/sessions/          # Session transcripts (JSONL)
├── app-remote.json                      # The phone remote's paired devices (key hashes, 0600)
└── logs/                                # Application logs
~/.cache/localgpt/                       # XDG_CACHE_HOME
└── memory/{agent_id}.sqlite             # Search index (regenerable)
```

## Conventions

- **Lint clean before committing:** `cargo clippy --workspace -- -D warnings`
  and `cargo fmt --check` must pass (enforced in CI). CI
  (`.github/workflows/ci.yml`) runs fmt, clippy and tests for the workspace
  with the four Bevy crates (`gen`, `md`, `verse`, `world-bevy`) in their own
  job, a `features` job for the opt-in tiers (`llm`, `local-llm`, `ml` — gated
  code is not compiled by anything else), an `interface` job that typechecks
  and builds the app's page and its design sheets (`crates/app/ui`), a `viewer` job running
  `website-world`'s headless render check, plus cargo-deny (`deny.toml`); advisories run on main and weekly, never on
  pull requests. Only Verse's `ml` tier links ONNX Runtime; the `features`
  job fetches the version it pins.
- **Commits:** conventional commits (`feat:`, `fix:`, `docs:`, `chore:`,
  `refactor:`, scoped like `feat(gen):`), with no Co-Authored-By or
  Claude-Session trailers.
- **Nothing new in `localgpt-core`:** it goes in 0.5.0. Put new code in the
  crate that outlives it (`world-agent`, `gen`, `app`).
- **Edition 2024**, workspace-shared version (`0.4.x`), `resolver = "3"`.
- Related docs live under `docs/` (architecture, gen, security, rfcs,
  roadmap; retired designs in `docs/archived/`). `AGENTS.md` is the
  longer-form contributor guide.
- **Websites:** `website/` is the Zola site for localgpt.app, the
  family hub and its only docs site: the assistant's docs plus
  `website/docs/gen`, `docs/verse` and `docs/md` (Verse and MD keep only
  landing pages in their own repos; update their docs here when those apps
  change). `website-gen/` is the static landing page for gen.localgpt.app (no
  build step; `website-gen/deploy.sh` deploys it) and links to the Gen docs.
  `website-md/` is localgpt.md, the previs page (`docs/world-strategy.md`
  §15): drop a screenplay — a PDF, a Final Draft `.fdx`, Fountain, or
  Markdown script pages — and get the
  board and the per-scene `.world` packages in the tab. Its draft is
  `crates/previs-web` (wasm-pack over `crates/previs`, which touches no
  model, renderer or network) and its page check (drop the four samples through
  the real input, require the rendered board and the downloads) runs in the
  same `viewer` CI job. MD has no page of its own; its docs are on
  localgpt.app.
  `website-world/` is localgpt.world: the gallery and the format's only web
  renderer. It is **assembled** — `npm run assemble` copies
  `crates/world-export/js/world-viewer.js` and the `crates/world-types`
  conformance scenes in, and those copies are gitignored, so edit them in the
  crate and re-assemble (the crate file itself is vendored from the
  `openworldformat` npm package via `scripts/sync-viewer.sh` — the renderer's
  upstream is that repository). Its `npm run check` (CI job `viewer`) is the only
  automated check on that renderer; it lived in a separate repository until the
  copy there drifted 185 lines behind. The curated worlds' shrunk GLBs and
  music are LFS objects in `localgpt-world-assets` under `web/`, not tracked
  here.
  Every family site carries the same family strip, in this order: LocalGPT,
  Gen, Verse, MD, localgpt.world, localgpt.rs.
