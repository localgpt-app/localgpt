# Building Music-Reactive 3D Worlds from Free Assets in Bevy: Asset Sourcing, Runtime Pipeline, and Semantic Mapping

## TL;DR
- **Ship CC0 as the spine, CC-BY as the curated supplement, and never ship marketplace royalty-free assets (TurboSquid/CGTrader/Fab) in an extractable glTF bundle** — their licenses explicitly forbid distributing assets in open formats where end users can extract them, and a desktop app's `assets/` folder is exactly that. CC0 (Poly Haven, Kenney, Quaternius, ambientCG, Smithsonian/NASA, Poly Pizza CC0 filter) and CC-BY-with-attribution are the only clean paths for a distributed bundle.
- **The Bevy runtime pipeline is production-viable in 0.16/0.17** for glTF loading, GPU-driven rendering, automatic GPU frustum culling, two-phase occlusion culling, and virtual geometry (Nanite-style) — but Draco compression is unsupported, meshopt is partial, animation retargeting is weak, and you should preprocess assets offline (normalize to glb, decompress, atlas) into a vetted manifest rather than loading raw downloads.
- **The "MIR-driven world assembled from a catalog of pre-made 3D assets" concept is essentially greenfield (confidence ~80%)** — precedents (Panoramical, FRACT OSC, Magic Music Visuals, Metagroove, VRChat AudioLink) either use abstract/shader/particle geometry, transform already-placed objects, or are human-parameter-driven, not MIR-driven asset assembly. All primitives (glTF spawn, FFT/beat, CLAP embeddings, procedural placement) are mature and unclaimed for this purpose.

## Key Findings

1. **License clarity splits cleanly into two tiers.** CC0 sources are unconditionally safe for a shipped, extractable bundle. Marketplace "royalty-free" licenses (TurboSquid, CGTrader, and Epic's Fab Standard License) are **categorically unsafe** for your use case because they require assets be embedded in a proprietary, non-extractable format — a requirement a Bevy `assets/` directory of `.glb` files structurally cannot meet.

2. **CC-BY is usable commercially with per-asset provenance tracking**, but Sketchfab carries real "license laundering" risk (assets uploaded by non-owners). De-risk by preferring CC0, recording provenance in a manifest, and archiving downloads (Sketchfab's store moved to Fab; free CC downloads persist but are not guaranteed forever).

3. **CC-BY-SA and GPL are traps analogous to copyleft code licenses.** CC-BY-SA forces derivatives (modified meshes/textures) to be re-shared under CC-BY-SA; GPL-licensed art can arguably infect a bundle. Avoid both for a commercial product, or restrict them to unmodified use with legal review.

4. **Bevy 0.16 shipped GPU-driven rendering + automatic GPU culling + two-phase occlusion culling; 0.15 shipped virtual geometry (meshlets).** These make large asset-populated scenes feasible, but occlusion culling has hardware-specific bugs and is still maturing.

5. **The semantic layer is buildable entirely on-device**: CLAP (LAION, open-source code) for audio↔text embeddings, sentence-transformers ONNX for tag matching, and a small Qwen2.5-class LLM for scene-recipe generation, all via `ort`. The valence/arousal→color/lighting mapping has a deep, quantified psychology literature to draw on.

## Details

### 1. Free 3D Asset Source Catalog

**Tier A — CC0 (ship freely, no attribution, extractable-safe):**

| Source | Scale | Content / quality | Formats | Notes |
|---|---|---|---|---|
| **Poly Haven** | Hundreds of models + HDRIs + textures | High-quality realistic props, environment art, PBR, HDRIs | glTF, HDRI, textures | 100% CC0, no login; artist-vetted, explicitly avoids AI/procedural |
| **Kenney.nl** | **60,000+ assets** (Kenney's "Game Assets All-in-1" download states it "contains more than 60.000 game assets in a single download … 2D sprites, 3D models, audio, fonts and more") | Clean, consistent stylized/low-poly; mix-and-match | glTF/OBJ/FBX, PNG | 100% CC0; one-person operation, extremely consistent style |
| **Quaternius** | Thousands of models | Stylized low-poly, rigged + animated characters/animals/vehicles | glTF/FBX | 100% CC0; cohesive "Quaternius style" |
| **ambientCG** | 2,000+ materials/HDRIs + some models | PBR materials to 8K/16K | textures, some models | Formerly CC0Textures; 100% CC0 |
| **Smithsonian Open Access** | 2,000+ 3D models (of 5.1M items) | Scanned artifacts, natural history, Apollo 11 command module | **glTF + OBJ** | CC0, but you may still need third-party (trademark/publicity) clearance |
| **NASA 3D** | Hundreds | Spacecraft, probes, terrain | OBJ/glTF/STL | Mostly public domain; verify per-asset |
| **Poly Pizza** (Google Poly successor) | ~6,000 hand-picked (of 200GB scraped) | Low-poly, game-ready | glTF/GLB/OBJ/FBX | **Mixed CC0 and CC-BY** — filter by CC0; created to replace killed Google Poly |
| **Icosa Gallery** (Google Poly archival successor) | Restored Poly content | Low-poly, Tilt Brush/Blocks | glTF | Open-source, NLnet/EU funded; has API |

**Tier B — CC-BY (usable with attribution; per-asset provenance):**
- **Sketchfab** (free downloadable filter): 300,000+ CC-BY models + a smaller CC0 cultural-heritage set (~1,700+ from Smithsonian et al.). Attribution and license must "follow the asset everywhere it is used." **Laundering risk is real.**
- **OpenGameArt**: large, mixed CC0/CC-BY/CC-BY-SA/GPL — filter carefully; SA and GPL are traps.
- **Poly Pizza CC-BY portion**, **itch.io CC-BY asset packs**.

**Tier C — DO NOT ship in an extractable bundle (marketplace royalty-free):**
- **TurboSquid Royalty Free License**: explicitly requires the model be "contained in proprietary format that cannot be opened in a publicly available software application and from which the … Product cannot be extracted or decompiled without reverse engineering," and "The software or game has no functionality for end users to import any open 3D file format or export any 3D model." Unity/Unreal are named as handling this automatically; a Bevy `.glb` in `assets/` does not.
- **CGTrader Royalty Free License**: "you must take all reasonable measures to prevent the end user from gaining access to the Product"; redistribution prohibited unless as "Incorporated Product" in a proprietary format.
- **Fab Standard License** (Epic; absorbed Quixel Megascans + Sketchfab store): governed by the Fab EULA. Megascans became paid for most content after 2024; free-claimed Megascans are kept under Fab Standard License. Fab assets are engine-agnostic in principle ("all engines and tools") but the Standard License's redistribution constraints make an extractable bundle risky — treat as Tier C unless a specific asset's terms clearly permit extraction.

**KEY COMMERCIAL ANSWER:** For a distributed desktop app where users can open the `assets/` folder, **only CC0 and CC-BY (with attribution) are clean.** Every major marketplace royalty-free license explicitly conditions redistribution on the asset being non-extractable — which a Bevy glTF asset pipeline cannot satisfy without custom encryption/packing (and even then, TurboSquid/CGTrader require "reverse engineering"-level protection, not trivial obfuscation).

**CC-BY compliance mechanics for a desktop app:**
- Attribution must be user-accessible (an in-app Credits screen + a bundled `CREDITS.txt`/`LICENSES` file). Per-asset credit (name, author, license, URL, modifications noted) is the safe standard; aggregate-by-source is acceptable for CC0 but risky for CC-BY where the license names a specific creator.
- If your app lets users export/share their creations, attribution must reach *their* end users too (Sketchfab's guideline: the license "must follow the asset everywhere").
- **De-risking workflow:** (1) prefer CC0 wherever quality suffices; (2) maintain a vetted asset manifest (JSON) recording source URL, author, license, download date, SHA-256, and any modifications; (3) archive the original download; (4) for Sketchfab CC-BY, spot-check that the uploader is plausibly the creator (avoid obvious rips of AAA game assets).

### 2. Bevy Runtime Asset Pipeline & Procedural Assembly

**glTF loading (bevy_gltf, 0.16/0.17):** Mature glTF 2.0 loader; a glTF file maps to a Bevy `Scene` you spawn (`SceneRoot(asset_server.load(GltfAssetLabel::Scene(0)...))`). Sub-assets (meshes, materials, `GltfMesh`, `GltfNode`) are individually addressable.
- **KHR extension support is partial.** Bevy tracks ratified KHR extensions in a support table; **KHR_draco_mesh_compression is not supported**, **EXT_meshopt_compression is partial/limited**, ktx2/webp are supported as formats but the extension syntax is not (issue #19104), and KHR_texture_transform is base-color-only (#15310). **Practical implication: preprocess your entire catalog offline** with `gltfpack`/`glTF-Transform` to decompress Draco/meshopt, normalize to `.glb`, quantize, and generate consistent texture formats before shipping.
- **Asset processor:** Bevy has an experimental asset processing/`AssetSaver` pipeline; as of the 0.17 cycle there was active churn ("Delete subassets; only asset processing!"). Treat runtime asset processing as immature; do it in your own offline build step.

**GPU rendering & culling (crate-by-crate maturity):**
- **GPU-driven rendering + automatic GPU frustum culling**: landed 0.16, on by default when hardware supports it. Per Bevy PR #16670: "The GpuCulling component has been removed. GPU culling is now automatically enabled for all cameras if the hardware and platform support it." (Use `NoIndirectDrawing` to opt out.) Per the Bevy 0.16 release notes: "For large scenes that may have tens of thousands of objects, GPU-driven rendering frequently results in a reduction in CPU rendering overhead of 3× or more." **Native, stable.**
- **Two-phase occlusion culling**: added 0.16, GPU-driven, HiZ-based. **Native but has hardware-specific bugs** — Bevy issue #19544 (v0.16.1) reports occlusion culling erroneously culling objects on a Surface Pro iGPU while the reporter's RTX 3060 machine "works fine"; validation errors also reported (#23108). Test on target GPUs; keep a toggle.
- **Virtual geometry / meshlets (Nanite-style)**: `bevy_pbr` meshlet feature since 0.14–0.15, with LOD + streaming ambitions. Powerful for high-poly scenes but still experimental (requires converting meshes to `MeshletMesh`; some wgpu feature gaps like 64-bit atomics). Good for a "hero" high-detail world; overkill for low-poly Kenney/Quaternius kit.
- **GPU instancing**: available via the render API (`custom_shader_instancing` example) and implicitly via multidraw; essential for multiplying assets (forests, crowds). **Native.**
- **LOD**: `VisibilityRange` component supports distance-based LOD swaps; meshlets do continuous LOD. Manual LOD via VisibilityRange is the pragmatic path.

**Procedural scene composition crates (license / nativeness / churn):**
- **bevy-scene-hook** (crates.io, MIT/Apache): ad-hoc component insertion into spawned glTF scenes; the idiomatic way to tag/modify loaded assets. Note: hot-reloading of scenes is historically broken in Bevy; the crate documents this. Third-party, mature-ish, low churn. Its pattern is increasingly replaceable by Bevy's built-in scene/observer improvements.
- **Procedural placement:**
  - **wfc** (gridbugs, **MIT**): Wave Function Collapse on arbitrary grids, ~45k downloads; `wfc_image` helper. Mature, but WASM issues reported. `wavefc` (MIT) and `wave-function-collapse` are alternatives; `wfc-rs` wraps a C impl (check its license). `bevy_procedural_tilemaps` provides a Bevy-native WFC tilemap workflow with sockets.
  - **noise-rs** and **fastnoise-lite** (crates.io, **MIT**): coherent noise for density fields, terrain, warping. fastnoise-lite is `no_std`-capable, f32/f64. Both native-friendly, stable.
  - **Poisson-disk sampling**: Rust crates exist (`fast_poisson` and others) for even, non-overlapping asset scatter — verify each crate's license (typically MIT/Apache).
  - **L-systems / grammar-based**: no dominant Bevy-native crate; roll your own or use generic Rust L-system crates.
- **Terrain:**
  - **bevy_terrain** (kurtkuehnert, dual MIT/Apache): UDLOD + Chunked Clipmap, large-scale terrain from a bachelor thesis. **Early development, API unstable**, no built-in physics/collision integration. Good foundation, needs work.
  - **bevy_generative** (crates.io): real-time procedural maps/textures/terrain/planets, integratable into Bevy. Younger.
  - Many hand-rolled height-map + `Collider::from_bevy_mesh` approaches exist (rapier for physics).

**Runtime asset modification:**
- **Material/shader swapping**: `StandardMaterial` is a mutable component; swapping base color, emissive, metallic/roughness on loaded glTF at runtime is trivial and cheap. This is your primary "mingling/retargeting" lever — recolor and re-light CC0 kits to unify disparate assets into a coherent scene.
- **Mesh manipulation**: `Mesh` assets are CPU-editable (vertex buffers) but editing per-frame is expensive; prefer **WGSL vertex displacement** in a custom material for music-reactive deformation of loaded meshes (feed FFT/onset/energy as uniforms). This is the clean path for beat-synced mesh pulsing.
- **Skeletal animation retargeting**: **This is the weakest area.** Bevy plays any clip on any armature if bone names match (`AnimationTargetId` is a hash of the bone name path), so same-rig retargeting works, but cross-rig retargeting (different skeletons/rest poses) is problematic — documented issues with position tracks, bone twist, and rest-pose mismatch (#15612). `bevy_animation_graph` (third-party, young, no API stability guarantee) adds graphs, state machines, a visual editor, and *basic* retargeting via bone-path overrides. **Recommendation: constrain to CC0 assets that share a rig (e.g., Quaternius/Mixamo-compatible), or avoid heavy character animation in v1.** `AnimationGraph` serialization changed 0.16→0.17 (drops AssetId fallback).

### 3. Semantic Music→Scene Mapping (the AI layer)

**Precedents for audio→geometry mapping:**
- **Beat Saber level generators (Beat Sage / DeepSaber)**: two neural nets — one predicts *when* to place blocks from the audio (à la Dance Dance Convolution), one predicts *what* block. Uses MFCC features. Notably, Beat Sage "doesn't reason about beats the same way humans do and … does not know the tempo." Confirms onset/energy-to-event mapping is a solved pattern.
- **Audiosurf / Audioshield / Beat Hazard**: intensity/energy-driven; "fine to be messy" because gameplay differs — analogous to your morphing-world tolerance for imprecision.
- **Panoramical**: 18 parallel "audio-visual dimensions" per scene, where one control simultaneously drives a visual parameter and a sound parameter — the canonical "isomorphic" mapping, but human-driven, not MIR-driven.

**Emotion→visual-attribute literature (well-established and quantified):**
- The landmark systematic review is Jonauskaitė & Mohr, "Do we feel colours? A systematic review of 128 years of psychological research linking colours and emotions," *Psychonomic Bulletin & Review* (2024/25, Univ. of Lausanne): "132 relevant peer-reviewed articles published in English between 1895 and 2022 … 42,266 participants from 64 different countries," finding systematic color↔affect correspondences "driven by lightness, saturation, and hue ('colour temperature')": light/dark → positive/negative valence; red/yellow → high arousal; blue/green → low arousal/calm.
- Corroborating studies: higher brightness → higher valence; higher chroma/saturation → higher arousal; red hue → arousal/dominance. A VR-workspace study found red → anxious/high-arousal, yellow → happy, blue → calm/low-arousal.
- **Design implication:** map valence→lightness/brightness + hue family, arousal/energy→saturation/chroma + light intensity + motion speed + asset density, tempo→animation/camera speed and onset cadence, stems→layered subsystems (bass→terrain/scale, drums→onset-triggered spawns/pulses, vocals/melody→lighting/color, harmony→fog/atmosphere).

**Local models for semantic asset selection:**
- **CLAP** (Contrastive Language-Audio Pretraining): LAION-AI CLAP (Wu et al., ICASSP 2023, github.com/LAION-AI/CLAP) is described as "an open-source CLIP-style dual-encoder model with 158M parameters. The audio encoder uses pretrained HTSAT-tiny … The text encoder uses RoBERTa-base," trained on LAION-Audio-630K ("633,526 audio-text pairs") plus AudioSet. Code is open-source (LAION-AI/CLAP repo). ONNX weights exist (e.g., `Xenova/larger_clap_music_and_speech`) — runnable via `ort`. Use CLAP to embed both the music (or MIR-derived text descriptors) and your asset tags into a shared space; nearest-neighbor to select assets. MS-CLAP (Microsoft) is an alternative. **Separate weights license from code license** — verify the specific checkpoint's terms before commercial ship.
- **sentence-transformers (ONNX)**: for matching MIR-derived mood/genre text to asset tag strings; small (tens–hundreds of MB), fast on CPU via `ort`.
- **Offline asset index**: precompute an embedding per asset from its tags/name/thumbnail-caption; store in a vector index (even a flat cosine-similarity table for a few thousand assets is instant). This is the "vetted manifest + embeddings" artifact.

**Small local LLM for scene recipes (Qwen2.5-class):**
- Feasible for **non-real-time planning** (once per song, or per structural section), not per-frame. Use it to turn MIR descriptors ("128 BPM, high energy, dark/aggressive, prominent drums, minor key") into a structured scene recipe (JSON scene graph: biome, palette, asset categories + counts, lighting rig, fog, camera path).
- **Structured output**: use grammar-constrained decoding (llama.cpp GBNF grammar or JSON schema constraints) to force valid scene-graph JSON. Latency budget: a 3B-class model emitting a few hundred tokens is ~1–3s on a consumer GPU — fine for a pre-analysis pass or a between-sections planner, not for beat-rate changes.

**Real-time continuous morphing:**
- **Two-tier architecture**: (1) an offline/upfront **pre-analysis pass** over the whole song (structure segmentation, per-section mood/energy/tempo via your existing Beat This!/PANNs/musicnn/Demucs stack) that produces a **scene timeline** (keyframes with target scene states); (2) a **real-time modulation layer** driven by live FFT/onsets that interpolates between timeline states and adds beat-synced micro-reactions.
- **Avoiding pops**: crossfade asset swaps (spawn new, fade in via material alpha/dither while fading out old), interpolate scalar parameters (fog density, light color temperature, camera position) with eased curves, quantize major transitions to bar/section boundaries, and stream/preload assets for the *next* section during the current one.
- **Precompute vs stream**: precompute the timeline upfront (you have the whole file — this is a visualizer, not a live-input tool by default); stream only the live FFT/onset reactivity. This eliminates most latency and lets the LLM planner run ahead of playback.

### 4. Reference Architecture & Precedent Findings

**Greenfield assessment (confidence ~80%, HIGH):** No shipping product or known open-source project assembles a *coherent morphing 3D world from a catalog of discrete pre-made 3D assets driven by MIR features*. Adjacent precedents and why they differ:
- **FRACT OSC**: music-driven but hand-authored fixed world, not asset-assembly (strong no-match).
- **Panoramical**: morphs landscapes, but human-controller-driven parameter morphs of hand-built abstract scenes, not MIR-driven, not asset-catalog.
- **Rez Infinite / Thumper / Auditorium / Drift**: on-rails rhythm games, fixed authored levels, co-composed audio-visuals.
- **Magic Music Visuals** (closest *commercial* tool): imports 3D models and drives them by audio/MIDI/OSC with per-stem reactivity ("your visuals can react differently to every individual instrument"), but it's a VJ compositor applying transforms to placed media, not autonomous world assembly.
- **Metagroove** (itch.io, closest *artifact*): imports model libraries + music ("Import your own models or free models from the web … A large amount of built in 3d models"), but launched without music-reactivity (a dev-comment afterthought) and is a manual visual toy.
- **Synesthesia / Plane9 / VZX**: shader/particle/scene-based abstract visualizers, not asset worlds. **Beatsee/Banger.Show** (browser, 2024–26) do beat-detection + 3D templates but are music-video exporters, not explorable worlds.
- **Web (Three.js) repos**: single-object/particle FFT reactivity; one loads Sketchfab glTF but as a bouncing demo.
- **VRChat AudioLink** (github.com/llealloo/audiolink): the de-facto standard — "analyzes and processes in-world audio into many different highly reactive data streams" exposed to shaders/Udon — but drives *already-placed* objects/shaders, not runtime asset-world assembly. Runtime spawning to music was tried and "scrapped before release as they just don't work well in VR."
- **Resonite ProtoFlux**: can spawn objects + has audio nodes, but audio-reactive DSP is a requested/partial feature; no shipping audio→asset-world found.

The primitives (glTF spawn in Bevy/Three/Unity/Godot, FFT/beat detection, CLAP embeddings, WFC/noise/Poisson placement) are all mature and unclaimed for this specific intersection. **Caveat**: the VRChat/Resonite space is large and under-documented — confidence that *no* obscure community experiment exists is lower (~60%); and because adjacent tools abound, competitors could emerge quickly.

**Recommended reference architecture:**
1. **Offline asset curation & indexing** (build-time): scrape/download CC0 (+ vetted CC-BY) → normalize with `glTF-Transform`/`gltfpack` to `.glb`, decompress Draco/meshopt, quantize, atlas textures, generate LODs → tag each asset (category, style, palette, poly count) → embed tags with CLAP/sentence-transformers → emit a signed manifest (provenance + license + hash + embedding).
2. **Song pre-analysis pass** (load-time, ~seconds): your existing stack (Symphonia decode → realfft → Beat This! beats, PANNs/musicnn mood-genre, Demucs stems) → structure segmentation → per-section descriptor vector.
3. **Scene timeline planner** (load-time): rule-based mapper (valence/arousal→palette/lighting via the color-emotion tables; energy→density; tempo→speed) OR Qwen2.5 with GBNF-constrained JSON output → a keyframed scene timeline (biome, palette, asset set + counts, lighting rig, fog, camera path). Select concrete assets by CLAP nearest-neighbor against the manifest.
4. **Runtime assembler** (Bevy): spawn scenes via `SceneRoot`, tag via bevy-scene-hook, place via WFC/Poisson/noise, multiply via GPU instancing, LOD via VisibilityRange, cull via automatic GPU + occlusion culling.
5. **Continuous modulation layer** (real-time): live FFT/onsets (realfft) → WGSL uniforms for vertex displacement + material params (color temp, emissive, fog), transform animation, bevy_hanabi particles, eased interpolation between timeline keyframes, bar-quantized asset crossfades.

**Performance budgets (Bevy, consumer GPU, 60fps):**
- With GPU-driven rendering + instancing, tens of thousands of instanced objects are feasible; Bevy's 0.16 notes explicitly target "scenes that may have tens of thousands of objects" with "a reduction in CPU rendering overhead of 3× or more."
- Prefer low/mid-poly CC0 kits (Kenney/Quaternius) for density; reserve high-poly (Poly Haven/Smithsonian scans) as hero objects, optionally via meshlets.
- VRAM is the ceiling for large libraries (streaming meshlets help; 8–12GB VRAM is the realistic consumer target). Keep the *active* working set small; stream next-section assets.
- Mobile/reduced: smaller asset sets, baked lighting, no occlusion-culling reliance, fewer instances, aggressive LOD.

## Recommendations

**Stage 0 — Legal foundation (do first):**
- Adopt a **CC0-first policy**. Build the initial catalog exclusively from Poly Haven, Kenney, Quaternius, ambientCG, Smithsonian/NASA CC0, and Poly Pizza (CC0 filter). This eliminates attribution UI work for v1.
- **Ban Tier C entirely** (TurboSquid/CGTrader/Fab royalty-free) from any shipped bundle. Threshold to revisit: only if you implement genuine encryption-at-rest *and* the specific license permits it (none of the three clearly do for open-format assets).
- Build the **manifest schema now** (source, author, license, URL, date, SHA-256, modifications, embedding). This is your audit trail and your CC-BY compliance engine.

**Stage 1 — Vertical slice (prove the loop):**
- Offline-normalize ~50–100 CC0 assets to `.glb` via `glTF-Transform`. Hand-tag them.
- Hard-code one rule-based mapping (energy→instance density, valence→palette via the color-emotion tables, tempo→animation speed) and one song's pre-analyzed timeline.
- Implement WGSL vertex displacement + `StandardMaterial` mutation driven by live realfft FFT. **Validation threshold:** 60fps with ~5,000 instanced objects on a mid-range GPU (RTX 3060-class), no visible pops on section transitions.

**Stage 2 — Semantic selection:**
- Add CLAP (ONNX via `ort`) for audio/tag embedding and nearest-neighbor asset selection. **Validation threshold:** blind A/B where CLAP-selected assets beat random selection for "fits the music" on a small listener panel; per-scene selection < 50ms.
- Verify the specific CLAP checkpoint's *weights* license permits commercial use before shipping.

**Stage 3 — LLM planner + procedural placement:**
- Add Qwen2.5-class scene-recipe generation with GBNF/JSON-schema-constrained output; add WFC (`wfc`, MIT) or `bevy_procedural_tilemaps` for structured layouts and Poisson/noise for organic scatter. **Validation threshold:** planner emits valid scene-graph JSON 100% of the time (grammar-enforced); full-song plan generated in < song length so it stays ahead of playback.

**Stage 4 — Scale & polish:**
- Add VisibilityRange LOD, meshlets for hero assets, occlusion culling (behind a per-GPU toggle given the known bugs), streaming of next-section assets. **Validation threshold:** 60fps with 20k+ instances + occlusion culling on target GPUs with zero validation errors across your test hardware matrix.

**Defer:** cross-rig animation retargeting (constrain to shared-rig CC0 assets instead); AI text-to-3D generation (note as a future option only — licensing of AI-generated 3D is unsettled and quality/latency are not there for real-time).

## Caveats
- **License terms change.** Fab/Quixel shifted dramatically in 2024–2025 (Megascans went mostly paid); Sketchfab's store moved to Fab. Re-verify Tier C terms and archive Tier A/B downloads; a link is not a license.
- **CC0 is copyright-only.** Smithsonian/NASA scans may still carry trademark, publicity, or third-party rights; scanned real-world branded objects can be a trap.
- **Sketchfab laundering risk is unquantified** — treat CC-BY Sketchfab assets as needing human vetting, not bulk scraping.
- **Bevy churn is real.** 0.16→0.17 had breaking changes (AnimationGraph serialization, GPU culling components, scene despawn semantics). Pin versions; budget for migration each release. Occlusion culling and virtual geometry are experimental with hardware-specific bugs.
- **Animation retargeting is genuinely weak in Bevy** — the single biggest technical risk if your worlds need diverse animated characters from mixed sources.
- **The greenfield claim is ~80% confident**; the VRChat/Resonite space is under-documented (~60% that no obscure experiment exists), and adjacent commercial tools (Magic Music Visuals, Beatsee) mean the concept is a natural next step others could ship.
- **CLAP/LLM weights licenses ≠ code licenses.** Verify each model checkpoint's commercial-use terms independently; code being Apache/MIT does not mean the weights are.
- Numbers like asset counts are provider-reported and fluctuate; treat as order-of-magnitude.