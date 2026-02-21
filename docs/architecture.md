# LocalGPT Architecture

## Relationship with OpenClaw

LocalGPT is a **fresh Rust implementation** inspired by OpenClaw's architecture, stripped down to essential components for local-only AI interaction.

### Design Philosophy

| Aspect | OpenClaw | LocalGPT |
|--------|----------|----------|
| Language | TypeScript | Rust |
| LOC (estimated) | ~200k | ~3k |
| Remote channels | 20+ (Telegram, Discord, Slack, etc.) | 0 |
| Dependencies | 500+ npm packages | ~30 crates |
| Memory system | Markdown + SQLite | Same approach |
| Heartbeat | HEARTBEAT.md driven | Same approach |
| Startup time | ~2-3s | <100ms |

### Borrowed Concepts

The following concepts were directly inspired by OpenClaw:

1. **Memory System Design**
   - `MEMORY.md` as curated long-term knowledge
   - `memory/*.md` for daily append-only logs
   - `HEARTBEAT.md` for pending tasks/reminders
   - SQLite for indexing with FTS5

2. **Heartbeat Runner**
   - Periodic autonomous execution
   - Active hours configuration
   - Simple prompt-based task checking

3. **Session Management**
   - Context compaction with summarization
   - Pre-compaction memory flush prompts
   - JSONL transcript storage

4. **Tool System**
   - Bash execution
   - File operations (read, write, edit)
   - Memory search and append

### Key Differences

1. **Bridge-Based Channels**: OpenClaw has built-in remote channels; LocalGPT uses standalone bridge binaries (Telegram, Discord, WhatsApp) that connect to the daemon via secure IPC
2. **No Plugin System**: OpenClaw has an extension architecture; LocalGPT is monolithic
3. **Bridge Architecture**: OpenClaw's gateway handles multi-channel routing; LocalGPT uses a bridge daemon model with encrypted credential exchange
4. **Web UI**: OpenClaw has a full web-based chat UI; LocalGPT has an embedded web UI served by the daemon, plus a desktop GUI (egui)

---

## Current Implementation Status

### Completed (MVP)

- [x] CLI interface (`chat`, `ask`, `daemon`, `memory`, `config` commands)
- [x] LLM providers (OpenAI, Anthropic, Ollama)
- [x] Memory files (MEMORY.md, memory/*.md, HEARTBEAT.md)
- [x] Memory search (FTS5)
- [x] Daemon mode with heartbeat
- [x] Basic tool set (bash, file ops, memory ops, web fetch)
- [x] TOML configuration
- [x] HTTP server with REST API
- [x] Session management with compaction

### Gaps and Remaining Work

#### High Priority

1. **Vector Search / Embeddings**
   - Currently: FTS5 keyword search only
   - Needed: Semantic search with embeddings
   - Options: `sqlite-vec`, local ONNX embeddings, or API-based
   - Effort: Medium

2. **Thread Safety for Agent**
   - Currently: Agent contains SQLite connection (not `Send`/`Sync`)
   - Impact: HTTP server creates new agent per request (no session persistence)
   - Fix: Wrap connection in `Arc<Mutex<>>` or use connection pooling
   - Effort: Medium

3. **Streaming Responses**
   - Currently: Full response only
   - Needed: SSE/WebSocket streaming for real-time output
   - WebSocket handler is stubbed but not connected
   - Effort: Medium

4. **Proper Token Counting**
   - Currently: Rough estimate (4 chars = 1 token)
   - Needed: Use `tiktoken-rs` properly per model
   - Effort: Low

#### Medium Priority

5. **Background Daemonization**
   - Currently: `--foreground` mode only
   - Needed: Proper Unix daemon with `fork()`
   - Effort: Low

6. **Memory Flush Prompts**
   - Currently: Pre-compaction flush is basic
   - Needed: Better integration with daily logs
   - Effort: Low

7. **Session Resume in HTTP**
   - Currently: Each HTTP request is stateless
   - Needed: Session ID tracking across requests
   - Effort: Medium

8. **Error Handling**
   - Currently: Basic `anyhow` errors
   - Needed: Structured error types, better user messages
   - Effort: Medium

#### Low Priority / Nice to Have

9. **Local LLM Inference**
   - Add `llama-cpp-rs` for fully offline operation
   - Effort: High

10. **TUI Interface**
    - Rich terminal UI with `ratatui`
    - Effort: High

11. **Shell Completions**
    - Bash/Zsh/Fish completion scripts
    - Effort: Low

12. **Systemd/Launchd Integration**
    - Service files for auto-start
    - Effort: Low

13. **Metrics/Observability**
    - Token usage tracking
    - Request latency metrics
    - Effort: Medium

14. **Multi-Session Support**
    - Multiple concurrent chat sessions
    - Session listing and management
    - Effort: Medium

---

## File Structure Reference

```
~/.localgpt/
├── config.toml              # Main configuration
├── workspace/
│   ├── MEMORY.md            # Curated long-term knowledge
│   ├── HEARTBEAT.md         # Pending tasks/reminders
│   └── memory/
│       ├── 2024-01-15.md    # Daily append-only logs
│       └── ...
├── sessions/
│   └── <session_id>.jsonl   # Conversation transcripts
├── memory.sqlite            # FTS index
└── logs/
    └── agent.log            # Operation logs
```

---

## Contributing

When working on LocalGPT:

1. Keep the codebase small and focused
2. Prefer simplicity over features
3. Maintain Rust idioms and safety
4. Test with `cargo test`
5. Format with `cargo fmt`
6. Lint with `cargo clippy`
