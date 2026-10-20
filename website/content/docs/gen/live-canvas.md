---
title: "Live canvas"
description: "A .world package opened as a live canvas: agents change it from outside through a local API while you watch, people edit it with handles and panels, and every change is one entry in a history that branches — undo per author, review, replay, git."
---

# Live canvas

A `.world` is a folder, and LocalGPT can be its canvas: the window shows the world and **no agent runs in it**. Agents work from their own terminal — Claude Code, Codex, `localgpt`, a script — and send batches of edit ops to a small HTTP API served on localhost. One authority in the window checks each batch and commits it whole, or refuses it with a reason per op, and the canvas applies what was committed. You edit the same world by hand in the same window, through the same authority. Every change, from anyone, is one attributed entry in the history.

The pattern comes from image editors whose projects are folders that agents edit while the canvas updates. A world adds what those lack: the history is a tree, and every entry is validated, attributed and undoable.

## What a live world is {#folder}

```text
scene-1.world/
├── manifest.json      # the world now — written only by the committer
├── ops.jsonl          # every committed batch, append-only; the log
├── package.json       # the head revision and the hash of manifest.json's bytes
├── snapshots/         # base.json, the oldest state the log folds over
├── assets/            # meshes, textures, sounds, stored by SHA-256
├── AGENTS.md          # how to change this world, for any agent that reads it
└── .live/
    ├── endpoint.json  # the API's URL and token while the canvas is open
    └── refusals.jsonl # direct writes that were put back, and why
```

The format is the [Open World Format](https://openworldformat.org): the state at any revision is a pure fold of the log over the base, and `manifest.json` is that fold at the head. That one rule is why rendering, undo, branches, replay and `verify` are all reads over one history.

## Open one {#open}

```bash
localgpt-gen --live scene-1.world          # the Gen binary
localgpt-app --world scene-1.world         # the desktop app recognizes a package and opens it live
localgpt-gen --live scene-1.world --review # an agent's batch shows before it commits; you accept or reject
```

The window prints the API:

```text
[live] scene-1.world · revision 0 · API http://127.0.0.1:57822 (see scene-1.world/AGENTS.md)
```

Any folder with a `manifest.json` will do — a previs draft from [localgpt.md](https://localgpt.md), a world Gen saved, an example from the format's repository. A package drafted elsewhere gets its `AGENTS.md` and `assets/` when it opens. To start from nothing:

```bash
localgpt world init demo.world --git       # a new package; --git makes every batch a commit
localgpt-gen --live demo.world
```

## Change it from outside {#api}

`AGENTS.md` in the folder tells an agent everything below; point any agent at the folder and it can work. The API's URL and bearer token are in `.live/endpoint.json`:

```bash
W=scene-1.world
URL=$(jq -r .url "$W/.live/endpoint.json"); TOKEN=$(jq -r .token "$W/.live/endpoint.json")

curl -s -X POST "$URL/ops" -H "Authorization: Bearer $TOKEN" -d '{
  "author": "claude", "message": "a lantern by the door",
  "ops": [
    {"SpawnEntity": {"entity": {"name": "lantern",
      "transform": {"position": [2.0, 0.5, 1.0]},
      "shape": {"Cylinder": {"radius": 0.1, "height": 1.0}},
      "material": {"color": [0.2, 0.2, 0.2, 1.0], "emissive": [4.0, 2.8, 1.2, 1.0]}}}},
    {"ModifyEntity": {"id": "bench", "patch": {"transform": {"position": [0.0, 0.0, 3.0]}}}},
    {"DeleteEntity": {"id": "torus"}}
  ]}'
```

- **A batch applies whole or not at all.** If any op is wrong, nothing changes and the reply says which op and why (HTTP 422, `errors`, one per failing op). A refusal is worded so the agent can correct the batch and resend.
- **Names bind.** Use an entity's name wherever an id goes — an op's `id`, `parent`, a behavior's `target`, a camera's `aim_entity` — and it commits as the id. A name nothing has refuses the batch, naming it.
- **The reply is a receipt:** `revision`, the entry's `id` and `parent`, `head`, `branch`, one line per op under `changes`, `spawned` names and ids, `stored` assets, `warnings` (a behavior left without its target, say) and `on_screen`.
- **`expected_revision`** pins a batch to the world the agent read; if anyone moved it since, the batch is refused naming both revisions — read again, then decide again.
- **A `patch` holds only what changes.** `transform`, `material` and `light` merge; `behaviors`, `shape`, `parent`, `audio`, `mesh_asset` and `ext-*` blocks are replaced whole; `null` removes a field.
- **Assets are files.** Write a `.glb`, `.png` or sound into `assets/` under any name and reference it by that path in an op; the authority stores an immutable copy named by its hash and points the world there. New bytes make a new version; the old one stays for the history that used it.
- **Op kinds:** `SpawnEntity`, `ModifyEntity`, `DeleteEntity` (takes children with it), `SetEnvironment`, `SetCamera`, `Batch`.
- **Review.** With `--review` on, the reply is `proposed` (HTTP 202) and the batch waits on the canvas for the person to accept (Enter) or reject (Esc); `GET /proposals` reports its status.

The cheapest way to put an MCP agent on a world is the shim, which reads `.live/endpoint.json` and serves the open canvas's tools over stdio — submit, undo, log, verify, screenshot, selection — with every refusal handed back verbatim:

```bash
localgpt world mcp scene-1.world
claude mcp add scene -- localgpt world mcp scene-1.world    # Claude Code, then ask
```

Or start the agent in the folder and tell it to read `AGENTS.md`; the guide and the endpoint are files it can read.

## Edit by hand {#hand}

The person has the same authority as the agents, and uses it through the window:

- **Handles** on the selection: three arrows move along an axis, three rings turn about one (red X, green Y, blue Z), a cube scales uniformly; Alt-drag an arrow to scale along that axis alone. **Y** toggles grid and angle snapping (Cmd/Ctrl held inverts); **L** switches the handles to the entity's own axes. Arrow keys nudge and turn.
- **Selection:** click; Shift- or Cmd/Ctrl-click adds; a drag from empty space (or a Shift-drag from anywhere) box-selects; Alt + right-drag orbits the camera around the selection.
- **The inspector** (**F1**): the detail panel edits every field of the selected entity — name, transform, shape, material, light, behaviors and their targets, audio, mesh, parent — and the outliner reparents by drag and offers duplicate, delete, unparent, group and focus from its context menu. Cmd+G groups, Cmd+D duplicates, Delete deletes.
- **The bottom bar** keeps camera bookmarks (a `bookmarks` tour in the document), the review toggle, and the status line that names a refused direct write.

Every gesture is one batch through the same authority, so it lands in the history as one entry, is undone with Cmd+Z, and is rolled back when the authority refuses it. The fly camera descends with **Q** (Shift is the selection's).

## History {#history}

The history is a tree, and the canvas can show any point in it without writing anything.

- **Undo is per author, with redo.** Cmd+Z takes back your own newest change, Cmd+Alt+Z anyone's, Cmd+Shift+Z redoes. An undo appends the inverse — nothing is ever removed from the log — and an own undo that a later batch built on is refused naming that entry. Over the API: `POST /undo` (with an `own` flag) and `POST /redo`.
- **Branches.** A batch sent `at` an earlier entry (`POST /ops?at=<id>`, an id from `GET /log` or `GET /tips`) starts a branch there; the head — `manifest.json`, main's tip — stays where it was. The canvas follows a commit that continues what is on screen and otherwise only grows the rail, so an agent working on main never yanks a person off the point they chose.
- **The history rail** lists the log and draws every branch; **H** opens the **history panel**: every entry a node left to right in write order, one lane per branch, the head and the entry on screen marked, authors by colour, the message of the node under the pointer below. Click a node to seek there; drag the scrubber; press play to walk the entries at 1x to 8x, at a fixed pace or at the pace they were written, and watch the world being made. Every move is a seek — `POST /goto` — so nothing is written.
- **Replay.** `localgpt-gen --replay <ops.jsonl | package | .world archive>` rebuilds a world batch by batch in the window as a time-lapse, loading each commit's own assets.
- **Git.** In a package made with `--git`, every batch is a commit authored by whoever sent it; `localgpt world history` walks them, and a `git checkout` under the open app is followed within a moment.
- **Verify.** `GET /verify` or `localgpt world verify` checks that folding the log gives the manifest, and that the assets match their hashes.
- **Guards.** A direct write to `manifest.json` is put back as it was and noted in `.live/refusals.jsonl` (`GET /refusals`) and on the bottom bar: the world changes only through ops.

## The API at a glance {#routes}

| Route | What it does |
|---|---|
| `GET /world` | the world now (`?tip=<id>` for any tip) |
| `POST /ops` | commit a batch (`?at=<id>` branches; `?author=` names the sender) |
| `POST /undo`, `POST /redo` | per-author undo and redo |
| `GET /log`, `GET /tips` | every entry with its id and `current`; every branch end |
| `POST /goto` | show any point in the history — a view move, not an edit |
| `GET /verify`, `GET /refusals` | the fold against the head; direct writes put back |
| `GET /screenshot`, `GET /selection` | a PNG of the canvas; what the person has selected, as drawn |
| `GET /board` | a previs package's printable board (`?tip=<a>&vs=<b>` compares two) |
| `GET /proposals` | batches waiting for review |
| `POST /replay` | play the history back in the window |
| `GET /agents`, `GET /widgets`, `POST /input` | what the pane host sees; the window's named widgets and a synthesized hand, for driving the canvas from a test |

## Headless: the same authority without a window {#headless}

`localgpt world` runs the same commands from a script, CI or the terminal, and refuses to write while an app answers at the folder's endpoint — one authority at a time.

```bash
localgpt world init demo.world --git
localgpt world submit demo.world batch.json --author me --message "lift the cube"
localgpt world submit demo.world batch.json --at <entry-id>     # a branch
localgpt world log demo.world
localgpt world tips demo.world
localgpt world undo demo.world --author me
localgpt world verify demo.world
localgpt world history demo.world
localgpt world mcp demo.world                                  # the shim, for the open canvas
```

See [CLI Commands](/docs/cli-commands/#world).

## Where this is going {#next}

Where the person types once the window has no agent of its own — launch a coding agent beside the canvas, speak the Agent Client Protocol, or host a terminal pane — is the open question; the window already follows an agent pane's status when [herdr](https://github.com/herdrdev/herdr) hosts one. The RFCs are in the repository under `docs/rfcs/authoring/`, and the [previs](/docs/gen/previs/) walk is the live canvas in use.
