// Stroke icons, 16 px, drawn in the current colour.
const PATHS = {
  first: "M5 4v8M12 4 6.5 8l5.5 4z",
  prev: "M11 4 5.5 8l5.5 4z",
  play: "M5.5 3.5v9l7-4.5z",
  pause: "M5.5 4v8M10.5 4v8",
  next: "M5 4l5.5 4L5 12z",
  last: "M11 4v8M4 4l5.5 4L4 12z",
  plus: "M8 3.5v9M3.5 8h9",
  close: "M4.5 4.5l7 7M11.5 4.5l-7 7",
  open: "M2.5 4.5h4l1.5 1.5h5.5v6.5h-11z",
  document: "M4 2.5h5l3 3v8h-8zM9 2.5v3h3M6 8.5h4M6 11h4",
  // The timeline's kinds of track.
  film: "M2.5 4.5h11v7h-11zM5 4.5v7M11 4.5v7",
  camera: "M2.5 5.5h7.5v5h-7.5zM10 7.25l3.5-2v5.5l-3.5-2",
  music: "M6 11.5V4l6-1.5v7.5M4.5 11.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M10.5 10a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0",
  wave: "M1.5 8c1.5-4.5 3-4.5 4.5 0s3 4.5 4.5 0 2.5-3 4-1.5",
  spin: "M12.5 8a4.5 4.5 0 1 1-1.3-3.2M12.5 2.5v2.75H9.75",
  wind: "M2.5 6h7.5a2 2 0 1 0-2-2M2.5 10h9a2 2 0 1 1-2 2M2.5 8h4",
  // Track and timeline controls.
  eye: "M1.5 8s2.5-4.5 6.5-4.5S14.5 8 14.5 8s-2.5 4.5-6.5 4.5S1.5 8 1.5 8zM8 6.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3",
  hidden: "M1.5 8s2.5-4.5 6.5-4.5S14.5 8 14.5 8s-2.5 4.5-6.5 4.5S1.5 8 1.5 8zM3 13 13 3",
  speaker: "M2.5 6h2.5l3.5-3v10l-3.5-3h-2.5zM11 5.5a3.5 3.5 0 0 1 0 5",
  mute: "M2.5 6h2.5l3.5-3v10l-3.5-3h-2.5zM10.5 6l3 4M13.5 6l-3 4",
  lock: "M4.5 7.5h7v6h-7zM6 7.5V5.5a2 2 0 1 1 4 0v2",
  loop: "M3 7.5V6.5a2 2 0 0 1 2-2h7.5M10.5 2.5l2 2-2 2M13 8.5v1a2 2 0 0 1-2 2H3.5M5.5 13.5l-2-2 2-2",
  magnet: "M4 2.5v5.5a4 4 0 0 0 8 0V2.5h-2.5v5.5a1.5 1.5 0 0 1-3 0V2.5zM4 5h2.5M9.5 5H12",
  zoomIn: "M7 3a4 4 0 1 0 0 8a4 4 0 1 0 0-8M10 10l3.5 3.5M5 7h4M7 5v4",
  zoomOut: "M7 3a4 4 0 1 0 0 8a4 4 0 1 0 0-8M10 10l3.5 3.5M5 7h4",
  chevronRight: "M6 4l4 4-4 4",
  chevronDown: "M4 6l4 4 4-4",
  // The outliner's kinds of thing, and the inspector's.
  cube: "M8 2.5l5 2.75v5.5L8 13.5l-5-2.75v-5.5zM3 5.25 8 8l5-2.75M8 8v5.5",
  bulb: "M6 12.25h4M6.75 14h2.5M8 2.5a3.5 3.5 0 0 0-2 6.37v1.88h4V8.87A3.5 3.5 0 0 0 8 2.5",
  group: "M8 3l6 3-6 3-6-3zM2 9.5l6 3 6-3",
  mesh: "M2.5 12.5 8 3l5.5 9.5zM5.25 7.75h5.5M8 3v9.5",
  search: "M7 3a4 4 0 1 0 0 8a4 4 0 1 0 0-8M10 10l3.5 3.5",
  aim: "M8 2.5v3M8 10.5v3M2.5 8h3M10.5 8h3M8 6.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3",
  out: "M6.5 4H12v5.5M12 4 4 12",
  move: "M8 2.5v11M2.5 8h11M6.5 4 8 2.5 9.5 4M6.5 12 8 13.5 9.5 12M4 6.5 2.5 8 4 9.5M12 6.5 13.5 8 12 9.5",
  // A paired device, and its page's views.
  phone: "M5.5 1.5h5a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1h-5a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1zM7 12h2",
  terminal: "M2.5 3.5h11v9h-11zM5 6.5l2 1.5-2 1.5M8.5 10h2.5",
  globe: "M8 2.5a5.5 5.5 0 1 0 0 11a5.5 5.5 0 1 0 0-11M2.5 8h11M8 2.5c-2.25 2.5-2.25 8.5 0 11M8 2.5c2.25 2.5 2.25 8.5 0 11",
  history: "M3 8a5 5 0 1 0 1.5-3.55M2.5 2.5v2.5H5M8 5.5V8l2 1.5",
  send: "M2.5 8h9M8 4.5 11.5 8 8 11.5",
  keyboard: "M1.5 4.5h13v7h-13zM4 7h1M7.5 7h1M11 7h1M5 9.5h6",
} as const;

export type IconName = keyof typeof PATHS;

/** The transport's arrows are solid; everything else is a line. */
const FILLED = new Set<IconName>(["play", "prev", "next", "first", "last"]);

/** 16 px unless `className` gives a size of its own. */
export function Icon({ name, className = "" }: { name: IconName; className?: string }) {
  return (
    <svg viewBox="0 0 16 16" className={className.includes("size-") ? className : `size-4 ${className}`} aria-hidden="true">
      <path
        d={PATHS[name]}
        fill={FILLED.has(name) ? "currentColor" : "none"}
        stroke="currentColor"
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
