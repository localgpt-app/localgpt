# LocalGPT strategy

**Status:** current product direction, updated 2026-11-06. This document records the decisions that still guide work; it is not a history of earlier architectures. Release history is in `CHANGELOG.md`, and superseded designs remain in Git history.

## The product

LocalGPT is a native desktop app where a document, a song, a screenplay or an existing package becomes a walkable, editable `.world`. The person works in the window; their own coding agents work beside it in terminals or over MCP. Gen, MD, Verse and Previs provide the world-building parts, and `localgpt world` provides the package command line and MCP shim.

The product is not an assistant. LocalGPT ships no prompt loop, model provider, key store, persistent personal memory or background agent. An external agent brings its own model and login and edits through the same world authority as a person or a script. Verse's optional CLAP and Demucs support is on-device audio analysis, not a language-model tier.

The desktop interface is native Rust: eframe owns the window, egui draws the panels, and Bevy renders the world into them. A paired phone or tablet uses a browser page served by the app; there is no separate native mobile app.

## The format is the center

A `.world` package contains the current world, its content-addressed assets and the log of every committed change. The Open World Format owns the meaning of those files. LocalGPT adds renderers, editing, audio, collaboration and agent tools around it.

Three rules keep the implementation coherent:

1. **One authority per open world.** Window edits, CLI commands and agent batches commit through `LiveWorld`; with no app open, `localgpt world` uses the same authority directly.
2. **One mapping per output.** `world-bevy` maps the format to Bevy, `world-audio` to sound, and `world-export` to the web viewer and HTML export. New format fields belong upstream, not in an app-specific parallel format.
3. **History is ordinary data.** Undo, redo, branches, replay, review and sharing are operations over the same log, not separate features with separate state.

## First audience: previsualization

Previs is the first focused workflow because greybox geometry is normal, revision is the work, and boards and shot lists are useful deliverables without a game-engine pipeline.

The workflow is:

1. **Script to board.** A PDF, Final Draft file, Fountain or Markdown screenplay becomes a board, a shot list and one `.world` per scene, deterministically and without a language model. The browser page at `localgpt.md` and `localgpt-previs` use the same code.
2. **Board to canvas.** Open a scene in the desktop app or Gen, look through its cameras, move and trim shots on the timeline, and revise coverage through an external agent.
3. **Compare treatments.** Branches hold alternative passes over the same scene; boards and history make the differences inspectable.

Posing, animatics and generated looks remain behind user validation of those steps. The immediate test is whether people who previs today can get from their own pages to a useful board quickly, revise it, and return with another scene. [`previs-user-test.md`](previs-user-test.md) is the observation protocol.

Documents and songs remain first-class modes of the same app, not separate product pushes.

## Surfaces

Each public surface has one job:

| Surface | Job |
|---|---|
| `localgpt.app` | The family hub and the only user-documentation site. |
| `gen.localgpt.app` | Gen's landing page; its details link to the hub. |
| `localgpt.md` | The no-install screenplay-to-previs page. |
| `verse.localgpt.app` | Verse's landing page. |
| `localgpt.world` | The world gallery and browser viewer. |
| `localgpt.rs` | The development log. |

The app, Gen, MD, Verse and Previs share the format rather than acquiring separate document sites or incompatible exports. Shared world links belong on `localgpt.world`, not on a temporary worker hostname or a LAN address. Relayed room links need the secure channel in [`rfcs/authoring/rooms-and-the-channel.md`](rfcs/authoring/rooms-and-the-channel.md) before they become a promoted sharing path.

## Current priorities

1. **Finish and harden the native shell** across macOS, Windows and Linux: panels, document editing, history, timeline, inspector and terminals.
2. **Prove the previs workflow with users** before adding more visual-generation machinery.
3. **Converge phone and tablet support** on the same panel model without introducing a second native app.
4. **Improve direct and agent editing** through one command and authority model, keeping the format and its conformance renderers in step.
5. **Add rooms to an open world only with clear identity and transport security.** Gen's existing LAN and browser sessions remain available in the meantime.
6. **Continue moving repository tooling to Rust** under `cargo xtask rule`; do not add new handwritten JavaScript, TypeScript, Python or shell to the application tree.

## Explicitly out of scope for now

- an embedded assistant, provider configuration, agent presets or a LocalGPT-held key;
- a return to native mobile applications;
- a cloud world authority, SpacetimeDB backend or MMO-scale design;
- generated-look pipelines ahead of evidence that the deterministic previs workflow is useful;
- parallel documentation archives, competitor catalogs and implementation trackers for completed work;
- a second renderer or document model for an individual app.

These are scope decisions, not claims that the ideas are permanently worthless. Reopen one with a current problem and a new design rather than reviving an obsolete document.
