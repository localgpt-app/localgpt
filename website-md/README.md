# localgpt.md

Drop a Markdown file, walk through it as a 3D world — in the tab. No install,
no account, **no upload**: the compiler is WASM in the page, so the file never
leaves the browser.

This is the developer wedge from `docs/world-strategy.md` §13.4: the cheapest
distribution the project has, because there is nothing to install, no key, no
model and no inference bill — the page cannot be farmed for compute, so it can
be free forever. It renders the **rule-derived draft** (the shape of the
document: sections become places, one tour stop each) plus any ` ```world `
fences, and labels itself as the draft; the LLM-authored scenery is the
desktop app's half, linked from the page.

## Assembled, not stored

`npm run assemble` (or `scripts/assemble.sh`) builds the page tree; only the
page itself and the scripts are tracked.

| Copied in | From |
|---|---|
| `viewer/world-viewer.js` | `crates/world-export/js/` — same commit, cannot drift |
| `vendor/three/` | `website-world/vendor/` (the no-third-party-requests rule) |
| `samples/*.md` | `crates/md/samples/` |
| `wasm/` | `crates/md-web` via `wasm-pack --target web` (~546 KB) |

The WASM depends on `localgpt-md` with `default-features = false` — the pure
half only. That dependency edge is the page's promise: nothing here can grow a
renderer or an inference engine without it being a Cargo.toml decision.

## Check

```bash
npm ci && npx playwright install --with-deps chromium
npm run assemble && npm run check
```

`check` serves the page, loads it in headless Chromium, drops the app's own
`hello.md` through the real file input, and requires a rendered world — the
same contract the page promises a visitor. CI runs it in the `viewer` job.

## Deploy

```bash
./deploy.sh    # assembles, then wrangler deploy
```

Publishes as the `localgpt-md` Worker. Domain routing (dashboard): localgpt.md
is the canonical host; md.localgpt.app 301s to it — the reverse of the old
redirect, per `docs/world-strategy.md` §13.5.

## License

Apache-2.0, as the rest of this repository.
