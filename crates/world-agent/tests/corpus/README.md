# The authoring corpus

Batches as authors sent them, each with the answer the authority must give.
`tests/corpus.rs` replays every case through `LiveWorld::submit` — no model
runs; a case is a file — and fails when a mistake stops being refused, when a
refusal stops saying what an author needs to fix it, or when a batch that
should commit no longer does.

A world's log shows what its authors got right. This is the other half.

## A case

One JSON file:

```json
{
  "what": "`materail` for `material`: the refusal points at the key.",
  "source": "seed",
  "world": "yard",
  "batch": [{"ModifyEntity": {"id": "crate", "patch": {"materail": {"color": [1, 0, 0, 1]}}}}],
  "expect": {"rejected": ["/ModifyEntity/patch/materail"]}
}
```

- `world` names the starting world, `worlds/<name>.json`: a manifest the
  batch is sent against, at revision 0.
- `batch` is what was sent: an array of ops, or the envelope
  (`{"ops": [...], "expected_revision": ...}`).
- `expect` is one of:
  - `{"rejected": [...]}` — one string per reason, in order. Each must
    appear in the reason the authority gives. Write the part an author acts
    on (the pointer, the name), not the whole sentence.
  - `{"committed": {"warnings": [...], "lint": [...]}}` — the batch commits,
    the receipt carries exactly as many warnings, each containing its
    string, and `localgpt world lint` then finds each code in `lint`. The
    last is for mistakes the authority cannot know are mistakes: a figure
    placed at its feet is a valid entity.

## Where cases come from

- `seed/` — written for the corpus on 2026-11-13, from the mistakes the
  package guide warns of and the shapes other tools give the same data. They
  were not taken from a session.
- `captured/` — sent by an agent to a live world. Empty until someone adds
  one: the authority has kept refused batches only since the change that
  added this corpus.

## Adding a captured case

The authority keeps every batch it refuses in the world's
`.live/rejected.jsonl`, as sent: `batch`, `author`, `errors`, and `against`,
the entry the batch was read against.

1. Take the line whose mistake is worth keeping.
2. Save the world it was sent against as a starting world, when no existing
   one will do: `localgpt world show --at <against> --json >
   worlds/<name>.json` (`--at base` when `against` is null). Cut it down to
   what the batch touches; a starting world is read by people.
3. Write the case in `captured/`, with `"source": "captured"`, the line's
   `batch` unchanged, and in `expect` the part of each reason that helped —
   or, when none did, the reason it *should* have given, and fix the
   authority until the case passes. A reason that did not help is the
   finding.

Leave out anything of the world that is not needed to reproduce the answer,
and the author's name: a case records a mistake, not who made it.
