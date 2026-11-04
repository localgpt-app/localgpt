# LocalGPT Internal Documentation

Internal specs, RFCs, research, and roadmap tracking. For **user-facing documentation**, see the [website](../website/docs/).

## Directory Structure

```
docs/
├── world-strategy.md        # The decisions behind the shape of it all, dated
├── rfcs/                    # Design proposals — see rfcs/README.md for the
│   │                        #   index; every RFC carries a **Status:** line
│   ├── authoring/           # The live-editing arc: agents outside the window,
│   │                        #   the harness going, the page, rooms, all-Rust
│   ├── multiplayer/         # Gen's collaborative rooms: one parent spec + 3 deep-dives
│   ├── worldgen/            # The world data model
│   └── research/            # Landscape surveys that informed decisions
├── gen/                     # Gen reference (audio, multiplayer, selection, USD)
├── ecosystem/               # Gen as an MCP component of other agent runtimes
├── case-studies/            # Why worlds built as source are code, not content
├── previs-user-test.md      # The observation sheet for previs sessions
├── roadmap/                 # Implementation tracking (historical since 2026-03)
│   ├── gen/                 # Phase 1-4 RFCs + tracker (Doc-0)
│   ├── headless/            # Headless pipeline TODOs (H1-H5)
│   └── world/               # World systems TODOs (P0-P5, WG1-WG7, AI3)
└── archived/                # Fully implemented or superseded docs
    ├── rfcs/                # Completed RFCs (XDG, security policy, monorepo, etc.),
    │                        #   the SpacetimeDB data model (gone 2026-11-01) and
    │                        #   the assistant's notifications draft
    ├── gen/                 # Superseded gen docs; AI1 asset generation and AI2 NPC
    │                        #   brains, the service-backed tiers (gone 2026-11-01)
    ├── security/            # The shell sandbox's spec, the sandbox comparison,
    │                        #   the assistant's security TODO
    ├── mobile/              # The native apps' research (gone 2026-10-31)
    └── architecture/        # Bridges, messaging, pairing, process isolation,
                             #   local embeddings (gone 2026-10-31); the SpacetimeDB
                             #   integration, the provider-layer spec, the assistant's
                             #   web search, its plugin-lifecycle reviews and the Orca
                             #   orchestration study (the harness, gone 2026-11-01)
```

## Key Entry Points

- **What LocalGPT is now**: `rfcs/authoring/the-harness-goes.md` — no model, no service, no key, no agent by name; the agents come from their makers and edit a world through its API
- **The decisions, dated**: `world-strategy.md` — §13 the one desktop app, §15 previs first
- **RFC index**: `rfcs/README.md` — every RFC with its status, what supersedes what, and how to write one
- **Roadmap status** (historical): `roadmap/gen/Doc-0-Implementation-Tracker.md`, `roadmap/world/TODO-START.md`, `roadmap/headless/TODO-START.md`

## Relationship to Website Docs

The `website/` folder is the published Zola site (https://localgpt.app). This `docs/` folder is for internal tracking and design work. Some docs here informed website pages but are kept separately for research detail that doesn't belong in user-facing documentation.
