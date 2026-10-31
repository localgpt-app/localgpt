# RFCs

Design proposals and the decisions behind them. **Every document here opens
with a `**Status:**` line, and that line is the authority** — an RFC whose
status says "implemented" is history, not a plan.

Research reports live in [`research/`](research/) and are a different genre:
landscape surveys that informed a decision, kept for the reasoning, never a
commitment. Implemented or superseded RFCs move to
[`../archived/rfcs/`](../archived/rfcs/) with the date and a pointer to the
code.

## Statuses used here

| | |
|---|---|
| **proposal** | written, not accepted; nothing built against it |
| **implemented** / **implemented in part** | shipped; the document's own status section says how far |
| **deferred** | a real design, deliberately not scheduled |
| **research** | a survey, not a proposal |
| **archived** | implemented or superseded; moved to `../archived/rfcs/` |

## Authoring — the live-editing arc

How a `.world` is changed, by people and by agents, while it is open. Read in
this order; each answers what the previous one left open.

| RFC | Status |
|---|---|
| [authoring/live-editing-poc.md](authoring/live-editing-poc.md) | proof of concept — a folder, a localhost ops API, agents outside the window. Its findings are now normative in the spec's draft 0.3 |
| [authoring/agents-outside-the-window.md](authoring/agents-outside-the-window.md) | proposal — where the person types once Gen has no agent of its own: launch, speak ACP, or emulate a terminal. Adopt ACP at protocol v1; Zed's code is GPL and cannot be used |
| [authoring/the-cli-and-the-world.md](authoring/the-cli-and-the-world.md) | decided, partly built — the CLI is the world's command line and the services other agents use, not another coding agent. `localgpt world` and `localgpt world mcp` exist. Its §3–4 are superseded by the next row |
| [authoring/the-harness-goes.md](authoring/the-harness-goes.md) | proposal, partly done — everything nothing used went 2026-10-31 (the mobile apps, server, bridges, sandbox, shell tools, core's embeddings and daemon modules); 0.5.0 removes the rest of the agent harness: `localgpt` becomes `world`, `world mcp`, `doctor` and `setup`; the app ships through cargo-dist and a desktop workflow; the agents come from their makers, never from here |
| [authoring/world-editor-stack.md](authoring/world-editor-stack.md) | proposal, partly built — the stack when history is the product. One rule: the ECS is a one-way projection of a fold. The headless core is `crates/world-editor`; its chrome section is superseded by the next row |
| [authoring/the-page.md](authoring/the-page.md) | decided, built on macOS — one interface: a web page (React, CodeMirror, xterm.js) over the Bevy viewport, with the agents' own terminals in it. egui and the `bevy_ui` chrome went; [egui-or-not.md](authoring/egui-or-not.md) is superseded |
| [authoring/mobile-remote.md](authoring/mobile-remote.md) | decided; step 1 built 2026-10-31, not yet tried on a phone in a hand — a phone or an iPad runs no agent: it is a remote for the desktop app, which serves it the page, pairs it by a QR code, mirrors the terminals and sends the world as frames. `apps/apple` and `apps/android` went with `mobile-ffi` on 2026-10-31 |
| [authoring/the-world-in-the-page.md](authoring/the-world-in-the-page.md) | measured on a Mac, not decided — should the window's world move into the page, drawn by the format's web viewer? Headless, the page held 60 fps with the world under every load an agent makes. In the app, the page drawing the world ran at 45–51 fps against Bevy's 30, and behind the page Bevy held the page's panels to its 30 too; Bevy's four shadow cascades double its GPU cost, and 120 Hz changed nothing. Windows is handed over (`scripts/world-in-the-page/WINDOWS.md`) |

## Lessons from other tools

Readings of other applications' source, for designs to adopt, adapt or avoid.

| Report | What it covers |
|---|---|
| [authoring/blender-lessons.md](authoring/blender-lessons.md) | Blender 5.4 (GPL, read only): schema and migrations, undo, operators, transform, the outliner, IDs, the Python API as the agent surface |
| [authoring/cad-lessons.md](authoring/cad-lessons.md) | research — SolidCraft and CADCraft, two agent-built CAD apps: one command catalog, contracts an agent can read, never crash, headless parity, `setup`, an in-app ACP chat |

## Multiplayer — collaborative rooms

One parent spec and three deep-dives that name it as their parent.

| RFC | Status |
|---|---|
| [multiplayer/collaborative-world-engine-architecture.md](multiplayer/collaborative-world-engine-architecture.md) | implemented in part — phases 1–4 and 7 shipped; phase 8's SpacetimeDB module is done but unjoined |
| [multiplayer/session-package-format.md](multiplayer/session-package-format.md) | implemented — one artifact for a world, its edits and its play |
| [multiplayer/spacetimedb-3d-audio-data-model.md](multiplayer/spacetimedb-3d-audio-data-model.md) | draft |
| [multiplayer/massively-multiplayer-co-creation.md](multiplayer/massively-multiplayer-co-creation.md) | deferred — §2 deep-dive: interest management, async inference pool |
| [multiplayer/massively-multiplayer-persistent-world.md](multiplayer/massively-multiplayer-persistent-world.md) | deferred — §2 deep-dive: MMO schema, ownership, governance |
| [multiplayer/multi-scale-3d-universe.md](multiplayer/multi-scale-3d-universe.md) | deferred — §2 deep-dive: coordinates and partitioning at planetary scale |

## The world data model

| RFC | Status |
|---|---|
| [worldgen/unified-world-data-model.md](worldgen/unified-world-data-model.md) | implemented, then moved upstream — `world-types` shipped, and the format is now the published `openworldformat` crate. Change the format there |

## Agent

| RFC | Status |
|---|---|
| [agent/notifications.md](agent/notifications.md) | draft |

## Research

Surveys, all from 2026-03-25 unless noted, kept for the reasoning behind
decisions already made.

| Report | What it informed |
|---|---|
| [research/gen-vision.md](research/gen-vision.md) | Gen's direction. Its code snapshot is long overtaken |
| [research/gen-technical-foundations.md](research/gen-technical-foundations.md) | the dual-artifact pattern that became `world-types` and the `.world` package |
| [research/three-tier-artifact-architecture.md](research/three-tier-artifact-architecture.md) | the Prompt / WorldSpec / WorldInstance split. Its bidirectional-sync problem is settled differently in `authoring/world-editor-stack.md` |
| [research/world-package-format.md](research/world-package-format.md) | the package format. Superseded in substance; the landscape survey stays useful |
| [research/multi-agent-strategy.md](research/multi-agent-strategy.md) | shipping subagent spawning before multi-user. `spawn_agent` is in core |
| [research/interaction-paradigms.md](research/interaction-paradigms.md) | the design frontier for persistent AI worlds. Nothing scheduled |

## Where the format's own RFCs live

The `.world` format is a separate project, and its RFCs are not here:
`openworldformat/spec/rfcs/` holds `live-authoring.md`,
`branching-histories.md`, `capability-tiers.md` and `ext-physics.md`. This
workspace consumes the published crate; proposals that change what the format
*means* belong in that repository.

## Writing one

1. Open with `# A sentence-case title` and a `**Status:**` line that says
   what is true today, not what is hoped.
2. State the problem before the design, and name what the design gives up.
3. If it supersedes something, say so in **both** documents.
4. Keep a status section when the work lands in phases, and update it there
   rather than in the prose.
5. When it is done, move it to `../archived/rfcs/` with
   `**Status:** Implemented (Archived YYYY-MM-DD). See <where the code is>.`
