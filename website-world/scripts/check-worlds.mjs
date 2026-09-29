// Headless render check for every world in worlds/: serve this directory,
// open world.html?src=<world> in Chromium with software WebGL, and fail on
// any page or console error, a missing canvas, or a viewer that reports no
// entities. Worlds with tours also get tour 0 started. After the check passes,
// a poster frame of every world is captured to worlds/posters/
// (scripts/posters.mjs) — the landing page's cards and og:image read those.
//
//   npm ci && npx playwright install --with-deps chromium && npm run check
import { createServer } from 'node:http';
import { readFile, readdir } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { posters } from './posters.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const types = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript',
  '.json': 'application/json', '.css': 'text/css', '.svg': 'image/svg+xml',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.ico': 'image/x-icon',
};

// The viewer fetches JSON and ES modules, so it needs a server, not file://.
const server = createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, 'http://localhost').pathname));
  const file = join(root, path === '/' ? 'index.html' : path);
  if (!file.startsWith(root)) { res.writeHead(403); res.end(); return; }
  try {
    const body = await readFile(file);
    res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404); res.end();
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;

const all = (await readdir(join(root, 'worlds'))).filter((f) => f.endsWith('.json')).sort();

// A world whose meshes or audio are not on disk is skipped, not failed: the
// curated Verse worlds reference the shrunk GLBs and music that live in the
// assets checkout, so `assemble.mjs` without `--all` leaves them absent. Every
// other world — the conformance scenes and the primitives-only gen/md ones —
// needs nothing external, which is what lets CI run this with no checkout.
// A referenced file that IS present but broken still renders and still fails.
const skipped = [];
const worlds = [];
for (const world of all) {
  const manifest = await readFile(join(root, 'worlds', world), 'utf8');
  const refs = [...manifest.matchAll(/"([^"]*\.(?:glb|gltf|mp3|ogg|wav|flac))"/g)].map((m) => m[1]);
  const missing = refs.filter((ref) => !existsSync(join(root, 'worlds/assets', ref)));
  if (missing.length) skipped.push(`${world} (${missing.length} asset(s) absent)`);
  else worlds.push(world);
}
for (const world of skipped) console.log(`skip ${world}`);
const browser = await chromium.launch({
  args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
});
const page = await browser.newPage({ viewport: { width: 960, height: 600 } });
const errors = [];
page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
page.on('console', (m) => { if (m.type() === 'error') errors.push(`console: ${m.text()}`); });

let failed = 0;
for (const world of worlds) {
  errors.length = 0;
  await page.goto(`${base}/world.html?src=worlds/${world}`, { waitUntil: 'load' });
  let info = null;
  try {
    await page.waitForFunction(
      () => window.localgptViewer && window.localgptViewer.sceneInfo().entityCount > 0,
      null, { timeout: 15000 },
    );
    info = await page.evaluate(() => window.localgptViewer.sceneInfo());
  } catch {
    // Reported through `info` below.
  }
  const canvas = await page.evaluate(() => Boolean(document.querySelector('#scene canvas')));
  // A world with a proximity trigger fires it: move the visitor (the
  // camera) into range and the lamp's once-trigger hides it — event,
  // runtime, action, scene graph, end to end.
  let triggerOk = true;
  if (info && info.triggerCount > 0 && world === 'triggers.json') {
    triggerOk = await page.evaluate(async () => {
      const viewer = window.localgptViewer;
      const lamp = viewer.entities.get('lamp');
      if (!lamp || lamp.object.visible !== true) return false;
      viewer.camera.position.set(0.0, 1.5, 2.0);
      await new Promise((resolve) => setTimeout(resolve, 400));
      return lamp.object.visible === false;
    });
  }
  let tourOk = true;
  if (info && info.tourCount > 0) {
    const caption = await page.evaluate(() => {
      window.localgptViewer.startTour(0);
      return document.getElementById('caption').textContent;
    });
    tourOk = caption.trim().length > 0;
  }
  const ok = Boolean(info) && canvas && tourOk && triggerOk && errors.length === 0;
  if (!ok) failed += 1;
  console.log(
    `${ok ? 'ok  ' : 'FAIL'} ${world}: entities=${info?.entityCount ?? 0}`
    + ` tours=${info?.tourCount ?? 0} canvas=${canvas} tour=${tourOk}`
    + ` triggers=${info?.triggerCount ?? 0}${triggerOk ? '' : '!'} errors=${errors.length}`,
  );
  for (const error of errors) console.log(`      ${error.slice(0, 300)}`);
}

await browser.close();
server.close();
if (failed) {
  console.log(`${failed} of ${worlds.length} worlds failed`);
  process.exit(1);
}
console.log(`${worlds.length} worlds rendered` + (skipped.length ? `, ${skipped.length} skipped for absent assets` : ''));
await posters(root, worlds);
