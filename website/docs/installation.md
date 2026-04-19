---
sidebar_position: 2
---

# Installation

## Prerequisites

- **Rust 1.70+** - Install from [rustup.rs](https://rustup.rs)
- **An LLM API key** (at least one of):
  - OpenAI API key
  - Anthropic API key
  - Local Ollama installation

## Install from crates.io (for users)

If you just want to run LocalGPT — no source checkout, no Git clone. `cargo install` fetches the published crate, compiles it, and drops the binary in `~/.cargo/bin/` (already on your PATH if you installed Rust via rustup).

```bash
# Full install (includes desktop GUI)
cargo install localgpt

# Headless install (no desktop GUI — for servers, Docker, CI)
cargo install localgpt --no-default-features
```

After install, run `localgpt chat` from anywhere. Upgrade later with `cargo install localgpt --force`.

## Optional Features

### Embedding Backend

By default, LocalGPT uses **fastembed** for local vector embeddings — pure Rust, no extra dependencies.

To use a **GGUF embedding model** via llama.cpp instead (requires a C++ compiler):

```bash
# Install with GGUF embedding support
cargo install localgpt --features gguf
```

Then configure the embedding provider in your `config.toml`:

```toml
[memory]
embedding_provider = "gguf"
```

### LocalGPT Gen (World Generation)

Gen is a separate binary — it does not add Bevy to the main CLI binary:

```bash
cargo install localgpt-gen
```

See the [Gen docs](/docs/gen) for usage details.

## From source (for developers)

If you've cloned the repository and want to hack on LocalGPT — iterate on code, try unreleased features, or debug a problem.

### Clone

```bash
git clone https://github.com/localgpt-app/localgpt.git
cd localgpt
```

### Run directly with `cargo run` (iterative development)

`cargo run` rebuilds on change and launches the binary — no install step. Use this while editing code. Arguments after `--` are forwarded to the binary.

```bash
# AI Assistant (default workspace member is the CLI)
cargo run -- chat
cargo run -- daemon start
cargo run -- ask "What is 2+2?"

# World Building (separate crate)
cargo run -p localgpt-gen
cargo run -p localgpt-gen -- "Create a desert scene with pyramids"

# Headless (no desktop GUI)
cargo run --no-default-features -- chat
```

### Build a release binary (optimized)

When you want an optimized binary to ship, test performance, or install onto another machine:

```bash
# Build release binary (includes desktop GUI)
cargo build --release

# Build headless (no desktop GUI — skips eframe/egui/winit)
cargo build --release --no-default-features

# The binary will be at target/release/localgpt
```

### Install a source build onto your PATH

After `cargo build --release`, copy the binary so you can run it from anywhere:

```bash
# Option 1: Install to /usr/local/bin
sudo cp target/release/localgpt /usr/local/bin/

# Option 2: Install to ~/.local/bin (no sudo required)
mkdir -p ~/.local/bin
cp target/release/localgpt ~/.local/bin/

# Option 3: Let cargo handle it (installs to ~/.cargo/bin)
cargo install --path crates/cli
```

## Docker / Headless Server

For Docker or headless environments where display servers (X11/Wayland) are unavailable, build without the desktop feature to avoid `winit` compilation issues:

```bash
cargo build --release --no-default-features
```

Or in a Dockerfile:

```dockerfile
FROM rust:1.83 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release --no-default-features

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/localgpt /usr/local/bin/
CMD ["localgpt", "daemon", "start", "--foreground"]
```

The headless binary includes all features except the desktop GUI: CLI, web UI, HTTP API, WebSocket, daemon mode, and heartbeat.

## Initial Setup

1. **Create the workspace directory:**

```bash
mkdir -p ~/.local/share/localgpt/workspace/memory
```

2. **Create the configuration file:**

```bash
mkdir -p ~/.config/localgpt
cp config.example.toml ~/.config/localgpt/config.toml
```

3. **Edit the configuration with your API key:**

```bash
# Set your API key in the environment or edit config.toml
export OPENAI_API_KEY="your-api-key"
```

## Verify Installation

```bash
# Check version and help
localgpt --help

# Test with a simple question
localgpt ask "What is 2+2?"
```

## Linux Desktop Build

On Linux, the desktop GUI requires X11 or Wayland development libraries. If building with the desktop feature:

```bash
# Debian/Ubuntu
sudo apt install libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev

# Or build headless to skip these requirements
cargo build --release --no-default-features
```

## Using with Ollama (Local Models)

If you prefer fully local operation with Ollama:

1. Install Ollama from [ollama.ai](https://ollama.ai)
2. Pull a model: `ollama pull llama3`
3. Update your config:

```toml
[agent]
default_model = "llama3"

[providers.ollama]
endpoint = "http://localhost:11434"
```

Ollama models with tool calling capability (e.g., `llama3`, `mistral`) support all 7 built-in tools.
