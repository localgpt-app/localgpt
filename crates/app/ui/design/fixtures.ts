// The sheet's world: a music video's previs at the harbour, cut to a song
// from the starter pack — every kind of track at once. Deterministic, so the
// sheet draws the same every time.

import type { History } from "../src/bridge";
import type { ClipState, Clock, FrameLook, Shot, Signal, Track } from "../src/timeline/model";

export const clock: Clock = {
  duration: 192,
  bpm: 92,
  beatOffset: 0.4,
  sections: [0, 0.1, 0.27, 0.43, 0.6, 0.78, 0.9],
};

const BEAT = 60 / clock.bpm;
/** Where bar `n` starts (bars count from 1). */
export const bar = (n: number) => clock.beatOffset + (n - 1) * 4 * BEAT;
export const sectionAt = (i: number) => clock.sections[i] * clock.duration;

// Intro, verse, pre-chorus, chorus, bridge, chorus, outro.
const LEVEL = [0.32, 0.55, 0.66, 0.92, 0.48, 0.96, 0.3];
const STARTS = clock.sections.map((f) => f * clock.duration);
const section = (t: number) => STARTS.findLastIndex((start) => t >= start);
/** The song's loudness, eased over two seconds into each section. */
function level(t: number) {
  const i = Math.max(0, section(t));
  const into = t - STARTS[i];
  const before = LEVEL[Math.max(0, i - 1)];
  return i > 0 && into < 2 ? before + (LEVEL[i] - before) * (into / 2) : LEVEL[i];
}
/** 0 on a beat, rising to 1 just before the next. */
const phase = (t: number) => ((((t - clock.beatOffset) % BEAT) + BEAT) % BEAT) / BEAT;
const ends = (t: number) => Math.min(1, t / 1.5, (clock.duration - t) / 4);

function noise(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), a | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function signal(rate: number, seed: number, f: (t: number, r: () => number) => number): Signal {
  const r = noise(seed);
  const n = Math.round(clock.duration * rate);
  return { rate, values: Array.from({ length: n + 1 }, (_, i) => Math.min(1, Math.max(0, f(i / rate, r)))) };
}

const peaks = signal(6, 7, (t, r) => level(t) * (0.6 + 0.4 * (1 - phase(t))) * (0.78 + 0.22 * r()) * ends(t));
const drums = signal(6, 11, (t, r) => ([0, 6].includes(section(t)) ? 0.04 : level(t) * (1 - phase(t)) ** 1.6 * (0.85 + 0.15 * r())));
const bass = signal(4, 13, (t, r) => (section(t) === 0 ? 0.03 : level(t) * (0.72 + 0.28 * Math.sin((t * Math.PI) / (4 * BEAT))) * (0.9 + 0.1 * r())));
const vocals = signal(4, 17, (t, r) => ([1, 2, 3, 5].includes(section(t)) ? (0.35 + 0.45 * Math.sin(t * 0.9) ** 2) * level(t) * (0.85 + 0.15 * r()) : 0.02));
const other = signal(2, 19, (t, r) => (0.3 + 0.3 * level(t) + 0.08 * Math.sin(t * 0.3)) * (0.92 + 0.08 * r()) * ends(t));
const smooth = (s: Signal, k: number): Signal => ({
  ...s,
  values: s.values.map((_, i) => {
    const w = s.values.slice(Math.max(0, i - k), i + k + 1);
    return w.reduce((a, b) => a + b, 0) / w.length;
  }),
});

const look = (subject: FrameLook["subject"], at: number, sun: [number, number] = [0.72, 0.42], horizon = 0.62): FrameLook => ({
  horizon,
  sun,
  subject,
  at,
});

/** 3B sits two bars after the bridge starts: the move an agent proposed. */
const THREE_B = sectionAt(4) + 8 * BEAT;

export const shots: Shot[] = [
  { id: "1a", name: "1A", size: "WS", lens: 24, from: 0, to: sectionAt(1), look: look("tower", 0.3) },
  { id: "1b", name: "1B", size: "MS", lens: 50, from: sectionAt(1), to: 36.4, look: look("jetty", 0.55, [0.2, 0.4]) },
  { id: "2a", name: "2A", size: "CU", lens: 85, from: 36.4, to: sectionAt(2), look: look("lamp", 0.4, [0.8, 0.5], 0.7) },
  { id: "2b", name: "2B", size: "OTS", lens: 35, from: sectionAt(2), to: sectionAt(3), look: look("boat", 0.45) },
  { id: "3a", name: "3A", size: "WS", lens: 18, from: sectionAt(3), to: sectionAt(4), look: look("tower", 0.68, [0.3, 0.45]) },
  { id: "3b", name: "3B", size: "MCU", lens: 65, from: THREE_B, to: sectionAt(5), look: look("boat", 0.6, [0.25, 0.5]) },
  { id: "4a", name: "4A", size: "EWS", lens: 14, from: sectionAt(5), to: clock.duration, look: look("none", 0, [0.5, 0.55], 0.58) },
];

/** The shots with some in a given state: `{ "2b": "selected" }`. */
export function shotsIn(states: Record<string, ClipState>, move: Record<string, [number, number]> = {}): Shot[] {
  return shots.map((shot) => ({
    ...shot,
    state: states[shot.id] ?? "idle",
    author: states[shot.id] === "proposed" ? "claude" : undefined,
    ...(move[shot.id] ? { from: move[shot.id][0], to: move[shot.id][1] } : {}),
  }));
}

export const shotTrack = (list: Shot[], locked = false): Track => ({ kind: "shots", id: "shots", name: "Shots", shots: list, flags: { locked } });

export const camera: Extract<Track, { kind: "camera" }> = {
  kind: "camera",
  id: "camera",
  name: "Camera",
  tour: "harbour approach",
  keys: [{ t: 0, hold: 4 }, { t: sectionAt(1) }, { t: 36.4, hold: 6 }, { t: sectionAt(2) }, { t: sectionAt(3), hold: 3 }, { t: sectionAt(4) }, { t: sectionAt(5), hold: 8 }, { t: clock.duration }],
};

export const soundtrack = (open: boolean): Track => ({
  kind: "soundtrack",
  id: "song",
  name: "Soundtrack",
  title: "Tidewater",
  artist: "Starter pack",
  licence: "CC0-1.0",
  peaks,
  open,
});

export const stems: Track[] = [
  { kind: "stem", id: "drums", name: "drums", envelope: drums },
  { kind: "stem", id: "bass", name: "bass", envelope: bass },
  { kind: "stem", id: "vocals", name: "vocals", envelope: vocals },
  { kind: "stem", id: "other", name: "other", envelope: other },
];

export const modulations: Track[] = [
  {
    kind: "modulation",
    id: "lantern",
    name: "lantern",
    target: "emissive",
    signal: "beat",
    range: [0.2, 1.6],
    samples: signal(12, 23, (t) => (1 - phase(t)) * (section(t) === 0 ? 0.35 : 1)),
  },
  { kind: "modulation", id: "boat-y", name: "boat", target: "offset y", signal: "bass", range: [-0.1, 0.25], samples: smooth(bass, 3) },
  { kind: "modulation", id: "sea", name: "sea", target: "opacity", signal: "energy", range: [0.6, 1], samples: smooth(signal(2, 29, (t) => level(t) * ends(t)), 2) },
];

export const behaviors: Track[] = [
  { kind: "behavior", id: "beacon", name: "beacon", what: "spin", period: 6, from: 0, to: clock.duration },
  { kind: "behavior", id: "gulls", name: "gulls", what: "orbit", period: 14, from: 30, to: 170 },
  { kind: "behavior", id: "bob", name: "boat", what: "bob", period: 3.5, from: 0, to: clock.duration },
];

export const ambience: Track[] = [
  { kind: "ambience", id: "ocean", name: "ocean", from: 0, to: clock.duration, fade: [3, 6] },
  { kind: "ambience", id: "wind", name: "wind", from: sectionAt(3), to: clock.duration, fade: [8, 4] },
];

/** The spike's harbour, written by two agents: main up to the boat, and
 *  gemini's dome and flag forked from the tower. */
const minutes = (m: number) => Date.now() - m * 60_000;
const ids = ["14894e0a1c77d2f3", "2b7c91d03e4a5b60", "3f02aa7e1d9c8b45", "47d9c0b55a1e3f72", "5a61e2f08c4d7b19", "6c3b7d419e2f0a58", "7e85f1c2a4b6d390"].map(
  (hex) => `sha256:${hex}`,
);
const row = (i: number, parent: number | null, author: string, summary: string, ago: number, tip = false, head = false) => ({
  id: ids[i],
  parent: parent === null ? null : ids[parent],
  revision: i + 1,
  author,
  summary,
  message: null,
  timestamp_ms: minutes(ago),
  tip,
  head,
});
export const history: History = {
  rows: [
    row(0, null, "claude", "ground and sun", 52),
    row(1, 0, "claude", "tower", 47),
    row(2, 1, "claude", "lamp", 41),
    row(3, 1, "gemini", "dome", 33),
    row(4, 2, "claude", "jetty", 21),
    row(5, 3, "gemini", "flag", 14, true),
    row(6, 4, "claude", "boat", 6, true, true),
  ],
  current: ids[6],
  status: "7 entries · main at boat",
  turns: [
    { n: 1, agent: "claude", rows: 3, revision: 3 },
    { n: 2, agent: "claude", rows: 7, revision: 7 },
  ],
};
