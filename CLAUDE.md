# CLAUDE.md

Guidance for coding agents working in this repository. `AGENTS.md` is the
catalog of Gen's MCP tools; `docs/README.md` indexes the living design
documents.

## What this is

LocalGPT is the desktop app where a prompt, a document, a song or a
screenplay becomes a walkable `.world`, with the person's own coding agents
in its terminals. The workspace holds the app, Gen (the Bevy world builder
and its MCP tools), Document, Verse and Previs (a document, a song and a screenplay
as worlds), the `localgpt` CLI (a `.world` package's command line, the MCP
shim and `localgpt previs`), the crates around the Open World Format, and
three websites.

**No language model, no service, no key, no agent by name.** Nothing in the
workspace loads a language model, calls a model API or a generation service,
reads a key or a model's configuration, or treats one coding agent
differently from another. Agents come from their makers: they run in the
app's terminals (plain login shells) and in MCP clients, under their own
logins, and reach the world over MCP or its ops API. What a model should
author (a document's places, a song's recipe) it writes into a sidecar
through the world's files and API. A change that adds a provider, a prompt
loop, a language model, a tool that calls a service, a model setting, an
agent launcher or an agent-specific affordance (presets, turn detection from
an agent's output) is wrong by construction, whatever it is for.

The models that do run analyse audio, on the CPU, and none is installed by
default: the **audio models** a person installs from the app's bar or with
`localgpt audio install` (`crates/audio-models`: stems, notes, tags and
match), which an agent then gets as tools, and Verse's opt-in `ml` feature
(CLAP moods over the same `match` model, Demucs stems). They turn sound into
what an agent can read and place: files, notes, labels, scores. CLAP's text
side embeds a description to compare with audio; nothing writes or chats.

The assistant (chat, daemon, memory, providers, `config.toml`,
`localgpt-core`) was removed on 2026-11-01; 0.4.2 is its last release. Text
that still describes it is stale.

## Commands

`cargo xtask` is the one entry point for tooling (`xtask/`, in Rust); run
alone, it lists every command.

```bash
cargo xtask verify                  # the CI checks on this machine (--rust --bevy --features --web --deps)
cargo xtask build app gen           # cargo build by name: cli app gen previs doc verse all
cargo xtask rule                    # the all-Rust rule (Conventions)
cargo xtask fetch-assets            # the 522 MB CC0 pack, once per machine, from a sibling
                                    # localgpt-world-assets checkout (or LOCALGPT_WORLD_ASSETS_SRC);
                                    # symlinks it into doc's and verse's asset roots
cargo xtask bundle --dmg            # LocalGPT.app for macOS
cargo xtask deploy world --dry-run  # the sites: app md world, or all

cargo build                         # default-members is the CLI alone; --workspace builds everything,
                                    # --release is LTO and stripped (slow)
cargo test --workspace              # or -p localgpt-gen; `-- --nocapture` shows stdout
cargo clippy --workspace -- -D warnings   # both required before a commit
cargo fmt --check
```

```bash
# The CLI: a .world package's command line, the MCP shim, and previs
cargo run -- world init demo.world  # also submit, undo, redo, log, tips, show, get, verify, history;
                                    # screenshot, selection, goto and proposals need an open app
cargo run -- world mcp              # a stdio MCP server over an app's open canvas
cargo run -- previs script.fdx --out previs/   # a screenplay → per-scene .world packages, shot list and board
cargo run -- audio list             # the optional audio models; also install, remove, mcp,
                                    # and stems, notes, tags and match on a file

# Gen: the world builder. An agent outside the window drives it
cargo run -p localgpt-gen -- --mcp-relay           # the window, with the relay an agent's
                                                   # `mcp-server --connect` reaches
cargo run -p localgpt-gen -- mcp-server            # the agent's own process: window + tools (stdio)
cargo run -p localgpt-gen -- mcp-server --headless --mcp-http 8765   # no window; also HTTP at /mcp
cargo run -p localgpt-gen -- --live demo.world     # a package as a live canvas (ops API)
cargo run -p localgpt-gen -- --world castle        # a saved world, a manifest or a URL
cargo run -p localgpt-gen -- -s model.glb          # an existing scene (-v: verbose logging)
cargo run -p localgpt-gen -- --host                # host a collaborative session (mDNS, port 9879)
cargo run -p localgpt-gen -- --join 192.168.1.5    # join one (bare --join browses)
cargo run -p localgpt-gen -- --replay castle       # a session as a time-lapse

# The desktop app: Gen's world in the shell's panels, with plain terminals
(cd crates/app/ui && npm ci && npm run build)      # a paired phone's page, once (node 22); the
                                                   # binary carries it, a debug build reads it from disk
cargo run -p localgpt-app                          # reopens what was opened last
cargo run -p localgpt-app -- --doc notes.md         # a document; the world rebuilds as it is edited
cargo run -p localgpt-app -- --song track.mp3      # a song as a world that performs it
cargo run -p localgpt-app -- --world harbour.world # a package as a live canvas: agents edit through
                                                   # its API, the history panel draws the branches
cargo run -p localgpt-app -- gen mcp-server        # Gen's CLI/MCP entry point inside the app binary
                                                   # (`localgpt gen …` forwards here)
LOCALGPT_APP_REMOTE=on cargo run -p localgpt-app   # also serve a phone on this network: prints a
                                                   # pairing link (port 9880, LOCALGPT_APP_REMOTE_PORT)
LOCALGPT_APP_SHELL_DRIVE='4:click 0.5 0.5;6:shot /tmp/s.png;7:quit' \
  cargo run -p localgpt-app                        # the shell by a script, a check without a hand
                                                   # (shell/drive.rs: click, tap, key, drag, swipe,
                                                   # wheel, type, shot, quit)
(cd crates/app/ui && npm run design)               # the phone page's design sheet

# Document and Verse on their own
cargo run -p localgpt-doc -- path/to/doc.md         # default samples/hello.md; rebuilds on save and
                                                   # applies the sidecar an agent wrote beside it
cargo run -p localgpt-verse                        # --features ml: CLAP moods and Demucs stems (ONNX)
```

### Releasing

`docs/releasing.md` has the steps. cargo-release with one shared version:
pass `--workspace` to every step that selects packages (`version`,
`publish`, `tag`, `push`), because the root's `default-members` is the CLI
alone. A signed `vX.Y.Z` tag publishes the crates (`release.yml`); a
`desktop-vX.Y.Z` tag builds `LocalGPT.app` and drafts its release
(`desktop.yml`).

## Architecture

### Workspace

Sixteen crates and `xtask`. Document and Verse are members, not separate
repositories, so the world format cannot drift between apps.

```
crates/
├── cli/          # localgpt — `world` (a .world package's command line), `world mcp` (the shim), `previs` (screenplay → board and .world packages), `completion`
├── gen/          # localgpt-gen — the world builder: Bevy window, 88 tools over MCP (stdio, HTTP, the relay), live canvas, sessions
├── app/          # localgpt-app — the one desktop app: Gen's world, documents and songs as modes; its interface is the egui shell in src/shell, and the page in ui/ is a paired phone's
├── doc/          # localgpt-doc — a Markdown file as a walkable world (pure lib + Bevy bin)
├── verse/        # localgpt-verse — a 3D world for every song (lib + Bevy bin)
├── previs/       # localgpt-previs — a screenplay (Fountain, Final Draft .fdx, PDF or Markdown pages) → per-scene .world packages and a board, deterministic (lib)
├── previs-web/   # localgpt-previs-web — previs's draft as WASM for the localgpt.md page
├── world-types/  # localgpt-world-types — serde-only world data model over the `openworldformat` crate (no Bevy)
├── world-bevy/   # localgpt-world-bevy — the one Bevy mapping of the format
├── world-audio/  # localgpt-world-audio — the one mapping of the format's audio (kira + FunDSP)
├── world-export/ # localgpt-world-export — the web viewer (three.js) and HTML export, no Bevy
├── world-sync/   # localgpt-world-sync — room authority: shared doc, EditOps, presence
├── world-editor/ # localgpt-world-editor — the non-linear editing model: tips, forks, folds, deltas (no engine, no I/O)
├── world-agent/  # localgpt-world-agent — the live authority, headless commands, packages, `mcp` (Tool trait + stdio/HTTP servers), `agent_env`, `paths`
├── audio-models/ # localgpt-audio-models — the optional audio models: catalog, installer, stems/notes/tags/match over ONNX Runtime, their MCP tools
└── relay/        # localgpt-relay — internet reach for collaborative rooms

apps/app-desktop/ # the LocalGPT.app bundler (Developer ID + notarization when given)
xtask/            # the repo's tooling: `cargo xtask <command>`
website/          # localgpt.app — Zola; the family hub and the only docs site
website-md/       # localgpt.md — drop a screenplay, get a previs board in the tab (WASM draft)
website-world/    # localgpt.world — the gallery and the one web viewer
```

```
localgpt (CLI)  → world-agent (features: mcp), world-editor, previs (features: pdf), audio-models (features: mcp)
localgpt-gen    → world-agent (features: mcp-http), previs, world-*
localgpt-app    → gen, doc (lib only), verse, previs, world-agent, world-bevy, world-types, audio-models (installer only)
doc, verse      → world-agent (paths), world-*; verse (ml) → audio-models (features: run)
previs, relay, world-*  → no app crate
```

**One format, one mapping per output.** `localgpt-world-types` is the source
format (serde-only; the `schema` feature writes `world.schema.json`).
`localgpt-world-bevy` is the only place a `Shape`, `MaterialDef`, `LightDef`
or `EnvironmentDef` becomes Bevy components; `localgpt-world-audio` is the
only place the format's audio becomes sound; `localgpt-world-export` holds
the only web renderer (`js/world-viewer.js` and the `js/cinematography.js`
it imports) and `generate_html`. Gen, Document and Verse all go through them. The
renderer's two files are vendored snapshots of the `openworldformat` npm
package: refresh them with `cargo xtask sync-viewer`, never edit them by
hand. Add fields to world-types, never a parallel format; what the format
means is decided in the Open World Format repository. Reference scenes live
in `crates/world-types/conformance/`: render them before and after changing
a mapping.

### Feature flags

| Crate | Feature | Default | Purpose |
|-------|---------|---------|---------|
| `world-agent` | `mcp` | no | The `Tool` trait, `ToolSchema`, `McpHandler` and the stdio MCP server (the CLI's shim) |
| `world-agent` | `mcp-http` | no | The streamable HTTP transport over `mcp` (axum + tower-http); Gen enables it |
| `gen` | `physics` | yes | avian3d rigid bodies, colliders, joints, forces, gravity |
| `gen` | `multiplayer` | yes | `--host`/`--join`, the ops room, mDNS, the browser guest page |
| `gen` | `character-controller` | no | The bevy-tnua player controller (needs `physics`); builds, not played since Bevy 0.18 |
| `doc` | `app` | yes | The Bevy bin; the lib builds without it, which is how `crates/app` takes it |
| `verse` | `ml` | no | CLAP moods over the shared `match` model, and Demucs stems (`stem-splitter-core`) |
| `world-types` | `schema` | no | Writes `world.schema.json` |
| `cli` | `audio` | yes | `localgpt audio`, and the installed models' tools in `world mcp`; links ONNX Runtime |
| `audio-models` | `run` | no | The models themselves (`ort`, decoding, resampling); without it the crate is the catalog and installer the app takes |
| `audio-models` | `mcp` | no | The models' tools (`audio_stems`, `audio_notes`, `audio_tags`, `audio_match`) |

### Key patterns

**Tools.** Every Gen tool implements `localgpt_world_agent::mcp::Tool`
(`name`, `schema`, `execute`, optional `annotations`).
`mcp_server::create_mcp_tools` assembles them and annotates each (read-only
/ destructive / idempotent). Tools reach the Bevy world through `GenBridge`
(channels into the ECS), and the same set serves stdio, HTTP and the relay.
`world-agent::mcp` holds `McpHandler` (initialize, tools/list, tools/call;
protocol `2024-11-05`), `ToolHandler` over a tool list and
`run_mcp_stdio_server` (JSON-RPC 2.0, one message per line on stdout; logs
go to stderr); `mcp::http` is `run_mcp_http_server`, at `/mcp` on localhost.
When a tool is added, removed or renamed, update `AGENTS.md` and the count
wherever it is quoted (`git grep -nw 88`).

**Bevy main thread.** Bevy must own the main thread (macOS windowing/GPU).
Gen spawns tokio on a background thread for the tool server. In the desktop
app eframe owns the window and Bevy renders into textures the egui panels
show (`docs/rfcs/authoring/all-rust.md`).

**Audio.** `localgpt-world-audio` is the one mapping from the format's audio
types to sound, and Gen and Verse both run on it. kira owns the device and
FunDSP is a source — different layers, not alternatives — so the crate is
one `AudioManager` with the synthesis graphs feeding it through
`FundspSound`. `graphs::build` takes `wt::AudioSource` and is pure, so it
tests without an audio device. Gen's Bevy side (`gen3d/audio.rs`) keeps
metadata and forwards listener and emitter transforms to the thread that
owns the engine; emitters are kira spatial tracks, so attenuation and
panning come from positions. Verse keeps kira directly for its own playback
layer. See `docs/gen/gen-audio.md`.

**Directories and the shared pack.** `world-agent::paths` is where every
directory comes from: `Dirs` (`data`, `state`, `logs()`) and
`world_pack_dir()`, which resolves the CC0 asset pack so one download serves
Gen, Document and Verse. Never re-implement the XDG rule in an app.

**Environment names** are constants in the crate that reads them, never bare
strings: the world's in `world-agent::agent_env` (`LOCALGPT_WORLD_DIR`,
`_API`, `_TOKEN`, `_AUTHOR`), the directories' in `world-agent::paths`,
Gen's session PIN in `crates/gen/src/net/mod.rs`, the app's in
`crates/app/src` (`LOCALGPT_APP_*`), the audio models' in
`audio-models::install` (`LOCALGPT_AUDIO_MODELS`).

**Audio models.** `localgpt-audio-models` is where a model runs. A model is
a catalog entry (each file pinned by a URL at a fixed revision, its SHA-256
and size; its licence, code and weights apart) and a module behind `run`;
the installer keeps a file only when its hash matches. `tools::installed`
offers a model's tool only once it is installed, and an MCP server lists its
tools when it starts, so an agent restarts its server after an install. A
new model is all of: the entry, the module, its arm in `tools::tool_for`, a
`localgpt audio` command, and a test against the installed model
(`#[ignore]`, with `LOCALGPT_AUDIO_MODELS` pointing somewhere disposable).
Weights whose licence is non-commercial or unclear stay out: Demucs's are
unclear, which is why the agent's stems are Spleeter's.

### The CLI (`crates/cli/src/cli/`)

`world`, `previs`, `audio` and `completion` are the whole binary: no chat,
no config, no key. `audio` lists, installs and removes the audio models,
runs each on a file and serves the installed ones' tools (`audio mcp`;
`world mcp` adds them too, writing into the world's `assets/`). `world` is init, submit (`--at <id>` forks), undo, redo, log, tips,
show, get, verify and history, plus screenshot, selection, goto and proposals,
which need an app, and `world mcp`, a stdio MCP server over an app's open
canvas (`world.rs`, `world_mcp.rs`). `previs` stages a screenplay (`.fountain`,
`.fdx`, `.pdf` or `.md`) into per-scene `.world` packages, `shotlist.csv` and
`board.html` (`previs.rs`).

One authority at a time. While an app answers at `.live/endpoint.json`,
every command is a call to its API (`world_api.rs`, which the shim shares)
and the CLI never opens the package; with nothing open it commits through
`world-agent::headless`. Both hand back `live::receipt`. The folder is the
first argument, else `$LOCALGPT_WORLD_DIR` (the app's terminals set it),
else the world the shell is in. The head is `refs.main` (else the last
entry): read it with `LiveWorld::main_tip`, never as "the last line" — a
branch commit is always the last line.

### Gen (`crates/gen`)

A Bevy window with procedural audio and no prompt, chat or model in it. It
is driven from outside: `--mcp-relay` serves a relay beside the window that
an agent's `mcp-server --connect` process is the stdio end of
(`gen3d/mcp_relay.rs`); `mcp-server` alone is the agent's own process with a
window, `--headless` without one, and `--mcp-http <port>` adds the HTTP
transport; `--live <folder>` opens a package as a live canvas with its ops
API (`gen3d/live.rs`). Started with none of these, the window says so and
shows what was opened last. `config::GenConfig { workspace, state_dir }`
comes from `world-agent::paths::Dirs`, and `settings.rs` remembers only what
was opened last.

World data is `localgpt-world-types`, so the same types serialize to RON for
local saves and travel as ops in a room.

- **Worlds as skills.** `gen_save_world` and `gen_load_world` write and read
  a directory: `SKILL.md` + `world.ron` (entities, behaviors, audio, the
  avatar and tours inline).
- **Behaviors** are declarative, eight kinds in world-types (`orbit`,
  `spin`, `bob`, `look_at`, `pulse`, `swing`, `path_follow`, `bounce`), and
  they stack: each frame starts from the authored transform and applies them
  in order, as the format says. A kind the build does not know is carried,
  never run.
- **WorldGen** is blockout-first (`gen_plan_layout` → `gen_apply_blockout` →
  `gen_populate_region`), with a grid navmesh, hero/medium/decorative
  placement and a screenshot-based evaluation loop. The image-input tools
  are pure image analysis; no tool calls a service.
- **Audio.** Ambient layers and spatial emitters, inferred from entity names
  ("campfire", "waterfall") when not given; see Key patterns.

**Multiplayer** (the `multiplayer` feature) is collaborative rooms over the
ops protocol. `--host` runs the room authority (`localgpt-world-sync`: the
shared document, validated `EditOp`s, revisions, presence) with a session
server (join page + WebSocket `/session`, port 9879) and an mDNS
announcement; `--join` is the native client (`net/ops_client.rs`), and
browser guests use the join page every hosted session serves (`--web`
prints the invite link). The host projects its scene into ops, so every
tool syncs, and committed ops apply back into its scene. No model runs for
the room: a guest's prompt is accepted by the authority and failed with a
reason (`net/web.rs`), and the agent that builds is the host's, outside the
window. A session is a package under `<workspace>/sessions/<name>/` (the
base `world.ron`, the one log `ops.jsonl`, `session.json`, snapshots), which
`--resume` and `--replay` read. A room lasts as long as its host's window:
`--host` takes no subcommand, so there is no headless host, and the relay
(`crates/relay`) only forwards frames for a host behind NAT, holding no
state. A persistent room is not built, and there is no cloud module.
`docs/gen/multiplayer.md` is the reference, and `docs/rfcs/multiplayer/`
holds the designs.

### A phone or a tablet

No native app: the desktop app serves its own page to a paired device's
browser (`crates/app/src/webui/remote.rs`, `crates/app/ui/src/phone/`;
`docs/rfcs/authoring/mobile-remote.md`). A phone gets three views; a tablet
the world and the terminals at once, beside or over each other
(`phone/layout.tsx`: by the page's width, never its height, which the
keyboard takes). `scripts/remote-smoke.mjs` plays a device against a running
app, and with `--phone` drives its page at an iPhone's and an iPad's size.

## Configuration and directories

There is no configuration file. What an app needs it takes from flags and
environment variables, and it remembers only what was opened last
(`gen-settings.json`) and, in the desktop app, where its panels were
(`app-layout.json`). An agent's own configuration (its login, its model, its
MCP servers) is the agent's.

```
~/.local/share/localgpt/                 # XDG_DATA_HOME (Dirs::data); LOCALGPT_DATA_DIR moves it
├── gen-workspace/                       # Gen's worlds (skills/), sessions/, exports/
├── models/pack/                         # the shared CC0 pack (cargo xtask fetch-assets);
│                                        # LOCALGPT_WORLD_ASSETS overrides, and it ignores the profile
└── models/audio/<id>/                   # the installed audio models; LOCALGPT_AUDIO_MODELS moves them
~/.local/state/localgpt/                 # XDG_STATE_HOME (Dirs::state); LOCALGPT_STATE_DIR moves it
├── gen-settings.json                    # what was opened last
├── app-remote.json                      # the phone remote's paired devices (key hashes, 0600)
├── app-layout.json                      # the panels as a hand left them (tiles, hidden, folded)
└── logs/                                # application logs, when there is no terminal
```

`LOCALGPT_PROFILE=x` moves both roots to `localgpt-x`. Worlds a 0.4.x
assistant saved under `workspace/skills/` are copied into
`gen-workspace/skills/` once (`config::migrate_legacy_worlds`).

## Conventions

- **Lint clean before committing:** `cargo clippy --workspace -- -D warnings`
  and `cargo fmt --check` must pass. After a Rust change, run `cargo check`
  and fix every error before reporting it done. Never edit Rust files with
  `sed`.
- **Rust above the operating system** (`docs/rfcs/authoring/all-rust.md`):
  nothing new in another language — no JavaScript, TypeScript, Python or
  shell — and no new crate depending on `wry`. A script is an `xtask`
  command, a check a Rust test. `cargo xtask rule` checks both against
  `xtask/rule.txt`, the list of what is not Rust yet, which may only shrink:
  `--write` drops what is gone, and an exception is added to the list by
  hand.
- **Commits:** conventional commits (`feat:`, `fix:`, `docs:`, `chore:`,
  `refactor:`, scoped like `feat(gen):`), with no Co-Authored-By or
  Claude-Session trailers.
- **Public and Apache-2.0.** Never name a closed-source sibling project or
  its domains in code, UI text, docs or commit messages; say "connected 3D
  app". Never copy AGPL or GPL code into the tree: study it, then implement
  independently.
- **Edition 2024**, one workspace-shared version, `resolver = "3"`.
- **Docs.** `docs/` holds only living references and active designs
  (`docs/README.md` has the rule): update a document in the change that
  outdates it, and delete it, with its inbound links, when it has no job
  left. History stays in Git and `CHANGELOG.md`, not in archive trees or
  status caveats. User documentation is `website/content/docs/`, including
  `gen/`, `verse/` and `md/`: update it when a mode's commands, keys or
  output change.

### CI (`.github/workflows/`)

`ci.yml` runs on push and pull request to main, on Linux; `cargo xtask
verify` runs the same checks on this machine. Its jobs: `fmt`; `rule`
(`cargo xtask rule`); `clippy` and `test` for the workspace minus the six
crates that need audio and windowing headers (`app`, `gen`, `md`, `verse`,
`world-bevy`, `world-audio`), which the `bevy` job lints and tests;
`features` (`cargo check -p localgpt-verse --features ml`, since nothing
else compiles gated code); `interface` (typecheck and build of the phone's
page and its design sheet); `viewer` (`cargo xtask sync-viewer --check`, the
headless render check of `website-world`'s viewer and the localgpt.md page
check — the only automated checks on the format's web renderer); `deps`
(cargo-deny, `deny.toml`) on every change, and `advisories` on main and
weekly, never on pull requests. `website.yml` builds `website/` and checks
its internal links when it changes.

### Websites

One Cloudflare Worker each, deployed by hand with `cargo xtask deploy
<app|md|world>`.

- **`website/`** — localgpt.app (Zola): the family hub and the only docs
  site. Gen, Verse and Document have no pages of their own; their docs are
  `website/content/docs/{gen,verse,document}`. See `website/CLAUDE.md`.
- **`website-md/`** — localgpt.md, the previs page: drop a screenplay, get
  the board and the per-scene `.world` packages in the tab. Its draft is
  `crates/previs-web` (wasm-pack over `crates/previs`), put together by
  `cargo xtask assemble md`. See `website-md/README.md`.
- **`website-world/`** — localgpt.world: the gallery and the format's only
  web renderer. It is **assembled**: `cargo xtask assemble world` copies
  the viewer and the conformance scenes in from the crates, and those
  copies are gitignored, so change them in the crate and re-assemble. See
  `website-world/CLAUDE.md`.

Every family site carries the same family strip, in this order: LocalGPT,
Gen, Verse, Document (those three point at their docs on localgpt.app),
localgpt.md, localgpt.world, localgpt.rs; a site leaves itself out.
