# LocalGPT Verse

A desktop app that imagines a **3D world for every song** — built with
[Bevy](https://bevyengine.org/): a music-reactive world assembled from free
3D assets, driven by on-device music analysis.

Verse plays the music files on this computer, analyses each track (tempo,
beat grid, sections, energy, mood) and builds its world from that: a
procedural, mood-tinted backdrop with props from a CC0 model pack, under the
chrome from the LocalGPT Verse design spec on Bevy UI. With no audio file or
no audio device the transport and beat are simulated (`src/playback.rs`), so
every screen still runs.

LocalGPT Verse is a **member of the `localgpt` workspace** (`crates/verse`), so it
shares one lockfile, one CI pipeline and the world-format crates with Gen and MD.

**Docs:** [localgpt.app/docs/verse](https://localgpt.app/docs/verse). Verse has no
landing page of its own.

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
folder is never written to). *Keep this world* pins the mood and the layout
seed into that sidecar. With nothing imported (or no audio device) the app
runs its built-in demo queue on the silent simulated transport; the last
folder you chose is imported again at the next start. A bundle
(`cargo xtask verse-bundle`) carries four CC0 starter tracks in
`assets/music/` and imports them at startup. In a dev tree
`cargo xtask fetch-assets` copies them to
`~/.local/share/localgpt/models/pack/music`, which Verse does not open on
its own: `VERSE_IMPORT=<dir>` imports any folder at startup.

**The `ml` tier (PLAN.md M5 and M7, optional):** build with `--features ml`
and run `cargo xtask fetch-clap` once (the CLAP audio and text models and
their tokenizer, LAION/Xenova ONNX, about 207 MB into `assets/ml/`). Tracks
then get a 512-d CLAP embedding (three windows averaged) and a zero-shot
mood from it in place of the rule mapper's; the embedding is stored in the
sidecar and weights which props a world places. The same feature separates
each track into four stems with Demucs (`stem-splitter-core`, which
downloads its model on first use), so the world reacts per stem. Without the
feature or the model files the rule mapper and the live tap's bands run. The
tree labels the CLAP weights CC-BY-NC-4.0; see PLAN.md §4 before
distributing them.

**Authored recipes (PLAN.md M7):** a track's analysis sidecar can carry a
`WorldRecipe` (world name, biomes, landmarks, atmosphere, per-section
choreography, particles) that modulates the world *within* the detected
mood: the recipe's world name replaces the mood name in the HUD's
now-playing eyebrow and the pause title; choreography shifts energy/motion
per song section (with optional palette washes); landmarks raise
kind-matched hero assets with emissive beacons; secondary biomes mix in
contrasting accent props. Verse applies a recipe it finds, clamped into its
safe ranges, and never writes one — no language model runs here (the
in-process one went on 2026-11-01). An agent working on a library's
sidecars can author recipes; without one the app keeps the rule-derived
world verbatim.

**Asset pack:** 171 CC0 Poly Haven models live in the separate
`localgpt-world-assets` repository, as one uncompressed `.glb` each (522 MB;
bevy_gltf reads neither quantized nor meshopt-compressed geometry) beside a
manifest that gives every model a tier, a mood and a semantic `kind` — 19
kinds, from `rock` ×29 variants to `lamp` ×17. `cargo xtask fetch-assets`
copies the files the manifest names from a checkout beside this repository
into the pack every LocalGPT app shares and links `assets/models` to it;
without the pack the worlds are procedural only. Placement holds a per-tier
budget (15 heroes / 25 props / 27 ground-cover seeds) and spends it across
the mood's pool by weighted round-robin, so a bigger pack means more variety,
not more entities. Heroes and props rescale to their tier's span (7 m /
2.5 m) and props cull by `VisibilityRange`; the ground cover is one merged
field of procedural pebbles, four to a seed. Each base mood has its own
arrangement: clusters (Ember Flats), a city grid (Velvet Circuit), terraces
(Tide Gardens), rings (Glass Expanse). The extended moods (Cinder Reach,
Mirage Circuit, Abyss Terraces, Dawn Expanse) arrange like their base and
layer their own models on top of its set. The Credits screen renders the
manifest.

### Controls

| Key | Action |
|-----|--------|
| `W A S D` + mouse | Fly / look (Explore locks the pointer — full 360°, straight up/down) |
| `Space` / `Shift` | Fly up / down (Explore) |
| Scroll | Fly speed (Explore) |
| `F` / click tabs | Toggle Explore / Drift camera (Drift frees the cursor) |
| `E` | Send a pulse |
| `N` / `B` | Next track / previous (restarts the track when more than 3 s in) |
| `←` / `→` | Scrub 5 s back / forward; while paused, move the world-intensity slider |
| `-` / `=` | Volume down / up |
| `Tab` | Open / close the queue |
| `L` | Open / close the library (browse your music by world) |
| `Esc` | Pause (world time-dilates) · resume · close the top overlay |
| `H` | Hide the HUD now |
| `P` | Photo mode — hide the chrome and save a shot to `verse-photos/` |

The media keys (next, previous, play/pause) work too; play/pause freezes the
transport without opening the pause menu, like the HUD's button.

The HUD follows the spec's "one system, three states": **Visible** while you're
active, fading to a **Minimized** hairline after 4s idle, then **Hidden**
entirely (after 12s in Explore, 8s in Drift). Any input wakes it. The one
variable is the accent, sampled from the current world's palette.

## Export a world

```bash
VERSE_EXPORT_WORLD=/tmp/verse-worlds cargo run -p localgpt-verse          # as each track becomes current
VERSE_EXPORT_ONLY=1 VERSE_IMPORT=~/Music VERSE_EXPORT_WORLD=/tmp/verse-worlds cargo run -p localgpt-verse   # a whole folder, no window
```

writes each track's world in LocalGPT's shared world format
(`localgpt-world-types`): `<track id>.world.json` for the web viewer on
localgpt.world and `<track id>.world.ron` for LocalGPT Gen (the batch form
uses the file stem, and without `VERSE_IMPORT` reads `assets/music/`). The
manifest carries the mood palette, a ground and a sun, the rule-based world
every track gets (the props, ground-cover field and waveform skyline
`world_assets` places), and a `soundtrack` section with the track's analysis
— tempo, beat grid, sections, the energy curve, stem envelopes — plus
`modulations` that bind entities to it, so the world performs the song
anywhere the format renders. What it never contains: the CLAP embedding, or
a path to the audio unless the track's folder has a `music.json` that
licenses it (the CC0 starter pack does); a personal library exports with no
`path` and the world performs silently from its curves. See
`src/world_manifest.rs` for what is not exported yet (the materialize rise,
particles and the palette's timbre wash).

## What's implemented

- **Design system** (`src/theme.rs`) — veils, hairline, radii, type roles, and
  eight world "moods" — Ember Flats, Velvet Circuit, Tide Gardens, Glass
  Expanse and their restrained variants Cinder Reach, Mirage Circuit, Abyss
  Terraces, Dawn Expanse — each contributing the single sampled accent.
- **HUD** (`src/hud.rs`) — now-playing cluster, transport buttons, a
  beat-reactive progress bar with section notches that scrubs on click or
  drag, control hints, Explore/Drift toggle, corner affordances, and the
  three-depth fade behaviour.
- **Overlays** (`src/overlays/`) — three-step **onboarding** (photosensitivity
  → controls → import), pause (with world-intensity slider), slide-in queue,
  **Settings** (Comfort group with working toggles), **Credits & Licenses**, and
  a **Library** (`L`): the worlds as filters beside a track table, where a
  row plays that song and enters its world.
- **Comfort** — *Reduce flashing* holds the world glow, beacons and particle
  sparkle steady and stops the playhead's beat pulse; *Gentler world motion*
  damps the sway, the props' bob and the ground waves. Both apply instantly.
- **Photo mode** (`P` or the pause button) — clears the chrome and saves a clean
  screenshot of the world to `verse-photos/`.
- **World props** (`src/world_assets.rs`) — the manifest-driven glTF pack:
  per-mood placement in three tiers (hero/prop/ground cover), heroes and props
  span-normalized from each model's native size, with `VisibilityRange`
  culling, per-mood arrangements, a rise-in materialize timed from the first
  downbeat and a skyline that draws the song's energy curve — over the same
  procedural drifting backdrop (`src/world.rs`) with HDR + bloom,
  Explore/Drift cameras, and pause time-dilation.
- **Queue panel** (`Tab`) — now-playing-first view with ↑/↓ reorder buttons and
  shuffle/repeat pills; tracks carry a content-hash id (blake3) so imports
  dedupe and reorder never restarts the playing track.

Not yet built: the non-Comfort settings groups and three of the Comfort rows
(display-only); anything that reads the world-intensity slider (its value is
saved, nothing uses it); gamepad input (the onboarding lists it); the
Library's search field, the queue's *Clear* and the pause menu's `R` and `Q`
shortcuts (labels only); Beat This! beat tracking and WFC layouts (the M7
remainders, see [PLAN.md](PLAN.md)).

## Fonts

The design uses **Marcellus** + **Hanken Grotesk** (both OFL). Both are
committed in `assets/fonts/` with their license texts; if a file is missing
LocalGPT Verse falls back to Bevy's embedded font, and glyphs the two do not
cover (CJK track titles, for one) come from the system's fonts. See
[`assets/fonts/README.md`](assets/fonts/README.md).

## Smoke test

```bash
VERSE_SMOKE=1 cargo run -p localgpt-verse                    # boots through every screen, then exits
VERSE_SMOKE=1 VERSE_SHOT=/tmp cargo run -p localgpt-verse  # also saves a PNG per screen (verse-hud.png, verse-overlays.png, …)
VERSE_SMOKE=1 VERSE_ONESHOT=1 VERSE_SHOT=/tmp cargo run -p localgpt-verse  # one settled frame: verse-world.png
```

The shots read back the window's frame, which can come back black; see
"Screenshots without a window" in [ARCHITECTURE.md](ARCHITECTURE.md). Other
switches: `VERSE_ASSET_ROOT=<dir>` reads assets from another folder,
`VERSE_PACK=1` mounts a demo world, `VERSE_SCOPES=1` logs what is mounted.

## Perf stress test

```bash
VERSE_STRESS=5000 cargo run -p localgpt-verse --release      # spawns 5000 prop instances, logs fps, exits after 30s
VERSE_STRESS=0 cargo run -p localgpt-verse --release         # control run: report only, no extra props
```

Runs uncapped (vsync off) so the numbers show true frame cost. On the dev
machine (2026-07): baseline world ~60 fps, ~1k props ≈ 14 fps, 5k ≈ 9 fps —
dense packs beyond that need real instancing (ARCHITECTURE.md, "Measured
limits").

## License

LocalGPT Verse's code is licensed under the [Apache License 2.0](LICENSE). The bundled
fonts (Marcellus, Hanken Grotesk) are under the SIL Open Font License 1.1 — see
[`assets/fonts/`](assets/fonts/). The 3D model pack and the starter music live in
the separate `localgpt-world-assets` repository and are dedicated to the
public domain under CC0 1.0. The optional `ml` models carry their own
licenses: `cargo xtask fetch-clap` downloads the CLAP models, which this tree
labels CC-BY-NC-4.0 (see PLAN.md §4), and `stem-splitter-core` downloads the
htdemucs model on first use.
