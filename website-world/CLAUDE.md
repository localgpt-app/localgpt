# CLAUDE.md

Guidance for Claude Code when working in this directory.

## What this is

The landing page and world viewer for localgpt.world: hand-written static
HTML/CSS/JS, no build step, deployed as a Cloudflare Worker with static assets
(`cargo xtask deploy world`, manual). `README.md` explains the layout, and
`../docs/world-strategy.md` says what the site is for. The workspace's rules
(commits, never naming a closed-source sibling project) are in the root
`CLAUDE.md`.

## Rules

- **This directory is assembled.** `npm run assemble` copies the viewer in from
  `crates/world-export/js/world-viewer.js` and the conformance scenes from
  `crates/world-types/conformance/`. Those copies are gitignored. Change them
  **upstream in the crate** (which vendors the renderer from the
  `openworldformat` npm package via `cargo xtask sync-viewer`) and re-assemble;
  never edit them here.
- **The gallery's worlds are `.world` packages**, `worlds/<name>.world/`: the
  manifest (canonical text) and `package.json` are tracked, the assets are
  not — they are web-optimized derivatives stored by content hash
  (`cargo xtask add-world` shrinks them with `@gltf-transform` and names them),
  and they live in `localgpt-world-assets` under `web/worlds/<name>.world/assets/`
  as LFS objects, with the posters beside them. `npm run assemble:all` brings
  them in, and the assemble step zips every complete package — the format's
  transport form, what the per-world page offers to download. The conformance
  scenes stay lone manifests, as the spec ships them.
- Everything in this directory is deployed **except** the files listed in
  `.assetsignore` — which must keep covering `scripts/`, `package.json`,
  `wrangler.toml`, or build tooling ships as site content.
- No third-party requests from the site: vendor what it needs (three.js is
  under `vendor/three/`, MIT).
- To check a page, assemble and serve it (`npm run assemble:all &&
  python3 -m http.server 8000`); the viewer fetches JSON and ES modules, so it
  needs a server.
- `npm run check` is the CI gate, and it is the only automated check on the
  canonical web renderer. It skips worlds whose assets are absent rather than
  failing them, so a world can go unchecked in CI — `cargo xtask deploy world` runs the full
  set, which is where the Verse worlds are verified.
