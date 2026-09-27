# LocalGPT MD — Plan

Open a Markdown file and walk through it as a 3D world. Verse turns a song
into a world; this turns a document into one. Most of the machinery already
exists in LocalGPT Verse, so this plan is mostly about porting it behind a
Markdown front end.

## M0 — Scaffold ✅

- Markdown → sections with BLAKE3 hashes (`doc.rs`).
- Rule-based draft → a `localgpt-world-types` `WorldManifest` (`draft.rs`),
  checked with Gen's validation.
- Bevy renderer using Gen's mapping (`scene.rs`); tour navigation and caption
  (`tour.rs`).
- Hot reload by polling the file (`watch.rs`); `--print-ron`; an offscreen
  screenshot mode for checking the render without a display.

## M1 — Recipe tier: a local LLM per section ✅

- Ported Verse's `llm.rs` and the recipe idea behind `llm` / `llm-metal`
  features (mistral.rs 0.8; Bonsai-8B Q4_K_M via `scripts/fetch-bonsai.sh`,
  which also reuses a sibling Verse checkout's model). Verse's `tier.rs`
  became a simpler lazy load inside the worker (load on first job, keep
  resident).
- Prompt per section: heading + body excerpt + genre → a `RegionRecipe`
  (palette, landmark kind/scale/glow, prop kind/count), clamped on the Rust
  side. Any failure keeps the draft; failures are sticky for the run.
- The draft renders instantly; a background `std::thread` worker (Verse's
  analysis-worker split, mpsc channels) styles uncached sections and each
  region upgrades in place as its recipe lands. `--generate` runs the same
  authoring headless.
- Verse's constraints carried over verbatim (`ARCHITECTURE.md` §10): plain
  generation with a lenient JSON parse (grammar-constrained generation hangs
  on GGUF), no `schemars` dependency, Metal for the 5 GB quant.
- Deliberately not in the recipe yet: atmosphere (fog/ambient are global in
  the manifest — needs a per-region tint mechanism first).

## M2 — Section cache (the "lockfile") ✅

- `src/sidecar.rs`: `<doc>.world.json`, keyed by BLAKE3 of heading + body,
  pruned to live sections on save, written atomically, clamped on load.
  Unchanged sections never regenerate; a `.md` + sidecar renders identically
  in any build (the default build applies cached recipes, it just can't
  author them).
- `meta.source` says "draft + llm recipe" when any recipe is applied.
  `meta.model` still isn't set: the sidecar doesn't record which model
  authored a recipe. Add a `model` field in sidecar v2 when a second model
  becomes an option (Bonsai-8B vs stock Qwen3-8B are drop-in `--generate`
  A/B candidates; the ternary Bonsai generations can't run on mistral.rs).

## M3 — Agent tier ✅

- Ported Verse's `agent.rs`/`agent_types.rs` session half (tool schemas,
  parsing, kind resolution, the mistral.rs tool-calling loop) behind `llm`;
  the Bevy executor/bridge Verse needs for its live scene became a pure
  `SceneInterpreter` on the worker thread — MD compiles to a manifest, so
  entities, not scene commands, are the deliverable. Verified on the Metal
  build: 4/4 sections of `samples/hello.md` built by the agent (~45 s each,
  17–22 entities), zero recipe fallbacks.
- Real assets: `assets.rs` ports the pack's pure block (manifest, kinds,
  span normalization, deterministic scatter math) with the mood coupling
  dropped; `place_asset`/`scatter_field` speak the kind vocabulary; `scene.rs`
  loads placed GLBs (`GltfAssetLabel::Scene(0)` + `WorldAssetRoot` — bevy
  needs the `jpeg` feature for the pack's textures); `--export *.html`
  copies every referenced GLB beside the page (16 files / 172 MB for the
  hello sample). The pack resolves from `$LOCALGPT_MD_ASSETS` → `assets/` →
  the sibling `localgpt-verse-assets` checkout;
  `scripts/fetch-assets.sh` copies it locally.
- ```` ```world ```` fences: a JSON array of world entities in
  platform-local coordinates is an exact override (no LLM); ids optional;
  part of the section hash.
- Sidecar v2: `builds` map with `BuildEntry { model, description, entities }`
  (closes M2's `meta.model` note for builds); v1 rejected with a warn.
- Consciously dropped from Verse (and why): `set_environment` (the env is
  global and draft-owned), `at_role` song scoping (no transport), Comfort
  gates (static caps instead), mood-preferred kind resolution (no moods),
  `VisibilityRange` LOD (no field in world-types; regions are small),
  live-streaming during sessions (results land atomically per section).
- Known follow-ups: `used`-rotation is per session, so sections can repeat
  the same first variants (share a session-scoped list per document if it
  bothers); a session log (`SceneBuild` commands) is no longer persisted —
  entities are — bring it back if debugging needs the raw calls.

## M4 — Genres

- ✅ `genre: deck` (M4a): `---`-separated slides (Marp/Slidev style) become
  tour stops along a straight presentation path; the first heading in a
  slide names it; the prompt carries a deck-specific note (one focal idea,
  palette from the slide's subject).
- Prompt tuning (rode along): the neon colors of the first M1 run are gone —
  recipes now come back muted and desaturated. Known limitation: the 8B
  mode-collapses on similar accents across slides (5 of 6 shared a hue in
  the sample run). Follow-ups if it matters: give the prompt the slide
  index/total so it can lean away from earlier slides, or a deterministic
  hue de-dup pass when two regions' accents land within ε.
- Then `script`, `adventure`, and `journal`.

## M5 — Extract the shared runtime ✅ (runtime done; Gen pending)

- ✅ The agent runtime is now the **`localgpt-world-agent`** crate
  (`localgpt/crates/world-agent`, Bevy-free): the tool-call protocol
  (AgentCommand + cmds, union of MD/Verse vocabularies incl.
  BeginSession/SetEnvironment/at_role/SectionRole), parse/resolve, the pure
  `SceneInterpreter` (commands → platform-local world-types entities with
  clamping), `SceneBuild`/`BuildOutput`, the asset-pack manifest and
  deterministic scatter math, and the mistral.rs session loop behind
  `llm`/`llm-metal` (+ a `schema` feature for JsonSchema derives). MD's
  `src/agent.rs`/`assets.rs` are now ~50-line shims over it; Verse's
  `agent_types.rs` is Bevy markers + crate re-exports and its `agent.rs`
  uses the crate's parse/resolve. Pins bumped to the commit with the crate;
  both apps' `llm` features forward into it.
- **Done for rendering and export:** `scene.rs` maps the manifest through
  `localgpt-world-bevy` (Gen's mapping too), and `--export` writes `.json`,
  `.ron` or `.html` through `localgpt-world-export`, the crate that holds
  the one web viewer. Both are pinned to the `localgpt` repository until
  their crates.io release.
- **Still open:** Gen is not migrated — it uses the `gen_`-prefixed
  command-level API (provider-agnostic Agent, batch ops with revision
  checks), a different tier. A later pass could add `place_asset`/
  `scatter_field` to Gen via the crate and consolidate the clamp regimes
  into a shared `Limits` struct (MD/Verse/Gen clamp differently; the crate
  currently carries MD's caps). Verse's `EulerRot::YXZ` executor is an
  outlier vs world-bevy's XYZ — its adapter should converge on XYZ on the
  next touch. crates.io publishing at the next 0.3.x release will let MD
  and Verse drop the git pins.

## Open questions

- Free-roam (WASD) as well as the tour, or tour only?
- A WASM build on md.localgpt.app, or desktop downloads only?
- ~~Export by writing `world.ron` and reusing Gen's HTML/glTF exporters?~~ —
  answered: `--export` writes `.json`/`.ron`/`.html` through
  `localgpt-world-export` (M5, rendering + export half).
