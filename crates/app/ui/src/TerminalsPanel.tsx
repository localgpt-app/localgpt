import { useEffect, useState } from "react";
import { last, listen } from "./bridge";
import { Icon } from "./Icon";
import { Terminal } from "./Terminal";

/** A tab's dot: green while its shell runs, grey once it has ended. */
export function tabDot(ended: boolean) {
  return ended ? "bg-faint" : "bg-[#9ece6a]";
}

type Tab = {
  id: string;
  ended: boolean;
  /** Opened by another page (a paired phone's): shown, not started. */
  attach: boolean;
  /** What the program in the tab calls itself, from its title. */
  label: string | null;
};

/** A tab id no other page has: a phone opens tabs too, and this page's
 *  count starts again when it reloads. */
const newId = () => `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}`;

function tab(id = newId(), attach = false): Tab {
  return { id, ended: false, attach, label: null };
}

/** `?open=terminal` opens a tab at start. */
function initialTabs(): Tab[] {
  const open = new URLSearchParams(location.search).get("open") ?? "";
  return open.split(",").includes("terminal") ? [tab()] : [];
}

/** The terminals: each a login shell started in the open world's folder,
 *  where the person runs whatever agent they have. The app knows nothing
 *  about what runs in a tab; its title is the tab's label. A tab a paired
 *  phone opened shows up here too. */
export function TerminalsPanel() {
  const [tabs, setTabs] = useState<Tab[]>(initialTabs);
  const [active, setActive] = useState<string | null>(() => tabs[0]?.id ?? null);

  useEffect(() => {
    // Tabs this page does not have: another page's, or this one's before a
    // reload. They are attached, and the app sends their screens.
    const adopt = (list: { id: string; label: string | null }[]) =>
      setTabs((all) => {
        const fresh = list.filter((t) => !all.some((a) => a.id === t.id));
        return fresh.length ? [...all, ...fresh.map((t) => ({ ...tab(t.id, true), label: t.label }))] : all;
      });
    const known = last("term.list");
    if (known) adopt(known.tabs);
    return listen((m) => {
      if (m.t === "term.exit") setTabs((all) => all.map((t) => (t.id === m.id ? { ...t, ended: true } : t)));
      if (m.t === "term.list") adopt(m.tabs);
      if (m.t === "term.state") setTabs((all) => all.map((t) => (t.id === m.id ? { ...t, label: m.label } : t)));
    });
  }, []);

  const open = () => {
    const t = tab();
    setTabs((all) => [...all, t]);
    setActive(t.id);
  };
  const close = (id: string) => {
    const rest = tabs.filter((t) => t.id !== id);
    setTabs(rest);
    if (active === id) setActive(rest.at(-1)?.id ?? null);
  };

  return (
    <aside className="flex min-h-0 flex-col border-l border-line bg-panel">
      <header className="relative flex h-9 shrink-0 items-stretch border-b border-line">
        <div className="flex min-w-0 flex-1 items-stretch overflow-x-auto">
          {tabs.map((t) => (
            <div
              key={t.id}
              onClick={() => setActive(t.id)}
              title={t.label ?? undefined}
              className={`group flex max-w-56 shrink-0 cursor-default items-center gap-1.5 border-r border-line pr-1 pl-3 ${t.id === active ? "bg-raised text-ink" : "text-muted hover:text-ink"}`}
            >
              <span className={`size-1.5 shrink-0 rounded-full ${tabDot(t.ended)}`} />
              <span className="truncate">{t.label ?? "Terminal"}</span>
              <button
                aria-label="Close terminal"
                onClick={(e) => {
                  e.stopPropagation();
                  close(t.id);
                }}
                className="grid size-5 shrink-0 place-items-center rounded text-faint opacity-0 group-hover:opacity-100 hover:bg-line hover:text-ink"
              >
                <Icon name="close" className="size-3" />
              </button>
            </div>
          ))}
        </div>
        <button aria-label="New terminal" onClick={open} className="grid w-9 shrink-0 place-items-center text-muted hover:bg-raised hover:text-ink">
          <Icon name="plus" />
        </button>
      </header>
      <div className="relative min-h-0 flex-1">
        {tabs.map((t) => (
          <Terminal key={t.id} id={t.id} visible={t.id === active} attach={t.attach} />
        ))}
        {tabs.length === 0 && (
          <div className="grid h-full place-content-center gap-2 p-6">
            <p className="mb-2 text-center text-muted">A terminal opens in this world's folder. Run your agent in it.</p>
            <button onClick={open} className="w-52 rounded-md border border-line px-3 py-2 text-left hover:bg-raised">
              New terminal
            </button>
          </div>
        )}
      </div>
    </aside>
  );
}
