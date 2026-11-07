# LocalGPT Website (Zola)

This directory contains [localgpt.app](https://localgpt.app), the LocalGPT landing page and user documentation, built with Zola and deployed manually to Cloudflare Workers.

## Commands

```bash
zola serve
zola check --skip-external-links
zola build
cargo xtask deploy app # Build and deploy to Cloudflare Workers
```

## Layout

- `content/docs/`: current user documentation (`app.md`, `gen/`, `verse/`, `md/`, installation, CLI and architecture).
- `templates/`: Tera templates (`base.html`, `index.html`, `docs_page.html`).
- `static/`: CSS, logos and updater manifests.
- `config.toml`: site metadata and the shared family links.

The desktop interface is native egui/eframe; the web page in `crates/app/ui` belongs to paired phones and tablets, not the desktop window. Verify product claims against the crates before editing copy.
