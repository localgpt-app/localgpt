// The page's line to the app. Messages come in through window.__lg.recv
// (the app calls it) and go out through window.ipc (wry's). Opened in a plain
// browser — the dev server without the app — sends go nowhere and the page
// still draws, which is how it is designed and checked.

import type { Clock, Shot, Signal, Track } from "./timeline/model";

export type HistoryRow = {
  id: string;
  /** The entry it builds on; null for one built on the base. */
  parent: string | null;
  revision: number;
  author: string;
  summary: string;
  message: string | null;
  timestamp_ms: number;
  /** Nothing builds on it: the end of a branch. */
  tip: boolean;
  /** Main's tip. */
  head: boolean;
};

/** A turn that ended: the log held `rows` entries then. */
export type TurnMark = { n: number; agent: string; rows: number; revision: number };

export type History = {
  rows: HistoryRow[];
  /** The entry the canvas shows; null for the base. */
  current: string | null;
  status: string | null;
  turns: TurnMark[];
};

/** Which panels have something to show. */
export type Mode = { history: boolean; document: boolean };

/** One outline row: a heading, and the section (place) it belongs to. */
export type OutlineRow = {
  /** 1 for the title, 2 for a section, 3+ for a sub-heading within one. */
  level: number;
  text: string;
  section: number | null;
  /** The heading's line in the text, from 0. */
  line: number | null;
  /** Where the section's place came from, on the section's own row. */
  tier: string | null;
  failed: boolean;
  /** The model can be asked for this place again. */
  reauthor: boolean;
};

export type Thing = { label: string; first: string; count: number };

export type PlaceCard = {
  section: number;
  heading: string | null;
  tier: string | null;
  description: string | null;
  entities: number;
  things: Thing[];
  more: number;
};

export type DocumentView = {
  name: string;
  path: string;
  status: string;
  conflict: boolean;
  authoring: string | null;
  author_enabled: boolean;
  author_error: string | null;
  follow_cursor: boolean;
  outline: OutlineRow[];
  selected: number | null;
  card: PlaceCard | null;
};

/** A track as the app sends it: a modulation names its signal (`source`,
 *  a key of `signals`) rather than carrying the samples. */
export type TrackIn = Exclude<Track, { kind: "modulation" }> | (Omit<Extract<Track, { kind: "modulation" }>, "samples"> & { source: string });

/** The world's clock and its tracks, read from the world's document. */
export type TimelineView = {
  /** None: the world keeps no time. */
  clock: Clock | null;
  tracks: TrackIn[];
  /** The signals modulations read, sampled on the clock. */
  signals: Record<string, Signal>;
  /** Shots can be trimmed and moved: a live canvas. */
  editable: boolean;
  /** An agent's change waiting for the person, with the shots it moves. */
  proposal: { author: string; what: string; shots: Shot[] } | null;
};

/** A shot's new in and out points, as a drag lets go of it. */
export type ShotEdit = { id: number; in_s: number; out_s: number };

export type ToPage =
  | ({ t: "mode" } & Mode)
  | ({ t: "history" } & History)
  | { t: "playback"; playing: boolean }
  | { t: "title"; text: string | null }
  | { t: "opener"; busy: boolean; status: string | null }
  | { t: "document"; doc: DocumentView }
  | { t: "doc.text"; text: string; rev: number }
  | { t: "doc.reveal"; line: number }
  | { t: "timeline"; view: TimelineView | null }
  | { t: "clock"; at: number; playing: boolean; loop: [number, number] | null }
  | { t: "thumb"; id: string; url: string }
  | { t: "term.data"; id: string; b64: string }
  | { t: "term.exit"; id: string; code: number | null; error: string | null };

export type FromPage =
  | { t: "ready" }
  | { t: "viewport"; x: number; y: number; w: number; h: number }
  | { t: "seek"; id: string | null }
  | { t: "step"; to: "first" | "prev" | "next" | "last" }
  | { t: "play"; playing: boolean }
  | { t: "open"; url: string }
  | { t: "open.file" }
  | { t: "clock.seek"; at: number }
  | { t: "clock.play"; playing: boolean }
  | { t: "clock.loop"; span: [number, number] | null }
  | { t: "shot.look"; id: number | null }
  | { t: "shot.time"; edits: ShotEdit[] }
  | { t: "review"; accept: boolean }
  | { t: "mute"; ids: string[] }
  | { t: "doc.edit"; text: string }
  | { t: "doc.caret"; line: number }
  | { t: "doc.go"; section: number; line: number | null }
  | { t: "doc.pick"; name: string }
  | { t: "doc.reauthor"; section: number }
  | { t: "doc.author"; on: boolean }
  | { t: "doc.follow"; on: boolean }
  | { t: "term.open"; id: string; kind: string; cols: number; rows: number }
  | { t: "term.input"; id: string; data: string }
  | { t: "term.resize"; id: string; cols: number; rows: number }
  | { t: "term.close"; id: string };

declare global {
  interface Window {
    ipc?: { postMessage(message: string): void };
    __lg?: { recv(message: ToPage): void };
  }
}

const listeners = new Set<(message: ToPage) => void>();
/** The newest of each snapshot the app sends, for a panel that mounts after
 *  it arrived: the document's text comes in the same breath as the mode
 *  that makes its panel appear. A terminal's bytes are a stream, not kept;
 *  thumbnails are one per shot, and the app's own view keeps them. */
const newest = new Map<ToPage["t"], ToPage>();
window.__lg = {
  recv: (message) => {
    if (!message.t.startsWith("term.") && message.t !== "thumb") newest.set(message.t, message);
    listeners.forEach((listener) => listener(message));
  },
};

/** The newest message of a kind, if one came. */
export function last<T extends ToPage["t"]>(t: T): Extract<ToPage, { t: T }> | undefined {
  return newest.get(t) as Extract<ToPage, { t: T }> | undefined;
}

/** Inside the app, rather than a browser tab. */
export const inApp = () => window.ipc !== undefined;

/** In a browser tab there is no app behind the page; `?preview=` puts a
 *  stand-in there (`preview.ts`). */
let standIn: ((message: FromPage) => void) | null = null;
export function setStandIn(fn: (message: FromPage) => void) {
  standIn = fn;
}

export function send(message: FromPage) {
  if (window.ipc) window.ipc.postMessage(JSON.stringify(message));
  else standIn?.(message);
}

export function listen(listener: (message: ToPage) => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function decode(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}
