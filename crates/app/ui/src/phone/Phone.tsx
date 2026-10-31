import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { activityDot, KINDS, kindLabel } from "../AgentsPanel";
import type { Activity, History, Mode, TermTab, TimelineView } from "../bridge";
import { listen, send } from "../bridge";
import { HistoryPanel } from "../HistoryPanel";
import { Icon, type IconName } from "../Icon";
import { PlaybackBar } from "../PlaybackBar";
import { type Line, unpair, watchLine } from "../remote";
import { Frame } from "../timeline/Frame";
import type { Shot } from "../timeline/model";
import { PhoneTerminal } from "./PhoneTerminal";

export type View = "agents" | "world" | "history";

/** An agent that stopped: done, or asking the person something. */
export type Turn = { id: string; kind: string; activity: Activity | null; task: string | null };

type Proposal = NonNullable<TimelineView["proposal"]>;

const EMPTY: History = { rows: [], current: null, status: null, turns: [] };

/** The page on a paired phone or tablet: the window's, laid out for one
 *  hand. The agents first — the one that finished or is asking is the
 *  reason to look — then the world as the window shows it, then the
 *  history. Everything comes from the messages the window's page gets. */
export function Phone() {
  const [line, setLine] = useState<Line>({ state: "connecting", why: null, device: null });
  const [view, setView] = useState<View>("agents");
  const [tabs, setTabs] = useState<TermTab[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [title, setTitle] = useState<string | null>(null);
  const [mode, setMode] = useState<Mode>({ history: false, document: false });
  const [history, setHistory] = useState<History>(EMPTY);
  const [playing, setPlaying] = useState(false);
  const [timeline, setTimeline] = useState<TimelineView | null>(null);
  const [clockPlaying, setClockPlaying] = useState(false);
  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const [turn, setTurn] = useState<Turn | null>(null);
  const [menu, setMenu] = useState(false);
  const height = useVisualHeight();
  const live = line.state === "live";
  /** Each tab's last activity and kind, read in the listener. */
  const known = useRef(new Map<string, { kind: string; activity: Activity | null }>());

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
            for (const t of m.tabs) if (!known.current.has(t.id)) known.current.set(t.id, { kind: t.kind, activity: t.activity });
            setTabs(m.tabs);
            break;
          case "term.state": {
            const was = known.current.get(m.id);
            if (was) known.current.set(m.id, { ...was, activity: m.activity });
            setTabs((all) => all.map((t) => (t.id === m.id ? { ...t, label: m.label, activity: m.activity } : t)));
            // A turn that ended: worth a look, and a buzz where the phone can.
            if (was?.activity === "working" && (m.activity === "idle" || m.activity === "asking")) {
              setTurn({ id: m.id, kind: was.kind, activity: m.activity, task: m.label });
              navigator.vibrate?.(60);
            }
            break;
          }
        }
      }),
    [],
  );

  // A turn's note goes after a while, or once its tab is looked at.
  useEffect(() => {
    if (!turn) return;
    const timer = window.setTimeout(() => setTurn(null), 12000);
    return () => clearTimeout(timer);
  }, [turn]);
  useEffect(() => {
    document.title = turn ? `● ${title ?? "LocalGPT"}` : (title ?? "LocalGPT");
  }, [turn, title]);

  const shown = tabs.find((t) => t.id === active) ?? tabs[0] ?? null;
  const proposal = timeline?.proposal ?? null;
  const open = (kind: string) => {
    const id = `p${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}`;
    // A first guess at what fits; the terminal says exactly once it is drawn.
    send({ t: "term.open", id, kind, cols: Math.max(20, Math.floor((innerWidth - 16) / 7.3)), rows: Math.max(8, Math.floor((height - 200) / 15)) });
    setActive(id);
    setView("agents");
    setMenu(false);
  };
  const look = (id: string) => {
    setActive(id);
    setView("agents");
    setTurn(null);
  };

  if (line.state === "unpaired" || line.state === "pairing") return <Unpaired line={line} height={height} />;

  return (
    <div className="flex flex-col overflow-hidden bg-panel text-[13px] text-ink" style={{ height }}>
      <PhoneHeader title={title} line={line} onMenu={() => setMenu(!menu)} />
      {menu && <Menu close={() => setMenu(false)} />}
      {turn && <TurnNote turn={turn} onLook={() => look(turn.id)} />}
      {proposal && <ProposalBar proposal={proposal} live={live} />}
      <main className="relative min-h-0 flex-1">
        <div className={view === "agents" ? "flex h-full flex-col" : "hidden"}>
          {tabs.length > 0 && <TabChips tabs={tabs} active={shown?.id ?? null} onPick={look} onOpen={open} />}
          <div className="relative min-h-0 flex-1">
            {tabs.map((t) => (
              <PhoneTerminal key={t.id} tab={t} visible={view === "agents" && t.id === shown?.id} live={live} />
            ))}
            {tabs.length === 0 && <NoTabs onOpen={open} />}
          </div>
        </div>
        {view === "world" && (
          <WorldView live={live} timeline={timeline} thumbs={thumbs} clockPlaying={clockPlaying} />
        )}
        {view === "history" && <HistoryView mode={mode} history={history} playing={playing} />}
      </main>
      <PhoneNav view={view} tabs={tabs} onView={setView} />
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

/** The agent that stopped, over whatever is shown. */
export function TurnNote({ turn, onLook }: { turn: Turn; onLook?: () => void }) {
  const asking = turn.activity === "asking";
  return (
    <button onClick={onLook} className="flex shrink-0 items-center gap-2 border-b border-line bg-raised px-3 py-2 text-left active:bg-line">
      <span className={`size-2 shrink-0 rounded-full ${asking ? "bg-head" : "bg-[#9ece6a]"}`} />
      <span className="min-w-0 flex-1">
        <span className="font-semibold">{kindLabel(turn.kind)}</span> {asking ? "is asking you" : "is done"}
        {turn.task && <span className="block truncate text-muted">{turn.task}</span>}
      </span>
      <span className="shrink-0 text-accent">Show</span>
    </button>
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

/** The tabs as chips: what each runs, its dot, and what its agent calls the
 *  task. Another agent starts from the + . */
export function TabChips({ tabs, active, onPick, onOpen }: { tabs: TermTab[]; active: string | null; onPick?: (id: string) => void; onOpen?: (kind: string) => void }) {
  const [menu, setMenu] = useState(false);
  return (
    <div className="relative flex shrink-0 items-center gap-1.5 overflow-x-auto border-b border-line px-2 py-1.5">
      {tabs.map((t) => (
        <button
          key={t.id}
          onClick={() => onPick?.(t.id)}
          className={`flex h-8 max-w-56 shrink-0 items-center gap-1.5 rounded-full border px-3 ${t.id === active ? "border-accent/60 bg-raised text-ink" : "border-line text-muted"}`}
        >
          <span className={`size-1.5 shrink-0 rounded-full ${activityDot(t.activity)}`} />
          <span className="shrink-0">{kindLabel(t.kind)}</span>
          {t.label && t.label !== kindLabel(t.kind) && <span className="truncate text-faint">{t.label}</span>}
        </button>
      ))}
      <button aria-label="Start an agent" onClick={() => setMenu(!menu)} className="grid size-8 shrink-0 place-items-center rounded-full border border-line text-muted active:bg-raised">
        <Icon name="plus" />
      </button>
      {menu && (
        <div className="fixed inset-x-2 top-28 z-30 rounded-md border border-line bg-raised py-1 shadow-xl">
          {KINDS.map((k) => (
            <button
              key={k.kind}
              onClick={() => {
                setMenu(false);
                onOpen?.(k.kind);
              }}
              className="block w-full px-4 py-3 text-left active:bg-line"
            >
              {k.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function NoTabs({ onOpen }: { onOpen: (kind: string) => void }) {
  return (
    <div className="grid h-full content-center gap-2 p-6">
      <p className="mb-2 text-center text-muted">No agent is running in this world. Start one: it runs on the computer, in the world's folder.</p>
      {KINDS.map((k) => (
        <button key={k.kind} onClick={() => onOpen(k.kind)} className="rounded-md border border-line px-4 py-3 text-left active:bg-raised">
          {k.label}
        </button>
      ))}
    </div>
  );
}

/** The world as the window shows it: a frame read back from the window,
 *  asked again as each arrives while this view is open. The shots look
 *  through their cameras; the song plays and pauses. */
function WorldView({ live, timeline, thumbs, clockPlaying }: { live: boolean; timeline: TimelineView | null; thumbs: Record<string, string>; clockPlaying: boolean }) {
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
    // is asked for again.
    const watchdog = window.setInterval(ask, 6000);
    return () => {
      stop();
      clearTimeout(next);
      clearInterval(watchdog);
    };
  }, [live]);
  const shots = useMemo(() => timeline?.tracks.flatMap((track) => (track.kind === "shots" ? track.shots : [])) ?? [], [timeline]);
  return <WorldFrame frame={frame} shots={shots} thumbs={thumbs} clock={timeline?.clock ? clockPlaying : null} />;
}

/** `clock`: whether the world's clock plays, or null when it keeps none. */
export function WorldFrame({ frame, shots, thumbs, clock, still }: { frame: string | null; shots: Shot[]; thumbs: Record<string, string>; clock: boolean | null; still?: ReactNode }) {
  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <div className="grid aspect-video w-full shrink-0 place-items-center bg-black">
        {frame ? <img alt="The world, as the window shows it" src={frame} className="max-h-full max-w-full object-contain" /> : (still ?? <span className="text-faint">Waiting for a frame…</span>)}
      </div>
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
          <div className="grid grid-cols-2 gap-2">
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
  { view: "agents", label: "Agents", icon: "terminal" },
  { view: "world", label: "World", icon: "globe" },
  { view: "history", label: "History", icon: "history" },
];

/** The views, at the thumb. The agents' dot is the one that most wants the
 *  person: one asking, else one working. */
export function PhoneNav({ view, tabs, onView }: { view: View; tabs: TermTab[]; onView?: (view: View) => void }) {
  const asking = tabs.some((t) => t.activity === "asking");
  const working = tabs.some((t) => t.activity === "working");
  return (
    <nav className="grid shrink-0 grid-cols-3 border-t border-line bg-panel pb-[env(safe-area-inset-bottom)]">
      {VIEWS.map((v) => (
        <button key={v.view} onClick={() => onView?.(v.view)} className={`relative flex h-14 flex-col items-center justify-center gap-0.5 ${view === v.view ? "text-accent" : "text-muted"}`}>
          <Icon name={v.icon} className="size-5" />
          <span className="text-[11px]">{v.label}</span>
          {v.view === "agents" && (asking || working) && <span className={`absolute top-2 right-[calc(50%-18px)] size-2 rounded-full ${asking ? "bg-head" : "animate-pulse bg-accent"}`} />}
        </button>
      ))}
    </nav>
  );
}

/** Not paired, or forgotten: how to pair. */
export function Unpaired({ line, height }: { line: Line; height?: number }) {
  return (
    <div className="grid content-center gap-4 bg-panel p-8 text-[14px] text-ink" style={{ height: height ?? "100%" }}>
      <Icon name="phone" className="size-8 text-accent" />
      <h1 className="text-[20px] font-semibold">{line.state === "pairing" ? "Pairing…" : "Pair this device"}</h1>
      <ol className="list-decimal space-y-1.5 pl-5 text-muted">
        <li>On the computer, in LocalGPT's top bar: Phone.</li>
        <li>Pair a device.</li>
        <li>Scan the code with this device's camera.</li>
      </ol>
      {line.why && <p className="text-faint">{line.why}</p>}
    </div>
  );
}
