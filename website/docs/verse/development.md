---
description: "Build, test, and package LocalGPT Verse: repo layout, feature flags, smoke and stress tests, the bundle script, and the full license breakdown."
---

# Development

*Build, test, and package LocalGPT Verse — plus the map of what's inside.*

## Repository layout {#layout}

```text
localgpt-verse/
├── src/
│   ├── main.rs           app wiring, schedules, states
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
│   ├── llm.rs            WorldRecipe generation            (feature: llm)
│   └── agent.rs          tool-calling scene builder        (feature: llm)
├── assets/               fonts, models (synced), ml/, llm/
├── scripts/              fetch-*.sh model fetchers, bundle.sh
└── website/              verse.localgpt.app, the landing page
```

Threading is channels-and-atomics only across boundaries (the single shared-state exception is a microsecond `Mutex` around the audio manager). No `unsafe`, no GPL dependencies.

## Feature flags {#features}

| Flag | What it adds | Fetch first |
|---|---|---|
| `(default)` | Playback, DSP analysis, rule-mapped worlds | — |
| `ml` | CLAP zero-shot moods, embedding-ranked placement, Demucs stems | `scripts/fetch-clap.sh` (~78 MB) |
| `llm` | WorldRecipe generation + agent scene builder | `scripts/fetch-bonsai.sh` (~5.2 GB, into the shared `~/.local/share/localgpt/models/llm/`) |
| `llm-metal` | Apple-Silicon GPU inference for the LLM tiers (macOS only) | same as `llm` |

CI compiles both optional tiers so they can't rot, and every tier falls back to the rule-derived world when its model is absent.

## Smoke test {#smoke}

Boots through every screen, then exits — the fastest way to catch a broken overlay:

```bash
VERSE_SMOKE=1 cargo run                    # walk every screen, then quit
VERSE_SMOKE=1 VERSE_SHOT=/tmp cargo run  # + save verse-hud.png / verse-overlays.png
```

:::note

Screenshot readback can intermittently return black frames on some machines (that's why Photo mode retries). A *consistent* black frame is a scene bug, not the flake — check the logs.

:::

## Perf stress test {#stress}

```bash
VERSE_STRESS=5000 cargo run --release   # 5000 extra props, fps log, exit after 30s
VERSE_STRESS=0 cargo run --release      # control run: report only
```

Runs uncapped so the numbers show true frame cost. On the dev machine (2026-07): baseline world ~60 fps, ~1k props ≈ 14 fps, 5k ≈ 9 fps — CPU-side entity overhead dominates, so ~70–90 props is the comfortable ceiling until real instancing lands. Treat dense-pack budgets as measured, not promised.

## Packaging {#bundle}

```bash
scripts/bundle.sh   # assembles dist/: release binary + fonts + models from verse-assets
```

The 171-model CC0 pack lives in the separate `verse-assets` repository; `fetch_polyhaven.py` downloads it and writes the manifest (with a semantic `kind` per asset — 19 kinds), `normalize.py` packs each model to a single uncompressed `.glb` (bevy_gltf supports neither quantized nor meshopt-compressed geometry) and syncs the manifest-referenced set into `assets/models/`.

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

The full review — load-bearing decisions, findings, and the LLM tier details — lives in [ARCHITECTURE.md](https://github.com/localgpt-app/localgpt-verse/blob/main/ARCHITECTURE.md); milestone status in [PLAN.md](https://github.com/localgpt-app/localgpt-verse/blob/main/PLAN.md).

## Licenses {#licenses}

| Part | License |
|---|---|
| LocalGPT Verse code | [Apache License 2.0](https://github.com/localgpt-app/localgpt-verse/blob/main/LICENSE) |
| Marcellus & Hanken Grotesk fonts | SIL Open Font License 1.1 |
| 3D model pack & starter music (`verse-assets`) | CC0 1.0 (public domain) |
| Bonsai-8B LLM weights | Apache-2.0 |
| CLAP audio model weights | CC-BY-NC-4.0 — non-commercial only |

The optional model downloads (`scripts/fetch-*.sh`) carry their own licenses; the asset manifest records every model's source, author, and license, and the in-app **Credits & Licenses** screen renders it.
