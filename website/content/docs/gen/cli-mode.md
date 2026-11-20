---
title: "MCP Relay"
---

# MCP Relay

When LocalGPT Gen runs interactively with `--mcp-relay`, an **MCP client outside it** — Claude Code, Gemini CLI, Codex, an editor — drives the **same Bevy window** you're looking at. Gen runs no agent and is nobody's model backend: the agent runs in its own terminal with its own login, and reaches the window through the relay.

The [desktop app](/docs/app/#agents) has the same relay: **MCP** in its top bar turns it on over the world in its window, and off again.

## The Problem

Without the relay, an MCP client configured with `localgpt-gen mcp-server` spawns its own process. This creates a **second Bevy window** that opens, renders the world, and immediately closes — while the original window stays empty.

```
  ┌──────────────┐     spawns      ┌──────────────────┐
  │  MCP client  │────────────────►│ localgpt-gen      │ ← second window!
  │  (an agent)  │   MCP stdio     │ mcp-server        │
  └──────────────┘                 │ (NEW Bevy + scene) │
                                   └──────────────────┘

  ┌──────────────────────────────────┐
  │  localgpt-gen (interactive)       │ ← your window (empty)
  │  Bevy window you're looking at    │
  └──────────────────────────────────┘
```

## The Fix: MCP Relay

LocalGPT Gen starts a **TCP relay server** alongside the Bevy window. When the client spawns `localgpt gen mcp-server --connect`, it connects to the relay instead of creating its own Bevy instance. All tool calls route to your existing window.

The `--connect` process is only a bridge, so `localgpt` runs it itself: it needs neither the desktop app nor `localgpt-gen` on the machine, only a window with the relay on. `localgpt-gen mcp-server --connect` and `localgpt-app mcp-server --connect` do the same.

```
  ┌──────────────────────────────────────────────────────────────────┐
  │  localgpt-gen (interactive)                                       │
  │                                                                   │
  │  ┌────────────┐   GenBridge    ┌────────┐    TCP :9878            │
  │  │ Bevy 3D    │◄─────────────►│ MCP     │◄──────────────┐        │
  │  │ Window     │  (in-process)  │ Relay   │               │        │
  │  │            │                │ Server  │               │        │
  │  └────────────┘                └────────┘               │        │
  │        ▲                                                │        │
  │        │ renders scene                                  │        │
  └────────┼────────────────────────────────────────────────┼────────┘
           │                                                │
           │                          ┌─────────────────────┤
           │                          │  stdio ↔ TCP bridge │
           │                          │                     │
           │                     ┌────┴──────────────┐      │
           │                     │ localgpt-gen       │      │
           │                     │ mcp-server         │◄─────┘
           │                     │ --connect           │  TCP relay
           │                     └────┬──────────────┘
           │                          │ MCP stdio
           │                     ┌────┴──────────────┐
           │                     │  Claude CLI /      │
      you see the                │  Gemini CLI /      │
      world here                 │  Codex             │
                                 └───────────────────┘
```

**Key points:**
- Only **one Bevy window** exists — the one you see
- The relay server listens on **TCP port 9878**
- `--connect` makes the MCP server process a thin stdio↔TCP bridge
- Tool calls travel: CLI → MCP stdio → bridge → TCP → relay → GenBridge → Bevy
- Results travel the reverse path back to the CLI

## How It Works

When you run `localgpt-gen --mcp-relay`:

1. **Bevy window opens** on the main thread
2. **MCP relay starts** on port 9878 — you see:
   ```
   MCP relay active on port 9878 (external MCP clients can connect to this window)
   ```
3. Nothing else runs in the window: no agent, no prompt
4. When you prompt the agent in its own terminal:
   - It reads its MCP config
   - It spawns `localgpt gen mcp-server --connect`
   - The `--connect` process connects to the relay on port 9878
   - Tool calls flow through to the **existing** Bevy window
5. You see the world being built in real-time in your window

Gen has no prompt of its own: the relay, `mcp-server`, or a live package's API is how the window is driven, and several clients may share one window.

## Configuration

### Start the relay

```bash
localgpt-gen --mcp-relay
```

Without the flag no port is opened and no relay message is shown.

### Point the client at it

Each client keeps its own MCP config; LocalGPT writes none of them.

### Claude Code

Add to project-level `.mcp.json`:

```json
{
  "mcpServers": {
    "localgpt-gen": {
      "command": "localgpt",
      "args": ["gen", "mcp-server", "--connect"]
    }
  }
}
```

### Gemini CLI

Add to `~/.gemini/settings.json` or workspace `.gemini/settings.json`:

```json
{
  "mcpServers": {
    "localgpt-gen": {
      "command": "localgpt",
      "args": ["gen", "mcp-server", "--connect"]
    }
  }
}
```

### Codex

Add to `~/.codex/config.toml`:

```toml
[mcp_servers.localgpt-gen]
command = "localgpt"
args = ["gen", "mcp-server", "--connect"]
```

A configuration that names `localgpt-gen` with `["mcp-server", "--connect"]` keeps working.

A config that already names `localgpt-gen mcp-server` **without** `--connect` spawns a window of its own; add `"--connect"` to its args to reach the one you have open.

### Standalone MCP mode

To run Gen as a **standalone MCP server** — the client spawns the whole of Gen, window included — use `mcp-server` without `--connect`:

```bash
localgpt-gen mcp-server
```

This is for desktop apps (Claude Desktop, Codex Desktop) and editors (VS Code, Zed, Cursor) where you don't need the interactive terminal. See [MCP Server](/docs/gen/mcp-server) for details.

## CLI Flags

```
localgpt gen mcp-server --connect [PORT]
```

| Flag | Description |
|------|-------------|
| `--connect` | Connect to an existing gen process's MCP relay |
| `--connect 9878` | Connect to a specific port (default: auto-discover) |
| `--headless` | Run without a window (for CI/batch generation) |
| `--mcp-relay` | Start the MCP relay beside the window (the interactive `localgpt-gen`) |

Port auto-discovery reads `$XDG_RUNTIME_DIR/localgpt/gen-mcp-relay.port` (the temporary folder's `localgpt/` without `XDG_RUNTIME_DIR`), which a window writes when its relay starts — `localgpt gen --mcp-relay`, or the desktop app's **MCP** — and removes when it stops. With two windows' relays on, it names the one that started last; pass the port to reach the other.

## MCP clients

Any MCP client that can spawn a stdio server. The three that come up most:

| Client | MCP config location |
|--------|-------------------|
| Claude Code | `.mcp.json` in the folder it starts in, or its user settings |
| Gemini CLI | `.gemini/settings.json` there, or `~/.gemini/settings.json` |
| Codex | `~/.codex/config.toml` |

## Troubleshooting

### "no MCP relay is running"

The `--connect` process can't find the relay. Make sure a window with the relay is open first — or, in the desktop app, that **MCP** is on:

```bash
# Terminal 1: the window, with the relay
localgpt gen --mcp-relay

# Terminal 2: the MCP connect process finds it automatically
localgpt gen mcp-server --connect
```

"no MCP relay answers on port …" is a port written by a window that has since closed without saying so: start the relay again, or pass the port it prints.

### Still seeing two windows

Check the client's own MCP config (`.mcp.json`, `.gemini/settings.json`, `~/.codex/config.toml`): the entry should have `--connect` in its args. If it shows `["gen", "mcp-server"]` (or `["mcp-server"]`) without `--connect`, the client is spawning a window of its own.

### Relay port conflict

If port 9878 is in use, the relay picks a random available port. Check the startup log for the actual port:

```
MCP relay active on port 51234 (external MCP clients can connect to this window)
```

The `--connect` flag auto-discovers the port, so this usually isn't a problem.
