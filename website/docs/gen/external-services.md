---
sidebar_position: 14.9
---

# External Services

Most of LocalGPT Gen runs entirely locally with no external dependencies. Three features require optional local services running on your machine — no cloud APIs, no remote calls.

## Overview

| Feature | Service | What It Does | GPU Required |
|---------|---------|--------------|-------------|
| [NPC Intelligence](#ollama--npc-brains) | Ollama | Autonomous NPC behavior via local LLMs | 2-7 GB |
| [Depth Preview](#comfyui--depth-preview) | ComfyUI | Styled 2D preview from depth maps | 2-8 GB |
| [3D Asset Generation](#model-server--3d-assets) | Model server | Text-to-3D mesh generation | 5-16 GB |

All services run on `localhost`. No API keys required.

## Quick Start

```bash
# Easiest: NPC brains (single binary, no Python)
brew install ollama   # or: curl -fsSL https://ollama.ai/install.sh | sh
ollama pull llama3.2:3b

# Moderate: depth-conditioned preview (Python)
git clone https://github.com/comfyanonymous/ComfyUI.git
cd ComfyUI && pip install -r requirements.txt
python main.py   # listens on 127.0.0.1:8188

# Advanced: 3D asset generation (Python + large GPU)
# Any server implementing the model server protocol below, on port 8741
```

## Ollama — NPC Brains

Attaches autonomous AI brains to NPCs. Each NPC runs a local language model that perceives the scene, reasons about goals, and outputs actions every few seconds.

### Install

```bash
# macOS
brew install ollama

# Linux
curl -fsSL https://ollama.ai/install.sh | sh

# Start server (if not already running as service)
ollama serve

# Pull models
ollama pull llama3.2:3b       # NPC brain (text-only, 2 GB)
```

### How It Works

```
gen_set_npc_brain { entity: "elder", personality: "wise sage" }
  → Spawns brain loop (ticks every 2 seconds):
    1. Perceive: gather nearby entities, player distance
    2. Build prompt: personality + goals + memories + perception
    3. Infer: POST to Ollama localhost:11434/api/chat
    4. Parse: extract action (speak, move_to, emote, etc.)
    5. Execute: trigger Bevy systems
```

### NPC Actions

| Action | Example | Result |
|--------|---------|--------|
| `speak("Hello!")` | Speech bubble, auto-dismiss |
| `move_to(10, 0, 5)` | Walk to position |
| `look_at("player")` | Smooth rotation |
| `emote(wave)` | Animation/particles |
| `interact("door")` | Trigger interaction system |
| `wander` | Random nearby movement |

### Supported Models

| Model | VRAM | Best For |
|-------|------|----------|
| `llama3.2:3b` | 2-3 GB | Default. Good action decisions. |
| `mistral:7b` | 5-7 GB | Faster, instruction-tuned. |
| `neural-chat:7b` | 5-6 GB | Optimized for dialogue. |

### Tools

| Tool | Description |
|------|-------------|
| `gen_set_npc_brain` | Attach an AI brain to an NPC with personality and goals (fails if Ollama or the model is missing) |
| `gen_npc_observe` | List what the NPC perceives (named entities in its view cone and radius) and optionally answer a question about it with its model |
| `gen_set_npc_memory` | Set NPC memory capacity and initial memories |

### Performance

- **Max concurrent brains:** 4 recommended
- **Distance culling:** Brain deactivates when NPC > 50m from player
- **Response latency:** ~200-500ms on GPU, ~2-5s on CPU
- **Tick rate:** Default 2.0 seconds (configurable per NPC)

Ollama's URL is `LOCALGPT_GEN_OLLAMA_URL` (default `http://localhost:11434`). If Ollama stops, brains pause and resume when it is back (checked every 30 seconds).

## ComfyUI — Depth Preview

Generates a styled 2D preview image from a scene's depth map. Lets the AI validate creative direction (colors, mood, style) before committing to full 3D generation.

### Install

```bash
git clone https://github.com/comfyanonymous/ComfyUI.git
cd ComfyUI
pip install -r requirements.txt

# SD 1.5 checkpoint and depth ControlNet, under the names Gen asks for
wget -P models/checkpoints https://huggingface.co/stable-diffusion-v1-5/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors
wget -O models/controlnet/control_v11f1p_sd15_depth.safetensors \
  https://huggingface.co/lllyasviel/control_v11f1p_sd15_depth/resolve/main/diffusion_pytorch_model.safetensors

python main.py   # listens on 127.0.0.1:8188
```

### How It Works

```
gen_preview_world { prompt, style_preset }
  → depth map: depth_map_path, or gen_render_depth of the current scene
  → POST /upload/image, POST /prompt (checkpoint + depth ControlNet workflow)
  → poll GET /history/{id}, fetch GET /view
  ← preview PNG written next to the depth map (or output_path)
```

If ComfyUI isn't reachable the tool fails with an error; it never returns a
path it didn't write.

### Configuration

| Variable | Default | Purpose |
|---|---|---|
| `LOCALGPT_GEN_COMFYUI_URL` | `http://127.0.0.1:8188` | ComfyUI server |
| `LOCALGPT_GEN_COMFYUI_CHECKPOINT` | `v1-5-pruned-emaonly.safetensors` | Checkpoint in `models/checkpoints/` |
| `LOCALGPT_GEN_COMFYUI_CONTROLNET` | `control_v11f1p_sd15_depth.safetensors` | Depth ControlNet in `models/controlnet/` |
| `LOCALGPT_GEN_COMFYUI_WORKFLOW` | — | Your own workflow (API format) instead of the built-in one; `{{prompt}}`, `{{negative}}`, `{{depth_image}}`, `{{width}}`, `{{height}}` and `{{seed}}` are filled in |

### Style Presets

| Preset | Style |
|--------|-------|
| `realistic` | Photorealistic, high detail, natural lighting |
| `stylized` | Pixar-style 3D render, vibrant colors |
| `pixel_art` | 16-bit retro game style |
| `watercolor` | Soft edges, muted colors |
| `concept_art` | Painterly, atmospheric perspective |

### GPU Requirements

- 2-4 GB VRAM for SD 1.5 + ControlNet
- 6-8 GB VRAM for SDXL + ControlNet (use a custom workflow)
- ~10-30 seconds per image at 512x512

### Tools

| Tool | Description |
|------|-------------|
| `gen_render_depth` | Render a depth map from the current scene |
| `gen_preview_world` | Generate a styled 2D preview from a depth map through ComfyUI |

## Model Server — 3D Assets

Generates 3D meshes and PBR textures from text prompts using local open-source models. LocalGPT Gen talks to the server over HTTP; it doesn't ship one, so any server that implements the protocol below works.

### How It Works

```
gen_generate_asset { prompt: "medieval sword", name: "sword", model: "tripo_sg" }
  → GET /health: fails right away if no server is running
  → POST /generate, returns a task_id; the tool returns at once
    ↓
Background: poll GET /status/{id} every 2 s (gen_generation_status shows it)
    ↓
Complete: GET /result/{id}/mesh → workspace/generated/meshes/{task}.glb,
  spawned as "sword" at the requested position and scale, saved with the world
```

`gen_generate_texture { entity, prompt }` works the same way and, on completion, downloads the maps it reports (`base_color`, `metallic_roughness`, `normal`, `emissive`) into `workspace/generated/textures/` and puts them on the entity's material. Saving the world copies them into its `assets/textures/`.

### Protocol

Base URL `LOCALGPT_GEN_MODEL_SERVER` (default `http://127.0.0.1:8741`).

| Request | Response |
|---|---|
| `GET /health` | `200` with any JSON (`{"status": "ok", ...}`) |
| `POST /generate` | `{"task_id": "..."}` |
| `GET /status/{id}` | `{"status": "queued" \| "generating" \| "complete" \| "failed", "progress": 0.4, "error": "...", "outputs": ["mesh"]}` |
| `GET /result/{id}/{output}` | The file: `mesh` is a GLB, texture maps are PNGs |
| `POST /cancel/{id}` | Anything |

`POST /generate` bodies:

```json
{ "type": "mesh", "prompt": "medieval sword", "model": "tripo_sg",
  "quality": "standard", "pbr": true, "output_format": "glb",
  "reference_image": null }

{ "type": "texture", "prompt": "mossy stone", "style": "realistic",
  "resolution": 1024, "shape": { "Cuboid": { "x": 1, "y": 1, "z": 1 } } }
```

`reference_image` is base64 image data or `null`; `shape` is the target's parametric shape, or `null` for imported meshes.

### Supported Models

| Model | VRAM | Speed (standard) | Output |
|-------|------|-------------------|--------|
| TripoSG | 8 GB | 30s | Mesh only |
| Hunyuan3D 2mini | 5-6 GB | 45s | Mesh + PBR textures |
| Hunyuan3D 2.1 | 10 GB | 60s | Full PBR |
| Step1X-3D | 16 GB | 90s | Mesh + PBR + LoRA |

### Tools

| Tool | Description |
|------|-------------|
| `gen_generate_asset` | Queue a 3D mesh generation task |
| `gen_generate_texture` | Queue PBR texture maps for an entity |
| `gen_generation_status` | Check progress, list tasks, or cancel one |
