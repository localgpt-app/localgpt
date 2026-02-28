---
sidebar_position: 14
---

# Configuration

LocalGPT is configured via a TOML file at `~/.config/localgpt/config.toml`.

## Quick Start

Initialize a default config file:

```bash
localgpt config init
```

Or create one manually:

```bash
mkdir -p ~/.config/localgpt
cat > ~/.config/localgpt/config.toml << 'EOF'
[agent]
default_model = "claude-cli/opus"
EOF
```

## Full Configuration Reference

```toml
# LocalGPT Configuration
# ~/.config/localgpt/config.toml

#──────────────────────────────────────────────────────────────────────────────
# Agent Settings
#──────────────────────────────────────────────────────────────────────────────

[agent]
# Default model to use for chat
# Prefix determines provider:
#   claude-cli/*       → Claude CLI (uses installed claude command)
#   gemini-cli/*       → Gemini CLI (uses installed gemini command)
#   codex-cli/*        → Codex CLI (uses installed codex command)
#   anthropic/*        → Anthropic API
#   openai/*           → OpenAI
#   openai-compatible/*→ OpenAI-compatible server (OpenRouter, DeepSeek, etc.)
#   github-copilot/*   → GitHub Copilot API
#   gemini/*           → Gemini API (simple API key)
#   vertex/*           → Google Vertex AI (service account)
#   xai/*              → xAI (Grok)
#   glm/* or glm       → GLM (Z.AI)
#   ollama/*           → Ollama
#   Aliases: opus, sonnet, gpt, gpt-mini
default_model = "claude-cli/opus"

# Context window size (in tokens)
# Common values: 128000 (GPT-4), 200000 (Claude), 8192 (older models)
context_window = 128000

# Reserve tokens for the response
# Ensures the model has room to generate a response
reserve_tokens = 8000

#──────────────────────────────────────────────────────────────────────────────
# Provider Configuration
#──────────────────────────────────────────────────────────────────────────────

[providers.openai]
# API key (supports environment variable expansion)
api_key = "${OPENAI_API_KEY}"

# API base URL (optional, for proxies or Azure)
base_url = "https://api.openai.com/v1"

[providers.anthropic]
# Anthropic API key
api_key = "${ANTHROPIC_API_KEY}"

# API base URL (optional)
base_url = "https://api.anthropic.com"

[providers.ollama]
# Ollama server endpoint
endpoint = "http://localhost:11434"

# Default model for Ollama
model = "llama3"

[providers.glm]
# GLM (Z.AI) API key
api_key = "${GLM_API_KEY}"

[providers.claude_cli]
command = "claude"
model = "opus"

[providers.gemini_cli]
command = "gemini"
model = "gemini-3.1-pro-preview"

[providers.codex_cli]
command = "codex"
model = "gpt-4o"

[providers.openai_compatible]
# Generic OpenAI-compatible endpoint (OpenRouter, DeepSeek, etc.)
api_key = "${OPENROUTER_API_KEY}"
base_url = "https://openrouter.ai/api/v1"

#──────────────────────────────────────────────────────────────────────────────
# Tool Settings
#──────────────────────────────────────────────────────────────────────────────

[tools]
# Timeout for bash commands (milliseconds)
bash_timeout_ms = 10000

# Maximum size for web_fetch responses
web_fetch_max_bytes = 1048576    # 1MB

# Tools that require user approval before execution
require_approval = ["bash", "write_file"]

# Maximum characters in tool output sent to the model
tool_output_max_chars = 8000

# Warn when tool output contains potential injection patterns
log_injection_warnings = true

# Wrap tool output in XML content delimiters
use_content_delimiters = true

#──────────────────────────────────────────────────────────────────────────────
# Heartbeat Settings
#──────────────────────────────────────────────────────────────────────────────

[heartbeat]
# Enable automatic heartbeat
enabled = true

# How often to check HEARTBEAT.md
# Formats: "30m", "1h", "2h30m", "90s"
interval = "30m"

# Only run during these hours (optional)
# Prevents late-night activity
active_hours = { start = "09:00", end = "22:00" }

# Timezone for active hours (optional)
# Uses system timezone if not specified
# timezone = "America/Los_Angeles"

#──────────────────────────────────────────────────────────────────────────────
# Memory Settings
#──────────────────────────────────────────────────────────────────────────────

[memory]
# Where to store memory files
# Supports ~ for home directory
workspace = "~/.local/share/localgpt/workspace"

# Chunk size for indexing (tokens)
# Smaller = more precise search, larger = more context
chunk_size = 400

# Overlap between chunks (tokens)
# Ensures context isn't lost at chunk boundaries
chunk_overlap = 80

# Embedding provider for semantic search
# Options: "local" (fastembed), "openai", "gguf", "none"
embedding_provider = "local"

# Embedding model
# For "local": "BAAI/bge-small-en-v1.5" (default), or multilingual models
# For "openai": "text-embedding-3-small"
# For "gguf": path to .gguf file
embedding_model = "BAAI/bge-small-en-v1.5"

# Cache directory for downloaded embedding models
embedding_cache_dir = "~/.cache/localgpt/embeddings"

# Additional paths to index (outside workspace)
# external_paths = ["~/projects/notes"]

# Temporal decay for search scoring (default: 0.0 = disabled)
# Higher values penalize older memories more.
# Recommended: 0.1 gives ~50% penalty to 7-day old memories
# temporal_decay_lambda = 0.0

#──────────────────────────────────────────────────────────────────────────────
# HTTP Server Settings
#──────────────────────────────────────────────────────────────────────────────

[server]
# Enable HTTP server when daemon starts
enabled = true

# Port to listen on
port = 31327

# Bind address
# "127.0.0.1" = localhost only (secure)
# "0.0.0.0" = all interfaces (use with caution)
bind = "127.0.0.1"

#──────────────────────────────────────────────────────────────────────────────
# Sandbox Settings
#──────────────────────────────────────────────────────────────────────────────

[sandbox]
# Enable kernel-enforced shell sandbox (Landlock + seccomp + Seatbelt)
enabled = true

# Sandbox level: "auto" detects highest available, or force a specific level
# Options: "auto", "full", "standard", "minimal", "none"
level = "auto"

# Kill sandboxed commands after N seconds
timeout_secs = 120

# Maximum stdout + stderr output from sandboxed commands
max_output_bytes = 1048576    # 1MB

# Maximum file size a sandboxed command can create
max_file_size_bytes = 52428800  # 50MB

# Additional paths beyond workspace (escape hatches for power users)
[sandbox.allow_paths]
read = []     # e.g., ["/data/datasets"]
write = []    # e.g., ["/tmp/builds"]

[sandbox.network]
policy = "deny"   # "deny" or "proxy" (future)

#──────────────────────────────────────────────────────────────────────────────
# Security Block Settings
#──────────────────────────────────────────────────────────────────────────────
# LocalGPT injects a security block at the end of every LLM context window.
# It has two independent layers:
#   1. User policy (LocalGPT.md) — your signed custom instructions
#   2. Hardcoded suffix — compiled-in security reminder (always last)
# Both are concatenated into the last user message on every API call.
# They are NOT saved to session logs or included in compaction.

[security]
# Abort session if LocalGPT.md tamper detected? (default: warn only)
strict_policy = false

# Skip loading the LocalGPT.md workspace security policy
# The hardcoded suffix still applies unless also disabled.
disable_policy = false

# Skip the hardcoded security suffix
# WARNING: disabling both removes all end-of-context security reinforcement.
disable_suffix = false

#──────────────────────────────────────────────────────────────────────────────
# Logging Settings
#──────────────────────────────────────────────────────────────────────────────

[logging]
# Log level: trace, debug, info, warn, error
level = "info"

# Log file path
file = "~/.local/state/localgpt/logs/agent.log"

#──────────────────────────────────────────────────────────────────────────────
# Telegram Bot
#──────────────────────────────────────────────────────────────────────────────

[telegram]
# Enable Telegram bot (runs inside daemon)
enabled = false

# Bot API token from @BotFather
api_token = "${TELEGRAM_BOT_TOKEN}"
```

## Environment Variables

API keys and other sensitive values can reference environment variables:

```toml
api_key = "${OPENAI_API_KEY}"
```

This expands to the value of the `OPENAI_API_KEY` environment variable at runtime.

### Setting Environment Variables

**Bash/Zsh:**
```bash
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
```

**Fish:**
```fish
set -gx OPENAI_API_KEY "sk-..."
```

**In ~/.bashrc or ~/.zshrc:**
```bash
export OPENAI_API_KEY="sk-..."
```

## Provider-Specific Configuration

### OpenAI

```toml
[agent]
default_model = "openai/gpt-4o"  # or openai/gpt-4o-mini, or alias: gpt

[providers.openai]
api_key = "${OPENAI_API_KEY}"
```

### Anthropic

```toml
[agent]
default_model = "anthropic/claude-opus-4-5"  # or anthropic/claude-sonnet-4-5, or alias: opus

[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"
```

### Claude CLI

If you have the `claude` CLI installed, LocalGPT can use it directly:

```toml
[agent]
default_model = "claude-cli/opus"  # or claude-cli/sonnet, claude-cli/haiku
```

No API key configuration needed - uses your existing Claude CLI authentication.

### Gemini CLI

If you have the `gemini` CLI installed, LocalGPT can use it directly:

```toml
[agent]
default_model = "gemini-cli/gemini-3.1-pro-preview"  # or gemini-3.1-flash
```

No API key configuration needed - uses your existing Gemini CLI authentication.

### Codex CLI

If you have the `codex` CLI installed, LocalGPT can use it directly:

```toml
[agent]
default_model = "codex-cli/gpt-4o"
```

No API key configuration needed - uses your existing Codex CLI authentication.

### GitHub Copilot

```toml
[agent]
default_model = "github-copilot/gpt-4o"

[providers.github_copilot]
access_token = "${GITHUB_COPILOT_TOKEN}"
```

### Ollama (Local)

```toml
[agent]
default_model = "llama3"  # or mistral, codellama, etc.

[providers.ollama]
endpoint = "http://localhost:11434"
```

For fully local operation, only configure Ollama (no API keys needed). Tool calling is supported for Ollama models that have tool calling capability.

### GLM (Z.AI)

```toml
[agent]
default_model = "glm/glm-4.7"  # or alias: glm

[providers.glm]
api_key = "${GLM_API_KEY}"
```

### xAI (Grok)

```toml
[agent]
default_model = "xai/grok-3-mini"  # or xai/grok-3

[providers.xai]
api_key = "${XAI_API_KEY}"
base_url = "https://api.x.ai/v1"
```

### Gemini (Google)

LocalGPT supports Gemini via API key (simplest) or Vertex AI.

**API Key (Simplest):**

```toml
[agent]
default_model = "gemini/gemini-2.0-flash"  # or gemini-2.5-pro, etc.

[providers.gemini]
api_key = "${GEMINI_API_KEY}"
```

Get your API key at [Google AI Studio](https://aistudio.google.com/app/apikey).

### Vertex AI (Google Cloud)

Access Claude and Gemini models via Google Cloud's enterprise platform:

```toml
[agent]
default_model = "vertex/claude-opus-4-6"  # or vertex/gemini-2.0-flash

[providers.vertex]
service_account_key = "~/.config/gcloud/service-account.json"
project_id = "${GOOGLE_CLOUD_PROJECT}"
location = "us-central1"  # or "global" for global endpoint
```

Setup steps:
1. Create a service account in [Google Cloud Console](https://console.cloud.google.com/)
2. Grant "Vertex AI User" role
3. Download the JSON key file
4. Configure the path in `service_account_key`

### OpenAI-Compatible Provider (OpenRouter, DeepSeek, Groq, etc.)

For external services that speak the OpenAI API (e.g., OpenRouter, DeepSeek, Groq):

```toml
[agent]
default_model = "openai-compatible/deepseek-coder"

[providers.openai_compatible]
api_key = "${OPENROUTER_API_KEY}"
base_url = "https://openrouter.ai/api/v1"
```

### Local OpenAI-Compatible Server (LM Studio, llamafile, etc.)

If you run a local server that speaks the OpenAI API (e.g., LM Studio, llamafile, vLLM), point LocalGPT at it with an `openai/*` model ID:

1. Start your local server and note the port and model name.
2. Edit `~/.config/localgpt/config.toml`:

```toml
[agent]
default_model = "openai/<your-model-name>"

[providers.openai]
# Many local servers accept a dummy key
api_key = "not-needed"
base_url = "http://127.0.0.1:8080/v1"   # or :1234 for LM Studio
```

3. Run `localgpt chat` and requests will go to your local server.

### Telegram Bot

Access LocalGPT from Telegram with full chat, tool use, and memory support:

1. Create a bot via [@BotFather](https://t.me/BotFather) and get the API token.
2. Configure:

```toml
[telegram]
enabled = true
api_token = "${TELEGRAM_BOT_TOKEN}"
```

3. Start the daemon: `localgpt daemon start`
4. Message your bot — enter the 6-digit pairing code shown in the daemon logs.

## Managing Configuration

```bash
localgpt config init              # Create default config file
localgpt config show              # Display loaded config (sensitive values masked)
localgpt config show --format json  # JSON output
localgpt config get agent.default_model   # Get a specific value
localgpt config set agent.default_model "claude-cli/opus"  # Set a value
localgpt config path              # Show config file location
```

## Workspace Path Customization

LocalGPT supports multiple workspaces via environment variables (OpenClaw-compatible):

```bash
# Use a custom workspace directory (absolute path)
export LOCALGPT_WORKSPACE=~/my-project/ai-workspace
localgpt chat

# Use profile-based workspaces
export LOCALGPT_PROFILE=work    # uses ~/.local/share/localgpt/workspace-work
export LOCALGPT_PROFILE=home    # uses ~/.local/share/localgpt/workspace-home
```

Resolution order:
1. `LOCALGPT_WORKSPACE` env var (absolute path override)
2. `LOCALGPT_PROFILE` env var (creates `~/.local/share/localgpt/workspace-{profile}`)
3. `memory.workspace` from config file
4. Default: `~/.local/share/localgpt/workspace`

## Configuration Precedence

Configuration is loaded in this order (later overrides earlier):

1. Default values
2. `~/.config/localgpt/config.toml`
3. Environment variables
4. Command-line flags (`-m`, `--model`, etc.)

## Common Issues

### API Key Not Found

```
Error: OpenAI API key not configured
```

**Solution:** Set the environment variable or add to config:
```bash
export OPENAI_API_KEY="sk-..."
```

### Invalid Model

```
Error: Unknown model: gpt5
```

**Solution:** Check the model name. Valid prefixes:
- `gpt-*` for OpenAI
- `claude-*` for Anthropic
- Anything else for Ollama

### Permission Denied

```
Error: Cannot write to ~/.local/share/localgpt/workspace
```

**Solution:** Create the directory with proper permissions:
```bash
mkdir -p ~/.local/share/localgpt/workspace
chmod 700 ~/.config/localgpt
```
