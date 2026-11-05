import type { ReactNode } from "react";
import type { History } from "../bridge";
import { send } from "../bridge";
import { HistoryPanel } from "../HistoryPanel";
import { Icon } from "../Icon";
import { PlaybackBar } from "../PlaybackBar";
import { Frame } from "../timeline/Frame";
import type { Shot } from "../timeline/model";

/** The world as the window shows it, letterboxed in black: the newest
 *  frame, or `still` (a design sheet's drawing) until one comes. */
export function FrameBox({ frame, still, className = "" }: { frame: string | null; still?: ReactNode; className?: string }) {
  return (
    <div className={`relative grid place-items-center overflow-hidden bg-black ${className}`}>
      {frame ? (
        <img alt="The world, as the window shows it" src={frame} className="absolute inset-0 size-full object-contain" />
      ) : (
        (still ?? <span className="text-faint">Waiting for a frame…</span>)
      )}
    </div>
  );
}

/** The shots as tiles; a tap looks through the shot's camera. Two across
 *  at a phone's width, more where the box is wider. */
export function ShotGrid({ shots, thumbs }: { shots: Shot[]; thumbs: Record<string, string> }) {
  return (
    <div className="@container">
      <div className="grid grid-cols-2 gap-2 @lg:grid-cols-3 @3xl:grid-cols-4">
        {shots.map((shot) => (
          <button key={shot.id} onClick={() => send({ t: "shot.look", id: Number(shot.id) })} className="overflow-hidden rounded-md border border-line text-left active:border-accent">
            <div className="aspect-video bg-raised">
              {thumbs[shot.id] ? <img alt="" src={thumbs[shot.id]} className="size-full object-cover" /> : shot.look && <Frame look={shot.look} className="size-full" />}
            </div>
            <div className="truncate px-2 py-1">
              <span className="font-semibold">{shot.name}</span> <span className="text-faint">{shot.size}</span>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}

/** What the world pane shows under its frame on a tablet. */
export type RailTab = "History" | "Shots";

/** The world's pane on a tablet: the frame, and under it (beside it when
 *  the tablet is upright) the world's play button and a tab each for the
 *  history and the shots, the history first. A world with none of these
 *  is the frame alone. */
export function WorldPane({
  dir,
  frame,
  still,
  shots,
  thumbs,
  clock,
  history,
  playing,
  tab,
  onTab,
}: {
  /** `column` beside the terminals, `row` over them. */
  dir: "column" | "row";
  frame: string | null;
  still?: ReactNode;
  shots: Shot[];
  thumbs: Record<string, string>;
  /** Whether the world's clock plays, or null when it keeps none. */
  clock: boolean | null;
  /** A live world's history, or null when the world keeps none. */
  history: History | null;
  /** Whether the history plays. */
  playing: boolean;
  /** The tab picked; the first there is when none was, or it went. */
  tab: RailTab | null;
  onTab?: (tab: RailTab) => void;
}) {
  const tabs: RailTab[] = [];
  if (history) tabs.push("History");
  if (shots.length > 0) tabs.push("Shots");
  const active = tab && tabs.includes(tab) ? tab : (tabs[0] ?? null);
  const rail = tabs.length > 0 || clock !== null;
  const column = dir === "column";
  const box = column ? (rail ? "aspect-video max-h-[62%] w-full shrink-0" : "min-h-0 flex-1") : "min-h-0";
  // Beside the frame, the tabs, the play button and Free camera need 290 px.
  const band = rail ? "grid-cols-[minmax(0,1.4fr)_minmax(290px,1fr)]" : "";
  return (
    <section className={column ? "flex min-h-0 flex-col border-r border-line" : `grid min-h-0 grid-rows-[minmax(0,1fr)] border-b border-line ${band}`}>
      <FrameBox frame={frame} still={still} className={box} />
      {rail && (
        <div className={`flex min-h-0 flex-col ${column ? "flex-1 border-t border-line" : "border-l border-line"}`}>
          <div className="flex h-11 shrink-0 items-center gap-1 border-b border-line px-2">
            {clock !== null && (
              <button
                aria-label={clock ? "Pause the world" : "Play the world"}
                onClick={() => send({ t: "clock.play", playing: !clock })}
                className="mr-1 grid size-8 shrink-0 place-items-center rounded-full bg-raised text-ink active:bg-line"
              >
                <Icon name={clock ? "pause" : "play"} />
              </button>
            )}
            {tabs.length > 1 ? (
              <div role="tablist" className="flex items-center gap-1">
                {tabs.map((t) => (
                  <button key={t} role="tab" aria-selected={t === active} onClick={() => onTab?.(t)} className={`h-8 rounded-md px-3 ${t === active ? "bg-raised text-ink" : "text-muted"}`}>
                    {t}
                  </button>
                ))}
              </div>
            ) : (
              <span className="px-1 text-muted">{tabs[0] ?? (clock ? "Playing" : "Paused")}</span>
            )}
            {active === "Shots" && (
              <button onClick={() => send({ t: "shot.look", id: null })} className="ml-auto h-8 shrink-0 rounded-md px-2 text-accent active:bg-raised">
                Free camera
              </button>
            )}
            {active === "History" && history && (
              <span className="ml-auto shrink-0 pr-1 text-muted tabular-nums">
                {history.rows.length} {history.rows.length === 1 ? "entry" : "entries"}
              </span>
            )}
          </div>
          {active === "Shots" && (
            <div className="min-h-0 flex-1 overflow-y-auto p-2">
              <ShotGrid shots={shots} thumbs={thumbs} />
            </div>
          )}
          {active === "History" && history && (
            <div className="flex min-h-0 flex-1 flex-col [&>aside]:min-h-0 [&>aside]:flex-1 [&>aside]:border-r-0">
              <HistoryPanel history={history} bare />
              <PlaybackBar history={history} playing={playing} />
            </div>
          )}
        </div>
      )}
    </section>
  );
}
