---
title: "Export (glTF, HTML, Screenshots)"
---

# Export (glTF, HTML, Screenshots)

LocalGPT Gen can export worlds in multiple formats — each preserving different aspects of the scene.

## Format Comparison

| | **RON** (native) | **glTF/GLB** | **HTML** | **Screenshot** |
|---|---|---|---|---|
| **Tool** | `gen_save_world` | `gen_export_gltf` | `gen_export_html` | `gen_export_screenshot` |
| **Geometry** | Parametric (editable) | Baked meshes | Three.js shapes | Rasterized image |
| **Materials** | Full PBR params | PBR (color, metallic, roughness) | PBR (Three.js MeshStandardMaterial) | Rendered |
| **Lighting** | Full (point, spot, directional) | Not included | Full (Three.js lights) | Rendered |
| **Behaviors** | Full (orbit, spin, bob, etc.) | Not included | Full (JS animation loop) | Static frame |
| **Audio** | Full (ambience + emitters) | Not included | Full (Web Audio API) | Not included |
| **Camera** | Full (position, look-at, FOV) | Not included | Full (OrbitControls) | Current view |
| **Environment** | Full (sky, fog, ambient) | Not included | Full (background, fog) | Rendered |
| **Reloadable in Gen** | Yes (`gen_load_world`) | Yes (`gen_load_gltf`) | No | No |
| **Viewable externally** | No | Blender, Unity, Unreal, web | Any browser | Any image viewer |
| **File size** | Small (text) | Medium (binary meshes) | Small-medium (inline JS) | Small (PNG) |

## glTF/GLB Export

Export the scene as a standard [glTF 2.0](https://www.khronos.org/gltf/) binary file. This is the universal 3D interchange format — supported by Blender, Unity, Unreal Engine, Godot, web viewers, and virtually every 3D tool.

### Usage

```bash
# In the Gen terminal
> Export the scene as glTF
```

Or call the tool directly:

```json
{
  "name": "gen_export_gltf",
  "arguments": {
    "path": "my-world.glb"
  }
}
```

### What's Exported

- **Meshes** — all primitives and loaded meshes with positions, normals, UVs, and indices
- **Materials** — PBR metallic-roughness (base color, metallic factor, roughness factor)
- **Transforms** — position, rotation (quaternion), scale for every entity
- **Hierarchy** — parent-child relationships preserved as glTF node tree

### What's NOT Exported

- Behaviors (animations) — glTF supports animations but Gen doesn't write them yet
- Audio — no glTF equivalent
- Lighting — not baked into the glTF (the viewer provides its own)
- Environment (sky, fog) — viewer-dependent

### Output Location

| Input | Output |
|-------|--------|
| No path | `{workspace}/exports/{timestamp}.glb` |
| `path/scene.glb` | Writes to that path |
| `path/scene.gltf` | Splits into `scene.gltf` (JSON) + `scene.bin` (geometry) |
| `path/scene` | Appends `.glb` automatically |

### Opening in External Tools

```bash
# Blender
blender --python-expr "import bpy; bpy.ops.import_scene.gltf(filepath='scene.glb')"

# Three.js (web)
const loader = new GLTFLoader();
loader.load('scene.glb', (gltf) => scene.add(gltf.scene));

# macOS Quick Look (GLB files open natively)
open scene.glb
```

## HTML Export

Export the world as a **self-contained HTML file** that runs in any browser. This is the only export format that preserves **everything** — geometry, materials, lighting, behaviors, audio, camera, and environment.

### Usage

Save the world first, then export:

```bash
> Save this world as "my-castle"
> Export as HTML
```

Or via tools:

```json
{"name": "gen_save_world", "arguments": {"name": "my-castle"}}
{"name": "gen_export_html", "arguments": {}}
```

### What's Exported

- **Geometry** — all shapes rendered as Three.js meshes
- **Materials** — PBR via `MeshStandardMaterial` (color, metallic, roughness, emissive, alpha)
- **Lighting** — point, spot, directional lights with color and intensity
- **Behaviors** — full animation system (orbit, spin, bob, pulse, bounce, path_follow, look_at) running in a JS animation loop
- **Audio** — procedural ambience and spatial emitters via Web Audio API (wind, rain, forest, ocean, fire, water, hum)
- **Camera** — OrbitControls with the saved camera position and look-at target
- **Environment** — background color, fog, ambient light

### Output Location

The HTML file is written to the world skill's `export/` directory:

```
workspace/skills/my-castle/
├── world.ron          # Native format
├── SKILL.md           # Metadata
├── history.jsonl      # Edit history
└── export/
    └── index.html     # ← self-contained HTML viewer
```

### Features of the HTML Viewer

- **No server required** — open `index.html` directly in a browser, works offline
- **Mouse controls** — orbit (left-drag), zoom (scroll), pan (right-drag) via Three.js OrbitControls
- **Responsive** — resizes with the browser window
- **Audio autoplay** — ambient and spatial audio start on first click (browser autoplay policy)
- **Animations** — all behaviors run at 60fps in the browser

### Sharing

The HTML file is completely self-contained (no external dependencies). Share it by:
- Uploading to any static host (GitHub Pages, Netlify, Vercel)
- Sending the file directly — recipients just open it in a browser
- Embedding in an iframe on any website

## Screenshot Export

Capture a high-resolution image of the current scene.

### Usage

```json
{
  "name": "gen_export_screenshot",
  "arguments": {
    "path": "scene-capture.png",
    "width": 1920,
    "height": 1080
  }
}
```

### Parameters

| Parameter | Default | Description |
|-----------|---------|-------------|
| `path` | Auto-generated in workspace | Output file path |
| `width` | 1920 | Image width in pixels |
| `height` | 1080 | Image height in pixels |

The AI uses screenshots during generation to see what it built and course-correct. You can also use `gen_screenshot` (the in-scene version) which captures to a temp file and returns the image for the AI to analyze.

## World Export (`gen_export_world`)

A convenience tool that exports the entire saved world as glTF:

```json
{
  "name": "gen_export_world",
  "arguments": {
    "format": "glb"
  }
}
```

Exports to the world skill's `export/` directory (e.g., `workspace/skills/my-castle/export/scene.glb`). Supports `"glb"` (default) or `"gltf"` format.

## Choosing an Export Format

| Goal | Format |
|------|--------|
| Continue editing later | RON (`gen_save_world`) |
| Import into Blender / Unity / Unreal | glTF (`gen_export_gltf`) |
| Share with anyone (no install needed) | HTML (`gen_export_html`) |
| Social media / documentation | Screenshot (`gen_export_screenshot`) |
| Archive the complete world | RON + HTML (save both) |
