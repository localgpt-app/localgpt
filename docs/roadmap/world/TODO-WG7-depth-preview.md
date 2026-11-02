# WorldGen 7: Depth-Conditioned Preview

**Nice-to-have preview workflow.** Before spending compute on full 3D population, show the user a 2D reference image of what the world will look like. Validates creative direction cheaply before committing to full generation.

**Source:** WorldGen paper Stage I.2 — blockout rendered as depth map from isometric camera, fed to depth-conditioned image generator to produce a reference image establishing visual style.

**Dependencies:** WG1 (blockout geometry to render), Bevy depth buffer access, external image generation API (ControlNet-style depth conditioning)

**Priority within WorldGen series:** 7 of 7 (lowest priority — depends on external API availability)

> **2026-11-01.** WG7.1 (`gen_render_depth`) stays: a depth map an agent can
> hand to its own image tools. WG7.2 (`gen_preview_world`, the ComfyUI
> client and the prompt composer) was removed with every other service call
> in the tree.

---

## Spec WG7.1: `gen_render_depth` — Depth Map from Blockout

**Goal:** Render the current scene (or blockout) as a depth map image from a configurable camera angle. The depth map captures spatial structure without material/color information.

### MCP Tool Schema

```json
{
  "name": "gen_render_depth",
  "description": "Render the current scene as a depth map image",
  "parameters": {
    "camera_angle": { "type": "enum", "values": ["isometric", "top_down", "front", "custom"], "default": "isometric" },
    "custom_position": { "type": "vec3", "optional": true },
    "custom_look_at": { "type": "vec3", "optional": true },
    "resolution": { "type": "vec2", "default": [1024, 1024] },
    "near_plane": { "type": "f32", "default": 0.1 },
    "far_plane": { "type": "f32", "default": 200.0 },
    "output_path": { "type": "string", "optional": true, "description": "File path for depth image. Auto-generated if omitted" },
    "add_noise": { "type": "bool", "default": true, "description": "Add small Gaussian perturbations to reduce grid artifacts" }
  }
}
```

### Implementation

1. **Depth rendering setup:** Spawn a temporary camera entity at the specified angle with:
   - `Camera3d` with depth-only rendering
   - `RenderTarget::Image` pointing to a CPU-accessible image buffer
   - Orthographic or perspective projection depending on camera angle (isometric uses orthographic for consistent depth)

2. **Camera positions:**
   - `isometric`: 45 degree elevation, 45 degree azimuth, positioned to frame the full scene. Orthographic projection.
   - `top_down`: directly above scene center, looking down. Orthographic.
   - `front`: ground level, looking at scene from north edge. Perspective.
   - `custom`: user-specified position and look-at target.

3. **Depth buffer extraction:** After rendering one frame:
   - Read the depth buffer from the render target
   - Normalize depth values to 0-1 range (0 = near plane, 1 = far plane)
   - Convert to grayscale image (white = close, black = far, matching ControlNet conventions)
   - Save as 16-bit PNG for precision (or 8-bit PNG for compatibility)

4. **Noise perturbation:** When `add_noise: true`, add small Gaussian noise (sigma = 0.01 × depth range) to each depth pixel. This reduces the grid-like appearance of procedural blockout geometry, matching WorldGen's approach (Section 3.1).

5. **Cleanup:** Despawn the temporary camera after capture.

6. **Output:** Return the file path and image dimensions:
   ```json
   {
     "path": "/tmp/gen_depth_001.png",
     "width": 1024,
     "height": 1024,
     "depth_range": [0.0, 150.0]
   }
   ```

### Acceptance Criteria

- [ ] Depth map renders from isometric angle capturing full scene
- [ ] Top-down angle provides overhead view
- [ ] Depth values are correctly normalized (near = white, far = black)
- [ ] Resolution parameter controls output image size
- [ ] Gaussian noise reduces grid artifacts when enabled
- [ ] Output image is valid PNG readable by standard tools
- [ ] Temporary camera is cleaned up after capture
- [ ] near/far plane parameters control depth range

### Files to Create/Modify

- `localgpt/crates/localgpt-gen/src/worldgen/depth.rs` — depth camera setup, buffer extraction, normalization
- `localgpt/crates/localgpt-gen/src/mcp/tools/gen_render_depth.rs` — MCP tool handler

---

## Spec WG7.2: `gen_preview_world` — Styled Preview from Depth Map

**Goal:** Take a depth map + text prompt and generate a styled 2D preview image showing what the world will look like. The user reviews this preview before committing to full 3D generation.

### MCP Tool Schema

```json
{
  "name": "gen_preview_world",
  "description": "Generate a styled 2D preview image from the blockout depth map",
  "parameters": {
    "prompt": { "type": "string", "required": true, "description": "Style description for the preview" },
    "depth_map_path": { "type": "string", "optional": true, "description": "Path to depth map. Auto-renders if omitted" },
    "style_preset": { "type": "enum", "values": ["realistic", "stylized", "pixel_art", "watercolor", "concept_art"], "optional": true },
    "negative_prompt": { "type": "string", "optional": true },
    "output_path": { "type": "string", "optional": true }
  }
}
```

### Implementation

1. **Depth map acquisition:** If `depth_map_path` is not provided, internally call `gen_render_depth(camera_angle: "isometric")` to generate one.

2. **Image generation:** Send the depth map + prompt to a depth-conditioned image generation API:
   - **Local option:** Use a local ControlNet model (via ComfyUI, Automatic1111, or a lightweight ONNX-based pipeline). This aligns with the local-first philosophy.
   - **Remote option:** Use an API endpoint that accepts depth-conditioned generation (e.g., Stability AI, Replicate).
   - The depth map constrains spatial structure; the prompt controls visual style.

3. **Style presets:** Map preset names to prompt suffixes and generation parameters:
   - `realistic`: "photorealistic, high detail, natural lighting"
   - `stylized`: "stylized 3D render, Pixar style, vibrant colors"
   - `pixel_art`: "pixel art, 16-bit, retro game style"
   - `watercolor`: "watercolor painting, soft edges, muted colors"
   - `concept_art`: "concept art, painterly, atmospheric perspective"

4. **Preview display:** After generation:
   - Save the preview image to `output_path`
   - Display it in the Gen window as a floating overlay (press any key to dismiss)
   - Return the path so the LLM can reference it

5. **Reference storage:** If the user approves the preview, store it in the WorldSpec as `reference_image_path`. This reference image guides style consistency during subsequent population passes (WG1.3) and evaluation (WG4.2).

6. **Iterate workflow:**
   ```
   User: "Create a mystical forest village"
   1. gen_plan_layout → BlockoutSpec
   2. gen_apply_blockout → coarse geometry
   3. gen_preview_world → styled 2D preview
   4. User reviews: "Looks good, but make it more autumn-colored"
   5. gen_preview_world(prompt adjusted) → new preview
   6. User approves
   7. gen_populate_region → full 3D content guided by reference image
   ```

### Acceptance Criteria

- [ ] Depth map is auto-rendered if not provided
- [ ] Preview image reflects depth map structure with styled appearance
- [ ] Style presets produce distinct visual styles
- [ ] Preview displays as overlay in Gen window
- [ ] Reference image can be stored in WorldSpec
- [ ] Multiple preview iterations work without accumulating artifacts
- [ ] Works with at least one image generation backend (local or remote)

### Files to Create/Modify

- `localgpt/crates/localgpt-gen/src/worldgen/preview.rs` — preview generation, API integration, overlay display
- `localgpt/crates/localgpt-gen/src/mcp/tools/gen_preview_world.rs` — MCP tool handler

---

## Summary

| Spec | Tool | What | Effort |
|------|------|------|--------|
| WG7.1 | `gen_render_depth` | Render scene as depth map image | Medium |
| WG7.2 | `gen_preview_world` | Generate styled 2D preview from depth map + prompt | Medium-High |

**Recommended build order:** WG7.1 → WG7.2

**Net effect:** Users can preview the visual direction of their world before committing to full 3D generation. This saves significant time when iterating on creative direction — adjusting a text prompt and regenerating a 2D preview takes seconds, while regenerating full 3D content takes minutes. The reference image also serves as a style guide for maintaining visual consistency during population passes.

**Caveat:** This spec depends on external image generation capability (ControlNet or equivalent). If no suitable API is available locally, this can be deferred. The depth map rendering (WG7.1) is independently useful for debugging and documentation even without the preview generation step.
