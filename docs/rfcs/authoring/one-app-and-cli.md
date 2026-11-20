# One desktop app, one command line

**Status:** steps 1–3 implemented. Step 1 (2026-11-12): `crates/md` is renamed to `crates/doc` (`localgpt-doc`, **Document** mode, `--doc`), `localgpt-app` dispatches `gen …` and `mcp-server …` directly through `localgpt_gen::cli::run_from`, and `localgpt gen …` forwards to `localgpt-app gen …` (with `localgpt-gen` as fallback). Steps 2 and 3 (2026-11-18): `localgpt` runs `gen mcp-server --connect` and `gen --export-session` itself, and the desktop app's bar hosts, joins, replays and exports sessions and turns Gen's MCP relay on and off — with the decisions under each step below. Step 4 waits on the app shipping beyond macOS.

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
2. **Done — `mcp-server --connect` in `localgpt`.** The stdio ↔ `--mcp-relay` bridge needs only tokio and TCP, so its client half — the port file and the bridge — moved to `localgpt-world-agent` (`mcp::relay`), which Gen and the CLI share. `localgpt gen` runs `mcp-server --connect [PORT]` and `--export-session <name> [--out]` itself (`session::export`, also Bevy-free) when the arguments are exactly those; anything else, help included, goes to Gen as typed. An MCP entry of `npx -y localgpt gen mcp-server --connect` works with nothing else installed. On the way: a bridge whose stdin closes half-closes the socket and still hands on the answers to what was asked, where it used to drop them; and the relay port file is removed only by the window that wrote it.
3. **Done — host, join, relay, replay and MCP toggle in `localgpt-app`.** The bar gains **Share** and **MCP** (`crates/app/src/shell/share.rs`), driving `crates/app/src/share.rs` over the protocol the phone also speaks (window-only asks), with `egui_kittest` coverage. Decisions:
   - **Share is Gen's room, installed dormant in every window but a guest's** (`NetHostPlugin`, started and now stopped through `HostControl`): browsers on this network (`--web`), a relay (`--relay`), the PIN and links shown and copied. Stopping ends the listener, the relay client and every guest's socket; the session's package stays.
   - **Guests watch; none edits.** `--web-edit` is not offered: on a live canvas a guest's edit would change the scene outside its one authority (`LiveWorld`). Editing guests are [rooms-and-the-channel.md](rooms-and-the-channel.md)'s, which commit through it.
   - **The MCP relay serves the app's own scene only** — a document's, a song's, a world file's. A live canvas changes only through its API (`localgpt world mcp`), and a replay and a guest's window are driven by a log and a room, so the toggle says why instead.
   - **Replay and join are launch shapes, like the canvas** (`localgpt-app --replay <session> [--replay-speed]`, `--join [host[:port]] [--pin]`), which the Share panel opens in a window of its own: no runtime switch of what drives the scene. Join asks the host first, in the panel, so a gone session or a missing PIN is said there; the PIN reaches the new window in its environment, never its arguments.
   - **A session started under a used name gets a folder of its own** (`session::new_session_dir`), since a second room under one name wrote its revisions after the first's and neither folded; resuming a session under its own name continues it.
4. **Retire the standalone `localgpt-gen` binary.** Once `localgpt-app` ships across macOS, Linux and Windows, keep `localgpt-gen` as a thin deprecated forwarder for one release cycle, then remove it.
