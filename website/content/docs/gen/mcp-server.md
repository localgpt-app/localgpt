---
title: "MCP Server"
---

# MCP Server

LocalGPT Gen can run as an [MCP](https://modelcontextprotocol.io/) (Model Context Protocol) server, exposing the gen tools over stdio. This lets desktop AI apps — Claude Desktop, Codex Desktop — coding agents, and MCP-compatible editors like VS Code, Zed, and Cursor drive the Bevy 3D window directly.

## Why MCP?

LocalGPT Gen runs no model of its own: the window and the tools that build in it are the whole binary, and whatever drives them runs in another process — Claude Desktop, Codex Desktop, a coding agent in a terminal, an editor's built-in AI.

MCP is how they reach it. It's a standard protocol that these tools already support. By running `localgpt gen mcp-server` (which forwards to `localgpt-app gen mcp-server` when the desktop app is installed, or to `localgpt-gen mcp-server`), the Bevy window opens and all gen tools become available to any MCP client over stdio. The client is the orchestrator — it manages the conversation, calls tools, and drives the scene building — under its own login; Gen holds no key.

## Quick Start

```bash
# Start Gen as an MCP server (Bevy window opens, tools served over stdio)
localgpt gen mcp-server        # or directly: localgpt-app mcp-server / localgpt-gen mcp-server
```

Then configure your client to spawn it (see sections below).

## Available Tools

The MCP server exposes the 88 gen tools, and nothing else: no memory, web, file or shell tools, and nothing that calls a model or a service.

### Scene tools

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

### The rest of the set

By area, for game-like world building:

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
| `gen_evaluate_scene` | Take a screenshot with optional entity highlighting for the agent's own evaluation |
| `gen_auto_refine` | Automatically evaluate and improve scene quality via screenshot loop |
| `gen_build_navmesh` | Build a walkability grid for the current terrain |
| `gen_validate_navigability` | Check that key points are reachable via the navmesh |
| `gen_edit_navmesh` | Manually override navmesh cells (block, allow, add connection) |
| `gen_regenerate` | Regenerate regions after blockout changes, preserving manual edits |
| `gen_render_depth` | Render a depth map of the scene from a specified camera angle |

### No memory, web, file or shell tools

Only the `gen_*` tools are served. An MCP client that is a coding agent has its own memory, web, file and shell tools, and Gen keeps no memory of its own: what it keeps is the worlds.

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

## Codex

Add to `~/.codex/config.toml`, which Codex's CLI and app share:

```toml
[mcp_servers.localgpt-gen]
command = "localgpt-gen"
args = ["mcp-server"]
```

## Coding agents in a terminal

Claude Code, Codex and Gemini CLI take the same entry in their own MCP configs. To drive a Gen window that is already open — started with `--mcp-relay` — point the client at `localgpt-gen mcp-server --connect` instead, a bridge to that window rather than a new one: see [MCP Relay](/docs/gen/cli-mode).

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

The gen tools become available in Zed's agent panel.

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
│  MCP client         │◄──────────────────────────────────►│  localgpt-gen    │
│  (Claude Desktop,   │        tools/list                  │                  │
│   Claude Code,      │        tools/call                  │  MCP Server      │
│   VS Code, Zed)     │                                    │       │          │
└─────────────────────┘                                    │   GenBridge      │
        ▲                                                  │       ↓          │
        │ manages conversation,                            │     Bevy         │
        │ decides which tools                              │     3D           │
        │ to call and when                                 │   Engine         │
        │                                                  └──────────────────┘
   the client is the
   orchestrator
```

The **MCP client is the orchestrator**. It manages the conversation, decides which tools to call, and drives scene building. LocalGPT Gen provides the runtime (the Bevy 3D engine) and exposes it through standard MCP tools; it runs no agent of its own.

1. The client spawns `localgpt-gen mcp-server` as a child process
2. MCP handshake happens over stdio (JSON-RPC 2.0, one message per line)
3. The client discovers the tools via `tools/list`
4. The agent reasons about the scene and calls tools as needed — `gen_spawn_primitive`, `gen_screenshot`, etc.
5. Tool calls are dispatched through the GenBridge channel to the Bevy main thread
6. Results are sent back to the client, which continues building

## Combining with Scene File

You can load an existing scene while starting the MCP server:

```bash
localgpt-gen mcp-server --scene ./my-scene.glb
```

The agent then works on the loaded scene.

## The world's own shim

For a `.world` package an app has open, `localgpt world mcp` serves the package's tools — `submit`, `undo`, `log`, `tips`, `goto`, `verify`, `screenshot`, `selection` — over stdio to any MCP client, with no window of its own; see [CLI Commands](/docs/cli-commands/#world).

```bash
claude mcp add world -- localgpt world mcp
```

## Tips

- **Verbose logging**: Add `--verbose` to see MCP protocol messages and tool list in stderr: `localgpt-gen mcp-server --verbose`
- **Binary path**: If `localgpt-gen` is not in your `$PATH`, use the full path (e.g., `/Users/you/.cargo/bin/localgpt-gen`) in the MCP server configuration
- **One instance**: Each standalone `mcp-server` spawns its own Bevy window. If a Gen window is already open with `--mcp-relay`, use `--connect` to reach it instead (see [MCP Relay](/docs/gen/cli-mode))
- **Screenshots**: the agent can take screenshots via `gen_screenshot` to see what it built and course-correct
