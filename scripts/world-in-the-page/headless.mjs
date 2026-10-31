// The world-in-the-page measurement without a screen: the same page, built
// the same way, in a headless browser, with this script standing in for the
// app. `?preview=song` already stands in for the app's timeline and clock;
// this adds the three terminals' bytes, read from load.py and delivered
// through `window.__lg.recv` a tab at a time, 120 times a second, as the
// app's `evaluate_script` does. What it cannot stand in for is Bevy, which
// only the app on an unlocked screen can measure (run.sh).
//
// usage: node scripts/world-in-the-page/headless.mjs <songs-dir> <out-dir> <chromium|msedge|webkit> <mode:load>...
//        <songs-dir> holds one song's folder with its manifest.json, as an
//        in-app run (run.sh) leaves it under <out-dir>/env.
// env:   SECS (measured seconds, 30), PORT (8766), PLAYWRIGHT (the package's
//        path; website-world's by default), PYTHON (python3; python on
//        Windows), HEADED=1 (a window on the real display, not headless)
import { spawn } from "node:child_process";
import { existsSync, mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const here = (p) => fileURLToPath(new URL(p, import.meta.url));
const pw = require(process.env.PLAYWRIGHT ?? here("../../website-world/node_modules/playwright"));

const [songs, out, browserName, ...specs] = process.argv.slice(2);
const SECS = Number(process.env.SECS ?? 30);
const PORT = Number(process.env.PORT ?? 8766);
const PYTHON = process.env.PYTHON ?? (process.platform === "win32" ? "python" : "python3");
mkdirSync(out, { recursive: true });

const server = spawn(PYTHON, [here("serve.py"), here("../../crates/app/ui/dist"), songs, out, String(PORT)], { stdio: "inherit" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
await sleep(500);

// Hardware WebGL, not a software fallback (the report names the GPU).
// `chromium` is Playwright's full build, in its new headless mode; `msedge`
// the installed Microsoft Edge, the engine of WebView2 and so of a Windows
// host; `webkit` Playwright's WebKit, the macOS host's engine. HEADED=1
// draws in a window on the real display instead, so the frames keep its
// refresh rate (a 144 Hz monitor, a ProMotion screen); leave it in front.
const headless = process.env.HEADED !== "1";
const gpu = ["--ignore-gpu-blocklist", "--enable-gpu", ...(process.platform === "darwin" ? ["--use-angle=metal"] : [])];
const launch =
  browserName === "chromium" || browserName === "msedge" ? { headless, channel: browserName, args: gpu } : { headless };
const browser = await pw[browserName === "msedge" ? "chromium" : browserName].launch(launch);
const engine = `${browserName}${headless ? "" : "+headed"}`;

for (const spec of specs) {
  const [mode, load] = spec.split(":");
  const run = `${engine}-${mode}-${load}`;
  const report = `${out}/${run}-page.json`;
  const context = await browser.newContext({ viewport: { width: 1440, height: 860 }, deviceScaleFactor: 2 });
  const page = await context.newPage();
  page.on("console", (m) => m.type() === "error" && console.error(`${run}: ${m.text()}`));
  page.on("pageerror", (e) => console.error(`${run}: ${e.message}`));
  await page.goto(`http://127.0.0.1:${PORT}/?preview=song&spike=${mode}&load=${load}&run=${run}&secs=${SECS}&open=shell,shell,shell`);

  // The app's half: each tab's PTY, read on its own and sent once a frame,
  // to the tabs `?open=` made (their ids are the page's own).
  await page.waitForFunction(() => document.querySelectorAll("[data-term]").length >= 3);
  const ids = await page.evaluate(() => [...document.querySelectorAll("[data-term]")].map((e) => e.dataset.term).slice(0, 3));
  const tabs = load === "idle" ? [] : ids.map((id) => {
    const proc = spawn(PYTHON, ["-u", here("load.py"), load, String(SECS + 40)]);
    const tab = { id, proc, chunks: [] };
    proc.stdout.on("data", (d) => tab.chunks.push(d));
    return tab;
  });
  const pump = setInterval(() => {
    const messages = [];
    for (const tab of tabs) {
      if (tab.chunks.length) messages.push({ t: "term.data", id: tab.id, b64: Buffer.concat(tab.chunks.splice(0)).toString("base64") });
    }
    for (const m of messages) page.evaluate((m) => window.__lg?.recv(m), m).catch(() => {});
  }, 1000 / 120);

  const deadline = Date.now() + (SECS + 60) * 1000;
  while (!existsSync(report) && Date.now() < deadline) await sleep(500);
  clearInterval(pump);
  for (const tab of tabs) tab.proc.kill();
  await context.close();
  console.log(`${run}: ${existsSync(report) ? "measured" : "no report"}`);
  await sleep(1500);
}
await browser.close();
server.kill();
