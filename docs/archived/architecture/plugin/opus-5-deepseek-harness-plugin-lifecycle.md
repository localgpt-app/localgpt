# Clean plugin apply/unapply: what deepseek-harness does, and what LocalGPT can take

Review of `external/deepseek-harness` (TypeScript, vendored Cordis) with a
translation to Rust and a concrete list of changes for `localgpt/`.

---

## 1. What the harness actually does

The harness is not "a plugin system" in the dlopen sense. It never loads new
machine code at runtime. What it has is **a discipline that makes every
registration reversible**, plus a small state machine that decides when to run
the reversals. That discipline is the transferable part.

### 1.1 Registration carries its own undo

The rule is stated in `AGENTS.md` as an invariant:

> **Registrations are effects**: every contribution goes through `ctx.effect()` /
> `ctx.on()`; a registry's `register()` returns the disposer.

A plugin never tracks its own teardown. It calls `ctx.on(...)`,
`ctx.provide(...)`, `ctx.tools.register(...)`, and each of those pushes an
inverse onto the *calling plugin instance's* disposer list. For a resource the
framework doesn't know about (a timer, a socket, a watcher), the author wraps it
in `ctx.effect()` and returns a closure that releases it. Nothing else is
required, and nothing else is allowed:

```ts
ctx.effect(() => {
  const timer = setInterval(tick, 200)
  return () => clearInterval(timer)     // ← the undo
})
```

Unloading is then mechanical: walk the list backwards, run everything.

### 1.2 A fiber is one loaded plugin instance

`vendor/cordis/src/fiber.ts` — a `Fiber` owns lifecycle state, validated config,
and a `DisposableList` of effects. States:

```
PENDING → LOADING → ACTIVE → UNLOADING → DISPOSED
                 ↘ FAILED
```

**`PENDING` is a legitimate resting state, not an error.** A plugin whose
dependency is missing simply waits — it does not fail, and it does not crash the
app. This is the thing that makes *unapply* safe: removing a service parks its
consumers instead of breaking them.

### 1.3 The epoch trick (the clever bit)

`Fiber._refresh()` builds an epoch string from the *identity of the fibers
currently providing each injected service*:

```ts
for (const name of Object.keys(this.inject)) {
  const impl = this._store[name]
  if (!impl) { epoch = INACTIVE; break }
  epoch += ':' + impl.fiber.uid          // provider identity, not just presence
}
this._setEpoch(epoch)
```

`_setEpoch` compares against the previous value and drives the transition:
INACTIVE → live means load; live → INACTIVE means unload; **live → *different*
live means the provider was swapped, so reload against the new one.**

The consequence is that nobody has to hand-maintain a reverse-dependency graph
to answer "who breaks if I unload this?". The epoch comparison answers it. Swap
an LLM provider plugin and every consumer restarts against the new one; remove
it and they all park in PENDING.

### 1.4 Unproviding waits for consumers

`reflect.ts`, the disposer returned by `ctx.provide()`:

```ts
return async () => {
  delete this.store[key]
  const fibers = this.notify([name])                       // recompute dependents
  await Promise.allSettled(fibers.map(f => f.await()))     // wait for them to settle
  delete this.ctx.fiber.store![name]                       // only then drop self
}
```

The provider does not finish tearing down until its consumers have finished
reacting. That ordering guarantee is what keeps unapply from leaving half-torn
state behind.

### 1.5 Config edits diff, they don't restart

The loader diffs config entries **by stable `id`**, so an edit to one entry
mounts/unmounts/reconfigures only that entry. From the tutorial, an operational
detail that is easy to get wrong:

> an entry without one gets a generated id on every read, so after any
> config-file edit it counts as removed-plus-added and remounts even if its own
> lines did not change.

`disabled: true` keeps the entry but unmounts it. `fiber.update(config)`
**validates first**, then runs an `internal/update` waterfall (which HMR can veto
or replace), then restarts. A config that fails validation never tears down the
running instance.

### 1.6 Teardown ordering is specified, not incidental

From the tutorial, stated as a caveat because it bit someone:

> disposers start in reverse registration order, but multiple **async** disposers
> run concurrently. If teardown steps must run in sequence, keep them in one
> disposer and await them there.

And from `docs/defensive-patterns.md`, the rule that matters most for Rust:

> **Dispose must reach quiescence, not just request it.** A teardown that issues
> kills/aborts but returns before the work stops leaves orphans. Make cleanup
> async and await the children's exit (kill → await `done`), and close
> listener/notification registries BEFORE killing so late completions stay silent.

### 1.7 Effects are labeled, so leaks are debuggable

`ctx.effect(fn, label)` and `fiber.getEffects()` return a labeled tree —
`ctx.on("event")`, `ctx.provide("llm")`. You can ask a running process what is
registered and who owns it. `cordis_inspect` exposes exactly this to the model:
services, live fibers, registered tools, fiber states.

---

## 2. What does *not* transfer to Rust

Worth being blunt about, because "plugins without restart" invites the wrong
implementation.

**Hot module replacement does not transfer.** The harness's HMR works because
Node can re-import a module. Rust is AOT-compiled with no stable ABI: `dlopen`ing
a `cdylib` means matching compiler version, allocator, and panic strategy exactly,
and unwinding across the FFI boundary is UB. If you genuinely need in-process
third-party code at runtime, the answer is WASM (wasmtime + component model —
which is what `external/claw/ironclaw/` does), not native dynamic libraries.

The realistic Rust scope is: **hot-swap configuration and out-of-process
components** — MCP servers, subprocess bridges, hooks, cron jobs, HTTP routes,
provider backends. That covers essentially everything a user means by "I added an
MCP server, why do I have to restart the daemon?"

**Proxy-based service resolution does not transfer.** Cordis resolves `ctx.llm`
through a JS `Proxy` that re-reads a store on every access, which is how
consumers transparently see provider swaps. Rust has no equivalent. Consumers
either re-resolve from a registry on each use, or hold a `watch::Receiver` and
restart on change. The epoch generation counter is the honest translation; the
proxy is not.

**String-keyed `inject: ['timer']` DI is a downgrade in Rust.** It trades
compile-time wiring checks for runtime PENDING states. Keep static wiring for
everything known at compile time; use a dynamic registry only for the genuinely
dynamic set (config-driven entries).

---

## 3. The Rust core: a scope that owns its undo

Rust's `Drop` is *almost* the disposer, and the gap is exactly the one the
harness calls out: **`Drop` cannot await.** Killing a child process and waiting
for its exit, closing a WebSocket, flushing a DB — none of that fits in `Drop`.
So `Drop` becomes best-effort plus leak detection, and the real teardown is an
explicit async method.

```rust
use futures::future::BoxFuture;
use std::sync::Mutex;

type Disposer = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>;

/// Accumulates teardown for one plugin instance.
///
/// Every registration made against a scope pushes its own inverse. `dispose`
/// runs those inverses in reverse registration order and awaits each one, so a
/// disposed scope has genuinely released its resources rather than merely asked
/// them to stop.
#[derive(Default)]
pub struct Scope {
    disposers: Mutex<Vec<(&'static str, Disposer)>>,
}

impl Scope {
    /// Register teardown work, labeled for diagnostics.
    pub fn defer<F, Fut>(&self, label: &'static str, undo: F)
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.disposers
            .lock()
            .unwrap()
            .push((label, Box::new(move || Box::pin(undo()))));
    }

    /// Run every deferred disposer in reverse order, awaiting each in turn.
    pub async fn dispose(&self) {
        let queued = std::mem::take(&mut *self.disposers.lock().unwrap());
        for (label, undo) in queued.into_iter().rev() {
            tracing::debug!(effect = label, "disposing");
            undo().await;
        }
    }

    /// Live registrations, for a `list-registrations` style diagnostic.
    pub fn effects(&self) -> Vec<&'static str> {
        self.disposers.lock().unwrap().iter().map(|(l, _)| *l).collect()
    }
}
```

Registries then take a `&Scope` and push their own inverse:

```rust
impl ToolRegistry {
    /// Register a tool for the lifetime of `scope`.
    pub fn register(&self, scope: &Scope, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.write().unwrap().insert(name.clone(), tool);

        let tools = Arc::clone(&self.tools);
        scope.defer("tools.register", move || async move {
            tools.write().unwrap().remove(&name);
        });
    }
}
```

**Task cancellation is where Rust-specific care is needed.** Dropping a
`JoinHandle` detaches the task; `JoinSet` drop aborts it at an await point,
skipping cleanup. Neither reaches quiescence. A scope that owns a task should
signal *and join*:

```rust
let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);   // no new dep
let task = tokio::spawn(run_until_stopped(stop_rx));
scope.defer("spawn(bridge)", move || async move {
    let _ = stop_tx.send(true);
    let _ = task.await;      // ← quiescence, not just a request
});
```

---

## 4. LocalGPT: findings

Ordered by leverage. Every one is a case where the harness's discipline is
absent and the symptom is a restart.

### 4.1 The tool list is append-only — this is the root cause

[`crates/core/src/agent/mod.rs:105`](../../../crates/core/src/agent/mod.rs) holds
`tools: Vec<Box<dyn Tool>>`, and
[`extend_tools`](../../../crates/core/src/agent/mod.rs) at line 491 is:

```rust
pub fn extend_tools(&mut self, extra: Vec<Box<dyn Tool>>) {
    self.tools.extend(extra);
}
```

There is no inverse. No `remove_tool`, no handle, no scope. Every downstream
"restart required" traces back here. The user-visible proof is in
[`crates/cli/src/cli/tool.rs`](../../../crates/cli/src/cli/tool.rs), which
prints **"Restart the daemon to apply changes."** at lines 173, 199, 223, and 247.

`todo/GAPS.md` #60 `8-dynamic-loading` ("Dynamic tool loading") is still ❌ — this
is that item.

**Change:** replace the owned `Vec` with a shared
`Arc<RwLock<HashMap<String, Arc<dyn Tool>>>>` registry that the agent reads from,
and make `register` take a `&Scope`. The `Agent` is deliberately not `Send + Sync`
(SQLite), but the registry can be, and multiple agents can share one — which is
what makes a live `tool add` visible to the daemon, bridges, and heartbeat at
once.

### 4.2 MCP servers mount as one indivisible batch

[`crates/core/src/mcp/mod.rs:35`](../../../crates/core/src/mcp/mod.rs)
`McpManager::connect_all` connects every configured server and returns one flat
`Vec<Box<dyn Tool>>`. `shutdown` at line 70 tears down *all* clients. There is no
per-server handle, so there is no way to express "unmount server X".

A failed server is `warn!`ed and dropped (line 60) with no record and no retry
path — the only recovery is a process restart. The harness would keep that entry
PENDING with a reason and retry on the next reconcile.

**Change:** per-server scope. Note that registration *order* does the right thing
for free under reverse disposal:

```rust
impl McpManager {
    /// Connect one server and register its tools. Disposing the returned
    /// handle's scope removes exactly this server's tools and closes its
    /// transport.
    pub async fn mount(
        &self,
        cfg: &McpServerConfig,
        tools: &ToolRegistry,
    ) -> Result<McpServerHandle> {
        let scope = Arc::new(Scope::default());
        let client = Arc::new(McpClient::connect(transport_for(cfg).await?, "localgpt").await?);

        // Registered FIRST, so under reverse-order disposal it runs LAST:
        // tools are unregistered before the transport closes, never after.
        let c = Arc::clone(&client);
        scope.defer("mcp.client", move || async move {
            if let Err(e) = c.shutdown().await {
                tracing::warn!("MCP '{}' shutdown: {e}", c.server_name());
            }
        });

        for def in client.list_tools().await? {
            tools.register(&scope, Arc::new(McpTool::new(
                &cfg.name, &def.name,
                def.description.as_deref().unwrap_or(""),
                def.input_schema.clone(), Arc::clone(&client),
            )));
        }

        Ok(McpServerHandle { name: cfg.name.clone(), scope })
    }
}
```

### 4.3 Config hot-reload is wired but nothing consumes it

[`crates/cli/src/cli/daemon.rs:186`](../../../crates/cli/src/cli/daemon.rs) —
the parameter is literally named `_config_watcher`, with the comment:

> Services that need hot-reload should subscribe to `config_watcher.subscribe()`
> and update their internal state when a new config is received. For simplicity,
> most services currently use the config passed at startup.

So [`ConfigWatcher`](../../../crates/core/src/config/watcher.rs) reloads the
file, debounces, and broadcasts on a `watch` channel — and has no subscribers.
`GAPS.md` #21 `9-hot-reload` is marked ✅, but what shipped is the *detection*
half. The *application* half — Cordis's loader diff — is missing.

**Change:** a reconciler that diffs desired config entries against mounted ones.
This is the whole loader, and it is small:

```rust
pub async fn reconcile(&mut self, cfg: &Config, tools: &ToolRegistry) {
    let desired: HashMap<&str, &McpServerConfig> = cfg.mcp.servers.iter()
        .filter(|s| s.enabled)                       // ← `disabled` already exists
        .map(|s| (s.name.as_str(), s))
        .collect();

    // Unmount what is gone, disabled, or reconfigured.
    let stale: Vec<String> = self.mounted.iter()
        .filter(|(name, m)| desired.get(name.as_str())
            .is_none_or(|want| m.fingerprint != fingerprint(want)))
        .map(|(name, _)| name.clone())
        .collect();

    for name in stale {
        if let Some(m) = self.mounted.remove(&name) {
            m.scope.dispose().await;
        }
    }

    // Mount what is new or changed; a failure stays retryable, not lost.
    for (name, want) in desired {
        if self.mounted.contains_key(name) { continue; }
        match self.mount(want, tools).await {
            Ok(h) => { self.mounted.insert(name.to_string(), h); }
            Err(e) => self.pending.insert(name.to_string(), e.to_string()),
        }
    }
}
```

`McpServerConfig.enabled` already exists
([`config/mod.rs:1027`](../../../crates/core/src/config/mod.rs)) — the data
model is already Cordis-shaped. Only the runtime application is missing.

The same reconciler generalizes to cron jobs, hooks, and bridges, all of which
have `enabled` flags and stable names to key on.

### 4.4 Config entries need stable ids, and `name` mostly already is one

MCP servers, bridges, and hooks key on `name`; cron jobs should be checked. The
Cordis warning applies directly: if the identity is derived from array position
or regenerated per read, every config save remounts everything and the reconciler
becomes a disguised restart. Key on the declared name and treat a rename as
remove-plus-add.

### 4.5 Async teardown is being attempted in `Drop`

[`crates/sandbox/src/docker.rs:199`](../../../crates/sandbox/src/docker.rs):

```rust
impl Drop for DockerSandbox {
    fn drop(&mut self) {
        // Best-effort synchronous cleanup
        let _ = std::process::Command::new(rt).args(["rm", "-f", id]).output();
    }
}
```

This is a blocking `.output()` on a container removal, inside `Drop`, in a tokio
runtime — it stalls a reactor thread for the duration, and if the runtime is
already shutting down it may not complete at all. It is the precise case
`defensive-patterns.md` names: cleanup that requests rather than reaches
quiescence.

**Change:** an async `shutdown(&self)` that awaits `docker rm -f`, called by the
owning scope's disposer. Keep `Drop` as a last-resort best-effort *plus* a
`tracing::warn!` when it fires without a prior `shutdown()` — that warning turns
a silent leak into a visible bug, which is how the harness's labeled effects
earn their keep.

Only five `Drop` impls exist across the workspace, so this is not a broad
refactor; it is two or three call sites.

### 4.6 There is no way to ask what is registered

The harness ships `cordis_inspect` over live fibers, services, and tools. LocalGPT
has no equivalent — no way to ask a running daemon which tools are live, which
MCP server contributed each one, or which entries failed to mount. Once scopes
carry labels (§3), `localgpt daemon inspect` is nearly free, and it is the thing
that makes an unapply bug findable.

---

## 5. Suggested staging

Each stage is independently useful and independently shippable.

1. **`Scope` + `ToolRegistry`** in `localgpt-core`. Keep `extend_tools` delegating
   to it so nothing breaks. No behavior change; this is the enabling refactor.
2. **Per-server MCP mount/unmount** (§4.2). First real capability: `localgpt tool
   add` applies live. Deletes four "Restart the daemon" messages.
3. **The reconciler** (§4.3), subscribed to the existing `ConfigWatcher`. Closes
   the half of `9-hot-reload` that was never built, and makes SIGHUP meaningful.
4. **Async `shutdown()` on `DockerSandbox` / `BrowserTool`** (§4.5), with `Drop`
   demoted to a warning path.
5. **`daemon inspect`** (§4.6).
6. Extend the reconciler to cron, hooks, and bridges.

What I would *not* do: introduce string-keyed DI with `inject` lists, a
Cordis-style event bus with four dispatch modes, or any form of native dynamic
loading. The value here is the lifecycle discipline, and it is available without
any of those.

---

## 6. Portable rules for any Rust codebase

The seven that survive translation, independent of LocalGPT:

1. **A registration API returns its own inverse.** If `register()` returns `()`,
   the subsystem it feeds will need a process restart eventually. Make it return
   a guard, or take a scope that collects the undo.
2. **Async teardown does not fit `Drop`.** Use an explicit `async fn shutdown()`;
   keep `Drop` for best-effort release plus a `warn!` that catches the missing
   call. Never block in `Drop` inside an async runtime.
3. **Dispose must reach quiescence.** Signal *and join*. A dropped `JoinHandle`
   detaches; a `JoinSet` abort skips cleanup. Both leave orphans.
4. **Reverse registration order, sequential within a scope.** Register the
   connection before the things that use it, and reverse order gets the teardown
   sequence right for free.
5. **Missing dependency is a resting state, not an error.** Park with a reason
   and retry on the next reconcile. A failure that is only recoverable by restart
   is a design gap, not an edge case.
6. **Give config entries stable ids and reconcile by diff.** Identity derived
   from position or regenerated per read turns every reconcile into a restart.
7. **Validate before you tear down.** Reject bad config while the old instance is
   still serving. Never unload into a failed load.

Rule 1 is the one that pays for the rest. The other six are what you need once
you have it.
