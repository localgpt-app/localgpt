# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
# Build
cargo build                     # Debug build (default-members = crates/cli)
cargo build --release           # Release build (LTO, stripped — slow)
cargo build --workspace         # Build all workspace crates

# Run
cargo run -- chat               # Interactive chat (REPL)
cargo run -- tui                # Ratatui terminal UI
cargo run -- ask "question"     # Single question, print answer
cargo run -- daemon start       # HTTP server + bridges + heartbeat + cron
cargo run -- doctor             # Diagnose setup (config, keys, providers)

# Test
cargo test --workspace          # All tests
cargo test -p localgpt-core     # Single crate
cargo test -- --nocapture       # Show stdout

# Lint (required before commits)
cargo clippy --workspace -- -D warnings
cargo fmt --check

# Cross-compile checks (mobile) — core must compile for these targets
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
  --language swift --out-dir apps/apple/Generated
target/debug/uniffi-bindgen generate \
  --library target/debug/liblocalgpt_mobile.dylib \
  --language kotlin --out-dir apps/android/Generated

# Mobile app builds (wrap the above)
apps/apple/scripts/build_apple.sh                  # XCFramework + Swift (iOS/macOS)
apps/android/scripts/build_android.sh              # cargo-ndk + Kotlin
```

## Architecture

LocalGPT is a local-only AI assistant with persistent markdown-based memory and
optional autonomous operation via heartbeat and cron. It speaks to many LLM
providers, runs across CLI / TUI / desktop GUI / HTTP server / chat bridges /
mobile, and includes a Bevy-based 3D world generator.

### Workspace

`Cargo.toml` defines 14 members. `crates/spacetime` is a **standalone** crate
(its own `[workspace]`) excluded from the main build because it targets
SpacetimeDB's wasm/module toolchain.

```
crates/
├── core/         # localgpt-core — shared library (agent, memory, config, security, mcp, cron, hooks)
├── cli/          # localgpt — main binary: clap CLI, TUI, desktop GUI, daemon
├── cli-tools/    # localgpt-cli-tools — dangerous tools (bash, read/write/edit_file, browser)
├── server/       # localgpt-server — HTTP/WS API, OpenAI-compat API, Telegram bot, TLS, BridgeManager
├── sandbox/      # localgpt-sandbox — Landlock/Seatbelt kernel-enforced shell isolation
├── mobile-ffi/   # localgpt-mobile-ffi — UniFFI bindings for iOS/Android
├── gen/          # localgpt-gen — Bevy 3D scene generation binary
├── world-types/  # localgpt-world-types — serde-only world data model (no Bevy/SpacetimeDB)
├── bridge/       # localgpt-bridge — secure IPC protocol for bridge daemons
└── spacetime/    # localgpt-spacetime — SpacetimeDB multiplayer world server (standalone)

bridges/          # Standalone bridge binaries (all depend on core + bridge)
├── telegram/     # localgpt-bridge-telegram
├── discord/      # localgpt-bridge-discord
├── whatsapp/     # localgpt-bridge-whatsapp
├── slack/        # localgpt-bridge-slack (Socket Mode)
└── cli/          # localgpt-bridge-cli — connects to a running daemon over the IPC socket

apps/             # Native client projects
├── apple/        # iOS/macOS (Swift)
├── android/      # Android (Kotlin)
└── web/          # Web client
```

### Dependency Graph

```
                        ┌─────────────────┐
                        │ localgpt-core   │  (no internal deps)
                        └────────┬────────┘
         ┌───────────────┬───────┼────────┬──────────────┐
         ▼               ▼       ▼        ▼              ▼
┌──────────────┐ ┌────────────┐ ┌──────┐ ┌────────┐ ┌──────────────┐
│ localgpt-    │ │ localgpt-  │ │ gen  │ │ mobile │ │ world-types  │
│ bridge       │ │ sandbox    │ │      │ │ -ffi   │ │ (serde only) │
│ (no internal)│ └─────┬──────┘ └──────┘ └────────┘ └──────┬───────┘
└──────┬───────┘       │                                   │
       │               ▼                          ┌────────┴───────┐
       │     ┌──────────────────┐                 ▼                ▼
       │     │ localgpt-cli-    │            gen, spacetime (multiplayer)
       │     │ tools (core+     │
       │     │  sandbox)        │
       │     └────────┬─────────┘
       ▼              │
┌─────────────────┐   │
│ localgpt-server │   │
│ (core + bridge) │   │
└────────┬────────┘   │
         ▼            ▼
      ┌──────────────────────────┐
      │ localgpt (CLI)           │
      │ core + server + sandbox  │
      │ + cli-tools + gen        │
      └──────────────────────────┘

Bridge daemons (core + bridge): telegram, discord, whatsapp, slack, cli
Mobile: mobile-ffi → core (default-features=false, embeddings-local + sqlite-vec)
```

**Critical rule:** `localgpt-core` must have zero platform-specific dependencies
and must compile cleanly for `aarch64-apple-ios` and `aarch64-linux-android`.
No clap, eframe, axum (except behind `mcp-http`), teloxide, landlock, nix,
tarpc, headless_chrome, `localgpt-bridge`, etc. Dangerous filesystem/shell
tools live in `localgpt-cli-tools` (depends on `localgpt-sandbox`), **not** core.

### Feature Flags (`localgpt-core`)

Default: `embeddings-local`, `claude-cli`, `gemini-cli`, `codex-cli`,
`sqlite-vec`, `image-resize`.

| Feature | Default | Purpose |
|---------|---------|---------|
| `embeddings-local` | yes | fastembed/ONNX local embeddings (works on mobile) |
| `embeddings-openai` | no | OpenAI API embeddings |
| `embeddings-gguf` (`gguf`) | no | llama.cpp GGUF embeddings (needs C++ compiler) |
| `embeddings-none` | no | FTS5 keyword search only |
| `sqlite-vec` | yes | sqlite-vec vector search extension (works on mobile) |
| `claude-cli` | yes | ClaudeCliProvider (subprocess — excluded on mobile) |
| `gemini-cli` | yes | GeminiCliProvider (subprocess — excluded on mobile) |
| `codex-cli` | yes | CodexCliProvider (subprocess — excluded on mobile) |
| `image-resize` | yes | Resize images before sending to vision models |
| `mcp-http` | no | MCP streamable HTTP transport (adds axum + tower-http — not mobile) |

Mobile crate uses `default-features = false, features = ["embeddings-local", "sqlite-vec"]`
— excludes all subprocess CLI providers and HTTP.

### Key Patterns

**Tool safety split:** `Agent::new()` registers safe tools only via
`tools::create_safe_tools()` (memory_search, memory_get, web_fetch, web_search,
document_load, wiki, audio_transcribe, spawn_agent). The CLI/daemon inject
dangerous tools (bash, read_file, write_file, edit_file, browser) via
`agent.extend_tools(localgpt_cli_tools::create_cli_tools(&config)?)`. Server
agents and chat bridges intentionally get safe tools only.

**Heartbeat / cron tool injection:** `HeartbeatRunner` and the cron runner
accept an optional `ToolFactory` callback to extend the agent. The CLI daemon
passes `create_cli_tools` so autonomous runs can touch the filesystem and shell.
Without the factory, they run with safe tools only.

**Custom tool sets:** `Agent::new_with_tools()` replaces all tools — used by Gen
mode for its own Bevy/world tools (spawn_entity, modify_entity, etc.).

**Thread safety:** Agent is not `Send+Sync` (SQLite). Use `AgentHandle`
(`Arc<tokio::sync::Mutex<Agent>>`) for mobile/server; the HTTP handler uses
`spawn_blocking`.

**Bevy main thread:** Bevy must own the main thread (macOS windowing/GPU). Gen
mode spawns tokio on a background thread.

**Session compaction:** Approaching context limits triggers a memory flush first
(the LLM saves important context to MEMORY.md before older messages are
truncated). See `agent/compaction.rs`; audited in `localgpt.audit.jsonl`.

**Provider failover:** `agent/failover.rs` can fall back across configured
providers when a primary errors out.

**Memory context:** New sessions auto-load `MEMORY.md`, recent daily logs, and
`HEARTBEAT.md`. Active recall (`memory/active_recall.rs`) can search memory
before each reply and inject results.

**Path expansion & safety:** Tools use `shellexpand::tilde()` for `~`; path
handling and traversal guards live in `agent/path_utils.rs` and
`security/protected_files.rs`.

**Provider routing:** Prefer the explicit `provider/model` form
(OpenClaw-compatible). Bare model names are routed by prefix in
`create_provider()`:

| Model prefix / scheme | Provider |
|-----------------------|----------|
| `claude-cli/*` | Claude CLI (subprocess) |
| `gemini-cli/*` | Gemini CLI (subprocess) |
| `codex-cli/*` | Codex CLI (subprocess) |
| `anthropic/*`, `claude-*` | Anthropic API |
| `openai/*`, `gpt-*`, `o1*` | OpenAI |
| `xai/*`, `grok-*` | xAI |
| `gemini/*`, `gemini-*` | Gemini API key |
| `vertex/*` | Vertex AI |
| `glm/*`, `glm-*` | GLM (Z.AI) |
| `ollama/*`, `local/*` | Ollama |
| `openrouter/*`, `openai-compat/*` | OpenAI-compatible endpoints |

Aliases resolve first (e.g. `opus` → `anthropic/claude-opus-4-6`, `sonnet`,
`gpt`, `glm`, `grok`, `codex`). Default `agent.default_model` is `claude-cli/opus`.

### Core Modules (`crates/core/src/`)

- **agent/providers.rs** — `LLMProvider` trait + implementations: OpenAI,
  OpenAI-compatible (OpenRouter etc.), xAI, Anthropic, Ollama, ClaudeCli,
  GeminiCli, CodexCli, Gemini (API key), Vertex AI, GLM. Includes model-alias
  resolution and per-model pricing.
- **agent/session.rs**, **compaction.rs**, **checkpoint.rs** — conversation
  state, automatic compaction, checkpoint/restore.
- **agent/session_store.rs**, **session_pruning.rs** — session metadata
  persistence and pruning.
- **agent/system_prompt.rs** — system prompt builder (identity, safety,
  workspace, tools, skills).
- **agent/skills.rs** — SKILL.md loading from `workspace/skills/`.
- **agent/tool_filters.rs**, **hardcoded_filters.rs**, **sanitize.rs**,
  **ssrf.rs** — tool input filtering, output sanitization, SSRF protection.
- **agent/approval.rs** — human-in-the-loop tool approval (HTTP approve flow).
- **agent/tools/** — safe tool set: `spawn_agent` (hierarchical delegation),
  `web_search`, plus memory/web/document/wiki/audio tools.
- **memory/** — SQLite FTS5 + sqlite-vec backends (`backend_sqlite.rs`,
  `backend_markdown.rs`, `backend_none.rs`), file watcher, embeddings, query
  expansion, session index, **dreaming** (background consolidation of session
  transcripts into memory), **active_recall**, and **wiki** (claims/evidence
  with staleness tracking).
- **heartbeat/** — autonomous task runner on a configurable interval.
- **cron/** — cron-expression + "every X" job scheduler with overlap
  prevention; each job runs in a fresh agent session.
- **hooks/** — lifecycle shell hooks (before/after_tool_call, on_message,
  on_session_start/end); `before_tool_call` can block.
- **mcp/** — MCP client (stdio + HTTP/SSE transports) that exposes external MCP
  server tools as LocalGPT tools, plus an MCP **server** exposing memory tools.
- **media/** — document loading, audio transcription, TTS, image optimization.
- **outbox.rs** — durable outbound message queue (SQLite-backed) with
  exponential-backoff retry; replays pending messages on daemon startup.
- **config/** — TOML config: `mod.rs` (schema), `migrate.rs`, `watcher.rs`.
  `Config::load()` (desktop), `Config::load_from_dir()` (mobile).
- **paths.rs** — XDG dirs. `Paths::resolve()` (desktop), `Paths::from_root()`
  (mobile). **env.rs** centralizes all `LOCALGPT_*` env var names.
- **commands.rs** — shared slash command definitions (CLI + bridges).
- **concurrency/** — TurnGate (one agent turn at a time) + WorkspaceLock.
- **security/** — LocalGPT.md policy signing/verification, audit log, at-rest
  encryption, protected files.

### Server (`crates/server/src/`)

- **http.rs** — Axum REST/WS API with RustEmbed'd Web UI. Routes include
  `/health`, `/api/status`, `/api/chat`, `/api/memory/search`,
  `/api/memory/stats`, plus the tool-approval endpoint.
- **openai_compat.rs** — OpenAI-compatible API surface so external clients can
  talk to LocalGPT as if it were the OpenAI API.
- **telegram.rs** — Telegram bot with 6-digit pairing auth, streaming edits,
  agent ID `"telegram"`.
- **tls.rs** — TLS termination (see `localgpt cert` and `--no-tls`).
- **rate_limiter.rs**, **security/** — request rate limiting and bridge auth.

### CLI Subcommands (`crates/cli/src/cli/`)

`chat`, `tui`, `ask`, `desktop` (feature-gated), `gen`, `daemon`, `memory`,
`config`, `md` (LocalGPT.md policy), `paths`, `sandbox`, `search`, `init`,
`bridge`, `doctor`, `encrypt`, `tool`/`plugin` (MCP servers), `completion`,
`cron`, `hooks`, `mcp-server` (run as stdio MCP server exposing memory),
`session`, `audit` (inspect compaction audit log), `cert`.

### Gen (3D Scene Generation with Audio + Multiplayer)

**Binary:** `localgpt-gen` — Bevy-based 3D scene generation with procedural
environmental audio. World data uses **`localgpt-world-types`** (serde-only,
zero Bevy/SpacetimeDB deps) so the same types serialize to RON for local saves
and map to SpacetimeDB rows for multiplayer via `crates/spacetime`.

**Audio System:** FunDSP v0.20 synthesis + cpal output, 3-thread model
(Bevy main → audio mgmt thread → cpal callback) with lock-free `Shared<f32>`
params. Ambient (Wind/Rain/Forest/Ocean/Cave/Stream/Silence) and spatial
distance-attenuated emitters (Water/Fire/Hum/Wind/Custom). Auto-inference from
entity names ("campfire", "waterfall"). Tools: `gen_set_ambience`,
`gen_audio_emitter`, `gen_modify_audio`, `gen_audio_info`. See
`docs/gen/gen-audio.md`.

**Behavior System:** Declarative, data-driven animations (all 7 types in
`world-types`): `orbit`, `spin`, `bob`, `look_at`, `pulse`, `path_follow`
(loop/ping-pong/once), `bounce`. Composable (stack on one entity). Tools:
`gen_add_behavior`, `gen_remove_behavior`, `gen_list_behaviors`,
`gen_pause_behaviors`.

**World Skills:** Save/load complete worlds as skill directories — `SKILL.md` +
`world.ron` (entities, shapes, materials, behaviors, audio, avatar, tours
inline). Tools: `gen_save_world`, `gen_load_world` (auto-clears by default),
`gen_clear_scene`. Avatar section defines user presence (spawn, PoV mode, speed,
height, optional model). Tours define guided named waypoint sequences
(walk/fly/teleport).

**WorldGen Pipeline (WG1–WG7):** Blockout-first workflow
(`gen_plan_layout` → `gen_apply_blockout` → `gen_populate_region`), grid navmesh
with A* pathfinding + slope/erosion, hierarchical hero/medium/decorative
placement with collision-aware ground snap, screenshot-based evaluation loop,
incremental blockout editing, semantic scene decomposition, depth/2D preview.
**40+ MCP tools** across P0–P5, WG1–WG7, AI1–AI2. See
`docs/gen/external-services.md` for external services (Ollama, ComfyUI, model
server).

### Mobile

UniFFI proc-macro bindings (`crates/mobile-ffi/`). `LocalGPTClient` owns its own
tokio runtime and wraps `AgentHandle`. Error type: `MobileError` enum (Init,
Chat, Memory, Config). Apple → `apps/apple/scripts/build_apple.sh` (XCFramework +
Swift); Android → `apps/android/scripts/build_android.sh` (cargo-ndk + Kotlin).

## Configuration

Config: `~/.config/localgpt/config.toml` (auto-created on first run, see
`config.example.toml`).

Key settings:
- `agent.default_model` — determines provider. Default: `claude-cli/opus`.
- `agent.subagent_model` — model for spawned subagents (defaults to
  `default_model`).
- `providers.claude_cli.effort` — `low` | `medium` | `high` | `max` (default
  `max`); switch at runtime with the `/effort` slash command.
- `providers.*` — per-provider API keys/endpoints (anthropic, openai, xai,
  gemini, vertex, glm, ollama, openrouter, openai-compat). Supports
  `${ENV_VAR}` interpolation.
- `memory.workspace` — workspace dir. Default
  `~/.local/share/localgpt/workspace`.
- `memory.embedding_provider` — `"local"` (default), `"openai"`, or `"none"`.
- `server.port` — HTTP port (default 31327); TLS via `cert` / `--no-tls`.
- `telegram.enabled` / `telegram.api_token` — Telegram bot.
- `cron` / `hooks` / bridge sections — autonomous jobs, lifecycle hooks, chat
  bridge credentials.

Workspace path resolution: `LOCALGPT_WORKSPACE` env > `LOCALGPT_PROFILE` env >
`memory.workspace` config > `~/.local/share/localgpt/workspace`. All
`LOCALGPT_*` env vars are defined centrally in `crates/core/src/env.rs`.

## Runtime Directory Structure (XDG Base Directory Specification)

```
~/.config/localgpt/                      # XDG_CONFIG_HOME
├── config.toml
~/.local/share/localgpt/                 # XDG_DATA_HOME
├── workspace/                           # Memory workspace
│   ├── MEMORY.md                        # Long-term curated memory
│   ├── HEARTBEAT.md                     # Pending autonomous tasks
│   ├── SOUL.md                          # Persona/tone
│   ├── memory/YYYY-MM-DD.md             # Daily logs
│   ├── knowledge/                       # Knowledge repository
│   └── skills/*/SKILL.md                # Custom skills
├── localgpt.device.key                  # Device key for policy signing
└── skills/                              # Managed skills
~/.local/state/localgpt/                 # XDG_STATE_HOME
├── agents/{agent_id}/sessions/          # Session transcripts (JSONL)
├── localgpt.audit.jsonl                 # Security/compaction audit log
└── logs/                                # Application logs
~/.cache/localgpt/                       # XDG_CACHE_HOME
├── memory/{agent_id}.sqlite             # Search index (regenerable)
└── embeddings/                          # Embedding model cache
```

## Conventions

- **Lint clean before committing:** `cargo clippy --workspace -- -D warnings`
  and `cargo fmt --check` must pass (enforced in CI).
- **Keep `localgpt-core` portable:** never add platform/desktop-only deps to it;
  put them in `cli`, `cli-tools`, `server`, `sandbox`, or `gen`. Verify with the
  mobile `cargo check` targets above.
- **Dangerous tools belong in `cli-tools`,** never in core's safe tool set.
- **Edition 2024**, workspace-shared version (`0.3.x`), `resolver = "3"`.
- Related docs live under `docs/` (architecture, gen, mobile, security, rfcs,
  roadmap). `AGENTS.md` is the longer-form contributor guide.
