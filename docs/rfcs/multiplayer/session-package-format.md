# The session package: one artifact for a world, its edits and its play

**Status:** implemented for Gen's hosted sessions. Every room
`localgpt-gen --host` opens keeps this directory, and `--resume`,
`--replay` and `--export-session` read it. Structural replay is what is
built; `input` and `clock` recording, playthrough replay, compaction, and
Document or Verse writing a session are not ([Not built](#not-built)). The
log's entry and op kinds are the Open World Format's, defined in its
specification. The directory around the log is Gen's own and is not the
format's package layout ([Two layouts, one log](#two-layouts-one-log)).

## The decision

**A world session is a base world plus an append-only log; state at any
revision is a pure fold.** Not a state file with history bolted on — an
event-sourced document. Replay, resume, undo, auditing changes, and
forking all become the same mechanism: read the log.

The reason is that a session otherwise leaves three artifacts that don't
know about each other: a `world.ron` with no memory of how it got there, a
room's op log that holds only what the document holds, and a generation
log of tool calls never reconciled with the ops those calls caused. Who
did what, watching a session again and forking from before an edit are
all reads over one history, so there is one history.

## Two layouts, one log

Two things in this workspace write an `ops.jsonl`. The line is the same
type in both (`OpLogEntry`, a batch of `SessionOp`s, from the
`openworldformat` crate); the directory around it is not.

| | Gen's session directory (this document) | The format's package |
|---|---|---|
| Written by | the room (`crates/gen/src/net/web.rs`), through `world-agent`'s `session` module | `world-agent`'s `LiveWorld` (the desktop app, `localgpt world`) and `localgpt-previs` |
| Base | `world.ron` | `snapshots/base.json` |
| Head | folded on read | `manifest.json`, kept by the authority |
| Metadata | `session.json` | `package.json` |
| Keyframes | `snapshots/rev-<N>.ron` | git commits, when the package is a repository |
| `id`, `parent`, `message` on an entry | no: a room's history is linear | yes: content-hash ids, so it can fork |
| Assets | none | `assets/<sha256>.<ext>` |

`session.json` is written with the crate's `SESSION_FORMAT_VERSION`, which
is 2, the format's head-first package, although this directory is not
head-first; nothing here reads the field. The session directory is one of
the places `world.ron` still lives after the decision that the package is
the only document (`docs/rfcs/authoring/world-editor-stack.md`).

## The session directory

```
<workspace>/sessions/<slug>/   a directory (canonical; the log appends)
  world.ron                    base world at revision base_revision
                                (WorldManifest, RON)
  ops.jsonl                    the one log — one JSON line per entry
  session.json                 package metadata (see below)
  snapshots/rev-<N>.ron        derived keyframes, never authoritative
```

`<workspace>` is Gen's (`~/.local/share/localgpt/gen-workspace/` by
default); `<slug>` is the session name in lowercase, with every run of
other characters a `-`.

A zip of the directory with a `.world` extension is the transport form;
the directory stays canonical because an append-only log cannot live
inside a zip without rewriting it.

`session.json`, as the room writes it:

```json
{
  "format_version": 2,
  "name": "Castle Build",
  "app": "gen",
  "base_revision": 0,
  "head_revision": 482,
  "world_sha256": "…",
  "log_sha256": "…",
  "updated_ms": 1790000000123
}
```

`world_sha256` is of `world.ron` and `log_sha256` of `ops.jsonl`. The room
refreshes `head_revision` and `log_sha256` when it opens and at each
snapshot, not on every append, so between snapshots they trail the log: a
reader takes the head from the log, as `read_package` does. `seed` is
reserved and not written. `forked_from` is written only by `fork_package`,
which nothing calls yet.

The invariant the reader implements and the tests pin:

```
fold(base, entries with revision > base_revision) == state at head
```

## The log

One line per entry; an entry is a committed batch of ops by one author.
The entry, the op kinds, the fold and the compatibility rule (op kinds are
recognized by shape, edits first, so a log of plain `EditOp`s still
parses) are the format's: `spec/session.md` in
https://github.com/openworldformat/openworldformat. Only edits change the
document and bump the revision; every other kind carries the current
revision and folds to nothing.

What the room writes:

| Kind | Written for | Author |
|---|---|---|
| edit | every committed batch: the host's projection, a guest's `submit`, an `undo` | `host`, or the guest's name and peer id |
| `tool` | the two commands that call `GenerationLog::log` today, `gen_check_drift` and `gen_sync` | `llm` |
| `state` | a score change (`ScoreBoard`) | `host` |
| `input`, `clock`, `merge`, extensions | nothing | |

```json
{"revision": 42, "author": {"peer": 3, "name": "maya"},
 "ops": [{"ModifyEntity": {"id": 17, "patch": {"transform": …}}}],
 "timestamp_ms": 1790000000123}
```

The room's entries carry no `id`, `parent` or `message`: file order is the
chain. A skill directory's `meta/generation-log.jsonl` is still written on
save (`gen3d/world.rs`), so tool calls are in two places and nothing
reconciles them; nothing links a `tool` record to the edits it caused
beyond their timestamps.

What stays out of the log: presence, physics scratch state, renderer
state, and anything wall-clock-dependent — timestamps describe the past;
they never drive the fold.

## Reading: resume, replay, export

- **`read_package(dir, at)`** picks the newest snapshot at or below the
  revision asked for, else `world.ron`, and folds the entries after it —
  the keyframe seek every video player has. It is tolerant: an unreadable
  line, a torn last line included, is skipped and counted, and the fold
  stops with an error at the first entry that no longer applies.
- **Snapshots.** The room writes `snapshots/rev-<N>.ron` every 500
  committed revisions. A snapshot is the folded document: derived, never
  authoritative, deleted freely.
- **`--resume <session>`** (with `--host --web`) loads a session into the
  authority before guests join: a session name, a package directory, a
  bare `ops.jsonl`, a `.world` archive, or a directory with a log and no
  `session.json`, which folds from an empty base. The room then appends
  to the directory of the session it is hosting (`--session-name`, by
  default `<user>'s world`), not the one it resumed. A new directory gets
  the resumed document as its `world.ron`, at the resumed head's
  revision, so its log folds onto it.
- **`--replay <session>`** plays the edits as a time-lapse,
  `--replay-speed` batches a second. It reads a package through
  `read_package`, so once a session has a snapshot it starts from the
  newest one, not from `world.ron`.
- **`--export-session <name> [--out x.world]`** zips the directory as it
  is (entries stored, the walk sorted, so one package zips to one
  archive); `--resume` and `--replay` extract such an archive to a
  temporary directory. The archive holds this layout, not the format's: a
  reader of Open World Format packages finds no `manifest.json` in it.

## Not built

- **`input` and `clock` recording.** The types and the fold's tolerance
  are in the crate; nothing here writes them. Playthrough replay —
  re-running triggers offline over logged `input` and `state` — waits on
  them. Bit-exact replay is not part of the format (`spec/session.md`,
  "Replay and determinism").
- **Compaction.** Nothing folds a session to a new base.
- **Assets.** The room stores no asset files in the directory; the
  format's content-addressed `assets/` is `LiveWorld`'s.
- **Document and Verse.** Neither writes a session, and neither would change
  its source (the `.md`, the song). The design: Document compiles `.md` and
  sidecar to a base world (`draft::compile`), and a rebuild is one `Batch`
  of spawn, delete and modify ops, which `world-sync`'s `diff_scene`
  already computes between a document and a new set of entities; Verse
  exports its manifest as the base and writes `clock` entries on
  transport, packaging only CC0 audio.
- **The web viewer's scrub controls.**

## Where it lives

- The `openworldformat` crate (`session.rs`, `oplog.rs`): `SessionOp` and
  its records, `SessionMeta`, `OpLogEntry`, `OpLogEntry::edit_ops()` and
  `fold_log`. `crates/world-sync` re-exports them and stays I/O-free.
- `crates/world-agent/src/session.rs`: the directory on disk —
  `write_base`, `write_meta`, `read_meta`, `refresh_meta`, `append_entry`,
  `write_snapshot`, `read_package`, `fork_package`, `export_zip`,
  `extract_zip`. It lives there because Gen, Document and Verse all depend on
  that crate.
- `crates/gen/src/net/web.rs`: the room's use of it — the base and
  `session.json` when the room opens (`ensure_package`), the log appended
  in `deliver`, tool and state records and snapshots in
  `web_session_history`, `--resume` in `replay_log`. `--replay` is
  `crates/gen/src/gen3d/replay.rs`.
- `crates/world-types/src/genlog.rs`: `GenLogEntry`, the generation log
  file's line.

## Related documents

- `docs/rfcs/multiplayer/collaborative-world-engine-architecture.md` — the
  authority, revisions, and the room this is the history of.
- `docs/rfcs/authoring/world-editor-stack.md` — the non-linear editor over
  the format's package, and the decision that the package is the only
  document.
- `docs/gen/multiplayer.md` — the user-facing session documentation.
- `spec/session.md` and `spec/package.md` in
  https://github.com/openworldformat/openworldformat — the log, and the
  package layout this directory is not.
