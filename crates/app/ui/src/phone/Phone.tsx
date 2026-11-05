import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import type { History, Mode, TermTab, TimelineView } from "../bridge";
import { listen, send } from "../bridge";
import { HistoryPanel } from "../HistoryPanel";
import { Icon, type IconName } from "../Icon";
import { PlaybackBar } from "../PlaybackBar";
import { type Line, unpair, watchLine } from "../remote";
import { tabDot } from "../TerminalsPanel";
import type { Shot } from "../timeline/model";
import { Body, useLayout } from "./layout";
import { PhoneTerminal } from "./PhoneTerminal";
import { FrameBox, type RailTab, ShotGrid, WorldPane } from "./WorldPane";

export type View = "terminals" | "world" | "history";

type Proposal = NonNullable<TimelineView["proposal"]>;

const EMPTY: History = { rows: [], current: null, status: null };

/** The page on a paired phone or tablet: the window's, laid out for one
 *  hand. The terminals first — what runs in them is the reason to look —
 *  then the world as the window shows it, then the history. A tablet shows
 *  the world and the terminals at once (`layout.tsx`). Everything comes
 *  from the messages the window's page gets. */
export function Phone() {
  const [line, setLine] = useState<Line>({ state: "connecting", why: null, device: null });
  const [view, setView] = useState<View>("terminals");
  const [tabs, setTabs] = useState<TermTab[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [title, setTitle] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode>({ history: false, document: false });
  const [history, setHistory] = useState<History>(EMPTY);
  const [playing, setPlaying] = useState(false);
  const [timeline, setTimeline] = useState<TimelineView | null>(null);
  const [clockPlaying, setClockPlaying] = useState(false);
  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const [menu, setMenu] = useState(false);
  const [rail, setRail] = useState<RailTab | null>(null);
  const height = useVisualHeight();
  const layout = useLayout();
  const stage = useRef<HTMLDivElement>(null);
  const live = line.state === "live";

  useEffect(() => watchLine(setLine), []);
  // The app sends to a page that is ready: say so each time the line is up.
  useEffect(() => {
    if (live) send({ t: "ready" });
  }, [live]);

  useEffect(
    () =>
      listen((m) => {
        switch (m.t) {
          case "mode":
            setMode({ history: m.history, document: m.document });
            break;
          case "title":
            setTitle(m.text);
            break;
          case "history":
            setHistory(m);
            break;
          case "playback":
            setPlaying(m.playing);
            break;
          case "timeline":
            setTimeline(m.view);
            break;
          case "clock":
            setClockPlaying(m.playing);
            break;
          case "thumb":
            setThumbs((all) => ({ ...all, [m.id]: m.url }));
            break;
          case "term.list":
            setTabs(m.tabs);
            break;
          case "term.state":
            setTabs((all) => all.map((t) => (t.id === m.id ? { ...t, label: m.label } : t)));
            break;
        }
      }),
    [],
  );

  useEffect(() => {
    document.title = title ?? "LocalGPT";
  }, [title]);

  const shown = tabs.find((t) => t.id === active) ?? tabs[0] ?? null;
  const proposal = timeline?.proposal ?? null;
  const open = () => {
    const id = `p${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}`;
    // A first guess at what fits — the terminals' pane, less the key row,
    // or the page while another view hides it; the terminal says exactly
    // once it is drawn.
    const box = stage.current;
    const [w, h] = box?.clientWidth ? [box.clientWidth - 8, box.clientHeight - 60] : [innerWidth - 16, height - 200];
    send({ t: "term.open", id, cols: Math.max(20, Math.floor(w / 7.3)), rows: Math.max(8, Math.floor(h / 15)) });
    setActive(id);
    setView("terminals");
    setMenu(false);
  };
  const look = (id: string) => {
    setActive(id);
    setView("terminals");
  };
  const phone = layout === "phone";

  if (line.state === "unpaired" || line.state === "pairing") return <Unpaired line={line} height={height} />;

  return (
    <div className="flex flex-col overflow-hidden bg-panel text-[13px] text-ink" style={{ height }}>
      <PhoneHeader title={title} line={line} onMenu={() => setMenu(!menu)} />
      {menu && <Menu close={() => setMenu(false)} />}
      {proposal && <ProposalBar proposal={proposal} live={live} />}
      <Body
        layout={layout}
        view={view}
        terminals={
          <>
            {tabs.length > 0 && <TabChips tabs={tabs} active={shown?.id ?? null} onPick={look} onOpen={open} />}
            <div ref={stage} className="relative min-h-0 flex-1">
              {tabs.map((t) => (
                <PhoneTerminal key={t.id} tab={t} visible={(!phone || view === "terminals") && t.id === shown?.id} live={live} />
              ))}
              {tabs.length === 0 && <NoTabs onOpen={open} />}
            </div>
          </>
        }
        world={
          phone ? (
            view === "world" ? (
              <WorldView live={live} timeline={timeline} thumbs={thumbs} clockPlaying={clockPlaying} />
            ) : (
              view === "history" && <HistoryView mode={mode} history={history} playing={playing} />
            )
          ) : (
            <LiveWorldPane
              dir={layout === "side" ? "column" : "row"}
              live={live}
              timeline={timeline}
              thumbs={thumbs}
              clockPlaying={clockPlaying}
              history={mode.history ? history : null}
              playing={playing}
              tab={rail}
              onTab={setRail}
            />
          )
        }
      />
      {phone && <PhoneNav view={view} onView={setView} />}
    </div>
  );
}

/** The height the page has above the keyboard: a phone's browser lays the
 *  page out at full height and scrolls it when the keyboard opens. */
function useVisualHeight() {
  const [height, setHeight] = useState(() => window.visualViewport?.height ?? window.innerHeight);
  useEffect(() => {
    const viewport = window.visualViewport;
    if (!viewport) return;
    const update = () => {
      setHeight(viewport.height);
      window.scrollTo(0, 0);
    };
    viewport.addEventListener("resize", update);
    viewport.addEventListener("scroll", update);
    return () => {
      viewport.removeEventListener("resize", update);
      viewport.removeEventListener("scroll", update);
    };
  }, []);
  return height;
}

/** Where the line is, in a few words. */
export function lineText(line: Line): string {
  switch (line.state) {
    case "live":
      return line.device ?? "Connected";
    case "connecting":
    case "pairing":
      return "Connecting…";
    case "offline":
      return "Lost the computer — trying again";
    case "unpaired":
      return "Not paired";
  }
}

export function PhoneHeader({ title, line, onMenu }: { title: string | null; line: Line; onMenu?: () => void }) {
  const live = line.state === "live";
  return (
    <header className="flex h-11 shrink-0 items-center gap-2 border-b border-line bg-panel px-3 pt-[env(safe-area-inset-top)]">
      <span className={`size-2 shrink-0 rounded-full ${live ? "bg-[#9ece6a]" : line.state === "offline" ? "bg-danger" : "animate-pulse bg-faint"}`} />
      <div className="min-w-0 flex-1 leading-tight">
        <div className="truncate font-semibold">{title ?? "LocalGPT"}</div>
        <div className={`truncate text-[11px] ${live ? "text-faint" : "text-muted"}`}>{lineText(line)}</div>
      </div>
      <button aria-label="More" onClick={onMenu} className="grid size-9 place-items-center rounded-md text-muted active:bg-raised">
        <svg viewBox="0 0 16 16" className="size-4" aria-hidden="true">
          <circle cx="3.5" cy="8" r="1.25" fill="currentColor" />
          <circle cx="8" cy="8" r="1.25" fill="currentColor" />
          <circle cx="12.5" cy="8" r="1.25" fill="currentColor" />
        </svg>
      </button>
    </header>
  );
}

function Menu({ close }: { close: () => void }) {
  return (
    <div className="absolute top-12 right-2 z-30 w-60 rounded-md border border-line bg-raised py-1 shadow-xl">
      <button
        onClick={() => {
          close();
          unpair();
        }}
        className="block w-full px-3 py-2.5 text-left text-danger active:bg-line"
      >
        Forget this computer
      </button>
    </div>
  );
}

/** An agent's change waiting on the canvas: the person's to take or leave,
 *  as Enter and Esc are at the desk. */
export function ProposalBar({ proposal, live }: { proposal: Proposal; live: boolean }) {
  return (
    <div className="flex shrink-0 items-center gap-2 border-b border-line bg-head/10 px-3 py-2">
      <span className="min-w-0 flex-1 leading-tight">
        <span className="font-semibold">{proposal.author}</span> proposes
        <span className="block truncate text-muted">{proposal.what}</span>
      </span>
      <button disabled={!live} onClick={() => send({ t: "review", accept: false })} className="h-9 shrink-0 rounded-md border border-line px-3 active:bg-line disabled:opacity-40">
        Reject
      </button>
      <button disabled={!live} onClick={() => send({ t: "review", accept: true })} className="h-9 shrink-0 rounded-md bg-accent px-3 font-medium text-panel disabled:opacity-40">
        Accept
      </button>
    </div>
  );
}

/** The tabs as chips: each with its dot and what its program calls itself.
 *  Another terminal opens from the + . */
export function TabChips({ tabs, active, onPick, onOpen }: { tabs: TermTab[]; active: string | null; onPick?: (id: string) => void; onOpen?: () => void }) {
  return (
    <div className="relative flex shrink-0 items-center gap-1.5 overflow-x-auto border-b border-line px-2 py-1.5">
      {tabs.map((t) => (
        <button
          key={t.id}
          onClick={() => onPick?.(t.id)}
          className={`flex h-8 max-w-56 shrink-0 items-center gap-1.5 rounded-full border px-3 ${t.id === active ? "border-accent/60 bg-raised text-ink" : "border-line text-muted"}`}
        >
          <span className={`size-1.5 shrink-0 rounded-full ${tabDot(false)}`} />
          <span className="truncate">{t.label ?? "Terminal"}</span>
        </button>
      ))}
      <button aria-label="New terminal" onClick={() => onOpen?.()} className="grid size-8 shrink-0 place-items-center rounded-full border border-line text-muted active:bg-raised">
        <Icon name="plus" />
      </button>
    </div>
  );
}

function NoTabs({ onOpen }: { onOpen: () => void }) {
  return (
    <div className="grid h-full content-center p-6">
      <div className="mx-auto grid w-full max-w-[420px] gap-2">
        <p className="mb-2 text-center text-muted">No terminal is open in this world. Open one: it runs on the computer, in the world's folder, and your agent runs in it.</p>
        <button onClick={onOpen} className="rounded-md border border-line px-4 py-3 text-left active:bg-raised">
          New terminal
        </button>
      </div>
    </div>
  );
}

/** The world as the window shows it: a frame read back from the window,
 *  asked again as each arrives, for as long as what shows it is open. */
function useFrame(live: boolean): string | null {
  const [frame, setFrame] = useState<string | null>(null);
  useEffect(() => {
    if (!live) return;
    let next: number | undefined;
    const ask = () => {
      if (document.visibilityState === "visible") send({ t: "frame" });
    };
    const stop = listen((m) => {
      if (m.t !== "frame") return;
      setFrame(m.url);
      clearTimeout(next);
      next = window.setTimeout(ask, 500);
    });
    ask();
    // A frame that never came — the window hidden, the computer locked —
    // is asked for again; and at once when the page is looked at again.
    const watchdog = window.setInterval(ask, 6000);
    document.addEventListener("visibilitychange", ask);
    return () => {
      stop();
      clearTimeout(next);
      clearInterval(watchdog);
      document.removeEventListener("visibilitychange", ask);
    };
  }, [live]);
  return frame;
}

/** The shots on the world's timeline, to look through. */
function useShots(timeline: TimelineView | null): Shot[] {
  return useMemo(() => timeline?.tracks.flatMap((track) => (track.kind === "shots" ? track.shots : [])) ?? [], [timeline]);
}

/** A phone's World view: the frame, the song's play button, the shots. */
function WorldView({ live, timeline, thumbs, clockPlaying }: { live: boolean; timeline: TimelineView | null; thumbs: Record<string, string>; clockPlaying: boolean }) {
  const frame = useFrame(live);
  const shots = useShots(timeline);
  return <WorldFrame frame={frame} shots={shots} thumbs={thumbs} clock={timeline?.clock ? clockPlaying : null} />;
}

/** A tablet's world pane, beside or over the terminals; its frames are
 *  asked for while the page is open. */
function LiveWorldPane({
  dir,
  live,
  timeline,
  thumbs,
  clockPlaying,
  history,
  playing,
  tab,
  onTab,
}: {
  dir: "column" | "row";
  live: boolean;
  timeline: TimelineView | null;
  thumbs: Record<string, string>;
  clockPlaying: boolean;
  history: History | null;
  playing: boolean;
  tab: RailTab | null;
  onTab: (tab: RailTab) => void;
}) {
  const frame = useFrame(live);
  const shots = useShots(timeline);
  return (
    <WorldPane
      dir={dir}
      frame={frame}
      shots={shots}
      thumbs={thumbs}
      clock={timeline?.clock ? clockPlaying : null}
      history={history}
      playing={playing}
      tab={tab}
      onTab={onTab}
    />
  );
}

/** `clock`: whether the world's clock plays, or null when it keeps none. */
export function WorldFrame({ frame, shots, thumbs, clock, still }: { frame: string | null; shots: Shot[]; thumbs: Record<string, string>; clock: boolean | null; still?: ReactNode }) {
  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <FrameBox frame={frame} still={still} className="aspect-video w-full shrink-0" />
      {clock !== null && (
        <div className="flex shrink-0 items-center gap-2 border-b border-line px-3 py-2">
          <button
            aria-label={clock ? "Pause the world" : "Play the world"}
            onClick={() => send({ t: "clock.play", playing: !clock })}
            className="grid size-10 place-items-center rounded-full bg-raised text-ink active:bg-line"
          >
            <Icon name={clock ? "pause" : "play"} />
          </button>
          <span className="text-muted">{clock ? "Playing" : "Paused"}</span>
        </div>
      )}
      {shots.length > 0 && (
        <div className="shrink-0 px-3 py-2">
          <div className="mb-1.5 flex items-center">
            <span className="text-[11px] font-semibold tracking-wide text-faint uppercase">Shots</span>
            <button onClick={() => send({ t: "shot.look", id: null })} className="ml-auto rounded px-2 py-1 text-accent active:bg-raised">
              Free camera
            </button>
          </div>
          <ShotGrid shots={shots} thumbs={thumbs} />
        </div>
      )}
    </div>
  );
}

function HistoryView({ mode, history, playing }: { mode: Mode; history: History; playing: boolean }) {
  if (!mode.history)
    return <p className="p-6 text-center text-muted">A world package opened live keeps a history: what each agent changed, entry by entry. This world has none.</p>;
  return (
    <div className="flex h-full flex-col [&>aside]:min-h-0 [&>aside]:flex-1 [&>aside]:border-r-0">
      <HistoryPanel history={history} bare />
      <PlaybackBar history={history} playing={playing} />
    </div>
  );
}

const VIEWS: { view: View; label: string; icon: IconName }[] = [
  { view: "terminals", label: "Terminals", icon: "terminal" },
  { view: "world", label: "World", icon: "globe" },
  { view: "history", label: "History", icon: "history" },
];

/** The views, at the thumb. */
export function PhoneNav({ view, onView }: { view: View; onView?: (view: View) => void }) {
  return (
    <nav className="grid shrink-0 grid-cols-3 border-t border-line bg-panel pb-[env(safe-area-inset-bottom)]">
      {VIEWS.map((v) => (
        <button key={v.view} onClick={() => onView?.(v.view)} className={`relative flex h-14 flex-col items-center justify-center gap-0.5 ${view === v.view ? "text-accent" : "text-muted"}`}>
          <Icon name={v.icon} className="size-5" />
          <span className="text-[11px]">{v.label}</span>
        </button>
      ))}
    </nav>
  );
}

/** Not paired, or forgotten: how to pair. */
export function Unpaired({ line, height }: { line: Line; height?: number }) {
  return (
    <div className="grid content-center bg-panel p-8 text-[14px] text-ink" style={{ height: height ?? "100%" }}>
      <div className="mx-auto grid w-full max-w-[420px] gap-4">
        <Icon name="phone" className="size-8 text-accent" />
        <h1 className="text-[20px] font-semibold">{line.state === "pairing" ? "Pairing…" : "Pair this device"}</h1>
        <ol className="list-decimal space-y-1.5 pl-5 text-muted">
          <li>On the computer, in LocalGPT's top bar: Phone.</li>
          <li>Pair a device.</li>
          <li>Scan the code with this device's camera.</li>
        </ol>
        {line.why && <p className="text-faint">{line.why}</p>}
      </div>
    </div>
  );
}
