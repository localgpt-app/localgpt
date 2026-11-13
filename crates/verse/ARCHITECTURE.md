# LocalGPT Verse — architecture

**Status:** current as of 2026-11-11.

Which thread owns what, the decisions the rest leans on, how a world
performs its song, and the limits that were measured. No language model
runs here and none may be added: a `WorldRecipe` is data in a track's
sidecar, authored outside Verse and applied after `WorldRecipe::clamped()`.

## System map

```text
             Bevy main thread                         other threads
┌────────────────────────────────────────┐  ┌────────────────────────────────┐
│ UI         hud.rs, overlays/           │  │ kira, through world-audio's    │
│  reads Playback, Beat, Theme, Comfort  │  │ Engine: device, mixer, decode  │
│  writes Paused, QueueOpen, SeekRequest │  │  TapEffect on the music track  │
│  and OverlayStack; sends UiAction      │  │  → atomics → Beat              │
├────────────────────────────────────────┤  ├────────────────────────────────┤
│ Transport  audio.rs                    │  │ import scan (audio.rs)         │
│  streams follow Playback; the audio    │  │  walkdir + lofty → mpsc        │
│  clock owns elapsed; crossfade, volume │  │  → poll_import → the queue     │
├────────────────────────────────────────┤  ├────────────────────────────────┤
│ Signals    analysis.rs, audio.rs,      │  │ analysis worker (analysis.rs)  │
│  playback.rs: sidecar → grid, sections │  │  symphonia → realfft: tempo,   │
│  and mood; stems; the tap; the beat    │  │  sections, energy, mood,       │
├────────────────────────────────────────┤  │  loudness; ml: CLAP, stems     │
│ World      world.rs, world_assets.rs   │  │  → JSON sidecar, and mpsc      │
│  backdrop, palette, section feel,      │  │  → sync_analysis               │
│  cameras; props, cover field, skyline  │  ├────────────────────────────────┤
├────────────────────────────────────────┤  │ ml: text embeddings of assets  │
│ Export     world_manifest.rs, lib.rs   │  │  → mutex → AssetEmbeddings     │
└────────────────────────────────────────┘  └────────────────────────────────┘
```

`plugins.rs` composes the app, a plugin per subsystem. What must be released
at run time mounts through `scope.rs` instead: the analysis worker
(unmounting joins it, 15 s at most) and mood packs (`mood_pack.rs`).
`tier.rs` loads an `ml` model on the first track that needs one. Threads
talk through mpsc channels (receivers in a `Mutex` so resources are `Sync`)
and atomics. Shared state behind a lock: the kira handles (`AudioPlayer`),
the mood registry (`RwLock`, `theme.rs`), the folder queue in `settings.rs`,
and under `ml` the map the embedding thread fills. There is no `unsafe`.

## Load-bearing decisions

1. **The UI talks through resources.** `hud.rs` and `overlays/` never call
   kira or the worker: buttons (the overlays' as `UiAction` messages) and
   keys change `Playback`, `Paused`, `OverlayStack` or `SeekRequest`, and
   `audio.rs` makes the stream follow them each frame.
2. **The signal-ownership ladder** (`playback::advance_playback`). With no
   stream the clock, energy and beat are simulated. A live stream owns
   `elapsed` and its tap owns energy, onsets and bands. An analysis adds the
   grid, sections and mood; `ml` fields and a recipe sit above. Each rung
   falls back to the one below when its input is missing.
3. **Content-hash identity.** `Track.id` is the file's blake3: the sidecar
   name, the `AnalysisStore` key and the import dedupe key, so renames and
   moves change nothing. Moods are stored by id beside their index
   (`theme::resolve_mood`); the playing stream is keyed by path, so a queue
   reorder never restarts it.
4. **The asset manifest does three jobs:** placement data (tier, mood, kind,
   scale, native size), the licence trail (author, licence, source) and the
   Credits rows. `world_assets::plan_world_props` is the one planner over
   it: the app spawns from its plan and the export writes it.
5. **Comfort is a gate at effect sites.** `reduce_flashing` and
   `gentler_motion` are read where an effect is computed (`animate_world`,
   `ground_waves`, `animate_props`, `pulse_beacons`, `update_hud_content`),
   not in one central filter: a new effect adds its own check.

## The performed world

`analysis::sync_analysis` applies a sidecar when its track becomes current:
sections, beat grid, mood, layout seed, timbre and the clamped recipe,
whose section moments are resolved against the track's measured sections.
Bass is the louder of the Demucs bass curve at the playhead and the live
tap's band.

- **Sections.** `world::sync_section_moment` eases `SectionFeel` (energy
  shift, motion, beacons, particles, scatter) toward the entered section's
  target: a recipe moment, else a role from position (intro, verse, a chorus
  in the middle, bridge, outro).
- **Structure.** With three or more sections props rise across the song: the
  cover field first, mediums up to the middle section, heroes and landmarks
  on it. The rim skyline is 28 stelae sampling the energy curve; bass
  displaces the ground; the Drift camera follows the section feel.
- **Palette.** A 0.8 s wash on a mood change, then recipe tints (40% at
  most), timbre from the spectral centroid, and a blend toward the
  neighbouring mood for a track near a mapper boundary.
- **Placement by embedding (`ml`).** The CLAP text tower embeds each manifest
  entry's name and tier on its own thread; cosine to the track's audio
  embedding weights the medium props and picks the landmark fallback.

## Measured limits

- **Props.** `VERSE_STRESS=N` (release, vsync off; 2026-07, not measured
  since): the baseline world ~60 fps, 1,000 scene-root props ~14 fps, 5,000
  ~9 fps; about 70–90 hold 60 fps. Hero and medium props are one scene root
  each (`Tier::budget`: 15 and 25); ground cover is one merged mesh.
  Instancing the glTF tiers is the remaining ceiling.
- **glTF.** bevy_gltf 0.20 supports none of `KHR_mesh_quantization`,
  `EXT_meshopt_compression` or `KHR_draco_mesh_compression`, so the pack is
  uncompressed `.glb`: 171 models, 522 MB, native sizes from 0.08 m to 92 m
  (heroes and props are rescaled to their tier's span).
- **CLAP.** Text-to-text similarity is off-manifold on this model (probe:
  `ml::tests::text_embedding_probe`), so only the track's audio embedding
  ranks assets; recipe landmarks match by keyword and kind. The licence of
  the weights is unsettled (`PLAN.md` §4).
- **Analysis** reads at most a track's first 8 minutes.

## Screenshots without a window

Verse has no windowless capture: `VERSE_SHOT` and Photo mode read back the
primary window's frame, which can come back all black (now and then on
some machines; always on a locked or headless Mac, as Document found). Photo mode
samples the capture and asks again, up to three times, over the same file
(`photo_black_check` in `lib.rs`). `VERSE_ONESHOT=1`, with `VERSE_SMOKE` and
`VERSE_SHOT`, saves one settled frame instead of the eight-shot walk. A
frame that is black on every run is a scene bug, not this. A check that has
to run unattended should render to an image, as `LOCALGPT_DOC_SCREENSHOT`
does in `crates/doc/src/main.rs`.
