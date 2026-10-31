import { useRef } from "react";
import type { History } from "./bridge";
import { send } from "./bridge";
import { Icon, type IconName } from "./Icon";

function Button({ icon, label, onClick }: { icon: IconName; label: string; onClick: () => void }) {
  return (
    <button title={label} aria-label={label} onClick={onClick} className="grid size-7 place-items-center rounded text-muted hover:bg-raised hover:text-ink">
      <Icon name={icon} />
    </button>
  );
}

/** Playback under the viewport: step through the history in the order it
 *  was written, or play it, so a person watches the world being made.
 *  Every move is a seek; nothing is written. */
export function PlaybackBar({ history, playing }: { history: History; playing: boolean }) {
  const { rows, current } = history;
  const n = rows.length;
  // Position 0 is the base; position i is rows[i - 1].
  const at = current === null ? 0 : rows.findIndex((row) => row.id === current) + 1;
  const lastSent = useRef(0);
  const scrub = (position: number) => {
    // A drag seeks as it goes, at most every 100 ms.
    const now = performance.now();
    if (now - lastSent.current < 100) return;
    lastSent.current = now;
    send({ t: "seek", id: position === 0 ? null : rows[position - 1].id });
  };
  return (
    <div className="flex h-11 shrink-0 items-center gap-0.5 border-t border-line bg-panel px-2">
      <Button icon="first" label="Base" onClick={() => send({ t: "step", to: "first" })} />
      <Button icon="prev" label="Previous entry" onClick={() => send({ t: "step", to: "prev" })} />
      <Button icon={playing ? "pause" : "play"} label={playing ? "Pause" : "Play"} onClick={() => send({ t: "play", playing: !playing })} />
      <Button icon="next" label="Next entry" onClick={() => send({ t: "step", to: "next" })} />
      <Button icon="last" label="Head" onClick={() => send({ t: "step", to: "last" })} />
      <input
        type="range"
        min={0}
        max={Math.max(n, 1)}
        value={at}
        disabled={n === 0}
        onChange={(e) => scrub(Number(e.target.value))}
        className="mx-3 h-1 flex-1 accent-accent"
        aria-label="History position"
      />
      <span className="w-16 text-right text-muted tabular-nums">
        {at} / {n}
      </span>
    </div>
  );
}
