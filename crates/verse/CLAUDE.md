# CLAUDE.md

> **This crate lives in the `localgpt` workspace** (folded in from the
> standalone `localgpt-verse` repository; see
> `docs/world-strategy.md` §13.6). Run everything from the
> workspace root with `-p localgpt-verse`. The shared model and asset
> downloads are fetched once by `scripts/fetch-model.sh` and
> `scripts/fetch-assets.sh`, and resolved by
> `localgpt-world-agent`'s `paths` module.
>
> Bevy takes one asset base, so `crates/verse/assets/models` is a **symlink**
> to the shared pack that `scripts/fetch-assets.sh` creates — run it once, or
> the app starts with no asset pack and builds procedural worlds only.

Guidance for Claude Code when working in this repository.

## What this is

LocalGPT Verse: a desktop app (Bevy 0.19) that imagines a 3D world for every
song, driven by on-device music analysis. `README.md` covers running it and
the optional `ml` / `llm` tiers; `ARCHITECTURE.md` is the system map and the
load-bearing decisions; `PLAN.md` the milestones. A member of the `localgpt`
workspace, Apache-2.0. Siblings in the same workspace: `crates/gen`
(prompt → world) and `crates/md` (Markdown → world).

## Commands

```bash
cargo run -p localgpt-verse                                   # open the app
cargo test -p localgpt-verse
cargo clippy -p localgpt-verse --tests -- -D warnings         # CI also checks --features llm and --features ml
cargo fmt
VERSE_IMPORT=<dir> cargo run -p localgpt-verse                # import a music folder at startup
VERSE_EXPORT_WORLD=<dir> cargo run -p localgpt-verse          # write each track's world in the LocalGPT world format
VERSE_SMOKE=1 VERSE_SHOT=/tmp cargo run -p localgpt-verse     # offscreen screenshots, then exit
```

Run `cargo check -p localgpt-verse` after every change and fix all errors before reporting
completion. Run clippy and fmt before committing. CI
(`.github/workflows/ci.yml`) runs fmt, clippy, the tests, the llm/ml feature
checks and cargo-deny (`deny.toml`); advisories run on main and weekly, never
on pull requests.

## Architecture notes

- `src/world_manifest.rs` exports a track's world as a `localgpt-world-types`
  `WorldManifest` (the shared LocalGPT world format): mood palette, the
  agent's scene build, a `SoundtrackDef` with the analysis curves, and
  modulations. Never the audio file or the CLAP embedding. Add fields to
  world-types upstream (`../world-types`) rather than a parallel
  format; the crate is pinned to a localgpt commit until its next release.
- The LLM tiers' constraints are documented in `ARCHITECTURE.md` §10 and must
  be re-verified on any mistral.rs bump.

## Rules

- License is Apache-2.0. Never copy code from Local Native or Fastxt (AGPL-3.0).
- This repo is public. Never name the closed-source sibling 3D platform; use
  generic terms such as "connected 3D app".
- Commits: conventional commits (`feat:`, `fix:`, `docs:`, `chore:`,
  `refactor:`), with no Co-Authored-By or Claude-Session trailers.
- Never use `sed` to edit Rust files; use the Edit tool.
- `website/` is the static landing page for verse.localgpt.app (no build
  step; `website/deploy.sh` deploys it). The docs live on localgpt.app, in the
  `localgpt` repository's `website/docs/verse/`: update them there when
  controls, worlds or the build change, and don't add docs pages here.
