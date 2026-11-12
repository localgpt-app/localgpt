# LocalGPT MD — milestones

The key to the milestone numbers the source cites (`PLAN.md M0` … `M5`):
what each one built and where it lives. It is a map of what exists, not a
plan; how it got here is in Git.

| | What it is | Where |
|---|---|---|
| **M0** | Markdown → sections with BLAKE3 hashes → a rule-based draft (a `localgpt-world-types` `WorldManifest`, checked with Gen's validation) → Bevy entities; the tour camera and caption; hot reload by polling the file; `--print-ron`; the offscreen screenshot mode | `doc.rs`, `draft.rs`, `scene.rs`, `tour.rs`, `watch.rs`, `main.rs` |
| **M1** | The recipe tier: a `RegionRecipe` per section (palette, landmark, props), every field optional and clamped; a missing or malformed one keeps the draft. Atmosphere is not in a recipe: fog and ambient light are global in the manifest | `recipe.rs`, `draft.rs` |
| **M2** | The sidecar, `<doc>.world.json`: what was authored for each section, keyed by the BLAKE3 of its heading and body, pruned to the live sections on save, written atomically, clamped on load. A `.md` and its sidecar render the same in any build | `sidecar.rs` |
| **M3** | The build tier and exact overrides. Sidecar v2 adds `builds` (`BuildEntry { model, description, entities }`; a v1 file is ignored with a warning); placements use the shared asset pack; a ```` ```world ```` fence (a JSON array of entities in platform-local coordinates) overrides both tiers and is part of the section hash | `sidecar.rs`, `draft.rs`, `assets.rs`, `scene.rs`, `doc.rs` |
| **M4** | Genres: `world` and `deck` (`---`-separated slides along a straight path) are built; `script`, `adventure` and `journal` are not | `doc.rs`, `draft.rs` |
| **M5** | The shared runtime: the scene renders through `localgpt-world-bevy`, `--export` writes through `localgpt-world-export`, and the pack is found by `localgpt-world-agent`'s `paths`. The renderer-free half is the lib, which `crates/app` takes without Bevy | `lib.rs`, `scene.rs`, `assets.rs` |

In M1 and M3 a model inside this process wrote the recipes and the builds.
That went on 2026-11-01: an agent working on the file writes the sidecar,
and MD only applies it.

## Open

- Free-roam (WASD) as well as the tour, or the tour only.
- The remaining genres (M4).
