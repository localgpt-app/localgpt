---
title: "CLI Commands"
---

# CLI Commands

Two binaries have a command line: `localgpt`, a `.world` package's, and `localgpt-gen`, the world builder's. Neither takes a model, a key or an agent's settings.

## `localgpt`

```bash
localgpt <COMMAND>

Commands:
  world       Work on a .world package: commit batches, read the log, verify, serve its tools over MCP
  completion  Generate shell completion scripts
  help        Print help information

Options:
  -v, --verbose   Enable verbose logging (to stderr; stdout is the transport while `world mcp` serves)
```

## `localgpt world` {#world}

A `.world` package is a folder — `manifest.json` (the world now), `ops.jsonl` (every change), `assets/` — and `localgpt world` is its command line, whoever holds it. While an app has the world open, every command is a call to that app's localhost API: the app commits, and what only a window knows — what's on screen, the selection, a screenshot, a batch waiting for review — comes from it. With nothing open, the commands are the authority themselves, the same one the [live canvas](/docs/gen/live-canvas/) uses, for scripts and CI. One authority at a time, either way, and a commit prints the same receipt whichever made it.

This is how an agent in a terminal changes a world: no token or JSON quoting on its command line, short lines unless `--json`, and one command prefix to allow.

```bash
localgpt world init <dir> [--git]             # open a folder as a package, creating one if needed; --git: every batch a commit
localgpt world submit [<dir>] [<batch> | -]   # commit a batch of edit ops — whole, or not at all; stdin by default
    --author <name> --message <why>           #   attribution ($LOCALGPT_WORLD_AUTHOR; "cli" by default) and the commit message
    --at <entry-id>                           #   build on an earlier entry: a branch; the head stays where it is
    --expected-revision <n>                   #   refused if the world moved since you read it
localgpt world undo [<dir>] [--own] [--at <id>]   # take back the newest edit by appending its inverse
localgpt world redo [<dir>] [--own] [--at <id>]   # take the newest undo back
localgpt world log [<dir>] [--last <n>]       # every committed batch with ids, the head marked
localgpt world tips [<dir>]                   # every branch end, the head marked
localgpt world show [<dir>] [--at <entry>]    # the world in a line per entity; --at current, <id> or base
localgpt world get [<dir>] <entity>…          # entities by name or id, as JSON: what a patch changes
localgpt world verify [<dir>]                 # the fold of the log still equals the head; assets match their hashes
localgpt world screenshot [<dir>]             # the open app's view now, as a PNG path
localgpt world selection [<dir>]              # what the person has selected in the open app
localgpt world goto [<dir>] <entry>           # show an entry in the open app's window; nothing is written
localgpt world proposals [<dir>]              # with review on, the batches waiting for the person
localgpt world history [<dir>] [<rev>]        # the git history, when the world is in git
localgpt world mcp [<dir>]                    # serve the open canvas's tools over stdio (submit, undo, log, tips, goto, verify, screenshot, selection)
```

The folder is the first argument when it's given; else `$LOCALGPT_WORLD_DIR`, which the desktop app sets in its terminals; else the folder you're in, or the nearest one above it that is a world. A batch is JSON: `[op, …]` or `{"ops": [op, …]}`, inline, a file, or stdin, with the op kinds `SpawnEntity`, `ModifyEntity`, `DeleteEntity`, `SetEnvironment`, `SetCamera`, `ModifyWorld` (the world's scene-wide fields: its song, tours, environment) and `Batch`. Names bind wherever the format marks a reference and commit as ids; a refusal names the op and the reason, on stderr with exit status 1. The folder's `AGENTS.md` describes the same rules for an agent.

```bash
localgpt world submit -m "a lamp by the crate" <<'EOF'
[{"SpawnEntity": {"entity": {"name": "lamp", "transform": {"position": [2, 0.5, 1]}}}}]
EOF
claude mcp add world -- localgpt world mcp          # or: Claude Code edits the open world over MCP
```

## `localgpt completion`

```bash
localgpt completion bash > /etc/bash_completion.d/localgpt
localgpt completion zsh > "${fpath[1]}/_localgpt"
localgpt completion fish > ~/.config/fish/completions/localgpt.fish
```

## `localgpt-gen`

The world builder's window, driven from outside. No prompt is typed into Gen itself.

```bash
localgpt-gen                              # the window alone (nothing drives it: pass one of the flags below)
localgpt-gen --mcp-relay                  # serve the MCP relay beside the window; an MCP client reaches it through `mcp-server --connect`
localgpt-gen mcp-server                   # the window as an MCP client's own process, tools over stdio
localgpt-gen mcp-server --mcp-http 8766   # and the streamable HTTP transport at http://127.0.0.1:8766/mcp
localgpt-gen mcp-server --headless        # no window: CI and batch work over MCP
localgpt-gen mcp-server --connect [PORT]  # the stdio end of a running window's relay (port auto-discovered)
localgpt-gen --live scene-1.world [--review]   # a .world package as a live canvas; --review holds each batch for Enter or Esc
localgpt-gen --world <PATH|URL> | --scene <glb> # open a world, or a glTF scene, at start
localgpt-gen --replay <session|dir|ops.jsonl|.world> [--replay-speed 4]   # a time-lapse of a history
localgpt-gen --export-session <name|dir> [--out x.world]                  # zip a session package
localgpt-gen --host [--session-name n] [--port 9879] [--open] [--web] [--web-edit] [--relay URL] [--resume x]
localgpt-gen --join [host[:port]] [--pin PIN]   # join a session as a viewer; lines typed are chat
```

See [MCP Server](/docs/gen/mcp-server/), [MCP Relay](/docs/gen/cli-mode/), [Live canvas](/docs/gen/live-canvas/) and [Collaborative Sessions](/docs/gen/multiplayer/).

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | An error, named on stderr |
