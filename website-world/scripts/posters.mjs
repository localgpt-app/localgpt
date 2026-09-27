// A world's poster: one frame of the viewer's canvas. Run by
// check-worlds.mjs per world; also usable directly:
//   node scripts/posters.mjs [world.json...]
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { basename, extname, join, normalize } from 'node:path';
import { chromium } from 'playwright';

const MIME = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript',
  '.json': 'application/json', '.css': 'text/css', '.svg': 'image/svg+xml',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.ico': 'image/x-icon',
  '.glb': 'model/gltf-binary', '.mp3': 'audio/mpeg', '.ogg': 'audio/ogg',
  '.wav': 'audio/wav', '.flac': 'audio/flac', '.woff2': 'font/woff2',
};

// Serve `root` as a static site; returns { base, close }.
export async function serve(root) {
  const { createServer } = await import('node:http');
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
  return {
    base: `http://127.0.0.1:${server.address().port}`,
    close: () => server.close(),
  };
}

// Capture one frame of each world's viewer canvas as worlds/posters/<name>.png
// (1280×720, the Open Graph 1.91:1 crop). The canvas needs its drawing buffer
// kept (world.html sets preserveDrawingBuffer) or it reads back transparent.
export async function posters(root, worlds) {
  const { base, close } = await serve(root);
  const outDir = join(root, 'worlds', 'posters');
  await mkdir(outDir, { recursive: true });
  const browser = await chromium.launch({
    args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
  });
  const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
  for (const world of worlds) {
    const name = basename(world, '.json');
    await page.goto(`${base}/world.html?src=worlds/${world}`);
    await page.waitForFunction(
      () => window.localgptViewer && window.localgptViewer.sceneInfo().entityCount > 0,
      null, { timeout: 30000 },
    );
    // Let a camera framing, fog and lights settle, then read the canvas.
    await page.waitForTimeout(1500);
    const png = await page.evaluate(() => {
      const canvas = document.querySelector('#scene canvas');
      return canvas ? canvas.toDataURL('image/png') : null;
    });
    if (!png) throw new Error(`${world}: no canvas`);
    await writeFile(join(outDir, `${name}.png`), Buffer.from(png.split(',')[1], 'base64'));
    console.log(`poster worlds/posters/${name}.png`);
  }
  await browser.close();
  close();
}

// Run directly: node scripts/posters.mjs [world.json...] (default: all).
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const { readdir } = await import('node:fs/promises');
  const { fileURLToPath } = await import('node:url');
  const root = fileURLToPath(new URL('../', import.meta.url));
  const given = process.argv.slice(2);
  const worlds = given.length
    ? given.map((f) => f.replace(/^.*worlds\//, '').replace(/\.json$/, '') + '.json')
    : (await readdir(join(root, 'worlds'))).filter((f) => f.endsWith('.json')).sort();
  await posters(root, worlds);
}
