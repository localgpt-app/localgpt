# SolidCraft and CADCraft: what two agent-built CAD apps teach LocalGPT

**Status:** research. A reading on 2026-10-31 of two CAD applications kept as
reference material, `external/3d/solidcraft` (v0.28.0) and
`external/3d/cadcraft` (v0.14.0); what it says of LocalGPT was checked
against the tree on 2026-11-11. Each section says what is applied and what
is open, and the list at the end is the open part. The companion reading of
Blender is [blender-lessons.md](blender-lessons.md).

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
| Its own verdict | "early but usable for simple parts"; 90 of 90 oracle cases pass (`docs/oracle.md`) | "pre-alpha": 245 of 491 menu items, about 38% ready for real work (`ROADMAP.md`) |
| Interface | egui 0.36 on wgpu; the desktop app, and the same app as WebAssembly in a browser | egui; desktop and WebAssembly |
| For agents | an MCP server that drives the app or runs headless, scripts, a browser command API, an in-app agent chat (§7) | an MCP server whose tools include the command line itself (`command_line`, `crates/mcp/src/tools.rs:11`), and the UI's elements and clicks when bridged |

**SolidCraft is the more mature**, on every count: about three and a half
times the commits, the code and the tests, an outside oracle, daily releases
and the deeper agent integration. Both are written by agents at 100 to 230
commits a day, so their size overstates how hardened either is. The machinery
that checks them is what transfers, and most of what follows is that
machinery.

### How they draw

**Both:** eframe, egui's shell on winit, owns the window; the model view is
the app's own wgpu pipelines in an `egui_wgpu` paint callback; and the same
program runs in a browser as WebAssembly, on WebGPU where it is and WebGL2
otherwise. Neither uses a web view or an engine. Both keep forks: SolidCraft
vendors egui-wgpu and winit with fixes (`vendor/`), CADCraft pins its own
forks of egui and winit.

**For LocalGPT:** applied. This is the shell of [all-rust.md](all-rust.md)
(`crates/app/src/shell`): eframe owns the window, the world is a texture
Bevy renders on the same device, and the world panel takes the keys while it
has focus, as CADCraft's canvas does. Two things still inform open work:

- **The browser** (all-rust.md, step F). Both run their egui interface in a
  browser, and neither has a mobile app. CADCraft's canvas is a display list
  uploaded once per edit and drawn by one shader with a 12-byte vertex that
  runs on WebGL2 (`crates/ui-egui/src/gpu.rs`), and its text is shaped with
  harfrust and ordered with unicode-bidi, which egui does not do itself.
- **Pictures with no window** (§5). SolidCraft's CLI snapshots, MCP
  screenshots and thumbnails go through the same pipelines offscreen
  (`capture.rs`), with a CPU rasteriser as the last tier, and every picture
  names the renderer that drew it.

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
routes, the shim's tools, `localgpt world`'s subcommands and Gen's 88
`gen_*` tools. Keep working them toward one catalog, with every entry saying
whether it can run now and, if not, why: the native shell greys it, and the
agent reads the reason instead of meeting a refusal. *Adopt* `planned!`-style
entries only where `AGENTS.md` must name work that is genuinely scheduled;
they are not a way to advertise speculative tools.

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
(`crates/world-agent/src/live.rs:1078`). The agent's half is missing:

- *Adopt* the schema in the tool. `world_submit` describes its ops as
  `{"type": "object"}` (`crates/cli/src/cli/world_mcp.rs:151`), so an agent
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

**For LocalGPT:** a panic in the live system ends the world's thread (the
whole app, in Gen's own window), and the agents' terminals with it: their
PTYs are that thread's. The workspace has one `catch_unwind`
(`crates/previs/src/pdf.rs:329`), no workspace lints, no property or fuzz
tests here or in the `openworldformat` crate, and roughly 30 `unwrap()` and
55 `expect(` outside test code, four of them in `world-agent`, `world-sync`
and `world-editor` together (a rough count, 2026-11-11). *Adopt*, in this
order:

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

**For LocalGPT:** `localgpt world` already runs either way: while an app
answers at the folder's endpoint each command is a call to its API, and with
none it commits through the headless authority
(`crates/cli/src/cli/world.rs`; `world-agent::headless` itself refuses a
write while an app answers, `headless.rs:16`). `localgpt world mcp` still
will not start without an open app (`crates/cli/src/cli/world_mcp.rs:38`),
and a commit's receipt does not say which authority made it. *Adopt* the CAD
rule for the shim: it starts either way, sends each call to the app when one
answers and to the headless authority when none does, and says in each reply
which one it was. A screenshot with no app open is the later half (`localgpt
world screenshot` needs one today): the web viewer in a headless browser, or
Bevy off-screen.

## 6. Setup, discovery and skills

**SolidCraft.** `solidcraft-cli mcp setup` adds a `solidcraft` entry to every
AI client installed, ten of them, editing each configuration file in place
(`crates/engine/src/agent_setup.rs`). Skills ship in `skills/`
(`solidcraft-commands`, `solidcraft-checking`, …) and reach agents through a
`skill_reference` tool.

**For LocalGPT:** the setup does not transfer (§12): writing an entry into
each client's file means knowing agents by name. The user docs show the MCP
entry to add. The skill reference is applied: a package's `AGENTS.md` is the
guide an agent reads where it already works (`AGENTS_GUIDE`,
`crates/world-agent/src/live.rs`), and the shim's `world_submit` description
names it.

## 7. The agent in the window

**SolidCraft** has an in-app agent chat over the Agent Client Protocol
(`crates/ui-egui/src/agent_chat/`), with a gate on every tool call and a
second backend that runs the tool loop itself against a model API, and an
agent cursor that acts out each command an agent sends before it runs
(`crates/ui-egui/src/agent_cursor.rs`). Neither transfers (§12): LocalGPT's
agents keep their own interface in a plain terminal. What the terminal and
the panels still owe:

- **Process cleanup.** An agent can leave helpers behind, so closing a
  terminal must close its input and end the whole process group, escalating
  when needed. Not done: `Terms::close` (`crates/app/src/webui/term.rs`)
  sends the shell one `SIGHUP` through `portable-pty`'s killer and drops the
  PTY, and no test has a child that leaves a helper behind.
- **Visible attribution without animation.** The history panel names each
  entry's author. The outliner does not yet mark what another author just
  changed or what a waiting proposal touches: the app sends who the person
  is and the proposal's entities (`SceneView.person` and `.proposal`,
  `crates/app/src/scene.rs`), and `crates/app/src/shell/scene.rs` draws
  nothing from either. Do not delay the authority to perform an agent
  cursor.

## 8. The control channel

**CADCraft** writes an unguessable token for each session to an owner-only
file in the per-user runtime directory, and refuses any request without it.
It bounds everything (request size, queued requests, idle connections; past
64 queued requests the answer is "busy" with `state: "not-run"`), and a
timeout says whether the app had started the request: `not-run` or
`may-have-run` (`docs/control-protocol.md:12–37`). SolidCraft's token is
likewise made fresh for each launch and readable only by the user
(`docs/mcp.md:36–45`).

**For LocalGPT:** the credentials are applied, in
`crates/gen/src/gen3d/live.rs`: the token is 128 random bits (`setup_live`),
`.live/` is the owner's alone and `endpoint.json` is written owner-only
(`write_endpoint`), and the token is taken from a query only on a board
page, where a browser needs it (`authorized`); the last two have tests.
Open: a shim call can still time out after the app took its batch (30
seconds on each side: `CALL_TIMEOUT` in `crates/cli/src/cli/world_api.rs`,
and the API thread's wait for the canvas), so its result should distinguish
`not-run` from `may-have-run`; because the authority commits whole or not at
all, the agent can settle the latter by reading the log's head.

## 9. Rules that name their enforcer

**SolidCraft's** `AGENTS.md` opens with "Hard rules (what enforces each)"
(`AGENTS.md:8`): each rule names the check that fails when it is broken (a
lint, a test, or an xtask such as `cargo xtask layers`, which both apps use
to hold their crate layering, `xtask/src/layers.rs`), and `cargo xtask ci`
runs them all.

**For LocalGPT:** every hard rule should name the check that enforces it, or
say plainly that it has none. `cargo xtask rule` already enforces the
language and `wry` rules; format conformance and the web renderer have their
own checks. The remaining layering constraint worth making executable is
that the format-facing crates do not acquire a Bevy or application-crate
dependency by accident.

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
  Applied: `GET /tips` answers the head, the entry on screen and its
  revision, and no document (`crates/gen/src/gen3d/live.rs`).

## 12. What does not transfer

- **An embedded ACP chat or model API backend.** The native terminal keeps
  the agent's own interface; LocalGPT does not compose prompts or hold keys,
  and does not rebuild an agent's turns from its glyphs or its title.
- **Registering with each AI client.** Writing an entry into each client's
  configuration means knowing agents by name, which nothing in the tree
  does.
- **B-rep kernels and constraint solvers.** A `.world` has no solids.
- **Parity with an incumbent as the measure of progress.** CADCraft counts
  AutoCAD's menu items; a `.world` editor has no incumbent to count against.
- **CADCraft's prompt machine as the agents' way in.** Agents here send
  typed ops; a prompt language would be a second vocabulary.
- **An agent cursor that delays commands for animation.** Attribution and
  change highlighting must not slow the authority.

## The useful checks from here

This is not a second roadmap. When touching the corresponding subsystem,
retain the habits that transfer:

1. the op schema in `world_submit`'s input schema, and the nearest field in
   a strict-ingest refusal (§3);
2. `catch_unwind` around the trial apply, a panic hunt over `EditOp`, and
   the deny list on `world-agent`, `world-sync` and `world-editor` (§4);
3. the shim starting with or without an app, and every reply naming the
   authority that answered (§5);
4. `not-run` or `may-have-run` when a call times out (§8);
5. process-group cleanup when a terminal closes, and the outliner's marks of
   others' work (§7);
6. an executable check that the format-facing crates take no Bevy or
   application dependency (§9);
7. one command vocabulary across the native shell, the live API, the shim
   and the CLI (§2);
8. counts in place of the full receipt for a large batch (§11);
9. in the spec repository: damaged-package and hand-worked-fold cases in the
   conformance corpus (§4, §10).
