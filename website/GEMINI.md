# LocalGPT Website (Zola) - GEMINI.md

This directory contains the source code for the [LocalGPT](https://localgpt.app) documentation website and templates showcase, built with **Zola**.

## Project Overview

- **Core Technology:** Zola (Rust-based static site generator).
- **Purpose:** Central documentation hub for LocalGPT, a local-first, privacy-focused AI assistant built in Rust.
- **Key Features:**
  - Fast, single-binary compilation with zero Node.js / npm dependencies.
  - Complete documentation for LocalGPT CLI, Gen, Verse, and MD.
  - Interactive World Templates catalog.
  - Dark-mode only design system matching the `#25c2a0` brand.
  - Client-side full-text search with Elasticlunr.

## Building and Running

```bash
zola serve             # Local development server at http://127.0.0.1:1111
zola build             # Production build into public/
zola check             # Verify internal and external links
```

## Project Structure

- `content/docs/`: Markdown files and sub-sections (`gen/`, `verse/`, `md/`).
- `content/templates/`: 3D World Templates.
- `templates/`: Tera templates (`base.html`, `index.html`, `docs_page.html`, `template_single.html`, `templates_index.html`).
- `static/`: Static assets (CSS, logos, icons, updater manifests).
- `config.toml`: Central Zola configuration.
- `firebase.json`: Hosting configuration (serves `public`).
