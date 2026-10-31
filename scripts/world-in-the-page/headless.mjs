// The world-in-the-page measurement without a screen: the same page, built
// the same way, in a headless browser, with this script standing in for the
// app. `?preview=song` already stands in for the app's timeline and clock;
// this adds the three terminals' bytes, read from load.py and delivered
// through `window.__lg.recv` a tab at a time, 120 times a second, as the
// app's `evaluate_script` does. What it cannot stand in for is Bevy, which
// only the app on an unlocked screen can measure (run.sh).
//
// usage: node scripts/world-in-the-page/headless.mjs <songs-dir> <out-dir> <chromium|webkit> <mode:load>...
//        <songs-dir> holds one song's folder with its manifest.json, as an
//        in-app run (run.sh) leaves it under <out-dir>/env.
// env:   SECS (measured seconds, 30), PORT (8766), PLAYWRIGHT (the package's
//        path; website-world's by default)
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
mkdirSync(out, { recursive: true });

const server = spawn("python3", [here("serve.py"), here("../../crates/app/ui/dist"), songs, out, String(PORT)], { stdio: "inherit" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
await sleep(500);

// Hardware WebGL, not a software fallback: Chromium's full build, new headless.
const launch = browserName === "chromium"
  ? { headless: true, channel: "chromium", args: ["--use-angle=metal", "--ignore-gpu-blocklist", "--enable-gpu"] }
  : { headless: true };
const browser = await pw[browserName].launch(launch);

for (const spec of specs) {
  const [mode, load] = spec.split(":");
  const run = `${browserName}-${mode}-${load}`;
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
    const proc = spawn("python3", ["-u", here("load.py"), load, String(SECS + 40)]);
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
