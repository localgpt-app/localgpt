# LocalGPT MD website

The landing page for [md.localgpt.app](https://md.localgpt.app/) (localgpt.md
redirects there). Hand-written static HTML and CSS: no build step, no
dependencies, and no external requests. The docs live on the family's docs
site, [localgpt.app/docs/md](https://localgpt.app/docs/md).

## View locally

```bash
python3 -m http.server 8000 -d website
# open http://localhost:8000
```

Opening `index.html` directly from disk also works.

## Deploy

```bash
website/deploy.sh
```

Deploys this directory as the `localgpt-md` Cloudflare Worker (static
assets; see `wrangler.toml`). Log in once with `npx wrangler login` first;
the script will remind you if you haven't. There is nothing to compile;
`website/` is uploaded as-is, except the files listed in `.assetsignore`.

The header links to the docs. The footer links back to
[localgpt.app](https://localgpt.app) and carries the family strip every
LocalGPT site shares: LocalGPT, Gen, Verse, MD, localgpt.world, localgpt.rs.
