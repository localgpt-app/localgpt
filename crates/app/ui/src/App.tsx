import { useEffect, useState } from "react";
import { AgentsPanel } from "./AgentsPanel";
import type { DocumentView, History, Mode, NowPlaying } from "./bridge";
import { inApp, listen, send } from "./bridge";
import { DocumentPanel } from "./DocumentPanel";
import { HistoryPanel } from "./HistoryPanel";
import { Icon } from "./Icon";
import { PlaybackBar } from "./PlaybackBar";
import { SongBar } from "./SongBar";
import { Viewport } from "./Viewport";

const EMPTY: History = { rows: [], current: null, status: null, turns: [] };

/** The window: what is open has its panel on the left — a live world's
 *  history, with its own transport, or a document — the world is in the
 *  middle with the song it performs under it, and the agents are on the
 *  right. */
export function App() {
  const [mode, setMode] = useState<Mode>({ history: false, document: false });
  const [history, setHistory] = useState<History>(EMPTY);
  const [playing, setPlaying] = useState(false);
  const [title, setTitle] = useState<string | null>(null);
  const [opener, setOpener] = useState<{ busy: boolean; status: string | null }>({ busy: false, status: null });
  const [doc, setDoc] = useState<DocumentView | null>(null);
  const [song, setSong] = useState<NowPlaying | null>(null);

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
        case "song":
          setSong(m.song);
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
      className="grid h-full grid-rows-[36px_minmax(0,1fr)]"
      style={{ gridTemplateColumns: left ? `${left} minmax(0,1fr) 480px` : "minmax(0,1fr) 480px" }}
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
        <div className="flex min-h-0 flex-col border-r border-line [&>aside]:flex-1 [&>aside]:border-r-0">
          <HistoryPanel history={history} />
          <PlaybackBar history={history} playing={playing} />
        </div>
      )}
      <main className="flex min-h-0 flex-col">
        <Viewport />
        {song && <SongBar song={song} />}
      </main>
      <AgentsPanel />
    </div>
  );
}
