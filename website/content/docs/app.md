---
slug: "/app"
title: "The desktop app"
description: "LocalGPT's one desktop app: a prompt, a Markdown document, a song or a .world package opens as a world in the same window — the modes, the keys, the model menu, collaboration, and how to build LocalGPT.app."
---

# The desktop app

There is one LocalGPT desktop app, `localgpt-app`, and its modes are the apps that used to be separate: a world from a **prompt** (Gen), from a **Markdown document** (MD), from a **song** (Verse), or any **`.world` package** opened as a live canvas. The shell is Gen's — a Bevy viewport, an egui prompt panel, an agent loop — assembled from the gen library, so everything Gen's window can do, this one can.

```bash
# from a checkout of the repository
cargo run -p localgpt-app                             # reopens what was opened last
cargo run -p localgpt-app -- --md notes.md            # a document as a world
cargo run -p localgpt-app -- --song track.mp3         # a song as a world that performs it
cargo run -p localgpt-app -- --prompt "a quiet harbor at dusk"
cargo run -p localgpt-app -- --world scene-1.world    # a package, opened live
cargo run -p localgpt-app -- --md notes.md --no-author   # rules and fences only, no model
```

**Open…** at the top of the window takes the same three kinds of file. The app is not on crates.io; it ships as a build — see [Installation](/docs/installation/#the-macos-app-bundle) for `LocalGPT.app`.

## The modes {#modes}

### A prompt {#prompt}

Press **F2** for the prompt panel, type, press **Enter**. The reply streams in with each tool call shown as it runs, and prompts sent while the agent is busy wait in a queue. The agent is Gen's own loop, with its slash commands (`/model <name>`, `/new`, `/clear`, `/quit`) and its toolbelt — the `core` profile by default, unless `--tools standard|full` or Gen's settings say otherwise. A first prompt can be given at launch with `--prompt`.

### A document {#document}

A Markdown file opens with an editor pane (**F3**) beside the viewport. Every `##` section is a place along a path. The file saves and the world rebuilds shortly after typing stops, **in place**: only the sections whose text changed are replaced, so the place you are standing in does not move.

The rule-based draft is the first frame; the app's model then authors each section's place in the background — one request per section with a JSON plan, so it works on the zero-config default, a signed-in CLI backend, as well as on an API model or a local GGUF. Builds land in MD's sidecar next to the document, keyed by each section's content hash, so a restart reopens them with no model call. `--no-author` keeps to the rules and any ```` ```world ```` fences.

The text and the world map onto each other: the outline labels each section (fence, model, authoring…, draft); moving the cursor into a section flies the camera there; a place card shows the model's description and what the place is made of; picking a thing in the world selects it in the inspector (**F1**).

### A song {#song}

A song opens as the world Verse builds for it. The on-device analysis — tempo, a beat grid, section boundaries, an energy curve, a mood — picks the world and furnishes it from the shared CC0 pack; nothing is uploaded and no model is involved. The soundtrack plays on the shared audio engine and the world's **modulations** follow it: the heroes' scale breathes with the energy, the glow follows the beat. A small transport at the bottom of the window shows the song and pauses it. The analysis is cached beside the app as a content-keyed sidecar, so a track is analyzed once.

### A package {#package}

A folder holding a `.world`'s `manifest.json` opens as a **live canvas**: the window holds the world's authority and serves the folder's local API, and no agent runs in the window. Agents — Claude Code, Codex, `localgpt`, a script — change the world from outside through batches of ops, each committed whole or refused with a reason; you edit by hand with handles, the inspector and the outliner; the history rail and the history panel (**H**) show every branch. This is the mode previs boards open in. The whole of it is on the [Live canvas](/docs/gen/live-canvas/) page.

## Models {#models}

The model menu lists what this machine can use right now:

- installed CLI backends — Claude CLI, Gemini CLI, Codex — which bring their own login, no API key;
- models pulled into a local Ollama;
- every GGUF in the model folder LocalGPT's apps share (`~/.local/share/localgpt/models/llm`), run in-process as `gguf/<name>` when the app is built with `local-llm` (`local-llm-metal` on Apple Silicon, which the macOS bundle is).

The choice is remembered in Gen's own settings (`~/.local/state/localgpt/gen-settings.json`), and document authoring uses the same model. The app reads and writes **no** `config.toml` and does not touch the assistant's memory workspace: the world modes keep Gen's settings and workspace, so nothing from the assistant's memory reaches a world unless you put it there.

If the chosen model is a CLI backend that isn't installed, the panel says so and points at the menu. Opened from Finder, the app reads your login shell's `PATH`, so `claude`, `gemini` and `codex` installed with Homebrew, npm or into `~/.local/bin` are found. With a CLI backend, tools run through the MCP relay (`localgpt-app mcp-server --connect`), so the panel shows the streamed text but not each tool call.

## Keys {#keys}

| Key | What it does |
|-----|--------------|
| **F1** | Gen's inspector: outliner, detail panel, world bar |
| **F2** | The prompt panel |
| **F3** | The document pane |
| **Enter** | In the 3D view, jump to the prompt box; in the box, send |
| **Shift+Enter** | A new line in the prompt |
| **Esc** | Leave the prompt box so WASD moves you again |
| **W A S D**, mouse | Fly and look; **Q** descends, Space rises |
| **H** | The history panel (live canvas) |
| **1–9** | Look through a shot (a previs package) |

While you type, keys don't reach the world.

## Collaborate {#collaborate}

A section under the model menu hosts or joins a [collaborative session](/docs/gen/multiplayer/) without command-line flags: **Host** picks a name, port and PIN (or an open session), then shows the PIN and how many guests are connected; **Join** browses the LAN or takes an address and pairs with the PIN. Guests' prompts run on a scene-only agent, as with `localgpt-gen --host`.

## Logs and settings {#logs}

Launched from a terminal the app logs to stderr (`RUST_LOG` sets the level, `warn` by default); opened from Finder it logs to `~/.local/state/localgpt/logs/localgpt-app.log`. Its settings and the last-opened file are in `gen-settings.json`; the worlds it saves go to Gen's workspace under `skills/<name>/`.

## Build LocalGPT.app {#build}

```bash
apps/app-desktop/macos/build-app.sh            # release build → apps/app-desktop/dist/LocalGPT.app
apps/app-desktop/macos/build-app.sh --dmg      # also dist/LocalGPT-<version>.dmg
apps/app-desktop/macos/build-app.sh --debug    # a quick bundle of the debug build
```

Without credentials the bundle is ad-hoc signed and runs on the Mac that built it. Given `APPLE_SIGNING_IDENTITY` and `APPLE_NOTARY_PROFILE`, the script signs with a Developer ID, enables the hardened runtime and notarizes. The shared downloads — the asset pack and, for local models, the GGUF — are not in the bundle: `scripts/fetch-assets.sh` and `scripts/fetch-model.sh` fetch them once per machine.

`localgpt-gen` stays a binary of its own for the MCP server, headless generation and `--host`/`--join`, and `cargo install localgpt-gen` keeps working; there is no second desktop bundle.

## What is next {#next}

The assistant as a mode of the same window — chat over the assistant's own config, memory and policy, one turn at a time — and a release pipeline that builds the bundle for every platform. Neither exists yet; `docs/world-strategy.md` §13 in the repository records the decisions and their dates.
