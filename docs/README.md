# LocalGPT Internal Documentation

Internal specs, RFCs, research, and roadmap tracking. For **user-facing documentation**, see the [website](../website/docs/).

## Directory Structure

```
docs/
├── rfcs/                    # Active/future design proposals
│   ├── multiplayer/         # SpacetimeDB, MMO, collaborative worlds
│   ├── worldgen/            # World generation pipeline, formats, data model
│   ├── agent/               # Multi-agent architecture, notifications
│   └── gen/                 # Gen mode vision and technical foundations
├── architecture/            # Current system architecture docs
│   └── plugin/              # Plugin lifecycle reviews (apply/unapply without restart)
├── gen/                     # Gen mode reference (audio, MCP tools, UX)
├── mobile/                  # Mobile platform research
├── security/                # Security specs and roadmap
├── ecosystem/               # Rust ecosystem integration
├── roadmap/                 # Implementation tracking (actively maintained)
│   ├── gen/                 # Phase 1-4 RFCs + tracker (Doc-0)
│   ├── headless/            # Headless pipeline TODOs (H1-H5)
│   └── world/               # World systems TODOs (P0-P5, WG1-WG7, AI1-AI2)
└── archived/                # Fully implemented or superseded docs
    ├── rfcs/                # 7 completed RFCs (XDG, security policy, monorepo, etc.)
    ├── gen/                 # Superseded gen docs
    ├── security/            # Merged security comparison
    └── ecosystem/           # Redundant checklist
```

## Key Entry Points

- **Roadmap status**: `roadmap/gen/Doc-0-Implementation-Tracker.md` (living document, updated each session)
- **World systems**: `roadmap/world/TODO-START.md` (92% complete, 76/83 specs done)
- **Headless pipeline**: `roadmap/headless/TODO-START.md` (81% complete, 25/31 specs done)
- **Security roadmap**: `security/SECURITY-TODO.md` (consolidated comparison + TODO)
- **Active RFCs**: `rfcs/` subdirectories by domain

## Relationship to Website Docs

The `website/docs/` folder contains the published Docusaurus site (https://localgpt.app). This `docs/` folder is for internal tracking and design work. Some docs here informed website pages but are kept separately for research detail that doesn't belong in user-facing documentation.
