# Conformance worlds

Reference `WorldManifest`s, one feature area per file, that every renderer of
the format must load and draw the same way:

| File | Covers |
|---|---|
| `shapes.json` | All eleven parametric shapes, a ground plane, a sun, a camera |
| `materials.json` | Metallic × roughness grid, emissive, blend / add / mask alpha, unlit, double-sided, reflectance |
| `lights.json` | Directional, point (range) and spot (angles) lights with shadows, dark environment with fog |
| `behaviors.json` | All seven behaviors: orbit (entity and point), spin, bob, look_at, pulse, path_follow (loop, ping-pong, once), bounce |
| `hierarchy_tours.json` | Parent/child transforms, a group entity, an avatar, a fly tour with captions, ambient and spatial procedural audio |
| `soundtrack.json` | A soundtrack (analysis only, no audio file) and modulations of every target by every signal kind |
| `textures.json` | Albedo (plain and tinted), metallic-roughness, normal and emissive maps, and all four together; images in `assets/textures/` |

`tests/conformance.rs` parses each file, checks `validate_manifest` reports no
errors, and round-trips it through RON and JSON. The web viewer
(`crates/world-export/js/world-viewer.js`) and the Bevy mapping
(`crates/world-bevy`) load the same files; localgpt.world serves them under
`worlds/`. When a renderer changes how it draws something, render these before
and after.

Numbers are plain JSON: colours are linear RGBA, rotations are XYZ Euler
degrees, directional light intensity is lux, point and spot are lumens, spot
angles are radians.
