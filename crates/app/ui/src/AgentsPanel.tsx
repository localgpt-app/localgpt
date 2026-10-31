import { useEffect, useState } from "react";
import type { Activity } from "./bridge";
import { last, listen } from "./bridge";
import { Icon } from "./Icon";
import { Terminal } from "./Terminal";

/** What a tab can run. The app knows each agent by its command's name and
 *  nothing more: no models, keys or settings — those stay with the agent. */
export const KINDS = [
  { kind: "claude", label: "Claude Code" },
  { kind: "codex", label: "Codex" },
  { kind: "agy", label: "agy" },
  { kind: "gemini", label: "Gemini CLI" },
  { kind: "opencode", label: "OpenCode" },
  { kind: "shell", label: "Shell" },
];

export const kindLabel = (kind: string) => KINDS.find((k) => k.kind === kind)?.label ?? kind;

/** A tab's dot: green at rest, blue while its agent works, amber when it
 *  asks, grey once it has ended. */
export function activityDot(activity: Activity | null, ended = false) {
  if (ended) return "bg-faint";
  if (activity === "working") return "animate-pulse bg-accent";
  if (activity === "asking") return "bg-head";
  return "bg-[#9ece6a]";
}

type Tab = {
  id: string;
  kind: string;
  label: string;
  ended: boolean;
  /** Opened by another page (a paired phone's): shown, not started. */
  attach: boolean;
  activity: Activity | null;
  /** What the agent calls the task, from its title. */
  task: string | null;
};

/** A tab id no other page has: a phone opens tabs too, and this page's
 *  count starts again when it reloads. */
const newId = () => `t${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}`;

function tab(kind: string, id = newId(), attach = false): Tab {
  return { id, kind, label: kindLabel(kind), ended: false, attach, activity: null, task: null };
}

/** `?open=claude,shell` opens those tabs at start. */
function initialTabs(): Tab[] {
  const open = new URLSearchParams(location.search).get("open");
  return (open ?? "")
    .split(",")
    .filter((kind) => KINDS.some((k) => k.kind === kind))
    .map((kind) => tab(kind));
}

/** The agents' tabs: each a full terminal running the agent the user
 *  installed, started in the open world's folder. A tab a paired phone
 *  opened shows up here too. */
export function AgentsPanel() {
  const [tabs, setTabs] = useState<Tab[]>(initialTabs);
  const [active, setActive] = useState<string | null>(() => tabs[0]?.id ?? null);
  const [menu, setMenu] = useState(false);

  useEffect(() => {
    // Tabs this page does not have: another page's, or this one's before a
    // reload. They are attached, and the app sends their screens.
    const adopt = (list: { id: string; kind: string }[]) =>
      setTabs((all) => {
        const fresh = list.filter((t) => !all.some((a) => a.id === t.id));
        return fresh.length ? [...all, ...fresh.map((t) => tab(t.kind, t.id, true))] : all;
      });
    const known = last("term.list");
    if (known) adopt(known.tabs);
    return listen((m) => {
      if (m.t === "term.exit") setTabs((all) => all.map((t) => (t.id === m.id ? { ...t, ended: true } : t)));
      if (m.t === "term.list") adopt(m.tabs);
      if (m.t === "term.state") setTabs((all) => all.map((t) => (t.id === m.id ? { ...t, activity: m.activity, task: m.label } : t)));
    });
  }, []);

  const open = (kind: string) => {
    const t = tab(kind);
    setTabs((all) => [...all, t]);
    setActive(t.id);
    setMenu(false);
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
              title={t.task ?? undefined}
              className={`group flex shrink-0 cursor-default items-center gap-1.5 border-r border-line pr-1 pl-3 ${t.id === active ? "bg-raised text-ink" : "text-muted hover:text-ink"}`}
            >
              <span className={`size-1.5 rounded-full ${activityDot(t.activity, t.ended)}`} />
              {t.label}
              <button
                aria-label={`Close ${t.label}`}
                onClick={(e) => {
                  e.stopPropagation();
                  close(t.id);
                }}
                className="grid size-5 place-items-center rounded text-faint opacity-0 group-hover:opacity-100 hover:bg-line hover:text-ink"
              >
                <Icon name="close" className="size-3" />
              </button>
            </div>
          ))}
        </div>
        <button aria-label="New tab" onClick={() => setMenu(!menu)} className="grid w-9 shrink-0 place-items-center text-muted hover:bg-raised hover:text-ink">
          <Icon name="plus" />
        </button>
        {menu && (
          <div className="absolute top-9 right-1 z-10 w-44 rounded-md border border-line bg-raised py-1 shadow-xl">
            {KINDS.map((k) => (
              <button key={k.kind} onClick={() => open(k.kind)} className="block w-full px-3 py-1.5 text-left hover:bg-line">
                {k.label}
              </button>
            ))}
          </div>
        )}
      </header>
      <div className="relative min-h-0 flex-1">
        {tabs.map((t) => (
          <Terminal key={t.id} id={t.id} kind={t.kind} visible={t.id === active} attach={t.attach} />
        ))}
        {tabs.length === 0 && (
          <div className="grid h-full place-content-center gap-2 p-6">
            <p className="mb-2 text-center text-muted">Run an agent against this world.</p>
            {KINDS.map((k) => (
              <button key={k.kind} onClick={() => open(k.kind)} className="w-52 rounded-md border border-line px-3 py-2 text-left hover:bg-raised">
                {k.label}
              </button>
            ))}
          </div>
        )}
      </div>
    </aside>
  );
}
