#!/usr/bin/env node
// Smoke-test the desktop app's phone listener with no phone and no one at
// the keyboard (crates/app/src/webui/remote.rs, docs/rfcs/authoring/
// mobile-remote.md).
//
// Launches `localgpt-app --world <world>` with the listener on
// (`LOCALGPT_APP_REMOTE=on`, on a port of its own) and its state in a
// throwaway folder, so the remembered devices and the last-opened world of
// whoever runs it are left alone. Then, as a paired device would:
//
//   - the page served and marked as a device's, nothing above it;
//   - pairing: a stranger's page refused, a wrong code refused, the code
//     working once, the key file the owner's alone and holding no key;
//   - the socket: an unpaired key refused, a paired one welcomed, nothing
//     sent before the page is ready, then the window's snapshots and tabs;
//   - a shell opened from the device, attached (its size, then its screen
//     from a reset), typed into, and titled — a spinner, then ✳;
//   - a second device attaching late and getting the screen; the size
//     following whichever device typed, and going back when it leaves;
//   - a frame of the window; the listener's own asks refused to a device.
//
// With --phone, the device's page also runs in headless Chromium at an
// iPhone's size (Playwright, from website-world's node_modules): a shell
// opened and typed into through the prompt box, each view screenshotted.
//
//     cargo build -p localgpt-app && (cd crates/app/ui && npm run build)
//     target/debug/localgpt world init demo.world
//     scripts/remote-smoke.mjs demo.world [--phone <dir for screenshots>]
//
// A locked screen draws black frames: the frame check sees one arrive, not
// what it shows. The listener binds every interface, so a Mac whose firewall
// is on asks once whether the app may accept connections; this runs over
// loopback either way.

import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
const world = args.find((a) => !a.startsWith("--") && args[args.indexOf(a) - 1] !== "--phone");
const phoneDir = args.includes("--phone") ? args[args.indexOf("--phone") + 1] : null;
if (!world) {
  console.error("usage: scripts/remote-smoke.mjs <world package> [--phone <dir>]");
  process.exit(2);
}
const PORT = Number(process.env.LOCALGPT_APP_REMOTE_PORT ?? 9890);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let failed = 0;
const check = (what, ok, detail = "") => {
  console.log(`${ok ? "ok  " : "FAIL"} ${what}${detail ? ` — ${detail}` : ""}`);
  if (!ok) failed++;
};

const state = mkdtempSync(join(tmpdir(), "lg-remote-smoke-"));
let log = "";
const app = spawn(join(ROOT, "target/debug/localgpt-app"), ["--world", resolve(world)], {
  env: { ...process.env, LOCALGPT_STATE_DIR: state, LOCALGPT_APP_REMOTE: "on", LOCALGPT_APP_REMOTE_PORT: String(PORT) },
  stdio: ["ignore", "pipe", "pipe"],
});
app.stdout.on("data", (d) => (log += d));
app.stderr.on("data", (d) => (log += d));
const stop = () => {
  app.kill();
  rmSync(state, { recursive: true, force: true });
};
process.on("exit", stop);

// The pairing link the app prints, then time for the window's page to load:
// a device that arrives first gets the window's snapshots as they come.
let link;
for (let i = 0; i < 240 && !link; i++) {
  link = log.match(/pair a phone at (\S+)/)?.[1];
  if (!link) await sleep(250);
}
if (!link) {
  console.log(`FAIL the app printed no pairing link:\n${log}`);
  process.exit(1);
}
const code = new URLSearchParams(new URL(link).hash.slice(1)).get("pair");
const origin = `http://127.0.0.1:${PORT}`;
await sleep(6000);

const html = await (await fetch(`${origin}/`)).text();
check("the page is served, marked as a device's", html.includes('name="lg-remote"'));
const script = html.match(/src="(\/assets\/[^"]+\.js)"/)?.[1];
const js = script ? await fetch(`${origin}${script}`) : null;
check("its script is served", js?.ok && js.headers.get("content-type") === "text/javascript", script ?? "no built page");
check("nothing above the page", (await fetch(`${origin}/../Cargo.toml`)).status === 404);

const pair = (body, headers = {}) =>
  fetch(`${origin}/pair`, { method: "POST", headers: { "content-type": "application/json", ...headers }, body: JSON.stringify(body) });
check("another site's page may not pair", (await pair({ code, name: "x" }, { origin: "https://example.com" })).status === 403);
check("a wrong code is refused", (await pair({ code: "0".repeat(32), name: "x" })).status === 403);
const paired = await pair({ code, name: "Smoke phone" });
const { key, device } = await paired.json();
check("the code pairs", paired.ok && key?.length === 64, `device ${device}`);
check("once", (await pair({ code, name: "again" })).status === 403);
const saved = join(state, "app-remote.json");
check("the app keeps the key's hash, not the key", !readFileSync(saved, "utf8").includes(key));
check("in a file the owner's alone", (statSync(saved).mode & 0o777) === 0o600, (statSync(saved).mode & 0o777).toString(8));

/** A device's socket: it says hello and keeps what comes back. */
function connect(key) {
  const ws = new WebSocket(`ws://127.0.0.1:${PORT}/remote`);
  ws.got = [];
  ws.onopen = () => ws.send(JSON.stringify({ t: "hello", key }));
  ws.onmessage = (e) => ws.got.push(JSON.parse(e.data));
  ws.onclose = () => (ws.closed = true);
  ws.say = (m) => ws.send(JSON.stringify(m));
  ws.wait = async (pred, ms = 8000) => {
    for (const end = Date.now() + ms; Date.now() < end; await sleep(50)) {
      const found = ws.got.find(pred);
      if (found) return found;
    }
    return null;
  };
  return ws;
}
const text = (ws, id) =>
  ws.got
    .filter((m) => m.t === "term.data" && m.id === id)
    .map((m) => Buffer.from(m.b64, "base64").toString())
    .join("");

const stranger = connect("f".repeat(64));
check("an unpaired key is refused", !!(await stranger.wait((m) => m.t === "remote.denied")));
const phone = connect(key);
check("a paired key is welcomed", (await phone.wait((m) => m.t === "remote.welcome"))?.name === "Smoke phone");
check("nothing comes before the page is ready", phone.got.length === 1);
phone.say({ t: "ready" });
await phone.wait((m) => m.t === "term.list");
const order = phone.got.map((m) => m.t);
check("ready: the window's snapshots, then the tabs", order.includes("mode") && order.indexOf("term.list") > order.indexOf("mode"), order.join(" "));

phone.say({ t: "term.open", id: "smoke", kind: "shell", cols: 60, rows: 20 });
check("a tab the device opens is listed for every page", !!(await phone.wait((m) => m.t === "term.list" && m.tabs.some((t) => t.id === "smoke"))));
phone.say({ t: "term.attach", id: "smoke", cols: 60, rows: 20 });
const size = await phone.wait((m) => m.t === "term.size" && m.id === "smoke");
check("attached: the size first", size?.cols === 60, JSON.stringify(size));
const screen = await phone.wait((m) => m.t === "term.data" && m.id === "smoke" && phone.got.indexOf(m) > phone.got.indexOf(size));
check("then the screen, from a reset", !!screen && Buffer.from(screen.b64, "base64").toString().startsWith("\x1bc"));
await sleep(1500); // the login shell's first prompt
phone.say({ t: "term.input", id: "smoke", data: "echo hello-$((40+2))\r" });
for (let i = 0; i < 100 && !text(phone, "smoke").includes("hello-42"); i++) await sleep(50);
check("typing runs in the tab, and its output comes back", text(phone, "smoke").includes("hello-42"));
phone.say({ t: "term.input", id: "smoke", data: "printf '\\033]0;\\342\\240\\202 Fix the jetty\\007'; sleep 0.5; printf '\\033]0;\\342\\234\\263 Fix the jetty\\007'\r" });
const working = await phone.wait((m) => m.t === "term.state" && m.id === "smoke" && m.activity === "working");
check("a spinner in the title: working", working?.label === "Fix the jetty", JSON.stringify(working));
check("✳ in the title: at rest", !!(await phone.wait((m) => m.t === "term.state" && m.id === "smoke" && m.activity === "idle")));

const late = connect(key);
await late.wait((m) => m.t === "remote.welcome");
late.say({ t: "ready" });
await late.wait((m) => m.t === "term.list");
late.say({ t: "term.attach", id: "smoke", cols: 40, rows: 12 });
const lateScreen = await late.wait((m) => m.t === "term.data" && m.id === "smoke");
check("a second device attaches late and gets the screen", !!lateScreen && Buffer.from(lateScreen.b64, "base64").toString().includes("hello-42"));
late.say({ t: "term.input", id: "smoke", data: "" });
const taken = await phone.wait((m) => m.t === "term.size" && m.id === "smoke" && m.cols === 40);
check("the device that typed last sets the size", !!taken);
late.close();
const back = await phone.wait((m) => m.t === "term.size" && m.id === "smoke" && m.cols !== 40 && phone.got.indexOf(m) > phone.got.indexOf(taken));
check("it goes, and the size goes back (the window's, when it adopted the tab)", !!back, JSON.stringify(back));

phone.say({ t: "frame" });
const frame = await phone.wait((m) => m.t === "frame", 10000);
check("a frame of the window", frame?.url?.startsWith("data:image/jpeg;base64,"), frame ? `${Math.round(frame.url.length / 1024)} KB` : "none");
phone.say({ t: "remote.forget", id: device });
await sleep(800);
check("a device may not forget devices", !phone.closed && readFileSync(saved, "utf8").includes(device));
phone.say({ t: "term.close", id: "smoke" });
check("a closed tab leaves the list", !!(await phone.wait((m) => m.t === "term.list" && !m.tabs.some((t) => t.id === "smoke"))));
phone.close();
stranger.close();

if (phoneDir) {
  const { chromium, devices } = await import(join(ROOT, "website-world/node_modules/playwright/index.mjs"));
  const browser = await chromium.launch();
  const context = await browser.newContext({ ...devices["iPhone 15"] });
  await context.addInitScript((key) => localStorage.setItem("lg.remote.key", key), key);
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(`${origin}/`);
  await page.getByText("Smoke phone").waitFor({ timeout: 10000 });
  await page.getByRole("button", { name: "Shell", exact: true }).click();
  await page.waitForTimeout(2500);
  await page.getByRole("button", { name: "Write a prompt" }).click();
  await page.getByPlaceholder("Write or dictate a prompt").fill("echo 'from the phone'");
  await page.getByRole("button", { name: "Send" }).click();
  await page.waitForTimeout(1500);
  await page.screenshot({ path: join(phoneDir, "phone-agents.png") });
  check("the phone's page types into its shell", (await page.locator(".phone-term .xterm-rows").innerText()).includes("from the phone"));
  await page.getByRole("button", { name: "World", exact: true }).click();
  await page.locator('img[alt="The world, as the window shows it"]').waitFor({ timeout: 10000 });
  await page.screenshot({ path: join(phoneDir, "phone-world.png") });
  await page.getByRole("button", { name: "History", exact: true }).click();
  await page.waitForTimeout(500);
  await page.screenshot({ path: join(phoneDir, "phone-history.png") });
  check("the phone's page shows each view, with no page errors", errors.length === 0, errors.join(" | "));
  await browser.close();
}

console.log(failed ? `${failed} failed` : "all passed");
process.exit(failed ? 1 : 0);
