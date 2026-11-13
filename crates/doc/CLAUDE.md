# CLAUDE.md

What is particular to `crates/doc`. The workspace's rules (no model, lint,
commits, the all-Rust rule) are in the root `CLAUDE.md`; run everything from
the workspace root with `-p localgpt-doc`.

Bevy takes one asset base, so `crates/doc/assets/models` is a **symlink** to
the shared pack that `cargo xtask fetch-assets` creates — run it once, or
the app starts with no asset pack and builds procedural worlds only.

## What this is

LocalGPT Document opens a Markdown file and turns it into a walkable 3D world
(Bevy 0.20). Each `##` section becomes a place; the file is watched and the
world rebuilds on save. What a section's place looks like beyond the
rule-based draft comes from the **sidecar** beside the document
(`<doc>.world.json`): a build (entities in place, real CC0 pack assets) or
a one-JSON recipe per section, keyed by the section's hash, which an agent
working on the file writes; a ```` ```world ```` fence in the file overrides
both. Document applies the sidecar and never writes it: **no model runs in this
crate**. Siblings in the workspace: `crates/verse` (song → world) and
`crates/gen` (prompt → world).

## Commands

```bash
cargo run -p localgpt-doc                                        # open samples/hello.md (world genre)
cargo run -p localgpt-doc -- crates/doc/samples/deck.md                     # present a Marp-style deck (deck genre)
cargo run -p localgpt-doc -- path/to/doc.md                      # open any Markdown file
cargo run -p localgpt-doc -- doc.md --print-ron                  # compiled world as RON, no window
cargo run -p localgpt-doc -- doc.md --export out.json            # or out.ron / out.html (web viewer page), no window
LOCALGPT_DOC_SCREENSHOT=/tmp/shot.png cargo run -p localgpt-doc   # render the first stop offscreen to a PNG, then exit
cargo test -p localgpt-doc
cargo clippy -p localgpt-doc --all-targets -- -D warnings
```

To check rendering, use the screenshot mode and read the PNG. It renders to
an offscreen image with no window, because reading back a window's frame
returns solid black when the Mac is locked or headless (Verse hit the same
thing: `crates/verse/ARCHITECTURE.md`, "Screenshots without a window").
Without a window, the camera has to be marked `IsDefaultUiCamera`, or the UI
isn't drawn.

## Architecture

`doc.rs` (Markdown → `Doc` of sections with BLAKE3 hashes) → `draft.rs`
(`Doc` + authored content → `WorldManifest`) → `scene.rs` (manifest → Bevy
entities), plus `tour.rs` (camera and caption), `watch.rs` (polling hot
reload), `recipe.rs`/`sidecar.rs` (the recipe type + the v2 cache: recipes
and builds) and `assets.rs` (where the pack is).

- **The lib/bin split is load-bearing**: `src/lib.rs` holds doc, draft,
  recipe, sidecar and assets — renderer-free, compiled without Bevy
  under `--no-default-features` — because `crates/app` takes exactly that,
  Bevy-free, with `default-features = false` (and a WASM build would too).
  The Bevy app is the bin behind the `app` feature. Keep new pure logic in the lib and renderer logic in the bin.
- **Genres** (`front_matter.genre`): `world` (default) — one section per
  `##` heading on a winding path; `deck` — one section per `---`-separated
  slide (Marp/Slidev) on a straight path. Deck separators must be
  blank-line padded: a bare `---` directly under text is a CommonMark setext
  H2, not a separator. In a deck, headings don't start sections; the first
  heading in a slide names it.
- **The sidecar is the authored tier** (M2, v2 in M3): per section, a
  `BuildEntry` (platform-local entities, who authored them, a description)
  or a `RegionRecipe` (palette, landmark, props), keyed by the section's
  hash. `draft::compile_with` applies a fence, else the build, else the
  recipe, else the rules; every placed entity is clamped (`clamp_local`:
  radius ≤ 12, y 0–25) and every recipe `clamped()` on load, so a hand- or
  agent-written sidecar can at worst be ignored. The writers
  (`RecipeStore::insert`, `insert_build`, `save`) have no caller in Document;
  they stay for the tests that prove the cache contract and for tooling.
- **The asset pack**: `assets.rs` resolves the pack root from
  `$LOCALGPT_DOC_ASSETS` (`$LOCALGPT_MD_ASSETS`, its old name, is still
  read) → `assets/` → the dev tree → the shared download
  (`localgpt_world_agent::world_pack_dir`); `cargo xtask fetch-assets` copies
  it locally. Placements land as `MeshAssetRef { path: "models/<file>" }`
  with span normalization baked into the transform; `scene.rs` loads them
  via `GltfAssetLabel::Scene(0)` + `WorldAssetRoot`, and `--export *.html`
  copies every referenced GLB to `<out>/assets/` (the viewer fetches them
  from there). Bevy needs the `jpeg` feature for the pack's textures.
- **```` ```world ```` fences** (M3): a JSON array of world entities in
  platform-local coordinates replaces the sidecar's tiers for that section —
  exact, deterministic, in the file. Entities may omit `id` (injected on
  parse). The fence text is part of the section hash, so editing only the
  fence invalidates the cache.
- **The world format is `localgpt-world-types`** (serde-only). Don't invent
  a parallel scene format; if something is missing, add it upstream in
  `../world-types`. Every manifest must pass `draft::validate`
  (Gen's save-time checks); a test in `draft.rs` enforces it.
- **`scene.rs` renders through `localgpt-world-bevy`**, the one Bevy mapping
  Gen and Verse use too (sRGB colours, linear emissive, `EulerRot::XYZ` in
  degrees, lux for directional lights and lumens for point/spot, spot angles
  in radians). Never re-implement a mapping here; fix it upstream so every
  app changes together. `--export x.html` embeds `localgpt-world-export`'s
  viewer, the same bytes Gen's `gen_export_html` writes.
- **Entity ids are stable per section**: `(section_index + 1) * 1000 + n`;
  ids 1–999 are global (ground, sun). Unchanged sections stay identical across
  edits, which the per-section cache relies on.
- **`Section::hash` is the cache key** for the sidecar. Anything that should
  invalidate an authored place must be part of the hashed text.
- **Recipes degrade, never break**: every `RegionRecipe` field is optional,
  everything is clamped (`RegionRecipe::clamped`, on sidecar insert and
  load), and a missing or malformed entry keeps the draft.

## Bevy 0.20 notes

Match LocalGPT Verse's idioms (`../verse/src`): `Hdr` is its own
component (`bevy::camera::Hdr`); scene-wide ambient light is the
`GlobalAmbientLight` resource; `TextFont { font_size: FontSize::Px(..), .. }`;
buffered events are messages (`MessageWriter<AppExit>`); hierarchy is
`ChildOf(parent)`, and `despawn()` is recursive.

## Notes

- `PLAN.md` is the key to the milestone numbers (M0–M5) the source cites.
- Copying from Verse and the rest of the workspace is fine (all Apache-2.0);
  name the source in a comment.
- `website-md/` at the workspace root is **localgpt.md, the previs page**,
  not Document's: it boards a screenplay through `crates/previs-web`. Walking a
  document lives in this crate's Bevy app and in `localgpt-app --doc`.
- Document's docs are on localgpt.app, in `website/content/docs/document/`: update them
  when commands, keys, export or the sidecar change.
