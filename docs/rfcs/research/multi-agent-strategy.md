# Multi-agent before multi-user: a LocalGPT architecture strategy

**Status:** research, 2026-03-25. A landscape survey, not a proposal — kept for the reasoning behind decisions already made. It informed the decision to ship hierarchical subagent spawning before multi-user. Implemented: `spawn_agent` is in core's safe tool set.


**LocalGPT should prioritize multi-agent orchestration over multi-user support, and it can do so while staying under 20MB.** The competitive landscape shows that single-level hierarchical subagent spawning (the pattern Claude Code uses) delivers the highest developer productivity gains, while multi-user features primarily serve enterprise sales motions that are premature for a single-binary local tool. Rust's actor ecosystem—particularly Kameo or raw Tokio channels—adds only **200–400KB** to binary size while enabling full agent orchestration. The recommended path: ship lightweight subagent spawning in the next release cycle, defer multi-user to a later enterprise-oriented milestone, and use MCP as the primary extensibility layer rather than building custom agent-to-agent protocols.

This analysis draws on architectural patterns from OpenClaw's gateway system, Claude Code's subagent model, seven major multi-agent frameworks, five AI coding tools, and Rust's actor framework ecosystem to build a concrete implementation roadmap.

---

## How OpenClaw and Claude Code actually handle agents

OpenClaw and Claude Code represent two fundamentally different multi-agent philosophies, and understanding both is critical for LocalGPT's design choices.

**OpenClaw uses a hub-and-spoke gateway model.** A single-process Gateway server runs on WebSocket (`ws://127.0.0.1:18789`) and owns all messaging surfaces (WhatsApp, Telegram, Slack, Discord, etc.). Each agent is a fully isolated persona with its own workspace, session store, auth profiles, and skills directory. Routing is declarative—config bindings in `~/.openclaw/openclaw.json` map channels, peers, and accounts to specific agents using a deterministic priority system (peer match > guild > team > account > fallback). The Lane Queue enforces **serial execution by default** to prevent race conditions, with explicit parallel lanes only for idempotent tasks. Agent-to-agent communication is off by default and requires an explicit allowlist. This is a peer model: agents don't delegate to each other hierarchically—they're independent personas behind one gateway.

**Claude Code uses single-level hierarchical delegation.** A main agent spawns subagents via a built-in `Task` tool, with each subagent operating in its own isolated context window. The critical constraint: **subagents cannot spawn other subagents**, preventing infinite nesting. Three built-in subagent types exist: general-purpose (full context inheritance), Plan (architect-mode), and Explore (read-only, fresh context). Custom agents are defined as Markdown files with YAML frontmatter in `.claude/agents/`. Communication flows through `TaskUpdate` (incremental progress), `TaskOutputTool` (final results with metrics), and async wake messages. Subagents run in the background by default, and lifecycle hooks (`SubagentStart`, `SubagentStop`) enable customization.

The key architectural lesson for LocalGPT: **OpenClaw's model solves multi-persona routing (different bots for different channels), while Claude Code's model solves task decomposition (breaking complex coding work into parallel subtasks).** For a developer tool, the Claude Code pattern is far more relevant. LocalGPT doesn't need to route between messaging platforms—it needs to let a lead agent delegate file exploration, code review, and implementation to specialist subagents.

---

## The industry has converged on six orchestration patterns

Research across AutoGPT, CrewAI, LangGraph, Microsoft AutoGen, OpenAI Agents SDK, and Google's A2A protocol reveals six distinct multi-agent patterns, each with different trade-offs for a lightweight local tool:

**Supervisor/Worker** (used by Amazon Bedrock, Claude Code) places a central orchestrator that decomposes tasks, delegates to workers, and synthesizes results. This is the most transparent and auditable pattern, but creates a central bottleneck. **Handoff/Delegation** (OpenAI Agents SDK) transfers active conversations between specialized agents via explicit `transfer_to_XXX` functions—lightweight and debuggable but sequential only. **Graph-based workflows** (LangGraph) model agents as nodes in a directed graph with conditional routing, enabling cycles and parallel branches, but require significant setup overhead. **Role-based crews** (CrewAI) assign organizational roles with built-in memory, using a team metaphor that's intuitive but rigid. **Conversational multi-agent** (Microsoft AutoGen) uses LLM-mediated chat between agents—flexible but stochastic. **Protocol-based interoperability** (Google A2A + MCP) standardizes cross-vendor agent communication via JSON-RPC.

The most important signal from the industry: **Cognition (makers of Devin) explicitly argues against multi-agent parallelism.** Their June 2025 blog post states that "running multiple agents in collaboration only results in fragile systems" because decision-making becomes too dispersed and context cannot be shared effectively. Claude Code's subagents are deliberately simple—subagents typically answer questions rather than write code, because parallel code-writing creates conflicting decisions. Cursor 2.0 takes the opposite approach with up to **8 parallel agents** using Git worktree isolation, but these agents don't communicate with each other—they're independent workers.

For LocalGPT, this means the minimum viable multi-agent system should start with single-level supervisor/worker delegation (Claude Code's proven pattern) rather than attempting full peer-to-peer agent swarms. The evidence strongly favors constrained hierarchies over open collaboration.

---

## Multi-user is an enterprise feature, not a developer feature

Analysis of multi-user patterns across AnythingLLM, Cursor, GitHub Copilot, Claude Code, and Zed reveals a clear stratification of when multi-user support matters:

**Individual developers don't need multi-user in a local tool.** Claude Code operates as a per-user CLI instance—each developer runs their own copy, and team collaboration happens through Git and shared `CLAUDE.md` configuration files. This per-instance model works because AI coding assistants are inherently personal: each developer's context, preferences, and active tasks are different. AnythingLLM explicitly separates its desktop version (single-user only) from its Docker deployment (multi-user capable), recognizing that the local use case is fundamentally single-user.

**Multi-user becomes essential for three enterprise scenarios**: shared infrastructure (a team running one LocalGPT instance on a shared dev server), admin control (IT managing configurations and permissions across developers), and cost management (centralized billing and usage caps). The minimum viable enterprise feature set observed across all major tools includes SSO (SAML/OIDC), RBAC with at least two roles, workspace isolation, centralized billing, and audit logs.

The most effective architectural pattern for local-first multi-user is **per-tenant SQLite databases** combined with workspace-scoped isolation. Each workspace gets its own SQLite file, providing true data isolation with trivial backup/deletion. AnythingLLM demonstrates this works well in practice. The workspace—not the user—should be the primary security boundary for a lightweight tool.

For real-time collaboration (if ever needed), **CRDTs** are the proven approach—Zed's editor uses them with Lamport clocks and unique logical locators—but this adds significant complexity and is unnecessary for AI coding tools where Git-based async collaboration suffices.

---

## Why multi-agent should come first for LocalGPT

The strategic case for prioritizing multi-agent over multi-user rests on four arguments:

**1. Target audience alignment.** LocalGPT targets developers who use OpenClaw/Claude Code. These users are individual practitioners who run the tool locally. They don't need multi-user—they need their tool to handle complex tasks better. Multi-agent directly improves task quality by enabling parallel file exploration, plan-then-execute workflows, and specialist delegation.

**2. Competitive necessity.** Claude Code already ships single-level subagent spawning, Cursor offers 8 parallel background agents, and GitHub Copilot is building AgentHQ for multi-agent coordination. If LocalGPT cannot decompose complex tasks into subtasks, it will hit a capability ceiling on any coding problem requiring coordination across multiple files or concerns. Multi-user, by contrast, is not a competitive differentiator—it's table stakes for enterprise sales that LocalGPT isn't pursuing yet.

**3. Implementation cost asymmetry.** Multi-agent can be implemented with **~200–400KB of additional binary size** using Rust's actor frameworks, primarily leveraging Tokio channels that are already in the dependency tree. Multi-user requires authentication middleware, RBAC enforcement, session management, per-tenant database routing, and potentially SSO integration—a far larger surface area that pulls the project toward server complexity.

**4. Differentiation opportunity.** Rather than matching OpenClaw's multi-agent feature-for-feature, LocalGPT can differentiate by offering multi-agent orchestration in a **single lightweight binary**. No gateway server, no TypeScript runtime, no Docker—just a 20MB binary that spawns subagents internally using Rust actors. This is a unique value proposition no competitor offers.

The recommended prioritization sequence:

- **Phase 1 (now)**: Single-level subagent spawning with Tokio tasks
- **Phase 2 (next quarter)**: MCP server integration for extensible tool use
- **Phase 3 (6+ months)**: Basic multi-user with per-workspace SQLite isolation
- **Phase 4 (enterprise milestone)**: SSO, RBAC, audit logs, managed policies

---

## A concrete Rust architecture that stays under 20MB

The recommended implementation uses a layered approach that adds orchestration capability incrementally without compromising the single-binary advantage.

**Core agent pattern: Tokio tasks with mpsc+oneshot channels.** Rather than pulling in a full actor framework immediately, start with Alice Ryhl's canonical actor pattern using raw Tokio primitives. Each subagent is a `tokio::spawn`ed task with an `mpsc::Receiver<AgentMessage>` for commands and `oneshot::Sender` channels embedded in messages for returning results. This adds zero new dependencies beyond Tokio, which LocalGPT already uses. The pattern looks like:

```
Orchestrator (main agent)
├── mpsc::channel → SubAgent A (Explorer, read-only)
├── mpsc::channel → SubAgent B (Planner)
└── mpsc::channel → SubAgent C (Implementer)
    Each returns results via oneshot::Sender
```

**Graduation path: Kameo for production supervision.** When subagent reliability becomes critical, migrate to the Kameo actor framework (highest overall score in January 2025 benchmarks: **8.98/10**, with bounded+unbounded mailboxes, supervision trees, and panic isolation). Kameo adds only ~200–400KB to binary size and runs on Tokio natively. The key capability gained: **a panic in one subagent cannot crash the entire system**—the supervision tree restarts it automatically.

**LLM abstraction: Rig.** The `rig-core` crate provides a unified interface across LLM providers (OpenAI, Anthropic, local models via mistral-rs/llamacpp) with an agent builder pattern, structured output extraction, and streaming. This lets subagents use different models—an explorer subagent on a fast cheap model, a planner on a reasoning model—without code changes.

**Multi-user data layer (Phase 3): Per-workspace SQLite via SQLx.** When multi-user is needed, implement per-tenant SQLite databases managed through an `Arc<RwLock<HashMap<TenantId, SqlitePool>>>` behind Axum middleware. Each workspace gets its own `.db` file, providing true isolation with trivial backup/deletion. The `axum-tenancy` crate provides basic middleware scaffolding.

**Binary size budget** for the full stack:

| Component | Size estimate |
|---|---|
| Tokio runtime | 2–3 MB |
| Axum + Tower | 1–2 MB |
| Kameo actors | 200–400 KB |
| SQLx + SQLite | 1.5–2 MB |
| Rig LLM client | 1–2 MB |
| Embedded web assets | 1–3 MB |
| Application logic | 500 KB–1 MB |
| **Total (stripped, LTO, opt-level z)** | **~10–15 MB** |

This stays well under LocalGPT's current ~27MB footprint, meaning multi-agent and multi-user can both fit within the existing binary size envelope.

---

## MCP is the highest-leverage investment for extensibility

Rather than building custom agent-to-agent protocols, LocalGPT should invest in **MCP (Model Context Protocol) as its primary extensibility layer**. MCP was donated to the Linux Foundation in December 2025, adopted by OpenAI, Google, and Microsoft, and has become the "USB-C port for AI." It standardizes agent-to-tool communication via JSON-RPC 2.0 over stdio or HTTP.

The strategic insight: **MCP lets LocalGPT's subagents access any tool in the ecosystem without LocalGPT needing to implement those tools natively.** A subagent can call an MCP server for database access, browser automation, or code analysis. This means LocalGPT can offer broad tool capabilities while keeping its binary small—the tools run as separate MCP servers that users install independently.

For agent-to-agent communication across different AI systems (a longer-term need), Google's **A2A protocol** (v0.3, mid-2025, 150+ organizations, Linux Foundation governance) provides the emerging standard. But this is premature for LocalGPT—internal subagent communication via Tokio channels is sufficient until LocalGPT needs to interoperate with external agent systems.

---

## Conclusion: constrained hierarchy, not swarm intelligence

The research points to a clear architectural philosophy for LocalGPT: **build the simplest multi-agent system that solves real developer problems, and resist the temptation to build a general-purpose agent framework.**

Three non-obvious insights emerge from this analysis. First, the most successful AI coding tools deliberately constrain their multi-agent systems—Claude Code's single-level hierarchy and Cursor's non-communicating parallel agents both avoid the context fragmentation problem that Cognition identifies as the fatal flaw of collaborative multi-agent systems. LocalGPT should enforce the same constraint: subagents complete tasks and return results, they don't negotiate with each other.

Second, **the single-binary advantage is itself a competitive moat** that multi-agent and multi-user features threaten to erode. Every dependency, every protocol, every server component makes the binary larger and the deployment story more complex. The architectural discipline should be: can this feature be implemented with Tokio primitives already in the dependency tree? If yes, use those. If no, is the feature important enough to justify the size increase?

Third, the timing question has a clear answer. Multi-agent orchestration is a **capability multiplier** that makes every existing LocalGPT feature more powerful—sandboxing, code editing, file exploration all benefit from being composable into multi-step agent workflows. Multi-user is a **market expansion** feature that opens new customer segments but doesn't improve the core product for existing users. Build the multiplier first.