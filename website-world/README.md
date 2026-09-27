# LocalGPT World (`localgpt.world`)

The gallery and viewer for [**localgpt.world**](https://localgpt.world) — worlds
made on someone's machine by the LocalGPT apps, that anyone can step into in the
browser.

This was its own repository until 2026-09-27. It lives here now because its
viewer was a hand-synced **copy** of `crates/world-export/js/world-viewer.js`
and had fallen 185 lines behind it — the published site was rendering with an
older renderer than the apps, and nothing failed. In one repository those files
come from a sibling directory in the same commit, and the render check sits next
to the renderer it tests. See [`docs/world-strategy.md`](../docs/world-strategy.md)
§13.6.

## Assembled, not stored

`npm run assemble` builds the site tree. Only the site's own source is tracked
here; everything else is copied in, and `.gitignore` keeps the copies out.

| Copied in | From | When |
|---|---|---|
| `viewer/world-viewer.js` | `crates/world-export/js/` | always |
| `worlds/{shapes,materials,lights,behaviors,hierarchy_tours,soundtrack,textures}.json` | `crates/world-types/conformance/` | always |
| `worlds/assets/textures/` | `crates/world-types/conformance/assets/` | always |
| `worlds/assets/models/`, `worlds/assets/music/` | the assets checkout's `web/` | `--all` |
| `worlds/posters/*.png` | the assets checkout's `web/` | `--all` |

The GLBs and music are **web-optimized derivatives**, not the pack itself:
`add-world.mjs` shrinks each model with `@gltf-transform` (512 px textures,
weld, simplify, prune). They live in `localgpt-world-assets` as LFS objects
rather than here, because 30 MB of glTF in a code repository's history grows
with every published world and never shrinks. `$LOCALGPT_WORLD_ASSETS` points at
that checkout; a sibling `localgpt-world-assets` (or its former name
`localgpt-verse-assets`) is found automatically.

## Tracked here

| Path | What it is |
|---|---|
| `index.html` | The landing page: a gallery of worlds you can enter |
| `worlds.js` | The gallery's worlds, in order — title, one-liner, which app made it |
| `w/index.html` | One page per world: poster, the one Gen command to keep building on it, and the live viewer |
| `world.html` | The viewer page: `world.html?src=worlds/shapes.json` opens any world in the format |
| `worlds/{gen,md,verse}-*.json` | The curated gallery worlds |
| `vendor/three/` | three.js (MIT), self-hosted so the site makes no third-party requests |
| `scripts/` | `assemble.mjs`; `check-worlds.mjs` (the render check, which also captures posters); `posters.mjs`; `add-world.mjs` |

## Checks

```bash
npm ci && npx playwright install --with-deps chromium
npm run check        # in-repo sources only — what CI runs
npm run check:all    # plus the curated worlds' binaries
```

`npm run check` renders every world whose assets are present and **skips** the
rest, naming them: the two Verse worlds need the derivatives, while the
conformance scenes and the primitives-only Gen and MD worlds need nothing
external. That is what lets CI run this with no assets checkout. `check:all`
renders all 14 and rewrites the posters.

## Adding a world

A world is a `WorldManifest` JSON (schema:
`crates/world-types/world.schema.json`). Every LocalGPT app writes one: Gen's
`gen_save_world` / `gen_export_html`, MD's `--export doc.json`, Verse's
`VERSE_EXPORT_WORLD=<dir>` (or `VERSE_EXPORT_ONLY=1` for a whole library, no
window).

```bash
node scripts/add-world.mjs <world.json> <name> --from <asset dir>
```

That writes `worlds/<name>.json` and shrinks every mesh and audio file it
references into `worlds/assets/`. Those outputs belong in the assets checkout's
`web/worlds/`, not committed here — copy them across and commit them there.

## Preview and deploy

```bash
npm run assemble:all && python3 -m http.server 8000     # from this directory
./deploy.sh                                             # assembles, then wrangler
```

The viewer needs a server (it fetches JSON and ES modules); the landing page
also opens from disk. `deploy.sh` always assembles the full set, so a deploy
needs the assets checkout with its LFS objects pulled. It publishes as the
`localgpt-world` Worker; `.assetsignore` keeps the build tooling and config out
of the published site.

## License

Apache-2.0, as the rest of this repository.
