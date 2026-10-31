import type { History, HistoryRow } from "./bridge";
import { send } from "./bridge";

const ROW = 30;
const LANE = 14;
const PAD = 12;
const NODE = 4;
const COLORS = ["#7aa2ff", "#f7768e", "#9ece6a", "#e0af68", "#bb9af7", "#7dcfff", "#ff9e64"];

/** A lane per branch, as Gen's history panel draws them: an entry continues
 *  the lane whose tip is its parent; one whose parent has moved on (a fork)
 *  opens a new lane. Lane 0 starts at the base. */
export function lanes(rows: HistoryRow[]): { lane: number[]; count: number } {
  const tips: (string | null)[] = [null];
  const lane: number[] = [];
  for (const row of rows) {
    let at = tips.indexOf(row.parent);
    if (at < 0) {
      tips.push(null);
      at = tips.length - 1;
    }
    tips[at] = row.id;
    lane.push(at);
  }
  return { lane, count: tips.length };
}

function shortId(id: string) {
  return id.startsWith("sha256:") ? id.slice(7, 14) : id;
}

function ago(ms: number) {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return "now";
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < 86400) return `${Math.floor(s / 3600)}h`;
  return `${Math.floor(s / 86400)}d`;
}

/** The log as `git log --graph` draws it: newest at the top, one lane per
 *  branch, the base at the bottom. A click shows that entry in the world. */
export function HistoryPanel({ history }: { history: History }) {
  const { rows, current, turns } = history;
  const n = rows.length;
  const { lane, count } = lanes(rows);
  const index = new Map(rows.map((row, i) => [row.id, i]));
  // Row i (write order) sits at position n-1-i from the top; the base at n.
  const y = (position: number) => position * ROW + ROW / 2;
  const x = (l: number) => PAD + l * LANE;
  const width = PAD * 2 + (count - 1) * LANE;
  const turnAt = new Map(turns.map((turn) => [turn.rows - 1, turn]));

  const edges = rows.map((row, i) => {
    const from = { x: x(lane[i]), y: y(n - 1 - i) };
    const j = row.parent === null ? undefined : index.get(row.parent);
    const to = j === undefined ? { x: x(0), y: y(n) } : { x: x(lane[j]), y: y(n - 1 - j) };
    const d =
      from.x === to.x
        ? `M${from.x} ${from.y}V${to.y}`
        : `M${from.x} ${from.y}V${to.y - ROW * 0.6}C${from.x} ${to.y - ROW * 0.2} ${to.x} ${to.y - ROW * 0.35} ${to.x} ${to.y}`;
    return <path key={row.id} d={d} stroke={COLORS[lane[i] % COLORS.length]} strokeWidth={1.75} fill="none" />;
  });

  return (
    <aside className="flex min-h-0 flex-col border-r border-line bg-panel">
      <header className="flex h-9 shrink-0 items-center justify-between border-b border-line px-3">
        <span className="font-medium">History</span>
        <span className="text-muted tabular-nums">{n} entries</span>
      </header>
      <div className="relative min-h-0 flex-1 overflow-y-auto">
        {n === 0 ? (
          <p className="p-4 leading-relaxed text-muted">
            Nothing written yet. Each edit an agent makes lands here, and a click shows the world as it was.
          </p>
        ) : (
          <div className="relative" style={{ height: (n + 1) * ROW }}>
            {/* Above the rows, so the current row's highlight never covers its node. */}
            <svg className="pointer-events-none absolute top-0 left-0 z-10" width={width} height={(n + 1) * ROW}>
              {edges}
              {rows.map((row, i) => {
                const cx = x(lane[i]);
                const cy = y(n - 1 - i);
                const color = COLORS[lane[i] % COLORS.length];
                return (
                  <g key={row.id}>
                    {row.head && <circle cx={cx} cy={cy} r={NODE + 3} fill="none" stroke="var(--color-head)" strokeWidth={1.5} />}
                    <circle cx={cx} cy={cy} r={NODE} fill={row.id === current ? color : "var(--color-panel)"} stroke={color} strokeWidth={1.75} />
                  </g>
                );
              })}
              <circle cx={x(0)} cy={y(n)} r={NODE} fill={current === null ? COLORS[0] : "var(--color-panel)"} stroke={COLORS[0]} strokeWidth={1.75} />
            </svg>
            {rows
              .map((row, i) => (
                <button
                  key={row.id}
                  title={`${row.message ?? row.summary}\n${shortId(row.id)} · ${row.id}`}
                  onClick={() => send({ t: "seek", id: row.id })}
                  className={`absolute right-0 left-0 flex items-center gap-2 pr-3 text-left hover:bg-raised ${row.id === current ? "bg-raised" : ""}`}
                  style={{ top: (n - 1 - i) * ROW, height: ROW, paddingLeft: width }}
                >
                  <span className={`min-w-0 flex-1 truncate ${row.id === current ? "text-ink" : "text-ink/85"}`}>{row.summary}</span>
                  {turnAt.has(i) && <span className="shrink-0 rounded bg-accent/15 px-1.5 text-[10px] text-accent">turn {turnAt.get(i)!.n}</span>}
                  {row.head && <span className="shrink-0 rounded bg-head/15 px-1.5 text-[10px] text-head">main</span>}
                  <span className="w-14 shrink-0 truncate text-right text-muted">{row.author}</span>
                  <span className="w-7 shrink-0 text-right text-faint tabular-nums">{ago(row.timestamp_ms)}</span>
                </button>
              ))
              .reverse()}
            <button
              onClick={() => send({ t: "seek", id: null })}
              className={`absolute right-0 left-0 flex items-center pr-3 text-left text-muted hover:bg-raised ${current === null ? "bg-raised" : ""}`}
              style={{ top: n * ROW, height: ROW, paddingLeft: width }}
            >
              base
            </button>
          </div>
        )}
      </div>
    </aside>
  );
}
