---
title: "WorldGen Pipeline"
---

# WorldGen Pipeline

The WorldGen pipeline transforms LocalGPT Gen from "LLM places things one by one" to "LLM designs worlds through a structured interface." Instead of the LLM generating geometry directly, it generates **parameters** — layout specs, density hints, style references — and procedural systems handle the actual placement.

This mirrors the architecture described in [Meta's WorldGen paper](https://www.meta.com/blog/worldgen-3d-world-generation-reality-labs-generative-ai-research/): LLMs should generate parameters, not geometry.

## Pipeline Stages

```
1. Plan          gen_plan_layout        Text → BlockoutSpec (JSON)
   ↓
2. Blockout      gen_apply_blockout     BlockoutSpec → terrain + regions + paths
   ↓
3. Navmesh       gen_build_navmesh      Terrain → walkability grid
   ↓
4. Populate      gen_populate_region    Regions → entities (hero/medium/decorative)
   ↓
5. Evaluate      gen_evaluate_scene     Screenshot + highlighting → LLM feedback
   ↓
6. Refine        gen_auto_refine        Automated evaluate → adjust loop
   ↓
7. Edit          gen_modify_blockout    Non-destructive region changes
   ↓
8. Regenerate    gen_regenerate         Incremental update after edits
```

## Tools (15)

### Layout & Blockout

| Tool | Description |
|------|-------------|
| `gen_plan_layout` | Generate a structured world layout plan from a text description. Returns a `BlockoutSpec` JSON with terrain type, regions, paths, and density hints. |
| `gen_apply_blockout` | Apply a blockout spec to create terrain, regions with hero slots, medium density zones, and connecting paths. |
| `gen_modify_blockout` | Add, remove, resize, or move blockout regions with incremental regeneration. Supports non-destructive editing. |
| `gen_regenerate` | Regenerate regions after blockout changes, preserving manually placed entities. |

### Placement

| Tool | Description |
|------|-------------|
| `gen_populate_region` | Populate a region with entities using three-tier placement: hero (landmarks), medium (structures), decorative (props/vegetation). Respects collision clearance and ground snap. |
| `gen_set_tier` | Set an entity's placement tier (hero, medium, decorative). Affects clearance and placement priority. |
| `gen_set_role` | Set an entity's semantic role (ground, structure, prop, vegetation, water, sky, light, effect). Enables bulk operations by role. |
| `gen_bulk_modify` | Modify multiple entities by role or tier. Example: recolor all vegetation, rescale all decorative props. |

### Navigation

| Tool | Description |
|------|-------------|
| `gen_build_navmesh` | Build a grid-based walkability analysis for the current terrain. Detects steep slopes, obstacles, and blocked areas. |
| `gen_validate_navigability` | Check that key points (spawn, landmarks, exits) are reachable via A* pathfinding on the navmesh. |
| `gen_edit_navmesh` | Manually override navmesh cells: block areas, allow previously blocked cells, add connections between disconnected regions. |

### Evaluation

| Tool | Description |
|------|-------------|
| `gen_evaluate_scene` | Take a screenshot with optional entity highlighting (red emissive overlay) and camera presets (current, top-down, isometric, front, entity-focus). Annotate with entity names and bounding boxes. For LLM self-evaluation. |
| `gen_auto_refine` | Automated generate → screenshot → evaluate → adjust loop. The LLM evaluates its own output and makes corrections. |

### Depth & Preview

| Tool | Description |
|------|-------------|
| `gen_render_depth` | Render a depth map of the scene from a specified camera angle (top-down, isometric, front, custom). Outputs grayscale PNG. |
| `gen_preview_world` | Generate a styled 2D preview image from a depth map (requires [external service](/docs/gen/external-services)). |

## Three-Tier Placement

The population system uses a hierarchy that matches how real environments are built:

| Tier | Role | Clearance | Examples |
|------|------|-----------|---------|
| **Hero** | Landmarks, focal points | Large (8m+) | Town hall, ancient tree, monument |
| **Medium** | Supporting structures | Medium (3-5m) | Houses, wells, market stalls |
| **Decorative** | Detail and atmosphere | None/small | Flowers, barrels, rocks, grass |

Hero slots are placed first at specific positions defined in the blockout. Medium entities fill density targets. Decorative entities scatter to reach the region's decorative density.

## BlockoutSpec Format

The `gen_plan_layout` tool returns a JSON blockout specification:

```json
{
  "terrain": {
    "type": "hills",
    "verticality": 0.6,
    "roughness": 0.4,
    "base_height": 0.0
  },
  "layout": {
    "style": "organic",
    "density": 0.5,
    "regularity": 0.3
  },
  "regions": [
    {
      "id": "village_center",
      "bounds": { "center": [0, 0], "size": [20, 20] },
      "type": "structured",
      "density": 0.7,
      "walkable": true,
      "hero_slots": [
        { "position": [0, 0, 0], "size": [8, 6, 8], "role": "landmark", "hint": "town hall" }
      ],
      "medium_density": 0.5,
      "decorative_density": 0.3
    }
  ],
  "paths": [
    {
      "from": "village_center",
      "to": "forest_edge",
      "style": "cobblestone",
      "width": 2.0
    }
  ]
}
```

## Semantic Roles

Every entity can be tagged with a semantic role for bulk operations:

| Role | Description | Example Bulk Operation |
|------|-------------|----------------------|
| `ground` | Terrain, floors | Retexture all ground |
| `structure` | Buildings, walls | Scale all structures 1.2x |
| `prop` | Furniture, objects | Hide all props |
| `vegetation` | Trees, bushes, grass | Recolor all vegetation to autumn |
| `water` | Rivers, lakes, ocean | Adjust all water transparency |
| `sky` | Sky dome, clouds | Change time of day |
| `light` | Light sources | Dim all lights by 50% |
| `effect` | Particles, fog | Disable all effects |

## Scene Decomposition

The pipeline analyzes imported glTF scenes to auto-assign tiers and roles:

- **Mesh segmentation** — splits a single glTF into connected components
- **Role inference** — flat/large → ground, tall/narrow → structure, small → decorative
- **Name matching** — entity names like "tree_01" → vegetation, "lamp_post" → light

This enables semantic operations on imported scenes, not just procedurally generated ones.
