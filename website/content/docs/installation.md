---
title: "Installation"
description: "Install LocalGPT: the binaries from crates.io, the desktop app from source, the two shared downloads, feature flags, Linux packages, and headless builds."
---

# Installation

## Prerequisites

- **Rust stable, 1.85 or newer** (the workspace is edition 2024) — install from [rustup.rs](https://rustup.rs)
- **On Linux**, the system packages the desktop crates (Gen, the app, MD, Verse) build against:

  ```bash
  # Debian / Ubuntu
  sudo apt install libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev libfontconfig1-dev libdbus-1-dev pkg-config
  ```

- **A model**, for the parts that use one: a signed-in Claude CLI, Gemini CLI or Codex (no API key needed), an API key, a local Ollama or LM Studio, or a GGUF for the in-process tier. Previs and song worlds need no model at all.

## From crates.io (for users)

`cargo install` fetches a published crate, compiles it and drops the binary in `~/.cargo/bin/` (on your `PATH` if you installed Rust with rustup). No source checkout.

```bash
cargo install localgpt-gen       # the world builder: interactive, MCP server, headless, sessions, the live canvas
cargo install localgpt-previs    # a screenplay → per-scene .world packages, a board, a shot list
cargo install localgpt           # the assistant (chat, daemon, memory) and `localgpt world`

# The assistant without the desktop GUI — servers, Docker, CI
cargo install localgpt --no-default-features
```

Upgrade later with `cargo install <crate> --force`. The desktop app, `localgpt-app`, is not on crates.io: it ships as a build (below).

## From source (for developers, and for the desktop app)

```bash
git clone https://github.com/localgpt-app/localgpt.git
cd localgpt
```

`cargo run` rebuilds on change and launches a binary; arguments after `--` are forwarded to it.

```bash
# The desktop app — one window, every mode
cargo run -p localgpt-app                             # reopens what was opened last
cargo run -p localgpt-app -- --md notes.md            # a document as a world
cargo run -p localgpt-app -- --song track.mp3         # a song as a world that performs it
cargo run -p localgpt-app -- --prompt "a quiet harbor at dusk"
cargo run -p localgpt-app -- --world scene-1.world    # a .world package, opened as a live canvas

# Gen, MD and Verse on their own
cargo run -p localgpt-gen -- "Create a desert scene with pyramids"
cargo run -p localgpt-md -- path/to/doc.md
cargo run -p localgpt-verse

# Previs on the command line
cargo run -p localgpt-previs -- script.fdx --out previs/

# The assistant (the CLI is the workspace's default member)
cargo run -- chat
cargo run -- daemon start
cargo run --no-default-features -- chat               # headless
```

### The two shared downloads

Every app reads the same two folders under `~/.local/share/localgpt/models/`, so each download happens once per machine:

```bash
scripts/fetch-assets.sh    # the 522 MB CC0 asset pack (Poly Haven models) → models/pack
scripts/fetch-model.sh     # the ~5 GB Bonsai-8B GGUF → models/llm (optional: the in-process local-model tier)
```

The pack is what document and song worlds are furnished with; without it the agent tiers still work, with primitives only. The GGUF is only needed for `gguf/<name>` models in Gen and the app (`local-llm`), and MD's and Verse's `llm` tiers. `$LOCALGPT_WORLD_ASSETS` and `$LOCALGPT_LLM_DIR` move the folders.

### The macOS app bundle

```bash
apps/app-desktop/macos/build-app.sh            # release build → apps/app-desktop/dist/LocalGPT.app
apps/app-desktop/macos/build-app.sh --dmg      # also a .dmg
open apps/app-desktop/dist/LocalGPT.app
```

On Apple Silicon the bundle builds with `local-llm-metal`, so it can run a GGUF from the shared folder on the GPU. Without a Developer ID it is ad-hoc signed and runs on the Mac that built it; the script signs and notarizes when given `APPLE_SIGNING_IDENTITY` and `APPLE_NOTARY_PROFILE`. There is no Windows installer or Linux AppImage yet; `cargo install --path crates/app` puts `localgpt-app` on your `PATH` on any platform, and `apps/app-desktop/linux/localgpt.desktop` adds it to a Linux app menu.

### Release builds and installing a source build

```bash
cargo build --release                         # the CLI, with the desktop GUI (LTO: slow)
cargo build --release --no-default-features   # the CLI without eframe/egui/winit
cargo build --release -p localgpt-gen
cargo install --path crates/cli               # or crates/gen, crates/app, crates/previs
```

## Feature flags

| Crate | Flag | What it adds | Fetch first |
|---|---|---|---|
| `localgpt` | `desktop` (default) | the egui desktop GUI for the assistant | — |
| `localgpt` | `gguf` | GGUF embeddings through llama.cpp instead of fastembed (needs a C++ compiler); set `memory.embedding_provider = "gguf"` | — |
| `localgpt-gen`, `localgpt-app` | `local-llm`, `local-llm-metal` (macOS) | `gguf/<name>` models run in-process through mistral.rs | `scripts/fetch-model.sh` |
| `localgpt-gen` | `physics`, `multiplayer` (default) | avian3d physics; `--host`/`--join`/`--web` sessions | — |
| `localgpt-md`, `localgpt-verse` | `llm`, `llm-metal` (macOS) | the local model authors each section's place / each song's recipe and scene | `scripts/fetch-model.sh` |
| `localgpt-verse` | `ml` | CLAP zero-shot moods and embedding-ranked placement (CC-BY-NC weights: personal and research use) | `crates/verse/scripts/fetch-clap.sh` |

```bash
cargo build --release -p localgpt-gen --features local-llm-metal     # Apple Silicon; plain `local-llm` elsewhere
cargo run -p localgpt-md --features llm-metal -- doc.md
cargo run -p localgpt-verse --features ml
```

## Docker / headless server

For Docker or headless environments where display servers are unavailable, build the assistant without the desktop feature:

```bash
cargo build --release --no-default-features
```

Or in a Dockerfile:

```dockerfile
FROM rust:1.90 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --no-default-features

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/localgpt /usr/local/bin/
CMD ["localgpt", "daemon", "start", "--foreground"]
```

The headless binary keeps everything but the GUI: CLI, web UI, HTTP API, WebSocket, daemon, heartbeat, cron, `localgpt world`. Gen generates without a window too (`localgpt-gen headless --prompt …`, `localgpt-gen mcp-server --headless`); see [Headless Mode](/docs/gen/headless/).

## Initial setup

The first run creates `~/.config/localgpt/config.toml` and the workspace; `localgpt init` does it explicitly and generates the device key. With a signed-in Claude CLI there is nothing else to configure — the default model is `claude-cli/opus`. Otherwise set a model:

```bash
localgpt config set agent.default_model "openai/qwen/qwen3.5-35b-a3b"   # LM Studio, fully local
localgpt config set providers.openai.api_key "lm-studio"
localgpt config set providers.openai.base_url "http://127.0.0.1:1234/v1"

export ANTHROPIC_API_KEY="sk-ant-..."                                      # or an API key, with anthropic/… models
```

The [Quick Start](/docs/quick-start/) walks through the options; the [Configuration](/docs/configuration/) page has the whole file.

## Verify

```bash
localgpt --help
localgpt doctor                  # config, keys, provider reachability, memory database, MCP servers
localgpt ask "What is 2+2?"
localgpt-gen --help
```

## Using with Ollama (local models)

1. Install Ollama from [ollama.ai](https://ollama.ai)
2. Pull a model with tool calling: `ollama pull llama3`
3. Update your config:

```toml
[agent]
default_model = "ollama/llama3"

[providers.ollama]
endpoint = "http://localhost:11434"
```

Gen's model menu lists the models pulled into a local Ollama as well, and Gen's `--tools core` profile keeps the toolbelt small enough for a 32k-context local model.
