---
title: "Headless"
---

# Headless

LocalGPT Gen builds worlds **without opening a window**: the same tools, served over MCP to an agent that runs without a display — CI, a server, a batch of variations overnight.

```bash
localgpt-gen mcp-server --headless
```

The process is the MCP client's own: the client spawns it, calls the gen tools over stdio, and the process exits when the client goes away. Everything a windowed Gen can do is in the tool set — spawn, modify, lights, environment, terrain, the WorldGen pipeline, `gen_save_world` — and screenshots render offscreen. See [MCP Server](/docs/gen/mcp-server/) for the client configuration.

## Why headless

Interactive work means sitting and watching one scene. Variation and comparison — five lighting passes over a village, three densities of a forest — are scripting jobs, and an agent that can run a loop runs them: it queues its own prompts, calls the tools, saves each result as a world, and you open the results in the morning.

The agent is yours — Claude Code, Codex, a script with an MCP client — and so is its memory of your preferences. Gen keeps none: what it keeps is the worlds, under `~/.local/share/localgpt/gen-workspace/skills/<name>/`, each loadable with `gen_load_world` and openable in the window.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | The client disconnected; the process ended with it |
| 1 | Gen could not start (a GPU could not be opened, a world failed to load) |

## GPU

Headless rendering wants any wgpu backend, software included (`Backends::all()`); on a machine with little GPU memory, close the Gen window while a headless process runs.

## The world's command line

For worlds already on disk, nothing of Gen is needed: `localgpt world submit`, `undo`, `log` and `verify` commit to a `.world` package from any shell with no display at all. See [CLI Commands](/docs/cli-commands/).
