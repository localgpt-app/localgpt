# Plugin lifecycle: applying and unapplying without restart

Four independent reviews of the same question, one per model: **how does
`deepseek-harness` apply and unapply plugins cleanly without a process restart,
which of those ideas port to Rust, and what should LocalGPT do about it?**

The subject repo (`deepseek-harness`, a TypeScript agent harness built on the
vendored **Cordis** framework) is a read-only reference checkout in the parent
monorepo's `external/` directory, not part of this repository. Paths of the form
`external/deepseek-harness/...` and `vendor/cordis/...` in these documents refer
to it and will not resolve from inside this repo.

## The documents

| Document | Model | Size | Strongest contribution |
|---|---|---|---|
| [glm5.3-plugin-architecture-review.md](glm5.3-plugin-architecture-review.md) | GLM 5.3 | 29K | Full options analysis (A–E) with effort/risk/dependency table; verified inventory of every LocalGPT seam |
| [opus-5-deepseek-harness-plugin-lifecycle.md](opus-5-deepseek-harness-plugin-lifecycle.md) | Opus 5 | 19K | Mechanism deep-dive (the epoch trick, unprovide-waits-for-consumers); working Rust `Scope` code; 7 portable rules |
| [k3-deepseek-harness-plugin-review.md](k3-deepseek-harness-plugin-review.md) | K3 | 10K | Tightest and most actionable: a 5-step MCP hot-plugin implementation slice with a test plan |
| [gemini-3.1-pro-deepseek_harness_review.md](gemini-3.1-pro-deepseek_harness_review.md) | Gemini 3.1 Pro | 5K | Broad survey of dynamic-loading options (WASM / scripting / dylib). **Contains errors — see below.** |

Start with **k3** for the implementation plan, **glm5.3** for the decision
record, **opus-5** for why the mechanism works.

## What all four agree on

Independent agreement across four models is the high-confidence signal here.

1. **The load-bearing idea is that every registration returns its own undo.**
   Not dynamic code loading — the harness never loads new machine code at
   runtime. Registration-carries-its-inverse is the whole trick; everything else
   is supporting structure.
2. **Teardown unwinds in reverse registration order, with per-disposer error
   containment**, and must be *awaited to quiescence* — not merely requested.
3. **Rust has no async `Drop`.** Teardown must be an explicit `async fn
   dispose()`; `Drop` is a best-effort backstop and leak detector only.
4. **Do not port the Proxy-based context.** JavaScript `Proxy`-resolved service
   lookup has no sound Rust analogue; use explicit typed handles.
5. **Do not use `libloading`/dylibs.** No stable Rust ABI, and unloading with any
   live reference is undefined behavior. WASM (`wasmtime`) is the only credible
   in-process option, and LocalGPT probably does not need it — MCP subprocesses
   already provide the code-loading boundary.
6. **Skip epoch-based dependency reactivity for now.** It pays off when providers
   and consumers churn interdependently; LocalGPT's dependency graph is shallow
   and static.

The three substantive reviews also converge on the same sequencing: **a
scope/disposer primitive first, then per-server MCP mount/unmount, then a
config reconciler subscribed to the existing watcher.** glm5.3 prepends a
bugfix-grade step; k3 folds it in.

## Findings verified against the tree

Spot-checked at review time (2026-08-16), all confirmed:

- **`Agent::new` connects MCP servers and immediately drops the manager.**
  `crates/core/src/agent/mod.rs:280` binds `Ok((_manager, mcp_tools))` — only the
  tools survive. `McpManager` has no `Drop` impl and `McpClient::shutdown()` is
  never called, so stdio MCP child processes are never cleanly terminated. This
  is a resource leak independent of any plugin work. (k3, glm5.3)
- **`ConfigWatcher` is built, started, advertised, and unsubscribed.**
  `crates/cli/src/cli/daemon.rs:186` takes it as `_config_watcher` with the
  comment "most services currently use the config passed at startup." The
  detection half of `GAPS.md` #21 shipped; the application half did not. (all
  four)
- **`HookEngine` is defined but never fired.** `crates/core/src/hooks/` has
  before/after-tool-call events that can Allow/Block, but the only reference
  outside the module is the management CLI — `Agent::execute_tool` never
  consults it. (glm5.3)
- **The tool list is append-only.** `crates/core/src/agent/mod.rs:105` plus
  `extend_tools` at `:491`, with no inverse. This is the root cause of the four
  "Restart the daemon to apply changes" messages in
  `crates/cli/src/cli/tool.rs`. Tracked as `GAPS.md` #60 `8-dynamic-loading`
  (❌). (all four)
- **`POST /api/config` leaves in-memory config stale.** `crates/server/src/http.rs`
  `set_config` writes `config.toml` but `AppState.config` is a startup snapshot.
  A subscribed `ConfigWatcher` fixes this for free. (glm5.3)
- **`DockerSandbox::drop` blocks on `docker rm -f`** inside `Drop` in a tokio
  runtime (`crates/sandbox/src/docker.rs:199`) — requests cleanup rather than
  reaching it. (opus-5)
- **`AgentActor::spawn_supervised` never restarts on panic** — `TODO` at
  `crates/core/src/concurrency/actor.rs:543`. (glm5.3)

## Corrections to the Gemini 3.1 Pro review

Kept for completeness of the model comparison, but **do not action its
LocalGPT-specific recommendations without checking them.** It is the only one of
the four that cites no file/line references and verified nothing against the
tree.

- **`bevy_dynamic_plugin` does not exist.** §3C recommends adopting it for
  development-time hot reload. That crate was deprecated in Bevy 0.14 and removed
  in 0.15; this repo is on Bevy 0.19 (`crates/gen/Cargo.toml:34`). The
  recommendation is not actionable.
- **The Wasm-tool-upload endpoint ignores the security model.** §3A proposes that
  the API server accept an uploaded Wasm module as a new tool. LocalGPT has a
  `PermissionLevel`/`ApprovalGate` model and a protected-files deny list; a
  remote code-upload path would have to engage both. The
  document does not mention them.
- **§3A's premise is a non sequitur** — it moves from "LocalGPT uses
  Landlock/Seatbelt sandboxing for tools" to "adding new internal tools likely
  requires recompiling core." Both statements are true but unrelated, and the
  second is hedged rather than verified.

Its useful contribution is the §2C survey of the three dynamic-loading
mechanisms (Wasm / scripting / dylib) and their tradeoffs, which is consistent
with the other reviews.

## Status

Analysis only. No implementation has been done, and no TODO spec has been cut.
The natural next step is a `todo/TODO-*.md` spec for the scope/registry
primitive, per the shared sequencing above.
