# CLAUDE.md

This file provides guidance when working with the LocalGPT website and documentation.

## Commands

```bash
zola serve             # Local dev server with live reload (http://127.0.0.1:1111)
zola build             # Production build into public/
zola check             # Verify internal and external links
./deploy.sh            # Build with Zola and deploy to Cloudflare Workers
```

No Node.js or npm dependencies required. Single fast Rust static site generator.

## Architecture

Zola static site for [LocalGPT](https://localgpt.app), deployed as a Cloudflare Worker static assets site. Dark mode only.

### Structure

- `content/docs/` — Documentation pages and sections: `app.md` (the desktop app), `gen/` (including `live-canvas.md` and `previs.md`), `verse/`, `md/`, and the assistant's pages.
- `content/templates/` — 3D World Templates catalog and detail pages.
- `templates/` — Tera templates:
  - `base.html` — Site header, navigation, search bar, and family footer.
  - `index.html` — Homepage with hero video, install commands, features, and app grid.
  - `docs_page.html` — Documentation layout with categories sidebar and page TOC.
  - `template_single.html` — Individual world template showcase.
  - `templates_index.html` — Categorized world templates catalog.
- `static/` — Static assets (CSS, logos, icons, redirect files).
- `config.toml` — Zola site configuration.
- `wrangler.toml` — Cloudflare Worker configuration (serves `./public`).
- `deploy.sh` — Deployment script (`zola build` + `npx wrangler deploy`).

## Conventions

- The primary color is `#25c2a0` (teal green), with dark background `#121214`.
- This site is the family hub and the only docs site: the assistant's docs plus `/docs/app`, `/docs/gen`, `/docs/verse`, and `/docs/md`.
- The footer "Family" strip is shared across all LocalGPT sites. MD's entry points at `/docs/md/` here, because `md.localgpt.app` redirects to the previs page at localgpt.md.
- Verify feature claims against the crates before changing copy; the landing page and `intro.md` must not promise what the apps don't do.
