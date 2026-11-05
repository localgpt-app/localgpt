import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import type { View } from "./Phone";

/** How a device's page is laid out. A phone has three views at the thumb;
 *  a tablet has two panes, the world over the terminals when it is held
 *  upright and beside them when it is turned. */
export type Layout = "phone" | "stacked" | "side";

/** The page's width decides, on the screen it is on. A screen whose short
 *  side is under 600 px is a phone's, whichever way it is held. On a
 *  tablet's (or a computer's) the page goes by its own width, so an iPad in
 *  Split View narrower than 600 px gets the phone's views, and an Android
 *  tablet upright (640 px, some of them) gets two panes. Side by side from
 *  1100 px, or from 900 on a tablet turned sideways. */
export function layoutFor(width: number, screen: { short: number; landscape: boolean }): Layout {
  if (screen.short < 600 || width < 600) return "phone";
  return width >= 1100 || (width >= 900 && screen.landscape) ? "side" : "stacked";
}

function current(): Layout {
  const short = Math.min(screen.width, screen.height);
  const landscape = screen.orientation ? screen.orientation.type.startsWith("landscape") : screen.width > screen.height;
  // The layout viewport's width: on iOS a pinch zoom changes innerWidth.
  return layoutFor(document.documentElement.clientWidth, { short, landscape });
}

/** The layout, again on each resize and turn. Never by the height: the
 *  keyboard takes that, and a layout that changed with it would move the
 *  terminal out from under the fingers typing into it. */
export function useLayout(): Layout {
  const [layout, setLayout] = useState(current);
  useEffect(() => {
    const update = () => setLayout(current());
    window.addEventListener("resize", update);
    screen.orientation?.addEventListener("change", update);
    return () => {
      window.removeEventListener("resize", update);
      screen.orientation?.removeEventListener("change", update);
    };
  }, []);
  return layout;
}

const MAIN: Record<Layout, string> = {
  phone: "relative min-h-0 flex-1",
  // The terminals take half, between 480 and 720 px; the world the rest.
  side: "grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_clamp(480px,50%,720px)]",
  // The world a third of the height, the terminals the rest. The height is
  // what is above the keyboard, so the third shrinks when it opens.
  stacked: "grid min-h-0 flex-1 grid-rows-[clamp(180px,34%,440px)_minmax(0,1fr)]",
};

/** The page under its header. The terminals' pane comes first in every
 *  layout, so a turn of the tablet keeps its terminals, and their screens,
 *  as they are; the world, in whichever form, after it. On a phone the
 *  pane shows while its view is open. */
export function Body({ layout, view, terminals, world }: { layout: Layout; view: View; terminals: ReactNode; world: ReactNode }) {
  const pane = {
    phone: view === "terminals" ? "flex h-full flex-col" : "hidden",
    side: "col-start-2 row-start-1 flex min-h-0 flex-col",
    stacked: "row-start-2 flex min-h-0 flex-col",
  }[layout];
  return (
    <main className={MAIN[layout]}>
      <section className={pane}>{terminals}</section>
      {world}
    </main>
  );
}
