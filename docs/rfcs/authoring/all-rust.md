# All Rust: nothing but Rust above the operating system

**Status:** proposal, 2026-11-01; asked, not decided, nothing built against
it. If adopted it supersedes [the-page.md](the-page.md) (decided 2026-10-30)
and the decision of [the-world-in-the-page.md](the-world-in-the-page.md)
(2026-10-31), and answers that RFC's open question, who owns the window.
[The two tests](#the-two-tests-first) come before any of it.

## The question

If the vision is Rust — everything that is not Rust goes, low-level code is
written where no crate exists, GPL is acceptable, and even an engine may be
written from scratch — what is the plan, and what does each part cost?

## The rule

**Rust everywhere above the operating system.** Bindings to the OS (objc2
and AppKit, windows-sys, CoreAudio through cpal, winit's) are the OS, not a
dependency. Nothing handwritten in another language is in the tree: no
JavaScript or TypeScript, no Python, no shell. The browser builds are the
Rust compiled to WebAssembly, with glue a tool generates. Third-party C under
a Rust crate is counted and worked down ([C under Rust](#c-under-rust-the-last-mile)),
not forbidden on day one. CADCraft's non-negotiable is the model, with its
check named beside it: "Rust only (no handwritten JS/TS). Never break wasm"
([cad-lessons.md](cad-lessons.md) §9).

## What is not Rust today

Counted on 2026-11-01 (`git ls-files`); the tree is 154,336 lines of Rust in
299 files.

| What | Size | Where it goes |
|---|---|---|
| The page, `crates/app/ui`: React 19, CodeMirror 6, xterm.js 6 with four addons, Tailwind, a Vite and Node build | 27 `.tsx` and 12 `.ts`, 8,155 lines; 1,501 of CSS | egui panels ([the stack](#the-stack)) |
| The design sheets, `crates/app/ui/design/` (HTML drawn with the page's components) | part of the 3,520 lines of HTML | `egui_kittest` snapshots ([tooling and checks](#tooling-and-checks)) |
| The world viewer, `crates/world-export/js`: `world-viewer.js` 1,674 lines, `cinematography.js` 384, `session-client.js` 516, vendored from the `openworldformat` npm package; three.js itself under `website-world/vendor` | 2,574 lines, plus three.js | a WebAssembly build of the Rust renderer ([the phone and the link](#the-phone-and-the-link)); the spec's JavaScript reference stays the spec's |
| The host: wry over WKWebView and the hit-test view (`crates/app/src/webui/mac.rs`); the Windows (WebView2) and Linux (webkit2gtk) hosts not yet written | | gone; eframe runs on all three and the web |
| Scripts: `fetch-model.sh`, `fetch-assets.sh`, `sync-viewer.sh`, `live-smoke.py`, `release-plan.py`, `trust-release-workflow.py`, `remote-smoke.mjs`, `scripts/world-in-the-page/`, `bin/lg` and `bin/*/` | 19 shell (1,053 lines), 8 Python (1,628), 8 `.mjs` (861) | `cargo xtask` ([tooling and checks](#tooling-and-checks)) |
| C under Rust in the app's tree (`cargo tree -p localgpt-app --all-features`): aws-lc through `jsonwebtoken` ← `localgpt-core`, and through `rustls` ← `reqwest` ← `hf-hub` ← `mistralrs`; `ring` the same way; SQLite through `rusqlite` ← `localgpt-core`; ONNX Runtime in Verse's `ml` tier (`ort` 2.0.0-rc.11, for htdemucs stems) | four libraries | [C under Rust](#c-under-rust-the-last-mile) |

Already Rust and staying: the format crates and `world-bevy`, Gen, previs
(`previs-web` is already WebAssembly on localgpt.md), the model tiers
(mistral.rs on candle; llama.cpp is gone), kira and FunDSP, the live API on
axum, Zola for the sites, `portable-pty` under the terminals.

## The stack

**eframe and egui on wgpu**, the shape of SolidCraft and CADCraft
([cad-lessons.md](cad-lessons.md) §1, "How they draw"): eframe owns the
windows, egui draws the panels, the world is the app's own wgpu work in a
paint callback, and the same program runs in a browser as WebAssembly.

Why egui: two agent-built applications of 752K and 224K lines prove it
carries a GPU canvas, a dock of panels and a browser build;
`egui_dock` (MIT) is the dock; `egui_kittest` (MIT OR Apache-2.0) drives it
headless for tests; `egui_system_font_provider` (0.37) loads the system's
fonts for characters its own lack, which is how CADCraft draws its Chinese,
Korean and Arabic interfaces; and egui has several native windows from one
program (`show_viewport_deferred`, since 0.24), which
[the world's viewport](#the-world-bevy-without-a-window) needs. The costs
[egui-or-not.md](egui-or-not.md) listed — input absorbed, hover, scheduling
— were `bevy_egui`'s, egui inside Bevy's window; with eframe owning the
window they do not arise, and CADCraft's canvas, which hides the cursor and
takes every key while it has focus, is the shape of a viewport widget.

The alternatives, read and not chosen: **iced** (MIT) has the better text
(cosmic-text, so shaping and a real `text_editor` widget) and `iced_term`
0.8 as a terminal widget, but the Elm architecture, fewer widgets and no
dock; it is the fallback if [the document](#the-document) outgrows egui's text.
**GPUI** (Apache-2.0, now with a `gpui_wgpu` backend in Zed's tree) is not
published as a library in any stable form, and the parts of Zed worth
having are GPL ([what it buys and costs](#gpl-what-it-buys-and-what-it-costs)).

### The window and the panels

eframe owns the windows; `egui_dock` lays out the panels: the history graph,
the timeline, the outliner and inspector, the document, the agents, Open….
Each panel reads the same snapshots the page reads today — the history, the
document's view, the timeline, the entities and the selection
(`crates/app/src/webui`, `timeline.rs`, `scene.rs`) — so the seam of
the-page.md stays, over a channel instead of a WebSocket, and the phone keeps
using the WebSocket ([the phone and the link](#the-phone-and-the-link)).

### The world: Bevy without a window

Bevy stays as a library: the ECS Gen's tools work on, `world-bevy`, avian,
the song's clock, `bevy_ui` for what is drawn in the world. Its winit plugin
is off. `RenderCreation::Manual` (bevy_render 0.19, `settings.rs:223`) takes
eframe's wgpu instance, adapter, device and queue, so Bevy draws on the same
device as the panels; each camera renders to an `Image`, and the paint
callback shows that texture (`egui_wgpu::Renderer::register_native_texture`)
with no copy. Bevy is ticked once per eframe frame, or on a thread of its
own. This keeps the three things only Bevy gives that the plan wants
(the-world-in-the-page.md, items 4–6): **one scene in several windows** with
one copy on the GPU, each window a viewport showing a camera's texture;
**simulation on every core**, Bevy's schedule untouched; and **drawing with
the window hidden**, since no surface is involved. The world's own interface
(HUD, labels, signs) stays `bevy_ui` in the texture. Untried here:
[the two tests](#the-two-tests-first).

### The terminal widget: the one real piece

The emulator exists: `alacritty_terminal` 0.26 (Apache-2.0) is what Zed's
terminal wraps (`crates/terminal/Cargo.toml:24` in Zed); WezTerm's `termwiz`
(MIT) and its git-only `wezterm-term` are another. The PTY is already here
(`portable-pty`). the-page.md's "the native routes were to write one"
overstated it: what is left to write is the widget, and xterm.js with the
page's four addons is the bar:

- the grid: true colour, bold, italic, underline, wide (CJK) cells, the
  widths `addon-unicode11` gives, scrollback;
- selection and copy, links (`addon-web-links`), mouse reporting, bracketed
  paste, resize sent to the PTY;
- the input method's composition drawn in place, so Chinese typed into Claude
  Code appears where it is typed;
- font fallback: box drawing, braille (spinners), powerline and Nerd glyphs,
  emoji (the ✳ and spinner marks `TurnEnded` reads);
- watermarks: stop reading a PTY while the widget is behind, which
  the-world-in-the-page.md asked for whoever draws.

`egui_term` 0.1 (MIT) is a reading reference, not a dependency. One
`alacritty_terminal` `Term` per tab in the app replaces `vt100` as the one
emulator, so the window draws its grid directly and the phone draws the same
grid streamed ([the phone and the link](#the-phone-and-the-link)).

### The document

egui's `TextEdit` with `egui_extras`' highlighting (syntect, MIT) for
Markdown and Fountain first: it is enough for a scene's pages. CodeMirror's
search and commands are the bar; the second piece, if a screenplay's length
or the editing outgrows egui's text (it lays the whole text out each frame),
is an editor on `ropey` and `cosmic-text`, or the switch to iced.

### The timeline and the history graph

Ordinary drawing: 1,600 lines of React and SVG today (`timeline/`,
`HistoryPanel.tsx`) become about as much egui painter code. If the tracks
get big, CADCraft's canvas is the pattern: a display list built once per
edit, a view uniform per frame (`crates/ui-egui/src/gpu.rs` there).

## The phone and the link

**The phone** ([mobile-remote.md](mobile-remote.md)) gets the same program
compiled with eframe's web runner and served by axum as the page is today,
no JavaScript written. The terminals come as the grid the app's emulator
holds, streamed as cell changes over the existing WebSocket, so the phone
runs no emulator and flow control lives in one place. The world comes as
frames (today's step 1), later as the WebAssembly renderer. The risk: touch
scrolling and the soft keyboard in egui on the web are weaker than the DOM's;
this is the piece most likely to stay worse than the page.

**The link, the export and the board.** localgpt.world, `generate_html` and
`crates/previs/src/board.rs` inline the three.js viewer today. Rust-only
means a WebAssembly world renderer: Bevy compiled for the web is tens of
megabytes before compression, an own wgpu renderer a few. The spec's
three.js reference stays the format's conformance renderer in its own
repository; this one stops vendoring it. This is the strongest reason for an
own renderer ([an engine from scratch](#an-engine-from-scratch)), and it is a later one.

**The sites** keep Zola (Rust) and their templates; the gallery's and the
viewer's JavaScript become the WebAssembly viewer; deploying stays a tool.

## The two tests, first

A week, before anything is decided. Each is a branch, measured with the
world-in-the-page harness's loads and GPU sampler on a quiet Mac.

1. **The terminal.** `alacritty_terminal` in an egui panel, Claude Code and
   Codex running in it through `portable-pty`; Chinese typed through the
   Mac's input method; box drawing, braille and emoji; a resize; the
   `load.py` loads (agent, build, flood) for the frame time. Side by side
   with the page's xterm.js. It decides whether the widget is weeks or
   months.
2. **The world.** Bevy windowless on eframe's device
   (`RenderCreation::Manual`), the song world (178 entities, 5 M triangles)
   drawn into a texture at the display's rate on the ProMotion display,
   shown in two egui viewports at once, then with the window hidden. GPU
   time against the RFC's numbers (12–14 ms a frame with four shadow
   cascades, 7 with one). If it fails: Bevy keeps a window of its own as a
   second process following the world's ops, as `localgpt-gen --join`
   follows a room's; or the own renderer moves up the list.

## Order and cost

Agent-days, the calibration cad-lessons.md §11 asks for, uncalibrated here.

| Step | Days |
|---|---|
| 0. The rule in `CLAUDE.md` with its checks: no `.js`/`.ts`/`.py`/`.sh` in `git ls-files`, no crate depending on `wry`, `cargo xtask wasm` ([tooling and checks](#tooling-and-checks)) | 1 |
| 1. The two tests | 3–5 |
| 2. The shell: eframe, `egui_dock`, the panels in order of size — history graph, timeline, outliner and inspector, Open…, the document on `TextEdit` | 10–15 |
| 3. [The terminal widget](#the-terminal-widget-the-one-real-piece) to its bar | 10–20 |
| 4. The world viewport on Bevy's texture: the fly camera, gestures, the selection box, gizmos, drag from the outliner into the world | 5–10 |
| 5. Remove: the page, wry and the hit-test view, the Node build and `rust-embed` of `ui/dist`, the `interface` CI job, the design sheets | 2–3 |
| 6. The phone as the WebAssembly build, terminals as streamed grids | 5–10 |
| 7. `cargo xtask` for the scripts, `egui_kittest` for the smoke and the sheets | 5 |
| 8. The last mile of C ([C under Rust](#c-under-rust-the-last-mile)) | with 0.5.0 |
| 9. The WebAssembly world renderer for the link ([an engine from scratch](#an-engine-from-scratch)) | later; 30–60 for a first one |

Two to three months of agent time to where the page is today, the renderer
not counted. Steps 2–4 are the same work whichever order; the terminal is
the one that can slip.

## Tooling and checks

`cargo xtask` (the two CAD apps' pattern, cad-lessons.md §9) replaces the
Python, shell and `.mjs`: `xtask fetch model|assets`, `xtask ci`,
`xtask release-plan`, `xtask smoke`, `xtask wasm`; `bin/lg` stays a router
to it or goes. `egui_kittest` drives the interface headless through
AccessKit, so the smoke's widget seam (`GET /widgets`) becomes kittest
queries by name, and the design sheets become snapshot pictures of each
panel with fixtures, reviewed as PNGs. The rule's checks run in CI and name
themselves in `CLAUDE.md`.

## C under Rust: the last mile

Worked down in this order; OS bindings stay.

1. `localgpt-core` goes in 0.5.0
   ([the-harness-goes.md](the-harness-goes.md)), and with it SQLite
   (`rusqlite`) and `jsonwebtoken`'s aws-lc.
2. `ring` and aws-lc come only through mistral.rs's model downloader
   (`hf-hub` → `reqwest` → `rustls`). The model is fetched by its script
   already, so drop the downloader if mistral.rs's features allow; else
   `rustls-rustcrypto` when it is more than 0.0.2-alpha.
3. ONNX Runtime in Verse's `ml` tier (htdemucs through `stem-splitter-core`),
   optional and off by default: a demucs port on candle is a
   write-from-scratch item, or the tier goes.
4. A CI check: `cargo tree -e normal,build` filtered for `-sys` crates
   against an allowlist of OS bindings.

## GPL: what it buys and what it costs

What GPL would buy is Zed's `terminal_view` and `editor`
(`GPL-3.0-or-later`). Neither is a library: `terminal_view` depends on 30 of
Zed's crates (`editor`, `workspace`, `project`, `language`, `settings`,
`theme`, `ui`, …) and `editor` on 73. Taking them is taking Zed's workspace:
LocalGPT as a Zed with a world pane, GPL-3.0 for the whole program. Blender
(GPL) likewise. The cost is the licence: the format crates published to
crates.io stay Apache only if kept apart, and nothing under another licence
can take code from the app again.

What it buys that MIT and Apache do not: nothing this plan needs. The
emulator is Apache (`alacritty_terminal`, the crate under Zed's own
terminal), another is MIT (`termwiz`); egui's family, `egui_dock`,
`cosmic-text`, mistral.rs and candle are MIT or Apache; the widgets are ours
to write either way. **Keep Apache 2.0.** Accepting GPL changes nothing in
this plan but the licence.

## An engine from scratch

What Bevy does here: the ECS Gen's 63,743 lines work on; `bevy_pbr` (PBR,
cascaded shadows, MSAA, tone mapping, environments); `bevy_gltf` (the CC0
pack, skins); `bevy_ui` in the world; avian; the scheduler on every core.
kira and FunDSP are not Bevy's (`world-audio`).

What an own renderer must draw for the format: the conformance scenes
(`crates/world-types/conformance/`): shapes, PBR materials, three kinds of
light with shadows, environments, GLB meshes with skins, instancing for the
pack's 5 M-triangle scans, text in the world, picking and gizmos. The
behaviors and modulations are pure math in `world-types` and `world-bevy`
and port as they are; physics is `ext-physics`'s Rust reference in the
`openworldformat` crate, or avian. The measure of the work is the two CAD
apps' GPU crates: SolidCraft's viewport, capture and path tracer about 28K
lines with their tests, CADCraft's 2D canvas and display list about 12K. A
first renderer that draws the conformance scenes is one to two months;
Bevy's look is months more; and Gen's tools would need re-homing off the
ECS.

When it is worth it: the WebAssembly link's size
([the phone and the link](#the-phone-and-the-link)); a frame-pacing problem measured in test 2
that Bevy's render graph cannot fix; compile time. Not the desktop first.
The seam already exists: `world-bevy` is the only crate where the format
becomes Bevy (`CLAUDE.md`, "the world format is shared across apps"), so the
renderer is that one crate's replacement, and the format's parity check (a
screenshot of each renderer from the same camera, the-world-in-the-page.md's
status) is the test it must pass. **Not first.**

## What it gives up

- The browser's text stack: the input method and font fallback for free,
  bidi, accessibility (egui's AccessKit is partial); CodeMirror's and
  xterm.js's years. The widget is ours to keep up.
- The phone's DOM, where egui on the web is the weaker layout.
- "Agents write React well" (the-page.md). They write egui at scale too:
  the two CAD apps, in a month each.
- Two decisions two days old. The Windows and Linux hosts not yet written
  are saved, not lost: one program, one host.

## What it keeps

The live API and the ops, the format crates, the seam's messages and
snapshots, previs and its board, the agents as processes in PTYs, Zola.

## Status

- [ ] Decided
- [ ] Test 1: the terminal widget on `alacritty_terminal`, with Claude Code,
      Codex and the input method
- [ ] Test 2: Bevy windowless on eframe's device, two windows, hidden window
- [ ] Steps 0–7 of [Order and cost](#order-and-cost)
- [ ] The last mile of C, with 0.5.0
- [ ] The WebAssembly world renderer for the link
