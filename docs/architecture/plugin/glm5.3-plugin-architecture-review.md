# Plugin Architecture Review: Applying deepseek-harness (Cordis) Ideas to LocalGPT

**Date:** 2026-08-16
**Purpose:** Review how `external/deepseek-harness` applies and unapplies plugins cleanly without restart, map those ideas onto LocalGPT's Rust codebase, and compare adoption options with pros and cons.

## Overview

This document covers three things:

1. A technical review of the plugin mechanism in `external/deepseek-harness` — specifically how it achieves apply/unapply without restart.
2. An inventory of LocalGPT's existing seams and gaps, mapped against that mechanism (all references verified against the current tree).
3. A pros/cons analysis of every adoption option, from minimal wiring to a full plugin manager, plus the patterns that generalize to any Rust codebase.

**TL;DR:** deepseek-harness's restart-free plugin system rests on one core discipline — *every registration is a reversible effect that returns its own undo action, and every plugin application owns an undo log that unwinds in reverse order on removal*. LocalGPT already has most of the raw ingredients (a watch-channel config watcher nobody subscribes to, `Send + Sync` tool trait objects, in-place provider swapping via `set_model`, serialized turns via `TurnGate`) but lacks the reversible-registration discipline and any unapply path. The recommended sequence is **B → A → C** (reversible tool registry → wire the hot-reload loop → plugin manager), with D as an immediate bugfix; see §3.

---

## 1. What deepseek-harness Does

### 1.1 Correcting the premise

Two facts reframe the review:

- **It is TypeScript, not Rust.** deepseek-harness (`dsh`) is a pnpm monorepo (~150 workspace packages) with vendored dependencies.
- **The apply/unapply machinery is not harness code.** It is **Cordis**, a plugin framework vendored at `vendor/cordis/`, described in the paper "A Programming Paradigm for Spatiotemporal Composability" (`external/deepseek-harness/README.md:5-7`). The harness is the flagship consumer; its docs state the philosophy directly: "There is no privileged core to patch: you extend dsh by mounting a plugin beside the others, and registrations are effects that unwind when their plugin unloads" (`docs/architecture.md:11-13`).

So what we are reviewing is Cordis's design, and the question is which parts of that design survive the port to Rust.

### 1.2 The five ideas

Summarized from `external/deepseek-harness/docs/cordis-primer.md:7-13`:

1. A **plugin** is an object/function implementing a service: `apply(ctx, config)`.
2. The **context** (`ctx`) is a repository of services, scoped per plugin.
3. Plugins declare dependencies by name (`inject: ['llm', 'tools']`) instead of manual ordering.
4. Communication is via **typed events** (emit / waterfall / parallel / serial).
5. **Registrations are reversible effects** — every contribution goes through `ctx.effect()` / `ctx.on()` and returns a disposer; a registry's `register()` returns the disposer (`AGENTS.md:102`).

Idea 5 is the load-bearing one for restart-free apply/unapply. The others are supporting structure.

### 1.3 The mechanism in detail

#### (a) `ctx.effect()` — reversible registration as the universal primitive

`vendor/cordis/src/fiber.ts:415-561`. An effect runs its body immediately; the body returns cleanup in one of several shapes (`fiber.ts:83-94`): a disposer function, a promise of one, or a sync/async iterable yielding disposers (generator effects register cleanup incrementally — used for long-lived streams). Disposal:

- unwinds in **reverse registration order**,
- awaits async disposers,
- is single-shot, and
- joins an in-flight cleanup started by another caller (`effectInertia`, `fiber.ts:112-117`).

Every registration API in the framework and the harness is a wrapper over this: `ctx.on/once` → `EventsService.register` (`vendor/cordis/src/events.ts:254-260`); `ctx.provide` → `ReflectService.provide` (`vendor/cordis/src/reflect.ts:277-305`); the harness's `tools.register()` "returns the exact disposer" via `this.layers.effect(...)` (`packages/core/tools/src/index.ts:1035-1060`).

#### (b) Fiber — one plugin application with an undo log

A **fiber** is one application of a plugin (a plugin can be applied N times concurrently; `runtime.fibers` is a `DisposableList`, `vendor/cordis/src/registry.ts:316-326`). Each fiber owns `_disposables = new DisposableList()` (`fiber.ts:203`), with lifecycle states `PENDING → LOADING → ACTIVE → FAILED / DISPOSED / UNLOADING` (`fiber.ts:147`).

Two details do the heavy lifting:

- **`DisposableList`** (`vendor/cordis/src/utils.ts:5-40`): an ordered collection with O(1) delete-by-value whose `clear()` returns values **reversed** — pop-the-undo-log semantics. `Fiber._unload()` (`fiber.ts:675-696`) clears it, runs every disposer with **per-disposer error containment** (one failing disposer cannot starve the rest), then rechecks its dependency epoch and may immediately reload.
- **A fiber's dispose is itself an effect registered on its parent fiber** (`fiber.ts:265-297`). Disposing a parent cascades through children automatically, children-first, and awaits `inertia` (any in-flight load/unload transition) until quiescent.

#### (c) `inject` + epochs — dependency reactivity without manual ordering

A fiber declaring `inject: ['llm', 'tools']` stays `PENDING` until every named service has an ACTIVE provider. Each fiber computes an **epoch string** — essentially a fingerprint of which fiber instances currently satisfy its dependencies, or `INACTIVE` if any is missing (`fiber.ts:597-639`). When a service appears or disappears, `ReflectService.notify` walks every fiber that injects the changed name and refreshes its epoch (`vendor/cordis/src/reflect.ts:314-336`).

Net effect: unload a provider → all its consumers automatically unload (park) → a replacement providing the same key revives them. No restart, no manual init/teardown sequencing. Teardown order is also dependency-aware: `provide()`'s disposer deletes the store entry, notifies dependents, and **awaits all their reconfigurations** (`Promise.allSettled`) before removing the provider's own access (`reflect.ts:297-303`).

#### (d) Scoped contexts — plugins never mutate their parent

`ctx.extend()` / `ctx.isolate()` / `ctx.intercept()` create prototypal child contexts (`vendor/cordis/src/context.ts:99-145`); a fiber's context is always a child (`fiber.ts:236`). Service isolation realms allow two providers of the same key to coexist in different scopes (`reflect.ts:277-293`, `vendor/loader/src/config/isolate.ts:26-173`). The harness uses this for per-session agent subtrees that "unwind with the agent" (`packages/preset/agent-presets/src/mount.ts:332-355`), with a leak guard refusing presets that publish services into the root realm (`mount.ts:189-203`).

#### (e) Transactional updates with rollback

`Entry.update()` (`vendor/loader/src/config/entry.ts:142-246`) diffs the new entry options against current state and branches:

- disable → dispose the fiber;
- config-only change → `fiber.update()` (an `internal/update` waterfall that handlers may veto, `fiber.ts:736-753`);
- name/inject change → dispose old, import + start new, and **on failure restart the previous plugin**, wrapping both errors in an `AggregateError` with a labeled stage (`entry.ts:24-27, 232-243`).

HMR goes further: back up the module caches, re-import, swap registrations; on any failure restore the caches and re-register the old plugins (`vendor/hmr/src/index.ts:461-545`). Failed config refreshes keep the last-good tree running (`hmr` lines 297-324). Failed applies never leave a fiber half-mounted — the error is recorded and the fiber forced inactive (`fiber.ts:646-673`).

#### (f) Containment as a codified rule

Unload failures are contained per-disposer; observer exceptions are isolated (`fiber.ts:120-137`); the harness writes this down as rules: "Dispose must reach quiescence, not just request it" and "Contain callback exceptions in the dispatcher" (`docs/defensive-patterns.md:19-25`).

### 1.4 What the harness builds on top

- **Dynamic in-process plugins**: the agent defines/runs/stops plugins at runtime via `cordis_define`/`cordis_run`/`cordis_stop` tools (`packages/extensions/tool-cordis/src/index.ts:148-379`); stopping is "an ordinary awaited `fiber.dispose()`" (`packages/extensions/cordis-host-runner/src/lifecycle.ts:1-8`).
- **Sandboxing**: untrusted dynamic code gets a whitelist facade context exposing only `CTX_VERBS` (`effect`, `on`, `once`, `provide`, `timeout`, ...) plus declared injected services, with cross-realm JSON cloning (`packages/extensions/cordis-host-runner/src/guard.ts:636-781`). Since even the facade's verbs are effects, teardown still unwinds everything the plugin did.
- **Testing discipline**: real-tree tests instead of mocks ("stop/undefine unwind both halves. Only the model and the browser are stand-ins", `packages/extensions/cordis-host-runner/tests/runner.spec.ts:1-10`), plus rollback/versioning tests asserting "keeps currentPackageId when an update fails" (`tests/versioning.spec.ts:6-36`).

---

## 2. LocalGPT Today: Seams and Gaps

Everything in this section was verified against the current tree.

### 2.1 Concept mapping

| Cordis concept | LocalGPT analog | State |
|---|---|---|
| Plugin (bundle of registrations) | MCP server (`localgpt plugin` CLI) | Config rows; apply = connect, but no unapply |
| Registry of services | `Vec<Box<dyn Tool>>` on `Agent` | Frozen at construction; append-only `extend_tools` |
| Reversible effect / disposer | — | **Absent.** Nothing returns an undo handle |
| Fiber (one application + undo log) | Agent construction (`Agent::new`) | All-or-nothing; no per-piece teardown |
| Config-driven reconfiguration (HMR) | `ConfigWatcher` | Built, started, **unused** |
| Typed events | `HookEngine` | Defined, **never wired** |
| In-place subsystem swap | `Agent::set_model` | Exists (provider only) |
| Scoped per-session subtrees | `BridgeManager`, `Scope`-like session maps | Attach/detach works at process granularity |
| Dependency reactivity (`inject`/epochs) | — | Absent (not clearly needed) |

### 2.2 Verified inventory

**Dormant infrastructure:**

- `ConfigWatcher` (`crates/core/src/config/watcher.rs:20-147`) is complete: notify-crate file watching with debounce, an `Arc<RwLock<Config>>` latest-value store, a `tokio::sync::watch` broadcast channel, and a SIGHUP handler (`watcher.rs:153`). The daemon **starts it, prints "Config hot-reload: enabled", then passes it to `run_daemon_services` as the explicitly unused `_config_watcher`** (`crates/cli/src/cli/daemon.rs:150-186`) with the comment "most services currently use the config passed at startup" (`daemon.rs:194-196`). Nobody subscribes.
- `HookEngine` (`crates/core/src/hooks/runner.rs:32`) discovers `*.hook.json` shell hooks with before/after-tool-call events that can `Allow`/`Block` — but no call site outside the hooks module fires them. `Agent::execute_tool` (`crates/core/src/agent/mod.rs:1348-1393`) and `chat` never consult it.
- `AgentActor` (`crates/core/src/concurrency/actor.rs`) has a `SetModel` message (defined `actor.rs:89`, handled `actor.rs:452`) but no tool/plugin messages, and `spawn_supervised`'s `Restart` is a TODO (`actor.rs:540-543`).

**Frozen-at-construction seams:**

- `Agent` holds `tools: Vec<Box<dyn Tool>>` and `app_config: Config` (cloned) — the tool set and config are pinned for the agent's lifetime (`crates/core/src/agent/mod.rs:99-126`). `extend_tools` is append-only; there is no remove or replace (`mod.rs:491-493`).
- MCP is all-or-nothing: `McpManager::connect_all` runs inside `Agent::new` and the manager handle is **immediately dropped** — only the discovered tools survive (`mod.rs:278-292`). There is no per-server disconnect path at all, and no session shutdown of MCP connections.
- `Tool` is `pub trait Tool: Send + Sync` (`crates/core/src/agent/tools/mod.rs:70`) with a permission model (`PermissionLevel` Safe/Elevated/Admin + `ApprovalGate`, checked in `execute_tool`). **`Send + Sync` matters**: `Arc<dyn Tool>` sharing across agents is already viable without touching the trait.
- Slash commands are a compile-time `const` array (`crates/core/src/commands.rs:39`).

**What already works dynamically (precedent to build on):**

- `Agent::set_model` hot-swaps the provider in place (`mod.rs:533-539`) — proof that in-place subsystem swap fits the codebase.
- `TurnGate` (`crates/core/src/concurrency/turn_gate.rs:14`) serializes agent turns across heartbeat/HTTP — a natural, already-existing safe point for swapping tool sets between turns.
- Fresh `Agent` per unit of work is the dominant pattern: HTTP sessions build a new agent on create/resume (`crates/server/src/http.rs:528-537`, `AppState` at `http.rs:74`), heartbeat builds one per run (`crates/core/src/heartbeat/runner.rs:484-493`), same for cron and subagents. **Most config changes therefore already propagate without restart — to new agents.**
- `BridgeManager` tracks bridge daemons in `Arc<RwLock<HashMap>>` with health checks; bridges attach/detach at runtime (`crates/server/src/security/bridge.rs:96-105`) — the closest existing analog to hot apply/unapply, at process granularity.
- The memory file watcher re-indexes changed `.md` files on the fly (`crates/core/src/memory/watcher.rs:23-120`) — the one genuinely working hot-reload loop.
- Skills are re-read from disk when building prompts — effectively already hot-reloadable.

**Known bug adjacent to this area:**

- `POST /api/config` → `set_config` (`crates/server/src/http.rs:1390-1428`) rewrites `config.toml` on disk but in-memory `AppState.config` stays stale until restart. Ironically, the dormant `ConfigWatcher` would fix this for free once subscribed to.

**Constraints any design must respect:**

- Core portability: `localgpt-core` must compile for iOS/Android; no platform-specific deps in core (`CLAUDE.md:130-134`).
- Security model: signed policy verification and an audit chain happen at agent start (`mod.rs:299-387`, `crates/core/src/security/`). Plugin apply/unapply events should be audited the same way.
- `Agent` is not `Sync` (SQLite); sharing goes through `AgentHandle = Arc<tokio::sync::Mutex<Agent>>` (`mod.rs:2290-2380`).

### 2.3 What this means

LocalGPT does not need Cordis. It has ~15 construction seams, one hot-swap precedent, and a dormant notification bus. The gap is precisely the Cordis discipline: **no registration returns an undo handle, so nothing can be unapplied** — and therefore nothing long-lived can be reconfigured. The options below differ in how much of that discipline they adopt.

---

## 3. Options Analysis

### Option A — Wire the hot-reload loop

Make the daemon actually subscribe to `ConfigWatcher`; diff old-vs-new config (MCP servers enabled/disabled/changed, tool config) and apply the delta transactionally: connect new servers, disconnect removed ones, swap tool sets — with rollback to last-good on failure and per-server error containment. Long-lived agents pick up the swap between turns.

- **Pros:**
  - Uses infrastructure that already exists and is tested-in-shape (watcher, SIGHUP, `localgpt plugin` CLI write side, `POST /api/config`).
  - Immediately user-visible: MCP servers and tools apply/unapply without restart; fixes the stale-`AppState.config` bug as a side effect.
  - Introduces the two most valuable Cordis behaviors — transactional apply with rollback (§1.3e) and per-step error containment (§1.3f) — without any new abstraction layer.
  - Natural fit with the fresh-agent-per-task pattern: most consumers get correctness for free.
- **Cons:**
  - Doesn't help long-lived agents (HTTP sessions idle up to 30 min, TUI/desktop for the whole process) without a swap seam inside `Agent` — which is Option B's deliverable.
  - Requires refactoring MCP lifecycle: `McpManager` must become daemon-owned and long-lived with per-server connect/disconnect, instead of connect-and-drop inside every `Agent::new`. This is the bulk of the work and touches the hottest constructor in the codebase.
  - "Diff two TOML configs correctly" is easy to get subtly wrong (renames vs remove+add); needs the rollback harness from day one.
- **Effort:** M. **Risk:** medium (MCP constructor refactor). **Depends on:** B for the in-place swap of long-lived agents; valuable even alone for new agents.

### Option B — Reversible tool registry in core

Introduce a `ToolRegistry` — `Arc<RwLock<Vec<Arc<dyn Tool>>>>` (or an insertion-ordered map) — where `register()` returns a disposer/guard, plus `unregister(name)` and snapshot-based reads at turn start. `extend_tools` migrates to registration and returns the guard (old signature kept as a deprecated wrapper). This is the Cordis "registrations are effects" idea (§1.3a) sized for Rust: RAII guard ≈ disposer; a `DisposableList`-style undo log (reverse-order unwind, per-entry error containment, single-shot, async dispose awaited to quiescence) as a small utility next to it.

- **Pros:**
  - The right primitive: every other option (A's swap, C's unapply, even a future hooks wiring) is built on it. Small, well-testable blast radius (one new module + `Agent` internals).
  - `Tool: Send + Sync` is already satisfied (`tools/mod.rs:70`), so `Arc<dyn Tool>` sharing is a mechanical change, not a trait redesign.
  - `set_model` proves in-place swap fits `Agent`; `TurnGate` proves there's a safe swap point between turns.
  - The undo-log utility is exactly the kind of thing that generalizes to other registries (slash commands, hooks, prompt sections later).
- **Cons:**
  - No user-visible behavior by itself — it's an enabler, so it must land with or be immediately followed by A to demonstrate value.
  - Touches every `self.tools` access in `Agent` (schema building, name lookup, execution) — a wide but shallow migration.
  - Async teardown needs care: Rust has no async `Drop`, so the registry needs an explicit `async fn dispose()` awaited to quiescence rather than relying on guards alone; mixing RAII (sync) and async disposal is a real design decision to get right (see §3.7).
  - Must preserve the safe/dangerous split (`create_safe_tools` in core at `tools/mod.rs:92`, dangerous tools injected later via `extend_tools`/`ToolFactory`) and the `PermissionLevel` + `ApprovalGate` checks.
- **Effort:** S–M. **Risk:** low-medium (mechanical but wide). **Depends on:** nothing.

### Option C — Plugin manager MVP

Named bundles — a plugin = a set of tools (later: hooks, prompt sections, slash commands) — with `apply`/`unapply` semantics: applying mounts a bundle's registrations into a scoped undo log; unapplying unwinds them in reverse with containment; failed apply rolls back fully. This is the closest analog to deepseek-harness's product surface, and the `localgpt plugin` CLI (`crates/cli/src/main.rs:99` aliases `Tool | Plugin`, `crates/cli/src/cli/tool.rs:13-49` manages MCP servers via config) already exists as the write-side.

- **Pros:**
  - Best long-term shape: matches how the product already talks about plugins; per-session scoping (spawned subagents mounting scoped bundles that unwind with the session) maps directly to Cordis's `Scope` (§1.4).
  - Unapply is a first-class product feature, not a refactor byproduct: `plugin remove` actually removes tools from running sessions.
  - Audit story composes cleanly: apply/unapply as `AuditAction`s in the existing chain.
- **Cons:**
  - Largest change; hard to land incrementally without B (registry) and most valuable with A (trigger).
  - Risk of overbuilding: deepseek-harness needs full Cordis because *everything* is a plugin in one long-lived process; LocalGPT's fresh-agent-per-task pattern already delivers most of "plugins without restart" for free to new agents. The remaining pain (long-lived sessions) is narrower than a full plugin manager.
  - The dependency-reactivity part of Cordis (`inject` + epochs, §1.3c) has no natural consumer here yet — LocalGPT's dependency graph is shallow and static. Adopting it now would be ceremony without payoff.
- **Effort:** L. **Risk:** medium-high (scope creep). **Depends on:** B (foundation), benefits from A (trigger).

### Option D — Minimal wiring (bugfix-grade)

Subscribe `AppState`'s session construction to `config_watcher.config()` instead of the startup snapshot, and make `POST /api/config` refresh in-memory state (or just let the watcher do it). Optionally fire the already-defined `HookEngine` events in `execute_tool`.

- **Pros:** tiny; fixes a real bug (`http.rs:1390-1428` stale config); zero new abstractions; makes the "Config hot-reload: enabled" startup line honest.
- **Cons:** no unapply semantics at all; doesn't survive the long-lived TUI/desktop process; MCP server changes still require new sessions and re-run `connect_all`; hooks wiring alone doesn't compose with anything yet.
- **Effort:** S. **Risk:** low. **Depends on:** nothing. Worth doing regardless.

### Option E — Full Cordis-style service container (analyzed, rejected for now)

A general service registry: `provide(name, Arc<dyn Any>)` behind `RwLock`, `inject`-declared dependencies, epoch-based parking/reloading of consumers when providers come and go (§1.3c), scoped context objects handed to every subsystem.

- **Pros:** the actual mechanism that makes deepseek-harness composable; would eventually absorb providers (`LLMProvider` trait at `crates/core/src/agent/providers.rs:348` has 11 impls + `FailoverProvider`), memory backends, STT, and tools uniformly.
- **Cons / why rejected:**
  - Cordis's context is a **Proxy whose property reads resolve services dynamically** — that ergonomics does not port to Rust; you'd hand explicit context handles everywhere, which fights the existing constructor-composed style for little gain at current scale.
  - Epoch-based reactivity pays off when plugins come and go *frequently and interdependently*; LocalGPT's set of subsystems is small, shallowly coupled, and mostly re-read per fresh agent.
  - The `AgentActor` supervision TODO (`actor.rs:540-543`) suggests process/actor-level restart is the intended local remedy for misbehaving subsystems — a simpler tool than dependency epochs.
  - Cost is front-loaded and certain; benefit is speculative. Revisit if C lands and plugin counts grow.

### Rust-specific challenges (common to A/B/C)

1. **No async `Drop`.** Cordis disposal awaits async disposers to quiescence. In Rust, teardown must be an explicit `async fn dispose()` (or `JoinHandle` join), called by the owner — guards can only handle sync cleanup. Design every registration API to return something with an explicit async dispose, and reserve `Drop` for leak-prevention backstops (log-and-drop).
2. **Containment, not propagation.** Mirror `fiber.ts:675-696`: each disposer runs in its own error boundary (`futures::future::catch_unwind`-style or `Result` + log), so one failing cleanup cannot starve the rest, and a failed apply never leaves a half-mounted state.
3. **Mid-turn safety.** `TurnGate` serializes turns; swap registries/config snapshots at turn boundaries, never mid-turn. Long-lived agents re-snapshot `Arc<RwLock<...>>` reads at turn start (cheap clone of an `Arc` list).
4. **Core portability.** The registry lives in `localgpt-core` → pure Rust only; MCP transport specifics stay where they are; mobile-ffi (`AgentHandle`) keeps working unchanged.
5. **Audit.** Apply/unapply are security-relevant state changes → `append_audit_entry` actions, consistent with policy verification at `mod.rs:299-387`.

### Comparison

| | A: hot-reload loop | B: reversible registry | C: plugin manager | D: minimal wiring | E: service container |
|---|---|---|---|---|---|
| User-visible value | High | None (enabler) | High | Low (bugfix) | Indirect |
| Effort | M | S–M | L | S | XL |
| Risk | Medium | Low-medium | Medium-high | Low | High |
| Introduces Cordis idea | §1.3e, §1.3f | §1.3a, §1.3b | §1.3a–d | — | all |
| Depends on | B (for full effect) | — | B, A helps | — | — |

### Recommended sequence

1. **D first** as an immediate bugfix (honest config hot-reload for new sessions).
2. **B next** — the reversible registry + undo-log utility, landed with tests modeled on deepseek-harness's rollback tests ("apply fails ⇒ registry unchanged", "unapply unwinds in reverse", "one bad disposer doesn't starve the rest").
3. **A then** — daemon-owned `McpManager` + watcher subscription + transactional diff/apply with rollback; this is where "plugins apply and unapply cleanly without restart" becomes true in the product.
4. **C only when demand shows** — if users actually want runtime tool bundles beyond MCP, build the manager on B/A. Keep E out unless plugin count and interdependency grow.

---

## 4. Patterns for Rust Codebases in General

Independent of LocalGPT, the review distills into portable guidance:

**Adopt directly:**

- **Disposer-returning registration APIs.** Any `register_*` should return an undo handle (`impl Drop` guard or an explicit `async fn dispose()` owner). The signature *is* the architecture: it makes "unapply" always expressible.
- **Reverse-order undo logs with per-entry containment.** A `DisposableList` equivalent — `Vec<Box<dyn FnOnce()>>` (or boxed futures for async), popped in reverse, each wrapped in an error boundary, single-shot, and joinable to quiescence. This is the scoped-guard/`defer` pattern generalized to registries, and it is what makes teardown *complete* rather than best-effort.
- **Transactional config apply.** Diff → apply → on failure roll back to last-good and keep the previous state serving; aggregate errors with stage labels instead of surfacing half-failures.
- **Hot-swap seams.** Subsystems intended to change at runtime live behind `Arc<RwLock<...>>` and are re-snapshotted at well-defined boundaries (a turn, a request, a loop iteration) rather than held across them.
- **Quiescence as a testable property.** "Dispose must reach quiescence, not just request it" (`docs/defensive-patterns.md:19-21`): await/join teardown in tests; assert the world is restored.

**Adapt with care:**

- Generator-style incremental cleanup (yielding disposers as a stream) → in Rust, either a builder that collects disposers eagerly or an explicit staged `async fn` with early-return rollback.
- Scoped child contexts → explicit context-handle structs passed down; Rust has no dynamic scoping, which is a feature: the dependency becomes visible in signatures.

**Do not port:**

- Proxy-based context/service resolution — replace with typed handles.
- Epoch-based dependency reactivity — only worth it when providers and consumers churn interdependently at runtime; most Rust services have static dependency graphs.
- Anything relying on synchronous GC-less teardown or runtime code swap (HMR) — the module-cache backup/restore trick (`vendor/hmr/src/index.ts:461-545`) is a JS-runtime capability; the Rust analog is `Arc` swap behind a seam, not code reload.

---

## 5. References

**deepseek-harness** (under `external/deepseek-harness/`):

- `README.md`, `AGENTS.md`, `docs/cordis-primer.md`, `docs/architecture.md`, `docs/defensive-patterns.md`
- `vendor/cordis/src/fiber.ts` (effects, fiber lifecycle, epochs), `vendor/cordis/src/reflect.ts` (service registry + notify), `vendor/cordis/src/registry.ts` (plugin shapes), `vendor/cordis/src/utils.ts` (DisposableList), `vendor/cordis/src/events.ts`, `vendor/cordis/src/context.ts`, `vendor/cordis/src/service.ts`
- `vendor/loader/src/config/entry.ts` (transactional update + rollback), `vendor/loader/src/config/tree.ts`, `vendor/loader/src/config/isolate.ts`
- `vendor/hmr/src/index.ts` (hot swap with module-cache rollback)
- `packages/core/tools/src/index.ts` (disposer-returning tool registry), `packages/core/scope/src/index.ts` (per-session scopes), `packages/preset/agent-presets/src/mount.ts` (leak guard), `packages/extensions/cordis-host-runner/` (dynamic plugins, sandbox facade, rollback/versioning tests), `packages/extensions/tool-cordis/src/index.ts`

**LocalGPT:**

- `CLAUDE.md` (workspace rules, portability constraint at lines 130-134), `AGENTS.md` (tool catalog, security model)
- `crates/core/src/agent/mod.rs` (Agent struct 99-126, `new` 237-292 incl. MCP connect-and-drop, `extend_tools` 491-493, `set_model` 533-539, `execute_tool` 1348-1393, `AgentHandle` 2290-2380)
- `crates/core/src/agent/tools/mod.rs` (`Tool` trait 70, `create_safe_tools` 92), `crates/core/src/agent/providers.rs` (`LLMProvider` 348)
- `crates/core/src/config/watcher.rs`, `crates/cli/src/cli/daemon.rs` (150-196: watcher started then unused)
- `crates/core/src/hooks/` (`HookEngine` runner.rs:32, events event.rs:9-36), `crates/core/src/concurrency/` (`TurnGate` turn_gate.rs:14, `AgentActor` actor.rs incl. SetModel 89/452 and supervision TODO 540-543)
- `crates/server/src/http.rs` (`AppState` 74, session agents 528-537, `set_config` 1390-1428), `crates/server/src/security/bridge.rs` (96-105)
- `crates/core/src/memory/watcher.rs` (working hot-reload precedent), `crates/cli/src/cli/tool.rs` (plugin CLI), `crates/cli/src/main.rs:99` (`Tool | Plugin` alias)
