# SolidCraft and CADCraft: what two agent-built CAD apps teach LocalGPT

**Status:** research, 2026-10-31. A reading of two CAD applications kept as
reference material, `external/3d/solidcraft` (v0.28.0) and
`external/3d/cadcraft` (v0.14.0). Nothing is built against it; the ranked
list at the end is what to act on. The companion reading of Blender is
[blender-lessons.md](blender-lessons.md).

Both are `MIT OR Apache-2.0` (`LICENSE-MIT` and `LICENSE-APACHE` in each), so
unlike Blender's or Zed's, their code may come into this repository with its
notices kept. What follows still describes designs, with pointers into each
tree, checked by hand on 2026-10-31. Marks as in blender-lessons.md:
**adopt** (as described), **adapt** (the idea fits, the mechanism does not),
**avoid** (does not map, or we already do better).

## 1. What they are

| | SolidCraft | CADCraft |
|---|---|---|
| What | 3D parametric solids, Fusion and Onshape class: sketches, a constraint solver, a feature timeline that rebuilds B-rep bodies; STEP, IGES, 3MF, STL, OBJ, glTF | 2D drafting, AutoCAD class: a command line with prompts, snaps, layers, dimensions, hatch, blocks, layouts, plotting; DXF R12–2018 read and R2000 written, DWG through `acadrust` |
| Started, commits, version | 2026-10-07, 3,354, v0.28.0 (released daily) | 2026-10-05, 955, v0.14.0 |
| Own Rust lines, tests, commands | ~752K (plus 93K vendored), ~10,300, 544 | ~224K, ~2,770, 430 |
| Its own verdict | "early but usable for simple parts"; 90 of 90 oracle cases pass (`docs/oracle.md`) | "pre-alpha": 245 of 491 menu items, about 38% ready for real work (`ROADMAP.md`) |
| Interface | egui 0.36 on wgpu; the desktop app, and the same app as WebAssembly in a browser | egui; desktop and WebAssembly |
| For agents | an MCP server that drives the app or runs headless, scripts, a browser command API, an in-app agent chat (§7) | an MCP server whose tools include the command line itself (`command_line`, `crates/mcp/src/tools.rs:11`), and the UI's elements and clicks when bridged |

**SolidCraft is the more mature**, on every count: about three and a half
times the commits, the code and the tests, an outside oracle, daily releases
and the deeper agent integration. Both are written by agents at 100 to 230
commits a day, so their size overstates how hardened either is. The machinery
that checks them is what transfers, and most of what follows is that
machinery.

## 2. Everything is a command

**Both.** Every action is a static `CommandSpec` in one registry (SolidCraft
`crates/engine/src/cmd/mod.rs:131`, CADCraft `crates/engine/src/cmd.rs:66`):
an id, a label, its parameters documented, `enabled` returning
`Result<(), reason>`, `run: fn(&mut Session, &Value) -> Result<Value>`, and
whether it undoes. The toolbar, the menus, scripts, the control channel and
the MCP server call the same entries, and a command called by a program never
opens a dialog. SolidCraft's `planned!` names a command that is not built
yet, so the catalog states its own gaps.

**For LocalGPT:** one world has four vocabularies today: the live API's
routes (`crates/gen/src/gen3d/live.rs:1717–1953`), the shim's eight tools,
`localgpt world`'s subcommands and Gen's ~100 `gen_*` tools. *Adopt* one
catalog for 0.6.0's re-fronting ([the-harness-goes.md](the-harness-goes.md)),
every entry saying whether it can run now and, if not, why: the page greys
it, and the agent reads the reason instead of meeting a refusal. *Adopt*
`planned!`-style entries, so `AGENTS.md` can name what is coming without an
agent calling it.

## 3. Contracts an agent can read

**SolidCraft.** Commands carry typed contracts (schemars,
`crates/engine/src/schema/mod.rs`). `describe_command` returns a command's
input and output as JSON Schema 2020-12, and `--param-check off|warn|strict`
checks every call against them, answering a misspelt key with "`operaton`;
did you mean `operation`?" (`docs/mcp.md:69–73`). The commands still without
a contract are listed in `crates/engine/src/schema/unconverted.txt`, which
may only shrink.

**For LocalGPT:** the format already refuses an unknown key. Strict ingest
answers "… is not a field of the format (readers would drop it — a typo?)"
(`crates/world-agent/src/live.rs:1048`). The agent's half is missing:

- *Adopt* the schema in the tool. `world_submit` describes its ops as
  `{"type": "object"}` (`crates/cli/src/cli/world_mcp.rs:208`), so an agent
  learns the vocabulary from `AGENTS.md`'s prose. The `openworldformat` crate
  already derives `JsonSchema` for `EditOp` behind its `schema` feature
  (`rust/src/history.rs:12`): put the generated schema in the tool's input
  schema, and the client checks a batch before sending it.
- *Adopt* the nearest field in the refusal ("`rotaton` is not a field of the
  format; did you mean `rotation`?"): an edit distance over the fields known
  at that path.

## 4. Never crash

**Both.** Each engine crate denies `unwrap_used`, `expect_used`, `panic`,
`unimplemented`, `todo` and `unreachable` (`crates/engine/src/lib.rs:7` in
SolidCraft, `:8` in CADCraft). CADCraft wraps every interactive command in a
last-resort guard that turns a panic into an error and restores the drawing
(`crates/engine/src/guard.rs`). Its panic hunt calls every registered command
with adversarial parameters, each on a fresh, populated session on its own
thread with a deadline, and counts panics with a hook, because the guard
hides them from the result (`crates/engine/tests/panic_hunt.rs`). SolidCraft
runs seeded fuzz sessions (`crates/engine/src/fuzz.rs`) and mutation testing
with cargo-mutants (`docs/mutation-testing.md`).

**For LocalGPT:** a panic in the live API's Bevy system ends the app, and
the agents' terminals with it. The workspace has one `catch_unwind`
(`crates/previs/src/pdf.rs:329`), no workspace lints, no property or fuzz
tests here or in the `openworldformat` crate, and roughly 120 `unwrap()` and
85 `expect(` outside test modules (a rough count, 2026-10-31). *Adopt*, in
this order:

- `catch_unwind` around the trial apply in the live system.
  `LiveWorld::submit` applies a batch to a trial copy and commits whole or
  not at all, so a panic needs no restore: drop the trial, refuse the batch
  as an internal error, log it.
- A panic hunt over `EditOp`: every variant with hostile fields (NaN and
  infinities, huge counts, empty and very long names, deep nesting, dangling
  and cyclic references) through `LiveWorld::submit`, which needs no Bevy, so
  it is a plain `cargo test`.
- The engine crates' deny list on `world-agent`, `world-sync` and
  `world-editor` first, with warnings for the rest of the workspace.

**For openworldformat:** *adopt* damaged packages beside the conformance
worlds (a truncated `ops.jsonl`, invalid UTF-8, duplicate ids, cycles, an
asset whose bytes don't match its name) that every reference must refuse
with an error and never crash on.

## 5. Headless parity

**Both.** The MCP server works with or without the app: `solidcraft-cli mcp`
"finds the running app; headless when none runs (`--headless` forces that)"
(`docs/mcp.md:30`). It finds the app through a file the app writes beside its
token, `control/current.port` (`crates/engine/src/control_auth.rs:75`).
SolidCraft's screenshots work headless too: on the GPU, falling back to the
CPU, and saying which renderer drew.

**For LocalGPT:** `localgpt world` already works headless and refuses a write
only while an app answers at the folder's endpoint
(`crates/world-agent/src/headless.rs:16`), but `localgpt world mcp` will not
start without an open app. *Adopt* the CAD rule: the shim starts either way,
sends each call to the app when one answers and to the headless authority
when none does, and says in each reply which one it was. A headless
screenshot (the web viewer in a headless browser, or Bevy off-screen) is the
later half.

## 6. Setup, discovery and skills

**SolidCraft.** `solidcraft-cli mcp setup`, also Help → Connect AI
assistants, adds a `solidcraft` entry to every AI client installed, ten of
them, and edits each file in place: only its own entry changes and every
other byte stays, the file is copied to `<name>.bak` first, a file it cannot
read is left alone, and so is an entry the user changed. A client counts as
installed when its configuration folder exists; no client program is run
(`crates/engine/src/agent_setup.rs`). Skills ship in `skills/`
(`solidcraft-commands`, `solidcraft-checking`, …) and reach agents through a
`skill_reference` tool.

**For LocalGPT:** *adopt* `agent_setup.rs`'s rules as `localgpt setup`'s
(the-harness-goes.md): in place, `.bak` first, never a changed entry,
detected by folder, nothing run. *Adopt* the port file as the answer to that
RFC's question of one registration or one per world: one, `localgpt world
mcp` with no folder, finding the world the app opened last through a file
the app writes in the user's state directory; a folder argument stays for
headless use. *Adapt* `skill_reference`: the world's `AGENTS.md` and the op
recipes served as MCP resources, so an agent started outside the folder has
them too.

## 7. The agent in the window

**SolidCraft** has an in-app agent chat (`crates/ui-egui/src/agent_chat/`,
about 17,000 lines with its tests): an egui side panel, desktop only. **It is
not a terminal.** It speaks the Agent Client Protocol: the agent is a child
process, and the two exchange JSON-RPC over its stdin and stdout (`acp.rs`).
Claude Code and Codex run through their ACP adapters (`claude-agent-acp`,
`codex-acp`); Gemini CLI, OpenCode, Copilot CLI, Qwen Code, goose and Grok
through ACP modes of their own; Antigravity through a separate ACP server
(`discover.rs`). The panel draws what the agent sends as structured events
(the reply as Markdown, its thinking, a card per tool call with its status,
the plan) and asks permission questions as Allow and Deny buttons.

- It tells the agent it offers no terminal and no file system
  (`"terminal": false`, `acp.rs:150`), switches off Claude Code's own shell,
  file and web tools for the session
  (`_meta.claudeCode.options.disallowedTools`), and starts the agent in an
  empty folder of its own (`mod.rs:888`). The agent's only way to the design
  is SolidCraft's MCP server, bridged to the window.
- Every tool call passes a gate (`gate.rs`): run, ask first (anything that
  reads or writes files or replaces the design), or never. An optional limit
  caps changes per turn, and each turn undoes as one step.
- Sign-in stays outside: an agent that needs a terminal login gets its
  command shown with a Copy button, to run in the user's own terminal. Slash
  commands are ignored; diffs and resuming a conversation are on its open
  list (`docs/design/in-app-agent-chat.md`).
- A second backend, `api_client.rs` (2,894 lines), runs the tool loop itself
  against an OpenAI- or Anthropic-compatible endpoint, with the key from the
  system keychain.

**The agent cursor** (`crates/ui-egui/src/agent_cursor.rs`) is a second,
marked pointer that acts out each command an agent sends: it glides to the
command's toolbar button and clicks it, then to each pick in the parameters,
and only then does the command run. The reply waits for the animation.

**For LocalGPT:** the page chose the other door, the agents' own TUIs in
xterm.js over a PTY, so they come whole, with their `/commands`, approvals and
sign-ins ([the-page.md](the-page.md)). Read against that:

- *Avoid* the API backend. It composes the prompt and runs the loop: the
  harness 0.5.0 removes, and what the *line to hold* of
  [agents-outside-the-window.md](agents-outside-the-window.md) forbids.
- *Adopt* the chat as the reference if that RFC's tier 1 is ever built. It
  is tier 1, working and licence-clean, and it has met what the RFC only
  predicted: the adapters' login methods, an agent that needs a server of its
  own.
- *Adopt* its process rule for the page's terminals now (`process.rs`). An
  agent starts helpers (an adapter starts the CLI it wraps, the CLI starts
  its MCP servers), so each agent gets a process group of its own, and
  stopping it closes its stdin, then sends the group TERM, then KILL; the
  tests use a fake agent that leaves helpers behind. Check `Terms::close`
  (`crates/app/src/webui/term.rs`) against it.
- *Adapt* the agent cursor without the wait: when a batch commits, mark what
  it touched in the world, with the agent's name, so the person watching sees
  where the change landed. The authority does not slow an agent down for the
  show.
- Note what the protocol has that a terminal lacks: the end of every turn
  (the prompt's reply carries its `stopReason`). The page's terminals have no
  such mark: `TurnEnded` (`crates/gen/src/gen3d/live.rs:529`) had no
  sender once herdr's pane went. Since 2026-10-31 the page's terminals send
  it, from the marks agents put in their titles (mobile-remote.md).

## 8. The control channel

**CADCraft** writes an unguessable token for each session to an owner-only
file in the per-user runtime directory, and refuses any request without it.
It bounds everything (request size, queued requests, idle connections; past
64 queued requests the answer is "busy" with `state: "not-run"`), and a
timeout says whether the app had started the request: `not-run` or
`may-have-run` (`docs/control-protocol.md:12–37`). SolidCraft's token is
likewise made fresh for each launch and readable only by the user
(`docs/mcp.md:36–45`).

**For LocalGPT:** the live API's token is the first 32 hex digits of a
SHA-256 of the process id, the time and the folder
(`crates/gen/src/gen3d/live.rs:603–612`), written to `.live/endpoint.json`
by `std::fs::write`, so with the umask's permissions (`:622`). The API also
takes it in the query string, so a panel can open the board in a browser
(`:94–96`). *Adopt*: the token from the operating system's random source;
`.live/` created 0700 and the file 0600; the query form accepted on the
board's pages only. (Done 2026-10-31, ahead of the phone's listener:
`write_endpoint` and `authorized` in `live.rs`.) *Adopt* `not-run` and `may-have-run` for the shim, whose
call can time out after the app took the batch; the authority commits whole
or not at all, so `may-have-run` is settled by reading the log's head.

## 9. Rules that name their enforcer

**SolidCraft's** `AGENTS.md` opens with "Hard rules (what enforces each)"
(`AGENTS.md:8`): each rule names the check that fails when it is broken (a
lint, a test, or an xtask such as `cargo xtask layers`, which both apps use
to hold their crate layering, `xtask/src/layers.rs`), and `cargo xtask ci`
runs them all.

**For LocalGPT:** `CLAUDE.md`'s critical rule, that `localgpt-core` compiles
for iOS and Android, has no check in CI, and it goes with `mobile-ffi`.
*Adopt* the form for the rules that outlive it. The format crates
(`world-types`, `world-agent`, `world-sync`, `world-editor`, `world-export`)
depend on neither `core` nor Bevy today, and nothing checks it; a `cargo
tree` check in CI would. Every hard rule in `CLAUDE.md` and `AGENTS.md` then
names its check, or says it has none.

## 10. An outside oracle

**SolidCraft** checks 90 reference parts against values computed outside its
code, 69 by hand from the geometry and 21 by FreeCAD, for volume, area,
centre of mass and bounding box (`docs/oracle.md`).

**For openworldformat:** the conformance corpus is already a differential
test (every reference runs every merge case and must agree;
`conformance/README.md`, "Rules every reference runs"), but its expected
results come from the JavaScript reference (`merge/generate.mjs`), so the
five agree with one of themselves. *Adapt* SolidCraft's analytic half: a few
cases whose expected fold is worked out by hand from the spec's text, which
check the JavaScript reference too.

## 11. Other things noticed

- **Batch replies** (`docs/mcp.md:290`). A failed batch names the failing
  step and lists `applied` and `not_run`; `verbose: false` keeps replies
  small in large designs; `timings` and `profile` say where the time went;
  `coalesce_parameters` rebuilds once per run of parameter edits. Batches
  here refuse whole, so `applied` is all or nothing; *adapt* `verbose: false`
  for large batches, counts in place of the full receipt.
- **A revision probe** (`document.revision`, `docs/mcp.md:330`) moves only
  when the design changes, so an agent can poll without reading the design.
  *Adopt* a route that answers the head's revision and nothing else.
- **Estimates in agent-hours** (CADCraft's `ROADMAP.md:32–42`). The roadmap
  prices work in "Opus 5.5 agent wall-clock hours", calibrated on the
  repository's own first build (about 11.5 agent-hours), with readiness
  given per audience. RFCs here estimate in days; the calibration is the
  part to copy if the estimates are to mean anything.
- **Both run in a browser** as WebAssembly, and neither has a mobile app:
  see [mobile-remote.md](mobile-remote.md).

## 12. What does not transfer

- **egui, and a Rust-only interface.** The app's interface is a web page
  (the-page.md).
- **B-rep kernels and constraint solvers.** A `.world` has no solids.
- **Parity with an incumbent as the measure of progress.** CADCraft counts
  AutoCAD's menu items; a `.world` editor has no incumbent to count against.
- **CADCraft's prompt machine as the agents' way in** (`command_line` types
  at the prompt as a person would). Agents here send typed ops, and a prompt
  language would be a second vocabulary.
- **SolidCraft's API backend** (§7).

## Top ten, ranked

1. **Shim: the op schema in `world_submit`, and the nearest field in strict
   ingest's refusal** (§3). Hours to a day.
2. **Live system: `catch_unwind` around the trial apply, and a panic hunt
   over `EditOp` through `LiveWorld::submit`** (§4). One to two days.
3. **Shim: start without an app, route each call, say which answered** (§5).
   One to two days.
4. **Live API token: the operating system's randomness, `.live/` 0700 and
   the file 0600, the query form on the board's pages only** (§8). Hours.
   Done 2026-10-31.
5. **`localgpt setup` on `agent_setup.rs`'s rules, with one registration
   that finds the open world** (§6). Part of 0.5.0; two to three days.
6. **The page's terminals: an agent's process group stopped as one**, so
   closing a tab or quitting ends its helpers too (§7). Hours, with a test
   like SolidCraft's.
7. **Lints: the engine crates' deny list on `world-agent`, `world-sync` and
   `world-editor`; warnings elsewhere** (§4). A day, then the fixes.
8. **Rules that name their check, and a CI check that the format crates
   depend on neither `core` nor Bevy** (§9). Hours.
9. **One command catalog under the live API, the shim, the CLI and the page,
   with a reason when a command can't run** (§2). 0.6.0; weeks, already
   scheduled.
10. **openworldformat: damaged packages every reference refuses, and a few
    hand-worked folds** (§4, §10). Days, in that repository.
