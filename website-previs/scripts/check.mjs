// End-to-end check of the previs page: serve it, load it, drop the crate's
// own sample screenplay through the real file input, and require the board
// to render every shot's frame and the downloads to hold the packages — the
// same contract the page promises a visitor.
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('../', import.meta.url));

const MIME = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript',
  '.json': 'application/json', '.css': 'text/css', '.svg': 'image/svg+xml',
  '.wasm': 'application/wasm', '.fountain': 'text/plain',
};
const server = createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, 'http://localhost').pathname));
  const file = join(root, path === '/' ? 'index.html' : path);
  if (!file.startsWith(root)) { res.writeHead(403); res.end(); return; }
  try {
    const body = await readFile(file);
    res.writeHead(200, { 'content-type': MIME[extname(file)] || 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404); res.end();
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const base = `http://127.0.0.1:${server.address().port}`;

const errors = [];
const browser = await chromium.launch({
  args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
});
const page = await browser.newPage({ viewport: { width: 1200, height: 800 } });
page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
page.on('console', (m) => { if (m.type() === 'error') errors.push(`console: ${m.text()}`); });

let failed = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? `: ${detail}` : ''}`);
  if (!ok) failed += 1;
};

await page.goto(base, { waitUntil: 'load' });
await page.waitForFunction(() => /Ready|Could not/.test(document.getElementById('status').textContent), null, { timeout: 15000 });
const ready = await page.textContent('#status');
check('page boots', /Ready/.test(ready), ready.trim());

// Drop the crate's own sample through the real input.
await page.setInputFiles('#file', join(root, 'samples/cold-eggs.fountain'));
try {
  await page.waitForFunction(() => window.localgptPrevis?.draft, null, { timeout: 20000 });
} catch {
  check('the sample drafts', false, (await page.textContent('#status')).trim());
}
const drafted = await page.evaluate(() => {
  const { draft, entries } = window.localgptPrevis ?? {};
  return draft && {
    title: draft.title,
    scenes: draft.scenes.length,
    shots: draft.scenes.reduce((n, s) => n + s.shots.length, 0),
    entries,
    tracked: draft.scenes.every((s) => s.shots.slice(1).every((row) => row.distance_m > 0)),
  };
});
check('the sample drafts: 2 scenes, 7 shots', drafted?.scenes === 2 && drafted?.shots === 7, JSON.stringify(drafted && { title: drafted.title, scenes: drafted.scenes, shots: drafted.shots }));
check('the zip holds the board, the shot list and both packages (10 files)', drafted?.entries?.length === 10, drafted?.entries?.join(' '));
check('every package is four files', ['manifest.json', 'ops.jsonl', 'snapshots/base.json', 'package.json']
  .every((f) => drafted?.entries?.includes(`scene-1.world/${f}`) && drafted?.entries?.includes(`scene-2.world/${f}`)));

// The board in the iframe rendered a frame per shot (the viewer runs
// inside the srcdoc page, WebGL through SwiftShader).
const frame = page.frames().find((f) => f !== page.mainFrame());
let frames = 0;
if (frame) {
  try {
    await frame.waitForFunction(() => {
      const imgs = [...document.querySelectorAll('img')];
      return imgs.length > 0 && imgs.every((i) => i.src.startsWith('data:image/png'));
    }, null, { timeout: 30000 });
  } catch { /* counted below */ }
  frames = await frame.evaluate(() => [...document.querySelectorAll('img')].filter((i) => i.src.startsWith('data:image/png')).length);
}
check('the board renders every shot\'s frame', frames === 7, `${frames} frames`);

// The downloads are real blobs.
const downloads = await page.evaluate(() => ['dl-board', 'dl-csv', 'dl-zip'].map((id) => document.getElementById(id).href.startsWith('blob:')));
check('the three downloads are ready', downloads.every(Boolean), downloads.join(','));

check('no page errors', errors.length === 0, errors.join(' | '));

await browser.close();
server.close();
if (failed > 0) {
  console.error(`${failed} check(s) failed`);
  process.exit(1);
}
console.log('previs page verified');
