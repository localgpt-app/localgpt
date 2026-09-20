# Composable Agent Guide: Using LocalGPT Gen as an MCP Component

How external agent runtimes discover and call LocalGPT Gen's MCP tools.

| Field | Value |
|-------|-------|
| Date | 2026-03-28 |
| Status | Living Document |
| MCP Protocol | 2024-11-05 |
| Transports | stdio, Streamable HTTP |

---

## Overview

LocalGPT Gen exposes 70+ tools for 3D world generation, audio, physics, terrain, and more through the Model Context Protocol (MCP). Any MCP-capable agent runtime can connect to it and drive the Bevy scene as a composable component -- no forking, no SDK integration, no shared process space.

The mental model: **your agent is the brain; LocalGPT Gen is the hands.** Your agent decides what to build. LocalGPT Gen spawns primitives, places terrain, configures lighting, saves worlds, and captures screenshots. The MCP wire protocol is the only coupling.

Two server binaries exist:

| Binary | Command | Tools | Use Case |
|--------|---------|-------|----------|
| `localgpt` | `localgpt mcp-server` | Memory + web (8 tools) | Note-taking, web research, document loading |
| `localgpt-gen` | `localgpt-gen mcp-server` | Memory + web + gen (70+ tools) | 3D world generation, audio, physics, terrain |

This guide focuses on `localgpt-gen mcp-server` since that is where the composable 3D generation lives.

---

## Quick Start

### 1. Build the binary

```bash
cd localgpt
cargo build --release -p localgpt-gen
```

The binary lands at `target/release/localgpt-gen`.

### 2. Choose a transport

**Stdio** (default) -- the MCP client launches the process and communicates over stdin/stdout:

```bash
localgpt-gen mcp-server
```

**Streamable HTTP** -- the MCP server listens on a port; any number of clients can POST to it:

```bash
localgpt-gen mcp-server --mcp-http 8080
```

This starts both the stdio server and an HTTP server at `http://127.0.0.1:8080/mcp`.

### 3. Verify with a raw JSON-RPC call

Stdio (pipe a single request):

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | localgpt-gen mcp-server --headless 2>/dev/null
```

HTTP:

```bash
curl -s -X POST http://127.0.0.1:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | jq .
```

Expected response (abbreviated):

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocolVersion": "2024-11-05",
    "capabilities": { "tools": {} },
    "serverInfo": { "name": "localgpt-gen", "version": "0.1.0" }
  }
}
```

---

## Stdio Transport

The stdio transport is the standard MCP integration path. The MCP client (your agent runtime or editor) spawns `localgpt-gen mcp-server` as a child process and exchanges newline-delimited JSON-RPC 2.0 messages over stdin/stdout. Stderr carries log output and is ignored by the protocol.

### CLI flags

| Flag | Description |
|------|-------------|
| `--headless` | No Bevy window. Useful for CI, batch generation, or headless servers. |
| `--connect [PORT]` | Connect to an already-running gen process via its TCP relay instead of starting Bevy. Port auto-discovered if omitted. |
| `--mcp-http PORT` | Also start an HTTP MCP server on this port (in addition to stdio). |

### Client configuration

Most MCP-capable tools read a JSON config that maps server names to launch commands.

**Claude Desktop / Claude Code:**

```json
{
  "mcpServers": {
    "localgpt-gen": {
      "command": "localgpt-gen",
      "args": ["mcp-server"]
    }
  }
}
```

**VS Code (Copilot MCP):**

In `.vscode/settings.json` or the user settings:

```json
{
  "mcp": {
    "servers": {
      "localgpt-gen": {
        "command": "localgpt-gen",
        "args": ["mcp-server"]
      }
    }
  }
}
```

**Cursor:**

In `.cursor/mcp.json` at the project root:

```json
{
  "mcpServers": {
    "localgpt-gen": {
      "command": "localgpt-gen",
      "args": ["mcp-server"]
    }
  }
}
```

**Zed:**

In Zed settings (`settings.json`):

```json
{
  "context_servers": {
    "localgpt-gen": {
      "command": {
        "path": "localgpt-gen",
        "args": ["mcp-server"]
      }
    }
  }
}
```

If `localgpt-gen` is not on your PATH, use the full path to the binary (e.g., `/path/to/localgpt/target/release/localgpt-gen`).

---

## HTTP Transport

The streamable HTTP transport serves MCP over standard HTTP, following the MCP streamable HTTP specification. This is useful when:

- The agent runtime does not support spawning child processes (e.g., browser-based agents)
- Multiple agents need to share a single Bevy session
- You want to keep the Bevy window running independently of client connections

### Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/mcp` | Send a JSON-RPC request, receive a JSON-RPC response |
| GET | `/mcp` | Open an SSE stream for server-initiated notifications |
| DELETE | `/mcp` | Terminate the session |

### Starting the server

```bash
localgpt-gen mcp-server --mcp-http 8080
```

This opens a Bevy window and starts both:
- Stdio MCP on stdin/stdout (for local clients)
- HTTP MCP on `http://127.0.0.1:8080/mcp`

Add `--headless` if you do not need the window:

```bash
localgpt-gen mcp-server --mcp-http 8080 --headless
```

### Session management

The server assigns a session ID via the `Mcp-Session-Id` response header. Clients should include this header in subsequent requests to maintain session continuity.

```bash
# First request -- server returns Mcp-Session-Id in response headers
curl -v -X POST http://127.0.0.1:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' 2>&1 | grep -i mcp-session-id

# Subsequent requests -- include the session ID
curl -X POST http://127.0.0.1:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: <session-id-from-above>" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
```

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
        "color": "#ff0000"
      }
    }
  }' | jq .
```

**Take a screenshot:**

```bash
curl -s -X POST http://127.0.0.1:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 3,
    "method": "tools/call",
    "params": {
      "name": "gen_screenshot",
      "arguments": { "camera_angle": "isometric" }
    }
  }' | jq .
```

---

## Tool Discovery

After connecting, call `tools/list` to get the full catalog:

```json
{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}
```

The response contains an array of tool definitions, each with `name`, `description`, and `inputSchema` (JSON Schema for the arguments). A typical response includes 70+ tools across these categories:

| Category | Prefix | Examples |
|----------|--------|----------|
| Scene query | `gen_scene_info`, `gen_entity_info`, `gen_screenshot` | Inspect entities, capture images |
| Entity manipulation | `gen_spawn_*`, `gen_modify_*`, `gen_delete_*` | Create, update, remove 3D objects |
| Camera/lighting | `gen_set_camera`, `gen_set_light`, `gen_set_environment` | Control the view |
| Audio | `gen_set_ambience`, `gen_audio_emitter` | Procedural sound |
| Behaviors | `gen_add_behavior`, `gen_remove_behavior` | Animation (orbit, bob, spin, etc.) |
| World management | `gen_save_world`, `gen_load_world`, `gen_export_*` | Persistence and export |
| Characters | `gen_spawn_player`, `gen_add_npc` | Player and NPC spawning |
| Interactions | `gen_add_trigger`, `gen_add_door`, `gen_add_collectible` | Gameplay mechanics |
| Terrain | `gen_add_terrain`, `gen_add_water`, `gen_add_foliage` | Landscape generation |
| Physics | `gen_set_physics`, `gen_add_collider`, `gen_add_joint` | Rigid body simulation |
| WorldGen pipeline | `gen_plan_layout`, `gen_apply_blockout`, `gen_populate_region` | High-level world creation |
| AI assets | `gen_generate_asset`, `gen_generate_texture` | AI-generated meshes and textures |
| Memory | `memory_search`, `memory_save` | Persistent knowledge base |
| Web | `web_fetch`, `web_search` | Internet access |

The full tool catalog with parameter details is documented in [AGENTS.md](../../AGENTS.md).

---

## Common Workflows

These are step-by-step tool call sequences that external agents typically execute. Each step is a separate `tools/call` request.

### Workflow 1: Create a world from scratch

```
1. gen_plan_layout       -- describe what you want: "a medieval village with a market square"
                            returns a BlockoutSpec (region definitions with sizes and roles)

2. gen_apply_blockout    -- generates the 3D blockout geometry from the spec
                            creates placeholder volumes for each region

3. gen_populate_region   -- fill each region with detailed content
                            call once per region (market, houses, walls, etc.)

4. gen_set_ambience      -- add environmental audio: wind, birds, distant crowd

5. gen_set_camera        -- position the camera for a good overview

6. gen_screenshot        -- capture the result for evaluation

7. gen_save_world        -- persist as a reusable world skill
                            saves SKILL.md + world.ron to workspace/skills/
```

### Workflow 2: Modify an existing world

```
1. gen_load_world        -- load a previously saved world skill by path

2. gen_scene_info        -- inspect the current scene hierarchy

3. gen_modify_entity     -- adjust position, color, scale of existing objects

4. gen_spawn_primitive   -- add new objects to the scene

5. gen_add_behavior      -- animate entities (orbit, bob, spin)

6. gen_save_world        -- save the updated world
```

### Workflow 3: Screenshot-driven evaluation loop

```
1. gen_screenshot        -- capture from the default camera angle
   (args: { "camera_angle": "isometric", "include_annotations": true })

2. [Agent evaluates the image and decides what to fix]

3. gen_modify_entity     -- fix issues identified by the agent
   or gen_delete_entity
   or gen_spawn_primitive

4. gen_screenshot        -- capture again to verify

5. Repeat steps 2-4 until satisfied
```

This loop is how LocalGPT Gen's own built-in agent operates. External agents can replicate the same pattern: generate, screenshot, evaluate, refine.

### Workflow 4: Batch world generation

```
1. gen_plan_layout       -- generate the layout spec

2. gen_apply_blockout    -- create blockout geometry

3. gen_spawn_batch       -- spawn many primitives in one call
   (args: { "entities": [ ... ] })

4. gen_modify_batch      -- adjust multiple entities at once

5. gen_export_html       -- export as self-contained HTML with Three.js
   or gen_export_gltf    -- export as glTF binary for external viewers
```

---

## Runtime-Specific Notes

### ZeroClaw (Rust)

ZeroClaw supports MCP tool servers via its `ToolServerConfig`. Point it at LocalGPT Gen's stdio transport:

```toml
# zeroclaw.toml
[[tool_servers]]
name = "localgpt-gen"
command = "localgpt-gen"
args = ["mcp-server"]
transport = "stdio"
```

ZeroClaw's trait-driven architecture means it will automatically discover the tools via `tools/list` and make them available to its agent loop. Because both ZeroClaw and LocalGPT Gen are Rust binaries, startup is fast (<10ms for ZeroClaw, ~1-2s for Bevy initialization in LocalGPT Gen).

If ZeroClaw is running on a constrained device (its typical deployment), consider running LocalGPT Gen on a separate host with the HTTP transport and pointing ZeroClaw at the HTTP endpoint instead.

### ADK-Rust (Google Agent Development Kit)

Google's ADK-Rust can integrate MCP servers as tool providers. Register LocalGPT Gen as an MCP tool source in your ADK agent definition:

```rust
use adk::tools::McpToolProvider;

let gen_tools = McpToolProvider::stdio("localgpt-gen", &["mcp-server"]);
let agent = Agent::builder()
    .tools(gen_tools)
    .build();
```

For the HTTP transport:

```rust
let gen_tools = McpToolProvider::http("http://127.0.0.1:8080/mcp");
```

ADK-Rust agents handle the MCP initialization handshake and tool discovery automatically. The gen tools appear alongside any other tools the agent has.

### goose (AAIF / Agent Alliance)

goose uses a YAML configuration for MCP servers. Add LocalGPT Gen to your goose profile:

```yaml
# ~/.config/goose/profiles.yaml
default:
  toolkits:
    - name: localgpt-gen
      type: mcp
      command: localgpt-gen
      args:
        - mcp-server
```

goose will launch the process, run the MCP handshake, and expose the gen tools to the agent. Because goose follows the AAIF AGENTS.md convention, it can also read LocalGPT's `AGENTS.md` file directly for tool documentation.

### OpenAI Agents SDK

The OpenAI Agents SDK supports MCP servers as tool providers. Configure LocalGPT Gen as an MCP server in your agent setup:

```python
from agents import Agent
from agents.mcp import MCPServerStdio

gen_server = MCPServerStdio(
    name="localgpt-gen",
    command="localgpt-gen",
    args=["mcp-server"],
)

agent = Agent(
    name="world-builder",
    instructions="You build 3D worlds using the gen tools.",
    mcp_servers=[gen_server],
)
```

For the HTTP transport:

```python
from agents.mcp import MCPServerHTTP

gen_server = MCPServerHTTP(
    name="localgpt-gen",
    url="http://127.0.0.1:8080/mcp",
)
```

The SDK handles `initialize` and `tools/list` automatically. All 70+ gen tools become available as callable tools in the agent's tool list.

### Other MCP-capable runtimes

Any runtime that implements the MCP client protocol can connect. The general pattern is:

1. Spawn `localgpt-gen mcp-server` as a subprocess (or POST to the HTTP endpoint)
2. Send `initialize` and `notifications/initialized`
3. Call `tools/list` to discover available tools
4. Call `tools/call` with tool name and arguments as needed

The protocol is JSON-RPC 2.0 over newline-delimited stdio or HTTP POST. No custom handshake, no proprietary framing.

---

## Security

### Tool safety split

LocalGPT deliberately excludes dangerous tools from the MCP server:

| Included (safe) | Excluded (dangerous) |
|------------------|----------------------|
| `memory_search`, `memory_get` | `bash` (shell execution) |
| `memory_save`, `memory_log` | `read_file` (arbitrary file read) |
| `web_fetch`, `web_search` | `write_file` (arbitrary file write) |
| `document_load`, `transcribe_audio` | `edit_file` (arbitrary file edit) |
| All `gen_*` tools | |

The rationale: external agent runtimes (Claude Code, Codex, goose) already provide their own file and shell tools with their own sandboxing. LocalGPT should not duplicate that surface area or weaken the external runtime's security model.

### Stdio transport security

- No authentication. The MCP server inherits the OS user's permissions.
- The client (your agent runtime) controls process lifetime -- closing stdin terminates the server.
- `web_fetch` has SSRF protection (URL validation, DNS/IP deny lists).
- `memory_save` and `memory_get` are sandboxed to the configured workspace directory. Path traversal is rejected.

### HTTP transport security

- Binds to `127.0.0.1` only (localhost). Not exposed to the network by default.
- Session tracking via `Mcp-Session-Id` header (UUID, server-generated).
- No authentication token on the HTTP transport. If you need network exposure, put it behind a reverse proxy with auth.
- CORS is permissive (any origin) to support browser-based MCP clients.

---

## Limitations

### State is in-process

The Bevy scene, audio engine, and behavior system all live in the `localgpt-gen` process. There is no external state store. If the process crashes, the scene is lost unless it was saved with `gen_save_world`.

Implication for external agents: call `gen_save_world` periodically during long generation sessions.

### Single Bevy instance per process

Each `localgpt-gen` process runs one Bevy instance with one scene. You cannot run multiple independent scenes in one process.

If you need multiple scenes, run multiple `localgpt-gen mcp-server` processes (each on a different HTTP port or stdio pipe) and coordinate from the orchestrating agent.

### Tool calls are sequential

The MCP server processes tool calls one at a time within a session. Batch tools (`gen_spawn_batch`, `gen_modify_batch`, `gen_delete_batch`) exist specifically to reduce round-trip overhead when operating on many entities.

### No streaming tool output

Tool results are returned as complete JSON-RPC responses. There is no streaming partial output for long-running tools. The `gen_screenshot` tool, for example, blocks until the frame is rendered and returns the result in one response.

### Bevy startup time

The first `localgpt-gen mcp-server` launch takes 1-2 seconds for Bevy initialization (GPU, window, asset loading). Use the `--connect` flag to attach to an already-running gen process if startup latency matters:

```bash
# Terminal 1: start gen with the window
localgpt-gen

# Terminal 2: connect MCP to the running instance
localgpt-gen mcp-server --connect
```

---

## Further Reading

- [AGENTS.md](../../AGENTS.md) -- Full tool catalog with parameters and descriptions
- [CLAUDE.md](../../CLAUDE.md) -- Build commands, workspace architecture, feature flags
- [gen/gen-audio.md](../gen/gen-audio.md) -- Audio system architecture
- [rust-ecosystem-integration-spec.md](rust-ecosystem-integration-spec.md) -- Crate integration strategy
