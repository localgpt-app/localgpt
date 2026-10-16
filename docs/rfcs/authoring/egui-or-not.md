# Should egui go?

**Status:** evaluation, 2026-10-16. Answers one question for the Bevy apps
(`crates/gen`, `crates/app`): remove `bevy_egui`, and if so when. The pure
egui apps — `localgpt desktop` (`crates/cli`, eframe) and `worldprobe`
(eframe, its own repository) — are not in scope: there egui is the whole
window, not a layer over a renderer, and it is the right tool.

## What egui is in Gen today

Roughly 400 lines across `gen` and `app` mention egui. Where they are:

| Surface | File | ui calls (rough) | What it does |
|---|---|---|---|
| Inspector detail panel | `inspector/detail.rs` | ~200 | every field of the selected entity: name, transform, shape (a generic JSON editor), material, light, behaviors with target combos, audio, mesh, parent combo |
| Prompt panel | `desktop/panel.rs` | ~120 | the chat panel, model picker, controls |
| Collaboration panel | `desktop/collab.rs` | ~90 | rooms, peers |
| Client panel | `desktop/client_panel.rs` | ~55 | the guest's panel |
| Gallery | `gen3d/gallery_ui.rs` | ~50 | the world gallery overlay |
| Shot strip | `gen3d/shots.rs` | ~45 | thumbnails per shot |
| World bar | `inspector/world_info.rs` | ~40 | snap, review, bookmarks, status |
| Outliner | `inspector/outliner.rs` | ~35 | the tree, drag-to-reparent, context menu |
| Review bar, box overlay | `gen3d/live.rs`, `gen3d/canvas_edit.rs` | ~30 | Accept/Reject, the marquee |
| App shell | `app/src/document.rs`, `now_playing.rs`, `open.rs` | ~105 | the song app's panels |

Two surfaces are already not egui: the HUD, labels, signs, notifications and
tooltips (`gen/src/ui/`, ~1500 lines of `bevy_ui`), and the history rail
(`gen3d/history_rail.rs`, `bevy_ui`).

## What it costs today

- **Input.** `enable_absorb_bevy_input_system` must stay off because it
  clears `ButtonInput<KeyCode>` and kills flying; five hand-rolled guards
  decide instead whether a key is the panel's or the world's
  (`detail.rs:125`, `canvas_edit.rs:1228`, `panel.rs:341`, `panel.rs:410`,
  `client_panel.rs:94`). Each new key the canvas takes is another place
  to get this wrong; this week's Shift and Q collision was the same class.
- **Hover.** egui's own `is_pointer_over_egui` ignores the legacy context
  panels drawn here, so a click on a panel reached the scene behind it
  until the panels' rectangles were collected by hand (`PanelRects`,
  `OtherPanels`) — a second source of truth for "what is under the pointer".
- **Scheduling.** With bevy_egui's multipass mode, every panel system must
  run in `EguiPrimaryContextPass`; drawn from `Update`, a panel panics. A
  rule to know, not a bug, but one more.
- **Offscreen and tests.** The panels composite into the offscreen image
  (fine), and the smoke harness's widget seam (`GET /widgets`, fifteen uses)
  is built on egui's response rectangles. Removing egui means rebuilding that
  seam on `bevy_ui` layout nodes before any of those checks run again.
- **Frame budget.** Immediate mode re-tessellates every frame. It has not
  shown up as a cost yet; the branch rail's thumbnails and a 60 fps scrub
  are where it would, which is why the rail was built on `bevy_ui`.

## What it gives today

- **The inspector exists and works.** A generic JSON editor for any shape,
  combos, drag values, drag-and-drop reparenting, context menus, text
  editing with focus, a bottom bar — about 400 call sites that 110 smoke
  checks exercise every run and that two days of real use have shaped.
- **Text input that is finished.** `bevy_ui_widgets` 0.19 has
  `text_input`; feathers calls itself "experimental and unfinished" and
  "will change in breaking ways". The detail panel's name field and the
  search box are the places a half-finished text input hurts first.
- **No tree, no docking, either way.** feathers' list is flat and it has no
  docking; the outliner's hierarchy would be hand-rolled on `bevy_ui` as it
  is hand-rolled on egui. This is the one place the two are equal.

## What is on the other side

Bevy 0.19.1 is the workspace's Bevy. `bevy_feathers` 0.19.1 and
`bevy_ui_widgets` are in `Cargo.lock` and **enabled nowhere**
(`cargo tree -i bevy_feathers` finds nothing); using them is a feature flag
on the `bevy` dependency, cheap but a step. `bevy_input_focus` is the
structural fix for the input problem: focus and tab navigation as a Bevy
concern, so the panel and the world stop fighting over one resource. The
editor-stack RFC's argument stands: the branch rail is the product, it is
Bevy, and chrome on the same stack keeps it first-class.

## The answer

**Not now, and not all at once. Freeze it, stage it, and let the triggers
decide.**

1. **Nothing new goes on egui.** A transcript, a terminal pane, a tree for
   the rail, any new panel: `bevy_ui`, with `bevy_feathers` behind the
   feature flag when its widgets fit. The rail already follows this.
2. **Spike one panel to price the rest.** The world bar (`world_info.rs`,
   ~40 calls: four checkboxes, two drag values, a menu, a status line) is the
   smallest complete surface. Port it to feathers behind the flag, keep the
   egui one until the smoke passes on the new one, measure the days and
   what feathers lacked. That number decides the schedule for the big two.
3. **Migrate per surface, inspector last.** Order by cost and by how often
   the input problem bites there: world bar, review bar and box overlay,
   gallery, shot strip, outliner (with the hand-rolled tree the rail will
   want too), then the detail panel — whose generic JSON editor is the one
   piece worth writing once, for both the inspector and a future asset
   shelf. The prompt, collaboration and client panels go when the agent
   transcript lands on `bevy_ui` (tier 1 in the agents RFC), since that is
   when their job changes anyway.
4. **Rebuild the harness seam first, not last.** Before the outliner moves,
   `GET /widgets` learns to report `bevy_ui` nodes by name, so every check
   that drives a widget keeps running through the migration.
5. **Triggers that pull the schedule in:** a feature the guards cannot
   handle (an in-window terminal wants every key); feathers shipping a tree
   or a dock; a frame-budget measurement on the rail that names egui.
   **Triggers that push it out:** feathers breaking twice across Bevy
   releases, or `text_input` not being usable for a name field.

Removing egui outright today would spend two to four weeks rewriting chrome
that works, on widgets that call themselves unfinished, and would take the
smoke's widget checks down while it happened. Keeping it forever would leave
the input problem structural and the rail the odd one out. Staging it per
surface, starting with the cheapest and ending with the inspector, costs
nothing until the first spike and lets the evidence set the pace.
