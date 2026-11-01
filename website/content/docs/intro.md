---
slug: "/intro"
title: "Introduction"
description: "What LocalGPT is: a desktop app where prompts, documents, songs and screenplays become .world packages that people and agents edit together, the world's command line and MCP shim, and the format they all share."
---

# Introduction

LocalGPT is a desktop app, a few binaries and a file format, all open source (Apache 2.0) and all running on your machine.

**A desktop app where things you already have become places.** A Markdown document, a song, a screenplay or a prompt opens as a 3D world in one window. A world is a `.world` package — the [Open World Format](https://openworldformat.org): the world now, the log of every change made to it, and its assets. People edit it in the window; agents edit it from outside through a small local API; every change is logged, attributed, undoable and branchable.

**No agent inside, no model inside.** LocalGPT runs no model and holds no key. The desktop app opens terminals in the folder of the world that is open, and you run your own coding agent in them — Claude Code, Codex, Gemini CLI, whichever you have. The folder carries `AGENTS.md`, which says how to change the world, and the address of the world's live API, so an agent started there finds everything it needs by reading files it already reads. The app knows nothing about what runs in a tab.

The two halves meet at a simple point: an agent that can edit files is one command away from an agent that edits worlds, and the worlds are a format no engine owns.

## The pieces

| | What it is | Get it |
|---|---|---|
| [**LocalGPT**, the desktop app](/docs/app/) | One window, every mode: a world from a document (MD), from a song (Verse), from a prompt (Gen), or any `.world` opened as a live canvas, with terminals for your agents beside it | `cargo run -p localgpt-app` from a checkout; `LocalGPT.app` from `apps/app-desktop/` |
| [**LocalGPT Gen**](/docs/gen/) | The world builder as a binary: its 95 tools over MCP, the live canvas, collaborative sessions, headless | `cargo install localgpt-gen` |
| [**Previs**](/docs/gen/previs/) | A screenplay in; per scene a `.world` with set, cast and coverage, plus a board and a shot list. Deterministic, no model | [localgpt.md](https://localgpt.md) in the browser, or `cargo install localgpt-previs` |
| [**`localgpt`**, the world's command line](/docs/cli-commands/) | `localgpt world`: commit, undo, log, tips, verify, screenshot, and `localgpt world mcp`, the shim that serves an open world's tools to any MCP agent | `cargo install localgpt` |
| [**localgpt.world**](https://localgpt.world) | The gallery, and the one web viewer every exported page carries | a browser |

## Where to start

- **Walk into something.** [The desktop app](/docs/app/): open a document, a song or a world, and keep the window open while an agent changes it from a terminal beside it — the [live canvas](/docs/gen/live-canvas/).
- **Previs a scene.** Drop script pages on [localgpt.md](https://localgpt.md); nothing is installed or uploaded. Then open one scene live and revise the shots by asking — [Previs](/docs/gen/previs/).
- **Drive the 3D window from your own agent.** `localgpt-gen mcp-server` for Claude Desktop, Claude Code, Codex, Gemini CLI and editors — [MCP Server](/docs/gen/mcp-server/); or `localgpt-gen --mcp-relay`, and the agent drives the window you are looking at — [MCP Relay](/docs/gen/cli-mode/).
- **Install.** [Installation](/docs/installation/) and the [Quick Start](/docs/quick-start/).

## What is on disk

LocalGPT follows the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/) on Linux and macOS; `LOCALGPT_DATA_DIR` and `LOCALGPT_STATE_DIR` move a directory, and `LOCALGPT_PROFILE=<name>` moves them all to `localgpt-<name>`.

```text
~/.local/share/localgpt/
├── gen-workspace/
│   ├── skills/<name>/           # saved worlds (world.ron, assets/)
│   ├── documents/<key>/         # the app's world for each document
│   └── sessions/<name>/         # collaborative sessions (a package each)
└── models/
    ├── pack/                    # the shared CC0 asset pack (scripts/fetch-assets.sh)
    └── llm/                     # the shared GGUF for MD's and Verse's llm tiers (scripts/fetch-model.sh)

~/.local/state/localgpt/
├── gen-settings.json            # what the app opened last
├── app-remote.json              # the paired phones' key hashes (0600)
└── logs/localgpt-app.log        # the app's log, when it has no terminal
```

There is no config file: nothing in LocalGPT takes a model, a key or an agent's settings. A `.world` you open stays where it is, and the app writes its live API's address into the package's `.live/` folder while it is open.

## How it works

1. **A world is a folder.** `manifest.json` is the world now, `ops.jsonl` every change made to it, `assets/` what it references, `AGENTS.md` how to change it.
2. **One authority at a time.** While an app has a world open, every change goes through its API — from the window, from `localgpt world`, from an agent over MCP — and lands as one entry in the log, attributed, undoable and branchable. With nothing open, `localgpt world` is the authority itself.
3. **Agents stay outside.** A terminal in the app, a shell anywhere, an MCP client: the agent brings its own model and login, and LocalGPT knows it only by what it commits.
4. **One format, many renderers.** The window draws the world with Bevy; the web viewer draws the same package; the format's conformance worlds keep them agreeing.

## Next Steps

- [Installation](/docs/installation) — install the binaries, or build the desktop app from source
- [The desktop app](/docs/app) — one window, every mode
- [Quick Start](/docs/quick-start) — a world and an agent in minutes
- [Live canvas](/docs/gen/live-canvas) — a `.world` that agents edit while you watch
- [Previs](/docs/gen/previs) — from a screenplay to a board to a canvas
- [Architecture](/docs/architecture) — the crates and how they depend on each other
- [CLI Commands](/docs/cli-commands) — `localgpt world`, the shim, and Gen's own flags
