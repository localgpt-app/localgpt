// End-to-end check of the localgpt.md page: serve it, load it, drop the app's
// own sample through the real file input, and require the viewer to render a
// world with the right shape — the same contract the page promises a visitor.
import { createServer } from 'node:http';
import { readFile, readdir } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('../', import.meta.url));

const MIME = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript',
  '.json': 'application/json', '.css': 'text/css', '.svg': 'image/svg+xml',
  '.wasm': 'application/wasm', '.md': 'text/markdown',
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
const page = await browser.newPage({ viewport: { width: 1080, height: 720 } });
page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
page.on('console', (m) => { if (m.type() === 'error') errors.push(`console: ${m.text()}`); });

let failed = 0;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${detail ? `: ${detail}` : ''}`);
  if (!ok) failed += 1;
};

await page.goto(base, { waitUntil: 'load' });
await page.waitForFunction(() => window.localgptViewer !== undefined || document.getElementById('status').textContent.includes('Ready'), null, { timeout: 8000 });

// The compiler loaded.
const ready = await page.textContent('#status');
check('page boots', /Ready|Could not/.test(ready) && !/Could not/.test(ready), ready.trim());

// Drop the app's own sample through the real input.
const sample = join(root, 'samples/hello.md');
await page.setInputFiles('#file', sample);
try {
  await page.waitForFunction(
    () => window.localgptViewer && window.localgptViewer.sceneInfo().entityCount > 0,
    null, { timeout: 20000 },
  );
  const info = await page.evaluate(() => window.localgptViewer.sceneInfo());
  const status = (await page.textContent('#status')).trim();
  check('world renders from a dropped file', true, status);
  check('sections became stops', /4 sections/.test(status), status);
  check('canvas exists', await page.evaluate(() => Boolean(document.querySelector('#scene canvas'))));
  check('the draft is labelled', !(await page.evaluate(() => document.getElementById('draftnote').hidden)));
} catch (e) {
  check('world renders from a dropped file', false, String(e).slice(0, 200));
}

check('no page errors', errors.length === 0, errors.slice(0, 3).join(' | '));

await browser.close();
server.close();
if (failed) { console.log(`${failed} check(s) failed`); process.exit(1); }
console.log('localgpt.md page verified');
await readdir(root); // keep the import used on all paths
