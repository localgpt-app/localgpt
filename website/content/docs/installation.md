---
title: "Installation"
description: "Install LocalGPT: the binaries from crates.io, the desktop app from source, the two shared downloads, feature flags, and headless builds."
---

# Installation

## Prerequisites

- **Rust stable, 1.85 or newer** (the workspace is edition 2024) — install from [rustup.rs](https://rustup.rs)
- **On Linux**, the system packages the desktop crates (the app, Gen, Document, Verse) build against:

  ```bash
  # Debian / Ubuntu
  sudo apt install libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev libfontconfig1-dev libdbus-1-dev pkg-config
  ```

- **Node 22**, to build the desktop app's page once (`crates/app/ui`); the release binary carries it.
- **An agent of your own**, if you want one: Claude Code, Codex, Gemini CLI or any other. LocalGPT installs none, runs no language model and holds no key; an agent runs in a terminal the app opens, or anywhere else, and brings its own login. Document, song and previs worlds need no model at all.

## From crates.io, npm or Homebrew (for users)

`cargo install` fetches a published crate, compiles it and drops the binary in `~/.cargo/bin/` (on your `PATH` if you installed Rust with rustup). No source checkout.

```bash
cargo install localgpt-gen       # the world builder: MCP server and relay, the live canvas, sessions, headless
cargo install localgpt           # `localgpt world`, `localgpt world mcp` (the shim), `localgpt previs` and `localgpt audio`
```

For the `localgpt` command line and MCP shim without a Rust toolchain, install the prebuilt binary from npm (or run it directly with `npx`):

```bash
npm install -g localgpt          # or: npx -y localgpt world mcp
```

On macOS, `LocalGPT.app` carries `localgpt` inside `Contents/MacOS/localgpt` (and puts it on `PATH` in the app's terminals), so `localgpt gen …` hands off directly to the desktop binary (`localgpt-app gen …`); the Homebrew cask installs the app and links `localgpt` onto your `PATH`:

```bash
brew install --cask localgpt-app/tap/localgpt
```

Upgrade later with `cargo install <crate> --force`, `npm update -g localgpt`, or `brew upgrade --cask localgpt`. The desktop app, `localgpt-app`, is not on crates.io: it ships as a build.

## From source (for developers, and for the desktop app)

```bash
git clone https://github.com/localgpt-app/localgpt.git
cd localgpt
(cd crates/app/ui && npm ci && npm run build)         # the app's page, once
```

`cargo run` rebuilds on change and launches a binary; arguments after `--` are forwarded to it.

```bash
# The desktop app — one window, every mode
cargo run -p localgpt-app                             # reopens what was opened last
cargo run -p localgpt-app -- --doc notes.md            # a document as a world
cargo run -p localgpt-app -- --song track.mp3         # a song as a world that performs it
cargo run -p localgpt-app -- --world scene-1.world    # a .world package, opened as a live canvas

# Gen, Document and Verse on their own
cargo run -p localgpt-gen -- --mcp-relay              # the window; an MCP client drives it
cargo run -p localgpt-gen -- --live scene-1.world     # the live canvas
cargo run -p localgpt-doc -- path/to/doc.md
cargo run -p localgpt-verse

# The command line (the CLI is the workspace's default member)
cargo run -- world log scene-1.world
cargo run -- previs script.fdx --out previs/
```

### The shared download

Every app reads the same folder under `~/.local/share/localgpt/models/`, so the download happens once per machine:

```bash
cargo xtask fetch-assets   # the 522 MB CC0 asset pack (Poly Haven models) → models/pack
```

The pack is what document and song worlds are furnished with; without it the worlds still build, with primitives only. `$LOCALGPT_WORLD_ASSETS` moves the folder.

The [audio models](@/docs/audio-models.md) go beside it, in `models/audio/`, only when you install one: **Audio** in the app's bar, or `localgpt audio install`. They analyse sound for an agent; nothing in LocalGPT runs a language model.

### The macOS app bundle

```bash
cargo xtask bundle             # release build → apps/app-desktop/dist/LocalGPT.app
cargo xtask bundle --dmg       # also a .dmg
open apps/app-desktop/dist/LocalGPT.app
```

Without a Developer ID the bundle is ad-hoc signed and runs on the Mac that built it; the script signs and notarizes when given `APPLE_SIGNING_IDENTITY` and `APPLE_NOTARY_PROFILE`. There is no Windows installer or Linux AppImage yet; `cargo install --path crates/app` puts `localgpt-app` on your `PATH` on any platform, and `apps/app-desktop/linux/localgpt.desktop` adds it to a Linux app menu. The native panels run on all three platforms; Windows and Linux are newer and less tried than macOS.

### Release builds and installing a source build

```bash
cargo build --release                         # the CLI (LTO: slow)
cargo build --release -p localgpt-gen
cargo install --path crates/cli               # or crates/gen, crates/app
```

## Feature flags

| Crate | Flag | What it adds | Fetch first |
|---|---|---|---|
| `localgpt-gen` | `physics`, `multiplayer` (default) | avian3d physics; `--host`/`--join`/`--web` sessions | — |
| `localgpt` | `audio` (default) | `localgpt audio`: the optional [audio models](@/docs/audio-models.md), and their tools in `localgpt world mcp` once installed; links ONNX Runtime | `localgpt audio install <id>` |
| `localgpt-verse` | `ml` | CLAP zero-shot moods and embedding-ranked placement (Apache-2.0 weights), Demucs stems (MIT code; its author calls the weights' licence unclear) — audio analysis, not a language model | `localgpt audio install match` (Demucs downloads itself) |

```bash
cargo run -p localgpt-verse --features ml
```

## Headless

Gen builds without a window as an MCP client's own process: `localgpt gen mcp-server --headless` (or `localgpt-app mcp-server --headless` / `localgpt-gen mcp-server --headless`). The CLI needs no display at all, so `localgpt world` runs in CI and on servers. See [Headless](/docs/gen/headless/).

## Initial setup

There is none on LocalGPT's side: no config file, no key, no model to choose. Give your agent the world's tools once, if it speaks MCP:

```bash
claude mcp add world -- localgpt world mcp     # Claude Code; Codex and Gemini CLI take the same command in their MCP config
```

An agent in one of the app's terminals needs even that only if it prefers tools to the `localgpt world` commands the folder's `AGENTS.md` teaches it.

## Verify

```bash
localgpt --help
localgpt world --help
localgpt-gen --help
```
