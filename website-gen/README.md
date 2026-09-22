# gen.localgpt.app

The landing page for LocalGPT Gen: static HTML with inline CSS and JS, and
no build step. The Gen docs stay at
[localgpt.app/docs/gen](https://localgpt.app/docs/gen) (`../website/docs/gen`),
and this page links to them rather than copying them.

## Preview and deploy

```bash
python3 -m http.server 8000 -d website-gen   # http://127.0.0.1:8000
website-gen/deploy.sh                        # the "localgpt-gen" Cloudflare Worker
```

The first deploy needs `npx wrangler login`. After that, attach
`gen.localgpt.app` to the Worker as a custom domain in the Cloudflare
dashboard.

## Files

| File | What it is |
|---|---|
| `index.html` | The whole page, including its CSS and JS |
| `poster.jpg` | Video poster, cropped from `../website/static/img/desert-pyramids-ufo-screenshot.png` (the same Pyramids world as the demo video) |
| `og.jpg` | 1200×630 link-preview image, from the same screenshot |
| `localgpt-icon.svg` | The family mark, same file as `../website/static/logo/localgpt-icon.svg` |

## Conventions

- Verify feature claims against the Gen source (`../crates/gen`) and docs
  before changing copy; this page must not promise what Gen doesn't do.
- Describe Gen as building explorable worlds, not just "3D scene generation".
  Music-driven worlds belong to LocalGPT Verse.
- The repo is public: never name the closed-source sibling 3D platform.
- The demo video loads only on click (`youtube-nocookie.com`); until then the
  page makes no third-party requests.
