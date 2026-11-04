---
description: "Install and run LocalGPT Verse, import your music folder, and enable the optional ML analysis tier."
title: "LocalGPT Verse"
---

# LocalGPT Verse

**LocalGPT Verse** is a desktop app that imagines a 3D world for every song — built with [Bevy](https://bevyengine.org/), driven by on-device music analysis, and free inside and out. See [verse.localgpt.app](https://verse.localgpt.app/) for the overview.

*From a fresh clone to your first world in about a minute.*

## Run LocalGPT Verse {#run}

LocalGPT Verse is built with [Bevy](https://bevyengine.org/). All you need is a stable Rust toolchain — there are no services, accounts, or API keys:

```bash
git clone https://github.com/localgpt-app/localgpt.git
cd localgpt
cargo run -p localgpt-verse
```

A window titled **LocalGPT Verse** opens. Verse is a member of the `localgpt` workspace (`crates/verse`), and a song is also a mode of the [desktop app](/docs/app/#song): `cargo run -p localgpt-app -- --song track.mp3` opens one song as its world in the one window, with the same analysis and the same performance.

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

> [!NOTE]
> **Dev shortcut.** `VERSE_IMPORT=<dir> cargo run -p localgpt-verse` imports that folder at startup. Without an import (or an audio device), LocalGPT Verse falls back to a silent simulated transport so the HUD and worlds still run.

## Optional: ML moods {#ml}

The default mood mapper is pure, deterministic DSP. For a research-grade upgrade, build with the `ml` feature and fetch the CLAP audio model once:

```bash
crates/verse/scripts/fetch-clap.sh   # ~78 MB CLAP model (LAION / Xenova ONNX)
cargo run -p localgpt-verse --features ml
```

Each track then gets a 512-dimensional CLAP embedding (three windows averaged) and a zero-shot mood vote, and the embedding is stored in the sidecar where it also ranks which assets a world plants. Without the feature or the model file, the rule mapper runs unchanged.

> [!WARNING]
> **License note.** The CLAP weights are **CC-BY-NC** — fine for personal and research use, not cleared for commercial distribution. The rule mapper is the commercial shipping tier; the app runs fully without `ml`.

## Authored recipes {#recipes}

A track's analysis sidecar can carry a `WorldRecipe` — world name, biomes, landmarks, atmosphere, per-section choreography, particles — that modulates the world *within* the detected mood. Verse applies a recipe it finds there, clamped into its safe ranges, and never writes one: no language model runs in Verse. An agent working on your library's sidecars can author recipes; without one, LocalGPT Verse keeps the rule-derived world verbatim. See [Under the hood](./analysis.md#recipes).

## The asset pack {#assets}

Worlds are furnished from a pack of 171 CC0 Poly Haven models that lives in the separate [`localgpt-world-assets`](https://github.com/localgpt-app/localgpt-world-assets) repository; `scripts/fetch-assets.sh` copies it once per machine into the folder every LocalGPT app reads (`~/.local/share/localgpt/models/pack`), and a sibling checkout is found on its own. The manifest records every model's source, author, license, and a semantic **kind** (19 kinds, from `rock` ×29 variants to `lamp` ×17) — and doubles as the in-app Credits screen. The pack is bundled when Verse is packaged with its `bundle.sh`, so there is nothing to download at runtime.
