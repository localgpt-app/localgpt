import { useEffect, useState } from "react";
import { listen } from "./bridge";
import { Icon } from "./Icon";
import { Terminal } from "./Terminal";

/** What a tab can run. The app knows each agent by its command's name and
 *  nothing more: no models, keys or settings — those stay with the agent. */
const KINDS = [
  { kind: "claude", label: "Claude Code" },
  { kind: "codex", label: "Codex" },
  { kind: "agy", label: "agy" },
  { kind: "gemini", label: "Gemini CLI" },
  { kind: "opencode", label: "OpenCode" },
  { kind: "shell", label: "Shell" },
];

type Tab = { id: string; kind: string; label: string; ended: boolean };

let next = 0;
function tab(kind: string): Tab {
  const label = KINDS.find((k) => k.kind === kind)?.label ?? kind;
  return { id: `t${++next}`, kind, label, ended: false };
}

/** `?open=claude,shell` opens those tabs at start. */
function initialTabs(): Tab[] {
  const open = new URLSearchParams(location.search).get("open");
  return (open ?? "")
    .split(",")
    .filter((kind) => KINDS.some((k) => k.kind === kind))
    .map(tab);
}

/** The agents' tabs: each a full terminal running the agent the user
 *  installed, started in the open world's folder. */
export function AgentsPanel() {
  const [tabs, setTabs] = useState<Tab[]>(initialTabs);
  const [active, setActive] = useState<string | null>(() => tabs[0]?.id ?? null);
  const [menu, setMenu] = useState(false);

  useEffect(
    () =>
      listen((m) => {
        if (m.t === "term.exit") setTabs((all) => all.map((t) => (t.id === m.id ? { ...t, ended: true } : t)));
      }),
    [],
  );

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
              className={`group flex shrink-0 cursor-default items-center gap-1.5 border-r border-line pr-1 pl-3 ${t.id === active ? "bg-raised text-ink" : "text-muted hover:text-ink"}`}
            >
              <span className={`size-1.5 rounded-full ${t.ended ? "bg-faint" : "bg-[#9ece6a]"}`} />
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
          <Terminal key={t.id} id={t.id} kind={t.kind} visible={t.id === active} />
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
