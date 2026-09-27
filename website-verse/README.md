# LocalGPT Verse website

The landing page for [verse.localgpt.app](https://verse.localgpt.app/). Hand-written
static HTML/CSS/JS — no build step, no dependencies. The docs live on the family's
docs site, [localgpt.app/docs/verse](https://localgpt.app/docs/verse), in the
`localgpt` repository (`website/docs/verse/`); `_redirects` sends the old
`/docs/*` URLs there.

## View locally

```bash
python3 -m http.server 8000 -d website
# open http://localhost:8000
```

(Opening `index.html` directly from disk also works.)

## Deploy

Published to Cloudflare Workers as the `localgpt-verse` Worker (see `wrangler.toml` —
static assets only, no Worker script):

```bash
website/deploy.sh
```

First run needs `npx wrangler login` (or `CLOUDFLARE_API_TOKEN` in CI). The files in
`.assetsignore` (this README and the deploy config) are not served.

## Structure

- `index.html` — landing page
- `_redirects` — the old docs URLs, now on localgpt.app
- `assets/css/style.css` — the design system (mirrors `src/theme.rs`)
- `assets/js/main.js` — nav toggle, hero world-cycler, scroll reveal
- `assets/fonts/` — Marcellus + Hanken Grotesk (SIL OFL, see `OFL-*.txt`)

The hero cycles through the eight in-app world palettes; animations switch off
under `prefers-reduced-motion`, matching the app's Comfort settings. The footer
carries the family strip every LocalGPT site shares: LocalGPT, Gen, Verse, MD,
localgpt.world, localgpt.rs.
