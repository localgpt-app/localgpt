# LocalGPT Verse — milestone key

The key to the milestone (`M1`–`M7`) and section (`§`) numbers the source
cites, as built. History is in Git; the map (once §2) is `ARCHITECTURE.md`.

## 0. Constraints

1. The UI talks through resources (`Playback`, `Beat`, `Theme`), not to kira.
2. Apache-2.0 and no GPL dependency (`deny.toml`); assets CC0, fonts OFL.
3. Design-spec timing and Comfort are requirements: audio starts at 0 s, the
   materialize is timed from the first downbeat, a song ends in a crossfade,
   the notches are the measured sections, and every effect checks `Comfort`
   where it is computed. A recipe modulates within a mood, inside these.
4. On-device: nothing is uploaded (under `ml`, one model downloads; see §4).
5. No language model runs here and none may be added; `ml` is audio analysis.

## 1. Libraries

### 1.1 Playback

`kira` on `localgpt-world-audio`'s `Engine` (the process's one device and
mixer): streaming decode, tweened fades, the tap `Effect`. `lofty` tags,
`walkdir` scan, `rfd` folder picker. Rejected: `rodio`/`bevy_audio` (no
tweens, no effect to tap the music with), `bevy_kira_audio` (a second device).

### 1.2 Analysis

`symphonia` decodes; `realfft` gives onset flux and centroid; tempo, beat
offset and sections are in-house DSP (`analysis.rs`); `ebur128` measures
loudness; `blake3`, `serde_json` and `dirs` are the cache (§5.2). `ml` adds
`ort`, `rubato` and `tokenizers` for CLAP (the mood, and the embeddings
placement ranks by: the M6 hook) and `stem-splitter-core` for htdemucs.
Rejected as GPL: `aubio`, `bliss-audio`, essentia. Not built: Beat This!.

### 1.3 Worlds and assets

`bevy` 0.20 loads the pack's `.glb` files, uncompressed: bevy_gltf reads no
Draco, meshopt or quantized geometry. Placement is seeded rules (splitmix64;
no `rand`, `noise` or `wfc`). `localgpt-world-types` and `ron` write the
export; `localgpt-world-agent`'s `paths` finds the shared pack.

## 3. Milestones

- **Status.** M1–M6 are built; M7 in part.
- **M1 — Playback.** `audio.rs`, `playback.rs`: folder import, the queue, kira
  streams on the audio clock, pause, seek, skip, volume, shuffle and repeat.
- **M2 — Live tap.** `TapEffect` on the music track (`audio.rs`): one-pole
  filters feed energy, bass onsets, bass and highs into `Beat`.
- **M3 — Offline analysis.** The `analysis.rs` worker caches tempo, beat
  offset, sections, energy and loudness for the playing track and the next
  three; M1–M3 are the rungs of `advance_playback`'s ownership ladder (live
  clock, tap, grid). Not built: crossfades on section boundaries.
- **M4 — Mood rules.** `analysis::map_mood`, the valence/arousal quadrant
  with brightness for valence: `arousal = 0.6·clamp((bpm − 70)/90, 0, 1) +
  0.4·mean energy` (driving above 0.55), `bright = mean centroid / 2500 Hz`
  (bright above 0.45). Driving+dark is Ember Flats, driving+bright Velvet
  Circuit, calm+dark Tide Gardens, calm+bright Glass Expanse; mean energy
  of 0.5 or less picks the restrained variant. Same file, same world.
- **M5 — CLAP (`ml`).** `ml.rs`, over the shared `match` model that
  `localgpt-audio-models` runs: a 512-d track embedding in the sidecar and
  a zero-shot mood in place of the rule's. **M5→M6:** the text tower embeds
  the manifest and placement weights assets by cosine to the track.
- **M6 — Asset pack.** `world_assets.rs`: manifest-driven glTF props in
  three tiers, an arrangement per mood, the merged ground-cover field, the
  skyline; Credits reads the manifest. **Stress notes** (2026-07,
  `VERSE_STRESS`): ~70–90 scene-root props hold 60 fps; 1,000 run at ~14.
- **M7 — Recipes and stems.** Built: the authored `WorldRecipe`
  (`recipe.rs`) and Demucs stem curves under `ml` (`demucs.rs`). Not built:
  Beat This!, WFC layouts. Removed 2026-11-01: the model that wrote recipes.

## 4. Risks still live

- **Model licences.** LAION's CLAP code is CC0-1.0 and the
  `laion/larger_clap_music_and_speech` card gives the weights Apache-2.0.
  Demucs's code is MIT, but its author's README says the weights' licence
  "is not clear": MUSDB, part of their training data, was research-only (all
  checked 2026-11-11). The ONNX conversions the tree downloads (Xenova's CLAP
  export, `gentij/htdemucs-ort`) state no licence of their own. The CLAP
  weights can ship once converted from the originals with their notice; the
  htdemucs weights should not ship.
- **`ml` is checked, not run, in CI**; `ort` is pinned to the release
  candidate `stem-splitter-core` pins, which also downloads htdemucs at run
  time. A panic in a model pass ends the analysis worker (no `catch_unwind`).
- **Prop count:** see the stress notes; denser worlds need instancing.

## 5. Decisions

1. **Pack and starter music** live in the `localgpt-world-assets` repository
   (CC0). `cargo xtask fetch-assets` copies them into the shared pack and
   links `assets/models` to it; `cargo xtask verse-bundle` ships them.
2. **JSON sidecars:** one per track, named by its blake3 hash, in
   `dirs::data_local_dir()/localgpt-verse/analysis/`, never the music folder
   (`LOCALGPT_DATA_DIR` does not move it). New fields default; old files load.
3. **Pinning:** "Keep this world" stores `pinned_mood` and `pinned_seed` in
   the sidecar; the pin wins over the mapper and a recipe's seed.
