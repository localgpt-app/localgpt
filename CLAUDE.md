# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
# Build
cargo build                     # Debug build (default-members = cli)
cargo build --release           # Release build
cargo build --workspace         # Build all crates

# Run
cargo run -- chat               # Interactive chat
cargo run -- ask "question"     # Single question
cargo run -- daemon start       # HTTP server + Telegram bot + heartbeat

# Test
cargo test --workspace          # All tests
cargo test -p localgpt-core     # Single crate
cargo test -- --nocapture       # Show stdout

# Lint (required before commits)
cargo clippy --workspace
cargo fmt --check

# Cross-compile checks (mobile)
cargo check -p localgpt-mobile-ffi --target aarch64-apple-ios
cargo check -p localgpt-mobile-ffi --target aarch64-apple-ios-sim

# Gen (3D scene generation with Bevy)
cargo run -p localgpt-gen                          # Interactive mode
cargo run -p localgpt-gen -- "build a castle"      # With initial prompt
cargo run -p localgpt-gen -- -s model.glb          # Load existing scene
cargo run -p localgpt-gen -- -v                    # Verbose logging

# Headless build (no desktop GUI)
cargo build -p localgpt --no-default-features

# Generate UniFFI bindings (after building mobile crate)
cargo build -p localgpt-mobile-ffi
target/debug/uniffi-bindgen generate \
  --library target/debug/liblocalgpt_mobile.dylib \
  --language swift --out-dir apps/ios/Generated
target/debug/uniffi-bindgen generate \
  --library target/debug/liblocalgpt_mobile.dylib \
  --language kotlin --out-dir apps/android/Generated
```

## Architecture

LocalGPT is a local-only AI assistant with persistent markdown-based memory and optional autonomous operation via heartbeat.

### Workspace (7 crates)

```
crates/
├── core/        # localgpt-core — shared library (agent, memory, config, security)
├── cli/         # localgpt — binary with clap CLI, desktop GUI, dangerous tools
├── server/      # localgpt-server — HTTP/WS API, Telegram bot, embedded Web UI
├── sandbox/     # localgpt-sandbox — Landlock/Seatbelt process sandboxing
├── mobile-ffi/  # localgpt-mobile-ffi — UniFFI bindings for iOS/Android
├── gen/         # localgpt-gen — Bevy 3D scene generation binary
└── bridge/      # localgpt-bridge — secure IPC protocol for bridge daemons

bridges/         # Standalone bridge binaries (Telegram, Discord, WhatsApp)
apps/            # Native mobile app projects (iOS, Android)
```

### Dependency Graph

```
localgpt ──→ localgpt-core
             ──→ localgpt-server ──→ localgpt-core
             ──→ localgpt-sandbox ──→ localgpt-core

localgpt-mobile-ffi ──→ localgpt-core (default-features = false, embeddings-openai)
localgpt-gen        ──→ localgpt-core
```

**Critical rule:** `localgpt-core` must have zero platform-specific dependencies. It must compile cleanly for `aarch64-apple-ios` and `aarch64-linux-android`. No clap, eframe, axum, teloxide, landlock, nix, etc.

### Feature Flags (`localgpt-core`)

| Feature | Default | Purpose |
|---------|---------|---------|
| `embeddings-local` | yes | fastembed/ONNX local embeddings |
| `embeddings-openai` | no | OpenAI API embeddings (mobile uses this) |
| `embeddings-gguf` | no | llama.cpp GGUF embeddings |
| `embeddings-none` | no | FTS5 keyword search only |
| `claude-cli` | yes | ClaudeCliProvider (subprocess-based, excluded on mobile) |

Mobile crate uses `default-features = false, features = ["embeddings-openai"]` — this excludes `embeddings-local` (fastembed/ONNX native deps) and `claude-cli` (subprocess execution).

### Key Patterns

**Tool safety split:** `Agent::new()` creates safe tools only (memory_search, memory_get, web_fetch, web_search). CLI injects dangerous tools (bash, read_file, write_file, edit_file) via `agent.extend_tools(create_cli_tools())`. Server agents intentionally only get safe tools.

**Heartbeat tool injection:** `HeartbeatRunner` in core accepts an optional `ToolFactory` callback to extend the agent with additional tools. CLI daemon provides `create_cli_tools` factory so heartbeat can perform file operations and execute commands. Without the factory, heartbeat runs with safe tools only.

**Custom tool sets:** `Agent::new_with_tools()` replaces all tools — used by Gen mode for its own Bevy tools (spawn_entity, modify_entity, etc.).

**Thread safety:** Agent is not `Send+Sync` due to SQLite. Use `AgentHandle` (`Arc<tokio::sync::Mutex<Agent>>`) for mobile/server. HTTP handler uses `spawn_blocking`.

**Bevy main thread:** Bevy must own the main thread (macOS windowing/GPU). Gen mode spawns tokio on a background thread.

**Session compaction:** When approaching context limits, compaction triggers a memory flush first (LLM saves important context to MEMORY.md before messages are truncated).

**Memory context:** New sessions automatically load `MEMORY.md`, recent daily logs, `HEARTBEAT.md`.

**Path expansion:** Tools use `shellexpand::tilde()` for `~` in paths.

**Provider routing:** Model prefix determines LLM provider: `claude-cli/*` → Claude CLI, `gpt-*`/`openai/*` → OpenAI, `claude-*`/`anthropic/*` → Anthropic API, `glm-*`/`glm/*` → GLM (Z.AI), `ollama/*` → Ollama.

### Core Modules

- **agent/providers.rs** — `LLMProvider` trait + 5 implementations (OpenAI, Anthropic, Ollama, ClaudeCliProvider, GLM)
- **agent/session.rs** — Conversation state with automatic compaction
- **agent/session_store.rs** — Session metadata persistence (`sessions.json`)
- **agent/system_prompt.rs** — System prompt builder (identity, safety, workspace, tools, skills)
- **agent/skills.rs** — SKILL.md file loading from workspace/skills/
- **memory/** — SQLite FTS5 + file watcher + workspace templates + embeddings
- **heartbeat/** — Autonomous task runner on configurable interval
- **config/** — TOML config. `Config::load()` (desktop), `Config::load_from_dir()` (mobile)
- **paths.rs** — XDG dirs. `Paths::resolve()` (desktop), `Paths::from_root()` (mobile)
- **commands.rs** — Shared slash command definitions (CLI + Telegram)
- **concurrency/** — TurnGate (one agent turn at a time) + WorkspaceLock
- **security/** — LocalGPT.md policy signing/verification

### Server

- **http.rs** — Axum REST API with RustEmbed'd Web UI. Routes: `/health`, `/api/status`, `/api/chat`, `/api/memory/search`, `/api/memory/stats`
- **telegram.rs** — Telegram bot with 6-digit pairing auth, streaming edits, agent ID `"telegram"`

### Gen (3D Scene Generation with Audio)

**Binary:** `localgpt-gen` — Bevy-based 3D scene generation with procedural environmental audio.

**Audio System:**
- **Engine:** FunDSP v0.20 for procedural synthesis, cpal for audio output
- **Architecture:** 3-thread model (Bevy main → audio mgmt thread → cpal callback) with lock-free `Shared<f32>` parameters
- **Ambient sounds:** Wind, Rain, Forest, Ocean, Cave, Stream, Silence (with LFO variation)
- **Emitter sounds:** Water, Fire, Hum, Wind, Custom waveforms (spatial, distance-attenuated)
- **Auto-inference:** Entity names like "campfire", "waterfall", "stream" automatically get sounds
- **Tools:** `gen_set_ambience`, `gen_audio_emitter`, `gen_modify_audio`, `gen_audio_info`

See `docs/gen-audio.md` for detailed architecture and usage examples.

### Mobile

UniFFI proc-macro bindings (`crates/mobile-ffi/`). `LocalGPTClient` owns its own tokio runtime and wraps `AgentHandle`. Error type: `MobileError` enum (Init, Chat, Memory, Config).

iOS: `apps/ios/scripts/build_ios.sh` → XCFramework + Swift bindings
Android: `apps/android/scripts/build_android.sh` → cargo-ndk + Kotlin bindings

## Configuration

Config: `~/.localgpt/config.toml` (auto-created on first run, see `config.example.toml`)

Key settings:
- `agent.default_model` — Determines provider. Default: `claude-cli/opus`
- `memory.workspace` — Workspace directory. Default: `~/.localgpt/workspace`
- `memory.embedding_provider` — `"local"` (default), `"openai"`, or `"none"`
- `server.port` — HTTP server port (default: 31327)
- `telegram.enabled` / `telegram.api_token` — Telegram bot (supports `${ENV_VAR}` syntax)

**OAuth Subscription Plans:**
- `providers.anthropic_oauth` — Claude Pro/Max OAuth credentials (uses Bearer token, preferred over API key)
- `providers.gemini_oauth` — Google Gemini subscription OAuth credentials (supports project_id for enterprise)

Workspace path resolution: `LOCALGPT_WORKSPACE` env > `LOCALGPT_PROFILE` env > `memory.workspace` config > `~/.localgpt/workspace`

## Runtime Directory Structure

```
~/.localgpt/
├── config.toml
├── agents/{agent_id}/sessions/   # Session transcripts (JSONL, Pi-compatible)
├── workspace/                    # Memory workspace
│   ├── MEMORY.md                 # Long-term curated memory
│   ├── HEARTBEAT.md              # Pending autonomous tasks
│   ├── SOUL.md                   # Persona/tone
│   ├── memory/YYYY-MM-DD.md      # Daily logs
│   ├── knowledge/                # Knowledge repository
│   └── skills/*/SKILL.md         # Custom skills
└── logs/
```
