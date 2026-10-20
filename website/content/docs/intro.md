---
slug: "/intro"
title: "Introduction"
description: "What LocalGPT is: a local AI agent with persistent memory, and a desktop app where prompts, documents, songs and screenplays become .world packages that people and agents edit together."
---

# Introduction

LocalGPT is two things that meet in the middle, both open source (Apache 2.0) and both running on your machine.

**An AI agent that lives where your data lives.** `localgpt` talks to many LLM providers — including a model running on the same machine — keeps its memory in plain Markdown, does its work through tools behind a kernel-enforced sandbox, and can run unattended: a daemon, a heartbeat, cron jobs. Nothing leaves the machine unless you send it.

**A desktop app where things you already have become places.** A prompt, a Markdown document, a song or a screenplay opens as a 3D world in one window. A world is a `.world` package — the [Open World Format](https://openworldformat.org): the world now, the log of every change made to it, and its assets. People edit it in the window; agents edit it from outside through a small local API; every change is logged, attributed, undoable and branchable.

The two halves meet at a simple point: an agent that can edit files is one command away from an agent that edits worlds, and the worlds are a format no engine owns.

## The pieces

| | What it is | Get it |
|---|---|---|
| [**LocalGPT**, the desktop app](/docs/app/) | One window, every mode: a world from a prompt (Gen), from a document (MD) or from a song (Verse), or any `.world` opened as a live canvas | `cargo run -p localgpt-app` from a checkout; `LocalGPT.app` from `apps/app-desktop/` |
| [**LocalGPT Gen**](/docs/gen/) | The world builder as a binary: interactive, an MCP server, headless generation, collaborative sessions, the live canvas | `cargo install localgpt-gen` |
| [**Previs**](/docs/gen/previs/) | A screenplay in; per scene a `.world` with set, cast and coverage, plus a board and a shot list. Deterministic, no model | [localgpt.md](https://localgpt.md) in the browser, or `cargo install localgpt-previs` |
| [**The assistant**](/docs/quick-start/) | Chat, TUI, daemon, memory, heartbeat and cron, the sandbox, an HTTP API, an MCP server, and `localgpt world` — a `.world` package's command line | `cargo install localgpt` |
| [**localgpt.world**](https://localgpt.world) | The gallery, and the one web viewer every exported page carries | a browser |

## Where to start

- **Walk into something.** [The desktop app](/docs/app/): open a document, a song or a prompt, and keep the window open while an agent changes the world from outside — the [live canvas](/docs/gen/live-canvas/).
- **Previs a scene.** Drop script pages on [localgpt.md](https://localgpt.md); nothing is installed or uploaded. Then open one scene live and revise the shots by asking — [Previs](/docs/gen/previs/).
- **Drive the 3D window from your own agent.** `localgpt-gen mcp-server` for Claude Desktop, Claude Code, Codex, Gemini CLI and editors — [MCP Server](/docs/gen/mcp-server/).
- **Run the assistant.** [Installation](/docs/installation/) and the [Quick Start](/docs/quick-start/).

## The assistant's features

- **Local & private** — single Rust binary; all data stays on your machine; no telemetry.
- **Hybrid memory search** — a Markdown knowledge store with pluggable backends (SQLite FTS5, Markdown grep, or none). The SQLite backend combines full-text search with local vector embeddings (fastembed) for semantic recall; active recall searches memory before each reply, and background consolidation turns session transcripts into memory.
- **Autonomous operation** — a heartbeat that works through `HEARTBEAT.md`, cron jobs that each run in a fresh session, a durable outbox with retry.
- **Multi-provider** — Claude CLI, Gemini CLI and Codex CLI as backends with their own login; Anthropic, OpenAI, xAI, Gemini, Vertex AI and GLM APIs; Ollama, LM Studio and any OpenAI-compatible endpoint — all with tool calling.
- **Shell sandbox** — kernel-level isolation (Landlock + seccomp on Linux, Seatbelt on macOS, containers as a backend) on every shell command. Zero configuration, enabled by default, graceful degradation. [Not a guarantee](/docs/sandbox#limitations) — defense in depth.
- **Standing instructions** — a plain `POLICY.md` of persistent directives — coding conventions, security boundaries, workflow preferences — sanitized before injection and protected from agent self-modification.
- **MCP in both directions** — it consumes external MCP tool servers, and `localgpt mcp-server` offers its memory and web tools to other agents.
- **Interfaces** — CLI, TUI, an embedded web UI, a desktop GUI, an HTTP/WebSocket API with an OpenAI-compatible surface, iOS/macOS and Android apps over UniFFI, and bridge daemons over a secure local IPC socket.
- **Sessions** — multi-session support with automatic compaction, checkpoints and branching.
- **The world's command line** — `localgpt world` commits batches to a `.world`, reads its log and tips, undoes, verifies, and `localgpt world mcp` serves an open canvas's tools to any MCP agent. [CLI Commands](/docs/cli-commands/#world).

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
│   ├── POLICY.md                # Standing instructions
│   └── memory/
│       └── YYYY-MM-DD.md        # Conversation logs
├── models/
│   ├── llm/                     # The shared GGUF (scripts/fetch-model.sh), read by every app
│   └── pack/                    # The shared CC0 asset pack (scripts/fetch-assets.sh)
└── localgpt.device.key          # Device key for bridge credential encryption (0600)

~/.local/state/localgpt/
├── logs/
│   └── localgpt-YYYY-MM-DD.log   # Daily application logs
├── gen-settings.json            # The world apps' own settings (model menu, last opened)
└── agents/<id>/sessions/        # Session transcripts (JSONL)

~/.cache/localgpt/
└── embeddings/                  # Downloaded embedding models
```

The world apps — the desktop app and Gen — keep their own settings and workspace (`gen-settings.json`, the `gen` agent) and read no `config.toml`, so nothing from the assistant's memory reaches a world unless you put it there.

### Windows

Windows has no separate config/state directories, so config, data, and state all share `%APPDATA%\localgpt` (Roaming AppData, `FOLDERID_RoamingAppData`). Cache uses `%LOCALAPPDATA%\localgpt` (`FOLDERID_LocalAppData`).

```text
%APPDATA%\localgpt\              # e.g. C:\Users\<you>\AppData\Roaming\localgpt
├── config.toml                  # Configuration file
├── workspace\
│   ├── MEMORY.md                # Curated long-term knowledge
│   ├── HEARTBEAT.md             # Pending autonomous tasks
│   ├── POLICY.md                # Standing instructions
│   └── memory\
│       └── YYYY-MM-DD.md        # Conversation logs
├── localgpt.device.key          # Device key for bridge credential encryption
├── agents\<id>\sessions\        # Session transcripts (JSONL)
└── logs\
    └── localgpt-YYYY-MM-DD.log   # Daily application logs

%LOCALAPPDATA%\localgpt\          # e.g. C:\Users\<you>\AppData\Local\localgpt
├── memory\<agent-id>.sqlite     # Search index (rebuildable)
└── embeddings\                  # Downloaded embedding models

%TEMP%\localgpt-<USERNAME>\       # Runtime: PID file, IPC locks
```

## How It Works

1. **Chat sessions** — interactive conversations that keep context, compact automatically and can be branched.
2. **Memory** — important information is saved to Markdown files and indexed for search; relevant memories are recalled before each reply.
3. **Tools** — the agent runs shell commands in the sandbox, reads and writes files, searches memory and the web, and calls external MCP servers.
4. **Heartbeat and cron** — the daemon works through `HEARTBEAT.md` on an interval and runs scheduled jobs.
5. **Worlds** — `localgpt world` and `localgpt world mcp` put the same agent, or any other, on a `.world` package.

## Supported Models

Models are named `provider/model`. The prefix picks the provider; a bare model name is routed by its prefix too:

| Prefix | Provider | Examples |
|--------|----------|----------|
| `claude-cli/*` | Claude CLI (subprocess, its own login) | `claude-cli/opus` (the default), `claude-cli/sonnet` |
| `gemini-cli/*` | Gemini CLI (subprocess) | `gemini-cli/gemini-2.5-pro` |
| `codex-cli/*` | Codex CLI (subprocess) | `codex-cli/o4-mini` |
| `anthropic/*`, `claude-*` | Anthropic API | `anthropic/claude-opus-4-6`, `anthropic/claude-sonnet-4-6` |
| `openai/*`, `gpt-*`, `o1*` | OpenAI, or LM Studio with a `base_url` | `openai/gpt-4o`, `openai/qwen/qwen3.5-35b-a3b` |
| `openrouter/*`, `openai-compat/*` | OpenAI-compatible endpoints (OpenRouter, DeepSeek, Groq, vLLM) | `openrouter/deepseek/deepseek-chat` |
| `xai/*`, `grok-*` | xAI | `xai/grok-3-mini` |
| `gemini/*`, `gemini-*` | Gemini API key | `gemini/gemini-2.5-pro` |
| `vertex/*` | Google Vertex AI | `vertex/gemini-2.5-pro` |
| `glm/*`, `glm-*` | GLM (Z.AI) | `glm/glm-4.7` |
| `ollama/*`, `local/*` | Ollama | `ollama/llama3` |
| `gguf/*` | A GGUF run in-process — Gen and the desktop app built with `local-llm` | `gguf/default` |
| Aliases | — | `opus`, `sonnet`, `gpt`, `glm`, `grok`, `codex` |

## Next Steps

- [Installation](/docs/installation) — install the binaries, or build the desktop app from source
- [The desktop app](/docs/app) — one window, every mode
- [Quick Start](/docs/quick-start) — chatting with the assistant in minutes
- [Live canvas](/docs/gen/live-canvas) — a `.world` that agents edit while you watch
- [Previs](/docs/gen/previs) — from a screenplay to a board to a canvas
- [Architecture](/docs/architecture) — the crates and how they depend on each other
- [CLI Commands](/docs/cli-commands) — every subcommand, including `localgpt world`
- [Shell Sandbox](/docs/sandbox) and [POLICY.md](/docs/policy) — the security model
