import type { Vec3 } from "./model";

// Another kind for a shape or a light. A shape keeps the size it fills —
// a cylinder 0.2 m across and 0.3 m tall becomes a cuboid 0.2 × 0.3 × 0.2 —
// and a light its colour and how bright it is.

type Shape = Record<string, Record<string, number>>;

export const SHAPES = ["Cuboid", "Sphere", "Cylinder", "Cone", "Capsule", "Torus", "Plane", "Pyramid", "Wedge", "Prism", "Tetrahedron", "Icosahedron"];

export const LIGHTS = ["point", "spot", "directional"];

/** The size a shape fills, x × y × z in metres. */
export function extent(shape: Shape): Vec3 {
  const [kind, f] = Object.entries(shape)[0] ?? ["", {}];
  const r = (k: string) => f[k] ?? 0;
  switch (kind) {
    case "Cuboid":
    case "Wedge":
      return [r("x"), r("y"), r("z")];
    case "Sphere":
    case "Tetrahedron":
    case "Icosahedron":
      return [2 * r("radius"), 2 * r("radius"), 2 * r("radius")];
    case "Cylinder":
    case "Cone":
    case "Prism":
      return [2 * r("radius"), r("height"), 2 * r("radius")];
    case "Capsule":
      return [2 * r("radius"), 2 * (r("half_length") + r("radius")), 2 * r("radius")];
    case "Torus":
      return [2 * (r("major_radius") + r("minor_radius")), 2 * r("minor_radius"), 2 * (r("major_radius") + r("minor_radius"))];
    case "Plane":
      return [r("x"), 0, r("z")];
    case "Pyramid":
      return [r("base_x"), r("height"), r("base_z")];
    default:
      return [1, 1, 1];
  }
}

/** A shape of `kind` filling `size`. */
export function reshaped(kind: string, [x, y, z]: Vec3): Shape {
  const across = Math.min(x, z) / 2;
  const round = (v: number) => Math.round(v * 1000) / 1000;
  const s = (fields: Record<string, number>): Shape => ({ [kind]: Object.fromEntries(Object.entries(fields).map(([k, v]) => [k, round(v)])) });
  switch (kind) {
    case "Cuboid":
    case "Wedge":
      return s({ x, y, z });
    case "Sphere":
    case "Tetrahedron":
    case "Icosahedron":
      return s({ radius: Math.min(x, y, z) / 2 });
    case "Cylinder":
    case "Cone":
      return s({ radius: across, height: y });
    case "Prism":
      return s({ sides: 6, radius: across, height: y });
    case "Capsule":
      return s({ radius: across, half_length: Math.max(0, y / 2 - across) });
    case "Torus":
      return s({ major_radius: across * 0.7, minor_radius: Math.min(across * 0.3, y / 2) });
    case "Plane":
      return s({ x, z });
    case "Pyramid":
      return s({ base_x: x, base_z: z, height: y });
    default:
      return s({});
  }
}

/** A kind's menu: the kinds, the current one marked, and what a change
 *  keeps. */
export function KindMenu({ kinds, current, keeps }: { kinds: string[]; current: string; keeps: string }) {
  return (
    <div className="w-[200px] overflow-hidden rounded-md bg-raised py-1 shadow-[0_6px_24px_rgba(0,0,0,0.5)] ring-1 ring-line">
      <div className="grid grid-cols-2">
        {kinds.map((kind) => (
          <span key={kind} className={`truncate px-2.5 py-1 ${kind === current ? "bg-accent/15 text-accent" : "text-ink"}`}>
            {kind}
          </span>
        ))}
      </div>
      <p className="mt-1 border-t border-line px-2.5 pt-1.5 pb-0.5 text-[10px] leading-snug text-faint">{keeps}</p>
    </div>
  );
}
