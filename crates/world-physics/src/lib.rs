//! # localgpt-world-physics
//!
//! The `ext-physics` extension's second reference implementation (the
//! JS reference is `openworldformat/physics`; the extension's spec is
//! `openworldformat/spec/extensions/physics.md`). Bodies are declared,
//! never inferred — only entities carrying the component participate —
//! and what a solver *does* stays the engine's. What this crate is:
//!
//! - [`collect_physics`] — the bodies a world declares;
//! - [`simulate`] — a deliberately minimal deterministic solver (spheres
//!   against floors and axis-aligned statics, fixed-timestep
//!   semi-implicit Euler, sleep on rest). Reference-grade, not
//!   production physics; conforming engines use real ones;
//! - [`fold_trajectories`]/[`trajectory_op`] — the extension's op kind:
//!   playback without a solver;
//! - [`run_outcomes`] — the conformance outcome runner: one assertion
//!   file, any engine, the same predicates.
//!
//! Determinism is by construction: no randomness, bodies in document
//! order, fixed dt, IEEE-754 doubles — the same algorithm as the JS
//! reference, step for step. Same engine and version, same trajectory.
//! Cross-engine, only the semantic contract holds (contact, rest,
//! bounce counts) — never bit-exact, exactly as the spec refuses.

use std::collections::BTreeMap;

use localgpt_world_sync::oplog::OpLogEntry;
use localgpt_world_sync::session::{ExtensionRecord, SessionOp};
use localgpt_world_types as wt;
use serde::{Deserialize, Serialize};
use wt::shape::Shape;

/// The extension this crate implements.
pub const EXTENSION_NAME: &str = "ext-physics";

/// The extension version this crate implements.
pub const EXTENSION_VERSION: &str = "0.1.0";

/// An impact at or above this speed (m/s) counts as a bounce.
pub const BOUNCE_SPEED: f64 = 0.5;

/// A dynamic body in contact moving slower than this (m/s) sleeps.
pub const SLEEP_SPEED: f64 = 0.05;

/// Default gravity when the environment block is absent (spec).
pub const DEFAULT_GRAVITY: [f64; 3] = [0.0, -9.81, 0.0];

// ---------------------------------------------------------------------------
// The declared bodies
// ---------------------------------------------------------------------------

/// One entity's `ext-physics` component, parsed from `WorldEntity::extra`.
#[derive(Debug, Clone, PartialEq)]
pub struct BodyComponent {
    /// `"static"`, `"kinematic"` or `"dynamic"`.
    pub body: String,
    /// kg. Dynamic bodies only.
    pub mass: f64,
    /// The collision volume: derived from the shape, or an explicit
    /// primitive.
    pub collider: Collider,
    /// 0..=1, bounciness of contacts.
    pub restitution: f64,
    /// 0..=1, tangential damping at contacts.
    pub friction: f64,
    /// Multiplies the environment gravity for this body.
    pub gravity_scale: f64,
    /// Velocity loss per second, contact or not.
    pub linear_damping: f64,
}

/// The collision volume an entity declares.
#[derive(Debug, Clone, PartialEq)]
pub enum Collider {
    /// Derive from the entity's parametric shape.
    Shape,
    /// An explicit sphere of this radius.
    Sphere(f64),
    /// An explicit cuboid with these full extents.
    Cuboid([f64; 3]),
}

impl BodyComponent {
    /// Parse the component from an entity's `extra` map. `None` when the
    /// entity declares no body — participation is declared, never
    /// inferred.
    pub fn of(entity: &wt::WorldEntity) -> Option<Self> {
        let value = entity.extra.get(EXTENSION_NAME)?.as_object()?;
        let collider = match value.get("collider") {
            None => Collider::Shape,
            Some(serde_json::Value::String(s)) if s == "shape" => Collider::Shape,
            Some(serde_json::Value::Object(o)) => {
                if let Some(r) = o.get("sphere").and_then(serde_json::Value::as_f64) {
                    Collider::Sphere(r)
                } else if let Some(e) = o.get("cuboid").and_then(|v| v.as_array()).and_then(|a| {
                    let e: Vec<f64> = a.iter().filter_map(serde_json::Value::as_f64).collect();
                    (e.len() == 3).then_some([e[0], e[1], e[2]])
                }) {
                    Collider::Cuboid(e)
                } else {
                    Collider::Shape
                }
            }
            _ => Collider::Shape,
        };
        let num = |k: &str, d: f64| {
            value
                .get(k)
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(d)
        };
        Some(BodyComponent {
            body: value.get("body")?.as_str()?.to_string(),
            mass: num("mass", 1.0),
            collider,
            restitution: num("restitution", 0.5),
            friction: num("friction", 0.5),
            gravity_scale: num("gravity_scale", 1.0),
            linear_damping: num("linear_damping", 0.0),
        })
    }
}

/// A dynamic body as the solver holds it.
#[derive(Debug, Clone)]
pub struct DynamicBody {
    /// The entity's id.
    pub id: u64,
    /// The entity's name.
    pub name: String,
    /// Position, world units.
    pub position: [f64; 3],
    /// Velocity, m/s.
    pub velocity: [f64; 3],
    /// The dynamics are spheres in the reference: the collider's radius,
    /// or the bounding sphere of whatever was declared.
    pub radius: f64,
    /// kg.
    pub mass: f64,
    /// 0..=1.
    pub restitution: f64,
    /// 0..=1.
    pub friction: f64,
    pub gravity_scale: f64,
    pub linear_damping: f64,
    /// Asleep bodies stop integrating; they rest.
    pub asleep: bool,
}

/// A static collider. A `Plane` shape is the floor at its y (the
/// reference ignores its rotation); anything else is an axis-aligned box
/// around its extents.
#[derive(Debug, Clone)]
pub struct StaticBody {
    /// The entity's id.
    pub id: u64,
    /// The entity's name.
    pub name: String,
    /// Floor at `y`, or a box between `min` and `max`.
    pub kind: StaticKind,
}

/// Which static an entity is.
#[derive(Debug, Clone)]
pub enum StaticKind {
    /// An infinite floor at this y.
    Floor(f64),
    /// An axis-aligned box.
    Box { min: [f64; 3], max: [f64; 3] },
}

/// The physics a world declares: gravity, the dynamic bodies, the
/// kinematic names, and the static colliders.
#[derive(Debug, Clone, Default)]
pub struct PhysicsWorld {
    /// Gravity, m/s². Defaults to `[0, -9.81, 0]` when undeclared.
    pub gravity: [f64; 3],
    /// The simulated bodies, in document order.
    pub dynamic: Vec<DynamicBody>,
    /// Kinematic bodies (moved by behaviors, not the solver); the
    /// reference treats them as static colliders.
    pub kinematic: Vec<String>,
    /// Static colliders, in document order.
    pub statics: Vec<StaticBody>,
}

/// The parametric shape as full extents `[x, y, z]` (its bounding box).
fn shape_extents(shape: &Shape) -> Option<[f64; 3]> {
    let f = |v: f32| v as f64;
    Some(match shape {
        Shape::Cuboid { x, y, z } => [f(*x), f(*y), f(*z)],
        Shape::Wedge { x, y, z } => [f(*x), f(*y), f(*z)],
        Shape::Sphere { radius } => [f(*radius) * 2.0; 3],
        Shape::Cylinder { radius, height } | Shape::Cone { radius, height } => {
            [f(*radius) * 2.0, f(*height), f(*radius) * 2.0]
        }
        Shape::Capsule {
            radius,
            half_length,
        } => [
            f(*radius) * 2.0,
            (f(*half_length) + f(*radius)) * 2.0,
            f(*radius) * 2.0,
        ],
        Shape::Torus {
            major_radius,
            minor_radius,
        } => {
            let w = (f(*major_radius) + f(*minor_radius)) * 2.0;
            [w, f(*minor_radius) * 2.0, w]
        }
        Shape::Pyramid {
            base_x,
            base_z,
            height,
        } => [f(*base_x), f(*height), f(*base_z)],
        Shape::Tetrahedron { radius } | Shape::Icosahedron { radius } => [f(*radius) * 2.0; 3],
        Shape::Plane { .. } => return None,
    })
}

/// Collect the physics a manifest declares. Entities appear in document
/// order, matching the fold's array order.
pub fn collect_physics(manifest: &wt::WorldManifest) -> PhysicsWorld {
    let mut world = PhysicsWorld {
        gravity: manifest
            .environment
            .as_ref()
            .and_then(|env| env.extra.get(EXTENSION_NAME))
            .and_then(|v| v.get("gravity"))
            .and_then(|v| v.as_array())
            .and_then(|a| {
                let g: Vec<f64> = a.iter().filter_map(serde_json::Value::as_f64).collect();
                (g.len() == 3).then_some([g[0], g[1], g[2]])
            })
            .unwrap_or(DEFAULT_GRAVITY),
        ..PhysicsWorld::default()
    };
    for entity in &manifest.entities {
        let Some(component) = BodyComponent::of(entity) else {
            continue;
        };
        let position: [f64; 3] = entity.transform.position.map(|p| p as f64);
        match component.body.as_str() {
            "dynamic" => {
                // The JS reference's collider derivation: a `"shape"`
                // collider over a Sphere shape is that sphere; over any
                // other shape it's the bounding box.
                let effective = match &component.collider {
                    Collider::Shape if matches!(entity.shape, Some(Shape::Sphere { .. })) => {
                        let Shape::Sphere { radius } = entity.shape.as_ref().unwrap() else {
                            unreachable!()
                        };
                        Collider::Sphere(*radius as f64)
                    }
                    other => other.clone(),
                };
                let extents = match &effective {
                    Collider::Sphere(r) => Some([r * 2.0; 3]),
                    Collider::Cuboid(e) => Some(*e),
                    Collider::Shape => entity.shape.as_ref().and_then(shape_extents),
                };
                let radius = match &effective {
                    Collider::Sphere(r) => Some(*r),
                    _ => extents.map(|e| {
                        let h = [e[0] / 2.0, e[1] / 2.0, e[2] / 2.0];
                        (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt()
                    }),
                }
                .unwrap_or(0.5);
                world.dynamic.push(DynamicBody {
                    id: entity.id.0,
                    name: entity.name.as_str().to_string(),
                    position,
                    velocity: [0.0; 3],
                    radius,
                    mass: component.mass,
                    restitution: component.restitution,
                    friction: component.friction,
                    gravity_scale: component.gravity_scale,
                    linear_damping: component.linear_damping,
                    asleep: false,
                });
            }
            "kinematic" => world.kinematic.push(entity.name.as_str().to_string()),
            _ => {}
        }
        if component.body != "dynamic" {
            push_static(&mut world.statics, entity, position, &component.collider);
        }
    }
    world
}

fn push_static(
    statics: &mut Vec<StaticBody>,
    entity: &wt::WorldEntity,
    position: [f64; 3],
    collider: &Collider,
) {
    let id = entity.id.0;
    let name = entity.name.as_str().to_string();
    if matches!(entity.shape, Some(Shape::Plane { .. })) && !matches!(collider, Collider::Sphere(_))
    {
        statics.push(StaticBody {
            id,
            name,
            kind: StaticKind::Floor(position[1]),
        });
        return;
    }
    let extents = match collider {
        Collider::Cuboid(e) => Some(*e),
        Collider::Sphere(r) => Some([r * 2.0; 3]),
        Collider::Shape => entity.shape.as_ref().and_then(shape_extents),
    };
    let Some(e) = extents else { return };
    let h = [e[0] / 2.0, e[1] / 2.0, e[2] / 2.0];
    statics.push(StaticBody {
        id,
        name,
        kind: StaticKind::Box {
            min: [position[0] - h[0], position[1] - h[1], position[2] - h[2]],
            max: [position[0] + h[0], position[1] + h[1], position[2] + h[2]],
        },
    });
}

// ---------------------------------------------------------------------------
// The solver
// ---------------------------------------------------------------------------

/// One recorded sample: where every dynamic body was, at `t_s`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Seconds since load.
    pub t_s: f64,
    /// Body name → position.
    pub bodies: BTreeMap<String, [f64; 3]>,
}

/// One impact: a body met a static.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    /// Seconds since load.
    pub t_s: f64,
    /// The dynamic body's name.
    pub body: String,
    /// The static's name.
    pub other: String,
    /// Where the body was.
    pub position: [f64; 3],
    /// The approach speed along the contact normal, m/s.
    pub normal_speed: f64,
}

/// One bounce: an impact at or above [`BOUNCE_SPEED`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bounce {
    /// The dynamic body's name.
    pub body: String,
    /// The static's name.
    pub other: String,
    /// Seconds since load.
    pub t_s: f64,
}

/// A simulation's result: samples for playback, contacts and bounces
/// for outcomes, resting positions for assertions.
#[derive(Debug, Clone, PartialEq)]
pub struct Simulation {
    /// Samples, ≤ 1 / `sample_dt_s` per second.
    pub samples: Vec<Sample>,
    /// Impacts, in order.
    pub contacts: Vec<Contact>,
    /// Impacts at or above [`BOUNCE_SPEED`].
    pub bounces: Vec<Bounce>,
    /// Each dynamic body's final position.
    pub resting: BTreeMap<String, [f64; 3]>,
    /// When every body slept (or when the simulation ended).
    pub settled_s: f64,
}

/// Simulation options. Defaults mirror the JS reference: 1/120 s steps,
/// 10 Hz samples, 5 s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimOptions {
    /// How long to simulate.
    pub until_s: f64,
    /// The fixed timestep.
    pub dt_s: f64,
    /// The sampling period.
    pub sample_dt_s: f64,
}

impl Default for SimOptions {
    fn default() -> Self {
        SimOptions {
            until_s: 5.0,
            dt_s: 1.0 / 120.0,
            sample_dt_s: 0.1,
        }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn len(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// Resolve one body against one static. `Some(impact)` on contact (0
/// when resting against it), `None` on a miss — the JS reference's
/// `-1`.
fn resolve_contact(body: &mut DynamicBody, stat: &StaticBody) -> Option<f64> {
    let n;
    match stat.kind {
        StaticKind::Floor(y) => {
            let pen = y + body.radius - body.position[1];
            if pen <= 0.0 {
                return None;
            }
            body.position[1] = y + body.radius;
            n = [0.0, 1.0, 0.0];
        }
        StaticKind::Box { min, max } => {
            let p = body.position;
            let c = [
                p[0].clamp(min[0], max[0]),
                p[1].clamp(min[1], max[1]),
                p[2].clamp(min[2], max[2]),
            ];
            let d = [p[0] - c[0], p[1] - c[1], p[2] - c[2]];
            let dist = len(d);
            if dist >= body.radius {
                return None;
            }
            n = if dist > 0.0 {
                [d[0] / dist, d[1] / dist, d[2] / dist]
            } else {
                [0.0, 1.0, 0.0]
            };
            body.position = [
                c[0] + n[0] * body.radius,
                c[1] + n[1] * body.radius,
                c[2] + n[2] * body.radius,
            ];
        }
    }
    let vn = dot(body.velocity, n);
    if vn >= 0.0 {
        return Some(0.0); // resting against it, no impact
    }
    // Reflect by restitution, damp tangentially by friction.
    let restitution = -vn * body.restitution;
    let keep = 1.0 - body.friction;
    for (v, ni) in body.velocity.iter_mut().zip(n.iter()) {
        let vt = *v - ni * vn;
        *v = ni * restitution + vt * keep;
    }
    Some(-vn)
}

/// Simulate a world: deterministic, fixed-timestep, semi-implicit
/// Euler — the JS reference's algorithm, step for step, so the two
/// engines agree on outcomes.
pub fn simulate(manifest: &wt::WorldManifest, opts: &SimOptions) -> Simulation {
    let mut world = collect_physics(manifest);
    let dt = opts.dt_s;
    let sample_dt = opts.sample_dt_s;
    let mut t = 0.0f64;
    let mut next_sample = 0.0f64;
    let mut settled: Option<f64> = None;

    let sample = |t: f64, dynamic: &[DynamicBody]| Sample {
        t_s: (t * 10000.0).round() / 10000.0,
        bodies: dynamic
            .iter()
            .map(|b| (b.name.clone(), b.position))
            .collect::<BTreeMap<_, _>>(),
    };
    let mut samples = vec![sample(0.0, &world.dynamic)];
    let mut contacts = Vec::new();
    let mut bounces = Vec::new();

    while t < opts.until_s && settled.is_none() {
        for body in &mut world.dynamic {
            if body.asleep {
                continue;
            }
            // Integrate, then resolve against every static in order.
            let g = [
                world.gravity[0] * body.gravity_scale,
                world.gravity[1] * body.gravity_scale,
                world.gravity[2] * body.gravity_scale,
            ];
            let damp = body.linear_damping * dt;
            for (v, gi) in body.velocity.iter_mut().zip(g.iter()) {
                *v += (gi - damp * *v) * dt;
            }
            for (p, v) in body.position.iter_mut().zip(body.velocity.iter()) {
                *p += v * dt;
            }
            let mut touching = false;
            for stat in &world.statics {
                if let Some(impact) = resolve_contact(body, stat) {
                    touching = true;
                    if impact >= SLEEP_SPEED {
                        contacts.push(Contact {
                            t_s: (t * 10000.0).round() / 10000.0,
                            body: body.name.clone(),
                            other: stat.name.clone(),
                            position: body.position,
                            normal_speed: (impact * 10000.0).round() / 10000.0,
                        });
                        if impact >= BOUNCE_SPEED {
                            bounces.push(Bounce {
                                body: body.name.clone(),
                                other: stat.name.clone(),
                                t_s: (t * 10000.0).round() / 10000.0,
                            });
                        }
                    }
                }
            }
            if touching && len(body.velocity) < SLEEP_SPEED {
                body.velocity = [0.0; 3];
                body.asleep = true;
            }
        }
        t += dt;
        if !world.dynamic.is_empty() && world.dynamic.iter().all(|b| b.asleep) && settled.is_none()
        {
            settled = Some(t);
        }
        if t >= next_sample {
            samples.push(sample(t, &world.dynamic));
            next_sample += sample_dt;
        }
    }

    Simulation {
        samples,
        contacts,
        bounces,
        resting: world
            .dynamic
            .iter()
            .map(|b| (b.name.clone(), b.position))
            .collect(),
        settled_s: (settled.unwrap_or(t) * 10000.0).round() / 10000.0,
    }
}

// ---------------------------------------------------------------------------
// Trajectories — playback without a solver
// ---------------------------------------------------------------------------

/// The trajectories a log's `ext-physics` ops carried: per body, the
/// sampled `(t, position)` track.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trajectories {
    /// Body name → samples, in log order.
    pub bodies: BTreeMap<String, Vec<(f64, [f64; 3])>>,
    /// The newest sample time across every body.
    pub span_s: f64,
}

/// Fold the `ext-physics` trajectory ops out of a log: playback data,
/// folding to nothing for the document (the fold proper ignores them;
/// this reads what they carried).
pub fn fold_trajectories(entries: &[OpLogEntry]) -> Trajectories {
    let mut tracks = Trajectories::default();
    for entry in entries {
        for op in &entry.ops {
            let SessionOp::Extension(ExtensionRecord { name, body }) = op else {
                continue;
            };
            if name != EXTENSION_NAME {
                continue;
            }
            let Some(times) = body.get("t_s").and_then(|v| v.as_array()) else {
                continue;
            };
            let Some(bodies) = body.get("bodies").and_then(|v| v.as_object()) else {
                continue;
            };
            for (body_name, positions) in bodies {
                let Some(positions) = positions.as_array() else {
                    continue;
                };
                let track = tracks.bodies.entry(body_name.clone()).or_default();
                for (i, position) in positions.iter().enumerate().take(times.len()) {
                    let Some(p) = position.as_array() else {
                        continue;
                    };
                    if p.len() != 3 {
                        continue;
                    }
                    let xyz: Vec<f64> = p.iter().filter_map(serde_json::Value::as_f64).collect();
                    if xyz.len() != 3 {
                        continue;
                    }
                    let t = times[i].as_f64().unwrap_or(0.0);
                    track.push((t, [xyz[0], xyz[1], xyz[2]]));
                    if t > tracks.span_s {
                        tracks.span_s = t;
                    }
                }
            }
        }
    }
    tracks
}

/// Build the `ext-physics` trajectory op from simulation samples (the
/// writer's half of playback). Sample at or below 10 Hz, per the spec.
pub fn trajectory_op(sim: &Simulation) -> SessionOp {
    let t_s: Vec<f64> = sim.samples.iter().map(|s| s.t_s).collect();
    let mut bodies = serde_json::Map::new();
    for sample in &sim.samples {
        for (name, position) in &sample.bodies {
            bodies
                .entry(name.clone())
                .or_insert_with(|| serde_json::Value::Array(Vec::new()))
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!(position));
        }
    }
    SessionOp::Extension(ExtensionRecord {
        name: EXTENSION_NAME.to_string(),
        body: serde_json::json!({ "t_s": t_s, "bodies": bodies }),
    })
}

// ---------------------------------------------------------------------------
// Outcome assertions — the extension's conformance
// ---------------------------------------------------------------------------

/// A conformance outcomes document: predicates over a simulation, not
/// pixels. `options` and `simulate_s` select the run; `expect` holds
/// the assertions.
#[derive(Debug, Clone, Deserialize)]
pub struct OutcomesDoc {
    /// The world the assertions belong to (a conformance path; the
    /// runner's caller resolves it).
    #[serde(default)]
    pub world: String,
    /// Options overriding the defaults (dt, sampling).
    #[serde(default)]
    pub options: Option<OutcomeOptions>,
    /// How long to simulate.
    pub simulate_s: f64,
    /// The assertions.
    pub expect: Vec<Assertion>,
}

/// The subset of [`SimOptions`] an outcomes file may set.
#[derive(Debug, Clone, Deserialize)]
pub struct OutcomeOptions {
    /// The fixed timestep.
    #[serde(default)]
    pub dt_s: Option<f64>,
    /// The sampling period.
    #[serde(default)]
    pub sample_dt_s: Option<f64>,
}

/// One assertion. The shapes match the conformance JSON: `contact`,
/// `rest`, `bounces`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Assertion {
    /// Two named bodies touch, within `within_s` seconds.
    Contact {
        /// `[dynamic, static]`, either order.
        contact: [String; 2],
        /// The deadline, seconds.
        #[serde(default)]
        within_s: Option<f64>,
    },
    /// A body's resting position is within `tolerance` of `near`.
    Rest {
        /// The rest assertion.
        rest: RestAssertion,
    },
    /// A body's impacts above [`BOUNCE_SPEED`] number at least `min`.
    Bounces {
        /// The bounce assertion.
        bounces: BounceAssertion,
    },
}

/// The `rest` assertion's payload.
#[derive(Debug, Clone, Deserialize)]
pub struct RestAssertion {
    /// The dynamic body's name.
    pub body: String,
    /// Where it should rest.
    pub near: [f64; 3],
    /// The tolerance radius (default 0.15).
    #[serde(default)]
    pub tolerance: Option<f64>,
}

/// The `bounces` assertion's payload.
#[derive(Debug, Clone, Deserialize)]
pub struct BounceAssertion {
    /// The dynamic body's name.
    pub body: String,
    /// The minimum bounce count.
    pub min: u32,
}

/// An outcomes run's verdict.
#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeResult {
    /// True when every assertion held.
    pub ok: bool,
    /// One line per failure, for the runner's report.
    pub failures: Vec<String>,
}

/// Run a conformance outcomes document against a world: simulate, then
/// check every assertion. Physics conformance is what the world *does*,
/// not what it renders.
pub fn run_outcomes(manifest: &wt::WorldManifest, outcomes: &OutcomesDoc) -> OutcomeResult {
    let defaults = SimOptions::default();
    let opts = SimOptions {
        until_s: outcomes.simulate_s,
        dt_s: outcomes
            .options
            .as_ref()
            .and_then(|o| o.dt_s)
            .unwrap_or(defaults.dt_s),
        sample_dt_s: outcomes
            .options
            .as_ref()
            .and_then(|o| o.sample_dt_s)
            .unwrap_or(defaults.sample_dt_s),
    };
    let sim = simulate(manifest, &opts);
    let mut failures = Vec::new();
    for assertion in &outcomes.expect {
        match assertion {
            Assertion::Contact { contact, within_s } => {
                let limit = within_s.unwrap_or(f64::INFINITY);
                let hit = sim.contacts.iter().any(|c| {
                    c.t_s <= limit
                        && ((c.body == contact[0] && c.other == contact[1])
                            || (c.body == contact[1] && c.other == contact[0]))
                });
                if !hit {
                    failures.push(format!(
                        "contact {}/{} within {limit}s never happened",
                        contact[0], contact[1]
                    ));
                }
            }
            Assertion::Rest { rest } => {
                let tolerance = rest.tolerance.unwrap_or(0.15);
                let ok = sim
                    .resting
                    .get(&rest.body)
                    .map(|at| {
                        len([
                            at[0] - rest.near[0],
                            at[1] - rest.near[1],
                            at[2] - rest.near[2],
                        ]) <= tolerance
                    })
                    .unwrap_or(false);
                if !ok {
                    failures.push(format!(
                        "{} rests at {:?}, not within {tolerance} of {:?}",
                        rest.body,
                        sim.resting.get(&rest.body),
                        rest.near
                    ));
                }
            }
            Assertion::Bounces { bounces } => {
                let count = sim
                    .bounces
                    .iter()
                    .filter(|b| b.body == bounces.body)
                    .count() as u32;
                if count < bounces.min {
                    failures.push(format!(
                        "{} bounced {count} times, expected at least {}",
                        bounces.body, bounces.min
                    ));
                }
            }
        }
    }
    OutcomeResult {
        ok: failures.is_empty(),
        failures,
    }
}
