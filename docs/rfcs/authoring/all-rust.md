# All Rust: the native application and its remaining work

**Status:** decided 2026-11-05. The desktop migration is implemented: steps A–E landed on 2026-11-06. eframe owns the window, egui draws the panels, Bevy renders the world into textures, and the terminal widget uses `alacritty_terminal`. The desktop web-view host is gone. Browser/mobile convergence and the remaining tooling migration are below.

## The rule

**Rust above the operating system.** Operating-system bindings are part of the platform, not a second application stack. New application code, checks and repository tools are Rust; browser builds compile Rust to WebAssembly with generated glue. Third-party C reached through Rust dependencies is audited separately and worked down where practical.

The rule is enforced as a ratchet, not a claim that every vendored file has already been rewritten:

```bash
cargo xtask rule           # no new non-Rust source and no new wry dependency
cargo xtask verify --rust  # the same check with the Rust build and tests
```

`xtask/rule.txt` records the remaining exceptions and may only shrink through the automated check. New JavaScript, TypeScript, Python or shell requires an explicit exception added by hand; the expected direction is to delete exceptions, not add them.

## The desktop stack

### Window and panels

eframe 0.37 owns the native windows. `egui_tiles` lays out the world, document, history, outliner/inspector, terminals and timeline as movable panels. The implementation is `crates/app/src/shell`.

The shell is a typed client of the app's existing state and commands. It uses the same history, document, scene, timeline and clock snapshots as the paired-device page, over an in-process channel rather than a desktop WebSocket. That keeps the native and remote interfaces from becoming separate products.

### Bevy without its own window

Bevy remains the world engine and ECS, but its winit plugin is not the desktop host. It shares eframe's wgpu instance, device and queue through manual render creation. Each world camera renders to an image that the egui paint callback displays without a CPU copy.

This preserves the reasons for keeping Bevy:

- Gen's tools and the existing ECS continue to work;
- one scene can be shown through several cameras and windows without duplicating GPU assets;
- the scheduler, physics and world simulation continue while no surface is visible;
- `world-bevy` remains the single mapping from the format to Bevy;
- in-world HUD, labels and signs still render in the world's texture.

The world panel forwards pointer, keyboard, wheel and picking input to Bevy. A stand-in Bevy `Window` supplies the primary-window identity picking expects without creating an operating-system surface.

### Terminals

Each tab still owns a `portable-pty` process. The shell feeds its bytes into an `alacritty_terminal::Term` through `vte` and draws the grid in an egui widget. The app launches a plain login shell and knows no coding agent by name.

The widget covers true colour and text attributes, wide CJK cells, font fallback, box drawing, braille and emoji, selection and copy, links, mouse reporting, bracketed paste, scrollback and resize. Input-method composition is drawn where the text is entered. The desktop and a paired device share the same PTY and terminal state; a phone does not spawn an agent of its own.

### Document, history and timeline

The document editor uses egui `TextEdit`, with outline, place card, find, Follow cursor and save/reload behavior. The history graph draws one lane per branch and can step or play through entries. The timeline draws shots, tours, soundtrack, modulations, behaviors and ambience from the world document.

On a live canvas, shot moves and trims, rolling cuts, soundtrack-section edits and tour-stop holds all commit through the world's authority as ordinary history entries. The outliner and inspector use the same path. This is the application of [world-editor-stack.md](world-editor-stack.md)'s central rule: the displayed scene is a projection of a fold, not a second document.

## What the migration established

The two implementation spikes preceded the shell and answered its main uncertainties.

### Terminal spike

`spikes/egui-terminal` ran Claude Code and Codex through the PTY and terminal widget, including CJK input, resizing, terminal glyphs and load tests. The widget was subsequently exercised in the app with Chinese entered through the macOS input method. This established that the remaining work was widget development, not a new terminal emulator.

### Shared-device rendering spike

`spikes/bevy-in-eframe` rendered the song world into egui textures, in two windows and with the windows hidden. Two windows did not add measurable GPU cost, and Bevy continued ticking and rendering while hidden.

On the test Mac, one shadow cascade held the world's target frame rate more comfortably than four. The important result was not a universal frame-rate promise; it was that shared-device rendering, multiple viewports and hidden-window simulation all worked without a second Bevy window.

## Remaining application work

### F: browser and phone convergence

The goal is the same panel model in a browser, compiled to WebAssembly, rather than a separately designed mobile interface. Until that lands, the app serves the existing paired phone/tablet page described in [mobile-remote.md](mobile-remote.md).

`spikes/egui-phone` established the first browser behavior at an iPhone's size: scaling, text input and a terminal grid driven by its key bar. Remaining questions are concrete:

- real iOS and Android keyboards, input methods, dictation and touch scrolling;
- CJK font delivery, since a browser does not expose the device's system fonts to egui;
- `alacritty_terminal`'s wasm limitations: stream the desktop's grids or parse terminal bytes in the browser;
- browser device-pixel reporting differences, including Chromium phone emulation;
- keeping terminal state and world controls in step with the desktop.

When this is complete, the paired-device page, its Node build and its desktop-independent design sheets can go.

### G: Rust tooling and headless UI checks

`cargo xtask verify`, `fetch-assets` and `bundle` have replaced their shell/Python implementations. `egui_kittest` now drives shell behavior headlessly: terminal focus, panel layout, tab sizing and key routing between terminals and the timeline.

Continue moving release, smoke and deployment tooling into `xtask` as those paths are touched. The remote-page smoke test remains necessary while the paired-device page remains.

### C dependencies and a future renderer

Audit normal and build dependencies for remaining `-sys` crates, distinguishing operating-system bindings from optional bundled native libraries. Verse's optional ONNX-based audio analysis is the significant remaining native runtime; replacing it is separate work and must not remove the default model-free path.

A renderer written from scratch is **not** a prerequisite for this architecture. It becomes worth considering for WebAssembly payload size, measured render-graph limitations or compile-time constraints. Its acceptance test is format conformance: the same scenes and cameras must render consistently, and Gen's ECS-facing tools need a deliberate migration path.

## Media editing: possible extensions, not current commitments

The same shape fits an audio workstation or video editing room: a deterministic engine beneath dense, custom-drawn panels, with agents issuing commands through a logged authority. The timeline, history and native terminal work transfers.

Neither is part of the current product. Each needs an Open World Format extension first—arrangement data for audio, edit decision data for video—then its own RFC and engine evaluation. Do not grow a private media format inside the app.

## Licensing

The application stays Apache 2.0. The terminal emulator and UI stack already provide what the design needs under Apache/MIT terms. Taking Zed's editor or terminal view would mean adopting a large GPL workspace and changing the application's licensing obligations; Blender is likewise a reading reference, not a code source. [cad-lessons.md](cad-lessons.md) and [blender-lessons.md](blender-lessons.md) record the implementation lessons without importing their licenses.

## Trade-offs

- The project owns terminal, editor and accessibility behavior that a browser previously supplied.
- Browser text layout and mobile input remain real engineering work, not a free consequence of compiling to WebAssembly.
- Bevy's rendering and ECS stay, so this is not a smaller engine dependency and does not by itself solve compile time or payload size.
- The paired-device page and the native shell coexist until the browser panel work is proven; removing it earlier would strand mobile users.

## Status

- [x] Workspace on Bevy 0.20 / wgpu 30, with compatible avian and character-controller versions.
- [x] The all-Rust rule and `cargo xtask rule` ratchet.
- [x] eframe shell and shared-device Bevy world panel.
- [x] Native terminals, history, timeline, outliner, inspector, document and top bar.
- [x] Native shell as the default; desktop `wry` host and hit-test view removed.
- [ ] Browser/phone panel convergence and removal of the paired-device page.
- [ ] Remaining script-to-`xtask` migration and broader `egui_kittest` coverage.
- [ ] Native dependency audit and optional ONNX replacement decision.
- [ ] Any WebAssembly renderer or media-editing engine, each with a separate design first.
