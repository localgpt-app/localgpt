import { useEffect, useRef } from "react";
import { inApp, send } from "./bridge";

/** The hole the world shows through. The app fits its camera to this rect
 *  and lets clicks inside it through to the viewport. */
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
  // In a browser tab there is no world behind the page: show where it goes.
  return (
    <div
      ref={ref}
      className={
        inApp()
          ? "min-h-0 flex-1"
          : "grid min-h-0 flex-1 place-items-center bg-[repeating-conic-gradient(#1b2030_0_25%,#141822_0_50%)] bg-size-[24px_24px] text-faint"
      }
    >
      {!inApp() && "viewport"}
    </div>
  );
}
