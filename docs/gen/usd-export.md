# USD Export (Planned)

LocalGPT Gen currently exports worlds as **glTF/GLB** (3D interchange), **HTML** (browser viewer), and **RON** (native format); what each carries is in `website/content/docs/gen/export.md`. This page tracks the plan for adding **USD/USDZ** export. Nothing is implemented: no tool or crate in the tree reads or writes USD.

## Why USD?

[Universal Scene Description](https://openusd.org/) (USD) is Pixar's scene description format, now the standard for spatial computing and film pipelines.

For LocalGPT Gen specifically:
- External Apple viewers could preview exported USDZ files in AR Quick Look
- Film and VFX tools could receive richer scene structure than glTF carries
- USDZ remains an export question, not a reason to build a separate native viewer

glTF and USD are complementary, not competing. glTF is the right default for web and game engines. The cost of USD is its SDK: OpenUSD is C++, and there is no mature Rust library.

## Implementation Plan

### Phase 1: USDZ via converter (short term)

Add a `gen_export_usdz` tool that converts the existing glTF export to USDZ using an external converter. No Rust USD library needed.

```bash
# Export flow
gen_export_gltf → scene.glb → usdzconvert → scene.usdz
```

**Converter options:**
- Apple's `usdzconvert` (part of Apple's USDZ Tools download, not of Xcode)
- Google's `usd_from_gltf` (cross-platform, open source)
- Blender's USD exporter (via headless Blender)

This is minimal code — one tool that shells out to the converter. It carries only what the glTF export carries: meshes, PBR materials, transforms and hierarchy. Lights, behaviors, audio, camera and environment are not in the `.glb` (`crates/gen/src/gen3d/gltf_export.rs`).

### Phase 2: Minimal Rust USD writer (medium term)

Write a focused USD exporter in Rust for the subset LocalGPT uses:
- Meshes (parametric shapes → USD mesh prims)
- Materials (PBR → UsdPreviewSurface)
- Transforms (hierarchy → Xform prims)
- Lights (point, spot, directional → UsdLux)
- Camera

LocalGPT's scene data is simple parametric shapes, not complex film assets. A minimal writer avoids the full OpenUSD C++ SDK dependency.

**Potential crate:** Write a `usd-writer` crate that produces binary USD (.usdc) files directly. The USDC format is [documented](https://openusd.org/release/spec_usdformat.html) and several minimal implementations exist as reference.

### Phase 3: Native USD (long term)

Convert between the format's `WorldManifest` and a USD stage, with no glTF in between:
- `WorldManifest` → USD stage conversion
- USD → `WorldManifest` import (load Meta WorldGen output, Omniverse scenes)
- Bidirectional: edit in Gen, refine in Blender/Maya, bring back

`localgpt-world-types` re-exports the `openworldformat` crate's document types and holds no exporter (the glTF one is Gen's and reads Bevy meshes), so this converter would be new code over those types. A change to the format itself belongs in the spec repository.

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
