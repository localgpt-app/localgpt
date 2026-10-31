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

What is open has its panel on the left — a document, or a package's history with its [outliner](#outliner) in a tab beside it — the world is in the middle, the agents are on the right, and the [timeline](#timeline) runs across the foot of all three. A click in the world gives it the mouse and the keys; a click in a panel or a terminal gives them back to the page.

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

On a live canvas the timeline edits the world. Drag a shot to move it, or an edge to trim it; where two shots meet, the cut between them rolls. The song's sections are the ruler's § band: drag a section's start along it to recut the song, double-click the band to start a section there, or drag a start onto its neighbour to join the two. Drag the end of a tour stop's hold, and the camera waits longer there while the stops after it come later by as much. The lock keeps a track's drags off. A drag snaps to beats, sections or cuts as the toolbar says (hold **⌥** to drag freely), draws a ghost while it is held, and writes nothing until it is let go — then it is one entry in the history, through the same authority the agents use, so it undoes like any other edit. **Esc** lets go without writing. With review on (`LOCALGPT_REVIEW=1`), an agent's proposal waits in the timeline too: the toolbar asks, and the shots it moves wait dashed under its name until you accept or reject it.

## The outliner and the inspector {#outliner}

On a live canvas the left panel has a second tab, **Outliner**: the world's things as a tree — what holds what, what kind each is, what moves it (a behavior, the song) and a camera's shot — read from the world's document, so a lantern pulsing on the beat shows the glow it was given and the timeline shows what the song does to it. Find a thing by name, or keep only shapes, meshes, lights, cameras, sounds or groups. The eye on a row hides it in the window alone; the world keeps it. A click selects, as a click in the world does, and Shift or ⌘ adds to the selection; a double-click, or **Fly to**, takes the camera to it, and a shot's **Look through** looks through its camera. Drag a row onto another to put it inside, or beside one to join what holds that row (at the edge of a top-level row, out to the top): it stays where it stands, or keeps its offset with **⌥** held, and a selected row carries the selection. A row an agent just changed says who, for a couple of minutes, and the inspector marks the fields it changed; what a waiting proposal would change is dashed under its author's name.

Under the tree the inspector shows the selected thing part by part. Its name, transform and visibility, a shape's sizes, a material's colour, roughness, metallic and glow, a light's colour, intensity, range, cone and shadows, a camera's lens and aim, a shot's place on the clock, and a sound's volume and reach edit: drag a number to scrub it, the world following (Shift for a tenth as fast), or click it to type one; a slider sets where it is pressed, and a colour opens a picker. Letting go is one entry in the history for every selected thing that has the field, through the same authority the agents use — a drag moves each by as much as the one shown, a typed value is every one's — and **Esc** before then puts the world back. A refusal, such as a name another thing has, is said under the field, and the field shows the world's value again. A behavior's **×** takes it off; what the song moves links to its track on the timeline. Adding a behavior, a behavior's own fields and another kind of shape or light come next.

## The agents {#agents}

**+** at the top of the right-hand panel opens a tab: **Claude Code**, **Codex**, **agy**, **Gemini CLI**, **OpenCode**, or a **Shell**. Each is a full terminal — the agent's own interface, its approvals, `/commands` and sign-in — started through your login shell in the folder of what is open: the package, the document's folder, or the song's world. So an agent finds a package's `AGENTS.md` and `.live/endpoint.json` from where it starts, and `localgpt world mcp` gives it the world's tools.

The app knows each agent by its command's name and nothing more: no keys, no models, no settings — those stay with the agent. A link in a terminal opens in your browser. **Shift+Enter** sends a new line the way Claude Code expects one. A tab whose program has ended stays open to read; close it with its **×**.

A tab's dot says what its agent is doing, read from the marks Claude Code and Gemini CLI put in their terminal titles. It is blue while the agent works and green once it stops, and amber when Gemini CLI asks you something. On a live canvas each turn that ends is marked in the history.

## A phone or a tablet {#phone}

**Phone**, at the right of the top bar, lets a phone or a tablet on the same network use the window. Turn it on and it shows a QR code. Scan the code with the device's camera, and its browser opens the app's page, paired. The page there has three views at the thumb:

- **Agents**: each tab's terminal, the agent's own. Under it are **esc**, **⇥**, **⇧⇥**, the arrows, **^C** and **⏎**, and a box to write or dictate a prompt.
- **World**: the world as the window shows it, the shots to look through, and the song to play or pause.
- **History**: the history, with its transport.

A waiting proposal can be accepted or rejected from any view. When an agent's turn ends, the phone says so. A tab started on the phone shows up in the window too.

A terminal has one size. The page that last typed into it or clicked into it sets the size, and the other page draws the terminal scaled to fit.

The code works once, for ten minutes. The device keeps a key of its own, and the app keeps only the key's hash, in `~/.local/state/localgpt/app-remote.json`. The panel lists the paired devices and can forget any of them, and the button names a device while it is attached.

The connection is plain HTTP on port 9880. A paired device can type into the terminals, which makes it a shell on your Mac, so use this on a network you trust or over a VPN you run. `LOCALGPT_APP_REMOTE=on` turns it on at launch and prints a pairing link to the terminal; `off` keeps it off whatever was remembered.

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

For work on the interface itself: `npm run dev` in `crates/app/ui` serves it with reload, and `LOCALGPT_APP_UI_URL=http://localhost:5174` points the app at it; `LOCALGPT_APP_UI_OPEN=claude,shell` starts with those tabs (`outliner` starts the left panel on its Outliner tab); `LOCALGPT_APP_UI=none` leaves the interface out. In a plain browser, `?preview=document`, `world` or `song` shows the page with stand-in data, and `npm run design` serves the design sheets new interface is reviewed on before it is built.

`localgpt-gen` stays a binary of its own for the MCP server, headless generation and `--host`/`--join`, and `cargo install localgpt-gen` keeps working; it takes its prompts from the terminal it runs in, and there is no second desktop bundle.

## What is next {#next}

Adding a behavior and its fields, and another kind of shape or light, in the inspector; and the Windows host. `docs/rfcs/authoring/the-page.md` in the repository records the decision and the order.
