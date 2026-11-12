# Composable Agent Guide: Using LocalGPT Gen as an MCP Component

How external agent runtimes discover and call LocalGPT Gen's MCP tools.

| Field | Value |
|-------|-------|
| Status | Living Document |
| MCP Protocol | 2024-11-05 |
| Transports | stdio, Streamable HTTP, the relay |

---

## Overview

LocalGPT Gen exposes 88 tools for 3D world generation, audio, physics, terrain, and more through the Model Context Protocol (MCP). Any MCP-capable agent runtime can connect to it and drive the Bevy scene as a composable component -- no forking, no SDK integration, no shared process space.

The mental model: **your agent is the brain; LocalGPT Gen is the hands.** Your agent decides what to build. LocalGPT Gen spawns primitives, places terrain, configures lighting, saves worlds, and captures screenshots. The MCP wire protocol is the only coupling. Gen runs no model, calls no service and holds no key.

This guide covers what the other documents leave out: how each transport behaves on the wire, what it does and does not protect, and the limits a runtime has to plan for. The rest is elsewhere:

- [AGENTS.md](../../AGENTS.md) -- the two MCP servers (`localgpt-gen mcp-server` and `localgpt world mcp`) and the tool catalog with parameters
- [MCP server user guide](../../website/content/docs/gen/mcp-server.md) -- client configuration
- [MCP relay](../../website/content/docs/gen/cli-mode.md) -- driving a window that is already open (`--mcp-relay`, `mcp-server --connect`)

---

## Quick Start

```bash
cd localgpt
cargo build --release -p localgpt-gen
```

The binary lands at `target/release/localgpt-gen`. Verify it with a raw JSON-RPC call.

Stdio (pipe a single request):

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | localgpt-gen mcp-server --headless 2>/dev/null
```

HTTP, against a `localgpt-gen mcp-server --mcp-http 8080` left running:

```bash
curl -s -X POST http://127.0.0.1:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | jq .
```

Expected response:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": { "tools": {} },
    "serverInfo": { "name": "localgpt-gen", "version": "<the workspace version>" }
  }
}
```

---

## Stdio Transport

The stdio transport is the standard MCP integration path. The MCP client (your agent runtime or editor) spawns `localgpt-gen mcp-server` as a child process and exchanges newline-delimited JSON-RPC 2.0 messages over stdin/stdout. Stderr carries log output and is ignored by the protocol. A line that is not JSON is skipped. When stdin closes the process exits, and its window with it.

### CLI flags

| Flag | Description |
|------|-------------|
| `--headless` | No Bevy window. Useful for CI, batch generation, or headless servers. |
| `--connect [PORT]` | Be the stdio end of a window started with `localgpt-gen --mcp-relay`, instead of starting Bevy. Port auto-discovered if omitted. The other two flags do nothing with it. |
| `--mcp-http PORT` | Also start an HTTP MCP server on this port (in addition to stdio). |

---

## HTTP Transport

`--mcp-http PORT` serves the same tools at `http://127.0.0.1:PORT/mcp`. It is for a runtime that cannot spawn a child process, and for more than one client on one scene (with the limit under [Tool calls are sequential](#tool-calls-are-sequential)).

The stdio server runs beside it and still ends the process when stdin closes, so a server used only over HTTP needs its stdin held open. Add `--headless` if you do not need the window.

### Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/mcp` | Send a JSON-RPC request, receive a JSON-RPC response. A notification gets `202` and no body |
| GET | `/mcp` | An SSE stream: one `session` event, then a keepalive comment every 30 seconds. The server pushes nothing else |
| DELETE | `/mcp` | End the session the `Mcp-Session-Id` header names |

### Session management

Every POST and GET response carries an `Mcp-Session-Id` header, and a request without a known id is given a new one. A session holds no state: every client drives the same scene, and the header changes nothing a tool does.

### curl examples

**List all available tools:**

```bash
curl -s -X POST http://127.0.0.1:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | jq '.result.tools[].name'
```

**Spawn a cube:**

```bash
curl -s -X POST http://127.0.0.1:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/call",
    "params": {
      "name": "gen_spawn_primitive",
      "arguments": {
        "name": "red_cube",
        "shape": "Cuboid",
        "position": [0, 1, 0],
        "color": [1.0, 0.0, 0.0, 1.0]
      }
    }
  }' | jq .
```

---

## The Relay

`localgpt-gen --mcp-relay` opens a localhost TCP port beside the window (9878, or a free one, written to the file `--connect` discovers it from) and serves the same tools as newline-delimited JSON-RPC. `mcp-server --connect` copies lines between its stdio and that port. The relay has its own dispatch (`crates/gen/src/gen3d/mcp_relay.rs`), which differs from the stdio and HTTP servers:

- `serverInfo.name` is `localgpt-gen-relay`.
- `tools/list` leaves `annotations` out.
- An unknown tool is an `isError` result, and a line that is not JSON is answered with a `-32700` error.
- A connection with no request for five minutes is closed, and the `--connect` process exits with it. A `ping` counts as a request.

---

## Tool Discovery

After connecting, call `tools/list` to get the full catalog:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}
```

The response holds the 88 tool definitions, each with `name`, `description`, `inputSchema` (JSON Schema for the arguments) and, over stdio and HTTP, `annotations` (`readOnlyHint`, `destructiveHint`, `idempotentHint`). The catalog with parameter details is [AGENTS.md](../../AGENTS.md); the sequences an agent runs are in the user docs ([WorldGen](../../website/content/docs/gen/worldgen.md), [World Skills](../../website/content/docs/gen/world-skills.md), [Export](../../website/content/docs/gen/export.md)).

A `tools/call` result is one text block. A tool that fails answers with `isError: true` and its message as text, not with a JSON-RPC error; over stdio and HTTP only a missing `name` or an unknown tool is one (`-32602`). `gen_screenshot` answers with the path of the PNG it wrote, so a runtime that wants to look at the image has to read that file.

---

## Security

### Tool safety split

Only the `gen_*` tools are served: no shell, no general file tools, no memory or web tools, and nothing that calls a model or a service. The rationale: external agent runtimes (Claude Code, Codex, goose) already provide their own file, shell, memory and web tools with their own sandboxing, and their own model. LocalGPT should not duplicate that surface area or weaken the external runtime's security model.

The tools do touch the disk. By default they write under Gen's workspace, in the open world's folder, or (a screenshot with no world open) in the system temp directory. `gen_save_world`, `gen_export_gltf` and `gen_export_screenshot` write wherever their `path` argument says, and `gen_load_world` and `gen_load_gltf` read the path they are given. No tool makes a network call.

### Stdio transport security

- No authentication. The MCP server inherits the OS user's permissions.
- The client (your agent runtime) controls process lifetime -- closing stdin terminates the server.

### HTTP transport security

- Binds to `127.0.0.1` only (localhost). Not exposed to the network by default.
- Session tracking via `Mcp-Session-Id` header (UUID, server-generated). It is not a credential.
- No authentication token on the HTTP transport. If you need network exposure, put it behind a reverse proxy with auth.
- Only this machine is served. A request whose `Origin` or `Host` names another host gets 403, which stops a web page open in a browser here, and a site that rebinds its name to 127.0.0.1, from calling the tools. CORS admits pages served from `localhost` or a loopback address, so a browser-based MCP client running locally still works.

### Relay security

The relay binds `127.0.0.1` only and has no authentication: any local process can connect to its port.

---

## Limitations

### State is in-process

The Bevy scene, audio engine, and behavior system all live in the `localgpt-gen` process. There is no external state store. If the process crashes, the scene is lost unless it was saved with `gen_save_world`.

Implication for external agents: call `gen_save_world` periodically during long generation sessions.

### Single Bevy instance per process

Each `localgpt-gen` process runs one Bevy instance with one scene. You cannot run multiple independent scenes in one process.

If you need multiple scenes, run multiple `localgpt-gen mcp-server` processes (each on a different HTTP port or stdio pipe) and coordinate from the orchestrating agent.

### Tool calls are sequential

A stdio or relay connection's calls run one at a time, in the order they arrive. Batch tools (`gen_spawn_batch`, `gen_modify_batch`, `gen_delete_batch`) exist specifically to reduce round-trip overhead when operating on many entities.

Across connections and concurrent HTTP requests, the calls still reach Bevy one at a time: `GenBridge::send` (`crates/gen/src/gen3d/mod.rs`) holds the one answer channel from sending a command until its answer comes, so a call waits behind a slow one (a screenshot, a glTF load) rather than receiving its answer. Nothing orders the calls beyond that: an orchestrator that puts several agents on one scene decides whose edit lands first.

### No streaming tool output

Tool results are returned as complete JSON-RPC responses. There is no streaming partial output for long-running tools. The `gen_screenshot` tool, for example, blocks until the frame is rendered and returns the result in one response.

### Bevy startup time

Each `localgpt-gen mcp-server` launch pays for Bevy initialization (GPU, window, asset loading). To avoid it, attach to a window that is already running: `localgpt-gen --mcp-relay` in one terminal, `localgpt-gen mcp-server --connect` as the client's command.

---

## Further Reading

- [AGENTS.md](../../AGENTS.md) -- Full tool catalog with parameters and descriptions
- [CLAUDE.md](../../CLAUDE.md) -- Build commands, workspace architecture, feature flags
- [gen/gen-audio.md](../gen/gen-audio.md) -- Audio system architecture
- [MCP server user guide](../../website/content/docs/gen/mcp-server.md) -- Client configuration and transports
