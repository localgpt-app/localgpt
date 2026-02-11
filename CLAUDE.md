# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
# Build
cargo build              # Debug build
cargo build --release    # Release build (~27MB binary)

# Run
cargo run -- <subcommand>   # Run with arguments
cargo run -- chat           # Interactive chat
cargo run -- ask "question" # Single question
cargo run -- daemon start   # Start daemon with HTTP server

# Test
cargo test                  # Run all tests
cargo test <test_name>      # Run specific test
cargo test -- --nocapture   # Show test output

# Lint
cargo clippy
cargo fmt --check
```

## Architecture

LocalGPT is a local-only AI assistant with persistent markdown-based memory and optional autonomous operation via heartbeat.

### Core Modules (`src/`)

- **agent/** - LLM interaction layer
  - `providers.rs` - Trait `LLMProvider` with implementations for OpenAI, Anthropic, Ollama, Claude CLI, and GLM (Z.AI). Model prefix determines provider (`claude-cli/*` → Claude CLI, `gpt-*` → OpenAI, `claude-*` → Anthropic API, `glm-*` → GLM, else Ollama)
  - `session.rs` - Conversation state with automatic compaction when approaching context window limits
  - `session_store.rs` - Session metadata store (`sessions.json`) with CLI session ID persistence
  - `system_prompt.rs` - Builds system prompt with identity, safety, workspace info, tools, skills, and special tokens
  - `skills.rs` - Loads SKILL.md files from workspace/skills/ for specialized task handling
  - `tools.rs` - Agent tools: `bash`, `read_file`, `write_file`, `edit_file`, `memory_search`, `memory_get`, `web_fetch`

- **memory/** - Markdown-based knowledge store
  - `index.rs` - SQLite FTS5 index for fast search. Chunks files (~400 tokens with 80 token overlap)
  - `watcher.rs` - File system watcher for automatic reindexing
  - `workspace.rs` - Auto-creates workspace templates on first run (MEMORY.md, HEARTBEAT.md, SOUL.md, .gitignore)
  - Files: `MEMORY.md` (curated knowledge), `HEARTBEAT.md` (pending tasks), `memory/YYYY-MM-DD.md` (daily logs)

- **heartbeat/** - Autonomous task runner
  - `runner.rs` - Runs on configurable interval within active hours. Reads `HEARTBEAT.md` and executes pending tasks

- **server/** - HTTP/WebSocket API and Telegram bot
  - `http.rs` - Axum-based REST API. Note: creates new Agent per request (no session persistence via HTTP)
  - `telegram.rs` - Telegram bot interface with one-time pairing auth, per-chat sessions, streaming responses with debounced message edits (2s), and full tool support
  - Endpoints: `/health`, `/api/status`, `/api/chat`, `/api/memory/search`, `/api/memory/stats`

- **config/** - TOML configuration at `~/.localgpt/config.toml`
  - Supports `${ENV_VAR}` expansion in API keys
  - `workspace_path()` returns expanded memory workspace path
  - `migrate.rs` - Auto-migrates from OpenClaw's `~/.openclaw/config.json5` if LocalGPT config doesn't exist

- **commands.rs** - Shared slash command definitions used by both CLI chat and Telegram bot

- **cli/** - Clap-based subcommands: `chat`, `ask`, `daemon`, `memory`, `config`

### Key Patterns

- Agent is not `Send+Sync` due to SQLite connections - HTTP handler uses `spawn_blocking`
- Session compaction triggers memory flush (prompts LLM to save important context before truncating)
- Memory context automatically loaded into new sessions: `MEMORY.md`, recent daily logs, `HEARTBEAT.md`
- Tools use `shellexpand::tilde()` for path expansion

## Configuration

Default config path: `~/.localgpt/config.toml` (see `config.example.toml`)

**Auto-creation**: If no config file exists on first run, LocalGPT automatically creates a default config with helpful comments. If an OpenClaw config exists at `~/.openclaw/config.json5`, it will be migrated automatically.

Key settings:
- `agent.default_model` - Model name (determines provider). Default: `claude-cli/opus`. Supported: Anthropic (`anthropic/claude-*`), OpenAI (`openai/gpt-*`), GLM/Z.AI (`glm/glm-4.7` or alias `glm`), Claude CLI (`claude-cli/*`), Ollama (`ollama/*`)
- `agent.context_window` / `reserve_tokens` - Context management
- `memory.workspace` - Workspace directory path. Default: `~/.localgpt/workspace`
- `heartbeat.interval` - Duration string (e.g., "30m", "1h")
- `heartbeat.active_hours` - Optional `{start, end}` in "HH:MM" format
- `server.port` - HTTP server port (default: 31327)
- `telegram.enabled` - Enable Telegram bot (default: false)
- `telegram.api_token` - Telegram Bot API token (supports `${TELEGRAM_BOT_TOKEN}`)

### Telegram Bot

The Telegram bot runs as a background task inside the daemon (`localgpt daemon start`). It provides the same chat capabilities as the CLI, including tool use and memory access.

**Setup:**

1. Create a bot via [@BotFather](https://t.me/BotFather) and get the API token
2. Configure in `~/.localgpt/config.toml`:
   ```toml
   [telegram]
   enabled = true
   api_token = "${TELEGRAM_BOT_TOKEN}"
   ```
3. Start the daemon: `localgpt daemon start`
4. Message your bot on Telegram — it will generate a 6-digit pairing code printed to the daemon console/logs
5. Send the code back to the bot to complete pairing

**Pairing & Auth:**
- First message triggers a one-time pairing flow with a 6-digit code
- Code is printed to stdout and daemon logs
- Only the paired user can interact with the bot
- Pairing persists in `~/.localgpt/telegram_paired_user.json`

**Telegram Commands:**
- `/help` - Show available commands
- `/new` - Start fresh session
- `/skills` - List available skills
- `/model [name]` - Show or switch model
- `/compact` - Compact session history
- `/clear` - Clear session history
- `/memory <query>` - Search memory files
- `/status` - Show session info
- `/unpair` - Remove pairing (re-enables pairing flow)

**Implementation notes:**
- Uses agent ID `"telegram"` (separate sessions from CLI's `"main"`)
- Messages >4096 chars are split into chunks for Telegram's limit
- Streaming responses are shown via debounced message edits (every 2s)
- Tool calls are displayed with extracted details during streaming
- Shares `TurnGate` concurrency control with HTTP server and heartbeat

### Workspace Path Customization (OpenClaw-Compatible)

Workspace path resolution order:
1. `LOCALGPT_WORKSPACE` env var - absolute path override
2. `LOCALGPT_PROFILE` env var - creates `~/.localgpt/workspace-{profile}`
3. `memory.workspace` from config file
4. Default: `~/.localgpt/workspace`

Examples:
```bash
# Use a custom workspace directory
export LOCALGPT_WORKSPACE=~/my-project/ai-workspace
localgpt chat

# Use profile-based workspaces (like OpenClaw's OPENCLAW_PROFILE)
export LOCALGPT_PROFILE=work    # uses ~/.localgpt/workspace-work
export LOCALGPT_PROFILE=home    # uses ~/.localgpt/workspace-home
```

## Skills System (OpenClaw-Compatible)

Skills are SKILL.md files that provide specialized instructions for specific tasks.

### Skill Sources (Priority Order)

1. **Workspace skills**: `~/.localgpt/workspace/skills/` (highest priority)
2. **Managed skills**: `~/.localgpt/skills/` (user-level, shared across workspaces)

### SKILL.md Format

```yaml
---
name: github-pr
description: "Create and manage GitHub PRs"
user-invocable: true              # Expose as /github-pr command (default: true)
disable-model-invocation: false   # Include in model prompt (default: false)
command-dispatch: tool            # Optional: direct tool dispatch
command-tool: bash                # Tool name for dispatch
metadata:
  openclaw:
    emoji: "🐙"
    always: false                 # Skip eligibility checks
    requires:
      bins: ["gh", "git"]         # Required binaries (all must exist)
      anyBins: ["python", "python3"]  # At least one required
      env: ["GITHUB_TOKEN"]       # Required environment variables
---

# GitHub PR Skill

Instructions for the agent on how to create PRs...
```

### Skill Features

| Feature | Description |
|---------|-------------|
| **Slash commands** | Invoke skills via `/skill-name [args]` |
| **Requirements gating** | Skills blocked if missing binaries/env vars |
| **Model prompt filtering** | `disable-model-invocation: true` hides from model but keeps command |
| **Multiple sources** | Workspace skills override managed skills of same name |
| **Emoji display** | Show emoji in `/skills` list and `/help` |

### CLI Commands

- `/skills` - List all skills with eligibility status
- `/skill-name [args]` - Invoke a skill directly

## CLI Commands (Interactive Chat)

In the `chat` command, these slash commands are available:

- `/help` - Show available commands
- `/quit`, `/exit`, `/q` - Exit chat
- `/new` - Start fresh session (reloads system prompt and memory context)
- `/skills` - List available skills with status
- `/compact` - Compact session history (summarize and truncate)
- `/clear` - Clear session history (keeps current context)
- `/memory <query>` - Search memory files
- `/save` - Save current session to disk
- `/status` - Show session info (ID, messages, tokens, compactions)

Plus any skill slash commands (e.g., `/github-pr`, `/commit`) based on installed skills.

## OpenClaw Compatibility

LocalGPT maintains strong compatibility with OpenClaw workspace files for seamless migration.

### Directory Structure

```
~/.localgpt/                          # State directory (OpenClaw: ~/.openclaw/)
├── config.toml                       # Config (OpenClaw uses JSON5)
├── agents/
│   └── main/                         # Default agent ID
│       └── sessions/
│           ├── sessions.json         # Session metadata (compatible format)
│           └── <sessionId>.jsonl     # Session transcripts (Pi format)
├── workspace/                        # Memory workspace (fully compatible)
│   ├── MEMORY.md                     # Long-term curated memory
│   ├── HEARTBEAT.md                  # Pending autonomous tasks
│   ├── SOUL.md                       # Persona and tone guidance
│   ├── USER.md                       # User profile (OpenClaw)
│   ├── IDENTITY.md                   # Agent identity (OpenClaw)
│   ├── TOOLS.md                      # Tool notes (OpenClaw)
│   ├── AGENTS.md                     # Operating instructions (OpenClaw)
│   ├── memory/
│   │   └── YYYY-MM-DD.md             # Daily logs
│   ├── knowledge/                    # Knowledge repository (optional)
│   └── skills/                       # Custom skills
└── logs/
```

### Workspace Files (Full Compatibility)

OpenClaw workspace files are fully supported. Copy them directly:

| File | Purpose | LocalGPT Support |
|------|---------|------------------|
| `MEMORY.md` | Long-term curated knowledge (facts, preferences, decisions) | ✅ Full |
| `HEARTBEAT.md` | Pending tasks for autonomous heartbeat runs | ✅ Full |
| `SOUL.md` | Persona, tone, and behavioral boundaries | ✅ Full |
| `USER.md` | User profile and addressing preferences | ✅ Loaded |
| `IDENTITY.md` | Agent name, vibe, emoji | ✅ Loaded |
| `TOOLS.md` | Notes about local tools and conventions | ✅ Loaded |
| `AGENTS.md` | Operating instructions for the agent | ✅ Loaded |
| `memory/*.md` | Daily logs (YYYY-MM-DD.md format) | ✅ Full |
| `knowledge/` | Structured knowledge repository | ✅ Indexed |
| `skills/*/SKILL.md` | Specialized task instructions | ✅ Full |

### Memory File Formats

All memory files are **plain Markdown** with no required frontmatter.

**MEMORY.md** - Curated long-term memory:
```markdown
# Long-term Memory

## User Info
- Name: Yi
- Role: Product software engineer
- Preference: Short, direct responses

## Key Decisions
- Using Rust for LocalGPT implementation
- SQLite FTS5 for memory search

## TODOs
- [ ] Action items to remember
```

**memory/YYYY-MM-DD.md** - Daily logs:
```markdown
# 2026-02-03

## Session Notes
- Discussed project architecture
- Decided on Pi-compatible session format

## Action Items
- [ ] Implement web UI
```

**HEARTBEAT.md** - Autonomous task list:
```markdown
# Pending Tasks

- [ ] Check for new emails and summarize
- [ ] Review yesterday's notes
```

**SOUL.md** - Persona definition:
```markdown
# Persona

You are a helpful assistant with a dry sense of humor.
Keep responses concise. Avoid unnecessary pleasantries.
```

### Memory Search Compatibility

LocalGPT uses the same chunking and indexing approach as OpenClaw:

| Parameter | Value | Notes |
|-----------|-------|-------|
| Chunk size | ~400 tokens | Same as OpenClaw |
| Chunk overlap | 80 tokens | Same as OpenClaw |
| Index format | SQLite FTS5 + vectors | Compatible approach |
| Hybrid search | 0.7 vector + 0.3 BM25 | Same weights |

**Indexed paths:**
- `MEMORY.md` (or `memory.md`)
- `memory/*.md` (daily logs)
- `knowledge/**/*.md` (if present)
- Session transcripts (optional)

### Session Format (Pi-Compatible)

Session transcripts use Pi's SessionManager JSONL format for OpenClaw compatibility:

**Header** (first line):
```json
{"type":"session","version":1,"id":"uuid","timestamp":"2026-02-03T10:00:00Z","cwd":"/path","compactionCount":0,"memoryFlushCompactionCount":0}
```

**Messages**:
```json
{"type":"message","message":{"role":"user","content":[{"type":"text","text":"Hello"}],"timestamp":1234567890}}
{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"Hi!"}],"usage":{"input":10,"output":5,"totalTokens":15},"model":"claude-3-opus","stopReason":"end_turn","timestamp":1234567891}}
```

**Tool calls**:
```json
{"type":"message","message":{"role":"assistant","content":[],"toolCalls":[{"id":"call_1","name":"bash","arguments":"{\"command\":\"ls\"}"}]}}
{"type":"message","message":{"role":"toolResult","content":[{"type":"text","text":"file1.txt\nfile2.txt"}],"toolCallId":"call_1"}}
```

### sessions.json Format (Compatible)

```json
{
  "main": {
    "sessionId": "uuid-here",
    "updatedAt": 1234567890,
    "cliSessionIds": {
      "claude-cli": "cli-session-uuid"
    },
    "claudeCliSessionId": "cli-session-uuid",
    "compactionCount": 0,
    "inputTokens": 1000,
    "outputTokens": 500,
    "totalTokens": 1500,
    "memoryFlushCompactionCount": 0,
    "lastHeartbeatText": "...",
    "lastHeartbeatSentAt": 1234567890
  }
}
```

### Migrating from OpenClaw

```bash
# Copy workspace files (fully compatible)
cp -r ~/.openclaw/workspace/* ~/.localgpt/workspace/

# Copy session data
cp -r ~/.openclaw/agents ~/.localgpt/agents

# Memory index will be rebuilt automatically on first run
```

**What works immediately:**
- All workspace files (MEMORY.md, SOUL.md, USER.md, etc.)
- Daily logs (memory/*.md)
- Knowledge repository (knowledge/)
- Skills (skills/*/SKILL.md)
- Session metadata (sessions.json)
- Session transcripts (*.jsonl)
- CLI session IDs for resume

**What differs:**
- Config format: TOML vs JSON5 (auto-migrated)
- No multi-channel routing (LocalGPT is local-only)
- No remote channels (Telegram, Discord, Slack)
- No subagent spawning (single "main" agent)
- No plugin/extension system

**Auto-config migration:**
If `~/.localgpt/config.toml` doesn't exist but `~/.openclaw/config.json5` does:
- `agents.defaults.workspace` → `memory.workspace`
- `agents.defaults.model` → `agent.default_model`
- `models.openai.apiKey` → `providers.openai.api_key`
- `models.anthropic.apiKey` → `providers.anthropic.api_key`

## Git Version Control

LocalGPT auto-creates `.gitignore` files. Recommended version control:

**Version control (workspace/):**
- `MEMORY.md` - Your curated knowledge
- `HEARTBEAT.md` - Pending tasks
- `SOUL.md` - Your persona
- `memory/*.md` - Daily logs
- `skills/` - Custom skills

**Do NOT version control:**
- `agents/*/sessions/*.jsonl` - Conversation transcripts (large)
- `logs/` - Debug logs
- `*.db` - SQLite database files

**Be careful with:**
- `config.toml` - May contain API keys. Use `${ENV_VAR}` syntax instead of raw keys.
