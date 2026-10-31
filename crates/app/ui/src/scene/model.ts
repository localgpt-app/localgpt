// What the outliner and the inspector draw: the world's entities as its
// document has them — the format's own JSON, as manifest.json holds it —
// and the tree they make. A panel edits by sending a patch for one entity,
// as ModifyEntity takes it; nothing here keeps a second copy of the world.

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
