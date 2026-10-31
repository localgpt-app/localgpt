import { Icon } from "../Icon";
import { ChoiceRow, type Mark, Num, Row, TextRow, ToggleRow } from "./fields";
import type { Entity, Vec3 } from "./model";

// A thing's behaviors: what the menu offers to add, what each starts as —
// the format's own fields, from the thing it goes on — and a behavior's
// fields, opened from its line in the inspector.

type Fields = Record<string, unknown>;

/** What the menu offers, in the order it offers them: each one's name, and
 *  what it does as it is added. Look at needs a thing to look at, picked
 *  next. */
export const BEHAVIORS: { kind: string; label: string; detail: string; picks?: boolean }[] = [
  { kind: "Spin", label: "Spin", detail: "about Y, 30° a second" },
  { kind: "Bob", label: "Bob", detail: "0.2 m, at 0.5 Hz" },
  { kind: "Pulse", label: "Pulse", detail: "0.9 to 1.1, at 1 Hz" },
  { kind: "Orbit", label: "Orbit", detail: "2 m around where it is" },
  { kind: "Bounce", label: "Bounce", detail: "0.5 m off where it is" },
  { kind: "PathFollow", label: "Follow a path", detail: "four points around it" },
  { kind: "LookAt", label: "Look at", detail: "the thing picked next", picks: true },
];

/** A new behavior as the format writes it, fitted to where the thing is. */
export function behaviorFor(kind: string, entity: Entity): Record<string, Fields> {
  const [x, y, z] = entity.transform.position;
  const around = (r: number): Vec3[] => [
    [x + r, y, z],
    [x, y, z + r],
    [x - r, y, z],
    [x, y, z - r],
  ];
  switch (kind) {
    case "Spin":
      return { Spin: { axis: [0, 1, 0], speed: 30 } };
    case "Bob":
      return { Bob: { axis: [0, 1, 0], amplitude: 0.2, frequency: 0.5, phase: 0 } };
    case "Pulse":
      return { Pulse: { min_scale: 0.9, max_scale: 1.1, frequency: 1 } };
    case "Orbit":
      return { Orbit: { center: null, center_point: [x, y, z], radius: 2, speed: 20, axis: [0, 1, 0], phase: 0, tilt: 0 } };
    case "Bounce":
      return { Bounce: { height: 0.5, gravity: 9.81, damping: 0.7, surface_y: y } };
    case "PathFollow":
      return { PathFollow: { waypoints: around(2), speed: 1, mode: "loop", orient_to_path: true } };
    default:
      return {};
  }
}

const axisName = (a: unknown) => {
  const v = (a as number[] | undefined) ?? [0, 1, 0];
  const i = v.map(Math.abs).indexOf(Math.max(...v.map(Math.abs)));
  return v.filter((c) => c !== 0).length > 1 ? "tilted" : "XYZ"[i];
};

/** A behavior in a line, as a person says it. */
export function describeBehavior(name: string, f: Fields): string {
  const n = (k: string) => Number(f[k] ?? 0);
  switch (name) {
    case "Spin":
      return `${n("speed")}° a second about ${axisName(f.axis)}`;
    case "Bob":
      return `${n("amplitude")} m at ${n("frequency")} Hz`;
    case "Pulse":
      return `${n("min_scale")}–${n("max_scale")} at ${n("frequency")} Hz`;
    case "Orbit":
      return `around ${f.center ?? "a point"}, ${n("radius")} m at ${n("speed")}°/s`;
    case "LookAt":
      return `turns to ${f.target}`;
    case "PathFollow":
      return `${(f.waypoints as unknown[] | undefined)?.length ?? 0} stops at ${n("speed")} m/s`;
    case "Bounce":
      return `${n("height")} m high`;
    default:
      return "";
  }
}

/** The menu under "+ add". */
export function BehaviorMenu() {
  return (
    <div className="overflow-hidden rounded-md bg-raised py-1 shadow-[0_6px_24px_rgba(0,0,0,0.5)] ring-1 ring-line">
      {BEHAVIORS.map((b, i) => (
        <div key={b.kind} className={`flex items-baseline gap-2 px-2.5 py-1 ${i === 0 ? "bg-accent/15" : ""}`}>
          <span className="w-[92px] shrink-0 font-medium text-behavior">{b.label}</span>
          <span className="min-w-0 truncate text-[11px] text-muted">{b.detail}</span>
        </div>
      ))}
      <p className="mt-1 border-t border-line px-2.5 pt-1.5 pb-0.5 text-[10px] leading-snug text-faint">
        Each is one entry, and opens to its fields. A behavior adds to the others: a thing can bob and spin.
      </p>
    </div>
  );
}

/** A chip naming a thing a behavior refers to, which selects it. */
function Chip({ name }: { name: string }) {
  return (
    <span className="flex h-6 items-center gap-1 rounded bg-camera/10 px-1.5 text-camera ring-1 ring-camera/30">
      <Icon name="aim" className="size-3" />
      {name}
    </span>
  );
}

/** One behavior: its line, and, opened, its fields. `drawn`: on a design
 *  sheet, which draws what isn't wired yet — the fields' chevron — where
 *  the app shows only what is, the line and `remove`. */
export function BehaviorCard({
  behavior,
  open,
  editable,
  mark,
  drawn,
  remove,
}: {
  behavior: Record<string, Fields>;
  open?: boolean;
  editable: boolean;
  mark?: Mark;
  drawn?: boolean;
  remove?: () => void;
}) {
  const [name, f] = Object.entries(behavior)[0];
  const n = (k: string) => Number(f[k] ?? 0);
  return (
    <div className={`mx-3 mb-1 rounded bg-raised ${mark ? "ring-1 ring-head/50" : open ? "ring-1 ring-behavior/40" : ""}`}>
      <div className="flex items-center gap-2 px-2 py-1.5">
        {drawn && <Icon name={open ? "chevronDown" : "chevronRight"} className="size-3 shrink-0 text-faint" />}
        <span className="font-medium text-behavior">{name}</span>
        <span className="min-w-0 flex-1 truncate text-muted">{describeBehavior(name, f)}</span>
        {editable && drawn && <Icon name="close" className="size-3 shrink-0 text-faint" />}
        {editable && remove && (
          <button aria-label={`Remove ${name}`} title={`Take the ${name.toLowerCase()} off`} onClick={remove} className="shrink-0 text-faint hover:text-ink">
            <Icon name="close" className="size-3" />
          </button>
        )}
      </div>
      {open && (
        <div className="-mx-1 pb-1.5 [&_.px-3]:px-2">
          {name === "Spin" && (
            <>
              <ChoiceRow label="Axis" value={axisName(f.axis)} />
              <Row label="Speed" unit="°/s">
                <Num value={n("speed")} digits={0} wide />
              </Row>
            </>
          )}
          {name === "Bob" && (
            <>
              <ChoiceRow label="Axis" value={axisName(f.axis)} />
              <Row label="Distance" unit="m">
                <Num value={n("amplitude")} wide />
              </Row>
              <Row label="Rate" unit="Hz">
                <Num value={n("frequency")} wide />
              </Row>
            </>
          )}
          {name === "Orbit" && (
            <>
              <Row label="Around">{f.center ? <Chip name={String(f.center)} /> : <span className="text-muted">a point, where it stood</span>}</Row>
              <Row label="Radius" unit="m">
                <Num value={n("radius")} digits={1} wide />
              </Row>
              <Row label="Speed" unit="°/s">
                <Num value={n("speed")} digits={0} wide />
              </Row>
              <Row label="Tilt" unit="°">
                <Num value={n("tilt")} digits={0} wide />
              </Row>
            </>
          )}
          {name === "LookAt" && <Row label="At">{<Chip name={String(f.target)} />}</Row>}
          {name === "Pulse" && (
            <Row label="Between" unit="×">
              <Num value={n("min_scale")} />
              <span className="text-faint">and</span>
              <Num value={n("max_scale")} />
            </Row>
          )}
          {name === "PathFollow" && (
            <>
              <TextRow label="Stops" value={`${(f.waypoints as unknown[] | undefined)?.length ?? 0}, placed on the canvas`} />
              <Row label="Speed" unit="m/s">
                <Num value={n("speed")} digits={1} wide />
              </Row>
              <ChoiceRow label="Then" value={String(f.mode ?? "loop").replace("_", "-")} />
              <ToggleRow label="Faces ahead" on={!!f.orient_to_path} />
            </>
          )}
          {name === "Bounce" && (
            <>
              <Row label="Height" unit="m">
                <Num value={n("height")} wide />
              </Row>
              <Row label="Keeps" unit="×">
                <Num value={n("damping")} wide />
              </Row>
            </>
          )}
        </div>
      )}
    </div>
  );
}
