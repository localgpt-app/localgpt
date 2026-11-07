# USD Export (Planned)

LocalGPT Gen currently exports worlds as **glTF/GLB** (3D interchange), **HTML** (browser viewer), and **RON** (native format). This page tracks the plan for adding **USD/USDZ** export.

## Why USD?

[Universal Scene Description](https://openusd.org/) (USD) is Pixar's scene description format, now the standard for spatial computing and film pipelines.

| Ecosystem | USD Role |
|-----------|----------|
| **Apple Vision Pro / RealityKit** | USDZ is the native 3D format — required for spatial experiences |
| **Meta WorldGen** | Outputs USD scenes from its generation pipeline |
| **NVIDIA Omniverse** | Built entirely on USD for collaborative 3D workflows |
| **Blender, Maya, Houdini** | Import/export USD for film and VFX |
| **Unity, Unreal Engine** | USD import for asset pipelines |

For LocalGPT Gen specifically:
- External Apple viewers could preview exported USDZ files in AR Quick Look
- Film and VFX tools could receive richer scene structure than glTF carries
- USDZ remains an export question, not a reason to build a separate native viewer

## Current Export Formats

| Format | Tool | Use Case |
|--------|------|----------|
| **RON** (`world.ron`) | `gen_save_world` | Native save/load — preserves all parametric data, behaviors, audio |
| **glTF/GLB** | `gen_export_gltf` | Standard 3D interchange — works in Blender, engines, web viewers |
| **HTML** | `gen_export_html` | Self-contained browser viewer with Three.js and Web Audio |

## USD vs glTF

| | glTF | USD |
|---|---|---|
| **Strengths** | Web-first, lightweight, wide engine support | Rich scene hierarchy, variants, physics, layers |
| **Weaknesses** | Limited scene composition, no variants | Heavy SDK (C++), no mature Rust library |
| **Best for** | Web, game engines, quick sharing | Spatial computing, film/VFX, Apple ecosystem |
| **LocalGPT today** | Fully supported | Not yet |

glTF and USD are complementary, not competing. glTF is the right default for web and game engines. USD unlocks spatial computing and professional pipelines.

## Implementation Plan

### Phase 1: USDZ via converter (short term)

Add a `gen_export_usdz` tool that converts the existing glTF export to USDZ using an external converter. No Rust USD library needed.

```bash
# Export flow
gen_export_gltf → scene.glb → usdzconvert → scene.usdz
```

**Converter options:**
- Apple's `usdzconvert` (ships with Xcode command line tools on macOS)
- Google's `usd_from_gltf` (cross-platform, open source)
- Blender's USD exporter (via headless Blender)

This is minimal code — one tool that shells out to the converter. Unlocks:
- Apple Quick Look previews on iOS/macOS
- Drag-and-drop into Vision Pro
- Import into Omniverse, Blender, Maya

### Phase 2: Minimal Rust USD writer (medium term)

Write a focused USD exporter in Rust for the subset LocalGPT uses:
- Meshes (parametric shapes → USD mesh prims)
- Materials (PBR → UsdPreviewSurface)
- Transforms (hierarchy → Xform prims)
- Lights (point, spot, directional → UsdLux)
- Camera

LocalGPT's scene data is simple parametric shapes, not complex film assets. A minimal writer avoids the full OpenUSD C++ SDK dependency.

**Potential crate:** Write a `usd-writer` crate that produces binary USD (.usdc) files directly. The USDC format is [documented](https://openusd.org/release/spec_usdformat.html) and several minimal implementations exist as reference.

### Phase 3: Native USD in world-types (long term)

Add USD as a first-class format in `localgpt-world-types` alongside RON and glTF:
- `WorldManifest` → USD stage conversion
- USD → `WorldManifest` import (load Meta WorldGen output, Omniverse scenes)
- Bidirectional: edit in Gen, refine in Blender/Maya, bring back

This becomes important if:
- A connected 3D app targets Apple Vision Pro
- Users want to import USD worlds from other tools
- The Rust USD ecosystem matures

## Status

| Phase | Status | Depends On |
|-------|--------|------------|
| Phase 1: USDZ via converter | Not started | `usdzconvert` or `usd_from_gltf` on PATH |
| Phase 2: Rust USD writer | Not started | Phase 1 validation |
| Phase 3: Native USD | Not started | Phase 2, ecosystem maturity |
