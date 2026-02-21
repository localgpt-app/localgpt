# GitHub Copilot Instructions for LocalGPT

This file provides guidance to GitHub Copilot when working with code in this repository.

## Project Overview

LocalGPT is a local-only AI assistant built in Rust with persistent markdown-based memory and optional autonomous operation via heartbeat. It runs entirely on your machine with no cloud dependency required.

## Quick Reference

### Build & Test Commands

```bash
# Build
cargo build                     # Debug build (default-members = cli)
cargo build --release           # Release build
cargo build --workspace         # Build all crates

# Test
cargo test --workspace          # All tests
cargo test -p localgpt-core     # Single crate
cargo test -- --nocapture       # Show stdout

# Lint (REQUIRED before commits)
cargo clippy --workspace
cargo fmt --check

# Run
cargo run -- chat               # Interactive chat
cargo run -- ask "question"     # Single question
cargo run -- daemon start       # HTTP server + Telegram bot + heartbeat
```

### Cross-platform Validation

```bash
# Mobile cross-compile checks (required for core changes)
cargo check -p localgpt-mobile-ffi --target aarch64-apple-ios
cargo check -p localgpt-mobile-ffi --target aarch64-apple-ios-sim
```

## Architecture

### Workspace Structure (7 crates)

```
crates/
├── core/        # localgpt-core — shared library (agent, memory, config, security)
├── cli/         # localgpt — binary with clap CLI, desktop GUI, dangerous tools
├── server/      # localgpt-server — HTTP/WS API, Telegram bot, optional WASM web UI
├── sandbox/     # localgpt-sandbox — Landlock/Seatbelt process sandboxing
├── mobile-ffi/  # localgpt-mobile-ffi — UniFFI bindings for iOS/Android
├── gen/         # localgpt-gen — Bevy 3D scene generation binary
└── bridge/      # localgpt-bridge — secure IPC protocol for bridge daemons

bridges/         # Standalone bridge binaries (Telegram, Discord, WhatsApp)
apps/            # Native mobile app projects (iOS, Android)
```

### Dependency Rules

**CRITICAL**: `localgpt-core` must have zero platform-specific dependencies. It must compile cleanly for `aarch64-apple-ios` and `aarch64-linux-android`. No clap, eframe, axum, teloxide, landlock, nix, etc.

```
localgpt ──→ localgpt-core
             ──→ localgpt-server ──→ localgpt-core
             ──→ localgpt-sandbox ──→ localgpt-core

localgpt-mobile-ffi ──→ localgpt-core (default-features = false, embeddings-openai)
localgpt-gen        ──→ localgpt-core
```

### Feature Flags (`localgpt-core`)

| Feature | Default | Purpose |
|---------|---------|---------|
| `embeddings-local` | yes | fastembed/ONNX local embeddings |
| `embeddings-openai` | no | OpenAI API embeddings (mobile uses this) |
| `embeddings-gguf` | no | llama.cpp GGUF embeddings |
| `embeddings-none` | no | FTS5 keyword search only |
| `claude-cli` | yes | ClaudeCliProvider (subprocess-based, excluded on mobile) |

Mobile crate uses `default-features = false, features = ["embeddings-openai"]` to exclude native dependencies.

## Key Design Patterns

### Tool Safety Split
- `Agent::new()` creates safe tools only (memory_search, memory_get, web_fetch, web_search)
- CLI injects dangerous tools (bash, read_file, write_file, edit_file) via `agent.extend_tools(create_cli_tools())`
- Server agents intentionally only get safe tools

### Thread Safety
- Agent is NOT `Send+Sync` due to SQLite
- Use `AgentHandle` (`Arc<tokio::sync::Mutex<Agent>>`) for mobile/server
- HTTP handler uses `spawn_blocking`

### Bevy Main Thread
- Bevy must own the main thread (macOS windowing/GPU requirement)
- Gen mode spawns tokio on a background thread

### Session Management
- When approaching context limits, compaction triggers a memory flush first
- LLM saves important context to MEMORY.md before messages are truncated
- New sessions automatically load `MEMORY.md`, recent daily logs, `HEARTBEAT.md`

### Path Expansion
- All tools use `shellexpand::tilde()` for `~` in paths

### Provider Routing
Model prefix determines LLM provider:
- `claude-cli/*` → Claude CLI
- `gpt-*`/`openai/*` → OpenAI
- `claude-*`/`anthropic/*` → Anthropic API
- `glm-*`/`glm/*` → GLM (Z.AI)
- `ollama/*` → Ollama

## Coding Standards

### General Guidelines
- Follow Rust idioms and best practices
- Use `anyhow::Result` for application errors, `thiserror` for library errors
- Prefer `tracing` macros over `println!` for logging
- Use structured logging with context fields
- All public APIs should have documentation comments

### Error Handling
- Propagate errors with `?` operator where appropriate
- Provide context with `.context()` or `.with_context()`
- Use specific error types for recoverable errors
- Fatal errors should be logged with `tracing::error!` before panicking

### Testing
- Unit tests go in `#[cfg(test)] mod tests` at end of file
- Integration tests go in `tests/` directory
- Use `#[tokio::test]` for async tests
- Mock external dependencies (filesystem, network, LLM APIs)

### Commit Conventions
- Use conventional commit format: `type(scope): description`
- Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`
- Keep commits atomic and focused
- Reference issue numbers in commit messages

## Security Considerations

### Sandbox
- Every shell command runs inside OS-level sandbox (Landlock on Linux, Seatbelt on macOS)
- Sandbox denies access to `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.docker`
- Network syscalls blocked by default
- rlimits: 120s timeout, 1MB output cap, 50MB file size, 64 process limit

### Protected Files
The agent is blocked from writing to:
- `LocalGPT.md`
- `.localgpt_manifest.json`
- `IDENTITY.md`
- `localgpt.device.key`
- `localgpt.audit.jsonl`

### Prompt Injection Defenses
- Strip known LLM control tokens from tool outputs
- Detect injection phrases with regex scanning
- Wrap external content in XML delimiters
- All security events logged to append-only, hash-chained audit file

## Configuration

Default config location: `~/.localgpt/config.toml` (see `config.example.toml`)

Key settings:
- `agent.default_model` — Determines provider (default: `claude-cli/opus`)
- `memory.workspace` — Workspace directory (default: `~/.localgpt/workspace`)
- `memory.embedding_provider` — `"local"` (default), `"openai"`, or `"none"`
- `server.port` — HTTP server port (default: 31327)

Workspace path resolution: `LOCALGPT_WORKSPACE` env > `LOCALGPT_PROFILE` env > `memory.workspace` config > `~/.localgpt/workspace`

## Common Tasks

### Adding a New Tool
1. Define tool in `crates/core/src/agent/tools/`
2. Implement `Tool` trait with name, description, parameters, and execute
3. Add to appropriate tool factory (safe vs dangerous)
4. Add tests for tool execution and error handling

### Adding a New LLM Provider
1. Create new provider struct in `crates/core/src/agent/providers/`
2. Implement `LLMProvider` trait
3. Add provider to `create_provider()` match in `providers/mod.rs`
4. Add config section to `config.example.toml`
5. Update documentation and model routing logic

### Adding a New Command
1. Define command in `crates/core/src/commands.rs`
2. Implement handler in CLI (`crates/cli/src/main.rs`) or server
3. Add to help text and documentation
4. Write integration test

### Mobile Changes
1. Make changes in `localgpt-core` with `default-features = false`
2. Verify no platform-specific deps: `cargo check -p localgpt-mobile-ffi --target aarch64-apple-ios`
3. Rebuild mobile crate: `cargo build -p localgpt-mobile-ffi`
4. Regenerate UniFFI bindings (Swift/Kotlin)

## Validation Checklist

Before submitting a PR:

- [ ] `cargo fmt --check` passes
- [ ] `cargo clippy --workspace` has no warnings
- [ ] `cargo test --workspace` passes
- [ ] If core changes: `cargo check -p localgpt-mobile-ffi --target aarch64-apple-ios` succeeds
- [ ] Documentation updated (if public API changed)
- [ ] CHANGELOG.md updated (if user-facing changes)
- [ ] Security considerations reviewed (if touching tool execution, sandbox, or memory)

## Documentation

- **CLAUDE.md** — Detailed technical reference for Claude Code agent
- **README.md** — User-facing project overview and quick start
- **docs/architecture.md** — System architecture and design decisions
- **docs/RFC-LocalGPT-Security-Policy.md** — Security policy specification
- **docs/gen-audio.md** — Gen mode audio system documentation
- **docs/web-search.md** — Web search provider setup guide
- **config.example.toml** — Example configuration with all options

## Resources

- Repository: https://github.com/localgpt-app/localgpt
- Website: https://localgpt.app
- Discord: https://discord.gg/yMQ8tfxG
- License: Apache-2.0

## Notes for AI Assistants

When working on this codebase:
1. Always run tests after making changes
2. Check cross-compilation for mobile if you touch `localgpt-core`
3. Respect the tool safety split — don't add dangerous tools to safe contexts
4. Preserve thread safety invariants (Agent not Send+Sync)
5. Don't break the mobile build by adding platform-specific deps to core
6. Follow the existing code style and patterns
7. Add appropriate logging and error context
8. Consider security implications of changes to tools, sandbox, or memory system
