---
description: "How LocalGPT Verse turns audio into worlds: the signal-ownership ladder, the DSP analysis pass, CLAP embeddings, stem reactivity, and authored recipes."
title: "Under the hood"
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
- No schema migrations; optional tiers (embeddings, stems, an authored recipe) are just more fields in the same document.

## CLAP moods & placement {#clap}

Behind the opt-in `ml` feature, a [CLAP](https://github.com/LAION-AI/CLAP) model (ONNX, run via `ort`) upgrades two decisions:

- **Zero-shot mood** — the track's 512-d audio embedding (three windows averaged) is compared against precomputed text embeddings for the moods. The mel frontend is validated against the reference feature extractor (≤0.5 dB; end-to-end cosine >0.995).
- **Embedding-ranked placement** — the CLAP text tower embeds each manifest entry's name on a background thread, and placement weights medium-tier counts and the hero fallback by `cos(track audio embedding, asset text embedding)`, min-max normalized to [0.4, 1.6] — neutral without either signal.

> [!NOTE]
> Only the **audio→text** direction is used: CLAP's text↔text comparisons measured off-manifold, so recipe prose stays keyword-matched.

## Stem reactivity {#stems}

Also under `ml`, Demucs stem separation (`stem-splitter-core`, wrapping htdemucs ONNX, CPU-only) splits each track into stems ahead of playback. The world samples the stem curves at the playhead — a stem value wins over the live band estimate — so bass, drums, and vocals can drive separate subsystems of the world.

## Authored recipes {#recipes}

The top rung is a `WorldRecipe` in the track's sidecar: world name, 1–2 biomes, 0–3 landmarks, atmosphere, per-section choreography, particles. The renderer interprets it: the recipe's world name replaces the mood name in the HUD, choreography shifts energy and motion per section, landmarks raise kind-matched hero assets with emissive beacons, and secondary biomes mix in contrasting accent props.

Verse reads a recipe; it never writes one. No language model runs in it (the in-process one it had went on 2026-11-01, with the agent tier that built scenes entity by entity). An agent working on a library can author a recipe into a sidecar from what the DSP measured — the sidecar is plain JSON, keyed by the track's content hash — and the world changes on the next analysis.

## Guardrails & fallback {#guardrails}

- All guardrails live on the Rust side: `WorldRecipe::clamped()` bounds every value on read, serde defaults patch partial input.
- Every emissive and light value a recipe asks for passes the [Comfort](./controls.md#comfort) gates at execution time.
- Any failure — a missing sidecar, malformed JSON — degrades to the rule-derived world. The optional tiers only ever add.

## Model licenses {#model-licenses}

| Component | License | Note |
|---|---|---|
| LocalGPT Verse code | Apache-2.0 | Commercial-friendly |
| Rule mood mapper | Apache-2.0 | The shipping mood tier |
| stem-splitter-core (Demucs) | MIT / Apache-2.0 | Commercial-friendly |
| CLAP weights (LAION) | CC-BY-NC-4.0 | **Not** cleared for commercial distribution |

> [!WARNING]
> **Weights ≠ code.** A model's code being Apache/MIT says nothing about its checkpoint. Verify each weight file's terms before shipping anything commercially — the CLAP weights in particular are research-only.
