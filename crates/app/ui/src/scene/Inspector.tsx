import { Icon } from "../Icon";
import { ChoiceRow, ColorRow, hex, type Mark, Num, Part, Row, SliderRow, TextRow, ToggleRow, Vec3Row } from "./fields";
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
};

const LABEL: Record<Kind, string> = { shape: "Shape", mesh: "Mesh", light: "Light", camera: "Camera", sound: "Sound", group: "Group" };
const MANY: Record<Kind, string> = { shape: "shapes", mesh: "meshes", light: "lights", camera: "cameras", sound: "sounds", group: "groups" };

/** The selected thing's fields as the world's document has them, part by
 *  part. On a live canvas a field edits — a drag or a typed value is one
 *  entry in the history, as a gesture is — and elsewhere it reads. */
export function Inspector({ entity, others = [], parent, children = 0, editable, marks = {}, mixed = [], world }: InspectorProps) {
  if (!entity) return <Nothing world={world} />;
  const kind = kindOf(entity, children > 0);
  const m = (path: string) => marks[path];
  const t = entity.transform;
  const vec = (path: string, value: number[]) => value.map((v, i) => (mixed.includes(`${path}.${i}`) ? null : v));
  const shapeKind = entity.shape && Object.keys(entity.shape)[0];
  const cine = entity["ext-cinematography"];
  return (
    <aside className="flex min-h-0 flex-col bg-panel">
      <header className="shrink-0 border-b border-line px-3 py-2.5">
        <div className="flex items-center gap-2">
          <Icon name={GLYPH[kind]} className={`size-4 shrink-0 ${TONE[kind]}`} />
          <span className={`min-w-0 flex-1 truncate rounded px-1 text-[13px] font-semibold text-ink ${editable ? "hover:bg-raised" : ""} ${m("name") ? "ring-1 ring-head/50" : ""}`}>
            {entity.name}
          </span>
          {cine?.camera && (
            <span className="flex shrink-0 items-center gap-1 rounded bg-camera/15 px-1.5 py-0.5 text-[10px] text-camera">
              <Icon name="camera" className="size-3" />
              Look through
            </span>
          )}
          <span className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] text-muted ring-1 ring-line" title="The camera goes to it">
            <Icon name="aim" className="size-3" />
            Fly to
          </span>
        </div>
        <p className="mt-1 truncate pl-6 text-[11px] text-faint">
          #{entity.id} · {LABEL[kind]}
          {shapeKind && kind !== "shape" ? ` · ${shapeKind}` : kind === "shape" && shapeKind ? ` · ${shapeKind}` : ""}
          {parent && (
            <>
              {" · in "}
              <span className="text-muted underline decoration-line underline-offset-2">{parent}</span>
            </>
          )}
          {children > 0 && ` · holds ${children}`}
        </p>
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
      <div className="min-h-0 flex-1 overflow-y-auto pb-6">
        <Part title="Transform" icon="move" aside={t.visible === false ? "hidden" : undefined}>
          <Vec3Row
            label="Position"
            value={vec("transform.position", t.position)}
            unit="m"
            marks={[0, 1, 2].map((i) => m(`transform.position.${i}`))}
            hint="Drag a label to move it along that axis"
          />
          <Vec3Row
            label="Rotation"
            value={vec("transform.rotation_degrees", t.rotation_degrees ?? [0, 0, 0])}
            digits={0}
            unit="°"
            marks={[0, 1, 2].map((i) => m(`transform.rotation_degrees.${i}`))}
          />
          <Vec3Row label="Scale" value={vec("transform.scale", t.scale ?? [1, 1, 1])} marks={[0, 1, 2].map((i) => m(`transform.scale.${i}`))} />
          <ToggleRow label="Visible" on={t.visible !== false} mark={m("transform.visible")} />
        </Part>

        {entity.shape && shapeKind && (
          <Part title="Shape" icon="cube">
            <ChoiceRow label="Kind" value={shapeKind} mark={m("shape")} />
            {Object.entries(entity.shape[shapeKind]).map(([key, value]) => (
              <Row key={key} label={key.replace(/_/g, " ")} mark={m(`shape.${key}`)}>
                <Num value={value} unit="m" wide mark={m(`shape.${key}`)} />
              </Row>
            ))}
          </Part>
        )}

        {entity.material && (
          <Part title="Material" icon="cube" tone="text-sound">
            {entity.material.color && <ColorRow label="Colour" value={entity.material.color} mark={m("material.color")} />}
            <SliderRow label="Roughness" value={entity.material.roughness ?? 0.5} mark={m("material.roughness")} />
            <SliderRow label="Metallic" value={entity.material.metallic ?? 0} mark={m("material.metallic")} />
            {entity.material.emissive && glows(entity.material.emissive) && (
              <ColorRow label="Glow" value={unit(entity.material.emissive)} strength={strength(entity.material.emissive)} mark={m("material.emissive")} />
            )}
            {entity.material.base_color_texture && <TextRow label="Texture" value={entity.material.base_color_texture} mono />}
          </Part>
        )}

        {entity.light && (
          <Part title="Light" icon="bulb" tone="text-head">
            <ChoiceRow label="Kind" value={entity.light.light_type} mark={m("light.light_type")} />
            <ColorRow label="Colour" value={entity.light.color} mark={m("light.color")} />
            <Row label="Intensity" mark={m("light.intensity")}>
              <Num value={entity.light.intensity} digits={0} unit={entity.light.light_type === "directional" ? "lux" : "lm"} wide mark={m("light.intensity")} />
            </Row>
            {entity.light.range !== undefined && (
              <Row label="Range">
                <Num value={entity.light.range} digits={1} unit="m" wide />
              </Row>
            )}
            {entity.light.outer_angle !== undefined && (
              <Row label="Cone">
                <Num value={((entity.light.inner_angle ?? 0) * 180) / Math.PI} digits={0} unit="°" />
                <span className="text-faint">to</span>
                <Num value={(entity.light.outer_angle * 180) / Math.PI} digits={0} unit="°" />
              </Row>
            )}
            <ToggleRow label="Shadows" on={!!entity.light.shadows} />
          </Part>
        )}

        {cine?.camera && (
          <Part title="Camera" icon="camera" tone="text-camera">
            <Row label="Lens">
              <Num value={cine.camera.focal_length_mm ?? 35} digits={0} unit="mm" wide mark={m("camera.focal_length_mm")} />
            </Row>
            <TextRow label="Sensor" value={`${(cine.camera.sensor_mm ?? [24.89, 18.66]).join(" × ")} mm`} />
            {cine.camera.aim && <Vec3Row label="Aims at" value={cine.camera.aim} digits={1} unit="m" />}
          </Part>
        )}
        {cine?.shot && (
          <Part title="Shot" icon="film" tone="text-shot" aside={cine.shot.scene && `scene ${cine.shot.scene}`}>
            <ChoiceRow label="Size" value={cine.shot.size ?? "—"} />
            <Row label="On the clock" mark={m("shot.in_s")}>
              <Num value={cine.shot.in_s ?? 0} unit="s" mark={m("shot.in_s")} />
              <span className="text-faint">to</span>
              <Num value={cine.shot.out_s ?? 0} unit="s" mark={m("shot.out_s")} />
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

        {(entity.behaviors?.length ?? 0) > 0 && (
          <Part title="Behaviors" icon="spin" tone="text-behavior" aside={editable && "+ add"}>
            {entity.behaviors!.map((behavior, i) => {
              const [name, fields] = Object.entries(behavior)[0];
              return (
                <div key={i} className={`mx-3 mb-1 flex items-center gap-2 rounded bg-raised px-2 py-1.5 ${m(`behaviors.${i}`) ? "ring-1 ring-head/50" : ""}`}>
                  <span className="font-medium text-behavior">{name}</span>
                  <span className="min-w-0 flex-1 truncate text-muted">{describeBehavior(name, fields)}</span>
                  {editable && <Icon name="close" className="size-3 shrink-0 text-faint" />}
                </div>
              );
            })}
          </Part>
        )}

        {(entity.modulations?.length ?? 0) > 0 && (
          <Part title="Moved by the song" icon="wave" tone="text-mod">
            {entity.modulations!.map((mod, i) => (
              <div key={i} className="mx-3 mb-1 flex items-center gap-2 rounded bg-raised px-2 py-1.5">
                <span className="text-ink">{mod.target.replace(/_/g, " ")}</span>
                <span className="text-faint">←</span>
                <span className="text-mod">{signalName(mod.signal)}</span>
                <span className="text-muted tabular-nums">
                  {mod.range[0]}–{mod.range[1]}
                </span>
                <span className="ml-auto flex items-center gap-0.5 text-[10px] text-faint">
                  timeline <Icon name="out" className="size-3" />
                </span>
              </div>
            ))}
          </Part>
        )}

        {entity.audio && (
          <Part title="Sound" icon="speaker" tone="text-ambience">
            <ChoiceRow label="Kind" value={entity.audio.kind ?? "ambient"} />
            <TextRow label="Source" value={sourceName(entity.audio.source)} />
            <SliderRow label="Volume" value={entity.audio.volume ?? 1} />
            {entity.audio.radius !== undefined && (
              <Row label="Heard to">
                <Num value={entity.audio.radius} digits={0} unit="m" wide />
              </Row>
            )}
          </Part>
        )}
      </div>
    </aside>
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

/** A behavior in a line, as a person says it. */
function describeBehavior(name: string, f: Record<string, unknown>): string {
  const n = (k: string) => Number(f[k] ?? 0);
  const axis = (k: string) => {
    const a = f[k] as number[] | undefined;
    if (!a) return "Y";
    const i = a.map(Math.abs).indexOf(Math.max(...a.map(Math.abs)));
    return "XYZ"[i];
  };
  switch (name) {
    case "Spin":
      return `${n("speed")}° a second about ${axis("axis")}`;
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
