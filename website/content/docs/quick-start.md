---
title: "Quick Start"
---

# Quick Start

A world in the window, an agent in a terminal beside it, in a few minutes. To previs a screenplay with nothing installed, drop it on [localgpt.md](https://localgpt.md) instead.

## 1. Open a world

```bash
# from a checkout (see Installation for the macOS bundle)
cargo run -p localgpt-app -- --md notes.md         # a Markdown document as a world
cargo run -p localgpt-app -- --song track.mp3      # a song as a world that performs it
cargo run -p localgpt-app -- --world harbour.world # a .world package, opened as a live canvas
cargo run -p localgpt-app                          # reopens what was opened last; a starter song the first time
```

A document rebuilds as you type; a song plays its world; a package opens live, with its history in the left panel.

## 2. Open a terminal, run your agent

The **+** on the right opens a terminal: your login shell, started in the folder of what is open. Run whatever agent you have in it:

```bash
claude      # or codex, gemini, opencode, …
```

Nothing is configured in LocalGPT: no model, no key. In a live package's folder the agent finds `AGENTS.md`, which tells it how to change the world, and the world's API address in `.live/endpoint.json`. Ask it to hang a lamp by the crate, and the lamp lands in the window as one entry in the history, attributed to the agent, undoable, with a branch if it built on an earlier entry.

A phone or a tablet on the same network can show the same terminals and the world: the **Phone** button pairs it with a QR code.

## 3. Or give the agent the shim

An MCP client gets the open world's tools from `localgpt world mcp`, registered once:

```bash
cargo install localgpt
claude mcp add world -- localgpt world mcp        # Claude Code; Codex and Gemini CLI take the same command in their MCP config
```

The shim finds the world the terminal is in (or `$LOCALGPT_WORLD_DIR`, which the app's terminals set), and serves `submit`, `undo`, `log`, `tips`, `goto`, `verify`, `screenshot` and `selection` over stdio.

## 4. The world's command line

The same commands, from any shell, whether or not the app has the world open:

```bash
localgpt world log harbour.world --last 5
localgpt world submit harbour.world -m "a lamp by the crate" <<'EOF'
[{"SpawnEntity": {"entity": {"name": "lamp", "transform": {"position": [2, 0.5, 1]}}}}]
EOF
localgpt world undo harbour.world
localgpt world verify harbour.world
```

With the app open, each command is a call to its API and the window follows; with nothing open, the command is the authority itself. See [CLI Commands](/docs/cli-commands/).

## 5. Gen on its own

```bash
cargo install localgpt-gen
localgpt-gen --mcp-relay                  # the window; an MCP client drives it through `mcp-server --connect`
localgpt-gen mcp-server                   # the window as an MCP client's own process (Claude Desktop, editors)
localgpt-gen --live harbour.world --review   # the live canvas, each agent batch accepted or rejected by you
localgpt-gen --host --web                 # a collaborative session; browsers join with the invite link
```

See [LocalGPT Gen](/docs/gen/), the [MCP relay](/docs/gen/cli-mode/) and the [MCP server](/docs/gen/mcp-server/).

## Next Steps

- [The desktop app](/docs/app) — every panel, and the phone
- [Live canvas](/docs/gen/live-canvas) — how a package is edited from outside
- [CLI Commands](/docs/cli-commands) — `localgpt world` in full
- [Previs](/docs/gen/previs) — a screenplay to a board to a canvas
