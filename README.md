
# LocalGPT

A local device focused AI assistant built in Rust — persistent memory, autonomous tasks, ~27MB binary. Inspired by and compatible with OpenClaw.

`cargo install localgpt`

## Why LocalGPT?

- **Single binary** — no Node.js, Docker, or Python required
- **Local device focused** — runs entirely on your machine, your memory data stays yours
- **Persistent memory** — markdown-based knowledge store with full-text and semantic search
- **Autonomous heartbeat** — delegate tasks and let it work in the background
- **Multiple interfaces** — CLI, web UI, desktop GUI, Telegram bot
- **Defense-in-depth security** — signed policy files, kernel-enforced sandbox, prompt injection defenses
- **Multiple LLM providers** — Anthropic (Claude), OpenAI, Ollama, GLM (Z.AI)
- **OpenClaw compatible** — works with SOUL, MEMORY, HEARTBEAT markdown files and skills format

## Install

```bash
# Full install (includes desktop GUI)
cargo install localgpt

# Headless (no desktop GUI — for servers, Docker, CI)
cargo install localgpt --no-default-features
```

## Quick Start

```bash
# Initialize configuration
localgpt config init

# Start interactive chat
localgpt chat

# Ask a single question
localgpt ask "What is the meaning of life?"

# Run as a daemon with heartbeat, HTTP API and web ui
localgpt daemon start
```

## How It Works

LocalGPT uses plain markdown files as its memory:

```
~/.localgpt/workspace/
├── MEMORY.md            # Long-term knowledge (auto-loaded each session)
├── HEARTBEAT.md         # Autonomous task queue
├── SOUL.md              # Personality and behavioral guidance
└── knowledge/           # Structured knowledge bank (optional)
    ├── finance/
    ├── legal/
    └── tech/
```

Files are indexed with SQLite FTS5 for fast keyword search, and sqlite-vec for semantic search with local embeddings 

## Configuration

Stored at `~/.localgpt/config.toml`:

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
workspace = "~/.localgpt/workspace"

# Optional: Telegram bot
[telegram]
enabled = true
api_token = "${TELEGRAM_BOT_TOKEN}"
```

### Using a local OpenAI-compatible server (LM Studio, llamafile, etc.)

If you run a local server that speaks the OpenAI API (e.g., LM Studio, llamafile, vLLM), point LocalGPT at it and pick an `openai/*` model ID so it does **not** try to spawn the `claude` CLI:

1. Start your server (LM Studio default port: `1234`; llamafile default: `8080`) and note its model name.
2. Edit `~/.localgpt/config.toml`:
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

The sandbox denies access to sensitive directories (`~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.docker`) and blocks all network syscalls by default. Configure extra paths as needed:

```toml
[sandbox]
enabled = true
level = "auto"    # auto | full | standard | minimal | none

[sandbox.allow_paths]
read = ["/opt/data"]
write = ["/tmp/scratch"]
```

```bash
localgpt sandbox status   # Show sandbox capabilities
localgpt sandbox test     # Run smoke tests
```

### Signed Security Policy (LocalGPT.md)

Place a `LocalGPT.md` in your workspace to add custom rules (e.g. "never execute `rm -rf`"). The file is HMAC-SHA256 signed with a device-local key so the agent cannot tamper with it:

```bash
localgpt md sign     # Sign policy with device key
localgpt md verify   # Check signature integrity
localgpt md status   # Show security posture
localgpt md audit    # View security event log
```

Verification runs at every session start. If the file is unsigned, missing, or tampered with, LocalGPT falls back to its hardcoded security suffix — it never operates with a compromised policy.

### Prompt Injection Defenses

- **Marker stripping** — known LLM control tokens (`<|im_start|>`, `[INST]`, `<<SYS>>`, etc.) are stripped from tool outputs
- **Pattern detection** — regex scanning for injection phrases ("ignore previous instructions", "you are now a", etc.) with warnings surfaced to the user
- **Content boundaries** — all external content is wrapped in XML delimiters (`<tool_output>`, `<memory_context>`, `<external_content>`) so the model can distinguish data from instructions
- **Protected files** — the agent is blocked from writing to `LocalGPT.md`, `.localgpt_manifest.json`, `.device_key`, and the audit log

### Audit Chain

All security events (signing, verification, tamper detection, blocked writes) are logged to an append-only, hash-chained audit file at `~/.localgpt/.security_audit.jsonl`. Each entry contains the SHA-256 of the previous entry, making retroactive modification detectable.

```bash
localgpt md audit               # View audit log
localgpt md audit --json        # Machine-readable output
localgpt md audit --filter=tamper_detected
```

## CLI Commands

```bash
# Chat
localgpt chat                     # Interactive chat
localgpt chat --session <id>      # Resume session
localgpt ask "question"           # Single question

# Daemon
localgpt daemon start             # Start background daemon
localgpt daemon stop              # Stop daemon
localgpt daemon status            # Show status
localgpt daemon heartbeat         # Run one heartbeat cycle

# Memory
localgpt memory search "query"    # Search memory
localgpt memory reindex           # Reindex files
localgpt memory stats             # Show statistics

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
```

## HTTP API

When the daemon is running:

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Health check |
| `GET /api/status` | Server status |
| `POST /api/chat` | Chat with the assistant |
| `GET /api/memory/search?q=<query>` | Search memory |
| `GET /api/memory/stats` | Memory statistics |

### Egui Web UI (PoC)

LocalGPT includes a Proof of Concept for running the desktop Egui UI in the browser via WebAssembly. This enables code reuse between desktop and web interfaces.

**Try it**: After building the WASM UI with `./build-egui-web.sh`, visit `http://localhost:31327/egui`

See [`docs/egui-web-poc.md`](docs/egui-web-poc.md) for details on architecture, benefits, tradeoffs, and implementation.

## Blog

[Why I Built LocalGPT in 4 Nights](https://localgpt.app/blog/why-i-built-localgpt-in-4-nights) — the full story with commit-by-commit breakdown.

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
