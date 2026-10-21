# P0: World Inspector Panel

A native overlay inside the Gen window that displays 100% of world state — matching everything in `world.ron` — via a hierarchical entity tree, click-to-select, and 3D viewport highlighting. Read-only V1 (inspection only, no editing).

**Dependencies:** Bevy 0.18, `bevy_egui` (egui integration for Bevy)

## Implementation Status

| Spec | Status | Description |
|------|--------|-------------|
| 0.1 | DONE | Inspector toggle & layout shell (F1 key, three modes) |
| 0.2 | DONE | Entity outliner tree (search, expand/collapse, DFS traversal) |
| 0.3 | DONE | Detail panel (9 collapsible sections, color swatches) |
| 0.4 | DONE | World info bar (name, entities, behaviors, audio) |
| 0.5 | DONE | 3D viewport selection (Pointer<Click> observer, parent chain walk, emissive highlight, visibility toggle, Escape to deselect, double-click focus) |
| 0.6 | DONE | WebSocket protocol + server (protocol types, Axum WS on :9877, bidirectional bridge, scene change detection, transform streaming at 10Hz, topic filtering) |
| 0.7 | DONE | SwiftUI client (Swift Package: protocol types, WS client, outliner, detail, world info) |
| 0.8 | DONE | Android client (Kotlin module: protocol types, OkHttp WS client, Compose UI — outliner, detail, inspector screen, world info bar) |

---

## Why egui, not native Bevy UI

The existing Gen crate uses native Bevy UI for **player-facing** elements (HUD, signs, labels, tooltips, notifications). That's correct — game UI should be game-engine native.

An inspector panel is a **developer/creator tool**. It needs:
- Hierarchical tree with expand/collapse (entity outliner)
- Scrollable property grid with labeled rows (detail panel)
- Text selection and copy (debug values)
- Resize handles and dockable sub-panels

Native Bevy UI has none of these built-in. Building them from scratch would be 5-10x the effort of using egui, which provides all of them out of the box. The `bevy_egui` crate is the ecosystem standard for debug overlays — used by `bevy-inspector-egui` (1.5k+ stars) and most production Bevy projects.

The CLI crate already uses egui (eframe) for its desktop GUI, so the dependency is not new to the project.

**Rule of thumb:** Player-facing UI → native Bevy UI. Creator/debug UI → egui via `bevy_egui`.

---

## Spec 0.1: Inspector Toggle & Layout Shell

**Goal:** F1 key cycles through three states: Hidden → Outliner Only (left panel) → Full Inspector (left + right + bottom). The panels render via `bevy_egui` and do not intercept game input when hidden.

### Implementation

1. **Inspector state resource:**
   ```
   InspectorState { mode: InspectorMode }
   enum InspectorMode { Hidden, OutlinerOnly, Full }
   ```
   F1 press cycles: Hidden → OutlinerOnly → Full → Hidden.

2. **Panel layout (Full mode):**
   - **Outliner** — left side panel, 250px default width, resizable. `egui::SidePanel::left`.
   - **Detail** — right side panel, 320px default width, resizable. `egui::SidePanel::right`.
   - **World Info** — bottom panel, 28px fixed height. `egui::TopBottomPanel::bottom`. Shows: world name, entity count, biome, time of day, behavior state (paused/running + elapsed).

3. **OutlinerOnly mode:** Only the left Outliner panel is shown. Detail and World Info panels are hidden.

4. **Camera isolation:** When the egui context reports pointer-over-area (`ctx.is_pointer_over_area()`), insert a `UiHovered` resource that the existing camera input systems check to suppress orbit/pan/zoom. Remove the resource when the pointer leaves egui panels.

5. **Persistence:** Store `InspectorMode` and panel widths in a `InspectorPrefs` resource. Optionally save to a local file so the inspector remembers its state across sessions.

### Acceptance Criteria

- [ ] F1 cycles through Hidden → OutlinerOnly → Full → Hidden
- [ ] Outliner panel renders on the left in both OutlinerOnly and Full modes
- [ ] Detail and World Info panels render only in Full mode
- [ ] Panels are resizable via drag handles
- [ ] Camera orbit/pan/zoom is suppressed when mouse is over any inspector panel
- [ ] Game input (click-to-select, WASD) works normally when mouse is in the 3D viewport
- [ ] Inspector panels do not appear in screenshots or world renders

### Files to Create/Modify

- `localgpt/crates/gen/Cargo.toml` — add `bevy_egui` dependency
- `localgpt/crates/gen/src/inspector/mod.rs` — module root, `InspectorPlugin`, state resource
- `localgpt/crates/gen/src/inspector/layout.rs` — panel layout systems, F1 toggle
- `localgpt/crates/gen/src/gen3d/plugin.rs` — register `InspectorPlugin`

---

## Spec 0.2: Entity Outliner Tree

**Goal:** A scrollable, hierarchical tree of all named entities in the scene. Each node shows an icon (by `GenEntityType`), entity name, and visibility toggle. Clicking a node selects the entity.

### Implementation

1. **Tree data source:** Query `NameRegistry` for all entity-name pairs. For each, read `GenEntity.entity_type` and `Parent`/`Children` components to build a tree. Group orphan (no parent) entities at the root level.

2. **Tree rebuild strategy:** Cache the tree structure. Rebuild only when `NameRegistry` changes (detect via `Changed<NameRegistry>` or a generation counter on the resource). Do NOT rebuild every frame.

3. **Node display format:**
   ```
   [icon] entity_name                [eye]
   ```
   - **Icon by GenEntityType:**
     - `Primitive` → cube icon (■)
     - `Light` → bulb icon (☀)
     - `Camera` → camera icon (📷 or ⊞)
     - `Mesh` → mesh icon (△)
     - `Group` → folder icon (▶/▼ when collapsed/expanded)
     - `AudioEmitter` → speaker icon (♪)
   - **Eye toggle:** Click to toggle `Visibility` on the entity (Visible/Hidden). Dimmed when hidden.

4. **Selection:** Clicking a tree node inserts `InspectorSelection(Entity)` resource. The detail panel (Spec 0.3) reads this resource. Multi-select is out of scope for V1.

5. **Search/filter:** Text input at the top of the outliner. Filters the tree to nodes whose name contains the search string (case-insensitive). Ancestors of matching nodes are shown expanded.

6. **Context info:** Below the tree, show total entity count: `"42 entities"`.

7. **Scroll position:** Preserve scroll position across frames. When an entity is selected via 3D click (Spec 0.5), auto-scroll the tree to reveal and highlight the selected node.

### Acceptance Criteria

- [ ] All named entities appear in the tree
- [ ] Parent-child hierarchy is rendered with indentation
- [ ] Correct icon shown for each GenEntityType variant
- [ ] Clicking a node updates InspectorSelection
- [ ] Selected node is visually highlighted (background color)
- [ ] Eye icon toggles entity visibility
- [ ] Search field filters the tree
- [ ] Tree rebuilds only when NameRegistry changes, not every frame
- [ ] Entity count shown below tree

### Files to Create/Modify

- `localgpt/crates/gen/src/inspector/outliner.rs` — tree building, rendering, search, selection
- `localgpt/crates/gen/src/inspector/mod.rs` — `InspectorSelection` resource

---

## Spec 0.3: Detail Panel — Full Entity Inspection

**Goal:** When an entity is selected, the right panel shows every component and property that `world.ron` would serialize for that entity, organized in collapsible sections. This must achieve 100% coverage of `WorldEntity` fields.

### Implementation

1. **Section layout:** Each section is an egui `CollapsingHeader`, open by default. Sections only appear if the entity has the relevant component.

2. **Sections and their data sources:**

   **a) Identity**
   - Name: `NameRegistry::get_name(entity)`
   - Entity ID: `GenEntity.world_id`
   - Type: `GenEntity.entity_type` (displayed as string)
   - Creation ID: `WorldEntity.creation_id` (if part of a creation)

   **b) Transform** — from `Transform` component
   - Position: `[x, y, z]` with 3 decimal places
   - Rotation: Euler degrees `[rx, ry, rz]` converted from quaternion
   - Scale: `[sx, sy, sz]`
   - Visible: `Visibility` component state

   **c) Shape** — from `ParametricShape` component
   - Shape variant name (Cuboid, Sphere, Cylinder, etc.)
   - Shape dimensions (width/height/depth for Cuboid, radius for Sphere, etc.)
   - Display the `wt::Shape` enum variant with all fields

   **d) Material** — from `StandardMaterial` handle (resolve via `Assets<StandardMaterial>`)
   - Base color: RGBA with color swatch
   - Metallic: f32
   - Roughness: f32
   - Reflectance: f32
   - Emissive: RGBA with color swatch
   - Alpha mode: Opaque/Blend/Mask
   - Double-sided: bool
   - Unlit: bool

   **e) Light** — from `PointLight`, `DirectionalLight`, or `SpotLight` component
   - Light type
   - Color, intensity, range/radius
   - Shadows enabled
   - For SpotLight: inner/outer angle

   **f) Behaviors** — from `EntityBehaviors` component
   - List each `BehaviorInstance`:
     - Behavior type (Orbit, Spin, Bob, LookAt, Pulse, PathFollow, Bounce)
     - Behavior ID
     - All parameters from `BehaviorDef` (speed, axis, radius, amplitude, etc.)
     - Base position and base scale (frozen anchors — flag if stale per GAP-P0-02)

   **g) Audio** — from `AudioEngine` resource, keyed by entity name
   - Sound type, sound parameters
   - Volume, radius
   - Attached entity
   - Position (if positional)

   **h) Mesh Asset** — from `MeshAssetRef` or `Handle<Scene>`
   - Asset path (glTF file reference)

   **i) Hierarchy**
   - Parent: name (clickable → selects parent)
   - Children: list of names (each clickable → selects child)

   **j) Chunk** — from chunk assignment
   - `ChunkCoord` if present

3. **Refresh strategy:** Re-read component data when:
   - Selection changes (immediate full rebuild)
   - Periodic timer (every 200ms) to catch live changes (e.g., behavior animation updating transform)
   - `Changed<Transform>` detected on selected entity (immediate refresh of transform section)

4. **Copy button:** Each section header has a small copy icon that copies all section data to clipboard as formatted text (for pasting into LLM chat context).

### Acceptance Criteria

- [ ] All WorldEntity fields are represented in the detail panel
- [ ] Transform shows position, rotation (degrees), scale, visibility
- [ ] ParametricShape shows variant name and all dimension fields
- [ ] StandardMaterial shows all PBR properties with color swatches
- [ ] Light section shows type-specific properties
- [ ] Behaviors section lists all attached behaviors with full parameters
- [ ] Audio section shows emitter metadata from AudioEngine
- [ ] Mesh asset path is shown for imported glTF entities
- [ ] Parent/children names are clickable for navigation
- [ ] Sections collapse/expand independently
- [ ] Data refreshes on selection change and periodically
- [ ] Copy button copies section data to clipboard

### Files to Create/Modify

- `localgpt/crates/gen/src/inspector/detail.rs` — section rendering, component queries
- `localgpt/crates/gen/src/inspector/detail_sections.rs` — individual section implementations (transform, material, shape, light, behaviors, audio)

---

## Spec 0.4: World Info Bar

**Goal:** A compact bottom bar showing global world state — the fields from `WorldManifest` that are not per-entity.

### Implementation

1. **Bar content (left to right):**
   ```
   world_name | 42 entities | biome: forest | time: 14:30 | env: amb 0.3 fog 0.02 | behaviors: running 12.4s | audio: ON 3 emitters
   ```

2. **Data sources:**
   - World name: `WorldMeta.name` (from loaded manifest or a `WorldName` resource)
   - Entity count: `NameRegistry.len()`
   - Biome: `WorldMeta.biome`
   - Time of day: `WorldMeta.time_of_day` (formatted as HH:MM, where 0.0=midnight, 0.5=noon)
   - Environment: `EnvironmentDef` — ambient intensity, fog density (from resources or manifest)
   - Behavior state: `BehaviorState.paused` (running/paused), `BehaviorState.elapsed` (formatted as seconds with 1 decimal)
   - Audio: `AudioEngine.active` (ON/OFF), count of `AudioEngine.emitter_meta` entries
   - Camera: current `CameraDef` position (truncated to integers)

3. **Compact display:** Single line, pipe-separated segments. Each segment is a `egui::Label` with a subtle separator. Truncate world name if too long.

4. **Click interaction:** Clicking a segment could expand a tooltip with full details (e.g., clicking "env" shows all EnvironmentDef fields). V1: tooltips only, no editing.

### Acceptance Criteria

- [ ] Bar appears at bottom of screen in Full mode
- [ ] World name, entity count, biome, and time displayed
- [ ] Environment summary shows ambient intensity and fog density
- [ ] Behavior state shows running/paused and elapsed time
- [ ] Audio state shows active/inactive and emitter count
- [ ] Bar updates reactively when world state changes
- [ ] Tooltip on hover shows expanded details for each segment

### Files to Create/Modify

- `localgpt/crates/gen/src/inspector/world_info.rs` — bottom bar rendering, data queries

---

## Spec 0.5: 3D Viewport Selection & Highlight

**Goal:** Click an entity in the 3D viewport to select it (same as clicking in the outliner). The selected entity gets a visual highlight (blue emissive glow outline).

### Implementation

1. **Raycast selection:** On mouse click in the 3D viewport (not over any egui panel):
   - Cast a ray from camera through the cursor position
   - Hit-test against all entities with `GenEntity` component (use mesh AABBs or Bevy's built-in picking/raycast)
   - If hit, set `InspectorSelection(hit_entity)`
   - If no hit, clear selection

2. **Selection highlight — emissive override:**
   - When `InspectorSelection` changes, on the newly selected entity:
     - Store the original emissive color in `OriginalEmissive(Color)` component
     - Set `StandardMaterial.emissive` to a blue glow: `Color::srgba(0.2, 0.4, 1.0, 1.0)` with emissive intensity ~2.0
   - When deselected (selection changes or cleared):
     - Restore `StandardMaterial.emissive` from `OriginalEmissive`
     - Remove `OriginalEmissive` component
   - For entities without `StandardMaterial` (lights, groups, audio emitters): spawn a temporary wireframe bounding box or small axis gizmo at the entity's position instead

3. **Outline alternative (future):** If Bevy 0.18 ships with outline/stencil support, prefer an outline post-process over emissive override. Emissive override is the pragmatic V1 approach.

4. **Guard against inspector-hidden state:** When inspector is Hidden, disable click-to-select and clear any active highlight. Selection highlight should only be visible when the inspector is active.

5. **Double-click to focus:** Double-clicking an entity in the 3D viewport smoothly animates the orbit camera to center on that entity at a comfortable viewing distance (2× entity bounding sphere radius, minimum 3m).

### Acceptance Criteria

- [ ] Left-click in viewport selects the clicked entity
- [ ] Selected entity shows blue emissive glow
- [ ] Previous selection's material is fully restored on deselect
- [ ] Clicking empty space clears selection
- [ ] Outliner tree highlights sync with 3D viewport selection
- [ ] Non-mesh entities (lights, groups) show a wireframe/gizmo indicator
- [ ] Selection is disabled when inspector is Hidden
- [ ] Double-click focuses camera on entity

### Files to Create/Modify

- `localgpt/crates/gen/src/inspector/selection.rs` — raycast picking, highlight system, OriginalEmissive component
- `localgpt/crates/gen/src/inspector/mod.rs` — InspectorSelection resource (shared with outliner)

---

## WorldManifest Coverage Checklist

Every field serialized to `world.ron` must be inspectable in the panel. Cross-reference with `localgpt_world_types::WorldManifest`:

| WorldManifest Field | Inspector Location | Source |
|---|---|---|
| `version` | World Info bar tooltip | WorldManifest |
| `meta.name` | World Info bar | WorldMeta |
| `meta.description` | World Info bar tooltip | WorldMeta |
| `meta.biome` | World Info bar | WorldMeta |
| `meta.time_of_day` | World Info bar | WorldMeta |
| `environment.background_color` | World Info bar tooltip | EnvironmentDef |
| `environment.ambient_intensity` | World Info bar | EnvironmentDef |
| `environment.ambient_color` | World Info bar tooltip | EnvironmentDef |
| `environment.fog_density` | World Info bar | EnvironmentDef |
| `environment.fog_color` | World Info bar tooltip | EnvironmentDef |
| `camera.position` | World Info bar | CameraDef |
| `camera.look_at` | World Info bar tooltip | CameraDef |
| `camera.fov_degrees` | World Info bar tooltip | CameraDef |
| `avatar` | World Info bar tooltip | AvatarDef |
| `tours` | World Info bar tooltip (count) | Vec\<TourDef\> |
| `next_entity_id` | World Info bar tooltip | WorldManifest |
| `entities[].id` | Detail: Identity | GenEntity.world_id |
| `entities[].name` | Detail: Identity / Outliner | NameRegistry |
| `entities[].transform` | Detail: Transform | Transform component |
| `entities[].parent` | Detail: Hierarchy | Parent component |
| `entities[].chunk` | Detail: Chunk | ChunkCoord |
| `entities[].creation_id` | Detail: Identity | CreationId |
| `entities[].shape` | Detail: Shape | ParametricShape |
| `entities[].material` | Detail: Material | StandardMaterial |
| `entities[].light` | Detail: Light | PointLight/DirectionalLight/SpotLight |
| `entities[].behaviors` | Detail: Behaviors | EntityBehaviors |
| `entities[].audio` | Detail: Audio | AudioEngine emitter_meta |
| `entities[].mesh_asset` | Detail: Mesh Asset | MeshAssetRef |
| `creations` | World Info bar tooltip (count) | Vec\<CreationDef\> |

**Coverage: 100% of WorldManifest fields.**

---

## GenEntityType Icon Coverage

| Variant | Icon | Outliner | Detail |
|---|---|---|---|
| `Primitive` | ■ (cube) | Yes | Shape + Material sections |
| `Light` | ☀ (sun) | Yes | Light section |
| `Camera` | ⊞ (viewfinder) | Yes | Camera properties |
| `Mesh` | △ (triangle) | Yes | Mesh Asset section |
| `Group` | ▶/▼ (folder) | Yes | Children list in Hierarchy |
| `AudioEmitter` | ♪ (note) | Yes | Audio section |

**Coverage: 100% of GenEntityType variants.**

---

## Cross-Reference with Existing Tools

The inspector panel displays the same data as `gen_scene_info` and `gen_entity_info` MCP tools, but in a persistent visual overlay instead of one-shot text responses:

| MCP Tool Field | Inspector Equivalent |
|---|---|
| `SceneInfoData.entity_count` | Outliner footer + World Info bar |
| `SceneInfoData.entities[]` | Outliner tree (all entries) |
| `EntityInfoData.name` | Detail: Identity |
| `EntityInfoData.entity_id` | Detail: Identity |
| `EntityInfoData.entity_type` | Detail: Identity + Outliner icon |
| `EntityInfoData.shape` | Detail: Shape |
| `EntityInfoData.position/rotation/scale` | Detail: Transform |
| `EntityInfoData.color/metallic/roughness/emissive/...` | Detail: Material |
| `EntityInfoData.visible` | Detail: Transform + Outliner eye icon |
| `EntityInfoData.light` | Detail: Light |
| `EntityInfoData.children/parent` | Detail: Hierarchy |
| `EntityInfoData.mesh_asset` | Detail: Mesh Asset |
| `EntityInfoData.audio` | Detail: Audio |
| `EntityInfoData.behaviors` | Detail: Behaviors |
| `AudioInfoResponse.active/emitters` | World Info bar |
| `BehaviorListResponse` | Detail: Behaviors (per-entity) |

**Net effect:** With the inspector panel open, `gen_scene_info` and `gen_entity_info` calls become largely unnecessary during interactive world-building sessions — the AI agent can reference the visual state directly or the creator can relay what they see.

---

## Performance Notes

- **Tree rebuild:** Only on `NameRegistry` change (entity add/remove/rename), not every frame. Store a `generation: u64` counter on NameRegistry, compare in the outliner system.
- **Detail panel:** Full re-query on selection change. Transform section updates every frame only if `Changed<Transform>` fires on the selected entity. Other sections refresh on a 200ms timer.
- **egui rendering:** `bevy_egui` renders to a texture overlay — zero impact on the 3D scene's draw calls or batching.
- **Selection highlight:** Single material property change (emissive). Negligible cost.
- **Memory:** The cached tree is a `Vec<TreeNode>` with name + entity + type + depth. For 1000 entities, ~50KB. Trivial.

---

## Summary

| Spec | What | Key Dependency |
|---|---|---|
| 0.1 | Toggle + layout shell | `bevy_egui` crate |
| 0.2 | Entity outliner tree | NameRegistry, GenEntity, Parent/Children |
| 0.3 | Detail panel (all components) | All Gen components + StandardMaterial + Lights |
| 0.4 | World info bottom bar | WorldMeta, EnvironmentDef, BehaviorState, AudioEngine |
| 0.5 | 3D click selection + highlight | Raycast, StandardMaterial emissive override |
| 0.6 | World Inspector Protocol (WebSocket) | Axum WebSocket, serde JSON, Bonjour/mDNS |
| 0.7 | SwiftUI inspector (iPad/macOS) | SceneKit, URLSessionWebSocketTask, SF Symbols |
| 0.8 | Jetpack Compose inspector (Android) | Filament/SceneView, OkHttp, Material Design 3 |

**Recommended build order:** 0.1 → 0.2 → 0.5 → 0.3 → 0.4 → 0.6 → 0.7 → 0.8

Phase 1 (Bevy desktop): 0.1 → 0.2 → 0.5 → 0.3 → 0.4 — egui inspector fully working in Gen window.
Phase 2 (Protocol): 0.6 — WebSocket protocol enabling remote inspection from any client.
Phase 3 (Native clients): 0.7 + 0.8 in parallel — SwiftUI and Jetpack Compose inspector apps consuming the protocol, each with native 3D viewport and audio.

---

## Cross-Platform Architecture

The inspector has three tiers:

```
┌─────────────────────────────────────────────────────────┐
│                    DATA LAYER (Rust)                     │
│  WorldManifest, NameRegistry, GenEntity, Components     │
│  ── single source of truth, lives in Bevy ECS ──       │
└──────────┬──────────────────────────┬───────────────────┘
           │ direct ECS query         │ WorldInspectorProtocol
           │ (in-process)             │ (JSON over WebSocket)
           ▼                          ▼
┌──────────────────┐    ┌──────────────────────────────────┐
│  BEVY INSPECTOR  │    │       REMOTE CLIENTS             │
│  (egui, Spec 0.1 │    │  ┌────────────┐ ┌─────────────┐ │
│   through 0.5)   │    │  │  SwiftUI   │ │  Jetpack    │ │
│                  │    │  │  iPad/Mac  │ │  Compose    │ │
│  Desktop only.   │    │  │  SceneKit  │ │  Filament   │ │
│  Full fidelity.  │    │  │  (Spec 0.7)│ │  (Spec 0.8) │ │
│                  │    │  └────────────┘ └─────────────┘ │
└──────────────────┘    └──────────────────────────────────┘
```

**Key principle:** The Bevy process is always the authority. Native clients are read-only viewers that connect over the local network. The protocol (Spec 0.6) is a thin JSON layer over the same data that `gen_scene_info` and `gen_entity_info` already produce.

**Why native apps, not just egui everywhere?**

- iPad has no egui — Bevy/egui can't run natively on iOS with full App Store compliance. SwiftUI + SceneKit/RealityKit is the only production path.
- Android similarly needs Jetpack Compose + Filament/SceneView for native 3D.
- Native apps get platform-standard gestures (pinch-zoom, swipe, haptics), accessibility (VoiceOver, TalkBack), and multitasking (Split View on iPad, freeform on Android).
- A connected 3D app in this ecosystem already has mature Swift and Kotlin clients with the same pattern: Rust data authority → native UI clients via WebSocket. The inspector follows the same architecture.

---

## Spec 0.6: World Inspector Protocol (WIP)

**Goal:** A WebSocket JSON protocol that exposes the full inspector data model to remote clients. The Bevy process hosts the WebSocket server; native clients connect as subscribers.

### Protocol Design

1. **Transport:** WebSocket on `ws://localhost:9877/inspector` (configurable port). Launched when Gen mode starts, gated behind `--inspector-server` flag or always-on in debug builds.

2. **Message format:** JSON envelopes with `type` field for routing.

   **Client → Server messages:**

   ```json
   { "type": "subscribe", "topics": ["scene", "selection", "world_info"] }
   { "type": "select_entity", "entity_id": 42 }
   { "type": "deselect" }
   { "type": "toggle_visibility", "entity_id": 42 }
   { "type": "request_entity_detail", "entity_id": 42 }
   { "type": "request_scene_tree" }
   { "type": "request_world_info" }
   { "type": "focus_entity", "entity_id": 42 }
   ```

   **Server → Client messages:**

   ```json
   { "type": "scene_tree", "entities": [
       { "id": 1, "name": "ground", "entity_type": "Primitive", "parent_id": null,
         "visible": true, "children": [2, 3] },
       { "id": 2, "name": "tree_01", "entity_type": "Group", "parent_id": 1,
         "visible": true, "children": [4, 5] }
   ]}

   { "type": "entity_detail", "entity_id": 42, "data": {
       "identity": { "name": "fountain", "id": 42, "entity_type": "Primitive", "creation_id": null },
       "transform": { "position": [5.0, 0.0, -3.0], "rotation_degrees": [0, 45, 0], "scale": [1, 1, 1], "visible": true },
       "shape": { "variant": "Cylinder", "radius": 0.5, "height": 1.2 },
       "material": { "base_color": [0.3, 0.5, 0.8, 1.0], "metallic": 0.1, "roughness": 0.7, "emissive": [0, 0, 0, 1], "alpha_mode": "Opaque", "double_sided": false, "unlit": false, "reflectance": 0.5 },
       "light": null,
       "behaviors": [{ "id": "bhv_1", "type": "Spin", "speed": 1.0, "axis": [0, 1, 0] }],
       "audio": { "sound_type": "water_flow", "volume": 0.8, "radius": 5.0, "position": [5.0, 0.5, -3.0] },
       "mesh_asset": null,
       "hierarchy": { "parent": null, "children": ["water_jet", "basin"] },
       "chunk": null
   }}

   { "type": "world_info", "data": {
       "version": 1,
       "name": "Medieval Village",
       "description": "A quiet village...",
       "biome": "forest",
       "time_of_day": 0.6,
       "entity_count": 42,
       "environment": { "background_color": [0.5, 0.7, 1.0, 1.0], "ambient_intensity": 0.3, "ambient_color": [1, 1, 1, 1], "fog_density": 0.02, "fog_color": [0.8, 0.8, 0.9, 1] },
       "camera": { "position": [10, 8, 15], "look_at": [0, 0, 0], "fov_degrees": 60 },
       "behavior_state": { "paused": false, "elapsed": 12.4 },
       "audio": { "active": true, "emitter_count": 3, "ambience_layers": ["wind", "birds"] },
       "tour_count": 1,
       "creation_count": 5
   }}

   { "type": "selection_changed", "entity_id": 42 }
   { "type": "selection_cleared" }
   { "type": "scene_changed" }
   { "type": "entity_transform_updated", "entity_id": 42, "position": [5.0, 0.1, -3.0], "rotation_degrees": [0, 46, 0] }
   ```

3. **Push updates:** The server pushes incremental updates when state changes:
   - `scene_changed` — entity added/removed/renamed (client should re-request `scene_tree`)
   - `entity_transform_updated` — throttled to 10 Hz for the selected entity's live transform (behavior animations)
   - `selection_changed` / `selection_cleared` — selection sync between Bevy inspector and remote clients
   - Full `entity_detail` is only sent on explicit request, not pushed continuously

4. **glTF scene snapshot:** For the native 3D viewports (Spec 0.7/0.8), the protocol includes a binary message type:
   ```json
   { "type": "request_scene_snapshot" }
   ```
   Server responds with a binary WebSocket frame containing the full scene as glTF/GLB (reusing the existing `gltf_export` pipeline). The native client loads this into SceneKit/Filament for 3D rendering. Scene snapshots are re-requested on `scene_changed` events.

5. **Concurrency:** Multiple clients can connect simultaneously. Selection state is shared — if a client selects an entity, all clients (including the Bevy egui inspector) see the selection change. Last-write-wins for selection.

6. **Reuse existing data structures:** The JSON payloads are serializations of the same `SceneInfoData`, `EntityInfoData`, `AudioInfoResponse`, and `WorldManifest` structs that the MCP tools already produce. Add `#[derive(serde::Serialize)]` to these types (most already have it) and route through the WebSocket.

### Implementation

1. **WebSocket server:** Add an Axum WebSocket endpoint to the existing LocalGPT server infrastructure (`localgpt/crates/server/`). In Gen mode, start the inspector WebSocket alongside the Bevy app using a shared `Arc<Mutex<InspectorBridge>>` or channel-based bridge.

2. **Bevy ↔ WebSocket bridge:** A Bevy system reads `InspectorBridge` commands (select, deselect, toggle_visibility) from a channel and applies them to ECS. Another system detects ECS changes and pushes updates to the WebSocket broadcast channel.

3. **Authentication:** V1: none (localhost only). Future: optional token-based auth for remote network inspection.

### Acceptance Criteria

- [ ] WebSocket server starts on Gen mode launch
- [ ] Client can subscribe and receive `scene_tree` on connect
- [ ] `request_entity_detail` returns full component data matching gen_entity_info output
- [ ] `request_world_info` returns all WorldManifest global fields
- [ ] `select_entity` from remote client updates InspectorSelection in Bevy (and vice versa)
- [ ] `scene_changed` is broadcast when entities are added/removed
- [ ] Transform updates for selected entity are pushed at ≤10 Hz
- [ ] GLB scene snapshot can be requested and received as binary frame
- [ ] Multiple simultaneous clients work correctly

### Files to Create/Modify

- `localgpt/crates/gen/src/inspector/protocol.rs` — message types, JSON serialization
- `localgpt/crates/gen/src/inspector/server.rs` — WebSocket server, client management, broadcast
- `localgpt/crates/gen/src/inspector/bridge.rs` — Bevy ECS ↔ WebSocket channel bridge
- `localgpt/crates/server/src/http.rs` — mount `/inspector` WebSocket route

---

## Spec 0.7: SwiftUI Inspector (iPad / macOS)

**Goal:** A native Swift app for iPad and macOS that connects to the Gen process via the World Inspector Protocol (Spec 0.6) and provides a platform-native inspector experience with a 3D scene viewport.

### Why Native Swift

- **iPad is a first-class creative surface.** Creators should be able to inspect and navigate their world on an iPad while the Gen process runs on a Mac (or the same device via localhost).
- **Apple HIG compliance.** SwiftUI provides standard navigation patterns (sidebar, detail, toolbar), accessibility (VoiceOver, Dynamic Type), and multitasking (Split View, Stage Manager) for free.
- **SceneKit / RealityKit** for 3D viewport. Load the GLB scene snapshot, render with native Metal pipeline. No Bevy dependency on iOS.
- **Precedent in the ecosystem.** A connected 3D app's iOS client already has a mature Swift codebase with a WebSocket data client, SceneKit rendering, and SwiftUI control panels. This inspector follows the same pattern.

### Implementation

1. **App structure (SwiftUI):**
   ```
   NavigationSplitView {
       Sidebar: EntityOutlinerView     // Spec 0.2 equivalent
       Detail: EntityDetailView         // Spec 0.3 equivalent
       Toolbar: WorldInfoBar            // Spec 0.4 equivalent
   }
   ```
   - iPad: sidebar + detail in split view. Toolbar at top.
   - macOS: same layout via Catalyst or native AppKit/SwiftUI.
   - Adapt to compact width (iPhone): tab-based navigation with Outliner and Detail as separate tabs.

2. **WebSocket client:**
   - Use `URLSessionWebSocketTask` (Foundation, no third-party dependency).
   - Connect to `ws://<host>:9877/inspector` (host configurable, defaults to localhost).
   - Bonjour/mDNS discovery: the Gen process advertises `_world-inspector._tcp` so the Swift app can auto-discover it on the local network without manual IP entry.
   - Reconnect with exponential backoff on disconnect.

3. **Entity outliner (sidebar):**
   - SwiftUI `List` with `DisclosureGroup` for hierarchy (expand/collapse).
   - `SF Symbols` for entity type icons:
     - `Primitive` → `cube.fill`
     - `Light` → `sun.max.fill`
     - `Camera` → `camera.fill`
     - `Mesh` → `square.stack.3d.up.fill`
     - `Group` → `folder.fill`
     - `AudioEmitter` → `speaker.wave.2.fill`
   - Search via SwiftUI `.searchable()` modifier.
   - Eye toggle via `Image(systemName: "eye")` / `"eye.slash"`.

4. **Entity detail (inspector):**
   - SwiftUI `Form` with `Section` for each component group.
   - Transform: three `LabeledContent` rows for position/rotation/scale.
   - Material: `ColorPicker` (read-only swatch, no editing in V1) for base color and emissive.
   - Behaviors: `ForEach` listing behavior instances with `DisclosureGroup` for parameters.
   - All data populated from `entity_detail` WebSocket message.

5. **3D viewport (SceneKit):**
   - `SceneView` (SwiftUI) wrapping a `SCNScene`.
   - On connect and on `scene_changed`, request GLB snapshot via the protocol.
   - Load GLB into SceneKit using `SCNScene(named:)` or `ModelIO` → `SCNScene` pipeline.
   - **Selection:** Tap gesture → `SCNHitTestResult` → find entity by name → send `select_entity` over WebSocket.
   - **Selection highlight:** Apply `SCNMaterial.emission.contents = NSColor.systemBlue` to selected node (same emissive glow concept as Spec 0.5).
   - **Camera sync:** Receive camera position from `world_info` and optionally sync SceneKit camera to match the Bevy camera. User can also orbit independently on iPad with standard SceneKit gestures (pinch-zoom, two-finger rotate, pan).
   - **Fallback for complex scenes:** If GLB exceeds a size threshold (e.g., 50MB), show outliner + detail only, no 3D viewport. Display a message: "Scene too large for preview."

6. **Audio preview:**
   - Display audio emitter metadata (sound type, volume, radius) in the detail panel.
   - Spatial audio preview is out of scope for V1 — the native app shows audio data but does not play back the synthesized audio (FunDSP runs in Bevy, not on iOS).
   - Future: stream audio from the Gen process to the native app via a separate audio WebSocket channel or AirPlay.

7. **Platform-specific features:**
   - **iPad:** Pencil support for tap-to-select in 3D viewport. Split View with other apps.
   - **macOS:** Menu bar integration (File → Connect, Edit → Copy Entity Data). Keyboard shortcuts matching Bevy inspector (F1 toggle, arrow key navigation in outliner).
   - **Shared:** Handoff — start inspecting on Mac, continue on iPad (via shared WebSocket connection state).

### Acceptance Criteria

- [ ] App connects to Gen process via WebSocket and displays scene tree
- [ ] Entity outliner shows hierarchy with correct SF Symbol icons
- [ ] Selecting an entity in outliner sends `select_entity` and shows detail panel
- [ ] Detail panel shows all component sections (transform, shape, material, light, behaviors, audio)
- [ ] 3D viewport renders GLB scene snapshot via SceneKit
- [ ] Tap in 3D viewport selects the tapped entity
- [ ] Selected entity highlighted with emissive glow in SceneKit
- [ ] Search filters the entity tree
- [ ] Auto-discovers Gen process via Bonjour on local network
- [ ] Reconnects automatically on disconnect
- [ ] Runs on iPad (iOS 17+) and macOS (14+)

### Files to Create/Modify

- `apps/ios/WorldInspector/` (in the inspector's own repository) — new Xcode project or target within an existing iOS app
- `WorldInspectorApp.swift` — app entry point, connection setup
- `InspectorWebSocket.swift` — WebSocket client, message parsing, reconnect logic
- `EntityOutlinerView.swift` — sidebar tree
- `EntityDetailView.swift` — component sections
- `WorldInfoBar.swift` — toolbar summary
- `ScenePreviewView.swift` — SceneKit 3D viewport with GLB loading
- `BonjourDiscovery.swift` — mDNS service browser

---

## Spec 0.8: Jetpack Compose Inspector (Android)

**Goal:** A native Android app that connects to the Gen process via the World Inspector Protocol (Spec 0.6) and provides a Material Design 3 inspector experience with a native 3D scene viewport and spatial audio preview.

### Why Native Android

- **Android tablets and Chromebooks** are viable creative surfaces. Same rationale as iPad — inspect the world on a secondary device while Gen runs on desktop.
- **Material Design 3 + Jetpack Compose** provides standard navigation (NavigationRail, adaptive layouts), accessibility (TalkBack, content descriptions), and large-screen support (foldables, tablets, Chromebooks) for free.
- **Filament** (Google's physically-based rendering engine) for 3D viewport. Android-native, Metal-grade quality, loads glTF/GLB directly. Used by Google's Scene Viewer and AR apps.
- **Oboe / OpenSL ES** for audio. Android's native low-latency audio APIs for spatial audio preview.
- **Precedent in the ecosystem.** A connected 3D app's Android client already has a mature Kotlin codebase with a WebSocket data client, native 3D rendering, and Compose UI panels.

### Implementation

1. **App structure (Jetpack Compose):**
   ```
   AdaptiveLayout {
       NavigationRail / Drawer: EntityOutlinerScreen
       Content: EntityDetailScreen or SceneViewport
       BottomBar: WorldInfoBar
   }
   ```
   - Tablet/Chromebook: side-by-side outliner + detail with 3D viewport as a third pane (or toggle).
   - Phone (compact): bottom navigation with Outliner, 3D View, and Detail as separate destinations.
   - Foldable: table-top mode puts 3D viewport on top half, inspector on bottom half.

2. **WebSocket client:**
   - Use OkHttp `WebSocket` (already a standard Android dependency, or Ktor if preferred for KMP).
   - Connect to `ws://<host>:9877/inspector`.
   - **Network Service Discovery (NSD):** Android's `NsdManager` for mDNS discovery of `_world-inspector._tcp` service on local network. Same protocol as Bonjour (they are interoperable).
   - Reconnect with exponential backoff. Show connection status in top bar.

3. **Entity outliner:**
   - Compose `LazyColumn` with expandable items (`AnimatedVisibility` for children).
   - Material Icons for entity types:
     - `Primitive` → `Icons.Default.ViewInAr` (3D cube)
     - `Light` → `Icons.Default.LightMode`
     - `Camera` → `Icons.Default.CameraAlt`
     - `Mesh` → `Icons.Default.Category`
     - `Group` → `Icons.Default.Folder`
     - `AudioEmitter` → `Icons.Default.VolumeUp`
   - Search via `SearchBar` composable (Material 3).
   - Visibility toggle via `Icon(Icons.Default.Visibility)` / `VisibilityOff`.

4. **Entity detail:**
   - Compose `LazyColumn` with `Card` sections for each component group.
   - Transform: `ListItem` rows with position/rotation/scale values.
   - Material: custom `ColorSwatch` composable showing base color and emissive as small filled circles.
   - Behaviors: `ElevatedCard` per behavior with type, ID, and expandable parameter list.
   - Copy button per section: copies JSON to clipboard via `ClipboardManager`.

5. **3D viewport (Filament / SceneView):**
   - Use `io.github.sceneview:sceneview` (SceneView for Android) which wraps Filament with Compose support.
   - On connect and on `scene_changed`, request GLB snapshot via the protocol.
   - Load GLB: `ModelNode.loadModel(glbByteBuffer)` — Filament loads glTF/GLB natively with full PBR material support.
   - **Selection:** Touch gesture → Filament ray pick (`View.pick()`) → find entity by name → send `select_entity` over WebSocket.
   - **Selection highlight:** Modify the selected entity's `MaterialInstance` emissive factor to blue glow. Restore on deselect.
   - **Camera:** Standard SceneView orbit controls (single-finger rotate, pinch-zoom, two-finger pan). Optionally sync with Bevy camera position from `world_info`.
   - **Performance:** Filament handles PBR, IBL, shadows natively on GPU. GLB scenes up to ~100MB render smoothly on modern Android devices (Snapdragon 8 Gen 2+, Mali-G720+).

6. **Audio preview (Oboe):**
   - **V1:** Display audio emitter metadata in detail panel (same as Swift — data only, no playback).
   - **V2 (future):** For basic spatial audio preview:
     - Use Android's `Spatializer` API (Android 12+) or Oboe with head-tracking for a simplified preview.
     - The Gen process could stream a mixed audio snapshot as Opus/AAC over a secondary WebSocket channel.
     - Show audio emitter positions as icons in the Filament viewport (speaker glyph billboards).
   - **V2 alternative:** Use Android's built-in `SoundPool` or `MediaPlayer` with `AudioAttributes.USAGE_GAME` to play placeholder audio assets that approximate the emitter's sound type (e.g., a generic water sound for `water_flow` type). This gives directional audio cues without streaming from the Gen process.

7. **Platform-specific features:**
   - **Tablets:** Multi-window support (split-screen with other apps). Keyboard/mouse support for Chromebooks (arrow keys for outliner navigation, mouse hover for tooltips).
   - **Foldables:** Detect `FoldingFeature` via Jetpack WindowManager and adapt layout (table-top: 3D on top, inspector on bottom).
   - **Android Auto (stretch goal):** Not applicable, but Android Automotive OS tablets could be a surface.
   - **Widgets:** Home screen widget showing world name + entity count for quick status (Glance composable).

### Acceptance Criteria

- [ ] App connects to Gen process via WebSocket and displays scene tree
- [ ] Entity outliner shows hierarchy with correct Material Design icons
- [ ] Selecting an entity sends `select_entity` and shows detail panel
- [ ] Detail panel shows all component sections with Material 3 styling
- [ ] 3D viewport renders GLB scene snapshot via Filament/SceneView
- [ ] Touch in 3D viewport selects the tapped entity
- [ ] Selected entity highlighted with emissive glow in Filament
- [ ] Search filters the entity tree
- [ ] Auto-discovers Gen process via NSD/mDNS on local network
- [ ] Reconnects automatically on disconnect
- [ ] Adaptive layout works on phone, tablet, foldable, and Chromebook
- [ ] Runs on Android API 28+ (Android 9+)

### Files to Create/Modify

- `apps/android/app/src/main/java/<app package>/inspector/` (in the inspector's own repository) — new package
- `InspectorActivity.kt` — entry point, adaptive layout scaffold
- `InspectorWebSocket.kt` — OkHttp WebSocket client, message parsing, reconnect
- `EntityOutlinerScreen.kt` — Compose tree with expand/collapse
- `EntityDetailScreen.kt` — component section cards
- `WorldInfoBar.kt` — bottom bar composable
- `SceneViewport.kt` — Filament/SceneView integration, GLB loading, ray pick selection
- `NsdDiscovery.kt` — mDNS service discovery
- `build.gradle.kts` — add SceneView, OkHttp dependencies

---

## Cross-Platform Feature Parity Matrix

| Feature | Bevy (egui) | iPad/Mac (SwiftUI) | Android (Compose) |
|---|---|---|---|
| Entity outliner tree | egui tree | `DisclosureGroup` | `LazyColumn` + expand |
| Entity detail panel | egui `CollapsingHeader` | SwiftUI `Form`/`Section` | Compose `Card` sections |
| World info bar | egui `TopBottomPanel` | SwiftUI `ToolbarItem` | Compose `BottomAppBar` |
| 3D viewport | Bevy renderer (native) | SceneKit (GLB import) | Filament (GLB import) |
| Selection highlight | Emissive override (ECS) | `SCNMaterial.emission` | `MaterialInstance` emissive |
| Click/tap to select | Bevy raycast | `SCNHitTestResult` | Filament `View.pick()` |
| Search | egui text input | `.searchable()` | `SearchBar` |
| Visibility toggle | Direct ECS mutation | WebSocket command | WebSocket command |
| Audio | FunDSP (live synthesis) | Metadata display only (V1) | Metadata display only (V1) |
| Discovery | N/A (same process) | Bonjour/mDNS | NSD/mDNS |
| Keyboard shortcuts | F1 toggle, arrow keys | macOS menu bar + shortcuts | Chromebook keyboard |

### Parity gaps accepted in V1

- **Audio playback:** Only the Bevy process runs FunDSP synthesis. Native clients display audio metadata but don't play audio. Streaming audio is a V2 feature.
- **3D fidelity:** SceneKit and Filament render the GLB export, which may not perfectly match Bevy's rendering (custom shaders, procedural textures). Acceptable for inspection purposes.
- **Write operations:** All platforms are read-only in V1. Future V2 would allow editing from native clients (modify transform, toggle behaviors) via protocol commands that mutate Bevy ECS.
- **Real-time transform animation:** Bevy inspector shows live transform updates (behaviors animating). Native clients receive 10 Hz position updates for the selected entity but the GLB snapshot is static — entities don't animate in the SceneKit/Filament viewport. Future: stream per-entity transform updates to animate the native 3D view.
