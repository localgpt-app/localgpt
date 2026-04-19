---
sidebar_position: 14
---

# LocalGPT Gen

**LocalGPT Gen** is a built-in world generation mode. You type natural language, and the AI builds explorable worlds — geometry, materials, lighting, behaviors, audio, and camera. All inside the same single Rust binary, powered by [Bevy](https://bevyengine.org/).

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

# Verbose logging
localgpt-gen --verbose

# Combine options
localgpt-gen -v -s ./scene.glb "add warm lighting"

# Custom agent ID (default: "gen")
localgpt-gen --agent my-gen-agent
```

The agent receives your prompt and iteratively builds a world — spawning shapes, adjusting materials, positioning the camera, and taking screenshots to course-correct. Type `/quit` or `/exit` in the terminal to close.

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

- **[Tools](/docs/gen/tools)** — 32 core tools plus 50+ MCP-only tools for characters, interactions, terrain, UI, physics, worldgen, and experiments
- **[WorldGen Pipeline](/docs/gen/worldgen)** — Structured world generation: blockout → navmesh → three-tier placement → evaluation
- **[Behaviors](/docs/gen/behaviors)** — Data-driven animations (orbit, spin, bounce, etc.)
- **[Audio](/docs/gen/audio)** — Procedural environmental audio with spatial emitters
- **[World Skills](/docs/gen/world-skills)** — Save and load complete worlds as reusable skills
- **[Export](/docs/gen/export)** — glTF/GLB (Blender, Unity, Unreal), HTML (browser-viewable with audio + behaviors), screenshots
- **[MCP Server](/docs/gen/mcp-server)** — Use gen tools from Claude Desktop, VS Code, Zed, Cursor, and other MCP clients
- **[CLI Mode](/docs/gen/cli-mode)** — MCP relay for Claude CLI, Gemini CLI, and Codex (no API key needed)
- **[Headless Mode](/docs/gen/headless)** — Batch generation, experiment queue, and creative memory
- **[External Services](/docs/gen/external-services)** — Optional local services for NPC brains, depth preview, and 3D asset generation
- **Undo/Redo** — Full undo/redo support for all scene edits with persistence
- **Streaming Chat** — Real-time tool call display and streaming responses

## Templates

Jumpstart your project with ready-to-customize world templates:

- **Fantasy** — [Medieval Village](/templates/fantasy/medieval-village), [Enchanted Forest](/templates/fantasy/enchanted-forest), [Japanese Temple](/templates/fantasy/japanese-temple), [Cozy Farm](/templates/fantasy/cozy-farm), [Winter Wonderland](/templates/fantasy/winter-wonderland)
- **Sci-Fi** — [Space Station](/templates/sci-fi/space-station), [Underwater World](/templates/sci-fi/underwater-world), [Alien World](/templates/sci-fi/alien-world)
- **Horror** — [Haunted House](/templates/horror/haunted-house), [Backrooms](/templates/horror/backrooms)
- **Urban** — [Cyberpunk City](/templates/urban/cyberpunk-city), [Modern City](/templates/urban/modern-city)

[Browse all templates →](/templates)

## Current Limitations

- Visual output depends on the LLM's spatial reasoning ability
- Requires a GPU-capable display for rendering

## Showcase

- **[proofof.video](https://proofof.video/)** — Video gallery comparing world generations across different models using the same or similar prompts
