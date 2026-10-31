import type { ReactNode } from "react";
import { Icon, type IconName } from "../Icon";
import type { Rgba } from "./model";

/** A field's look besides its value: held by a drag (with what the drag has
 *  done so far), changed last by someone else, or refused by the world. */
export type Mark = { scrub?: string; changed?: string; refused?: string };

const AXIS = ["text-danger", "text-sound", "text-accent"];

const ring = (mark?: Mark) =>
  mark?.refused ? "ring-1 ring-danger/70" : mark?.scrub ? "ring-1 ring-accent bg-accent/10" : mark?.changed ? "ring-1 ring-head/50" : "";

/** A number in a box; in the app the label beside it drags it. */
export function Num({ value, digits = 2, unit, axis, mark, wide }: { value: number | null; digits?: number; unit?: string; axis?: number; mark?: Mark; wide?: boolean }) {
  return (
    <span className={`flex h-6 min-w-0 items-center gap-1 rounded bg-raised px-1.5 tabular-nums ${wide ? "flex-1" : "w-[68px]"} ${ring(mark)}`}>
      {axis !== undefined && <span className={`text-[10px] font-semibold ${AXIS[axis]}`}>{"XYZ"[axis]}</span>}
      <span className={`min-w-0 flex-1 truncate text-right ${value === null ? "text-faint" : "text-ink"}`}>{value === null ? "—" : value.toFixed(digits)}</span>
      {unit && <span className="text-[10px] text-faint">{unit}</span>}
    </span>
  );
}

/** One labelled line of the inspector, with what is marked on it under it. */
export function Row({ label, unit, children, mark, hint }: { label: string; unit?: string; children: ReactNode; mark?: Mark; hint?: string }) {
  return (
    <div className="px-3 py-0.5">
      <div className="flex items-center gap-2">
        <span className={`w-[76px] shrink-0 truncate ${mark?.scrub ? "cursor-ew-resize text-accent" : "text-muted"}`} title={hint}>
          {label}
          {unit && <span className="ml-1 text-[10px] text-faint">{unit}</span>}
        </span>
        <span className="flex min-w-0 flex-1 items-center gap-1">{children}</span>
      </div>
      {mark?.scrub && <p className="mt-0.5 pl-[84px] text-[10px] text-accent">{mark.scrub}</p>}
      {mark?.changed && !mark.scrub && (
        <p className="mt-0.5 flex items-center gap-1 pl-[84px] text-[10px] text-head">
          <span className="size-1.5 rounded-full bg-head" />
          {mark.changed}
        </p>
      )}
      {mark?.refused && <p className="mt-0.5 pl-[84px] text-[10px] leading-snug text-danger">{mark.refused}</p>}
    </div>
  );
}

/** Three numbers on one line, X Y Z, the unit said once beside the label. */
export function Vec3Row({ label, value, digits = 2, unit, marks, hint }: { label: string; value: (number | null)[]; digits?: number; unit?: string; marks?: (Mark | undefined)[]; hint?: string }) {
  const mark = marks?.find((m) => m?.scrub || m?.refused || m?.changed);
  return (
    <Row label={label} unit={unit} mark={mark} hint={hint}>
      {value.map((v, i) => (
        <Num key={i} value={v} digits={digits} axis={i} mark={marks?.[i]} wide />
      ))}
    </Row>
  );
}

/** sRGB in `0..=1`, as the format writes colours, as `#rrggbb`. */
export function hex([r, g, b]: Rgba | [number, number, number]) {
  const c = (v: number) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, "0");
  return `#${c(r)}${c(g)}${c(b)}`;
}

export function ColorRow({ label, value, strength, mark }: { label: string; value: Rgba; strength?: number; mark?: Mark }) {
  return (
    <Row label={label} mark={mark}>
      <span className={`flex h-6 flex-1 items-center gap-2 rounded bg-raised px-1.5 ${ring(mark)}`}>
        <span className="size-3.5 shrink-0 rounded-sm ring-1 ring-black/40" style={{ background: hex(value) }} />
        <span className="font-mono text-[11px] text-ink">{hex(value)}</span>
        {value[3] < 1 && <span className="text-[10px] text-faint">{Math.round(value[3] * 100)}%</span>}
        {strength !== undefined && <span className="ml-auto text-[10px] text-muted tabular-nums">× {strength.toFixed(1)}</span>}
      </span>
    </Row>
  );
}

export function SliderRow({ label, value, max = 1, mark }: { label: string; value: number; max?: number; mark?: Mark }) {
  return (
    <Row label={label} mark={mark}>
      <span className={`relative h-6 flex-1 overflow-hidden rounded bg-raised ${ring(mark)}`}>
        <span className="absolute inset-y-0 left-0 bg-accent/25" style={{ width: `${(value / max) * 100}%` }} />
        <span className="relative flex h-full items-center justify-end px-1.5 text-ink tabular-nums">{value.toFixed(2)}</span>
      </span>
    </Row>
  );
}

export function ToggleRow({ label, on, mark }: { label: string; on: boolean; mark?: Mark }) {
  return (
    <Row label={label} mark={mark}>
      <span className={`flex h-4 w-7 items-center rounded-full px-0.5 ${on ? "justify-end bg-accent" : "bg-line"}`}>
        <span className="size-3 rounded-full bg-ink" />
      </span>
    </Row>
  );
}

export function ChoiceRow({ label, value, mark }: { label: string; value: string; mark?: Mark }) {
  return (
    <Row label={label} mark={mark}>
      <span className={`flex h-6 flex-1 items-center justify-between rounded bg-raised px-1.5 ${ring(mark)}`}>
        <span className="text-ink">{value}</span>
        <Icon name="chevronDown" className="size-3 text-faint" />
      </span>
    </Row>
  );
}

export function TextRow({ label, value, mono, mark }: { label: string; value: string; mono?: boolean; mark?: Mark }) {
  return (
    <Row label={label} mark={mark}>
      <span className={`flex h-6 min-w-0 flex-1 items-center rounded bg-raised px-1.5 ${ring(mark)}`}>
        <span className={`truncate text-ink ${mono ? "font-mono text-[11px]" : ""}`}>{value}</span>
      </span>
    </Row>
  );
}

/** A part of the inspector: a heading with its icon, and its rows. */
export function Part({ title, icon, tone = "text-muted", aside, children }: { title: string; icon: IconName; tone?: string; aside?: ReactNode; children: ReactNode }) {
  return (
    <section className="border-b border-line pb-2">
      <header className="flex h-8 items-center gap-1.5 px-3">
        <Icon name="chevronDown" className="size-3 text-faint" />
        <Icon name={icon} className={`size-3.5 ${tone}`} />
        <span className="font-medium text-ink">{title}</span>
        <span className="ml-auto flex items-center gap-1 text-[10px] text-faint">{aside}</span>
      </header>
      {children}
    </section>
  );
}
