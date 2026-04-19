---
sidebar_position: 1
slug: /intro
---

# Introduction

LocalGPT is a **local AI assistant with persistent memory, semantic search, and autonomous operation** — built in Rust, inspired by OpenClaw. A single binary gives you a CLI, desktop app, embedded web UI, and HTTP API — all keeping your data on your machine.

## Key Features

- **Local & Private** - Single Rust binary. All data stays on your machine. No cloud storage, no telemetry.
- **Hybrid Memory Search** - Markdown-based knowledge store with pluggable backends (SQLite FTS5, Markdown grep, or disabled). SQLite backend supports full-text search with AND matching and rank-based scoring plus local vector embeddings (fastembed) for semantic search
- **Desktop App** - Optional native desktop GUI built with egui — chat, sessions, memory browser, and status dashboard. Disable with `--no-default-features` for headless/Docker deployments.
- **Embedded Web UI** - Browser-based chat interface served directly from the binary
- **Multi-Provider Support** - Works with Claude CLI, Anthropic API, OpenAI, Ollama, and GLM (Z.AI) — all with full tool calling support
- **Telegram, Discord & WhatsApp** - Access LocalGPT from Telegram, Discord, or WhatsApp via bridge binaries with full chat, tool use, and memory support — secured with one-time pairing auth
- **Autonomous Heartbeat** - Daemon mode with scheduled background tasks that run automatically
- **Skills System** - Extensible skills for specialized tasks
- **Shell Sandbox** - Kernel-level isolation (Landlock + seccomp + Seatbelt) on every shell command. Zero configuration, enabled by default, graceful degradation. [Not a guarantee](/docs/sandbox#limitations) — defense in depth.
- **Standing Instructions** - Cryptographically signed `LocalGPT.md` for persistent, end-of-context directives — coding conventions, security boundaries, workflow preferences — with HMAC-SHA256 tamper detection
- **Session Management** - Multi-session support with automatic context compaction
- **HTTP API & WebSocket** - RESTful API and real-time WebSocket for integrations

## Architecture Overview

LocalGPT follows the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/) on Linux and macOS, and uses the native Known Folder IDs on Windows:

### Linux / macOS

```text
~/.config/localgpt/
└── config.toml                  # Configuration file

~/.local/share/localgpt/
├── workspace/
│   ├── MEMORY.md                # Curated long-term knowledge
│   ├── HEARTBEAT.md             # Pending autonomous tasks
│   ├── LocalGPT.md              # Standing instructions
│   └── memory/
│       └── YYYY-MM-DD.md        # Conversation logs
└── localgpt.device.key          # HMAC signing key (0600)

~/.local/state/localgpt/
├── logs/
│   └── localgpt-YYYY-MM-DD.log   # Daily application logs
└── localgpt.audit.jsonl         # Append-only audit log

~/.cache/localgpt/
└── embeddings/                  # Downloaded embedding models
```

### Windows

Windows has no separate config/state directories, so config, data, and state all share `%APPDATA%\localgpt` (Roaming AppData, `FOLDERID_RoamingAppData`). Cache uses `%LOCALAPPDATA%\localgpt` (`FOLDERID_LocalAppData`).

```text
%APPDATA%\localgpt\              # e.g. C:\Users\<you>\AppData\Roaming\localgpt
├── config.toml                  # Configuration file
├── workspace\
│   ├── MEMORY.md                # Curated long-term knowledge
│   ├── HEARTBEAT.md             # Pending autonomous tasks
│   ├── LocalGPT.md              # Standing instructions
│   └── memory\
│       └── YYYY-MM-DD.md        # Conversation logs
├── localgpt.device.key          # HMAC signing key
├── agents\<id>\sessions\        # Session transcripts (JSONL)
├── logs\
│   └── localgpt-YYYY-MM-DD.log   # Daily application logs
└── localgpt.audit.jsonl         # Append-only audit log

%LOCALAPPDATA%\localgpt\          # e.g. C:\Users\<you>\AppData\Local\localgpt
├── memory\<agent-id>.sqlite     # Search index (rebuildable)
└── embeddings\                  # Downloaded embedding models

%TEMP%\localgpt-<USERNAME>\       # Runtime: PID file, IPC locks
```

## How It Works

1. **Chat Sessions** - Start interactive conversations that maintain context
2. **Memory System** - Important information is saved to markdown files and indexed for search
3. **Tool Execution** - The AI can execute bash commands, read/write files, and search memory
4. **Heartbeat** - Background process checks `HEARTBEAT.md` for pending tasks

## Supported Models

LocalGPT automatically detects the provider based on model name prefix:

| Prefix | Provider | Examples |
|--------|----------|----------|
| `openai/*` | LM Studio / OpenAI | openai/qwen/qwen3.5-35b-a3b, openai/gpt-4o |
| Other | Ollama (local) | llama3, mistral, codellama |
| `claude-cli/*` | Claude CLI | claude-cli/opus, claude-cli/sonnet |
| `anthropic/*` | Anthropic API | anthropic/claude-opus-4-5, anthropic/claude-sonnet-4-5 |
| `glm/*` or `glm` | GLM (Z.AI) | glm/glm-4.7, glm |
| Aliases | Any | opus, sonnet, gpt, gpt-mini |

## Next Steps

- [Installation](/docs/installation) - Install LocalGPT on your system
- [Quick Start](/docs/quick-start) - Get up and running in minutes
- [Architecture](/docs/architecture) - Understand the crate structure and dependencies
- [CLI Commands](/docs/cli-commands) - Learn the available commands
- [Shell Sandbox](/docs/sandbox) - Understand the security sandbox
- [LocalGPT.md](/docs/localgpt) - Your standing instructions to the AI
