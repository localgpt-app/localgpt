# LocalGPT Gen — TODO Implementation Index

Master index of all TODO specs for LocalGPT Gen. Organized by category with implementation status, dependencies, and recommended build order.

Last updated: 2026-03-17

---

## Status Legend

- **DONE** — Implemented and working
- **PARTIAL** — Some sub-specs done, others remain
- **NOT STARTED** — Spec written, no implementation yet
- **SCAFFOLDED** — Code structure exists, logic not yet functional

---

## 1. Implementation Status Matrix

### P0: World Inspector Panel — DONE

| Spec | Description | Status |
|------|-------------|--------|
| 0.1 | Inspector toggle & layout shell | DONE |
| 0.2 | Entity outliner tree | DONE |
| 0.3 | Detail panel (9 sections) | DONE |
| 0.4 | World info bar | DONE |
| 0.5 | 3D viewport selection & highlight | DONE |
| 0.6 | WebSocket protocol + server | DONE |
| 0.7 | SwiftUI client (iPad/macOS) | DONE |
| 0.8 | Android client (Jetpack Compose) | DONE |

**File:** `TODO-P0-world-inspector-panel.md`

---

### P1: Avatar & Character System — DONE (tools)

All 5 MCP tools are implemented with data structures, command dispatch, and entity spawning.

| Spec | Tool | Status |
|------|------|--------|
| 1.1 | `gen_spawn_player` | DONE |
| 1.2 | `gen_set_spawn_point` | DONE |
| 1.3 | `gen_add_npc` | DONE |
| 1.4 | `gen_set_npc_dialogue` | DONE |
| 1.5 | `gen_set_camera_mode` | DONE |

**File:** `TODO-P1-avatar-character-system.md`

---

### P2: Interaction & Trigger System — DONE (tools)

All 5 MCP tools are implemented.

| Spec | Tool | Status |
|------|------|--------|
| 2.1 | `gen_add_trigger` | DONE |
| 2.2 | `gen_add_teleporter` | DONE |
| 2.3 | `gen_add_collectible` | DONE |
| 2.4 | `gen_add_door` | DONE |
| 2.5 | `gen_link_entities` | DONE |

**File:** `TODO-P2-interaction-trigger-system.md`

---

### P3: Terrain & Landscape — DONE (tools)

All 5 MCP tools are implemented. Bonus: `gen_query_terrain_height` also exists.

| Spec | Tool | Status |
|------|------|--------|
| 3.1 | `gen_add_terrain` | DONE |
| 3.2 | `gen_add_water` | DONE |
| 3.3 | `gen_add_path` | DONE |
| 3.4 | `gen_add_foliage` | DONE |
| 3.5 | `gen_set_sky` | DONE |

**File:** `TODO-P3-terrain-landscape.md`

---

### P4: In-World Text & UI — DONE (tools)

All 5 MCP tools are implemented.

| Spec | Tool | Status |
|------|------|--------|
| 4.1 | `gen_add_sign` | DONE |
| 4.2 | `gen_add_hud` | DONE |
| 4.3 | `gen_add_label` | DONE |
| 4.4 | `gen_add_tooltip` | DONE |
| 4.5 | `gen_add_notification` | DONE |

**File:** `TODO-P4-in-world-text-ui.md`

---

### P5: Physics Integration — DONE (tools)

All 5 MCP tools are implemented.

| Spec | Tool | Status |
|------|------|--------|
| 5.1 | `gen_set_physics` | DONE |
| 5.2 | `gen_add_collider` | DONE |
| 5.3 | `gen_add_joint` | DONE |
| 5.4 | `gen_add_force` | DONE |
| 5.5 | `gen_set_gravity` | DONE |

**File:** `TODO-P5-physics-integration.md`

---

### Runtime System Gaps — DONE

All 24 runtime system gaps have been resolved. MCP tool components now have full Bevy runtime systems.

| Gap ID | Description | Status |
|--------|-------------|--------|
| GAP-P0-01 through P0-04 | P0 Workflow Blockers (4 gaps) | ALL DONE |
| GAP-P1-01 through P1-03 | P1 Avatar Gaps (3 gaps) | ALL DONE |
| GAP-P2-01 through P2-05 | P2 Interaction Gaps (5 gaps) | ALL DONE |
| GAP-P3-01 through P3-02 | P3 Terrain Gaps (2 gaps) | ALL DONE |
| GAP-P4-01 through P4-05 | P4 UI Gaps (5 gaps) | ALL ALREADY DONE |
| GAP-P5-01 through P5-05 | P5 Physics Gaps (5 gaps) | ALL DONE |

**File:** `TODO-GAPS-runtime-systems.md`

---

### WG1: Procedural Blockout Pipeline — DONE

| Spec | Tool | Status |
|------|------|--------|
| WG1.1 | `gen_plan_layout` — text → structured blockout JSON | DONE |
| WG1.2 | `gen_apply_blockout` — blockout → coarse 3D scene | DONE |
| WG1.3 | `gen_populate_region` — fill region with content | DONE |

**File:** `TODO-WG1-procedural-blockout-pipeline.md`

---

### WG2: Navmesh Infrastructure — DONE

| Spec | Tool | Status |
|------|------|--------|
| WG2.1 | `gen_build_navmesh` — generate navmesh from geometry | DONE |
| WG2.2 | `gen_validate_navigability` — traversability check | DONE |
| WG2.3 | Navmesh-constrained placement guard | DONE (via collision_check.rs) |

**File:** `TODO-WG2-navmesh-infrastructure.md`

---

### WG3: Hierarchical Placement — DONE

| Spec | Tool | Status |
|------|------|--------|
| WG3.1 | Entity tier tagging (hero/medium/decorative) + `gen_set_tier` | DONE |
| WG3.2 | Three-pass generation workflow | DONE |
| WG3.3 | Collision-aware placement with ground snap | DONE |

**File:** `TODO-WG3-hierarchical-placement.md`

---

### WG4: Screenshot Evaluation Loop — DONE

| Spec | Tool | Status |
|------|------|--------|
| WG4.1 | `gen_screenshot` highlight mode + camera angles | DONE |
| WG4.2 | `gen_evaluate_scene` — automated quality check | DONE |
| WG4.3 | `gen_auto_refine` — iterative refinement loop | DONE |

**File:** `TODO-WG4-screenshot-evaluation-loop.md`

---

### WG5: Blockout Editing — DONE

| Spec | Tool | Status |
|------|------|--------|
| WG5.1 | `gen_modify_blockout` — edit regions with dirty tracking | DONE |
| WG5.2 | `gen_edit_navmesh` — manual walkable/blocked overrides | DONE |
| WG5.3 | `gen_regenerate` — incremental regeneration with plan/apply | DONE |

**File:** `TODO-WG5-blockout-editing.md`

---

### WG6: Scene Decomposition — DONE

| Spec | Tool | Status |
|------|------|--------|
| WG6.1 | `gen_set_role` + `gen_bulk_modify` — semantic tagging | DONE |
| WG6.2 | Connectivity-ordered generation | DONE |
| WG6.3 | GLTF mesh segmentation | DONE |

**File:** `TODO-WG6-scene-decomposition.md`

---

### WG7: Depth-Conditioned Preview — DONE

| Spec | Tool | Status |
|------|------|--------|
| WG7.1 | `gen_render_depth` — depth map rendering | DONE |
| WG7.2 | `gen_preview_world` — styled 2D preview from depth | REMOVED 2026-11-01 (it called ComfyUI) |

**File:** `TODO-WG7-depth-preview.md`

---

### AI1: Local 3D Asset Generation — REMOVED 2026-11-01

The Rust side was built against a model server that was never written, and went with every other service call in the tree.

| Spec | Tool | Status |
|------|------|--------|
| AI1.1 | `gen_generate_asset` — text/image → 3D mesh | REMOVED |
| AI1.2 | `gen_generate_texture` — PBR texture generation | REMOVED |
| AI1.3 | `gen_generation_status` — generation queue management | REMOVED |

**File:** `../../archived/gen/TODO-AI1-local-asset-generation.md`

---

### AI2: AI-Driven NPC Intelligence — REMOVED 2026-11-01

The Rust side was built over an Ollama client, and went with every other model call in the tree. Patrol, wander and dialogue trees stay as data.

| Spec | Tool | Status |
|------|------|--------|
| AI2.1 | `gen_set_npc_brain` — local SLM-driven NPC behavior | REMOVED |
| AI2.2 | `gen_npc_observe` — visual observation via VLM | REMOVED |
| AI2.3 | `gen_set_npc_memory` — persistent NPC memory | REMOVED |

**File:** `../../archived/gen/TODO-AI2-ai-npc-intelligence.md`

---

## 2. Summary Counts

| Category | Total Specs | Done | Scaffolded | Not Started |
|----------|-------------|------|------------|-------------|
| P0 Inspector | 8 | 8 | 0 | 0 |
| P1 Avatar | 5 | 5 | 0 | 0 |
| P2 Interaction | 5 | 5 | 0 | 0 |
| P3 Terrain | 5 | 5 | 0 | 0 |
| P4 UI | 5 | 5 | 0 | 0 |
| P5 Physics | 5 | 5 | 0 | 0 |
| Runtime Gaps | 24 | 24 | 0 | 0 |
| WG1 Blockout | 3 | 3 | 0 | 0 |
| WG2 Navmesh | 3 | 3 | 0 | 0 |
| WG3 Placement | 3 | 3 | 0 | 0 |
| WG4 Evaluation | 3 | 3 | 0 | 0 |
| WG5 Editing | 3 | 3 | 0 | 0 |
| WG6 Decomposition | 3 | 3 | 0 | 0 |
| WG7 Preview | 2 | 1 | 0 | 1 removed |
| AI1 Asset Gen | 3 | 0 | 0 | 3 removed |
| AI2 NPC AI | 3 | 0 | 0 | 3 removed |
| **TOTAL** | **83** | **76** | **0** | **7 removed** |

---

## 3. Completion Status

Everything that does not call a model or a service is implemented. The seven specs that did — AI1, AI2 and WG7.2 — were scaffolded against backends that never arrived and were removed on 2026-11-01, when LocalGPT decided nothing in its tree runs a model or calls a service (`docs/rfcs/authoring/the-harness-goes.md`).

### Dependency Graph

```
P0 Inspector ─────── DONE
P1 Avatar ─────────── DONE ──→ AI2 NPC Intelligence ── REMOVED 2026-11-01
P2 Interaction ────── DONE
P3 Terrain ────────── DONE ──→ WG1 Blockout ──→ WG2 Navmesh ──→ WG3 Placement ── ALL DONE
P4 UI ─────────────── DONE                                    ──→ WG5 Editing ─── DONE
P5 Physics ────────── DONE                  ──→ WG3 Collision-aware placement ─── DONE
Runtime Gaps ──────── DONE (24/24)

WG4 Evaluation ───── DONE
WG6 Decomposition ── DONE (8-layer ordering, role tagging, mesh segmentation)
WG7 Preview ──────── DONE for the depth render; the styled preview REMOVED 2026-11-01

AI1 Asset Gen ────── REMOVED 2026-11-01
AI2 NPC AI ───────── REMOVED 2026-11-01
```

---

## 4. Next Steps

An agent that wants a generated mesh, a styled preview or a brain behind an NPC runs its own model or service and brings the result in through the tools that stay: `gen_load_gltf` for a mesh, `gen_render_depth` for the depth map an image model conditions on, the world's API for an NPC's moves.

**Not yet specced** (see `NOT-YET-SPECCED.md`):
- Text-to-skeleton animation (HY-Motion) — premature, no humanoid rigs yet
- Procedural world streaming — WG1–WG6 need production hardening first
- Bevy MCP extraction — strategic decision, not a feature spec
