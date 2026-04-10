---
sidebar_position: 14.5
---

# MCP Server

LocalGPT Gen can run as an [MCP](https://modelcontextprotocol.io/) (Model Context Protocol) server, exposing gen tools and core LocalGPT tools over stdio. This lets desktop AI apps — Claude Desktop, Codex Desktop — and MCP-compatible editors like VS Code, Zed, and Cursor drive the Bevy 3D window directly, with full access to LocalGPT's memory system.

## Why MCP?

When using LocalGPT Gen in its default interactive mode, the built-in LLM agent calls gen tools directly inside the same process. But if you want to use a different AI backend — Claude Desktop, Codex Desktop, or an editor's built-in AI — those tools aren't accessible because they run in separate processes.

MCP solves this. It's a standard protocol that these tools already support. By running `localgpt-gen mcp-server`, the Bevy window opens and all gen tools become available to any MCP client over stdio. The AI backend becomes the orchestrator — it manages the conversation, calls tools, and drives the scene building.

## Quick Start

```bash
# Start Gen as an MCP server (Bevy window opens, tools served over stdio)
localgpt-gen mcp-server
```

Then configure your AI tool to connect to it (see sections below).

## Available Tools

The MCP server exposes gen tools plus core LocalGPT tools:

### Gen Tools (32 core tools)

- **Scene query** — `gen_scene_info`, `gen_screenshot`, `gen_entity_info`
- **Entity creation** — `gen_spawn_primitive`, `gen_spawn_batch`, `gen_spawn_mesh`, `gen_load_gltf`
- **Entity modification** — `gen_modify_entity`, `gen_modify_batch`, `gen_delete_entity`, `gen_delete_batch`
- **Camera & environment** — `gen_set_camera`, `gen_set_light`, `gen_set_environment`
- **Audio** — `gen_set_ambience`, `gen_audio_emitter`, `gen_modify_audio`, `gen_audio_info`
- **Behaviors** — `gen_add_behavior`, `gen_remove_behavior`, `gen_list_behaviors`, `gen_pause_behaviors`
- **World skills** — `gen_save_world`, `gen_load_world`, `gen_export_world`, `gen_clear_scene`
- **Export** — `gen_export_screenshot`, `gen_export_gltf`, `gen_export_html`
- **Undo/Redo** — `gen_undo`, `gen_redo`, `gen_undo_info`

See [Gen Tools Reference](/docs/gen/tools) for full documentation on each tool.

### MCP-Only Tools (50+ tools)

These additional tools are available exclusively through the MCP server, enabling richer game-like world building:

#### Avatar & Characters (5 tools)

| Tool | Description |
|------|-------------|
| `gen_spawn_player` | Spawn a controllable player character with movement, camera, and collision |
| `gen_set_spawn_point` | Set spawn/respawn location for the player |
| `gen_add_npc` | Create an NPC with idle, patrol, or wander behavior |
| `gen_set_npc_dialogue` | Attach a branching conversation tree to an NPC |
| `gen_set_camera_mode` | Switch camera mode (first_person, third_person, top_down, fixed) |

#### Interactions & Triggers (5 tools)

| Tool | Description |
|------|-------------|
| `gen_add_trigger` | Add trigger+action pairs (proximity, click, area, collision, timer) |
| `gen_add_teleporter` | Create a portal that teleports the player to a destination |
| `gen_add_collectible` | Make an entity collectible with score value and pickup effects |
| `gen_add_door` | Add interactive door behavior with optional key requirements |
| `gen_link_entities` | Wire one entity's event to trigger another entity's action |

#### Terrain & Landscape (5 tools)

| Tool | Description |
|------|-------------|
| `gen_add_terrain` | Generate procedural terrain from Perlin/Simplex noise |
| `gen_add_water` | Create a transparent animated water plane |
| `gen_add_path` | Create walkable paths between waypoints |
| `gen_add_foliage` | Scatter vegetation (trees, bushes, grass, flowers, rocks) |
| `gen_set_sky` | Configure sky, sun direction, ambient light, and fog |

#### In-World UI (5 tools)

| Tool | Description |
|------|-------------|
| `gen_add_sign` | Place readable billboard text in the 3D world |
| `gen_add_hud` | Add persistent screen-space HUD elements (score, health, timer) |
| `gen_add_label` | Attach a floating name label to an entity |
| `gen_add_tooltip` | Add contextual tooltips on proximity or look-at |
| `gen_add_notification` | Show transient notification messages (toast, banner, achievement) |

#### Physics (5 tools)

| Tool | Description |
|------|-------------|
| `gen_set_physics` | Enable physics on an entity (dynamic, static, or kinematic body) |
| `gen_add_collider` | Add collision shapes (box, sphere, capsule, cylinder, mesh) |
| `gen_add_joint` | Create physical constraints between entities (fixed, revolute, spring, etc.) |
| `gen_add_force` | Create force fields or apply impulses |
| `gen_set_gravity` | Control gravity globally or per-zone (presets: earth, moon, mars, zero) |

#### WorldGen Pipeline (15 tools)

| Tool | Description |
|------|-------------|
| `gen_plan_layout` | Generate a structured world layout plan from a text description |
| `gen_apply_blockout` | Apply a blockout spec to create terrain, regions, and paths |
| `gen_populate_region` | Populate a region with entities using three-tier placement (hero/medium/decorative) |
| `gen_set_tier` | Set an entity's placement tier (hero, medium, decorative) |
| `gen_set_role` | Set an entity's semantic role (ground, structure, prop, vegetation, etc.) |
| `gen_bulk_modify` | Modify multiple entities by role or tier (e.g., recolor all vegetation) |
| `gen_modify_blockout` | Add, remove, resize, or move blockout regions with incremental regeneration |
| `gen_evaluate_scene` | Take a screenshot with optional entity highlighting for LLM self-evaluation |
| `gen_auto_refine` | Automatically evaluate and improve scene quality via screenshot loop |
| `gen_build_navmesh` | Build a walkability grid for the current terrain |
| `gen_validate_navigability` | Check that key points are reachable via the navmesh |
| `gen_edit_navmesh` | Manually override navmesh cells (block, allow, add connection) |
| `gen_regenerate` | Regenerate regions after blockout changes, preserving manual edits |
| `gen_render_depth` | Render a depth map of the scene from a specified camera angle |
| `gen_preview_world` | Generate a styled 2D preview image from a depth map (external API) |

#### Asset Generation (3 tools)

| Tool | Description |
|------|-------------|
| `gen_generate_asset` | Queue a 3D asset generation task (text-to-3D via external model server) |
| `gen_asset_status` | Check the status of an asset generation task |
| `gen_list_assets` | List all queued, running, and completed asset generation tasks |

#### Experiment Queue (3 tools)

| Tool | Description |
|------|-------------|
| `gen_queue_experiment` | Queue a world generation experiment for background processing |
| `gen_list_experiments` | List experiments by status (pending, running, completed, failed) |
| `gen_experiment_status` | Get detailed status of a specific experiment by ID |

### Core Tools

| Tool | Description |
|------|-------------|
| `memory_search` | Search MEMORY.md + daily logs using hybrid semantic + keyword search |
| `memory_get` | Fetch specific lines from memory files (use after `memory_search`) |
| `memory_save` | Append to MEMORY.md — long-term curated knowledge |
| `memory_log` | Append to today's daily log (`memory/YYYY-MM-DD.md`) |
| `web_fetch` | Fetch and extract content from URLs |
| `web_search` | Search the web (if configured in `config.toml`) |

These are the same core tools available via `localgpt mcp-server` (see [Memory-only MCP Server](#memory-only-mcp-server) below).

### Why Not File/Shell Tools?

CLI tools like `bash`, `read_file`, `write_file`, and `edit_file` are **not** exposed via MCP. External AI backends (Claude CLI, Gemini CLI, Codex) already have their own file and shell tools. Exposing duplicates would create confusion and security concerns.

## Claude Desktop

Add to your Claude Desktop MCP configuration:

- **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Windows**: `%APPDATA%\Claude\claude_desktop_config.json`

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

Restart Claude Desktop after saving. The gen tools appear in Claude's tool list — ask it to build a scene:

> Build a medieval castle with a moat, drawbridge, and warm torchlight

Claude will call `gen_spawn_primitive`, `gen_set_light`, `gen_set_camera`, etc. to construct the scene in the Bevy window.

## Codex Desktop

Add to your Codex configuration:

- **Config file**: `~/.codex/config.json`

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

## CLI Tools

If you're using a CLI-based AI tool (Claude CLI, Gemini CLI, Codex CLI) as the backend for LocalGPT Gen's interactive mode, see [CLI Mode (MCP Relay)](/docs/gen/cli-mode) for configuration details. CLI tools use `--connect` to relay tool calls to your existing Bevy window instead of spawning a new one.

## VS Code (Copilot)

VS Code supports MCP servers through its Copilot agent mode. Add to your workspace `.vscode/settings.json` or user settings:

```json
{
  "mcp": {
    "servers": {
      "localgpt-gen": {
        "command": "localgpt-gen",
        "args": ["mcp-server"]
      }
    }
  }
}
```

You can also add it via the command palette: **MCP: Add Server** and choose "stdio" transport.

Once configured, use Copilot in agent mode (`@workspace`) and ask it to build 3D scenes. The gen tools show up as available tools that Copilot can call.

## Zed Editor

Add to your Zed settings (`~/.config/zed/settings.json`):

```json
{
  "context_servers": {
    "localgpt-gen": {
      "command": {
        "path": "localgpt-gen",
        "args": ["mcp-server"]
      }
    }
  }
}
```

The gen tools become available in Zed's AI assistant panel.

## Cursor

Add to your Cursor MCP configuration (`.cursor/mcp.json` in your project or global config):

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

## Windsurf

Add to your Windsurf MCP configuration (`~/.codeium/windsurf/mcp_config.json`):

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

## How It Works

```
┌─────────────────────┐       MCP stdio (JSON-RPC)        ┌──────────────────┐
│  AI Backend         │◄──────────────────────────────────►│  localgpt-gen    │
│  (Claude Desktop,   │        tools/list                  │                  │
│   Codex Desktop,    │        tools/call                  │  MCP Server      │
│   VS Code, Zed)     │                                    │    │       │     │
└─────────────────────┘                                    │ GenBridge Memory │
        ▲                                                  │    ↓       ↓     │
        │ manages conversation,                            │  Bevy   SQLite   │
        │ decides which tools                              │  3D     FTS5 +   │
        │ to call and when                                 │ Engine  vectors  │
        │                                                  └──────────────────┘
   AI Backend is the
   orchestrator
```

In MCP mode, the **AI backend is the orchestrator**. It manages the conversation, decides which tools to call, and drives scene building. LocalGPT Gen provides the runtime (Bevy 3D engine + memory database) and exposes it through standard MCP tools.

1. The AI backend spawns `localgpt-gen mcp-server` as a child process
2. MCP handshake happens over stdio (JSON-RPC 2.0, one message per line)
3. The backend discovers all tools via `tools/list` (gen tools + memory + web)
4. The AI reasons about the scene and calls tools as needed — `gen_spawn_primitive`, `memory_search`, `gen_screenshot`, etc.
5. Gen tool calls are dispatched through the GenBridge channel to the Bevy main thread; memory tool calls query the LocalGPT SQLite database
6. Results are sent back to the AI backend, which continues building

This is different from LocalGPT Gen's interactive mode, where LocalGPT's own agent loop is the orchestrator. In MCP mode, LocalGPT doesn't run its agent loop at all — it's purely a tool server.

## Combining with Scene File

You can load an existing scene while starting the MCP server:

```bash
localgpt-gen mcp-server --scene ./my-scene.glb
```

The AI backend can then modify the pre-loaded scene.

## Memory-only MCP Server

If you don't need gen tools and just want to give an AI backend access to LocalGPT's memory, use the standalone MCP server:

```bash
localgpt mcp-server
```

This exposes only the core tools: `memory_search`, `memory_get`, `memory_save`, `memory_log`, `web_fetch`, and `web_search`. No Bevy window, no gen tools.

Configure it the same way as `localgpt-gen mcp-server`:

```json
{
  "mcpServers": {
    "localgpt": {
      "command": "localgpt",
      "args": ["mcp-server"]
    }
  }
}
```

This is useful when you want to use Claude CLI, Gemini CLI, or an editor for regular coding tasks while still having access to LocalGPT's persistent memory system — notes, preferences, and context from past sessions.

## Memory Integration

The MCP server initializes LocalGPT's memory system using the workspace configured in `~/.localgpt/config.toml`. This means:

- **`memory_search`** queries the same MEMORY.md, daily logs, and knowledge files used by LocalGPT's interactive mode
- If embeddings are enabled (`memory.embedding_provider = "local"`), semantic search works across all indexed memory chunks
- **`memory_save`** and **`memory_log`** write to the same workspace files, following LocalGPT's conventions — the AI backend doesn't need to know about file paths or formats
- Any notes saved in MCP mode are available in future `localgpt chat` sessions and vice versa

## Tips

- **Verbose logging**: Add `--verbose` to see MCP protocol messages and tool list in stderr: `localgpt-gen mcp-server --verbose`
- **Binary path**: If `localgpt-gen` is not in your `$PATH`, use the full path (e.g., `/Users/you/.cargo/bin/localgpt-gen`) in the MCP server configuration
- **One instance**: Each standalone `mcp-server` spawns its own Bevy window. If you're using Gen interactively, use `--connect` to relay to the existing window instead (see [CLI Mode](/docs/gen/cli-mode))
- **Screenshots**: The AI can take screenshots via `gen_screenshot` to see what it built and course-correct — this works the same as in interactive mode
- **Memory workspace**: The MCP server reads memory from the same workspace as `localgpt chat`. Any notes saved in interactive mode are available via `memory_search` in MCP mode
