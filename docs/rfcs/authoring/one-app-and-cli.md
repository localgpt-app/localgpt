# One desktop app, one command line

**Status:** step 1 implemented (2026-11-12). `crates/md` is renamed to `crates/doc` (`localgpt-doc`, **Document** mode, `--doc`), `localgpt-app` dispatches `gen …` and `mcp-server …` directly through `localgpt_gen::cli::run_from`, and `localgpt gen …` forwards to `localgpt-app gen …` (with `localgpt-gen` as fallback). Steps 2–4 are proposed, not built.

## Problem

The workspace grew four product-facing names on top of the desktop app (`localgpt`, `localgpt-app`, `localgpt-gen`, `localgpt-md`, `localgpt-verse`) plus four domains:

1. **"MD" meant two unrelated things.** `localgpt-md` walked through an ordinary Markdown file live in Bevy, while `https://localgpt.md` turns a screenplay into a previs board using `localgpt-previs` in WebAssembly. The domain cannot move; the mode name had to.
2. **Gen's front end is duplicated.** `localgpt-app` already links the entire `localgpt-gen` crate (renderer, physics, audio, all 88 MCP tools), yet `localgpt-gen` remains a second Bevy binary with modes the desktop app does not yet expose in its UI (collaborative `--host`/`--join`/`--web`/`--relay`, `--replay`, `--export-session`, `--mcp-relay`, and `mcp-server`).

## The shape

One brand with two binaries and four modes matching what goes in:

- **The desktop app** (`LocalGPT.app` / `localgpt-app`) — opens a document (**Document**, `localgpt-doc`), a song (**Verse**, `localgpt-verse`), a screenplay (**Previs**, `localgpt-previs`), or a `.world` package, and owns the Bevy runtime.
- **The command line** (`localgpt`) — stays light and Bevy-free for `npm install -g localgpt`, crates.io, CI and headless servers (`world`, `previs`, `audio`, `completion`), and hands off `localgpt gen …` to the desktop binary when 3D rendering or Gen's engine is requested.
- **Standalone development binaries** (`localgpt-doc`, `localgpt-verse`) stay `publish = false` inside the workspace.

## Trade-offs

- **Keeping `localgpt` light.** Folding Bevy directly into `crates/cli` would bloat every npm/crates install with wgpu, windowing and audio drivers. Forwarding `localgpt gen …` (via `exec` on Unix) into `localgpt-app gen …` gives users one CLI name while keeping `localgpt` small.
- **Linux and Windows availability.** Until `localgpt-app` has Linux/Windows installers or crates.io publication, users on those platforms can still install `localgpt-gen`; `localgpt gen …` falls back to `localgpt-gen` automatically when `localgpt-app` is absent.
- **Existing MCP client configs.** Many configs invoke `"command": "localgpt-gen"`; keeping `localgpt-gen` working alongside `localgpt gen mcp-server` avoids breaking setups before the app has full parity.

## Plan

1. **Done — Document rename and shared Gen entrypoint.**
   - Rename `crates/md` / `localgpt-md` → `crates/doc` / `localgpt-doc`, mode label → **Document**, app flag `--doc` (keeping `--md` alias), and docs path `/docs/document/` (keeping `/docs/md/` aliases). `localgpt.md` (`website-md/`) keeps its domain-based name as the browser entry for writing (currently previs).
   - Move `crates/gen`'s parser and dispatch into `localgpt_gen::cli::run_from`.
   - Accept `localgpt-app gen …` and `localgpt-app mcp-server …` in `crates/app` before initializing app logging or eframe.
   - Add `localgpt gen …` in `crates/cli` to resolve sibling `localgpt-app`, `PATH`, macOS `.app` bundles, or fallback `localgpt-gen` and replace/spawn it cleanly.
2. **Move `mcp-server --connect` into `localgpt`.** The stdio ↔ `--mcp-relay` bridge needs only tokio and TCP/WebSocket (no Bevy), so `localgpt` can run `--connect` natively alongside `localgpt world mcp`.
3. **Host, join, relay, replay and MCP toggle in `localgpt-app`.** Expose Share (`--host`, `--web`, `--relay`), session time-lapse replay, session `.world` export, and an MCP relay toggle in the native egui shell, with `egui_kittest` coverage.
4. **Retire the standalone `localgpt-gen` binary.** Once `localgpt-app` ships across macOS, Linux and Windows, keep `localgpt-gen` as a thin deprecated forwarder for one release cycle, then remove it.
