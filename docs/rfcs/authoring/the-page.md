# The page: one interface over the world

**Status:** decided and built on macOS, 2026-10-30. Supersedes the answer
of [egui-or-not.md](egui-or-not.md) (stage egui out onto `bevy_ui` and
feathers) and the chrome half of
[world-editor-stack.md](world-editor-stack.md); builds tier 2 of
[agents-outside-the-window.md](agents-outside-the-window.md), the terminal
pane, and retires its tier 0, the herdr pane. The app half of
[the-harness-goes.md](the-harness-goes.md) — the prompt panel, the model
menu, `--prompt`, `mcp-server --connect` — went with it.

## The problem

The desktop app drew its chrome twice over: egui panels, and the same
panels again on `bevy_ui` and feathers behind a feature flag, staged one
surface at a time. Three things it now has to do decided the stack:

- **Playback and a history that branches**: the log as a `git log
  --graph`, scrubbed and played.
- **A multi-track timeline** for the world's clock — shots, the song and
  what it moves — in the way video editors do it.
- **The agents in a panel** — Claude Code, Codex, agy and the rest, whole:
  their own TUIs, approvals, `/commands` and sign-ins. That is a complete
  terminal emulator, not a transcript.

What other editors chose, read for this (none of their code is used):
OpenCut's rewrite (GPUI desktop scaffold, React web stub, MIT) and
waterblower/opencut (a 33k-line GPUI editor, no licence: read only). GPUI
shows an outside frame only as a macOS `CVPixelBuffer` — its Windows
`draw_surfaces` is empty — so that editor converts every video frame with a
Metal shader and plays no video off macOS. Orca runs Claude Code and Codex
side by side on Electron, xterm.js and node-pty. Zed's terminal is GPL;
herdr's builds libghostty-vt with Zig.

## The decision

**One web page in one transparent web view over the Bevy window**: React,
TypeScript and Tailwind; CodeMirror for documents; xterm.js for the agents'
terminals (`crates/app/ui`). Bevy owns the window and draws the world; the
page leaves a hole where the world shows, reports where it is, and the world
camera's viewport is fitted to it. A click inside the hole falls through to
Bevy — on macOS an AppKit view whose hit test answers nothing there — and
the keyboard follows the click.

- **The terminal is the industry's.** xterm.js is what VS Code, Cursor and
  Orca run agents in. The native routes were to write one.
- **The world stays native.** Core Animation composites the page over the
  Metal layer with no copy, at full quality; no frame enters the page.
- **The rest is easy there**: a graph, a timeline, a Markdown editor; agents
  write React well; and the design sheets' components are the app's own.

What it costs: a Node build inside the app crate (the release bundle runs
it, CI has a job for it); a host per platform — macOS is built, Windows
(WebView2, a transparent child over wgpu, the hole cut with `SetWindowRgn`)
is next and Linux (webkit2gtk, X11 child views only) after; and input
belongs to whichever side was clicked last.

## What it replaced

| Surface | Before | Now |
|---|---|---|
| Prompt panel, model menu, Collaborate, the viewer's panel | egui | gone: agents are processes in the page's terminals; `localgpt-gen --host`/`--join` keep collaboration |
| Outliner, detail panel, world bar | egui, and feathers | gone for now; selection, the canvas's gestures and the remote inspector's WebSocket stay |
| History rail, history panel | `bevy_ui` | the page's graph, with its own transport |
| Review bar | egui, and feathers | Enter and Esc; `GET /proposals` carries the warnings |
| Shot strip | egui, and feathers | 1–9 look through; the shot list and its thumbnails stay for the timeline |
| Gallery | egui, and feathers | Open… |
| Document pane, now playing, Open… | egui | the page |
| herdr's pane (tier 0) | the bar's menu | the page's terminals; turn marks arrive as `TurnEnded` |
| Gen's desktop mode (`--desktop`) | the prompt panel | gone: `localgpt-gen` takes prompts from its terminal |

`bevy_egui`, the `feathers` feature and the widget seam (`GET /widgets`)
are gone from the workspace. What stays on `bevy_ui` is the world's own —
HUD, labels, signs, notifications, dialogue — and the canvas's selection
box: things drawn in the world, not around it.

## The seam

Each side keeps its own state. The page asks with the app's own messages —
`SeekTo`, `DocumentAsk`, `OpenFile`, `ToggleSong` — and the app sends it
snapshots: the history, the document's view and its text, the song, which
panels have something to show (`crates/app/src/webui`). Only the host is
per platform; the protocol and the terminals are built and tested
everywhere. So the page also opens in a plain browser:
`?preview=document|world|song` stands the app in with fixtures, which is how
it is looked at and checked with nothing behind it.

New interface is designed first, as a static sheet drawn with the app's own
components and fixtures (`npm run design`, `crates/app/ui/design/`), and
wired only once the sheet is reviewed. The timeline is the first.

**The timeline runs across the window** (reviewed and built 2026-10-30),
under the left panel, the world and the agents. It is the one panel that wants
width: across the window its lanes get 1,264 px of a 1,440 px window,
under the world alone 484 px — a three-minute song at 2.5 px a second,
enough to see the cuts and too little to see a beat. Its height comes from
the terminal and the history instead. The song bar under the world goes
into the timeline's toolbar; the history keeps its own transport, because
stepping through entries is authoring time and the timeline's is world
time.

The app reads the timeline from the world's document — the canvas's on
screen, or the folder the scene loaded — and sends it as one view
(`crates/app/src/timeline.rs`; the page's half is
`ui/src/timeline/LiveTimeline.tsx`). The clock is the song's position while
it plays and is heard, else the app's own, and the world is shown at it:
`Soundtrack::position` holds while paused, so the modulations do. A drag
lets go as one `ShotOps` batch, the path a reorder took, so a trim is an
entry in the history; the cut between two shots that meet rolls. Things
moved alike share a track, or a song world's boulders would fill the
panel. A shot's frame is read back from the thumbnail its camera renders.

## Next

1. The timeline's rest: the tour and the song's sections edited there —
   recutting a section and holding a stop longer are drawn on the sheet
   (2026-10-31), for review before they are wired. The authority takes `ModifyWorld` now, so a
   recut section is one entry like a trim, and an agent can give a package
   its song; hiding a modulation's or a behavior's track stands what it
   moves still, as a mute silences.
2. The outliner and inspector in the page, over the remote inspector's
   builders and `PanelIntents` patches.
3. Turn marks from the page's own terminals (`TurnEnded`).
4. The Windows host; then Linux.
5. A render check of the page in CI, through `?preview=`.

## Not decided

- Whether the page's panels also ship on localgpt.world, over the three.js
  viewer.
- Whether `localgpt-gen --live`, the smoke's canvas, gets the page too.
