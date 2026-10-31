import { Icon, type IconName } from "../Icon";
import { timecode } from "../timecode";
import { Frame } from "./Frame";
import { areaPath, barBeat, beatGrid, linePath, type Scale, sectionStarts, ticks, xOf } from "./geometry";
import type { Clock, Shot, Track, TrackKind } from "./model";

/** The track headers' column; the lanes start after it. */
export const HEADER = 176;
const RULER = 40;
const HEIGHT: Record<TrackKind, number> = {
  shots: 56,
  camera: 26,
  soundtrack: 46,
  stem: 18,
  modulation: 30,
  behavior: 22,
  ambience: 22,
};
const GLYPH: Record<TrackKind, IconName> = {
  shots: "film",
  camera: "camera",
  soundtrack: "music",
  stem: "wave",
  modulation: "wave",
  behavior: "spin",
  ambience: "wind",
};
const TEXT: Record<TrackKind, string> = {
  shots: "text-shot",
  camera: "text-camera",
  soundtrack: "text-sound",
  stem: "text-sound",
  modulation: "text-mod",
  behavior: "text-behavior",
  ambience: "text-ambience",
};
const AUDIO = new Set<TrackKind>(["soundtrack", "stem", "ambience"]);

export type Snap = "beats" | "sections" | "shots" | "off";
const SNAP_LABEL: Record<Snap, string> = { beats: "Beats", sections: "Sections", shots: "Cuts", off: "Snap off" };

/** A drag while it is held: the clip's new place on its track, and the line
 *  it snapped to, if it did. Nothing is written until it is let go. */
export type Drag = { track: string; from: number; to: number; label: string; snapped?: { at: number; label: string } };

export type TimelineProps = {
  /** None: the world keeps no time — no soundtrack, no shots, no tour. */
  clock: Clock | null;
  tracks: Track[];
  /** The seconds in view. */
  view: [number, number];
  /** Px across the lanes; the headers add `HEADER`. */
  width: number;
  playhead: number;
  playing?: boolean;
  loop?: [number, number] | null;
  snap?: Snap;
  /** What is selected, as the toolbar says it. */
  selection?: { label: string; detail: string } | null;
  /** An agent's change waiting for the person: the toolbar asks. */
  proposal?: { author: string; what: string } | null;
  drag?: Drag | null;
  /** The lanes alone, without the toolbar. */
  bare?: boolean;
};

/** World time: the shots, the song and what the song moves, on one clock.
 *  Authoring time — who changed what, and when — is the history's. */
export function Timeline(props: TimelineProps) {
  const { clock, tracks, view, width, playhead, loop, drag, bare } = props;
  const s: Scale = { from: view[0], to: view[1], width };
  const lanes = tracks.reduce((sum, track) => sum + HEIGHT[track.kind], 0);
  const at = (t: number) => HEADER + xOf(s, t);
  const inView = (t: number) => t >= view[0] && t <= view[1];
  if (clock === null) {
    // Nothing to play: one quiet line, and the world gets the height back.
    return (
      <div className="flex h-8 items-center gap-2 bg-panel px-3 text-[11px]" style={{ width: HEADER + width }}>
        <Icon name="film" className="size-3.5 text-faint" />
        <span className="text-muted">This world keeps no time yet.</span>
        <span className="truncate text-faint">A soundtrack, a shot list or a tour gives it a clock; its behaviors loop without one.</span>
      </div>
    );
  }
  return (
    <div className="flex flex-col bg-panel text-[11px] text-ink" style={{ width: HEADER + width }}>
      {!bare && <Toolbar {...props} />}
      <div className="relative">
        <div className="flex border-b border-line">
          <div className="shrink-0 border-r border-line" style={{ width: HEADER, height: RULER }} />
          <Ruler clock={clock} s={s} loop={loop ?? null} />
        </div>
        <div className="relative">
          <Grid clock={clock} s={s} height={lanes} loop={loop ?? null} />
          {tracks.map((track) => (
            <div key={track.id} className="flex border-b border-line/60">
              <TrackHeader track={track} />
              <Lane track={track} s={s} clock={clock} drag={drag?.track === track.id ? drag : null} />
            </div>
          ))}
        </div>
        {drag?.snapped && inView(drag.snapped.at) && (
          <div className="pointer-events-none absolute top-0 bottom-0 border-l border-dashed border-head" style={{ left: at(drag.snapped.at) }}>
            <span className="absolute top-0.5 left-1 rounded bg-head px-1 text-[10px] whitespace-nowrap text-panel">{drag.snapped.label}</span>
          </div>
        )}
        {inView(playhead) && (
          <div className="pointer-events-none absolute top-[14px] bottom-0 w-px bg-playhead" style={{ left: at(playhead) }}>
            <svg className="absolute -top-px -left-[5px]" width="11" height="12" viewBox="0 0 11 12">
              <path d="M0 0h11v7l-5.5 5L0 7z" fill="var(--color-playhead)" />
            </svg>
          </div>
        )}
      </div>
    </div>
  );
}

function Toolbar({ clock, view, playhead, playing, loop, snap = "off", selection, proposal }: TimelineProps) {
  const bb = clock && barBeat(clock, playhead);
  const span = view[1] - view[0];
  const zoom = clock ? Math.min(1, Math.max(0.04, Math.log2(clock.duration / span) / 5)) : 0;
  return (
    <div className="@container flex h-8 shrink-0 items-center gap-0.5 border-b border-line px-1.5 whitespace-nowrap">
      <Tool icon="first" label="Previous cut" />
      <Tool icon={playing ? "pause" : "play"} label={playing ? "Pause" : "Play"} />
      <Tool icon="last" label="Next cut" />
      <Tool icon="loop" label="Loop" on={!!loop} />
      <span className="ml-2 shrink-0 font-mono tabular-nums">
        <span className="text-ink">{timecode(playhead, true)}</span>
        <span className="text-faint"> / {timecode(clock?.duration ?? 0, true)}</span>
      </span>
      {bb && (
        <span className="ml-3 shrink-0 text-muted tabular-nums">
          bar {bb[0]} · {bb[1]}
        </span>
      )}
      {/* What the person is asked, or what they hold: it gives way first. */}
      <span className="mx-3 flex min-w-0 flex-1">
        {proposal ? (
          <span className="flex min-w-0 items-center gap-1.5 overflow-hidden rounded border border-dashed border-head/60 bg-head/[0.08] py-px pr-px pl-2">
            <span className="shrink-0 text-head">{proposal.author} proposes</span>
            <span className="shrink-0 rounded-sm bg-head px-1.5 text-panel">Accept ⏎</span>
            <span className="shrink-0 rounded-sm px-1 text-muted">Reject ⎋</span>
            <span className="min-w-0 truncate pr-1.5 text-ink">{proposal.what}</span>
          </span>
        ) : (
          selection && (
            <span className="flex min-w-0 items-center gap-1.5 overflow-hidden rounded bg-raised px-2 py-0.5">
              <Icon name="film" className="size-3.5 shrink-0 text-shot" />
              <span className="shrink-0 text-ink">{selection.label}</span>
              <span className="min-w-0 truncate text-muted">{selection.detail}</span>
            </span>
          )
        )}
      </span>
      <span className="flex shrink-0 items-center gap-0.5">
        <span className={`flex h-6 items-center gap-1 rounded px-1.5 ${snap === "off" ? "text-faint" : "bg-accent/10 text-accent"}`}>
          <Icon name="magnet" className="size-3.5" />
          {SNAP_LABEL[snap]}
          <Icon name="chevronDown" className="size-3" />
        </span>
        {/* In a narrow window the zoom gives way; a pinch still zooms. */}
        <span className="flex items-center gap-0.5 @max-2xl:hidden">
          <Tool icon="zoomOut" label="Zoom out" />
          <span className="h-1 w-14 rounded bg-line">
            <span className="block h-1 rounded bg-muted" style={{ width: `${zoom * 100}%` }} />
          </span>
          <Tool icon="zoomIn" label="Zoom in" />
          <span className="ml-1 rounded px-1.5 py-0.5 text-muted">Fit</span>
        </span>
      </span>
    </div>
  );
}

function Tool({ icon, label, on }: { icon: IconName; label: string; on?: boolean }) {
  return (
    <span title={label} className={`grid size-6 place-items-center rounded ${on ? "bg-accent/10 text-accent" : "text-muted"}`}>
      <Icon name={icon} className="size-3.5" />
    </span>
  );
}

/** Time along the bottom, bar numbers above it, the song's sections on top,
 *  and the loop as a band. */
function Ruler({ clock, s, loop }: { clock: Clock; s: Scale; loop: [number, number] | null }) {
  const { major, minor } = ticks(s);
  const starts = sectionStarts(clock);
  const { bars } = beatGrid(clock, s);
  return (
    <div className="relative overflow-hidden" style={{ width: s.width, height: RULER }}>
      {/* One section is no form at all: a song without sections, or no song. */}
      {starts.length > 1 &&
        starts.map((t, i) => {
        const left = xOf(s, t);
        const right = xOf(s, starts[i + 1] ?? clock.duration);
        return (
          <div
            key={t}
            className={`absolute top-0 h-3.5 border-l border-line pl-1 text-[9px] leading-[14px] text-faint ${i % 2 ? "bg-raised" : ""}`}
            style={{ left, width: right - left }}
          >
            §{i + 1}
          </div>
        );
      })}
      {loop && <div className="absolute top-3.5 bottom-0 border-x border-accent/70 bg-accent/15" style={{ left: xOf(s, loop[0]), width: xOf(s, loop[1]) - xOf(s, loop[0]) }} />}
      {bars
        .filter((bar) => bar.label)
        .map((bar) => (
          <span key={bar.n} className="absolute top-[15px] pl-0.5 text-[9px] leading-none text-faint tabular-nums" style={{ left: xOf(s, bar.t) }}>
            {bar.n}
          </span>
        ))}
      <svg className="absolute bottom-0 left-0" width={s.width} height={14}>
        {bars.map((bar) => (
          <line key={bar.n} x1={xOf(s, bar.t)} x2={xOf(s, bar.t)} y1={8} y2={14} stroke="var(--color-line)" />
        ))}
        {minor.map((t) => (
          <line key={t} x1={xOf(s, t)} x2={xOf(s, t)} y1={10} y2={14} stroke="var(--color-faint)" strokeOpacity={0.6} />
        ))}
        {major.map((t) => (
          <line key={t} x1={xOf(s, t)} x2={xOf(s, t)} y1={3} y2={14} stroke="var(--color-muted)" />
        ))}
      </svg>
      {major.map((t) => (
        <span key={t} className="absolute bottom-[3px] pl-1 text-[10px] leading-none text-muted tabular-nums" style={{ left: xOf(s, t) }}>
          {timecode(t)}
        </span>
      ))}
    </div>
  );
}

/** Bars, beats and sections behind every lane, so a cut can be seen to land
 *  on the music. */
function Grid({ clock, s, height, loop }: { clock: Clock; s: Scale; height: number; loop: [number, number] | null }) {
  const { bars, beats } = beatGrid(clock, s);
  return (
    <svg className="pointer-events-none absolute top-0" style={{ left: HEADER }} width={s.width} height={height}>
      {loop && <rect x={xOf(s, loop[0])} width={xOf(s, loop[1]) - xOf(s, loop[0])} height={height} fill="var(--color-accent)" fillOpacity={0.04} />}
      {beats.map((t) => (
        <line key={t} x1={xOf(s, t)} x2={xOf(s, t)} y2={height} stroke="var(--color-line)" strokeOpacity={0.35} />
      ))}
      {bars.map((bar) => (
        <line key={bar.n} x1={xOf(s, bar.t)} x2={xOf(s, bar.t)} y2={height} stroke="var(--color-line)" strokeOpacity={0.8} />
      ))}
      {sectionStarts(clock).map((t) => (
        <line key={t} x1={xOf(s, t)} x2={xOf(s, t)} y2={height} stroke="var(--color-faint)" strokeOpacity={0.55} />
      ))}
    </svg>
  );
}

function subtitle(track: Track): string | null {
  switch (track.kind) {
    case "shots":
      return `${track.shots.length} shots`;
    case "camera":
      return `tour · ${track.tour}`;
    case "soundtrack":
      return `${track.title} — ${track.artist}`;
    case "modulation":
      return `${track.target} ← ${track.signal} · ${track.range[0]}–${track.range[1]}`;
    default:
      return null;
  }
}

function TrackHeader({ track }: { track: Track }) {
  const h = HEIGHT[track.kind];
  const flags = track.flags ?? {};
  const sub = h >= 26 ? subtitle(track) : null;
  const quiet = flags.hidden || flags.muted;
  return (
    <div
      className="flex shrink-0 items-center gap-1.5 border-r border-line bg-panel pr-1.5"
      style={{ width: HEADER, height: h, paddingLeft: track.kind === "stem" ? 30 : 8 }}
    >
      {track.kind === "soundtrack" && <Icon name={track.open ? "chevronDown" : "chevronRight"} className="-mr-1 -ml-1 size-3 shrink-0 text-faint" />}
      <Icon name={GLYPH[track.kind]} className={`size-3.5 shrink-0 ${TEXT[track.kind]} ${quiet ? "opacity-40" : ""}`} />
      <span className="min-w-0 flex-1 leading-tight">
        <span className={`block truncate ${quiet ? "text-faint" : track.kind === "stem" ? "text-muted" : "text-ink"}`}>{track.name}</span>
        {sub && <span className="block truncate text-[10px] text-faint">{sub}</span>}
      </span>
      <span className="flex shrink-0 items-center">
        {AUDIO.has(track.kind) ? (
          <>
            <span className={`grid size-4 place-items-center rounded-sm text-[9px] font-semibold ${flags.solo ? "bg-head text-panel" : "text-faint/70"}`}>S</span>
            <Flag icon={flags.muted ? "mute" : "speaker"} on={flags.muted} tone="text-danger" />
          </>
        ) : (
          <Flag icon={flags.hidden ? "hidden" : "eye"} on={flags.hidden} tone="text-muted" />
        )}
        <Flag icon="lock" on={flags.locked} tone="text-ink" />
      </span>
    </div>
  );
}

function Flag({ icon, on, tone }: { icon: IconName; on?: boolean; tone: string }) {
  return (
    <span className={`grid size-4 place-items-center ${on ? tone : "text-faint/50"}`}>
      <Icon name={icon} className="size-3" />
    </span>
  );
}

function Lane({ track, s, clock, drag }: { track: Track; s: Scale; clock: Clock; drag: Drag | null }) {
  const h = HEIGHT[track.kind];
  const flags = track.flags ?? {};
  return (
    <div className={`relative overflow-hidden ${flags.hidden || flags.muted ? "opacity-35" : ""}`} style={{ width: s.width, height: h }}>
      <LaneBody track={track} s={s} h={h} clock={clock} />
      {drag && (
        <div
          className="absolute top-1 bottom-1 flex items-end rounded-[3px] border border-dashed border-accent bg-accent/10 p-1 text-[10px] whitespace-nowrap text-accent"
          style={{ left: xOf(s, drag.from) + 1, width: xOf(s, drag.to) - xOf(s, drag.from) - 2 }}
        >
          <span className="rounded-sm bg-panel/85 px-1">{drag.label}</span>
        </div>
      )}
      {flags.locked && (
        <div
          className="pointer-events-none absolute inset-0"
          style={{ backgroundImage: "repeating-linear-gradient(135deg, #ffffff0d 0 5px, transparent 5px 11px)" }}
        />
      )}
    </div>
  );
}

function LaneBody({ track, s, h, clock }: { track: Track; s: Scale; h: number; clock: Clock }) {
  switch (track.kind) {
    case "shots":
      return <Shots shots={track.shots} s={s} />;
    case "camera": {
      const y = h / 2;
      const keys = track.keys;
      return (
        <svg className="absolute inset-0" width={s.width} height={h}>
          {keys.slice(1).map((key, i) => (
            <line key={key.t} x1={xOf(s, keys[i].t + (keys[i].hold ?? 0))} x2={xOf(s, key.t)} y1={y} y2={y} stroke="var(--color-camera)" strokeOpacity={0.45} />
          ))}
          {keys.map(
            (key) =>
              key.hold && <rect key={`h${key.t}`} x={xOf(s, key.t)} y={y - 2.5} width={xOf(s, key.t + key.hold) - xOf(s, key.t)} height={5} rx={2.5} fill="var(--color-camera)" fillOpacity={0.55} />,
          )}
          {keys.map((key) => (
            <rect key={key.t} x={xOf(s, key.t) - 3.5} y={y - 3.5} width={7} height={7} transform={`rotate(45 ${xOf(s, key.t)} ${y})`} fill="var(--color-camera)" />
          ))}
        </svg>
      );
    }
    case "soundtrack":
      return (
        <>
          <div className="absolute top-1 bottom-1 rounded-[3px] border border-sound/30 bg-sound/[0.07]" style={{ left: xOf(s, 0) + 1, width: xOf(s, clock.duration) - xOf(s, 0) - 2 }} />
          <svg className="absolute inset-0" width={s.width} height={h}>
            <path d={areaPath(track.peaks, s, h, true)} fill="var(--color-sound)" fillOpacity={0.5} />
          </svg>
          <span className="absolute top-1.5 left-2 flex items-center gap-1 rounded bg-panel/75 px-1 text-[10px] leading-4" style={{ left: Math.max(4, xOf(s, 0) + 6) }}>
            <span className="text-ink">{track.title}</span>
            <span className="text-muted">{track.artist}</span>
            <span className="rounded-sm bg-sound/20 px-1 text-[9px] text-sound">{track.licence}</span>
          </span>
        </>
      );
    case "stem":
      return (
        <svg className="absolute inset-0" width={s.width} height={h}>
          <path d={areaPath(track.envelope, s, h, false)} fill="var(--color-sound)" fillOpacity={0.3} />
        </svg>
      );
    case "modulation":
      return (
        <svg className="absolute inset-0" width={s.width} height={h}>
          <path d={`${linePath(track.samples, s, h)}V${h}H${xOf(s, track.samples.from ?? 0)}Z`} fill="var(--color-mod)" fillOpacity={0.12} />
          <path d={linePath(track.samples, s, h)} fill="none" stroke="var(--color-mod)" strokeWidth={1.25} />
        </svg>
      );
    case "behavior": {
      const left = xOf(s, track.from);
      const width = xOf(s, track.to) - left;
      const cycles = track.period * (s.width / (s.to - s.from)) >= 5 ? Math.floor((track.to - track.from) / track.period) : 0;
      return (
        <div className="absolute top-1 bottom-1 overflow-hidden rounded-[3px] border border-behavior/35 bg-behavior/[0.1]" style={{ left: left + 1, width: width - 2 }}>
          <svg className="absolute inset-0" width={width} height={h - 8}>
            {Array.from({ length: cycles }, (_, i) => {
              const cx = xOf(s, track.from + (i + 1) * track.period) - left;
              return <line key={i} x1={cx} x2={cx} y1={3} y2={h - 11} stroke="var(--color-behavior)" strokeOpacity={0.35} />;
            })}
          </svg>
          <span className="absolute top-0 left-1.5 leading-[12px] text-[10px] text-behavior">
            {track.what} · every {track.period} s
          </span>
        </div>
      );
    }
    case "ambience": {
      const [a, b] = [xOf(s, track.from), xOf(s, track.to)];
      const [fi, fo] = [xOf(s, track.from + track.fade[0]) - a, b - xOf(s, track.to - track.fade[1])];
      return (
        <>
          <svg className="absolute inset-0" width={s.width} height={h}>
            <path d={`M${a} ${h - 3}L${a + fi} 3H${b - fo}L${b} ${h - 3}Z`} fill="var(--color-ambience)" fillOpacity={0.16} stroke="var(--color-ambience)" strokeOpacity={0.45} />
          </svg>
          <span className="absolute top-[4px] leading-[12px] text-[10px] text-ambience" style={{ left: a + fi + 4 }}>
            {track.name} bed
          </span>
        </>
      );
    }
  }
}

function seconds(span: number) {
  return span < 60 ? `${span.toFixed(1)} s` : timecode(span);
}

function Shots({ shots, s }: { shots: Shot[]; s: Scale }) {
  const gaps = shots.slice(1).flatMap((shot, i) => (shot.from - shots[i].to > 0.5 ? [[shots[i].to, shot.from]] : []));
  return (
    <>
      {gaps.map(([from, to]) => (
        <div
          key={from}
          className="absolute top-1 bottom-1 grid place-items-center rounded-[3px] border border-dashed border-line text-[10px] text-faint"
          style={{ left: xOf(s, from) + 1, width: xOf(s, to) - xOf(s, from) - 2 }}
        >
          {xOf(s, to) - xOf(s, from) > 44 && seconds(to - from)}
        </div>
      ))}
      {shots.map((shot) => (
        <ShotClip key={shot.id} shot={shot} s={s} />
      ))}
    </>
  );
}

const CLIP: Record<NonNullable<Shot["state"]>, string> = {
  idle: "border-shot/40 bg-shot/[0.12]",
  hover: "border-shot/80 bg-shot/[0.2]",
  selected: "border-accent bg-shot/[0.24] ring-1 ring-accent/50",
  proposed: "border-dashed border-head bg-head/[0.08]",
  locked: "border-line bg-raised",
};

function ShotClip({ shot, s }: { shot: Shot; s: Scale }) {
  const left = xOf(s, shot.from);
  const width = xOf(s, shot.to) - left - 2;
  const state = shot.state ?? "idle";
  const thumb = width >= 120 ? Math.min(74, width * 0.42) : 0;
  return (
    <div className={`absolute top-1 bottom-1 flex overflow-hidden rounded-[3px] border ${CLIP[state]}`} style={{ left: left + 1, width: Math.max(2, width) }}>
      {thumb > 0 && (
        <span className="block h-full shrink-0" style={{ width: thumb }}>
          <Frame look={shot.look} className={`size-full ${state === "locked" ? "opacity-50 grayscale" : ""}`} />
        </span>
      )}
      <span className="min-w-0 flex-1 px-1.5 py-1 leading-tight" style={{ maxWidth: width - thumb }}>
        <span className="flex items-baseline gap-1">
          <span className={`font-semibold ${state === "locked" ? "text-muted" : "text-ink"}`}>{shot.name}</span>
          {width - thumb > 74 && (
            <span className="truncate text-muted">
              {shot.size} · {shot.lens} mm
            </span>
          )}
        </span>
        {width - thumb > 40 && <span className="block text-faint tabular-nums">{seconds(shot.to - shot.from)}</span>}
      </span>
      {state === "proposed" && shot.author && (
        <span className="absolute right-1 bottom-1 rounded-sm bg-head/20 px-1 text-[9px] leading-[13px] text-head">{shot.author}</span>
      )}
      {state === "selected" && (
        <>
          <span className="absolute inset-y-0 left-0 w-1 bg-accent" />
          <span className="absolute inset-y-0 right-0 w-1 bg-accent" />
        </>
      )}
      {state === "locked" && <Icon name="lock" className="absolute top-1 right-1 size-3 text-faint" />}
    </div>
  );
}
