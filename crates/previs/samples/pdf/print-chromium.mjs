import { createRequire } from 'node:module';
import { readFile } from 'node:fs/promises';
// Playwright and its Chromium come from the page's own node_modules
// (`npm ci` in website-md); run this from this directory.
const require = createRequire(new URL('../../../../website-md/package.json', import.meta.url));
const { chromium } = require('playwright');
const browser = await chromium.launch();
const page = await browser.newPage();
for (const name of ['cold-eggs', 'gull-point']) {
  let html = await readFile(`${name}.html`, 'utf8');
  html = html.replace('</head>', `<style>
    @page { size: Letter; margin: 1in 1in 1in 1.5in; }
    body { font: 12pt "Courier New", Courier, monospace; }
  </style></head>`);
  await page.setContent(html, { waitUntil: 'load' });
  await page.pdf({ path: `${name}-chromium.pdf`, format: 'Letter', printBackground: false, preferCSSPageSize: true });
  console.log(name, 'printed');
}
await browser.close();
