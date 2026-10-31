// The harbour again, as its document has it: the entities of the timeline
// sheet's world in the format's own JSON — what manifest.json would hold —
// for the outliner and the inspector to draw.

import type { Entity, Kind } from "../src/scene/model";
import { tree } from "../src/scene/model";

const at = (x: number, y: number, z: number) => ({ position: [x, y, z] as [number, number, number] });

const post = (id: number, x: number): Entity => ({
  id,
  name: `post ${id - 32}`,
  parent: 32,
  transform: at(x, 0.6, 0),
  shape: { Cylinder: { radius: 0.12, height: 1.2 } },
  material: { color: [0.32, 0.24, 0.18, 1], roughness: 0.9 },
});

const stone = (id: number, x: number, z: number): Entity => ({
  id,
  name: `stone ${id - 10}`,
  parent: 10,
  transform: { position: [x, 0.4, z], rotation_degrees: [0, (id * 37) % 90, 0] },
  shape: { Icosahedron: { radius: 0.8 } },
  material: { color: [0.45, 0.44, 0.42, 1], roughness: 1 },
});

const shot = (id: number, name: string, size: string, lens: number, from: number, to: number, x: number, z: number): Entity => ({
  id,
  name,
  parent: 60,
  transform: at(x, 1.6, z),
  "ext-cinematography": {
    camera: { focal_length_mm: lens, aim: [0, 1.4, -4] },
    shot: { scene: name[0], order: id - 60, size, in_s: from, out_s: to },
  },
});

export const entities: Entity[] = [
  { id: 1, name: "ground", transform: at(0, 0, 0), shape: { Plane: { x: 60, z: 60 } }, material: { color: [0.36, 0.4, 0.33, 1], roughness: 0.95 } },
  { id: 2, name: "sea", transform: at(0, 0.02, -18), shape: { Plane: { x: 80, z: 30 } }, material: { color: [0.12, 0.2, 0.3, 1], roughness: 0.2, metallic: 0.1 } },
  {
    id: 3,
    name: "sun",
    transform: at(0, 20, 0),
    light: { light_type: "directional", color: [1, 0.78, 0.62, 1], intensity: 9000, shadows: true, direction: [-0.4, -0.6, -0.5] },
  },
  { id: 10, name: "breakwater", transform: at(-6, 0, -6) },
  stone(11, -1.5, 0.2),
  stone(12, 0, -0.6),
  stone(13, 1.4, 0.3),
  stone(14, 2.8, -0.4),
  stone(15, 4.1, 0.1),
  { id: 20, name: "lighthouse", transform: at(-8, 0, -7) },
  {
    id: 21,
    name: "tower",
    parent: 20,
    transform: at(0, 4, 0),
    shape: { Cylinder: { radius: 0.9, height: 8 } },
    material: { color: [0.93, 0.92, 0.88, 1], roughness: 0.7 },
  },
  {
    id: 22,
    name: "lamp room",
    parent: 20,
    transform: at(0, 8.6, 0),
    shape: { Cylinder: { radius: 0.7, height: 1.2 } },
    material: { color: [0.8, 0.9, 1, 0.35], roughness: 0.05, metallic: 0.2 },
  },
  {
    id: 23,
    name: "lamp",
    parent: 20,
    transform: at(0, 8.6, 0),
    light: { light_type: "point", color: [1, 0.86, 0.6, 1], intensity: 60000, range: 40, shadows: false },
  },
  {
    id: 24,
    name: "beam",
    parent: 20,
    transform: { position: [0, 8.6, 0], rotation_degrees: [-6, 0, 0] },
    light: { light_type: "spot", color: [1, 0.92, 0.75, 1], intensity: 400000, range: 120, inner_angle: 0.05, outer_angle: 0.12, shadows: false },
    behaviors: [{ Spin: { axis: [0, 1, 0], speed: 60 } }],
  },
  { id: 30, name: "jetty", transform: at(4, 0, -3) },
  {
    id: 31,
    name: "planks",
    parent: 30,
    transform: at(0, 0.5, -4),
    shape: { Cuboid: { x: 1.8, y: 0.12, z: 9 } },
    material: { color: [0.5, 0.38, 0.27, 1], roughness: 0.85, base_color_texture: "4be2c1…f09a.png" },
  },
  { id: 32, name: "posts", parent: 30, transform: at(0, 0, -4) },
  post(33, -0.8),
  post(34, 0.8),
  post(35, -0.8),
  post(36, 0.8),
  {
    id: 37,
    name: "lantern",
    parent: 30,
    transform: at(0.7, 1.35, -8.4),
    shape: { Cylinder: { radius: 0.1, height: 0.3 } },
    material: { color: [0.2, 0.2, 0.2, 1], roughness: 0.6, metallic: 0.4, emissive: [4, 2.8, 1.2, 1] },
    modulations: [{ target: "emissive", signal: "beat", range: [0.4, 2] }],
  },
  {
    id: 38,
    name: "lantern light",
    parent: 37,
    transform: at(0, 0, 0),
    light: { light_type: "point", color: [1, 0.7, 0.4, 1], intensity: 800, range: 8, shadows: true },
    modulations: [{ target: "light_intensity", signal: "beat", range: [0.4, 2] }],
  },
  {
    id: 40,
    name: "boat",
    transform: { position: [9, 0.1, -10], rotation_degrees: [0, -24, 0] },
    mesh_asset: { path: "9c1d…77e2.glb" },
    behaviors: [{ Bob: { axis: [0, 1, 0], amplitude: 0.12, frequency: 0.35 } }],
    modulations: [{ target: "offset_y", signal: { stem: "bass" }, range: [-0.1, 0.1] }],
  },
  { id: 50, name: "gulls", transform: at(-8, 12, -7) },
  { id: 51, name: "gull", parent: 50, transform: at(3, 0, 0), mesh_asset: { path: "ae31…01b4.glb" }, behaviors: [{ Orbit: { center: "lighthouse", radius: 3, speed: 30 } }] },
  { id: 52, name: "gull 2", parent: 50, transform: at(-3, 1, 0), mesh_asset: { path: "ae31…01b4.glb" }, behaviors: [{ Orbit: { center: "lighthouse", radius: 4.5, speed: -22 } }] },
  { id: 60, name: "shots", transform: at(0, 0, 0) },
  shot(61, "1A", "WS", 24, 0, 19.2, 6, 10),
  shot(62, "1B", "MS", 40, 19.2, 36.4, 3, 4),
  shot(63, "2A", "WS", 18, 36.4, 51.84, -4, 9),
  {
    ...shot(64, "2B", "OTS", 35, 51.84, 82.56, 2.5, -1),
    "ext-cinematography": {
      camera: { focal_length_mm: 35, sensor_mm: [24.89, 18.66], aim: [4.7, 1.3, -11.4] },
      shot: { scene: "2", order: 4, size: "OTS", in_s: 51.84, out_s: 82.56, description: "Over her shoulder to the lantern as the bridge lifts." },
    },
  },
  shot(65, "3A", "WS", 18, 82.56, 115.2, -9, 12),
  { id: 70, name: "ocean", transform: at(0, 0, -20), audio: { kind: "ambient", source: { Ocean: { wave_size: 0.5 } }, volume: 0.6 } },
];

export const harbour = tree(entities);

export const byId = (id: number) => entities.find((e) => e.id === id)!;

export const counts: [Kind, number][] = (() => {
  const n = new Map<Kind, number>();
  for (const node of harbour.nodes.values()) n.set(node.kind, (n.get(node.kind) ?? 0) + 1);
  return (["shape", "mesh", "light", "camera", "sound", "group"] as Kind[]).flatMap((kind) => (n.get(kind) ? [[kind, n.get(kind)!] as [Kind, number]] : []));
})();
