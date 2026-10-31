// What the outliner and the inspector draw: the world's entities as its
// document has them — the format's own JSON, as manifest.json holds it —
// and the tree they make. A panel edits by sending a merge patch for each
// thing it changes, as ModifyEntity takes it; nothing here keeps a second
// copy of the world.

export type Vec3 = [number, number, number];
export type Rgba = [number, number, number, number];

/** An entity as the format writes it: the fields the panels draw. */
export type Entity = {
  id: number;
  name: string;
  parent?: number | null;
  transform: { position: Vec3; rotation_degrees?: Vec3; scale?: Vec3; visible?: boolean };
  /** `{"Cuboid": {"x": 1, "y": 1, "z": 1}}` */
  shape?: Record<string, Record<string, number>>;
  material?: {
    color?: Rgba;
    metallic?: number;
    roughness?: number;
    /** Linear; above 1 it glows. */
    emissive?: Rgba;
    base_color_texture?: string;
    normal_map_texture?: string;
  };
  light?: {
    light_type: "directional" | "point" | "spot";
    color: Rgba;
    intensity: number;
    range?: number;
    inner_angle?: number;
    outer_angle?: number;
    shadows?: boolean;
    direction?: Vec3;
  };
  /** `[{"Spin": {"axis": [0, 1, 0], "speed": 60}}]` */
  behaviors?: Record<string, Record<string, unknown>>[];
  modulations?: { target: string; signal: string | Record<string, unknown>; range: [number, number]; smoothing?: number }[];
  audio?: { kind?: string; source: Record<string, Record<string, number>> | string; volume?: number; radius?: number };
  mesh_asset?: { path: string };
  "ext-cinematography"?: {
    camera?: { focal_length_mm?: number; sensor_mm?: [number, number]; aim?: Vec3; aim_entity?: number };
    shot?: { scene?: string; order?: number; size?: string; in_s?: number; out_s?: number; description?: string };
  };
};

export type Kind = "group" | "shape" | "light" | "camera" | "mesh" | "sound";

export const KINDS: Kind[] = ["shape", "mesh", "light", "camera", "sound", "group"];

/** What kind of thing an entity is, by what it carries: a camera before a
 *  light before a mesh before a shape; a sound with nothing else to show; a
 *  group when all it holds is children. */
export function kindOf(entity: Entity, hasChildren: boolean): Kind {
  if (entity["ext-cinematography"]?.camera) return "camera";
  if (entity.light) return "light";
  if (entity.mesh_asset) return "mesh";
  if (entity.shape) return "shape";
  if (entity.audio) return "sound";
  return hasChildren ? "group" : "shape";
}

/** One row of the outliner. */
export type Node = {
  id: number;
  name: string;
  kind: Kind;
  /** The shape's or the light's own kind, or the mesh's file: "Cuboid", "spot". */
  detail?: string;
  parent: number | null;
  /** In the document's order. */
  children: number[];
  visible: boolean;
  /** What moves it, counted. */
  behaviors: number;
  modulations: number;
  /** A camera's shot, when it is in the shot list: "2B". */
  shot?: string;
};

/** The tree a world's entities make: the roots and every node, children in
 *  the document's order. */
export function tree(entities: Entity[]): { nodes: Map<number, Node>; roots: number[] } {
  const ids = new Set(entities.map((e) => e.id));
  const children = new Map<number, number[]>();
  for (const e of entities) {
    if (e.parent != null && ids.has(e.parent)) children.set(e.parent, [...(children.get(e.parent) ?? []), e.id]);
  }
  const nodes = new Map<number, Node>();
  const roots: number[] = [];
  for (const e of entities) {
    const kids = children.get(e.id) ?? [];
    const kind = kindOf(e, kids.length > 0);
    const shot = e["ext-cinematography"]?.shot;
    nodes.set(e.id, {
      id: e.id,
      name: e.name,
      kind,
      detail:
        kind === "shape" ? Object.keys(e.shape ?? {})[0] : kind === "light" ? e.light?.light_type : kind === "mesh" ? e.mesh_asset?.path.split("/").pop() : undefined,
      parent: e.parent != null && ids.has(e.parent) ? e.parent : null,
      children: kids,
      visible: e.transform.visible !== false,
      behaviors: e.behaviors?.length ?? 0,
      modulations: e.modulations?.length ?? 0,
      shot: shot ? e.name : undefined,
    });
    if (e.parent == null || !ids.has(e.parent)) roots.push(e.id);
  }
  return { nodes, roots };
}

/** The nodes a filter keeps — what matches, and the way down to it — or
 *  `null` with no filter set. */
export function matching(nodes: Map<number, Node>, query: string, kinds: Kind[]): Set<number> | null {
  const q = query.trim().toLowerCase();
  if (!q && kinds.length === 0) return null;
  const keep = new Set<number>();
  for (const node of nodes.values()) {
    const named = !q || node.name.toLowerCase().includes(q);
    const kinded = kinds.length === 0 || kinds.includes(node.kind);
    if (!named || !kinded) continue;
    for (let at: Node | undefined = node; at && !keep.has(at.id); at = at.parent == null ? undefined : nodes.get(at.parent)) keep.add(at.id);
  }
  return keep;
}

/** The fields a patch merges into what is there; the rest it replaces. */
const MERGED = ["transform", "material", "light"];

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);

/** JSON merge patch: objects merge key by key, `null` removes, anything
 *  else replaces. */
function merged(now: unknown, change: unknown): unknown {
  if (!isObject(now) || !isObject(change)) return change;
  const out: Record<string, unknown> = { ...now };
  for (const [key, value] of Object.entries(change)) {
    if (value === null) delete out[key];
    else out[key] = merged(out[key], value);
  }
  return out;
}

/** The entity a patch would leave, folded as the authority folds it —
 *  what the inspector shows between letting go of a field and the
 *  document's answer. */
export function patched(entity: Entity, patch: Record<string, unknown>): Entity {
  const out: Record<string, unknown> = { ...entity };
  for (const [field, change] of Object.entries(patch)) {
    if (change === null) delete out[field];
    else out[field] = MERGED.includes(field) ? merged(out[field], change) : change;
  }
  return out as Entity;
}

/** A field's value by its path — `transform.position.1`, `shape.radius`
 *  (the shape's own kind is skipped), `camera.aim.0` (in the
 *  cinematography extension) — or `undefined` where there is none. */
export function valueAt(entity: Entity, path: string): unknown {
  const [part, ...rest] = path.split(".");
  let at: unknown =
    part === "shape"
      ? Object.values(entity.shape ?? {})[0]
      : part === "camera" || part === "shot"
        ? entity["ext-cinematography"]?.[part]
        : (entity as Record<string, unknown>)[part];
  for (const key of rest) at = isObject(at) || Array.isArray(at) ? (at as Record<string, unknown>)[key] : undefined;
  return at;
}

/** Every field an entity has, by the path the inspector marks it with —
 *  `transform.position.0`, `shape.radius`, `material.color`, `camera.aim`,
 *  `behaviors.1` — and its value, as text to compare. */
export function leaves(entity: Entity): Map<string, string> {
  const out = new Map<string, string>();
  const put = (path: string, value: unknown) => out.set(path, JSON.stringify(value ?? null));
  put("name", entity.name);
  put("parent", entity.parent ?? null);
  const t = entity.transform;
  t.position.forEach((v, i) => put(`transform.position.${i}`, v));
  (t.rotation_degrees ?? [0, 0, 0]).forEach((v, i) => put(`transform.rotation_degrees.${i}`, v));
  (t.scale ?? [1, 1, 1]).forEach((v, i) => put(`transform.scale.${i}`, v));
  put("transform.visible", t.visible !== false);
  const [kind, fields] = Object.entries(entity.shape ?? {})[0] ?? [];
  put("shape", kind ?? null);
  for (const [key, value] of Object.entries(fields ?? {})) put(`shape.${key}`, value);
  for (const part of ["material", "light", "audio"] as const)
    for (const [key, value] of Object.entries(entity[part] ?? {})) put(`${part}.${key}`, value);
  const cine = entity["ext-cinematography"];
  for (const part of ["camera", "shot"] as const) for (const [key, value] of Object.entries(cine?.[part] ?? {})) put(`${part}.${key}`, value);
  (entity.behaviors ?? []).forEach((b, i) => put(`behaviors.${i}`, b));
  (entity.modulations ?? []).forEach((m, i) => put(`modulations.${i}`, m));
  put("mesh_asset", entity.mesh_asset?.path ?? null);
  return out;
}

/** What changed between two documents, by entity: the paths whose values
 *  differ, and every path of a thing that is new. */
export function changes(before: Entity[], after: Entity[]): Map<number, Set<string>> {
  const was = new Map(before.map((e) => [e.id, leaves(e)]));
  const out = new Map<number, Set<string>>();
  for (const entity of after) {
    const now = leaves(entity);
    const then = was.get(entity.id);
    const paths = new Set([...now.keys(), ...(then?.keys() ?? [])].filter((path) => now.get(path) !== then?.get(path)));
    if (paths.size) out.set(entity.id, paths);
  }
  return out;
}
