---
description: "Install and run LocalGPT Verse, import your music folder, and enable the optional ML and LLM imagination tiers."
sidebar_label: Getting started
---

# LocalGPT Verse

**LocalGPT Verse** is a desktop app that imagines a 3D world for every song — built with [Bevy](https://bevyengine.org/), driven by on-device music analysis, and free inside and out. See [verse.localgpt.app](https://verse.localgpt.app/) for the overview.

*From a fresh clone to your first world in about a minute.*

## Run LocalGPT Verse {#run}

LocalGPT Verse is a standalone Cargo project built with [Bevy](https://bevyengine.org/). All you need is a stable Rust toolchain — there are no services, accounts, or API keys:

```bash
git clone https://github.com/localgpt-app/localgpt-verse.git
cd localgpt-verse
cargo run
```

A window titled **LocalGPT Verse** opens. Because it declares its own `[workspace]`, the project builds the same on its own or checked out inside another Cargo workspace.

## Onboarding {#onboarding}

The first launch walks you through three steps:

1. **Photosensitivity** — before anything pulses or glows, LocalGPT Verse asks about your comfort with flashing light and motion. Your answers set the [Comfort options](./controls.md#comfort), which you can change any time in Settings.
2. **Controls** — the fly/look basics and the keys that matter.
3. **Import** — point LocalGPT Verse at a music folder, or stay with the built-in demo.

Click through it, or press **Skip setup** / <kbd>Enter</kbd> to jump straight into a world.

## Import your music {#import}

**Choose your music folder…** opens a native folder picker. LocalGPT Verse scans the folder — MP3, FLAC, WAV, OGG, M4A, and AIFF — replaces the demo queue with your tracks, and plays them through its real audio engine:

- The HUD clock, progress bar, and queue follow the actual audio; pausing audibly holds its breath, and track ends advance the world.
- A **live audio tap** on the output drives the beat pulse and world glow from the real signal.
- A **background analysis pass** recovers tempo, a beat grid, section boundaries (the notches on the progress bar), an energy curve, and a mood — the world the track lands in.

Analysis results are cached as JSON sidecars, keyed by a blake3 content hash and stored in the app data directory — rename- and move-proof, and **your music folder is never written to**. A track is analyzed once, then never again.

:::note[Dev shortcut]

`VERSE_IMPORT=<dir> cargo run` imports that folder at startup. Without an import (or an audio device), LocalGPT Verse falls back to a silent simulated transport so the HUD and worlds still run.

:::

## Optional: ML moods {#ml}

The default mood mapper is pure, deterministic DSP. For a research-grade upgrade, build with the `ml` feature and fetch the CLAP audio model once:

```bash
scripts/fetch-clap.sh   # ~78 MB CLAP model (LAION / Xenova ONNX)
cargo run --features ml
```

Each track then gets a 512-dimensional CLAP embedding (three windows averaged) and a zero-shot mood vote, and the embedding is stored in the sidecar where it also ranks which assets a world plants. Without the feature or the model file, the rule mapper runs unchanged.

:::warning[License note]

The CLAP weights are **CC-BY-NC** — fine for personal and research use, not cleared for commercial distribution. The rule mapper is the commercial shipping tier; the app runs fully without `ml`.

:::

## Optional: LLM worlds {#llm}

A second opt-in tier puts a small local language model in charge of imagining within a world. Build with `llm` and fetch the model once:

```bash
scripts/fetch-bonsai.sh   # ~5.2 GB GGUF, once for Verse, MD and Gen (any standard Q4_K_M GGUF works)
cargo run --features llm          # Apple Silicon: --features llm-metal
```

The model goes to `~/.local/share/localgpt/models/llm/` (`$LOCALGPT_LLM_DIR` moves it), which [MD](/docs/md) and [Gen](/docs/gen) read too, so a model already fetched for either app isn't downloaded again.

Two tiers ride the model, both per-track and both cached in the sidecar so they never re-run:

- **Recipe** — the model writes a `WorldRecipe` (world name, biomes, landmarks, atmosphere, per-section choreography, particles) that modulates the world *within* the detected mood.
- **Agent** — a tool-calling session that builds a scene entity-by-entity and replays deterministically on every revisit.

Without the feature or the model, LocalGPT Verse keeps the rule-derived world verbatim. See [Under the hood](./analysis.md#llm-tiers) for how both tiers work.

## The asset pack {#assets}

Worlds are furnished from a pack of 171 CC0 Poly Haven models that lives in the separate `verse-assets` repository and syncs into `assets/models/`. The manifest records every model's source, author, license, and a semantic **kind** (19 kinds, from `rock` ×29 variants to `lamp` ×17) — and doubles as the in-app Credits screen. The pack is bundled when the app is packaged, so there is nothing to download at runtime.
