import { useEffect, useRef } from "react";
import { inApp, send } from "./bridge";
import { spikeMode, spikeWorld } from "./spike";

/** The hole the world shows through. The app fits its camera to this rect
 *  and lets clicks inside it through to the viewport. Under `?spike=web` the
 *  page draws the world here itself, from the song's `manifest.json`, with
 *  the format's web viewer — a chunk of its own, fetched only then
 *  (docs/rfcs/authoring/the-world-in-the-page.md). */
export function Viewport() {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current!;
    const report = () => {
      const r = el.getBoundingClientRect();
      send({ t: "viewport", x: r.x, y: r.y, w: r.width, h: r.height });
    };
    const observer = new ResizeObserver(report);
    observer.observe(el);
    window.addEventListener("resize", report);
    report();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", report);
    };
  }, []);
  useEffect(() => {
    if (spikeMode !== "web") return;
    const el = ref.current!;
    let viewer: any = null;
    let gone = false;
    Promise.all([
      fetch("/world/manifest.json").then((r) => r.json()),
      // @ts-ignore: the viewer is JavaScript; vite.config.ts maps the `three` it imports
      import("../../../world-export/js/world-viewer.js"),
    ])
      .then(([manifest, { createWorldViewer }]) => {
        if (gone) return;
        viewer = createWorldViewer(el, manifest, { assetBase: "/world/assets/", keyboard: false, embedApi: false });
        spikeWorld.viewer = viewer;
        spikeWorld.renderer = viewer.renderer;
        spikeWorld.entities = manifest.entities?.length;
      })
      .catch((e) => console.error("spike: the world", e));
    return () => {
      gone = true;
      viewer?.dispose();
    };
  }, []);
  // In a browser tab there is no world behind the page: show where it goes.
  return (
    <div
      ref={ref}
      data-spike-viewport
      className={
        spikeMode === "web"
          ? "relative min-h-0 flex-1 overflow-hidden bg-black"
          : inApp()
            ? "min-h-0 flex-1"
            : "grid min-h-0 flex-1 place-items-center bg-[repeating-conic-gradient(#1b2030_0_25%,#141822_0_50%)] bg-size-[24px_24px] text-faint"
      }
    >
      {!inApp() && spikeMode !== "web" && "viewport"}
    </div>
  );
}
