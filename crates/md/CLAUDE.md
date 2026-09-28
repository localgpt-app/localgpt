# CLAUDE.md

> **This crate lives in the `localgpt` workspace** (folded in from the
> standalone `localgpt-md` repository; see
> `docs/world-strategy.md` §13.6). Run everything from the
> workspace root with `-p localgpt-md`. The shared model and asset
> downloads are fetched once by `scripts/fetch-model.sh` and
> `scripts/fetch-assets.sh`, and resolved by
> `localgpt-world-agent`'s `paths` module.
>
> Bevy takes one asset base, so `crates/md/assets/models` is a **symlink**
> to the shared pack that `scripts/fetch-assets.sh` creates — run it once, or
> the app starts with no asset pack and builds procedural worlds only.

Guidance for Claude Code when working in this repository.

## What this is

LocalGPT MD opens a Markdown file and turns it into a walkable 3D world
(Bevy 0.19). Each `##` section becomes a place; the file is watched and the
world rebuilds on save. An optional local LLM (`llm` feature, ported from
LocalGPT Verse) authors each section — an agent composes the place through
tool calls (primitives, lights, real CC0 pack assets), falling back to a
one-JSON recipe tier; a ```` ```world ```` fence overrides both without a
model. Results cache per section. A member of the `localgpt`
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
../../scripts/fetch-model.sh                     # the ~5.2 GB LLM, once, shared with Verse and Gen
./scripts/fetch-assets.sh                        # copy the CC0 asset pack (optional; sibling repo works too)
cargo run -p localgpt-md --features llm-metal --                # open with live agent authoring
cargo run -p localgpt-md --features llm-metal -- doc.md --generate   # author all sections headless, exit
cargo test -p localgpt-md
cargo clippy -p localgpt-md --all-targets -- -D warnings        # and again with --features llm-metal
cargo fmt
```

Run `cargo check -p localgpt-md` after every change and fix all errors before reporting
completion. Run clippy and fmt before committing. CI
(`.github/workflows/ci.yml`) runs fmt, clippy, the tests, the sample exports,
`cargo check -p localgpt-md --features llm` and cargo-deny (`deny.toml`); advisories run on
main and weekly, never on pull requests.

To check rendering, use the screenshot mode and read the PNG. It renders to
an offscreen image with no window, because reading back a window's frame
returns solid black when the Mac is locked or headless (Verse hit the same
thing; see its ARCHITECTURE.md R8). Without a window, the camera has to be
marked `IsDefaultUiCamera`, or the UI isn't drawn.

## Architecture

`doc.rs` (Markdown → `Doc` of sections with BLAKE3 hashes) → `draft.rs`
(`Doc` + authored content → `WorldManifest`) → `scene.rs` (manifest → Bevy
entities), plus `tour.rs` (camera and caption), `watch.rs` (polling hot
reload), `recipe.rs`/`sidecar.rs` (recipe type + the v2 cache: recipes and
agent builds), `agent.rs`/`assets.rs` (the agent tier: command protocol,
pure interpreter, pack manifest), and `llm.rs`/`generation.rs` (the `llm`
feature: model + background worker running the tier chain **agent build →
recipe → draft**, first success wins).

- **The lib/bin split is load-bearing**: `src/lib.rs` holds doc, draft,
  recipe, sidecar, assets and agent — renderer-free, compiled without Bevy
  under `--no-default-features` — because `crates/md-web` builds exactly that
  to WASM for the localgpt.md page. The Bevy app is the bin behind the
  `app` feature. Keep new pure logic in the lib and renderer logic in the bin.
- **Genres** (`front_matter.genre`): `world` (default) — one section per
  `##` heading on a winding path; `deck` — one section per `---`-separated
  slide (Marp/Slidev) on a straight path. Deck separators must be
  blank-line padded: a bare `---` directly under text is a CommonMark setext
  H2, not a separator. In a deck, headings don't start sections; the first
  heading in a slide names it.
- **The agent tier (M3, ported from Verse's `agent.rs`/`agent_types.rs`)**:
  the model calls tools (`spawn_primitive`, `place_asset`, `scatter_field`,
  `modify_entity`, `delete_entity`, `set_light`, `scene_info`); a pure
  `SceneInterpreter` applies them to platform-local entities — no bridge, no
  Bevy executor (Verse needed those for its live scene; MD compiles to a
  manifest). Every model-authored value is clamped (position radius ≤ 12,
  y 0–25, emissive ≤ 1, 64 entities/section); kind→file resolution happens
  in-session (Verse's two-level vocabulary) so cached builds are
  self-contained. Consciously dropped from Verse: `set_environment`,
  `at_role`, Comfort gates, mood-preferred resolution, live streaming.
- **The asset pack**: `assets.rs` reads `models/manifest.json` (171 Poly
  Haven CC0 GLBs) from `$LOCALGPT_MD_ASSETS` → `assets/` → the sibling
  `localgpt-verse-assets` checkout; `scripts/fetch-assets.sh` copies it
  locally. Placements land as `MeshAssetRef { path: "models/<file>" }`
  with span normalization baked into the transform; `scene.rs` loads them
  via `GltfAssetLabel::Scene(0)` + `WorldAssetRoot`, and `--export *.html`
  copies every referenced GLB to `<out>/assets/` (the viewer fetches them
  from there). Bevy needs the `jpeg` feature for the pack's textures.
- **```` ```world ```` fences** (M3): a JSON array of world entities in
  platform-local coordinates replaces both LLM tiers for that section —
  exact, deterministic, no model. Entities may omit `id` (injected on
  parse). The fence text is part of the section hash, so editing only the
  fence invalidates the cache.
- **The world format is `localgpt-world-types`** (serde-only). Don't invent
  a parallel scene format; if something is missing, add it upstream in
  `../world-types`. Every manifest must pass `draft::validate`
  (Gen's save-time checks); a test in `draft.rs` enforces it. The three
  world crates are pinned to the `localgpt` repository in `Cargo.toml` until
  the next crates.io release; switch them back to version requirements then.
- **`scene.rs` renders through `localgpt-world-bevy`**, the one Bevy mapping
  Gen and Verse use too (sRGB colours, linear emissive, `EulerRot::XYZ` in
  degrees, lux for directional lights and lumens for point/spot, spot angles
  in radians). Never re-implement a mapping here; fix it upstream so every
  app changes together. `--export x.html` embeds `localgpt-world-export`'s
  viewer, the same bytes Gen's `gen_export_html` writes.
- **Entity ids are stable per section**: `(section_index + 1) * 1000 + n`;
  ids 1–999 are global (ground, sun). Unchanged sections stay identical across
  edits, which the per-section cache relies on.
- **`Section::hash` is the cache key** for LLM output. Anything that should
  trigger regeneration must be part of the hashed text.
- **Recipes degrade, never break**: every `RegionRecipe` field is optional,
  everything is clamped (`RegionRecipe::clamped`, again on sidecar insert and
  load), and a failed generation keeps the draft. The default (no-`llm`)
  build still *applies* cached sidecars — which is why recipe/sidecar are
  unconditionally compiled (targeted `#[allow(dead_code)]`, Verse `tier.rs`
  precedent).
- **The LLM constraints are Verse's, verified** (`../verse/ARCHITECTURE.md`
  §10): plain instructed-JSON generation with a lenient parse — mistral.rs
  0.8's grammar-constrained `generate_structured` hangs on GGUF (so no
  `schemars`); the 5 GB Q4_K_M needs `llm-metal` (macOS-only, never in the
  Linux CI job); model discovery is `$LOCALGPT_MD_LLM` → the directory the
  LocalGPT apps share (`$LOCALGPT_LLM_DIR`, default
  `~/.local/share/localgpt/models/llm`; `shared_llm_dir` in `src/llm.rs`
  must match localgpt-core's and Verse's) → `assets/llm` → Verse's
  `assets/llm`.

## Bevy 0.19 notes

Match LocalGPT Verse's idioms (`../verse/src`): `Hdr` is its own
component (`bevy::camera::Hdr`); scene-wide ambient light is the
`GlobalAmbientLight` resource; `TextFont { font_size: FontSize::Px(..), .. }`;
buffered events are messages (`MessageWriter<AppExit>`); hierarchy is
`ChildOf(parent)`, and `despawn()` is recursive.

## Plan and reuse

See PLAN.md. M0–M2 and M4a (`deck`) are done; M5 is done for rendering and
export. Next: M3 (agent tier — Verse's `agent.rs`/`agent_types.rs`
tool-calling port), then the remaining genres and M5's runtime extraction.
Verse and LocalGPT are Apache-2.0, so copying from them is fine; name the
source in a comment. Model notes: Bonsai-8B Q4_K_M is the verified default;
stock Qwen3-8B-Instruct Q4_K_M is a drop-in A/B; the newer ternary Bonsai
generations (1-bit/2-bit) cannot run on mistral.rs 0.8 — skip them.

## Rules

- License is Apache-2.0. Never copy code from Local Native or Fastxt (AGPL-3.0).
- This repo is public. Never name the closed-source sibling 3D platform; use
  generic terms such as "connected 3D app".
- Commits: conventional commits (`feat:`, `fix:`, `docs:`, `chore:`,
  `refactor:`), with no Co-Authored-By or Claude-Session trailers.
- Never use `sed` to edit Rust files; use the Edit tool.
- `website-md/` (workspace root) is the **localgpt.md page**: drop a Markdown
  file, walk the world, in the tab. Its compiler is this crate's lib half via
  `crates/md-web` (wasm-pack; `localgpt-md --no-default-features` must stay
  Bevy-free for it), and it is assembled like website-world. The docs live on
  localgpt.app under `website/docs/md/`; update them when commands, keys,
  export or the LLM tier change.
