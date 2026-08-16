# DeepSeek Harness Plugin Lifecycle — Review and Application to LocalGPT (Rust)

> Review of `external/deepseek-harness` (vendored Cordis framework): how it applies and
> unapplies plugins cleanly without process restart, which ideas transfer to Rust codebases
> in general, and a concrete adoption proposal for LocalGPT. All file references verified
> against source on 2026-08-16.

## 1. How Cordis achieves clean apply/unapply without restart

The harness (`external/deepseek-harness`) is a TypeScript agent harness on a vendored copy
of the **Cordis** framework (`vendor/cordis`), where "everything is a plugin" and hot
reload is driven by `cordis.yml`. Five mechanisms make teardown clean:

### 1.1 Registrations are effects

Every contribution — event listener, service, timer, child plugin — goes through
`ctx.effect()` / `ctx.on()`, and every registry's `register()` returns its disposer
(`AGENTS.md:102`, `vendor/cordis/src/fiber.ts:415-561`, `vendor/cordis/src/events.ts:254-260`).
This is a lint-level project rule, not a convention: nothing enters the runtime without a
registered undo.

### 1.2 Scope = effect stack with reverse-order async disposal

Each plugin runs on a `Fiber` owning a disposer list. Unload awaits all disposers **LIFO**,
containing per-disposer errors (`fiber.ts:675-686`, `vendor/cordis/src/utils.ts:27-31`).
Async disposers are fully awaited — disposal resolves only when teardown actually finished.

### 1.3 Ownership tree

A child fiber's `dispose` is itself registered as an effect on the parent (`fiber.ts:265`),
so disposing a plugin unwinds its whole subtree. Dispose is idempotent (a second call joins
the in-flight disposal) and creating an effect on a disposed fiber throws
(`INACTIVE_EFFECT`, `fiber.ts:419-422`).

### 1.4 Consumer restart instead of reference rebinding

`ctx.inject(deps, callback)` ties a consumer to its services. When a provider unloads,
dependent consumers are torn down to PENDING and re-run from scratch when the provider
returns (`vendor/cordis/src/reflect.ts:297-336`, `fiber.ts:611-696`). The provider's
disposer waits for dependents to settle before completing. There is no proxy invalidation;
the project convention is *access the service through `ctx` per call, never cache it*.

### 1.5 Transactional config reconciliation

The loader diffs `cordis.yml` entries by id on file change: no-op / dispose-only
(newly disabled) / in-place restart with new config / dispose + re-import — with **rollback**
to the previous entry list on failure (`vendor/loader/src/config/entry.ts:142-246`,
`config/group.ts:59-106`). Updates are serialized per include tree (explicitly
non-reentrant). Plugin-local state does not survive reload by design; what survives is
config, the durable session log, and state deliberately parked at lower, longer-lived seams
(e.g. a background process is owned by the subprocess seam, so it survives a shell-executor
reload).

### 1.6 Dispose must reach quiescence

Disposers abort *and join* their work. The subprocess provider escalates SIGTERM → SIGKILL
and awaits whole-tree exit before its disposer returns
(`packages/subprocess/subprocess-local/src/index.ts:49-102`). "Teardown that fires kills but
returns before work stops" is documented as a defect class (`docs/defensive-patterns.md:19-21`).

## 2. What does NOT port to Rust

- **Proxy-based contexts / traceable service proxies** — JavaScript `Proxy` has no cheap
  sound Rust analogue. Use explicit registry handles instead; Rust's type system prefers
  this anyway.
- **Code hot-reload via module-cache surgery** — dylib unloading (`libloading`) is
  unsound-adjacent: any leaked reference or live thread makes unloading undefined behavior,
  and Rust has no stable ABI across compiler versions. The honest "no restart" units in
  Rust are:
  1. **in-process scopes** for state/config/composition changes (tools, providers,
     bridges, watchers), and
  2. **subprocess plugins** for true code apply/unapply — the process boundary is the
     unload guarantee. (WebAssembly via `wasmtime` is the third option when in-process
     code hot-swap with a strong safety story is genuinely needed.)

LocalGPT already bet on MCP subprocesses for external tools; the harness architecture
validates that bet and shows how to manage their lifecycle.

## 3. Patterns that generalize to any Rust codebase

- **Effect-stack `Scope`** with async reverse-order disposal — replaces abort-all teardown.
- **Every `register()` returns a `Disposer`** — enforceable as a review/lint rule; the
  registry never offers an unregister-less registration path.
- **Scope tree for structured concurrency** — child scopes registered as effects on the
  parent; dispose = quiescence (join tasks, not `abort()`).
- **Config reconciliation = diff-by-id + transactional rollback**, serialized on a single
  task (never reentrant).
- **Provider swap → kill-and-respawn the consumer** from a `watch` channel, instead of
  mutating shared references or trying to invalidate held handles.
- **Fail loud on lifecycle misuse** — registering into a disposed scope or duplicate
  registration is a hard error, not a silent skip.

## 4. LocalGPT's current gaps (verified)

- `localgpt plugin add/enable/disable/remove` exists (`crates/cli/src/cli/tool.rs:14-49`)
  but every path prints **"Restart the daemon to apply changes"**.
- `Agent::new` connects MCP servers, then **drops `McpManager` immediately**
  (`crates/core/src/agent/mod.rs:280`) — stdio MCP child processes are never cleanly shut
  down; they die with the process.
- Tools are a hand-built `Vec<Box<dyn Tool>>` (`crates/core/src/agent/mod.rs:105`,
  `crates/core/src/mcp/server.rs:112`) with linear-scan dispatch and no unregistration.
- `ConfigWatcher` already broadcasts config over a `watch` channel
  (`crates/core/src/config/watcher.rs:24,126`), but the daemon notes *"most services
  currently use the config passed at startup"* (`crates/cli/src/cli/daemon.rs:194-196`) —
  nothing reconciles deltas.
- No `CancellationToken` anywhere; daemon shutdown is `JoinSet` abort-all; no SIGTERM
  handler in background mode; two bridge architectures (in-process Telegram task vs. tarpc
  bridge daemons) with no shared lifecycle trait; LLM provider selection is one monolithic
  `match` (`crates/core/src/agent/providers.rs:475`).

## 5. Adoption proposal: MCP hot-plugin slice

**Goal: `localgpt plugin enable/disable` takes effect on the running daemon within ~1s, no
restart — by porting the effect/scope + registry + config-reconcile ideas.**

### Step 1 — `Scope` primitive (`crates/core/src/scope.rs`, new, ~200 lines + tests)

```rust
impl Scope {
    pub fn root() -> Self;
    pub fn child(&self) -> Scope;                 // child's dispose is an effect on the parent
    pub fn effect<F, D, Fut>(&self, f: F) -> Result<Disposer>; // Err if scope is disposing
    pub async fn dispose(&self);                  // idempotent, LIFO, await each, contain errors
}
```

Disposers are `Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>`. Cordis semantics:
effect on a disposed scope errors; `dispose()` resolves only after all disposers complete.

### Step 2 — Shared tool registry (`crates/core/src/agent/tools/registry.rs`, new)

`ToolRegistry` (`Arc` + `RwLock`, insertion-ordered). API: `register(tool) -> Disposer`
(built on `Scope::effect`), `list()`, `find(name)`. Rewire dispatch to look up per call —
Cordis's "access per call, don't cache" convention, so a tool that vanishes mid-session
simply stops resolving:

- `Agent.tools` (`agent/mod.rs:105`, dispatch at `:1348-1353`, `extend_tools` at `:491`)
- `ToolHandler.tools` (`crates/core/src/mcp/server.rs:112-125`, dispatch `:166-175`)
- Factories (`create_safe_tools`, `create_cli_tools`, gen factories) register into a
  passed-in registry instead of returning a `Vec`.

### Step 3 — MCP plugin supervisor (`crates/core/src/mcp/supervisor.rs`, new)

`McpSupervisor::start(root: &Scope, watch::Receiver<Config>, registry: ToolRegistry)` — one
serialized reconcile task holding `HashMap<String, (McpServerConfig, Scope)>`:

- server removed/disabled → dispose its child scope: disposer unregisters that server's
  tools **and** awaits `McpClient::shutdown()` (kills the stdio child via
  `crates/core/src/mcp/transport.rs:146-152`) — this also fixes the dropped-`McpManager`
  leak;
- server added → child scope: connect (`mcp/mod.rs:83`), wrap tools as `McpTool`, register
  each into the registry;
- config changed → dispose + re-add; on re-add failure, roll back by reconnecting the
  previous config (Cordis entry rollback);
- one server's failure never blocks the others (current behavior, kept).

### Step 4 — Daemon integration + CLI messaging

- `run_daemon_services` (`crates/cli/src/cli/daemon.rs:181-401`): create root `Scope` +
  `ToolRegistry`, start the supervisor on the existing `ConfigWatcher` subscription, share
  the registry into `AppState` / the session-agent path.
- One-shot CLI path (`Agent::new`): keep connect-at-construction, but hold the manager and
  add an explicit `async fn shutdown()` called by CLI call sites.
- `crates/cli/src/cli/tool.rs`: replace "Restart the daemon to apply changes" with
  live-apply messaging (config watcher picks up the edit; note the not-running fallback).
- Update `AGENTS.md` (MCP section) and related docs for the live apply/unapply behavior.

### Step 5 — Tests

- scope: LIFO order, idempotent dispose, child-on-parent unwind, effect-after-dispose
  error, per-disposer error containment;
- registry: register/dispose visibility, duplicate-name policy (fail loud);
- supervisor integration: fixture stdio MCP server, drive enable/disable/config-change
  through the `watch` channel, assert tools appear/vanish and the child process exits;
- update existing tests that construct `Agent`/`ToolHandler` with `Vec` tools.

## 6. Later phases (same primitive, bigger blast radius)

- **Unified graceful shutdown** — root scope tree for all daemon tasks, SIGTERM handler,
  replace `JoinSet` abort-all with quiescence-reaching disposal.
- **Provider registry** — name → factory map replacing the monolithic `match` at
  `providers.rs:475`; decouples core from per-provider feature gates.
- **Common bridge lifecycle trait** — unify the in-process Telegram task and the tarpc
  bridge daemons under `start`/`stop`.
- **Gen/Bevy** — Bevy plugins are static; runtime extensions would go through scripting or
  `bevy_dynamic_plugin` in dev, with the same scope/disposer discipline for cleanup.
