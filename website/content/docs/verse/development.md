---
description: "Build, test, and package LocalGPT Verse: repo layout, feature flags, smoke and stress tests, the bundle script, and the full license breakdown."
title: "Development"
---

# Development

*Build, test, and package LocalGPT Verse — plus the map of what's inside.*

## Repository layout {#layout}

```text
crates/verse/             # a member of the localgpt workspace
├── src/
│   ├── lib.rs            the app (`run`): wiring, schedules, states; `song_world` for the desktop app
│   ├── main.rs           calls `run`
│   ├── theme.rs          design tokens + the world-mood registry
│   ├── hud.rs            now-playing, progress, hints, three-state fade
│   ├── overlays/         onboarding, pause, queue, settings, credits, library
│   ├── playback.rs       transport signals, the ownership ladder
│   ├── audio.rs          kira playback, live tap, pause/end-of-track
│   ├── analysis.rs       symphonia → realfft: tempo, grid, sections, mood
│   ├── world.rs          procedural backdrop, cameras, time-dilation
│   ├── world_assets.rs   manifest pack, placement, LOD, embeddings
│   ├── ml.rs             CLAP zero-shot mood + embeddings  (feature: ml)
│   ├── demucs.rs         stem separation + curves          (feature: ml)
│   └── recipe.rs         the authored WorldRecipe, applied from the sidecar
├── assets/               fonts, models (synced), ml/

xtask/                    cargo xtask fetch-assets — the asset pack every app shares
```

Threading is channels-and-atomics only across boundaries (the single shared-state exception is a microsecond `Mutex` around the audio manager). No `unsafe`, no GPL dependencies.

## Feature flags {#features}

| Flag | What it adds | Fetch first |
|---|---|---|
| `(default)` | Playback, DSP analysis, rule-mapped worlds | — |
| `ml` | CLAP zero-shot moods, embedding-ranked placement, Demucs stems | `cargo xtask fetch-clap` (~78 MB) |

CI compiles the optional tier so it can't rot, and it falls back to the rule-derived world when its model is absent. There is no language-model tier: a recipe comes from the sidecar, written by an agent.

## Smoke test {#smoke}

Boots through every screen, then exits — the fastest way to catch a broken overlay:

```bash
VERSE_SMOKE=1 cargo run -p localgpt-verse                    # walk every screen, then quit
VERSE_SMOKE=1 VERSE_SHOT=/tmp cargo run -p localgpt-verse  # + save verse-hud.png / verse-overlays.png
```

> [!NOTE]
> Screenshot readback can intermittently return black frames on some machines (that's why Photo mode retries). A *consistent* black frame is a scene bug, not the flake — check the logs.

## Perf stress test {#stress}

```bash
VERSE_STRESS=5000 cargo run -p localgpt-verse --release   # 5000 extra props, fps log, exit after 30s
VERSE_STRESS=0 cargo run -p localgpt-verse --release      # control run: report only
```

Runs uncapped so the numbers show true frame cost. On the dev machine (2026-07): baseline world ~60 fps, ~1k props ≈ 14 fps, 5k ≈ 9 fps — CPU-side entity overhead dominates, so ~70–90 props is the comfortable ceiling until real instancing lands. Treat dense-pack budgets as measured, not promised.

## Packaging {#bundle}

```bash
cargo xtask verse-bundle         # assembles crates/verse/dist/: release binary + fonts + models from localgpt-world-assets
```

The 171-model CC0 pack lives in the separate `localgpt-world-assets` repository (formerly `localgpt-verse-assets`); its `fetch_polyhaven.py` downloads it and writes the manifest (with a semantic `kind` per asset — 19 kinds), `normalize.py` packs each model to a single uncompressed `.glb` (bevy_gltf supports neither quantized nor meshopt-compressed geometry), and the workspace's `cargo xtask fetch-assets` copies the manifest-referenced set into the folder every LocalGPT app reads. The macOS `LocalGPT.app` bundle does not carry the pack; that script fetches it once per machine.

## Architecture at a glance {#architecture}

```text
                 Bevy main thread                        other threads
┌──────────────────────────────────────────┐   ┌───────────────────────────┐
│ UI (hud.rs, overlays/)                   │   │ kira audio thread (cpal)  │
│   reads: Playback, Beat, Theme, Comfort  │   │   StreamingSound decode   │
│   writes: intent resources (Paused, …)   │   │   TapEffect: energy/onset │
├──────────────────────────────────────────┤   │   → AtomicU32 (lock-free) │
│ Transport (audio.rs systems)             │◄──┤                           │
├──────────────────────────────────────────┤   ├───────────────────────────┤
│ Signals (playback.rs advance_playback)   │   │ import scan thread        │
│   ladder: simulation → live tap → grid   │◄──┤   walkdir+lofty → mpsc    │
├──────────────────────────────────────────┤   ├───────────────────────────┤
│ World (world.rs, world_assets.rs)        │   │ analysis worker thread    │
│   procedural ambient + manifest props    │   │   symphonia→realfft→tempo │
└──────────────────────────────────────────┘   │   /sections/energy/mood   │
                                               │   → mpsc, JSON sidecars   │
                                               └───────────────────────────┘
```

The system map, the load-bearing decisions and the measured limits are in [ARCHITECTURE.md](https://github.com/localgpt-app/localgpt/blob/main/crates/verse/ARCHITECTURE.md); [PLAN.md](https://github.com/localgpt-app/localgpt/blob/main/crates/verse/PLAN.md) is the key to the milestone numbers the source cites. Audio runs on the shared `localgpt-world-audio` crate (one kira mixer, FunDSP graphs as sources); Verse's own code keeps its analysis, moods, placement and performance.

## Licenses {#licenses}

| Part | License |
|---|---|
| LocalGPT Verse code | [Apache License 2.0](https://github.com/localgpt-app/localgpt/blob/main/LICENSE) |
| Marcellus & Hanken Grotesk fonts | SIL Open Font License 1.1 |
| 3D model pack & starter music (`localgpt-world-assets`) | CC0 1.0 (public domain) |
| CLAP audio model weights | CC-BY-NC-4.0 — non-commercial only |

The optional model downloads (`cargo xtask fetch-clap`, and the Demucs model `stem-splitter-core` fetches) carry their own licenses; the asset manifest records every model's source, author, and license, and the in-app **Credits & Licenses** screen renders it.
