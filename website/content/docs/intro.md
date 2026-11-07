---
slug: "/intro"
title: "Introduction"
description: "LocalGPT is a native desktop app and a set of local tools for building, editing and sharing Open World Format packages with your own coding agents."
---

# Introduction

LocalGPT turns a Markdown document, a song or a screenplay into a 3D world you can walk through and edit. A world is an [Open World Format](https://openworldformat.org) package: the scene, its assets and the history of every change.

The desktop app is native Rust: eframe and egui draw the panels, and Bevy renders the world. Your own coding agent can work in a terminal beside it or connect over MCP. LocalGPT provides the world, editing tools and local API; the agent provides its own model and login.

## Choose a starting point

| I want to… | Start here |
|---|---|
| Open a document, song or world | [The desktop app](/docs/app/) |
| Turn a screenplay into a previs board | [localgpt.md](https://localgpt.md), then [the previs workflow](/docs/gen/previs/) |
| Build a world with my coding agent | [Quick start](/docs/quick-start/) and [MCP server](/docs/gen/mcp-server/) |
| Edit a `.world` from a script or CI | [`localgpt world`](/docs/cli-commands/) |
| Host a world for other people | [Collaborative sessions](/docs/gen/multiplayer/) |
| Open a world in a browser | [localgpt.world](https://localgpt.world) |

## The pieces

- **[LocalGPT app](/docs/app/)** — documents, songs and live `.world` packages in one native window, with history, a timeline, an inspector and terminals.
- **[Gen](/docs/gen/)** — the world builder and its 88 MCP tools, also available as a standalone binary for headless work and collaborative sessions.
- **[Previs](/docs/gen/previs/)** — deterministic screenplay-to-board drafting in the browser or on the command line.
- **[`localgpt world`](/docs/cli-commands/)** — a package's command line and MCP shim: commit, undo, inspect history, verify and drive an open canvas.
- **[localgpt.world](https://localgpt.world)** — the gallery and browser viewer for the same packages.

## What a world contains

```text
scene.world/
├── manifest.json       # the world at its current head
├── ops.jsonl           # every committed change
├── assets/             # meshes, textures and sounds by content hash
└── AGENTS.md           # how an external agent can edit this world
```

One authority writes at a time. While an app has the package open, edits from the window, the CLI and agents all go through its local API. With no app open, `localgpt world` commits directly to the folder. Both paths produce the same attributed, undoable and branchable history.

[Install LocalGPT](/docs/installation/) or follow the [Quick start](/docs/quick-start/).
