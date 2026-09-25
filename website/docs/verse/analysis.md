---
description: "How LocalGPT Verse turns audio into worlds: the signal-ownership ladder, the DSP analysis pass, CLAP embeddings, stem reactivity, and the local LLM recipe and agent tiers."
---

# Under the hood

*From samples to a staged world: the analysis pipeline, the cache, and the optional imagination tiers.*

## The signal ladder {#ladder}

LocalGPT Verse's signals are owned by a strict ladder, and each rung is a strict upgrade with automatic fallback:

1. **Simulation** owns everything — a silent transport so the app runs with no device and no import.
2. **Live audio** (a kira tap on the output) owns the clock, energy, and onsets once real playback starts.
3. **The analysis grid** owns phase, sections, and mood once the background pass finishes the track.

This is why a missing device, file, manifest, or sidecar can never break the app — each rung only ever *improves* the world. The UI reads three data resources (`Playback`, `Beat`, `Theme`) and survived every backend replacement with zero changes.

The analysis pass itself is offline decode ([symphonia](https://github.com/pdeljanov/Symphonia)) → DSP (realfft): tempo (±0.2% against reference), a beat grid with downbeats, section boundaries, an energy curve, and the quadrant mood — all computed on a worker thread while earlier tracks play.

## The sidecar cache {#cache}

Every result lives in a JSON sidecar keyed by the track's blake3 content hash, in the app data directory:

- Rename- and move-proof — the key is the file's contents, not its path.
- Human-debuggable — open one and read exactly what LocalGPT Verse heard.
- Never touches the music folder.
- No schema migrations; optional tiers (embeddings, recipes, scene builds) are just more fields in the same document.

## CLAP moods & placement {#clap}

Behind the opt-in `ml` feature, a [CLAP](https://github.com/LAION-AI/CLAP) model (ONNX, run via `ort`) upgrades two decisions:

- **Zero-shot mood** — the track's 512-d audio embedding (three windows averaged) is compared against precomputed text embeddings for the moods. The mel frontend is validated against the reference feature extractor (≤0.5 dB; end-to-end cosine >0.995).
- **Embedding-ranked placement** — the CLAP text tower embeds each manifest entry's name on a background thread, and placement weights medium-tier counts and the hero fallback by `cos(track audio embedding, asset text embedding)`, min-max normalized to [0.4, 1.6] — neutral without either signal.

:::note

Only the **audio→text** direction is used: CLAP's text↔text comparisons measured off-manifold, so recipe prose stays keyword-matched.

:::

## Stem reactivity {#stems}

Also under `ml`, Demucs stem separation (`stem-splitter-core`, wrapping htdemucs ONNX, CPU-only) splits each track into stems ahead of playback. The world samples the stem curves at the playhead — a stem value wins over the live band estimate — so bass, drums, and vocals can drive separate subsystems of the world.

## The LLM tiers {#llm-tiers}

The opt-in `llm` feature adds a local 8B chat model (**Bonsai-8B**, Qwen3 architecture, Apache-2.0, run as a Q4_K_M GGUF via mistral.rs — Metal-accelerated under `llm-metal`). It is the *imagination layer only*: it never hears the song and never sees the scene.

```text
audio file ─► symphonia decode + DSP ─► numbers (BPM, energy,
                                            sections, mood)
                                            │
                                            ▼
                            Bonsai: text in → text out
                                            │
                                            ▼
                      WorldRecipe JSON / tool-calls ─► Bevy renderer
```

### Recipe

Once per track, the model receives what the DSP measured and returns one JSON object — a `WorldRecipe`: world name, 1–2 biomes, 0–3 landmarks, atmosphere, per-section choreography, particles. The renderer interprets it: the recipe's world name replaces the mood name in the HUD, choreography shifts energy and motion per section, landmarks raise kind-matched hero assets with emissive beacons, and secondary biomes mix in contrasting accent props.

### Agent

The world-builder alternative: a tool-calling loop over tools like `spawn_primitive`, `modify_entity`, `delete_entity`, `set_light`, `set_environment`, and `scene_info` to review its own work. With the manifest mounted, `place_asset` and `scatter_field` join the set: they place models by **kind** (`rock`, `tree`, `lamp`, … — 19 kinds), a small stable enum the local model can hold reliably, while the host resolves each call to a concrete variant that fits the track's mood and rotates so repeats differ. Sessions run to a 24-tool-call budget and cache as a `SceneBuild` that records both the asked kind and the resolved file, replayed deterministically on every revisit — entities are scoped to their track, so a lookahead session never pops into the playing world.

## Guardrails & fallback {#guardrails}

- All guardrails live on the Rust side: `WorldRecipe::clamped()` bounds every value, serde defaults patch partial output.
- Every emissive and light value the agent authors passes the [Comfort](./controls.md#comfort) gates at execution time.
- Any failure — missing model, malformed JSON, over-budget session — degrades to the rule-derived world. The optional tiers only ever add.

## Model licenses {#model-licenses}

| Component | License | Note |
|---|---|---|
| LocalGPT Verse code | Apache-2.0 | Commercial-friendly |
| Rule mood mapper | Apache-2.0 | The shipping mood tier |
| stem-splitter-core (Demucs) | MIT / Apache-2.0 | Commercial-friendly |
| Bonsai-8B weights | Apache-2.0 | Commercial-friendly |
| CLAP weights (LAION) | CC-BY-NC-4.0 | **Not** cleared for commercial distribution |

:::warning

**Weights ≠ code.** A model's code being Apache/MIT says nothing about its checkpoint. Verify each weight file's terms before shipping anything commercially — the CLAP weights in particular are research-only.

:::
