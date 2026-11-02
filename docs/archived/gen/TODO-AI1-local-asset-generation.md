# AI Integration 1: Local 3D Asset Generation Pipeline

**Status:** Archived 2026-11-01. The Rust side was built (`AssetGenManager`, the model-server client, `gen_generate_asset`, `gen_generate_texture`, `gen_generation_status`) against a model server that was never written, and removed with every other service call in the tree. An agent that wants a generated mesh runs its own generator and loads the result with `gen_load_gltf`.

**Transforms LocalGPT Gen from primitives-only to real 3D content.** Integrates open-source text/image-to-3D models that run on consumer GPUs (8–24 GB VRAM), producing game-engine-ready meshes with PBR materials. Output is standard glTF/GLB — Bevy loads it natively via the existing `gen_load_gltf` pipeline.

**Source:** AI World Generation Research — "Open-source models now enable a local-first 3D generation stack" (TripoSG, Hunyuan3D 2.0/2.1/2mini, Step1X-3D, HunyuanWorld 1.0, TRELLISWorld)

**Dependencies:** Python runtime (model inference), `gen_load_gltf` (already implemented), `gen_spawn_mesh` (already implemented), async task infrastructure

**Priority within AI series:** 1 of 2 (do this first — highest impact gap between current capabilities and research landscape)

---

## Spec AI1.1: `gen_generate_asset` — Text/Image to 3D Mesh

**Goal:** A single tool that takes a text prompt (or reference image path) and generates a 3D mesh using a locally-running open-source model. The mesh is saved as glTF/GLB and automatically loaded into the scene. Generation is async — the agent can continue other work while the mesh generates.

### MCP Tool Schema

```json
{
  "name": "gen_generate_asset",
  "description": "Generate a 3D mesh from text description or reference image using local AI model",
  "parameters": {
    "prompt": { "type": "string", "required": true, "description": "Text description of the 3D object to generate" },
    "reference_image": { "type": "string", "optional": true, "description": "Path to reference image for image-to-3D generation" },
    "name": { "type": "string", "required": true, "description": "Entity name for the generated mesh" },
    "position": { "type": "vec3", "default": [0, 0, 0] },
    "scale": { "type": "f32", "default": 1.0 },
    "model": { "type": "enum", "values": ["triposg", "hunyuan3d", "hunyuan3d_mini", "step1x"], "default": "triposg" },
    "quality": { "type": "enum", "values": ["draft", "standard", "high"], "default": "standard" },
    "pbr": { "type": "bool", "default": true, "description": "Generate PBR textures (albedo, roughness, metallic, normal). Requires hunyuan3d or step1x model." }
  }
}
```

### Response

Returns immediately with a generation task ID. The mesh auto-loads into the scene when generation completes:

```json
{
  "task_id": "gen_abc123",
  "status": "generating",
  "estimated_seconds": 45,
  "message": "Generating 'medieval_sword' with triposg (standard quality). Will auto-spawn at [0, 0, 0] when ready."
}
```

### Supported Models (Consumer GPU Targets)

| Model | VRAM | Speed | Output | License | Best For |
|-------|------|-------|--------|---------|----------|
| TripoSG | 8 GB+ | ~30s | Mesh only | MIT-compatible | Quick iteration, low VRAM |
| Hunyuan3D 2mini | 5–6 GB | ~45s | Mesh + PBR | Commercial OK | Lowest VRAM with textures |
| Hunyuan3D 2.1 | 10 GB+ | ~60s | Mesh + full PBR | Commercial OK | Best PBR quality |
| Step1X-3D | 16 GB+ | ~90s | Mesh + PBR, LoRA | Apache 2.0 | Stylistic control via LoRA |

### Implementation

1. **Model server process:** A persistent Python subprocess (`localgpt-model-server`) wraps inference models behind a local HTTP API. Launched on first `gen_generate_asset` call, stays alive for subsequent requests. Communicates via HTTP on localhost.

   ```
   POST http://localhost:8741/generate
   {
     "prompt": "medieval sword with ornate hilt",
     "model": "triposg",
     "quality": "standard",
     "output_format": "glb",
     "pbr": true
   }
   → { "task_id": "abc123" }

   GET http://localhost:8741/status/abc123
   → { "status": "complete", "output_path": "/tmp/gen_assets/abc123.glb" }
   ```

2. **Model server internals (Python):**
   - FastAPI server with task queue (asyncio + threading for GPU inference)
   - Model lazy-loaded on first request for that model type
   - GPU memory management: unload previous model before loading new one (single-model-at-a-time for consumer GPUs)
   - Health check endpoint: `GET /health` returns installed models, VRAM usage, active tasks

3. **Async flow in Bevy:**
   - Tool call → spawn async task via `IoTaskPool`
   - Task sends HTTP request to model server, returns task_id immediately
   - Bevy system polls model server every 2 seconds for active tasks
   - On completion: copy glTF to `skills/{world}/assets/generated/{name}.glb`, load via `asset_server.load()`, spawn entity with name/position/scale
   - Send `GenResponse::AssetGenerated` back to agent

4. **Asset caching:** Generated assets saved to world skill's `assets/generated/` directory. Same prompt + model + quality combo returns cached result (hash-based lookup).

5. **Fallback:** If no GPU available, model not installed, or server fails, return error with message suggesting `gen_spawn_primitive` as alternative. Never block the agent.

### Acceptance Criteria

- [ ] `gen_generate_asset` with text prompt produces a 3D mesh and spawns it in scene
- [ ] Image-to-3D via `reference_image` parameter works
- [ ] At least TripoSG model works on 8 GB VRAM GPU
- [ ] Generation is async — agent can continue other tool calls while mesh generates
- [ ] Generated asset saved to world skill assets folder
- [ ] Cached assets reused for identical prompt + model + quality requests
- [ ] Error handling: graceful failure when GPU unavailable or model not installed
- [ ] Model server auto-starts on first generation call, persists across generations
- [ ] Model server shuts down cleanly on gen process exit

### Files to Create/Modify

- `localgpt/crates/gen/src/gen3d/asset_gen.rs` — AssetGenManager resource, HTTP client, polling system
- `localgpt/crates/gen/src/gen3d/model_server.rs` — Model server process management (start/stop/health)
- `localgpt/crates/gen/src/gen3d/tools.rs` — Add `gen_generate_asset` tool definition
- `localgpt/crates/gen/src/gen3d/commands.rs` — Add `GenerateAsset` command and `AssetGenerated` response
- `localgpt/crates/gen/src/gen3d/plugin.rs` — Register asset gen polling system
- `localgpt/scripts/model_server/server.py` — FastAPI model server
- `localgpt/scripts/model_server/models/triposg.py` — TripoSG wrapper
- `localgpt/scripts/model_server/models/hunyuan3d.py` — Hunyuan3D wrapper
- `localgpt/scripts/model_server/requirements.txt` — Python dependencies

---

## Spec AI1.2: `gen_generate_texture` — PBR Texture Generation for Existing Meshes

**Goal:** Apply AI-generated PBR textures to existing scene entities (both parametric primitives and imported meshes). Uses Hunyuan3D-Paint or similar texture generation model to produce consistent albedo, roughness, metallic, and normal maps from a text description. Solves the "all my primitives look like solid colors" problem.

### MCP Tool Schema

```json
{
  "name": "gen_generate_texture",
  "description": "Generate and apply AI-created PBR textures to an existing entity",
  "parameters": {
    "entity": { "type": "string", "required": true, "description": "Name of entity to texture" },
    "prompt": { "type": "string", "required": true, "description": "Text description of desired material appearance" },
    "style": { "type": "enum", "values": ["realistic", "stylized", "pixel_art", "hand_painted", "toon"], "default": "realistic" },
    "resolution": { "type": "enum", "values": ["512", "1024", "2048"], "default": "1024" }
  }
}
```

### Implementation

1. **Geometry extraction:** Export the target entity's mesh geometry to a temporary glTF. For parametric primitives, convert Shape → mesh vertices. For imported meshes, re-export existing geometry.

2. **Multi-view rendering:** Render the entity from 4–6 canonical views (front, back, left, right, top, ¾ angle) as depth/normal maps. These condition the texture generation model.

3. **Texture model pipeline:**
   - Hunyuan3D-Paint: mesh geometry + text prompt → UV-mapped PBR texture maps
   - Fallback: depth-conditioned ControlNet for simpler single-texture results
   - Output: albedo map, roughness map, metallic map, normal map (PNG files)

4. **Material application:** Load generated texture maps as Bevy `Image` assets. Apply via `StandardMaterial` with proper UV coordinates. Preserve existing material properties (emissive, alpha mode) unless contradicted by prompt.

5. **Async flow:** Same pattern as AI1.1 — returns task_id immediately, polls for completion, auto-applies textures when ready.

### Acceptance Criteria

- [ ] `gen_generate_texture` produces PBR textures matching the text prompt
- [ ] Textures are UV-consistent across mesh surface (no obvious seams on simple geometry)
- [ ] Works on parametric primitives (cuboid, sphere, cylinder, etc.)
- [ ] Works on imported glTF meshes
- [ ] Generated textures saved to world skill `assets/textures/` folder
- [ ] Existing material properties (emissive, alpha) preserved unless overridden
- [ ] Multiple entities can be textured in sequence

### Files to Create/Modify

- `localgpt/crates/gen/src/gen3d/texture_gen.rs` — Texture generation pipeline, geometry extraction
- `localgpt/scripts/model_server/models/texture_paint.py` — Texture model wrapper (Hunyuan3D-Paint)
- `localgpt/crates/gen/src/gen3d/tools.rs` — Add `gen_generate_texture` tool
- `localgpt/crates/gen/src/gen3d/commands.rs` — Add `GenerateTexture` / `TextureGenerated` commands

---

## Spec AI1.3: `gen_generation_status` — Generation Queue & Status

**Goal:** Query and manage the async generation queue. Since 3D generation takes 30–120+ seconds per asset, the agent needs visibility into pending, active, and completed jobs to make informed decisions about what to do next.

### MCP Tool Schema

```json
{
  "name": "gen_generation_status",
  "description": "Check status of AI generation tasks (meshes, textures) or cancel pending tasks",
  "parameters": {
    "task_id": { "type": "string", "optional": true, "description": "Specific task ID to query. If omitted, returns all tasks" },
    "action": { "type": "enum", "values": ["status", "cancel", "list"], "default": "status" }
  }
}
```

### Response

```json
{
  "active": [
    {
      "task_id": "gen_abc123",
      "type": "mesh",
      "prompt": "medieval sword",
      "model": "triposg",
      "elapsed_seconds": 23,
      "estimated_remaining": 12
    }
  ],
  "completed": [
    {
      "task_id": "gen_xyz789",
      "type": "texture",
      "prompt": "mossy stone",
      "entity": "wall_north",
      "output_path": "assets/generated/wall_north_texture.png"
    }
  ],
  "failed": [],
  "queue_depth": 1,
  "model_server": {
    "status": "running",
    "gpu_memory_used_mb": 7800,
    "gpu_memory_total_mb": 12288,
    "loaded_model": "triposg"
  }
}
```

### Implementation

1. **GenerationManager resource:** Bevy resource tracking all generation tasks with state machine: `Queued → Generating → Loading → Complete | Failed`.

2. **Queue management:** Serial GPU execution (one generation at a time). Tasks queued in FIFO order. Cancel removes from queue or sends cancel signal to model server for active generation.

3. **GPU monitoring:** Query VRAM usage from model server `/health` endpoint. Agent can use this to choose appropriate model (e.g., pick `hunyuan3d_mini` instead of `hunyuan3d` when VRAM is limited).

4. **Auto-cleanup:** Completed/failed tasks purged after 10 minutes. Task history not persisted across gen restarts (ephemeral).

### Acceptance Criteria

- [ ] `gen_generation_status` returns accurate state of all generation tasks
- [ ] Cancel action stops in-progress generation or removes queued task
- [ ] GPU memory reporting helps agent choose appropriate model/quality
- [ ] Completed tasks include output paths for reference
- [ ] Failed tasks include error message for agent diagnosis

### Files to Create/Modify

- `localgpt/crates/gen/src/gen3d/asset_gen.rs` — Extend GenerationManager with status query and cancel
- `localgpt/crates/gen/src/gen3d/tools.rs` — Add `gen_generation_status` tool

---

## Architecture: Model Server

The model server is a shared Python process serving both mesh generation (AI1.1) and texture generation (AI1.2):

```
┌──────────────────────────────────────┐
│            Bevy Gen Process          │
│                                      │
│  ┌────────────┐  ┌────────────────┐  │
│  │ asset_gen  │  │ texture_gen    │  │
│  │ .rs        │  │ .rs            │  │
│  └──────┬─────┘  └───────┬────────┘  │
│         │   HTTP          │           │
└─────────┼────────────────┼───────────┘
          │                │
┌─────────▼────────────────▼───────────┐
│      localgpt-model-server           │
│      (Python / FastAPI)              │
│                                      │
│  ┌──────────┐ ┌──────────┐ ┌──────┐  │
│  │ TripoSG  │ │Hunyuan3D │ │Step1X│  │
│  │ wrapper  │ │ wrapper  │ │wrap. │  │
│  └──────────┘ └──────────┘ └──────┘  │
│                                      │
│  ┌─────────────────────────────────┐ │
│  │ GPU Manager                     │ │
│  │ (load/unload models, VRAM mon.) │ │
│  └─────────────────────────────────┘ │
└──────────────────────────────────────┘
          │
          ▼
     GPU (8–24 GB VRAM)
```

**Key design decisions:**
- **Separate process, not in-process Python:** Keeps Bevy's main loop clean. Model server can crash/restart without affecting the scene.
- **HTTP, not gRPC/pipes:** Simple, debuggable, curl-testable. Latency is irrelevant (generation takes 30–120s).
- **Single-model-at-a-time:** Consumer GPUs can't hold multiple 3D generation models. Explicit unload-before-load.
- **Auto-start, lazy-load:** Model server starts on first generation call. Models loaded on first request for that model type.
