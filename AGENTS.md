# AGENTS.md

> Agent and tool discovery document for LocalGPT.
> Follows the [AAIF AGENTS.md convention](https://github.com/anthropics/agents/blob/main/AGENTS.md).

## Project

**LocalGPT** is a local-only autonomous AI assistant written in Rust. It provides persistent markdown-based memory, configurable LLM providers, 3D scene generation (Bevy), procedural audio, and an MCP server for tool integration with external AI backends.

- License: Apache 2.0
- Repository: `localgpt/`
- Language: Rust
- Primary documentation: [CLAUDE.md](CLAUDE.md)

## MCP Servers

LocalGPT exposes two MCP server modes, both using JSON-RPC 2.0 over stdio.

### localgpt mcp-server (Core)

Memory and web tools for external AI backends.

```bash
localgpt mcp-server              # default agent ID "mcp"
localgpt mcp-server -a my-agent  # custom agent ID for memory indexing
```

**Exposed tools:** memory_search, memory_get, memory_save, memory_log, web_fetch, web_search, document_load, transcribe_audio (if STT configured).

### localgpt-gen mcp-server (Gen)

Full 3D scene generation tools plus all core tools.

```bash
localgpt-gen mcp-server
```

**Exposed tools:** All core tools above, plus 70+ gen tools for scene manipulation, audio, behaviors, world management, physics, terrain, interactions, UI, worldgen pipeline, asset generation, multimodal input, and experiments.

### Transport

| Transport | Status | Notes |
|-----------|--------|-------|
| stdio | Supported | JSON-RPC 2.0, one message per line |
| HTTP/SSE | Planned | Future server mode |

### Protocol Version

`2024-11-05`

### Client Configuration

Add to your AI tool's MCP config (e.g., Claude Desktop, VS Code, Zed):

```json
{
  "mcpServers": {
    "localgpt": {
      "command": "localgpt",
      "args": ["mcp-server"]
    },
    "localgpt-gen": {
      "command": "localgpt-gen",
      "args": ["mcp-server"]
    }
  }
}
```

## Tool Catalog

### Core: Memory (Read)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `memory_search` | Search the memory index (FTS5 + vector) for relevant information | `query` | `limit` (default: 5) |
| `memory_get` | Read lines from MEMORY.md or memory/*.md; use after memory_search | `path` | `from` (line number), `lines` (default: 50) |

### Core: Memory (Write)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `memory_save` | Append content to MEMORY.md (long-term curated knowledge) | `content` | -- |
| `memory_log` | Append an entry to today's daily log (memory/YYYY-MM-DD.md) | `content` | -- |

### Core: Web & Documents

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `web_fetch` | Fetch and extract content from a URL (SSRF-protected) | `url` | -- |
| `web_search` | Search the web for current information | `query` | `count` (1-10, default: 5) |
| `document_load` | Extract text from PDF, DOCX, EPUB, or HTML files | `path` | -- |
| `transcribe_audio` | Transcribe audio (MP3, M4A, WAV, OGG, FLAC, WEBM) to text | `path` | `language` (ISO 639-1) |

### Gen: Scene Query

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_scene_info` | Get complete scene hierarchy with all entities, transforms, and materials | -- | -- |
| `gen_entity_info` | Get detailed information about a specific entity | `name` | -- |
| `gen_screenshot` | Capture a screenshot with optional highlighting and annotations | -- | `width`, `height`, `highlight_entity`, `highlight_color`, `camera_angle` (current/top_down/isometric/front/entity_focus), `include_annotations`, `wait_frames` |

### Gen: Entity Manipulation

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_spawn_primitive` | Spawn a 3D primitive shape (Cuboid, Sphere, Cylinder, Cone, Capsule, Torus, Plane, Pyramid, Tetrahedron, Icosahedron, Wedge) | `name`, `shape` | `dimensions`, `position`, `rotation_degrees`, `scale`, `color`, `metallic`, `roughness`, `emissive`, `alpha_mode`, `unlit`, `parent` |
| `gen_modify_entity` | Modify properties of an existing entity (partial update) | `name` | `position`, `rotation_degrees`, `scale`, `color`, `metallic`, `roughness`, `emissive`, `alpha_mode`, `unlit`, `double_sided`, `reflectance`, `visible`, `parent` |
| `gen_delete_entity` | Delete an entity and all its children | `name` | -- |
| `gen_spawn_batch` | Spawn multiple primitives in a single call | `entities` (array) | -- |
| `gen_modify_batch` | Modify multiple entities in a single call | `entities` (array) | -- |
| `gen_delete_batch` | Delete multiple entities in a single call | `names` (array) | -- |
| `gen_spawn_mesh` | Create custom geometry from raw vertex data | `name`, `vertices`, `indices` | `normals`, `uvs`, `color`, `metallic`, `roughness`, `position`, `rotation_degrees`, `scale`, `parent`, `emissive`, `alpha_mode`, `unlit`, `double_sided`, `reflectance` |
| `gen_load_gltf` | Load a glTF/GLB file into the scene | `path` | decompose options |

### Gen: Camera, Lighting & Environment

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_set_camera` | Set camera position and look-at target | -- | `position`, `look_at`, `fov_degrees` |
| `gen_set_light` | Add or update a light source (directional, point, spot) | `name` | `light_type`, `color`, `intensity`, `position`, `direction`, `shadows`, `range`, `outer_angle`, `inner_angle` |
| `gen_set_environment` | Set background color and ambient light | -- | `background_color`, `ambient_light`, `ambient_color` |

### Gen: Audio

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_set_ambience` | Set global ambient soundscape (wind, rain, forest, ocean, cave, stream, silence) | `layers` (array) | `master_volume` |
| `gen_audio_emitter` | Create a spatial audio emitter (water, fire, hum, wind, custom waveforms) | `name`, `sound` | `entity`, `position`, `radius`, `volume` |
| `gen_modify_audio` | Modify an existing audio emitter | `name` | `volume`, `radius`, `sound` |
| `gen_audio_info` | Get current audio state (layers, emitters, volumes) | -- | -- |

### Gen: Behaviors

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_add_behavior` | Add a continuous behavior (orbit, spin, bob, look_at, pulse, path_follow, bounce) | `entity`, `behavior` | `behavior_id` |
| `gen_remove_behavior` | Remove behaviors from an entity | `entity` | `behavior_id` (omit to remove all) |
| `gen_list_behaviors` | List all active behaviors | -- | `entity` (filter) |
| `gen_pause_behaviors` | Pause or resume all behaviors | `paused` | -- |

### Gen: World Management

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_save_world` | Save scene as a world skill (world.ron + SKILL.md) | `name` | `description`, `path` |
| `gen_load_world` | Load a world skill, restoring scene, behaviors, audio, and camera | `path` | `clear` (default: true) |
| `gen_export_world` | Export world to glTF for external viewers | -- | `format` (glb/gltf) |
| `gen_export_html` | Export world as self-contained HTML with Three.js | -- | -- |
| `gen_export_screenshot` | Render a high-resolution image to a file | -- | `path`, `width`, `height` |
| `gen_export_gltf` | Export the current scene as a glTF binary (.glb) | -- | `path` |
| `gen_fork_world` | Copy a world skill to a new name with attribution | `source`, `new_name` | -- |
| `gen_clear_scene` | Remove all entities, stop audio, reset behaviors | -- | `keep_camera`, `keep_lights` |
| `gen_undo` | Undo the last scene edit | -- | -- |
| `gen_redo` | Redo the last undone edit | -- | -- |
| `gen_undo_info` | Show undo/redo stack state | -- | -- |

### Gen: Avatar & Characters (P1)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_spawn_player` | Spawn a controllable player character with movement and collision | -- | `position`, `rotation`, `walk_speed`, `run_speed`, `jump_force`, `camera_mode`, `camera_distance`, `collision_radius`, `collision_height` |
| `gen_set_spawn_point` | Set a spawn/respawn location | `position` | `rotation`, `name`, `is_default` |
| `gen_add_npc` | Create an NPC with optional patrol or wander behavior | `position`, `name` | `model`, `behavior` (idle/patrol/wander), `patrol_points`, `patrol_speed`, `dialogue_id` |
| `gen_set_npc_dialogue` | Attach a branching conversation tree to an NPC | `npc_id`, `nodes`, `start_node` | `trigger` (proximity/click), `trigger_radius` |
| `gen_set_camera_mode` | Switch camera mode (first_person, third_person, top_down, fixed) | `mode` | `distance`, `pitch`, `fov`, `transition_duration`, `fixed_position`, `fixed_look_at` |

### Gen: NPC Intelligence (AI2)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_set_npc_brain` | Attach an AI brain (local SLM) for autonomous NPC decisions | `entity` | `personality`, `model` (default: llama3.2:3b), `tick_rate`, `perception_radius`, `goals`, `knowledge` |
| `gen_npc_observe` | Make an NPC observe the scene from its perspective | `entity` | `question`, `fov`, `resolution` |
| `gen_set_npc_memory` | Configure persistent memory for an NPC | `entity` | `capacity`, `initial_memories`, `auto_memorize` |

### Gen: Interaction & Triggers (P2)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_add_trigger` | Add trigger+action to an entity (proximity, click, area, collision, timer) | `entity_id`, `trigger_type` | action params vary by type |
| `gen_add_teleporter` | Create a portal that teleports the player on contact | entity/position params | destination, visual effect params |
| `gen_add_collectible` | Make an entity collectible with score and pickup effects | entity params | `score`, respawn options |
| `gen_add_door` | Add interactive door with open/close and optional key requirement | entity params | key, animation params |
| `gen_link_entities` | Wire one entity's event to trigger another entity's action | entity pair params | -- |

### Gen: Terrain & Landscape (P3)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_add_terrain` | Generate procedural terrain from noise with collision mesh | -- | `size`, `resolution`, noise params |
| `gen_add_water` | Create animated transparent water plane | -- | `height`, `size`, visual params |
| `gen_add_path` | Create walkable path between waypoints | waypoints | `width`, terrain-conform params |
| `gen_add_foliage` | Scatter vegetation via Poisson disk sampling (trees, bushes, grass, flowers, rocks) | -- | density, distribution params |
| `gen_set_sky` | Configure sky, sun direction, ambient light, and fog | -- | sky, sun, fog params |
| `gen_query_terrain_height` | Query terrain Y height at (x, z) coordinates | coordinates | -- |

### Gen: In-World UI (P4)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_add_sign` | Place readable text in the world as a sign or billboard | `position`, `text` | style, size params |
| `gen_add_hud` | Add persistent screen-space HUD element (score, health, timer, text) | display params | position, style params |
| `gen_add_label` | Attach a floating name label to an entity (billboards toward camera) | entity, text params | style params |
| `gen_add_tooltip` | Add contextual tooltip shown on proximity or look-at | entity, text params | trigger params |
| `gen_add_notification` | Show transient notification (toast, banner, achievement) | message params | animation, duration params |

### Gen: Physics (P5)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_set_physics` | Enable physics on an entity (dynamic/static/kinematic) | `entity_id` | `body_type`, `mass`, `friction`, `bounciness`, `damping` |
| `gen_add_collider` | Add collision shape (box, sphere, capsule, cylinder, mesh) | `entity_id` | `shape`, `sensor` (trigger-only) |
| `gen_add_joint` | Create physical constraint between entities (fixed, revolute, spherical, prismatic, spring) | `entity_a`, `entity_b`, `joint_type` | joint-specific params |
| `gen_add_force` | Create force field or impulse (directional, point_attract, point_repel, vortex, impulse) | `force_type` | direction, strength params |
| `gen_set_gravity` | Set gravity globally or per zone (presets: earth, moon, mars, jupiter, zero) | -- | `preset`, `direction`, `strength` |

### Gen: WorldGen Pipeline (WG)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_plan_layout` | Generate a world layout plan (BlockoutSpec) from text description | `prompt` | `size` |
| `gen_apply_blockout` | Generate 3D blockout scene from a BlockoutSpec | spec params | -- |
| `gen_populate_region` | Fill a blockout region with 3D content | region params | density params |
| `gen_set_tier` | Set entity placement tier (hero, medium, decorative) | entity, tier params | -- |
| `gen_set_role` | Set entity semantic role (ground, structure, prop, vegetation, etc.) | entity, role params | -- |
| `gen_bulk_modify` | Modify all entities matching a role (scale, recolor, remove, hide, show) | role, action params | region filter |
| `gen_edit_blockout` | Add, remove, resize, or move blockout regions | edit params | -- |
| `gen_evaluate` | Capture screenshot and scene metadata for quality evaluation | -- | camera params |
| `gen_refine` | Iteratively evaluate and refine the scene | -- | iteration params |
| `gen_build_navmesh` | Generate navigation mesh from scene geometry | -- | resolution params |
| `gen_check_traversability` | Check scene traversability between points | -- | point params |
| `gen_edit_navmesh` | Manually mark walkable/blocked areas on navmesh | edit params | -- |
| `gen_regenerate_dirty` | Regenerate content in modified blockout regions | -- | -- |
| `gen_depth_preview` | Render depth map of scene for preview generation | -- | camera, resolution params |
| `gen_styled_preview` | Generate styled 2D preview from depth map + text prompt | -- | style params |

### Gen: Multi-File WorldGen & Sync

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_write_world_plan` | Create SKILL.md, world.md, and root world.ron from a structured plan | `name`, `description` | `generation_strategy` |
| `gen_write_region` | Write a .md + .ron file pair for a region | region params | `flush` |
| `gen_load_region` | Load a region .ron and spawn its entities into the scene | region params | -- |
| `gen_unload_region` | Remove all entities belonging to a region from the scene | region params | -- |
| `gen_persist_blockout` | Save BlockoutSpec to layout/blockout.md and layout/blockout.ron | -- | -- |
| `gen_write_behaviors` | Write a behavior library .md + .ron file pair | behavior params | -- |
| `gen_write_audio` | Write an audio spec .md + .ron file pair | audio params | -- |
| `gen_check_drift` | Compare .md, .ron, and live scene for inconsistencies | -- | -- |
| `gen_sync` | Reconcile drift between .md, .ron, and live scene | source-of-truth params | `preview` |
| `gen_generation_status` | Report current generation phase and progress | -- | -- |

### Gen: AI Asset Generation (AI1)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_generate_asset` | Generate a 3D mesh from text prompt via local AI model server | `prompt`, `name` | `reference_image`, `position`, model params |
| `gen_generate_texture` | Generate PBR textures for an entity from text prompt | entity, prompt params | style params |
| `gen_generation_status` | Check status of AI asset generation tasks | -- | `task_id`, `action` (cancel) |

### Gen: Multimodal Input (AI3)

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_image_to_layout` | Analyze reference image and generate matching blockout layout | `image` | `prompt`, `scale` |
| `gen_match_style` | Adjust scene materials and lighting to match a reference image style | image params | -- |
| `gen_reference_board` | Manage reference images (mood board) for the generation session | action params | -- |
| `gen_panorama_to_world` | Generate explorable 3D world from a 360-degree panorama image | image params | -- |

### Gen: Experiments

| Tool | Description | Required Params | Optional Params |
|------|-------------|-----------------|-----------------|
| `gen_queue_experiment` | Queue a world generation experiment for background processing | `prompt` | variation params |
| `gen_list_experiments` | List all queued, running, and completed experiments | -- | filter params |
| `gen_experiment_status` | Get detailed status of a specific experiment | experiment ID params | -- |

## Authentication & Security

- **Local-only by default.** LocalGPT runs on-device with no cloud dependency.
- **No authentication for stdio MCP.** The MCP server inherits the user's OS-level permissions.
- **Tool safety split.** The MCP server exposes only safe tools (memory read/write, web fetch, web search) plus gen tools. Dangerous tools (bash, read_file, write_file, edit_file) are excluded because external backends provide their own.
- **SSRF protection.** `web_fetch` validates URLs against deny lists and DNS/IP checks to prevent server-side request forgery.
- **Workspace sandbox.** `memory_get` and `memory_save` are restricted to the configured workspace directory. Path traversal is rejected.

## Capabilities

```json
{
  "tools": {}
}
```

The server advertises the `tools` capability. Resources and prompts are not currently exposed.

## Related Files

- `CLAUDE.md` -- Build commands, architecture, feature flags, key patterns
- `docs/gen-audio.md` -- Audio system architecture and usage
- `docs/gen/external-services.md` -- External service setup (Ollama, ComfyUI)
- `config.example.toml` -- Configuration reference
