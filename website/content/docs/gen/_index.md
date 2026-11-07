---
title: "LocalGPT Gen"
---

# LocalGPT Gen

Gen is the world builder behind the [desktop app](/docs/app/), also available as a standalone binary. It provides 88 tools for geometry, materials, lighting, behaviors, audio, cameras, physics and worldgen. Your coding agent calls those tools over MCP or edits a live package through its local API; Gen runs no language model and holds no key.

```bash
cargo install localgpt-gen
```

## Drive a window from your agent

Start a window with its relay, then configure your agent to connect to it:

```bash
localgpt-gen --mcp-relay
localgpt-gen mcp-server --connect   # the agent's MCP command, not a second window
```

Or let an MCP client spawn Gen itself:

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

See [MCP server](/docs/gen/mcp-server/) for client configuration and [MCP relay](/docs/gen/cli-mode/) for connecting to a window you already have open.

## Open an editable package

```bash
localgpt-gen --live scene.world
localgpt-gen --live scene.world --review   # approve each agent batch
```

Every change commits whole, with an author and a place in the world's history. You can undo, replay, fork from an earlier entry, or edit directly in the window. [Live canvas](/docs/gen/live-canvas/) covers the workflow and API.

## Other ways to use Gen

```bash
localgpt-gen --world scene.world       # open a package, saved world, manifest or URL
localgpt-gen --scene model.glb         # start from glTF
localgpt-gen --host --web              # host a room with browser guests
localgpt-gen --join HOST --pin PIN     # join from another Gen window
localgpt-gen mcp-server --headless     # the same tools without a window
localgpt-gen --replay session          # replay a session or package history
```

## Reference

- [Tools](/docs/gen/tools/) and the [WorldGen pipeline](/docs/gen/worldgen/)
- [Behaviors](/docs/gen/behaviors/) and [procedural audio](/docs/gen/audio/)
- [World skills](/docs/gen/world-skills/) and [export](/docs/gen/export/)
- [Collaborative sessions](/docs/gen/multiplayer/)
- [Headless mode](/docs/gen/headless/)
- [Previs](/docs/gen/previs/)

For the native window with document and song modes, terminals, history and the timeline, use the [LocalGPT desktop app](/docs/app/).
