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
(`previs-web` is already WebAssembly on localgpt.md), kira and FunDSP, the
live API on axum, Zola for the sites, `portable-pty` under the terminals.
(The model tiers — mistral.rs on candle — were on this list when it was
written and went later the same day, with `localgpt-core`: the dependency
rows below that name them describe a tree that no longer exists, and
ONNX Runtime in Verse's `ml` tier is the one C library left under the app.)

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

## A DAW and an editing room, driven by agents

Asked 2026-11-01: do professional DAW capabilities and video editing,
driven by coding agents, fit this architecture? Yes, and better than the
page would have: both are an engine under a dense, custom-drawn interface,
which is the shape of everything above. What they add is two engines and
two format questions; nothing in the stack has to change.

### The shape is already here

- **The interface.** A mixer with meters, a piano roll, an arrangement, a
  monitor with scrubbing, a colour wheel: painter widgets at the display's
  rate, the timeline's pattern, and CADCraft's display list when the tracks
  get big. Rerun (MIT OR Apache-2.0, egui on wgpu) draws millions of points
  on time axes; a dense timeline is not where egui gives out. Meters repaint
  through `request_repaint_after`; the audio thread is never waited on.
- **The engine thread.** `world-audio` already runs its engine on a thread of
  its own with transforms forwarded to it. A DAW engine is that shape with a
  real-time thread that never locks or allocates: `rtrb` ring buffers
  (MIT OR Apache-2.0) in, `basedrop` to free off the thread.
- **The agents.** Their command surface is the world's API — `localgpt world
  submit`, the ops, receipts, undo, redo and forks — and an arrangement or an
  edit is a document with a history, which is `world-editor`'s model (tips,
  forks, folds). An agent in a terminal places a clip at bar 17, writes an
  automation lane or renders a shot sequence the way it hangs a lamp: one
  command catalog, contracts it can read, never crash, headless parity
  ([cad-lessons.md](cad-lessons.md)). A DAW driven through a command log is
  easier to test headless than one driven by a mouse.
- **Bevy windowless** is the render farm: each shot of the world is a take
  rendered at the project's rate into a texture and encoded, the "pictures
  nobody is watching" item of the-world-in-the-page.md.
- **One wgpu device.** Decoded frames are textures on the device the panels
  and the world draw on; on macOS a VideoToolbox frame is an IOSurface that
  wgpu's Metal backend imports with no copy (`create_texture_from_hal`), and
  the compositor is WGSL in a paint callback — SolidCraft's pattern.
- **The phone** as a control surface: faders, transport and a shot list over
  the WebSocket that already carries the terminals, in the WebAssembly build.

### The audio engine: the second real piece

What a DAW needs that `world-audio` lacks — kira is a game mixer that owns
the device:

- the device, owned directly: `cpal` (Apache-2.0) over CoreAudio, WASAPI and
  ALSA; a callback at a fixed block size; a sample-accurate transport with a
  tempo map, loop and punch;
- a graph with delay compensation: Firewheel (MIT OR Apache-2.0; the engine
  Bevy plans to make its default, through `bevy_seedling`) is the graph to
  build on. It is a game engine's graph, so clip scheduling, automation
  lanes, plugin delay compensation, freeze and recording are the DAW layer on
  top. Meadowlark's engine, Dropseed, is AGPL and not ours to take; its
  `meadowlark-dsp-mit` (MIT OR Apache-2.0) is DSP to take;
- disk streaming for long clips: `creek` (MIT OR Apache-2.0); decoding:
  `symphonia` (MPL-2.0, pure Rust: WAV, FLAC, MP3, AAC, Vorbis, ALAC);
  resampling: `rubato` (MIT); MIDI in and out: `midir` (MIT); stems as
  tracks: Verse's splitter already, the ONNX item of
  [C under Rust](#c-under-rust-the-last-mile);
- time-stretch and pitch: no pure-Rust library of professional quality. The
  `signalsmith-stretch` crate binds the C++ (MIT), `ssstretch` likewise, so
  a port of Signalsmith's published algorithm is a write-from-scratch item,
  two to three thousand lines;
- plugins: CLAP first, hosted through `clack` (MIT OR Apache-2.0, Rust over
  the C ABI); the plugin binaries are what they are, like the OS. AU through
  AudioToolbox, the OS. VST3 through the `vst3` crate (MIT OR Apache-2.0),
  which needs Steinberg's SDK headers at build time, and the SDK is GPLv3 or
  Steinberg's own licence: the one place GPL buys a feature. A plugin's
  interface needs a native parent view; eframe's viewports are winit
  windows, so it gets its `NSView` or `HWND` through `raw-window-handle`,
  or from a winit window of our own;
- the format: tracks, clips, automation and the tempo map are an extension
  in the spec repo. The song world (`soundtrack`, its sections and curves) is
  the seed, and the agents' commands come with the extension.

The engine with transport, tracks, clips, automation, recording and CLAP is
fifteen to twenty-five thousand lines, three to four months of agent time to
one that is used. "Professional" — delay compensation at its edges, freeze,
comping, stretch quality, plugin interfaces on three systems — is the long
tail every open DAW has spent years in; it is a product decision measured
against Reaper and Ardour, not a step.

### The editing room

What it needs: decode, a frame cache, a compositor, colour, encode, a muxer.

- **Codecs are the rule's hard edge.** H.264, HEVC and ProRes, both ways,
  are the OS: VideoToolbox on macOS (`objc2-video-toolbox` 0.3,
  `objc2-core-video`, `objc2-core-media`, MIT), Media Foundation on Windows
  (the `windows` crate). Pure Rust has AV1 both ways (`rav1e` to encode,
  `rav1d` to decode, the Rust port of dav1d, both BSD-2-Clause) and the
  containers (`mp4`, MIT; Matroska and WebM crates); audio tracks through
  symphonia, and AAC out through AudioToolbox. Linux has no codec of its own
  above C: the choice there is AV1 only, or the distribution's FFmpeg or
  GStreamer counted as the OS, which [the rule](#the-rule) would have to say.
- **The compositor:** frames as wgpu textures; transforms, blends,
  transitions, titles (text laid out to a texture), LUTs and grading as
  WGSL. No OpenColorIO (C++): a scene-linear pipeline with named transforms,
  written here.
- **The sequence** is in the format already: the timeline's shots with
  their frames, and a cut that rolls when a trim meets the next shot. The
  edit is that sequence with imported footage and transitions, a second
  extension in the spec repo; rendering is Bevy windowless per shot, into
  textures, into the encoder.
- **The agents:** a cut at a time, a dissolve of twelve frames, a render at
  1080p are ops through the API; the receipt carries the render's hash, and
  a proposal waits for review like any edit.

Decode to texture to encode through VideoToolbox, with a cache, is five to
eight thousand lines; the sequence and the compositor another eight to
twelve. Two to three months to an editing room for the world's own takes and
imported H.264 and ProRes on macOS; Windows follows with Media Foundation;
Linux is AV1 only until the rule says otherwise.

### What it changes in the plan

- `world-audio`'s rule, "kira owns the device", becomes "the engine owns the
  device"; the FunDSP graphs stay as sources, and kira goes or becomes a
  track.
- A real-time thread joins eframe's, Bevy's and the PTY readers; nothing
  above it may block it. The terminal and the world keep to that already.
- Two format extensions before the engines, the arrangement and the edit,
  in the spec repo: the format decides, and the app renders what the fold
  says.
- Order: after parity ([order and cost](#order-and-cost)), the audio engine
  first (the song world is its seed), the editing room second (the world's
  takes before imported footage).
- GPL, reconsidered: hosting VST3 is the one feature it buys. CLAP and AU
  cover the current plugins and macOS. **Keep Apache 2.0** unless VST3
  is required.

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
the one that can slip. Not in the table: the audio engine and the editing
room ([a DAW and an editing room](#a-daw-and-an-editing-room-driven-by-agents)),
three to four and two to three months after parity, each with an RFC of its
own.

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
- [ ] The audio engine, after parity: the arrangement extension in the spec
      repo, then its own RFC
- [ ] The editing room, after the audio engine: the edit extension, then its
      own RFC
