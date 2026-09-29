# The session package: one artifact for a world, its edits and its play

**Status:** implemented (see [Implementation status](#implementation-status)).
Supersedes the tool-call-log half of
`docs/rfcs/worldgen/world-package-format.md` (the landscape survey below its
"Synthesis" stays useful); extends the op log of
`docs/rfcs/multiplayer/collaborative-world-engine-architecture.md` (spec
phase 5) from "the room's history" to "the world's file format".

## The problem

A Gen session leaves artifacts in three places that don't know about each
other:

- `world.ron` — the world at the moment of a save, with no memory of how it
  got there.
- `<workspace>/sessions/<name>/ops.jsonl` — the room's op log: every
  committed batch with its author, so `--resume` and `--replay` work, but
  only for web sessions, and it records nothing the room's document doesn't
  hold.
- the generation log (`world-types::GenLogEntry`) — every tool call the
  model made, with arguments and a result hash, written to its own file and
  never reconciled with the ops those calls caused.

Three questions have no answer today. *What did the model do here, versus
the people?* (audit). *Can I watch this session again, visitors and all?*
(replay). *Can I fork from the moment before that edit?* (branching). All
three are reads over one history, so the format should be one history.

## The decision

**A world session is a base world plus an append-only log; state at any
revision is a pure fold.** Not a state file with history bolted on — an
event-sourced document. Replay, resume, undo, auditing the model, and
forking all become the same mechanism: read the log.

The package is the *artifact* every app emits and consumes. It is not the
*source* any app is authored in — Gen's source is the world itself, MD's is
the `.md`, Verse's is the song and its analysis sidecar
(`docs/world-strategy.md` §5's rule, unchanged).

## The package

```
<name>.world/                  a directory (canonical; the log appends)
  world.ron                    base world at revision base_revision
                                (WorldManifest, RON — the authoring form)
  ops.jsonl                    the one log — one JSON line per entry
  session.json                 package metadata (see below)
  snapshots/rev-<N>.ron        derived keyframes, never authoritative
  assets/<sha256>…             content-addressed leaves (already the
                                pattern: MeshAssetRef::sha256)
  transcript.jsonl             optional: prompts/completions sidecar
```

A zipped `.world` is a transport form for publishing a finished session;
the directory stays canonical because an append-only log cannot live inside
a zip without rewriting it.

`session.json`:

```json
{
  "format_version": 1,
  "name": "castle-build",
  "app": "gen",
  "base_revision": 0,
  "head_revision": 482,
  "seed": 1730204311,
  "world_sha256": "…",       // of world.ron at base_revision
  "log_sha256": "…",         // of ops.jsonl as of head_revision
  "updated_ms": 1790000000123
}
```

The invariant, asserted by the reader and by tests:

```
fold(base, entries with revision > base_revision) == state at head
```

## The log line

One line per entry; an entry is a committed batch of ops by one author.
The envelope is today's `OpLogEntry` (`world-sync/src/oplog.rs`) with `ops`
generalized from `Vec<EditOp>` to `Vec<SessionOp>`:

```rust
#[serde(untagged)]
pub enum SessionOp {
    Edit(EditOp),        // serializes exactly as the EditOp always did
    Tool(ToolRecord),    // the model's call, next to its effects
    Input(InputRecord),  // a visitor's sampled state
    State(StateRecord),  // host game state (score, inventory, …)
    Clock(ClockRecord),  // transport (Verse's song clock, tour clocks)
}
```

`untagged` with `Edit` first is the compatibility rule: a log written
before this change holds `EditOp` JSON and parses as `Edit`; a `SessionOp`
written today serializes its `Edit` variant exactly as the old format did.
Old readers see new logs' edit entries and skip what they don't know (the
same tolerance the reader already has for unreadable lines).

```json
{"revision": 42, "timestamp_ms": 1790000000123,
 "author": {"peer": 3, "name": "maya"},
 "ops": [{"ModifyEntity": {"id": 17, "patch": {"transform": …}}}]}
{"revision": 42, "timestamp_ms": 1790000000131,
 "author": {"name": "llm"},
 "ops": [{"tool": "gen_spawn_primitive", "args": {…},
          "result_hash": "sha256:…", "phase": "blockout"}]}
{"revision": 42, "timestamp_ms": 1790000001400,
 "author": {"name": "visitor-7"},
 "ops": [{"input": {"actor": "visitor-7",
                    "sample": {"position": [3.1, 1.8, -2.0], "click": 17}}}]}
{"revision": 42, "timestamp_ms": 1790000001455,
 "ops": [{"state": {"score.chest": 10}}]}
{"revision": 42, "timestamp_ms": 1790000002000,
 "ops": [{"clock": {"playing": true, "position_s": 41.5}}]}
```

Which op kinds change the document: **only `Edit`.** Everything else is
history that folds to nothing — the document's revision is the authority's
revision, and non-edit entries carry the current revision rather than
bumping it. Readers must tolerate repeated and out-of-order-looking
revisions between edits; the edit entries themselves stay totally ordered.

Why each kind is there:

| Kind | Who writes it | What it buys |
|---|---|---|
| `edit` | the authority, for anyone | resume, replay, per-user undo (inverses already exist), forking |
| `tool` | the app, when a tool call runs | audit ("what did the model do"), genlog becomes a view over the log instead of a second file |
| `input` | the app, sampled (~10 Hz) while recording | playthrough replay: triggers re-run offline over logged inputs |
| `state` | the app, when host state changes | the game state that lives in no document (score, inventory) is now history |
| `clock` | the app, on transport events | a Verse performance replays: modulations are deterministic given the clock |

`tool` records intent; the `edit` entries the same call caused record
effect. Linking them precisely (a `cause` field) is left until a reader
needs it — both carry timestamps within milliseconds of each other.

## Replay, in tiers

1. **Structural** — `fold(base, entries)` at any revision. `--resume` (seek
   to head), `--replay` (time-lapse through edits), diffing two revisions,
   forking at a revision (copy base + log prefix).
2. **Playthrough** — offline trigger re-run over `input` and `state`
   entries, rendered headless. Gen's trigger runtime is already driven by
   visitor position and clicks; this tier feeds it the log instead of the
   live player. The web viewer can do the same, which turns a session on
   localgpt.world into something watchable.
3. **Bit-exact cinematic** — needs a determinism contract: the `seed` in
   `session.json`, behaviors and triggers keyed to log time rather than
   wall clock, physics seeded or recorded as state. FunDSP graphs are
   already pure functions of time, so audio holds. Not built; the fields
   are reserved.

Tier 1 is exact by construction (it is the room's own document path).
Tier 2 is approximate by default — input is sampled, not streamed — and
that is the right trade: presence was never going to be bit-exact, and a
10 Hz visitor curve replays a trigger's story faithfully.

## Snapshots and compaction

A snapshot is `doc.to_manifest()` written to `snapshots/rev-<N>.ron`:
derived, never authoritative, deleted freely. Reading at a revision picks
the nearest base-or-snapshot at or below it and folds forward — the
keyframe seek every video player has. Compaction folds to a new `world.ron`
base and keeps the old log for history (Kafka's log compaction, and
localgpt-core's session compaction, are the same move). Neither changes
the invariant; a test asserts it both ways.

## What stays out of the log

Presence at full rate (sample into `input` coarsely if ghost replay is ever
wanted, never stream), physics scratch state, renderer state, and anything
wall-clock-dependent — timestamps describe the past; they never drive the
fold.

## MD and Verse

Neither app changes its source; both can emit and consume the package:

- **MD** compiles `.md` + sidecar to a base world (`draft::compile`), and a
  document rebuild is one `Batch` of spawn/delete/modify ops —
  `world-sync::diff` already computes exactly that between two world
  documents, so the file watcher can feed a live room without a new op.
  Visitors walking a deck are `input` entries.
- **Verse** exports its manifest (soundtrack curves, modulations) as the
  base and writes `clock` entries on transport; a performance is then
  base + clock + visitor input. The licensing gate stands: only CC0 audio
  is packaged, exactly as on localgpt.world today.

## Implementation status

- `world-types`: nothing new needed; `GenLogEntry` remains for readers of
  the old generation log.
- `world-sync` (stays I/O-free): `SessionOp`, `ToolRecord`, `InputRecord`,
  `StateRecord`, `ClockRecord`, `SessionMeta` in `session.rs`;
  `OpLogEntry.ops` is `Vec<SessionOp>`; `OpLogEntry::edit_ops()` and
  `fold_log` are the pure reader paths.
- `world-agent` `session.rs`: the package on disk — `write_base`,
  `append_entry`, `write_snapshot`, `update_meta`, `read_package` (with
  revision seek and torn-line tolerance), `sha256` integrity helpers.
  Lives here because Gen, MD and Verse all depend on this crate and none
  can depend on `localgpt-core`.
- Gen: web sessions are packages (base + `session.json` written when the
  room opens, tool calls and score changes land in the log, snapshots
  every 500 committed revisions); `--resume` and `--replay` accept a
  session name, an `ops.jsonl` path, or a package directory.
- Not yet wired: `input`/`clock` recording (types and fold tolerance are
  in), MD publish, Verse transport logging, the web viewer's scrub
  controls.

## Related documents

- `docs/rfcs/multiplayer/collaborative-world-engine-architecture.md` — the
  authority, revisions, and the op log this builds on.
- `docs/rfcs/worldgen/world-package-format.md` — the survey whose
  "deterministic tool call log" this replaces with the real thing.
- `docs/world-strategy.md` §5 — source format versus distribution export;
  the package is a third thing, the artifact, and changes nothing there.
- `docs/gen/multiplayer.md` — the user-facing session documentation.
