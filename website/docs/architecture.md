---
sidebar_position: 1
slug: /architecture
---

# Architecture

LocalGPT is built as a Cargo workspace with modular crates, designed for local-first operation with optional mobile support.

## Workspace Structure

```
crates/
├── core/        # localgpt-core — shared library (agent, memory, config, security)
├── cli/         # localgpt — binary with clap CLI, desktop GUI, dangerous tools
├── server/      # localgpt-server — HTTP/WS API, BridgeManager
├── sandbox/     # localgpt-sandbox — Landlock/Seatbelt process sandboxing
├── mobile-ffi/  # localgpt-mobile-ffi — UniFFI bindings for iOS/Android
├── gen/         # localgpt-gen — Bevy 3D scene generation binary
└── bridge/      # localgpt-bridge — secure IPC protocol for bridge daemons

bridges/         # Standalone bridge binaries
└── cli/         # localgpt-bridge-cli — terminal chat client (connects to the daemon over IPC)

apps/            # Native mobile app projects
├── ios/         # Swift iOS app with UniFFI bindings
└── android/     # Kotlin Android app with UniFFI bindings
```

## Dependency Graph

```
                      ┌─────────────────┐
                      │ localgpt-core   │  ← No internal deps, mobile-compatible
                      └────────┬────────┘
                               │
       ┌───────────────────────┼───────────────────────┬───────────────┐
       │                       │                       │               │
       ▼                       ▼                       ▼               ▼
┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  ┌───────────────┐
│ localgpt-bridge │  │ localgpt-sandbox│  │ localgpt-gen    │  │ mobile-ffi    │
│ (IPC protocol)  │  │ (process jail)  │  │ (3D + audio)    │  │ (UniFFI)      │
└────────┬────────┘  └────────┬────────┘  └─────────────────┘  └───────────────┘
         │                    │
         ▼                    │
┌─────────────────┐           │
│ localgpt-server │◄──────────┘
│ (HTTP + Bridge) │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ localgpt (CLI)  │
│ (end-user bin)  │
└─────────────────┘

┌─────────────────────────────────────────────────────────────────────────┐
│                          BRIDGE DAEMON                                   │
│                        ┌───────────────────┐                            │
│                        │    bridge-cli     │                            │
│                        │    (rustyline)    │                            │
│                        └─────────┬─────────┘                            │
│                                  │                                       │
│                                  ▼                                       │
│                        ┌─────────────────┐                              │
│                        │ localgpt-bridge │  ← Unix socket IPC           │
│                        └─────────────────┘                              │
└─────────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────────┐
│                            MOBILE APPS                                   │
│  ┌───────────────────┐              ┌───────────────────┐               │
│  │     iOS App       │              │   Android App     │               │
│  │     (Swift)       │              │    (Kotlin)       │               │
│  └─────────┬─────────┘              └─────────┬─────────┘               │
│            │                                  │                         │
│            ▼                                  ▼                         │
│  ┌───────────────────┐              ┌───────────────────┐               │
│  │  Swift Bindings   │              │  Kotlin Bindings  │               │
│  │  (uniffi-bindgen) │              │  (uniffi-bindgen) │               │
│  └─────────┬─────────┘              └─────────┬─────────┘               │
│            │                                  │                         │
│            └──────────────────┬───────────────┘                         │
│                               │                                         │
│                               ▼                                         │
│                    ┌─────────────────┐                                  │
│                    │ mobile-ffi      │  ← core with local embeddings    │
│                    └─────────────────┘                                  │
└─────────────────────────────────────────────────────────────────────────┘
```

## Crate Summary

| Crate | Type | Dependencies | Purpose |
|-------|------|--------------|---------|
| `localgpt-core` | lib | None | Agent, memory, config, security |
| `localgpt-bridge` | lib | None | IPC protocol for bridge daemons |
| `localgpt-sandbox` | lib | core | Landlock/Seatbelt process isolation |
| `localgpt-server` | lib | core, bridge | HTTP server, BridgeManager |
| `localgpt` | bin | core, server, sandbox | CLI binary with all features |
| `localgpt-gen` | bin | core | 3D scene generation with Bevy |
| `localgpt-mobile-ffi` | lib+bin | core (minimal) | UniFFI bindings for iOS/Android |
| `localgpt-bridge-cli` | bin | core, bridge | Terminal chat client (daemon IPC) |

## Core Libraries

### `localgpt-core`

Foundation library with zero platform-specific dependencies:

- **Agent**: LLM provider abstraction (OpenAI, Anthropic, Ollama, Claude CLI, GLM)
- **Memory**: SQLite FTS5 + markdown files + vector embeddings
- **Config**: TOML configuration with XDG path resolution
- **Security**: HMAC signing, policy verification, audit logging
- **Heartbeat**: Autonomous task runner based on `HEARTBEAT.md`
- **Session**: Conversation management with automatic compaction

### `localgpt-bridge`

IPC protocol for daemon-to-bridge communication:

- tarpc-based async RPC
- Peer identity verification (Unix UID/GID)
- Secure credential exchange
- Cross-platform: Unix sockets + Windows named pipes

## Desktop Components

### `localgpt-server`

HTTP/WebSocket server and daemon services:

- **Axum HTTP**: REST API + embedded Web UI (RustEmbed)
- **BridgeManager**: Unix socket server for bridge daemons
- **WebSocket**: Real-time chat streaming

### `localgpt-sandbox`

Kernel-level process isolation:

| Platform | Technology |
|----------|------------|
| Linux | Landlock + seccomp |
| macOS | Seatbelt (sandbox-init) |
| Windows | Restricted tokens |

Falls back gracefully on unsupported systems.

## Binaries

### `localgpt` (CLI)

Primary user-facing binary with commands:

| Command | Purpose |
|---------|---------|
| `chat` | Interactive conversation |
| `ask` | Single question |
| `daemon` | HTTP server + heartbeat |
| `memory` | Search/manage memory |
| `config` | View/edit configuration |
| `bridge` | Register bridge credentials |
| `gen` | 3D scene generation |

### `localgpt-gen`

3D scene generation with Bevy:

- Entity spawning and modification tools
- Procedural environmental audio (FunDSP)
- glTF/GLB scene export

## System Prompt

All binaries share the same base system prompt, built by `build_system_prompt()` in `localgpt-core`. This function assembles identity, safety guidelines, tool descriptions, workspace info, and runtime metadata. On each new session, the agent loads workspace context (SOUL.md, MEMORY.md, daily logs, HEARTBEAT.md) and appends it to the system prompt.

### What's shared

Every binary — CLI, Gen, server, mobile, bridge daemons — gets the same core prompt through `Agent::new_session()`. This includes:

- Identity and safety guidelines
- Tool documentation (generated from the registered tool set)
- Workspace directory and current time
- Memory file conventions
- Runtime info (model, hostname, OS)

### What differs

| | CLI | Gen | Server / Mobile |
|---|---|---|---|
| **Agent creation** | `Agent::new()` — async, connects MCP servers | `Agent::new_with_tools()` — sync, tools provided upfront | `Agent::new()` — safe tools only |
| **Tools** | Safe tools + CLI tools (bash, file ops) + spawn_agent | Safe tools + gen3d tools (spawn_entity, etc.); interactive mode also adds CLI tools | Safe tools only (no file system access) |
| **Extra prompts** | None | `GEN_MEMORY_PROMPT` (creative memory guidance) added as a user message; `HEADLESS_EXPERIMENT_PROMPT` for headless mode | None |

Gen's additional prompts live in `crates/gen/src/gen3d/system_prompt.rs` and are injected as **user messages** in the chat history, not modifications to the system prompt itself. This keeps the core prompt identical while giving Gen domain-specific guidance for creative world-building and memory usage.

## Mobile

### `localgpt-mobile-ffi`

UniFFI bindings for iOS/Android:

```rust
// Exposed to Swift/Kotlin
pub struct LocalGPTClient {
    // Wraps Arc<Mutex<Agent>>
}
```

Build outputs:
- **iOS**: `liblocalgpt_mobile.a` + Swift bindings → XCFramework
- **Android**: `liblocalgpt_mobile.so` + Kotlin bindings → AAR

Uses `embeddings-local` + `sqlite-vec` features (local embeddings work on mobile).

## Bridge Daemon

A standalone binary that connects to the main LocalGPT daemon:

| Bridge | Library | Notes |
|--------|---------|-------|
| CLI | rustyline | Interactive terminal chat |

The bridge uses the IPC protocol defined in `localgpt-bridge`.

## Design Principles

1. **Mobile compatibility** — `localgpt-core` compiles for iOS/Android with no desktop deps
2. **Feature flags** — Toggle embeddings providers, desktop-only features
3. **Bridge isolation** — Messaging daemons run as separate processes
4. **Graceful degradation** — Sandbox falls back on unsupported systems
