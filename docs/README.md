# LocalGPT documentation

This directory contains the small set of internal documents that still describe current behavior, active designs or ongoing product decisions. User documentation lives in [`website/content/docs/`](../website/content/docs/); contributor commands and the workspace map live in [`CLAUDE.md`](../CLAUDE.md).

Historical RFCs, completed implementation trackers, assistant/harness documentation and superseded designs are not kept as parallel documentation trees. Read them through Git history when needed; `CHANGELOG.md` remains the release record.

## Current documents

### Product and validation

- [`world-strategy.md`](world-strategy.md) — the current product shape, audience, surfaces and priorities.
- [`previs-user-test.md`](previs-user-test.md) — the observation sheet for previs user sessions.

### Living technical references

- [`gen/gen-audio.md`](gen/gen-audio.md) — Gen's audio architecture and authoring tools.
- [`gen/multiplayer.md`](gen/multiplayer.md) — how Gen's collaborative sessions work today.
- [`ecosystem/composable-agent-guide.md`](ecosystem/composable-agent-guide.md) — connecting external MCP runtimes to Gen.
- [`releasing.md`](releasing.md) — cutting a crates release and a desktop release.

### Designs and implementation records

The [RFC index](rfcs/README.md) lists only active designs and references that remain useful for current work:

- the native all-Rust application and the non-linear world editor;
- the phone/tablet remote and the proposed rooms channel;
- Blender and CAD implementation lessons that still inform the editor;
- the current multiplayer authority and session package.

The Open World Format has its own [specification and RFCs](https://github.com/openworldformat/openworldformat). Format changes belong there, not in a second LocalGPT design document.

## Maintenance rule

A document stays here only while it does at least one of these jobs:

1. explains current behavior not covered by the user docs;
2. records an active design and its remaining work;
3. supports an ongoing product decision or user-validation effort.

When code or a decision changes, update the document in the same change. When none of those jobs remains, delete the document and its inbound links instead of adding another status caveat.
