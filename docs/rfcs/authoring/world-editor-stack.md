# The stack for a non-linear `.world` editor

**Status:** proposal, partly built — the headless core is
`crates/world-editor` (see [Built, and what it found](#built-and-what-it-found));
the chrome and the reconciler are not. Greenfield: it assumes no existing
choice in `crates/gen` is load-bearing. Companion to
[agents-outside-the-window.md](agents-outside-the-window.md), which answers
where the person types; this answers what the editor is built out of when
**history is the product**: fork at any entry, scrub as a tree, fold any tip.

## What is already decided, and must not be re-litigated

Most of the hard architecture questions a non-linear editor usually opens are
already answered by the format, and the single biggest risk in a greenfield
pass is re-solving them worse.

| Question an NLE normally agonizes over | Already answered |
|---|---|
| How is history represented? | Append-only op log; entries carry `id` and `parent`; a branch is an entry whose parent already has a child; a tip is nobody's parent |
| What is the state at a point in time? | `fold_path(base, entries, tip)` — O(path), and branches share prefixes |
| How does undo work? | An appended inverse entry, not a pop |
| Merge / concurrent edits? | **No CRDT.** Forks are explicit; `merge` is a provenance-only op kind; per-entity field patches are the conflict unit, which is the same unit the multiplayer authority already linearizes |
| Naming across history | Content-addressed entries dedupe across forks; assets are `assets/<sha256>.<ext>` and immutable |
| Physics under replay | Settled in `ext-physics`: lockstep determinism is refused as "a lie"; trajectories are **recorded** and playback "scrubs recorded motion instead of simulating it" |

Two consequences worth stating plainly, because they remove the two classic
time-travel bugs by construction:

- **Scrubbing can never show the wrong asset bytes.** A fold at any tip names
  exact hashes, so the asset cache is keyed by hash and an old tip loads the
  bytes that tip referenced. (Today's `gen3d/replay.rs` loads assets from the
  working tree instead, which is exactly the bug the live-editing POC
  flagged; content addressing is the fix, not a workaround.)
- **Scrubbing can never diverge from physics.** Because trajectories are
  data, no solver has to reproduce anything, and Avian's
  non-determinism across platforms stops being the editor's problem.

The spec also says where the remaining work is: *"The new cost is
visualizing a large tree — a UI concern (the branch rail), not a format
concern."* That is the stack question.

## The one decision that matters: the ECS is a projection

Today Gen's scene is a co-author of the document. It projects scene state
into ops at roughly 4 Hz, and `gen3d/ops_apply.rs` carries the tell: a host
must apply authority-originated ops *"or its next projection would 'heal' the
document back to the pre-undo scene."* That loop is survivable for a live
room. It is **fatal for non-linear editing**, because the moment the scene can
author the document, scrubbing to another tip races a projection that is busy
writing the old tip back.

So the rule the whole stack hangs from:

> **Ops → document → fold → diff → ECS. One direction. Always.**
> A gizmo drag emits an op; it never mutates a `Transform` and lets a
> projection notice later.

Everything good follows from it. Undo is an entry. Scrub is a fold. An
agent's batch and a human's drag are the same kind of event. The log is
complete by construction rather than by a 4 Hz sampler being lucky. And the
editor's correctness becomes testable without a GPU.

## The layers

```
openworldformat (crates.io)     document, EditOp, OpLogEntry, fold_path,
                                fold_state, fold_trajectories, refs
        ↓
history store            NEW    entries by hash; snapshots every N; an LRU of
                                folded docs keyed by entry id; branch/tip index
        ↓
editor core              NEW    current tip, selection, the pending batch,
         no Bevy                validation, undo — emits ops, owns no rendering
        ↓  (doc→doc delta, world-sync/src/diff.rs is 280 lines of this today)
reconciler                      minimal ECS mutation from a document delta;
         Bevy system            asset cache keyed by sha256
        ↓
viewport + chrome               bevy_picking, bevy_gizmos, bevy_ui_widgets,
         Bevy                   bevy_feathers, bevy_input_focus
        ↕
agent surface                   the live ops API, the MCP shim, ACP;
                                a terminal when a terminal is wanted
```

**`editor core` has no Bevy, and that is the point.** `worldprobe` is egui,
`worldwalk` is SwiftUI/RealityKit and Compose, and the web viewer is three.js
— four renderers over one format. A Bevy-free core means the non-linear
editing model (tips, forks, pending batches, undo, validation) is written once
and tested headless in CI, and the Bevy layer is the part that can be wrong
without being incorrect. This is the same discipline §5 already enforces for
the format itself.

**The reconciler is the only performance-sensitive new code.** Scrubbing must
not despawn and respawn a world per frame, so it is a diff-and-patch —
React's reconciliation for a scene graph. `world-sync/src/diff.rs` is already
document-to-document diff producing ops, so the shape exists; what is new is
keying ECS entities to world ids stably across a re-fold, which `NameRegistry`
and `GenEntity` do today.

**Seek cost needs keyframes, and the spec already names the trick.** Folding
is O(path), so a timeline scrub over a long session is O(n) per frame without
caching. Snapshots every N entries plus an LRU of folded documents makes a
nearby scrub a short diff and a distant jump bounded. Content-addressed
snapshots — hashed over the folded document, keyed by the entry they fold
from — additionally let two branches that reach the same state confirm
convergence without replaying, which is git's tree-equality trick and the
thing that makes a branch rail honest about convergence.

## Chrome: Bevy's own widgets, not egui

This is where "fully utilize Bevy" has recently become the right answer rather
than the loyal one. Bevy 0.19 ships an editor-widget toolkit.

**It is one feature flag away, not already present.** `bevy_feathers` and
`bevy_ui_widgets` are pinned in `Cargo.lock`, so adopting them resolves
nothing new — but they are **not** default Bevy features and nothing in the
workspace depends on them today (`cargo tree -i bevy_feathers` finds
nothing). Using them means adding the `bevy_feathers` feature to the `bevy`
dependency, which also pulls `bevy_ui_widgets`. Cheap, but a step, and this
RFC previously implied they were available as-is:

- **`bevy_ui_widgets` 0.19** — headless widget behaviour: `button`,
  `checkbox`, `list`, `menu`, `popover`, `radio`, `scrollarea`, `scrollbar`,
  `slider`, **`text_input`**.
- **`bevy_feathers` 0.19** — the styled layer over it, described in its own
  docs as "a collection of styled and themed widgets for building editors and
  inspectors… designed with a future Bevy Editor in mind," with
  `theme.rs`/`tokens.rs`/`palette.rs`/`dark_theme.rs`, `controls/`,
  `containers/`, `display/`, `focus.rs` and `cursor.rs`.
- **`bevy_input_focus`** — focus and tab navigation as a first-class Bevy
  concern, which is the structural fix for the problem egui creates here:
  `inspector/mod.rs:121` must keep `enable_absorb_bevy_input_system` off
  because it clears `ButtonInput<KeyCode>` and kills WASD, with four
  hand-rolled focus guards elsewhere. Feathers routes focus through
  `bevy_input_focus::tab_navigation` instead of fighting the same input
  resource.
- **`bevy_picking`** and **`bevy_gizmos`** for selection and direct
  manipulation, with `bevy_clipboard` for copy/paste.

The honest caveats, in the crate's own words: *"this crate is still
experimental and unfinished! It will change in breaking ways, and there will
be both bugs and limitations."* And it is deliberately not a complete editor
kit — there is **no docking, no tree view** (`list.rs` is flat) and no graph
view, so the outliner's hierarchy and the branch rail are hand-rolled either
way.

That is tolerable, because of what this editor's chrome actually is. It is
not Blender's hundreds of panels. It is six surfaces: viewport, outliner,
inspector, **timeline/branch rail**, asset shelf, log. Five of the six are
within `feathers` plus a tree widget.

**And the sixth is the argument.** The branch rail is the product, not a
footnote: thumbnails per keyframe, a scrub that re-folds at interactive
rates, fork and merge, "diff this tip against that tip". It wants render-target
thumbnails, which is Bevy; a smooth 60 fps scrub driven by the reconciler,
which is Bevy; and the same frame budget as the viewport, which an
immediate-mode overlay re-tessellating every frame does not help with.
`Screenshot` is already used in `gen3d/live.rs`, so per-keyframe thumbnails
are a render target away. Putting the timeline in Bevy and the rest of the
chrome in egui would make the primary surface the odd one out; putting all of
it in Bevy makes the timeline first-class and costs a tree widget.

## The terminal, in this stack

With chrome on `bevy_ui` and focus owned by `bevy_input_focus`, an in-window
PTY pane gets substantially more defensible than it was as an egui widget:

- `alacritty_terminal` (Apache-2.0) is the emulator; `portable-pty` (MIT) is
  the process. Neither is a licence problem and neither is ours to write —
  and the process half is already written here: `core/src/pty.rs` is a
  portable `PtyHost` trait with scrollback and reattach, and
  `cli-tools/src/pty.rs` is 742 lines of `portable-pty` behind it, already
  served over the bridge IPC protocol.
- The grid is the easiest thing a 3D engine draws: one texture of (glyph
  index, foreground, background) per cell, one quad, a glyph atlas sampled in
  WGSL. An immediate-mode UI would tessellate per glyph instead.
- Focus is the part that used to be fatal and now is not. A terminal wants
  Tab, Escape, arrows and `Ctrl-C`, and so does the player controller;
  `bevy_input_focus` is where that arbitration belongs.
- What remains ours regardless: the key-to-escape-sequence table (Zed's is
  442 lines), mouse reporting, alt-screen, selection, scrollback. Weeks of
  work, not days — and alacritty's own tables are Apache-2.0 if it comes to
  that.

**The Bevy payoff worth naming:** once the terminal is a texture, it can be a
surface *in* the world — an agent's pane docked beside the thing it is
editing, or standing in the scene at the coordinates it is changing. That is a
genuinely new editor idea rather than a reimplementation of a pane, and it is
only available because the engine owns the chrome.

**But still do the cheap things first.** Launch the user's own terminal in the
world folder; ship the MCP shim so an outside agent has tools; speak ACP at
protocol version 1 so turns, tool calls and plans arrive as data the timeline
can attribute to revisions. A terminal pane is a nicety on top of those three,
and the ordering in
[agents-outside-the-window.md](agents-outside-the-window.md) does not change.

## Built, and what it found

Step 1 and most of step 2 exist: `crates/world-editor`
(`localgpt-world-editor`) — `History` (the branch index), `FoldCache` (folds
that scrub), `delta_between` (what changed, as ordered ops) and `Editor`
(tip, document, selection, submit / undo / goto / fork). No Bevy, no I/O, 43
tests, clippy-clean. Three things it found are worth more than the code.

**The ECS rule held, and submitting while scrubbed back is a fork.** Nothing
in the model needed a special case for "editing in the middle of history":
an entry's parent is the current tip, so a submission after a seek branches
rather than rewrites, and the trunk's tip survives as a second tip. That
falls out of the format; it did not have to be built.

**Fork points must be snapshotted, or sibling seeks pay for the trunk.** The
cadence rule alone is not enough. Seeking from one branch to its sibling
finds no cached document on the new path — the branch you came from is not
on it — so it re-folds from the base. Pinning every entry with more than one
child fixes it, because those are exactly the entries two branches share: a
50-entry branch beside a 1,000-entry trunk now costs 50 applies to cross
instead of 1,050. This is a cache rule the format's branching RFC does not
imply and a reader would not guess.

**The fold was O(n²), upstream — now fixed.** This was the real finding, and
it moved where the work was. `WorldDoc::apply_entry` cloned the document for
atomicity, called `apply_all` which cloned it *again*, then called
`resolve_refs`, which cloned the whole name map to dodge a borrow — three
O(n) clones per entry. Measured over a 10,000-entry session, folding whole
took about **11 seconds**, roughly 1.1 ms per entry, and that figure was
*flat against the snapshot cadence*, which is what proved the cost was in
the applies rather than the snapshots:

| cadence | snapshots kept | cold seek |
|---|---|---|
| 32 | 312 | 17.6 ms |
| 128 | 78 | 17.2 ms |
| 512 | 19 | 404 ms |
| 2048 | 4 | 880 ms |
| none | 1 | 2.6 s |

The cache earns its place regardless: a cold seek is 150× faster with
snapshots than without, a drag along a branch costs exactly one entry, and
cadence 128 is the knee — a quarter of the snapshots of cadence 32 for the
same seek, so those are the defaults.

**The upstream fix landed** as `openworldformat` 0.3.1 (`perf(rust): a
linear fold, from three document copies per entry to none`). Two copies went
outright, and `fold_log` now uses a new `apply_entry_in_place`: a fold owns
its document and returns `Err` without it, so the per-entry transactional
copy was protecting a document nobody could observe. Name binding is
untouched — an entry is still the unit of ingestion even where it is no
longer the unit of rollback — and three tests pin that. The fold is linear
now, roughly a microsecond per entry:

| entries | before | after |
|---|---|---|
| 1,000 | 128.7 ms | 0.55 ms |
| 4,000 | 1.9 s | 2.5 ms |
| 8,000 | 7.6 s | 6.6 ms |
| 16,000 | 31.3 s | 10.7 ms |

0.3.1 is published and the workspace is on it: `FoldCache::doc_at` uses
`apply_entry_in_place` for the same reason `fold_log` does, and the
ten-thousand-entry gate is back and no longer ignored — the whole scrub
budget file now runs in 0.84 s.

**It was not Rust's bug.** Every reference paid the same cost, for the same
two reasons, so folding a long log was quadratic in all five:

| reference | before | after | at |
|---|---|---|---|
| Rust | 31.3 s | 10.7 ms | 16,000 entries |
| JavaScript | 8.8 s | 4.0 ms | 4,000 entries |
| Python | 4.2 s | 7.4 ms | 2,000 entries |
| Swift | 1.21 s | 6.2 ms | 2,000 entries |
| Kotlin | 4.14 s | 0.62 s | 16,000 entries |

The per-entry trial copy was redundant in all five, and Swift and Kotlin
additionally rebuilt an `id -> entity` map and a name set inside *every
edit*, both carrying a comment that worlds are tens of entities. Swift
needed two maintained indexes to actually go linear, because its entities
are an array of structs holding a `String` and a dictionary — so even an
id scan is ARC traffic per element. It now carries the `nameToId` the other
references always had.

Kotlin is 6.6× better and still superlinear, and the remainder is
structural: its state is an immutable data class, so `entities + entity`
rebuilds the list on every spawn. Going fully linear there needs a
persistent list or a mutable fold-internal builder — a design decision about
that reference, not a bug.

That the JavaScript one mattered most is worth saying: that fold is the
renderer behind localgpt.world and the `openworldformat` npm package, and it
was the slowest of the five.

## Decided: the package is the only document

**No backwards compatibility** (2026-10-04). `world.ron` stops being a
document format; an openworldformat package is the one the desktop app opens,
edits and saves. That is 87 references across eight crates, concentrated in
three files — `gen3d/world.rs` (30), `gen3d/world_import.rs` (11),
`gen3d/plugin.rs` (11) — so it is a sequence, not a commit. The sequence
starts by making the package the document rather than by deleting the old
one: once the app opens and edits packages, `world.ron` falls out
unreferenced and removing it is mechanical.

### And `world-editor` duplicated the authority

Worth recording because it is the drift class this workspace has paid for
before. `world-agent`'s `LiveWorld` already does the package half, and more
of the editing half than this RFC credited it with:

| | `LiveWorld` | `world-editor::Editor` |
|---|---|---|
| package read/write, `manifest.json` guard | yes | no, deliberately pure |
| `submit` (ingest, commit whole or refuse) | **yes** | **yes** |
| `undo` (appended inverse, per author) | **yes** | **yes** |
| git commits, `verify` fold == manifest | yes | no |
| tips, forks, scrub to any tip | no | yes |
| a fold cache fast enough to drag | no | yes |
| delta between two documents | no | yes |
| selection | no | yes |

Two implementations of submit and undo is one too many, and the fix is cheap
only while nothing depends on the new crate — which is now.

**Resolution: one authority, one view.** `LiveWorld` keeps everything that
touches the package — persistence, the manifest guard, git, `verify`, and
committing — because it is the thing the live API, the headless example and
the app already go through. `world-editor` keeps the non-linear half it
actually added: the branch index, the fold cache, deltas and selection. Its
own `submit`/`undo` go, in favour of the authority's; what remains is the
view that can seek, fork and say what changed.

That also settles the layering: the adapter needs `LiveWorld`'s base
document, which it does not expose yet (`entries()`, `head()` and
`revision()` are public; `base()` is not), and the wiring belongs in the
crate that can see both — gen, which does not depend on `world-editor` yet.

## Order of work

1. ~~**`editor core`, headless.**~~ **Done** — `crates/world-editor`.
2. **The history store.** Mostly done: snapshots on a cadence, at fork
   points and at the tip, with LRU eviction that never drops the snapshot
   the next step needs, and the scrub budget proven on a synthetic session.
   The upstream fold fix is done; what remains is persistence — entries by
   content hash (`history/<sha>` objects, `oplog::compute_entry_id`) — plus
   publishing 0.3.1 so the workspace can use `apply_entry_in_place`.
3. **The reconciler.** Document delta → minimal ECS mutation, asset cache
   keyed by sha256. Delete the scene→ops projection in the same change; they
   cannot both exist.
4. **The viewport.** `bevy_picking` + `bevy_gizmos`, with every manipulation
   emitting one op per intent — one op per drag, not per frame.
5. **The branch rail.** Render-target thumbnails, scrub, fork, tip switching.
   First surface to exercise steps 1–3 for real.
6. **Chrome.** Outliner, inspector, asset shelf, log on
   `bevy_ui_widgets`/`feathers`, with a hand-rolled tree.
7. **Agents.** External terminal, then the MCP shim, then ACP, then — only if
   wanted — the in-window pane.

## What would change the answer

- **If `bevy_feathers` stalls.** It is explicitly experimental, and this plan
  leans on it for five of six surfaces. The hedge is that steps 1–3 contain no
  UI at all, so a chrome decision can be deferred until the rail exists and
  made again with evidence.
- **If the editor needs to be a pro desktop app before Bevy UI is ready.** The
  fallback is not egui; it is the browser chrome Gen already serves over
  WebSocket, which has the best widgets and the worst integration.
- **If the branch rail turns out to want a real graph layout engine.** Then
  the rail is the one surface that leaves the window, and the rest of the plan
  is unaffected.

## Not decided

- Whether `editor core` lives in this repository or in the spec repository
  beside the reference layer. It is not format semantics, so probably here —
  but `worldprobe` and `worldwalk` would both consume it, which is the
  argument for there.
- Snapshot cadence, and whether snapshots are persisted in the package
  (`history/<sha>`, as the branching RFC sketches) or are purely an in-process
  cache. The spec calls them an optimization and does not require them.
- Whether direct manipulation batches per drag or per gesture group — "move
  three rocks" as one entry or three.
- Where state (`fold_state`) and recorded trajectories appear in the
  timeline. They fold separately from the document, so the rail may need more
  than one lane.
