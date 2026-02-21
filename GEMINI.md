# LocalGPT Project Context

LocalGPT is a local-first, privacy-focused AI assistant built in Rust. It is inspired by the OpenClaw architecture but implemented as a fast, single-binary application with native OS security features.

## Project Overview

- **Core Goal**: Provide a powerful AI assistant that runs locally, maintains long-term memory via markdown files, and executes tasks autonomously while ensuring system security through sandboxing.
- **Main Technologies**:
  - **Language**: Rust (Edition 2024)
  - **Async Runtime**: [Tokio](https://tokio.rs/)
  - **Database**: SQLite with FTS5 (keyword search) and `sqlite-vec` (semantic search)
  - **UI**: CLI (clap), Desktop (egui/eframe), Telegram (teloxide)
  - **Security**: OS-level sandboxing (Landlock on Linux, Seatbelt on macOS)
  - **LLM Providers**: Support for Anthropic, OpenAI, Ollama, xAI, and OAuth-based subscriptions.

## Workspace Structure

The project is organized as a Rust workspace:

- `crates/core/`: The "brain" of LocalGPT. Contains agent logic, memory management, configuration handling, and core security modules (signing, audit logs).
- `crates/cli/`: The main entry point for the `localgpt` command. Handles subcommands like `chat`, `ask`, `daemon`, `config`, etc.
- `crates/server/`: Implements the background daemon, HTTP REST API, embedded Web UI, and Telegram bot.
- `crates/sandbox/`: Low-level OS-specific sandbox implementations used to confine shell commands executed by the agent.
- `crates/gen/`: A specialized 3D world generation tool (`localgpt-gen`) built with the Bevy engine.
- `crates/mobile-ffi/`: UniFFI bindings for embedding LocalGPT core into iOS/Android apps.
- `crates/bridge/`: Secure IPC protocol for bridge daemons (credential exchange, peer identity).
- `bridges/`: Standalone bridge binaries (Telegram, Discord, WhatsApp) that connect to the daemon via IPC.
- `apps/`: Native mobile app projects (iOS SwiftUI, Android Jetpack Compose).

## Key Concepts

### Memory System
LocalGPT uses a workspace directory (typically in XDG data dirs) containing:
- `SOUL.md`: Defines the agent's personality, tone, and behavioral instructions.
- `MEMORY.md`: Long-term, curated knowledge.
- `HEARTBEAT.md`: A task queue for autonomous background operations.
- `memory/YYYY-MM-DD.md`: Daily append-only logs of interactions.

### Security Model
- **Sandboxing**: All shell commands run via the `bash` tool are executed in a kernel-enforced sandbox that restricts filesystem and network access.
- **Signed Policies**: A `LocalGPT.md` file can define custom safety rules. This file is HMAC-signed to prevent unauthorized modification.
- **Audit Log**: Security events are recorded in an append-only, hash-chained JSONL file.

## Development Commands

### Building and Running
```bash
# Build the entire workspace
cargo build

# Run the CLI chat
cargo run -- chat

# Run the daemon (HTTP API + Heartbeat)
cargo run -- daemon start --foreground

# Run the 3D generation tool
cargo run -p localgpt-gen
```

### Testing and Quality
```bash
# Run all tests
cargo test

# Check formatting
cargo fmt --all -- --check

# Run linter
cargo clippy --workspace -- -D warnings
```

## Coding Conventions

- **Error Handling**: Use `anyhow::Result` for application-level logic and `thiserror` for library-level error types in `crates/core`.
- **Logging**: Use the `tracing` crate. Debug information should be meaningful for troubleshooting agent reasoning.
- **Async**: Use `tokio` for all I/O and concurrent tasks.
- **Agent Tools**: When adding new tools, implement the `Tool` trait in `crates/core/src/agent/tools/mod.rs` (safe tools) or `crates/cli/src/tools.rs` (system tools).
- **Safety**: Never bypass the sandbox for shell execution. Always ensure sensitive paths (like `.ssh`) are protected in `crates/core/src/security/protected_files.rs`.
