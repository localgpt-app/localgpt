# Agents outside the window: what gives Gen a prompt back when the harness goes

**Status:** proposal. Assumes the embedded harness in `crates/gen` is removed
in favour of the filesystem-and-API pattern proved in
[live-editing-poc.md](live-editing-poc.md). Evidence from two editors that
already made this choice: `external/orca` (Electron, xterm.js, node-pty) and
`external/zed` (Rust, GPUI, `alacritty_terminal`, and the Agent Client
Protocol). Zed's relevant crates are GPL-3.0-or-later and none of their code
may enter this repository — see [Licence](#licence-read-zed-copy-nothing-adopt-acp).
The greenfield counterpart, where history is the product and the chrome is
rebuilt on Bevy's own widgets, is
[world-editor-stack.md](world-editor-stack.md).

## The problem

The live-editing proof of concept answered "can Gen be the canvas of a world
that agents change from outside?" with yes. It left the next question open,
and it is a product question, not a format one: **with no agent conversation
in the window, where does the person type?**

Today they type into the prompt panel, which feeds `agent_loop.rs` — 1663
lines that own a conversation, slash commands, model switching, streaming and
a tool loop, with `local_llm.rs` (568) behind it and roughly 6.9k lines of
`mcp/*` tool implementations in front. Take the loop away and the canvas is a
window with a world in it and no way to ask for a change except a second
application.

Three answers are available, and the honest version of this RFC is that they
are not alternatives — they are tiers, and the right move is to buy the
cheapest one now and leave the expensive one unbuilt until something demands
it.

## What the harness removal actually subtracts

Worth separating, because "remove the harness" reads as "delete 10k lines" and
it should not:

| | Lines | Fate |
|---|---|---|
| `agent_loop.rs` | 1663 | **goes** — the conversation, the REPL, the slash commands, the streaming |
| `local_llm.rs` | 568 | **goes** — one of the three mistral.rs loaders §13.6 counted |
| `mcp_server.rs` | 251 | **becomes the shim** — same endpoints, now over the live API |
| `mcp/*` + `character_tools.rs` | 7197 | **stays, re-fronted** — the `Tool` impls are shells over `GenBridge` commands; the commands are the world's vocabulary and the ops API is their new front door |
| `desktop/panel.rs`, `desktop/chat.rs` | 1057 | **stays** — see *The panel is already the right shape* |

The loop is the thing being removed. The tools are not the loop: a
`GenSetPhysicsTool` is a `localgpt_core::agent::tools::Tool` wrapper around a
`GenBridge` command, and the wrapper is the part coupled to the harness.
`worldgen_tools.rs` and `terrain_tools.rs` especially are generators — they
would have to be rebuilt the next day if deleted with the loop.

## The three tiers

| | What Gen does | Buys | Cost |
|---|---|---|---|
| **0. Launch** | Opens the user's own terminal in the world folder | The agent's own best UI; `AGENTS.md` and `.live/endpoint.json` are found from the cwd | hours |
| **1. Speak a protocol** | Spawns an agent server over stdio and renders its events as Gen's own panel | Turns, tool calls, plans and refusals as **data** Gen can tie to revisions | days, and the protocol exists (below) |
| **2. Emulate a terminal** | Hosts a real PTY pane — in the Bevy window, or in the browser tab Gen already serves | Interactive approvals, `/commands`, auth flows, the familiar thing | emulation, render and input only — the PTY host already exists |

**Recommendation: 0 now, 1 next, 2 only if Gen becomes where people live.**

## Does Zed solve the terminal problem with a better stack?

Yes to the stack, and — more usefully — no to the premise. Zed is the closest
precedent Gen has: a GPU-rendered Rust application with its own layout and
text stack, exactly Gen's situation, and it ships a terminal good enough that
people use it instead of their own.

**The stack is real and partly reusable.** `crates/terminal` depends on
`alacritty_terminal` and `vte 0.15` with the `ansi` feature. That is the right
boundary: the VT state machine, the grid, scrollback and selection are a
solved problem in Rust and nobody should write them again. Note that Zed
depends on **its own fork** of `alacritty_terminal`, pinned by git rev — the
emulator core was not quite enough off the shelf even for them.

**The part you cannot reuse is the part you would be buying.**
`crates/terminal` plus `crates/terminal_view` is **22,827 lines**, and the
reusable crate is not in it:

| | Lines |
|---|---|
| `terminal/src/terminal.rs` | 6472 |
| `terminal_view/src/terminal_view.rs` | 3660 |
| `terminal_view/src/terminal_panel.rs` | 3370 |
| `terminal_view/src/terminal_element.rs` | 3042 |
| `terminal/src/alacritty/hyperlinks.rs` | 2012 |
| `terminal/src/mappings/keys.rs` | 442 |
| `terminal/src/mappings/mouse.rs` | 318 |

`terminal_element.rs` is the GPUI element that paints the grid; `mappings/`
is the keyboard-and-mouse-to-bytes translation. Those ~9k lines are the ones
that would be rewritten against `bevy_egui`, where the input side is harder
than in GPUI, not easier: egui hands you cooked key events, not raw bytes, so
alt-screen keys, ctrl-chords, bracketed paste, mouse reporting and IME are
all hand-work. Orca pays this bill too, on top of a mature emulator —
`terminal-kitty-keyboard-flags.ts`, `terminal-osc-color-reply.ts`,
`terminal-partial-escape-tail.ts`, `pty-slave-line-discipline-echo.ts`,
`terminal-multiplex-flow-control.ts`.

**And Zed does not drive agents through that terminal.** It drives them
through the **Agent Client Protocol** — `agent-client-protocol = "=2.2.0"`,
stdio JSON-RPC to a child process with piped stdio
(`agent_servers/src/acp/transport.rs::spawn_stdio`), with agent ids
`claude-acp`, `codex-acp` and `gemini`. The terminal is a separate feature for
people, not the channel for agents.

**The inversion is the finding.** In ACP the terminal is a capability the
*editor offers the agent*: `AcpThread::create_terminal` takes a command the
agent asked to run and runs it under an OS sandbox — macOS Seatbelt, Linux
Bubblewrap, Windows Bubblewrap-inside-WSL — with the project's worktrees as
the writable set and an explicit note that the command's working directory is
model-controlled and must not widen its own scope. Gen already owns both
halves of that: `localgpt-sandbox` is Landlock and Seatbelt, and
`localgpt-cli-tools` is the dangerous-tool set built on it.

So the answer to "better tech stack": **the better stack is to not be the
terminal.** Take `alacritty_terminal` the day a pane is genuinely wanted;
until then the thing Zed would tell us to build is tier 1.

## Licence: read Zed, copy nothing; adopt ACP

**Zed's own code is GPL-3.0-or-later and cannot come into this repository.**
207 of its 241 crates carry `license = "GPL-3.0-or-later"`, and that includes
every one relevant here: `terminal`, `terminal_view`, `acp_thread`,
`agent_servers`, `acp_tools`, `agent_ui`. The 34 Apache-2.0 crates are the
platform layer — `gpui*`, `collections`, `util`, `sum_tree`, `http_client`,
`path`, `scheduler` — so GPUI itself is permissive but
`terminal_element.rs`, the part worth having, is not. LocalGPT is Apache-2.0,
and `deny.toml` allows no copyleft beyond MPL-2.0, so a GPL dependency fails
CI on the way in. This is the same rule the monorepo already applies to
`bevy_debugger_mcp`: study the approach, implement independently.

What this RFC takes from Zed is therefore facts, not expression — which crate
it depends on, how large the view layer is, that agents ride a protocol
instead of a PTY. If a pane is ever built, the temptation to copy is
`mappings/keys.rs`, and the answer is that `alacritty_terminal` and alacritty
itself carry equivalent tables under Apache-2.0.

**The terminal stack is clean.** `alacritty_terminal` 0.26 is Apache-2.0,
`vte` 0.15 is `Apache-2.0 OR MIT`, `portable-pty` 0.9 is MIT. All three pass
`deny.toml` as written. Tier 2 is expensive, not encumbered.

**ACP is clean, and is no longer Zed's.** The protocol moved out of
`zed-industries` into its own vendor-neutral organisation,
`github.com/agentclientprotocol`, and everything in it is Apache-2.0 with no
CLA and an explicit inbound-licence statement:

| | |
|---|---|
| `agent-client-protocol` (crates.io) | Apache-2.0, the runtime crate, 4.9M downloads; `rust-sdk` pushed 2026-10-02 |
| `agent-client-protocol-schema` | Apache-2.0, the wire types alone, for tooling and codegen |
| Official SDKs | Rust, TypeScript, Python, Kotlin, Java — all Apache-2.0 |
| `claude-agent-acp` / `@zed-industries/claude-code-acp` | Apache-2.0 |
| `codex-acp` | Apache-2.0, **copyright JetBrains s.r.o.** (GitHub reports NOASSERTION; the LICENSE file is plainly Apache-2.0) |
| `registry` | Apache-2.0 — a CI-verified index of **~50 agents** |

The registry is the adoption argument: `claude-acp`, `codex-acp`, `gemini`,
`cursor`, `devin`, `goose`, `opencode`, `github-copilot`,
`github-copilot-cli`, `qwen-code`, `kimi`, `glm-acp-agent`, `grok-build`,
`amp-acp`, `cline`, `kilo`, `junie`, `factory-droid`, `mistral-vibe`,
`poolside` and more, each verified in CI to return valid `authMethods` in the
handshake. JetBrains maintains one of the agents *and* consumes a dedicated
registry index for its IDEs, so there are at least two independent client
implementations and a second vendor with skin in it.

**Recommendation: adopt ACP, at protocol version 1.** The repository states
plainly that "the current stable ACP protocol version is `1`", and wire
compatibility is negotiated by `protocolVersion` at `initialize` rather than
inferred from the crate version. Zed's
`features = ["unstable", "unstable_protocol_v2"]` is opt-in bleeding edge and
is not what Gen should pin. Depend on the runtime crate without those
features, negotiate v1, and read capabilities to decide what is available.

Two things to verify before the dependency lands, neither expected to bite:
`cargo deny check licenses` over the new transitive tree, and that the crate
does not drag platform-specific dependencies into anything
`localgpt-core`-shaped — it would live in the Gen/app layer, which is allowed
everything, so this is a hygiene check rather than a risk.

## If egui is not a given

Dropping `bevy_egui` is not a detail of tier 2 — it changes which tier is
cheap, and it opens a third door that is better than either.

**Gen already runs two UI stacks, and only one of them is egui.**
`crates/gen/src/ui/` — `hud.rs`, `label.rs`, `notification.rs`, `sign.rs`,
`tooltip.rs`, 1484 lines — is pure `bevy_ui` and `bevy_text` with no egui in
it. egui is 233 call sites across nine files: `desktop/panel.rs` (58),
`desktop/collab.rs` (35), `gen3d/gallery_ui.rs` (30), `desktop/client_panel.rs`
(26), `inspector/detail.rs` (41), `inspector/world_info.rs` (9),
`inspector/outliner.rs` (8), `inspector/mod.rs` (6), `desktop/fonts.rs` (20).
So "no egui" is a real option, not a rewrite from zero.

**Tier 1 needs no egui at all.** An ACP transcript is a scrolling list of
message chunks, tool calls and a plan — which is `notification.rs`'s pattern
with scrollback. In `bevy_ui` it is ordinary work, it themes with the rest of
the window, and it survives whatever happens to the egui chrome. This makes
the cheap tier cheaper.

**Tier 2 gets better without egui, and still is not cheap.** Two halves move
in opposite directions from what the GPUI comparison suggests:

- *Painting gets easier.* A terminal is a uniform cell grid, which is the
  single easiest thing a 3D engine can draw: upload one texture of (glyph
  index, foreground, background) per cell, draw one quad, sample a glyph
  atlas in WGSL. That is the fast path, and it is native territory for a
  Bevy app that already owns a render pipeline. egui would tessellate per
  glyph instead, so egui is the *worse* renderer here, not the easier one.
- *Input gets better.* Bevy's `KeyboardInput` carries the logical `Key`, the
  `KeyCode` and the press state separately, and winit's IME events come
  through — closer to raw bytes than egui's cooked events. The 442-line
  key-to-escape-sequence table, mouse reporting, alt-screen, selection and
  scrollback are unchanged, so call it 2–4k lines rather than Zed's ~9k
  view layer. Weeks, not days, and `alacritty_terminal` is Apache-2.0 so the
  emulator stays free.

**And egui is already the input problem.** `inspector/mod.rs:121` carries a
NOTE that `enable_absorb_bevy_input_system` must stay off because it clears
`ButtonInput<KeyCode>` while egui has focus and kills WASD, with three more
hand-rolled focus guards at `panel.rs:341`, `panel.rs:410`,
`client_panel.rs:94` and a fourth at `gallery_ui.rs:150`. A terminal is the
worst possible egui citizen in that scheme, because it wants *every*
keystroke — Tab, Escape, arrows, `Ctrl-C` — and so does the player
controller. Putting a terminal in egui would make a workaround into a
structural problem.

**The third door: the panel does not have to be in the window.**
`crates/gen/src/net/web.rs` is 1150 lines that already serve a join page, a
session client, the three.js viewer, vendored three and a WebSocket; and
`inspector/ws_server.rs` already speaks a WebSocket inspector protocol. Gen
is a web server with a browser UI today. So a PTY pane is `portable-pty`
(MIT) plus a WebSocket pump plus xterm.js in a page — Orca's stack exactly,
with the emulator and all its escape-sequence edge cases on the other side of
the socket and not our problem. That is tier 2's value at roughly tier 1's
cost.

For Gen's *own* panel in that world there is also `ratatui` 0.30, which this
repository already uses for `localgpt tui` (`crates/cli/src/cli/tui.rs`) — a
cell-grid widget tree is a clean fit for a Bevy texture. Note the
distinction: ratatui draws *our* panel; it cannot host *Claude Code's* TUI,
which needs a VT emulator. The two are not substitutes.

**What this makes consistent.** The world is a folder, the agents are
processes outside it, the authoring surface is HTTP, the public renderer is
three.js, and the inspector already has a wire protocol. "The editor chrome
lives in a browser tab and the Bevy window is only the canvas" is the same
sentence as the rest of this design, and it retires 233 egui call sites and a
second font stack with it.

The honest cost: two windows is a worse consumer product than one, and §13.3's
tiers aim at consumers. That cuts cleanly, though — a browser tab is right for
the developer wedge, where the person already has a terminal and a coding
agent open, and the consumer build's answer is probably tier 0 and the gallery
with no panel at all.

## The panel is already the right shape

The strongest argument for tier 1 is that Gen's existing panel does not need
redesigning. `desktop/chat.rs`'s `ChatEvent` is very nearly a subset of ACP's
`SessionUpdate`:

| `ChatEvent` | ACP `SessionUpdate` |
|---|---|
| `Delta(String)` | `AgentMessageChunk` |
| `ToolStarted` / `ToolFinished` | `ToolCall` / tool-call updates |
| `Notice` / `Warning` | `Notice` |
| `Ready { model }` | `SessionInfoUpdate`, `CurrentModeUpdate` |
| `ModelOptions(Vec<String>)` | `ConfigOptionUpdate` |
| `TurnFinished { error }` | the prompt response's stop reason |
| — | `AgentThoughtChunk`, `Plan`, `CompactionUpdate` |
| `/model`, `/new`, `/clear` in `agent_loop.rs` | `AvailableCommandsUpdate` |

The panel, its channels, its fonts and its model menu all survive. What is
deleted is the half that *produces* those events, and it is replaced by a
transport and a deserialize. The slash commands get better in the trade:
instead of Gen defining `/model` and `/new`, the agent advertises its own.

## What spawning the agent buys that launching it does not

Two things worth more than the convenience:

- **Attribution that cannot be fudged.** Today `author` is a string in the
  batch, and in a git repository it becomes the commit author. If Gen spawns
  the child it sets the identity, and the history stops depending on an agent
  correctly describing itself.
- **A token per child instead of a token in a file.** `.live/endpoint.json`
  holds one bearer token that every agent reads and users paste into curl. A
  spawned child can be handed a scoped, labeled, revocable token in its
  environment and never read the file — which also makes "who sent this
  batch" a property of the connection rather than a claim in the body.

## What to do first, before any pane

**The MCP shim.** It is already in the proof of concept's *Not done* list and
draft 0.3 already decided its shape: a stdio server the agent starts, which
reads `.live/endpoint.json` and calls the same local HTTP API. That one
binary makes Claude Code a first-class editor of an open world from the
user's own terminal — tier 0 with tools — with no emulator, no second way in,
and no agent logic inside Gen. `mcp_server.rs` is 251 lines and is most of it
already.

Then tier 1, in this order: spawn an ACP server (`claude-code-acp`,
`codex-acp`), render `SessionUpdate` into the existing `ChatEvent` channel,
and tie tool calls to the revisions their ops produced — which is the thing
neither a terminal pane nor a transcript can do, and the reason to prefer
events over bytes. The log already knows what changed and who asked; the
missing link is *which turn* asked.

## The process half is already built

`crates/core/src/pty.rs` is a portable `PtyHost` trait — sessions, a
scrollback replay buffer for reattaching clients, and deliberately
three-valued liveness so that "we could not ask" is never reported as "it
exited", because that is what makes a client discard a pane whose process is
alive and still producing output. `crates/cli-tools/src/pty.rs` is 742 lines
implementing it on `portable-pty` 0.9. The daemon registers it
(`cli/src/cli/daemon.rs:318`) and the bridge serves it
(`server/src/security/bridge.rs:142`), with the trait kept in the portable
crate precisely so sessions can move into a supervised process later without
changing a caller.

So PTY sessions are already a service in this architecture, reattachable over
IPC. Tier 2's remaining cost is only VT emulation, rendering and input
mapping — the process, scrollback and lifecycle layer does not need writing,
and the seam already sits where a pane would want it.

## Three things that bite regardless

- **`PATH`.** `desktop/shell_env.rs` exists because a build launched from
  Finder inherits launchd's minimal environment and cannot find a
  Homebrew-installed `claude`. Any spawned agent — pane, ACP server or
  terminal — hits the same wall, and the fix is already written. Reuse it;
  do not rediscover it.
- **Login flows still want a real terminal.** Even with ACP, Zed carries
  `GEMINI_TERMINAL_AUTH_METHOD_ID = "spawn-gemini-cli"`: an auth method whose
  `meta` is a command, args and env, so the editor can spawn `gemini /auth`
  in a terminal because the protocol cannot carry an interactive login.
  Whatever Gen builds, "open a terminal here" stays a button.
- **Lifecycle is already safe, and should stay that way.** A child killed
  mid-batch cannot half-commit, because batches commit whole or refuse whole.
  That property is doing real work in a world where agents are processes
  someone can Ctrl-C, and no convenience is worth relaxing it.

## The line to hold

A pane or an ACP session makes Gen the agent's **window**, not the agent.
That distinction is the whole value of the subtraction, and it erodes one
reasonable-sounding step at a time: *let Gen inject the opening prompt; let
Gen retry the op the authority refused; let Gen summarize the turn for the
log; let Gen pick the model.* Each is a small feature and all four together
are `agent_loop.rs` grown back behind a different front end.

The rule: **Gen spawns, shows, sandboxes and kills processes. It never
composes a prompt or interprets a reply.** Anything the person needs that
breaks this rule is a request for the harness back, and should be argued as
that.

## Not decided

- How a world rides a protocol written for code. ACP assumes a code editor:
  text buffers, diffs, file mentions. A 3D canvas has no slot in it, so
  Gen's own content — "revision 7 changed these entities" — would ride
  `meta`, which is what `meta` is for but is not the same as fitting. Worth
  raising upstream: the protocol has an RFD process and meeting notes, and
  "the client's document is not text" is a general gap, not a Gen quirk.
- Whether Gen or the authority spawns the agent. The authority
  (`world-agent`) owns the folder, the token and the attribution, and works
  headless; Gen owns the window the panel is in. Tier 1 wants both, and the
  split is not obvious.
- Whether ACP's `create_terminal` is worth implementing on `localgpt-sandbox`
  — it is the one place Gen's existing sandbox would be exactly the right
  tool, and also an invitation to run arbitrary commands from a 3D editor.
- Whether the prompt panel survives at all in the shipping consumer app, or
  whether tier 0 plus the gallery is the whole story for people who are not
  running a coding agent (§13.3's tiers).
- Whether dropping egui takes the inspector with it. The transcript and a
  terminal both have non-egui homes, but `inspector/detail.rs` and
  `outliner.rs` are 49 egui call sites of real editor chrome. They have a
  WebSocket protocol already, so the browser is where they want to go — but
  that is a separate decision with its own cost, and it should not ride in on
  a panel RFC.
