---
slug: "/ecosystem"
title: "Ecosystem"
description: "The LocalGPT family of apps and sites, the Open World Format they share, and the wider landscape of AI assistants and explorable world systems."
---

# Ecosystem

The LocalGPT family, the open format at its center, and the broader landscape of AI assistant implementations and explorable world systems.

## LocalGPT Apps

Everything under [localgpt.app](https://localgpt.app) runs locally and is open source (Apache 2.0), in [one repository](https://github.com/localgpt-app/localgpt).

| | What it does |
|-----|--------------|
| **[LocalGPT](/docs/app)**, the desktop app | One window, every mode: a world from a prompt, from a Markdown document or from a song, or any `.world` package opened as a live canvas that agents edit from outside |
| **[LocalGPT Gen](/docs/gen)** | The world builder as a binary: interactive, an [MCP server](/docs/gen/mcp-server) for Claude Desktop, Claude Code, Codex, Gemini CLI and editors, [headless](/docs/gen/headless) generation, [collaborative sessions](/docs/gen/multiplayer), the [live canvas](/docs/gen/live-canvas) |
| **[Previs](/docs/gen/previs)** | Drop a screenplay on [localgpt.md](https://localgpt.md/) — a PDF, a Final Draft file, Fountain or Markdown — and get a board in the tab: a set, stand-ins and coverage per scene with real lenses, nothing installed, nothing uploaded. `localgpt-previs` is the same draft on the command line. Then revise the shots in the live canvas |
| **[The assistant](/docs/intro)** | A local AI agent with persistent Markdown memory, heartbeat and cron, a kernel-enforced shell sandbox, an HTTP API, an MCP server — and `localgpt world`, a `.world` package's command line |
| **[LocalGPT Verse](/docs/verse)** | Listens to your music and imagines a living 3D world for every song — analyzed, staged and performed on your machine. A mode of the desktop app, and a window of its own |
| **[LocalGPT MD](/docs/md)** | Opens a Markdown file and walks through it as a 3D world; every section becomes a place, and saving rebuilds it. A mode of the desktop app, with an editor pane and in-place rebuilds, and a window of its own |

The sites: [localgpt.app](https://localgpt.app/) is the hub and the only docs site; [gen.localgpt.app](https://gen.localgpt.app/) and [verse.localgpt.app](https://verse.localgpt.app/) are landing pages; [localgpt.md](https://localgpt.md/) is the previs page (`md.localgpt.app` redirects there); [localgpt.world](https://localgpt.world/) is the gallery and the one web viewer, which opens worlds from every app in the browser; [localgpt.rs](https://localgpt.rs/) is the devlog about building it all in Rust.

## The Open World Format

Every LocalGPT app reads and writes one format, and the format is not LocalGPT's: the [Open World Format](https://openworldformat.org) is an open standard for `.world` packages — interactive, AI-authorable 3D worlds: the scene, the history that built it, and what happened in it. A package is a folder holding a world document, a typed state document, an append-only session log and content-addressed assets; the state at any revision is a pure fold of the log over the base. It sits above glTF, not beside it: meshes stay glTF, referenced by hash.

The [specification repository](https://github.com/openworldformat/openworldformat) holds the spec (draft 0.3), the normative JSON Schema, the conformance worlds, and five reference parse-and-fold packages — npm, PyPI, crates.io (`openworldformat`, which this workspace consumes), SwiftPM and Kotlin. Other tools over the format include [worldprobe](https://github.com/yilab/worldprobe), a desktop inspector that folds, validates and walks a package's history and branches, and [worldwalk](https://github.com/yilab/worldwalk), a viewer for macOS, iOS, iPadOS, visionOS and Android that opens a package and walks the world it folds to, at any branch tip.

## Claw Ecosystem

Feature parity matrix comparing fourteen implementations of the personal AI assistant architecture across multiple languages (TypeScript, Rust, Python, Go, Zig, C).

[View Claw Feature Parity Matrix →](/docs/claw)

## Explorable World Systems

Comparison of AI systems that generate or interact with 3D explorable worlds, including Genie 3, SIMA 2, Marble, Artcraft, Intangible, and LocalGPT Gen.

[View Explorable World Systems →](/docs/worlds)
