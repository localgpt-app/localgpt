// world-viewer.js — the Open World Format reference 3D renderer.
//
// Provenance: born as localgpt's crates/world-export/js/world-viewer.js
// (Apache-2.0), which drew the conformance suite in production before this
// repository existed. This file is upstream now — the LocalGPT apps vendor
// it from the published package (their scripts/sync-viewer.sh, byte-checked
// in their CI) and songworld assembles it from the tarball; renderer
// changes release here first, on npm, and flow out. Keep it that way: one
// renderer, released once, consumed everywhere.
//
// Input: a `WorldManifest` as JSON (crate `localgpt-world-types`; schema in
// `crates/world-types/world.schema.json`). Output: a three.js scene that
// draws it the way the Bevy renderer (`localgpt-world-bevy`) does: colours are
// RGBA in 0..1, sRGB-encoded except `emissive` (linear); rotations XYZ Euler
// degrees; directional light intensity in lux, point and spot lights in
// lumens, spot angles in radians. The conventions are stated once, on
// `WorldManifest` in the schema.
//
// This file is embedded verbatim by `localgpt-world-export::html::generate_html`
// (Gen's `gen_export_html`, MD's `--export x.html`) and served as a module by
// localgpt.world. Keep it dependency-free beyond `three` and its addons, and
// never write the string "</" followed by "script" in it.
//
// Types: JSDoc (checked by `npm run build:types`); three's declarations come
// from @types/three at check time only — not a runtime dependency.
//
// Usage:
//   import { createWorldViewer } from './world-viewer.js';
//   const viewer = createWorldViewer(container, manifest, { assetBase: 'assets/' });

import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
import { cameraOf, frameOf, viewOf, shotList } from './cinematography.js';

/** @typedef {import('./index.js').Vec3} Vec3 */
/** @typedef {import('./index.js').WorldEntity} WorldEntity */
/** @typedef {import('./index.js').WorldManifest} WorldManifest */
/** @typedef {import('./index.js').WorldTransform} WorldTransform */
/** @typedef {import('./index.js').EnvironmentDef} EnvironmentDef */
/** @typedef {import('./index.js').Shape} Shape */

export const VIEWER_VERSION = '0.3.0';

/// Calibration between Bevy's light units and three.js', in one place.
/// Bevy renders physical units through an exposure; three's lights are
/// unitless, so one exposure applies to every light: a directional light of
/// DIRECTIONAL_LUX lux is intensity 1.0; point lights go lumens → candela
/// through the full sphere (÷ 4π); spot lights go lumens → candela through
/// the cone their outer angle sweeps (÷ 2π·(1 − cos(θ/2)), the conversion
/// spec/world.md "Conventions" asks renderers to make at draw time); then
/// ÷ DIRECTIONAL_LUX. Bevy's `GlobalAmbientLight.brightness` (default 80)
/// scales by AMBIENT_SCALE.
export const AMBIENT_SCALE = 0.0012;
export const DIRECTIONAL_LUX = 10000;
const LIGHT_EXPOSURE = 1 / DIRECTIONAL_LUX;

/** @type {{position: Vec3, look_at: Vec3, fov_degrees: number}} */
const DEFAULT_CAMERA = { position: [5, 5, 5], look_at: [0, 0, 0], fov_degrees: 45 };
const DEFAULT_MATERIAL = { color: [0.8, 0.8, 0.8, 1.0], metallic: 0.0, roughness: 0.5, emissive: [0, 0, 0, 0] };
const TAU = Math.PI * 2;

/**
 * Options for createWorldViewer.
 * @typedef {object} ViewerOptions
 * @property {string} [assetBase] URL prefix for mesh assets and the soundtrack file ('' = none: placeholders, silent)
 * @property {HTMLElement} [audioButton] element whose click toggles audio (shown when the world has sound)
 * @property {HTMLElement} [tourButton] element whose click starts/stops the first tour (shown when tours exist)
 * @property {HTMLElement} [tourCaption] element that shows waypoint descriptions
 * @property {HTMLElement} [triggerCaption] element for `show_text` trigger actions (default: tourCaption)
 * @property {boolean} [keyboard] WASD/Space/Shift navigation (default true)
 * @property {boolean} [embedApi] postMessage API for a parent frame (default: when framed)
 * @property {number} [ambientScale] override for AMBIENT_SCALE
 * @property {boolean} [preserveDrawingBuffer] keep the canvas readable after render
 * @property {string} [shot] an ext-cinematography camera's entity name: view through that shot, letterboxed to its frame
 */

/**
 * One entity as the viewer holds it: the definition it was built from,
 * its scene object, and its runtime state.
 * @typedef {object} EntityRecord
 * @property {WorldEntity} def
 * @property {THREE.Object3D} object
 * @property {THREE.Material|null} material
 * @property {THREE.Light|null} light
 * @property {{position: THREE.Vector3, scale: THREE.Vector3,
 *             emissive: THREE.Color|null, emissiveIntensity: number,
 *             lightIntensity: number, opacity: number}} base
 * @property {((dt: number, t: number) => void)[]} behaviors
 * @property {{def: any, target: string, s: number}[]} mods
 * @property {boolean} resetPosition
 * @property {boolean} resetScale
 * @property {TriggerState[]} triggers
 * @property {ViewerAnimation|null} [anim]
 */

/**
 * One trigger, as the runtime tracks it.
 * @typedef {object} TriggerState
 * @property {any} def
 * @property {any} area
 * @property {boolean} done
 * @property {boolean} inside
 * @property {number|null} last
 * @property {number} acc
 */

/**
 * An `animate` action in flight: lerp/slerp from a to b over duration.
 * @typedef {{kind: "position"|"scale", a: THREE.Vector3, b: THREE.Vector3, duration: number, t: number}
 *          |{kind: "rotation", a: THREE.Quaternion, b: THREE.Quaternion, duration: number, t: number}} ViewerAnimation
 */

/**
 * The viewer's audio state (null until the visitor turns sound on).
 * @typedef {object} AudioState
 * @property {AudioContext|null} ctx
 * @property {boolean} started
 * @property {(() => void)[]} stops
 * @property {HTMLAudioElement|null} element
 * @property {AnalyserNode|null} analyser
 * @property {Uint8Array<ArrayBuffer>|null} bins
 * @property {{rec: EntityRecord, gain: GainNode, volume: number}[]} spatial
 */

// ---------------------------------------------------------------------------
// Pure helpers (mirrors of the Rust ones in localgpt-world-types)
// ---------------------------------------------------------------------------

/**
 * Colour conventions, the same as the Bevy mapping: `color`, light colours,
 * background, fog and ambient are sRGB-encoded (what Gen's tools take and
 * `Color::srgba` reads); `emissive` is linear (`LinearRgba::new`).
 * @param {Vec3} [c]
 * @returns {THREE.Color}
 */
export function srgbColor(c) {
  const [r, g, b] = c || [1, 1, 1];
  return new THREE.Color().setRGB(r, g, b, THREE.SRGBColorSpace);
}

/** Linear RGBA array → three Color (three's working space is linear).
 * @param {Vec3} [c]
 * @returns {THREE.Color} */
export function linearColor(c) {
  const [r, g, b] = c || [1, 1, 1];
  return new THREE.Color(r, g, b);
}

/** `SoundtrackDef::energy_at` / `curve_at`: per-second curve, linear interpolation.
 * @param {number[]|null} curve
 * @param {number} t
 * @returns {number} */
export function curveAt(curve, t) {
  if (!curve || curve.length === 0) return 0;
  const n = curve.length;
  if (n === 1) return clamp01(curve[0]);
  const tt = Math.min(Math.max(t, 0), n - 1);
  const i = Math.floor(tt);
  const f = tt - i;
  if (i + 1 >= n) return clamp01(curve[n - 1]);
  return clamp01(curve[i] + (curve[i + 1] - curve[i]) * f);
}

/** `SoundtrackDef::beat_at`: 1 on a beat, decaying to 0 at the next.
 * @param {any} soundtrack
 * @param {number} t
 * @returns {number} */
export function beatAt(soundtrack, t) {
  const bpm = soundtrack?.bpm || 0;
  if (!(bpm > 0)) return 0;
  const period = 60 / bpm;
  const since = mod(t - (soundtrack.beat_offset || 0), period);
  return 1 - since / period;
}

/** `SoundtrackDef::section_at`.
 * @param {any} soundtrack
 * @param {number} t
 * @returns {number} */
export function sectionAt(soundtrack, t) {
  const sections = soundtrack?.sections || [];
  if (!(soundtrack?.duration > 0) || sections.length === 0) return 0;
  const frac = clamp01(t / soundtrack.duration);
  let idx = 0;
  for (let i = 0; i < sections.length; i++) if (sections[i] <= frac) idx = i;
  return idx;
}

/** `ModulationDef::factor`.
 * @param {any} def
 * @param {number} signal
 * @returns {number} */
export function modulationFactor(def, signal) {
  const [a, b] = def.range || [1, 1];
  return a + (b - a) * clamp01(signal);
}

/** @param {number} v @returns {number} */
function clamp01(v) { return Math.min(1, Math.max(0, v)); }
/** @param {number} a @param {number} n @returns {number} */
function mod(a, n) { return ((a % n) + n) % n; }
/**
 * Externally tagged enum, unpacked: "energy" → ["energy", null];
 * {stem: "drums"} → ["stem", "drums"].
 * @param {any} v
 * @returns {[string|null, any]}
 */
function variant(v) {
  if (typeof v === 'string') return [v, null];
  if (v && typeof v === 'object') { const k = Object.keys(v)[0]; return [k, v[k]]; }
  return [null, null];
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

/** @param {number[][][]} triangles
 *  @returns {THREE.BufferGeometry} */
function flatGeometry(triangles) {
  const positions = new Float32Array(triangles.length * 9);
  let o = 0;
  for (const tri of triangles) for (const v of tri) { positions[o++] = v[0]; positions[o++] = v[1]; positions[o++] = v[2]; }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  geo.computeVertexNormals();
  return geo;
}

/** A square-based pyramid centered on the origin: base at -h/2, apex at +h/2.
 * @param {number} bx @param {number} bz @param {number} h
 * @returns {THREE.BufferGeometry} */
function pyramidGeometry(bx, bz, h) {
  const hx = bx / 2, hz = bz / 2, hy = h / 2;
  const a = [-hx, -hy, -hz], b = [hx, -hy, -hz], c = [hx, -hy, hz], d = [-hx, -hy, hz], apex = [0, hy, 0];
  // Counter-clockwise seen from outside, so the sides are front faces.
  return flatGeometry([[b, a, apex], [c, b, apex], [d, c, apex], [a, d, apex], [a, b, d], [b, c, d]]);
}

/** A ramp: right-triangle profile in XY (vertical face at -x, slope down toward +x), extruded along Z.
 * @param {number} x @param {number} y @param {number} z
 * @returns {THREE.BufferGeometry} */
function wedgeGeometry(x, y, z) {
  const hx = x / 2, hy = y / 2, hz = z / 2;
  // Front (z = +hz) and back (z = -hz) triangles: A bottom-left, B bottom-right, C top-left.
  const Af = [-hx, -hy, hz], Bf = [hx, -hy, hz], Cf = [-hx, hy, hz];
  const Ab = [-hx, -hy, -hz], Bb = [hx, -hy, -hz], Cb = [-hx, hy, -hz];
  return flatGeometry([
    [Af, Bf, Cf], // front cap
    [Bb, Ab, Cb], // back cap
    [Ab, Bb, Bf], [Ab, Bf, Af], // bottom
    [Ab, Af, Cf], [Ab, Cf, Cb], // vertical face at -x
    [Bf, Bb, Cb], [Bf, Cb, Cf], // slope
  ]);
}

/** @param {Shape} [shape]
 *  @returns {THREE.BufferGeometry} */
export function createGeometry(shape) {
  const [kind, p] = variant(shape);
  switch (kind) {
    case 'Cuboid': return new THREE.BoxGeometry(p.x ?? 1, p.y ?? 1, p.z ?? 1);
    case 'Sphere': return new THREE.SphereGeometry(p.radius ?? 0.5, 32, 24);
    case 'Cylinder': return new THREE.CylinderGeometry(p.radius ?? 0.5, p.radius ?? 0.5, p.height ?? 1, 32);
    case 'Cone': return new THREE.ConeGeometry(p.radius ?? 0.5, p.height ?? 1, 32);
    case 'Capsule': return new THREE.CapsuleGeometry(p.radius ?? 0.5, (p.half_length ?? 0.5) * 2, 16, 32);
    case 'Torus': return new THREE.TorusGeometry(p.major_radius ?? 1, p.minor_radius ?? 0.25, 24, 48);
    case 'Plane': { const g = new THREE.PlaneGeometry(p.x ?? 10, p.z ?? 10); g.rotateX(-Math.PI / 2); return g; }
    case 'Pyramid': return pyramidGeometry(p.base_x ?? 1, p.base_z ?? 1, p.height ?? 1);
    case 'Tetrahedron': return new THREE.TetrahedronGeometry(p.radius ?? 1);
    case 'Icosahedron': return new THREE.IcosahedronGeometry(p.radius ?? 1, 0);
    case 'Wedge': return wedgeGeometry(p.x ?? 1, p.y ?? 1, p.z ?? 1);
    default: return new THREE.BoxGeometry(1, 1, 1);
  }
}

// ---------------------------------------------------------------------------
// Materials and lights
// ---------------------------------------------------------------------------

// Texture maps: paths relative to the world's `assets/` folder, glTF
// conventions (colour maps sRGB, data maps linear; roughness in green and
// metallic in blue of one metallic-roughness image), multiplied by the scalar
// factors as in the Bevy mapping (an emissive map shows only where `emissive`
// is non-zero). Without an `assetBase` they are skipped.
/** @type {[string, string[], boolean][]} */
const TEXTURE_SLOTS = [
  ['base_color_texture', ['map'], true],
  ['metallic_roughness_texture', ['roughnessMap', 'metalnessMap'], false],
  ['normal_map_texture', ['normalMap'], false],
  ['emissive_texture', ['emissiveMap'], true],
];
/** @type {THREE.TextureLoader|null} */
let textureLoader = null;

/** @param {THREE.Material} material @param {any} mat @param {string|null} assetBase */
function applyTextures(material, mat, assetBase) {
  if (!assetBase) return;
  for (const [key, props, srgb] of TEXTURE_SLOTS) {
    const path = mat[key];
    if (!path) continue;
    if (props.some((p) => !(p in material))) continue; // e.g. unlit has no normalMap
    textureLoader ||= new THREE.TextureLoader();
    const texture = textureLoader.load(assetBase + path, () => { material.needsUpdate = true; });
    texture.colorSpace = srgb ? THREE.SRGBColorSpace : THREE.NoColorSpace;
    texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
    for (const p of props) (/** @type {any} */ (material))[p] = texture;
  }
}

/** @param {any} def @param {string|null} [assetBase] @returns {THREE.MeshBasicMaterial|THREE.MeshStandardMaterial} */
export function createMaterial(def, assetBase = null) {
  const mat = { ...DEFAULT_MATERIAL, ...(def || {}) };
  const color = srgbColor(mat.color);
  const opacity = mat.color?.[3] ?? 1;
  const [alphaKind, alphaArg] = variant(mat.alpha_mode);
  const transparent = opacity < 1 || alphaKind === 'blend' || alphaKind === 'add' || alphaKind === 'multiply' || alphaKind === 'mask';
  let material;
  if (mat.unlit) {
    material = new THREE.MeshBasicMaterial({ color, opacity, transparent });
  } else {
    const e = mat.emissive || [0, 0, 0, 0];
    const hasEmissive = e[0] > 0 || e[1] > 0 || e[2] > 0;
    material = new THREE.MeshStandardMaterial({
      color,
      metalness: mat.metallic ?? 0,
      roughness: mat.roughness ?? 0.5,
      emissive: linearColor(e),
      // The emissive alpha doubles as an intensity multiplier (>= 1), as in the Bevy mapping.
      emissiveIntensity: hasEmissive ? Math.max(e[3] ?? 1, 1) : 0,
      opacity,
      transparent,
      side: mat.double_sided ? THREE.DoubleSide : THREE.FrontSide,
    });
  }
  if (alphaKind === 'add') material.blending = THREE.AdditiveBlending;
  else if (alphaKind === 'multiply') material.blending = THREE.MultiplyBlending;
  else if (alphaKind === 'mask') material.alphaTest = typeof alphaArg === 'number' ? alphaArg : 0.5;
  // `reflectance` has no MeshStandardMaterial equivalent; Bevy F0 = 0.16 * r².
  applyTextures(material, mat, assetBase);
  return material;
}

/** @param {any} def @param {Vec3} position
 *  @returns {THREE.DirectionalLight|THREE.PointLight|THREE.SpotLight} */
function createLight(def, position) {
  const color = srgbColor(def.color);
  const type = def.light_type || 'directional';
  const range = def.range ?? 50;
  let light;
  let direction = null;
  if (type === 'directional') {
    light = new THREE.DirectionalLight(color, Math.max((def.intensity ?? 10000) / DIRECTIONAL_LUX, 0.01));
    if (def.shadows !== false) {
      light.castShadow = true;
      light.shadow.mapSize.set(2048, 2048);
      light.shadow.camera.near = 0.5;
      light.shadow.camera.far = 100;
      light.shadow.camera.left = -20; light.shadow.camera.right = 20;
      light.shadow.camera.top = 20; light.shadow.camera.bottom = -20;
    }
    direction = def.direction || [0, -1, -0.5];
  } else if (type === 'point') {
    light = new THREE.PointLight(color, ((def.intensity ?? 800) / (4 * Math.PI)) * LIGHT_EXPOSURE, range, 2);
    light.castShadow = def.shadows !== false;
  } else {
    const outer = def.outer_angle ?? 0.5;
    const inner = def.inner_angle ?? 0;
    const penumbra = outer > 0 ? Math.max(0, 1 - inner / outer) : 0;
    // Lumens → candela through the cone's solid angle, not the sphere's
    // 4π (spec/world.md "Conventions": renderers SHOULD convert by the
    // cone angle): Ω = 2π·(1 − cos(θ/2)) for the outer angle θ handed to
    // SpotLight below. A degenerate cone clamps to a small positive
    // solid angle rather than dividing by zero.
    const solidAngle = Math.max(2 * Math.PI * (1 - Math.cos(outer / 2)), 1e-6);
    light = new THREE.SpotLight(color, ((def.intensity ?? 800) / solidAngle) * LIGHT_EXPOSURE, range, outer, penumbra, 2);
    light.castShadow = def.shadows !== false;
    direction = def.direction || [0, -1, 0];
  }
  if (direction) {
    // Bevy aims the light along `direction`; three aims it at `target`.
    (/** @type {any} */ (light)).target.position.set(position[0] + direction[0], position[1] + direction[1], position[2] + direction[2]);
  }
  return light;
}

// ---------------------------------------------------------------------------
// Behaviors (all seven, evaluated from the entity's authored transform)
// ---------------------------------------------------------------------------

/**
 * @param {any} def
 * @param {EntityRecord} rec
 * @param {Map<string, EntityRecord>} byName
 * @param {Map<string, EntityRecord>} byId
 * @returns {((dt: number, t: number) => void)|null}
 */
function makeBehavior(def, rec, byName, byId) {
  const [kind, p] = variant(def);
  const obj = rec.object;
  const base = rec.base;
  switch (kind) {
    case 'Spin': {
      const axis = new THREE.Vector3(...(p.axis || [0, 1, 0])).normalize();
      const speed = THREE.MathUtils.degToRad(p.speed ?? 90);
      return (dt) => { obj.rotateOnAxis(axis, speed * dt); };
    }
    case 'Bob': {
      const axis = p.axis || [0, 1, 0];
      const amp = p.amplitude ?? 0.5, freq = p.frequency ?? 0.5, phase = THREE.MathUtils.degToRad(p.phase ?? 0);
      rec.resetPosition = true;
      return (dt, t) => {
        const off = Math.sin(t * freq * TAU + phase) * amp;
        obj.position.set(base.position.x + axis[0] * off, base.position.y + axis[1] * off, base.position.z + axis[2] * off);
      };
    }
    case 'Orbit': {
      // Numeric refs resolve by id, name refs by name (spec/world.md
      // "Identity"): saved worlds hold ids — the fold resolves names at
      // ingestion — while an unfolded manifest may still name its center.
      const centerRec = p.center != null
        ? (typeof p.center === 'string' ? byName.get(p.center) : byId.get(String(p.center)))
        : null;
      const cp = centerRec ? centerRec.base.position : new THREE.Vector3(...(p.center_point || [0, 0, 0]));
      const speed = THREE.MathUtils.degToRad(p.speed ?? 36);
      const phase = THREE.MathUtils.degToRad(p.phase ?? 0);
      const tilt = THREE.MathUtils.degToRad(p.tilt ?? 0);
      const r = p.radius ?? 5;
      const ax = new THREE.Vector3(...(p.axis || [0, 1, 0])).normalize();
      const up = Math.abs(ax.y) < 0.99 ? new THREE.Vector3(0, 1, 0) : new THREE.Vector3(1, 0, 0);
      const right = new THREE.Vector3().crossVectors(up, ax).normalize();
      const fwd = new THREE.Vector3().crossVectors(ax, right).normalize();
      rec.resetPosition = true;
      return (dt, t) => {
        const angle = t * speed + phase;
        const cosA = Math.cos(angle), sinA = Math.sin(angle);
        const dx = right.x * cosA + fwd.x * sinA;
        const dy = right.y * cosA + fwd.y * sinA + Math.sin(tilt) * cosA;
        const dz = right.z * cosA + fwd.z * sinA;
        obj.position.set(cp.x + dx * r, cp.y + dy * r, cp.z + dz * r);
      };
    }
    case 'Pulse': {
      const mn = p.min_scale ?? 0.8, mx = p.max_scale ?? 1.2, freq = p.frequency ?? 0.5;
      rec.resetScale = true;
      return (dt, t) => {
        const s = mn + (mx - mn) * (Math.sin(t * freq * TAU) * 0.5 + 0.5);
        obj.scale.set(base.scale.x * s, base.scale.y * s, base.scale.z * s);
      };
    }
    case 'Bounce': {
      const h = p.height ?? 2, g = p.gravity ?? 9.8, d = p.damping ?? 0.8, sy = p.surface_y ?? 0;
      let vel = 0, posY = sy + h;
      rec.resetPosition = true;
      return (dt) => {
        vel -= g * dt; posY += vel * dt;
        if (posY <= sy) { posY = sy; vel = Math.abs(vel) * d; if (vel < 0.1) vel = Math.sqrt(2 * g * h) * d; }
        obj.position.y = posY;
      };
    }
    case 'PathFollow': {
      const wp = p.waypoints || [];
      if (wp.length < 2) return null;
      const spd = p.speed ?? 2, mode = p.mode || 'loop', orient = !!p.orient_to_path;
      let seg = 0, frac = 0, dir = 1;
      rec.resetPosition = true;
      return (dt) => {
        const a = wp[seg], b = wp[dir < 0 ? seg - 1 : (seg + 1) % wp.length];
        if (!b) return;
        const dx = b[0] - a[0], dy = b[1] - a[1], dz = b[2] - a[2];
        const len = Math.sqrt(dx * dx + dy * dy + dz * dz) || 1;
        frac += (spd * dt) / len;
        if (frac >= 1) {
          frac = 0;
          if (mode === 'ping_pong') {
            dir *= -1; seg += dir;
            if (seg < 0) { seg = 0; dir = 1; }
            if (seg >= wp.length - 1) { seg = wp.length - 1; dir = -1; }
          } else if (mode === 'loop') {
            seg = (seg + 1) % wp.length;
          } else {
            seg = Math.min(seg + 1, wp.length - 2);
          }
        }
        const nx = wp[seg], nb = wp[(seg + 1) % wp.length];
        obj.position.set(nx[0] + (nb[0] - nx[0]) * frac, nx[1] + (nb[1] - nx[1]) * frac, nx[2] + (nb[2] - nx[2]) * frac);
        if (orient) {
          const look = new THREE.Vector3(nb[0] - nx[0], nb[1] - nx[1], nb[2] - nx[2]);
          if (look.length() > 0) obj.lookAt(obj.position.clone().add(look));
        }
      };
    }
    case 'LookAt': {
      // Same rule as Orbit's center: ids by id, names by name, resolved
      // at tick time so entities spawned later are seen either way.
      const target = p.target;
      return () => {
        const tgt = typeof target === 'string' ? byName.get(target) : byId.get(String(target));
        if (tgt) obj.lookAt(tgt.object.getWorldPosition(new THREE.Vector3()));
      };
    }
    default: return null;
  }
}

// ---------------------------------------------------------------------------
// Procedural audio (Web Audio), one graph per AudioDef
// ---------------------------------------------------------------------------

/** @param {AudioContext} ctx @param {number} seconds
 *  @param {(d: Float32Array, sampleRate: number) => void} fill
 *  @returns {AudioBuffer} */
function noiseBuffer(ctx, seconds, fill) {
  const buf = ctx.createBuffer(1, Math.floor(ctx.sampleRate * seconds), ctx.sampleRate);
  const d = buf.getChannelData(0);
  fill(d, ctx.sampleRate);
  return buf;
}

/** @param {AudioContext} ctx @param {number} [seconds] @param {number} [gain] @returns {AudioBuffer} */
function whiteNoise(ctx, seconds = 2, gain = 1) {
  return noiseBuffer(ctx, seconds, (d) => { for (let i = 0; i < d.length; i++) d[i] = (Math.random() * 2 - 1) * gain; });
}

/** @param {AudioContext} ctx @param {AudioBuffer} buffer @returns {AudioBufferSourceNode} */
function loopSource(ctx, buffer) {
  const src = ctx.createBufferSource(); src.buffer = buffer; src.loop = true; return src;
}

/** @param {AudioContext} ctx @param {BiquadFilterType} type @param {number} frequency
 *  @param {number} [q] @returns {BiquadFilterNode} */
function filterNode(ctx, type, frequency, q) {
  const f = ctx.createBiquadFilter(); f.type = type; f.frequency.value = frequency; if (q != null) f.Q.value = q; return f;
}

/** Builds the graph for one audio source into `out`; returns a stop function.
 * @param {AudioContext} ctx @param {any} audio @param {AudioNode} out
 * @returns {() => void} */
function buildAudioSource(ctx, audio, out) {
  const [kind, p] = variant(audio.source);
  /** @type {(() => void)[]} */ const stops = [];
  /** @param {any} src @param {...any} nodes */
  const chain = (src, ...nodes) => {
    let prev = src; for (const n of nodes) { prev.connect(n); prev = n; } prev.connect(out);
    if (src.start) { src.start(); stops.push(() => { try { src.stop(); } catch (_) { /* already stopped */ } }); }
  };
  /** @type {number[]} */ const timers = [];
  /** @param {() => void} fn @param {number} ms @returns {number} */
  const schedule = (fn, ms) => { const id = setTimeout(fn, ms); timers.push(id); return id; };
  stops.push(() => timers.forEach(clearTimeout));
  switch (kind) {
    case 'Wind': {
      const filt = filterNode(ctx, 'lowpass', 200 + (p.speed ?? 0.5) * 600);
      const lfo = ctx.createOscillator(); lfo.frequency.value = (p.gustiness ?? 0.5) * 2;
      const lfoG = ctx.createGain(); lfoG.gain.value = (p.gustiness ?? 0.5) * 200;
      lfo.connect(lfoG); lfoG.connect(filt.frequency); lfo.start(); stops.push(() => lfo.stop());
      chain(loopSource(ctx, whiteNoise(ctx)), filt);
      break;
    }
    case 'Rain': chain(loopSource(ctx, whiteNoise(ctx, 2, 0.5)), filterNode(ctx, 'bandpass', 2000 + (p.intensity ?? 0.5) * 3000, 0.5)); break;
    case 'Ocean': {
      const ws = p.wave_size ?? 0.5;
      const buf = noiseBuffer(ctx, 4, (d, sr) => { for (let i = 0; i < d.length; i++) { const t = i / sr; d[i] = (Math.random() * 2 - 1) * Math.sin(t * 0.3 * Math.PI) * ws; } });
      chain(loopSource(ctx, buf), filterNode(ctx, 'lowpass', 400));
      break;
    }
    case 'Fire': {
      const intensity = p.intensity ?? 0.5, crackle = p.crackle ?? 0.5;
      const buf = noiseBuffer(ctx, 2, (d) => { let last = 0; for (let i = 0; i < d.length; i++) { last = (last + Math.random() * 2 - 1) * 0.5; d[i] = last * intensity; if (Math.random() < crackle * 0.001) d[i] += Math.random() * crackle; } });
      chain(loopSource(ctx, buf), filterNode(ctx, 'lowpass', 800));
      break;
    }
    case 'Water': chain(loopSource(ctx, whiteNoise(ctx)), filterNode(ctx, 'bandpass', 300 + (p.turbulence ?? 0.5) * 500, 1.5)); break;
    case 'Hum': {
      const osc = ctx.createOscillator(); osc.type = 'sine'; osc.frequency.value = p.frequency ?? 60;
      const warmth = p.warmth ?? 0.5;
      if (warmth > 0) chain(osc, filterNode(ctx, 'lowpass', (p.frequency ?? 60) * (1 + warmth * 4))); else chain(osc);
      break;
    }
    case 'Stream': chain(loopSource(ctx, whiteNoise(ctx)), filterNode(ctx, 'bandpass', 500 + (p.flow_rate ?? 0.5) * 1000, 2)); break;
    case 'Forest': {
      chain(loopSource(ctx, whiteNoise(ctx, 2, p.wind ?? 0.5)), filterNode(ctx, 'lowpass', 300));
      const density = p.bird_density ?? 0.5;
      if (density > 0) {
        const chirp = () => {
          const o = ctx.createOscillator(); o.frequency.value = 2000 + Math.random() * 3000;
          const cg = ctx.createGain(); cg.gain.value = density * 0.15;
          o.connect(cg); cg.connect(out); o.start();
          cg.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.15); o.stop(ctx.currentTime + 0.2);
          schedule(chirp, 500 + Math.random() * (3000 / Math.max(density, 0.1)));
        };
        schedule(chirp, Math.random() * 2000);
      }
      break;
    }
    case 'Cave': {
      const rate = Math.max(p.drip_rate ?? 0.5, 0.1), res = p.resonance ?? 0.5;
      const drip = () => {
        const o = ctx.createOscillator(); o.frequency.value = 800 + Math.random() * 2000;
        const cg = ctx.createGain(); cg.gain.value = res * 0.3;
        o.connect(cg); cg.connect(out); o.start();
        cg.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.3 * res + 0.01); o.stop(ctx.currentTime + 0.4);
        schedule(drip, 500 / rate + Math.random() * (2000 / rate));
      };
      schedule(drip, Math.random() * 1000);
      break;
    }
    case 'WindEmitter': chain(loopSource(ctx, whiteNoise(ctx)), filterNode(ctx, 'bandpass', (p.pitch ?? 1) * 500, 3)); break;
    case 'Custom': {
      const wave = p.waveform || 'Sine';
      const ftype = /** @type {BiquadFilterType} */ (String(p.filter_type || 'lowpass').toLowerCase());
      const cutoff = p.filter_cutoff ?? 1000;
      if (/noise/i.test(wave)) chain(loopSource(ctx, whiteNoise(ctx)), filterNode(ctx, ftype, cutoff));
      else {
        const osc = ctx.createOscillator();
        osc.type = wave === 'Saw' ? 'sawtooth' : wave === 'Square' ? 'square' : 'sine';
        osc.frequency.value = 220;
        chain(osc, filterNode(ctx, ftype, cutoff));
      }
      break;
    }
    default: break; // Silence, Abc, File: not synthesized in the browser.
  }
  return () => stops.forEach((s) => s());
}

// ---------------------------------------------------------------------------
// Instances (world-types `instance`): one creation, many placements
// ---------------------------------------------------------------------------

/** Apply an `EntityPatch` (JSON) to a plain entity: a present key replaces the field, `null` clears it.
 * @param {any} def @param {any} patch */
export function applyPatchToDef(def, patch) {
  for (const [key, value] of Object.entries(patch || {})) def[key] = value;
  return def;
}

/** The first id free for expanded parts (`WorldManifest::first_expansion_id`).
 * @param {WorldManifest} manifest @returns {number} */
export function firstExpansionId(manifest) {
  let max = 0;
  for (const e of manifest.entities || []) max = Math.max(max, Number(e.id) || 0);
  return Math.max(Number(manifest.next_entity_id ?? 1), max + 1);
}

/**
 * `instance::expand_instances`: each instance followed by copies of its
 * creation's parts, with fresh ids from `firstId`, names `instance/part`,
 * the instance (or the parent part's copy) as parent, and the per-instance
 * overrides applied. Same ids and order as the Rust function, so both
 * renderers draw the same entities.
 * @param {WorldEntity[]} entities
 * @param {any[]|null|undefined} creations
 * @param {number} firstId
 * @returns {WorldEntity[]}
 */
export function expandInstances(entities, creations, firstId) {
  const defs = new Map((creations || []).map((c) => [String(c.id), c]));
  const done = new Set();
  for (const e of entities || []) if (e.parent != null && e.creation_id != null) done.add(`${e.parent}:${e.creation_id}`);
  let next = firstId;
  const out = [];
  for (const e of entities || []) {
    out.push(e);
    const inst = e.instance_of;
    if (!inst) continue;
    const def = defs.get(String(inst.creation));
    if (!def || done.has(`${e.id}:${def.id}`)) continue;
    // Parts an override removes drop out, with everything under them.
    const removed = new Set((def.parts || []).filter((/** @type {any} */ p) => (inst.overrides || []).some((/** @type {any} */ o) => o.removed && o.part === p.name)).map((/** @type {any} */ p) => String(p.id)));
    for (let grew = true; grew;) {
      grew = false;
      for (const p of def.parts || []) {
        if (p.parent != null && removed.has(String(p.parent)) && !removed.has(String(p.id))) { removed.add(String(p.id)); grew = true; }
      }
    }
    const parts = (def.parts || []).filter((/** @type {any} */ p) => !removed.has(String(p.id)));
    /** @type {Map<string, number>} */ const ids = new Map();
    for (const p of parts) ids.set(String(p.id), next++);
    for (const p of parts) {
      const x = JSON.parse(JSON.stringify(p));
      for (const o of inst.overrides || []) {
        if (o.part !== p.name) continue;
        if (o.removed) continue;
        const { name, parent, instance_of, ...rest } = o.patch || {};
        applyPatchToDef(x, rest);
      }
      x.id = ids.get(String(p.id));
      x.name = `${e.name}/${p.name}`;
      x.parent = p.parent != null && ids.has(String(p.parent)) ? ids.get(String(p.parent)) : e.id;
      if (e.chunk != null) x.chunk = e.chunk; else delete x.chunk;
      x.creation_id = def.id;
      delete x.instance_of;
      out.push(x);
    }
  }
  return out;
}

/** `Shape::local_aabb_half`: a shape's box half-extents in its own frame.
 * @param {Shape} shape @returns {Vec3|null} */
export function shapeHalfExtents(shape) {
  const [kind, p] = variant(shape);
  const a = p || {};
  switch (kind) {
    case 'Cuboid': case 'Wedge': return [a.x / 2, a.y / 2, a.z / 2];
    case 'Sphere': case 'Tetrahedron': case 'Icosahedron': return [a.radius, a.radius, a.radius];
    case 'Cylinder': case 'Cone': return [a.radius, a.height / 2, a.radius];
    case 'Capsule': return [a.radius, a.half_length + a.radius, a.radius];
    case 'Torus': return [a.major_radius + a.minor_radius, a.minor_radius, a.major_radius + a.minor_radius];
    case 'Plane': return [a.x / 2, 0, a.z / 2];
    case 'Pyramid': return [a.base_x / 2, a.height / 2, a.base_z / 2];
    default: return null;
  }
}

/** `TriggerDef::area`: the event's volume, else the shape's box, else a sphere of radius 3.
 * @param {any} trigger @param {WorldEntity} entity */
export function triggerArea(trigger, entity) {
  const ev = trigger.on || {};
  if (ev.event !== 'area_enter' && ev.event !== 'area_exit') return null;
  if (ev.volume) return ev.volume;
  const half = entity.shape ? shapeHalfExtents(entity.shape) : null;
  return half ? { shape: 'box', half_extents: half } : { shape: 'sphere', radius: 3 };
}

/** `TriggerVolume::contains_local`.
 * @param {any} volume @param {THREE.Vector3} p @returns {boolean} */
export function volumeContains(volume, p) {
  if (!volume) return false;
  if (volume.shape === 'box') {
    const h = volume.half_extents || [0, 0, 0];
    return Math.abs(p.x) <= h[0] && Math.abs(p.y) <= h[1] && Math.abs(p.z) <= h[2];
  }
  const r = volume.radius ?? 0;
  return p.x * p.x + p.y * p.y + p.z * p.z <= r * r;
}

/** An `animate` action: move a transform property linearly to `to`.
 * @param {THREE.Object3D} object @param {any} action
 * @returns {ViewerAnimation|null} */
function startAnimation(object, action) {
  const to = action.to || [];
  const vec = () => (to.length >= 3 ? new THREE.Vector3(to[0], to[1], to[2]) : null);
  const duration = Math.max(action.duration ?? 1, 0);
  switch (action.property || 'position') {
    case 'position': case 'translation': {
      const b = vec(); return b && { kind: 'position', a: object.position.clone(), b, duration, t: 0 };
    }
    case 'rotation': {
      const r = vec(); if (!r) return null;
      const b = new THREE.Quaternion().setFromEuler(new THREE.Euler(THREE.MathUtils.degToRad(r.x), THREE.MathUtils.degToRad(r.y), THREE.MathUtils.degToRad(r.z), 'XYZ'));
      return { kind: 'rotation', a: object.quaternion.clone(), b, duration, t: 0 };
    }
    case 'scale': {
      const b = to.length === 1 ? new THREE.Vector3(to[0], to[0], to[0]) : vec();
      return b && { kind: 'scale', a: object.scale.clone(), b, duration, t: 0 };
    }
    default: return null;
  }
}

/** Advance an animation; true when it has arrived.
 * @param {THREE.Object3D} object @param {ViewerAnimation} anim @param {number} dt
 * @returns {boolean} */
function stepAnimation(object, anim, dt) {
  anim.t += dt;
  const f = anim.duration > 0 ? Math.min(anim.t / anim.duration, 1) : 1;
  if (anim.kind === 'position') object.position.lerpVectors(anim.a, anim.b, f);
  else if (anim.kind === 'scale') object.scale.lerpVectors(anim.a, anim.b, f);
  else object.quaternion.slerpQuaternions(/** @type {THREE.Quaternion} */ (anim.a), /** @type {THREE.Quaternion} */ (anim.b), f);
  return f >= 1;
}

/** The manifest's entities with every instance expanded.
 * @param {WorldManifest} manifest @returns {WorldEntity[]} */
export function expandedEntities(manifest) {
  const entities = manifest.entities || [];
  if (!entities.some((e) => e.instance_of)) return entities;
  return expandInstances(entities, manifest.creations, firstExpansionId(manifest));
}

/**
 * `MeshAssetRef::node_overrides`: hide or recolour named nodes inside a
 * loaded glTF scene (the node and everything under it). Materials are
 * cloned so other placements of the same file keep theirs.
 * @param {THREE.Object3D} root @param {any[]|null} [overrides]
 */
export function applyNodeOverrides(root, overrides) {
  for (const o of overrides || []) {
    const node = root.getObjectByName(o.node);
    if (!node) continue;
    if (o.visible != null) node.visible = !!o.visible;
    if (o.color) {
      const color = srgbColor(o.color);
      const recolor = (/** @type {any} */ m) => { const c = m.clone(); c.color?.copy(color); return c; };
      node.traverse((/** @type {any} */ m) => {
        if (!m.isMesh || !m.material) return;
        m.material = Array.isArray(m.material) ? m.material.map(recolor) : recolor(m.material);
      });
    }
  }
}

// ---------------------------------------------------------------------------
// The viewer
// ---------------------------------------------------------------------------

/**
 * Render `manifest` into `container`.
 *
 * options (see ViewerOptions):
 *   assetBase    URL prefix for mesh assets and the soundtrack file ('' = none: placeholders, silent).
 *   audioButton  element whose click toggles audio (shown when the world has sound).
 *   tourButton   element whose click starts/stops the first tour (shown when tours exist).
 *   tourCaption  element that shows waypoint descriptions.
 *   triggerCaption element for `show_text` trigger actions (default: tourCaption).
 *   keyboard     WASD/Space/Shift navigation (default true).
 *   embedApi     postMessage API for a parent frame (default: when framed).
 *   ambientScale override for AMBIENT_SCALE.
 *
 * @param {HTMLElement} container
 * @param {WorldManifest} manifest
 * @param {ViewerOptions} [options]
 */
export function createWorldViewer(container, manifest, options = {}) {
  const opts = { keyboard: true, embedApi: typeof window !== 'undefined' && window.parent !== window, ...options };
  const assetBase = opts.assetBase ? String(opts.assetBase).replace(/\/?$/, '/') : '';
  const env = /** @type {EnvironmentDef} */ (manifest.environment || {});

  // ---- Scene, camera, renderer ----
  const scene = new THREE.Scene();
  if (env.background_color) scene.background = srgbColor(env.background_color);
  if ((env.fog_density ?? 0) > 0) {
    // Bevy: exponential fog in `fog_density`, coloured by the fog colour, else the background.
    const fogColor = env.fog_color || env.background_color;
    scene.fog = new THREE.Fog(fogColor ? srgbColor(fogColor) : new THREE.Color(1, 1, 1), 1, 100 / Math.max(env.fog_density ?? 0.01, 0.01));
  }
  const ambient = new THREE.AmbientLight(env.ambient_color ? srgbColor(env.ambient_color) : new THREE.Color(1, 1, 1),
    (env.ambient_intensity ?? 80) * (opts.ambientScale ?? AMBIENT_SCALE));
  scene.add(ambient);

  const cam = manifest.camera || (manifest.avatar
    ? { position: manifest.avatar.spawn_position || DEFAULT_CAMERA.position, look_at: manifest.avatar.spawn_look_at || DEFAULT_CAMERA.look_at, fov_degrees: DEFAULT_CAMERA.fov_degrees }
    : DEFAULT_CAMERA);
  const width = container.clientWidth || (typeof window !== 'undefined' ? window.innerWidth : 800);
  const height = container.clientHeight || (typeof window !== 'undefined' ? window.innerHeight : 600);
  const camera = new THREE.PerspectiveCamera(cam.fov_degrees ?? DEFAULT_CAMERA.fov_degrees, width / height, 0.1, 1000);
  camera.position.set(...(/** @type {Vec3} */ (cam.position || DEFAULT_CAMERA.position)));

  const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: !!opts.preserveDrawingBuffer });
  renderer.setPixelRatio(Math.min(typeof window !== 'undefined' ? window.devicePixelRatio : 1, 2));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.0;
  container.appendChild(renderer.domElement);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.target.set(...(/** @type {Vec3} */ (cam.look_at || DEFAULT_CAMERA.look_at)));
  controls.enableDamping = true;
  controls.dampingFactor = 0.05;
  controls.update();

  // ---- Shot view (ext-cinematography): opts.shot looks through that
  // shot's camera — vertical FOV and frame aspect from the normative
  // crop math, the canvas letterboxed to it. A shot is a fixed setup:
  // no orbiting, no walking.
  const shotEntity = opts.shot ? (manifest.entities ?? []).find((e) => e.name === opts.shot) : null;
  const shotCamera = shotEntity ? cameraOf(shotEntity) : null;
  const shotFrame = shotCamera ? frameOf(shotCamera) : null;
  const shotView = shotEntity && shotCamera ? viewOf(shotEntity, shotCamera) : null;
  if (opts.shot && !shotView) {
    console.warn(`no ext-cinematography camera named '${opts.shot}' — the default view stands`);
  }
  if (shotView && shotFrame && shotCamera) {
    camera.fov = shotFrame.vfov_degrees;
    camera.aspect = shotFrame.aspect;
    camera.position.set(...(/** @type {Vec3} */ (shotView.position)));
    controls.target.set(...(shotCamera.aim
      ? /** @type {Vec3} */ (shotCamera.aim)
      : /** @type {Vec3} */ ([
        shotView.position[0] + shotView.forward[0],
        shotView.position[1] + shotView.forward[1],
        shotView.position[2] + shotView.forward[2],
      ])));
    controls.enabled = false;
    opts.keyboard = false;
    container.style.background = '#000';
    camera.updateProjectionMatrix();
    controls.update();
  }

  function resize() {
    const w = container.clientWidth || width, h = container.clientHeight || height;
    if (shotFrame) {
      // Letterbox to the frame's aspect: the largest centered rect of it.
      let vw = w, vh = w / shotFrame.aspect;
      if (vh > h) { vh = h; vw = h * shotFrame.aspect; }
      renderer.setSize(vw, vh, false);
      renderer.domElement.style.width = `${vw}px`;
      renderer.domElement.style.height = `${vh}px`;
      renderer.domElement.style.margin = `${(h - vh) / 2}px ${(w - vw) / 2}px`;
      camera.aspect = shotFrame.aspect;
      camera.updateProjectionMatrix();
      return;
    }
    renderer.setSize(w, h, false);
    renderer.domElement.style.width = '100%';
    renderer.domElement.style.height = '100%';
    camera.aspect = w / h;
    camera.updateProjectionMatrix();
  }
  resize();
  const resizeObserver = typeof ResizeObserver !== 'undefined' ? new ResizeObserver(resize) : null;
  resizeObserver?.observe(container);

  // ---- Entities ----
  /** @type {EntityRecord[]} */ const records = [];
  /** @type {Map<string, EntityRecord>} */ const byName = new Map();
  /** @type {Map<string, EntityRecord>} */ const byId = new Map();
  const gltfLoader = assetBase ? new GLTFLoader() : null;

  // Build one entity's scene object and record (no parent attach, no
  // dynamics) — shared by initial load and live ops.
  /** @param {WorldEntity} def @returns {EntityRecord} */
  function buildRecord(def) {
    const t = /** @type {WorldTransform} */ (def.transform || {});
    const position = /** @type {Vec3} */ (t.position || [0, 0, 0]);
    const hasShape = !!def.shape, hasLight = !!def.light;
    let object = /** @type {THREE.Object3D|null} */ (null);
    let material = /** @type {THREE.Material|null} */ (null);
    let light = /** @type {THREE.Light|null} */ (null);
    if (hasShape) {
      material = createMaterial(def.material, assetBase);
      object = new THREE.Mesh(createGeometry(def.shape), material);
      object.castShadow = true;
      object.receiveShadow = true;
    } else if (def.mesh_asset) {
      object = new THREE.Group();
      // Capability tiers (rfcs/capability-tiers.md): a mesh may name its
      // cheaper sibling — a parametric `fallback` shape. It draws while
      // the mesh loads, and stays when the mesh can't (or the renderer
      // won't): the declared silhouette instead of a wireframe guess.
      const fallbackShape = def.mesh_asset.fallback;
      const placeholder = fallbackShape
        ? new THREE.Mesh(createGeometry(fallbackShape), createMaterial(def.material, assetBase))
        : new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial({ color: 0x888888, wireframe: true }));
      placeholder.castShadow = Boolean(fallbackShape);
      placeholder.receiveShadow = Boolean(fallbackShape);
      object.add(placeholder);
      if (gltfLoader) {
        gltfLoader.load(assetBase + def.mesh_asset.path, (gltf) => {
          const obj = /** @type {THREE.Object3D} */ (object);
          obj.remove(placeholder);
          const node = def.mesh_asset.node ? gltf.scene.getObjectByName(def.mesh_asset.node) || gltf.scene : gltf.scene;
          node.traverse((/** @type {any} */ o) => { if (o.isMesh) { o.castShadow = true; o.receiveShadow = true; } });
          applyNodeOverrides(node, def.mesh_asset.node_overrides);
          obj.add(node);
        }, undefined, () => { /* keep the fallback (or the wireframe) */ });
      }
    } else if (!hasLight) {
      object = new THREE.Group();
    }
    if (hasLight) {
      light = createLight(def.light, position);
      if (object) object.add(light); else object = light;
      const target = /** @type {any} */ (light).target;
      if (target) scene.add(target);
    }
    // Every path above assigns `object` (shape, mesh, group, or light); the
    // alias just lets the type say so.
    const obj = /** @type {THREE.Object3D} */ (object);
    obj.position.set(...position);
    const rot = /** @type {Vec3} */ (t.rotation_degrees || [0, 0, 0]);
    // Rotations are intrinsic XYZ Euler degrees (spec/world.md
    // "Conventions"); three's 'XYZ' order composes R = Rx·Ry·Rz, which
    // is exactly intrinsic XYZ — stated so nobody "fixes" it to 'ZYX'.
    obj.rotation.set(THREE.MathUtils.degToRad(rot[0]), THREE.MathUtils.degToRad(rot[1]), THREE.MathUtils.degToRad(rot[2]), 'XYZ');
    obj.scale.set(...(/** @type {Vec3} */ (t.scale || [1, 1, 1])));
    if (t.visible === false) obj.visible = false;
    obj.name = def.name;
    const std = /** @type {any} */ (material);
    return {
      def, object: obj, material, light,
      base: {
        position: obj.position.clone(),
        scale: obj.scale.clone(),
        emissive: std?.emissive ? std.emissive.clone() : null,
        emissiveIntensity: std?.emissiveIntensity ?? 0,
        lightIntensity: light?.intensity ?? 0,
        opacity: std?.opacity ?? 1,
      },
      behaviors: [], mods: [], resetPosition: false, resetScale: false,
      triggers: [], anim: null,
    };
  }

  /** @param {EntityRecord} rec */
  function attachRecord(rec) {
    const parent = rec.def.parent != null ? byId.get(String(rec.def.parent)) : null;
    (parent ? parent.object : scene).add(rec.object);
  }

  /** @param {EntityRecord} rec */
  function initDynamics(rec) {
    rec.behaviors = [];
    rec.mods = [];
    rec.resetPosition = false;
    rec.resetScale = false;
    for (const b of rec.def.behaviors || []) { const fn = makeBehavior(b, rec, byName, byId); if (fn) rec.behaviors.push(fn); }
    for (const m of rec.def.modulations || []) {
      const [target] = variant(m.target);
      if (target === 'offset_y') rec.resetPosition = true;
      if (target === 'scale') rec.resetScale = true;
      rec.mods.push({ def: m, target: /** @type {string} */ (target), s: 0 });
    }
    // The viewer has no inventory, so triggers that need an item never fire.
    rec.triggers = (rec.def.triggers || [])
      .filter((/** @type {any} */ t) => !t.requires_item)
      .map((/** @type {any} */ def) => ({ def, area: triggerArea(def, rec.def), done: false, inside: false, last: null, acc: 0 }));
  }

  /** @param {THREE.Object3D} object */
  function disposeObject(object) {
    object.traverse((/** @type {any} */ o) => {
      o.geometry?.dispose?.();
      if (o.material) (Array.isArray(o.material) ? o.material : [o.material]).forEach((/** @type {any} */ m) => m.dispose?.());
    });
  }

  /** @param {EntityRecord} rec */
  function removeRecord(rec) {
    rec.object.removeFromParent();
    const target = /** @type {any} */ (rec.light)?.target;
    if (target) target.removeFromParent();
    disposeObject(rec.object);
    byName.delete(rec.def.name);
    byId.delete(String(rec.def.id));
    const i = records.indexOf(rec);
    if (i >= 0) records.splice(i, 1);
  }

  // Replace an entity's object (shape/material/light/mesh changes), keeping
  // its children attached.
  /** @param {EntityRecord} rec */
  function rebuildRecord(rec) {
    const id = rec.def.id;
    const children = records.filter((r) => r.def.parent != null && String(r.def.parent) === String(id));
    removeRecord(rec);
    const fresh = buildRecord(rec.def);
    records.push(fresh);
    byName.set(fresh.def.name, fresh);
    byId.set(String(fresh.def.id), fresh);
    attachRecord(fresh);
    for (const child of children) fresh.object.add(child.object);
    initDynamics(fresh);
  }

  // Apply an EntityPatch to a live record.
  /** @param {number|string} id @param {any} patch */
  function applyEntityPatch(id, patch) {
    const rec = byId.get(String(id));
    if (!rec) return;
    const def = rec.def;
    if (patch.name != null) {
      byName.delete(def.name);
      def.name = patch.name;
      rec.object.name = patch.name;
      byName.set(def.name, rec);
    }
    if (patch.transform) {
      def.transform = patch.transform;
      const t = patch.transform;
      rec.object.position.set(...(/** @type {Vec3} */ (t.position || [0, 0, 0])));
      const rot = t.rotation_degrees || [0, 0, 0];
      // Intrinsic XYZ, as above — the live patch composes like the load.
      rec.object.rotation.set(THREE.MathUtils.degToRad(rot[0]), THREE.MathUtils.degToRad(rot[1]), THREE.MathUtils.degToRad(rot[2]), 'XYZ');
      rec.object.scale.set(...(/** @type {Vec3} */ (t.scale || [1, 1, 1])));
      rec.object.visible = t.visible !== false;
      rec.base.position.copy(rec.object.position);
      rec.base.scale.copy(rec.object.scale);
    }
    if (patch.parent !== undefined) {
      def.parent = patch.parent;
      attachRecord(rec);
    }
    let rebuild = false;
    if (patch.shape !== undefined) { def.shape = patch.shape; rebuild = true; }
    if (patch.material !== undefined) { def.material = patch.material; rebuild = true; }
    if (patch.light !== undefined) { def.light = patch.light; rebuild = true; }
    if (patch.mesh_asset !== undefined) { def.mesh_asset = patch.mesh_asset; rebuild = true; }
    if (rebuild) { rebuildRecord(rec); return; }
    if (patch.behaviors) { def.behaviors = patch.behaviors; initDynamics(rec); }
    if (patch.modulations) { def.modulations = patch.modulations; initDynamics(rec); }
    if (patch.triggers) { def.triggers = patch.triggers; initDynamics(rec); }
    if (patch.audio !== undefined) def.audio = patch.audio;
  }

  // Scene-wide environment changes (background, fog, ambient light).
  /** @param {any} envDef */
  function applyEnvironment(envDef) {
    if (!envDef) return;
    if (envDef.background_color) scene.background = srgbColor(envDef.background_color);
    if (envDef.fog_density > 0) {
      const fogColor = envDef.fog_color || envDef.background_color;
      scene.fog = new THREE.Fog(fogColor ? srgbColor(fogColor) : new THREE.Color(1, 1, 1), 1, 100 / Math.max(envDef.fog_density, 0.01));
    } else if (envDef.fog_density != null) {
      scene.fog = null;
    }
    if (envDef.ambient_color) ambient.color = srgbColor(envDef.ambient_color);
    if (envDef.ambient_intensity != null) ambient.intensity = envDef.ambient_intensity * (opts.ambientScale ?? AMBIENT_SCALE);
    Object.assign(env, envDef);
  }

  // Apply committed world ops (world-types EditOp in serde's externally
  // tagged JSON form). Used by collaborative sessions; the document on the
  // authority guarantees order and validity.
  /** @param {any[]|null} [ops] */
  function applyOps(ops) {
    for (const op of ops || []) {
      const entry = Object.entries(op)[0];
      if (!entry) continue;
      const [variant, body] = entry;
      switch (variant) {
        case 'SpawnEntity': {
          if (byId.has(String(body.entity.id))) break;
          const rec = buildRecord(body.entity);
          records.push(rec);
          byName.set(rec.def.name, rec);
          byId.set(String(rec.def.id), rec);
          attachRecord(rec);
          initDynamics(rec);
          break;
        }
        case 'DeleteEntity': {
          // Subtree deletes arrive children-first; each may already be gone.
          const rec = byId.get(String(body.id));
          if (rec) removeRecord(rec);
          break;
        }
        case 'ModifyEntity':
          applyEntityPatch(body.id, body.patch);
          break;
        case 'SetEnvironment':
          applyEnvironment(body.env);
          break;
        case 'SetCamera':
          // Guests keep their own cameras; camera ops matter for exports.
          break;
        case 'SpawnAudioEmitter': {
          const rec = byName.get(body.name);
          if (rec) rec.def.audio = body.audio;
          break;
        }
        case 'RemoveAudioEmitter': {
          const rec = byName.get(body.name);
          if (rec) rec.def.audio = null;
          break;
        }
        case 'SetAmbience':
          break;
        case 'Batch':
          applyOps(body.ops);
          break;
        default:
          break;
      }
    }
  }

  for (const def of expandedEntities(manifest)) {
    const rec = buildRecord(def);
    records.push(rec);
    byName.set(def.name, rec);
    byId.set(String(def.id), rec);
  }
  // Capability tiers (rfcs/capability-tiers.md): lights carry an optional
  // `priority`; over this renderer's punctual-light budget, the least
  // important are hidden — dropped from the tail, never rejected.
  // Document order breaks ties; a world without priorities is unchanged.
  {
    const budget = 16; // three.js shader cost grows per light
    const lights = records.filter((r) => r.light)
      .map((r) => ({ record: r, priority: r.def.light?.priority ?? 0 }));
    if (lights.length > budget) {
      lights.sort((a, b) => b.priority - a.priority); // stable: ties keep document order
      for (const { record } of lights.slice(budget)) (/** @type {THREE.Light} */ (record.light)).visible = false;
    }
  }
  for (const rec of records) attachRecord(rec);
  for (const rec of records) initDynamics(rec);

  // ---- Soundtrack and signals ----
  const soundtrack = manifest.soundtrack || null;
  /** @type {AudioState} */
  const audioState = { ctx: null, started: false, stops: [], element: null, analyser: null, bins: null, spatial: [] };
  const live = { bass: 0, highs: 0 };

  /** @param {number} elapsed @returns {number} */
  function soundtrackTime(elapsed) {
    if (audioState.element && !audioState.element.paused) return audioState.element.currentTime;
    return soundtrack && soundtrack.duration > 0 ? mod(elapsed, soundtrack.duration) : elapsed;
  }

  function updateLive() {
    const a = audioState.analyser;
    if (!a || !audioState.bins) return;
    a.getByteFrequencyData(audioState.bins);
    const bins = audioState.bins;
    let bass = 0, highs = 0, nb = 0, nh = 0;
    for (let i = 1; i <= 3; i++) { bass += bins[i]; nb++; }
    for (let i = 24; i < bins.length; i++) { highs += bins[i]; nh++; }
    live.bass = nb ? bass / nb / 255 : 0;
    live.highs = nh ? Math.min(1, (highs / nh / 255) * 2.5) : 0;
  }

  /** @param {any} sig @param {number} t @param {boolean} playing @returns {number|null} */
  function signalValue(sig, t, playing) {
    const [kind, arg] = variant(sig);
    switch (kind) {
      case 'energy': return soundtrack ? curveAt(soundtrack.energy, t) : null;
      case 'beat': return soundtrack ? beatAt(soundtrack, t) : null;
      case 'bass': return playing ? live.bass : soundtrack ? (soundtrack.stems?.bass?.length ? curveAt(soundtrack.stems.bass, t) : curveAt(soundtrack.energy, t)) : null;
      case 'highs': return playing ? live.highs : soundtrack ? (soundtrack.stems?.other?.length ? curveAt(soundtrack.stems.other, t) : curveAt(soundtrack.energy, t)) : null;
      case 'stem': { if (!soundtrack) return null; const c = soundtrack.stems?.[arg]; return c && c.length ? curveAt(c, t) : curveAt(soundtrack.energy, t); }
      case 'oscillator': return (Math.sin(t * (arg?.frequency ?? 1) * TAU) + 1) * 0.5;
      case 'constant': return clamp01(typeof arg === 'number' ? arg : 0);
      default: return null;
    }
  }

  // Several modulations on one target combine: factors multiply, offsets
  // add (the format's rule; world-bevy's `modulation` does the same).
  /** @param {EntityRecord} rec @param {number} dt @param {number} t @param {boolean} playing */
  function applyModulations(rec, dt, t, playing) {
    const mat = /** @type {any} */ (rec.material);
    /** @type {Record<string, number>} */
    const combined = { emissive: 1, scale: 1, light_intensity: 1, opacity: 1, offset_y: 0 };
    const driven = new Set();
    for (const m of rec.mods) {
      const raw = signalValue(m.def.signal, t, playing);
      let factor;
      if (raw == null) {
        factor = m.target === 'offset_y' ? 0 : 1;
      } else {
        const sm = m.def.smoothing || 0;
        m.s = sm > 0 ? m.s + (raw - m.s) * Math.min(1, dt / sm) : raw;
        factor = modulationFactor(m.def, m.s);
      }
      if (!(m.target in combined)) continue;
      driven.add(m.target);
      if (m.target === 'offset_y') combined.offset_y += factor;
      else combined[m.target] *= factor;
    }
    if (driven.has('emissive') && mat?.emissive) mat.emissiveIntensity = rec.base.emissiveIntensity * combined.emissive;
    if (driven.has('scale')) rec.object.scale.multiplyScalar(combined.scale);
    if (driven.has('light_intensity') && rec.light) rec.light.intensity = rec.base.lightIntensity * combined.light_intensity;
    if (driven.has('opacity') && mat) { mat.opacity = rec.base.opacity * combined.opacity; mat.transparent = true; }
    if (driven.has('offset_y')) rec.object.position.y += combined.offset_y;
  }

  // ---- Audio control ----
  const hasAudio = records.some((r) => r.def.audio) || !!(soundtrack && soundtrack.path);
  function startAudio() {
    const AudioCtx = typeof window !== 'undefined' ? (window.AudioContext || /** @type {any} */ (window).webkitAudioContext) : null;
    if (!AudioCtx) return;
    const ctx = new AudioCtx();
    const master = ctx.createGain(); master.gain.value = 0.5; master.connect(ctx.destination);
    audioState.ctx = ctx; audioState.stops = []; audioState.spatial = [];
    for (const rec of records) {
      if (!rec.def.audio) continue;
      const g = ctx.createGain(); g.gain.value = rec.def.audio.volume ?? 1; g.connect(master);
      audioState.stops.push(buildAudioSource(ctx, rec.def.audio, g));
      if (rec.def.audio.radius > 0) audioState.spatial.push({ rec, gain: g, volume: rec.def.audio.volume ?? 1 });
    }
    if (soundtrack && soundtrack.path && assetBase && typeof Audio !== 'undefined') {
      const el = new Audio(assetBase + soundtrack.path);
      el.crossOrigin = 'anonymous'; el.loop = true;
      const src = ctx.createMediaElementSource(el);
      const analyser = ctx.createAnalyser(); analyser.fftSize = 256;
      src.connect(analyser); analyser.connect(master);
      audioState.element = el; audioState.analyser = analyser; audioState.bins = new Uint8Array(analyser.frequencyBinCount);
      el.play().catch(() => { /* autoplay policy: the user toggles again */ });
    }
    audioState.started = true;
  }
  function stopAudio() {
    audioState.stops.forEach((s) => s());
    audioState.element?.pause();
    audioState.ctx?.close();
    Object.assign(audioState, { ctx: null, started: false, stops: [], element: null, analyser: null, bins: null, spatial: [] });
    live.bass = 0; live.highs = 0;
  }
  function toggleAudio() {
    if (audioState.started) stopAudio(); else startAudio();
    if (opts.audioButton) opts.audioButton.textContent = audioState.started ? 'Sound Off' : 'Sound On';
    return audioState.started;
  }
  if (opts.audioButton) {
    opts.audioButton.style.display = hasAudio ? '' : 'none';
    opts.audioButton.addEventListener('click', toggleAudio);
  }

  function updateSpatial() {
    if (!audioState.spatial.length) return;
    const camPos = camera.position;
    const tmp = new THREE.Vector3();
    for (const s of audioState.spatial) {
      const d = s.rec.object.getWorldPosition(tmp).distanceTo(camPos);
      const r = s.rec.def.audio.radius;
      const rolloff = s.rec.def.audio.rolloff || 'inverse_square';
      const att = rolloff === 'linear' ? clamp01(1 - d / r) : rolloff === 'exponential' ? Math.exp(-3 * d / r) : 1 / (1 + Math.pow(d / (r / 3), 2));
      s.gain.gain.value = s.volume * att;
    }
  }

  // ---- Keyboard navigation ----
  /** @type {Record<string, boolean>} */ const keys = {};
  const moveSpeed = manifest.avatar?.movement_speed ?? 5;
  /** @param {KeyboardEvent} e */
  const onKeyDown = (e) => { keys[e.code] = true; };
  /** @param {KeyboardEvent} e */
  const onKeyUp = (e) => { keys[e.code] = false; };
  if (opts.keyboard && typeof document !== 'undefined') {
    document.addEventListener('keydown', onKeyDown);
    document.addEventListener('keyup', onKeyUp);
  }
  /** @param {number} dt */
  function updateMovement(dt) {
    if (typeof document !== 'undefined' && /^(INPUT|TEXTAREA)$/.test(document.activeElement?.tagName || '')) return;
    const dir = new THREE.Vector3(); camera.getWorldDirection(dir);
    const right = new THREE.Vector3().crossVectors(dir, camera.up).normalize();
    const move = new THREE.Vector3();
    if (keys.KeyW) move.add(dir);
    if (keys.KeyS) move.sub(dir);
    if (keys.KeyA) move.sub(right);
    if (keys.KeyD) move.add(right);
    if (keys.Space) move.y += 1;
    if (keys.ShiftLeft || keys.ShiftRight) move.y -= 1;
    if (move.lengthSq() > 0) { move.normalize().multiplyScalar(moveSpeed * dt); camera.position.add(move); controls.target.add(move); }
  }

  // ---- Guided tours ----
  const tours = (manifest.tours || []).filter((t) => t.waypoints && t.waypoints.length > 0);
  /** @type {{active: any, idx: number, frac: number, paused: number}} */
  const tour = { active: null, idx: 0, frac: 0, paused: 0 };
  /** @param {string|null} text */
  function showCaption(text) {
    if (!opts.tourCaption) return;
    if (text) { opts.tourCaption.textContent = text; opts.tourCaption.style.display = ''; } else opts.tourCaption.style.display = 'none';
  }
  /** @param {number} [index] */
  function startTour(index = 0) {
    const t = tours[index]; if (!t) return;
    tour.active = t; tour.idx = 0; tour.frac = 0; tour.paused = 0;
    controls.enabled = false;
    if (opts.tourButton) opts.tourButton.textContent = 'Stop Tour';
    const wp0 = t.waypoints[0];
    if (t.mode === 'teleport') { camera.position.set(.../** @type {Vec3} */ (wp0.position)); camera.lookAt(.../** @type {Vec3} */ (wp0.look_at)); }
    showCaption(wp0.description);
  }
  function stopTour() {
    tour.active = null;
    controls.enabled = true;
    if (opts.tourButton) opts.tourButton.textContent = 'Start Tour';
    showCaption(null);
  }
  /** @param {number} dt */
  function updateTour(dt) {
    const t = tour.active; if (!t) return;
    const wp = t.waypoints;
    if (tour.paused > 0) { tour.paused -= dt; return; }
    if (t.mode === 'teleport') {
      tour.idx++;
      if (tour.idx >= wp.length - 1) { if (t.loop_tour) tour.idx = 0; else { stopTour(); return; } }
      const next = wp[tour.idx];
      camera.position.set(.../** @type {Vec3} */ (next.position)); camera.lookAt(.../** @type {Vec3} */ (next.look_at));
      tour.paused = next.pause_duration || 0; tour.frac = 0;
      showCaption(next.description);
      return;
    }
    const a = wp[tour.idx], b = wp[(tour.idx + 1) % wp.length];
    const dist = Math.hypot(b.position[0] - a.position[0], b.position[1] - a.position[1], b.position[2] - a.position[2]) || 1;
    tour.frac += ((t.speed ?? 2) * dt) / dist;
    if (tour.frac >= 1) {
      tour.idx++;
      if (tour.idx >= wp.length - 1) { if (t.loop_tour) tour.idx = 0; else { stopTour(); return; } }
      tour.frac = 0; tour.paused = wp[tour.idx].pause_duration || 0;
      showCaption(wp[tour.idx].description);
    }
    const na = wp[tour.idx], nb = wp[(tour.idx + 1) % wp.length], f = tour.frac;
    camera.position.set(na.position[0] + (nb.position[0] - na.position[0]) * f, na.position[1] + (nb.position[1] - na.position[1]) * f, na.position[2] + (nb.position[2] - na.position[2]) * f);
    camera.lookAt(na.look_at[0] + (nb.look_at[0] - na.look_at[0]) * f, na.look_at[1] + (nb.look_at[1] - na.look_at[1]) * f, na.look_at[2] + (nb.look_at[2] - na.look_at[2]) * f);
    if (t.mode === 'walk') camera.position.y = Math.max(camera.position.y, Math.min(na.position[1], nb.position[1]));
  }
  if (opts.tourButton) {
    opts.tourButton.style.display = tours.length ? '' : 'none';
    opts.tourButton.addEventListener('click', () => { if (tour.active) stopTour(); else startTour(0); });
  }

  // ---- Triggers (world-types `trigger`) ----
  // The same semantics as world-bevy's trigger runtime: the visitor is the
  // camera; `host` actions (score, inventory, named sounds) are skipped.
  const triggerCaption = opts.triggerCaption || opts.tourCaption || null;
  let triggerTextTime = 0;
  /** @param {string|null|undefined} text @param {number|null|undefined} [seconds] */
  function showTriggerText(text, seconds) {
    if (!triggerCaption || !text) return;
    triggerCaption.textContent = text;
    triggerCaption.style.display = '';
    triggerTextTime = seconds ?? 4;
  }
  /** @param {EntityRecord} rec @param {TriggerState} trig */
  function runAction(rec, trig) {
    const action = trig.def.action || {};
    switch (action.action) {
      case 'show_text': showTriggerText(action.text, action.seconds); break;
      case 'show': rec.object.visible = true; break;
      case 'hide': rec.object.visible = false; break;
      case 'toggle': rec.object.visible = !rec.object.visible; break;
      case 'remove':
        rec.object.visible = false;
        for (const t of rec.triggers) t.done = true;
        break;
      case 'animate': {
        const anim = startAnimation(rec.object, action);
        if (anim) rec.anim = anim;
        break;
      }
      case 'teleport': {
        const d = action.destination || [0, 0, 0];
        const delta = new THREE.Vector3(d[0], d[1], d[2]).sub(camera.position);
        camera.position.add(delta);
        controls.target.add(delta);
        break;
      }
      default: break;
    }
  }
  // Proximity fires again every second while the visitor stays; the rest
  // have no cooldown unless they set one.
  /** @param {EntityRecord} rec @param {TriggerState} trig */
  function fireTrigger(rec, trig) {
    if (trig.done) return;
    const ev = trig.def.on?.event;
    const cooldown = trig.def.cooldown ?? (ev === 'proximity' ? 1 : 0);
    if (trig.last != null && elapsed - trig.last < cooldown) return;
    trig.last = elapsed;
    if (trig.def.once) trig.done = true;
    runAction(rec, trig);
  }
  const triggerPos = new THREE.Vector3();
  const triggerLocal = new THREE.Vector3();
  /** @param {number} dt */
  function updateTriggers(dt) {
    if (triggerTextTime > 0) {
      triggerTextTime -= dt;
      if (triggerTextTime <= 0 && !tour.active && triggerCaption) triggerCaption.style.display = 'none';
    }
    for (const rec of records) {
      for (const trig of rec.triggers) {
        const ev = trig.def.on || {};
        switch (ev.event) {
          case 'start':
            if (!trig.inside) { trig.inside = true; fireTrigger(rec, trig); }
            break;
          case 'proximity': {
            const near = rec.object.getWorldPosition(triggerPos).distanceTo(camera.position) <= (ev.radius ?? 5);
            if (near) fireTrigger(rec, trig);
            break;
          }
          case 'area_enter': case 'area_exit': {
            rec.object.updateWorldMatrix(true, false);
            triggerLocal.copy(camera.position);
            rec.object.worldToLocal(triggerLocal);
            const inside = volumeContains(trig.area, triggerLocal);
            if (inside !== trig.inside && inside === (ev.event === 'area_enter')) fireTrigger(rec, trig);
            trig.inside = inside;
            break;
          }
          case 'collision': {
            const inside = rec.object.getWorldPosition(triggerPos).distanceTo(camera.position) <= (ev.radius ?? 3);
            if (inside && !trig.inside) fireTrigger(rec, trig);
            trig.inside = inside;
            break;
          }
          case 'timer': {
            if (!(ev.interval > 0)) break;
            trig.acc += dt;
            if (trig.acc >= ev.interval) { trig.acc -= ev.interval; fireTrigger(rec, trig); }
            break;
          }
          default: break;
        }
      }
      if (rec.anim && stepAnimation(rec.object, rec.anim, dt)) rec.anim = null;
    }
  }
  // A click (not a drag, which orbits) fires the click triggers of the
  // nearest hit entity or, failing that, of its nearest ancestor that has
  // some, so clicking any part of an instance fires the instance's trigger.
  // Reach is measured from the visitor to the entity's origin.
  const raycaster = new THREE.Raycaster();
  const press = { x: 0, y: 0, down: false };
  /** @param {PointerEvent} e */
  const onPointerDown = (e) => { press.x = e.clientX; press.y = e.clientY; press.down = true; };
  /** @param {PointerEvent} e */
  const onPointerUp = (e) => {
    if (!press.down) return;
    press.down = false;
    if (Math.hypot(e.clientX - press.x, e.clientY - press.y) > 5) return;
    const rect = renderer.domElement.getBoundingClientRect();
    const ndc = new THREE.Vector2(((e.clientX - rect.left) / rect.width) * 2 - 1, -((e.clientY - rect.top) / rect.height) * 2 + 1);
    raycaster.setFromCamera(ndc, camera);
    const hit = raycaster.intersectObjects(scene.children, true).find((h) => h.object.visible);
    if (!hit) return;
    for (let o = /** @type {THREE.Object3D|null} */ (hit.object); o; o = o.parent) {
      const rec = records.find((r) => r.object === o);
      const clicks = rec ? rec.triggers.filter((t) => t.def.on?.event === 'click') : [];
      if (!rec || !clicks.length) continue;
      const distance = rec.object.getWorldPosition(triggerPos).distanceTo(camera.position);
      for (const trig of clicks) if (distance <= (trig.def.on.max_distance ?? 5)) fireTrigger(rec, trig);
      break;
    }
  };
  renderer.domElement.addEventListener('pointerdown', onPointerDown);
  renderer.domElement.addEventListener('pointerup', onPointerUp);

  // ---- Frame loop ----
  const clock = new THREE.Clock();
  let elapsed = 0;
  let running = true;
  /** @param {number} dt */
  function tick(dt) {
    elapsed += dt;
    for (const rec of records) {
      if (rec.resetPosition) rec.object.position.copy(rec.base.position);
      if (rec.resetScale) rec.object.scale.copy(rec.base.scale);
    }
    for (const rec of records) for (const b of rec.behaviors) b(dt, elapsed);
    const playing = !!(audioState.element && !audioState.element.paused);
    if (playing) updateLive();
    const st = soundtrackTime(elapsed);
    for (const rec of records) if (rec.mods.length) applyModulations(rec, dt, soundtrack ? st : elapsed, playing);
    updateSpatial();
    updateMovement(dt);
    updateTour(dt);
    updateTriggers(dt);
    controls.update();
    if (!shotFrame && camera.aspect !== (container.clientWidth || width) / (container.clientHeight || height)) resize();
    renderer.render(scene, camera);
  }
  function animate() {
    if (!running) return;
    requestAnimationFrame(animate);
    tick(Math.min(clock.getDelta(), 0.1));
  }
  animate();
  if (tours.length && tours[0].autostart) startTour(0);

  // ---- Embed API (postMessage) ----
  function sceneInfo() {
    return {
      type: 'sceneInfo',
      entityCount: records.length,
      cameraPosition: [camera.position.x, camera.position.y, camera.position.z],
      cameraTarget: [controls.target.x, controls.target.y, controls.target.z],
      tourCount: tours.length,
      triggerCount: records.reduce((n, r) => n + r.triggers.length, 0),
      audioEnabled: audioState.started,
      shots: shotList(manifest).map((s) => ({ name: s.name, ...s.shot })),
      shot: shotView ? opts.shot : null,
      viewerVersion: VIEWER_VERSION,
    };
  }
  /** @param {MessageEvent} event */
  const onMessage = (event) => {
    const msg = event.data;
    if (!msg || typeof msg !== 'object' || !msg.action) return;
    switch (msg.action) {
      case 'startTour': startTour(msg.index || 0); break;
      case 'stopTour': stopTour(); break;
      case 'toggleAudio': toggleAudio(); break;
      case 'setCameraPosition': if (Array.isArray(msg.position) && msg.position.length === 3) camera.position.set(.../** @type {Vec3} */ (msg.position)); break;
      case 'setCameraTarget': if (Array.isArray(msg.target) && msg.target.length === 3) { controls.target.set(.../** @type {Vec3} */ (msg.target)); controls.update(); } break;
      case 'getSceneInfo': (/** @type {Window|null} */ (event.source))?.postMessage(sceneInfo(), event.origin !== 'null' ? event.origin : '*'); break;
      default: break;
    }
  };
  if (opts.embedApi && typeof window !== 'undefined') {
    window.addEventListener('message', onMessage);
    if (window.parent !== window) window.parent.postMessage({ type: 'localgpt-scene-ready' }, '*');
  }

  function dispose() {
    running = false;
    stopAudio();
    resizeObserver?.disconnect();
    if (typeof document !== 'undefined') { document.removeEventListener('keydown', onKeyDown); document.removeEventListener('keyup', onKeyUp); }
    if (typeof window !== 'undefined') window.removeEventListener('message', onMessage);
    renderer.domElement.removeEventListener('pointerdown', onPointerDown);
    renderer.domElement.removeEventListener('pointerup', onPointerUp);
    controls.dispose();
    renderer.dispose();
    renderer.domElement.remove();
  }

  return {
    scene, camera, renderer, controls,
    entities: byName,
    entitiesById: byId,
    tours,
    startTour, stopTour, toggleAudio, sceneInfo, tick, applyOps, dispose,
    get audioEnabled() { return audioState.started; },
  };
}
