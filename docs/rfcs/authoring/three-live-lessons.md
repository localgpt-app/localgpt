# Three-Live: what a Three.js compiler for agents teaches LocalGPT

**Status:** research, applied. A reading on 2026-11-13 of
`external/3d/three-live` (google/three-live, v1.4.0, commit `c6f7ee2`), kept
as reference material; what it says of LocalGPT was checked against the tree
the same day, and the adoptions below landed with it. Each section says what
is applied and what is open, and the list at the end is the open part. The
companion readings are [cad-lessons.md](cad-lessons.md) and
[blender-lessons.md](blender-lessons.md), and the marks are theirs: **adopt**
(as described), **adapt** (the idea fits, the mechanism does not), **avoid**
(does not map, or we already do better).

Three-Live is Apache-2.0, so its code could come into this repository with
its notices kept. None did: what was taken is designs, written again over
the format's own types. It was read, not built or run, so its claims about
speed are its own.

## 1. What it is

| | Three-Live |
|---|---|
| What | A compiler that takes a Three.js scene script — the kind a language model writes from memory — runs it in a sandbox and lowers it to Android (Filament), iOS and visionOS (RealityKit), WebGL2/WebXR, `.glb` and `.usdz` |
| Its own verdict | "Beta"; "not an officially supported Google product" |
| How it is written | Rust for the compiler and its command line (`compiler/`), TypeScript for the web runtimes, Kotlin and Swift for the phones. 256 commits in the six weeks since 2026-09-30, 237 of them by one author |
| For agents | One binary, `three-live-cli`, that lints a scene, draws it without a GPU, steps its simulation and compares it with a reference picture, and serves all of that as ten MCP tools (`compiler/src/mcp.rs`) |

Like the CAD applications, it is written at a pace that makes its size
overstate how hardened it is. And as with them, the machinery that *checks*
is what transfers: most of what follows is that machinery.

## 2. The same bet, a different centre

Both projects bet that an agent authors 3D as text and that the product's
job is to check the text, not to hold the model. Three-Live's command line
and MCP server run no model for linting, drawing or simulating; a model is
reached only from the features that say so (`editScene`, `from-image`,
`textures bake`), through an endpoint the developer supplies.

The centres differ. Three-Live's unit of change is the whole script: an edit
asks the model to "return the complete corrected script"
(`compiler/src/ai_edit.rs:98`), and undo is the last twenty scripts held in
memory (`three-live/src/plugins/ai.ts`, `MAX_HISTORY`). There are no entity
ids, no authors and no branches. Ours is the op: a batch against a document
with a history, which is what makes undo, review, branches and two authors
ordinary. Code is a good way to *generate* geometry and a poor way to *edit*
a world several hands are in; nothing here argues for moving the centre.

## 3. Checks that need no window

**Three-Live.** `validate` lints a scene from bounding boxes and a projected
camera alone (`compiler/src/spatial_lint.rs:727`): nothing in the scene,
the camera inside a mesh, a mesh outside the frustum, the share of the
viewport each mesh covers, a scene a thousand times too small or too large,
two siblings in the same place, a body floating or buried
(`TL301`–`TL307`). `render` draws a four-view contact sheet — camera, front,
top, isometric, with a metre grid and labelled boxes — a filmstrip or a GIF,
from a CPU rasteriser in plain Rust (`compiler/src/render.rs`), with its own
PNG and GIF encoders. The crate's direct dependencies are a JavaScript
engine, serde, a deflate, a JPEG decoder and clap.

**For LocalGPT:** the lint is applied; the picture is a decision still open.

- *Adopted:* `localgpt world lint` and the shim's `world_lint`
  (`crates/world-agent/src/lint.rs`). It is arithmetic over the document —
  the shapes' boxes as the format states them, the transform hierarchy, and
  the cinematography extension's own `project` — so "in frame" means here
  what it means to the conformance suite, and it runs with no app, in the
  CLI, which links no Bevy. Per camera it says what is in the frame and the
  share each thing covers; per finding, a code, the entities and a hint. The
  rules that are ours and not Three-Live's are the cinematographic ones: a
  subject hidden behind something, a focus outside the frame, a shot and its
  reverse across the line.
- *What it found the day it ran.* On our own drafts: every scene with two
  or more speakers put its singles on opposite sides of the line, because
  each camera stepped to its own right; and with exactly four speakers a
  single stood inside a third actor. Both are fixed in
  `crates/previs/src/stage.rs` (the singles stay on the master's side of
  the line, and a camera steps out of a spot another actor holds), and a
  test stages one to eight speakers and holds the draft to the lint
  (`crates/cli/src/cli/previs.rs`). In Gen: `gen_add_trigger`'s `spawn`
  action writes a host action that no system runs. It is reported
  (`carried-not-run`), not fixed. And in the gallery:
  `gen-misty-temple-pool`'s two pool rims are tori turned a quarter about
  X, from before the format said a torus lies flat, so they stand on edge
  as a fifteen-metre arch over the pool; the lint notes each as a ring half
  in the ground (`sunk`). The world is not changed here.
- *Open:* a picture with no app. [cad-lessons.md](cad-lessons.md) §5 leaves
  `localgpt world screenshot` needing one and names two ways out (the web
  viewer in a headless browser, Bevy off-screen). Three-Live is a third: a
  CPU rasteriser of the twelve parametric shapes would draw contact sheets
  and board frames in the CLI. It is also a third mapping of the format,
  against "one mapping per output", and would have to pass the geometry
  fixture to earn that. Not built. The lint was the cheaper half and needed
  no renderer; build the rasteriser when a deliverable needs pixels with no
  app — a board whose frames print without a browser is the likely one.

What the lint does not see, by construction: a box is not a silhouette, so
a share is the projected box's; nothing hides anything except on the one
line from a lens to its subject; and a mesh asset, whose size the document
does not state, is counted and left unmeasured.

## 4. Diagnostics as an interface

**Three-Live.** Every diagnostic is a record: a stable code, a severity, the
node, a line and column where there is one, and a hint. A misspelt trait
gets the nearest name (`TL201`). And what it downgrades instead of refusing
— a shader it replaces with a standard material, an unknown class it
replaces with a box — is reported (`TL101`, `TL102`, `TL105`), so a scene
that "worked" still says what it lost.

**For LocalGPT:** *adopted* where the text is ours.

- The lint's findings carry codes from one list, `lint::RULES`, never
  renamed; the package guide and the user documentation's table are held
  to that list by tests.
- *Carried, not run.* The format keeps what a reader does not know, which
  is right for reading and silent for authoring. A batch that brings a
  trigger whose host action this app does not have, an `animate` it cannot
  perform, or an extension it has no code for now says so on its receipt,
  once, on the batch that brought it (`live::carried_by`), each line ending
  with the code.
- A refused value of the wrong type now carries the pointer the format's
  live-authoring RFC asks for (`op 0: /ModifyEntity/patch/material/color:
  invalid type…`), as a field the format lacks always did.
- *Open, here:* the nearest field in an unknown-field refusal
  ([cad-lessons.md](cad-lessons.md) §3); `lint::nearest` is the edit
  distance, used so far only for extension names.
- *Open, in the spec repository:* codes on the format crate's own
  validation messages, which arrive here as prose.

We do not downgrade. An unknown class becomes a box in Three-Live because a
phone must draw *something*; an authority that keeps a history refuses, and
that is the better trade for a world other hands will build on.

## 5. Measuring whether authors succeed

**Three-Live.** `evals/` holds 100 prompts in five categories, and the
scripts two prompting arms produced for each — one told nothing about the
compiler, one given its authoring guide — committed as files
(`evals/scripts/{vanilla,guided}/`). Scoring them compiles them; no model
runs. A third pass feeds the diagnostics back for one repair turn and
counts how often the scene then compiles: a measure of whether a diagnostic
is *actionable*. No results are committed, so there is nothing to quote.

**For LocalGPT:** *adapted.* The whole product rests on an arbitrary agent
doing well with a package's `AGENTS.md` and a batch of ops, and nothing
measured it.

- `crates/world-agent/tests/corpus/` is batches as authors send them, each
  with the answer the authority must give, down to the words of a refusal
  that let an author fix it; `tests/corpus.rs` replays them through
  `LiveWorld::submit`. A case is a file, so the no-model rule holds.
- The same test sends every example in the package guide as written. An
  example the authority refuses teaches the refusal.
- The authority now keeps every batch it refuses, as sent, in
  `.live/rejected.jsonl`: the log shows what authors got right, and this is
  the half it never showed.
- *What it found the day it ran:* two refusals that did not say which field
  was wrong (fixed, §4), and an `ext-*` key nobody registered committing in
  silence — a misspelt `ext-cinematograpy` makes an entity that is not a
  camera. The receipt now names it and the extension that was meant.
- *Open:* the corpus is seeded by hand; `captured/` is empty until a real
  session's refusals are added (the README says how). A harness that
  *generates* batches by calling a model belongs outside this workspace, if
  anywhere.

**For openworldformat:** `spec/profiles.md` says a strict reader treats an
unregistered `ext-*` key as a typo; `rfcs/live-authoring.md` and the
reference authority admit any `ext-*` key on an entity. One of the two
should say what the other does.

## 6. A scene in a link

**Three-Live.** `link` puts the whole script in a URL's fragment — raw
DEFLATE, unpadded base64url, 128 KiB at most — which a browser never sends
to a server (`compiler/src/scene_link.rs`), and prints a QR code when the
link is under 1,150 characters.

**For LocalGPT:** *adopted.* `localgpt world link` writes
`https://localgpt.world/world.html#world=z:<payload>` with the manifest as
the payload, and the viewer page inflates it and draws it with no asset
base, so a linked world loads no file. A drafted previs scene is about
1,600 characters. A world that references files is refused, naming them.
This is the strategy's "shared links belong on localgpt.world" with nothing
uploaded, which matters for a scene from a script nobody has read yet.

- *Open:* the page is changed in the tree and checked by the site's render
  check; links open once localgpt.world is deployed.
- *Not taken:* the QR code. A scene's link is past the length Three-Live
  itself stops at.

## 7. Pictures in a tool's answer

**Three-Live's** render tool answers with MCP `image` content, so the
picture lands in the model's context in the call that made it.

**For LocalGPT:** *adopted.* `Tool::execute` returned a string, so every
screenshot was a path — an answer only to an agent that shares the server's
files, which the container `crates/gen/server.json` advertises does not. A
tool can now override `Tool::call` and hand back a `ToolOutput` with a
picture beside its text (`crates/world-agent/src/mcp/mod.rs`); the stdio
and HTTP servers and Gen's relay answer through one `tool_result`.
`gen_screenshot` and `world_screenshot` take `inline`. It is asked for, not
the default: a client that cannot take a three-megabyte result should not
lose the path with it.

## 8. "The one on the left"

**Three-Live.** A phrase such as "the two red chairs on the left" resolves
to mesh names with no model: names with plurals, colours, sides from the
current camera, size, ordinals, counts, "all … except …"
(`compiler/src/ai_select.rs`). When it cannot decide it asks and changes
nothing. Two implementations, Rust and TypeScript, are held to one fixture
file (`three-live/tests/fixtures/select_cases.json`).

**For LocalGPT:** *adapted.* `selection` already gives an agent what the
person clicked; `localgpt world find` and `world_find` give it what they
said (`crates/world-agent/src/find.rs`), with sides read from the person's
view — the open app's `/selection` now reports where they stand and look —
or through a named shot. It answers only with what the phrase picks out: a
phrase that could mean several is a question, and one it half understands
(a relation, a pointer, a word it has no rule for where the noun should
be) picks nothing and says which word. It is deterministic and calls
nothing, so it serves every agent alike.

- *Open:* relations ("behind", "next to"), the selection as "this", and
  any language but English come back as not understood.

## 9. Generated looks, when their time comes

**Three-Live.** A material may carry a prompt beside a preset:
`{preset: 'brick', prompt: 'weathered temple stone'}`. The compiler never
calls an image model. Pixels come from a *texture pack*, keyed by a hash of
the prompt, size and seed (`compiler/src/ai_texture.rs`); with no pack the
preset draws, so the scene is the same with no network.

**For LocalGPT:** *adapt, later.* Generated looks are out of scope until
the deterministic previs workflow is proven (`docs/world-strategy.md`). When
they are reopened, this is the shape that keeps the no-service rule: the
intent in the world, the pixels in content-addressed assets an agent's own
tool writes, a deterministic fallback when they are absent. It is the
sidecar rule again.

## 10. What does not transfer

- **The scene as code** (§2).
- **A model reached from the product**: `editScene`, voice, image-to-scene
  and texture baking all call an endpoint. Nothing here does.
- **Shims that swap what they do not know for something that draws** (§4).
- **The reference-picture comparison** (`compiler/src/reference.rs`):
  silhouette overlap against a photo. It needs the rasteriser of §3 and
  serves generated looks; neither is here yet.
- **The agent gym** (`compiler/src/gym.rs`): stepping a game to see whether
  a level can be won. A `.world`'s behaviors are functions of time, so
  "what does the frame hold at second four" is the lint at a clock, not an
  episode; there is no game to win in a previs scene.
- **Native phone and headset runtimes**: Three-Live's reason to exist, and
  out of scope here; the format's viewers on those platforms are their own
  project.
- **The 57 parametric parts** (gears, stairs, doors, pipes). They are
  TypeScript and a second shape vocabulary. An agent that wants one can run
  Three-Live's own command line and bring the `.glb` in as a mesh asset.

## The open part

1. A picture with no app: the CPU rasteriser, as a third mapping that must
   pass the geometry fixture, when a deliverable needs it (§3).
2. `gen_add_trigger`'s `spawn`: run it, or stop writing it (§3).
3. The nearest field in an unknown-field refusal (§4).
4. Real refusals in the corpus, from `.live/rejected.jsonl` (§5).
5. localgpt.world deployed with the page that opens a linked world (§6).
6. In the spec repository: whether an unregistered `ext-*` key on an entity
   is refused, and codes on the validator's messages (§4, §5).
7. `find`'s relations and "this" (§8).
8. What the lint cannot see: a mesh asset's size, where a visitor's avatar
   starts, a world with no light on it.
9. The gallery's temple pool: lay its two rims flat (§3).
