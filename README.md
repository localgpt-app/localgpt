
# <img src="https://localgpt.app/logo/localgpt-icon-app.png" width="50" height="50" alt="LocalGPT" /> LocalGPT

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](https://github.com/localgpt-app/localgpt#license)
[![Crates.io](https://img.shields.io/crates/v/localgpt.svg)](https://crates.io/crates/localgpt)
[![Downloads](https://img.shields.io/crates/d/localgpt.svg)](https://crates.io/crates/localgpt)
[![Docs](https://docs.rs/localgpt/badge.svg)](https://docs.rs/localgpt/latest/localgpt)
[![CI](https://github.com/localgpt-app/localgpt/workflows/CI/badge.svg)](https://github.com/localgpt-app/localgpt/actions)
[![Discord](https://img.shields.io/discord/691052431525675048.svg?label=&logo=discord&logoColor=ffffff&color=7389D8&labelColor=6A7EC2)](https://discord.gg/yMQ8tfxG)

A local device focused AI assistant built in Rust — persistent memory, autonomous tasks. Inspired by and compatible with OpenClaw.

`cargo install localgpt`

## Why LocalGPT?

- **Single binary** — no Node.js, Docker, or Python required
- **Local device focused** — runs entirely on your machine, your memory data stays yours
- **Persistent memory** — markdown-based knowledge store with full-text and semantic search
- **Hybrid web search** — native provider search passthrough plus client-side fallback providers
- **Autonomous heartbeat** — delegate tasks and let it work in the background
- **Multiple interfaces** — CLI, web UI, desktop GUI, Telegram bot
- **Defense-in-depth security** — signed policy files, kernel-enforced sandbox, prompt injection defenses
- **Multiple LLM providers** — Anthropic (Claude), OpenAI, xAI (Grok), Ollama, GLM (Z.AI), OAuth subscriptions (Claude Pro/Max, Gemini)
- **OpenClaw compatible** — works with SOUL, MEMORY, HEARTBEAT markdown files and skills format

## Install

```bash
# From crates.io (includes desktop GUI)
cargo install localgpt

# Headless (no desktop GUI — for servers, Docker, CI)
cargo install localgpt --no-default-features

# From source checkout
cargo install --path crates/cli
```

## Quick Start

```bash
# Initialize configuration
localgpt config init

# Start interactive chat
localgpt chat

# Ask a single question
localgpt ask "What is the meaning of life?"

# Inspect resolved config/data/state/cache paths
localgpt paths

# Run as a daemon with heartbeat, HTTP API and web ui
localgpt daemon start
```

## How It Works

LocalGPT uses XDG-compliant directories (or platform equivalents) for config/data/state/cache. Run `localgpt paths` to see your resolved paths.

Workspace memory layout:

```
<data_dir>/workspace/
├── MEMORY.md            # Long-term knowledge (auto-loaded each session)
├── HEARTBEAT.md         # Autonomous task queue
├── SOUL.md              # Personality and behavioral guidance
└── knowledge/           # Structured knowledge bank (optional)
    ├── finance/
    ├── legal/
    └── tech/
```

Files are indexed with SQLite FTS5 for fast keyword search, and sqlite-vec for semantic search with local embeddings.

## Configuration

Stored at `<config_dir>/config.toml` (run `localgpt config path` or `localgpt paths`):

```toml
[agent]
default_model = "claude-cli/opus"

[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"

[heartbeat]
enabled = true
interval = "30m"
active_hours = { start = "09:00", end = "22:00" }

[memory]
workspace = "~/.local/share/localgpt/workspace" # optional override

# Optional: Telegram bot
[telegram]
enabled = true
api_token = "${TELEGRAM_BOT_TOKEN}"
```

### Using a local OpenAI-compatible server (LM Studio, llamafile, etc.)

If you run a local server that speaks the OpenAI API (e.g., LM Studio, llamafile, vLLM), point LocalGPT at it and pick an `openai/*` model ID so it does **not** try to spawn the `claude` CLI:

1. Start your server (LM Studio default port: `1234`; llamafile default: `8080`) and note its model name.
2. Edit your config file (`localgpt config path`):
   ```toml
   [agent]
   default_model = "openai/<your-model-name>"

   [providers.openai]
   # Many local servers accept a dummy key
   api_key = "not-needed"
   base_url = "http://127.0.0.1:8080/v1" # or http://127.0.0.1:1234/v1 for LM Studio
   ```
3. Run `localgpt chat` (or `localgpt daemon start`) and requests will go to your local server.

Tip: If you see `Failed to spawn Claude CLI`, change `agent.default_model` away from `claude-cli/*` or install the `claude` CLI.

### Web Search

Configure web search providers under `[tools.web_search]` and validate with:

```bash
localgpt search test "rust async runtime"
localgpt search stats
```

Full setup guide: [`docs/web-search.md`](docs/web-search.md)

### OAuth Subscription Plans

Use Claude Pro/Max or Google Gemini subscription credentials via OAuth instead of pay-per-request API keys:

```toml
# Claude Pro/Max OAuth (preferred over api_key when configured)
[providers.anthropic_oauth]
access_token = "${ANTHROPIC_OAUTH_TOKEN}"
refresh_token = "${ANTHROPIC_OAUTH_REFRESH_TOKEN}"

# Google Gemini subscription OAuth
[providers.gemini_oauth]
access_token = "${GEMINI_OAUTH_TOKEN}"
refresh_token = "${GEMINI_OAUTH_REFRESH_TOKEN}"
project_id = "${GOOGLE_CLOUD_PROJECT}"  # Optional, for enterprise
```

Full setup guide: [`docs/oauth-setup.md`](docs/oauth-setup.md)

## Telegram Bot

Access LocalGPT from Telegram with full chat, tool use, and memory support.

1. Create a bot via [@BotFather](https://t.me/BotFather) and get the API token
2. Set `TELEGRAM_BOT_TOKEN` or add the token to `config.toml`
3. Start the daemon: `localgpt daemon start`
4. Message your bot — enter the 6-digit pairing code shown in the daemon logs

Once paired, use `/help` in Telegram to see available commands.

## Security

LocalGPT ships with layered security to keep the agent confined and your data safe — no cloud dependency required.

### Kernel-Enforced Sandbox

Every shell command the agent runs is executed inside an OS-level sandbox:

| Platform | Mechanism | Capabilities |
|----------|-----------|-------------|
| **Linux** | Landlock LSM + seccomp-bpf | Filesystem allow-listing, network denial, syscall filtering |
| **macOS** | Seatbelt (SBPL) | Filesystem allow-listing, network denial |
| **All** | rlimits | 120s timeout, 1MB output cap, 50MB file size, 64 process limit |

The sandbox denies access to sensitive directories including `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.docker`, `~/.kube`, and credential files (`~/.npmrc`, `~/.pypirc`, `~/.netrc`). It blocks all network syscalls by default. Configure extra paths as needed:

```toml
[sandbox]
enabled = true
level = "auto"    # auto | full | standard | minimal | none

[sandbox.allow_paths]
read = ["/opt/data"]
write = ["/tmp/scratch"]
```

:::note Claude CLI Backend
If using the Claude CLI as your LLM backend (`agent.default_model = "claude-cli/*"`), the sandbox does not apply to Claude CLI subprocess calls — only to tool-executed shell commands. The subprocess itself runs outside the sandbox with access to your system.
:::

```bash
localgpt sandbox status   # Show sandbox capabilities
localgpt sandbox test     # Run smoke tests
```

### Signed Custom Rules (LocalGPT.md)

Place a `LocalGPT.md` in your workspace to add custom rules (e.g. "never execute `rm -rf`"). The file is HMAC-SHA256 signed with a device-local key so tampering will be detected:

```bash
localgpt md sign     # Sign policy with device key
localgpt md verify   # Check signature integrity
localgpt md status   # Show security posture
localgpt md audit    # View security event log
```

Verification runs at every session start. If the file is unsigned, missing, or tampered with, LocalGPT falls back to its hardcoded security suffix — it never operates with a compromised LocalGPT.md.

### Prompt Injection Defenses

- **Marker stripping** — known LLM control tokens (`<|im_start|>`, `[INST]`, `<<SYS>>`, etc.) are stripped from tool outputs
- **Pattern detection** — regex scanning for injection phrases ("ignore previous instructions", "you are now a", etc.) with warnings surfaced to the user
- **Content boundaries** — all external content is wrapped in XML delimiters (`<tool_output>`, `<memory_context>`, `<external_content>`) so the model can distinguish data from instructions
- **Protected files** — the agent is blocked from writing to `LocalGPT.md`, `.localgpt_manifest.json`, `IDENTITY.md`, `localgpt.device.key`, and `localgpt.audit.jsonl`

### Audit Chain

All security events (signing, verification, tamper detection, blocked writes) are logged to an append-only, hash-chained audit file at `<state_dir>/localgpt.audit.jsonl`. Each entry contains the SHA-256 of the previous entry, making retroactive modification detectable.

```bash
localgpt md audit               # View audit log
localgpt md audit --json        # Machine-readable output
localgpt md audit --filter=tamper_detected
```

## CLI Commands

```bash
# Chat
localgpt chat                     # Interactive chat
localgpt chat --resume            # Resume most recent session
localgpt chat --session <id>      # Resume session
localgpt ask "question"           # Single question
localgpt ask -f json "question"   # JSON output

# Desktop GUI (default build)
localgpt desktop

# Daemon
localgpt daemon start             # Start background daemon
localgpt daemon start --foreground
localgpt daemon restart           # Restart daemon
localgpt daemon stop              # Stop daemon
localgpt daemon status            # Show status
localgpt daemon heartbeat         # Run one heartbeat cycle

# Memory
localgpt memory search "query"    # Search memory
localgpt memory recent            # List recent entries
localgpt memory reindex           # Reindex files
localgpt memory stats             # Show statistics

# Web search
localgpt search test "query"      # Validate search provider config
localgpt search stats             # Show cumulative search usage/cost

# Security
localgpt md sign                  # Sign LocalGPT.md policy
localgpt md verify                # Verify policy signature
localgpt md status                # Show security posture
localgpt md audit                 # View security audit log
localgpt sandbox status           # Show sandbox capabilities
localgpt sandbox test             # Run sandbox smoke tests

# Config
localgpt config init              # Create default config
localgpt config show              # Show current config
localgpt config get agent.default_model
localgpt config set logging.level debug
localgpt config path

# Paths
localgpt paths                    # Show resolved XDG/platform paths
```

## HTTP API

When the daemon is running:

| Endpoint | Description |
|----------|-------------|
| `GET /` | Embedded web UI |
| `GET /health` | Health check |
| `GET /api/status` | Server status |
| `GET /api/config` | Effective config summary |
| `GET /api/heartbeat/status` | Last heartbeat status/event |
| `POST /api/sessions` | Create session |
| `GET /api/sessions` | List active in-memory sessions |
| `GET /api/sessions/{session_id}` | Session status |
| `DELETE /api/sessions/{session_id}` | Delete session |
| `GET /api/sessions/{session_id}/messages` | Session transcript/messages |
| `POST /api/sessions/{session_id}/compact` | Compact session history |
| `POST /api/sessions/{session_id}/clear` | Clear session history |
| `POST /api/sessions/{session_id}/model` | Switch model for session |
| `POST /api/chat` | Chat with the assistant |
| `POST /api/chat/stream` | SSE streaming chat |
| `GET /api/ws` | WebSocket chat endpoint |
| `GET /api/memory/search?q=<query>` | Search memory |
| `GET /api/memory/stats` | Memory statistics |
| `POST /api/memory/reindex` | Trigger memory reindex |
| `GET /api/saved-sessions` | List persisted sessions |
| `GET /api/saved-sessions/{session_id}` | Get persisted session |
| `GET /api/logs/daemon` | Tail daemon logs |

### Egui Web UI (PoC)

LocalGPT includes a Proof of Concept for running the desktop Egui UI in the browser via WebAssembly. This enables code reuse between desktop and web interfaces.

**Try it**: After building the WASM UI with `./build-egui-web.sh`, visit `http://localhost:31327/egui`

See [`docs/egui-web-poc.md`](docs/egui-web-poc.md) for details on architecture, benefits, tradeoffs, and implementation.

## <img src="https://localgpt.app/logo/localgpt-icon.png" width="100" height="100" alt="LocalGPT" /> Gen Mode (World Generation)

`Gen` is a separate binary (`localgpt-gen`) in the workspace — not a `localgpt gen` subcommand.

```bash
# Install from crates.io
cargo install localgpt-gen

# Install from this repo
cargo install --path crates/gen

# Or run directly from the workspace
cargo run -p localgpt-gen

# Start interactive Gen mode
localgpt-gen

# Start with an initial prompt
localgpt-gen "Create a low-poly forest scene with a path and warm lighting"

# Load an existing glTF/GLB scene
localgpt-gen --scene ./scene.glb

# Verbose logging
localgpt-gen --verbose

# Combine options
localgpt-gen -v -s ./scene.glb "Add warm lighting"

# Custom agent ID (default: "gen")
localgpt-gen --agent my-gen-agent
```

`localgpt-gen` runs a Bevy window (1280x720) on the main thread and an agent loop on a background tokio runtime. The agent gets safe tools (memory, web) plus Gen-specific tools (spawn/modify entities, scene inspection, glTF export). Type `/quit` or `/exit` in the terminal to close.

Built something cool with Gen? Share your creation on [Discord](https://discord.gg/yMQ8tfxG)!

## Blog

[Why I Built LocalGPT in 4 Nights](https://localgpt.app/blog/why-i-built-localgpt-in-4-nights) — the initial story with commit-by-commit breakdown.

## Built With

Rust, Tokio, Axum, SQLite (FTS5 + sqlite-vec), fastembed, eframe

## Contributors

<a href="https://github.com/localgpt-app/localgpt/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=localgpt-app/localgpt" />
</a>

## Stargazers

[![Star History Chart](https://api.star-history.com/svg?repos=localgpt-app/localgpt&type=Date)](https://star-history.com/#localgpt-app/localgpt&Date)

## License

[Apache-2.0](LICENSE)

### Your contributions

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be licensed under the Apache-2.0 license, without any additional terms or conditions.
