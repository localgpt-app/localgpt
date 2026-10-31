// What the timeline draws: the world's clock and its tracks, in seconds.
// The design sheet feeds it fixtures; the app sends the same shapes, read
// from the world's document — a track is a view of format data, never a
// second copy of it.

/** The world's clock: its soundtrack's length and beat grid
 *  (`SoundtrackDef`), or a shot list's running time when there is no song. */
export type Clock = {
  duration: number;
  /** Beats per minute; 0 when there is no beat grid. */
  bpm: number;
  /** The first beat, seconds. */
  beatOffset: number;
  /** Section starts as fractions of the duration, ascending from 0. */
  sections: number[];
};

/** Samples of a signal in `0..=1`, `rate` per second, from `from` seconds. */
export type Signal = { values: number[]; rate: number; from?: number };

export type ClipState = "idle" | "hover" | "selected" | "proposed" | "locked";

/** A shot's thumbnail until the app renders the real frame: a harbour at
 *  dusk in four strokes. `horizon` and `sun` are fractions of the frame. */
export type FrameLook = {
  horizon: number;
  sun: [number, number];
  subject: "tower" | "boat" | "lamp" | "jetty" | "none";
  /** Where the subject stands, a fraction of the width. */
  at: number;
};

/** A camera entity with `ext-cinematography`: its name, size and lens, and
 *  its `in_s`/`out_s` on the clock. */
export type Shot = {
  id: string;
  name: string;
  size: string;
  lens: number;
  from: number;
  to: number;
  look: FrameLook;
  state?: ClipState;
  /** Who proposed it, while it waits for review. */
  author?: string;
};

/** A tour waypoint at the time the camera reaches it, held for `hold`. */
export type CameraKey = { t: number; hold?: number };

export type TrackFlags = { hidden?: boolean; muted?: boolean; solo?: boolean; locked?: boolean };

export type Track =
  | { kind: "shots"; id: string; name: string; shots: Shot[]; flags?: TrackFlags }
  | { kind: "camera"; id: string; name: string; tour: string; keys: CameraKey[]; flags?: TrackFlags }
  | {
      kind: "soundtrack";
      id: string;
      name: string;
      title: string;
      artist: string;
      licence: string;
      /** The waveform's outline. */
      peaks: Signal;
      /** Its stems are listed under it. */
      open?: boolean;
      flags?: TrackFlags;
    }
  | { kind: "stem"; id: string; name: string; envelope: Signal; flags?: TrackFlags }
  | {
      kind: "modulation";
      id: string;
      /** The entity it drives. */
      name: string;
      target: string;
      signal: string;
      range: [number, number];
      samples: Signal;
      flags?: TrackFlags;
    }
  | { kind: "behavior"; id: string; name: string; what: string; period: number; from: number; to: number; flags?: TrackFlags }
  | { kind: "ambience"; id: string; name: string; from: number; to: number; fade: [number, number]; flags?: TrackFlags };

export type TrackKind = Track["kind"];
