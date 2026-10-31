/** The left panel's tabs on a live world: the history and the outliner
 *  share it — the world's structure in time and in space, read one at a
 *  time — with what the open one holds counted at the right. */
export type LeftTab = "History" | "Outliner";

export function PanelTabs({ active, count, choose }: { active: LeftTab; count: string; choose?: (tab: LeftTab) => void }) {
  return (
    <div role="tablist" className="flex h-9 shrink-0 items-center gap-1 border-b border-line px-2">
      {(["History", "Outliner"] as const).map((tab) => {
        const look = `rounded px-2 py-1 ${tab === active ? "bg-raised text-ink" : "text-muted"}`;
        return choose ? (
          <button key={tab} role="tab" aria-selected={tab === active} onClick={() => choose(tab)} className={`${look} ${tab === active ? "" : "hover:text-ink"}`}>
            {tab}
          </button>
        ) : (
          <span key={tab} className={look}>
            {tab}
          </span>
        );
      })}
      <span className="ml-auto pr-1 text-muted tabular-nums">{count}</span>
    </div>
  );
}
