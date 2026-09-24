// world-viewer.js — the one web renderer of LocalGPT world manifests.
//
// Input: a `WorldManifest` as JSON (crate `localgpt-world-types`; schema in
// `crates/world-types/world.schema.json`). Output: a three.js scene that
// draws it the way the Bevy renderer (`localgpt-world-bevy`) does: colours are
// linear RGBA, rotations XYZ Euler degrees, directional light intensity in
// lux, point and spot lights in lumens, spot angles in radians.
//
// This file is embedded verbatim by `localgpt-world-export::html::generate_html`
// (Gen's `gen_export_html`, MD's `--export x.html`) and served as a module by
// localgpt.world. Keep it dependency-free beyond `three` and its addons, and
// never write the string "</" followed by "script" in it.
//
// Usage:
//   import { createWorldViewer } from './world-viewer.js';
//   const viewer = createWorldViewer(container, manifest, { assetBase: 'assets/' });

import * as THREE from 'three';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';

export const VIEWER_VERSION = '0.1.0';

/// Calibration between Bevy's light units and three.js', in one place.
/// Bevy renders physical units through an exposure; three's lights are
/// unitless, so one exposure applies to every light: a directional light of
/// DIRECTIONAL_LUX lux is intensity 1.0, and point/spot lights go
/// lumens → candela (÷ 4π) → ÷ DIRECTIONAL_LUX. Bevy's
/// `GlobalAmbientLight.brightness` (default 80) scales by AMBIENT_SCALE.
export const AMBIENT_SCALE = 0.0012;
export const DIRECTIONAL_LUX = 10000;
const LIGHT_EXPOSURE = 1 / DIRECTIONAL_LUX;

const DEFAULT_CAMERA = { position: [5, 5, 5], look_at: [0, 0, 0], fov_degrees: 45 };
const DEFAULT_MATERIAL = { color: [0.8, 0.8, 0.8, 1.0], metallic: 0.0, roughness: 0.5, emissive: [0, 0, 0, 0] };
const TAU = Math.PI * 2;

// ---------------------------------------------------------------------------
// Pure helpers (mirrors of the Rust ones in localgpt-world-types)
// ---------------------------------------------------------------------------

/**
 * Colour conventions, the same as the Bevy mapping: `color`, light colours,
 * background, fog and ambient are sRGB-encoded (what Gen's tools take and
 * `Color::srgba` reads); `emissive` is linear (`LinearRgba::new`).
 */
export function srgbColor(c) {
  const [r, g, b] = c || [1, 1, 1, 1];
  return new THREE.Color().setRGB(r, g, b, THREE.SRGBColorSpace);
}

/** Linear RGBA array → three Color (three's working space is linear). */
export function linearColor(c) {
  const [r, g, b] = c || [1, 1, 1, 1];
  return new THREE.Color(r, g, b);
}

/** `SoundtrackDef::energy_at` / `curve_at`: per-second curve, linear interpolation. */
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

/** `SoundtrackDef::beat_at`: 1 on a beat, decaying to 0 at the next. */
export function beatAt(soundtrack, t) {
  const bpm = soundtrack?.bpm || 0;
  if (!(bpm > 0)) return 0;
  const period = 60 / bpm;
  const since = mod(t - (soundtrack.beat_offset || 0), period);
  return 1 - since / period;
}

/** `SoundtrackDef::section_at`. */
export function sectionAt(soundtrack, t) {
  const sections = soundtrack?.sections || [];
  if (!(soundtrack?.duration > 0) || sections.length === 0) return 0;
  const frac = clamp01(t / soundtrack.duration);
  let idx = 0;
  for (let i = 0; i < sections.length; i++) if (sections[i] <= frac) idx = i;
  return idx;
}

/** `ModulationDef::factor`. */
export function modulationFactor(def, signal) {
  const [a, b] = def.range || [1, 1];
  return a + (b - a) * clamp01(signal);
}

function clamp01(v) { return Math.min(1, Math.max(0, v)); }
function mod(a, n) { return ((a % n) + n) % n; }
function variant(v) {
  // Externally tagged enum: "energy" → ["energy", null]; {stem: "drums"} → ["stem", "drums"].
  if (typeof v === 'string') return [v, null];
  if (v && typeof v === 'object') { const k = Object.keys(v)[0]; return [k, v[k]]; }
  return [null, null];
}

// ---------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------

function flatGeometry(triangles) {
  const positions = new Float32Array(triangles.length * 9);
  let o = 0;
  for (const tri of triangles) for (const v of tri) { positions[o++] = v[0]; positions[o++] = v[1]; positions[o++] = v[2]; }
  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  geo.computeVertexNormals();
  return geo;
}

/** A square-based pyramid centered on the origin: base at -h/2, apex at +h/2. */
function pyramidGeometry(bx, bz, h) {
  const hx = bx / 2, hz = bz / 2, hy = h / 2;
  const a = [-hx, -hy, -hz], b = [hx, -hy, -hz], c = [hx, -hy, hz], d = [-hx, -hy, hz], apex = [0, hy, 0];
  return flatGeometry([[a, b, apex], [b, c, apex], [c, d, apex], [d, a, apex], [a, d, b], [b, d, c]]);
}

/** A ramp: right-triangle profile in XY (vertical face at -x, slope down toward +x), extruded along Z. */
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

export function createMaterial(def) {
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
  return material;
}

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
    light = new THREE.SpotLight(color, ((def.intensity ?? 800) / (4 * Math.PI)) * LIGHT_EXPOSURE, range, outer, penumbra, 2);
    light.castShadow = def.shadows !== false;
    direction = def.direction || [0, -1, 0];
  }
  if (direction) {
    // Bevy aims the light along `direction`; three aims it at `target`.
    light.target.position.set(position[0] + direction[0], position[1] + direction[1], position[2] + direction[2]);
  }
  return light;
}

// ---------------------------------------------------------------------------
// Behaviors (all seven, evaluated from the entity's authored transform)
// ---------------------------------------------------------------------------

function makeBehavior(def, rec, byName) {
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
      const centerRec = p.center != null ? byName.get(typeof p.center === 'string' ? p.center : String(p.center)) : null;
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
      const name = typeof p.target === 'string' ? p.target : String(p.target);
      return () => { const tgt = byName.get(name); if (tgt) obj.lookAt(tgt.object.getWorldPosition(new THREE.Vector3())); };
    }
    default: return null;
  }
}

// ---------------------------------------------------------------------------
// Procedural audio (Web Audio), one graph per AudioDef
// ---------------------------------------------------------------------------

function noiseBuffer(ctx, seconds, fill) {
  const buf = ctx.createBuffer(1, Math.floor(ctx.sampleRate * seconds), ctx.sampleRate);
  const d = buf.getChannelData(0);
  fill(d, ctx.sampleRate);
  return buf;
}

function whiteNoise(ctx, seconds = 2, gain = 1) {
  return noiseBuffer(ctx, seconds, (d) => { for (let i = 0; i < d.length; i++) d[i] = (Math.random() * 2 - 1) * gain; });
}

function loopSource(ctx, buffer) {
  const src = ctx.createBufferSource(); src.buffer = buffer; src.loop = true; return src;
}

function filterNode(ctx, type, frequency, q) {
  const f = ctx.createBiquadFilter(); f.type = type; f.frequency.value = frequency; if (q != null) f.Q.value = q; return f;
}

/** Builds the graph for one audio source into `out`; returns a stop function. */
function buildAudioSource(ctx, audio, out) {
  const [kind, p] = variant(audio.source);
  const stops = [];
  const chain = (src, ...nodes) => {
    let prev = src; for (const n of nodes) { prev.connect(n); prev = n; } prev.connect(out);
    if (src.start) { src.start(); stops.push(() => { try { src.stop(); } catch (_) { /* already stopped */ } }); }
  };
  const timers = [];
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
      const ftype = String(p.filter_type || 'lowpass').toLowerCase();
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
// The viewer
// ---------------------------------------------------------------------------

/**
 * Render `manifest` into `container`.
 *
 * options:
 *   assetBase    URL prefix for mesh assets and the soundtrack file ('' = none: placeholders, silent).
 *   audioButton  element whose click toggles audio (shown when the world has sound).
 *   tourButton   element whose click starts/stops the first tour (shown when tours exist).
 *   tourCaption  element that shows waypoint descriptions.
 *   keyboard     WASD/Space/Shift navigation (default true).
 *   embedApi     postMessage API for a parent frame (default: when framed).
 *   ambientScale override for AMBIENT_SCALE.
 */
export function createWorldViewer(container, manifest, options = {}) {
  const opts = { keyboard: true, embedApi: typeof window !== 'undefined' && window.parent !== window, ...options };
  const assetBase = opts.assetBase ? String(opts.assetBase).replace(/\/?$/, '/') : '';
  const env = manifest.environment || {};

  // ---- Scene, camera, renderer ----
  const scene = new THREE.Scene();
  if (env.background_color) scene.background = srgbColor(env.background_color);
  if (env.fog_density > 0) {
    // Bevy: exponential fog in `fog_density`, coloured by the fog colour, else the background.
    const fogColor = env.fog_color || env.background_color;
    scene.fog = new THREE.Fog(fogColor ? srgbColor(fogColor) : new THREE.Color(1, 1, 1), 1, 100 / Math.max(env.fog_density, 0.01));
  }
  scene.add(new THREE.AmbientLight(env.ambient_color ? srgbColor(env.ambient_color) : new THREE.Color(1, 1, 1),
    (env.ambient_intensity ?? 80) * (opts.ambientScale ?? AMBIENT_SCALE)));

  const cam = manifest.camera || (manifest.avatar
    ? { position: manifest.avatar.spawn_position || DEFAULT_CAMERA.position, look_at: manifest.avatar.spawn_look_at || DEFAULT_CAMERA.look_at, fov_degrees: DEFAULT_CAMERA.fov_degrees }
    : DEFAULT_CAMERA);
  const width = container.clientWidth || (typeof window !== 'undefined' ? window.innerWidth : 800);
  const height = container.clientHeight || (typeof window !== 'undefined' ? window.innerHeight : 600);
  const camera = new THREE.PerspectiveCamera(cam.fov_degrees ?? DEFAULT_CAMERA.fov_degrees, width / height, 0.1, 1000);
  camera.position.set(...(cam.position || DEFAULT_CAMERA.position));

  const renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: !!opts.preserveDrawingBuffer });
  renderer.setPixelRatio(Math.min(typeof window !== 'undefined' ? window.devicePixelRatio : 1, 2));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.0;
  container.appendChild(renderer.domElement);

  const controls = new OrbitControls(camera, renderer.domElement);
  controls.target.set(...(cam.look_at || DEFAULT_CAMERA.look_at));
  controls.enableDamping = true;
  controls.dampingFactor = 0.05;
  controls.update();

  function resize() {
    const w = container.clientWidth || width, h = container.clientHeight || height;
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
  const records = [];
  const byName = new Map();
  const byId = new Map();
  const gltfLoader = assetBase ? new GLTFLoader() : null;
  for (const def of manifest.entities || []) {
    const t = def.transform || {};
    const position = t.position || [0, 0, 0];
    const hasShape = !!def.shape, hasLight = !!def.light;
    let object, material = null, light = null;
    if (hasShape) {
      material = createMaterial(def.material);
      object = new THREE.Mesh(createGeometry(def.shape), material);
      object.castShadow = true;
      object.receiveShadow = true;
    } else if (def.mesh_asset) {
      object = new THREE.Group();
      const placeholder = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), new THREE.MeshBasicMaterial({ color: 0x888888, wireframe: true }));
      object.add(placeholder);
      if (gltfLoader) {
        gltfLoader.load(assetBase + def.mesh_asset.path, (gltf) => {
          object.remove(placeholder);
          const node = def.mesh_asset.node ? gltf.scene.getObjectByName(def.mesh_asset.node) || gltf.scene : gltf.scene;
          node.traverse((o) => { if (o.isMesh) { o.castShadow = true; o.receiveShadow = true; } });
          object.add(node);
        }, undefined, () => { /* keep the placeholder */ });
      }
    } else if (!hasLight) {
      object = new THREE.Group();
    }
    if (hasLight) {
      light = createLight(def.light, position);
      if (object) object.add(light); else object = light;
      if (light.target) scene.add(light.target);
    }
    object.position.set(...position);
    const rot = t.rotation_degrees || [0, 0, 0];
    object.rotation.set(THREE.MathUtils.degToRad(rot[0]), THREE.MathUtils.degToRad(rot[1]), THREE.MathUtils.degToRad(rot[2]));
    object.scale.set(...(t.scale || [1, 1, 1]));
    if (t.visible === false) object.visible = false;
    object.name = def.name;
    const rec = {
      def, object, material, light,
      base: {
        position: object.position.clone(),
        scale: object.scale.clone(),
        emissive: material?.emissive ? material.emissive.clone() : null,
        emissiveIntensity: material?.emissiveIntensity ?? 0,
        lightIntensity: light?.intensity ?? 0,
        opacity: material?.opacity ?? 1,
      },
      behaviors: [], mods: [], resetPosition: false, resetScale: false,
    };
    records.push(rec);
    byName.set(def.name, rec);
    byId.set(String(def.id), rec);
  }
  for (const rec of records) {
    const parent = rec.def.parent != null ? byId.get(String(rec.def.parent)) : null;
    (parent ? parent.object : scene).add(rec.object);
  }
  for (const rec of records) {
    for (const b of rec.def.behaviors || []) { const fn = makeBehavior(b, rec, byName); if (fn) rec.behaviors.push(fn); }
    for (const m of rec.def.modulations || []) {
      const [target] = variant(m.target);
      if (target === 'offset_y') rec.resetPosition = true;
      if (target === 'scale') rec.resetScale = true;
      rec.mods.push({ def: m, target, s: 0 });
    }
  }

  // ---- Soundtrack and signals ----
  const soundtrack = manifest.soundtrack || null;
  const audioState = { ctx: null, started: false, stops: [], element: null, analyser: null, bins: null, spatial: [] };
  const live = { bass: 0, highs: 0 };

  function soundtrackTime(elapsed) {
    if (audioState.element && !audioState.element.paused) return audioState.element.currentTime;
    return soundtrack && soundtrack.duration > 0 ? mod(elapsed, soundtrack.duration) : elapsed;
  }

  function updateLive() {
    const a = audioState.analyser;
    if (!a) return;
    a.getByteFrequencyData(audioState.bins);
    const bins = audioState.bins;
    let bass = 0, highs = 0, nb = 0, nh = 0;
    for (let i = 1; i <= 3; i++) { bass += bins[i]; nb++; }
    for (let i = 24; i < bins.length; i++) { highs += bins[i]; nh++; }
    live.bass = nb ? bass / nb / 255 : 0;
    live.highs = nh ? Math.min(1, (highs / nh / 255) * 2.5) : 0;
  }

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

  function applyModulations(rec, dt, t, playing) {
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
      switch (m.target) {
        case 'emissive': if (rec.material?.emissive) rec.material.emissiveIntensity = rec.base.emissiveIntensity * factor; break;
        case 'scale': rec.object.scale.multiplyScalar(factor); break;
        case 'light_intensity': if (rec.light) rec.light.intensity = rec.base.lightIntensity * factor; break;
        case 'opacity': if (rec.material) { rec.material.opacity = rec.base.opacity * factor; rec.material.transparent = true; } break;
        case 'offset_y': rec.object.position.y += factor; break;
        default: break;
      }
    }
  }

  // ---- Audio control ----
  const hasAudio = records.some((r) => r.def.audio) || !!(soundtrack && soundtrack.path);
  function startAudio() {
    const AudioCtx = typeof window !== 'undefined' ? (window.AudioContext || window.webkitAudioContext) : null;
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
  const keys = {};
  const moveSpeed = manifest.avatar?.movement_speed ?? 5;
  const onKeyDown = (e) => { keys[e.code] = true; };
  const onKeyUp = (e) => { keys[e.code] = false; };
  if (opts.keyboard && typeof document !== 'undefined') {
    document.addEventListener('keydown', onKeyDown);
    document.addEventListener('keyup', onKeyUp);
  }
  function updateMovement(dt) {
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
  const tour = { active: null, idx: 0, frac: 0, paused: 0 };
  function showCaption(text) {
    if (!opts.tourCaption) return;
    if (text) { opts.tourCaption.textContent = text; opts.tourCaption.style.display = ''; } else opts.tourCaption.style.display = 'none';
  }
  function startTour(index = 0) {
    const t = tours[index]; if (!t) return;
    tour.active = t; tour.idx = 0; tour.frac = 0; tour.paused = 0;
    controls.enabled = false;
    if (opts.tourButton) opts.tourButton.textContent = 'Stop Tour';
    const wp0 = t.waypoints[0];
    if (t.mode === 'teleport') { camera.position.set(...wp0.position); camera.lookAt(...wp0.look_at); }
    showCaption(wp0.description);
  }
  function stopTour() {
    tour.active = null;
    controls.enabled = true;
    if (opts.tourButton) opts.tourButton.textContent = 'Start Tour';
    showCaption(null);
  }
  function updateTour(dt) {
    const t = tour.active; if (!t) return;
    const wp = t.waypoints;
    if (tour.paused > 0) { tour.paused -= dt; return; }
    if (t.mode === 'teleport') {
      tour.idx++;
      if (tour.idx >= wp.length - 1) { if (t.loop_tour) tour.idx = 0; else { stopTour(); return; } }
      const next = wp[tour.idx];
      camera.position.set(...next.position); camera.lookAt(...next.look_at);
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

  // ---- Frame loop ----
  const clock = new THREE.Clock();
  let elapsed = 0;
  let running = true;
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
    controls.update();
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
      audioEnabled: audioState.started,
      viewerVersion: VIEWER_VERSION,
    };
  }
  const onMessage = (event) => {
    const msg = event.data;
    if (!msg || typeof msg !== 'object' || !msg.action) return;
    switch (msg.action) {
      case 'startTour': startTour(msg.index || 0); break;
      case 'stopTour': stopTour(); break;
      case 'toggleAudio': toggleAudio(); break;
      case 'setCameraPosition': if (Array.isArray(msg.position) && msg.position.length === 3) camera.position.set(...msg.position); break;
      case 'setCameraTarget': if (Array.isArray(msg.target) && msg.target.length === 3) { controls.target.set(...msg.target); controls.update(); } break;
      case 'getSceneInfo': event.source?.postMessage(sceneInfo(), event.origin !== 'null' ? event.origin : '*'); break;
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
    controls.dispose();
    renderer.dispose();
    renderer.domElement.remove();
  }

  return {
    scene, camera, renderer, controls,
    entities: byName,
    tours,
    startTour, stopTour, toggleAudio, sceneInfo, tick, dispose,
    get audioEnabled() { return audioState.started; },
  };
}
