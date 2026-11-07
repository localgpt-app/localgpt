# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

`cargo xtask` is the one entry point for building and releasing, in Rust
(`xtask/`); run alone, it lists every command:

```bash
cargo xtask build app gen               # cargo build, by name: cli app gen previs md verse all
cargo xtask bundle --dmg                # LocalGPT.app for macOS
cargo xtask verify                      # the CI checks on this machine (--rust --bevy --features --web --deps)
cargo xtask rule                        # the all-Rust rule: nothing new in another language, nothing new on wry
cargo xtask deploy world --dry-run      # the family's sites: app md world, or all
cargo xtask fetch-assets                # the shared asset pack
```

```bash
# Build
cargo build                     # Debug build (default-members = crates/cli)
cargo build --release           # Release build (LTO, stripped — slow)
cargo build --workspace         # Build all workspace crates

# Run
cargo run -- world init demo.world   # a .world package's command line (init, submit,
                                     # undo, redo, log, tips, show, get, verify, history;
                                     # screenshot, selection, goto, proposals with an app);
                                     # through the app's API while one has the world open
cargo run -- world mcp               # the shim: a stdio MCP server over an app's open canvas

# Test
cargo test --workspace          # All tests
cargo test -p localgpt-gen      # Single crate
cargo test -- --nocapture       # Show stdout

# Lint (required before commits)
cargo clippy --workspace -- -D warnings
cargo fmt --check

# Gen (the world builder with Bevy; no model runs in it — an agent outside
# the window drives it over MCP or through a live package's API)
cargo run -p localgpt-gen -- --mcp-relay           # the window, with the relay an agent's
                                                   # `mcp-server --connect` reaches
cargo run -p localgpt-gen -- mcp-server            # the agent's own process: window + tools (stdio)
cargo run -p localgpt-gen -- mcp-server --headless --mcp-http 8765   # no window; also HTTP
cargo run -p localgpt-gen -- --live demo.world     # a package as a live canvas (ops API)
cargo run -p localgpt-gen -- --world castle        # open a saved world, a manifest or a URL
cargo run -p localgpt-gen -- -s model.glb          # Load existing scene
cargo run -p localgpt-gen -- -v                    # Verbose logging
cargo run -p localgpt-gen -- --host                # Host a collaborative session (mDNS, UDP 9879)
cargo run -p localgpt-gen -- --join 192.168.1.5    # Join a session (or --join to browse)
cargo run -p localgpt-gen -- --replay castle       # a session as a time-lapse

# The one desktop app — Gen's world in the shell's panels (egui on eframe,
# crates/app/src/shell; docs/rfcs/authoring/all-rust.md) with plain terminals
# in them, where the person runs whatever agent they have
(cd crates/app/ui && npm ci && npm run build)      # a phone's page, once; the binary carries it
cargo run -p localgpt-app                          # reopens what was opened last
cargo run -p localgpt-app -- --md notes.md         # edit a document, the world rebuilds
cargo run -p localgpt-app -- --song track.mp3      # a song as a world that performs it
cargo run -p localgpt-app -- --world harbour.world  # a .world package opens as a live canvas:
                                                   # agents edit through its API ("at" an
                                                   # earlier entry forks), the history panel
                                                   # draws the branches and plays them
(cd crates/app/ui && npm run design)               # the phone's design sheet; a debug build reads
                                                   # the built page from disk, so after `npm run
                                                   # build` a phone's reload shows a change
LOCALGPT_APP_REMOTE=on cargo run -p localgpt-app   # and on a phone on this network: prints a
                                                   # pairing link (webui/remote.rs; port 9880,
                                                   # LOCALGPT_APP_REMOTE_PORT for another)
LOCALGPT_APP_SHELL_DRIVE='4:click 0.5 0.5;6:shot /tmp/s.png;7:quit' \
  cargo run -p localgpt-app                        # the shell by a script, a check without a hand
                                                   # (shell/drive.rs: click, tap, key, drag, swipe,
                                                   # wheel, type, shot, quit)

# MD — a Markdown file as a walkable world
cargo run -p localgpt-md                           # samples/hello.md
cargo run -p localgpt-md -- path/to/doc.md         # any file (rebuilds on save; applies
                                                   # the sidecar an agent wrote beside it)

# Verse — a 3D world for every song
cargo run -p localgpt-verse
cargo run -p localgpt-verse --features ml          # CLAP moods and Demucs stems (ONNX)

# The shared download (once per machine, not per app)
cargo xtask fetch-assets                           # 522 MB CC0 pack, once;
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
(`cargo xtask release-plan`, so a rerun finishes a partial publish), builds
the rest from their packages and uploads them in dependency order, through
Trusted Publishing. A crate published for the first time needs the
`CARGO_REGISTRY_TOKEN` secret, since crates.io can only trust a workflow
for a crate that exists; crates.io also limits how many new crates go up
at once, and cargo waits only a minute for an upload to reach the index
before publishing what depends on it (0.4.2 lost two crates to that): a
rerun of the failed job picks up the rest. Members with `publish = false`
(app, md, verse, relay, previs-web) stay off crates.io; `localgpt-core`
published its last version, 0.4.2, and is no longer in the tree.
`cargo publish --workspace --dry-run` is the local check.

The desktop app releases on its own tag: `desktop-vX.Y.Z` (the same
version) runs `.github/workflows/desktop.yml`, which builds `LocalGPT.app`
for Apple silicon and Intel with `cargo xtask bundle --dmg`
and drafts a GitHub release with the two `.dmg` files and the Homebrew
cask; `apps/app-desktop/README.md` has the secrets and the steps after
the draft.

## Architecture

LocalGPT is the desktop app where a prompt, a document, a song or a
screenplay becomes a walkable `.world`, with the coding agents in its
terminals; the `localgpt` CLI, a `.world` package's command line and the MCP
shim; Gen, MD, Verse and Previs; and the format crates. **Nothing in the tree
runs a language model, holds a key, or knows an agent by name.** The agents
come from their makers: they run in the app's terminals and reach the world
over MCP or its ops API, with their own logins. The `Tool` trait and MCP
servers live in `world-agent::mcp`. No tool calls a generation service.
Verse's opt-in `ml` tier (CLAP moods, Demucs stems over ONNX Runtime) is
audio analysis, not a language model, and stays.

### Workspace

`Cargo.toml` defines 15 members. **MD and Verse are members**, not separate
repositories — they were folded in so the world format cannot drift between
apps (see `docs/world-strategy.md`).

```
crates/
├── cli/          # localgpt — `localgpt world` (a .world package's command line), `world mcp` (the shim), `completion`
├── gen/          # localgpt-gen — the world builder: Bevy window, 88 tools over MCP (stdio, HTTP, the relay), live canvas, sessions
├── app/          # localgpt-app — the one desktop app: Gen's world, documents and songs as modes; its interface is the shell in app/src/shell (egui, docs/rfcs/authoring/all-rust.md), and the page in app/ui is a paired phone's
├── md/           # localgpt-md — a Markdown file as a walkable world (pure lib + Bevy bin)
├── verse/        # localgpt-verse — a 3D world for every song (lib + Bevy bin; `song_world`)
├── previs/       # localgpt-previs — screenplay (Fountain, Final Draft .fdx, PDF or Markdown pages) → per-scene .world with set/cast/coverage + board (deterministic)
├── previs-web/   # localgpt-previs-web — previs's draft as WASM for the localgpt.md page
├── world-types/  # localgpt-world-types — serde-only world data model (no Bevy)
├── world-bevy/   # localgpt-world-bevy — the one Bevy mapping of the format (Gen, MD, Verse)
├── world-audio/  # localgpt-world-audio — the one mapping of the format's audio (kira + FunDSP)
├── world-export/ # localgpt-world-export — web viewer (three.js) + HTML export, no Bevy
├── world-sync/   # localgpt-world-sync — room authority: shared doc, EditOps, presence
├── world-editor/ # localgpt-world-editor — the non-linear editing model: tips, forks, folds, deltas (no engine, no I/O)
├── world-agent/  # localgpt-world-agent — the live authority, headless commands, packages, `mcp` (Tool trait + stdio/HTTP servers), `agent_env`, `paths` (Dirs, the shared pack)
└── relay/        # localgpt-relay — internet reach for collaborative rooms

apps/
└── app-desktop/  # LocalGPT.app bundler for the one app (Developer ID + notarization when given; replaced gen-desktop)

xtask/            # The repo's tooling in Rust: cargo xtask rule, verify, fetch-assets
                  # (the 522 MB CC0 pack -> ~/.local/share/localgpt/models/pack)

website*/         # One Cloudflare Worker each, all deployed by hand
├── website/      # localgpt.app — Zola, the family hub and only docs site (cargo xtask deploy app)
├── website-md/   # localgpt.md — drop a screenplay (PDF, Final Draft, Fountain or Markdown), get a previs board in the tab (WASM draft)
└── website-world/# localgpt.world — the gallery and the one web viewer
```

### Dependency Graph

```
localgpt (CLI)  → world-agent (features: mcp), world-editor
localgpt-gen    → world-agent (features: mcp-http), previs, world-*
localgpt-app    → gen, md, verse, previs, world-agent, world-bevy, world-types
md, verse       → world-agent (paths), world-*
previs, relay, world-*  → no app crate

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
snapshots of the `openworldformat` npm package — `cargo xtask sync-viewer`)
and `generate_html`. Reference scenes live in
`crates/world-types/conformance/`; render them before and after changing a
mapping. Add fields to world-types, never a parallel format.

**No model, no service, no key, no agent by name.** Nothing in the
workspace loads a language model, calls a model API or a generation
service, reads a key or a model's configuration, or treats one coding agent
differently from another: the agents run in the app's terminals (plain login
shells) and in MCP clients, under their own logins, and what a model should
author (a document's places, a song's recipe) it authors into a sidecar
through the world's files and API. Do not add a provider, a prompt loop, an
in-process model, a tool that calls a service, an agent launcher or an
agent-specific affordance (presets, turn detection from an agent's output).
Verse's `ml` tier is the one model tier that stays: audio analysis (CLAP
moods, Demucs stems), not language.

### Feature Flags

| Crate | Feature | Default | Purpose |
|-------|---------|---------|---------|
| `world-agent` | `mcp` | no | The `Tool` trait, `ToolSchema`, `McpHandler` and the stdio MCP server (the CLI's shim) |
| `world-agent` | `mcp-http` | no | The streamable HTTP transport over `mcp` (adds axum + tower-http); Gen enables it |
| `gen` | `multiplayer` | yes | `--host`/`--join`, the ops room, mDNS, the browser guest page |
| `verse` | `ml` | no | ONNX Runtime models for audio analysis (the only tier that links `ort`) |
| `world-types` | `schema` | no | Writes `world.schema.json` |

### Key Patterns

**Tools:** every Gen tool implements `localgpt_world_agent::mcp::Tool`
(`name`, `schema`, `execute`, optional `annotations`);
`mcp_server::create_mcp_tools` assembles the 88 and annotates them
(read-only / destructive / idempotent). Tools reach the Bevy world through
`GenBridge` (channels into the ECS); the same set serves stdio, HTTP and the
relay.

**Bevy main thread:** Bevy must own the main thread (macOS windowing/GPU). Gen
spawns tokio on a background thread for the tool server.

**Audio:** `localgpt-world-audio` is the one mapping from the format's audio
types to sound, the counterpart to `world-bevy` for meshes. **kira owns the
device, FunDSP is a source** — they are different layers, not alternatives, so
the crate is one `AudioManager` with the synthesis graphs feeding it through
`FundspSound`. `graphs::build` takes `wt::AudioSource` (already the union of
Gen's `AmbientSound` and `EmitterSound`) and is pure, so it tests without an
audio device. Gen's `gen3d/audio.rs` engine and Verse's `audio.rs` still run
their own; migrating them onto this crate is what lets one process play a
document world's ambience and a song world's soundtrack, which Worlds needs.

**The shared pack:** the 522 MB CC0 asset pack is resolved by
`localgpt-world-agent`'s `paths` module — `world_pack_dir()` — so one
download serves Gen, MD and Verse. It lives there because all three depend
on that crate. Never re-implement the XDG rule in an app;
`LOCALGPT_WORLD_ASSETS` overrides. The same module's `Dirs` (`data`,
`state`, `logs()`) is where an app's own directories come from:
`LOCALGPT_DATA_DIR` and `LOCALGPT_STATE_DIR` move one each,
`LOCALGPT_PROFILE` moves both to `localgpt-<profile>`; the shared pack
ignores the profile on purpose.

**Environment names:** the world's are constants in
`world-agent::agent_env` (`LOCALGPT_WORLD_DIR`, `_API`, `_TOKEN`, `_AUTHOR`),
the directories' in `world-agent::paths`, Gen's session PIN in
`crates/gen/src/net/mod.rs`, the app's in `crates/app/src`
(`LOCALGPT_APP_*`). Add a name to the crate that reads it, as a constant,
never a bare string.

### World-agent's `mcp` module (`crates/world-agent/src/mcp/`)

- **mod.rs** — the `Tool` trait and `ToolSchema`, `McpHandler` (initialize,
  tools/list, tools/call; protocol `2024-11-05`), `ToolHandler` over a tool
  list, and `run_mcp_stdio_server` (JSON-RPC 2.0, one message per line on
  stdout; logs go to stderr).
- **http.rs** (`mcp-http`) — `run_mcp_http_server`: the streamable HTTP
  transport at `/mcp` on localhost.

### CLI Subcommands (`crates/cli/src/cli/`)

`world` (a `.world` package's command
line — init, submit (`--at <id>` forks), undo, redo, log, tips, show, get,
verify, history, and screenshot, selection, goto and proposals, which need an
app — and `world mcp`, a stdio MCP server over an app's open canvas;
`crates/cli/src/cli/world.rs`), and `completion`. That is the whole binary:
no chat, no config, no key. One authority at a time: while an app answers
at `.live/endpoint.json`, every command is a call to its API
(`world_api.rs`, which the shim shares) and the CLI never opens the package;
with nothing open it commits through `world-agent::headless`, which is shared
with the `live` example. Both hand back `live::receipt`. The folder is the
first argument, else `$LOCALGPT_WORLD_DIR` (the app's tabs set it), else the
world the shell is in. The head is `refs.main` (else the last entry): read it
with `LiveWorld::main_tip`, never as "the last line" — a branch commit is
always the last line.

### Gen (3D Scene Generation with Audio + Multiplayer)

**Binary:** `localgpt-gen` — Bevy-based 3D scene generation with procedural
environmental audio, driven from outside: `--mcp-relay` serves a relay beside
the window that an agent's `mcp-server --connect` process is the stdio end
of (`gen3d/mcp_relay.rs`); `mcp-server` alone is the agent's own process
with a window, `--headless` without one, `--mcp-http <port>` adds the HTTP
transport; `--live <folder>` opens a package as a live canvas with its ops
API (`gen3d/live.rs`). Started with none of these, the window says so and
shows what was opened last. There is no prompt, no chat and no model in the
binary; `config::GenConfig { workspace, state_dir }` comes from
`world-agent::paths::Dirs`, and `settings.rs` remembers only what was opened
last. World data uses **`localgpt-world-types`** (serde-only, no Bevy) so
the same types serialize to RON for local saves and travel as ops in a
collaborative room. Meshes, materials, lights and environment come from
`localgpt-world-bevy`; `gen_export_html` embeds `localgpt-world-export`'s
viewer.

**Multiplayer (`multiplayer` feature, default on):** collaborative rooms
over the ops protocol — `localgpt-gen --host` runs the room authority
(`localgpt-world-sync`, `crates/world-sync`: shared document, validated
`EditOp`s, revisions, presence, jobs) plus a session HTTP server (join page
+ WebSocket `/session`, TCP 9879) and an mDNS announcement
(`_localgpt-world._udp.local.`). `localgpt-gen --join [addr] [--pin PIN]`
runs the native client (`net/ops_client.rs` — the full gen scene driven by
the room); `--web` adds browser guests via an invite link (`--web-edit`
gives them the editor role). No model runs for the room: a guest's prompt is
accepted by the authority and failed with a reason (`net/web.rs`), and the
agent that builds is the host's, outside the window, driving it over MCP.
The host projects its scene into ops (~4 Hz), so every tool syncs; committed
ops apply back into its scene. Sessions are packages
(`<workspace>/sessions/<name>/`: base `world.ron`, the one log `ops.jsonl` —
edits plus tool/state history — `session.json`, keyframe snapshots; see
`docs/rfcs/multiplayer/session-package-format.md`) with per-user `/undo`,
`--resume`, and `--replay` time-lapse. See `docs/gen/multiplayer.md` and
`crates/gen/src/net/`. The SpacetimeDB module that was to be the cloud-room
authority went on 2026-11-01, unjoined; a persistent room is a headless
host or a relay over the same `world-sync` authority.

**Audio System:** `localgpt-world-audio` — one kira `AudioManager` with the
FunDSP graphs feeding it as sources. The Bevy side (`gen3d/audio.rs`) keeps
metadata and forwards listener/emitter transforms to a dedicated thread that
owns the engine; emitters are kira spatial tracks, so distance attenuation and
panning come from positions rather than per-frame math. Ambient
(Wind/Rain/Forest/Ocean/Cave/Stream/Silence) and spatial emitters
(Water/Fire/Hum/Wind/Custom). Auto-inference from entity names ("campfire",
"waterfall"). Tools: `gen_set_ambience`, `gen_audio_emitter`,
`gen_modify_audio`, `gen_audio_info`. See `docs/gen/gen-audio.md`.

**Behavior System:** Declarative, data-driven animations (all 8 types in
`world-types`): `orbit`, `spin`, `bob`, `look_at`, `pulse`, `swing`,
`path_follow` (loop/ping-pong/once), `bounce`; `spin`, `pulse` and `swing`
act about a `pivot` and take a `phase`. Composable (stack on one entity;
each frame starts from the authored transform and applies them in order, as
the format says; a kind the build does not know is carried, never run). Tools:
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
incremental blockout editing, semantic scene decomposition, a depth-map
render. **88 MCP tools** across P0–P5, WG1–WG7 and AI3 (`AGENTS.md` is the
catalog). AI1 (asset and texture generation on a model server), AI2 (NPC
brains on Ollama, with their memory and observation) and WG7.2 (a ComfyUI
preview) went on 2026-11-01: no tool calls a service. AI3's image input
(layout from an image, a panorama, a style match, the reference board) is
pure image analysis and stays.

### A phone or a tablet

No native app: the desktop app serves its own page to a paired device's
browser (`crates/app/src/webui/remote.rs`, `crates/app/ui/src/phone/`;
`docs/rfcs/authoring/mobile-remote.md`). A phone gets three views; a
tablet the world and the terminals at once, beside or over each other
(`phone/layout.tsx`: by the page's width, never its height, which the
keyboard takes). `scripts/remote-smoke.mjs` plays a device against a
running app, and with `--phone` drives its page at an iPhone's and an
iPad's size.

## Configuration

There is no configuration file: `config.toml` and `config.example.toml` went
with the assistant. What an app needs it takes from flags and environment
variables — the directories from `LOCALGPT_DATA_DIR`, `LOCALGPT_STATE_DIR`
and `LOCALGPT_PROFILE`, the shared pack from `LOCALGPT_WORLD_ASSETS` — and
remembers only what was opened last (`gen-settings.json`) and, in the
desktop app, where its panels were (`app-layout.json`). An agent's own
configuration (its login, its model, its MCP servers) is the agent's.

## Runtime Directory Structure (XDG Base Directory Specification)

```
~/.local/share/localgpt/                 # XDG_DATA_HOME (Dirs::data)
├── gen-workspace/                       # Gen's worlds (skills/), sessions/, exports/
└── models/                              # the shared download; ignores LOCALGPT_PROFILE
    └── pack/                            # the CC0 asset pack (cargo xtask fetch-assets)
~/.local/state/localgpt/                 # XDG_STATE_HOME (Dirs::state)
├── gen-settings.json                    # what was opened last
├── app-remote.json                      # The phone remote's paired devices (key hashes, 0600)
├── app-layout.json                      # The panels as a hand left them (tiles, hidden, folded)
└── logs/                                # Application logs, when there is no terminal
```

A profile (`LOCALGPT_PROFILE=x`) moves both roots to `localgpt-x`. Worlds a
0.4.x assistant saved under `workspace/skills/` are copied into
`gen-workspace/skills/` once (`config::migrate_legacy_worlds`).

## Conventions

- **Lint clean before committing:** `cargo clippy --workspace -- -D warnings`
  and `cargo fmt --check` must pass (enforced in CI). CI
  (`.github/workflows/ci.yml`) runs fmt, clippy and tests for the workspace
  with the four Bevy crates (`gen`, `md`, `verse`, `world-bevy`) in their own
  job, a `features` job for the opt-in tier (Verse's `ml` — gated code is
  not compiled by anything else), an `interface` job that typechecks
  and builds the phone's page and its design sheet (`crates/app/ui`), a `viewer` job running
  `website-world`'s headless render check, a `rule` job (`cargo xtask rule`, below), plus cargo-deny (`deny.toml`); advisories run on main and weekly, never on
  pull requests. Only Verse's `ml` tier links ONNX Runtime; the `features`
  job fetches the version it pins.
- **Rust above the operating system** (`docs/rfcs/authoring/all-rust.md`,
  decided 2026-11-05): nothing new in another language — no JavaScript,
  TypeScript, Python or shell — and no new crate depending on `wry`. A
  script is an `xtask` command (`xtask/`, run as `cargo xtask <command>`),
  a check a Rust test. `cargo xtask rule` checks both against
  `xtask/rule.txt`, the list of what is not Rust yet, which may only
  shrink: CI's `rule` job and `cargo xtask verify --rust` run it, `--write`
  drops what is gone, and an exception is added to the list by hand.
- **Commits:** conventional commits (`feat:`, `fix:`, `docs:`, `chore:`,
  `refactor:`, scoped like `feat(gen):`), with no Co-Authored-By or
  Claude-Session trailers.
- **No model, no service, no key, no agent by name** (above): a change
  that adds a provider, a prompt loop, an in-process model, a tool that
  calls a service, a model setting or an agent-specific affordance is wrong
  by construction, whatever it is for.
- **Edition 2024**, workspace-shared version (`0.4.x`), `resolver = "3"`.
- Related living references and active designs live under `docs/`; see
  `docs/README.md`. Historical designs stay in Git history, not parallel
  archive or roadmap trees. `AGENTS.md` is the MCP and tool catalog.
- **Websites:** `website/` is the Zola site for localgpt.app, the
  family hub and its only docs site: the app's, the CLI's and the format's
  docs plus `website/content/docs/gen`, `docs/verse` and `docs/md` (update
  them here when those modes change: Gen, Verse and MD have no pages of
  their own — the Gen and Verse landing pages went on 2026-11-07, as MD's
  had, and the family strip points at these docs).
  `website-md/` is localgpt.md, the previs page (`docs/world-strategy.md`):
  drop a screenplay — a PDF, a Final Draft `.fdx`, Fountain, or
  Markdown script pages — and get the
  board and the per-scene `.world` packages in the tab. Its draft is
  `crates/previs-web` (wasm-pack over `crates/previs`, which touches no
  model, renderer or network) and its page check (drop the four samples through
  the real input, require the rendered board and the downloads) runs in the
  same `viewer` CI job.
  `website-world/` is localgpt.world: the gallery and the format's only web
  renderer. It is **assembled** — `npm run assemble` copies
  `crates/world-export/js/world-viewer.js` and the `crates/world-types`
  conformance scenes in, and those copies are gitignored, so edit them in the
  crate and re-assemble (the crate file itself is vendored from the
  `openworldformat` npm package via `cargo xtask sync-viewer` — the renderer's
  upstream is that repository). Its `npm run check` (CI job `viewer`) is the only
  automated check on that renderer; it lived in a separate repository until the
  copy there drifted 185 lines behind. The curated worlds' shrunk GLBs and
  music are LFS objects in `localgpt-world-assets` under `web/`, not tracked
  here.
  Every family site carries the same family strip, in this order: LocalGPT,
  Gen, Verse, MD (those three point at their docs here), localgpt.md,
  localgpt.world, localgpt.rs; a site leaves itself out.
