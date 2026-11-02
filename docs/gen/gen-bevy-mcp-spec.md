# LocalGPT Gen — Bevy MCP Implementation Spec

**LLM-Driven 3D World Creation via Primitives, Code, and Conversational Iteration**

Single Binary  |  Zero Dependencies  |  In-Process Bevy Renderer  |  Intent-Level Tools

| Field | Value |
|-------|-------|
| Version | 1.0 Draft |
| Date | February 13, 2026 |
| Author | Yi / LocalGPT |
| Status | Historical — built as proposed; the in-process model and `localgpt-core` it assumed went on 2026-11-01, and Gen is driven over MCP by an agent outside the window |
| Priority | P1 — Product Expansion |
| Crate | `localgpt-gen` (`crates/gen/`) |

---

## 1. Executive Summary

LocalGPT Gen extends LocalGPT with AI-driven 3D content creation. Users describe scenes in natural language; LocalGPT's agent composes them from geometric primitives, procedural code, and asset imports inside a Bevy renderer — all within the single binary.

The approach deliberately avoids neural 3D generation APIs (Meshy, Tripo3D, Hunyuan3D). Instead, it exploits the proven fact — demonstrated by blender-mcp (16.8K stars), the demoscene (entire cinematic scenes from 64KB), and academic benchmarks (CadQuery 69.3% exact match, CAD-Coder VLM 100% valid syntax) — that standard LLMs generating code and structured data can produce compelling 3D content from primitives alone.

### 1.1 Why This Matters

No existing tool combines local-first execution, zero dependencies, conversational iteration, and 3D generation in a single binary. The competitive landscape is fragmented:

| Product | Architecture | Limitation |
|---------|-------------|------------|
| blender-mcp | External MCP server → TCP → Blender | Requires Blender install (~700MB), Python, two processes |
| bevy_brp_mcp | External MCP server → HTTP → Bevy App | Two processes, raw ECS CRUD, no creative tools |
| bevy_debugger_mcp | External MCP server → WebSocket → Bevy App | GPL-3.0, vibe-coded, debugging-only |
| Rosebud AI | Cloud service → Three.js | Cloud-dependent, no local execution |
| SceneCraft | Research prototype → Blender | Not productized, heavy Python stack |

LocalGPT Gen's value proposition: **type a description, watch it materialize in a local window, refine with conversation, export to glTF — all offline, all in one binary.**

### 1.2 The OpenClaw Gap

OpenClaw has no 3D generation capability. This is a pure expansion of LocalGPT's feature surface with no competing reference implementation to match. The target users — developers prototyping game levels, architects blocking out spaces, educators creating interactive visualizations — overlap with OpenClaw's base but require capabilities OpenClaw doesn't offer.

---

## 2. Architecture

### 2.1 High-Level Design

```
┌─────────────────────────────────────────────────────────┐
│               localgpt-gen binary                        │
│               (crates/gen/)                              │
│                                                         │
│  ┌──────────────┐     mpsc channels     ┌────────────┐  │
│  │  Agent Loop   │◄───────────────────►│ Bevy App   │  │
│  │  (tokio rt)   │   GenCommand/       │ (main thd) │  │
│  │  (bg thread)  │   GenResponse        │            │  │
│  │              │                      │ Renderer   │  │
│  │  LLM ↔ Tools │                      │ ECS World  │  │
│  │  Memory      │                      │ Window     │  │
│  └──────────────┘                      └────────────┘  │
│                                                         │
│  cargo run -p localgpt-gen                              │
│  Separate binary — Bevy is NOT linked into localgpt-cli │
└─────────────────────────────────────────────────────────┘
```

**Why in-process, not two-process?**

- **Single binary philosophy.** No "install Bevy separately" step.
- **Sub-millisecond tool round-trip.** Channel send + Bevy system poll + channel respond ≈ 16ms (one frame at 60fps). HTTP JSON-RPC adds 1-5ms per call plus serialization overhead.
- **No process management.** No port discovery, no "is the app running?" checks, no zombie processes.
- **Shared memory.** Scene state accessible to both agent and renderer without serialization.

**Thread model:**

- Bevy must own the main thread (windowing/GPU requirement on macOS).
- Agent loop runs on tokio runtime in background threads.
- Communication via `tokio::sync::mpsc` unbounded channels (GenCommand → Bevy, GenResponse → Agent).

This is the same pattern LocalGPT already uses for its egui desktop GUI.

### 2.2 Workspace Crate Split

Gen is a separate binary crate (`crates/gen/`) rather than a feature flag. This keeps Bevy out of the main CLI binary.

```toml
# crates/gen/Cargo.toml
[package]
name = "localgpt-gen"

[dependencies]
localgpt-core = { workspace = true }
bevy = "0.18"
```

| Binary | Size | Includes |
|--------|------|----------|
| `localgpt` (cli) | ~27MB | CLI, API, web GUI, memory, heartbeat |
| `localgpt-gen` | ~45-55MB | Bevy renderer, 3D tools, core agent |

### 2.3 Startup Sequence

1. Parse CLI args via clap (`localgpt-gen [PROMPT] [-s SCENE] [-a AGENT] [-v]`).
2. Load config, resolve workspace path.
3. Create `mpsc` channel pair via `create_gen_channels()`.
4. Spawn tokio runtime on background thread with agent loop (safe tools + gen tools).
5. On main thread: initialize Bevy `App` with `DefaultPlugins`, `GenPlugin`, window "LocalGPT Gen" (1280x720).
6. `GenPlugin` registers a Bevy system that polls commands each frame, executes them, sends responses.
7. Bevy enters its event loop. Agent loop runs concurrently with interactive stdin prompt.

### 2.4 Command/Response Protocol

```rust
/// Sent from agent → Bevy main thread
enum GenCommand {
    // Tier 1: Perceive
    SceneInfo,
    Screenshot { width: u32, height: u32 },
    EntityInfo { name: String },

    // Tier 2: Mutate
    SpawnPrimitive(SpawnPrimitiveCmd),
    ModifyEntity(ModifyEntityCmd),
    DeleteEntity { name: String },
    SetCamera(CameraCmd),
    SetLight(LightCmd),
    SetEnvironment(EnvironmentCmd),

    // Tier 3: Advanced
    SpawnVoxels(VoxelGridCmd),
    SpawnMesh(RawMeshCmd),
    ExecuteScript { rhai_code: String },

    // Tier 4: Export
    ExportScene { path: String, format: ExportFormat },
    ExportScreenshot { path: String, width: u32, height: u32 },
}

/// Sent from Bevy main thread → agent
enum GenResponse {
    SceneInfo(SceneInfoData),
    Screenshot { image_path: String },
    EntityInfo(EntityInfoData),
    Spawned { name: String, entity_id: u64 },
    Modified { name: String },
    Deleted { name: String },
    Exported { path: String },
    Error { message: String },
}
```

---

## 3. Tool Design

### 3.1 Design Principles

1. **Intent-level, not ECS-level.** Users say "add a red cube at position 5,0,0" — not "spawn entity with Transform, Mesh3d, MeshMaterial3d components." The tool bundles the 3-5 components needed for a visible object into one call.
2. **Every tool is simple and composable.** Complex scenes emerge from many simple tool calls, not one complex tool call. This matches how LLMs reason — sequentially, not holistically.
3. **Perception closes the loop.** The `screenshot` tool is not optional — it's the mechanism that lets the LLM iterate. Without it, generation is blind and quality collapses.
4. **Names, not IDs.** Entities are referenced by human-readable names ("red_cube", "main_light"), not opaque u64 entity IDs. The system maintains a name→entity map.

### 3.2 Tier 1: Perceive (Read-Only)

#### `gen_scene_info`

Returns the full scene graph: entity names, types (primitive/light/camera/custom), transforms, materials, parent-child relationships.

```json
// Tool schema
{
  "name": "gen_scene_info",
  "description": "Get complete scene hierarchy with all entities, transforms, and materials.",
  "parameters": {}
}

// Response example
{
  "entity_count": 4,
  "entities": [
    {
      "name": "ground_plane",
      "type": "primitive",
      "primitive": "Plane",
      "position": [0, 0, 0],
      "scale": [10, 1, 10],
      "material": { "color": [0.3, 0.3, 0.3, 1.0], "metallic": 0.0, "roughness": 0.8 }
    },
    {
      "name": "main_camera",
      "type": "camera",
      "position": [5, 5, 5],
      "looking_at": [0, 0, 0]
    }
  ]
}
```

#### `gen_screenshot`

Renders the current viewport to an image. Returns a file path (or base64 for multimodal LLMs).

```json
{
  "name": "gen_screenshot",
  "description": "Capture the current viewport as an image. Use after spawning/modifying entities to see results.",
  "parameters": {
    "width": { "type": "integer", "default": 800 },
    "height": { "type": "integer", "default": 600 },
    "wait_frames": { "type": "integer", "default": 3, "description": "Frames to wait before capture for render pipeline to process new geometry" }
  }
}
```

**Implementation detail:** `wait_frames` solves the render-sync problem identified in bevy_debugger_mcp analysis. After spawning entities, the render pipeline needs 1-3 frames to process new geometry, upload textures, and compute lighting. Capturing too early shows stale frames.

The Bevy system increments a frame counter after processing a `Screenshot` command. It only captures once `current_frame >= command_frame + wait_frames`.

#### `gen_entity_info`

Detailed inspection of a single entity: all component values, children, parent.

```json
{
  "name": "gen_entity_info",
  "description": "Get detailed information about a specific entity by name.",
  "parameters": {
    "name": { "type": "string", "description": "Entity name to inspect" }
  }
}
```

### 3.3 Tier 2: Mutate (Scene Composition)

#### `gen_spawn_primitive`

The workhorse tool. Spawns a named, visible 3D primitive with a single call.

```json
{
  "name": "gen_spawn_primitive",
  "description": "Spawn a 3D primitive shape with material and transform. This creates a fully visible object — no additional components needed.",
  "parameters": {
    "name": { "type": "string", "description": "Unique name for this entity (e.g., 'red_cube', 'table_leg_1')" },
    "shape": {
      "type": "string",
      "enum": ["Cuboid", "Sphere", "Cylinder", "Cone", "Capsule", "Torus", "Plane", "ConicalFrustum"],
      "description": "Primitive shape type"
    },
    "dimensions": {
      "type": "object",
      "description": "Shape-specific dimensions. Cuboid: {x,y,z}. Sphere: {radius}. Cylinder: {radius, height}. Cone: {radius, height}. Torus: {major_radius, minor_radius}."
    },
    "position": { "type": "array", "items": { "type": "number" }, "default": [0, 0, 0] },
    "rotation_degrees": { "type": "array", "items": { "type": "number" }, "default": [0, 0, 0], "description": "Euler angles in degrees (pitch, yaw, roll)" },
    "scale": { "type": "array", "items": { "type": "number" }, "default": [1, 1, 1] },
    "color": { "type": "array", "items": { "type": "number" }, "default": [0.8, 0.8, 0.8, 1.0], "description": "RGBA color, 0.0-1.0" },
    "metallic": { "type": "number", "default": 0.0, "minimum": 0, "maximum": 1 },
    "roughness": { "type": "number", "default": 0.5, "minimum": 0, "maximum": 1 },
    "emissive": { "type": "array", "items": { "type": "number" }, "default": [0, 0, 0, 0], "description": "Emissive RGBA color for glowing objects" },
    "parent": { "type": "string", "description": "Name of parent entity for hierarchy. Omit for root-level." }
  },
  "required": ["name", "shape"]
}
```

**Internally, this spawns:**
- `Name` component with the given name
- `Transform` from position/rotation/scale
- `Mesh3d` with the selected primitive
- `MeshMaterial3d` with `StandardMaterial` from color/metallic/roughness/emissive
- Optional `Parent` component if `parent` is specified

**Why this matters:** bevy_brp requires the LLM to know fully-qualified type names (`bevy_transform::components::transform::Transform`) and compose 3-5 separate component insertions. `gen_spawn_primitive` does it in one call with intuitive parameter names. This is the abstraction gap bevy_debugger_mcp identified but didn't execute well.

#### `gen_modify_entity`

Update any property of an existing entity.

```json
{
  "name": "gen_modify_entity",
  "description": "Modify properties of an existing entity. Only specified fields are changed; others remain unchanged.",
  "parameters": {
    "name": { "type": "string", "description": "Name of entity to modify" },
    "position": { "type": "array", "description": "New position [x, y, z]" },
    "rotation_degrees": { "type": "array", "description": "New rotation [pitch, yaw, roll] in degrees" },
    "scale": { "type": "array", "description": "New scale [x, y, z]" },
    "color": { "type": "array", "description": "New RGBA color" },
    "metallic": { "type": "number" },
    "roughness": { "type": "number" },
    "emissive": { "type": "array" },
    "visible": { "type": "boolean", "description": "Show/hide entity" },
    "parent": { "type": "string", "description": "Reparent to named entity, or null to unparent" }
  },
  "required": ["name"]
}
```

#### `gen_delete_entity`

Remove an entity and all its children.

```json
{
  "name": "gen_delete_entity",
  "description": "Delete an entity and all its children from the scene.",
  "parameters": {
    "name": { "type": "string" }
  },
  "required": ["name"]
}
```

#### `gen_set_camera`

Position and orient the camera. Critical for framing screenshots.

```json
{
  "name": "gen_set_camera",
  "description": "Set camera position and target. The camera always looks at the target point.",
  "parameters": {
    "position": { "type": "array", "default": [5, 5, 5], "description": "Camera position [x, y, z]" },
    "look_at": { "type": "array", "default": [0, 0, 0], "description": "Point camera looks at [x, y, z]" },
    "fov_degrees": { "type": "number", "default": 45, "description": "Vertical field of view" },
    "projection": { "type": "string", "enum": ["perspective", "orthographic"], "default": "perspective" }
  }
}
```

#### `gen_set_light`

Add or modify a light source.

```json
{
  "name": "gen_set_light",
  "description": "Add or update a light source. Lighting is the primary driver of visual quality.",
  "parameters": {
    "name": { "type": "string", "description": "Light name (e.g., 'sun', 'key_light', 'fill')" },
    "light_type": { "type": "string", "enum": ["directional", "point", "spot"], "default": "directional" },
    "color": { "type": "array", "default": [1, 1, 1, 1] },
    "intensity": { "type": "number", "default": 1000, "description": "Lumens for point/spot, lux for directional" },
    "position": { "type": "array", "description": "Position for point/spot lights" },
    "direction": { "type": "array", "description": "Direction for directional/spot lights" },
    "shadows": { "type": "boolean", "default": true }
  },
  "required": ["name"]
}
```

#### `gen_set_environment`

Global scene settings.

```json
{
  "name": "gen_set_environment",
  "description": "Set global environment: background color, ambient light, fog.",
  "parameters": {
    "background_color": { "type": "array", "description": "RGBA background color" },
    "ambient_light": { "type": "number", "default": 0.1, "description": "Ambient light intensity 0.0-1.0" },
    "ambient_color": { "type": "array", "default": [1, 1, 1, 1] },
    "fog_color": { "type": "array", "description": "RGBA fog color. Omit to disable fog." },
    "fog_start": { "type": "number", "description": "Distance where fog begins" },
    "fog_end": { "type": "number", "description": "Distance where fog is fully opaque" }
  }
}
```

### 3.4 Tier 3: Advanced

#### `gen_spawn_voxels`

Bulk voxel placement using the floor-by-floor grid encoding pattern (from BuilderGPT / MC-Bench research). Backed by `bevy_voxel_world`.

```json
{
  "name": "gen_spawn_voxels",
  "description": "Place voxels using layer-by-layer grid encoding. Each layer is a 2D text grid where each character represents a block type. Efficient for buildings, terrain, pixel art.",
  "parameters": {
    "name": { "type": "string" },
    "origin": { "type": "array", "default": [0, 0, 0] },
    "block_size": { "type": "number", "default": 1.0 },
    "palette": {
      "type": "object",
      "description": "Character → material mapping. Example: {'#': 'stone', '.': 'air', 'W': 'wood'}",
      "additionalProperties": {
        "type": "object",
        "properties": {
          "color": { "type": "array" },
          "metallic": { "type": "number" },
          "roughness": { "type": "number" }
        }
      }
    },
    "layers": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "y": { "type": "integer", "description": "Layer height (y-level)" },
          "grid": { "type": "array", "items": { "type": "string" }, "description": "Array of strings, each string is one row of the grid" }
        }
      },
      "description": "Bottom-up layers. Each layer is a 2D grid of palette characters."
    }
  },
  "required": ["name", "palette", "layers"]
}
```

**Example usage by LLM:**

```json
{
  "name": "small_house",
  "palette": {
    "#": { "color": [0.6, 0.3, 0.1, 1.0], "roughness": 0.9 },
    "D": { "color": [0.4, 0.2, 0.05, 1.0], "roughness": 0.7 },
    "W": { "color": [0.7, 0.85, 0.95, 0.5], "roughness": 0.1, "metallic": 0.3 },
    ".": "air"
  },
  "layers": [
    { "y": 0, "grid": ["#####", "#...#", "#...#", "#...#", "#####"] },
    { "y": 1, "grid": ["#####", "#...#", "#...#", "#...#", "#####"] },
    { "y": 2, "grid": ["#W#W#", "#...#", "D...#", "#...#", "#W#W#"] },
    { "y": 3, "grid": ["#####", "#####", "#####", "#####", "#####"] }
  ]
}
```

This encoding is LLM-friendly: spatial reasoning happens on a 2D grid per layer, not in 3D coordinates. The palette mapping keeps grid strings compact.

#### `gen_spawn_mesh`

Direct vertex/index/normal/UV construction for when primitives aren't enough.

```json
{
  "name": "gen_spawn_mesh",
  "description": "Create custom geometry from raw vertex data. Use when primitives and voxels are insufficient.",
  "parameters": {
    "name": { "type": "string" },
    "vertices": { "type": "array", "items": { "type": "array" }, "description": "Array of [x,y,z] vertex positions" },
    "indices": { "type": "array", "items": { "type": "integer" }, "description": "Triangle indices (groups of 3)" },
    "normals": { "type": "array", "items": { "type": "array" }, "description": "Per-vertex normals [x,y,z]. Auto-computed if omitted." },
    "uvs": { "type": "array", "items": { "type": "array" }, "description": "Per-vertex UV coordinates [u,v]" },
    "color": { "type": "array", "default": [0.8, 0.8, 0.8, 1.0] },
    "metallic": { "type": "number", "default": 0.0 },
    "roughness": { "type": "number", "default": 0.5 },
    "position": { "type": "array", "default": [0, 0, 0] }
  },
  "required": ["name", "vertices", "indices"]
}
```

#### `gen_execute_script`

Sandboxed Rhai scripting for behavior that tools can't express. Equivalent to blender-mcp's `execute_blender_code` but safe.

```json
{
  "name": "gen_execute_script",
  "description": "Execute a Rhai script in the Bevy ECS. Sandboxed: no filesystem, no network. Has access to spawn/modify/delete functions and math utilities.",
  "parameters": {
    "code": { "type": "string", "description": "Rhai script code" }
  }
}
```

**Sandbox constraints (via `bevy_mod_scripting` Rhai backend):**
- No `std::fs`, `std::net`, `std::process` access
- No `unsafe` operations
- Execution timeout: 5 seconds
- Memory limit: 10MB heap allocation
- Available APIs: `spawn()`, `modify()`, `delete()`, `math::*`, `rand::*`

### 3.5 Tier 4: Export

#### `gen_export_scene`

```json
{
  "name": "gen_export_scene",
  "description": "Export the current scene to a file.",
  "parameters": {
    "path": { "type": "string", "description": "Output file path" },
    "format": { "type": "string", "enum": ["gltf", "glb"], "default": "glb" }
  },
  "required": ["path"]
}
```

#### `gen_export_screenshot`

High-resolution render to file (separate from viewport screenshot, can be different resolution).

```json
{
  "name": "gen_export_screenshot",
  "description": "Render a high-resolution image of the scene.",
  "parameters": {
    "path": { "type": "string" },
    "width": { "type": "integer", "default": 1920 },
    "height": { "type": "integer", "default": 1080 },
    "samples": { "type": "integer", "default": 4, "description": "MSAA sample count for anti-aliasing" }
  },
  "required": ["path"]
}
```

### 3.6 Tool Summary

| Tier | Tool | Impact | Purpose |
|------|------|--------|---------|
| Perceive | `gen_scene_info` | Read-only | Scene graph introspection |
| Perceive | `gen_screenshot` | Read-only | Visual feedback loop (critical) |
| Perceive | `gen_entity_info` | Read-only | Entity detail inspection |
| Mutate | `gen_spawn_primitive` | Additive | Create visible 3D objects |
| Mutate | `gen_modify_entity` | Idempotent | Update entity properties |
| Mutate | `gen_delete_entity` | Destructive | Remove entities |
| Mutate | `gen_set_camera` | Idempotent | Frame the view |
| Mutate | `gen_set_light` | Additive/Idempotent | Control lighting |
| Mutate | `gen_set_environment` | Idempotent | Background, ambient, fog |
| Advanced | `gen_spawn_voxels` | Additive | Bulk block placement |
| Advanced | `gen_spawn_mesh` | Additive | Custom geometry |
| Advanced | `gen_execute_script` | Variable | Sandboxed scripting |
| Export | `gen_export_scene` | External | Save to glTF |
| Export | `gen_export_screenshot` | External | High-res render |

**Total: 14 tools.** Deliberately compact — each tool does one thing well. Complex scenes emerge from composition.

---

## 4. Default Scene

When LocalGPT Gen starts, it initializes a default scene so the LLM has context immediately:

- **Ground plane**: 20×20 gray plane at origin
- **Camera**: Position `[5, 5, 5]`, looking at `[0, 0, 0]`, 45° FOV
- **Directional light**: Warm white, 10000 lux, casting shadows from `[4, 8, 4]`
- **Ambient light**: 10% intensity, neutral white
- **Background**: Dark gray `[0.1, 0.1, 0.1, 1.0]`

The system prompt for the agent includes the default scene state so the LLM knows what exists before making any tool calls.

---

## 5. The Iterative Loop

The core workflow that makes this system effective:

```
User: "Build a small cabin in the woods"

LLM reasoning:
  1. gen_set_environment(background_color=[0.5, 0.7, 0.9, 1.0])  ← sky
  2. gen_spawn_primitive(name="cabin_floor", shape="Cuboid",
       dimensions={x:4, y:0.2, z:3}, color=[0.5, 0.3, 0.15, 1])
  3. gen_spawn_primitive(name="cabin_wall_front", ...)
  4. gen_spawn_primitive(name="cabin_wall_back", ...)
  5. gen_spawn_primitive(name="cabin_wall_left", ...)
  6. gen_spawn_primitive(name="cabin_wall_right", ...)
  7. gen_spawn_primitive(name="cabin_roof", shape="Cuboid",
       dimensions={x:5, y:0.15, z:4}, rotation_degrees=[0, 0, 15], ...)
  8. gen_set_camera(position=[8, 4, 6], look_at=[2, 1.5, 1.5])
  9. gen_screenshot(wait_frames=3)                                ← SEE RESULT

LLM sees screenshot, notices roof angle is wrong:
  10. gen_modify_entity(name="cabin_roof", rotation_degrees=[0, 0, 25])
  11. gen_screenshot()                                             ← VERIFY

LLM sees improvement, adds trees:
  12. gen_spawn_primitive(name="tree_1_trunk", shape="Cylinder", ...)
  13. gen_spawn_primitive(name="tree_1_canopy", shape="Sphere",
        color=[0.2, 0.5, 0.1, 1], parent="tree_1_trunk", ...)
  ...
  20. gen_screenshot()                                             ← FINAL
```

**Key principle:** At 60fps with in-process channels, round trip from "spawn" to "screenshot ready" is <100ms. The LLM can iterate dozens of times per conversation turn. Quality comes from iteration, not from individual call sophistication.

---

## 6. Implementation Phases

### Phase 1: Foundation (Weeks 1-3)

**Goal:** Single binary with Bevy window, channel communication, 3 tools working end-to-end.

| # | Task | Details | Est. |
|---|------|---------|------|
| 1.1 | Feature flag skeleton | `gen` feature in `Cargo.toml`, conditional compilation gates, CI builds both variants | 2d |
| 1.2 | Bevy app initialization | `GenPlugin` with window, default scene (ground + camera + light), main thread ownership | 2d |
| 1.3 | Channel infrastructure | `GenCommand`/`GenResponse` enums, `mpsc` channels, Bevy system that polls commands each frame | 2d |
| 1.4 | `gen_spawn_primitive` | All 8 shapes, position/rotation/scale, PBR material, name registry | 3d |
| 1.5 | `gen_screenshot` | Render to texture, save PNG, `wait_frames` logic, return path | 2d |
| 1.6 | `gen_scene_info` | Walk entity tree, serialize names/types/transforms/materials to JSON | 1d |
| 1.7 | Tool registration | Wire tools into LocalGPT's existing `ToolSchema` system in `tools.rs` | 1d |
| 1.8 | System prompt update | Add Gen tools to agent system prompt, include default scene state | 1d |
| 1.9 | Smoke test | Manual test: start binary, describe a scene in chat, verify objects appear in window | 1d |

**Exit criteria:** User types "make a red cube and a blue sphere", they appear in the Bevy window, screenshot shows them.

### Phase 2: Full Tool Suite (Weeks 4-6)

**Goal:** All 14 tools operational. Iterative creation workflow validated.

| # | Task | Details | Est. |
|---|------|---------|------|
| 2.1 | `gen_modify_entity` | Partial update of transform/material/visibility by name | 2d |
| 2.2 | `gen_delete_entity` | Recursive despawn with children, name registry cleanup | 1d |
| 2.3 | `gen_entity_info` | Component introspection per entity | 1d |
| 2.4 | `gen_set_camera` | Position, look_at, FOV, perspective/orthographic | 1d |
| 2.5 | `gen_set_light` | Directional, point, spot lights with shadows | 2d |
| 2.6 | `gen_set_environment` | Background, ambient, fog | 1d |
| 2.7 | `gen_spawn_voxels` | Palette + layer grid parser, voxel mesh generation, integration with `bevy_voxel_world` or custom chunked meshing | 5d |
| 2.8 | `gen_spawn_mesh` | Raw vertex/index/normal/UV → Bevy Mesh, auto-normal computation | 2d |
| 2.9 | `gen_export_scene` | glTF/GLB export via `bevy_gltf` | 2d |
| 2.10 | `gen_export_screenshot` | High-res offscreen render | 1d |
| 2.11 | Iterative loop validation | End-to-end test: "build a cabin" → multi-step creation with screenshots → export | 2d |

**Exit criteria:** "Build a small house with a door, two windows, and a chimney, surrounded by 4 trees" produces a recognizable scene through iterative refinement. Export to .glb opens correctly in external viewer.

### Phase 3: Scripting & Polish (Weeks 7-9)

**Goal:** Rhai scripting, scene persistence, UX polish.

| # | Task | Details | Est. |
|---|------|---------|------|
| 3.1 | `gen_execute_script` | Rhai integration via `bevy_mod_scripting`, sandbox constraints, timeout/memory limits | 5d |
| 3.2 | Scene save/load | Serialize scene to JSON/Ron, reload on startup, integrate with LocalGPT memory system | 3d |
| 3.3 | Undo/checkpoint | Scene state snapshots, rollback to previous checkpoint (experiment pattern from bevy_debugger_mcp) | 3d |
| 3.4 | Orbit camera controls | Mouse drag to orbit/pan/zoom in the Bevy window (user can manually inspect while LLM works) | 2d |
| 3.5 | System prompt optimization | Tune tool descriptions, add few-shot examples for common tasks, test with multiple LLM providers | 2d |
| 3.6 | Performance profiling | Measure: binary size, startup time, tool round-trip latency, memory usage with 1000+ entities | 2d |
| 3.7 | Documentation | README section for Gen, example prompts, architecture diagram, GIF demos | 2d |

**Exit criteria:** Feature-complete for v1.0 announcement. Binary < 55MB. Startup < 2s. Tool latency < 50ms.

### Phase 4: Ecosystem (Post-v1.0, Ongoing)

| # | Task | Details |
|---|------|---------|
| 4.1 | BRP compatibility layer | Optionally expose Gen tools as BRP methods so external MCP clients (bevy_brp_mcp, etc.) can also use them |
| 4.2 | Asset import | Load .glb/.obj files from disk into the scene (complement to export) |
| 4.3 | Texture support | Apply image textures to materials (load from disk, tile, offset) |
| 4.4 | Procedural SDF | LLM generates SDF functions → marching cubes → Bevy mesh (via `isosurface` crate) |
| 4.5 | Animation | Simple keyframe animations (rotate, translate, scale over time) |
| 4.6 | Multi-view | Split viewport, top/front/side views for LLM spatial reasoning |
| 4.7 | Headless mode | Render without window (CI/CD, server-side generation) |
| 4.8 | LocalGPT heartbeat integration | Autonomous scene generation via heartbeat tasks ("every morning, generate a new abstract art piece") |

---

## 7. Bevy Crate Dependencies

| Crate | Purpose | License | Required for |
|-------|---------|---------|-------------|
| `bevy` (with features: `default`, `png`, `bevy_remote`) | Core engine, rendering, windowing | MIT/Apache-2.0 | Phase 1 |
| `bevy_voxel_world` | Chunk-managed voxel world, multithreaded meshing | MIT/Apache-2.0 | Phase 2 (voxels) |
| `bevy_mod_scripting` | Rhai scripting runtime in ECS | MIT/Apache-2.0 | Phase 3 (scripting) |
| `image` | PNG encoding for screenshots | MIT/Apache-2.0 | Phase 1 |
| `isosurface` | Marching cubes for SDF→mesh | MIT | Phase 4 (SDF) |

All crates use permissive licenses compatible with LocalGPT's licensing.

---

## 8. Quality Ceiling & Sweet Spots

Based on research into LLM 3D generation capabilities:

### Where Gen Excels (Target Use Cases)

- **Architectural blockouts**: Buildings, room layouts, furniture placement. SceneCraft demonstrated 100+ asset scenes with LLM composition.
- **Game level prototyping**: Gray-boxing with primitives is standard game dev practice. Gen automates it.
- **Voxel worlds**: Minecraft-style structures. MC-Bench shows Claude and GPT-4 producing recognizable castles, characters, landscapes.
- **Abstract/geometric art**: Mathematical patterns, data visualization, parametric designs.
- **Technical diagrams**: Exploded views, assembly sequences, mechanical illustrations.
- **Educational models**: Solar systems, molecular structures, geometric proofs.

### Where Gen Struggles (Explicitly Out of Scope for v1)

- Organic characters (faces, hands, animals)
- Photorealistic natural environments (terrain, vegetation, water)
- Production game assets (clean topology, LOD, optimized UVs)
- Detailed props with complex topology (thin features, holes, undercuts)

### Quality Principle

**Visual quality is 80% lighting, materials, and composition — not geometric complexity.** The demoscene proved this: Ctrl-Alt-Test's *B – Incubation* used only cubes and looked compelling. 50 well-placed, well-lit cuboids with good materials > 50,000 poorly arranged triangles.

Gen's system prompt should emphasize this principle to the LLM: spend tool calls on lighting and camera positioning, not on adding more geometry.

---

## 9. Competitive Positioning

### vs. blender-mcp

| Dimension | blender-mcp | LocalGPT Gen |
|-----------|-------------|-------------|
| Requires | Blender (~700MB) + Python + MCP server | Nothing — single binary |
| Startup | ~5-10s (Blender launch) | <2s |
| Tool latency | ~100-500ms (TCP + Python) | <50ms (in-process) |
| Perception | Viewport screenshot via bpy | Viewport screenshot via Bevy |
| Escape hatch | `execute_blender_code` (unrestricted Python) | `gen_execute_script` (sandboxed Rhai) |
| Export | Full Blender export pipeline | glTF/GLB |
| Quality ceiling | Professional (Blender's renderer) | Stylized/low-poly (Bevy's PBR) |
| Target | Professional 3D artists + LLM | Developers + LLM, no 3D expertise needed |

**Positioning**: Gen is not a Blender replacement. It's the tool for people who will never open Blender. The "I just need a 3D scene, not a 3D art career" market.

### vs. bevy_brp_mcp

| Dimension | bevy_brp_mcp | LocalGPT Gen |
|-----------|-------------|-------------|
| Architecture | Two-process (MCP server + Bevy app) | Single binary, in-process |
| Abstraction | Raw ECS CRUD | Intent-level creative tools |
| Type system | Fully-qualified Bevy type names | Simple parameter names |
| Target user | Bevy developers debugging their game | Non-Bevy users creating 3D content |
| Tool count | ~30 (comprehensive ECS access) | 14 (focused creative workflow) |

**Positioning**: bevy_brp_mcp is for Bevy developers who need an AI assistant while building games. Gen is for anyone who wants to describe a 3D scene and see it materialize.

---

## 10. Integration with LocalGPT Core

### 10.1 Memory System

Gen scenes integrate with LocalGPT's existing memory:

- **MEMORY.md**: Store scene descriptions, user preferences ("user prefers low-poly aesthetic"), project context.
- **Scene files**: Saved scenes stored alongside workspace files, indexed by memory search.
- **Cross-session continuity**: "Continue the castle I was building yesterday" → load scene from memory, resume.

### 10.2 Heartbeat Tasks

Gen supports autonomous creation via heartbeat:

```toml
# HEARTBEAT.md
[[tasks]]
schedule = "daily 6:00"
action = "gen_create_daily_art"
prompt = "Create an abstract geometric art piece inspired by today's date. Export as PNG to ~/art/"
```

### 10.3 Skills System

Gen tools register as a skill module. Custom skills can compose Gen tools:

```toml
# skills/architecture.toml
[skill]
name = "architecture_blockout"
description = "Creates architectural floor plan blockouts from room descriptions"
tools = ["gen_spawn_primitive", "gen_set_camera", "gen_screenshot", "gen_export_scene"]
```

### 10.4 Web GUI Integration

When running in web GUI mode, the Bevy viewport can be streamed as video/images to the browser. Phase 4 consideration — initial version requires the native Bevy window.

---

## 11. Testing Strategy

### Unit Tests

- Command/response serialization round-trip
- Name registry: insert, lookup, delete, duplicate handling
- Primitive dimension validation (no negative radii, etc.)
- Material parameter clamping (metallic/roughness 0-1)
- Voxel grid parser: palette mapping, layer dimensions, air handling

### Integration Tests

- Spawn primitive → screenshot → verify non-empty image
- Spawn + modify + delete → scene_info shows correct state
- Export to glTF → reimport → verify entity count matches
- 1000 entities → verify no OOM, stable framerate
- Script execution → verify sandbox prevents file system access

### LLM Evaluation

- Prompt: "Make a red cube" → verify screenshot contains red-ish pixels in center
- Prompt: "Build a house" → verify ≥5 entities spawned, ≥1 has roof-like position (y > other entities)
- Prompt: "Delete everything" → verify scene_info returns only default entities
- 10 diverse prompts → measure tool call count, error rate, subjective quality score

---

## 12. Open Questions

1. **Bevy version pinning.** Bevy 0.15 vs 0.16 vs 0.17 vs 0.18? The `bevy_brp` ecosystem tracks latest (0.18). Using a recent stable release maximizes crate compatibility. Decision needed before Phase 1.

2. **Multimodal screenshot delivery.** When the LLM supports vision (GPT-4o, Claude), should screenshots be delivered as base64 inline or as file paths? Base64 enables the LLM to "see" the scene without external file reading. File paths work with all LLMs but require tool-use to read the image. Recommendation: support both, prefer base64 when the model supports vision.

3. **Headless mode priority.** Should headless rendering (no window) be Phase 1 or Phase 4? Headless enables CI/CD and server-side use but adds complexity (virtual framebuffer, offscreen render targets). Recommendation: Phase 4 unless a concrete server deployment use case emerges.

4. **BRP compatibility.** Should Gen tools also be exposed as BRP methods so bevy_brp_mcp can call them? This enables the ecosystem to use Gen tools from any MCP client. Adds complexity but increases adoption. Recommendation: Phase 4 after core tools are stable.

5. **Texture workflow.** Users will quickly want textured surfaces. Polyhaven integration (like blender-mcp's) provides free PBR textures. But this requires network access (Polyhaven API) which conflicts with LocalGPT's offline-first philosophy. Resolution: download textures to local cache, work offline from cache. Phase 4.

---

## 13. Success Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Binary size (gen build) | < 55MB | `ls -la target/release/localgpt` |
| Startup to first frame | < 2s | Timestamp from binary start to window visible |
| Tool round-trip (spawn → response) | < 50ms p99 | Instrumented channel latency |
| Screenshot capture (spawn → PNG ready) | < 200ms | Including wait_frames=3 |
| 1000 entities framerate | > 30 fps | Bevy diagnostics plugin |
| "Red cube" test (prompt → correct scene) | > 95% success | 20 trials across LLM providers |
| "Build a house" test (multi-step) | > 80% recognizable | Human evaluation, 10 trials |
| Export validity | 100% | glTF validator pass rate |
