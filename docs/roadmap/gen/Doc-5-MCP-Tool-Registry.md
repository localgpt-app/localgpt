# MCP Tool Specification Registry

## Summary

> **2026-11-01.** The registered set is 88 tools (`AGENTS.md` at the
> repository root is the current catalog). Rows below marked *Removed*
> went that day with the services they called: `gen_set_npc_brain`,
> `gen_npc_observe` and `gen_set_npc_memory` (Ollama), `gen_generate_asset`,
> `gen_generate_texture` and `gen_generation_status` (a model server),
> `gen_preview_world` (ComfyUI), and the experiment tools (the in-process
> agent loop). The rest of this document is the record of what was built.

**Total tools:** 84 (as written; 88 registered today)
**Categories:** Scene (16), Export (4), Audio (4), Behavior (4), World (5), Physics (5), Character (8), Interaction (5), Terrain (6), UI (5), WorldGen (15), MultiFile (10), AssetGen (3), Experiment (3)

**Source files:**
- `crates/gen/src/gen3d/tools.rs` -- `create_gen_tools()` (P0: 38 tools)
- `crates/gen/src/mcp/avatar_tools.rs` -- `create_character_tools()` (P1: 8 tools)
- `crates/gen/src/mcp/interaction_tools.rs` -- `create_interaction_tools()` (P2: 5 tools)
- `crates/gen/src/mcp/terrain_tools.rs` -- `create_terrain_tools()` (P3: 6 tools)
- `crates/gen/src/mcp/ui_tools.rs` -- `create_ui_tools()` (P4: 5 tools)
- `crates/gen/src/mcp/physics_tools.rs` -- `create_physics_tools()` (P5: 5 tools)
- `crates/gen/src/mcp/worldgen_tools.rs` -- `create_worldgen_tools()` (WG: 15 tools)
- `crates/gen/src/mcp/multifile_tools.rs` -- `create_multifile_tools()` (MF: 10 tools)
- `crates/gen/src/mcp/asset_gen_tools.rs` -- `create_asset_gen_tools()` (AI1: 3 tools)
- `crates/gen/src/mcp/experiment_tools.rs` -- `create_experiment_tools()` (EXP: 3 tools)

---

## Tool Index

| # | Name | Category | Phase | Status |
|---|------|----------|-------|--------|
| 1 | `gen_scene_info` | Scene | P0 | Implemented |
| 2 | `gen_screenshot` | Scene | P0 | Implemented |
| 3 | `gen_entity_info` | Scene | P0 | Implemented |
| 4 | `gen_spawn_primitive` | Scene | P0 | Implemented |
| 5 | `gen_modify_entity` | Scene | P0 | Implemented |
| 6 | `gen_delete_entity` | Scene | P0 | Implemented |
| 7 | `gen_spawn_batch` | Scene | P0 | Implemented |
| 8 | `gen_modify_batch` | Scene | P0 | Implemented |
| 9 | `gen_delete_batch` | Scene | P0 | Implemented |
| 10 | `gen_set_camera` | Scene | P0 | Implemented |
| 11 | `gen_set_light` | Scene | P0 | Implemented |
| 12 | `gen_set_environment` | Scene | P0 | Implemented |
| 13 | `gen_spawn_mesh` | Scene | P0 | Implemented |
| 14 | `gen_load_gltf` | Scene | P0 | Implemented |
| 15 | `gen_clear_scene` | Scene | P0 | Implemented |
| 16 | `gen_undo` | Scene | P0 | Implemented |
| 17 | `gen_redo` | Scene | P0 | Implemented |
| 18 | `gen_undo_info` | Scene | P0 | Implemented |
| 19 | `gen_export_screenshot` | Export | P0 | Implemented |
| 20 | `gen_export_gltf` | Export | P0 | Implemented |
| 21 | `gen_export_world` | Export | P0 | Implemented |
| 22 | `gen_export_html` | Export | P0 | Implemented |
| 23 | `gen_set_ambience` | Audio | P0 | Implemented |
| 24 | `gen_audio_emitter` | Audio | P0 | Implemented |
| 25 | `gen_modify_audio` | Audio | P0 | Implemented |
| 26 | `gen_audio_info` | Audio | P0 | Implemented |
| 27 | `gen_add_behavior` | Behavior | P0 | Implemented |
| 28 | `gen_remove_behavior` | Behavior | P0 | Implemented |
| 29 | `gen_list_behaviors` | Behavior | P0 | Implemented |
| 30 | `gen_pause_behaviors` | Behavior | P0 | Implemented |
| 31 | `gen_save_world` | World | P0 | Implemented |
| 32 | `gen_load_world` | World | P0 | Implemented |
| 33 | `gen_fork_world` | World | P0 | Implemented |
| 34 | `gen_set_physics` | Physics | P0/P5 | Implemented |
| 35 | `gen_add_collider` | Physics | P0/P5 | Implemented |
| 36 | `gen_add_joint` | Physics | P0/P5 | Implemented |
| 37 | `gen_add_force` | Physics | P0/P5 | Implemented |
| 38 | `gen_set_gravity` | Physics | P0/P5 | Implemented |
| 39 | `gen_spawn_player` | Character | P1 | Implemented |
| 40 | `gen_set_spawn_point` | Character | P1 | Implemented |
| 41 | `gen_add_npc` | Character | P1 | Implemented |
| 42 | `gen_set_npc_dialogue` | Character | P1 | Implemented |
| 43 | `gen_set_camera_mode` | Character | P1 | Implemented |
| 44 | `gen_set_npc_brain` | Character | AI2 | Removed 2026-11-01 |
| 45 | `gen_npc_observe` | Character | AI2 | Removed 2026-11-01 |
| 46 | `gen_set_npc_memory` | Character | AI2 | Removed 2026-11-01 |
| 47 | `gen_add_trigger` | Interaction | P2 | Implemented |
| 48 | `gen_add_teleporter` | Interaction | P2 | Implemented |
| 49 | `gen_add_collectible` | Interaction | P2 | Implemented |
| 50 | `gen_add_door` | Interaction | P2 | Implemented |
| 51 | `gen_link_entities` | Interaction | P2 | Implemented |
| 52 | `gen_add_terrain` | Terrain | P3 | Implemented |
| 53 | `gen_add_water` | Terrain | P3 | Implemented |
| 54 | `gen_add_path` | Terrain | P3 | Implemented |
| 55 | `gen_add_foliage` | Terrain | P3 | Implemented |
| 56 | `gen_set_sky` | Terrain | P3 | Implemented |
| 57 | `gen_query_terrain_height` | Terrain | P3 | Implemented |
| 58 | `gen_add_sign` | UI | P4 | Implemented |
| 59 | `gen_add_hud` | UI | P4 | Implemented |
| 60 | `gen_add_label` | UI | P4 | Implemented |
| 61 | `gen_add_tooltip` | UI | P4 | Implemented |
| 62 | `gen_add_notification` | UI | P4 | Implemented |
| 63 | `gen_plan_layout` | WorldGen | WG1 | Implemented |
| 64 | `gen_apply_blockout` | WorldGen | WG1 | Implemented |
| 65 | `gen_populate_region` | WorldGen | WG1 | Implemented |
| 66 | `gen_set_tier` | WorldGen | WG3 | Implemented |
| 67 | `gen_set_role` | WorldGen | WG3 | Implemented |
| 68 | `gen_bulk_modify` | WorldGen | WG3 | Implemented |
| 69 | `gen_modify_blockout` | WorldGen | WG5 | Implemented |
| 70 | `gen_evaluate_scene` | WorldGen | WG4 | Implemented |
| 71 | `gen_auto_refine` | WorldGen | WG4 | Implemented |
| 72 | `gen_build_navmesh` | WorldGen | WG2 | Implemented |
| 73 | `gen_validate_navigability` | WorldGen | WG2 | Implemented |
| 74 | `gen_edit_navmesh` | WorldGen | WG5 | Implemented |
| 75 | `gen_regenerate` | WorldGen | WG5 | Implemented |
| 76 | `gen_render_depth` | WorldGen | WG7 | Implemented |
| 77 | `gen_preview_world` | WorldGen | WG7 | Removed 2026-11-01 |
| 78 | `gen_write_world_plan` | MultiFile | MF | Implemented |
| 79 | `gen_write_region` | MultiFile | MF | Implemented |
| 80 | `gen_load_region` | MultiFile | MF | Implemented |
| 81 | `gen_unload_region` | MultiFile | MF | Implemented |
| 82 | `gen_persist_blockout` | MultiFile | MF | Implemented |
| 83 | `gen_write_behaviors` | MultiFile | MF | Implemented |
| 84 | `gen_write_audio` | MultiFile | MF | Implemented |
| 85 | `gen_check_drift` | MultiFile | MF | Implemented |
| 86 | `gen_sync` | MultiFile | MF | Implemented |
| 87 | `gen_generation_status` | MultiFile | MF | Removed 2026-11-01 |
| 88 | `gen_generate_asset` | AssetGen | AI1 | Removed 2026-11-01 |
| 89 | `gen_generate_texture` | AssetGen | AI1 | Removed 2026-11-01 |
| 90 | `gen_generation_status` | AssetGen | AI1 | Removed 2026-11-01 |
| 91 | `gen_queue_experiment` | Experiment | EXP | Implemented |
| 92 | `gen_list_experiments` | Experiment | EXP | Implemented |
| 93 | `gen_experiment_status` | Experiment | EXP | Implemented |

---

## Tool Specifications

### Category: Scene (P0 -- Core Scene Management)

---

#### `gen_scene_info`

**Description:** Get complete scene hierarchy with all entities, transforms, and materials.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| *(none)* | | | | |

**Returns:** JSON with complete scene hierarchy.

**Example:**
```json
{}
```

---

#### `gen_screenshot`

**Description:** Capture a screenshot of the current scene. Supports entity highlighting, camera angle presets, and annotation overlays for visual evaluation.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `width` | integer | no | 800 | Image width in pixels |
| `height` | integer | no | 600 | Image height in pixels |
| `wait_frames` | integer | no | 3 | Frames to wait before capture for render pipeline to process new geometry |
| `highlight_entity` | string | no | | Entity name to highlight with emissive glow |
| `highlight_color` | array[4] | no | [1.0, 0.0, 0.0, 1.0] | Highlight color as [r, g, b, a] |
| `camera_angle` | string | no | "current" | Preset: "current", "top_down", "isometric", "front", "entity_focus" |
| `include_annotations` | boolean | no | false | Overlay entity names as labels |

**Returns:** File path to saved screenshot image.

**Example:**
```json
{"width": 1024, "height": 768, "camera_angle": "isometric", "include_annotations": true}
```

---

#### `gen_entity_info`

**Description:** Get detailed information about a specific entity by name.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | Entity name to inspect |

**Returns:** JSON with entity transform, material, and component details.

**Example:**
```json
{"name": "red_cube"}
```

---

#### `gen_spawn_primitive`

**Description:** Spawn a 3D primitive shape with material and transform. Creates a fully visible object -- no additional components needed.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | Unique name (e.g., "red_cube", "table_leg_1") |
| `shape` | string | **yes** | | "Cuboid", "Sphere", "Cylinder", "Cone", "Capsule", "Torus", "Plane", "Pyramid", "Tetrahedron", "Icosahedron", "Wedge" |
| `dimensions` | object | no | | Shape-specific: Cuboid {x,y,z}, Sphere {radius}, Cylinder {radius, height}, etc. |
| `position` | array[3] | no | [0,0,0] | Position [x, y, z] |
| `rotation_degrees` | array[3] | no | [0,0,0] | Euler angles in degrees (pitch, yaw, roll) |
| `scale` | array[3] | no | [1,1,1] | Scale [x, y, z] |
| `color` | array[4] | no | [0.8,0.8,0.8,1.0] | RGBA color, 0.0-1.0 |
| `metallic` | number | no | 0.0 | Metallic factor (0-1) |
| `roughness` | number | no | 0.5 | Roughness factor (0-1) |
| `emissive` | array[4] | no | [0,0,0,0] | Emissive RGBA color for glowing objects |
| `alpha_mode` | string | no | "opaque" | "opaque", "blend", "mask:0.5", "add", "multiply" |
| `unlit` | boolean | no | | If true, ignore lighting (flat shaded) |
| `parent` | string | no | | Parent entity name for hierarchy |

**Returns:** Entity name and entity_id.

**Example:**
```json
{"name": "tower_base", "shape": "Cylinder", "dimensions": {"radius": 2, "height": 5}, "position": [0, 2.5, 0], "color": [0.6, 0.5, 0.4, 1.0]}
```

---

#### `gen_modify_entity`

**Description:** Modify properties of an existing entity. Only specified fields are changed; others remain unchanged.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | Name of entity to modify |
| `position` | array[3] | no | | New position [x, y, z] |
| `rotation_degrees` | array[3] | no | | New rotation [pitch, yaw, roll] in degrees |
| `scale` | array[3] | no | | New scale [x, y, z] |
| `color` | array[4] | no | | New RGBA color |
| `metallic` | number | no | | New metallic factor |
| `roughness` | number | no | | New roughness factor |
| `emissive` | array[4] | no | | New emissive RGBA color |
| `alpha_mode` | string | no | | "opaque", "blend", "add", "multiply" |
| `unlit` | boolean | no | | If true, material ignores lighting |
| `double_sided` | boolean | no | | Render both sides of faces |
| `reflectance` | number | no | | Specular reflectance (0-1) |
| `visible` | boolean | no | | Show/hide entity |
| `parent` | string | no | | Reparent to named entity, or null to unparent |

**Returns:** Confirmation of modification.

**Example:**
```json
{"name": "red_cube", "position": [3, 0, 0], "color": [0, 0, 1, 1]}
```

---

#### `gen_delete_entity`

**Description:** Delete an entity and all its children from the scene.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | Name of entity to delete |

**Returns:** Confirmation of deletion.

**Example:**
```json
{"name": "old_tree"}
```

---

#### `gen_spawn_batch`

**Description:** Spawn multiple primitives in a single call. More efficient than repeated gen_spawn_primitive calls.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entities` | array | **yes** | | Array of entity specs (same format as gen_spawn_primitive) |

**Returns:** Batch result with count and per-entity status.

**Example:**
```json
{"entities": [{"name": "pillar_1", "shape": "Cylinder", "position": [-3, 1, 0]}, {"name": "pillar_2", "shape": "Cylinder", "position": [3, 1, 0]}]}
```

---

#### `gen_modify_batch`

**Description:** Modify multiple entities in a single call. More efficient than repeated gen_modify_entity calls.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entities` | array | **yes** | | Array of entity modifications (same format as gen_modify_entity) |

**Returns:** Batch result with count and per-entity status.

**Example:**
```json
{"entities": [{"name": "pillar_1", "color": [1, 0, 0, 1]}, {"name": "pillar_2", "color": [0, 0, 1, 1]}]}
```

---

#### `gen_delete_batch`

**Description:** Delete multiple entities in a single call. More efficient than repeated gen_delete_entity calls.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `names` | array[string] | **yes** | | Array of entity names to delete |

**Returns:** Batch result with count and per-entity status.

**Example:**
```json
{"names": ["old_wall_1", "old_wall_2", "old_wall_3"]}
```

---

#### `gen_set_camera`

**Description:** Set camera position and target. The camera always looks at the target point.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `position` | array[3] | no | [5,5,5] | Camera position [x, y, z] |
| `look_at` | array[3] | no | [0,0,0] | Point camera looks at [x, y, z] |
| `fov_degrees` | number | no | 45 | Vertical field of view |

**Returns:** Confirmation.

**Example:**
```json
{"position": [10, 8, 10], "look_at": [0, 0, 0], "fov_degrees": 60}
```

---

#### `gen_set_light`

**Description:** Add or update a light source. Lighting is the primary driver of visual quality.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | Light name (e.g., "sun", "key_light") |
| `light_type` | string | no | "directional" | "directional", "point", "spot" |
| `color` | array[4] | no | [1,1,1,1] | RGBA light color |
| `intensity` | number | no | 1000 | Lumens for point/spot, lux for directional |
| `position` | array[3] | no | | Position for point/spot lights |
| `direction` | array[3] | no | | Direction for directional/spot lights |
| `shadows` | boolean | no | true | Enable shadows |
| `range` | number | no | | Max range for point/spot lights |
| `outer_angle` | number | no | | Outer cone angle in radians (spot only) |
| `inner_angle` | number | no | | Inner cone angle in radians (spot only) |

**Returns:** Light name confirmation.

**Example:**
```json
{"name": "sun", "light_type": "directional", "direction": [-0.5, -1, -0.3], "intensity": 10000, "shadows": true}
```

---

#### `gen_set_environment`

**Description:** Set global environment: background color, ambient light.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `background_color` | array[4] | no | | RGBA background color |
| `ambient_light` | number | no | 0.1 | Ambient light intensity 0.0-1.0 |
| `ambient_color` | array[4] | no | [1,1,1,1] | Ambient light RGBA color |

**Returns:** Confirmation.

**Example:**
```json
{"background_color": [0.1, 0.1, 0.15, 1.0], "ambient_light": 0.2}
```

---

#### `gen_spawn_mesh`

**Description:** Create custom geometry from raw vertex data. Use when primitives are insufficient.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | Entity name |
| `vertices` | array | **yes** | | Array of [x,y,z] vertex positions |
| `indices` | array[int] | **yes** | | Triangle indices (groups of 3) |
| `normals` | array | no | | Per-vertex normals (auto-computed if omitted) |
| `uvs` | array | no | | Per-vertex UV coordinates [u,v] |
| `color` | array[4] | no | [0.8,0.8,0.8,1.0] | RGBA color |
| `metallic` | number | no | 0.0 | Metallic factor |
| `roughness` | number | no | 0.5 | Roughness factor |
| `position` | array[3] | no | [0,0,0] | World position |
| `rotation_degrees` | array[3] | no | [0,0,0] | Euler angles in degrees |
| `scale` | array[3] | no | [1,1,1] | Scale |
| `parent` | string | no | | Parent entity name |
| `emissive` | array[4] | no | [0,0,0,0] | Emissive RGBA color |
| `alpha_mode` | string | no | | Transparency mode |
| `unlit` | boolean | no | | Ignore lighting |
| `double_sided` | boolean | no | | Render both sides |
| `reflectance` | number | no | | Specular reflectance (0-1) |

**Returns:** Entity name and entity_id.

**Example:**
```json
{"name": "custom_ramp", "vertices": [[0,0,0],[1,0,0],[1,1,0],[0,0,1],[1,0,1],[1,1,1]], "indices": [0,1,2,3,4,5], "color": [0.5,0.5,0.5,1]}
```

---

#### `gen_load_gltf`

**Description:** Load a glTF/GLB file from disk into the scene. Optionally decompose into editable sub-objects.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | **yes** | | Path to glTF/GLB file (absolute, relative, or filename to search in workspace) |
| `segment` | boolean | no | false | Decompose mesh into individually editable sub-objects |

**Returns:** Name and path of loaded asset.

**Example:**
```json
{"path": "castle.glb", "segment": true}
```

---

#### `gen_clear_scene`

**Description:** Clear the 3D scene. Removes all entities, stops audio, and resets behaviors.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `keep_camera` | boolean | no | true | Keep the camera |
| `keep_lights` | boolean | no | true | Keep lights |

**Returns:** Count of entities removed.

**Example:**
```json
{"keep_camera": true, "keep_lights": false}
```

---

#### `gen_undo`

**Description:** Undo the last scene edit (spawn, delete, or modify entity). Can be called multiple times.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| *(none)* | | | | |

**Returns:** Description of undone operation, or "Nothing to undo".

---

#### `gen_redo`

**Description:** Redo a previously undone scene edit. Only available after gen_undo.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| *(none)* | | | | |

**Returns:** Description of redone operation, or "Nothing to redo".

---

#### `gen_undo_info`

**Description:** Show the current undo/redo stack state: how many operations can be undone and redone.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| *(none)* | | | | |

**Returns:** Undo count, redo count, entity count, unsaved changes count.

---

### Category: Export (P0)

---

#### `gen_export_screenshot`

**Description:** Render a high-resolution image of the scene to a file.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | **yes** | | Output file path |
| `width` | integer | no | 1920 | Image width |
| `height` | integer | no | 1080 | Image height |

**Returns:** Output file path.

**Example:**
```json
{"path": "~/exports/scene.png", "width": 3840, "height": 2160}
```

---

#### `gen_export_gltf`

**Description:** Export the current scene as a glTF binary (.glb) file.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | no | | Output file path (.glb added if missing). Defaults to {workspace}/exports/{timestamp}.glb |

**Returns:** Output file path.

**Example:**
```json
{"path": "my_scene.glb"}
```

---

#### `gen_export_world`

**Description:** Export the current world to a glTF file for external viewers.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `format` | string | no | "glb" | "glb" (single binary) or "gltf" (JSON + BIN) |

**Returns:** Output file path.

**Example:**
```json
{"format": "glb"}
```

---

#### `gen_export_html`

**Description:** Export the current world as a self-contained HTML file using Three.js. Open in any browser -- no server required.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| *(none)* | | | | |

**Returns:** Output file path.

---

### Category: Audio (P0)

---

#### `gen_set_ambience`

**Description:** Set the global ambient soundscape. Replaces previous ambience. Each layer is a continuous procedural sound.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `layers` | array | **yes** | | Ambient sound layers to mix. Each: {name, sound: {type: "wind"/"rain"/"forest"/"ocean"/"cave"/"stream"/"silence", ...params}, volume: 0-1} |
| `master_volume` | number | no | 0.8 | Master volume for all audio (0-1) |

**Returns:** Confirmation.

**Example:**
```json
{"layers": [{"name": "wind", "sound": {"type": "wind", "speed": 0.6}, "volume": 0.4}, {"name": "birds", "sound": {"type": "forest", "bird_density": 0.3}, "volume": 0.3}]}
```

---

#### `gen_audio_emitter`

**Description:** Create a spatial audio emitter. Sound gets louder as camera approaches. Can attach to existing entity or use a position.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | Unique emitter name |
| `sound` | object | **yes** | | Sound type: water {turbulence}, fire {intensity, crackle}, hum {frequency, warmth}, wind {pitch}, custom {waveform, filter_cutoff, filter_type} |
| `entity` | string | no | | Existing entity name to attach to |
| `position` | array[3] | no | | Position for standalone emitter |
| `radius` | number | no | 10.0 | Maximum audible distance |
| `volume` | number | no | 0.7 | Base volume at closest distance (0-1) |

**Returns:** Emitter name confirmation.

**Example:**
```json
{"name": "campfire_sound", "entity": "campfire", "sound": {"type": "fire", "intensity": 0.8, "crackle": 0.6}, "radius": 15, "volume": 0.5}
```

---

#### `gen_modify_audio`

**Description:** Modify an existing audio emitter's volume, radius, or sound parameters.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | Name of audio emitter to modify |
| `volume` | number | no | | New base volume (0-1) |
| `radius` | number | no | | New audible radius |
| `sound` | object | no | | New sound type (replaces current) |

**Returns:** Confirmation.

**Example:**
```json
{"name": "campfire_sound", "volume": 0.8}
```

---

#### `gen_audio_info`

**Description:** Get current audio state: active layers, emitters with positions/volumes.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| *(none)* | | | | |

**Returns:** JSON with active audio layers and emitter details.

---

### Category: Behavior (P0)

---

#### `gen_add_behavior`

**Description:** Add a continuous behavior to an entity. Behaviors animate entities automatically each frame. Multiple behaviors can be stacked.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity` | string | **yes** | | Entity name |
| `behavior` | object | **yes** | | Behavior definition with type: "orbit", "spin", "bob", "look_at", "pulse", "path_follow", "bounce". Each type has specific params. |
| `behavior_id` | string | no | | Optional unique ID (auto-generated if omitted) |

**Returns:** Behavior ID and entity name.

**Example:**
```json
{"entity": "crystal", "behavior": {"type": "bob", "axis": [0,1,0], "amplitude": 0.5, "frequency": 0.8}}
```

---

#### `gen_remove_behavior`

**Description:** Remove behaviors from an entity. If behavior_id specified, removes only that; otherwise removes all.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity` | string | **yes** | | Entity name |
| `behavior_id` | string | no | | Specific behavior ID to remove (omit to remove all) |

**Returns:** Count of removed behaviors.

**Example:**
```json
{"entity": "crystal", "behavior_id": "bob_1"}
```

---

#### `gen_list_behaviors`

**Description:** List all active behaviors. Optionally filter by entity name.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity` | string | no | | Filter to specific entity |

**Returns:** JSON with behavior list.

**Example:**
```json
{"entity": "crystal"}
```

---

#### `gen_pause_behaviors`

**Description:** Pause or resume all behaviors. Entities freeze in place when paused.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `paused` | boolean | **yes** | | true to pause, false to resume |

**Returns:** Confirmation.

**Example:**
```json
{"paused": true}
```

---

### Category: World (P0)

---

#### `gen_save_world`

**Description:** Save the current scene as a world skill. Creates skill directory with world.ron and SKILL.md.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | World/skill name (used as directory name) |
| `description` | string | no | | Brief description for SKILL.md |
| `path` | string | no | | Custom output path. Default: {workspace}/skills/{name}/ |

**Returns:** Save path and skill name.

**Example:**
```json
{"name": "solar-system", "description": "A scale model of the solar system with orbiting planets"}
```

---

#### `gen_load_world`

**Description:** Load a world skill. Restores 3D scene, behaviors, audio, environment, and camera. Clears existing scene by default.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `path` | string | **yes** | | Path to world skill directory, or skill name |
| `clear` | boolean | no | true | Clear existing scene before loading |

**Returns:** Path, entity count, behavior count.

**Example:**
```json
{"path": "solar-system", "clear": true}
```

---

#### `gen_fork_world`

**Description:** Fork (copy) an existing world skill to a new name with attribution metadata.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `source` | string | **yes** | | Source world name or path |
| `new_name` | string | **yes** | | Name for the forked world |

**Returns:** Forked world path and skill name.

**Example:**
```json
{"source": "solar-system", "new_name": "solar-system-v2"}
```

---

### Category: Physics (P5)

---

#### `gen_set_physics`

**Description:** Enable physics simulation on an entity. Sets body type, mass, friction, bounciness, and damping.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity_id` | string | **yes** | | Target entity name |
| `body_type` | string | no | "dynamic" | "dynamic", "static", "kinematic" |
| `mass` | number | no | | Mass in kg (auto-calculated if omitted) |
| `restitution` | number | no | 0.3 | Bounciness (0-1) |
| `friction` | number | no | 0.5 | Surface friction (0-1) |
| `gravity_scale` | number | no | 1.0 | Gravity multiplier |
| `linear_damping` | number | no | 0.1 | Linear air resistance |
| `angular_damping` | number | no | 0.1 | Angular air resistance |
| `lock_rotation` | boolean | no | false | Prevent rotation |

**Returns:** Confirmation.

**Example:**
```json
{"entity_id": "ball", "body_type": "dynamic", "restitution": 0.9, "mass": 1.0}
```

---

#### `gen_add_collider`

**Description:** Add a collision shape to an entity. Can be a sensor (trigger-only, no physics response).

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity_id` | string | **yes** | | Target entity name |
| `shape` | string | no | "box" | "box", "sphere", "capsule", "cylinder", "mesh" |
| `size` | array[3] | no | | Dimensions [x, y, z] (auto-fit if omitted) |
| `offset` | array[3] | no | [0,0,0] | Offset from entity origin |
| `is_trigger` | boolean | no | false | Sensor only (detect overlap, no physics) |
| `visible_in_debug` | boolean | no | true | Show in debug view |

**Returns:** Confirmation.

**Example:**
```json
{"entity_id": "wall", "shape": "box", "size": [4, 3, 0.5]}
```

---

#### `gen_add_joint`

**Description:** Create a physical joint/constraint between two entities.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity_a` | string | **yes** | | First entity name |
| `entity_b` | string | **yes** | | Second entity name |
| `joint_type` | string | no | "fixed" | "fixed", "revolute" (hinge), "spherical" (ball), "prismatic" (slider), "spring" |
| `anchor_a` | array[3] | no | [0,0,0] | Anchor on entity A (local space) |
| `anchor_b` | array[3] | no | [0,0,0] | Anchor on entity B (local space) |
| `axis` | array[3] | no | [0,1,0] | Rotation/slide axis |
| `limits` | array[2] | no | | Angle limits [min, max] in degrees |
| `stiffness` | number | no | | Spring stiffness |
| `damping` | number | no | | Spring damping |

**Returns:** Joint name and entity_id.

**Example:**
```json
{"entity_a": "door_frame", "entity_b": "door_panel", "joint_type": "revolute", "axis": [0, 1, 0], "limits": [0, 90]}
```

---

#### `gen_add_force`

**Description:** Create a force field or apply an impulse.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `position` | array[3] | **yes** | | World position [x, y, z] |
| `force_type` | string | no | "directional" | "directional", "point_attract", "point_repel", "vortex", "impulse" |
| `strength` | number | no | 10.0 | Force strength |
| `radius` | number | no | 5.0 | Area of effect radius |
| `direction` | array[3] | no | | Force direction (directional type only) |
| `falloff` | string | no | "linear" | "none", "linear", "quadratic" |
| `affects_player` | boolean | no | true | Affects the player entity |
| `continuous` | boolean | no | true | Continuous force vs one-shot impulse |

**Returns:** Force field name and entity_id.

**Example:**
```json
{"position": [0, 0, 0], "force_type": "point_attract", "strength": 20, "radius": 10, "falloff": "quadratic"}
```

---

#### `gen_set_gravity`

**Description:** Set gravity direction and strength. Can affect whole scene or create a localized gravity zone. Presets: earth (9.81), moon (1.62), mars (3.72), jupiter (24.79), zero (0).

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity_id` | string | no | | Target entity (global if omitted) |
| `direction` | array[3] | no | [0,-1,0] | Gravity direction (normalized) |
| `strength` | number | no | 9.81 | Gravity strength in m/s^2 |
| `zone_position` | array[3] | no | | Create gravity zone at position |
| `zone_radius` | number | no | | Gravity zone radius |
| `transition_duration` | number | no | 0.5 | Transition time in seconds |

**Returns:** Confirmation.

**Example:**
```json
{"strength": 1.62, "direction": [0, -1, 0]}
```

---

### Category: Character (P1 -- Avatar & NPC System)

---

#### `gen_spawn_player`

**Description:** Spawn a controllable player character with movement, camera, and collision. Only one player allowed; calling again replaces.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `position` | array[3] | no | [0,1,0] | Spawn position |
| `rotation` | array[3] | no | [0,0,0] | Spawn rotation in degrees [pitch, yaw, roll] |
| `walk_speed` | number | no | 5.0 | Walk speed (units/s) |
| `run_speed` | number | no | 10.0 | Run speed (units/s) |
| `jump_force` | number | no | 8.0 | Jump force (upward velocity) |
| `camera_mode` | string | no | "third_person" | "first_person" or "third_person" |
| `camera_distance` | number | no | 5.0 | Camera distance (third-person) |
| `collision_radius` | number | no | 0.3 | Collision capsule radius |
| `collision_height` | number | no | 1.8 | Collision capsule height |

**Returns:** Player name and entity_id.

**Example:**
```json
{"position": [0, 1, 0], "camera_mode": "third_person", "walk_speed": 6.0}
```

---

#### `gen_set_spawn_point`

**Description:** Set a spawn/respawn location for the player. Only one spawn point can be the default.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `position` | array[3] | **yes** | | Spawn position |
| `rotation` | array[3] | no | [0,0,0] | Spawn rotation in degrees |
| `name` | string | no | | Optional spawn point name |
| `is_default` | boolean | no | true | Whether this is the default spawn point |

**Returns:** Spawn point name.

**Example:**
```json
{"position": [10, 1, 10], "name": "village_entrance", "is_default": true}
```

---

#### `gen_add_npc`

**Description:** Create a non-player character with optional patrol or wander behavior.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `position` | array[3] | **yes** | | Spawn position |
| `name` | string | **yes** | | NPC display name |
| `model` | string | no | "default_humanoid" | Model type or asset URL |
| `behavior` | string | no | "idle" | "idle", "patrol", "wander" |
| `patrol_points` | array | no | | Waypoints (required if behavior is patrol) |
| `patrol_speed` | number | no | 3.0 | Movement speed |
| `dialogue_id` | string | no | | Optional dialogue ID reference |

**Returns:** NPC name and entity_id.

**Example:**
```json
{"position": [5, 0, 5], "name": "Guard", "behavior": "patrol", "patrol_points": [[5,0,5],[10,0,5],[10,0,10]], "patrol_speed": 2.0}
```

---

#### `gen_set_npc_dialogue`

**Description:** Attach a branching conversation tree to an NPC.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `npc_id` | string | **yes** | | NPC entity name/ID |
| `nodes` | array | **yes** | | Dialogue nodes: [{id, text, speaker, choices: [{text, next_node_id}]}] |
| `start_node` | string | **yes** | | Starting node ID |
| `trigger` | string | no | "click" | "proximity" or "click" |
| `trigger_radius` | number | no | 3.0 | Trigger radius |

**Returns:** Confirmation.

**Example:**
```json
{"npc_id": "Guard", "start_node": "greeting", "nodes": [{"id": "greeting", "text": "Halt! Who goes there?", "speaker": "Guard", "choices": [{"text": "A friend.", "next_node_id": "friendly"}, {"text": "None of your business.", "next_node_id": "hostile"}]}]}
```

---

#### `gen_set_camera_mode`

**Description:** Switch or configure the player camera mode with smooth transitions.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `mode` | string | **yes** | | "first_person", "third_person", "top_down", "fixed" |
| `distance` | number | no | 5.0 | Distance from player (third_person/top_down) |
| `pitch` | number | no | -20.0 | Initial pitch in degrees |
| `fov` | number | no | 60.0 | Field of view in degrees |
| `transition_duration` | number | no | 0.5 | Seconds to blend between modes |
| `fixed_position` | array[3] | no | | Camera position (fixed mode only) |
| `fixed_look_at` | array[3] | no | | Look-at target (fixed mode only) |

**Returns:** Confirmation.

**Example:**
```json
{"mode": "top_down", "distance": 15, "pitch": -60}
```

---

#### `gen_set_npc_brain` (AI2)

**Description:** Attach an AI brain to an NPC for autonomous decision-making using a local SLM.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity` | string | **yes** | | NPC entity name |
| `personality` | string | no | "a friendly villager" | Personality description |
| `model` | string | no | "llama3.2:3b" | Ollama model name |
| `tick_rate` | number | no | 2.0 | Seconds between brain decisions |
| `perception_radius` | number | no | 15.0 | Perception range (meters) |
| `goals` | array[string] | no | [] | List of goals |
| `knowledge` | array[string] | no | [] | Facts the NPC knows |

**Returns:** Entity, model, tick_rate confirmation.

**Example:**
```json
{"entity": "Guard", "personality": "a suspicious guard who protects the gate", "goals": ["keep strangers out", "patrol the gate"], "tick_rate": 3.0}
```

---

#### `gen_npc_observe` (AI2)

**Description:** Make an NPC observe the scene from its perspective. Optionally ask a question about what it sees.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity` | string | **yes** | | NPC entity name |
| `question` | string | no | | Question about what the NPC sees |
| `fov` | number | no | 90.0 | Field of view in degrees |
| `resolution` | array[2] | no | [512, 512] | Render resolution [width, height] |

**Returns:** NPC observation description.

**Example:**
```json
{"entity": "Guard", "question": "Is anyone approaching the gate?"}
```

---

#### `gen_set_npc_memory` (AI2)

**Description:** Configure persistent memory for an NPC. Memories persist across save/load and influence brain decisions.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity` | string | **yes** | | NPC entity name |
| `capacity` | integer | no | 50 | Maximum memories to retain |
| `initial_memories` | array[string] | no | [] | Seed memories |
| `auto_memorize` | boolean | no | true | Auto-create memories from interactions |

**Returns:** Entity, capacity, initial memory count.

**Example:**
```json
{"entity": "Guard", "capacity": 100, "initial_memories": ["I was assigned to the north gate", "The captain warned about thieves"], "auto_memorize": true}
```

---

### Category: Interaction (P2 -- Triggers & Logic)

---

#### `gen_add_trigger`

**Description:** Add an interaction trigger and action to an entity. Combines triggers (proximity, click, area, collision, timer) with actions (animate, teleport, play sound, show text, toggle state, spawn, destroy, add score, enable, disable).

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity_id` | string | **yes** | | Target entity name |
| `trigger_type` | string | **yes** | | "proximity", "click", "area_enter", "area_exit", "collision", "timer" |
| `action` | string | **yes** | | "animate", "teleport", "play_sound", "show_text", "toggle_state", "spawn", "destroy", "add_score", "enable", "disable" |
| `radius` | number | no | 3.0 | Trigger radius |
| `cooldown` | number | no | 1.0 | Cooldown between triggers |
| `interval` | number | no | | Timer interval (timer trigger only) |
| `max_distance` | number | no | 5.0 | Max click distance |
| `once` | boolean | no | false | Fire only once |
| `destination` | array[3] | no | | Teleport destination (teleport action) |
| `text` | string | no | | Text content (show_text action) |
| `amount` | integer | no | 1 | Score amount (add_score action) |
| `state_key` | string | no | | State key (toggle_state action) |
| `category` | string | no | "points" | Score category (add_score action) |

**Returns:** Confirmation.

**Example:**
```json
{"entity_id": "button", "trigger_type": "click", "action": "toggle_state", "state_key": "is_active", "max_distance": 3.0}
```

---

#### `gen_add_teleporter`

**Description:** Create a portal that teleports the player to a destination when they step into it.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `position` | array[3] | **yes** | | Portal position |
| `destination` | array[3] | **yes** | | Teleport destination |
| `size` | array[3] | no | [2,3,2] | Portal trigger size |
| `effect` | string | no | "fade" | "none", "fade", "particles" |
| `sound` | string | no | | Optional sound effect |
| `label` | string | no | | Optional label text above portal |

**Returns:** Teleporter name.

**Example:**
```json
{"position": [0, 0, 0], "destination": [50, 0, 50], "effect": "particles", "label": "To the Forest"}
```

---

#### `gen_add_collectible`

**Description:** Make an entity collectible with score value, pickup effects, and optional respawning.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity_id` | string | **yes** | | Target entity name |
| `value` | integer | no | 1 | Score value |
| `category` | string | no | "points" | Score category |
| `pickup_sound` | string | no | | Optional pickup sound |
| `pickup_effect` | string | no | "sparkle" | "none", "sparkle", "dissolve" |
| `respawn_time` | number | no | | Seconds until respawn (null = no respawn) |

**Returns:** Confirmation.

**Example:**
```json
{"entity_id": "gold_coin", "value": 10, "category": "coins", "pickup_effect": "sparkle", "respawn_time": 30}
```

---

#### `gen_add_door`

**Description:** Add interactive door behavior to an entity with open/close logic and optional key requirement.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity_id` | string | **yes** | | Target entity (should have hinge at origin) |
| `trigger` | string | no | "proximity" | "proximity" or "click" |
| `open_angle` | number | no | 90.0 | Opening angle in degrees |
| `open_duration` | number | no | 1.5 | Animation duration |
| `auto_close` | boolean | no | true | Auto-close after delay |
| `auto_close_delay` | number | no | 3.0 | Seconds before auto-close |
| `requires_key` | string | no | | Required key item name |

**Returns:** Confirmation.

**Example:**
```json
{"entity_id": "castle_door", "trigger": "click", "open_angle": 90, "requires_key": "iron_key"}
```

---

#### `gen_link_entities`

**Description:** Wire one entity's event to trigger another entity's action. Enables chain reactions and puzzle logic.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `source_id` | string | **yes** | | Source entity name |
| `source_event` | string | **yes** | | Event: "clicked", "state_changed:is_active", "proximity_entered", etc. |
| `target_id` | string | **yes** | | Target entity name |
| `target_action` | string | **yes** | | Action: "toggle_state:is_open", "play_animation:open", "enable", "disable", "destroy" |
| `condition` | string | no | | Boolean expression: "source.is_active AND other.is_active" |

**Returns:** Confirmation of link.

**Example:**
```json
{"source_id": "lever", "source_event": "clicked", "target_id": "gate", "target_action": "toggle_state:is_open"}
```

---

### Category: Terrain (P3 -- Landscape & Environment)

---

#### `gen_add_terrain`

**Description:** Generate procedural terrain from noise with automatic collision mesh.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `size` | array[2] | no | [100,100] | Terrain size in meters [x, z] |
| `resolution` | integer | no | 128 | Vertices per side |
| `height_scale` | number | no | 20.0 | Maximum height in meters |
| `noise_type` | string | no | "perlin" | "perlin", "simplex", "flat" |
| `noise_octaves` | integer | no | 4 | Number of noise octaves |
| `noise_frequency` | number | no | 0.02 | Noise frequency |
| `seed` | integer | no | | Random seed |
| `material` | string | no | "grass" | "grass", "sand", "snow", "rock", "custom" |
| `position` | array[3] | no | [0,0,0] | World position |

**Returns:** Terrain name.

**Example:**
```json
{"size": [200, 200], "height_scale": 30, "noise_type": "perlin", "material": "grass", "seed": 42}
```

---

#### `gen_add_water`

**Description:** Create a transparent animated water plane at a specified height.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `height` | number | no | 0.0 | Water plane Y height |
| `size` | array[2] | no | [100,100] | Water plane size [x, z] |
| `color` | string | no | "#2389da" | Water color (hex) |
| `opacity` | number | no | 0.7 | Transparency (0-1) |
| `wave_speed` | number | no | 1.0 | Wave animation speed |
| `wave_height` | number | no | 0.3 | Wave height amplitude |
| `position` | array[3] | no | | Center position [x, y, z] |

**Returns:** Water name.

**Example:**
```json
{"height": 3.0, "size": [80, 80], "color": "#1a6b8a", "wave_speed": 0.5}
```

---

#### `gen_add_path`

**Description:** Create a walkable path between waypoints with terrain-conforming mesh.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `points` | array | **yes** | | Waypoints [[x,y,z], ...] |
| `width` | number | no | 2.0 | Path width in meters |
| `material` | string | no | "stone" | "stone", "dirt", "wood", "cobblestone", "custom" |
| `curved` | boolean | no | true | Use smooth curves |
| `raised` | number | no | 0.02 | Height above terrain |
| `border` | boolean | no | false | Add stone border edges |

**Returns:** Path name.

**Example:**
```json
{"points": [[0,0,0],[10,0,5],[20,0,0]], "width": 3, "material": "cobblestone", "curved": true}
```

---

#### `gen_add_foliage`

**Description:** Scatter vegetation (trees, bushes, grass, flowers, rocks) across terrain using Poisson disk sampling.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `foliage_type` | string | **yes** | | "tree", "bush", "grass", "flower", "rock" |
| `center` | array[3] | no | [0,0,0] | Center of placement area |
| `radius` | number | no | 30.0 | Placement area radius |
| `density` | number | no | 0.5 | Density (0-1) |
| `scale_range` | array[2] | no | [0.8, 1.2] | Random scale range [min, max] |
| `seed` | integer | no | | Random seed |
| `avoid_paths` | boolean | no | true | Avoid placing on paths |
| `avoid_water` | boolean | no | true | Avoid placing in water |
| `max_slope` | number | no | 30.0 | Maximum terrain slope (degrees) |

**Returns:** Foliage group name.

**Example:**
```json
{"foliage_type": "tree", "center": [0, 0, 0], "radius": 50, "density": 0.3, "seed": 123}
```

---

#### `gen_set_sky`

**Description:** Configure sky, sun direction, ambient light, and fog.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `preset` | string | no | "day" | "day", "sunset", "night", "overcast", "custom" |
| `sun_altitude` | number | no | | Sun angle above horizon (0-90 degrees) |
| `sun_azimuth` | number | no | | Sun compass direction (0=north) |
| `sun_intensity` | number | no | | Sun brightness multiplier |
| `ambient_color` | string | no | | Ambient light color (hex) |
| `ambient_intensity` | number | no | | Ambient light brightness |
| `fog_enabled` | boolean | no | false | Enable distance fog |
| `fog_color` | string | no | "#c8d0d8" | Fog color (hex) |
| `fog_start` | number | no | 50.0 | Fog start distance |
| `fog_end` | number | no | 200.0 | Fog end distance |

**Returns:** Confirmation with preset name.

**Example:**
```json
{"preset": "sunset", "fog_enabled": true, "fog_start": 30, "fog_end": 150}
```

---

#### `gen_query_terrain_height`

**Description:** Query terrain height at one or more (x, z) coordinates. Returns world Y height for each point.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `points` | array | no | | Array of [x, z] coordinates to query |
| `x` | number | no | | Single point X coordinate (shortcut) |
| `z` | number | no | | Single point Z coordinate (shortcut) |

**Returns:** JSON with heights array [{x, y, z}].

**Example:**
```json
{"points": [[0, 0], [10, 10], [20, 5]]}
```

---

### Category: UI (P4 -- In-World Text & HUD)

---

#### `gen_add_sign`

**Description:** Place readable text in the 3D world as a sign or billboard.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `position` | array[3] | **yes** | | World position |
| `text` | string | **yes** | | Text content |
| `font_size` | number | no | 24.0 | Font size in world units |
| `color` | string | no | "#ffffff" | Text color (hex) |
| `background_color` | string | no | | Background color (hex with alpha) |
| `billboard` | boolean | no | true | Always face camera |
| `max_width` | number | no | | Word wrap width |
| `rotation` | array[3] | no | [0,0,0] | Rotation when billboard=false |

**Returns:** Sign name.

**Example:**
```json
{"position": [0, 3, 0], "text": "Welcome to the Village", "font_size": 32, "color": "#ffcc00"}
```

---

#### `gen_add_hud`

**Description:** Add a persistent screen-space HUD element (score, health, timer, text).

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `element_type` | string | **yes** | | "score", "health", "text", "timer" |
| `position` | string | no | "top-left" | "top-left", "top-right", "bottom-left", "bottom-right", "center-top", "center-bottom" |
| `label` | string | no | | Label prefix text |
| `initial_value` | string | no | "0" | Initial display value |
| `font_size` | number | no | 18.0 | Font size |
| `color` | string | no | "#ffffff" | Text color (hex) |
| `id` | string | no | | Unique ID for updates |

**Returns:** HUD element name.

**Example:**
```json
{"element_type": "score", "position": "top-right", "label": "Score: ", "initial_value": "0"}
```

---

#### `gen_add_label`

**Description:** Attach a floating name label to an entity that billboards toward the camera.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity_id` | string | **yes** | | Target entity name |
| `text` | string | **yes** | | Label text |
| `color` | string | no | "#ffffff" | Text color (hex) |
| `background_color` | string | no | "#00000088" | Background color (hex with alpha) |
| `offset_y` | number | no | 0.5 | Height above entity |
| `font_size` | number | no | 16.0 | Font size |
| `visible_distance` | number | no | 15.0 | Maximum visible distance |

**Returns:** Confirmation.

**Example:**
```json
{"entity_id": "Guard", "text": "Guard", "color": "#ff0000", "offset_y": 2.0}
```

---

#### `gen_add_tooltip`

**Description:** Add a contextual tooltip that appears on proximity or look-at.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity_id` | string | **yes** | | Target entity name |
| `text` | string | **yes** | | Tooltip text |
| `trigger` | string | no | "proximity" | "proximity" or "look_at" |
| `range` | number | no | 3.0 | Trigger range |
| `style` | string | no | "floating" | "floating", "screen_center", "screen_bottom" |
| `color` | string | no | "#ffffff" | Text color (hex) |
| `duration` | number | no | | Auto-dismiss after seconds |

**Returns:** Confirmation.

**Example:**
```json
{"entity_id": "chest", "text": "Press E to open", "trigger": "proximity", "range": 2.0}
```

---

#### `gen_add_notification`

**Description:** Show a transient notification message with animation (toast, banner, achievement).

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `text` | string | **yes** | | Notification text |
| `style` | string | no | "toast" | "toast", "banner", "achievement" |
| `position` | string | no | "top" | "top", "center", "bottom" |
| `duration` | number | no | 3.0 | Display duration in seconds |
| `color` | string | no | "#ffffff" | Text color (hex) |
| `icon` | string | no | "none" | "none", "star", "coin", "key", "heart", "warning" |
| `sound` | string | no | | Sound to play |

**Returns:** Notification name.

**Example:**
```json
{"text": "Achievement Unlocked!", "style": "achievement", "icon": "star", "duration": 5.0}
```

---

### Category: WorldGen (WG -- Procedural Pipeline)

---

#### `gen_plan_layout`

**Description:** Generate a structured world layout plan from a text description. Returns BlockoutSpec JSON for review before applying.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `prompt` | string | **yes** | | Natural language world description |
| `size` | array[2] | no | [50,50] | World dimensions in meters [X, Z] |
| `seed` | integer | no | | Random seed for deterministic generation |

**Returns:** BlockoutSpec JSON.

**Example:**
```json
{"prompt": "a medieval village with a forest and lake", "size": [100, 100], "seed": 42}
```

---

#### `gen_apply_blockout`

**Description:** Generate a 3D blockout scene from a BlockoutSpec. Creates terrain, region debug volumes, hero slot markers, and connecting paths.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `spec` | object | **yes** | | BlockoutSpec JSON (from gen_plan_layout or manual) |
| `show_debug_volumes` | boolean | no | true | Render translucent region volumes |
| `generate_terrain` | boolean | no | true | Generate terrain mesh |
| `generate_paths` | boolean | no | true | Generate path geometry between regions |

**Returns:** Entities spawned, region count, path count.

---

#### `gen_populate_region`

**Description:** Fill a blockout region with 3D content (foliage, decorations) based on density and biome parameters.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `region_id` | string | **yes** | | Region ID from BlockoutSpec |
| `style_hint` | string | no | | Style guidance (e.g., "autumn colors") |
| `replace_existing` | boolean | no | false | Clear existing content first |

**Returns:** Region ID and entity count.

---

#### `gen_set_tier`

**Description:** Set an entity's placement tier (hero, medium, decorative, untiered). Tiers control generation priority.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity` | string | **yes** | | Entity name |
| `tier` | string | **yes** | | "hero", "medium", "decorative", "untiered" |

**Returns:** Confirmation.

---

#### `gen_set_role`

**Description:** Set an entity's semantic role. Roles enable bulk operations with gen_bulk_modify.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity` | string | **yes** | | Entity name |
| `role` | string | **yes** | | "ground", "structure", "prop", "vegetation", "decoration", "character", "lighting", "audio", "untagged" |

**Returns:** Confirmation.

---

#### `gen_bulk_modify`

**Description:** Apply a modification to all entities matching a semantic role. Optionally filter by blockout region.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `role` | string | **yes** | | Semantic role to match |
| `action` | object | **yes** | | Action: {type: "scale", factor: 1.5} or {type: "recolor", color: [r,g,b,a]} or {type: "remove"/"hide"/"show"} |
| `region_id` | string | no | | Limit to entities in this region |

**Returns:** Role, action, and affected count.

---

#### `gen_modify_blockout`

**Description:** Edit the world blockout layout -- add, remove, resize, or move regions.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `action` | object | **yes** | | Edit action (add_region, remove_region, resize_region, move_region, set_density) |
| `auto_regenerate` | boolean | no | false | Repopulate affected region after editing |

**Returns:** Action, region_id, entities removed/spawned.

---

#### `gen_evaluate_scene`

**Description:** Capture a screenshot and gather scene metadata for quality evaluation.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `focus_entity` | string | no | | Entity to focus on and highlight |
| `reference_prompt` | string | no | | Original world description for comparison |

**Returns:** Screenshot path, scene stats, evaluation guidance.

---

#### `gen_auto_refine`

**Description:** Iteratively evaluate and refine the scene. Captures screenshots and suggests fixes.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `max_iterations` | integer | no | 3 | Maximum evaluate-refine iterations |
| `target_score` | number | no | 0.7 | Target quality score (0.0-1.0) |
| `reference_prompt` | string | no | | Original world description |

**Returns:** Iteration results with screenshots.

---

#### `gen_build_navmesh`

**Description:** Generate a navigation mesh from current scene geometry for walkability analysis.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `agent_radius` | number | no | 0.3 | Agent collision radius (meters) |
| `agent_height` | number | no | 1.8 | Agent height (meters) |
| `max_slope` | number | no | 45.0 | Maximum walkable slope (degrees) |
| `step_height` | number | no | 0.4 | Maximum step-up height (meters) |
| `cell_size` | number | no | 0.5 | Grid cell size (smaller = more detail) |

**Returns:** Walkable coverage, connected regions, cell count.

---

#### `gen_validate_navigability`

**Description:** Check scene traversability between points or overall.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `from` | array[3] | no | | Start point [x, y, z] |
| `to` | array[3] | no | | End point [x, y, z] |
| `check_all_regions` | boolean | no | false | Check connectivity between all regions |

**Returns:** JSON with walkable coverage, connectivity, warnings.

---

#### `gen_edit_navmesh`

**Description:** Manually mark areas as walkable/blocked, or add/remove off-mesh connections.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `action` | string | **yes** | | "block_area", "allow_area", "add_connection", "remove_connection" |
| `position` | array[3] | **yes** | | Center position |
| `radius` | number | no | 2.0 | Area of effect radius |
| `connection_target` | array[3] | no | | Target for add_connection |
| `bidirectional` | boolean | no | true | Whether connection is bidirectional |

**Returns:** Confirmation.

---

#### `gen_regenerate`

**Description:** Regenerate content in dirty blockout regions. Only modified regions are regenerated.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `region_ids` | array[string] | no | | Specific regions (omit for all dirty) |
| `preview_only` | boolean | no | false | Show what would change without executing |
| `preserve_manual` | boolean | no | true | Keep manually placed entities |

**Returns:** Regions processed and entities removed counts.

---

#### `gen_render_depth`

**Description:** Render a depth map for preview generation. Captures depth buffer from configurable camera angle.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `camera_angle` | string | no | "isometric" | "isometric", "top_down", "front", "custom" |
| `custom_position` | array[3] | no | | Custom camera position (custom angle only) |
| `custom_look_at` | array[3] | no | | Custom look-at target (custom angle only) |
| `resolution` | array[2] | no | [1024, 1024] | Output resolution [width, height] |
| `near_plane` | number | no | 0.1 | Near clipping plane |
| `far_plane` | number | no | 200.0 | Far clipping plane |
| `add_noise` | boolean | no | true | Add Gaussian noise to reduce artifacts |
| `output_path` | string | no | | Custom output path |

**Returns:** Path, dimensions, depth range.

---

#### `gen_preview_world`

**Description:** Generate a styled 2D preview image from the blockout depth map + text prompt.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `prompt` | string | **yes** | | Desired scene appearance description |
| `depth_map_path` | string | no | | Path to depth map PNG (from gen_render_depth) |
| `style_preset` | string | no | | "realistic", "stylized", "pixel_art", "watercolor", "concept_art" |
| `negative_prompt` | string | no | | Things to avoid |
| `output_path` | string | no | | Custom output path |

**Returns:** Path, style, depth_map_used status.

---

### Category: MultiFile (MF -- Iterative World Generation & Sync)

---

#### `gen_write_world_plan`

**Description:** Create SKILL.md, world.md, and root world.ron from a structured world plan. First step in iterative world generation.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | World name |
| `description` | string | no | | World description |
| `generation_strategy` | string | no | | "blockout", "manual", "hybrid" |
| `regions` | array | no | | Region definitions [{id, center, size, estimated_entities}] |
| `constraints` | object | no | | Generation constraints (max entities, performance budget) |
| `environment` | object | no | | Environment settings (lighting, fog, sky) |
| `camera` | object | no | | Initial camera configuration |

**Returns:** JSON with file paths created.

---

#### `gen_write_region`

**Description:** Write a .md + .ron file pair for a region. Files held in memory until gen_save_world unless flush=true.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `region_id` | string | **yes** | | Region identifier |
| `design_intent` | string | no | | Design intent for the .md file |
| `entities` | array | no | | Entity definitions for the .ron file |
| `bounds` | object | no | | Bounding box {center: [x,z], size: [w,h]} |
| `flush` | boolean | no | false | Write to disk immediately |

**Returns:** region_id, md_path, ron_path.

---

#### `gen_load_region`

**Description:** Load a region .ron file and spawn its entities into the live Bevy scene.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `region_id` | string | **yes** | | Region identifier to load |

**Returns:** region_id and entity_count.

---

#### `gen_unload_region`

**Description:** Remove all entities belonging to a region from the live scene.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `region_id` | string | **yes** | | Region identifier to unload |

**Returns:** region_id and entities_removed count.

---

#### `gen_persist_blockout`

**Description:** Save the current BlockoutSpec to layout/blockout.md and layout/blockout.ron.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| *(none)* | | | | |

**Returns:** md_path and ron_path.

---

#### `gen_write_behaviors`

**Description:** Write a behavior library .md + .ron file pair.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | Behavior library name |
| `behaviors` | object | no | | Mapping of behavior name to definition |

**Returns:** name, md_path, ron_path.

---

#### `gen_write_audio`

**Description:** Write an audio spec .md + .ron file pair.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | string | **yes** | | Audio spec name |
| `ambience` | array | no | | Ambient sound definitions |
| `emitters` | array | no | | Sound emitter definitions |

**Returns:** name, md_path, ron_path.

---

#### `gen_check_drift`

**Description:** Compare .md files, .ron files, and the live Bevy scene to detect inconsistencies.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `domain` | string | no | | Specific domain (e.g., "region/forest", "audio") or omit for all |
| `detail_level` | string | no | "structural" | "summary", "structural", "full" |

**Returns:** Drift report JSON.

---

#### `gen_sync`

**Description:** Reconcile drift between .md, .ron, and the live scene. Specify which is the source of truth.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `domain` | string | **yes** | | Domain to sync (e.g., "region/forest") |
| `source` | string | **yes** | | Source of truth: "md", "ron", "scene" |
| `preview` | boolean | no | true | Show what would change without applying |
| `resolve_conflicts` | object | no | | Per-field conflict resolution |

**Returns:** Preview of changes or applied results.

---

#### `gen_generation_status`

**Description:** Report current generation phase and progress.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| *(none)* | | | | |

**Returns:** Generation status JSON.

---

### Category: AssetGen (AI1 -- AI-Powered Asset Generation)

---

#### `gen_generate_asset`

**Description:** Generate a 3D mesh from a text prompt using a local AI model server. Queued for background generation; auto-spawns when ready.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `prompt` | string | **yes** | | Text description (e.g., "a weathered wooden barrel") |
| `name` | string | **yes** | | Entity name for the generated asset |
| `reference_image` | string | no | | Path to reference image |
| `position` | array[3] | no | [0,0,0] | Placement position |
| `scale` | number | no | 1.0 | Uniform scale factor |
| `model` | string | no | "tripo_sg" | "tripo_sg" (~8GB, 30s), "hunyuan3d" (~10GB, 60s), "hunyuan3d_mini" (~5.5GB, 45s), "step1x" (~16GB, 90s) |
| `quality` | string | no | "standard" | "draft" (fast), "standard" (balanced), "high" (slow, detailed) |
| `pbr` | boolean | no | true | Generate PBR textures |

**Returns:** task_id, estimated_seconds, message.

**Example:**
```json
{"prompt": "a weathered wooden barrel", "name": "barrel_1", "position": [5, 0, 3], "model": "tripo_sg", "quality": "standard"}
```

---

#### `gen_generate_texture`

**Description:** Generate PBR textures for an existing entity using a text prompt.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `entity` | string | **yes** | | Existing entity name |
| `prompt` | string | **yes** | | Texture description (e.g., "mossy stone bricks") |
| `style` | string | no | "realistic" | "realistic", "stylized", "pixel_art", "hand_painted", "toon" |
| `resolution` | integer | no | 1024 | Texture resolution: 512, 1024, 2048, 4096 |

**Returns:** task_id, estimated_seconds, message.

**Example:**
```json
{"entity": "wall_1", "prompt": "mossy stone bricks", "style": "realistic", "resolution": 2048}
```

---

#### `gen_generation_status` (AssetGen)

**Description:** Check the status of AI asset generation tasks. List all, get specific, or cancel.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `action` | string | no | "status" | "status"/"list" to view, "cancel" to cancel |
| `task_id` | string | no | | Task ID (required for cancel, optional for status filter) |

**Returns:** Status JSON.

**Example:**
```json
{"action": "list"}
```

---

### Category: Experiment (EXP -- Experiment Queue)

---

#### `gen_queue_experiment`

**Description:** Queue a world generation experiment for background processing.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `prompt` | string | **yes** | | World generation prompt |
| `name` | string | **yes** | | World name (used for skill folder) |
| `style` | string | no | | Style hint or memory reference |
| `variations` | object | no | | Variation spec {axis: string, values: [string]} |

**Returns:** Queued count and experiment_ids.

**Example:**
```json
{"prompt": "a peaceful zen garden", "name": "zen-garden", "variations": {"axis": "lighting", "values": ["dawn", "noon", "dusk"]}}
```

---

#### `gen_list_experiments`

**Description:** List all queued, running, and completed experiments.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `status` | string | no | "all" | "all", "pending", "running", "completed", "failed" |
| `limit` | integer | no | 20 | Max experiments to return |

**Returns:** Total count and experiment array with id, prompt, status, paths, entity_count, duration.

**Example:**
```json
{"status": "completed", "limit": 10}
```

---

#### `gen_experiment_status`

**Description:** Get detailed status of a specific experiment by ID.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `id` | string | **yes** | | Experiment ID |

**Returns:** Full experiment details JSON.

**Example:**
```json
{"id": "exp-zen-garden-dawn"}
```
