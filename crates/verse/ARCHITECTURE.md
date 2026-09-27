# LocalGPT Verse — Architecture Review (as built)

**Status:** Accepted (review of shipped M1–M6 state)
**Date:** 2026-07-11
**Scope:** `localgpt-verse` (5,143 lines, 9 modules) + `verse-assets`, evaluated
against `idea.md` (product research) and the design spec's timing promises.
**Verdict up front:** the architecture is sound for its scale and matches the
product thesis; nothing needs a rewrite. The two structural debts worth paying
soon are the **missing transition engine** (crossfade/materialize — the
product's signature feel, and the one place shipped milestones fell short of
their own spec) and **UI-layer coupling** (`overlays.rs` + boolean-resource
sprawl). Everything else is maintenance-grade.

## 1. System map

```
                 Bevy main thread                        other threads
┌──────────────────────────────────────────┐   ┌───────────────────────────┐
│ UI (hud.rs, overlays.rs)                 │   │ kira audio thread (cpal)  │
│   reads: Playback, Beat, Theme, Comfort  │   │   StreamingSound decode   │
│   writes: intent resources (Paused, …)   │   │   TapEffect: energy/onset │
├──────────────────────────────────────────┤   │   → AtomicU32 (lock-free) │
│ Transport (audio.rs systems)             │◄──┤                           │
│   sync_track_playback / pause / clock    │   ├───────────────────────────┤
│   owns elapsed + end-of-track while live │   │ import scan thread        │
├──────────────────────────────────────────┤   │   walkdir+lofty → mpsc    │
│ Signals (playback.rs advance_playback)   │   ├───────────────────────────┤
│   ladder: simulation → live tap → grid   │◄──┤ analysis worker thread    │
├──────────────────────────────────────────┤   │   symphonia→realfft→tempo │
│ World (world.rs, world_assets.rs)        │   │   /sections/energy/mood   │
│   procedural ambient + manifest props    │   │   → mpsc, JSON sidecars   │
└──────────────────────────────────────────┘   └───────────────────────────┘
         persistence: blake3-keyed sidecars in the app data dir
         assets: verse-assets repo → assets/models/ (bundled at ship)
```

Threading is channel/atomic-only across boundaries; the single shared-state
exception is `Mutex<AudioManager>` (kira is `Send`, not `Sync`; locks are
microseconds). No `unsafe`, no GPL deps, CC0-only assets, OFL fonts.

## 2. Load-bearing decisions that are working

1. **The resource seam.** The UI reads exactly three data resources
   (`Playback`, `Beat`, `Theme`) and writes small intent resources. This held
   through three backend replacements (simulated → kira playback → live tap →
   analysis grid) with **zero UI changes** — the strongest validation of the
   design. Keep treating these three as the app's public API.
2. **The signal-ownership ladder** (`advance_playback` doc): simulation owns
   everything → live audio owns clock+energy+onsets → analysis grid owns
   phase/sections/mood. Each rung is a strict upgrade with automatic fallback,
   which is why the app cannot be broken by a missing device, file, manifest,
   or sidecar. This pattern should be preserved as M5/M7 land (CLAP/Beat This!
   are just higher rungs).
3. **Content-hash sidecars.** blake3-keyed JSON in the app data dir: rename-
   and move-proof, human-debuggable, no schema migrations, never touches the
   music folder. Right call for this scale (revisit rusqlite only when a
   library view needs search over thousands of tracks — PLAN §5.2).
4. **The asset manifest as triple-duty artifact** — placement data (tier/mood/
   scale), legal audit trail (author/license/source), and the Credits screen's
   data source. One file keeps the CC0-first policy enforceable and visible.
5. **Comfort as a cross-cutting gate.** `reduce_flashing`/`gentler_motion`
   are checked at the effect sites (world glow, playhead pulse, sway), not in
   the UI — new effects inherit the discipline by convention. Worth keeping
   ruthless about as the modulation layer grows.

## 3. Alignment with `idea.md`'s reference architecture

| Layer (idea.md §4) | Target | Built | Gap |
|---|---|---|---|
| 1. Offline asset curation | normalize glb, tag, embed, signed manifest, 50–100 models | manifest + provenance, 7 CC0 models, raw 1k gltf | no normalization step (fine at 24 MB), no embeddings (M5), pack is a starter |
| 2. Song pre-analysis | beats, mood, structure, stems | in-house DSP: tempo ±0.2%, beat grid, sections, energy, quadrant mood; cached | no ML mood (M5), no stems (M7 — live band proxy instead), single-song ahead-of-play only |
| 3. Scene planner | rule mapper v1, later LLM recipes + CLAP selection | quadrant mapper + per-track pin | CLAP (M5), LLM (M7) — both explicitly optional tiers |
| 4. Runtime assembler | SceneRoot spawn, WFC/Poisson placement, instancing, `VisibilityRange` LOD | glTF spawn + tiered golden-angle scatter | fixed counts (3/5/9), one placement pattern, **no LOD/instancing** — irrelevant at 16 props, required before dense packs |
| 5. Continuous modulation | WGSL displacement, particles, eased keyframes, bar-quantized crossfades | emissive glow + UI pulse from live tap; palette morph | **the transition engine** — see §4 |

Stage-0 legal policy (CC0-first, manifest, no Tier-C): followed exactly.
Stage-1 vertical slice: functionally achieved, but its 60 fps @ 5k-instance
validation threshold has **never been attempted** (current world ≈ 120
entities). Treat that budget as an unvalidated claim, not a met one.

## 4. The honest gap: the transition engine

The design spec's most distinctive promises are temporal:

- materialize: palette wash 0.8s → terrain rises 1.3–2.4s → **settles on the
  first downbeat** (1g);
- song → song: **6–10s crossfade landing on a section boundary** of the
  incoming track; close moods morph in place (1h, 1r).

M3 shipped the *data* for this (beat grid, downbeats, sections, per-track
mood) and kira ships the *mechanism* (clocks, scheduled starts, tweens), but
the wiring was never built: track changes today are a 300ms fade + instant
world swap. This is the largest spec-vs-built divergence, it sits squarely on
the product's identity, and it is now the cheapest high-value work in the
plan — no new dependencies, no downloads, data already cached.

## 5. Findings & risks (ranked)

| # | Severity | Finding | Note |
|---|---|---|---|
| R1 | High (product) | No transition engine (§4) | Data + mechanism exist; wire A→B dual-stream crossfade scheduled on B's nearest section boundary + keyframed materialize against the beat grid |
| R2 | High (process) | **LocalGPT Verse is not in CI.** Monorepo CI builds `localgpt` workspace only; the standalone localgpt-verse workspace is never compiled/tested upstream | Add a CI job: build + clippy + fmt + `cargo test` (headless; no smoke) |
| R3 | Med (structure) | `overlays.rs` = 1,737 lines (34% of app); `handle_buttons` takes 14 `ResMut`s and grows with every action | Split per-overlay modules; dispatch `ButtonAction` as a Bevy message/observer so handlers take only what they touch |
| R4 | Med (structure) | Overlay state = 5 independent bools (`Paused`, `QueueOpen`, `SettingsOpen`, `CreditsOpen`, `LibraryOpen`) with hand-ordered Esc priority | Replace with one `enum OverlayStack` (or Bevy sub-states); Esc = pop |
| R5 | Med (correctness-later) | Track identity is `PathBuf` (in-memory analysis map, `playing` key) + blake3 (sidecars). Fine today; queue reorder ("drag to reorder" is promised in the queue panel) and dedupe will want a stable `TrackId` | Introduce `TrackId(blake3)` on import; key everything on it |
| R6 | Med (spec) | "Keep this world" pins **mood only**; PLAN §5.3 decided `{mood, seed}` but world generation takes no seed — "Build a different world" cycles mood instead of re-rolling layout | Add a layout seed to prop placement; store it in the pin |
| R7 | Low (perf, deferred) | No `VisibilityRange`/instancing; placement counts hard-coded | Only matters when packs grow past ~hundreds of props; add with the next asset expansion |
| R8 | Low (env) | GPU screenshot readback intermittently returns black frames on this machine (workaround: `VERSE_ONESHOT` + retry) | Affects Photo mode UX too — consider a retry-on-black in `photo_capture` |
| R9 | Low (packaging) | "Bundle at ship-time" has no implementing step; dev copy was manual | Add an `xtask bundle` (copy `verse-assets/models` + fonts, build release) when distribution nears |
| R10 | Low (docs) | PLAN marks M3 "done" though its crossfade exit criterion wasn't met (see §4) | This review is the correction; fold R1 into the next milestone |

Non-findings worth recording: `bevy_audio` is *not* in Bevy 0.19's default
features (no dead audio stack); the `Mutex<AudioManager>` pattern is
appropriate (kira parks the cpal stream on its own thread; `AudioManager` is
`Send`); JSON sidecars remain the right persistence at this scale.

## 6. Decisions ahead (mini-ADRs)

**D1 — Transition engine now, before more content.** Options: (a) wire
crossfade+materialize next; (b) grow asset packs first. **Recommend (a)** —
it is the product's differentiator, all inputs exist, and it de-risks the
audio architecture (dual concurrent streams) before packs make worlds heavier.

**D2 — Overlay refactor before the next screen.** Options: (a) keep booleans
and grow `handle_buttons`; (b) `OverlayStack` enum + message-based actions.
**Recommend (b)** at the *next* UI addition, not as a standalone rewrite —
piggyback the refactor on real work.

**D3 — M5 (CLAP) stays opt-in.** The quadrant mapper is deterministic,
explainable, and shippable. Adopt CLAP only behind an `ml` feature with the
mapper as fallback, per PLAN §1.2 — and only after D1, since selection quality
matters less than transition feel.

**D4 — CI now.** No options worth debating; a 20-line workflow job removes the
only unprotected-regression surface in the monorepo.

## 7. Action items

1. [ ] R2/D4: add a localgpt-verse job to `.github/workflows/ci.yml` (build, clippy,
   fmt, test; no GPU steps)
2. [ ] R1/D1: transition engine — dual-stream crossfade on kira clocks landing
   on the incoming track's section boundary; keyframed materialize settling on
   the first downbeat; close-mood morph-in-place path
3. [ ] R6: layout seed in prop placement; extend pin to `{mood, seed}`;
   "Build a different world" re-rolls the seed
4. [ ] R3/R4/D2: `OverlayStack` + per-overlay modules + action messages (fold
   into the next UI change)
5. [ ] R5: `TrackId` keyed on content hash; unblock queue reorder
6. [ ] R9: `xtask bundle` when distribution nears; R7 LOD with the next pack

## 8. Remediation status (same day)

All actionable findings were fixed the day of the review:

| # | Status | Commit |
|---|---|---|
| R1 | **Fixed** — dual-stream equal-power crossfade (final `min(6s, 25%)` window), 0.8s palette wash, staggered prop materialize settling on the incoming track's first downbeat; same-mood morphs in place | `1b995a5` |
| R2 | **Fixed** — dedicated localgpt-verse CI job (fmt/clippy/test) | LocalGPT `78d6ede`; now `.github/workflows/ci.yml` |
| R3 | **Fixed** — `dispatch_buttons` + `UiAction` messages + five focused handlers; `overlays.rs` split into 9 modules (largest 353 lines). Bonus: fixed the Resume button not restoring `playback.playing` | `6f674e4` |
| R4 | **Fixed** — `OverlayStack` (Esc pops); Paused stays transport state, queue stays a panel | `6f674e4` |
| R5 | **Mitigated** — `playing` is path-keyed, so queue reorder won't restart the current track; a content-hash `TrackId` remains deferred until dedupe needs it | `1b995a5` |
| R6 | **Fixed** — splitmix64-seeded layouts, per-track default seed, re-roll on "Build a different world", pin stores `{mood, seed}` | `f0b6699` |
| R7 | Deferred by design — LOD/instancing with the next asset-pack expansion | — |
| R8 | **Fixed** — photo mode samples the readback and retries black frames (bounded, single output path) | `baf0118` |
| R9 | **Fixed** — `scripts/bundle.sh` assembles dist/ (release binary + fonts + models from `verse-assets`) | this commit |
| R10 | **Fixed** — PLAN status corrected; transition engine actually built (R1) | `1b995a5` |

## 9. Follow-up findings (2026-07, post-review work)

The §7 action items are all closed. New work since (PLAN.md status has the
full list); findings worth recording:

- **R7 closed:** `VisibilityRange` LOD landed with the pack expansion (7 → 52
  CC0 models, ~13/mood). Props are span-normalized from native dims in the
  manifest (Poly Haven scans span 0.1–92 m — without normalization a 138 m
  cliff could swallow the camera; caught via a black screenshot, fixed by
  tier target spans).
- **The 5k-instance budget is measured, not met:** ~60 fps baseline,
  ~14 fps @ 1k scene-root props, ~9 fps @ 5k (uncapped, release). CPU-side
  scene/entity overhead dominates (draw batching already handles the GPU
  side). Dense packs need real instancing before they ship; ~70–90 props is
  the comfortable ceiling today.
- **Screenshot black frames (R8) are environment-sensitive:** oneshot shots
  flake black ~50% of runs (the in-app Photo retry mitigates). A *consistent*
  black frame means a scene bug (see the cliff above), not the flake — check
  logs before retrying.
- **Track identity (R5) fully closed:** `Track.id` = blake3 content hash =
  sidecar key; import dedupes by it; queue reorder (↑/↓ in the panel) is
  path-keyed-safe.
- **M5 (CLAP) landed behind `ml`:** zero-shot mood from 3-window averaged
  audio embeddings vs precomputed text embeddings; mel frontend validated
  against HF's ClapFeatureExtractor (≤0.5 dB), end-to-end embedding cosine
  >0.995, 5/5 on a small genre panel after prompt tuning. Weights are
  CC-BY-NC — do not ship commercially without clearing that.
- **bevy_gltf extension limits now documented:** no KHR_mesh_quantization,
  no EXT_meshopt_compression (0.19) — offline normalization packs
  uncompressed `.glb` (173 MB for 52 models).

## 11. The performed world (2026-09)

The renderer's post-M7 growth, in one place (PROGRESS.html tracks status):

- **Signal routing.** `analysis.rs` → `Beat` (bpm/pulse/energy + live tap
  bass/highs bands) and `StemLevels` (Demucs curves sampled at the playhead,
  stem value winning over the live band) → world systems. `SectionFeel` is
  the eased per-section control vector (energy shift, motion, beacons,
  particles, scatter); `sync_section_moment` resolves roles against measured
  boundaries and positional song shape as the rule-path fallback.
- **Structure, not just palette.** Whole-song materialize (scatter in the
  intro, mediums in verses, heroes on the chorus), waveform skyline (rim
  stelae from the energy curve), bass-displaced ground mesh, prop
  bob/spin, section-aware Drift camera, horizon world title.
- **The 8-mood continuum.** `BUILTIN_MOODS` doubled with quadrant variants
  that borrow their base's assets (`ASSET_BASE_MOODS`); `Timbre` (sidecar
  centroid) nudges hues and `MoodBlend` mixes the nearest boundary palette.
- **Perf shape.** Ground cover is one merged vertex-tinted mesh at 4× the
  old density (scatter no longer pays per-entity). Hero/medium glTF tiers
  remain scene-root clones — true instancing is the remaining ceiling.
- **Embedding-ranked placement (M5→M6).** The CLAP text tower runs at
  runtime on a background thread (`TextEmbedder` + `sync_asset_embeddings`
  in `world_assets.rs`), embedding each manifest entry's name. Placement
  weights medium-tier counts and the hero fallback by
  `cos(track audio embedding, asset text embedding)`, min-max normalized to
  [0.4, 1.6] — neutral without either signal, and the first world replants
  once when embeddings land. Driven by the *audio* side only: CLAP's
  text↔text direction is off-manifold (measured by
  `ml::tests::text_embedding_probe`), so recipe prose stays keyword-matched.

## 10. The LLM tiers (M7, runtime-verified 2026-09)

The `llm` feature adds a local text LLM — **Bonsai-8B** (Qwen3-architecture
8B chat model, prism-ml, Apache-2.0; we run the Q4_K_M GGUF via mistral.rs
under the opt-in `llm-metal` feature). It is the *imagination layer only*: it
never hears the song and never sees the scene. The division of labor:

```
audio file ─► symphonia decode + DSP (analysis.rs) ─► numbers (BPM, energy,
                                                        sections, mood)
                                                        │
                                                        ▼
                                        Bonsai (llm.rs / agent.rs): text in → text out
                                                        │
                                                        ▼
                                  WorldRecipe JSON / tool-calls → Bevy renderer
```

All guardrails live on the Rust side: `WorldRecipe::clamped()` bounds every
value, serde defaults patch partial output, and any failure degrades to the
rule-derived world.

### The recipe tier (`src/llm.rs`)

Runs once per track (then cached in the sidecar forever).

- **Input** — a two-message chat prompt. System: the WorldRecipe field
  vocabulary (names, enums, ranges) and the taste constraints ("modulate
  WITHIN the given mood, 1–2 biomes, 0–3 landmarks"). User: what the DSP
  measured — `Mood: TIDE GARDENS (index must be 2). Tempo: 96 BPM. Mean
  energy: medium. Sections: 5.`
- **Output** — one JSON object. A real, verified example (model-authored for
  `nightglass.mp3`): `{"world_name":"Tide Gardens", "biomes":[{mood:2,layout:
  "spiral",density:0.7,tint:[1,1,1]}, {mood:1,layout:"grid",…}],
  "landmarks":[{kind:"spire",at:"center",scale:1.5,emissive:0.7},…],
  "atmosphere":{"fog_density":0.4,…}, "section_choreography":[{at_role:
  "chorus",energy_shift:0.3,motion:"active"},…], "particles":{"kind":"spark",
  "rate":0.6,drift:1.2}, "motion_speed":1.5, "density":0.8}`. The renderer
  interprets it (fog band, prop count, landmarks with beacons, per-section
  energy/motion/palette washes, particle field).

Generation is plain instructed-JSON + a lenient balanced-object parse — see
the mistral.rs constraints below.

### The agent tier (`src/agent.rs`)

The world-builder alternative: a tool-calling loop.

- **Input** — a system prompt (song context + "8–16 structures") plus eight
  JSON tool schemas: `spawn_primitive`, `place_asset`, `scatter_field`,
  `modify_entity`, `delete_entity`, `set_light`, `set_environment`,
  `scene_info`. `place_asset`/`scatter_field` speak a two-level vocabulary:
  their enum is the manifest's semantic *kinds* (~19 stable tokens), and the
  host resolves each call to a concrete variant (mood-preferred, rotating), so
  the model can only ask for what exists while diversity scales with the
  pool, not the enum.
- **Output** — a sequence of chat turns, each either tool calls (executed by
  the Bevy-side executor, results fed back, so it can review via
  `scene_info` and iterate) or a closing text description. Verified sessions
  issue 24 tool-calls (the step budget) and cache the command stream as a
  `SceneBuild`, replayed deterministically on revisit — no LLM re-run.

### mistral.rs 0.8 constraints (re-verify on any bump)

Empirically established 2026-09 (M2 Max, 32 GB); the ignored
`llm_generation_probe` test re-checks them:

| Fact | Consequence |
|---|---|
| Q1_0 (native 1-bit) quants don't parse | fetch script ships Q4_K_M |
| 5 GB Q4_K_M exceeds the CPU device map (~7.6 GB free beside the renderer) | `llm-metal` feature (macOS GPU); plain `llm` needs a smaller GGUF |
| `generate_structured` (grammar-constrained) hangs on GGUF — even a two-field schema, 0 tokens over minutes — while plain chat runs ~25 tok/s | recipe tier uses plain generation + lenient parse |

Observed performance: plain chat ~25 tok/s; a full recipe in 20–80 s per
track (idea.md's "< song length" bar); agent sessions ~2.5 min. Known
polish gaps: the model echoes mood names as `world_name` instead of
inventing evocative ones, and agent sessions build to the budget without a
closing description.
