---
title: "Collaborative Sessions"
---

# Collaborative Sessions

Build a world together. One machine **hosts** a session and owns the world; friends on the same network **join** from their own Gen window, or from a browser with the invite link, and everyone walks the same world live. No model runs for the room: the host builds with its own agent, outside the window, and every change reaches every guest.

## Quick Start

On the host machine:

```bash
localgpt-gen --host                 # prints a 6-digit session PIN
localgpt-gen --host --web           # also a join page and an invite link for browsers
localgpt-gen --host --mcp-relay     # and your agent builds in the hosted world over MCP
```

The host console prints the PIN:

```
  Session PIN: 482 913   (joiners: localgpt-gen --join <this-host> --pin <PIN>)
```

On each joining machine:

```bash
localgpt-gen --join                            # find sessions on the LAN; you'll be asked for the PIN
localgpt-gen --join 192.168.1.5 --pin 482913   # or connect directly
```

A browser guest opens the invite link the host prints (`http://<host>:<port>/#t=<token>`): nothing to install and no internet, since the host serves the page itself.

## Who builds

The room runs no AI. The host's own agent builds — over the window's [MCP relay](/docs/gen/cli-mode/) (`--mcp-relay` beside `--host`) or through a live package's API — and its edits reach everyone as the host's ops. A guest who wants something built says so in chat, and the host asks its agent; a prompt a guest's client sends is refused by the room with the reason.

Browser guests joined with `--web-edit` edit directly: click to select, drag to move, `Q`/`E` rotate, `+`/`-` scale, `Delete` remove, and `/undo` steps back through their own changes. Native joiners watch, walk and chat.

## Options

**Host (`--host`)**

| Flag | Default | Description |
|------|---------|-------------|
| `--session-name <name>` | `<user>'s world` | Name shown to joiners browsing the LAN |
| `--port <port>` | `9879` | The session port: the join page and the WebSocket the room runs over |
| `--open` | off | Skip the PIN (anyone on the network can join). Trusted networks only |
| `--web` | off | Serve the join page and print an invite link for browsers |
| `--web-edit` | off | With `--web`: browser guests edit directly, not just watch and chat |
| `--relay <url>` | — | Reach guests beyond the LAN through a `localgpt-relay` ([below](#beyond-the-lan)) |
| `--resume <session>` | — | With `--web`: replay a session's log before guests join |

**Join (`--join [address]`)**

| Flag | Default | Description |
|------|---------|-------------|
| `--pin <PIN>` | prompt | The host's session PIN |

With no address, `--join` browses the LAN over mDNS and joins the first session it finds.

### In a joiner's window

- **W A S D**, mouse — fly and look, as in any Gen window; every guest is a labelled capsule
- `/stats` — what your client is receiving
- `/quit` — leave

Any other line typed in the joiner's terminal is chat to the room.

## Sessions are packages

Every session is a package under `~/.local/share/localgpt/gen-workspace/sessions/<name>/`: a base world (`world.ron`), one log of every committed batch (`ops.jsonl`), `session.json` and keyframe snapshots. The log is the history, so:

- `/undo` takes back your own newest batch; undoing an undo redoes it.
- `localgpt-gen --host --web --resume <session>` restores a session before guests join.
- `localgpt-gen --replay <session> [--replay-speed 4]` rebuilds it batch by batch in a window, as a time-lapse.
- `localgpt-gen --export-session <name> [--out x.world]` zips the package into the one-file `.world` other software reads; `--resume` and `--replay` take those archives back.

## Beyond the LAN {#beyond-the-lan}

Run `localgpt-relay` anywhere with a public address, and host with it:

```bash
localgpt-relay 9911                                   # on the server; put a TLS terminator in front for wss
localgpt-gen --host --relay https://relay.example.com # prints: Internet guests: https://relay.example.com/r/Q8QJVD/#t=<token>
```

The relay forwards frames by room code and keeps no world state, but it encrypts nothing and serves the join page itself, so whoever runs a relay can read its rooms: run your own, and relay only a world you would show anyone. LAN guests and relay guests share the same room.

## Security

### Joining takes the PIN

- A joiner sends the PIN when it says hello, and the host lets in whoever sends the right one. Every joiner uses the same PIN and picks their own name.
- The PIN, like everything else in a session, crosses the network unencrypted: anyone who can watch the traffic can read it and the world. Host on a network where you trust everyone, or one you'd show the world to.
- The PIN is six digits. After five wrong ones the session stops taking it: the invite link still works, and a new session gets a new PIN.
- Through a relay (`--relay`), guests join with the invite link's token only, never the PIN, and whoever runs the relay can read the session.
- `--open` skips the PIN — only use it on networks where you trust everyone.

### Guests can only edit the scene

Nothing a guest sends runs on the host beyond the world's own ops:

- No AI runs for the room: a prompt a guest sends is refused by the authority with the reason, and the host's agent is the host's alone
- No shell commands, no reading or writing files, no web access
- No saving or exporting to the host's disk — the host saves worlds
- Scene edits (browser guests with `--web-edit`) are regular edits the host can undo, and the authority checks every op from anyone: roles, schema, limits, revision

## Current Limitations

- Native joiners watch and chat but don't edit directly; browser guests edit with `--web-edit`.
- glTF meshes show as placeholders for guests. Terrain, water, foliage, sky, in-world UI and NPC bodies aren't in the world format, so they are neither saved nor synced.
- Guests sync the whole document, which is fine at room scale; a room holds 16 guests.
- Host and joiners must speak the same protocol version; a session announces its own on the LAN, and browsing skips sessions on another.
- A guest's own agent building through the room, rather than through the host's window, is a follow-up the protocol is shaped for.
