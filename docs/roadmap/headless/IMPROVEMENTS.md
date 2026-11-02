# Headless Pipeline — Code & Docs Improvement Plan

Issues identified from code review of the headless generation pipeline implementation.

Last updated: 2026-03-18

---

## Code Issues

### Critical

| # | Issue | File | Status |
|---|-------|------|--------|
| C1 | Unsafe `Send+Sync` impl on `RelayTools` — removed (Tool trait already requires Send+Sync) | `gen3d/mcp_relay.rs` | DONE |
| C2 | Detached threads without `JoinHandle` — partial writes on exit | `main.rs` | WONT FIX (Bevy owns main thread; threads are tied to process lifetime) |
| C3 | Hard-coded 500ms sleep assumes Bevy init time — should use readiness signal | `heartbeat_gen.rs:70,138` | DEFERRED (heartbeat gen dispatch not yet wired) |

### High

| # | Issue | File | Status |
|---|-------|------|--------|
| H1 | No file locking on experiment JSONL — added flock on Unix | `experiment.rs` | DONE |
| H2 | Silent JSON serialization failure — now returns error response | `mcp_relay.rs` | DONE |
| H3 | GPU lock no-op on Windows — added platform-specific locking | `gpu_lock.rs` | DONE |

### Medium

| # | Issue | File | Status |
|---|-------|------|--------|
| M1 | MCP server `process::exit(0)` kills gen on disconnect | `main.rs` | WONT FIX (correct for standalone MCP mode; interactive mode has separate path) |
| M2 | Gallery UI clones 8 fields per entry per frame | `gallery_ui.rs` | DEFERRED (premature optimization; no perf issue with <50 worlds) |
| M3 | Stale relay port file not cleaned up on exit | `main.rs` | DONE |
| M4 | No connection timeout in MCP relay — added 5 min idle timeout | `mcp_relay.rs` | DONE |

## Documentation Issues

### Critical

| # | Issue | File | Status |
|---|-------|------|--------|
| D1 | 29 undocumented MCP tools — added WorldGen (15), asset gen (3), experiment (3) sections | `mcp-server.md` | DONE |
| D2 | Tool count wrong — updated to "50+ MCP-only tools" | `index.md` | DONE |

### Moderate

| # | Issue | File | Status |
|---|-------|------|--------|
| D3 | Templates marked "coming soon" — removed label | `index.md` | DONE |
| D4 | WorldGen pipeline doc page — pipeline stages, 15 tools, BlockoutSpec format, three-tier placement, semantic roles | `website/docs/gen/worldgen.md` | DONE |
| D5 | External services doc — Ollama, ComfyUI, model server setup with status badges | ~~`website/docs/gen/external-services.md`~~ | REMOVED 2026-11-01 with the services |

## Additional Fixes (second pass)

| # | Issue | Status |
|---|-------|--------|
| A1 | `GEN_MEMORY_PROMPT` not injected into agents — now wired for both interactive and headless | DONE |
| A2 | `/gallery` and `/experiments` missing from `/help` output — registered in commands.rs | DONE |
| A3 | `windows-sys` dep missing from Cargo.toml — GPU lock would fail to compile on Windows | DONE |
| A4 | `compact()` missing file lock (same issue as append) | DONE |
| A5 | heartbeat_gen duplicated 95 lines — extracted `boot_headless_and_create_tools` | DONE |
| A6 | experiment_tools: empty string validation + max 50 variations limit | DONE |
| A7 | Gallery Load button was a no-op — wired via PendingGalleryLoad + cmd_tx injection | DONE |

## Summary

- **All actionable issues resolved.**
- **18 fixed** (C1, H1, H2, H3, M3, M4, D1-D5, A1-A7)
- **2 won't fix** (C2, M1 — by design)
- **2 deferred** (C3: Bevy readiness signal, M2: gallery UI perf — need runtime context)
