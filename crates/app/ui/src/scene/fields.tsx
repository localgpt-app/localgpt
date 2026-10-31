import { type ReactNode, useEffect, useRef, useState } from "react";
import { Icon, type IconName } from "../Icon";
import type { Rgba } from "./model";

/** A field's look besides its value: held by a drag (with what the drag has
 *  done so far), changed last by someone else, or refused by the world. */
export type Mark = { scrub?: string; changed?: string; refused?: string };

/** What a field does with what a person gives it: shows it while a drag
 *  holds it, commits it when let go of, and puts the world back on Esc.
 *  `absolute` is a value to give every selected thing — typed, picked, a
 *  slider's place — where a drag on a number moves each of them by as much
 *  as it moved the shown one. Without an edit, a field only reads (and a
 *  design sheet draws it). */
export type FieldEdit<T> = {
  held: (value: T, absolute: boolean) => void;
  set: (value: T, absolute: boolean) => void;
  cancel: () => void;
  /** Where a drag starts when the field shows no value: the selection
   *  differs in it. */
  from?: T;
};

const AXIS = ["text-danger", "text-sound", "text-accent"];

/** A field's outline for what is marked on it. */
export const ring = (mark?: Mark) =>
  mark?.refused ? "ring-1 ring-danger/70" : mark?.scrub ? "ring-1 ring-accent bg-accent/10" : mark?.changed ? "ring-1 ring-head/50" : "";

/** Rounded past what is shown, so a value written reads as it was set. */
const round = (v: number, digits: number) => {
  const k = 10 ** Math.max(0, digits + 2);
  return Math.round(v * k) / k;
};

/** Esc while a drag holds a field: the drag ends, and the world is put back. */
function useEscape(active: boolean, cancel: () => void) {
  const latest = useRef(cancel);
  latest.current = cancel;
  useEffect(() => {
    if (!active) return;
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") latest.current();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [active]);
}

/** A number in a box. With an edit, a drag along it scrubs it — `step` a
 *  pixel, a tenth with Shift — and a click types a value instead: Enter
 *  sets it, Esc lets it go. */
export function Num({
  value,
  digits = 2,
  unit,
  axis,
  mark,
  wide,
  edit,
  step,
  min,
}: {
  value: number | null;
  digits?: number;
  unit?: string;
  axis?: number;
  mark?: Mark;
  wide?: boolean;
  edit?: FieldEdit<number>;
  step?: number;
  min?: number;
}) {
  const [typing, setTyping] = useState<string | null>(null);
  const [dragged, setDragged] = useState<number | null>(null);
  const press = useRef<{ x: number; from: number; moved: boolean; last: number } | null>(null);
  /** Esc let the typed value go: the blur that follows sets nothing. */
  const dropped = useRef(false);
  const shown = dragged ?? value;
  const box = `flex h-6 min-w-0 items-center gap-1 rounded bg-raised px-1.5 tabular-nums ${wide ? "flex-1" : "w-[68px]"}`;

  useEscape(dragged !== null, () => {
    press.current = null;
    setDragged(null);
    edit?.cancel();
  });

  const label = axis !== undefined && <span className={`text-[10px] font-semibold ${AXIS[axis]}`}>{"XYZ"[axis]}</span>;
  const units = unit && <span className="text-[10px] text-faint">{unit}</span>;
  if (!edit)
    return (
      <span className={`${box} ${ring(mark)}`}>
        {label}
        <span className={`min-w-0 flex-1 truncate text-right ${value === null ? "text-faint" : "text-ink"}`}>{value === null ? "—" : value.toFixed(digits)}</span>
        {units}
      </span>
    );

  // Leaving the box is the one way a typed value is set: Enter leaves it,
  // and Esc leaves it having let the value go.
  const leave = () => {
    const text = typing?.trim() ?? "";
    setTyping(null);
    if (dropped.current) {
      dropped.current = false;
      return;
    }
    const v = Number(text);
    if (text === "" || !Number.isFinite(v) || v === value) return;
    edit.set(min === undefined ? v : Math.max(min, v), true);
  };
  return (
    <span
      className={`${box} ${typing === null ? "cursor-ew-resize select-none" : ""} ${ring(dragged !== null ? { scrub: "held" } : mark)}`}
      onPointerDown={(e) => {
        if (typing !== null || e.button !== 0) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        const from = value ?? edit.from ?? 0;
        press.current = { x: e.clientX, from, moved: false, last: from };
      }}
      onPointerMove={(e) => {
        const p = press.current;
        if (!p) return;
        const dx = e.clientX - p.x;
        if (!p.moved && Math.abs(dx) < 3) return;
        p.moved = true;
        const per = (step ?? Math.max(10 ** -digits, Math.abs(p.from) * 0.005)) * (e.shiftKey ? 0.1 : 1);
        let v = round(p.from + dx * per, digits);
        if (min !== undefined) v = Math.max(min, v);
        if (v === p.last) return;
        p.last = v;
        setDragged(v);
        edit.held(v, false);
      }}
      onPointerUp={() => {
        const p = press.current;
        press.current = null;
        if (!p) return;
        if (p.moved) {
          setDragged(null);
          if (p.last !== p.from) edit.set(p.last, false);
          else edit.cancel();
        } else setTyping(value === null ? "" : value.toFixed(digits));
      }}
      onPointerCancel={() => {
        if (!press.current) return;
        press.current = null;
        setDragged(null);
        edit.cancel();
      }}
    >
      {label}
      {typing !== null ? (
        <input
          autoFocus
          value={typing}
          onFocus={(e) => e.currentTarget.select()}
          onChange={(e) => setTyping(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") dropped.current = true;
            if (e.key === "Enter" || e.key === "Escape") e.currentTarget.blur();
          }}
          onBlur={leave}
          className="min-w-0 flex-1 bg-transparent text-right text-ink outline-none"
        />
      ) : (
        <span className={`min-w-0 flex-1 truncate text-right ${shown === null ? "text-faint" : "text-ink"}`}>{shown === null ? "—" : shown.toFixed(digits)}</span>
      )}
      {units}
    </span>
  );
}

/** One labelled line of the inspector, with what is marked on it under it. */
export function Row({ label, unit, children, mark, hint }: { label: string; unit?: string; children: ReactNode; mark?: Mark; hint?: string }) {
  return (
    <div className="px-3 py-0.5">
      <div className="flex items-center gap-2">
        <span className={`w-[76px] shrink-0 truncate ${mark?.scrub ? "text-accent" : "text-muted"}`} title={hint}>
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
export function Vec3Row({
  label,
  value,
  digits = 2,
  unit,
  marks,
  hint,
  edit,
  step,
  min,
}: {
  label: string;
  value: (number | null)[];
  digits?: number;
  unit?: string;
  marks?: (Mark | undefined)[];
  hint?: string;
  edit?: (axis: number) => FieldEdit<number> | undefined;
  step?: number;
  min?: number;
}) {
  const mark = marks?.find((m) => m?.scrub || m?.refused || m?.changed);
  return (
    <Row label={label} unit={unit} mark={mark} hint={hint}>
      {value.map((v, i) => (
        <Num key={i} value={v} digits={digits} axis={i} mark={marks?.[i]} wide edit={edit?.(i)} step={step} min={min} />
      ))}
    </Row>
  );
}

/** sRGB in `0..=1`, as the format writes colours, as `#rrggbb`. */
export function hex([r, g, b]: Rgba | [number, number, number]) {
  const c = (v: number) =>
    Math.round(Math.min(1, Math.max(0, v)) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${c(r)}${c(g)}${c(b)}`;
}

/** `#rrggbb` back to the format's colour, keeping its alpha. */
export function fromHex(text: string, alpha: number): Rgba {
  const n = (i: number) => Math.round((parseInt(text.slice(1 + i * 2, 3 + i * 2), 16) / 255) * 1e4) / 1e4;
  return [n(0), n(1), n(2), alpha];
}

/** A colour. With an edit, a click opens the system's picker: the world
 *  follows what is picked, and closing the picker sets it. */
export function ColorRow({ label, value, strength, mark, edit }: { label: string; value: Rgba; strength?: number; mark?: Mark; edit?: FieldEdit<Rgba> }) {
  const input = useRef<HTMLInputElement>(null);
  const latest = useRef({ edit, value });
  latest.current = { edit, value };
  useEffect(() => {
    const el = input.current;
    if (!el) return;
    // React's onChange is the input event; the picker closing is `change`.
    const closed = () => {
      const { edit, value } = latest.current;
      const picked = fromHex(el.value, value[3]);
      if (hex(picked) !== hex(value)) edit?.set(picked, true);
      else edit?.cancel();
    };
    el.addEventListener("change", closed);
    return () => el.removeEventListener("change", closed);
  }, [edit !== undefined]);
  return (
    <Row label={label} mark={mark}>
      <span className={`relative flex h-6 flex-1 items-center gap-2 rounded bg-raised px-1.5 ${edit ? "cursor-pointer hover:ring-1 hover:ring-line" : ""} ${ring(mark)}`}>
        <span className="size-3.5 shrink-0 rounded-sm ring-1 ring-black/40" style={{ background: hex(value) }} />
        <span className="font-mono text-[11px] text-ink">{hex(value)}</span>
        {value[3] < 1 && <span className="text-[10px] text-faint">{Math.round(value[3] * 100)}%</span>}
        {strength !== undefined && <span className="ml-auto text-[10px] text-muted tabular-nums">× {strength.toFixed(1)}</span>}
        {edit && (
          <input
            ref={input}
            type="color"
            aria-label={label}
            value={hex(value)}
            onChange={(e) => edit.held(fromHex(e.currentTarget.value, value[3]), true)}
            className="absolute inset-0 size-full cursor-pointer opacity-0"
          />
        )}
      </span>
    </Row>
  );
}

/** A value from 0 to `max` as a bar. With an edit, a press sets it where
 *  it lands and a drag carries it; letting go sets it. */
export function SliderRow({ label, value, max = 1, mark, edit }: { label: string; value: number; max?: number; mark?: Mark; edit?: FieldEdit<number> }) {
  const [dragged, setDragged] = useState<number | null>(null);
  const held = useRef<number | null>(null);
  const shown = dragged ?? value;
  useEscape(dragged !== null, () => {
    held.current = null;
    setDragged(null);
    edit?.cancel();
  });
  const at = (e: React.PointerEvent<HTMLSpanElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    return Math.round(Math.min(1, Math.max(0, (e.clientX - box.left) / box.width)) * max * 100) / 100;
  };
  const hold = (v: number) => {
    if (v === held.current) return;
    held.current = v;
    setDragged(v);
    edit?.held(v, true);
  };
  return (
    <Row label={label} mark={mark}>
      <span
        className={`relative h-6 flex-1 overflow-hidden rounded bg-raised ${edit ? "cursor-pointer" : ""} ${ring(dragged !== null ? { scrub: "held" } : mark)}`}
        onPointerDown={
          edit &&
          ((e) => {
            if (e.button !== 0) return;
            e.currentTarget.setPointerCapture(e.pointerId);
            held.current = null;
            hold(at(e));
          })
        }
        onPointerMove={edit && ((e) => held.current !== null && hold(at(e)))}
        onPointerUp={
          edit &&
          (() => {
            const v = held.current;
            held.current = null;
            setDragged(null);
            if (v === null) return;
            if (v !== value) edit.set(v, true);
            else edit.cancel();
          })
        }
      >
        <span className="absolute inset-y-0 left-0 bg-accent/25" style={{ width: `${(shown / max) * 100}%` }} />
        <span className="relative flex h-full items-center justify-end px-1.5 text-ink tabular-nums">{shown.toFixed(2)}</span>
      </span>
    </Row>
  );
}

export function ToggleRow({ label, on, mark, onToggle }: { label: string; on: boolean; mark?: Mark; onToggle?: () => void }) {
  const knob = (
    <span className={`flex h-4 w-7 items-center rounded-full px-0.5 ${on ? "justify-end bg-accent" : "bg-line"}`}>
      <span className="size-3 rounded-full bg-ink" />
    </span>
  );
  return (
    <Row label={label} mark={mark}>
      {onToggle ? (
        <button role="switch" aria-checked={on} aria-label={label} onClick={onToggle} className="rounded-full">
          {knob}
        </button>
      ) : (
        knob
      )}
    </Row>
  );
}

/** One of a few values. `fixed`: it reads, without the menu's chevron. */
export function ChoiceRow({ label, value, mark, fixed }: { label: string; value: string; mark?: Mark; fixed?: boolean }) {
  return (
    <Row label={label} mark={mark}>
      <span className={`flex h-6 flex-1 items-center justify-between rounded bg-raised px-1.5 ${ring(mark)}`}>
        <span className="text-ink">{value}</span>
        {!fixed && <Icon name="chevronDown" className="size-3 text-faint" />}
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

/** A part of the inspector: a heading with its icon, and its rows, which a
 *  click on the heading folds away. */
export function Part({ title, icon, tone = "text-muted", aside, children }: { title: string; icon: IconName; tone?: string; aside?: ReactNode; children: ReactNode }) {
  const [open, setOpen] = useState(true);
  return (
    <section className={`border-b border-line ${open ? "pb-2" : ""}`}>
      <header className="flex h-8 items-center gap-1.5 px-3">
        <button onClick={() => setOpen(!open)} className="flex min-w-0 items-center gap-1.5" aria-expanded={open}>
          <Icon name={open ? "chevronDown" : "chevronRight"} className="size-3 text-faint" />
          <Icon name={icon} className={`size-3.5 ${tone}`} />
          <span className="font-medium text-ink">{title}</span>
        </button>
        <span className="ml-auto flex items-center gap-1 text-[10px] text-faint">{aside}</span>
      </header>
      {open && children}
    </section>
  );
}
