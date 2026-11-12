# The stack for a non-linear `.world` editor

**Status:** partly implemented. Built: the headless view
(`crates/world-editor`), one authority that forks (`world-agent`'s
`LiveWorld`), and, on a live canvas, the one-direction path from ops to the
scene, a person's edits included. A live canvas is what
`localgpt-app --world` and `localgpt-gen --live` open. Not built: the same
path for the tools in Gen's own window, which still mutate the scene and
are projected into ops in a hosted room; switching the open world at run
time; history kept in the package beyond the log
([What remains](#what-remains)). The desktop chrome is the native
eframe/egui shell described in [all-rust.md](all-rust.md). The rule that the
ECS is a projection of a fold stands: this document is about what the editor
is built out of when **history is the product** — fork at any entry, scrub
as a tree, fold any tip.

## What is already decided, and must not be re-litigated

Most of the hard architecture questions a non-linear editor usually opens are
already answered by the format, and the single biggest risk in a greenfield
pass is re-solving them worse.

| Question an NLE normally agonizes over | Already answered |
|---|---|
| How is history represented? | Append-only op log; entries carry `id` and `parent`; a branch is an entry whose parent already has a child; a tip is nobody's parent |
| What is the state at a point in time? | `fold_path(base, entries, tip)` — O(path), and branches share prefixes |
| How does undo work? | An appended inverse entry, not a pop |
| Merge / concurrent edits? | **No CRDT.** Forks are explicit; `merge` is a provenance-only op kind; per-entity field patches are the conflict unit, which is the same unit the multiplayer authority already linearizes. The crate's `merge_branch` implements the merge rules; nothing in the workspace calls it yet |
| Naming across history | Content-addressed entries dedupe across forks; assets are `assets/<sha256>.<ext>` and immutable |
| Physics under replay | Settled in `ext-physics`: lockstep determinism is refused as "a lie"; trajectories are **recorded** and playback "scrubs recorded motion instead of simulating it" |

Two consequences worth stating plainly, because they remove the two classic
time-travel bugs by construction:

- **Scrubbing can never show the wrong asset bytes.** A fold at any tip names
  exact hashes, and the canvas loads those files from the package's
  `assets/`, so an old tip loads the bytes that tip referenced. A replay of
  a package's git history resolves each frame against that commit's own
  `assets/` (`live::assets_at`).
- **Scrubbing can never diverge from physics.** Because trajectories are
  data, no solver has to reproduce anything, and Avian's
  non-determinism across platforms stops being the editor's problem.

The spec also says where the remaining work is: *"The new cost is
visualizing a large tree — a UI concern (the branch rail), not a format
concern."* That is the stack question.

## The one decision that matters: the ECS is a projection

In a hosted room (`localgpt-gen --host`) Gen's scene is still a co-author of
the document. It projects scene state into ops four times a second
(`net/web.rs`), and `gen3d/ops_apply.rs` carries the tell: a host must apply
authority-originated ops *"or its next projection would 'heal' the document
back to the pre-undo scene."* That loop is survivable for a live room. It is
**fatal for non-linear editing**, because the moment the scene can author
the document, scrubbing to another tip races a projection that is busy
writing the old tip back.

So the rule the whole stack hangs from:

> **Ops → document → fold → diff → ECS. One direction. Always.**
> A gizmo drag emits an op; it never mutates a `Transform` and lets a
> projection notice later.

Everything good follows from it. Undo is an entry. Scrub is a fold. An
agent's batch and a human's drag are the same kind of event. The log is
complete by construction rather than by a 4 Hz sampler being lucky. And the
editor's correctness becomes testable without a GPU.

A live canvas keeps the rule, and bends it in one stated place
(`gen3d/canvas_edit.rs`): while a drag or a run of key presses is in
progress the entity's `Transform` moves directly, so the person sees it
follow; when the gesture ends its result is one batch through the authority,
and the canvas applies what was committed or, on a refusal, puts the entity
back from the document. The document has the last word either way. No world
has both the live authority and the projection.

## The layers

```
openworldformat (crates.io)     document, EditOp, OpLogEntry, fold_path,
                                fold_state, fold_trajectories, refs
        ↓
authority                       world-agent's LiveWorld: the package, ingest,
         no Bevy                commit, undo and redo, forks, the guard
        ↓  (hands each committed entry to the view)
history store + editor view     world-editor: History, FoldCache, Editor;
         no Bevy, no I/O        a seek or a followed commit is a Delta
        ↓  (the delta is ops)
reconciler                      gen3d/live.rs wires authority, view and
         Bevy system            scene; gen3d/ops_apply.rs patches the ECS
        ↓
viewport                        gen3d/canvas_edit.rs: bevy picking, gizmo
         Bevy                   handles, one batch per gesture
chrome                          crates/app/src/shell: history, outliner,
         egui on eframe         inspector, timeline, document, terminals
        ↕
agent surface                   the live ops API, `localgpt world`, the MCP
                                shim; plain terminals in the app
```

**The authority and the view have no Bevy, and that is the point.**
`worldprobe` is egui, `worldwalk` is SwiftUI/RealityKit and Compose, and the
web viewer is three.js — four renderers over one format. A Bevy-free core
means the non-linear editing model (tips, forks, seeks, deltas, selection)
and the commit are written once and tested headless in CI, and the Bevy
layer is the part that can be wrong without being incorrect.

**A seek is a diff-and-patch, never a rebuild.** Scrubbing must not despawn
and respawn a world per frame — React's reconciliation for a scene graph.
`Editor::goto` returns the delta between the document on screen and the one
at the tip (`delta_between`, which reuses `world-sync`'s `diff_entities` and
adds the scene-wide fields), and `OpsApplier` patches the ECS with it. A
commit moves the scene the same way, by the view's delta and not by the
batch's own ops, which differ once the canvas has been scrubbed back
(`follow` in `gen3d/live.rs`). ECS entities stay keyed to world ids across a
re-fold through `NameRegistry` and `GenEntity`.

**Seek cost needs keyframes.** Folding is O(path), so a timeline scrub over a
long session is O(n) per frame without caching. `FoldCache` keeps snapshots
on a cadence, at fork points and at the tip, so a nearby scrub is a short
diff and a distant jump is bounded.

## Chrome: the native all-Rust shell

The desktop interface is eframe and egui, implemented in
`crates/app/src/shell`; Bevy renders the world into a texture in that shell.
[all-rust.md](all-rust.md) records the current stack, its terminals and its
remaining work. The editor-specific lessons that still apply are in
[blender-lessons.md](blender-lessons.md): operators, modal keymaps,
region-scoped input, the outliner's drop zones, and the separation between
authored and evaluated state.

Focus is explicit: typing in a panel or terminal does not move the world,
and world input does not leak into a panel.

The history graph and timeline are native panels over the same folds and
deltas described here. They are not a second history implementation. A
remaining terminal or panel feature must preserve that rule and use the live
authority for edits, rather than writing beside it.

## Built, and what it found

`crates/world-editor` (`localgpt-world-editor`) is the non-linear half, with
no Bevy and no I/O: `History` (the branch index), `FoldCache` (folds that
scrub), `delta_between` (what changed, as ordered ops) and `Editor` (tip,
document, selection, seek). `world-agent`'s `LiveWorld` is the authority.
What building them settled:

**One authority, one view.** `LiveWorld` keeps everything that touches the
package — ingestion, committing a batch whole or refusing it, undo and redo
as appended inverses, forks, the `manifest.json` guard, git, `verify` —
because it is what the live API, `localgpt world` and the app go through.
`world-editor` briefly had a `submit` and an `undo` of its own. Two
implementations of submit and undo is one too many. They went: the view
never appends. The authority commits and hands the entry to
`Editor::observe` (or `Editor::committed`); the view follows and says what
changed. `world-agent/tests/authority_and_view.rs` pins that the two never
disagree, on linear and on branched histories.

**Editing while scrubbed back is a fork.** Nothing in the model needed a
special case for "editing in the middle of history": an entry's parent is
the entry it builds on. A person's edit builds on the entry on screen, and
an agent's batch on the head unless it names `"at"`. So an edit after a
seek branches rather than rewrites, and the trunk's tip survives as a
second tip. That falls out of the format; it did not have to be built.

**A person's edit is a batch.** On a live canvas a gesture (move, turn,
scale, delete, duplicate, group), a panel's field and a timeline edit each
become one batch through `LiveWorld`, committed at the entry on screen. One
gesture is one entry, a multi-selection included (`gen3d/canvas_edit.rs`;
the panels are [all-rust.md](all-rust.md)'s).

**Fork points must be snapshotted, or sibling seeks pay for the trunk.** The
cadence rule alone is not enough. Seeking from one branch to its sibling
finds no cached document on the new path — the branch you came from is not
on it — so it re-folds from the base. Pinning every entry with more than one
child fixes it, because those are exactly the entries two branches share: a
50-entry branch beside a 5,000-entry trunk costs 50 applies to cross
instead of 5,050 (`crates/world-editor/tests/scrub_budget.rs`). This is a
cache rule the format's branching RFC does not imply and a reader would not
guess.

**The fold is linear, and the cache is still what makes a seek cheap.**
Building the crate found the format's fold quadratic, three O(n) copies per
entry, and that was fixed upstream in `openworldformat` 0.3.1: `fold_log`
and `FoldCache::doc_at` apply an entry in place (`apply_entry_in_place`),
because a fold owns its document and discards it on error. A linear fold
still costs the whole path, so the cache keeps a seek proportional to the
distance moved: snapshots every 128 entries and at fork points, 256 held,
ad-hoc ones evicted before pinned ones and the newest never. The
measurements are in `crates/world-editor/src/fold.rs`; `scrub_budget.rs`
holds the budget on a 10,000-entry session by counting entries applied, not
time.

**The head is the fold of a path, never of every line.** `manifest.json` is
the fold of the path to main's tip (`fold_head`, over the crate's
`fold_path` and `main_tip`), and undo takes back the newest batch on that
path. Folding the file in order is the same thing on a linear log, and on a
branched one a world that never existed. The tests that claimed authority
and view agree had only tried linear histories; branched fixtures are part
of them now.

**Every entry the authority writes has an identity**: its content hash as
`id` (`compute_entry_id`), the `parent` it builds on (none for the first),
and the author's `message` when there is one. A ref never names a
synthesized `line-<n>`.

**The authority rewrites `package.json` on every commit, so it keeps what it
does not know.** `SessionMeta` carries `refs` and an `extra` catch-all for
that; a rewrite without them destroys `refs.main`.

## Forking through the authority

The authority forks. A batch names the entry it builds on the way it names
its author — `"at"` in the batch, `?at=` on `POST /ops`, `--at` on
`localgpt world submit` — and leaves it out to build on the head. An earlier
entry starts a branch; a branch's tip extends it. Decisions, each pinned by
a test in `world-agent/tests/authority_and_view.rs`:

- **A branch leaves the head alone.** It is ingested against the fold of its
  own path, appended with `at` as its parent, and `manifest.json` keeps its
  bytes: it is main's, and main did not move. The git commit (in a git
  package) says `off main — builds on <id>`, and replay skips it because the
  manifest did not change.
- **`refs.main` is written before the first branch entry,** naming main's
  tip. `ops.jsonl` is append-only, so a branch entry always lands on the
  last line, and under the head rule's "else the log's last entry" fallback
  that would silently make the branch `main`. Once named, the ref moves
  with every commit on main, and every reader (the authority on reopen,
  `localgpt world tips`, the canvas's view) takes the head from the crate's
  `main_tip`.
- **A commit on main builds on main's tip, not on the last line.** They stop
  being the same entry the moment a branch lands.
- **Revisions stay one total order across branches** — one past the highest
  anywhere, not past the head's — because undo names what it undid by
  revision, and a reused number would make it take back the wrong entry.
- **Entity ids are never minted twice, on any branch.** The next id is above
  every spawn in the log, not just the head's, so two branches of one
  authority never collide and a later merge has nothing to reallocate.
- **A parent or a ref never names a line number.** A fork from an entry
  written before ids, or off a main whose tip is one, is refused with the way
  out (commit once on main, then branch), rather than written wrong.

The view opens where the package says the head is (`Editor::open_at`, given
`LiveWorld::main_tip`), because the last line may be a branch's. And it
follows a commit only when the commit continues the entry on screen
(`Editor::observe`) — a terminal's rule for output: keep up only if you were
already at the end. An agent working on main does not yank a person off a
branch they chose; the history grows a row instead. The reply says
`on_screen`, because an agent that commits and then screenshots would
otherwise be looking at a picture without its change, and the screenshot's
own reply names the entry it shows.

The same moves reach every surface: `GET /tips` and `POST /goto` on the live
API, `localgpt world tips` and `goto`, `world_tips` and `world_goto` in the
MCP shim, and the history panel, which seeks through the path `POST /goto`
takes (`seek` in `gen3d/live.rs`).

## Decided: the package is the only document

**No backwards compatibility** (2026-10-04). `world.ron` stops being a
document format; an openworldformat package is the one the desktop app opens,
edits and saves. It is a sequence, not a commit, and it starts by making the
package the document rather than by deleting the old one: once the app opens
and edits packages, `world.ron` falls out unreferenced and removing it is
mechanical.

Done: `localgpt-app --world <package>` — and a bare relaunch of a remembered
package — opens the folder as a live canvas: the authority, its ops API and
the canvas, with no agent in the window, because the outside agents are the
editors (`localgpt world`, a terminal, `localgpt world mcp`).

Not done: `world.ron` is still what Gen's saved worlds, the app's document
and song modes, Verse and a hosted room's session directory write. As of
2026-11-11, 89 lines across seven crates name it, most of them in
`gen3d/world.rs` (30), `gen3d/world_import.rs` (11) and `gen3d/plugin.rs`
(11).

## What remains

1. **The tools in Gen's own window.** Under `--mcp-relay`, `mcp-server` and
   `--host`, Gen's tools mutate the ECS and save `world.ron`, and a hosted
   room projects the scene into ops. The rule's other half is those tools
   emitting ops through the authority, with the scene→ops projection deleted
   in the same change — they cannot both exist. The projection also goes if
   `--host` gives way to rooms over a live canvas
   ([rooms-and-the-channel.md](rooms-and-the-channel.md)).
2. **Switching worlds at run time.** A package opens at launch: the
   authority, its API server and the canvas systems are built with the app,
   and Open… answers a package with the command that launches it
   (`crates/app/src/open.rs`).
3. **History in the package.** The fold cache is in-process and rebuilt on
   open; the log is the only store. Entries have content-hash ids, but
   nothing keeps them, or snapshots, as objects of their own.
4. **Thumbnails in the history.** The history panel
   (`crates/app/src/shell/history.rs`) draws rows and a lane per branch; an
   entry has no picture.

## Not decided

- Whether the editor view lives in this repository or in the spec repository
  beside the reference layer. It is not format semantics, so probably here,
  where it is — but `worldprobe` and `worldwalk` would both consume it, which
  is the argument for there.
- Whether the fold cache's snapshots are persisted in the package (the spec
  names `snapshots/entry-<id>.json`; its branching RFC sketches a
  `history/<sha>` entry store) or stay an in-process cache. The spec calls
  them derived and does not require them. Hashing a snapshot over the folded
  document would also let two branches that reach the same state confirm it
  without replaying, which is git's tree-equality trick.
- Where state (`fold_state`) and recorded trajectories appear in the
  timeline. They fold separately from the document, so the history may need
  more than one lane. Nothing in the workspace reads either yet.
