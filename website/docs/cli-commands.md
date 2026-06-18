---
sidebar_position: 4
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
  daemon      Manage the background daemon
  memory      Memory management operations
  config      Configuration management
  md          Manage LocalGPT.md standing instructions
  paths       Show resolved directory paths
  sandbox     Shell sandbox diagnostics
  search      Test and manage web search
  init        Initialize configuration and keys
  bridge      Manage bridges and credentials
  doctor      Run diagnostics to validate setup
  encrypt     Manage encryption at rest
  tool        Manage MCP tool servers
  plugin      Manage MCP tool servers (alias for 'tool')
  completion  Generate shell completion scripts
  cron        Manage cron jobs
  hooks       Manage lifecycle hooks
  mcp-server  Run as MCP server (stdio) exposing memory tools
  session     Manage sessions (list, branch, checkpoints)
  audit       View and verify compaction audit log
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
| [`daemon`](/docs/cli-daemon) | Start/stop/status of the background daemon |
| [`memory`](/docs/cli-memory) | Search, reindex, and manage memory |
| `config` | Init, show, get, and set configuration values |
| [`md`](/docs/localgpt#quick-reference) | Sign, verify, and audit LocalGPT.md |
| `paths` | Show resolved XDG directory paths |
| [`sandbox`](/docs/sandbox#cli-commands) | Inspect sandbox capabilities and run tests |
| `search` | Test and manage web search providers |
| `init` | Initialize configuration and device keys |
| `bridge` | Manage chat bridges and credentials |
| `doctor` | Run diagnostics to validate setup (config, keys, providers) |
| `encrypt` | Manage encryption at rest (sessions, config secrets) |
| `desktop` | Launch the native desktop GUI (egui) |
| `completion` | Generate shell completion scripts (bash, zsh, fish) |
| `cron` | Manage cron jobs (list, add, remove) |
| `hooks` | Manage lifecycle hooks |
| `tool` / `plugin` | Manage MCP tool servers (list, add, remove, enable, disable) |
| `mcp-server` | Run as an MCP server (stdio) exposing memory tools |
| `session` | Manage sessions (list, branch, compaction checkpoints) |
| `audit` | View and verify compaction audit log (show, verify, stats) |
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
localgpt config set agent.default_model "claude-cli/opus"

# Check sandbox capabilities
localgpt sandbox status

# Sign LocalGPT.md after editing
localgpt md sign

# View security audit log
localgpt md audit

# Launch world generation (separate binary)
localgpt-gen "create a solar system with planets"

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

# Compaction audit log
localgpt audit show               # Show recent compaction events
localgpt audit show --limit 5     # Show last 5 events
localgpt audit show --json        # JSON output
localgpt audit verify             # Verify hash chain integrity
localgpt audit stats              # Show compaction statistics

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
