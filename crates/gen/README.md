# LocalGPT Gen

The 3D world builder with procedural audio and entity behaviors, built on Bevy, driven by the coding agents you already use: 88 MCP tools for scene creation, manipulation, and export. It runs no model, calls no service and holds no key.

## Install

```bash
cargo install localgpt-gen
```

Or via Docker:

```bash
docker pull ghcr.io/localgpt-app/localgpt-gen:latest
```

## The agent and the window

Gen takes no prompt of its own: an agent outside the window drives it.
`localgpt-gen --mcp-relay` opens the window with a relay beside it, and the
agent's MCP server is `localgpt-gen mcp-server --connect`, so its tool calls
reach the window you are looking at; `localgpt-gen mcp-server` alone gives the
agent a window of its own, `--headless` none, and `--mcp-http <port>` adds the
streamable HTTP transport. `--live <folder>` opens a `.world` package as a
live canvas with an ops API, for an agent with a shell (`localgpt world`).
Gen draws no panels of its own. The windowed app is the LocalGPT app
(`crates/app`), which opens documents, songs and `.world` packages as worlds,
with terminals beside the world for whatever agent you run; its macOS bundle
is built by `apps/app-desktop/` in the repository.

## MCP Server Setup

LocalGPT Gen runs as an MCP server over stdio. Connect it to any MCP-compatible client:

### Claude CLI

```bash
claude mcp add localgpt-gen -- localgpt-gen mcp-server
```

### Gemini CLI

```bash
gemini mcp add --name localgpt-gen -- localgpt-gen mcp-server
```

### VS Code (GitHub Copilot)

Add to `.vscode/mcp.json`:

```json
{
  "servers": {
    "localgpt-gen": {
      "command": "localgpt-gen",
      "args": ["mcp-server"]
    }
  }
}
```

### Cursor

Add to `.cursor/mcp.json`:

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

### Zed

Add to `~/.config/zed/settings.json`:

```json
{
  "context_servers": {
    "localgpt-gen": {
      "command": { "path": "localgpt-gen", "args": ["mcp-server"] }
    }
  }
}
```

### Docker (any client)

```bash
docker run --rm -i ghcr.io/localgpt-app/localgpt-gen:latest mcp-server --headless
```

### HTTP Transport

```bash
localgpt-gen mcp-server --mcp-http 8080
# Connect to http://127.0.0.1:8080/mcp
```

## Example Prompts

**Medieval Village**
> Build a medieval village with a central market square, a church with a bell tower, several timber-frame houses, cobblestone paths connecting them, and a river with a stone bridge on the east side. Add ambient forest sounds and a campfire with crackling audio near the inn.

**Sci-Fi Outpost**
> Create a desert sci-fi outpost: a landing pad with a hovering shuttle, two domed habitats connected by a glass walkway, solar panel arrays, and a watchtower. Add a dust storm wind ambience and a humming generator emitter on the power station.

**NPC Patrol Route**
> Spawn 3 guard NPCs near the castle gate. Give each a patrol behavior following a triangular path around the walls. Add a trigger zone at the main gate that shows a "Halt! State your business." dialogue when the player approaches.

## Features

- **Bevy 0.20** — real-time 3D rendering with PBR materials, dynamic lighting, and an interactive viewport
- **Procedural Audio** — algorithmic soundscapes (wind, rain, forest, ocean, cave) and spatial emitters via FunDSP
- **Entity Behaviors** — composable animations (orbit, spin, bob, bounce, path_follow) with no scripting required
- **NPCs** — characters with patrol and wander behavior and branching dialogue trees
- **Physics** — rigid bodies, colliders, joints, and force fields via Avian3D
- **Interaction** — triggers, teleporters, collectibles, doors, and entity event wiring
- **Terrain** — heightmap terrain, water bodies, paths, and procedural foliage
- **WorldGen Pipeline** — blockout-first workflow with navmesh, hierarchical placement, and LLM self-evaluation
- **Export** — glTF/GLB, self-contained HTML (Three.js), screenshots, and world skill files (.ron)
- **Undo/Redo** — full operation history

## Tool Categories

| Category | Tools | Count |
|----------|-------|-------|
| Scene | scene_info, entity_info, clear_scene | 3 |
| Spawn | spawn_primitive, spawn_batch, spawn_mesh, load_gltf | 4 |
| Modify | modify_entity, modify_batch, delete_entity, delete_batch | 4 |
| Camera | set_camera, set_camera_mode | 2 |
| Lighting | set_light, set_environment, set_sky | 3 |
| Audio | set_ambience, audio_emitter, modify_audio, audio_info | 4 |
| Behavior | add/remove/list/pause_behaviors | 4 |
| Character | spawn_player, set_spawn_point, add_npc, set_npc_dialogue | 4 |
| Interaction | add_trigger, add_teleporter, add_collectible, add_door, link_entities | 5 |
| Terrain | add_terrain, add_water, add_path, add_foliage | 4 |
| UI | add_sign, add_hud, add_label, add_tooltip, add_notification | 5 |
| Physics | set_physics, add_collider, add_joint, add_force, set_gravity | 5 |
| WorldGen | plan_layout, apply_blockout, populate_region, evaluate, navmesh, render_depth, ... | 14 |
| Export | export_screenshot, export_gltf, export_html, save/load/export_world | 6 |
| History | undo, redo, undo_info | 3 |
| Creations | define_creation, spawn_instance | 2 |
| Multi-file worlds | write_world_plan, write_region, load/unload_region, persist_blockout, write_behaviors, write_audio, check_drift, sync | 9 |
| Image input | image_to_layout, match_style, reference_board, panorama_to_world (pure image analysis, no service) | 4 |

No memory, web or file tools: the agent brings its own. No tool calls a
model or a service: nothing here needs the network.

## License

Apache-2.0
