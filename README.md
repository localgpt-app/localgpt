# <img src="https://localgpt.app/logo/localgpt-icon-app.svg" width="50" height="50" alt="LocalGPT" /> LocalGPT

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](https://github.com/localgpt-app/localgpt#license)
[![Crates.io](https://img.shields.io/crates/v/localgpt.svg)](https://crates.io/crates/localgpt)
[![Downloads](https://img.shields.io/crates/d/localgpt.svg)](https://crates.io/crates/localgpt)
[![Docs](https://docs.rs/localgpt/badge.svg)](https://docs.rs/localgpt/latest/localgpt)
[![CI](https://github.com/localgpt-app/localgpt/workflows/CI/badge.svg)](https://github.com/localgpt-app/localgpt/actions)
[![Discord](https://img.shields.io/discord/691052431525675048.svg?label=&logo=discord&logoColor=ffffff&color=7389D8&labelColor=6A7EC2)](https://discord.gg/spKRr6mRyp)

**An AI agent that lives where your data lives, and a desktop app where a prompt, a document, a song or a screenplay becomes a `.world` you can walk through.** A world is a package with a history: people edit it in the window, agents edit it from outside, and every change is logged, attributed, undoable and branchable. Open source, Apache 2.0, runs on your machine.

[![LocalGPT Gen Demo](https://img.youtube.com/vi/R__tg7YY0T8/maxresdefault.jpg)](https://www.youtube.com/watch?v=R__tg7YY0T8)

## What is in this repository

| | Where | What |
|---|---|---|
| **LocalGPT**, the desktop app | `crates/app` | One window, every mode: a world from a prompt (Gen), from a Markdown document (MD) or from a song (Verse), or any `.world` package opened as a live canvas. |
| **LocalGPT Gen**, the binary | `crates/gen` · `cargo install localgpt-gen` | The world builder on its own: interactive, an MCP server for Claude Desktop, Claude Code, Codex, Gemini CLI and editors, headless batch generation, collaborative sessions on the LAN or through a relay. |
| **Previs** | `crates/previs` · `cargo install localgpt-previs` · [localgpt.md](https://localgpt.md) | A screenplay in — PDF, Final Draft, Fountain or Markdown — and per scene a `.world` with the set, the cast and the coverage, plus a printable board and a shot list. Deterministic, no model; the same code runs in the browser tab. |
| **The assistant** | `crates/cli` · `cargo install localgpt` | Chat, TUI, daemon, persistent Markdown memory, heartbeat and cron, a kernel-enforced shell sandbox, an HTTP API, an MCP server — and `localgpt world`, a `.world` package's command line. **0.4.2 is the assistant's last release:** 0.5.0 removes it and keeps the world commands, adding `doctor` and `setup` ([why](docs/rfcs/authoring/the-harness-goes.md)). |
| **The world crates** | `crates/world-*` | `world-types`, `world-bevy`, `world-audio`, `world-export`, `world-sync`, `world-editor`, `world-agent`: one format for every app, over the [Open World Format](https://openworldformat.org) reference crate. |
| **The sites** | `website*/` | [localgpt.app](https://localgpt.app) (hub and the only docs site) · [gen.localgpt.app](https://gen.localgpt.app) · [verse.localgpt.app](https://verse.localgpt.app) · [localgpt.md](https://localgpt.md) (previs) · [localgpt.world](https://localgpt.world) (gallery and web viewer) · [localgpt.rs](https://localgpt.rs) (devlog) |

## Install

```bash
# From crates.io
cargo install localgpt-gen       # the world builder: MCP server, headless generation, sessions
cargo install localgpt-previs    # a screenplay to a previs board, on the command line
cargo install localgpt           # `localgpt world` and the shim; the assistant until 0.5.0 (pin 0.4.2 to keep it)

# From source — the desktop app ships as a build, not a crate
git clone https://github.com/localgpt-app/localgpt.git
cd localgpt
cargo run -p localgpt-app                             # reopens what was opened last
cargo run -p localgpt-app -- --md notes.md            # a document as a world
cargo run -p localgpt-app -- --song track.mp3         # a song as a world that performs it
cargo run -p localgpt-app -- --prompt "a quiet harbor at dusk"
cargo run -p localgpt-app -- --world scene-1.world    # a package, opened as a live canvas
apps/app-desktop/macos/build-app.sh                   # LocalGPT.app for macOS

# The two shared downloads, once per machine, read by every app
scripts/fetch-assets.sh    # the 522 MB CC0 asset pack (Poly Haven models)
scripts/fetch-model.sh     # the ~5 GB GGUF for the in-process local-model tier (optional)
```

Rust stable (edition 2024, so 1.85 or newer). On Linux the desktop crates need Bevy's system packages: `libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev libfontconfig1-dev libdbus-1-dev pkg-config`. Full options, feature flags and headless builds: [Installation](https://localgpt.app/docs/installation/).

## The desktop app: one window, every mode

`localgpt-app` is Gen's shell — a Bevy viewport, a prompt panel, an agent loop — with documents, songs and packages as inputs. **Open…** at the top of the window takes any of them; the launch flags above do the same.

- **A prompt.** Type in the panel (**F2**), watch the reply stream in with each tool call as it runs. The model menu lists what this machine can use right now: installed CLI backends (Claude CLI, Gemini CLI, Codex), models pulled into a local Ollama, and every GGUF in the shared model folder, run in-process when built with `local-llm` (`local-llm-metal` on Apple Silicon). The app reads no `config.toml`; the choice is remembered in Gen's own settings.
- **A document.** An editor pane (**F3**) beside the viewport. The file saves and the world rebuilds shortly after typing stops, in place, changing only the sections whose text changed. The app's model authors each section's place in the background; the cursor moving into a section flies the camera there, and picking a thing in the world selects it in the inspector (**F1**). Builds are cached beside the document, so a restart reopens them without a model call.
- **A song.** Verse's on-device analysis — tempo, a beat grid, sections, an energy curve, a mood — picks and furnishes the world; the soundtrack plays on the shared mixer and the world's modulations follow it. No model, no key, no upload.
- **A package.** A `.world` folder opens as a live canvas: the window holds the authority and serves the folder's API, and agents edit from outside (next section).
- **Together.** Host or join a collaborative session from the panel's Collaborate section; guests' prompts run on a scene-only agent.

Docs: [The desktop app](https://localgpt.app/docs/app/). `localgpt-gen` stays a binary of its own for the MCP server, headless generation and `--host`/`--join`; there is one desktop bundle, `LocalGPT.app`, built by `apps/app-desktop/`.

## A world is a package, and agents edit it from outside

A `.world` is a folder: `manifest.json` is the world now, `ops.jsonl` is every change ever made to it, `assets/` holds meshes, textures and sounds by content hash, and `AGENTS.md` tells an agent how to change it. The format is the [Open World Format](https://openworldformat.org): the state at any revision is a fold of the log over the base, and `manifest.json` is that fold at the head, written down. Rendering, editing, undo, branches, replay and multiplayer are reads over one history.

Open one as a live canvas. No agent runs in the window:

```bash
localgpt-gen --live scene-1.world      # or: localgpt-app --world scene-1.world
# [live] scene-1.world · revision 0 · API http://127.0.0.1:57822 (see scene-1.world/AGENTS.md)
```

Any agent that can read the folder can change the world — Claude Code, Codex, a script, `localgpt` itself. It sends a batch of ops to the API in `.live/endpoint.json`; the batch commits whole, or is refused with a reason per op that the agent can act on:

```bash
URL=$(jq -r .url scene-1.world/.live/endpoint.json); TOKEN=$(jq -r .token scene-1.world/.live/endpoint.json)
curl -s -X POST "$URL/ops" -H "Authorization: Bearer $TOKEN" -d '{
  "author": "claude", "message": "a lantern by the door",
  "ops": [{"SpawnEntity": {"entity": {"name": "lantern",
            "transform": {"position": [2.0, 0.5, 1.0]},
            "shape": {"Cylinder": {"radius": 0.1, "height": 1.0}},
            "material": {"emissive": [4.0, 2.8, 1.2, 1.0]}}}},
          {"ModifyEntity": {"id": "bench", "patch": {"transform": {"position": [0.0, 0.0, 3.0]}}}}]}'
```

The cheapest way to put an MCP agent on it is the shim, which serves the open canvas's tools over stdio:

```bash
localgpt world mcp scene-1.world
claude mcp add scene -- localgpt world mcp scene-1.world     # then ask Claude Code to change the world
```

- **People edit too.** Handles in the viewport (move, turn, scale, with snapping), an inspector that edits every field of the selected entity, an outliner that reparents by drag. Every gesture is one batch through the same authority, one entry in the history, undone with Cmd+Z. Undo is per author, with redo. With `--review`, an agent's batch shows on the canvas before it commits and you accept or reject it.
- **History is a tree.** A batch sent `at` an earlier entry starts a branch while the head stays put. The history rail draws every branch; **H** opens the history panel — the log as a graph with a scrubber and playback, so you watch the world being made. `POST /goto` moves the canvas to any point without writing anything. `localgpt-gen --replay` plays a session back as a time-lapse.
- **Git, if you want it.** `localgpt world init demo.world --git` makes every batch a commit by whoever sent it; a checkout under the open app is followed; `verify` checks that folding the log gives the manifest.
- **Guards.** A direct write to `manifest.json` is put back and noted; a batch may carry the revision it was decided on and is refused if the world moved; a name nothing has refuses the batch, naming it.

The headless half is `localgpt world` — the same authority with no window, for scripts and CI:

```bash
localgpt world init demo.world --git
localgpt world submit demo.world batch.json --author me --message "lift the cube"
localgpt world submit demo.world batch.json --at <entry-id>     # an earlier entry: a branch
localgpt world log demo.world        # every committed batch, oldest first
localgpt world tips demo.world       # every branch end
localgpt world undo demo.world --author me
localgpt world verify demo.world     # the fold still equals the head
localgpt world history demo.world    # the git history, when the world is in git
```

Docs: [Live canvas](https://localgpt.app/docs/gen/live-canvas/) · [`localgpt world`](https://localgpt.app/docs/cli-commands/#world) · the decisions: [`docs/rfcs/authoring/`](docs/rfcs/authoring/).

## Previs: a screenplay becomes a board, the board becomes a canvas

The first audience is film previsualization: people who previs today by hand, or with a generator that cannot measure. Greybox is the norm there, iteration is the work, and the deliverables — boards, shot lists — are things a `.world` with a history can produce.

1. **Script → board.** Drop the script on [localgpt.md](https://localgpt.md) — the PDF, a Final Draft `.fdx`, `.fountain`, or Markdown script pages. The page draws the board in the tab and offers `board.html`, `shotlist.csv` (scene, shot, size, lens, sensor, aspect, camera height, distance, in and out) and a zip with one `.world` per scene: a set, a stand-in per speaking character, a master and a single on everyone, with real lenses and measured camera positions. Nothing is installed and nothing is uploaded. The same draft runs on the command line, with no ceiling on scenes:

   ```bash
   localgpt-previs script.fdx --out previs/
   # previs/scene-1.world/  previs/scene-2.world/  previs/shotlist.csv  previs/board.html
   ```

   It is deterministic and model-free: the same pages give the same board, byte for byte, on the page and on the command line, from any of the four file kinds.

2. **Board → canvas.** Open a scene live and look through any shot (click its card, or press 1–9); drag cards to reorder; open the board from the shot strip.

   ```bash
   localgpt-gen --live previs/scene-1.world
   ```

3. **Revise by asking.** Put an agent on the folder (`localgpt world mcp previs/scene-1.world`) and ask: "make 1B a 35", "stay on her", "from the doorway". A revision is an ordinary patch on a camera entity's `ext-cinematography` block. A batch sent at an earlier entry is a treatment; two treatments' boards go side by side with the changed shots marked (`GET /board?tip=<a>&vs=<b>`).

Posing, animatics and a generated look wait until people who previs today have used these two steps on their own pages. Docs: [Previs](https://localgpt.app/docs/gen/previs/) · why previs first: [`docs/world-strategy.md` §15](docs/world-strategy.md) · the observation sheet: [`docs/previs-user-test.md`](docs/previs-user-test.md).

## <img src="https://localgpt.app/logo/localgpt-icon.svg" width="32" height="32" alt="LocalGPT" /> LocalGPT Gen, the binary

`localgpt-gen` builds explorable worlds from natural language with the Bevy engine, and is the piece other tools drive.

```bash
localgpt-gen                                        # interactive: type prompts in the terminal, or press F2 for the panel
localgpt-gen "a desert at dusk, pyramids, a UFO hovering above"
localgpt-gen --world https://localgpt.world/worlds/md-deck.json   # open a world folder, a saved name, a manifest or a URL
localgpt-gen --live demo.world                      # the live canvas (above)
localgpt-gen mcp-server                             # a tool server for Claude Desktop, Claude Code, Codex, Gemini CLI, VS Code, Zed, Cursor
localgpt-gen headless --prompt "a cozy cabin in a snowy forest"   # no window: batch runs, CI, the experiment queue
localgpt-gen --host --web                           # host a session; friends join from a browser with the invite link
localgpt-gen --join 192.168.1.5 --pin 482913        # or from their own Gen window
localgpt-gen --host --relay https://relay.example   # reach guests beyond the LAN
```

- **What it builds.** Parametric shapes (box, sphere, cylinder, cone, capsule, plane, torus, pyramid, wedge and more), PBR materials, point/spot/directional lights, sky and fog, glTF meshes, data-driven behaviors (orbit, spin, bob, look-at, pulse, path-follow, bounce), procedural audio (ambient wind, rain, forest, ocean, cave, stream; spatial emitters), NPCs with dialogue, triggers and collectibles, terrain and water, optional physics, and a blockout-first WorldGen pipeline with a navmesh and a screenshot evaluation loop.
- **Tools.** 77 `gen_*` tools in three profiles — `core` (24, for small local models), `standard` (49), `full` — selected with `--tools`. MCP clients always get the full set.
- **Three ways to drive it.** LocalGPT's own agent with an API key or a local model; a CLI backend (Claude CLI, Gemini CLI, Codex) whose tool calls reach your window through the MCP relay, so no API key is needed; or an external app that runs `localgpt-gen mcp-server` and orchestrates the conversation itself.

  ```json
  { "mcpServers": { "localgpt-gen": { "command": "localgpt-gen", "args": ["mcp-server"] } } }
  ```

- **Together.** One machine hosts and owns the world; others join from their own Gen window or from a browser, walk the same world live, chat, and ask the host's AI to build where they are looking. Guests' prompts run on a scene-only agent unless the host says otherwise; `--web-edit` lets browser guests move things directly; `--relay` reaches guests beyond the LAN through `localgpt-relay`, a small binary that forwards frames and never holds world state. Every session is a package — a base world plus one log — with per-user undo, `--resume` and `--replay`.
- **Export.** `.world` packages and `world.ron` skills, glTF/GLB for Blender, Unity and Unreal, a self-contained HTML page with the same three.js viewer [localgpt.world](https://localgpt.world) serves, and screenshots.

Docs: [LocalGPT Gen](https://localgpt.app/docs/gen/) · [MCP Server](https://localgpt.app/docs/gen/mcp-server/) · [Collaborative Sessions](https://localgpt.app/docs/gen/multiplayer/) · [Headless Mode](https://localgpt.app/docs/gen/headless/). Built something? Share it on [Discord](https://discord.gg/spKRr6mRyp) or [YouTube](https://www.youtube.com/@localgpt-gen).

## The assistant

`localgpt` is a local-first AI assistant with persistent memory, autonomous operation and many interfaces. Its job in this family is to be the world's command line and the services other agents use — memory, scheduled work, a sandbox — rather than one more coding agent ([why](docs/rfcs/authoring/the-cli-and-the-world.md)).

```bash
localgpt chat                    # interactive chat
localgpt tui                     # terminal UI
localgpt ask "question"          # one question, one answer
localgpt daemon start            # HTTP API + web UI + heartbeat + cron
localgpt mcp-server              # memory and web tools over stdio, for Claude Code, Codex, Gemini CLI, editors
localgpt doctor                  # diagnose config, keys, providers, MCP servers
```

- **Single binary.** No Node.js, Docker or Python. Everything stays on your machine; there is no telemetry.
- **Memory.** A Markdown workspace — `MEMORY.md`, daily logs, a knowledge bank — indexed with SQLite FTS5 and sqlite-vec with local embeddings; active recall before a reply; background consolidation of transcripts into memory; a wiki of claims and evidence with staleness tracking.
- **Autonomy.** A heartbeat that works through `HEARTBEAT.md`, cron jobs in a fresh session each, a durable outbox with retry. No coding agent does scheduled unattended work.
- **Defense in depth.** A kernel-enforced shell sandbox (Landlock and seccomp on Linux, Seatbelt on macOS, containers as a backend), `POLICY.md` standing instructions sanitized before injection and protected from self-modification, prompt injection defenses, protected files, tool approval, TLS.
- **Providers.** Anthropic, OpenAI, xAI, Gemini, Vertex AI, GLM, Ollama, LM Studio and any OpenAI-compatible endpoint, plus the Claude, Gemini and Codex CLIs as backends with their own login. Models are named `provider/model`; the default is `claude-cli/opus`.
- **Interfaces.** CLI, TUI, an embedded web UI, a desktop GUI, an OpenAI-compatible HTTP API, iOS/macOS and Android apps over UniFFI, bridge daemons over a secure IPC protocol, MCP in both directions (it consumes external MCP servers and is one).

Workspace layout and XDG paths (`localgpt paths` prints the resolved ones):

```
<workspace>/
├── MEMORY.md     # long-term knowledge, loaded every session
├── HEARTBEAT.md  # the autonomous task queue
├── POLICY.md     # standing instructions
├── memory/       # daily logs
└── knowledge/    # structured knowledge bank
```

Configuration lives at `~/.config/localgpt/config.toml`. A local model through LM Studio, no key, fully offline:

```toml
[agent]
default_model = "openai/qwen/qwen3.5-35b-a3b"

[providers.openai]
api_key = "lm-studio"
base_url = "http://127.0.0.1:1234/v1"
```

Docs: [Introduction](https://localgpt.app/docs/intro/) · [Quick Start](https://localgpt.app/docs/quick-start/) · [Configuration](https://localgpt.app/docs/configuration/) · [CLI Commands](https://localgpt.app/docs/cli-commands/) · [HTTP API](https://localgpt.app/docs/http-api/) · [Shell Sandbox](https://localgpt.app/docs/sandbox/) · [POLICY.md](https://localgpt.app/docs/policy/) · [Memory System](https://localgpt.app/docs/memory-system/).

## The format, and the crates that carry it

Every app reads and writes one format, so a document world, a song world and a drafted set are the same kind of file, and a `.world` made here opens in any viewer of the [Open World Format](https://openworldformat.org) — the spec, the JSON Schema, the conformance worlds and five reference parse-and-fold packages (npm, PyPI, crates.io, SwiftPM, Kotlin) live in [their own repository](https://github.com/openworldformat/openworldformat). This workspace consumes the `openworldformat` crate and adds LocalGPT's own surface on top:

| Crate | What it is |
|---|---|
| [`localgpt-world-types`](https://crates.io/crates/localgpt-world-types) | The data model: the format's types re-exported unchanged, plus NPCs, behavior libraries, regions and the sync machinery. Serde only; `schema` writes `world.schema.json`. |
| [`localgpt-world-bevy`](https://crates.io/crates/localgpt-world-bevy) | The one Bevy mapping of the format — meshes, materials, lights, environment, camera, modulations — used by every app. |
| [`localgpt-world-audio`](https://crates.io/crates/localgpt-world-audio) | The one mapping of the format's audio to sound: FunDSP graphs as sources inside a kira mixer, ambience, spatial emitters, soundtracks. |
| [`localgpt-world-export`](https://crates.io/crates/localgpt-world-export) | The web side with no Bevy: the three.js viewer (vendored from the format's npm package) and self-contained HTML export. |
| [`localgpt-world-sync`](https://crates.io/crates/localgpt-world-sync) | Collaborative rooms: the shared document, validated ops, the wire protocol and the room authority. No Bevy, no I/O. |
| [`localgpt-world-editor`](https://crates.io/crates/localgpt-world-editor) | The non-linear editing model: tips, forks, folds and deltas, fast enough to scrub. No engine, no I/O. |
| [`localgpt-world-agent`](https://crates.io/crates/localgpt-world-agent) | The live authority (`LiveWorld`), packages, the headless commands behind `localgpt world`, the tool-call protocol an LLM builds with, and the shared paths for the model and asset downloads. |
| [`localgpt-previs`](https://crates.io/crates/localgpt-previs) | Script to board, deterministic. |

Two renderers draw the conformance worlds, Bevy and three.js; `website-world`'s headless render check runs in CI and is the automated check on the web renderer. The rule inside the workspace: add fields to `world-types`, never a parallel format.

## Develop

```bash
cargo build                                   # the CLI (default member)
cargo build --workspace                       # everything
cargo test --workspace
cargo clippy --workspace -- -D warnings       # required before commits
cargo fmt --check
cargo run -p localgpt-md -- notes.md          # MD and Verse still run on their own
cargo run -p localgpt-verse
```

CI runs fmt, clippy and tests, the Bevy crates in their own job, a job for the opt-in feature tiers (`llm`, `ml`, `local-llm`), the headless render check of the web viewer and the previs page, and cargo-deny. Releases publish every crate to crates.io from a signed `vX.Y.Z` tag through Trusted Publishing. [`CLAUDE.md`](CLAUDE.md) is the map of the workspace, [`AGENTS.md`](AGENTS.md) the tool catalog, [`docs/`](docs/) the RFCs, the roadmap and [`world-strategy.md`](docs/world-strategy.md), where the decisions behind the shape of all this are recorded with their dates.

## Devlog

[localgpt.rs](https://localgpt.rs) is the build-in-public log, in Rust: [Why I Built LocalGPT in 4 Nights](https://localgpt.rs/why-i-built-localgpt-in-4-nights/) · [Explorable World as Agent Skill](https://localgpt.rs/world-as-skill/).

## Built With

Rust, Tokio, Axum, Bevy, egui, kira and FunDSP, three.js, SQLite (FTS5 + sqlite-vec), fastembed, mistral.rs, wasm-bindgen, Zola, Cloudflare Workers

## Contributors

<a href="https://github.com/localgpt-app/localgpt/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=localgpt-app/localgpt" />
</a>

## License

[Apache-2.0](LICENSE)
