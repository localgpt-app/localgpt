---
sidebar_position: 14.1
---

# Gen Tools Reference

The gen agent has access to 32 specialized tools organized by category. When running as an [MCP server](/docs/gen/mcp-server), 25 additional tools are available for characters, interactions, terrain, UI, and physics.

## Scene Query

| Tool | Description |
|------|-------------|
| `gen_scene_info` | Get complete scene hierarchy |
| `gen_screenshot` | Capture viewport screenshot |
| `gen_entity_info` | Get detailed info about a named entity |

## Entity Creation

| Tool | Description |
|------|-------------|
| `gen_spawn_primitive` | Spawn geometric primitives (sphere, cube, cylinder, torus, pyramid, etc.) |
| `gen_spawn_batch` | Spawn multiple primitives in a single batch call |
| `gen_spawn_mesh` | Spawn custom mesh geometry from raw vertex data |
| `gen_load_gltf` | Load entities from a glTF/GLB file |

## Entity Modification

| Tool | Description |
|------|-------------|
| `gen_modify_entity` | Modify entity transform, material, or visibility |
| `gen_modify_batch` | Modify multiple entities in a single batch call |
| `gen_delete_entity` | Remove an entity and its children |
| `gen_delete_batch` | Delete multiple entities in a single batch call |

## Camera & Environment

| Tool | Description |
|------|-------------|
| `gen_set_camera` | Position and orient the camera |
| `gen_set_light` | Configure scene lighting |
| `gen_set_environment` | Set background color and ambient light |

## Export

| Tool | Description |
|------|-------------|
| `gen_export_screenshot` | Export high-res image to file |
| `gen_export_gltf` | Export scene as glTF/GLB file |
| `gen_export_world` | Export world with localized mesh assets for portability |
| `gen_export_html` | Export world as self-contained HTML with Three.js (includes procedural audio via Web Audio API) |

## Behaviors

Data-driven animations that stack on entities — no scripting required. See [Behaviors](/docs/gen/behaviors) for details.

| Tool | Description |
|------|-------------|
| `gen_add_behavior` | Add a behavior (orbit, spin, bob, look_at, pulse, path_follow, bounce) |
| `gen_remove_behavior` | Remove a behavior from an entity |
| `gen_list_behaviors` | List all behaviors on an entity |
| `gen_pause_behaviors` | Pause or resume all behaviors globally |

## Audio

Procedural environmental audio with spatial emitters. See [Audio](/docs/gen/audio) for details.

| Tool | Description |
|------|-------------|
| `gen_set_ambience` | Set ambient soundscape (wind, rain, forest, ocean, cave, stream) |
| `gen_audio_emitter` | Attach a sound emitter to an entity |
| `gen_modify_audio` | Modify an existing audio emitter |
| `gen_audio_info` | Get audio system status |

## World Skills

Save and load complete worlds as reusable skills. See [World Skills](/docs/gen/world-skills) for details.

| Tool | Description |
|------|-------------|
| `gen_save_world` | Save scene, behaviors, and audio to a skill directory |
| `gen_load_world` | Load a saved world (clears existing scene by default) |
| `gen_clear_scene` | Clear all entities, behaviors, and audio |

## Undo/Redo

| Tool | Description |
|------|-------------|
| `gen_undo` | Undo the last scene edit (spawn, delete, or modify) |
| `gen_redo` | Redo a previously undone edit |
| `gen_undo_info` | Show undo stack status and available operations |
