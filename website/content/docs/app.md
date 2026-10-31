---
slug: "/app"
title: "The desktop app"
description: "LocalGPT's one desktop app: a Markdown document, a song or a .world package opens as a world in the same window, with the history, the document, the timeline and your own coding agents around it — the modes, the agents' tabs, the keys, and how to build LocalGPT.app."
---

# The desktop app

There is one LocalGPT desktop app, `localgpt-app`, and its modes are the apps that used to be separate: a world from a **Markdown document** (MD), from a **song** (Verse), or any **`.world` package** opened as a live canvas, which agents change from outside. The world is Gen's Bevy viewport; around it is one interface — the document and its outline, the history and its playback, the timeline of the world's clock — and, in tabs, the coding agents you already use, each in a full terminal.

```bash
# from a checkout of the repository
(cd crates/app/ui && npm ci && npm run build)         # the interface, once (node 22)
cargo run -p localgpt-app                             # reopens what was opened last
cargo run -p localgpt-app -- --md notes.md            # a document as a world
cargo run -p localgpt-app -- --song track.mp3         # a song as a world that performs it
cargo run -p localgpt-app -- --world scene-1.world    # a package, opened live
cargo run -p localgpt-app -- --md notes.md --no-author   # rules and fences only, no model
```

**Open…** at the top of the window takes a document, a song or a world file. With nothing given and nothing remembered, the app opens a starter song from the CC0 pack. The app is not on crates.io; it ships as a build — see [Installation](/docs/installation/#the-macos-app-bundle) for `LocalGPT.app`.

The interface is macOS's for now. On Windows and Linux the window shows the world alone, with none of the panels below; their hosts come next.

## The window {#window}

What is open has its panel on the left — a document, or a package's history — the world is in the middle, the agents are on the right, and the [timeline](#timeline) runs across the foot of all three. A click in the world gives it the mouse and the keys; a click in a panel or a terminal gives them back to the page.

## The modes {#modes}

### A document {#document}

A Markdown file opens with its panel: the file's outline, the selected section's place, and the text in an editor. Every `##` section is a place along a path. The file saves and the world rebuilds shortly after typing stops, **in place**: only the sections whose text changed are replaced, so the place you are standing in does not move. The file stays the source of truth: an edit made in another editor reloads the panel when it has no unsaved typing of its own, and is flagged rather than overwritten when it has.

The rule-based draft is the first frame; a model then authors each section's place in the background — one request per section with a JSON plan, so it works on a signed-in CLI backend as well as on an API model or a local GGUF. Builds land in MD's sidecar next to the document, keyed by each section's content hash, so a restart reopens them with no model call. The **Author** toggle stops and resumes it; `--no-author` keeps to the rules and any ```` ```world ```` fences.

The text and the world map onto each other: the outline labels each section with where its place came from (fence, model, authoring…, recipe, draft); a click on a section, or the cursor moving into one (**Follow cursor**), flies the camera there; the place card shows the model's description and what the place is made of, and a click on a thing selects it in the world and flies to it; clicking a thing in the world selects its section and brings its heading into view.

The model is the one named in Gen's settings (`default_model` in `gen-settings.json`), else a GGUF already in the shared model folder when the app is built with `local-llm` (`local-llm-metal` on Apple Silicon, which the macOS bundle is), else the first installed CLI backend — Claude CLI, Gemini CLI, Codex. The app shows no model menu and manages no agent's configuration.

### A song {#song}

A song opens as the world Verse builds for it. The on-device analysis — tempo, a beat grid, section boundaries, an energy curve, a mood — picks the world and furnishes it from the shared CC0 pack; nothing is uploaded and no model is involved. The soundtrack plays on the shared audio engine and the world's **modulations** follow it: the heroes' scale breathes with the energy, the glow follows the beat. The song is on the [timeline](#timeline), its waveform over its bars and sections, and it is played, paused and moved through there. The analysis is cached as a content-keyed sidecar, so a track is analyzed once.

### A package {#package}

A folder holding a `.world`'s `manifest.json` opens as a **live canvas**: the window holds the world's authority and serves the folder's local API, and no agent runs in the window. Agents — Claude Code, Codex, `localgpt world`, a script — change the world through batches of ops, each committed whole or refused with a reason; you edit by hand with handles in the viewport. The history panel draws the log as a graph, newest at the top and one lane per branch, with **main**'s head marked; a click on an entry shows the world as it was then, and the transport under the graph steps through the entries in the order they were written, or plays them. Moving through history writes nothing. A previs package's shots are on the [timeline](#timeline), where they are trimmed and moved like clips. This is the mode previs boards open in; the whole of it, the editing keys included, is on the [Live canvas](/docs/gen/live-canvas/) page.

## The timeline {#timeline}

Across the foot of the window runs the world's clock, in every mode: the shots, the song and what the song moves, on one ruler of bars and sections. Each track is read from the world's document, not kept beside it:

- **Shots** — the cameras that carry a shot, a previs package's coverage, each from its in point to its out point with its frame rendered from its own camera. Coverage that overlaps its master stacks under it.
- **Camera** — a tour's stops, timed by its speed and the pause at each.
- **Soundtrack** — the song's waveform; its stems fold out under it when the analysis separated them.
- **Modulations** and **behaviors** — what the song moves, and what loops. Things moved alike, every boulder breathing with the energy, share one track.
- **Ambience** — the world's ambient layers and emitters.

The clock is the song while one plays; otherwise the app keeps it, and a previs with no song runs on its shot list. Where the playhead stands is when the world is shown: paused, the modulations hold there, and a click or a drag on the ruler moves it. Double-click a shot to look through it from its in point; while you look through a shot, moving the playhead cuts to the shot under it, so playing plays the cut. The toolbar plays and pauses, steps from cut to cut, loops the selected shot (else the section under the playhead) and zooms; a pinch, or ⌘ with the wheel, zooms about the pointer, and a sideways swipe pans. **S** and the speaker solo and mute the song and the ambience; the eye hides what a modulation or behavior track moves, and it stands still where the world puts it — like a mute, the world is not changed.

On a live canvas the shots are edited here. Drag a shot to move it, or an edge to trim it; where two shots meet, the cut between them rolls. A drag snaps to beats, sections or cuts as the toolbar says (hold **⌥** to drag freely), draws a ghost while it is held, and writes nothing until it is let go — then it is one entry in the history, through the same authority the agents use, so it undoes like any other edit. **Esc** lets go without writing. With review on (`LOCALGPT_REVIEW=1`), an agent's proposal waits in the timeline too: the toolbar asks, and the shots it moves wait dashed under its name until you accept or reject it.

## The agents {#agents}

**+** at the top of the right-hand panel opens a tab: **Claude Code**, **Codex**, **agy**, **Gemini CLI**, **OpenCode**, or a **Shell**. Each is a full terminal — the agent's own interface, its approvals, `/commands` and sign-in — started through your login shell in the folder of what is open: the package, the document's folder, or the song's world. So an agent finds a package's `AGENTS.md` and `.live/endpoint.json` from where it starts, and `localgpt world mcp` gives it the world's tools.

The app knows each agent by its command's name and nothing more: no keys, no models, no settings — those stay with the agent. A link in a terminal opens in your browser. **Shift+Enter** sends a new line the way Claude Code expects one. A tab whose program has ended stays open to read; close it with its **×**.

## Keys {#keys}

| Key | What it does |
|-----|--------------|
| **W A S D**, mouse | In the world: fly and look; **Q** descends, Space rises |
| **1–9** | Look through a shot (a previs package); **Esc** hands the free camera back |
| **Esc** | Clear the selection |
| **Space** | In the timeline: play and pause |
| **← →**, **Shift+← →** | In the timeline: a beat back or on (a second with no beat grid); the previous or the next cut |
| **Home**, **End** | In the timeline: to the start, to the end |
| **Enter**, **Esc** | Accept or reject an agent's proposal, in the world or the timeline |
| **⌥** while dragging | In the timeline: no snapping |
| **⌘C ⌘V ⌘A ⌘Z** | In the page: copy, paste, select all, undo |
| **⌘F** | In the document editor: find |
| **Shift+Enter** | In an agent's terminal: a new line |

The keys go where the last click went: typing in a panel or a terminal never moves the world, and keys in the world never reach a panel.

## Logs and settings {#logs}

Launched from a terminal the app logs to stderr (`RUST_LOG` sets the level, `warn` by default); opened from Finder it logs to `~/.local/state/localgpt/logs/localgpt-app.log`. Its settings and the last-opened file are in Gen's `gen-settings.json`; the worlds it builds go to Gen's workspace.

## Build LocalGPT.app {#build}

```bash
apps/app-desktop/macos/build-app.sh            # release build → apps/app-desktop/dist/LocalGPT.app
apps/app-desktop/macos/build-app.sh --dmg      # also dist/LocalGPT-<version>.dmg
apps/app-desktop/macos/build-app.sh --debug    # a quick bundle of the debug build
```

The script builds the interface first (`crates/app/ui`, which needs node 22) and the release binary carries it inside. Without credentials the bundle is ad-hoc signed and runs on the Mac that built it. Given `APPLE_SIGNING_IDENTITY` and `APPLE_NOTARY_PROFILE`, the script signs with a Developer ID, enables the hardened runtime and notarizes. The shared downloads — the asset pack and, for local models, the GGUF — are not in the bundle: `scripts/fetch-assets.sh` and `scripts/fetch-model.sh` fetch them once per machine.

For work on the interface itself: `npm run dev` in `crates/app/ui` serves it with reload, and `LOCALGPT_APP_UI_URL=http://localhost:5174` points the app at it; `LOCALGPT_APP_UI_OPEN=claude,shell` starts with those tabs; `LOCALGPT_APP_UI=none` leaves the interface out. In a plain browser, `?preview=document`, `world` or `song` shows the page with stand-in data, and `npm run design` serves the design sheets new interface is reviewed on before it is built.

`localgpt-gen` stays a binary of its own for the MCP server, headless generation and `--host`/`--join`, and `cargo install localgpt-gen` keeps working; it takes its prompts from the terminal it runs in, and there is no second desktop bundle.

## What is next {#next}

The tour and the song's sections edited on the timeline; the outliner and inspector in the page; and the Windows host. `docs/rfcs/authoring/the-page.md` in the repository records the decision and the order.
