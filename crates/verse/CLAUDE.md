# CLAUDE.md

What is particular to `crates/verse`. The workspace's rules (no model, lint,
commits, the all-Rust rule) are in the root `CLAUDE.md`; run everything from
the workspace root with `-p localgpt-verse`.

Bevy takes one asset base, so `crates/verse/assets/models` is a **symlink**
to the shared pack that `cargo xtask fetch-assets` creates — run it once, or
the app starts with no asset pack and builds procedural worlds only.

## What this is

LocalGPT Verse: a desktop app (Bevy 0.20) that imagines a 3D world for every
song, driven by on-device music analysis. `README.md` covers running it and
the optional `ml` tier; `ARCHITECTURE.md` is the system map, the
load-bearing decisions and the measured limits; `PLAN.md` is the key to the
milestone and section numbers the source cites. Siblings in the workspace:
`crates/gen` (prompt → world) and `crates/md` (Markdown → world).

No language model runs here and none may be added: a `WorldRecipe` in a
track's sidecar, written by an agent, is applied after `clamped()`. The
`ml` feature (CLAP moods, Demucs stems) is audio analysis.

## Commands

```bash
cargo run -p localgpt-verse                                   # open the app
cargo test -p localgpt-verse
cargo clippy -p localgpt-verse --tests -- -D warnings         # CI also checks --features ml
VERSE_IMPORT=<dir> cargo run -p localgpt-verse                # import a music folder at startup
VERSE_EXPORT_WORLD=<dir> cargo run -p localgpt-verse          # write each track's world in the LocalGPT world format
VERSE_SMOKE=1 VERSE_SHOT=/tmp cargo run -p localgpt-verse     # offscreen screenshots, then exit
```

## Notes

- `src/world_manifest.rs` exports a track's world as a
  `localgpt-world-types` `WorldManifest`: the mood palette, the rule-based
  world every track gets, a `SoundtrackDef` with the analysis curves, and
  the modulations that bind entities to it. Never the audio file or the
  CLAP embedding. Add fields to world-types (`../world-types`) rather than
  a parallel format.
- Verse's docs are on localgpt.app, in `website/content/docs/verse/`:
  update them when controls, worlds or the build change, and don't add docs
  pages here.
