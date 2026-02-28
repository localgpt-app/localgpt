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
├── server/      # localgpt-server — HTTP/WS API, Telegram bot, BridgeManager
├── sandbox/     # localgpt-sandbox — Landlock/Seatbelt process sandboxing
├── mobile-ffi/  # localgpt-mobile-ffi — UniFFI bindings for iOS/Android
├── gen/         # localgpt-gen — Bevy 3D scene generation binary
└── bridge/      # localgpt-bridge — secure IPC protocol for bridge daemons

bridges/         # Standalone bridge binaries
├── telegram/    # localgpt-bridge-telegram — Telegram bot daemon
├── discord/     # localgpt-bridge-discord — Discord bot daemon
└── whatsapp/    # localgpt-bridge-whatsapp — WhatsApp bridge daemon

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
│                          BRIDGE DAEMONS                                  │
│  ┌───────────────────┐  ┌───────────────────┐  ┌───────────────────┐   │
│  │ bridge-telegram   │  │ bridge-discord    │  │ bridge-whatsapp   │   │
│  │ (teloxide)        │  │ (serenity)        │  │ (baileys/Node)    │   │
│  └─────────┬─────────┘  └─────────┬─────────┘  └─────────┬─────────┘   │
│            │                      │                      │              │
│            └──────────────────────┼──────────────────────┘              │
│                                   │                                     │
│                                   ▼                                     │
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
| `localgpt-server` | lib | core, bridge | HTTP server, Telegram bot, BridgeManager |
| `localgpt` | bin | core, server, sandbox | CLI binary with all features |
| `localgpt-gen` | bin | core | 3D scene generation with Bevy |
| `localgpt-mobile-ffi` | lib+bin | core (minimal) | UniFFI bindings for iOS/Android |
| `localgpt-bridge-telegram` | bin | core, bridge | Telegram bot daemon |
| `localgpt-bridge-discord` | bin | core, bridge | Discord bot daemon |
| `localgpt-bridge-whatsapp` | bin | core, bridge | WhatsApp bridge daemon |

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
- **Telegram bot**: Streaming responses via teloxide
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

## Bridge Daemons

Standalone binaries that connect to the main LocalGPT daemon:

| Bridge | Library | Notes |
|--------|---------|-------|
| Telegram | teloxide | Streaming with edit updates |
| Discord | serenity | Gateway client |
| WhatsApp | baileys (Node.js) | Embedded process + webhooks |

All bridges use the same IPC protocol defined in `localgpt-bridge`.

## Design Principles

1. **Mobile compatibility** — `localgpt-core` compiles for iOS/Android with no desktop deps
2. **Feature flags** — Toggle embeddings providers, desktop-only features
3. **Bridge isolation** — Messaging daemons run as separate processes
4. **Graceful degradation** — Sandbox falls back on unsupported systems
