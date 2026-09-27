# LocalGPT Verse

A desktop app that imagines a **3D world for every song** — built with
[Bevy](https://bevyengine.org/). See [`idea.md`](idea.md) for the concept
(a music-reactive world assembled from free 3D assets, driven by on-device
music analysis).

This milestone is **UI-first**: the chrome from the LocalGPT Verse design spec is
implemented on Bevy UI over a placeholder, mood-tinted 3D world. The music
analysis + asset-assembly pipeline comes later; for now the transport and beat
are simulated (`src/playback.rs`) so the HUD is already wired to react.

LocalGPT Verse is a **member of the `localgpt` workspace** (`crates/verse`), so it
shares one lockfile, one CI pipeline and the world-format crates with Gen and MD.

**Website:** the landing page for [verse.localgpt.app](https://verse.localgpt.app/)
lives in [`website-verse/`](../../website-verse/) — static HTML/CSS/JS, no build step
(`python3 -m http.server -d website`). The docs are at
[localgpt.app/docs/verse](https://localgpt.app/docs/verse).

## Run

```bash
cargo run -p localgpt-verse
```

Opens a window titled **LocalGPT Verse**. You start in a three-step onboarding
(photosensitivity → controls → import); click through it, or *Skip setup* /
press **Enter** to jump straight into a world.

**Real playback + analysis (PLAN.md M1–M4):** *Choose your music folder…*
scans a folder (MP3/FLAC/WAV/OGG/M4A/AIFF), replaces the demo queue with your
tracks, and plays them through kira/symphonia — the HUD clock, progress bar,
and queue follow the actual audio, pause audibly holds its breath, and track
ends advance the world. A **live audio tap** (a kira effect) drives the beat
pulse and world glow from the real signal, and a **background analysis pass**
(symphonia decode → realfft) recovers tempo, a beat grid, section boundaries
(the progress notches), an energy curve, and a **mood** (which world) — cached
as a JSON sidecar per track (blake3-keyed, in the app data dir; your music
folder is never written to). *Keep this world* pins the mood into that sidecar.
Without an import (or an audio device) the app falls back to the silent
simulated transport. Dev shortcut: `VERSE_IMPORT=<dir>` imports at startup.

**ML moods (PLAN.md M5, optional):** build with `--features ml` and run
`scripts/fetch-clap.sh` once (78 MB CLAP audio model, LAION/Xenova ONNX) —
tracks then get a 512-d CLAP embedding (3 windows averaged) and a zero-shot
mood from it, upgrading the rule mapper; the embedding is stored in the
sidecar for future asset selection. Without the feature or the model file the
rule mapper runs. Note the CLAP weights are CC-BY-NC (see PLAN.md §4).

**LLM worlds (PLAN.md M7, optional):** build with `--features llm` and run
`scripts/fetch-bonsai.sh` once (~5.2 GB GGUF; any standard Q4_K_M GGUF +
`tokenizer.json` works too). The model lives in
`~/.local/share/localgpt/models/llm/` (`$LOCALGPT_LLM_DIR` overrides), the
folder LocalGPT MD and Gen read too, so one download serves all three. Two tiers ride it,
both per-track, both cached in the sidecar so they never re-run:

- **Recipe** — schema-constrained generation of a `WorldRecipe` (world name,
  biomes, landmarks, atmosphere, per-section choreography, particles) that
  modulates the world *within* the detected mood. The recipe's world name
  replaces the mood name in the HUD's now-playing eyebrow and the pause title;
  choreography shifts energy/motion per song section (with optional palette
  washes); landmarks raise kind-matched hero assets with emissive beacons;
  secondary biomes mix in contrasting accent props.
- **Agent** — a tool-calling session that builds a scene entity-by-entity:
  `place_asset` places the curated CC0 models by *kind* (`rock`, `tree`,
  `lamp`, …) — a small stable enum the local model can hold reliably, while
  the host resolves each call to a concrete variant that fits the track's mood
  and rotates so repeats differ — `scatter_field` scatters a whole field of
  one kind in a single call, `spawn_primitive` composes raw shapes, plus
  lights/environment/`scene_info` to review and iterate. Its `SceneBuild`
  records both the asked kind and the resolved file, so it replays
  deterministically on every revisit, and entities are scoped to their track —
  a lookahead session never pops into the playing world; its scene reveals
  when the track becomes current.

Every emissive/light value the agent authors passes the Comfort gates at
execution time. Without the feature or the model the app keeps the
rule-derived world verbatim (CI compiles both tiers so they can't rot).

**Asset pack:** 171 CC0 Poly Haven models live in the separate `verse-assets`
repo (`fetch_polyhaven.py` downloads + writes the manifest with a semantic
`kind` per asset — 19 kinds, from `rock` ×29 variants to `lamp` ×17;
`normalize.py` packs each model to a single uncompressed `.glb` — bevy_gltf
supports neither quantized nor meshopt-compressed geometry — and syncs the
manifest-referenced set into `assets/models/`). Placement holds a per-tier
entity budget (15 heroes / 25 props / 27 cover seeds) and spends it across the
mood's pool by weighted round-robin, so a bigger pack means more variety, not
more entities; each model rescales to its tier's span (hero 7 m / prop 2.5 m /
cover 1 m), culls by `VisibilityRange`, and arranges per mood: organic spiral
(Ember/Tide), city grid (Velvet Circuit), crystal rings (Glass Expanse). The
extended moods (Cinder Reach, Mirage Circuit, Abyss Terraces, Dawn Expanse)
layer their own accents on top of their base quadrant's set. The Credits
screen renders the manifest.

### Controls

| Key | Action |
|-----|--------|
| `W A S D` + mouse | Fly / look (Explore locks the pointer — full 360°, straight up/down) |
| `Space` / `Shift` | Fly up / down (Explore) |
| Scroll | Fly speed (Explore) |
| `F` / click tabs | Toggle Explore / Drift camera (Drift frees the cursor) |
| `E` | Send a pulse |
| `N` | Next track |
| `Tab` | Open / close the queue |
| `L` | Open / close the library (pick a world) |
| `Esc` | Pause (world time-dilates) · resume · close the top overlay |
| `H` | Hide the HUD now |
| `P` | Photo mode — hide the chrome and save a shot to `verse-photos/` |
| `←` / `→` | Adjust world intensity (while paused) |

The HUD follows the spec's "one system, three states": **Visible** while you're
active, fading to a **Minimized** hairline after 4s idle, then **Hidden**
entirely. Any input wakes it. The one variable is the accent, sampled from the
current world's palette.

## Export a world

```bash
VERSE_EXPORT_WORLD=/tmp/verse-worlds cargo run -p localgpt-verse          # as each track becomes current
VERSE_EXPORT_ONLY=1 VERSE_EXPORT_WORLD=/tmp/verse-worlds cargo run -p localgpt-verse   # the whole library, no window
```

writes each track's world in LocalGPT's shared world format
(`localgpt-world-types`): `<track id>.world.json` for the web viewer on
localgpt.world and `<track id>.world.ron` for LocalGPT Gen (the batch form
uses the file stem). The manifest carries the mood palette, a ground and a
sun, the rule-based world every track gets (the props, ground-cover field and
waveform skyline `world_assets` places), the agent's scene build, and a
`soundtrack` section with the track's analysis — tempo, beat grid, sections,
the energy curve, stem envelopes — plus `modulations` that bind entities to
it, so the world performs the song anywhere the format renders. What it never
contains: the audio file (only a track its folder's `music.json` licenses,
like the CC0 starter pack, ships it; personal libraries export with no `path`
and the world performs silently from its curves) and the CLAP embedding. See
`src/world_manifest.rs` for what is not exported yet (section scoping and
particles).

## What's implemented

- **Design system** (`src/theme.rs`) — veils, hairline, radii, type roles, and
  four world "moods" (Dawn Chorus, Neon Surge, Night Bloom, Glass Runner), each
  contributing the single sampled accent.
- **HUD** (`src/hud.rs`) — now-playing cluster, beat-reactive progress with
  section notches, control hints, Explore/Drift toggle, corner affordances, and
  the three-depth fade behaviour.
- **Overlays** (`src/overlays.rs`) — three-step **onboarding** (photosensitivity
  → controls → import), pause (with world-intensity slider), slide-in queue,
  **Settings** (Comfort group with working toggles), **Credits & Licenses**, and
  a **Library** that shows the world moods as selectable cards (`L`).
- **Comfort** — *Reduce flashing* holds the world glow steady (no beat pulse) and
  caps UI pulses; *Gentler world motion* damps the sway. Both apply instantly.
- **Photo mode** (`P` or the pause button) — clears the chrome and saves a clean
  screenshot of the world to `verse-photos/`.
- **World props** (`src/world_assets.rs`) — the manifest-driven glTF pack:
  per-mood placement in three tiers (hero/prop/ground cover), span-normalized
  from each model's native size, with `VisibilityRange` LOD, per-mood
  arrangements, and a rise-in materialize that settles on the first downbeat —
  over the same procedural drifting backdrop (`src/world.rs`) with HDR + bloom,
  Explore/Drift cameras, and pause time-dilation.
- **Queue panel** (`Tab`) — now-playing-first view with ↑/↓ reorder buttons;
  tracks carry a content-hash id (blake3) so imports dedupe and reorder never
  restarts the playing track.

Not yet built (follow-ups): the non-Comfort settings groups (display-only for
now); Beat This! beat tracking and Demucs-driven stem reactivity (the M7
remainders — see [PLAN.md](PLAN.md) for the status of each).

## Fonts

The design uses **Marcellus** + **Hanken Grotesk** (both OFL). They aren't
committed yet — drop the `.ttf`s into `assets/fonts/` and LocalGPT Verse picks them up;
until then it falls back to Bevy's embedded font. See
[`assets/fonts/README.md`](assets/fonts/README.md).

## Smoke test

```bash
VERSE_SMOKE=1 cargo run -p localgpt-verse                    # boots through every screen, then exits
VERSE_SMOKE=1 VERSE_SHOT=/tmp cargo run -p localgpt-verse  # also saves verse-hud.png / verse-overlays.png
```

## Perf stress test

```bash
VERSE_STRESS=5000 cargo run -p localgpt-verse --release      # spawns 5000 prop instances, logs fps, exits after 30s
VERSE_STRESS=0 cargo run -p localgpt-verse --release         # control run: report only, no extra props
```

Runs uncapped (vsync off) so the numbers show true frame cost. On the dev
machine (2026-07): baseline world ~60 fps, ~1k props ≈ 14 fps, 5k ≈ 9 fps —
dense packs beyond that need real instancing (PLAN.md status).

## License

LocalGPT Verse's code is licensed under the [Apache License 2.0](LICENSE). The bundled
fonts (Marcellus, Hanken Grotesk) are under the SIL Open Font License 1.1 — see
[`assets/fonts/`](assets/fonts/). The 3D model pack and the starter music live in
the separate `verse-assets` repository and are dedicated to the public domain
under CC0 1.0. The optional model downloads (`scripts/fetch-*.sh`) carry their
own licenses; the CLAP weights in particular are CC-BY-NC.
