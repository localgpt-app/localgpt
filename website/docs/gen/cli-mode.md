---
sidebar_position: 14.6
---

# CLI Mode (MCP Relay)

When LocalGPT Gen runs interactively with a **CLI backend** — Claude CLI, Gemini CLI, or Codex — it uses a relay architecture to ensure tool calls go to the **same Bevy window** you're looking at.

## The Problem

Without the relay, each CLI backend spawns its own `localgpt-gen mcp-server` process. This creates a **second Bevy window** that opens, renders the world, and immediately closes — while the original window stays empty.

```
  ┌──────────────┐     spawns      ┌──────────────────┐
  │  Claude CLI  │────────────────►│ localgpt-gen      │ ← second window!
  │  (backend)   │   MCP stdio     │ mcp-server        │
  └──────────────┘                 │ (NEW Bevy + scene) │
                                   └──────────────────┘

  ┌──────────────────────────────────┐
  │  localgpt-gen (interactive)       │ ← your window (empty)
  │  Bevy window you're looking at    │
  └──────────────────────────────────┘
```

## The Fix: MCP Relay

LocalGPT Gen starts a **TCP relay server** alongside the Bevy window. When a CLI backend spawns `localgpt-gen mcp-server --connect`, it connects to the relay instead of creating its own Bevy instance. All tool calls route to your existing window.

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

When you run `localgpt-gen` interactively:

1. **Bevy window opens** on the main thread
2. **MCP relay starts** on port 9878 (background thread)
3. **Agent loop starts** — you see:
   ```
   MCP relay active on port 9878 (CLI backends will use this window)
   CLI backend detected (claude-cli/opus). Gen tools will route to this window via MCP relay.
   ```
4. When the agent sends a prompt to the CLI backend:
   - The CLI backend reads its MCP config
   - It spawns `localgpt-gen mcp-server --connect`
   - The `--connect` process connects to the relay on port 9878
   - Tool calls flow through to the **existing** Bevy window
5. You see the world being built in real-time in your window

## Configuration

### Automatic (recommended)

If you run `localgpt-gen` interactively with a CLI backend model, it **automatically** configures the relay. No manual setup needed — just:

```bash
# Set your model to a CLI backend
localgpt-gen   # uses default model from config.toml
```

If `config.toml` has `default_model = "claude-cli/opus"` (or any `gemini-cli/*` / `codex-cli/*` model), the relay activates automatically.

### Claude CLI

Add to project-level `.mcp.json`:

```json
{
  "mcpServers": {
    "localgpt-gen": {
      "command": "localgpt-gen",
      "args": ["mcp-server", "--connect"]
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
      "command": "localgpt-gen",
      "args": ["mcp-server", "--connect"]
    }
  }
}
```

### Codex CLI

Add to `~/.codex/config.json`:

```json
{
  "mcpServers": {
    "localgpt-gen": {
      "command": "localgpt-gen",
      "args": ["mcp-server", "--connect"]
    }
  }
}
```

These config files live in your LocalGPT workspace (default: `~/.local/share/localgpt/workspace/`). They're created automatically on first run.

:::tip Migrating from standalone MCP
If you previously configured `localgpt-gen mcp-server` **without** `--connect`, update your configs to add `"--connect"` to the args array. This prevents the duplicate window issue.
:::

### Standalone MCP mode

If you want to run gen as a **standalone MCP server** (no interactive mode, the CLI is the sole orchestrator), use `mcp-server` without `--connect`:

```bash
localgpt-gen mcp-server
```

This is for desktop apps (Claude Desktop, Codex Desktop) and editors (VS Code, Zed, Cursor) where you don't need the interactive terminal. See [MCP Server](/docs/gen/mcp-server) for details.

## CLI Flags

```
localgpt-gen mcp-server --connect [PORT]
```

| Flag | Description |
|------|-------------|
| `--connect` | Connect to an existing gen process's MCP relay |
| `--connect 9878` | Connect to a specific port (default: auto-discover) |
| `--headless` | Run without a window (for CI/batch generation) |

Port auto-discovery reads from `$XDG_RUNTIME_DIR/localgpt/gen-mcp-relay.port`, which the interactive gen process writes on startup.

## Supported Backends

| Backend | Model prefix | MCP config location |
|---------|-------------|-------------------|
| Claude CLI | `claude-cli/*` | `.mcp.json` in workspace |
| Gemini CLI | `gemini-cli/*` | `.gemini/settings.json` in workspace |
| Codex CLI | `codex-cli/*` | `.mcp.json` in workspace |

## Troubleshooting

### "No relay port specified and no running gen process found"

The `--connect` process can't find the relay. Make sure `localgpt-gen` is running interactively first:

```bash
# Terminal 1: start gen interactively
localgpt-gen

# Terminal 2: the MCP connect process finds it automatically
localgpt-gen mcp-server --connect
```

### Still seeing two windows

Check that your MCP configs use `--connect`:

```bash
# Check workspace configs
cat ~/.local/share/localgpt/workspace/.mcp.json
cat ~/.local/share/localgpt/workspace/.gemini/settings.json
```

Both should have `["mcp-server", "--connect"]` in the args. If they show `["mcp-server"]` without `--connect`, update them.

### Relay port conflict

If port 9878 is in use, the relay picks a random available port. Check the startup log for the actual port:

```
MCP relay active on port 51234 (CLI backends will use this window)
```

The `--connect` flag auto-discovers the port, so this usually isn't a problem.
