import type { Clock, Signal } from "./model";

/** Seconds across a lane: `from` at 0 px, `to` at `width` px. */
export type Scale = { from: number; to: number; width: number };

export const xOf = (s: Scale, t: number) => ((t - s.from) / (s.to - s.from)) * s.width;
export const pxPerSecond = (s: Scale) => s.width / (s.to - s.from);

/** Label steps in seconds, each with the minor step drawn between. */
const STEPS: [number, number][] = [
  [1, 0.5],
  [2, 0.5],
  [5, 1],
  [10, 2],
  [15, 5],
  [30, 10],
  [60, 10],
  [120, 30],
  [300, 60],
];

/** The ruler's labelled ticks, at least `gap` px apart, and the minor ticks
 *  between them. */
export function ticks(s: Scale, gap = 64): { major: number[]; minor: number[] } {
  const pps = pxPerSecond(s);
  const [step, minorStep] = STEPS.find(([k]) => k * pps >= gap) ?? STEPS[STEPS.length - 1];
  const major: number[] = [];
  const minor: number[] = [];
  const first = Math.ceil(s.from / minorStep) * minorStep;
  for (let t = first; t <= s.to + 1e-9; t += minorStep) {
    const k = Math.round(t / minorStep) * minorStep;
    (Math.abs(k / step - Math.round(k / step)) < 1e-6 ? major : minor).push(k);
  }
  return { major, minor };
}

/** The beat grid in view: bar lines (every four beats, numbered from 1, with
 *  `label` set on those far enough apart to number) and the beats between
 *  when they are far enough apart to see. Empty without a grid. */
export function beatGrid(clock: Clock, s: Scale): { bars: { t: number; n: number; label: boolean }[]; beats: number[] } {
  if (clock.bpm <= 0) return { bars: [], beats: [] };
  const beat = 60 / clock.bpm;
  const pps = pxPerSecond(s);
  const showBeats = beat * pps >= 7;
  const every = [1, 2, 4, 8, 16, 32].find((k) => k * 4 * beat * pps >= 30) ?? 64;
  const bars: { t: number; n: number; label: boolean }[] = [];
  const beats: number[] = [];
  const start = Math.max(0, Math.floor((s.from - clock.beatOffset) / beat));
  for (let i = start; ; i++) {
    const t = clock.beatOffset + i * beat;
    if (t > Math.min(s.to, clock.duration)) break;
    if (t < s.from) continue;
    if (i % 4 === 0) {
      const n = i / 4 + 1;
      bars.push({ t, n, label: (n - 1) % every === 0 });
    } else if (showBeats) beats.push(t);
  }
  return { bars, beats };
}

/** Section starts in seconds. */
export const sectionStarts = (clock: Clock) => clock.sections.map((f) => f * clock.duration);

/** Bar and beat (both from 1) at `t`, or null without a grid. */
export function barBeat(clock: Clock, t: number): [number, number] | null {
  if (clock.bpm <= 0) return null;
  const beats = Math.max(0, (t - clock.beatOffset) * (clock.bpm / 60));
  return [Math.floor(beats / 4) + 1, Math.floor(beats % 4) + 1];
}

function points(signal: Signal, s: Scale): [number, number][] {
  const from = signal.from ?? 0;
  return signal.values.map((v, i) => [xOf(s, from + i / signal.rate), v]);
}

const f = (n: number) => n.toFixed(1);

/** A signal as a filled outline over a lane `h` px tall: mirrored about the
 *  middle (a waveform), or standing on the bottom (an envelope). */
export function areaPath(signal: Signal, s: Scale, h: number, mirror: boolean): string {
  const pts = points(signal, s);
  if (pts.length === 0) return "";
  if (mirror) {
    const mid = h / 2;
    const top = pts.map(([x, v]) => `${f(x)} ${f(mid - v * mid * 0.9)}`);
    const bottom = pts.reverse().map(([x, v]) => `${f(x)} ${f(mid + v * mid * 0.9)}`);
    return `M${top.join("L")}L${bottom.join("L")}Z`;
  }
  const line = pts.map(([x, v]) => `${f(x)} ${f(h - v * (h - 3))}`);
  return `M${f(pts[0][0])} ${h}L${line.join("L")}L${f(pts[pts.length - 1][0])} ${h}Z`;
}

/** A signal as a line over a lane `h` px tall, 0 at the bottom. */
export function linePath(signal: Signal, s: Scale, h: number): string {
  const pts = points(signal, s);
  return pts.length === 0 ? "" : `M${pts.map(([x, v]) => `${f(x)} ${f(h - 3 - v * (h - 6))}`).join("L")}`;
}
