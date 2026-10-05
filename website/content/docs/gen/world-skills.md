---
title: "World Skills"
---

# World Skills

Save and load complete worlds as reusable skills. Worlds are stored as skill directories containing all scene data in a single RON manifest.

## World Format

A saved world consists of:

```
~/.localgpt/workspace/skills/my-world/
├── SKILL.md          # Skill description for LLM context
├── world.ron         # WorldManifest (entities, materials, behaviors, audio, tours, camera, avatar)
├── history.jsonl     # Undo/redo edit history
└── assets/
    └── meshes/       # Copied mesh assets referenced by world.ron
        ├── tree.glb
        └── rock.glb
```

### world.ron

The `world.ron` file is a RON (Rusty Object Notation) manifest containing everything about the world inline — entities with parametric shapes, PBR materials, behaviors, audio, environment, camera, avatar, and tours. This format preserves full parametric shape data (unlike glTF exports which bake geometry).

Example structure (simplified):

```ron
(
    version: 3,
    meta: ( name: "forest-clearing", description: Some("A peaceful clearing") ),
    environment: Some((
        background_color: Some((0.53, 0.81, 0.92, 1.0)),
        ambient_intensity: Some(0.3),
    )),
    camera: Some(( position: (0.0, 5.0, 10.0), look_at: (0.0, 0.0, 0.0), fov_degrees: 45.0 )),
    avatar: Some((
        spawn_position: (0.0, 1.8, 5.0),
        spawn_look_at: (0.0, 0.0, 0.0),
        pov: first_person,
        movement_speed: 5.0,
        height: 1.8,
    )),
    tours: [
        (
            name: "overview",
            description: Some("A quick tour of the main areas"),
            speed: 3.0,
            mode: fly,
            waypoints: [
                ( position: (0.0, 3.0, 10.0), look_at: (0.0, 0.0, 0.0), description: Some("Welcome"), pause_duration: 3.0 ),
                ( position: (10.0, 2.0, 0.0), look_at: (0.0, 1.0, 0.0), description: Some("Main structure"), pause_duration: 5.0 ),
            ],
        ),
    ],
    entities: [
        (
            id: (1), name: ("ground"),
            shape: Some(Plane( x: 50.0, z: 50.0 )),
            material: Some(( color: (0.2, 0.5, 0.1, 1.0), roughness: 0.9 )),
        ),
    ],
)
```

Numbers follow the conventions stated at the top of the format's JSON Schema
(`crates/world-types/world.schema.json`): positions in world units with Y up,
rotations as XYZ Euler degrees, colours as RGBA in `0..=1` (sRGB-encoded,
except `emissive`, which is linear), directional light in lux, point and spot
lights in lumens.

### Reusable creations

A creation with `parts` is defined once and placed many times. Each part is
an entity in the creation's own coordinates; an entity with `instance_of`
places a copy, and its `overrides` change single parts of that copy only:

```ron
creations: [
    (
        id: (1), name: "tree", semantic_category: Some(vegetation),
        parts: [
            ( id: (1), name: ("trunk"), shape: Some(Cylinder( radius: 0.2, height: 2.0 )) ),
            ( id: (2), name: ("crown"), parent: Some((1)),
              transform: ( position: (0.0, 1.6, 0.0) ),
              shape: Some(Sphere( radius: 1.0 )) ),
        ],
    ),
],
entities: [
    ( id: (10), name: ("oak_1"), transform: ( position: (-3.0, 0.0, 0.0) ),
      instance_of: Some(( creation: (1) )) ),
    ( id: (11), name: ("oak_2"), transform: ( position: (3.0, 0.0, 0.0) ),
      instance_of: Some(( creation: (1), overrides: [
          ( part: ("crown"), patch: ( material: Some(Some(( color: (0.8, 0.3, 0.1, 1.0) ))) ) ),
      ] )) ),
],
```

On load, every instance expands into its parts, named `<instance>/<part>`
(`oak_2/crown`), which you can modify like any entity. Saving writes the
instance back with only what differs from the definition. Make creations with
`gen_define_creation` and place them with `gen_spawn_instance`.

### Triggers

`triggers` on an entity pair an event with an action on that entity. The
events are `start`, `click`, `proximity`, `area_enter`, `area_exit`,
`collision` and `timer`. The actions are `show_text`, `show`, `hide`,
`toggle`, `remove`, `animate`, `teleport` (moves the visitor) and `host`.
Every renderer runs the same core: Gen through `localgpt-world-bevy`'s trigger
runtime, localgpt.world through the web viewer. `host` actions are the ones
only an app understands; Gen runs `add_score`, `play_sound` and `set_state`,
and other renderers skip them.

An area is a volume in the entity's own frame, so it moves, turns and scales
with the entity, and it doesn't depend on what the entity looks like:

```ron
triggers: [
    (
        on: ( event: "area_enter", volume: Some(( shape: "box", half_extents: (1.5, 1.5, 1.5) )) ),
        action: ( action: "show_text", text: "Through the gate.", seconds: 4.0 ),
    ),
],
```

Without a `volume`, the area is the box of the entity's parametric shape, or a
sphere of radius 3 when it has none. Triggers added with `gen_add_trigger` are
saved with the world.

### Imported meshes

A `mesh_asset` can carry the file's `sha256`, which Gen fills in on save and
checks on load (a mismatch logs a warning), and `node_overrides` that hide or
recolour named nodes inside the glTF for this entity only.

## Saving Worlds

```json
gen_save_world({
  "name": "forest-clearing",
  "description": "A peaceful forest clearing with stream and campfire"
})
```

This saves the current scene to `~/.localgpt/workspace/skills/forest-clearing/`.

## Loading Worlds

```json
gen_load_world({
  "path": "forest-clearing"
})
```

By default, loading a world clears the existing scene first. To preserve existing entities:

```json
gen_load_world({
  "path": "forest-clearing",
  "clear": false
})
```

You can also load by full path:

```json
gen_load_world({
  "path": "/path/to/world-skill-directory"
})
```

## Clearing Scenes

To clear all entities, behaviors, and audio without loading a new world:

```json
gen_clear_scene({
  "keep_camera": true,
  "keep_lights": true
})
```

## HTML Export

Export a world as a self-contained HTML file with Three.js rendering:

```json
gen_export_html()
```

The exported HTML includes:
- Full 3D scene with PBR materials and lighting
- WASD keyboard navigation and orbit controls
- Procedural audio synthesis (Web Audio API)
- Guided tour playback
- Embeddable via `<iframe>` with postMessage API for parent-frame control
- SEO metadata (Open Graph, JSON-LD structured data)

## Showcase

- **[localgpt-gen-workspace](https://github.com/localgpt-app/localgpt-gen-workspace)** — "World as skill" examples: complete explorable worlds saved as reusable, shareable skills
