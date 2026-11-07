# CLAUDE.md

> **This crate lives in the `localgpt` workspace** (folded in from the
> standalone `localgpt-md` repository; see
> `docs/world-strategy.md` §13.6). Run everything from the
> workspace root with `-p localgpt-md`. The shared asset pack is fetched
> once by `cargo xtask fetch-assets` and resolved by
> `localgpt-world-agent`'s `paths` module.
>
> Bevy takes one asset base, so `crates/md/assets/models` is a **symlink**
> to the shared pack that `cargo xtask fetch-assets` creates — run it once, or
> the app starts with no asset pack and builds procedural worlds only.

Guidance for Claude Code when working in this repository.

## What this is

LocalGPT MD opens a Markdown file and turns it into a walkable 3D world
(Bevy 0.20). Each `##` section becomes a place; the file is watched and the
world rebuilds on save. What a section's place looks like beyond the
rule-based draft comes from the **sidecar** beside the document
(`<doc>.world.json`): a build (entities in place, real CC0 pack assets) or
a one-JSON recipe per section, keyed by the section's hash, which an agent
working on the file writes; a ```` ```world ```` fence in the file overrides
both. MD applies the sidecar and never writes it — **no model runs in this
crate** (the in-process one went on 2026-11-01). A member of the `localgpt`
workspace, Apache-2.0. Siblings in the same workspace: `crates/verse`
(song → world) and `crates/gen` (prompt → world).

## Commands

```bash
cargo run -p localgpt-md                                        # open samples/hello.md (world genre)
cargo run -p localgpt-md -- crates/md/samples/deck.md                     # present a Marp-style deck (deck genre)
cargo run -p localgpt-md -- path/to/doc.md                      # open any Markdown file
cargo run -p localgpt-md -- doc.md --print-ron                  # compiled world as RON, no window
cargo run -p localgpt-md -- doc.md --export out.json            # or out.ron / out.html (web viewer page), no window
LOCALGPT_MD_SCREENSHOT=/tmp/shot.png cargo run -p localgpt-md   # render the first stop offscreen to a PNG, then exit
cargo xtask fetch-assets                         # copy the CC0 asset pack (optional; sibling repo works too)
cargo test -p localgpt-md
cargo clippy -p localgpt-md --all-targets -- -D warnings
cargo fmt
```

Run `cargo check -p localgpt-md` after every change and fix all errors before reporting
completion. Run clippy and fmt before committing. CI
(`.github/workflows/ci.yml`) runs fmt, clippy, the tests and cargo-deny
(`deny.toml`); advisories run on main and weekly, never on pull requests.

To check rendering, use the screenshot mode and read the PNG. It renders to
an offscreen image with no window, because reading back a window's frame
returns solid black when the Mac is locked or headless (Verse hit the same
thing; see its ARCHITECTURE.md R8). Without a window, the camera has to be
marked `IsDefaultUiCamera`, or the UI isn't drawn.

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
  (`RecipeStore::insert`, `insert_build`, `save`) have no caller in MD;
  they stay for the tests that prove the cache contract and for tooling.
- **The asset pack**: `assets.rs` resolves the pack root from
  `$LOCALGPT_MD_ASSETS` → `assets/` → the dev tree → the shared download
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

## Plan and reuse

See PLAN.md: M0–M5 are done as built; M1's and M3's in-process model went
on 2026-11-01, and the sidecar they filled is now the agent's to write.
Verse and LocalGPT are Apache-2.0, so copying from them is fine; name the
source in a comment.

## Rules

- License is Apache-2.0. Never copy code from Local Native or Fastxt (AGPL-3.0).
- This repo is public. Never name the closed-source sibling 3D platform; use
  generic terms such as "connected 3D app".
- No model in this crate: nothing may load a language model, call a
  provider or a service, or hold a key. An agent authors; MD applies.
- Commits: conventional commits (`feat:`, `fix:`, `docs:`, `chore:`,
  `refactor:`), with no Co-Authored-By or Claude-Session trailers.
- Never use `sed` to edit Rust files; use the Edit tool.
- `website-md/` (workspace root) is **localgpt.md, the previs page** — not
  MD's: since 2026-10-12 it drops a screenplay (PDF, Final Draft, Fountain
  or Markdown script pages) and boards it through `crates/previs-web`. The drop-a-Markdown-file
  demo and `crates/md-web` are gone; walking a document lives in this crate's
  Bevy app. The docs live on localgpt.app under `website/docs/md/`; update
  them when commands, keys, export or the sidecar change.
