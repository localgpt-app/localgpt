import { useRef, useState } from "react";
import { Icon } from "../Icon";
import { BehaviorCard, BehaviorMenu } from "./behaviors";
import { ChoiceRow, ColorRow, type FieldEdit, hex, type Mark, Num, Part, Row, ring, SliderRow, TextRow, ToggleRow, Vec3Row } from "./fields";
import { extent, KindMenu, LIGHTS, SHAPES } from "./kinds";
import { type Entity, type Kind, kindOf, type Rgba } from "./model";
import { GLYPH, TONE } from "./Outliner";

export type WorldSummary = {
  name: string;
  counts: [Kind, number][];
  sky?: Rgba;
  song?: string;
  tours: number;
  shots: number;
};

export type InspectorProps = {
  /** The selection's primary, as the document has it; `null` with nothing
   *  selected. */
  entity: Entity | null;
  /** The rest of the selection, by name: changes go to all of them. */
  others?: string[];
  parent?: string | null;
  children?: number;
  /** Fields edit: a live canvas. Elsewhere they read. */
  editable: boolean;
  /** What is marked on a field, by its path: `transform.position.1`. */
  marks?: Record<string, Mark>;
  /** Fields the selection holds different values in, by path: drawn "—". */
  mixed?: string[];
  /** With nothing selected: the world in a few lines. */
  world?: WorldSummary;
  /** What the fields and the header do. Without it the inspector only
   *  draws, as a design sheet has it; with it, only what is wired shows. */
  actions?: InspectorActions;
  /** On a sheet: a menu drawn open, and the behaviors drawn opened. */
  menu?: "behavior" | "shape" | "light";
  opened?: number[];
};

export type InspectorActions = {
  /** A field's edit, by its path; none where the field only reads. */
  field: <T>(path: string) => FieldEdit<T> | undefined;
  /** Flip a switch, by its path. */
  toggle: (path: string) => void;
  rename: (name: string) => void;
  /** The camera goes to it. */
  focus: () => void;
  /** Look through its camera: a shot. */
  look?: () => void;
  /** Select what holds it. */
  parent?: () => void;
  /** A shot's place on the clock, set as the timeline's drag sets it. */
  shotTime?: (inS: number, outS: number) => void;
  /** Take a behavior off it, by its place in the list. */
  unbehave?: (i: number) => void;
  /** Its modulation's track on the timeline, shown — where there is one. */
  track?: (i: number) => (() => void) | undefined;
};

const RAD = Math.PI / 180;

/** An edit in degrees of a field the format keeps in radians. */
const inDegrees = (edit?: FieldEdit<number>): FieldEdit<number> | undefined =>
  edit && {
    held: (v, absolute) => edit.held(v * RAD, absolute),
    set: (v, absolute) => edit.set(v * RAD, absolute),
    cancel: edit.cancel,
    from: edit.from === undefined ? undefined : edit.from / RAD,
  };

/** An edit of a glow's colour that keeps how bright it is. */
const atStrength = (edit: FieldEdit<Rgba> | undefined, by: number, alpha: number): FieldEdit<Rgba> | undefined => {
  if (!edit) return undefined;
  const times = ([r, g, b]: Rgba): Rgba => [...[r, g, b].map((c) => Math.round(c * by * 1e4) / 1e4), alpha] as Rgba;
  return { held: (c, absolute) => edit.held(times(c), absolute), set: (c, absolute) => edit.set(times(c), absolute), cancel: edit.cancel };
};

const LABEL: Record<Kind, string> = { shape: "Shape", mesh: "Mesh", light: "Light", camera: "Camera", sound: "Sound", group: "Group" };
const MANY: Record<Kind, string> = { shape: "shapes", mesh: "meshes", light: "lights", camera: "cameras", sound: "sounds", group: "groups" };

/** The selected thing's fields as the world's document has them, part by
 *  part. On a live canvas a field edits — a drag or a typed value is one
 *  entry in the history, as a gesture is — and elsewhere it reads. */
export function Inspector({ entity, others = [], parent, children = 0, editable, marks = {}, mixed = [], world, actions, menu, opened = [] }: InspectorProps) {
  if (!entity) return <Nothing world={world} />;
  // On a sheet everything designed is drawn; in the app, what is wired.
  const drawn = !actions;
  const f = <T,>(path: string) => (editable ? actions?.field<T>(path) : undefined);
  const kind = kindOf(entity, children > 0);
  const m = (path: string) => marks[path];
  const t = entity.transform;
  const vec = (path: string, value: number[]) => value.map((v, i) => (mixed.includes(`${path}.${i}`) ? null : v));
  const shapeKind = entity.shape && Object.keys(entity.shape)[0];
  const cine = entity["ext-cinematography"];
  const shown = (path: string, value: number) => (mixed.includes(path) ? null : value);
  /** A shot's end on the clock, set through the timeline's own edit. */
  const timed = (end: "in" | "out"): FieldEdit<number> | undefined => {
    const shot = cine?.shot;
    const set = editable ? actions?.shotTime : undefined;
    if (!shot || !set) return undefined;
    return { held: () => {}, set: (v) => (end === "in" ? set(v, shot.out_s ?? v) : set(shot.in_s ?? 0, v)), cancel: () => {} };
  };
  return (
    <aside className="flex min-h-0 flex-col bg-panel">
      <header className="shrink-0 border-b border-line px-3 py-2.5">
        <div className="flex items-center gap-2">
          <Icon name={GLYPH[kind]} className={`size-4 shrink-0 ${TONE[kind]}`} />
          <Name name={entity.name} mark={m("name")} rename={editable ? actions?.rename : undefined} hover={editable && drawn} />
          {cine?.camera &&
            (drawn || actions?.look) &&
            (actions?.look ? (
              <button onClick={actions.look} className="flex shrink-0 items-center gap-1 rounded bg-camera/15 px-1.5 py-0.5 text-[10px] text-camera hover:bg-camera/25">
                <Icon name="camera" className="size-3" />
                Look through
              </button>
            ) : (
              <span className="flex shrink-0 items-center gap-1 rounded bg-camera/15 px-1.5 py-0.5 text-[10px] text-camera">
                <Icon name="camera" className="size-3" />
                Look through
              </span>
            ))}
          {actions ? (
            <button
              onClick={actions.focus}
              className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-muted ring-1 ring-line hover:bg-raised hover:text-ink"
              title="The camera goes to it"
            >
              <Icon name="aim" className="size-3" />
              Fly to
            </button>
          ) : (
            <span className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-muted ring-1 ring-line" title="The camera goes to it">
              <Icon name="aim" className="size-3" />
              Fly to
            </span>
          )}
        </div>
        <p className="mt-1 truncate pl-6 text-[11px] text-faint">
          #{entity.id} · {LABEL[kind]}
          {shapeKind && kind !== "shape" ? ` · ${shapeKind}` : kind === "shape" && shapeKind ? ` · ${shapeKind}` : ""}
          {parent && (
            <>
              {" · in "}
              {actions?.parent ? (
                <button onClick={actions.parent} className="text-muted underline decoration-line underline-offset-2 hover:text-ink">
                  {parent}
                </button>
              ) : (
                <span className="text-muted underline decoration-line underline-offset-2">{parent}</span>
              )}
            </>
          )}
          {children > 0 && ` · holds ${children}`}
        </p>
        {m("name")?.refused && <p className="mt-1 pl-6 text-[10px] leading-snug text-danger">{m("name")!.refused}</p>}
        {others.length > 0 && (
          <p className="mt-1.5 rounded bg-accent/10 px-2 py-1 text-[11px] leading-snug text-accent">
            And {others.length === 1 ? others[0] : `${others.slice(0, -1).join(", ")} and ${others.at(-1)}`}: a change here goes to all {others.length + 1}.
          </p>
        )}
        {!editable && (
          <p className="mt-1.5 rounded bg-raised px-2 py-1 text-[11px] leading-snug text-muted">
            This world was built from a file; open a <span className="text-ink">.world</span> package to change it here.
          </p>
        )}
      </header>
      {/* Keyed by the thing: another one starts at its top. */}
      <div key={entity.id} className="min-h-0 flex-1 overflow-y-auto pb-6">
        <Part title="Transform" icon="move" aside={t.visible === false ? "hidden" : undefined}>
          <Vec3Row
            label="Position"
            value={vec("transform.position", t.position)}
            unit="m"
            marks={[0, 1, 2].map((i) => m(`transform.position.${i}`))}
            hint="Drag a number to move it along that axis; click it to type one"
            edit={(i) => f<number>(`transform.position.${i}`)}
            step={0.01}
          />
          <Vec3Row
            label="Rotation"
            value={vec("transform.rotation_degrees", t.rotation_degrees ?? [0, 0, 0])}
            digits={0}
            unit="°"
            marks={[0, 1, 2].map((i) => m(`transform.rotation_degrees.${i}`))}
            edit={(i) => f<number>(`transform.rotation_degrees.${i}`)}
            step={1}
          />
          <Vec3Row
            label="Scale"
            value={vec("transform.scale", t.scale ?? [1, 1, 1])}
            marks={[0, 1, 2].map((i) => m(`transform.scale.${i}`))}
            edit={(i) => f<number>(`transform.scale.${i}`)}
            step={0.01}
            min={0.01}
          />
          <ToggleRow
            label="Visible"
            on={t.visible !== false}
            mark={m("transform.visible")}
            onToggle={editable && actions ? () => actions.toggle("transform.visible") : undefined}
          />
        </Part>

        {entity.shape && shapeKind && (
          <Part title="Shape" icon="cube">
            <div className="relative">
              <ChoiceRow label="Kind" value={shapeKind} mark={m("shape")} fixed={!drawn} />
              {menu === "shape" && (
                <div className="absolute top-full left-[96px] z-20 mt-0.5">
                  <KindMenu kinds={SHAPES} current={shapeKind} keeps={`Another kind keeps its size: ${extent(entity.shape).map((v) => v.toFixed(2)).join(" × ")} m.`} />
                </div>
              )}
            </div>
            {Object.entries(entity.shape[shapeKind]).map(
              ([key, value]) =>
                typeof value === "number" && (
                  <Row key={key} label={key.replace(/_/g, " ")} mark={m(`shape.${key}`)}>
                    {/* A prism's sides are counted, not measured. */}
                    <Num
                      value={shown(`shape.${key}`, value)}
                      unit={key === "sides" ? undefined : "m"}
                      wide
                      mark={m(`shape.${key}`)}
                      edit={f<number>(`shape.${key}`)}
                      {...(key === "sides" ? { digits: 0, step: 1, min: 3, max: 64, whole: true } : { step: 0.01, min: 0.01 })}
                    />
                  </Row>
                ),
            )}
          </Part>
        )}

        {entity.material && (
          <Part title="Material" icon="cube" tone="text-sound">
            {entity.material.color && <ColorRow label="Colour" value={entity.material.color} mark={m("material.color")} edit={f<Rgba>("material.color")} />}
            <SliderRow label="Roughness" value={entity.material.roughness ?? 0.5} mark={m("material.roughness")} edit={f<number>("material.roughness")} />
            <SliderRow label="Metallic" value={entity.material.metallic ?? 0} mark={m("material.metallic")} edit={f<number>("material.metallic")} />
            {entity.material.emissive && glows(entity.material.emissive) && (
              <ColorRow
                label="Glow"
                value={unit(entity.material.emissive)}
                strength={strength(entity.material.emissive)}
                mark={m("material.emissive")}
                edit={atStrength(f<Rgba>("material.emissive"), strength(entity.material.emissive), entity.material.emissive[3])}
              />
            )}
            {entity.material.base_color_texture && <TextRow label="Texture" value={entity.material.base_color_texture} mono />}
          </Part>
        )}

        {entity.light && (
          <Part title="Light" icon="bulb" tone="text-head">
            <div className="relative">
              <ChoiceRow label="Kind" value={entity.light.light_type} mark={m("light.light_type")} fixed={!drawn} />
              {menu === "light" && (
                <div className="absolute top-full left-[96px] z-20 mt-0.5">
                  <KindMenu
                    kinds={LIGHTS}
                    current={entity.light.light_type}
                    keeps="Another kind keeps its colour and how bright it is; a spot opens to 45°, looking the way the light faces."
                  />
                </div>
              )}
            </div>
            <ColorRow label="Colour" value={entity.light.color} mark={m("light.color")} edit={f<Rgba>("light.color")} />
            <Row label="Intensity" mark={m("light.intensity")}>
              <Num
                value={mixed.includes("light.intensity") ? null : entity.light.intensity}
                digits={0}
                unit={entity.light.light_type === "directional" ? "lux" : "lm"}
                wide
                mark={m("light.intensity")}
                edit={f<number>("light.intensity")}
                min={0}
              />
            </Row>
            {entity.light.range !== undefined && (
              <Row label="Range" mark={m("light.range")}>
                <Num value={shown("light.range", entity.light.range)} digits={1} unit="m" wide mark={m("light.range")} edit={f<number>("light.range")} step={0.1} min={0} />
              </Row>
            )}
            {entity.light.outer_angle !== undefined && (
              <Row label="Cone" mark={m("light.inner_angle") ?? m("light.outer_angle")}>
                {/* The inner cone stays inside the outer, and a spot opens to a right angle at most. */}
                <Num
                  value={shown("light.inner_angle", (entity.light.inner_angle ?? 0) / RAD)}
                  digits={0}
                  unit="°"
                  mark={m("light.inner_angle")}
                  edit={inDegrees(f<number>("light.inner_angle"))}
                  step={0.5}
                  min={0}
                  max={entity.light.outer_angle / RAD}
                />
                <span className="text-faint">to</span>
                <Num
                  value={shown("light.outer_angle", entity.light.outer_angle / RAD)}
                  digits={0}
                  unit="°"
                  mark={m("light.outer_angle")}
                  edit={inDegrees(f<number>("light.outer_angle"))}
                  step={0.5}
                  min={(entity.light.inner_angle ?? 0) / RAD}
                  max={90}
                />
              </Row>
            )}
            <ToggleRow
              label="Shadows"
              on={!!entity.light.shadows}
              mark={m("light.shadows")}
              onToggle={editable && actions ? () => actions.toggle("light.shadows") : undefined}
            />
          </Part>
        )}

        {cine?.camera && (
          <Part title="Camera" icon="camera" tone="text-camera">
            <Row label="Lens" mark={m("camera.focal_length_mm")}>
              <Num
                value={shown("camera.focal_length_mm", cine.camera.focal_length_mm ?? 35)}
                digits={0}
                unit="mm"
                wide
                mark={m("camera.focal_length_mm")}
                edit={f<number>("camera.focal_length_mm")}
                step={1}
                min={1}
              />
            </Row>
            <TextRow label="Sensor" value={`${(cine.camera.sensor_mm ?? [24.89, 18.66]).join(" × ")} mm`} />
            {cine.camera.aim && (
              <Vec3Row
                label="Aims at"
                value={cine.camera.aim.map((v, i) => shown(`camera.aim.${i}`, v))}
                digits={1}
                unit="m"
                marks={[0, 1, 2].map((i) => m(`camera.aim.${i}`))}
                edit={(i) => f<number>(`camera.aim.${i}`)}
                step={0.05}
              />
            )}
          </Part>
        )}
        {cine?.shot && (
          <Part title="Shot" icon="film" tone="text-shot" aside={cine.shot.scene && `scene ${cine.shot.scene}`}>
            <ChoiceRow label="Size" value={cine.shot.size ?? "—"} fixed={!drawn} />
            <Row label="On the clock" mark={m("shot.in_s") ?? m("shot.out_s")}>
              <Num value={cine.shot.in_s ?? 0} unit="s" mark={m("shot.in_s")} edit={timed("in")} step={0.05} min={0} max={(cine.shot.out_s ?? 0) - 0.1} />
              <span className="text-faint">to</span>
              <Num value={cine.shot.out_s ?? 0} unit="s" mark={m("shot.out_s")} edit={timed("out")} step={0.05} min={(cine.shot.in_s ?? 0) + 0.1} />
            </Row>
            {cine.shot.description && (
              <Row label="Says">
                <span className="text-[11px] leading-snug text-muted">{cine.shot.description}</span>
              </Row>
            )}
          </Part>
        )}

        {entity.mesh_asset && (
          <Part title="Mesh" icon="mesh" tone="text-accent">
            <TextRow label="File" value={entity.mesh_asset.path} mono />
          </Part>
        )}

        {((entity.behaviors?.length ?? 0) > 0 || (editable && drawn)) && (
          <Part title="Behaviors" icon="spin" tone="text-behavior" aside={editable && drawn && "+ add"}>
            {menu === "behavior" && (
              <div className="relative">
                <div className="absolute top-0 right-3 left-3 z-20">
                  <BehaviorMenu />
                </div>
              </div>
            )}
            {(entity.behaviors?.length ?? 0) === 0 && <p className="px-3 pb-1 text-muted">Nothing moves it on its own.</p>}
            {(entity.behaviors ?? []).map((behavior, i) => (
              <BehaviorCard
                key={i}
                behavior={behavior as Record<string, Record<string, unknown>>}
                open={opened.includes(i)}
                editable={editable}
                mark={m(`behaviors.${i}`)}
                drawn={drawn}
                remove={actions?.unbehave && (() => actions.unbehave!(i))}
              />
            ))}
          </Part>
        )}

        {(entity.modulations?.length ?? 0) > 0 && (
          <Part title="Moved by the song" icon="wave" tone="text-mod">
            {entity.modulations!.map((mod, i) => {
              const show = actions?.track?.(i);
              return (
                <div key={i} className={`mx-3 mb-1 flex items-center gap-2 rounded bg-raised px-2 py-1.5 ${m(`modulations.${i}`) ? "ring-1 ring-head/50" : ""}`}>
                  <span className="text-ink">{mod.target.replace(/_/g, " ")}</span>
                  <span className="text-faint">←</span>
                  <span className="text-mod">{signalName(mod.signal)}</span>
                  <span className="text-muted tabular-nums">
                    {mod.range[0]}–{mod.range[1]}
                  </span>
                  {drawn && (
                    <span className="ml-auto flex items-center gap-0.5 text-[10px] text-faint">
                      timeline <Icon name="out" className="size-3" />
                    </span>
                  )}
                  {show && (
                    <button onClick={show} title="Its track on the timeline" className="ml-auto flex items-center gap-0.5 text-[10px] text-faint hover:text-ink">
                      timeline <Icon name="out" className="size-3" />
                    </button>
                  )}
                </div>
              );
            })}
          </Part>
        )}

        {entity.audio && (
          <Part title="Sound" icon="speaker" tone="text-ambience">
            <ChoiceRow label="Kind" value={entity.audio.kind ?? "ambient"} fixed={!drawn} />
            <TextRow label="Source" value={sourceName(entity.audio.source)} />
            <SliderRow label="Volume" value={entity.audio.volume ?? 1} max={Math.max(1, entity.audio.volume ?? 1)} mark={m("audio.volume")} edit={f<number>("audio.volume")} />
            {entity.audio.radius !== undefined && (
              <Row label="Heard to" mark={m("audio.radius")}>
                <Num value={shown("audio.radius", entity.audio.radius)} digits={0} unit="m" wide mark={m("audio.radius")} edit={f<number>("audio.radius")} step={0.5} min={0} />
              </Row>
            )}
          </Part>
        )}
      </div>
    </aside>
  );
}

/** The thing's name. With `rename`, a click types a new one: Enter sets
 *  it, Esc lets it go. */
function Name({ name, mark, rename, hover }: { name: string; mark?: Mark; rename?: (name: string) => void; hover: boolean }) {
  const [typing, setTyping] = useState<string | null>(null);
  const dropped = useRef(false);
  const look = `min-w-0 flex-1 truncate rounded px-1 text-[13px] font-semibold text-ink ${ring(mark)}`;
  if (!rename) return <span className={`${look} ${hover ? "hover:bg-raised" : ""}`}>{name}</span>;
  if (typing === null)
    return (
      <button onClick={() => setTyping(name)} title="Rename" className={`${look} text-left hover:bg-raised`}>
        {name}
      </button>
    );
  // Leaving the box sets it, as a number's does; Esc leaves it unset.
  const leave = () => {
    const next = typing.trim();
    setTyping(null);
    if (dropped.current) dropped.current = false;
    else if (next && next !== name) rename(next);
  };
  return (
    <input
      autoFocus
      value={typing}
      aria-label="Name"
      onFocus={(e) => e.currentTarget.select()}
      onChange={(e) => setTyping(e.currentTarget.value)}
      onKeyDown={(e) => {
        if (e.key === "Escape") dropped.current = true;
        if (e.key === "Enter" || e.key === "Escape") e.currentTarget.blur();
      }}
      onBlur={leave}
      className={`${look} bg-raised outline-none ring-1 ring-accent/60`}
    />
  );
}

/** With nothing selected, the world in a few lines and how to select. */
function Nothing({ world }: { world?: WorldSummary }) {
  return (
    <aside className="flex min-h-0 flex-col bg-panel">
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
        {world && (
          <>
            <p className="text-[13px] font-semibold text-ink">{world.name}</p>
            <div className="mt-2 grid grid-cols-2 gap-1">
              {world.counts.map(([kind, n]) => (
                <span key={kind} className="flex items-center gap-1.5 rounded bg-raised px-2 py-1">
                  <Icon name={GLYPH[kind]} className={`size-3.5 ${TONE[kind]}`} />
                  <span className="text-ink tabular-nums">{n}</span>
                  <span className="text-muted">{n === 1 ? LABEL[kind].toLowerCase() : MANY[kind]}</span>
                </span>
              ))}
            </div>
            <div className="mt-3 space-y-1 text-muted">
              {world.sky && (
                <p className="flex items-center gap-2">
                  <span className="size-3 rounded-sm ring-1 ring-black/40" style={{ background: hex(world.sky) }} /> sky {hex(world.sky)}
                </p>
              )}
              {world.song && (
                <p className="flex items-center gap-2">
                  <Icon name="music" className="size-3.5 text-sound" /> {world.song}
                </p>
              )}
              <p className="flex items-center gap-2">
                <Icon name="film" className="size-3.5 text-shot" /> {world.shots} shots · {world.tours} tour{world.tours === 1 ? "" : "s"}
              </p>
            </div>
          </>
        )}
        <p className="mt-4 leading-relaxed text-faint">Select a thing in the world or above to see what it is made of; shift-click adds to the selection.</p>
      </div>
    </aside>
  );
}

const glows = (c: Rgba) => Math.max(c[0], c[1], c[2]) > 0;
const strength = (c: Rgba) => Math.max(1, c[0], c[1], c[2]);
const unit = (c: Rgba): Rgba => {
  const s = strength(c);
  return [c[0] / s, c[1] / s, c[2] / s, 1];
};

function signalName(signal: string | Record<string, unknown>): string {
  if (typeof signal === "string") return signal;
  const [kind, value] = Object.entries(signal)[0];
  if (kind === "stem") return `${value} stem`;
  if (kind === "oscillator") return `sine ${(value as { frequency: number }).frequency} Hz`;
  return kind;
}

function sourceName(source: Record<string, Record<string, number>> | string): string {
  if (typeof source === "string") return source;
  const [kind, fields] = Object.entries(source)[0];
  const said = Object.entries(fields)
    .map(([k, v]) => `${k.replace(/_/g, " ")} ${v}`)
    .join(", ");
  return said ? `${kind} · ${said}` : kind;
}
