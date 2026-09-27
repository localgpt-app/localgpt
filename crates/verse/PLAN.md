# LocalGPT Verse — Backend Plan: Libraries & Implementation Steps

The UI-first milestone is done: every screen from the design spec runs over a
placeholder world, driven by a **simulated** transport and beat
(`src/playback.rs`). This plan chooses the libraries and sequences the work to
replace the simulation with the real thing described in `idea.md`:

> decode local music → analyze it (beats, sections, energy, mood) → imagine a
> world per song from a catalog of free 3D assets → react live, comfortably.

## 0. Constraints that drive every choice

1. **The seam is already built.** The UI reads two resources —
   `Playback { queue, current, elapsed, sections, playing }` and
   `Beat { bpm, phase, pulse, energy }` — and the theme system takes a mood
   index. The backend's only job is to feed these with real values. Nothing in
   `hud.rs` / `overlays.rs` / `theme.rs` should need to change.
2. **License: Apache-2.0 app.** No GPL dependencies (rules out `aubio`,
   `bliss-audio`, essentia). MPL-2.0 (file-level copyleft, e.g. Symphonia) is
   fine. Shipped *assets* must be CC0-first, CC-BY with attribution — the
   Credits screen already renders per-asset attribution; the manifest (§3)
   becomes its data source.
3. **Design-spec timing is a hard requirement**, not polish:
   audio starts at 0s; geometry settles **on the first downbeat**; song
   crossfades run 6–10s and land **on a section boundary** of the incoming
   track; progress notches **are** the section boundaries; every effect obeys
   the Comfort gates (reduce-flashing caps pulses at 2 Hz, gentler-motion
   swaps movement for opacity). This forces a **sample-accurate playback
   clock** and an **ahead-of-playback analysis pass**.
4. **On-device only.** "Nothing is uploaded, ever" is in the onboarding copy.
   All analysis and ML runs locally.
5. **Format promise:** onboarding says `MP3 · FLAC · WAV · OGG · AIFF`.
6. **Monorepo consistency:** gen already ships cpal audio; localgpt-core
   already ships ONNX inference (fastembed/ort). Prefer the same foundations.

## 1. Library choices

### 1.1 Playback & audio I/O

| Concern | Choice | Why | License |
|---|---|---|---|
| Decode | **symphonia** | Pure Rust; MP3/FLAC/WAV/OGG(Vorbis)/AAC/ALAC, AIFF in recent releases (verify the `aiff`/riff feature at pin time). Also what kira uses internally. | MPL-2.0 |
| Playback engine | **kira** (directly, *not* bevy_kira_audio) | Game-audio mixer on cpal with the three features the spec demands for free: **precise clocks** (schedule a start/crossfade on a musical boundary), **tweens** (6–10s equal-power crossfades, pause "time-dilation" as a tween on a track volume/rate), and **custom `Effect`s** (our FFT tap inside the audio thread). Symphonia loading built in. Engine-agnostic → immune to Bevy version lag. | MIT/Apache-2.0 |
| Device output | cpal (transitively via kira) | Already proven in LocalGPT's `crates/gen` on the same targets. | Apache-2.0 |
| Tags/metadata | **lofty** | Title/artist/duration for the queue + library rows. | MIT/Apache-2.0 |
| Folder import | **rfd** (native folder picker) + **walkdir** | "Choose your music folder…" opens the OS picker (spec 1j). rfd must run on the main thread on macOS — call it from a Bevy system, or use `AsyncFileDialog`. | MIT |
| Loudness | **ebur128** | EBU R128 integrated loudness + short-term envelope: volume normalization across the queue and a robust energy curve. | MIT |

**Rejected:** `rodio`/`bevy_audio` (no clocks/scheduling — can't land a
crossfade on a beat), `bevy_kira_audio` (tracks Bevy releases; we'd be blocked
on 0.19/0.20 lag — we own the integration seam anyway), hand-rolled
cpal mixer (gen's pattern, but ~1–2k lines of decode/ring-buffer/seek/
crossfade code that kira provides; keep as fallback if kira's position
granularity disappoints), `aubio`/`bliss-audio` (GPL).

### 1.2 Analysis (MIR)

Two paths, both local:

| Concern | v1 (pure Rust, in-house) | Upgrade (ONNX via ort) |
|---|---|---|
| FFT | **realfft**/rustfft (MIT/Apache) | — |
| Resample to analysis rate | **rubato** (MIT) | — |
| Onsets & live pulse | Spectral-flux novelty (per-band, ~30 lines on realfft) | — |
| Tempo + beat grid | Autocorrelation tempo + Ellis dynamic-programming beat tracking over the onset envelope (well-documented, ~200 lines, good on pop/electronic) | **Beat This!** exported to ONNX (SOTA; CPJKU code is MIT — verify weights terms) |
| Sections (the notches) | Foote novelty on a self-similarity matrix of chroma/mel features; peaks = boundaries | (same, better features from CLAP embeddings) |
| Key/mode (valence input) | Chroma + Krumhansl-Schmuckler correlation | — |
| Mood / semantic tags | Rule proxies: tempo, mode, loudness dynamics, spectral centroid → valence/arousal (the color-emotion mapping in `idea.md`) | **CLAP** (LAION, ONNX from HF, e.g. `Xenova/larger_clap_music_and_speech`): zero-shot mood tags **and** the audio↔text embedding reused for asset selection — one model, two jobs. Verify checkpoint license before shipping (idea.md caveat). |
| ML runtime | — | **ort** (Apache/MIT) — same runtime family localgpt-core already ships via fastembed. Feature-gate: `ml`. |
| Stems (Demucs) | **Deferred.** Per-band energy (bass/mid/high from the live FFT) approximates stem reactivity at ~zero cost | Revisit post-M6; htdemucs ONNX export is awkward and heavy |

Cache keying: **blake3** content hash (CC0/Apache-2.0). Cache dir via
**dirs** (`~/Library/Application Support/localgpt-verse` / XDG equivalent).
Analysis artifacts serialize with serde to one JSON/RON sidecar per track.

### 1.3 Worlds & assets

| Concern | Choice | Why | License |
|---|---|---|---|
| Runtime loading | `bevy_gltf` (built-in `SceneRoot` + `GltfAssetLabel`) | No new dep. **No Draco/meshopt at runtime** — normalize offline (idea.md §2). | — |
| Offline normalization | `gltf-transform` / `gltfpack` CLIs, wrapped in a repo `xtask` (not shipped) | Decompress, quantize, normalize to `.glb`, emit the manifest | tooling only |
| Asset manifest | serde JSON: `source, author, license, url, date, blake3, tags, embedding` | Legal audit trail (idea.md Stage 0) **and** the live data source for the Credits screen (replaces its hardcoded rows) | — |
| Scatter/placement | **noise** (already in gen) + golden-angle/jittered-grid scatter we already have; **fast_poisson** when overlap matters | Blockout-first: hero / medium / ground-cover tiers | MIT/Apache |
| LOD / perf | `VisibilityRange` (built-in), GPU-driven rendering (automatic ≥0.16) | 60 fps @ ~5k instances on a mid GPU is the M6 budget | — |
| Structured layouts | **wfc** (gridbugs) | Only at M7 — organic scatter first | MIT |
| Scene recipes (LLM) | Rule-based mapper first; later **llama-cpp-2** + GBNF *or* optional Ollama HTTP | Deferred to M7; grammar-constrained JSON recipes per idea.md Stage 3 | MIT |

Asset sources for the starter pack (Stage-0 policy): **CC0 only** — Kenney,
Quaternius, Poly Haven, ambientCG. CC-BY enters later with per-asset
attribution already wired through the manifest → Credits screen.

## 2. Architecture (threads & data flow)

```
                    ┌────────────────────────────────────────────────┐
                    │ Analysis worker (AsyncComputeTaskPool / thread)│
 library scan ────► │ symphonia decode → rubato → realfft            │
 (walkdir+lofty)    │ onsets → tempo/beat grid → sections → key      │
                    │ loudness (ebur128) → [CLAP embed  (ort, `ml`)] │
                    └───────────────┬────────────────────────────────┘
                                    │ TrackAnalysis {beats, sections,
                                    │  energy curve, key, mood, embedding}
                                    ▼ (blake3-keyed sidecar cache)
 Bevy main ◄── Playback/Beat/Theme resources ◄── transport systems
     │                                              ▲
     ▼                                              │ position(), clock ticks
 kira AudioManager ── track: song A ──┐             │
     (audio thread)   track: song B ──┼── FFT-tap Effect (live bands,
                      clocks, tweens ─┘    lock-free → Beat.pulse/energy)
```

- **Clock truth** lives in the audio thread (kira clock + `position()`);
  `Playback.elapsed` becomes a read-model of it. `Beat.phase` = position
  mapped onto the precomputed beat grid; `Beat.pulse` spikes from the grid
  (predictive, tight) with the live onset tap as fallback for unanalyzed
  tracks.
- **Materialize:** world build is keyframed against the clock so geometry
  "settles" exactly on beat[0] ≥ the 1.3–2.4s terrain window.
- **Crossfade:** when A nears its end, B is pre-decoded and scheduled on a
  clock tick that coincides with B's nearest section boundary; equal-power
  tween 6–10s. Close moods (|Δvalence| < 0.2) morph palette in place instead.
- **Comfort gates stay central:** every new signal passes through the existing
  `Comfort` checks (the 2 Hz cap and gentler-motion damping already exist).

## 3. Implementation steps (each lands green: build + clippy + fmt + tests + smoke)

> **Status:** M1–M6 done — real playback, live tap, offline analysis +
> sidecar cache, rule-based mood + pinning, the glTF asset pipeline, and the
> **transition engine** (ARCHITECTURE R1: dual-stream equal-power crossfade in
> the final `min(6s, 25%)`, 0.8s palette wash, staggered prop materialize
> settling on the incoming track's first downbeat; same-mood morphs in place).
> Fonts (Marcellus + Hanken Grotesk, OFL) bundled.
>
> **2026-09 follow-up** (all on `main`):
> - **Kind-based asset vocabulary (manifest v2):** the pack grew 52 → 171
>   CC0 Poly Haven models tagged with 19 semantic kinds (`rock` ×29, `plant`
>   ×20, `machine` ×19, `lamp` ×17, …). The agent's `place_asset` enum is now
>   the *kind* list (a small stable vocabulary a local GGUF holds reliably);
>   the host resolves each call to a concrete variant — mood-neighbourhood
>   preferred, rotating so repeats differ — and records kind + file in the
>   `SceneBuild`, so replay stays exact. New `scatter_field` tool: one call
>   scatters ≤48 instances of a kind across a disk (up to four variants
>   cycled, (track,name)-seeded jitter) — the per-token richness multiplier.
>   World placement moved from per-asset counts to per-tier budgets
>   (15/25/27) spent by weighted round-robin: the bigger pool adds variety,
>   not entities. Extended moods layer their own accents over the base
>   quadrant's set instead of all-or-nothing borrowing.

> **2026-07 follow-up** (all on `main`):
> - **Asset pack: 52 CC0 Poly Haven models** (was 7), ~13 per mood incl. full
>   VELVET CIRCUIT + GLASS EXPANSE coverage; `verse-assets` gained
>   `fetch_polyhaven.py` (provenance + reproducibility) and `normalize.py`
>   (offline .glb packing — **uncompressed**: bevy_gltf supports neither
>   KHR_mesh_quantization nor EXT_meshopt_compression, so geometry stays
>   fp32; 173 MB packed). Placement: `VisibilityRange` LOD (R7) + per-mood
>   layout rules (organic spiral / city grid / crystal rings).
> - **Perf validated** (`VERSE_STRESS=N`, uncapped): baseline world ~60 fps;
>   ~1k scene-root props ≈ 14 fps; 5k ≈ 9 fps. The idea.md "5k instances"
>   budget is **not** met via scene-root clones — it needs real instancing
>   (draw-call batching exists; CPU-side scene/entity overhead dominates).
>   Real worlds use ~70–90 props, so the app sits at the 60 fps cap; dense
>   packs want the instancing path first.
> - **M5 done (opt-in `ml` feature):** CLAP (LAION music+speech, Xenova
>   quantized ONNX, 78 MB audio branch via `scripts/fetch-clap.sh`) embeds 3
>   windows/track (rubato → 48 kHz, Slaney log-mel frontend matching HF's
>   ClapFeatureExtractor, ort) → zero-shot mood vs 4 precomputed text
>   embeddings; 512-d embedding stored in the sidecar (asset-selection hook).
>   5/5 on a small genre panel vs 2/5 for the first prompt draft — prompt
>   tuning matters; rule mapper stays the no-model fallback. **License
>   caveat:** CLAP weights are CC-BY-NC — verify before any commercial
>   distribution (PLAN §4); the app runs fine without them.
> - **Track identity:** `Track.id` = blake3 content hash (sidecar key) —
>   import dedupe, reorder-safe, rename-proof (R5 done). Queue panel: rotated
>   now-playing-first view with ↑/↓ reorder buttons.
> - Analysis lookahead deepened (4 tracks) so back-to-back skips land
>   pre-analyzed.
> - **Transport controls filled in:** click/drag **seek** strip over the
>   progress bar + arrow-key scrub (±5s); **previous** (B / media key, restart
>   if >3s in); **MediaTrackNext/Previous/PlayPause**; **volume fader** (−/=)
>   combined with **EBU R128 loudness normalization** toward −14 LUFS
>   (`ebur128` measured in the analysis decode, stored in the sidecar, applied
>   ±12 dB on the kira sub-track). Section notches now refresh on analysis.
> - **CC0 starter music** (`verse-assets/generate_music.py` → 4 original
>   algorithmic ambient tracks, ~4 MB, CC0-1.0): auto-loaded on first run
>   through the same import path as user music, so the app **plays
>   immediately** without a folder pick. `bundle.sh` ships `assets/music/`.
> - **Shuffle + repeat** (queue-panel pills, spec 1l): playback sequences
>   through an explicit order (Fisher–Yates while shuffled, current kept
>   playing on toggle, fresh permutation each pass; `previous` retraces).
>   Repeat Off stops at the end of the order; One replays in place; the
>   crossfade target and HUD "next" always match the mode.
> - **Library rebuilt as the spec-1i music browser**: sidebar (search
>   affordance, Import/Settings, worlds-as-playlists with counts, privacy
>   line) + scrollable `# / TITLE / ALBUM / WORLD / LENGTH` table; row click
>   plays that song and enters its world; Play/Shuffle start the filtered
>   set. The old mood-card grid is gone — world jumping goes through songs.
>
> **M7 remains the deferred tier** (Beat This!/WFC/LLM recipes/Demucs):
> `beat-this` (a 1.0.0 Rust+ONNX wrap of Beat This!) appeared on crates.io
> and is a plausible drop-in, unaudited; WFC is superseded for now by the
> per-mood layout rules above; LLM recipes need a local server (Ollama) +
> model pull — none load-bearing.
>
> **2026-09 — the LLM half of M7 landed** (behind the `llm` feature, mistral.rs
> + a local GGUF — the Ollama-server idea was dropped for the embedded route):
> - **Recipe tier fully consumed:** the `WorldRecipe` the model emits now
>   drives the renderer end-to-end — `world_name` in the HUD eyebrow and pause
>   title (recipe.rs's old "shown in the HUD" claim is finally true), per-
>   section choreography (`section_choreography` → energy shift / motion /
>   palette wash keyed off the measured boundaries via `resolve_section_moments`),
>   fog density + ambient tint + primary-biome tint in the palette wash,
>   biome layout override + secondary-biome accent props in placement,
>   kind-matched landmarks with emissive beacons, and `seed` (pin > recipe >
>   path precedence).
> - **Agent tier made load-bearing:** `place_asset` gives the model the actual
>   asset vocabulary (the tool's enum *is* the manifest, read from disk on the
>   worker thread); lights/environment are name-registered (update-in-place,
>   despawned with the scene — no more leaks); every emissive/light passes the
>   Comfort gates at execution; entities are **track-scoped** (a lookahead
>   session's scene stays hidden until its track is current, so nothing pops
>   into the playing world); the session's closing description is captured in
>   the cached `SceneBuild` and logged. Step budget 12 → 24.
> - **CI now compiles both gated tiers** (`cargo check --features llm` / `ml`
>   in the localgpt-verse job) — the default-feature gate alone let them rot.
>
> **2026-09-12 — runtime-verified with the real model, three defects found &
> fixed:** Bonsai-8B **Q4_K_M** (5.2 GB, Apache-2.0 — license caveat cleared
> for the LLM tier) over a new opt-in `llm-metal` feature: plain generation
> ~25 tok/s, per-track recipes in 20–80 s, agent sessions of 24 tool-calls,
> cached-build replay exercised on track changes. Along the way: (1) the
> original Q1_0 pick doesn't parse in mistral.rs 0.8 — the fetch script now
> defaults to the verified Q4_K_M; (2) the 5 GB model doesn't fit the *CPU*
> device map beside the renderer (~7.6 GB free) — hence `llm-metal` (macOS
> GPU; never in Linux CI); (3) mistral.rs 0.8's grammar-constrained
> `generate_structured` **hangs on GGUF even for a two-field schema** (plain
> chat on the same loaded model is fine) — recipe generation is now plain
> instructed-JSON + a lenient balanced-object parse (serde defaults +
> `clamped()` + rule fallback keep the safety), under a 180 s timeout; the
> `llm_generation_probe` ignored test documents all three. Worker shutdown is
> also bounded now: a `WorkerCancel` flag checked between tracks/passes/turns
> plus a 15 s join with detach backstop.
>
> **2026-09-12/13 — the "world reads as the song" rounds** (all on `main`;
> PROGRESS.html is the tracker):
> - **Section structural verbs** (eased): chorus lights beacons ×1.4 / swells
>   particles, bridge strips/dims, outro sinks scatter — positional fallback
>   without the LLM tier. **Whole-song materialize**: scatter in the intro,
>   mediums through the verses, heroes land exactly on the chorus. **Waveform
>   skyline**: 28 rim stelae sample the energy curve.
> - **Performed world**: prop bob/spin riding bass; section-aware Drift-camera
>   moves; the recipe name as 3D horizon text over the intro.
> - **Eight moods + continuum**: CINDER REACH / MIRAGE CIRCUIT / ABYSS TERRACES
>   / DAWN EXPANSE split quadrants by energy (borrowing base asset sets via
>   `ASSET_BASE_MOODS`); spectral centroid (now in the sidecar) nudges hues;
>   boundary-adjacent tracks blend toward the neighbour world.
> - **Ground as instrument** (bass-driven vertex displacement) and the
>   **merged scatter field**: the scatter tier is one vertex-tinted
>   procedural mesh at 4× density in one draw — 86 entities → 41 props + a
>   180-pebble field.
> - **Stem/band reactivity** (Demucs curves sampled at the playhead into
>   `StemLevels`; the live tap publishes bass/highs envelopes) and **LLM
>   polish**: inventive-name prompt, agent wrap-up nudge, `at_role`
>   section-scoped agent placements, tier status + agent description in the
>   pause overlay.
> - **The M5→M6 hook landed (2026-09-13):** the CLAP text tower now runs at
>   runtime (`TextEmbedder`, background thread) and embeds the manifest;
>   placement weights medium counts + hero fallback by
>   cosine(track audio embed, asset text embed) ([0.4, 1.6], neutral without
>   either signal). Measured caveat: CLAP text↔text similarity is
>   off-manifold (probe: `ml::tests::text_embedding_probe`), so recipe prose
>   stays keyword-matched and only the track's *audio* embedding ranks.
>   CLAP's mood vote covers the base quadrants; the worker folds mean energy
>   into the variant. Known rough edge: `stem-splitter-core` can panic on a
>   background thread when its model can't be fetched — the tier survives
>   off; report upstream.


**M1 — Real playback core.**
Add kira/symphonia/lofty/rfd/walkdir/dirs. Folder import fills `Playback.queue`
from disk (tags via lofty); play/pause/seek/skip drive kira; `elapsed` reads
the kira clock; pause overlay tweens volume (time-dilation you can hear).
*Exit:* onboarding folder-pick → your actual music audibly plays; HUD time,
progress, and queue are real. Beat stays simulated.

**M2 — Live tap.**
Custom kira `Effect`: windowed FFT → per-band energy + spectral flux, pushed
lock-free into `Beat.pulse`/`energy`. Delete the sine-wave energy simulator.
*Exit:* playhead pulse and world glow visibly follow the actual song;
reduce-flashing still clamps to steady.

**M3 — Pre-analysis pass + musical timing.**
Background worker computes `TrackAnalysis` (onsets → tempo → DP beat grid,
Foote sections, key, loudness curve), cached by blake3. Wire: progress notches
= real sections; `Beat.bpm/phase` from the grid; materialize settles on the
first downbeat; crossfades land on section boundaries; next track analyzed
"quietly" during the current one (the queue already promises this).
*Exit:* notches match audible structure on a test set; skip lands a 400ms
smear into a boundary-aligned entry. Unit tests on the DSP (synthetic clicks →
recovered BPM ±2%; boundary detection on constructed signals).

**M4 — Mood → world mapping (rule-based).**
Valence/arousal from key mode, tempo, dynamics, centroid → pick among the four
moods + continuous params (density, motion speed, glow ceiling) through the
existing `Theme`/`WorldIntensity` plumbing. "Build a different world" re-rolls
the seed, keeps the mood.
*Exit:* an aggressive track and a calm track land in audibly-fitting worlds
deterministically (same file → same world).

**M5 — CLAP embeddings (`ml` feature).**
ort + CLAP ONNX: zero-shot mood tags + stored 512-d embedding per track.
Mapping upgrades from rules to nearest-neighbor over tagged palettes; the
embedding is the hook M6 reuses for asset selection.
*Exit:* blind A/B where CLAP mood beats the rule mapper on a small panel;
graceful fallback to M4 rules when the feature/model is absent.

**M6 — Asset pipeline v1.**
`xtask normalize-assets`: CC0 starter pack (~50–100 Kenney/Quaternius/Poly
Haven models) → normalized `.glb` + manifest. Runtime: per-mood asset sets,
hero/medium/scatter placement (poisson + noise), `VisibilityRange` LOD;
drifting primitives become real props. Credits screen reads the manifest.
*Exit:* 60 fps with ~5k instances on a mid GPU; Credits shows real
attribution; `assets/` passes a `cargo deny`-style license audit.

**M7 — Deferred tier.**
Beat This! ONNX upgrade; WFC structured layouts; grammar-constrained LLM scene
recipes (llama-cpp-2 or optional Ollama); Demucs stems. Each behind a feature
flag, none load-bearing.

**Cross-cutting from M1:** add a localgpt-verse job to CI (build/clippy/fmt/test,
`VERSE_SMOKE` needs a GPU—keep it local-only); `cargo deny` for license
enforcement; keep `default` features light (`ml` opt-in — ort binaries are
large).

## 4. Risks & mitigations

- **kira position/scheduling granularity** for "settle on the downbeat" —
  prototype in M1; fallback is gen-style custom cpal mixer (pattern already in
  LocalGPT's `crates/gen`).
- **AIFF coverage** in kira's symphonia features — if gapped, decode AIFF
  ourselves into a kira static sound (raw frames), or trim the onboarding
  format line.
- **In-house beat tracker quality** on rubato/ambient material — acceptable
  for v1 (worlds tolerate imprecision; spec: "the void just keeps listening"),
  upgrade path is M7 Beat This!.
- **ort binary size / build friction** — feature-gated; rules-only build stays
  pure Rust.
- **CLAP checkpoint license** — verify before shipping weights (idea.md
  caveat); app must run without them.
- **Bevy 0.20 churn** — pin 0.19; migration notes live in the project memory.

## 5. Open questions & decisions

1. **Starter pack location & distribution — decided.**
   - *Source control (dev):* a separate `verse-assets` repo (monorepo
     `*-assets` convention), cloned alongside the app; the app repo keeps only
     fonts + fallback primitives. `assets/models/` is gitignored
     in the app repo and populated from `verse-assets`.
   - *Distribution (ship):* the packaging step **bundles** the vetted pack into
     the app download — **no first-run asset download**. Rationale: preserves
     the "everything local, nothing uploaded" promise the onboarding makes,
     needs no CDN/download-manager/failure-handling, and ~100–300 MB is
     unremarkable for a native desktop app. (If the library ever grows large,
     bundle a ~50–100 MB core pack and make *extra* packs an optional download
     — deferred; not needed for v1.)
   - *Contents:* ~50–100 CC0 `.glb` models (Kenney/Quaternius/Poly Haven) in
     three placement tiers (hero landmarks / medium props / ground scatter),
     tagged per mood (Ember Flats desert rock & dry wood · Velvet Circuit
     abstract neon & chrome · Tide Gardens coral & kelp · Glass Expanse ice &
     crystal), plus PBR ground textures and `manifest.json` (provenance +
     license + tags; feeds the Credits screen). Loaded raw (uncompressed glb,
     no runtime Draco); offline normalization is an optimization, not required
     for v1.
2. **Persistence — decided: JSON sidecars.** One JSON per track in the app
   cache dir, named by blake3 content hash (rename/move-proof), e.g.
   `…/localgpt-verse/analysis/<hash>.json` holding beats/sections/key/loudness/
   valence-arousal/mood. Never writes into the user's music folder. Migrate
   to rusqlite (monorepo standard) only when the library view needs
   search/sort at scale; sidecars then become the import format.
3. **"Keep this world" pinning — decided: confirmed.** The pause overlay's
   "Keep this world — this song will always return here" stores
   `"pinned_world": { "mood": N, "seed": S }` in that track's sidecar; world
   build prefers the pin over the mapper, un-pinning removes the field. The
   button exists in the UI as a no-op today; M4 wires it.
