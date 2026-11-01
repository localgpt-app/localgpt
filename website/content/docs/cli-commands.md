---
title: "CLI Commands"
---

# CLI Commands

LocalGPT provides a comprehensive command-line interface with several subcommands.

## Overview

```bash
localgpt <COMMAND>

Commands:
  chat        Interactive multi-turn conversation
  tui         Launch terminal UI (TUI) with streaming display
  ask         Single question and response
  gen         Launch world generation mode (Bevy renderer)
  world       Work on a .world package: commit batches, read the log, verify, serve its tools over MCP
  daemon      Manage the background daemon
  memory      Memory management operations
  config      Configuration management
  policy      Manage POLICY.md standing instructions (alias: md)
  paths       Show resolved directory paths
  sandbox     Shell sandbox diagnostics
  search      Test and manage web search
  init        Initialize configuration and keys
  bridge      Manage bridges and credentials
  doctor      Run diagnostics to validate setup
  tool        Manage MCP tool servers
  plugin      Manage MCP tool servers (alias for 'tool')
  completion  Generate shell completion scripts
  cron        Manage cron jobs
  hooks       Manage lifecycle hooks
  mcp-server  Run as MCP server (stdio) exposing memory tools
  session     Manage sessions (list, branch, checkpoints)
  cert        Manage TLS certificates (info, regenerate)
  desktop     Launch desktop GUI (feature-gated)
  help        Print help information
```

## Global Options

```bash
localgpt [OPTIONS] <COMMAND>

Options:
  -c, --config <PATH>  Path to config file (default: ~/.config/localgpt/config.toml on Linux/macOS, %APPDATA%\localgpt\config.toml on Windows)
  -a, --agent <ID>     Agent ID (default: "main")
  -m, --model <MODEL>  Override the default model
  -v, --verbose        Enable verbose logging
  -h, --help           Print help
  -V, --version        Print version
```

## Command Summary

| Command | Description |
|---------|-------------|
| [`chat`](/docs/cli-chat) | Interactive multi-turn conversation with session support |
| `tui` | Terminal UI with streaming responses and slash commands |
| [`ask`](/docs/cli-ask) | Single-turn question answering |
| [`gen`](/docs/gen) | Launch world generation mode (Bevy renderer) |
| [`world`](#world) | A `.world` package's command line: commit batches, read the log and tips, undo, verify, and serve an open canvas's tools over MCP |
| [`daemon`](/docs/cli-daemon) | Start/stop/status of the background daemon |
| [`memory`](/docs/cli-memory) | Search, reindex, and manage memory |
| `config` | Init, show, get, and set configuration values |
| [`policy`](/docs/policy#quick-reference) | Manage POLICY.md standing instructions (alias: `md`) |
| `paths` | Show resolved XDG directory paths |
| [`sandbox`](/docs/sandbox#cli-commands) | Inspect sandbox capabilities and run tests |
| `search` | Test and manage web search providers |
| `init` | Initialize configuration and device keys |
| `bridge` | Manage chat bridges and credentials |
| `doctor` | Run diagnostics to validate setup (config, keys, providers) |
| `desktop` | Launch the native desktop GUI (egui) |
| `completion` | Generate shell completion scripts (bash, zsh, fish) |
| `cron` | Manage cron jobs (list, add, remove) |
| `hooks` | Manage lifecycle hooks |
| `tool` / `plugin` | Manage MCP tool servers (list, add, remove, enable, disable) |
| `mcp-server` | Run as an MCP server (stdio) exposing memory tools |
| `session` | Manage sessions (list, branch, compaction checkpoints) |
| `cert` | Manage TLS certificates (info, regenerate) |

## Examples

```bash
# Start an interactive chat
localgpt chat

# Launch the terminal UI
localgpt tui

# Ask a single question
localgpt ask "What is the capital of France?"

# Use a specific model
localgpt -m anthropic/claude-sonnet-4-5 chat

# Start the daemon
localgpt daemon start

# Search memory
localgpt memory search "project ideas"

# Show memory statistics
localgpt memory stats

# Configuration management
localgpt config init              # Create default config
localgpt config show              # Display loaded config
localgpt config get agent.default_model   # Get a specific value
localgpt config set agent.default_model "openai/gpt-4o"

# Check sandbox capabilities
localgpt sandbox status

# Launch world generation (separate binary)
localgpt-gen "create a solar system with planets"

# Work on a .world package (see below)
localgpt world init demo.world --git
localgpt world submit demo.world batch.json --author me --message "lift the cube"
localgpt world log demo.world --last 5
localgpt world mcp demo.world          # the open canvas's tools, over stdio, for any MCP agent

# Test web search provider
localgpt search test

# Initialize config and device keys (first-time setup)
localgpt init

# Run setup diagnostics (config, keys, providers, MCP)
localgpt doctor

# Show resolved directory paths
localgpt paths

# Generate shell completion
localgpt completion bash > /etc/bash_completion.d/localgpt
localgpt completion zsh > "${fpath[1]}/_localgpt"
localgpt completion fish > ~/.config/fish/completions/localgpt.fish

# Manage cron jobs
localgpt cron list
localgpt cron add "0 */6 * * *" "Summarize recent memory and update MEMORY.md"
localgpt cron remove <job-id>

# Manage lifecycle hooks
localgpt hooks list
localgpt hooks set beforeToolCall "/path/to/hook.sh"

# Manage MCP tool servers
localgpt tool list                # List servers with enabled/disabled status
localgpt tool add myserver --command "npx" -- "@anthropic/mcp-searxng"
localgpt tool enable myserver     # Enable a disabled server
localgpt tool disable myserver    # Disable without removing
localgpt tool remove myserver     # Remove from config

# Session management and compaction checkpoints
localgpt session list                          # List recent sessions
localgpt session branch <session-id>           # Branch a session into a new one
localgpt session checkpoints                   # List checkpoints for the latest session
localgpt session restore 2                     # Restore latest session from checkpoint #2

# Run as an MCP server (stdio) for external AI backends
localgpt mcp-server

# Manage TLS certificates for the HTTP server
localgpt cert info                # Show certificate expiry, SANs, and paths
localgpt cert regenerate          # Force certificate regeneration
```

## `localgpt world` {#world}

A `.world` package is a folder — `manifest.json` (the world now), `ops.jsonl` (every change), `assets/` — and `localgpt world` is its command line, whoever holds it. While an app has the world open, every command is a call to that app's localhost API: the app commits, and what only a window knows — what's on screen, the selection, a screenshot, a batch waiting for review — comes from it. With nothing open, the commands are the authority themselves, the same one the [live canvas](/docs/gen/live-canvas/) uses, for scripts and CI. One authority at a time, either way, and a commit prints the same receipt whichever made it.

This is how an agent in a terminal changes a world: no token or JSON quoting on its command line, short lines unless `--json`, and one command prefix to allow.

```bash
localgpt world init <dir> [--git]             # open a folder as a package, creating one if needed; --git: every batch a commit
localgpt world submit [<dir>] [<batch> | -]   # commit a batch of edit ops — whole, or not at all; stdin by default
    --author <name> --message <why>           #   attribution ($LOCALGPT_WORLD_AUTHOR) and the commit message
    --at <entry-id>                           #   build on an earlier entry: a branch; the head stays where it is
    --expected-revision <n>                   #   refused if the world moved since you read it
localgpt world undo [<dir>] [--own] [--at <id>]   # take back the newest edit by appending its inverse
localgpt world redo [<dir>] [--own] [--at <id>]   # take the newest undo back
localgpt world log [<dir>] [--last <n>]       # every committed batch with ids, the head marked
localgpt world tips [<dir>]                   # every branch end, the head marked
localgpt world show [<dir>] [--at <entry>]    # the world in a line per entity; --at current, <id> or base
localgpt world get [<dir>] <entity>…          # entities by name or id, as JSON: what a patch changes
localgpt world verify [<dir>]                 # the fold of the log still equals the head; assets match their hashes
localgpt world screenshot [<dir>]             # the open app's view now, as a PNG path
localgpt world selection [<dir>]              # what the person has selected in the open app
localgpt world goto [<dir>] <entry>           # show an entry in the open app's window; nothing is written
localgpt world proposals [<dir>]              # with review on, the batches waiting for the person
localgpt world history [<dir>] [<rev>]        # the git history, when the world is in git
localgpt world mcp [<dir>]                    # serve the open canvas's tools over stdio (submit, undo, log, tips, goto, verify, screenshot, selection)
```

The folder is the first argument when it's given; else `$LOCALGPT_WORLD_DIR`, which the desktop app sets in an agent's terminal tab; else the folder you're in, or the nearest one above it that is a world. A batch is JSON: `[op, …]` or `{"ops": [op, …]}`, inline, a file, or stdin, with the op kinds `SpawnEntity`, `ModifyEntity`, `DeleteEntity`, `SetEnvironment`, `SetCamera`, `ModifyWorld` (the world's scene-wide fields: its song, tours, environment) and `Batch`. Names bind wherever the format marks a reference and commit as ids; a refusal names the op and the reason, on stderr with exit status 1. The folder's `AGENTS.md` describes the same rules for an agent.

```bash
localgpt world submit -m "a lamp by the crate" <<'EOF'
[{"SpawnEntity": {"entity": {"name": "lamp", "transform": {"position": [2, 0.5, 1]}}}}]
EOF
claude mcp add scene -- localgpt world mcp scene-1.world    # or: Claude Code edits the open world over MCP
```

## Built-in Chat Commands

When in interactive chat mode, these commands are available:

| Command | Description |
|---------|-------------|
| `/help`, `/h`, `/?` | Show help for chat commands |
| `/quit`, `/exit`, `/q` | Exit the chat session |
| `/new` | Start a fresh session |
| `/sessions` | List saved sessions |
| `/resume <id>` | Resume a saved session |
| `/search <query>` | Search across sessions |
| `/memory <query>` | Search memory for a term |
| `/save` | Force save current context to memory |
| `/compact` | Manually trigger session compaction |
| `/model [name]` | Show or switch the current model |
| `/models` | List available model prefixes |
| `/context` | Show context window usage |
| `/status` | Show session status (tokens, turns) |
| `/export [file]` | Export session as markdown |
| `/attach <file>` | Attach a file to the conversation |
| `/clear` | Clear the terminal screen |
| `/skills` | List available skills |

Additionally, any installed skills can be invoked via `/skill-name` (e.g., `/commit`, `/github-pr`). See [Skills System](/docs/skills) for details.

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Configuration error |
| 3 | API/Provider error |
