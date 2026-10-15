---
title: "Previs"
description: "From a screenplay to a previs board on localgpt.md, and from the board into the live canvas: look through the shots, revise them by asking, compare treatments as boards."
---

# Previs

LocalGPT's previs is two halves. The first is a page, [localgpt.md](https://localgpt.md/): drop a screenplay and get a board — a set, a stand-in per speaking character and a first pass of coverage per scene, with real lenses and measured camera positions — drafted in the browser tab, nothing installed, nothing uploaded. The second is the app: open a scene's package as a live canvas, look through any shot, and revise the coverage by asking.

## 1. Script → board (the page) {#board}

Drop a `.fdx` (a Final Draft file, as saved), a `.fountain`, or Markdown script pages on [localgpt.md](https://localgpt.md/). The page shows the board and offers three downloads:

| Download | What it is |
|---|---|
| `board.html` | the printable board: one card per shot, its frame rendered by the Open World Format viewer |
| `shotlist.csv` | scene, shot, size, lens, sensor, aspect, camera height, distance to subject, in and out on the clock |
| `<script>-previs.zip` | one `.world` package per scene — `scene-1.world/`, `scene-2.world/` …; a numbered shooting script keeps its numbers (`scene-12A.world/`, shots `12A-A`, `12A-B`) |

The same draft runs on the command line, with no ceiling on scenes:

```bash
# from a checkout of the repository
cargo run -p localgpt-previs -- script.fdx --out previs/
# previs/scene-1.world/  previs/scene-2.world/  previs/shotlist.csv  previs/board.html
```

The draft is deterministic and model-free: the same pages give the same board, byte for byte, on the page and on the command line. A Final Draft file, its Fountain export and the same script as Markdown pages draft to the same board.

## 2. Board → canvas (the app) {#canvas}

Unzip the download (or take the command line's folder) and open one scene's package as a live canvas:

```bash
localgpt-gen --live previs/scene-1.world
```

The window shows the set, the stand-ins and the cameras, and prints the folder's API:

```
[live] previs/scene-1.world · revision 0 · API http://127.0.0.1:57822 (see previs/scene-1.world/AGENTS.md)
```

In the window:

- **Look through a shot** — click its card in the shot strip, or press its number (1–9). The free camera becomes that shot's camera, letterboxed to its frame; Esc hands the free camera back.
- **Reorder** — drag cards in the strip. The reorder is one history entry, undoable like any edit.
- **The board** — the strip's buttons open the folder's `GET /board` in the browser: the tip on screen as a printable board, or two tips side by side with the changed shots marked.

`localgpt-app --world previs/scene-1.world` opens the same package in the one-app window: it recognizes a package and opens it live.

## 3. Revising shots by asking {#asking}

No agent runs in the window. The package folder is the handoff: `AGENTS.md` says how to change the world, and `.live/endpoint.json` holds the API's URL and token while the canvas is open. Any agent that reads the folder can revise the coverage — "make 1B a 35", "stay on her", "from the doorway" — as ordinary `ModifyEntity` patches on a camera entity's `ext-cinematography` block, committed whole or refused with a reason it can act on.

The cheapest way to put an agent on it is the MCP shim, which serves the open canvas's tools over stdio:

```bash
# in another terminal, while the canvas is open
localgpt world mcp previs/scene-1.world
```

Add it to your MCP client (for Claude Code: `claude mcp add previs -- localgpt world mcp previs/scene-1.world`) and ask. Or start the agent in the folder and point it at `AGENTS.md` — the guide and the endpoint are files it can read.

Without an agent, the API is plain HTTP. The camera entity is named after its shot, so a revision reads the block and sends it back with the change (an `ext-*` patch replaces the whole block):

```bash
# URL and token are in previs/scene-1.world/.live/endpoint.json
curl -s -H "Authorization: Bearer $TOKEN" "$URL/world" \
  | jq '.entities[] | select(.name == "1B")["ext-cinematography"]'
curl -s -X POST -H "Authorization: Bearer $TOKEN" "$URL/ops?author=me" \
  -d '{"ops": [{"ModifyEntity": {"id": "1B", "patch": {"ext-cinematography": { …the block, with "focal_length_mm": 35… }}}}]}'
```

Every commit lands in `ops.jsonl` and rewrites `manifest.json`; `GET /log` lists the entries and `localgpt world verify previs/scene-1.world` checks that folding the log gives the manifest.

## 4. Treatments as branches, compared as boards {#treatments}

A revision batch sent `at` an earlier entry (`POST /ops?at=<id>`, an id from `GET /log` or `GET /tips`) starts a branch; main's tip stays where it was. Two tips' boards go side by side, changed shots badged:

```
GET /board?tip=<a>&vs=<b>
```

The history rail in the window draws every branch, and `POST /goto` moves the canvas to any point without writing anything — a view move, not an edit.

## Where this is going {#next}

Posing, animatics and a generated look on top of the plan wait until people who previs today have used these two steps on their own pages — `docs/world-strategy.md` §15 in the repository says why, and `docs/previs-user-test.md` is the sheet those sessions run to.
