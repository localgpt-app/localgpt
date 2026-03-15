# Changelog

All notable changes to LocalGPT are documented in this file.

## [Unreleased]

## [0.3.2] - 2026-03-08

### Added

- **Gen MCP server** — MCP server mode for external CLI backends with core tools, memory-only mode with write tools, and Ctrl+C handling.
- **Gen new primitives** — `pyramid`, `tetrahedron`, `icosahedron`, and `wedge` parametric shapes.
- **Gen HTML export** — export worlds as interactive browser-based experiences with Three.js viewer.
- **Gen avatar movement and camera control** — WASD/arrow key movement, mouse look, first-person/third-person camera modes.
- **CLI TUI slash commands and line editing** — `/help`, `/clear`, `/exit`, `/model`, `/session` commands with readline-style editing (Ctrl+A/E, arrow keys, history).
- **Gen streaming chat** — real-time tool call display and streaming responses in Gen mode for better visibility into scene building.
- **Gen batch entity operations** — `gen_spawn_entities` tool for efficient multi-entity creation in a single call.
- **Gen on-demand export** — `gen_export_world` tool with mesh asset localization for portable scene exports.
- **Gen human-readable filenames** — export filenames now use `YYYY-MM-DD_HH-MM-SS` format instead of timestamps.

### Changed

- **Gen mode architecture** — delegated to `localgpt-gen` binary via subprocess for cleaner separation.
- **Gen glTF export format** — switched to JSON format with version compatibility for better interoperability.

### Fixed

- **Core provider error messages** — API errors now show actual provider name instead of generic label for easier debugging.
- **Agent streaming** — correct tool message ordering in streaming path. ([#88](https://github.com/localgpt-app/localgpt/pull/88))
- **Gen compiler warnings** — silenced warnings in plugin.rs.

### Contributors

Thanks to **[@itripn](https://github.com/itripn)** (Ron Forrester) for fixing tool message ordering in the streaming path!

## [0.3.1] - 2026-03-03

### Added

- **Gen undo/redo system** — full undo/redo support with stable entity IDs, LLM tools (`gen_undo`, `gen_redo`), and persistence via `history.jsonl`. Covers entities, lights, behaviors, camera, and environment.
- **Gen audio undo/redo** — audio emitter commands now support full undo/redo with `gen_audio_emitter` and `gen_modify_audio` operations reversible.
- **Gen behavior system** — declarative entity animations: `orbit`, `spin`, `bob`, `look_at`, `pulse`, `path_follow`, `bounce`. Behaviors stack and persist through save/load.
- **Gen world save/load** — complete worlds serialized as skills (`SKILL.md` + `world.ron`). Includes `gen_save_world`, `gen_load_world`, and `gen_clear_scene` tools.
- **Gen avatar and tours** — avatar and tours sections in `world.ron` for user presence and guided waypoint sequences with descriptions and movement modes.
- **Gen parametric shapes** — unified world data model with `shape` field on entities. Supported: `box`, `sphere`, `cylinder`, `capsule`, `plane`, `torus`.
- **Gen material properties** — full PBR material support: `alpha_mode`, `unlit`, `double_sided`, `reflectance`, `emissive`. Exposed in spawn and modify tools.
- **Gen light properties** — `range`, `outer_angle`, `inner_angle` for spot lights; `direction` for directional/spot lights. All persisted and exposed in `entity_info`.
- **Gen entity info enrichment** — `entity_info` now includes shape type, emissive, light data (direction, range, angles), audio emitter type, and behavior info.
- **Gen glTF tracking** — source path tracked through save/load cycle for asset provenance.
- **Document loaders** — new module for loading documents (PDF, DOCX, etc.) for RAG workflows.
- **Audio transcription** — new module for transcribing audio files via Whisper-compatible APIs.
- **TTS module** — text-to-speech integration for voice output.
- **MMR re-ranking** — Maximal Marginal Relevance re-ranking for diverse memory search results.
- **CLI completion** — shell completion generation for bash, zsh, fish via `localgpt completion`.
- **CLI cron command** — manage cron jobs from CLI (`localgpt cron list/add/remove`).
- **CLI hooks command** — manage lifecycle hooks from CLI.
- **SpacetimeDB multiplayer** — web app for multiplayer 3D world collaboration.
- **Mobile workspace editor** — iOS/Android UI for editing workspace files with security hardening.

### Changed

- **Mobile apps restructured** — `apps/ios` renamed to `apps/apple` for multi-platform support (iOS + macOS).
- **OAuth providers removed** — all OAuth provider implementations removed for simplified authentication.

### Fixed

- **Gen visibility persistence** — visibility state now persists correctly through save/load and undo snapshots.
- **Gen camera FOV** — saves actual camera FOV instead of hardcoded 45 degrees.
- **Gen light saving** — light components save on any entity type, not just `GenEntityType::Light`.
- **Gen loop detection** — raised threshold and fixed command channel race condition.
- **Gen world load** — undo stack cleared when loading world without saved history.
- **Codex CLI provider** — updated for new CLI interface.

## [0.3.0] - 2026-02-27

A major release bringing the agent platform to production readiness with MCP tool integration, OpenAI-compatible API, cron scheduling, security hardening, mobile apps, and multi-agent orchestration.

### Added

- **MCP client support** — connect to external MCP tool servers via stdio or HTTP/SSE transports. Tools are auto-discovered and namespaced as `mcp_{server}_{tool}`. Configure in `[mcp]` config section.
- **OpenAI-compatible HTTP API** — `/v1/chat/completions` (streaming + non-streaming) and `/v1/models` endpoints. Enables integration with Cursor, Continue, Open WebUI, and the Python `openai` library.
- **Cron job scheduling** — run prompts on cron expressions (`0 */6 * * *`) or simple intervals (`every 30m`). Each job gets a fresh agent session with overlap prevention and configurable timeout.
- **Per-IP rate limiting** — token bucket rate limiter on all API routes. Configurable `requests_per_minute` and `burst` in `[server.rate_limit]`.
- **Oversized payload guard** — `RequestBodyLimitLayer` prevents OOM from large POST bodies (default: 10MB, configurable via `server.max_request_body`).
- **Configuration hot-reload** — daemon watches `config.toml` for changes and propagates updates to running services without restart. Also responds to SIGHUP on Unix.
- **Session pruning** — auto-cleanup of old session files at daemon startup and hourly. Configurable `session_max_age` (default: 30 days) and `session_max_count` (default: 500).
- **`localgpt doctor` command** — diagnostics that validate config, check provider reachability, test memory database, verify MCP connections, and report disk space. Supports `--fix` and `--json` flags.
- **Multi-agent orchestration** with `spawn_agent` tool for hierarchical delegation to specialist subagents.
- **OpenAI-compatible provider** for third-party APIs (OpenRouter, DeepSeek, Groq, vLLM, LiteLLM).
- **Multi-provider failover** with automatic retry across configured fallback models.
- **Lifecycle hook system** — `beforeToolCall`, `onMessage`, `onSessionStart` extensibility points.
- **Stuck loop detection** — prevents infinite tool-call loops by detecting repeated identical calls (configurable `max_tool_repeats`).
- **Bearer token authentication** for all HTTP API routes via `server.auth_token`.
- **Session file permissions** hardened to `0o600`.
- **Codex CLI provider** integration (`codex/*` models).
- **Apple Foundation Models** integration for on-device AI on iOS.
- **iOS app** with SwiftUI, MVVM architecture, and UniFFI bindings.
- **Android app** initial project structure.
- **Profile isolation** via `-p/--profile` CLI option for complete XDG path separation.
- **Hybrid web search** with configurable providers (`searxng`, `brave`, `tavily`, `perplexity`) and native-search passthrough.
- **xAI provider support** (`xai/*`, `grok-*`) with native `web_search` tool passthrough.
- **Vertex AI provider** — access Claude and Gemini models via Google Cloud with service account authentication (`vertex/*` models).
- **Gemini API key provider** — simple `GEMINI_API_KEY` authentication without OAuth (`gemini/*` models).
- **Bridge health monitoring** — automatic health status tracking (healthy/degraded/unhealthy) for all connected bridges.
- **CLI bridge** (`localgpt-bridge-cli`) — interactive terminal that connects to a running daemon via bridge IPC socket.
- **Skill routing rules** — `useWhen` and `dontUseWhen` conditions for context-aware skill activation.
- **Temporal decay for memory search** — optional scoring penalty for older memories (`temporal_decay_lambda` config).
- **LLM reasoning text preservation** — thinking/reasoning content emitted alongside tool calls is now preserved in responses.

### Changed

- **Actor-based agent execution** with `Arc<MemoryManager>` for improved thread safety.
- **`web_fetch` extraction upgraded** to use the `readability` crate with fallback text sanitization.
- **BridgeManager moved** from core to server crate for cleaner dependency graph.
- **Model routing updated** to support Claude 4.6 models.
- Replaced unsafe string byte-slicing with `floor_char_boundary` to prevent UTF-8 panics.
- Config templates expanded with `providers.xai`, `[tools.web_search]`, `[cron]`, and `[mcp]` examples.

### Fixed

- Mobile init EPERM by passing Config to MemoryManager.
- iOS XCFramework library identifiers, actor isolation, and C++ linking.
- Silent `NO_REPLY` tokens filtered from user-facing chat responses.
- Daemon foreground mode logging level.

### Contributors

Thanks to all contributors who helped shape this release! Special thanks to **[@jcorbin](https://github.com/jcorbin)** for generalizing daemon process handles, improving heartbeat reliability, incremental session saves, provider tooling improvements, and web search summaries; **[@TranscriptionFactory](https://github.com/TranscriptionFactory)** for tool filter infrastructure.

## [0.2.0] - 2026-02-14

A milestone release introducing LocalGPT Gen for 3D scene generation, XDG Base Directory compliance, Docker Compose support, and workspace restructuring.

### Added

- **LocalGPT Gen** — a new `localgpt-gen` subcrate for AI-driven 3D scene generation (Phase 1+2). ([55aa127](https://github.com/localgpt-app/localgpt/commit/55aa127))
- **Secure Docker Compose setup** for running LocalGPT in containers. ([#2](https://github.com/localgpt-app/localgpt/pull/2))
- **XDG Base Directory layout** for all paths, following platform conventions on Linux, macOS, and Windows. ([#18](https://github.com/localgpt-app/localgpt/issues/18))
- **Local server config guidance** for OpenAI-compatible server setups. ([#25](https://github.com/localgpt-app/localgpt/pull/25))
- Security section added to README covering sandbox, signed policy, and injection defenses.

### Changed

- Extracted `localgpt-gen` into its own workspace member and bumped to v0.2.0.
- Replaced OpenClaw auto-migration with a detection notice (no longer silently migrates config).
- Configured cargo-release for joint crates.io publishing.

### Fixed

- Added glibc compatibility shim for `ort-sys` on glibc < 2.38 (fixes builds on older Linux distros).

### Contributors

Thanks to **[@ttulttul](https://github.com/ttulttul)** (Ken Simpson) for the secure Docker Compose setup, and **[@cnaples79](https://github.com/cnaples79)** (Chase Naples) for documenting local server configuration!

## [0.1.3] - 2026-02-12

A major release focused on security hardening, new provider support, and the Telegram bot interface.

### Added

- **Telegram bot interface** with one-time pairing auth, slash commands, streaming responses with debounced edits, and full tool support. Runs as a background task inside the daemon. ([#15](https://github.com/localgpt-app/localgpt/pull/15))
- **Telegram HTML rendering** for agent responses with markdown-to-HTML conversion. ([#16](https://github.com/localgpt-app/localgpt/pull/16))
- **GLM (Z.AI) provider** support, adding Z.AI's GLM models as a new LLM backend. ([#21](https://github.com/localgpt-app/localgpt/pull/21))
- **Security policy module** with HMAC signing and tamper-detecting audit chain.
- **Kernel-enforced shell sandbox** for LLM-issued commands using macOS Seatbelt and Linux Landlock/seccomp.
- **Prompt injection defenses** with per-turn injection, suspicious-content warnings surfaced to users, and configurable strict policy mode.
- **Windows build support** by gating Unix-only sandbox and nix APIs behind `cfg(unix)`.

### Changed

- Renamed `localgpt security` CLI subcommand to `localgpt md`.
- Updated `LocalGPT.md` init template to match standing-instructions framing.
- Upgraded all dependencies to latest versions.

### Fixed

- Security block incorrectly sent as a prompt to Claude CLI instead of as a user message.
- Clippy warnings for Rust 1.93.
- Landlock/seccompiler API usage updated for latest crate versions.

### Contributors

Thanks to **[@haraldh](https://github.com/haraldh)** for building the Telegram bot interface and HTML rendering, and **[@austingreisman](https://github.com/austingreisman)** for adding GLM (Z.AI) provider support!

## [0.1.2] - 2026-02-09

This release enables tool calling for Ollama and OpenAI-compatible providers, and improves memory search quality.

### Added

- **Ollama tool calling support**, allowing Ollama models to execute agent tools. ([#14](https://github.com/localgpt-app/localgpt/pull/14))
- **Desktop feature flag** for headless builds (compile without GUI dependencies).
- GitHub Actions CI workflow with license audit via `cargo-deny`.

### Fixed

- **OpenAI provider tools** were silently dropped during streaming — the default `chat_stream` fallback now forwards tools and handles `ToolCalls` responses correctly. ([#11](https://github.com/localgpt-app/localgpt/pull/11))
- **Memory search** improved with token AND matching and rank-based scoring for more relevant results. ([#10](https://github.com/localgpt-app/localgpt/pull/10))
- Linux desktop builds now include x11 and Wayland features.

### Contributors

Thanks to **[@JarvisDeLaAri](https://github.com/JarvisDeLaAri)** for enabling Ollama tool calling, and **[@Ax73](https://github.com/Ax73)** for fixing OpenAI provider tool support!

## [0.1.1] - 2026-02-07

Introduces the desktop GUI, GGUF embedding support, and workspace concurrency safety.

### Added

- **Desktop GUI** built with egui, providing a native app experience.
- **GGUF embedding support** via llama.cpp for fully local semantic search.
- **Streaming tool details and slash commands** in the egui and web UIs.
- **Concurrency protections** for workspace file access.

### Fixed

- UTF-8 boundary panics in memory search snippets resolved; indexing simplified.

## [0.1.0] - 2026-02-04

Initial release of LocalGPT — a local-only AI assistant with persistent markdown-based memory.

### Added

- **Interactive CLI chat** with streaming responses and tool execution.
- **Multi-provider LLM support**: Anthropic, OpenAI, Ollama, and Claude CLI.
- **Markdown-based memory system** with `MEMORY.md`, daily logs, and `HEARTBEAT.md`.
- **Semantic search** using SQLite FTS5 and local embeddings via fastembed.
- **Autonomous heartbeat** runner for background task execution on configurable intervals.
- **HTTP/WebSocket API** with REST endpoints and real-time chat.
- **Embedded Web UI** for browser-based interaction.
- **OpenClaw compatibility** for workspace files, session format, and skills system.
- **Agent tools**: bash, read_file, write_file, edit_file, memory_search, memory_get, web_fetch.
- **Session management** with persistence, compaction, search, and export.
- **Image attachment support** for multimodal LLMs.
- **Tool approval mode** for dangerous operations.
- **Zero-config startup** defaulting to `claude-cli/opus`.
- **Auto-migration** from OpenClaw config if present.

[Unreleased]: https://github.com/localgpt-app/localgpt/compare/v0.3.2...HEAD
[0.3.2]: https://github.com/localgpt-app/localgpt/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/localgpt-app/localgpt/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/localgpt-app/localgpt/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/localgpt-app/localgpt/compare/v0.1.3...v0.2.0
[0.1.3]: https://github.com/localgpt-app/localgpt/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/localgpt-app/localgpt/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/localgpt-app/localgpt/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/localgpt-app/localgpt/releases/tag/v0.1.0
