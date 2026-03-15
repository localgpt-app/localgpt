
# <img src="https://localgpt.app/logo/localgpt-icon-app.svg" width="50" height="50" alt="LocalGPT" /> LocalGPT

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](https://github.com/localgpt-app/localgpt#license)
[![Crates.io](https://img.shields.io/crates/v/localgpt.svg)](https://crates.io/crates/localgpt)
[![Downloads](https://img.shields.io/crates/d/localgpt.svg)](https://crates.io/crates/localgpt)
[![Docs](https://docs.rs/localgpt/badge.svg)](https://docs.rs/localgpt/latest/localgpt)
[![CI](https://github.com/localgpt-app/localgpt/workflows/CI/badge.svg)](https://github.com/localgpt-app/localgpt/actions)
[![Discord](https://img.shields.io/discord/691052431525675048.svg?label=&logo=discord&logoColor=ffffff&color=7389D8&labelColor=6A7EC2)](https://discord.gg/spKRr6mRyp)

Build explorable 3D worlds with natural language — geometry, materials, lighting, audio, and behaviors. Open source, runs locally.

[![LocalGPT Gen Demo](https://img.youtube.com/vi/R__tg7YY0T8/maxresdefault.jpg)](https://www.youtube.com/watch?v=R__tg7YY0T8)

## Install

```bash
# World Building
cargo install localgpt-gen

# AI Assistant (chat, memory, daemon)
cargo install localgpt
```

## <img src="https://localgpt.app/logo/localgpt-icon.svg" width="32" height="32" alt="LocalGPT" /> Gen Mode (World Building)

`localgpt-gen` is a standalone binary for AI-driven 3D world creation with the Bevy game engine.

```bash
# Start interactive mode
localgpt-gen

# Start with an initial prompt
localgpt-gen "Create a desert scene with pyramids and a UFO hovering above"

# Load an existing scene
localgpt-gen --scene ./world.glb

# Verbose logging
localgpt-gen --verbose
```

### Features

- **Parametric shapes** — box, sphere, cylinder, capsule, plane, torus, pyramid, tetrahedron, icosahedron, wedge
- **PBR materials** — color, metalness, roughness, emissive, alpha, double-sided
- **Lighting** — point, spot, directional lights with color and intensity
- **Behaviors** — orbit, spin, bob, look_at, pulse, path_follow, bounce
- **Audio** — ambient sounds (wind, rain, forest, ocean, cave) and spatial emitters
- **Export** — glTF/GLB, HTML (browser-viewable), screenshots
- **World skills** — save/load complete worlds as reusable skills

### MCP Server

Use Gen from any MCP-compatible (Claude Desktop, Codex Desktop/CLI, Gemini CLI, etc.):

```bash
localgpt-gen mcp-server
```

Add to your `.mcp.json`:

```json
{
  "mcpServers": {
    "localgpt-gen": {
      "command": "localgpt-gen",
      "args": ["mcp-server"]
    }
  }
}
```

Full docs: [`website/docs/gen/index.md`](website/docs/gen/index.md) | [MCP Server](website/docs/gen/mcp-server.md)

### Templates

Jumpstart your project with ready-to-customize world templates:

| Template | Category |
|----------|----------|
| [Medieval Fantasy Village](https://localgpt.app/templates/fantasy/medieval-village) | Fantasy |
| [Enchanted Forest](https://localgpt.app/templates/fantasy/enchanted-forest) | Fantasy |
| [Japanese Temple & Gardens](https://localgpt.app/templates/fantasy/japanese-temple) | Fantasy |
| [Cozy Farm Village](https://localgpt.app/templates/fantasy/cozy-farm) | Fantasy |
| [Winter Wonderland](https://localgpt.app/templates/fantasy/winter-wonderland) | Fantasy |
| [Cyberpunk Neon City](https://localgpt.app/templates/urban/cyberpunk-city) | Urban |
| [Modern City](https://localgpt.app/templates/urban/modern-city) | Urban |
| [Space Station](https://localgpt.app/templates/sci-fi/space-station) | Sci-Fi |
| [Underwater Ocean World](https://localgpt.app/templates/sci-fi/underwater-world) | Sci-Fi |
| [Alien Bioluminescent World](https://localgpt.app/templates/sci-fi/alien-world) | Sci-Fi |
| [Haunted House](https://localgpt.app/templates/horror/haunted-house) | Horror |
| [Liminal Spaces / Backrooms](https://localgpt.app/templates/horror/backrooms) | Horror |

Browse all templates: [localgpt.app/templates](https://localgpt.app/templates)

Built something cool? Share on [Discord](https://discord.gg/spKRr6mRyp) or [YouTube](https://www.youtube.com/@localgpt-gen)!

---

## AI Assistant

`localgpt` is a local-first AI assistant with persistent memory, autonomous tasks, and multiple interfaces.

```bash
# Interactive chat
localgpt chat

# Single question
localgpt ask "What is the meaning of life?"

# Run as daemon with HTTP API and web UI
localgpt daemon start
```

### Why LocalGPT?

- **Single binary** — no Node.js, Docker, or Python required
- **Local device focused** — runs entirely on your machine, your data stays yours
- **Persistent memory** — markdown-based knowledge store with full-text and semantic search
- **Hybrid web search** — native provider search passthrough plus client-side fallback
- **Autonomous heartbeat** — delegate tasks and let it work in the background
- **Multiple interfaces** — CLI, web UI, desktop GUI, Telegram bot
- **Defense-in-depth security** — signed policy files, kernel-enforced sandbox, prompt injection defenses
- **Multiple LLM providers** — Anthropic, OpenAI, xAI, Ollama, GLM, Vertex AI, CLI providers

### How It Works

LocalGPT uses XDG-compliant directories for config/data/state/cache. Run `localgpt paths` to see resolved paths.

Workspace memory layout:

```
<workspace>/
├── MEMORY.md     # Long-term knowledge (auto-loaded each session)
├── HEARTBEAT.md  # Autonomous task queue
├── SOUL.md       # Personality and behavioral guidance
└── knowledge/    # Structured knowledge bank
```

Files are indexed with SQLite FTS5 for keyword search and sqlite-vec for semantic search with local embeddings.

### Configuration

Stored at `<config_dir>/config.toml`:

```toml
[agent]
default_model = "claude-cli/opus"

[providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"

[heartbeat]
enabled = true
interval = "30m"

[telegram]
enabled = true
api_token = "${TELEGRAM_BOT_TOKEN}"
```

Full config reference: [`website/docs/configuration.md`](website/docs/configuration.md)

### Security

- **Kernel-enforced sandbox** — Landlock/seccomp on Linux, Seatbelt on macOS
- **Signed policy files** — HMAC-SHA256 signed `LocalGPT.md` with tamper detection
- **Prompt injection defenses** — marker stripping, pattern detection, content boundaries
- **Audit chain** — hash-chained security event log

Security docs: [`website/docs/sandbox.md`](website/docs/sandbox.md) | [`website/docs/localgpt.md`](website/docs/localgpt.md)

### HTTP API

| Endpoint | Description |
|----------|-------------|
| `GET /` | Embedded web UI |
| `POST /api/chat` | Chat with assistant |
| `POST /api/chat/stream` | SSE streaming chat |
| `GET /api/memory/search?q=<query>` | Search memory |

Full API reference: [`website/docs/http-api.md`](website/docs/http-api.md)

### CLI Commands

```bash
localgpt chat                   # Interactive chat
localgpt ask "question"         # Single question
localgpt daemon start           # Start daemon
localgpt memory search "query"  # Search memory
localgpt config show            # Show config
localgpt paths                  # Show resolved paths
```

Full CLI reference: [`website/docs/cli-commands.md`](website/docs/cli-commands.md)

## Blog

- [Explorable World as Agent Skill](https://localgpt.app/blog/world-as-skill)
- [Why I Built LocalGPT in 4 Nights](https://localgpt.app/blog/why-i-built-localgpt-in-4-nights)

## Built With

Rust, Tokio, Axum, Bevy, SQLite (FTS5 + sqlite-vec), fastembed, eframe

## Contributors

<a href="https://github.com/localgpt-app/localgpt/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=localgpt-app/localgpt" />
</a>

## License

[Apache-2.0](LICENSE)
