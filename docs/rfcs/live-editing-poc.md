# Live editing — proof of concept

**Branch:** `poc/live-editing`. **Status:** a proof of concept, not a
proposal yet. Nothing here changes the Open World Format repository.

The question: can Gen be the *canvas* of a `.world` that agents change from
outside — Claude Code, Codex, `localgpt`, a script — with no agent
conversation inside the app? The pattern comes from an image editor whose
projects are folders that agents edit while the canvas updates. A world
adds what that editor lacks: every change is a validated, attributed,
undoable entry in the log.

## The rules the proof of concept enforces

1. **The world changes only through ops.** An agent sends a batch of edit
   ops to the open app's API. The batch commits whole, or not at all with
   a reason per op.
2. **Assets are files.** Agents write meshes and textures into `assets/`
   under any name. An op that references one stores an immutable copy at
   `assets/<sha256>.<ext>` and points the world there. A new version is new
   bytes referenced again; old versions stay for the history that used them.
3. **The package is head-first.** `manifest.json` is the world now, written
   only by the committer (a direct write is put back). `fold(base, log) ==
   manifest.json` is checkable at any time.
4. **In a git repository, every batch is a commit** authored by whoever sent
   it, and replay walks the commits as keyframes.

## Run it

```sh
# a world folder from any manifest
mkdir demo.world && cp ../openworldformat/examples/hello-world/manifest.json demo.world/
cargo run -p localgpt-world-agent --example live -- init demo.world --git

# the canvas: no agent in the window; the API is in demo.world/.live/endpoint.json
cargo run -p localgpt-gen -- --live demo.world

# from any terminal or agent (demo.world/AGENTS.md says the same)
URL=$(jq -r .url demo.world/.live/endpoint.json); TOKEN=$(jq -r .token demo.world/.live/endpoint.json)
curl -s -X POST "$URL/ops" -H "Authorization: Bearer $TOKEN" -d '{"author": "claude",
  "message": "lift the cube", "ops": [{"ModifyEntity": {"id": "cuboid",
  "patch": {"transform": {"position": [0.0, 2.0, 0.0]}}}}]}'
curl -s "$URL/screenshot" -H "Authorization: Bearer $TOKEN"     # → a PNG path
curl -s -X POST "$URL/replay?seconds=1" -H "Authorization: Bearer $TOKEN"
```

Without the app, the `live` example is the authority: `submit`, `undo`,
`log`, `verify`, `history`. It refuses to write while an app answers at the
folder's endpoint.

## Where the code is

| | |
|---|---|
| `crates/world-agent/src/live.rs` | the authority: ingestion (names → ids, new ids, struct patches merged, strict fields, assets by hash), all-or-nothing commit, undo as an appended inverse, the manifest guard, `verify`, git commits and history |
| `crates/gen/src/gen3d/live.rs` | the canvas: `--live`, the localhost API (token in `.live/endpoint.json`), scene updates through `OpsApplier`, previews, screenshots, selection, git replay |
| `crates/world-agent/examples/live.rs` | the headless authority |

## What it showed

Run against `hello-world` with an agent working only through files and the
API:

- A batch by name — environment, a spawn, a move, a delete — applied
  live and committed as revision 1; the reply listed the new entity's id.
- A batch with a misspelled field (`colour`) and an unknown parent name was
  refused whole with one JSON-pointer reason per op; the valid op in it was
  not applied, and `manifest.json` and the log were byte-identical after.
- A texture written as `brick.png`, referenced, rewritten and referenced
  again became two stored versions; undo restored the first, which only
  works because stored versions never change.
- A direct write to `manifest.json` was put back within half a second.
- `git log` read as the change history (authors `claude`, `yi`, `codex`),
  `git diff` of `manifest.json` read like a review, and replay showed the
  base, every commit, and returned to the head — from `git show
  <commit>:manifest.json` alone, no fold.
- `verify` held after every step: the fold of the log equals the manifest,
  and every asset matches its hash.

## What it found for the format

- **Patches replace whole structs.** `ModifyEntity` with only a `position`
  resets rotation and scale; a material patch with only a texture drops the
  color. Agents send partial changes by nature. The proof of concept merges
  `transform`, `material`, `light` and `SetEnvironment` at ingestion (JSON
  merge patch); the spec should say which, or define merge for struct
  fields.
- **No op reaches meta, avatar, tours, soundtrack or creations.** Through an
  ops-only API an agent cannot change them at all. A `ModifyWorld` op closes
  it.
- **Names at ingestion are right.** Binding names to ids (and allocating ids
  for spawns) at ingestion is what made the ops writable by an agent — the
  spec's rule, validated.
- **Strict mode belongs to the authoring API.** Must-ignore would have kept
  the `colour` typo silently; refusing it with a pointer is what lets an
  agent correct itself.
- **Content addressing and git overlap.** Hash-named copies keep a package
  self-contained without git (a zip of one revision still verifies). In a
  repository, git already versions the bytes, so working names could be
  referenced directly; the two models need one answer.
- **What git takes over, and what it doesn't.** Commits gave history,
  authorship, diff, revert, branches and transport for free, and replay
  became walking keyframes. It does not give op-level intent (the log
  still does), real-time collaboration, or a semantic merge: `manifest.json`
  merges need a driver that merges by entity and field (`ops.jsonl` uses
  `merge=union`).

## Not done

A cold test with a fresh agent that has only `AGENTS.md`; an MCP wrapper
over the same endpoints; a semantic git merge driver; switching git branches
under an open app (the guard would put `manifest.json` back — a checkout
should reopen instead); redo and per-author undo; mesh, audio and camera
changes in the open scene (`OpsApplier` doesn't apply them); telling the
agent, not only the app's log, when its direct write was put back.
