---
title: "LocalGPT Gen"
---

# LocalGPT Gen

**LocalGPT Gen** builds explorable worlds from natural language — geometry, materials, lighting, behaviors, audio, and camera — on [Bevy](https://bevyengine.org/). It is the viewport behind the [desktop app](/docs/app/), an MCP server any agent drives, the host of [collaborative sessions](/docs/gen/multiplayer/), and the canvas of a live `.world` that agents edit from outside ([live canvas](/docs/gen/live-canvas/)). Gen runs no model of its own and holds no key: the agent you already use does the building.

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
cargo run -p localgpt-gen -- --mcp-relay      # iterate without installing
cargo install --path crates/gen               # install the current checkout as the localgpt-gen binary
```

## Usage

```bash
# The window, with the MCP relay: an agent outside drives it (below)
localgpt-gen --mcp-relay

# The window as an MCP client's own process (Claude Desktop, editors)
localgpt-gen mcp-server

# Load an existing glTF/GLB scene, or open a world and keep building on it:
# a world folder, a saved world's name, a .json/.ron world file, or a URL
# (for example from localgpt.world)
localgpt-gen --mcp-relay --scene ./scene.glb
localgpt-gen --mcp-relay --world https://localgpt.world/worlds/md-deck.json

# Open a .world folder as a live canvas: agents edit it from outside through
# the folder's API (see Live canvas)
localgpt-gen --live scene-1.world
localgpt-gen --live scene-1.world --review     # accept or reject each agent batch before it commits

# Replay a session or a .world's history as a time-lapse
localgpt-gen --replay <session | package dir | ops.jsonl | .world>

# Host a session and let browsers join with the invite link
localgpt-gen --host --web

# Verbose logging
localgpt-gen --verbose --mcp-relay
```

`--world` imports a world file or URL into `skills/<name>/` in your workspace, with the meshes and audio it references, then opens it; from there it saves like any world you made. Running the same command again opens that copy rather than importing over your edits; delete the folder to import afresh.

Gen keeps one file of settings, `~/.local/state/localgpt/gen-settings.json` (what the app opened last), and reads no config file. The windowed app with terminals for your agents beside the world is the [LocalGPT desktop app](/docs/app/); build its macOS bundle with `apps/app-desktop/macos/build-app.sh` from the repository.

## The window is driven from outside

Gen takes no prompt of its own. An agent — a coding agent in a terminal, a desktop app, an editor — calls Gen's tools over MCP, and the window shows the world being built. Two ways to connect, one tool set:

| | **The relay** | **MCP server** |
|---|---|---|
| **Command** | `localgpt-gen --mcp-relay` | `localgpt-gen mcp-server` |
| **Who spawns whom** | you start the window; the agent's MCP config spawns `localgpt-gen mcp-server --connect`, a bridge to it | the MCP client spawns the whole of Gen, window included |
| **Best for** | a coding agent in a terminal (Claude Code, Codex, Gemini CLI) building in the window you are looking at | Claude Desktop, Codex Desktop, VS Code, Zed, Cursor; headless with `--headless` |
| **Several clients** | yes, one window | one client, one window each |

### The relay

```bash
localgpt-gen --mcp-relay
```

You'll see:
```
MCP relay active on port 9878 (external MCP clients can connect to this window)
```

Point the agent at `localgpt-gen mcp-server --connect` in its MCP config, start it in another terminal, and prompt it there. It decides which tools to call and builds the scene; you watch it happen in the Bevy window.

```
You prompt the agent in its terminal
  → The agent spawns `localgpt-gen mcp-server --connect` (from its MCP config)
    → Connects to the MCP relay (TCP :9878)
      → Tool calls go to the existing Bevy window
        → You see the world being built
```

See [MCP Relay](/docs/gen/cli-mode) for the client configurations and troubleshooting.

### MCP server

```bash
localgpt-gen mcp-server
```

Configure the client to spawn it (example `.mcp.json`):
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

Gen is purely a tool server here: the client manages the conversation and decides which tools to call. See [MCP Server](/docs/gen/mcp-server) for every supported client, and [Headless](/docs/gen/headless) for `--headless`.

### The live canvas

A `.world` package opened with `--live` is edited through its own API: `localgpt world` from any shell, the [shim](/docs/cli-commands/#world) for an MCP agent, or the ops an agent sends by hand. Every batch is one entry in the package's history, attributed, undoable and branchable, and `--review` holds each one for Enter or Esc. See [Live canvas](/docs/gen/live-canvas).

## Features

- **[Live canvas](/docs/gen/live-canvas)** — open a `.world` folder; agents edit it from outside through a local API while you edit by hand; every change is one entry in a history that branches
- **[Previs](/docs/gen/previs)** — a screenplay's scenes as sets with cast and coverage, revised by asking, treatments compared as boards
- **[Tools](/docs/gen/tools)** — 88 `gen_*` tools for scenes, characters, interactions, terrain, UI, physics and worldgen
- **[WorldGen Pipeline](/docs/gen/worldgen)** — Structured world generation: blockout → navmesh → three-tier placement → evaluation
- **[Behaviors](/docs/gen/behaviors)** — Data-driven animations (orbit, spin, bounce, etc.)
- **[Audio](/docs/gen/audio)** — Procedural environmental audio with spatial emitters
- **[World Skills](/docs/gen/world-skills)** — Save and load complete worlds as reusable skills
- **[Collaborative Sessions](/docs/gen/multiplayer)** — Host a world on your network; friends join with a PIN, watch and chat, and browsers edit with `--web-edit`
- **[Export](/docs/gen/export)** — glTF/GLB (Blender, Unity, Unreal), HTML (browser-viewable with audio + behaviors), screenshots
- **[MCP Server](/docs/gen/mcp-server)** — Use gen tools from Claude Desktop, VS Code, Zed, Cursor, and other MCP clients
- **[MCP Relay](/docs/gen/cli-mode)** — A coding agent drives the window you are looking at
- **[Headless](/docs/gen/headless)** — The same tools with no window, for CI and batch work
- **Undo/Redo** — per author, with redo, through the same history the live canvas keeps

## Templates

Jumpstart your project with ready-to-customize world templates:

- **Fantasy** — [Medieval Village](/templates/medieval-village), [Enchanted Forest](/templates/enchanted-forest), [Japanese Temple](/templates/japanese-temple), [Cozy Farm](/templates/cozy-farm), [Winter Wonderland](/templates/winter-wonderland)
- **Sci-Fi** — [Space Station](/templates/space-station), [Underwater World](/templates/underwater-world), [Alien World](/templates/alien-world)
- **Horror** — [Haunted House](/templates/haunted-house), [Backrooms](/templates/backrooms)
- **Urban** — [Cyberpunk City](/templates/cyberpunk-city), [Modern City](/templates/modern-city)

[Browse all templates →](/templates)

## Current Limitations

- Visual output depends on the agent's spatial reasoning ability
- Requires a GPU-capable display for rendering (`mcp-server --headless` renders offscreen)

## More LocalGPT Apps

Gen's viewport is also the **[LocalGPT desktop app](/docs/app/)**'s, where a document, a song or a `.world` package opens in the same window with terminals for your agents beside it. Documents are **[LocalGPT MD](/docs/md)**'s pipeline with an editor pane and in-place rebuilds; songs are **[LocalGPT Verse](/docs/verse)**'s analysis and performance. Both also run as windows of their own from the repository.

## Showcase

- **[proofof.video](https://proofof.video/)** — Video gallery comparing world generations across different models using the same or similar prompts
