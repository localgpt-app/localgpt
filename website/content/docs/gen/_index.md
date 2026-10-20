---
title: "LocalGPT Gen"
---

# LocalGPT Gen

**LocalGPT Gen** builds explorable worlds from natural language — geometry, materials, lighting, behaviors, audio, and camera — on [Bevy](https://bevyengine.org/). It is the binary behind the [desktop app](/docs/app/)'s prompt mode, an MCP server other agents drive, a headless generator, the host of [collaborative sessions](/docs/gen/multiplayer/), and the canvas of a live `.world` that agents edit from outside ([live canvas](/docs/gen/live-canvas/)).

## Demo Videos

<iframe width="100%" height="400" src="https://www.youtube.com/embed/n18qnSDmBK0" title="LocalGPT Gen Demo" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture" allowfullscreen></iframe>

<br/>

<iframe width="100%" height="400" src="https://www.youtube.com/embed/cMCGW7eMUNE" title="LocalGPT Gen Demo" frameborder="0" allow="accelerometer; autoplay; clipboard-write; encrypted-media; gyroscope; picture-in-picture" allowfullscreen></iframe>

## Installation

**For users (from crates.io):** install the published binary onto your PATH. No source checkout needed.

```bash
cargo install localgpt-gen
```

**For developers (from a source checkout):** use `cargo run` to iterate, or `cargo install --path` to install a local build.

```bash
# Iterate without installing
cargo run -p localgpt-gen
cargo run -p localgpt-gen -- "create a heart outline with spheres and cubes"

# Install the current checkout as the localgpt-gen binary
cargo install --path crates/gen
```

## Usage

```bash
# Interactive mode — type prompts in the terminal
localgpt-gen

# Start with an initial prompt
localgpt-gen "create a heart outline with spheres and cubes"

# Load an existing glTF/GLB scene
localgpt-gen --scene ./scene.glb

# Open a world and keep building on it: a world folder, a saved world's
# name, a .json/.ron world file, or a URL (for example from localgpt.world)
localgpt-gen --world https://localgpt.world/worlds/md-deck.json

# Verbose logging
localgpt-gen --verbose

# Combine options
localgpt-gen -v -s ./scene.glb "add warm lighting"

# Custom agent ID (default: "gen")
localgpt-gen --agent my-gen-agent

# Open a .world folder as a live canvas: no agent in the window, agents edit
# it from outside through the folder's API (see Live canvas)
localgpt-gen --live scene-1.world
localgpt-gen --live scene-1.world --review     # accept or reject each agent batch before it commits

# Replay a session or a .world's history as a time-lapse, no agent
localgpt-gen --replay <session | package dir | ops.jsonl | .world>

# Host a session and let browsers join with the invite link
localgpt-gen --host --web
```

The agent receives your prompt and iteratively builds a world — spawning shapes, adjusting materials, positioning the camera, and taking screenshots to course-correct. Type `/quit` or `/exit` to close, or close the window.

`--world` imports a world file or URL into `skills/<name>/` in your workspace, with the meshes and audio it references, then opens it; from there it saves like any world you made. Running the same command again opens that copy rather than importing over your edits; delete the folder to import afresh.

## Prompt Panel & Desktop Mode

You don't need to keep a terminal beside the window. Press **F2** in the Gen window to open the **prompt panel**: type a prompt, press **Enter**, and watch the reply stream in, with each tool call shown as it runs. Prompts you send while Gen is busy wait in a queue.

```bash
# Desktop mode: the panel opens at startup and replaces the terminal prompt
localgpt-gen --desktop
```

Gen switches to desktop mode on its own when it isn't started from a terminal. The same shell is the [LocalGPT desktop app](/docs/app/), which also opens documents, songs and `.world` packages; build its macOS bundle with `apps/app-desktop/macos/build-app.sh` from the repository.

| Key | What it does |
|-----|--------------|
| **F2** | Show or hide the panel |
| **Enter** | Send the prompt (in the prompt box), or jump to the prompt box (in the 3D view) |
| **Shift+Enter** | Start a new line |
| **Esc** | Leave the prompt box so WASD moves you again |

- **Model menu:** lists the models this machine can use right now, meaning installed CLI backends (Claude CLI, Gemini CLI, Codex), models pulled into a local Ollama, and — in a build with `local-llm` or `local-llm-metal` — every GGUF in the shared model folder (`~/.local/share/localgpt/models/llm`). A switch is remembered in Gen's own settings (`~/.local/state/localgpt/gen-settings.json`); Gen reads no `config.toml`. A CLI backend only appears if Gen started on that backend, because its tool connection is set up at startup.
- **Slash commands:** `/model <name>`, `/new`, `/clear`, and `/quit` work in the panel. The other commands print their results in the terminal.
- **First run:** if your model is a CLI backend that isn't installed, the panel says so instead of failing silently. Opened from Finder, Gen reads your login shell's `PATH`, so `claude`, `gemini`, and `codex` installed with Homebrew, npm, or into `~/.local/bin` are found.
- **Logs:** desktop mode has no terminal, so it logs to `~/.local/state/localgpt/logs/gen-desktop.log`.
- **Hosting:** with `--host`, the panel shows the session name and the current PIN, and friends' prompts appear in it as they run.

With a CLI backend, tools run through the [MCP relay](/docs/gen/cli-mode), so the panel shows the model's text but not each tool call.

## Three Ways to Use Gen (with Bevy Window)

All three modes open a Bevy 3D window where you watch worlds being built in real-time. They differ in **who drives the AI** and **whether you need an API key**.

| | **Interactive (API)** | **Interactive (CLI Backend)** | **MCP Server (External App)** |
|---|---|---|---|
| **Command** | `localgpt-gen` | `localgpt-gen` | `localgpt-gen mcp-server` |
| **Who builds the world** | LocalGPT's built-in agent | External CLI (Claude CLI, Gemini CLI, Codex) via MCP relay | External app (Claude Desktop, Codex Desktop, VS Code, Zed, Cursor) |
| **LLM provider** | API key (Anthropic, OpenAI, Ollama, etc.) | CLI subprocess (`claude`, `gemini`, `codex`) | Whatever the external app uses |
| **Requires API key?** | Yes (or Ollama for local) | No — uses the CLI's own auth | No — the external app handles auth |
| **Who manages conversation?** | LocalGPT agent loop | The CLI backend (autonomous) | The external app |
| **Tool execution** | In-process (GenBridge) | CLI → [MCP relay](/docs/gen/cli-mode) → GenBridge | MCP stdio → GenBridge |
| **Memory system** | Full (MEMORY.md, daily logs, search) | Full (via MCP relay) | Full (via MCP tools) |
| **Best for** | Direct control, fast iteration | Using Claude/Gemini/Codex without API keys | Editors, desktop apps, multi-tool workflows |

### Mode 1: Interactive with API Key

The default. LocalGPT's own agent calls gen tools directly — fastest response, tightest feedback loop.

```bash
# Uses your configured model (e.g., claude-sonnet-4-6 via Anthropic API)
localgpt-gen
localgpt-gen "build a castle on a hill"
```

Set your model in `config.toml`:
```toml
[agent]
default_model = "claude-sonnet-4-6"  # or "gpt-4o", "ollama/llama3", etc.

[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"
```

#### A local model, no server

Built with the `local-llm` feature, Gen runs a GGUF model itself: no API key, no Ollama. It reads the model folder [MD](/docs/md/llm) and [Verse](/docs/verse#llm) share, `~/.local/share/localgpt/models/llm/` (`$LOCALGPT_LLM_DIR` moves it), so a model fetched for either app is used as is.

```bash
cargo build --release -p localgpt-gen --features local-llm-metal   # Apple Silicon; plain `local-llm` elsewhere
```

Pick `gguf/<name>` from the model menu (every `.gguf` in the folder is listed), type `/model gguf/<name>`, or make it the default with `default_model = "gguf/default"` under `[agent]`. The first prompt loads the model, which can take a minute; it then stays loaded. A tokenizer is read from `<name>.tokenizer.json` or `tokenizer.json` beside the model, else from the GGUF itself.

Scene building is a long tool-calling session with a large tool list, so a small model is much weaker at it than a hosted one. Bonsai-8B (the model MD and Verse share) runs and saves worlds, but it can call the wrong tool name or repeat a failing call until its turn budget runs out. An instruction-tuned 7–14B+ model with good tool calling (Qwen2.5-Instruct, for example) does noticeably better.

### Mode 2: Interactive with CLI Backend (no API key)

Use Claude CLI, Gemini CLI, or Codex as the LLM — they handle auth through their own login. LocalGPT auto-starts an [MCP relay](/docs/gen/cli-mode) when it detects a CLI backend model, so tool calls go to your existing Bevy window.

```bash
# Set model to a CLI backend in config.toml
localgpt-gen  # with default_model = "claude-cli/opus"
```

You'll see:
```
MCP relay active on port 9878 (external MCP clients can connect to this window)
CLI backend detected (claude-cli/opus). Gen tools will route to this window via MCP relay.
```

The CLI backend runs autonomously — it decides which tools to call and builds the scene. You watch it happen in the Bevy window and can type follow-up prompts.

**How it works under the hood:**

```
You type prompt
  → LocalGPT sends to Claude CLI subprocess
    → Claude CLI spawns `localgpt-gen mcp-server --connect`
      → Connects to MCP relay (TCP :9878)
        → Tool calls go to existing Bevy window
          → You see the world being built
```

See [CLI Mode (MCP Relay)](/docs/gen/cli-mode) for setup and troubleshooting.

### Mode 3: MCP Server (external app drives the window)

LocalGPT Gen runs as a tool server. An external app is the orchestrator — it spawns the Bevy window and drives scene building via MCP.

Supported apps include:
- **Desktop apps** — Claude Desktop, Codex Desktop
- **CLI tools** — Claude CLI, Gemini CLI, Codex CLI (running directly, not as a LocalGPT backend)
- **Editors** — VS Code Copilot, Zed, Cursor, Windsurf

```bash
localgpt-gen mcp-server
```

Configure the app to connect (example `.mcp.json`):
```json
{
  "mcpServers": {
    "localgpt-gen": {
      "command": "localgpt-gen",
      "args": ["mcp-server"]
    }
  }
}
```

LocalGPT doesn't run its own agent loop — it's purely a tool server. The external app manages the conversation and decides which tools to call.

:::caution Don't confuse Mode 2 and Mode 3 for CLI tools
**Mode 2** = you run `localgpt-gen` and it uses Claude CLI/Gemini CLI/Codex *as its LLM backend* (the CLI is a subprocess of LocalGPT).
**Mode 3** = you run Claude CLI/Gemini CLI/Codex *directly* and it uses `localgpt-gen mcp-server` as a tool (LocalGPT is a subprocess of the CLI).

The difference is **who is the parent process**. In Mode 2, LocalGPT is in charge. In Mode 3, the external app is in charge.
:::

See [MCP Server](/docs/gen/mcp-server) for all supported apps and configuration.

## Headless Mode (no window)

Separate from the three modes above, **headless mode** generates worlds without opening a Bevy window — for batch runs, CI pipelines, and overnight experiment queues.

```bash
localgpt-gen headless --prompt "Build a cozy cabin in a snowy forest"
```

Combined with the memory system, the AI learns your creative style across sessions and applies it automatically. Queue multiple experiments via `HEARTBEAT.md` or MCP tools, and browse results in the in-app gallery.

See [Headless Mode & Experiment Queue](/docs/gen/headless) for full details.

## Features

- **[Live canvas](/docs/gen/live-canvas)** — open a `.world` folder; agents edit it from outside through a local API while you edit by hand; every change is one entry in a history that branches
- **[Previs](/docs/gen/previs)** — a screenplay's scenes as sets with cast and coverage, revised by asking, treatments compared as boards
- **[Tools](/docs/gen/tools)** — 77 `gen_*` tools in three profiles (`--tools core|standard|full`) for scenes, characters, interactions, terrain, UI, physics, worldgen and experiments
- **[WorldGen Pipeline](/docs/gen/worldgen)** — Structured world generation: blockout → navmesh → three-tier placement → evaluation
- **[Behaviors](/docs/gen/behaviors)** — Data-driven animations (orbit, spin, bounce, etc.)
- **[Audio](/docs/gen/audio)** — Procedural environmental audio with spatial emitters
- **[World Skills](/docs/gen/world-skills)** — Save and load complete worlds as reusable skills
- **[Collaborative Sessions](/docs/gen/multiplayer)** — Host a world on your network; friends join with a PIN and build with your AI
- **[Export](/docs/gen/export)** — glTF/GLB (Blender, Unity, Unreal), HTML (browser-viewable with audio + behaviors), screenshots
- **[MCP Server](/docs/gen/mcp-server)** — Use gen tools from Claude Desktop, VS Code, Zed, Cursor, and other MCP clients
- **[CLI Mode](/docs/gen/cli-mode)** — MCP relay for Claude CLI, Gemini CLI, and Codex (no API key needed)
- **[Headless Mode](/docs/gen/headless)** — Batch generation, experiment queue, and creative memory
- **[External Services](/docs/gen/external-services)** — Optional local services for NPC brains, depth preview, and 3D asset generation
- **Undo/Redo** — per author, with redo, through the same history the live canvas keeps
- **Streaming Chat** — Real-time tool call display and streaming responses

## Templates

Jumpstart your project with ready-to-customize world templates:

- **Fantasy** — [Medieval Village](/templates/medieval-village), [Enchanted Forest](/templates/enchanted-forest), [Japanese Temple](/templates/japanese-temple), [Cozy Farm](/templates/cozy-farm), [Winter Wonderland](/templates/winter-wonderland)
- **Sci-Fi** — [Space Station](/templates/space-station), [Underwater World](/templates/underwater-world), [Alien World](/templates/alien-world)
- **Horror** — [Haunted House](/templates/haunted-house), [Backrooms](/templates/backrooms)
- **Urban** — [Cyberpunk City](/templates/cyberpunk-city), [Modern City](/templates/modern-city)

[Browse all templates →](/templates)

## Current Limitations

- Visual output depends on the LLM's spatial reasoning ability
- Requires a GPU-capable display for rendering

## More LocalGPT Apps

Gen's shell is also the **[LocalGPT desktop app](/docs/app/)**, where a prompt, a document, a song or a `.world` package opens in the same window. Documents are **[LocalGPT MD](/docs/md)**'s pipeline with an editor pane and in-place rebuilds; songs are **[LocalGPT Verse](/docs/verse)**'s analysis and performance. Both also run as windows of their own from the repository.

## Showcase

- **[proofof.video](https://proofof.video/)** — Video gallery comparing world generations across different models using the same or similar prompts
