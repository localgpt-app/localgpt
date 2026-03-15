---
sidebar_position: 14.4
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
    version: 1,
    meta: ( name: "forest-clearing", description: Some("A peaceful clearing") ),
    environment: Some((
        background_color: Some([0.53, 0.81, 0.92, 1.0]),
        ambient_intensity: Some(0.3),
    )),
    camera: Some(( position: [0.0, 5.0, 10.0], look_at: [0.0, 0.0, 0.0], fov_degrees: 45.0 )),
    avatar: Some((
        spawn_position: [0.0, 1.8, 5.0],
        spawn_look_at: [0.0, 0.0, 0.0],
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
                ( position: [0.0, 3.0, 10.0], look_at: [0.0, 0.0, 0.0], description: Some("Welcome"), pause_duration: 3.0 ),
                ( position: [10.0, 2.0, 0.0], look_at: [0.0, 1.0, 0.0], description: Some("Main structure"), pause_duration: 5.0 ),
            ],
        ),
    ],
    entities: [
        (
            id: (1), name: "ground",
            shape: Some(plane( x: 50.0, z: 50.0 )),
            material: Some(( color: [0.2, 0.5, 0.1, 1.0], roughness: 0.9 )),
        ),
    ],
)
```

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
