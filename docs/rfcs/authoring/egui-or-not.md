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

## The spike, done (2026-10-16)

The world bar is on feathers, behind the `feathers` feature (`cargo build
-p localgpt-gen --features feathers`): `crates/gen/src/inspector/world_bar.rs`
is the egui bar's controls on `bevy_ui` + `bevy_feathers` 0.19.1 — the same
widget names, the same intents into the same authority — and the egui bar
(`world_info.rs`) stays, the feature choosing. The harness seam came first,
as item 4 said: `crates/gen/src/ui/widgets.rs` reports every visible
`bevy_ui` node that carries a `Widget("name")` into `GET /widgets` beside
egui's rectangles (window points, from `ComputedNode` and
`UiGlobalTransform`), the history rail and its rows report through it
(`panel.history_rail`, `rail.row.<id>`, `rail.turn.<n>`), and the smoke
drives either bar by name (`world_info.local`, `world_info.review`,
`world_info.agent`, `menu.terminal here`).

**The price.** About an hour and a half for the bar and an hour for the
seam, the hover rule and the smoke checks. 787 lines against the egui bar's
321 (both with their doc comments; the feathers file carries the popups
and the sync systems that immediate mode gives for free), so ~2.4× the code
for ~40 egui calls. Scaled by call sites, the detail panel (~200 calls, a
generic JSON editor, combos, text fields) is one to two weeks, the outliner
(a tree and drag-to-reparent, hand-rolled either way) three or four days,
the rest a day or two each: the two-to-four-week figure above holds, and
the recommendation with it — freeze, stage, let the triggers decide — with
one amendment below.

**What feathers lacked, concretely.**

- No drag value. `number_input` exists only as a BSN scene (`#[scene]` +
  `bsn!`), so the grid and angle fields are sliders (0.05–10 m, 1–90°), a
  worse fit for a value one types.
- No menu without BSN either: the bookmarks and agent menus are popups of
  our own — a hidden, absolutely placed child of the button, toggled,
  rebuilt when what it lists changes.
- No tooltips; the egui bar's hover hints ("Y toggles; hold Cmd…") are gone.
- Every `*_bundle` constructor used here is `#[deprecated]` in 0.19 in
  favour of the BSN forms. **The amendment:** the next surface starts on
  `bsn!`, or this one is on borrowed time.
- Controlled widgets: `Checked` and `SliderValue` are the app's to keep,
  so `sync_controls` writes the settings back into the widgets each frame
  (the `Y` and `L` keys change them too). Right by design; code all the same.
- Focus: a click leaves a widget focused, and Space — the fly camera's
  ascend — then toggles it as well; the bar clears `InputFocus` off its own
  widgets each frame. The structural fix hoped for from `bevy_input_focus`
  is there, and here had to be undone.
- Layout: buttons and sliders ship with `flex_grow: 1`, and `Node` cannot
  be overridden through `overrides` (a duplicate component), so each sits in
  a holder of its own width. Text with a colour of its own must stay out of
  `ThemedText`, or the theme writes over it on the next hover.
- Layering: `bevy_ui` draws under egui, so a bar between the egui side
  panels had 710 px and its right-hand controls fell under the detail
  panel. It takes the full width now because an empty, frameless egui
  bottom panel reserves its strip before the side panels are drawn — which
  works because egui's panels take no pointer input of their own, only its
  areas and widgets do, so the bar's widgets under the strip still get
  their clicks. A coupling that lasts until the side panels move too.

**What the seam found on its first run.** Two bugs that predate it. The
egui bar, drawn after the side panels, was laid out *under* the right one:
with the full inspector open its review checkbox, bookmarks and status
line sat behind the detail panel, and nobody had clicked them by name
before (the bar is drawn first now and spans the window). And nothing on
`bevy_ui` had ever shown in an offscreen render: `bevy_ui` draws through
the camera that targets the primary window, and with the canvas rendering
into an image no camera did, so the history rail was laid out against a
zero-size target (its reported rectangle had a negative x) and left out of
every screenshot the smoke took. The retargeted camera is the default UI
camera now. A third followed from the hover rule: once the rail blocked the handles
under it, as a panel should, its place mattered — it had floated at the
top right under the detail panel, mostly hidden, and over the middle of
the free viewport. It keeps clear of the detail panel now, and with the
full inspector open docks above the bottom bar with three rows. A fourth was older than all of them: the canvas's handle, pointer and
key systems sat in a tuple inside a chain without being chained
themselves, so Bevy was free to run the pointer system first — a press
on a ring then began a box too, and the release emptied the selection.
Which order Bevy picked depended on what else was in the schedule, so
adding the seam's systems flipped it for the default build and not for
the feathers one; the three run in order now. All four are the kind of
thing a named-widget check catches and a rectangle-free check never
would.

**The smoke.** `scripts/live-smoke.py --offscreen --herdr` on a fresh git
world passes all 123 checks against both builds — the egui bar and the
feathers one — driving the bar's checkboxes, the rail's rows and the
agent menu by the same names, and herdr's pane end to end.

**What it gave.** Hover and clicks come through `bevy_picking` like the
scene's own, so one rule now covers all the `bevy_ui` chrome: the pointer
over a named node, or anything inside one, marks `UiHovered` — the history
rail included, which it never was, so a click on a rail row no longer
reaches the scene behind it. The bar renders into the camera's image, so an
offscreen screenshot shows it without the compositing egui needs, and the
synthetic pointer already targets that image. The fonts are embedded (Fira
Sans) and the dark theme's tokens fit the inspector without tuning.
