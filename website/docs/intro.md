---
sidebar_position: 1
slug: /intro
---

# Introduction

LocalGPT is a **local AI assistant with persistent memory, semantic search, and autonomous operation** — built in Rust, inspired by OpenClaw. A single binary gives you a CLI, desktop app, embedded web UI, and HTTP API — all keeping your data on your machine.

## Key Features

- **Local & Private** - Single Rust binary. All data stays on your machine. No cloud storage, no telemetry.
- **Hybrid Memory Search** - Markdown-based knowledge store with SQLite FTS5 full-text search (with AND matching and rank-based scoring) and local vector embeddings (fastembed) for semantic search
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

LocalGPT follows the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/):

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
│   └── agent.log                # Application logs
└── localgpt.audit.jsonl         # Append-only audit log

~/.cache/localgpt/
└── embeddings/                  # Downloaded embedding models
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
| `claude-cli/*` | Claude CLI | claude-cli/opus, claude-cli/sonnet |
| `anthropic/*` | Anthropic API | anthropic/claude-opus-4-5, anthropic/claude-sonnet-4-5 |
| `openai/*` | OpenAI | openai/gpt-4o, openai/gpt-4o-mini |
| `glm/*` or `glm` | GLM (Z.AI) | glm/glm-4.7, glm |
| Aliases | Any | opus, sonnet, gpt, gpt-mini |
| Other | Ollama (with tool calling) | llama3, mistral, codellama |

## Next Steps

- [Installation](/docs/installation) - Install LocalGPT on your system
- [Quick Start](/docs/quick-start) - Get up and running in minutes
- [CLI Commands](/docs/cli-commands) - Learn the available commands
- [Shell Sandbox](/docs/sandbox) - Understand the security sandbox
- [LocalGPT.md](/docs/localgpt) - Your standing instructions to the AI
