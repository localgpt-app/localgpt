// The ext-cinematography reference implementation (extension version 0.2).
//
// Spec: spec/extensions/cinematography.md. An entity carrying
// `ext-cinematography.camera` is a camera — a filmback and a lens — and
// one also carrying `ext-cinematography.shot` is a setup in the shot
// list. What this file provides is the contract's executable half:
//
//   - the normative crop math (frame size, horizontal and vertical FOV)
//     — Unreal's crop-to-aspect rule written out;
//   - the view (aim look-at, +Y up, else the entity's local −Z) and a
//     pinhole projection, so "is it in frame" is a predicate;
//   - the shot list (shot.order, ties by entity id);
//   - the outcome runner for conformance/outcomes/*.json.
//
// Conformance is math, not pixels: implementations agree on the
// numbers, within the stated tolerance — never on rendered output.

/** @typedef {import('./index.js').Vec3} Vec3 */
/** @typedef {import('./index.js').WorldEntity} WorldEntity */
/** @typedef {import('./index.js').WorldManifest} WorldManifest */

/** The extension this module implements. */
export const EXTENSION_NAME = "ext-cinematography";

/** The extension version this module implements. */
export const EXTENSION_VERSION = "0.2.0";

/** The default sensor (filmback): Super 35, [w, h] in mm. */
export const DEFAULT_SENSOR = [24.89, 18.66];

/** The default focal length, in mm. */
export const DEFAULT_FOCAL_LENGTH = 35;

/**
 * An entity's `ext-cinematography.camera` component with every default
 * applied, or `null` when the entity is no camera.
 * @typedef {object} CameraComponent
 * @property {number[]} sensor_mm
 * @property {number} focal_length_mm
 * @property {number|undefined} aspect_ratio
 * @property {number} squeeze
 * @property {number[]|undefined} aim
 * @property {number|undefined} focus_distance_m
 * @property {number|undefined} f_stop
 */

/**
 * A camera's frame: the largest rectangle of the aspect inside the
 * desqueezed sensor, centred, and the fields of view it gives the lens.
 * @typedef {object} CameraFrame
 * @property {number} width_mm frame width (desqueezed)
 * @property {number} height_mm frame height
 * @property {number} hfov_degrees horizontal field of view
 * @property {number} vfov_degrees vertical field of view
 * @property {number} aspect frame width / height
 */

/**
 * A camera's view: where it stands and which way it looks.
 * @typedef {object} CameraView
 * @property {Vec3} position world point
 * @property {Vec3} forward unit vector, the look direction
 * @property {Vec3} right unit vector
 * @property {Vec3} up unit vector
 */

/**
 * An entity's `ext-cinematography.camera` with defaults applied per
 * absent field (`{}` is a 35 mm on Super 35), or `null` when the entity
 * carries no camera component.
 * @param {WorldEntity} entity
 * @returns {CameraComponent|null}
 */
export function cameraOf(entity) {
  const camera = /** @type {any} */ (entity)?.[EXTENSION_NAME]?.camera;
  if (camera === null || typeof camera !== "object") return null;
  return {
    sensor_mm: camera.sensor_mm ?? [...DEFAULT_SENSOR],
    focal_length_mm: camera.focal_length_mm ?? DEFAULT_FOCAL_LENGTH,
    aspect_ratio: camera.aspect_ratio,
    squeeze: camera.squeeze ?? 1,
    aim: camera.aim,
    focus_distance_m: camera.focus_distance_m,
    f_stop: camera.f_stop,
  };
}

/**
 * The normative crop math (spec "Derived values"): with sensor `w × h`,
 * squeeze `s`, focal length `f` and aspect `a` — desqueezed sensor
 * aspect `A = w·s / h`; frame `W = w·s · min(1, a/A)`,
 * `H = h · min(1, A/a)` (no aspect: the whole desqueezed sensor);
 * FOVs `2·atan(W / 2f)`, `2·atan(H / 2f)`. Cropping never widens the
 * frame past the sensor, it only trims.
 * @param {CameraComponent} camera
 * @returns {CameraFrame}
 */
export function frameOf(camera) {
  const [w, h] = camera.sensor_mm;
  const s = camera.squeeze;
  const f = camera.focal_length_mm;
  const a = camera.aspect_ratio;
  const A = (w * s) / h;
  const width_mm = w * s * Math.min(1, a ? a / A : 1);
  const height_mm = h * Math.min(1, a ? A / a : 1);
  const deg = (/** @type {number} */ rad) => (rad * 180) / Math.PI;
  return {
    width_mm,
    height_mm,
    hfov_degrees: deg(2 * Math.atan(width_mm / (2 * f))),
    vfov_degrees: deg(2 * Math.atan(height_mm / (2 * f))),
    aspect: width_mm / height_mm,
  };
}

/** @param {Vec3} a @param {Vec3} b @returns {Vec3} */
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
/** @param {Vec3} a @param {Vec3} b @returns {Vec3} */
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
/** @param {Vec3} a @param {Vec3} b */
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
/** @param {Vec3} v @returns {Vec3} */
const norm = (v) => {
  const l = Math.hypot(v[0], v[1], v[2]);
  return [v[0] / l, v[1] / l, v[2] / l];
};

/** Rotate `v` by intrinsic XYZ Euler degrees — R = Rx·Ry·Rz, the
 *  transform convention of spec/world.md "Conventions".
 *  @param {Vec3} v @param {Vec3} degrees @returns {Vec3} */
function rotateIntrinsicXyz(v, degrees) {
  const [rx, ry, rz] = degrees.map((d) => (d * Math.PI) / 180);
  let [x, y, z] = v;
  // Rz first (rightmost), then Ry, then Rx — R = Rx·Ry·Rz applied to v.
  let c = Math.cos(rz), s = Math.sin(rz);
  [x, y] = [c * x - s * y, s * x + c * y];
  c = Math.cos(ry); s = Math.sin(ry);
  [x, z] = [c * x + s * z, -s * x + c * z];
  c = Math.cos(rx); s = Math.sin(rx);
  [y, z] = [c * y - s * z, s * y + c * z];
  return [x, y, z];
}

/**
 * A camera's view: with `aim`, the camera looks at the world point, +Y
 * up; without it, it looks down the entity's local −Z (the glTF /
 * three.js / Bevy convention), its frame carried by the entity's
 * rotation.
 * @param {WorldEntity} entity the camera entity (its transform places it)
 * @param {CameraComponent} camera
 * @returns {CameraView}
 */
export function viewOf(entity, camera) {
  const position = /** @type {Vec3} */ (entity.transform?.position ?? [0, 0, 0]);
  if (camera.aim) {
    const forward = norm(sub(/** @type {Vec3} */ (camera.aim), position));
    const right = norm(cross(forward, [0, 1, 0]));
    const up = cross(right, forward);
    return { position, forward, right, up };
  }
  const rotation = /** @type {Vec3} */ (entity.transform?.rotation_degrees ?? [0, 0, 0]);
  return {
    position,
    forward: rotateIntrinsicXyz([0, 0, -1], rotation),
    right: rotateIntrinsicXyz([1, 0, 0], rotation),
    up: rotateIntrinsicXyz([0, 1, 0], rotation),
  };
}

/**
 * Project a world point through a camera: the point in the camera's
 * normalized frame — `x` and `y` in frame-half units (inside when
 * `|x| <= 1` and `|y| <= 1`), `z` the distance along the look direction
 * (in front when positive).
 * @param {CameraView} view
 * @param {CameraFrame} frame
 * @param {CameraComponent} camera
 * @param {Vec3} point
 * @returns {{x: number, y: number, z: number}}
 */
export function project(view, frame, camera, point) {
  const d = sub(point, view.position);
  const z = dot(d, view.forward);
  const scale = camera.focal_length_mm / z;
  return {
    x: (dot(d, view.right) * scale) / (frame.width_mm / 2),
    y: (dot(d, view.up) * scale) / (frame.height_mm / 2),
    z,
  };
}

/**
 * The shot list: every entity carrying `ext-cinematography.shot`, as
 * `{id, name, shot}`, ordered by `shot.order` (absent last), ties by
 * entity id. The entity's name is the shot's name.
 * @param {WorldManifest|import('./index.js').FoldState} world
 * @returns {{id: number, name: string, shot: Record<string, any>}[]}
 */
export function shotList(world) {
  const shots = [];
  for (const entity of world.entities ?? []) {
    const shot = /** @type {any} */ (entity)?.[EXTENSION_NAME]?.shot;
    if (shot === null || typeof shot !== "object") continue;
    shots.push({ id: entity.id, name: entity.name, shot });
  }
  const rank = (/** @type {any} */ s) => (typeof s.shot.order === "number" ? s.shot.order : Number.MAX_SAFE_INTEGER);
  shots.sort((a, b) => rank(a) - rank(b) || a.id - b.id);
  return shots;
}

/**
 * Run a conformance outcomes document against a world: derive, then
 * check every assertion. This is the extension's conformance — math,
 * not pixels.
 *
 * @param {WorldManifest} manifest the world (parsed)
 * @param {any} outcomes {expect: [...]}
 * @returns {{ok: boolean, failures: string[]}}
 */
export function runOutcomes(manifest, outcomes) {
  /** @type {string[]} */ const failures = [];
  const byName = new Map((manifest.entities ?? []).map((e) => [e.name, e]));
  /** @param {string} name */
  const cameraEntity = (name) => {
    const entity = byName.get(name);
    const camera = entity && cameraOf(entity);
    if (!entity || !camera) failures.push(`no camera named ${name}`);
    return entity && camera ? { entity, camera, frame: frameOf(camera), view: viewOf(entity, camera) } : null;
  };
  /** @param {string} name */
  const originOf = (name) => {
    const entity = byName.get(name);
    if (!entity) failures.push(`no entity named ${name}`);
    return entity?.transform?.position ?? [0, 0, 0];
  };

  for (const assertion of outcomes.expect ?? []) {
    if (assertion.fov) {
      const { camera: name, hfov_degrees, vfov_degrees, aspect, tolerance = 0.001 } = assertion.fov;
      const c = cameraEntity(name);
      if (!c) continue;
      if (Math.abs(c.frame.hfov_degrees - hfov_degrees) > tolerance) {
        failures.push(`${name}: hfov ${c.frame.hfov_degrees} != ${hfov_degrees} (tolerance ${tolerance})`);
      }
      if (Math.abs(c.frame.vfov_degrees - vfov_degrees) > tolerance) {
        failures.push(`${name}: vfov ${c.frame.vfov_degrees} != ${vfov_degrees} (tolerance ${tolerance})`);
      }
      if (Math.abs(c.frame.aspect - aspect) > tolerance) {
        failures.push(`${name}: aspect ${c.frame.aspect} != ${aspect} (tolerance ${tolerance})`);
      }
    } else if (assertion.in_frame || assertion.out_of_frame) {
      const { camera: name, entity: target } = assertion.in_frame ?? assertion.out_of_frame;
      const c = cameraEntity(name);
      if (!c) continue;
      const p = project(c.view, c.frame, c.camera, originOf(target));
      const inside = p.z > 0 && Math.abs(p.x) <= 1 && Math.abs(p.y) <= 1;
      if (assertion.in_frame && !inside) failures.push(`${name}: ${target} projects outside the frame (${p.x.toFixed(3)}, ${p.y.toFixed(3)})`);
      if (assertion.out_of_frame && inside) failures.push(`${name}: ${target} projects inside the frame (${p.x.toFixed(3)}, ${p.y.toFixed(3)})`);
    } else if (Array.isArray(assertion.shot_list)) {
      const actual = shotList(manifest).map((s) => s.name);
      const expected = assertion.shot_list;
      if (actual.length !== expected.length || actual.some((n, i) => n !== expected[i])) {
        failures.push(`shot list ${JSON.stringify(actual)} != ${JSON.stringify(expected)}`);
      }
    } else {
      failures.push(`unknown assertion ${JSON.stringify(assertion)}`);
    }
  }
  return { ok: failures.length === 0, failures };
}
