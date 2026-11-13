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
| `viewer/world-viewer.js`, `viewer/cinematography.js` | `crates/world-export/js/` (themselves vendored from the `openworldformat` npm package by `cargo xtask sync-viewer`; the viewer imports the second as `./cinematography.js`) | always |
| `worlds/*.json` — the conformance scenes, lone manifests as the spec ships them | `crates/world-types/conformance/` | always |
| `worlds/assets/textures/` | `crates/world-types/conformance/assets/` | always |
| `worlds/<name>.world/assets/` — each gallery package's meshes and music, by content hash | the assets checkout's `web/worlds/<name>.world/assets/` | `--all` |
| `worlds/posters/*.png` | the assets checkout's `web/` | `--all` |
| `worlds/<name>.world.zip` — a package's transport form, what its page offers to download | written here, once a package's assets are all present | always |

The GLBs and music are **web-optimized derivatives**, not the pack itself:
`cargo xtask add-world` shrinks each model with `@gltf-transform` (512 px textures,
weld, simplify, prune) and stores it by its hash. They live in
`localgpt-world-assets` as LFS objects rather than here, because 30 MB of glTF
in a code repository's history grows with every published world and never
shrinks. `$LOCALGPT_WORLD_ASSETS` points at
that checkout; a sibling `localgpt-world-assets` (or its former name
`localgpt-verse-assets`) is found automatically.

## Tracked here

| Path | What it is |
|---|---|
| `index.html` | The landing page: a gallery of worlds you can enter |
| `worlds.js` | The gallery's worlds, in order — title, one-liner, which app made it |
| `w/index.html` | One page per world: poster, what the package holds, its download, the commands that open it on your machine, and the live viewer |
| `world.html` | The viewer page: `world.html?src=worlds/shapes.json` opens a lone manifest, `?src=worlds/gen-basalt-lighthouse.world` a package |
| `worlds/<name>.world/` | The gallery's worlds, as Open World Format packages: `manifest.json` in the format's canonical text and `package.json` naming its bytes. The assets are assembled in |
| `vendor/three/` | three.js (MIT), self-hosted so the site makes no third-party requests |
| `scripts/` | `check-worlds.mjs` (the render check, which also captures posters); `posters.mjs` — both drive Chromium, so they stay JavaScript until the web renderer is Rust. Assembling and adding a world are `cargo xtask assemble world` and `cargo xtask add-world` |

## Checks

```bash
npm ci && npx playwright install --with-deps chromium
npm run check        # in-repo sources only — what CI runs
npm run check:all    # plus the curated worlds' binaries
```

`npm run check` renders every world whose assets are present and **skips** the
rest, naming them: the two Verse packages need the derivatives, while the
conformance scenes and the primitives-only Gen packages need nothing
external. That is what lets CI run this with no assets checkout. A package
whose assets are present must have its zip, which the assemble step writes.
`check:all` renders all 15 and rewrites the posters.

## Adding a world

A gallery world is a `.world` package (`spec/package.md` in the
[openworldformat](https://github.com/openworldformat/openworldformat)
repository): `manifest.json` is the world now, in the format's canonical
text; `package.json` names its bytes; `assets/` holds every file the manifest
references by content hash, `<sha256>.<ext>` — the format's baseline for a
published world, and what `localgpt world verify` checks. Every LocalGPT app
writes a manifest (Gen's `gen_save_world` / `gen_export_html`, Document's
`--export doc.json`, Verse's `VERSE_EXPORT_WORLD=<dir>`), and the desktop
app's live canvas writes whole packages.

```bash
cargo xtask add-world <world.json|dir.world> <name> --from <asset dir>… [--keep]
```

That writes `worlds/<name>.world/manifest.json` and `package.json`, which are
tracked here, and the content-addressed copies into `worlds/<name>.world/assets/`,
each glTF model shrunk for the web on the way (`--keep` stores files as they
are). The copies belong in the assets checkout under
`web/worlds/<name>.world/assets/`, not here: copy the folder across and commit
it there. Then list the package in `worlds.js`.

## Preview and deploy

```bash
npm run assemble:all && python3 -m http.server 8000     # from this directory
cargo xtask deploy world                                # assembles, then wrangler
```

The viewer needs a server (it fetches JSON and ES modules); the landing page
also opens from disk. `cargo xtask deploy world` always assembles the full set, so a deploy
needs the assets checkout with its LFS objects pulled. It publishes as the
`localgpt-world` Worker; `.assetsignore` keeps the build tooling and config out
of the published site.

## License

Apache-2.0, as the rest of this repository.
