---
sidebar_position: 14.8
---

# Collaborative Sessions

Build a world together. One machine **hosts** a session — it runs the AI and owns the world — and friends on the same network **join** from their own machines. Everyone walks around the same world in their own window, and anyone can ask the host's AI to build something where they're looking.

## Quick Start

On the host machine:

```bash
localgpt-gen --host
```

The host console prints a 6-digit **session PIN**:

```
  Session PIN: 482 913   (joiners: localgpt-gen --join <this-host> --pin <PIN>)
```

On each joining machine:

```bash
localgpt-gen --join                            # find sessions on the LAN, you'll be asked for the PIN
localgpt-gen --join 192.168.1.5 --pin 482913   # or connect directly
```

Type a prompt in the joiner's terminal — "put a lighthouse on that hill" — and a translucent placeholder appears where you're looking. It turns amber while the AI works and disappears when the real build streams in for everyone.

## Options

**Host (`--host`)**

| Flag | Default | Description |
|------|---------|-------------|
| `--session-name <name>` | `<user>'s world` | Name shown to joiners browsing the LAN |
| `--port <port>` | `9879` | Session port (UDP for the world, TCP for pairing and assets) |
| `--remote-tools safe\|full` | `safe` | What joiners' prompts may do — see [Security](#security) |
| `--open` | off | Skip the PIN (anyone on the network can join). Trusted networks only |

**Join (`--join [address]`)**

| Flag | Default | Description |
|------|---------|-------------|
| `--pin <PIN>` | prompt | The host's session PIN |
| `--view-radius <n>` | `2` | How many 64-unit chunks around you stream in full detail (max 8) |
| `--no-bake` | off | Disable mesh merging (see [Performance](#performance)) |

With no address, `--join` browses the LAN over mDNS and joins the first session it finds.

### Joiner controls

- **WASD / arrows** — move · **Q/E** — down/up · **right-drag** — look · **scroll** — speed
- `/stats` — what your client is receiving (entities, placeholders, merged meshes, streamed assets)
- `/goto x y z` — jump the camera somewhere

Everything else you type goes to the host's AI.

## How Prompts Work

Joiners' prompts go into a queue on the host and run one at a time, in order. You'll see your place in line:

```
[queue] job #3 queued — 1 ahead of it
[queue] job #3 is being built…
[queue] job #3 done
```

The AI is told where you were looking when you sent the prompt, so "build a tower here" lands in front of you. Each joiner can have up to 4 prompts waiting.

## Performance

Sessions are built to stay smooth as worlds grow:

- **Nearby detail only** — the host streams full detail only for the area around each joiner. Distant areas show as simple colored blocks, so the horizon isn't empty.
- **Mesh merging** — once part of the world stops changing for a few seconds, your client merges its static pieces into a handful of meshes, cutting draw calls. Anything that changes is un-merged instantly.
- **On-demand assets** — custom geometry is downloaded once, verified, and cached on disk, so rejoining (or reusing the same model) doesn't download it again.

## Security

Joiners steer an AI running on **the host's machine**, so sessions are locked down by default.

### Joining requires the PIN

- Each session has its own secret key that never leaves the host. A joiner gets in only by proving it knows the PIN.
- The PIN exchange is designed so that someone watching network traffic learns nothing, and a fake host that doesn't know the PIN can't impersonate yours.
- After 5 wrong PINs the host picks a new one and prints it.
- `--open` turns all of this off — only use it on networks where you trust everyone.

### Joiners can only edit the scene

With the default `--remote-tools safe`, joiners' prompts run on a separate assistant that can **only edit the world**:

- No shell commands, no reading or writing files, no web access
- No access to the host's [memory](/docs/memory-system) or conversation
- No saving or exporting to the host's disk — the host saves worlds
- Scene edits are regular edits the host can undo

If the host uses Claude CLI as its backend, its built-in shell and file tools are switched off for joiners' prompts. Gemini CLI and Codex CLI can't have their built-in tools switched off, so with those backends joiners' prompts are refused unless the host chooses `--remote-tools full`.

:::danger
`--remote-tools full` runs joiners' prompts with **all of the host's tools, including shell access on the host machine**. Only use it with people you'd hand your terminal to.
:::

## Current Limitations

- Joiners see and prompt, but don't edit directly and don't appear as avatars yet.
- Terrain, water, foliage, sky, in-world UI, NPCs, and loaded glTF models aren't shared with joiners yet (glTF models show as placeholders).
- Host and joiners must run the same `localgpt-gen` version.
- Any paired joiner can prompt changes anywhere in the world — there are no per-user permissions yet.
