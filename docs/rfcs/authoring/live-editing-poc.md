# Live editing — proof of concept

**Branch:** `poc/live-editing`. **Status:** a proof of concept. The
decisions it fed are now normative — spec draft 0.3, the accepted
live-authoring RFC in the spec repository (`spec/rfcs/live-authoring.md`,
branch `rfc/live-authoring`). The code here predates them; see
*Catching up to draft 0.3*. What it did not answer — where the person
types once Gen has no agent of its own — is
[agents-outside-the-window.md](agents-outside-the-window.md).

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
   `assets/<sha256>.<ext>` and points the world there — content
   addressing that keeps the `.world` a self-contained, portable
   database even on devices without git (mobile included). A new version
   is new bytes referenced again; old versions stay for the history that
   used them.
3. **The package is head-first.** `manifest.json` is the world now,
   written only by the committer (a direct write is put back).
   `fold(base, log) == manifest.json` is checkable at any time.
4. **In a git repository, every batch is a commit** authored by whoever
   sent it, and replay walks the commits as keyframes.

Where the code differs from the draft-0.3 decisions below, the decisions
win and the catch-up list says so.

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
  resets rotation and scale; a material patch with only a texture drops
  the color. Agents send partial changes by nature. The proof of concept
  merges `transform`, `material`, `light` and `SetEnvironment` at
  ingestion (JSON Merge Patch, RFC 7396). *Decided:* the spec adopts that
  merge for the Authoring profile, at ingestion — the committed op
  carries the merged whole value, so the fold itself never merges and
  the log's patch semantics are unchanged.
- **No op reaches meta, avatar, tours, soundtrack or creations.** Through
  an ops-only API an agent cannot change them at all. *Decided:* one op —
  `ModifyWorld`, patching those fields the way `ModifyEntity` patches an
  entity — not per-field ops.
- **Names at ingestion are right.** Binding names to ids (and allocating
  ids for spawns) at ingestion is what made the ops writable by an agent —
  the spec's rule, validated, and extended to op addresses: a string
  where an entity id goes is a name.
- **Strict mode belongs to the authoring API.** Must-ignore would have
  kept the `colour` typo silently; refusing it with a pointer is what
  lets an agent correct itself. *Decided:* the Authoring profile MUST
  read the batches it is sent strictly.
- **Portability first; git as an extension.** Platforms like mobile have
  no git, so the package must be fully self-contained: hash-named asset
  copies plus the append-only ops log are a portable, dependency-free
  history — a `.world` is its own database anywhere. Git stays strictly
  optional, an optimization for desktop workflows (batches as commits,
  keyframe replay), never a mandatory dependency. *Decided:* content
  addressing is the baseline everywhere. Even under git the immutable
  copies cost almost nothing — identical bytes are one git blob, so only
  genuinely new versions add to the object store, which any history
  would. The spec's allowance of logical names under git (`MAY`,
  spec/package.md) is there for repositories that want readable asset
  diffs; it is not this project's default.
- **What git takes over, and what it doesn't.** When enabled, commits
  give history, authorship, diff, revert, branches and transport for
  free, and replay becomes walking keyframes. It does not give op-level
  intent (the log still does), real-time collaboration, or a semantic
  merge. *Decided:* deterministic serialization first — the canonical
  text (members sorted, entities by id, plain arrays inline) makes
  ordinary textual merges of `manifest.json` clean; a semantic merge
  driver stays open (`ops.jsonl` uses `merge=union` either way).

## Decided since (spec draft 0.3)

Beyond the findings above, from the review of this proof of concept:

- **A checkout is not a direct write.** When `manifest.json` changes to
  bytes `package.json` names (`world_sha256`), something moved the whole
  package — a `git checkout`, a pull, a sync client — and the authority
  reopens it and shows the new head. Any other change is still refused.
  Reloading on *any* outside change would quietly accept direct writes
  again; the hash match is the line.
- **A read-only `manifest.json` is a hint, not a guard.** An authority
  MAY clear the file's write permission while it holds the package, so
  an ordinary write fails at once with the OS's own error. A writer that
  saves by renaming a temp file over the target — or that makes the file
  writable again — gets through it, so the authority still checks every
  change and refuses what isn't a checkout.
- **Entries carry a `message`** — a commit message, part of the entry's
  identity — instead of this proof of concept's tool-record workaround.
- **MCP is an equal way in, not a replacement.** A stdio MCP server is
  started by the agent, so it is a shim that reads
  `.live/endpoint.json` and calls the same local HTTP API; an MCP server
  over HTTP on localhost needs the token for the same reason the API
  does — a web page in the browser can post to localhost.

## Catching up to draft 0.3

The code here still builds on the 0.1 crate and predates the decisions:

- move to openworldformat 0.3, and send `ModifyWorld` through the API;
- carry the batch's message as the entry's `message` field;
- write `manifest.json` with the crate's `manifest_text` (the canonical
  text) instead of preserving the previous entity order;
- reopen on a checkout — bytes matching `world_sha256` — instead of
  restoring, and make `manifest.json` read-only while held, as the hint;
- keep hash-named asset copies everywhere (portability first); git
  stays the optional layer it is here.

## Not done

- A cold test with a fresh agent that has only `AGENTS.md`.
- Switching git branches under an open app.
- Replay reading assets at the replayed commit: replay walks commits'
  `manifest.json`, but loads asset files from the working tree, so an
  old commit would show today's bytes for a changed asset — the canvas
  must load them from that commit (`git show <commit>:assets/…`).
- Redo, and per-author undo.
- Mesh, audio and camera changes in the open scene (`OpsApplier`
  doesn't apply them).
- Telling the agent, not only the app's log, when its direct write was
  refused — the read-only hint makes ordinary writes fail fast, but the
  authority's refusal stays the source of truth.
