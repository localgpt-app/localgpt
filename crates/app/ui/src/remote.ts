// A paired device's line to the app. The app serves this page on the local
// network and marks it (`crates/app/src/webui/remote.rs`); its messages then
// go over a WebSocket rather than window.ipc, and come back through the
// same window.__lg.recv. Pairing trades the code in the address's fragment
// — the QR code the computer shows — once, for a key this browser keeps.

import { setStandIn } from "./bridge";

/** Served to a device by the app, rather than opened in the window. */
export const isRemote = () => document.querySelector('meta[name="lg-remote"]') !== null;

/** Where the line is: trading a code, connecting, open, refused (not
 *  paired, or forgotten), or lost and trying again. */
export type Line = {
  state: "pairing" | "connecting" | "live" | "unpaired" | "offline";
  why: string | null;
  /** What the computer calls this device. */
  device: string | null;
};

const KEY = "lg.remote.key";

let line: Line = { state: "connecting", why: null, device: null };
const watchers = new Set<(line: Line) => void>();
let socket: WebSocket | null = null;
let retry = 0;
let timer: number | undefined;
/** The line was open once: a later try is a reconnection. */
let wasLive = false;
/** The key, when this browser cannot keep it (a private window). */
let memory: string | null = null;

function set(next: Partial<Line>) {
  line = { ...line, ...next };
  watchers.forEach((watch) => watch(line));
}

/** Told now, and whenever the line changes. A page re-asks what it shows
 *  each time the line comes back: the app sends to a page that is ready. */
export function watchLine(watch: (line: Line) => void): () => void {
  watchers.add(watch);
  watch(line);
  return () => {
    watchers.delete(watch);
  };
}

function stored(): string | null {
  try {
    return localStorage.getItem(KEY) ?? memory;
  } catch {
    return memory;
  }
}

function store(key: string | null) {
  memory = key;
  try {
    if (key) localStorage.setItem(KEY, key);
    else localStorage.removeItem(KEY);
  } catch {
    // A private window keeps nothing: this visit is paired, the next is not.
  }
}

/** What the computer's list calls this device. */
function deviceName(): string {
  const ua = navigator.userAgent;
  if (/iPad/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return "iPad";
  if (/iPhone/.test(ua)) return "iPhone";
  if (/Android/.test(ua)) return /Mobile/.test(ua) ? "Android phone" : "Android tablet";
  if (/Macintosh/.test(ua)) return "Mac";
  if (/Windows/.test(ua)) return "Windows PC";
  return "Browser";
}

async function pair(code: string) {
  set({ state: "pairing", why: null });
  try {
    const reply = await fetch("/pair", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ code, name: deviceName() }),
    });
    const body = await reply.json().catch(() => ({}));
    if (!reply.ok) {
      set({ state: "unpaired", why: body.error ?? "the computer refused the code" });
      return;
    }
    store(body.key);
  } catch {
    set({ state: "offline", why: "the computer didn't answer" });
  }
}

function connect() {
  clearTimeout(timer);
  const key = stored();
  if (!key) {
    set({ state: "unpaired" });
    return;
  }
  set({ state: wasLive ? "offline" : "connecting" });
  const ws = new WebSocket(`${location.protocol === "https:" ? "wss" : "ws"}://${location.host}/remote`);
  socket = ws;
  ws.onopen = () => ws.send(JSON.stringify({ t: "hello", key }));
  ws.onmessage = (event) => {
    const message = JSON.parse(event.data);
    if (message.t === "remote.welcome") {
      retry = 0;
      wasLive = true;
      set({ state: "live", why: null, device: message.name });
    } else if (message.t === "remote.denied") {
      store(null);
      set({ state: "unpaired", why: message.why });
    } else {
      window.__lg?.recv(message);
    }
  };
  ws.onclose = () => {
    if (socket !== ws) return;
    socket = null;
    if (line.state === "unpaired") return;
    set({ state: "offline" });
    // Again soon, then less often: the computer may be asleep.
    retry = Math.min(retry + 1, 6);
    timer = window.setTimeout(connect, 500 * 2 ** (retry - 1));
  };
}

/** Opens the line: the code in the address first, when there is one. What
 *  the page sends while the line is down is dropped, not kept for later —
 *  keys typed into a terminal minutes late would do harm — and a page
 *  re-asks once it is back. */
export async function start() {
  setStandIn((message) => {
    if (socket?.readyState === WebSocket.OPEN && line.state === "live") socket.send(JSON.stringify(message));
  });
  // A phone wakes with its sockets dead: try at once rather than waiting.
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible" && line.state === "offline" && socket === null) {
      retry = 0;
      connect();
    }
  });
  const code = new URLSearchParams(location.hash.slice(1)).get("pair");
  if (code) {
    history.replaceState(null, "", location.pathname + location.search);
    await pair(code);
    // A stale code on a device paired before: its own key still opens.
    if (line.state === "unpaired" && !stored()) return;
  }
  connect();
}

/** Forgets this device's key here; pairing again needs a new code. */
export function unpair() {
  store(null);
  set({ state: "unpaired", why: "forgotten on this device: scan a new code to pair again" });
  socket?.close();
  socket = null;
}
