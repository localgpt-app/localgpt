# CLAUDE.md

This file provides guidance when working with the LocalGPT website and documentation.

## Commands

```bash
zola serve             # Local dev server with live reload (http://127.0.0.1:1111)
zola build             # Production build into public/
zola check             # Verify internal and external links
```

No Node.js or npm dependencies required. Single fast Rust static site generator.

## Architecture

Zola static site for [LocalGPT](https://localgpt.app). Dark mode only.

### Structure

- `content/docs/` — Documentation pages and sections (`gen/`, `verse/`, `md/`).
- `content/templates/` — 3D World Templates catalog and detail pages.
- `templates/` — Tera templates:
  - `base.html` — Site header, navigation, search bar, and family footer.
  - `index.html` — Homepage with hero video, install commands, features, and app grid.
  - `docs_page.html` — Documentation layout with categories sidebar and page TOC.
  - `template_single.html` — Individual world template showcase.
  - `templates_index.html` — Categorized world templates catalog.
- `static/` — Static assets (CSS, logos, icons, redirect files).
- `config.toml` — Zola site configuration.
- `firebase.json` — Hosting deployment configuration (serves `public`).

## Conventions

- The primary color is `#25c2a0` (teal green), with dark background `#121214`.
- This site is the family hub and the only docs site: the assistant's docs plus `/docs/gen`, `/docs/verse`, and `/docs/md`.
- The footer "Family" strip is shared across all LocalGPT sites.
