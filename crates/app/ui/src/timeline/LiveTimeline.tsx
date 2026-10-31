import { useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type PointerEvent as ReactPointerEvent } from "react";
import type { TimelineView } from "../bridge";
import { send } from "../bridge";
import { timecode } from "../timecode";
import { sectionStarts } from "./geometry";
import type { CameraKey, Clock, Shot, Signal, Toggle, Track, TrackFlags } from "./model";
import { type Drag, type Edge, HEADER, type Recut, type Snap, Timeline, type TimelineHandlers } from "./Timeline";

/** The clock as the app last told it, and when that arrived
 *  (`performance.now()`). */
export type ClockState = { at: number; playing: boolean; loop: [number, number] | null; received: number };

const EMPTY: Signal = { values: [], rate: 1 };
/** The shortest a trim leaves a shot, seconds. */
const SHORTEST = 0.1;
/** How near a line a dragged edge comes before it snaps there, px. */
const PULL = 8;
/** The narrowest zoom, seconds across. */
const CLOSEST = 1;
/** The shortest a recut leaves a section, seconds. */
const MIN_SECTION = 1;
/** How near its neighbour a section's start comes before the two join, px. */
const JOIN = 6;
const FLAG: Record<Toggle, keyof TrackFlags> = { hide: "hidden", mute: "muted", solo: "solo", lock: "locked" };

/** The playhead between the app's clock messages: while it plays, it runs
 *  on from the last one, round its loop or the whole clock. */
export function runOn(clock: ClockState, duration: number, now: number): number {
  let at = clock.at + Math.max(0, now - clock.received) / 1000;
  const [from, to] = clock.loop ?? [0, 0];
  if (clock.loop && to > from && at >= to) at = from + ((at - to) % (to - from));
  else if (duration > 0 && at >= duration) at %= duration;
  return at;
}

function usePlayhead(clock: ClockState | null, duration: number): number {
  const [t, setT] = useState(clock?.at ?? 0);
  useEffect(() => {
    if (!clock) return;
    if (!clock.playing) {
      setT(clock.at);
      return;
    }
    let frame = 0;
    const tick = () => {
      setT(runOn(clock, duration, performance.now()));
      frame = requestAnimationFrame(tick);
    };
    tick();
    return () => cancelAnimationFrame(frame);
  }, [clock, duration]);
  return t;
}

type Line = { at: number; label: string };

/** What a drag can snap to under `snap`, and the playhead always. */
function snapLines(snap: Snap, clock: Clock, shots: Shot[], except: string[], playhead: number, view: [number, number]): Line[] {
  const lines: Line[] = [{ at: playhead, label: `playhead · ${timecode(playhead, true)}` }];
  if (snap === "beats" && clock.bpm > 0) {
    const beat = 60 / clock.bpm;
    for (let i = Math.max(0, Math.floor((view[0] - clock.beatOffset) / beat)); ; i++) {
      const t = clock.beatOffset + i * beat;
      if (t > Math.min(view[1], clock.duration)) break;
      const bar = Math.floor(i / 4) + 1;
      lines.push({ at: t, label: i % 4 === 0 ? `bar ${bar}` : `bar ${bar} · ${(i % 4) + 1}` });
    }
  }
  if (snap === "beats" || snap === "sections") {
    sectionStarts(clock).forEach((t, i) => lines.push({ at: t, label: `§${i + 1} · ${timecode(t, true)}` }));
    lines.push({ at: clock.duration, label: `end · ${timecode(clock.duration, true)}` });
  }
  if (snap === "shots") {
    for (const shot of shots) {
      if (except.includes(shot.id) || shot.state === "proposed") continue;
      lines.push({ at: shot.from, label: `${shot.name} in` }, { at: shot.to, label: `${shot.name} out` });
    }
  }
  return lines;
}

function nearest(lines: Line[], t: number, pull: number): Line | null {
  let best: Line | null = null;
  for (const line of lines) {
    const d = Math.abs(line.at - t);
    if (d <= pull && (!best || d < Math.abs(best.at - t))) best = line;
  }
  return best;
}

/** What a section's start snaps to: bars, or the cuts, and the playhead. */
function recutLines(snap: Snap, clock: Clock, shots: Shot[], playhead: number, view: [number, number]): Line[] {
  if (snap === "off") return [];
  if (snap === "shots") return snapLines("shots", clock, shots, [], playhead, view);
  return snapLines("beats", clock, [], [], playhead, view).filter((line) => /^bar \d+$/.test(line.label) || line.label.startsWith("playhead"));
}

/** A tour's stops with one stop's hold changed: the stops after it come
 *  later by as much, and the travel between them stays. */
function heldKeys(keys: CameraKey[], stop: number, hold: number): CameraKey[] {
  const by = hold - (keys[stop]?.hold ?? 0);
  return keys.map((key, i) => (i === stop ? { ...key, hold: hold > 0 ? hold : undefined } : i > stop ? { ...key, t: key.t + by } : key));
}

const clamp = (v: number, lo: number, hi: number) => Math.min(Math.max(v, lo), Math.max(lo, hi));
const signed = (d: number) => `${d < 0 ? "−" : "+"}${Math.abs(d).toFixed(2)} s`;
const seconds = (span: number) => (span < 60 ? `${span.toFixed(1)} s` : timecode(span));

/** A clip held by the pointer: what was pressed, the neighbour whose edge
 *  rolls with it, and where it would go if let go now. */
type Held = {
  edge: Edge;
  shot: Shot;
  partner: Shot | null;
  x0: number;
  from: number;
  to: number;
  snapped: Line | null;
  moved: boolean;
};

/** A section's start held on the band: where it would go, and whether it
 *  is onto a neighbour, to join it. */
type Recutting = { section: number; to: number; snapped: Line | null; join: boolean; moved: boolean; x0: number };

/** A tour stop's hold, held by its end. */
type Holding = { track: string; tour: string; stop: number; at: number; was: number; hold: number; snapped: Line | null; moved: boolean; x0: number };

/** The timeline in the app: the view the app sends, played and edited. It
 *  keeps only what the page alone knows — the zoom, the selection, a drag
 *  in progress, the headers' toggles — and asks the app for the rest, so a
 *  trim lands as an entry in the history before the page draws it for good. */
export function LiveTimeline({ timeline, clock, thumbs }: { timeline: TimelineView; clock: ClockState | null; thumbs: Record<string, string> }) {
  const box = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);
  const [maxLanes, setMaxLanes] = useState(280);
  const duration = timeline.clock?.duration ?? 0;
  const [range, setRange] = useState<[number, number]>([0, Math.max(1, duration)]);
  const [snap, setSnap] = useState<Snap>(timeline.clock && timeline.clock.bpm > 0 ? "beats" : "shots");
  const [menu, setMenu] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const [held, setHeld] = useState<Held | null>(null);
  const [recutting, setRecutting] = useState<Recutting | null>(null);
  const [holding, setHolding] = useState<Holding | null>(null);
  const [flags, setFlags] = useState<Record<string, TrackFlags>>({});
  const [open, setOpen] = useState<string[]>([]);
  /** Edits sent — shots' times, the sections, a hold — drawn until the
   *  app's next view has them. */
  const [pending, setPending] = useState<Record<string, [number, number]> | null>(null);
  const [pendingSections, setPendingSections] = useState<number[] | null>(null);
  const [pendingHold, setPendingHold] = useState<{ track: string; stop: number; hold: number } | null>(null);
  const [scrub, setScrub] = useState<number | null>(null);
  const running = usePlayhead(clock, duration);
  const playhead = scrub ?? running;
  const playing = clock?.playing ?? false;

  useEffect(() => {
    const el = box.current!;
    const fit = () => {
      setWidth(Math.max(200, el.clientWidth - HEADER));
      setMaxLanes(Math.round(clamp(window.innerHeight * 0.3, 110, 340)));
    };
    const observer = new ResizeObserver(fit);
    observer.observe(el);
    fit();
    return () => observer.disconnect();
  }, []);

  // The whole clock in view to begin with, and again when the world changes
  // under a view that showed all of it.
  const fitted = useRef(duration);
  useEffect(() => {
    const was = fitted.current;
    fitted.current = duration;
    if (duration <= 0) return;
    setRange(([from, to]) => (from <= 1e-3 && Math.abs(to - was) < 1e-2) || to > duration ? [0, duration] : [from, to]);
  }, [duration]);

  // A new view is the app's word on the edits: what was sent is in it, or refused.
  useEffect(() => {
    setPending(null);
    setPendingSections(null);
    setPendingHold(null);
  }, [timeline]);
  const clockNow = useMemo(
    () => (timeline.clock && pendingSections ? { ...timeline.clock, sections: pendingSections } : timeline.clock),
    [timeline.clock, pendingSections],
  );
  // A clock message after a scrub has the seek in it.
  useEffect(() => setScrub(null), [clock]);

  const hasSong = timeline.tracks.some((track) => track.kind === "soundtrack");
  const tracks: Track[] = useMemo(() => {
    const stems = open.includes("soundtrack");
    const hasStems = timeline.tracks.some((track) => track.kind === "stem");
    const proposal = timeline.proposal;
    return timeline.tracks.flatMap((track): Track[] => {
      // The app names the toggles that do something; a track it names none
      // for draws none.
      const f = flags[track.id];
      const toggles = track.toggles ?? [];
      switch (track.kind) {
        case "modulation": {
          const { source, ...rest } = track;
          return [{ ...rest, samples: timeline.signals[source] ?? EMPTY, flags: f, toggles }];
        }
        case "soundtrack":
          return [{ ...track, open: hasStems ? stems : undefined, flags: f, toggles }];
        case "stem":
          return stems ? [{ ...track, flags: f, toggles }] : [];
        case "camera": {
          const keys = pendingHold?.track === track.id ? heldKeys(track.keys, pendingHold.stop, pendingHold.hold) : track.keys;
          return [{ ...track, keys, flags: f, toggles }];
        }
        case "shots": {
          const locked = !!f?.locked;
          const shots: Shot[] = track.shots.map((shot) => {
            const moved = pending?.[shot.id];
            return {
              ...shot,
              from: moved?.[0] ?? shot.from,
              to: moved?.[1] ?? shot.to,
              thumb: thumbs[shot.id],
              state: locked ? "locked" : shot.id === selected ? "selected" : "idle",
            };
          });
          const proposed: Shot[] = (proposal?.shots ?? []).map((shot) => ({ ...shot, thumb: thumbs[shot.id], state: "proposed", author: proposal?.author }));
          return [{ ...track, flags: f, toggles, shots: [...shots, ...proposed] }];
        }
        default:
          return [{ ...track, flags: f, toggles }];
      }
    });
  }, [timeline, flags, open, selected, thumbs, pending, pendingHold]);
  const shots = useMemo(() => tracks.flatMap((track) => (track.kind === "shots" ? track.shots.filter((shot) => shot.state !== "proposed") : [])), [tracks]);

  // What is silenced: the muted, or with a solo anywhere, everything else.
  const heard = timeline.tracks.filter((track) => track.toggles?.includes("mute")).map((track) => track.id);
  const soloed = heard.filter((id) => flags[id]?.solo);
  const muted = heard.filter((id) => flags[id]?.muted || (soloed.length > 0 && !soloed.includes(id)));
  const mutedKey = muted.join("\n");
  const sentMutes = useRef("");
  useEffect(() => {
    if (sentMutes.current === mutedKey) return;
    sentMutes.current = mutedKey;
    send({ t: "mute", ids: mutedKey ? mutedKey.split("\n") : [] });
  }, [mutedKey]);

  // What is hidden: the seen tracks whose eye is shut — what they move
  // stands still.
  const hidable = timeline.tracks.filter((track) => track.toggles?.includes("hide")).map((track) => track.id);
  const hiddenKey = hidable.filter((id) => flags[id]?.hidden).join("\n");
  const sentHidden = useRef("");
  useEffect(() => {
    if (sentHidden.current === hiddenKey) return;
    sentHidden.current = hiddenKey;
    send({ t: "hide", ids: hiddenKey ? hiddenKey.split("\n") : [] });
  }, [hiddenKey]);

  // Playing past the edge of the view turns the page.
  useEffect(() => {
    if (!playing || held || recutting || holding || scrub !== null) return;
    const [from, to] = range;
    if (playhead >= from && playhead <= to) return;
    const span = to - from;
    const start = clamp(playhead - span * 0.1, 0, duration - span);
    setRange([start, start + span]);
  }, [playhead, playing, held, recutting, holding, scrub, range, duration]);

  // The handlers read the newest state through one ref, so they stay the
  // same functions and the lanes they are passed to needn't draw again.
  const latest = useRef({ timeline, clock, clockNow, tracks, range, width, snap, shots, selected, playhead, flags, duration, hasSong });
  latest.current = { timeline, clock, clockNow, tracks, range, width, snap, shots, selected, playhead, flags, duration, hasSong };

  // What the handlers, the wheel and the keys share: time under the
  // pointer, seeking (one ask a frame while scrubbing), the cuts, the zoom.
  const tools = useMemo(() => {
    const pps = () => latest.current.width / (latest.current.range[1] - latest.current.range[0]);
    const timeAt = (clientX: number) => {
      const left = box.current!.getBoundingClientRect().left + HEADER;
      return latest.current.range[0] + (clientX - left) / pps();
    };
    let frame = 0;
    let seekTo = 0;
    const seek = (t: number) => {
      seekTo = clamp(t, 0, latest.current.duration);
      setScrub(seekTo);
      if (!frame)
        frame = requestAnimationFrame(() => {
          frame = 0;
          send({ t: "clock.seek", at: seekTo });
        });
    };
    const cuts = () => {
      const { shots, clockNow, duration } = latest.current;
      const times = shots.length > 0 ? shots.flatMap((shot) => [shot.from, shot.to]) : clockNow ? sectionStarts(clockNow) : [];
      return [...new Set([0, ...times, duration])].sort((a, b) => a - b);
    };
    const zoom = (factor: number, anchor?: number) => {
      const { range, duration, playhead } = latest.current;
      if (duration <= 0) return;
      const [from, to] = range;
      const span = clamp((to - from) / factor, Math.min(CLOSEST, duration), duration);
      const at = anchor ?? (playhead >= from && playhead <= to ? playhead : (from + to) / 2);
      const start = clamp(at - ((at - from) * span) / (to - from), 0, duration - span);
      setRange([start, start + span]);
    };
    /** Where a held clip goes for the pointer `dt` seconds on. */
    const place = (held: Held, dt: number, free: boolean): Pick<Held, "from" | "to" | "snapped"> => {
      const { snap, timeline, shots, playhead, range, duration, hasSong } = latest.current;
      const clock = timeline.clock!;
      const except = [held.shot.id, held.partner?.id ?? ""];
      const lines = free || snap === "off" ? [] : snapLines(snap, clock, shots, except, playhead, range);
      const pull = PULL / pps();
      // A song is the clock's length; a shot list's grows with its last shot.
      const limit = hasSong ? duration : Number.POSITIVE_INFINITY;
      const { shot, partner } = held;
      if (held.edge === "in") {
        const line = nearest(lines, shot.from + dt, pull);
        const from = clamp(line?.at ?? shot.from + dt, partner ? partner.from + SHORTEST : 0, shot.to - SHORTEST);
        return { from, to: shot.to, snapped: line && line.at === from ? line : null };
      }
      if (held.edge === "out") {
        const line = nearest(lines, shot.to + dt, pull);
        const to = clamp(line?.at ?? shot.to + dt, shot.from + SHORTEST, partner ? partner.to - SHORTEST : limit);
        return { from: shot.from, to, snapped: line && line.at === to ? line : null };
      }
      // A move snaps whichever edge comes nearer a line.
      const length = shot.to - shot.from;
      const start = shot.from + dt;
      const a = nearest(lines, start, pull);
      const b = nearest(lines, start + length, pull);
      let line: Line | null = null;
      let from = start;
      if (a && (!b || Math.abs(a.at - start) <= Math.abs(b.at - start - length))) [from, line] = [a.at, a];
      else if (b) [from, line] = [b.at - length, b];
      const placed = clamp(from, 0, limit - length);
      return { from: placed, to: placed + length, snapped: placed === from ? line : null };
    };
    return { pps, timeAt, seek, cuts, zoom, place };
  }, []);

  const on: TimelineHandlers = useMemo(
    () => ({
      rulerDown(e: ReactPointerEvent) {
        if (e.button !== 0) return;
        setMenu(false);
        tools.seek(tools.timeAt(e.clientX));
        const move = (ev: PointerEvent) => tools.seek(tools.timeAt(ev.clientX));
        const up = (ev: PointerEvent) => {
          tools.seek(tools.timeAt(ev.clientX));
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", up);
        };
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", up);
      },
      clipDown(e: ReactPointerEvent, shot: Shot, edge: Edge) {
        if (e.button !== 0) return;
        setMenu(false);
        setSelected(shot.id);
        const { timeline, flags, shots } = latest.current;
        if (!timeline.editable || flags.shots?.locked) return;
        // An edge that meets the next shot's rolls the cut between them.
        const near = (a: number, b: number) => Math.abs(a - b) < 0.01;
        const partner =
          edge === "out"
            ? (shots.find((other) => other.id !== shot.id && near(other.from, shot.to)) ?? null)
            : edge === "in"
              ? (shots.find((other) => other.id !== shot.id && near(other.to, shot.from)) ?? null)
              : null;
        let held: Held = { edge, shot, partner, x0: e.clientX, from: shot.from, to: shot.to, snapped: null, moved: false };
        const stop = () => {
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", up);
          window.removeEventListener("keydown", key, true);
          setHeld(null);
        };
        const move = (ev: PointerEvent) => {
          if (!held.moved && Math.abs(ev.clientX - held.x0) < 3) return;
          held = { ...held, moved: true, ...tools.place(held, (ev.clientX - held.x0) / tools.pps(), ev.altKey) };
          setHeld(held);
        };
        const up = () => {
          stop();
          if (!held.moved) return;
          const { shot, partner, from, to } = held;
          const edits = [{ id: Number(shot.id), in_s: from, out_s: to }];
          if (partner && held.edge === "out") edits.push({ id: Number(partner.id), in_s: to, out_s: partner.to });
          if (partner && held.edge === "in") edits.push({ id: Number(partner.id), in_s: partner.from, out_s: from });
          send({ t: "shot.time", edits });
          setPending(Object.fromEntries(edits.map((edit) => [String(edit.id), [edit.in_s, edit.out_s]])));
        };
        // Esc lets go without writing anything.
        const key = (ev: KeyboardEvent) => {
          if (ev.key !== "Escape") return;
          ev.stopPropagation();
          held = { ...held, moved: false };
          stop();
        };
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", up);
        window.addEventListener("keydown", key, true);
        setHeld(held);
      },
      clipOpen(shot: Shot) {
        tools.seek(shot.from);
        send({ t: "shot.look", id: Number(shot.id) });
      },
      laneDown(e: ReactPointerEvent) {
        if (e.button !== 0) return;
        setMenu(false);
        setSelected(null);
      },
      play() {
        send({ t: "clock.play", playing: !latest.current.clock?.playing });
      },
      step(direction: -1 | 1) {
        const { playhead } = latest.current;
        const all = tools.cuts();
        const to = direction > 0 ? all.find((t) => t > playhead + 0.02) : [...all].reverse().find((t) => t < playhead - 0.02);
        if (to !== undefined) tools.seek(to);
      },
      loop() {
        const { clock, selected, shots, timeline, playhead, range } = latest.current;
        if (clock?.loop) {
          send({ t: "clock.loop", span: null });
          return;
        }
        // The selected shot, else the section under the playhead, else what is in view.
        const shot = shots.find((s) => s.id === selected);
        const starts = timeline.clock ? [...sectionStarts(timeline.clock), timeline.clock.duration] : [];
        const section = starts.length > 2 ? starts.findIndex((t, i) => playhead >= t && playhead < starts[i + 1]) : -1;
        const span: [number, number] = shot ? [shot.from, shot.to] : section >= 0 ? [starts[section], starts[section + 1]] : range;
        send({ t: "clock.loop", span });
      },
      zoom: (factor: number) => tools.zoom(factor),
      fit() {
        const { duration } = latest.current;
        if (duration > 0) setRange([0, duration]);
      },
      snapMenu: () => setMenu((open) => !open),
      snap(snap: Snap) {
        setSnap(snap);
        setMenu(false);
      },
      toggle(track: string, toggle: Toggle) {
        const flag = FLAG[toggle];
        setFlags((all) => ({ ...all, [track]: { ...all[track], [flag]: !all[track]?.[flag] } }));
      },
      open: (track: string) => setOpen((all) => (all.includes(track) ? all.filter((t) => t !== track) : [...all, track])),
      review: (accept: boolean) => send({ t: "review", accept }),
      sectionDown(e: ReactPointerEvent, section: number) {
        if (e.button !== 0) return;
        setMenu(false);
        const { timeline, clockNow, duration, hasSong } = latest.current;
        if (!timeline.editable || !hasSong || !clockNow) return;
        const starts = sectionStarts(clockNow);
        const from = starts[section];
        const [before, after] = [starts[section - 1], starts[section + 1] ?? duration];
        let held: Recutting = { section, to: from, snapped: null, join: false, moved: false, x0: e.clientX };
        const stop = () => {
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", up);
          window.removeEventListener("keydown", key, true);
          setRecutting(null);
        };
        const move = (ev: PointerEvent) => {
          if (!held.moved && Math.abs(ev.clientX - held.x0) < 3) return;
          const { snap, shots, playhead, range } = latest.current;
          const pps = tools.pps();
          const t = from + (ev.clientX - held.x0) / pps;
          const reach = JOIN / pps;
          if (t <= before + reach || t >= after - reach) {
            // Onto a neighbour: the two sections join.
            held = { ...held, moved: true, to: t <= before + reach ? before : after, snapped: null, join: true };
          } else {
            const line = ev.altKey ? null : nearest(recutLines(snap, clockNow, shots, playhead, range), t, PULL / pps);
            const to = clamp(line?.at ?? t, before + MIN_SECTION, after - MIN_SECTION);
            held = { ...held, moved: true, to, snapped: line && line.at === to ? line : null, join: false };
          }
          setRecutting(held);
        };
        const up = () => {
          stop();
          if (!held.moved) return;
          const next = held.join ? starts.filter((_, i) => i !== section) : starts.map((t, i) => (i === section ? held.to : t));
          const sections = next.map((t) => t / duration);
          send({ t: "song.sections", sections });
          setPendingSections(sections);
        };
        // Esc lets go without writing anything.
        const key = (ev: KeyboardEvent) => {
          if (ev.key !== "Escape") return;
          ev.stopPropagation();
          held = { ...held, moved: false };
          stop();
        };
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", up);
        window.addEventListener("keydown", key, true);
        setRecutting(held);
      },
      bandOpen(e: ReactMouseEvent) {
        const { timeline, clockNow, duration, hasSong, snap, shots, playhead, range } = latest.current;
        if (!timeline.editable || !hasSong || !clockNow) return;
        const pps = tools.pps();
        const at = tools.timeAt(e.clientX);
        const line = e.altKey ? null : nearest(recutLines(snap, clockNow, shots, playhead, range), at, PULL / pps);
        const t = line?.at ?? at;
        const starts = sectionStarts(clockNow);
        if (t < MIN_SECTION || t > duration - MIN_SECTION || starts.some((start) => Math.abs(start - t) < MIN_SECTION)) return;
        const sections = [...starts, t].sort((a, b) => a - b).map((start) => start / duration);
        send({ t: "song.sections", sections });
        setPendingSections(sections);
      },
      holdDown(e: ReactPointerEvent, track: string, stop: number) {
        if (e.button !== 0) return;
        setMenu(false);
        const { timeline, tracks, flags } = latest.current;
        const camera = tracks.find((t) => t.id === track);
        if (!timeline.editable || flags[track]?.locked || camera?.kind !== "camera") return;
        const key = camera.keys[stop];
        const was = key.hold ?? 0;
        let held: Holding = { track, tour: camera.tour, stop, at: key.t, was, hold: was, snapped: null, moved: false, x0: e.clientX };
        const end = () => {
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", up);
          window.removeEventListener("keydown", escape, true);
          setHolding(null);
        };
        const move = (ev: PointerEvent) => {
          if (!held.moved && Math.abs(ev.clientX - held.x0) < 3) return;
          const { snap, shots, playhead, range, clockNow } = latest.current;
          const pps = tools.pps();
          const t = key.t + was + (ev.clientX - held.x0) / pps;
          const lines = ev.altKey || snap === "off" || !clockNow ? [] : snapLines(snap, clockNow, shots, [], playhead, range);
          const line = nearest(lines, t, PULL / pps);
          const hold = Math.max(0, (line?.at ?? t) - key.t);
          held = { ...held, moved: true, hold, snapped: line && Math.abs(line.at - key.t - hold) < 1e-9 ? line : null };
          setHolding(held);
        };
        const up = () => {
          end();
          if (!held.moved || Math.abs(held.hold - was) < 0.005) return;
          const hold = Math.round(held.hold * 100) / 100;
          send({ t: "tour.hold", tour: held.tour, stop, hold });
          setPendingHold({ track, stop, hold });
        };
        const escape = (ev: KeyboardEvent) => {
          if (ev.key !== "Escape") return;
          ev.stopPropagation();
          held = { ...held, moved: false };
          end();
        };
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", up);
        window.addEventListener("keydown", escape, true);
        setHolding(held);
      },
    }),
    [tools],
  );

  // A pinch (or ⌘/Ctrl with the wheel) zooms about the pointer; a sideways
  // swipe pans. Upright scrolling is left to the lanes.
  useEffect(() => {
    const el = box.current!;
    const wheel = (e: WheelEvent) => {
      const { range, duration } = latest.current;
      if (duration <= 0) return;
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        tools.zoom(Math.exp(-e.deltaY * 0.01), tools.timeAt(e.clientX));
      } else if (Math.abs(e.deltaX) > Math.abs(e.deltaY) || e.shiftKey) {
        e.preventDefault();
        const dt = (e.shiftKey && !e.deltaX ? e.deltaY : e.deltaX) / tools.pps();
        const span = range[1] - range[0];
        const start = clamp(range[0] + dt, 0, duration - span);
        setRange([start, start + span]);
      }
    };
    el.addEventListener("wheel", wheel, { passive: false });
    return () => el.removeEventListener("wheel", wheel);
  }, [tools]);

  const keys = (e: ReactKeyboardEvent) => {
    const { timeline, clock, duration } = latest.current;
    const beat = timeline.clock && timeline.clock.bpm > 0 ? 60 / timeline.clock.bpm : 1;
    switch (e.key) {
      case " ":
        e.preventDefault();
        on.play();
        break;
      case "Enter":
        if (timeline.proposal) on.review(true);
        break;
      case "Escape":
        if (timeline.proposal) on.review(false);
        else setSelected(null);
        setMenu(false);
        break;
      case "ArrowLeft":
      case "ArrowRight": {
        e.preventDefault();
        const direction = e.key === "ArrowLeft" ? -1 : 1;
        if (e.shiftKey) on.step(direction);
        else tools.seek((clock ? playhead : 0) + direction * beat);
        break;
      }
      case "Home":
        tools.seek(0);
        break;
      case "End":
        tools.seek(duration);
        break;
    }
  };

  const shot = shots.find((s) => s.id === selected) ?? null;
  const selection = held?.moved
    ? {
        label: held.partner ? (held.edge === "in" ? `${held.partner.name} | ${held.shot.name}` : `${held.shot.name} | ${held.partner.name}`) : held.shot.name,
        detail:
          held.edge === "out"
            ? `out ${timecode(held.shot.to, true)} → ${timecode(held.to, true)} · ${signed(held.to - held.shot.to)}`
            : `in ${timecode(held.shot.from, true)} → ${timecode(held.from, true)} · ${signed(held.from - held.shot.from)}`,
      }
    : shot && { label: shot.name, detail: `${shot.size} · ${shot.lens} mm · ${timecode(shot.from, true)} → ${timecode(shot.to, true)} · ${seconds(shot.to - shot.from)}` };
  const recut: Recut | null = recutting?.moved
    ? {
        section: recutting.section,
        to: recutting.to,
        label: recutting.join
          ? `§${recutting.section} and §${recutting.section + 1} join`
          : !recutting.snapped
            ? timecode(recutting.to, true)
            : recutting.snapped.label.startsWith("playhead")
              ? recutting.snapped.label
              : `${recutting.snapped.label} · ${timecode(recutting.to, true)}`,
      }
    : null;
  const drag: Drag | null = holding?.moved
    ? {
        track: holding.track,
        from: holding.at,
        to: holding.at + holding.hold,
        label: `holds ${holding.hold.toFixed(1)} s · the stops after ${signed(holding.hold - holding.was)}`,
        snapped: holding.snapped ?? undefined,
      }
    : held?.moved
    ? {
        track: "shots",
        from: held.from,
        to: held.to,
        label:
          held.edge === "body"
            ? `${held.shot.name} at ${timecode(held.from, true)}`
            : `${selection ? selection.label : held.shot.name} at ${held.snapped?.label ?? timecode(held.edge === "in" ? held.from : held.to, true)}`,
        snapped: held.snapped ?? undefined,
      }
    : null;

  return (
    <div ref={box} tabIndex={0} onKeyDown={keys} className="outline-none">
      <Timeline
        clock={clockNow}
        tracks={tracks}
        view={range}
        width={width}
        playhead={playhead}
        playing={playing}
        loop={clock?.loop ?? null}
        snap={snap}
        selection={selection}
        proposal={timeline.proposal}
        drag={drag}
        recut={recut}
        editable={timeline.editable}
        maxLanes={maxLanes}
        snapMenu={menu}
        on={on}
      />
    </div>
  );
}
