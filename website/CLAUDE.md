# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
npm run start          # Dev server with hot reload (localhost:3000)
npm run build          # Production build (catches broken links)
npm run serve          # Serve the production build locally
npm run typecheck      # TypeScript type checking
```

Always run `npm run build` after changes to verify no broken links (`onBrokenLinks: 'throw'` in config).

## Architecture

Docusaurus 3 site for the [LocalGPT](https://github.com/localgpt-app/localgpt) project. Dark mode only, no locale switching.

### Content

- `docs/*.md` — documentation pages, referenced by slug in `sidebars.ts`
- `blog/*.md` — blog posts (historical records, don't modify retroactively)
- `sidebars.ts` — sidebar structure; doc IDs must match filenames without `.md`

### Custom Components

- `src/pages/index.tsx` — homepage with hero (left/right split: branding + Gen card) and feature grid
- `src/pages/index.module.css` — homepage styles (hero layout, Gen card, install command)
- `src/components/HomepageFeatures/index.tsx` — feature grid below the hero (3-column layout)
- `src/components/HomepageApps/index.tsx` — "LocalGPT family" app cards (Gen, Verse, MD)
- `src/css/custom.css` — global styles, Infima overrides, navbar icon SVGs (YouTube, X, GitHub)

### Key Config

- `docusaurus.config.ts` — site config, navbar, footer, Prism languages (bash, toml, rust, json)
- `static/logo/` — logos and favicons
- `firebase.json` — hosting config for deployment

## Conventions

- The primary color is `#25c2a0` (teal green), used in Infima vars and component styles.
- LocalGPT Gen vision: immersive explorable worlds (geometry + materials + lighting + camera). Don't describe Gen as only "3D scene generation." Gen's procedural audio is launched; music-driven worlds belong to LocalGPT Verse, not Gen.
- This site is the family hub and the only docs site: the assistant's docs plus `/docs/gen`, `/docs/verse` (a 3D world for every song) and `/docs/md` (walk through a Markdown file as a 3D world). Each app's landing page (gen.localgpt.app, verse.localgpt.app, md.localgpt.app) lives next to its code and links into these docs; don't grow docs pages on those sites. Check the Verse and MD pages against `../localgpt-verse` and `../localgpt-md` when those apps change.
- The navbar "Apps" dropdown points at the app docs. The footer "Family" column is the family strip every LocalGPT site shares: LocalGPT, Gen, Verse, MD, localgpt.world, localgpt.rs, in that order and with the same one-liners (each site skips itself). The apps also appear in the homepage family section (`src/components/HomepageApps`) and `docs/ecosystem.md`.
- Doc pages that reference LocalGPT CLI commands should link to the corresponding docs page when one exists (e.g., `[`gen`](/docs/gen)`).
- When documenting CLI features, verify against the Rust source at `../localgpt/` — the docs have historically lagged behind the code.
