// The world-in-the-page measurement (docs/rfcs/authoring/the-world-in-the-page.md):
// the page measures its own frames while the app plays the song and three
// agents' terminals stream. `?spike=web` draws the world here with the
// format's viewer (Viewport.tsx); `?spike=native` leaves the hole to Bevy.
// The report goes to the measurement's server
// (scripts/world-in-the-page/serve.py), which writes it beside Bevy's own
// frame log. Without `?spike=` nothing here runs.
import { listen, send } from "./bridge";

const q = new URLSearchParams(location.search);
export const spikeMode = q.get("spike") as "web" | "native" | null;
const load = q.get("load") ?? "idle";
const run = q.get("run") ?? `${spikeMode}-${load}`;
const warmup = Number(q.get("warmup") ?? 10);
const secs = Number(q.get("secs") ?? 30);
const loadPy = q.get("loadpy") ?? "";

/** The tabs `?open=` opened, by their ids, which are the page's own (and
 *  random, so a phone's tabs never collide with them). */
const tabIds = () => [...document.querySelectorAll<HTMLElement>("[data-term]")].map((e) => e.dataset.term!).slice(0, 3);

/** What the web world exposes for the report. */
export const spikeWorld: { viewer?: any; renderer?: any; entities?: number } = {};

const sleep = (s: number) => new Promise((r) => setTimeout(r, s * 1000));

/** A line in the spike's page.log, for seeing where a run got to. */
function note(what: string) {
  void fetch("/__spike/log", { method: "POST", body: `${new Date().toISOString()} ${run} ${what}` }).catch(() => {});
}

/** Which GPU the page's WebGL runs on: a software fallback would void a run. */
function glRenderer(): string | undefined {
  const gl = spikeWorld.renderer?.getContext?.() ?? document.createElement("canvas").getContext("webgl2");
  const ext = gl?.getExtension("WEBGL_debug_renderer_info");
  return gl ? String(gl.getParameter(ext ? ext.UNMASKED_RENDERER_WEBGL : gl.RENDERER)) : undefined;
}

export function startSpike() {
  if (!spikeMode) return;
  note(`loaded; in app: ${"ipc" in window}; visibility ${document.visibilityState}`);
  window.addEventListener("error", (e) => note(`error: ${e.message} at ${e.filename}:${e.lineno}`));
  window.addEventListener("unhandledrejection", (e) => note(`rejection: ${String(e.reason)}`));
  let termBytes = 0;
  let clocks = 0;
  let timeline = false;
  const hidden: number[] = [];
  listen((m: any) => {
    if (m.t === "term.data") termBytes += (m.b64.length * 3) / 4;
    if (m.t === "clock") clocks++;
    if (m.t === "timeline" && m.view && !timeline) {
      timeline = true;
      // Silent from the first view on: the clock still runs, unheard.
      send({ t: "mute", ids: ["soundtrack"] } as any);
    }
  });
  document.addEventListener("visibilitychange", () => hidden.push(Date.now(), document.hidden ? 1 : 0));

  void (async () => {
    await sleep(warmup);
    const tabs = tabIds();
    note(`warm; timeline ${timeline}; world ${spikeWorld.entities ?? "-"}; tabs ${tabs.join(" ")}; visibility ${document.visibilityState}`);
    send({ t: "mute", ids: ["soundtrack"] } as any);
    send({ t: "clock.play", playing: true } as any);
    if (load !== "idle")
      for (const id of tabs) send({ t: "term.input", id, data: `exec python3 -u '${loadPy}' ${load} ${secs + 30}\r` } as any);
    await sleep(4);

    note("measuring");
    const bytes0 = termBytes;
    const clocks0 = clocks;
    const deltas: number[] = [];
    const start = Date.now();
    await new Promise<void>((done) => {
      let last = 0;
      const until = performance.now() + secs * 1000;
      const step = (now: number) => {
        if (last) deltas.push(+(now - last).toFixed(3));
        last = now;
        if (now < until) requestAnimationFrame(step);
        else done();
      };
      requestAnimationFrame(step);
    });
    const stop = Date.now();
    note(`measured ${deltas.length} frames`);
    const window_s = (stop - start) / 1000;

    const render = spikeWorld.renderer?.info?.render;
    const vp = document.querySelector("[data-spike-viewport]")?.getBoundingClientRect();
    // One frame of the page's world, to look at: read back in the same task
    // that drew it, before the drawing buffer is cleared.
    let snapshot: string | undefined;
    // What one frame of the world costs the page's thread: the viewer's own
    // tick (behaviors, modulations, render calls), timed back to back, then
    // again with a one-pixel read so the GPU's work is waited for too.
    let tickMs: number | undefined;
    let tickSyncMs: number | undefined;
    if (spikeWorld.viewer) {
      const gl = spikeWorld.renderer.getContext();
      const pixel = new Uint8Array(4);
      const bench = (sync: boolean) => {
        const n = 60;
        const t0 = performance.now();
        for (let i = 0; i < n; i++) {
          spikeWorld.viewer.tick(1 / 60);
          if (sync) gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
        }
        return (performance.now() - t0) / n;
      };
      tickMs = bench(false);
      tickSyncMs = bench(true);
      spikeWorld.viewer.tick(0);
      snapshot = spikeWorld.renderer.domElement.toDataURL("image/jpeg", 0.85);
    }
    const report: Record<string, unknown> = {
      tickMs,
      tickSyncMs,
      run,
      mode: spikeMode,
      load,
      start,
      stop,
      deltas,
      termKBps: (termBytes - bytes0) / 1024 / window_s,
      clockHz: (clocks - clocks0) / window_s,
      timeline,
      hidden,
      visibility: document.visibilityState,
      dpr: devicePixelRatio,
      viewport: vp && { w: Math.round(vp.width), h: Math.round(vp.height) },
      gl: glRenderer(),
      calls: render?.calls,
      triangles: render?.triangles,
      entities: spikeWorld.entities,
      ua: navigator.userAgent,
      snapshot,
    };
    send({ t: "clock.play", playing: false } as any);
    await fetch("/__spike/report", { method: "POST", body: JSON.stringify(report) });
  })();
}
