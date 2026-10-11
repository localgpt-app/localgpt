# The previs page

Drop a Fountain screenplay, get a previs board — in the tab. No install,
no account, **no upload**: the draft is WASM in the page, so the script
never leaves the browser.

This is the no-install first contact `docs/world-strategy.md` §15 needs:
a director with script pages gets a set, stand-ins and a first pass of
coverage per scene — real lenses, measured camera positions, the actor
tracked by entity — plus the shot list and the printable board, without
`cargo` or the desktop app. Revising shots by asking (and treatments as
branches) is the app's half, linked from the page.

The page shows the board the draft produced (the self-contained
`board.html`, in an iframe) and offers three downloads: the board, the
shot list, and a zip of the per-scene `.world` packages — Open World
Format worlds any viewer of the format opens.

## Assembled, not stored

`npm run assemble` (or `scripts/assemble.sh`) builds the page tree; only
the page itself and the scripts are tracked.

| Copied in | From |
|---|---|
| `samples/cold-eggs.fountain` | `crates/previs/samples/` |
| `localgpt-icon.svg` | `website-md/` |
| `wasm/` | `crates/previs-web` via `wasm-pack --target web` |

The WASM is `localgpt_previs::draft` — the same function the
`localgpt-previs` command line writes to disk — so the page's board is
the CLI's, byte for byte. Nothing in `crates/previs` touches a model, a
renderer or the network; that dependency edge is the page's promise.

## Check

```bash
npm ci && npx playwright install --with-deps chromium
npm run assemble && npm run check
```

`check` serves the page, loads it in headless Chromium, drops the crate's
own `cold-eggs.fountain` through the real file input, and requires two
scenes, seven shots, a rendered frame per shot in the board, and a zip
holding both packages — the same contract the page promises a visitor.
CI runs it in the `viewer` job.

## Deploy

`./deploy.sh` assembles and runs `wrangler deploy` (the `localgpt-previs`
Worker, static assets). The hostname is set in the Cloudflare dashboard.
