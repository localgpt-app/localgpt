import { Icon } from "../src/Icon";
import * as w from "./fixtures";

// The window's own parts, drawn the same on every sheet: the top bar and
// the agents' column, with a transcript that stays the same.

const TRANSCRIPT = `~/worlds/harbour.world $ claude

> cut 3B two bars later, so it lands on the
  bridge's second phrase

  Reading the shot list through the world's MCP.
  3B starts with the bridge at 1:55.20; two bars
  at 92 bpm is 5.22 s.

  world.submit — 1 op: 3B in_s 115.20 → 120.42

  Proposed. It waits in the timeline for review.
`;

export function TopBar() {
  return (
    <header className="col-span-3 flex items-center gap-3 border-b border-line bg-panel px-4">
      <span className="font-semibold tracking-tight">LocalGPT</span>
      <span className="text-muted">World · harbour.world</span>
      <span className="flex items-center gap-1 rounded px-1.5 py-0.5 text-muted">
        <Icon name="open" className="size-3.5" />
        Open…
      </span>
      <span className="ml-auto text-faint">{w.history.status}</span>
    </header>
  );
}

export function Agents() {
  return (
    <aside className="flex min-h-0 flex-col border-l border-line bg-panel">
      <div className="flex h-9 shrink-0 items-center gap-1 border-b border-line px-2">
        <span className="rounded bg-raised px-2 py-1 text-ink">Claude Code</span>
        <span className="px-2 py-1 text-muted">Shell</span>
        <span className="ml-auto grid size-6 place-items-center text-muted">
          <Icon name="plus" />
        </span>
      </div>
      <pre className="m-0 min-h-0 flex-1 overflow-hidden bg-[#0b0e13] p-3 font-mono text-[11.5px] leading-[17px] text-ink/90">{TRANSCRIPT}</pre>
    </aside>
  );
}

