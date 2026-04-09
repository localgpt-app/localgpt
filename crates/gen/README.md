# LocalGPT Gen

AI-driven 3D world builder with procedural audio and entity behaviors, built on Bevy. Exposes 80+ MCP tools for scene creation, manipulation, and export.

## Install

```bash
cargo install localgpt-gen
```

Or via Docker:

```bash
docker pull ghcr.io/localgpt-app/localgpt-gen:latest
```

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

- **Bevy 0.18** — real-time 3D rendering with PBR materials, dynamic lighting, and an interactive viewport
- **Procedural Audio** — algorithmic soundscapes (wind, rain, forest, ocean, cave) and spatial emitters via FunDSP
- **Entity Behaviors** — composable animations (orbit, spin, bob, bounce, path_follow) with no scripting required
- **NPCs** — characters with patrol AI, branching dialogue trees, and memory
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
| WorldGen | plan_layout, apply_blockout, populate_region, evaluate, navmesh, ... | 15 |
| Export | export_screenshot, export_gltf, export_html, save/load/export_world | 6 |
| History | undo, redo, undo_info | 3 |
| Memory | memory_search, memory_get, memory_save, memory_log | 4 |
| Web | web_fetch, web_search | 2 |

## License

Apache-2.0
