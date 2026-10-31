import { useEffect, useState } from "react";
import { AgentsPanel } from "./AgentsPanel";
import type { DocumentView, History, Mode, TimelineView } from "./bridge";
import { inApp, listen, send } from "./bridge";
import { DocumentPanel } from "./DocumentPanel";
import { HistoryPanel } from "./HistoryPanel";
import { Icon } from "./Icon";
import { type LeftTab, PanelTabs } from "./PanelTabs";
import { PlaybackBar } from "./PlaybackBar";
import { ScenePanel } from "./scene/ScenePanel";
import { type ClockState, LiveTimeline } from "./timeline/LiveTimeline";
import { Viewport } from "./Viewport";

const EMPTY: History = { rows: [], current: null, status: null, turns: [] };

/** `?open=outliner` starts the left panel on the Outliner tab, as
 *  `?open=claude` starts a terminal. */
const startTab = (): LeftTab => ((new URLSearchParams(location.search).get("open") ?? "").split(",").includes("outliner") ? "Outliner" : "History");

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** The window: what is open has its panel on the left — a live world's
 *  history, with its own transport, and its outliner over the inspector in
 *  a tab beside it; or a document — the world is in the middle, the agents
 *  are on the right, and the timeline of the world's clock runs across the
 *  foot of all three. */
export function App() {
  const [mode, setMode] = useState<Mode>({ history: false, document: false });
  const [history, setHistory] = useState<History>(EMPTY);
  const [playing, setPlaying] = useState(false);
  const [title, setTitle] = useState<string | null>(null);
  const [opener, setOpener] = useState<{ busy: boolean; status: string | null }>({ busy: false, status: null });
  const [doc, setDoc] = useState<DocumentView | null>(null);
  const [timeline, setTimeline] = useState<TimelineView | null>(null);
  const [clock, setClock] = useState<ClockState | null>(null);
  const [thumbs, setThumbs] = useState<Record<string, string>>({});
  const [tab, setTab] = useState<LeftTab>(startTab);
  const [things, setThings] = useState(0);

  useEffect(() => {
    const stop = listen((m) => {
      switch (m.t) {
        case "mode":
          setMode({ history: m.history, document: m.document });
          break;
        case "history":
          setHistory(m);
          break;
        case "playback":
          setPlaying(m.playing);
          break;
        case "title":
          setTitle(m.text);
          break;
        case "opener":
          setOpener({ busy: m.busy, status: m.status });
          break;
        case "document":
          setDoc(m.doc);
          break;
        case "timeline":
          setTimeline(m.view);
          break;
        case "clock":
          setClock({ at: m.at, playing: m.playing, loop: m.loop, received: performance.now() });
          break;
        case "thumb":
          setThumbs((all) => ({ ...all, [m.id]: m.url }));
          break;
        case "scene":
          setThings(m.view?.entities.length ?? 0);
          break;
      }
    });
    send({ t: "ready" });
    return stop;
  }, []);

  const left = mode.document ? "420px" : mode.history ? "300px" : null;
  // A live world opens at launch: there is no Open… in it.
  const canOpen = !mode.history;
  const status = mode.history ? history.status : opener.status;

  return (
    <div
      className="grid h-full"
      style={{
        gridTemplateColumns: left ? `${left} minmax(0,1fr) 480px` : "minmax(0,1fr) 480px",
        gridTemplateRows: timeline ? "36px minmax(0,1fr) auto" : "36px minmax(0,1fr)",
      }}
    >
      <header className="col-span-full flex items-center gap-3 border-b border-line bg-panel px-4">
        <span className="font-semibold tracking-tight">LocalGPT</span>
        <span className="truncate text-muted">{title ?? "Nothing open"}</span>
        {canOpen && (
          <button
            disabled={opener.busy}
            onClick={() => send({ t: "open.file" })}
            title="A Markdown document, a song, or a world"
            className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-muted hover:bg-raised hover:text-ink disabled:opacity-40"
          >
            <Icon name="open" className="size-3.5" />
            Open…
          </button>
        )}
        <span className="ml-auto truncate text-faint">{status}</span>
        {!inApp() && <span className="shrink-0 rounded bg-head/15 px-1.5 text-[10px] text-head">browser preview</span>}
      </header>
      {mode.document && <DocumentPanel doc={doc} />}
      {!mode.document && mode.history && (
        <div className="flex min-h-0 flex-col border-r border-line bg-panel">
          <PanelTabs
            active={tab}
            count={tab === "History" ? count(history.rows.length, "entry", "entries") : count(things, "thing", "things")}
            choose={setTab}
          />
          {/* Both stay mounted: the outliner keeps what was opened and found. */}
          <div className={tab === "History" ? "flex min-h-0 flex-1 flex-col [&>aside]:flex-1 [&>aside]:border-r-0" : "hidden"}>
            <HistoryPanel history={history} bare />
            <PlaybackBar history={history} playing={playing} />
          </div>
          <div className={tab === "Outliner" ? "flex min-h-0 flex-1 flex-col" : "hidden"}>
            <ScenePanel className="flex-1" />
          </div>
        </div>
      )}
      <main className="flex min-h-0 flex-col">
        <Viewport />
      </main>
      <AgentsPanel />
      {/* Across the window: the timeline is the panel that wants width. */}
      {timeline && (
        <div className="col-span-full min-w-0 border-t border-line">
          <LiveTimeline timeline={timeline} clock={clock} thumbs={thumbs} />
        </div>
      )}
    </div>
  );
}
