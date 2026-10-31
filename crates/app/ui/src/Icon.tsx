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
  music: "M6 11.5V4l6-1.5v7.5M4.5 11.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0M10.5 10a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0-3 0",
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
