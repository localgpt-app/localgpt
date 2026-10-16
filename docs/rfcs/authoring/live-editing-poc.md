# Live editing — proof of concept

**Branch:** `poc/live-editing`. **Status:** a proof of concept. The
decisions it fed are now normative — spec draft 0.3, the accepted
live-authoring RFC in the spec repository (`spec/rfcs/live-authoring.md`,
branch `rfc/live-authoring`). The code here predates them; see
*Catching up to draft 0.3*. What it did not answer — where the person
types once Gen has no agent of its own — is
[agents-outside-the-window.md](agents-outside-the-window.md).

The question: can Gen be the *canvas* of a `.world` that agents change from
outside — Claude Code, Codex, `localgpt`, a script — with no agent
conversation inside the app? The pattern comes from an image editor whose
projects are folders that agents edit while the canvas updates. A world
adds what that editor lacks: every change is a validated, attributed,
undoable entry in the log.

## The rules the proof of concept enforces

1. **The world changes only through ops.** An agent sends a batch of edit
   ops to the open app's API. The batch commits whole, or not at all with
   a reason per op.
2. **Assets are files.** Agents write meshes and textures into `assets/`
   under any name. An op that references one stores an immutable copy at
   `assets/<sha256>.<ext>` and points the world there — content
   addressing that keeps the `.world` a self-contained, portable
   database even on devices without git (mobile included). A new version
   is new bytes referenced again; old versions stay for the history that
   used them.
3. **The package is head-first.** `manifest.json` is the world now,
   written only by the committer (a direct write is put back).
   `fold(base, log) == manifest.json` is checkable at any time.
4. **In a git repository, every batch is a commit** authored by whoever
   sent it, and replay walks the commits as keyframes.

Where the code differs from the draft-0.3 decisions below, the decisions
win and the catch-up list says so.

## Run it

```sh
# a world folder from any manifest
mkdir demo.world && cp ../openworldformat/examples/hello-world/manifest.json demo.world/
cargo run -p localgpt-world-agent --example live -- init demo.world --git

# the canvas: no agent in the window; the API is in demo.world/.live/endpoint.json
cargo run -p localgpt-gen -- --live demo.world

# from any terminal or agent (demo.world/AGENTS.md says the same)
URL=$(jq -r .url demo.world/.live/endpoint.json); TOKEN=$(jq -r .token demo.world/.live/endpoint.json)
curl -s -X POST "$URL/ops" -H "Authorization: Bearer $TOKEN" -d '{"author": "claude",
  "message": "lift the cube", "ops": [{"ModifyEntity": {"id": "cuboid",
  "patch": {"transform": {"position": [0.0, 2.0, 0.0]}}}}]}'
curl -s "$URL/screenshot" -H "Authorization: Bearer $TOKEN"     # → a PNG path
curl -s -X POST "$URL/replay?seconds=1" -H "Authorization: Bearer $TOKEN"
```

Without the app, the `live` example is the authority: `submit`, `undo`,
`log`, `verify`, `history`. It refuses to write while an app answers at the
folder's endpoint.

## Where the code is

| | |
|---|---|
| `crates/world-agent/src/live.rs` | the authority: ingestion (names → ids, new ids, struct patches merged, strict fields, assets by hash), all-or-nothing commit, undo as an appended inverse, the manifest guard, `verify`, git commits and history |
| `crates/gen/src/gen3d/live.rs` | the canvas: `--live`, the localhost API (token in `.live/endpoint.json`), scene updates through `OpsApplier`, previews, screenshots, selection, git replay |
| `crates/world-agent/examples/live.rs` | the headless authority |

## What it showed

Run against `hello-world` with an agent working only through files and the
API:

- A batch by name — environment, a spawn, a move, a delete — applied
  live and committed as revision 1; the reply listed the new entity's id.
- A batch with a misspelled field (`colour`) and an unknown parent name was
  refused whole with one JSON-pointer reason per op; the valid op in it was
  not applied, and `manifest.json` and the log were byte-identical after.
- A texture written as `brick.png`, referenced, rewritten and referenced
  again became two stored versions; undo restored the first, which only
  works because stored versions never change.
- A direct write to `manifest.json` was put back within half a second.
- `git log` read as the change history (authors `claude`, `yi`, `codex`),
  `git diff` of `manifest.json` read like a review, and replay showed the
  base, every commit, and returned to the head — from `git show
  <commit>:manifest.json` alone, no fold.
- `verify` held after every step: the fold of the log equals the manifest,
  and every asset matches its hash.

## What it found for the format

- **Patches replace whole structs.** `ModifyEntity` with only a `position`
  resets rotation and scale; a material patch with only a texture drops
  the color. Agents send partial changes by nature. The proof of concept
  merges `transform`, `material`, `light` and `SetEnvironment` at
  ingestion (JSON Merge Patch, RFC 7396). *Decided:* the spec adopts that
  merge for the Authoring profile, at ingestion — the committed op
  carries the merged whole value, so the fold itself never merges and
  the log's patch semantics are unchanged.
- **No op reaches meta, avatar, tours, soundtrack or creations.** Through
  an ops-only API an agent cannot change them at all. *Decided:* one op —
  `ModifyWorld`, patching those fields the way `ModifyEntity` patches an
  entity — not per-field ops.
- **Names at ingestion are right.** Binding names to ids (and allocating
  ids for spawns) at ingestion is what made the ops writable by an agent —
  the spec's rule, validated, and extended to op addresses: a string
  where an entity id goes is a name.
- **Strict mode belongs to the authoring API.** Must-ignore would have
  kept the `colour` typo silently; refusing it with a pointer is what
  lets an agent correct itself. *Decided:* the Authoring profile MUST
  read the batches it is sent strictly.
- **Portability first; git as an extension.** Platforms like mobile have
  no git, so the package must be fully self-contained: hash-named asset
  copies plus the append-only ops log are a portable, dependency-free
  history — a `.world` is its own database anywhere. Git stays strictly
  optional, an optimization for desktop workflows (batches as commits,
  keyframe replay), never a mandatory dependency. *Decided:* content
  addressing is the baseline everywhere. Even under git the immutable
  copies cost almost nothing — identical bytes are one git blob, so only
  genuinely new versions add to the object store, which any history
  would. The spec's allowance of logical names under git (`MAY`,
  spec/package.md) is there for repositories that want readable asset
  diffs; it is not this project's default.
- **What git takes over, and what it doesn't.** When enabled, commits
  give history, authorship, diff, revert, branches and transport for
  free, and replay becomes walking keyframes. It does not give op-level
  intent (the log still does), real-time collaboration, or a semantic
  merge. *Decided:* deterministic serialization first — the canonical
  text (members sorted, entities by id, plain arrays inline) makes
  ordinary textual merges of `manifest.json` clean; a semantic merge
  driver stays open (`ops.jsonl` uses `merge=union` either way).

## Decided since (spec draft 0.3)

Beyond the findings above, from the review of this proof of concept:

- **A checkout is not a direct write.** When `manifest.json` changes to
  bytes `package.json` names (`world_sha256`), something moved the whole
  package — a `git checkout`, a pull, a sync client — and the authority
  reopens it and shows the new head. Any other change is still refused.
  Reloading on *any* outside change would quietly accept direct writes
  again; the hash match is the line.
- **A read-only `manifest.json` is a hint, not a guard.** An authority
  MAY clear the file's write permission while it holds the package, so
  an ordinary write fails at once with the OS's own error. A writer that
  saves by renaming a temp file over the target — or that makes the file
  writable again — gets through it, so the authority still checks every
  change and refuses what isn't a checkout.
- **Entries carry a `message`** — a commit message, part of the entry's
  identity — instead of this proof of concept's tool-record workaround.
- **MCP is an equal way in, not a replacement.** A stdio MCP server is
  started by the agent, so it is a shim that reads
  `.live/endpoint.json` and calls the same local HTTP API; an MCP server
  over HTTP on localhost needs the token for the same reason the API
  does — a web page in the browser can post to localhost.

## Catching up to draft 0.3

The code here still builds on the 0.1 crate and predates the decisions:

- move to openworldformat 0.3, and send `ModifyWorld` through the API;
- carry the batch's message as the entry's `message` field;
- write `manifest.json` with the crate's `manifest_text` (the canonical
  text) instead of preserving the previous entity order;
- reopen on a checkout — bytes matching `world_sha256` — instead of
  restoring, and make `manifest.json` read-only while held, as the hint;
- keep hash-named asset copies everywhere (portability first); git
  stays the optional layer it is here.

## What a person edits in the canvas (2026-10-15)

The canvas grew the editing surfaces an editor is judged by, each one a
batch through the same authority the agents use — one history, one
rule. The comparison that drove it was DeepMonkey Studio's editor
(property panel, scene tree, gizmos, transaction receipts with
revisions, previewable AI edits); what was taken is the mechanism, not
the product.

- **The detail panel edits** (F1 twice, or `LOCALGPT_INSPECTOR=full`
  to open it at start): name, position, rotation, scale, visibility,
  material (color, metallic, roughness, reflectance, emissive, alpha
  mode, double-sided, unlit), lights (color, intensity, range, spot
  angles, direction, shadows), the parent (a combo that keeps the world
  placement), a shape's parameters and a behavior's parameters (the
  document's JSON, field by field, so a new variant edits without a new
  widget; a target is a combo of entity names), and a behavior's
  removal. A held slider changes
  the Bevy component so the person sees it; when the pointer is up and
  nothing is being typed, the fields that moved go out as one
  `ModifyEntity` merge patch, and a refusal puts the entity back from the
  document. The values shown are the document's, so angles keep the form
  their author gave them.
- **The outliner edits**: shift-click adds to the selection; a row
  dragged onto another hangs under it (onto the zone at the bottom: the
  root), keeping its world placement; right-click offers duplicate,
  delete, unparent, group and focus.
- **Handles and snapping**: three axis arrows, a ring about each axis
  and a cube for uniform scale, around the primary selection. `Y`
  toggles snapping to the grid and the angle step (defaults 0.25 m,
  15°; the world bar sets them), and Cmd/Ctrl held during a drag means
  the opposite. `L` lays the handles along the entity's own axes instead
  of the world's (the bar has the same switch); an Alt-drag of an arrow
  scales along that one axis of the entity's own; a drag from empty
  space, or a Shift-drag from anywhere, box-selects what the box encloses
  whole; and Alt + right-drag orbits the camera around the selection.
- **A multi-selection** moves, turns, scales, deletes and duplicates
  together, as one entry; Cmd/Ctrl + G groups it under a new empty at
  its centroid (siblings only).
- **A behavior whose target is gone** is marked in the panel with a
  retarget combo and a one-click removal, rather than silently looking
  at nothing; the authority's receipt warns the batch that caused it, and
  the format crate's validation (0.3.6) reports it to inspectors.
- **Review of agent edits** (`--review`, the world bar's checkbox, or
  `LOCALGPT_REVIEW=1`): `POST /ops` replies `proposed` (202) and the
  batch shows on the canvas, read against the world on screen without
  being written; Enter or the bar's button accepts it (then it goes
  through the authority as it would have), Esc rejects it (the canvas is
  put back from the document). `GET /proposals` reports each one's
  status; a second batch, an undo, a seek or a person's edit waits for
  the decision.
- **A held drag blocks a seek**: the rail refuses to move through
  history while an entity is where the pointer has it rather than where
  the document says, so a gesture can't land on the wrong entry.
- **Camera bookmarks**: the world bar's ★ menu saves the view as a
  waypoint of a `bookmarks` tour in the document (a `ModifyWorld`), so
  they travel with the world; each is a click away, or dropped.
- **Receipts and revisions** (spec draft 0.3, Authoring): the reply to a
  batch is the author's receipt — `revision`, `id`, `spawned`,
  `warnings`, or `errors` and nothing changed — and a batch may carry
  `expected_revision`, the revision its author read; a world that moved
  since refuses it, naming both, so a stale edit re-reads instead of
  landing.

A smoke test drives all of this with no one at the keyboard —
`scripts/live-smoke.py <world>` launches the canvas with the inspector
open, selects over the inspector's WebSocket, and with a synthesized hand
(`LOCALGPT_SYNTHETIC_INPUT=1` enables `POST /input`, a test's seam that
is off otherwise so an agent can't press Enter on its own proposal)
rejects a proposal with Esc and accepts one with Enter, clicks the bar's
Reject and Accept buttons, drags the X handle of an entity
(`GET /selection` reports where the handles are, and which is under the
pointer), turns it with the comma key, pulls a ring to turn it about
an axis, snaps a drag onto the grid, drags
an outliner row onto another, frees it from the row's context menu,
picks a parent and an alpha mode from the panel's combos and drags a
value field (`GET /widgets` names every panel widget's rectangle),
shift-clicks a second entity and drags both, groups them with Cmd+G and
deletes the group, and measures what a hand would feel — the handles
keep their screen size as the camera moves, and hover forgives a few
pixels off an arrow but not many; it checks the log stayed untouched while a proposal
was pending, that a stale `expected_revision` is refused and the right
one commits, and that the receipt warns about a deleted target. Two
things it caught: a click on a panel reached the scene behind it (egui's
own hover test skips the legacy context panels drawn here, so hover is
now decided from the panels' rectangles), and the outliner rebuilt its
tree only when the entity count changed, so a reparent left a stale row.
The first pass with a real mouse (2026-10-15, after the hand) found two
more: Shift was also the fly camera's descend key, so a Shift-click landed
on a moving scene and never counted as a click (descend is Q now, and
Cmd/Ctrl-click adds to the selection too), and the rings — grown from one
to three, one about each axis — crossed every arrow at their own radius
and took the grab there (they sit outside the arrows now). The screenshots
it takes land in `<world>/.live/`; on a locked or sleeping screen a
window's surface stays black, and `--offscreen`
(`LOCALGPT_OFFSCREEN_SHOTS=1`) has the canvas render into an image the
screenshots read instead. The same mode keeps a run clear of a real mouse
crossing the window, which a smoke run beside a person's window needs,
and `LOCALGPT_INSPECTOR_PORT` gives that second canvas its own inspector
socket.

## Since then (2026-10-16)

The list below was the previous "Not done". Each item is in, verified by
the authority's unit tests and the smoke run (`scripts/live-smoke.py` on a
`--git` world: 70-odd checks).

- **Redo, and per-author undo.** `undo_scoped` / `redo_at` on the
  authority take an `UndoScope`: anyone's newest on the path, or the
  author's own newest, skipping others'. Undo and redo entries are
  markers (`via: undo|redo`, `of: <revision>`), replayed in path order so
  an undo, a redo and an undo again come out as they went; a batch after
  an undo ends the redo, as in any editor. An own undo of a batch that a
  later live batch built on (the same entities) is refused naming that
  entry — "take that back first" — never a silent clobber. Undo and redo
  are per path: an undo made at a branch's tip is redone at that tip, and
  main's own undo and redo are untouched by the branch's — a unit test
  holds both, and the smoke does it live over the API, reading the branch
  with `GET /world?tip=<id>` before and after. In the canvas Cmd+Z is the person's own newest, Cmd+Shift+Z
  their redo, Cmd+Alt+Z anyone's newest; the API's `POST /undo` takes
  `own`, and `POST /redo` exists.
- **Telling the agent when its direct write was refused.** The guard
  still puts `manifest.json` back; now it also appends a line to
  `.live/refusals.jsonl` (what, why, the revision the world stands at),
  `GET /refusals` returns them, the bottom bar shows the refusal, and
  `AGENTS.md` says where to look when a change vanished. A fresh
  package's first normalization is not a refusal and is not noted.
- **Switching git branches under an open app.** Every entry is a commit,
  so the authority remembers the commit it last made or read and asks
  `.git/HEAD` (through its ref; git only for a packed ref) each guard
  tick whether that moved. When it has, the canvas reopens the package
  from disk, rebuilds the view at main's tip and applies the delta from
  what it showed to the new head — before the guard runs, so the old
  head is never written back over the checkout. A drag, a pending
  proposal or a replay defers it a tick. The smoke checks out `HEAD~1`
  and back under the open window.
- **Replay reading assets at the replayed commit.** `assets_at(dir,
  commit, into)` materializes a commit's `assets/` under
  `.live/replay/<commit>/` from `git ls-tree` + `cat-file`, each blob
  cached once by hash and hard-linked (copied on a filesystem that
  can't), and the replay resolves each frame's ops against that root.
  The roots are fetched on a thread when the replay starts, ahead of
  playback; a frame waits for its own only when playback outruns git.
  Live, the smoke spawns a swatch with a red texture, overwrites the
  stored copy with blue bytes by hand and commits that, replays, and
  samples the screenshot where the swatch is: the committed red, with
  the materialized root holding the red bytes while the working tree
  holds blue.
- **Mesh, audio and camera changes in the open scene.** `OpsApplier`
  now applies an entity's `audio` (an emitter named after the entity,
  attached to it; `null` removes it), `mesh_asset` (what the entity drew
  goes, and the glTF scene fills it in place through the same pending
  load a world load uses — `null` leaves an empty), and `SetAmbience` /
  `SpawnAudioEmitter` / `RemoveAudioEmitter` through the in-app undo
  stack's path. `SetCamera` and a world patch's `camera` move the live
  canvas's own camera on the next frame (`note_camera`), while the
  applier itself still leaves cameras alone for guests. A patched
  `modulations` list rebuilds the entity's modulation component the way
  a load builds it, or removes it when nothing valid is left. Live, the
  smoke commits an `offset_y` modulation on a free-running oscillator to
  the torus and watches its place on screen travel some 50 px where it
  stood still before; the checkout check then takes that commit away and
  watches it stand still again. Getting that measurement right taught the
  API something: a `GlobalTransform` read in `Update` (what the handles
  and the first version of the check used) is not the pose on screen,
  because the physics schedule runs transform propagation itself each
  fixed step, from the pose the modulation restore left — so on any frame
  with a step it is the authored pose, and a slower canvas, with a step
  every frame, read the modulation as absent while it showed. The
  selection reply now carries `scene.rendered`, recorded after the
  frame's last propagation, and that is what the check reads.
- **The canvas's editor follow-ups** (same day): `L` lays the handles
  along the entity's own axes, an Alt-drag of an arrow scales along that
  axis alone, a drag from empty space or a Shift-drag from anywhere
  box-selects what it encloses whole, and Alt + right-drag orbits the
  camera around the selection. The box reads the button and picking's
  hover map rather than picking's events, because picking sends none for
  a press on the sky when the canvas renders into an image.
- **A cold test with a fresh agent that has only `AGENTS.md`.** Run on
  2026-10-16: a fresh agent was given the world's path and told to learn
  the API from the guide alone, then asked to add a bench by the cuboid,
  make the lantern look at it, take a screenshot, undo, look at the base
  and come back, and list its entries. It did all six in 21 calls, every
  one HTTP 200 — and its report found one real defect and a dozen gaps.
  The defect: a behavior target sent by name (`LookAt: {target:
  "bench"}`) committed as the name, the fold bound it, and `GET /verify`
  said the lantern "differs from the history" while that entry was the
  head. The authority's intake bound only an op's `id` and `parent`; it
  now runs the format crate's `bind_committed_refs` over every op, so
  the log carries ids in every marked field (behavior targets and
  centers, a camera's aim and focus, the avatar's model) and a name
  nothing has refuses the batch, naming it. The gaps, all in the guide,
  all closed: no word on behaviors (the agent guessed the shape
  correctly), on which fields take a name, on the receipt's full shape,
  on `/undo` taking an `author` (its undo landed as "you") and
  replying a receipt, on how to read the state the window shows after a
  `goto` (`GET /world` and `manifest.json` answer from the head; now
  `GET /world?tip=current` and `goto {"tip": "head"}` exist and are
  documented), on `/tips.revision` being the view's while
  `/log.revision` is the head's, on the screenshot being the person's
  camera with no framing from the API (`at_head` documented), on
  shape origins (centered, every shape), on `/world` and `/verify`
  existing at all, and a curl example that assumed the shell sat in
  the folder. `GET /world` also printed f32 fields through their f64
  widening (`0.550000011920929`); it goes through text now and prints
  what `manifest.json` prints. The smoke checks the binding, the
  refusal and the numbers. Two of the agent's suggestions went further
  than the guide: the committer now proves, before it writes, that the
  entry as it will be logged folds (by the strict fold every reader
  uses) to the world it made in memory — the class of bug the agent
  found — and refuses, naming the entity, if not; the format gained an
  authoring outcome suite (`conformance/authoring/cases.json`, nine
  batches over a base world with what must come back), which the
  reference crate runs over its own ingest and this authority runs over
  a vendored copy in CI, so the two cannot drift over what a batch
  commits; and the cold run is the standing check after a guide change. Run again against the revised
  guide with a different task mix (a stale `expected_revision`, a branch
  and its state, an orbit by name, a direct write, the undo's author),
  the second agent did all eight tasks in 27 calls with no guess that
  went wrong — and found the deepest bug of the day, in the canvas, not
  the guide: its moon, committed (correctly, now) with the lantern's id
  as its orbit center, circled the world origin on screen. The behavior
  tick knew its targets by name only, while every committed reference
  converts to an id key, and entities without behaviors of their own
  were not in its map at all; so every id-bound orbit in every world
  loaded from disk circled the origin, and every id-bound look-at did
  nothing. Fixed by registering every entity under both keys. Its guide
  stumbles were smaller and all closed: `"at": null` was taken as the
  head — a batch meant for the base would have landed on main — and is
  refused now once there is history; `GET /world?tip=<id>` silently
  returned the head and now folds any entry (unknown: 404); the log
  entry's `message` sits at the top level as the guide said; the undo
  receipt carries the batch receipt's fields; and the guide says how
  revisions number across branches, that refusals are 422, that the
  refusals file appears on the first refusal, that entries are one git
  line with the tree in `ops.jsonl`, and what a behavior looks like read
  back.

Nothing on the proof of concept's list remains open.
