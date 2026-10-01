# CLAUDE.md

Guidance for Claude Code when working in this directory.

## What this is

The landing page and world viewer for localgpt.world: hand-written static
HTML/CSS/JS, no build step, deployed as a Cloudflare Worker with static assets
(`./deploy.sh`, manual). It was its own repository until it folded into this
workspace; `README.md` explains the layout, and `../docs/world-strategy.md` is
the plan for the site.

## Rules

- **This directory is assembled.** `npm run assemble` copies the viewer in from
  `crates/world-export/js/world-viewer.js` and the conformance scenes from
  `crates/world-types/conformance/`. Those copies are gitignored. Change them
  **upstream in the crate** (which vendors the renderer from the
  `openworldformat` npm package via `scripts/sync-viewer.sh`) and re-assemble;
  never edit them here. The whole
  reason this folded in is that the same copies used to cross a repository
  boundary and drifted 185 lines without failing anything.
- **The curated worlds' GLBs, music and posters are not tracked here.** They are
  web-optimized derivatives (`add-world.mjs` shrinks them with
  `@gltf-transform`), and they live in `localgpt-world-assets` under `web/` as
  LFS objects. `npm run assemble:all` brings them in.
- Everything in this directory is deployed **except** the files listed in
  `.assetsignore` — which must keep covering `scripts/`, `package.json`,
  `wrangler.toml` and `deploy.sh`, or build tooling ships as site content.
- No third-party requests from the site: vendor what it needs (three.js is
  under `vendor/three/`, MIT).
- To check a page, assemble and serve it (`npm run assemble:all &&
  python3 -m http.server 8000`); the viewer fetches JSON and ES modules, so it
  needs a server.
- `npm run check` is the CI gate, and it is the only automated check on the
  canonical web renderer. It skips worlds whose assets are absent rather than
  failing them, so a world can go unchecked in CI — `deploy.sh` runs the full
  set, which is where the Verse worlds are verified.
- Commits: conventional commits, no Co-Authored-By or Claude-Session trailers.
- This site is public. Never name the closed-source sibling 3D platform; use
  generic terms such as "connected 3D app".
