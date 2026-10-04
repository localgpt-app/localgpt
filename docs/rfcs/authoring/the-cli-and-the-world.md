# What the CLI is for, once the world is the product

**Status:** decided 2026-10-04, partly built. `localgpt world` and
`localgpt world mcp` exist (`crates/cli/src/cli/`, over
`world-agent::headless` and the open app's API); the world-shaped autonomy
below does not.

## The question

LocalGPT's CLI is an OpenClaw-compatible agent harness: eleven providers,
`provider/model` routing, chat, a TUI, a daemon. Against Claude Code and Codex
it has no advantage as a coding agent — and the config says so out loud.
`crates/core/src/config/mod.rs:1056`:

```rust
"claude-cli/opus".to_string()
```

**The default model shells out to Claude Code.** Out of the box, LocalGPT's
answer to "be a coding agent" is to delegate to the agent it would be
competing with. That is not a position to defend, and the honest move is to
stop trying: a better chat loop is not why anyone would choose this.

## What is actually differentiated

Four things in `localgpt-core` are not what Claude Code or Codex do, and none
of them is a chat loop:

| | |
|---|---|
| **Memory** | a markdown workspace indexed by SQLite FTS5 + sqlite-vec; daily logs; *dreaming* (background consolidation of transcripts into memory); active recall before a reply; a wiki of claims and evidence with staleness tracking |
| **Autonomy** | heartbeat and cron, each job a fresh agent session, with a durable SQLite outbox and exponential backoff. No coding agent does scheduled unattended work |
| **Sandbox** | Landlock and Seatbelt — kernel-enforced shell isolation, not a prompt asking nicely |
| **Policy** | POLICY.md, the protected-files deny list, injection sanitization |

And one shape that is already right: `localgpt mcp-server` exposes memory and
web tools over stdio to "Claude CLI, Gemini CLI, Codex, VS Code, Zed". That is
the CLI *serving* other agents instead of being one, and it is the template
for everything below.

## The decision

**The CLI is the world's command line, and the services other agents use.**
Not an agent that competes; the authority a world has when no app holds it,
and the surface through which any agent reaches a world.

### 1. `localgpt world` — the headless authority (built)

`init`, `submit`, `log`, `verify`, `undo`, `history`. A world is a folder, so
its command line is the natural CLI shape for a format: scriptable, CI-able,
no window. The commands live in `world-agent::headless` because `LiveWorld`
is the one authority — a second implementation of commit is the drift this
workspace has paid for before — and a write refuses while an app answers at
the folder's endpoint, because one authority at a time is the rule the whole
live-editing design rests on.

### 2. `localgpt world mcp` — the shim (built)

A stdio MCP server that reads `.live/endpoint.json` and offers the open
world's tools. It makes Claude Code a first-class editor of a live world with
no emulator, no pane and no new protocol, which is the conclusion the terminal
and ACP investigations both arrived at from opposite directions. It has been
in the live-editing POC's *Not done* list since the beginning. Six tools —
submit, undo, log, verify, screenshot, selection — each one HTTP call, with a
refusal handed back verbatim so the agent can correct the batch, and the
world's own `AGENTS.md` named in the submit description so an agent that has
never heard of the format can find the rules from the tool list.

### 3. Point memory, heartbeat and cron at worlds

"Remember what we decided about this world across sessions" and "tend this
world on a schedule" are things no coding agent offers, and they are the two
differentiated assets applied to the thing that is now the product. This is
where a moat exists, if one does.

### 4. Keep the provider layer as plumbing, not as positioning

Eleven providers plus a local GGUF means a world can be authored by whatever
model is on the machine, including none — offline, private, deterministic
first (§13.3's tiers). Worth every line. It is simply not a product claim.

### What to stop

`chat` and `tui` as *product* surfaces. They stay as the development and
debugging path for the agent layer — cheap, and genuinely useful for that —
but they are not a reason to choose LocalGPT and should not be invested in as
though they were. OpenClaw compatibility stays as a convenience of the routing
table, not as a thing the project is.

## Why this follows from the editor vision

The desktop app is the product and an openworldformat package is the document.
That leaves three jobs a GUI cannot do, and each is a CLI job: commit a batch
from a script or a CI run; give an outside agent tools against an open world;
and do unattended work to a world on a timer. None of them is chat.

## Not decided

- Whether `localgpt world` should grow the non-linear commands — `tips`,
  `fork`, `goto` — or whether branching stays the app's. The view that would
  back them exists (`crates/world-editor`), unwired.
- Whether the shim is one `localgpt world mcp` for the open world, or a
  per-world server an agent's own config names.
- Whether memory-about-a-world lives in the agent's workspace or in the
  package, which decides whether it travels with a shared link.
