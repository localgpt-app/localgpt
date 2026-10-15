# localgpt.md — the previs page

Drop a screenplay — a Final Draft file, Fountain, or script pages in Markdown
— and get a previs board, in the tab. No install, no account, **no upload**: the draft is
WASM in the page, so the script never leaves the browser.

This is the no-install first contact `docs/world-strategy.md` §15 needs: a
director with script pages gets a set, stand-ins and a first pass of coverage
per scene — real lenses, measured camera positions, the actor tracked by
entity — plus the shot list and the printable board, without `cargo` or the
desktop app. Revising shots by asking (and treatments as branches) is the
app's half, linked from the page: unzip the packages, open one scene live
(`localgpt-gen --live scene-1.world`, or `localgpt-app --world …`), put an
agent on the folder (`localgpt world mcp`) — the walk is
`website/content/docs/gen/previs.md`.

The page shows the board the draft produced (the self-contained `board.html`,
in an iframe) and offers three downloads: the board, the shot list, and a zip
of the per-scene `.world` packages — Open World Format worlds any viewer of
the format opens.

## Why this domain

Until 2026-10-12 `localgpt.md` was the drop-a-Markdown-file-walk-it page of
`docs/world-strategy.md` §13.4. §15 made previs the first audience and that
page's reasons for the domain — nothing to install, no model, no inference
bill, the domain is the pitch, the demo and the funnel are one artifact —
carry over to script pages whole. So the previs page took the domain, the
document-walk page and its `crates/md-web` went (the capability stays in the
desktop app, `localgpt-md`), and the page reads `.md` too — a Markdown
screenplay convention, `crates/previs/src/markdown.rs` — so the domain stays
literal: localgpt.md takes `.md`.

## Assembled, not stored

`npm run assemble` (or `scripts/assemble.sh`) builds the page tree; only the
page, its icon and the scripts are tracked.

| Copied in | From |
|---|---|
| `samples/cold-eggs.fountain`, `samples/cold-eggs.fdx`, `samples/cold-eggs.md` | `crates/previs/samples/` — the same screenplay three ways |
| `wasm/` | `crates/previs-web` via `wasm-pack --target web` |

The WASM is `localgpt_previs::draft::draft_as` — the same function the
`localgpt-previs` command line writes to disk — so the page's board is the
CLI's, byte for byte. The file's name picks the format (`.fdx` is Final Draft
— `crates/previs/src/fdx.rs`, the typed paragraphs with their scene numbers;
`.md` is Markdown pages; anything else Fountain — and XML at the head is Final
Draft whatever the name). Nothing in `crates/previs` touches a model, a
renderer or the network; that dependency edge is the page's promise.

## Limits

The page refuses a file over 20 MB before reading it, and a script with more
than 300 scenes after parsing it and before staging anything (a feature is a
few hundred KB and 100–200 scenes); binary dropped on it is refused as not a
screenplay, and XML that isn't Final Draft as not Final Draft. The
`localgpt-previs` command line has no ceiling.

## Check

```bash
npm ci && npx playwright install --with-deps chromium
npm run assemble && npm run check
```

`check` serves the page, loads it in headless Chromium, drops the crate's own
`cold-eggs.fountain` through the real file input, and requires two scenes,
seven shots, a rendered frame per shot in the board, and a zip holding both
packages — the same contract the page promises a visitor. It then drops
`cold-eggs.fdx` and `cold-eggs.md`, the same screenplay as a Final Draft file
and as Markdown pages, and requires the same board byte for byte; then a 21 MB
file, a 301-scene script, random bytes and an XML page that isn't Final Draft,
and requires each refusal's message. CI runs it in the `viewer` job.

## Deploy

`./deploy.sh` assembles and runs `wrangler deploy` (the `localgpt-md` Worker,
static assets). Routing is in the Cloudflare dashboard: localgpt.md is the
canonical host and md.localgpt.app 301s to it. The old `localgpt-previs`
Worker is idle and can be deleted there.

## License

Apache-2.0, as the rest of this repository.
