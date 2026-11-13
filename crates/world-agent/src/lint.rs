//! What a world looks like to its cameras, in numbers — and what in it is
//! probably a mistake.
//!
//! `verify` asks whether a package agrees with itself. This asks whether
//! the world it holds is the one its author meant: a camera standing inside
//! a wall, a shot whose subject is behind something, two singles on opposite
//! sides of the line, a figure sunk to its waist in the floor, a trigger
//! whose action this app keeps and never runs. Each finding is a
//! [`Diagnostic`] with a stable [`code`](Diagnostic::code), the entities it
//! is about and a hint, and each shot gets a [`ShotReport`]: what its frame
//! holds, and how much of the frame each thing fills.
//!
//! No renderer, no window and no model: everything here is arithmetic over
//! the document — the shapes' boxes as the format states them
//! (`Shape::local_aabb_half`), the transform hierarchy, and the
//! cinematography extension's own projection, so "in frame" means here what
//! it means to the conformance suite. That is also the limit: a box is not a
//! silhouette, so coverage is the projected box's, and a mesh asset, whose
//! size the document does not state, is counted and left unmeasured.
//!
//! The codes are listed in [`RULES`]; the package guide and the user
//! documentation are held to that list by tests.

use std::collections::HashMap;

use localgpt_world_types as wt;
use serde_json::{Value, json};
use wt::cinematography as cinema;

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Probably a mistake.
    Warning,
    /// Worth knowing; often deliberate.
    Note,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Warning => "warning",
            Self::Note => "note",
        }
    }
}

/// One rule the lint checks: its code, how much a finding matters, and what
/// it means, as the documentation's table says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rule {
    pub code: &'static str,
    pub severity: Severity,
    pub what: &'static str,
}

pub const CAMERA_INSIDE: &str = "camera-inside";
pub const EMPTY_FRAME: &str = "empty-frame";
pub const SUBJECT_HIDDEN: &str = "subject-hidden";
pub const FOCUS_OUT_OF_FRAME: &str = "focus-out-of-frame";
pub const DANGLING_TARGET: &str = "dangling-target";
pub const LINE_CROSSED: &str = "line-crossed";
pub const SHOT_TIMING: &str = "shot-timing";
pub const DUPLICATE: &str = "duplicate";
pub const TINY_WORLD: &str = "tiny-world";
pub const SUNK: &str = "sunk";
pub const CARRIED_NOT_RUN: &str = "carried-not-run";
pub const NOTHING_TO_DRAW: &str = "nothing-to-draw";

/// Every rule, in the order the documentation lists them. A code is a
/// contract: it is never renamed, and a rule that goes is retired, not
/// reused.
pub const RULES: &[Rule] = &[
    Rule {
        code: CAMERA_INSIDE,
        severity: Severity::Warning,
        what: "a camera, or the view a visitor opens on, stands inside a solid shape",
    },
    Rule {
        code: EMPTY_FRAME,
        severity: Severity::Warning,
        what: "a camera, or the opening view, has no shape in its frame",
    },
    Rule {
        code: SUBJECT_HIDDEN,
        severity: Severity::Warning,
        what: "something solid stands between a camera and the entity it aims at or focuses on",
    },
    Rule {
        code: FOCUS_OUT_OF_FRAME,
        severity: Severity::Warning,
        what: "a camera focuses on an entity that is outside its frame",
    },
    Rule {
        code: DANGLING_TARGET,
        severity: Severity::Warning,
        what: "a camera's aim_entity or focus_entity is not in the world, so the fallback applies",
    },
    Rule {
        code: LINE_CROSSED,
        severity: Severity::Warning,
        what: "a shot and its reverse are on opposite sides of the line between their subjects",
    },
    Rule {
        code: SHOT_TIMING,
        severity: Severity::Warning,
        what: "a shot ends at or before the second it starts",
    },
    Rule {
        code: DUPLICATE,
        severity: Severity::Warning,
        what: "two entities are the same shape in the same place",
    },
    Rule {
        code: TINY_WORLD,
        severity: Severity::Warning,
        what: "the whole world is smaller than 5 cm: probably not authored in metres",
    },
    Rule {
        code: SUNK,
        severity: Severity::Note,
        what: "a shape's centre is at the top of the ground under it, so half of it is below: a position is a shape's centre",
    },
    Rule {
        code: CARRIED_NOT_RUN,
        severity: Severity::Note,
        what: "content the format carries and this app does not run: it is kept, and nothing happens",
    },
    Rule {
        code: NOTHING_TO_DRAW,
        severity: Severity::Note,
        what: "no entity has a shape or a mesh",
    },
];

/// The rule a code names.
pub fn rule(code: &str) -> Option<&'static Rule> {
    RULES.iter().find(|rule| rule.code == code)
}

/// The trigger actions the app defines for itself (`action: "host"`), by
/// name. A host action with any other name is kept in the world and skipped
/// when its trigger fires; Gen's interaction system matches on these
/// constants, so the list and the code that runs them cannot part.
pub mod host_action {
    pub const ADD_SCORE: &str = "add_score";
    pub const PLAY_SOUND: &str = "play_sound";
    pub const SET_STATE: &str = "set_state";
    /// Written by `gen_add_trigger`'s `spawn` action and run by nothing:
    /// Gen's interaction system has no arm for it. Named here so the tool
    /// that writes it and the lint that reports it use one word; it joins
    /// [`ALL`] when something runs it.
    pub const SPAWN: &str = "spawn";
    /// Every host action the app runs.
    pub const ALL: [&str; 3] = [ADD_SCORE, PLAY_SOUND, SET_STATE];
}

/// The transform properties an `animate` action moves. The format names
/// three; `translation` is the app's older word for `position`.
const ANIMATED: [&str; 4] = ["position", "translation", "rotation", "scale"];

/// The extensions this workspace has code for, and the one that is a record
/// with nothing to run. Any other registered extension rides along unread.
const EXTENSIONS_RUN: [&str; 1] = [cinema::EXTENSION_NAME];
const EXTENSIONS_INERT: [&str; 1] = ["ext-provenance"];

/// One finding.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    /// The rule's code, one of [`RULES`].
    pub code: &'static str,
    pub severity: Severity,
    /// The entities it is about, the one to change first.
    pub entities: Vec<String>,
    /// What is the case.
    pub message: String,
    /// What would change it.
    pub hint: String,
}

impl Diagnostic {
    fn new(
        code: &'static str,
        entities: &[&str],
        message: impl Into<String>,
        hint: impl Into<String>,
    ) -> Self {
        Self {
            code,
            severity: rule(code).map_or(Severity::Note, |rule| rule.severity),
            entities: entities.iter().map(|name| name.to_string()).collect(),
            message: message.into(),
            hint: hint.into(),
        }
    }

    /// `warning[camera-inside]: 1B is inside wall-back` — the one-line form.
    pub fn line(&self) -> String {
        format!(
            "{}[{}]: {}",
            self.severity.as_str(),
            self.code,
            self.message
        )
    }

    pub fn to_json(&self) -> Value {
        json!({
            "code": self.code,
            "severity": self.severity.as_str(),
            "entities": self.entities,
            "message": self.message,
            "hint": self.hint,
        })
    }
}

/// One thing in a camera's frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Framed {
    pub name: String,
    /// The share of the frame its projected box covers, 0–1.
    pub covers: f64,
    /// The share of the frame's height its projected box spans, 0–1.
    pub height: f64,
    /// Where its box's centre lands, in frame halves: `[0, 0]` is the
    /// centre, `[-1, 1]` the top left corner.
    pub centre: [f64; 2],
    /// The part of the frame its box covers, `[x0, y0, x1, y1]` in frame
    /// halves: what `covers`, `height` and `centre` are read from.
    pub rect: [f64; 4],
    /// How far its origin is from the lens, in metres.
    pub distance: f64,
}

impl Framed {
    fn of(name: &str, rect: [f64; 4], distance: f64) -> Self {
        let (w, h) = (rect[2] - rect[0], rect[3] - rect[1]);
        Self {
            name: name.to_string(),
            covers: w * h / 4.0,
            height: h / 2.0,
            centre: [(rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0],
            rect,
            distance,
        }
    }

    /// `MAYA 29%`, `KAI 22% (right)`, `MAYA.face <1% (centre)`.
    fn said(&self) -> String {
        let place = match self.centre {
            [x, y] if x.abs() < 0.2 && y.abs() < 0.2 => " (centre)",
            [x, _] if x <= -0.5 => " (left)",
            [x, _] if x >= 0.5 => " (right)",
            _ => "",
        };
        match (self.covers * 100.0).round() {
            share if share < 1.0 => format!("{} <1%{place}", self.name),
            share => format!("{} {share}%{place}", self.name),
        }
    }
}

/// What one camera sees.
#[derive(Debug, Clone, PartialEq)]
pub struct ShotReport {
    pub id: u64,
    pub name: String,
    /// The `shot` block, when the camera is a setup in the shot list.
    pub shot: Option<Value>,
    pub focal_length_mm: f64,
    pub hfov_degrees: f64,
    pub vfov_degrees: f64,
    pub aspect: f64,
    /// The entity the camera aims at, when it aims at one that exists.
    pub aims_at: Option<String>,
    /// The shapes whose boxes reach into the frame, the nearest first:
    /// foreground, then what is behind it.
    pub sees: Vec<Framed>,
}

/// The lint's whole answer for one world.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    pub diagnostics: Vec<Diagnostic>,
    pub shots: Vec<ShotReport>,
    /// Entities in the world, creations' instances expanded to their parts.
    pub entities: usize,
    /// Entities whose geometry is a mesh file: counted, not measured.
    pub unmeasured: usize,
}

impl Report {
    /// How many findings are probably mistakes.
    pub fn warnings(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "ok": self.warnings() == 0,
            "entities": self.entities,
            "unmeasured": self.unmeasured,
            "diagnostics": self.diagnostics.iter().map(Diagnostic::to_json).collect::<Vec<_>>(),
            "shots": self.shots.iter().map(shot_json).collect::<Vec<_>>(),
        })
    }

    /// The report as short lines: a line per camera, then a line per finding
    /// with its hint under it.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for shot in &self.shots {
            lines.push(shot_line(shot));
        }
        for d in &self.diagnostics {
            lines.push(d.line());
            if !d.hint.is_empty() {
                lines.push(format!("    {}", d.hint));
            }
        }
        let (warnings, notes) = (self.warnings(), self.diagnostics.len() - self.warnings());
        let measured = match self.unmeasured {
            0 => String::new(),
            n => format!(" ({n} mesh{} not measured)", if n == 1 { "" } else { "es" }),
        };
        lines.push(match (warnings, notes) {
            (0, 0) => format!(
                "ok: nothing to flag in {} entities{measured}",
                self.entities
            ),
            _ => format!(
                "{warnings} warning{}, {notes} note{} in {} entities{measured}",
                if warnings == 1 { "" } else { "s" },
                if notes == 1 { "" } else { "s" },
                self.entities
            ),
        });
        lines
    }
}

fn round(v: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (v * k).round() / k
}

fn shot_json(shot: &ShotReport) -> Value {
    json!({
        "id": shot.id,
        "name": shot.name,
        "shot": shot.shot,
        "focal_length_mm": round(shot.focal_length_mm, 2),
        "hfov_degrees": round(shot.hfov_degrees, 2),
        "vfov_degrees": round(shot.vfov_degrees, 2),
        "aspect": round(shot.aspect, 3),
        "aims_at": shot.aims_at,
        "sees": shot.sees.iter().map(|f| json!({
            "name": f.name,
            "covers": round(f.covers, 3),
            "height": round(f.height, 3),
            "centre": [round(f.centre[0], 2), round(f.centre[1], 2)],
            "rect": f.rect.map(|v| round(v, 2)),
            "distance": round(f.distance, 2),
        })).collect::<Vec<_>>(),
    })
}

/// `1B    MCU     50 mm  on MAYA.head 14% (centre) · KAI 22% (right) · MAYA 29% · +2 more`:
/// the subject first, when the camera aims at one, then each figure in the
/// frame, the nearest first — a figure being an entity and its parts
/// (`MAYA`, `MAYA.head`, `MAYA.face`) taken as one.
fn shot_line(shot: &ShotReport) -> String {
    const SHOWN: usize = 4;
    let size = shot
        .shot
        .as_ref()
        .and_then(|s| s["size"].as_str())
        .unwrap_or("-");
    let mut figures: Vec<Framed> = Vec::new();
    for seen in &shot.sees {
        match figures.iter_mut().find(|f| f.name == figure(&seen.name)) {
            Some(whole) => {
                let rect = [
                    whole.rect[0].min(seen.rect[0]),
                    whole.rect[1].min(seen.rect[1]),
                    whole.rect[2].max(seen.rect[2]),
                    whole.rect[3].max(seen.rect[3]),
                ];
                *whole = Framed::of(&whole.name, rect, whole.distance.min(seen.distance));
            }
            None => figures.push(Framed::of(figure(&seen.name), seen.rect, seen.distance)),
        }
    }
    figures.sort_by(|a, b| {
        a.distance
            .partial_cmp(&b.distance)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.name.cmp(&b.name))
    });
    let subject = shot
        .sees
        .iter()
        .find(|f| Some(&f.name) == shot.aims_at.as_ref());
    let mut parts: Vec<String> = subject
        .map(|subject| format!("on {}", subject.said()))
        .into_iter()
        .collect();
    parts.extend(figures.iter().take(SHOWN).map(Framed::said));
    if figures.len() > SHOWN {
        parts.push(format!("+{} more", figures.len() - SHOWN));
    }
    let seen = match (parts.is_empty(), subject) {
        (true, _) => "sees nothing".to_string(),
        (false, Some(_)) => parts.join(" · "),
        (false, None) => format!("sees {}", parts.join(" · ")),
    };
    format!(
        "{:<5} {:<6} {:>3} mm  {seen}",
        shot.name,
        size,
        shot.focal_length_mm.round()
    )
}

// ---------------------------------------------------------------------------
// Geometry: the document's transforms and the shapes' boxes
// ---------------------------------------------------------------------------

pub(crate) type V3 = [f64; 3];

pub(crate) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scaled(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub(crate) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn length(a: V3) -> f64 {
    dot(a, a).sqrt()
}

pub(crate) fn unit(a: V3) -> Option<V3> {
    let l = length(a);
    (l > 1e-9).then(|| scaled(a, 1.0 / l))
}

/// `v` turned by intrinsic XYZ Euler degrees, R = Rx·Ry·Rz: the transform
/// convention of the format (spec/world.md, "Conventions").
fn rotated(v: V3, degrees: V3) -> V3 {
    let [rx, ry, rz] = degrees.map(f64::to_radians);
    let [x, y, z] = v;
    let (s, c) = rz.sin_cos();
    let (x, y) = (c * x - s * y, s * x + c * y);
    let (s, c) = ry.sin_cos();
    let (x, z) = (c * x + s * z, -s * x + c * z);
    let (s, c) = rx.sin_cos();
    let (y, z) = (c * y - s * z, s * y + c * z);
    [x, y, z]
}

/// A point's way from an entity's own frame to the world's: `p ↦ M·p + t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    /// The columns of `M`: where the local axes land.
    axes: [V3; 3],
    origin: V3,
}

impl Affine {
    fn of(transform: &wt::WorldTransform) -> Self {
        let rotation = transform.rotation_degrees.map(f64::from);
        let scale = transform.scale.map(f64::from);
        let axis = |i: usize| {
            let mut e = [0.0; 3];
            e[i] = scale[i];
            rotated(e, rotation)
        };
        Self {
            axes: [axis(0), axis(1), axis(2)],
            origin: transform.position.map(f64::from),
        }
    }

    /// The direction `v` as the world has it (no translation).
    fn turn(&self, v: V3) -> V3 {
        add(
            add(scaled(self.axes[0], v[0]), scaled(self.axes[1], v[1])),
            scaled(self.axes[2], v[2]),
        )
    }

    /// The point `p` in the world.
    pub fn apply(&self, p: V3) -> V3 {
        add(self.turn(p), self.origin)
    }

    /// `self ∘ inner`: a child's transform seen through its parent's.
    fn then(&self, inner: &Self) -> Self {
        Self {
            axes: inner.axes.map(|axis| self.turn(axis)),
            origin: self.apply(inner.origin),
        }
    }

    /// The way back, when the transform has one (a zero scale has none).
    fn inverse(&self) -> Option<Self> {
        let [a, b, c] = self.axes;
        let det = dot(a, cross(b, c));
        if det.abs() < 1e-12 {
            return None;
        }
        // The rows of the inverse are the cross products of the columns.
        let rows = [
            scaled(cross(b, c), 1.0 / det),
            scaled(cross(c, a), 1.0 / det),
            scaled(cross(a, b), 1.0 / det),
        ];
        let axes = [
            [rows[0][0], rows[1][0], rows[2][0]],
            [rows[0][1], rows[1][1], rows[2][1]],
            [rows[0][2], rows[1][2], rows[2][2]],
        ];
        let back = Self {
            axes,
            origin: [0.0; 3],
        };
        let origin = scaled(back.turn(self.origin), -1.0);
        Some(Self { axes, origin })
    }
}

/// An entity as the lint measures it.
pub(crate) struct Placed<'a> {
    pub(crate) entity: &'a wt::WorldEntity,
    pub(crate) world: Affine,
    /// Its shape's box in its own frame, as half extents.
    pub(crate) half: Option<V3>,
    /// The box in the world, axis-aligned: `[min, max]`.
    pub(crate) bounds: Option<[V3; 2]>,
}

impl Placed<'_> {
    pub(crate) fn name(&self) -> &str {
        self.entity.name.as_str()
    }

    /// Where its origin is in the world.
    pub(crate) fn origin(&self) -> V3 {
        self.world.apply([0.0; 3])
    }

    /// Something a camera cannot see through and a figure cannot stand in:
    /// a visible shape with thickness and an opaque material.
    fn solid(&self) -> bool {
        let opaque = self
            .entity
            .material
            .as_ref()
            .is_none_or(|material| material.color[3] >= 0.5);
        self.entity.transform.visible
            && opaque
            && !matches!(self.entity.shape, None | Some(wt::Shape::Plane { .. }))
    }

    /// The eight corners of its box, in the world.
    fn corners(&self) -> Option<[V3; 8]> {
        let h = self.half?;
        let mut corners = [[0.0; 3]; 8];
        for (i, corner) in corners.iter_mut().enumerate() {
            let sign = |bit: usize| if i >> bit & 1 == 0 { -1.0 } else { 1.0 };
            *corner = self
                .world
                .apply([sign(0) * h[0], sign(1) * h[1], sign(2) * h[2]]);
        }
        Some(corners)
    }
}

/// The parent links are followed this far; a longer chain is a cycle.
const MAX_DEPTH: usize = 64;

pub(crate) fn place(manifest: &wt::WorldManifest) -> Vec<Placed<'_>> {
    let by_id: HashMap<u64, &wt::WorldEntity> =
        manifest.entities.iter().map(|e| (e.id.0, e)).collect();
    manifest
        .entities
        .iter()
        .map(|entity| {
            let mut world = Affine::of(&entity.transform);
            let mut current = entity;
            for _ in 0..MAX_DEPTH {
                let Some(parent) = current.parent.and_then(|id| by_id.get(&id.0)) else {
                    break;
                };
                world = Affine::of(&parent.transform).then(&world);
                current = parent;
            }
            let half = entity
                .shape
                .as_ref()
                .map(|shape| shape.local_aabb_half().map(f64::from));
            let mut placed = Placed {
                entity,
                world,
                half,
                bounds: None,
            };
            placed.bounds = placed.corners().map(|corners| {
                let mut min = [f64::INFINITY; 3];
                let mut max = [f64::NEG_INFINITY; 3];
                for corner in corners {
                    for axis in 0..3 {
                        min[axis] = min[axis].min(corner[axis]);
                        max[axis] = max[axis].max(corner[axis]);
                    }
                }
                [min, max]
            });
            placed
        })
        .collect()
}

/// Whether `p`, in a shape's own frame, is inside it. `None` for the shapes
/// this module has no exact test for: no claim is better than a wrong one.
fn contains(shape: &wt::Shape, p: V3) -> Option<bool> {
    let [x, y, z] = p;
    let radial = (x * x + z * z).sqrt();
    Some(match *shape {
        wt::Shape::Cuboid { x: w, y: h, z: d } => {
            x.abs() <= f64::from(w) / 2.0
                && y.abs() <= f64::from(h) / 2.0
                && z.abs() <= f64::from(d) / 2.0
        }
        wt::Shape::Sphere { radius } => length(p) <= f64::from(radius),
        wt::Shape::Cylinder { radius, height } => {
            radial <= f64::from(radius) && y.abs() <= f64::from(height) / 2.0
        }
        wt::Shape::Capsule {
            radius,
            half_length,
        } => {
            let on_axis = y.clamp(-f64::from(half_length), f64::from(half_length));
            (radial * radial + (y - on_axis) * (y - on_axis)).sqrt() <= f64::from(radius)
        }
        wt::Shape::Cone { radius, height } => {
            let (radius, height) = (f64::from(radius), f64::from(height));
            y.abs() <= height / 2.0 && radial <= radius * (height / 2.0 - y) / height.max(1e-9)
        }
        _ => return None,
    })
}

/// Where the segment `a → b` (in a shape's own frame) is inside the box of
/// half extents `h`, as the span of its parameter, when it enters at all.
fn clip_to_box(a: V3, b: V3, h: V3) -> Option<(f64, f64)> {
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for axis in 0..3 {
        let d = b[axis] - a[axis];
        if d.abs() < 1e-12 {
            if a[axis].abs() > h[axis] {
                return None;
            }
            continue;
        }
        let (near, far) = ((-h[axis] - a[axis]) / d, (h[axis] - a[axis]) / d);
        t0 = t0.max(near.min(far));
        t1 = t1.min(near.max(far));
        if t0 > t1 {
            return None;
        }
    }
    Some((t0, t1))
}

/// How far along the world segment `from → to` it first enters `solid`, as
/// a parameter in 0–1, when it does.
fn enters(solid: &Placed<'_>, from: V3, to: V3) -> Option<f64> {
    const SAMPLES: usize = 48;
    let shape = solid.entity.shape.as_ref()?;
    let back = solid.world.inverse()?;
    let (a, b) = (back.apply(from), back.apply(to));
    let (t0, t1) = clip_to_box(a, b, solid.half?)?;
    if matches!(shape, wt::Shape::Cuboid { .. }) {
        return Some(t0);
    }
    // The other shapes are inside their box: walk the span the box clips.
    (0..=SAMPLES)
        .map(|i| t0 + (t1 - t0) * i as f64 / SAMPLES as f64)
        .find(|&t| contains(shape, add(a, scaled(sub(b, a), t))) == Some(true))
}

/// Whether the world point `p` is inside `solid`, for the shapes that can say.
fn holds(solid: &Placed<'_>, p: V3) -> bool {
    let (Some(shape), Some(back)) = (solid.entity.shape.as_ref(), solid.world.inverse()) else {
        return false;
    };
    contains(shape, back.apply(p)) == Some(true)
}

/// The figure a name belongs to: `MAYA.head` and `MAYA.face` are parts of
/// `MAYA`, and an instance's parts are `<instance>/<part>`. Two entities of
/// one figure do not hide each other, and are one subject.
fn figure(name: &str) -> &str {
    name.split(['.', '/']).next().unwrap_or(name)
}

// ---------------------------------------------------------------------------
// Cameras
// ---------------------------------------------------------------------------

/// A camera as the lint looks through it: a pinhole, with the frame's half
/// width and half height at distance 1.
struct Lens {
    position: V3,
    forward: V3,
    right: V3,
    up: V3,
    tan_h: f64,
    tan_v: f64,
}

/// Nothing nearer the lens than this is in the frame, in metres.
const NEAR: f64 = 0.05;

impl Lens {
    fn of(
        entity: &wt::WorldEntity,
        camera: &cinema::CameraComponent,
        manifest: &wt::WorldManifest,
    ) -> Self {
        let view = cinema::view_in(entity, camera, manifest);
        let frame = cinema::frame_of(camera);
        Self {
            position: view.position,
            forward: view.forward,
            right: view.right,
            up: view.up,
            tan_h: frame.width_mm / (2.0 * camera.focal_length_mm),
            tan_v: frame.height_mm / (2.0 * camera.focal_length_mm),
        }
    }

    /// The view a visitor opens on. The window's shape is not the world's
    /// to say, so the frame is taken as 16:9.
    fn opening(camera: &wt::CameraDef) -> Option<Self> {
        let position = camera.position.map(f64::from);
        let forward = unit(sub(camera.look_at.map(f64::from), position))?;
        let right = unit(cross(forward, [0.0, 1.0, 0.0]))?;
        let tan_v = (f64::from(camera.fov_degrees).to_radians() / 2.0).tan();
        Some(Self {
            position,
            forward,
            right,
            up: cross(right, forward),
            tan_h: tan_v * 16.0 / 9.0,
            tan_v,
        })
    }

    /// A world point in the camera's own frame: right, up, and distance
    /// along the look.
    fn local(&self, p: V3) -> V3 {
        let d = sub(p, self.position);
        [dot(d, self.right), dot(d, self.up), dot(d, self.forward)]
    }

    /// A point in front of the lens, in frame halves.
    fn onto_frame(&self, local: V3) -> [f64; 2] {
        [
            local[0] / (local[2] * self.tan_h),
            local[1] / (local[2] * self.tan_v),
        ]
    }

    /// The rectangle a box covers in the frame, `[x0, y0, x1, y1]` in frame
    /// halves and not yet cut to the frame: its twelve edges, cut at the
    /// near plane, projected. `None` when the whole box is behind the lens.
    fn rect(&self, corners: &[V3; 8]) -> Option<[f64; 4]> {
        let local = corners.map(|corner| self.local(corner));
        let mut rect = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut any = false;
        let mut take = |p: V3| {
            let [x, y] = self.onto_frame(p);
            rect = [
                rect[0].min(x),
                rect[1].min(y),
                rect[2].max(x),
                rect[3].max(y),
            ];
            any = true;
        };
        for a in 0..8 {
            for bit in 0..3 {
                let b = a ^ (1 << bit);
                if a > b {
                    continue;
                }
                let (p, q) = (local[a], local[b]);
                match (p[2] >= NEAR, q[2] >= NEAR) {
                    (true, true) => {
                        take(p);
                        take(q);
                    }
                    (false, false) => {}
                    (p_in, _) => {
                        let t = (NEAR - p[2]) / (q[2] - p[2]);
                        take(add(p, scaled(sub(q, p), t)));
                        take(if p_in { p } else { q });
                    }
                }
            }
        }
        any.then_some(rect)
    }

    /// What of `placed` is in the frame, when any of it is.
    fn frames(&self, placed: &Placed<'_>) -> Option<Framed> {
        let rect = self.rect(&placed.corners()?)?;
        let cut = [
            rect[0].max(-1.0),
            rect[1].max(-1.0),
            rect[2].min(1.0),
            rect[3].min(1.0),
        ];
        if cut[0] > cut[2] || cut[1] > cut[3] {
            return None;
        }
        // A plane seen edge-on is in the frame and covers none of it.
        Some(Framed::of(
            placed.name(),
            cut,
            length(sub(placed.origin(), self.position)),
        ))
    }

    /// Every shape whose box reaches into the frame, the nearest first.
    fn sees(&self, world: &[Placed<'_>]) -> Vec<Framed> {
        let mut seen: Vec<Framed> = world
            .iter()
            .filter(|placed| placed.entity.transform.visible)
            .filter_map(|placed| self.frames(placed))
            .collect();
        seen.sort_by(|a, b| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.cmp(&b.name))
        });
        seen
    }
}

/// One camera entity, read.
struct Camera<'a> {
    placed: &'a Placed<'a>,
    component: cinema::CameraComponent,
    lens: Lens,
    shot: Option<&'a Value>,
}

impl Camera<'_> {
    fn name(&self) -> &str {
        self.placed.name()
    }

    fn scene(&self) -> Option<&str> {
        self.shot?.get("scene")?.as_str()
    }
}

fn metres(v: f64) -> String {
    format!("{:.2} m", v)
}

// ---------------------------------------------------------------------------
// The lint
// ---------------------------------------------------------------------------

/// Lint a world: every rule in [`RULES`], and a report per camera.
///
/// Instances of creations are expanded first, so a placed copy is measured
/// like anything else; the manifest itself is not changed.
pub fn lint(manifest: &wt::WorldManifest) -> Report {
    let mut expanded;
    let manifest = if manifest.has_instances() {
        expanded = manifest.clone();
        expanded.expand_instances();
        &expanded
    } else {
        manifest
    };
    let world = place(manifest);
    let mut report = Report {
        entities: world.len(),
        unmeasured: world
            .iter()
            .filter(|p| p.entity.mesh_asset.is_some() && p.entity.shape.is_none())
            .count(),
        ..Report::default()
    };
    let out = &mut report.diagnostics;

    let cameras: Vec<Camera<'_>> = world
        .iter()
        .filter_map(|placed| {
            let component = cinema::CameraComponent::of(placed.entity)?;
            Some(Camera {
                lens: Lens::of(placed.entity, &component, manifest),
                shot: placed
                    .entity
                    .extra
                    .get(cinema::EXTENSION_NAME)
                    .and_then(|ext| ext.get("shot"))
                    .filter(|shot| shot.is_object()),
                component,
                placed,
            })
        })
        .collect();

    if !world
        .iter()
        .any(|p| p.entity.shape.is_some() || p.entity.mesh_asset.is_some())
    {
        out.push(Diagnostic::new(
            NOTHING_TO_DRAW,
            &[],
            "no entity has a shape or a mesh: the world draws nothing",
            "spawn an entity with a `shape`",
        ));
    }

    for camera in &cameras {
        lint_camera(camera, &world, manifest, out);
        report.shots.push(ShotReport {
            id: camera.placed.entity.id.0,
            name: camera.name().to_string(),
            shot: camera.shot.cloned(),
            focal_length_mm: camera.component.focal_length_mm,
            hfov_degrees: (2.0 * camera.lens.tan_h.atan()).to_degrees(),
            vfov_degrees: (2.0 * camera.lens.tan_v.atan()).to_degrees(),
            aspect: camera.lens.tan_h / camera.lens.tan_v,
            aims_at: camera
                .component
                .aim_entity
                .as_ref()
                .and_then(|reference| cinema::entity_of(reference, manifest))
                .map(|entity| entity.name.as_str().to_string()),
            sees: camera.lens.sees(&world),
        });
    }
    // The shot list's order, cameras that are not setups after it.
    let order = |shot: &ShotReport| {
        shot.shot
            .as_ref()
            .and_then(|s| s["order"].as_f64())
            .unwrap_or(f64::MAX)
    };
    report.shots.sort_by(|a, b| {
        order(a)
            .partial_cmp(&order(b))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });

    lint_opening_view(manifest, &world, out);
    lint_line(&cameras, manifest, out);
    lint_duplicates(&world, out);
    lint_ground(&world, out);
    lint_scale(&world, out);
    for placed in &world {
        out.extend(carried_not_run(placed.entity));
    }
    report
}

/// [`lint`] for a world held as JSON: a manifest as `manifest.json` and the
/// open app's API give it.
pub fn lint_json(world: &Value) -> Result<Report, String> {
    let manifest: wt::WorldManifest = serde_json::from_value(world.clone())
        .map_err(|e| format!("that is not a world manifest: {e}"))?;
    Ok(lint(&manifest))
}

fn lint_camera(
    camera: &Camera<'_>,
    world: &[Placed<'_>],
    manifest: &wt::WorldManifest,
    out: &mut Vec<Diagnostic>,
) {
    let name = camera.name();
    let eye = camera.lens.position;

    for solid in world.iter().filter(|p| p.solid() && holds(p, eye)) {
        out.push(Diagnostic::new(
            CAMERA_INSIDE,
            &[name, solid.name()],
            format!("{name} is inside {}", solid.name()),
            format!(
                "move {name} out of {}, or make room for it: the lens sees the inside of the shape",
                solid.name()
            ),
        ));
    }

    if !world
        .iter()
        .any(|p| p.entity.transform.visible && camera.lens.frames(p).is_some())
        && world.iter().any(|p| p.half.is_some())
    {
        out.push(Diagnostic::new(
            EMPTY_FRAME,
            &[name],
            format!("{name} has no shape in its frame"),
            format!("aim {name} at something (`aim` or `aim_entity`), or move it"),
        ));
    }

    // What the camera is for: the entity it aims at, and the one it
    // focuses on. A reference to nothing falls back, quietly, to `aim`.
    let targets = [
        ("aim_entity", &camera.component.aim_entity, "aims at"),
        ("focus_entity", &camera.component.focus_entity, "focuses on"),
    ];
    let mut hidden: Vec<&str> = Vec::new();
    for (field, reference, verb) in targets {
        let Some(reference) = reference else {
            continue;
        };
        let Some(target) = cinema::entity_of(reference, manifest) else {
            let said = match reference {
                wt::EntityRef::Id(id) => format!("entity {}", id.0),
                wt::EntityRef::Name(name) => format!("\"{name}\""),
            };
            out.push(Diagnostic::new(
                DANGLING_TARGET,
                &[name],
                format!("{name} {verb} {said}, which is not in the world"),
                format!("point `{field}` at an entity that exists, or drop it; until then the fallback applies"),
            ));
            continue;
        };
        let target_name = target.name.as_str();
        let point = cinema::world_origin(target, manifest);
        if field == "focus_entity" {
            let p = camera.lens.local(point);
            let [x, y] = camera.lens.onto_frame(p);
            if p[2] <= NEAR || x.abs() > 1.0 || y.abs() > 1.0 {
                out.push(Diagnostic::new(
                    FOCUS_OUT_OF_FRAME,
                    &[name, target_name],
                    format!("{name} focuses on {target_name}, which is outside its frame"),
                    format!("aim {name} at {target_name}, or focus on something it sees"),
                ));
            }
        }
        if hidden.contains(&target_name) {
            continue;
        }
        let blocker = world
            .iter()
            .filter(|p| p.solid() && figure(p.name()) != figure(target_name))
            // Inside it, the camera has its own finding; around the
            // target, it is the target's own clothing.
            .filter(|p| !holds(p, eye) && !holds(p, point))
            .filter_map(|p| enters(p, eye, point).map(|t| (t, p)))
            .filter(|(t, _)| *t > 1e-6 && *t < 1.0 - 1e-6)
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((t, blocker)) = blocker {
            hidden.push(target_name);
            let distance = length(sub(point, eye));
            out.push(Diagnostic::new(
                SUBJECT_HIDDEN,
                &[name, blocker.name(), target_name],
                format!(
                    "{} is between {name} and {target_name}, {} from the lens",
                    blocker.name(),
                    metres(t * distance)
                ),
                format!(
                    "move {name} or {} until the line from the lens to {target_name} is clear",
                    blocker.name()
                ),
            ));
        }
    }

    if let Some(shot) = camera.shot
        && let (Some(start), Some(end)) = (shot["in_s"].as_f64(), shot["out_s"].as_f64())
        && end <= start
    {
        out.push(Diagnostic::new(
            SHOT_TIMING,
            &[name],
            format!("{name} runs from {start} s to {end} s: it has no length"),
            "set `shot.out_s` after `shot.in_s`",
        ));
    }
}

fn lint_opening_view(
    manifest: &wt::WorldManifest,
    world: &[Placed<'_>],
    out: &mut Vec<Diagnostic>,
) {
    let Some(lens) = manifest.camera.as_ref().and_then(Lens::opening) else {
        return;
    };
    for solid in world
        .iter()
        .filter(|p| p.solid() && holds(p, lens.position))
    {
        out.push(Diagnostic::new(
            CAMERA_INSIDE,
            &[solid.name()],
            format!("the opening view is inside {}", solid.name()),
            "move the world's camera with SetCamera, to where a visitor should start",
        ));
    }
    if world.iter().any(|p| p.half.is_some())
        && !world
            .iter()
            .any(|p| p.entity.transform.visible && lens.frames(p).is_some())
    {
        out.push(Diagnostic::new(
            EMPTY_FRAME,
            &[],
            "the opening view has no shape in its frame",
            "point the world's camera at the world with SetCamera (`position`, `look_at`)",
        ));
    }
}

/// A shot and its reverse: each aims at its own subject from the other
/// subject's side. Both must stand on one side of the line between the two,
/// or a cut between them flips which way each looks.
fn lint_line(cameras: &[Camera<'_>], manifest: &wt::WorldManifest, out: &mut Vec<Diagnostic>) {
    let subject = |camera: &Camera<'_>| {
        let entity = cinema::entity_of(camera.component.aim_entity.as_ref()?, manifest)?;
        Some((
            entity.name.as_str().to_string(),
            cinema::world_origin(entity, manifest),
        ))
    };
    let shots: Vec<(&Camera<'_>, String, V3)> = cameras
        .iter()
        .filter(|camera| camera.shot.is_some())
        .filter_map(|camera| subject(camera).map(|(name, at)| (camera, name, at)))
        .collect();
    for (i, (first, a_name, a)) in shots.iter().enumerate() {
        for (second, b_name, b) in &shots[i + 1..] {
            if first.scene() != second.scene() || figure(a_name) == figure(b_name) {
                continue;
            }
            // Seen from above: the line is drawn on the ground.
            let flat = |p: V3| [p[0], 0.0, p[2]];
            let (a, b) = (flat(*a), flat(*b));
            let (p, q) = (flat(first.lens.position), flat(second.lens.position));
            let reverse =
                length(sub(p, b)) < length(sub(p, a)) && length(sub(q, a)) < length(sub(q, b));
            if !reverse {
                continue;
            }
            let line = sub(b, a);
            let side = |p: V3| cross(line, sub(p, a))[1];
            let (first_side, second_side) = (side(p), side(q));
            // On the line itself is neither side.
            if first_side * second_side < -1e-9 {
                out.push(Diagnostic::new(
                    LINE_CROSSED,
                    &[first.name(), second.name()],
                    format!(
                        "{} and {} are on opposite sides of the line between {a_name} and {b_name}",
                        first.name(),
                        second.name()
                    ),
                    format!(
                        "put both on one side of that line (mirror {} across it), or a cut between them flips which way each looks",
                        second.name()
                    ),
                ));
            }
        }
    }
}

/// Two boxes' shared volume over the volume they span together.
fn overlap(a: &[V3; 2], b: &[V3; 2]) -> f64 {
    let mut shared = 1.0;
    let (mut va, mut vb) = (1.0, 1.0);
    for axis in 0..3 {
        // A plane has no thickness; a millimetre keeps its volume from being
        // zero without changing what overlaps what.
        let pad = 0.0005;
        let (a0, a1) = (a[0][axis] - pad, a[1][axis] + pad);
        let (b0, b1) = (b[0][axis] - pad, b[1][axis] + pad);
        shared *= (a1.min(b1) - a0.max(b0)).max(0.0);
        va *= a1 - a0;
        vb *= b1 - b0;
    }
    shared / (va + vb - shared)
}

fn lint_duplicates(world: &[Placed<'_>], out: &mut Vec<Diagnostic>) {
    /// Boxes this alike are one thing spawned twice.
    const SAME: f64 = 0.9;
    let shaped: Vec<&Placed<'_>> = world.iter().filter(|p| p.bounds.is_some()).collect();
    for (i, a) in shaped.iter().enumerate() {
        for b in &shaped[i + 1..] {
            let (Some(sa), Some(sb)) = (&a.entity.shape, &b.entity.shape) else {
                continue;
            };
            if sa.kind() != sb.kind() {
                continue;
            }
            let (Some(ba), Some(bb)) = (&a.bounds, &b.bounds) else {
                continue;
            };
            if overlap(ba, bb) >= SAME {
                out.push(Diagnostic::new(
                    DUPLICATE,
                    &[a.name(), b.name()],
                    format!(
                        "{} and {} are the same {} in the same place",
                        a.name(),
                        b.name(),
                        sa.kind()
                    ),
                    format!("delete one, or move {}", b.name()),
                ));
            }
        }
    }
}

/// A position is a shape's centre, so a figure placed at the height of its
/// feet stands waist-deep in the floor. The ground is anything flat and
/// wide, and what counts is a shape whose *centre* is at its top face: the
/// mark of a position written as where the thing stands. A rock half in the
/// sea, a post through a counter and a tower sunk a metre for effect are
/// cut by a top face too, anywhere but there, and are as meant.
fn lint_ground(world: &[Placed<'_>], out: &mut Vec<Diagnostic>) {
    /// No thicker than this, and at least this much floor, in metres.
    const FLAT: f64 = 0.5;
    const WIDE: f64 = 4.0;
    /// How near the top face a centre is to be on it: 2 cm, or 2% of the
    /// shape's height when that is more.
    const ON: f64 = 0.02;
    let grounds: Vec<(&Placed<'_>, [V3; 2])> = world
        .iter()
        .filter(|p| p.entity.transform.visible)
        .filter_map(|p| Some((p, p.bounds?)))
        .filter(|(_, [min, max])| {
            max[1] - min[1] <= FLAT && (max[0] - min[0]) * (max[2] - min[2]) >= WIDE
        })
        .collect();
    for placed in world.iter().filter(|p| p.entity.transform.visible) {
        let Some([min, max]) = placed.bounds else {
            continue;
        };
        let height = max[1] - min[1];
        if height <= FLAT {
            continue;
        }
        let centre = [(min[0] + max[0]) / 2.0, (min[2] + max[2]) / 2.0];
        let under = grounds.iter().find(|(ground, [gmin, gmax])| {
            !std::ptr::eq(*ground, placed)
                && (gmin[0]..=gmax[0]).contains(&centre[0])
                && (gmin[2]..=gmax[2]).contains(&centre[1])
                && ((min[1] + max[1]) / 2.0 - gmax[1]).abs() <= ON.max(ON * height)
        });
        if let Some((ground, [_, gmax])) = under {
            let rise = gmax[1] - min[1];
            let y = f64::from(placed.entity.transform.position[1]) + rise;
            out.push(Diagnostic::new(
                SUNK,
                &[placed.name(), ground.name()],
                format!(
                    "{} is {} into {}",
                    placed.name(),
                    metres(rise),
                    ground.name()
                ),
                if placed.entity.parent.is_none() {
                    format!(
                        "a position is the shape's centre: y {} stands it on {}",
                        round(y, 3),
                        ground.name()
                    )
                } else {
                    format!(
                        "a position is the shape's centre: raise it by {}",
                        metres(rise)
                    )
                },
            ));
        }
    }
}

fn lint_scale(world: &[Placed<'_>], out: &mut Vec<Diagnostic>) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for [lo, hi] in world.iter().filter_map(|p| p.bounds) {
        for axis in 0..3 {
            min[axis] = min[axis].min(lo[axis]);
            max[axis] = max[axis].max(hi[axis]);
        }
    }
    let size = length(sub(max, min));
    if size.is_finite() && size > 0.0 && size < 0.05 {
        out.push(Diagnostic::new(
            TINY_WORLD,
            &[],
            format!("the whole world is {:.1} mm across", size * 1000.0),
            "the format is in metres: a person is about 1.7, a room a few across",
        ));
    }
}

/// What `entity` holds that the format carries and this app does not run:
/// a host action it does not define, an `animate` it cannot perform, an
/// extension it has no code for. The world keeps every one of them and
/// another app may run them; here nothing happens, and silence is how that
/// gets mistaken for a bug.
///
/// The authority puts these on the receipt of the batch that brought them
/// ([`crate::live::LiveWorld::submit`]), and [`lint`] lists them all.
pub fn carried_not_run(entity: &wt::WorldEntity) -> Vec<Diagnostic> {
    let name = entity.name.as_str();
    let mut out = Vec::new();
    for trigger in &entity.triggers {
        match &trigger.action {
            wt::TriggerActionDef::Host { name: action, .. }
                if !host_action::ALL.contains(&action.as_str()) =>
            {
                out.push(Diagnostic::new(
                    CARRIED_NOT_RUN,
                    &[name],
                    format!("{name} has a trigger whose host action \"{action}\" this app does not have: it is kept, and skipped when it fires"),
                    format!("this app's host actions are {}", host_action::ALL.join(", ")),
                ));
            }
            wt::TriggerActionDef::Animate { property, to, .. } => {
                let known = ANIMATED.contains(&property.as_str());
                // One number scales uniformly; everything else is x, y, z.
                let fits = to.len() >= 3 || (property == "scale" && to.len() == 1);
                if !known {
                    out.push(Diagnostic::new(
                        CARRIED_NOT_RUN,
                        &[name],
                        format!("{name} has a trigger that animates \"{property}\", which is not a property this app moves: nothing happens when it fires"),
                        "`animate` moves `position`, `rotation` or `scale`",
                    ));
                } else if !fits {
                    out.push(Diagnostic::new(
                        CARRIED_NOT_RUN,
                        &[name],
                        format!("{name} has a trigger that animates {property} to {} number{}: nothing happens when it fires", to.len(), if to.len() == 1 { "" } else { "s" }),
                        format!("`to` is [x, y, z] for {property}"),
                    ));
                }
            }
            _ => {}
        }
    }
    for key in entity.extra.keys().filter(|key| key.starts_with("ext-")) {
        if EXTENSIONS_RUN.contains(&key.as_str()) || EXTENSIONS_INERT.contains(&key.as_str()) {
            continue;
        }
        let registered = openworldformat::strict::REGISTERED_EXTENSIONS;
        if registered.contains(&key.as_str()) {
            out.push(Diagnostic::new(
                CARRIED_NOT_RUN,
                &[name],
                format!("{name} carries `{key}`, an extension this app does not run: it is kept as written"),
                format!("this app runs {}; another reader may run `{key}`", EXTENSIONS_RUN.join(", ")),
            ));
            continue;
        }
        // An authority takes any `ext-*` key on an entity, so a misspelt
        // one commits, and the camera it was meant to make is not one.
        let meant = nearest(key, registered);
        out.push(Diagnostic::new(
            CARRIED_NOT_RUN,
            &[name],
            match meant {
                Some(meant) => format!("{name} carries `{key}`, which is not a registered extension — did you mean `{meant}`? It is kept as written, and nothing reads it"),
                None => format!("{name} carries `{key}`, which is not a registered extension: it is kept as written, and nothing reads it"),
            },
            format!("the format registers {}", registered.join(", ")),
        ));
    }
    out
}

/// The candidate a misspelling most likely meant: the nearest by edits,
/// when it is near enough to be a slip and not another word.
pub fn nearest<'a>(word: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let word: Vec<char> = word.chars().collect();
    candidates
        .iter()
        .map(|candidate| {
            (
                edits(&word, &candidate.chars().collect::<Vec<_>>()),
                *candidate,
            )
        })
        // A third of the word wrong is another word.
        .filter(|(distance, _)| *distance > 0 && *distance <= (word.len() / 3).max(1))
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, candidate)| candidate)
}

/// How many single-character edits turn `a` into `b` (Levenshtein).
fn edits(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != cb))
                .min(above + 1)
                .min(row[j] + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(id: u64, name: &str, shape: wt::Shape, at: [f32; 3]) -> wt::WorldEntity {
        let mut e = wt::WorldEntity::new(id, name).with_shape(shape);
        e.transform.position = at;
        e
    }

    fn cuboid(id: u64, name: &str, size: [f32; 3], at: [f32; 3]) -> wt::WorldEntity {
        entity(
            id,
            name,
            wt::Shape::Cuboid {
                x: size[0],
                y: size[1],
                z: size[2],
            },
            at,
        )
    }

    fn camera(
        id: u64,
        name: &str,
        at: [f32; 3],
        camera: Value,
        shot: Option<Value>,
    ) -> wt::WorldEntity {
        let mut e = wt::WorldEntity::new(id, name);
        e.transform.position = at;
        let mut ext = json!({ "camera": camera });
        if let Some(shot) = shot {
            ext["shot"] = shot;
        }
        e.extra.insert(cinema::EXTENSION_NAME.to_string(), ext);
        e
    }

    fn world(entities: Vec<wt::WorldEntity>) -> wt::WorldManifest {
        let mut manifest = wt::WorldManifest::new("test");
        manifest.next_entity_id = entities.iter().map(|e| e.id.0).max().unwrap_or(0) + 1;
        manifest.entities = entities;
        manifest
    }

    fn codes(report: &Report) -> Vec<&'static str> {
        report.diagnostics.iter().map(|d| d.code).collect()
    }

    /// A room, two figures facing each other, a master and two singles on
    /// one side of the line: the staging a draft makes.
    fn two_hander() -> Vec<wt::WorldEntity> {
        let figure = |id: u64, name: &str, x: f32| {
            vec![
                entity(
                    id,
                    name,
                    wt::Shape::Capsule {
                        radius: 0.18,
                        half_length: 0.67,
                    },
                    [x, 0.85, 0.0],
                ),
                entity(
                    id + 1,
                    &format!("{name}.head"),
                    wt::Shape::Sphere { radius: 0.12 },
                    [x, 1.6, 0.0],
                ),
            ]
        };
        let mut entities = vec![
            cuboid(1, "floor", [8.0, 0.1, 6.0], [0.0, -0.05, 0.0]),
            cuboid(2, "wall-back", [8.0, 3.0, 0.1], [0.0, 1.5, -3.05]),
        ];
        entities.extend(figure(10, "MAYA", -0.7));
        entities.extend(figure(20, "KAI", 0.7));
        entities.push(camera(
            30,
            "1A",
            [0.0, 1.6, 5.75],
            json!({"focal_length_mm": 24, "aspect_ratio": 2.39, "aim": [0, 1.2, 0]}),
            Some(json!({"scene": "1", "order": 1, "in_s": 0, "out_s": 10, "size": "WS"})),
        ));
        entities.push(camera(
            31,
            "1B",
            [1.5, 1.6, 0.6],
            json!({"focal_length_mm": 50, "aspect_ratio": 2.39, "aim_entity": 11, "focus_entity": 11}),
            Some(json!({"scene": "1", "order": 2, "in_s": 1, "out_s": 5, "size": "MCU"})),
        ));
        entities.push(camera(
            32,
            "1C",
            [-1.5, 1.6, 0.6],
            json!({"focal_length_mm": 50, "aspect_ratio": 2.39, "aim_entity": 21, "focus_entity": 21}),
            Some(json!({"scene": "1", "order": 3, "in_s": 5, "out_s": 10, "size": "MCU"})),
        ));
        entities
    }

    fn moved(mut entities: Vec<wt::WorldEntity>, name: &str, to: [f32; 3]) -> Vec<wt::WorldEntity> {
        let entity = entities
            .iter_mut()
            .find(|e| e.name.as_str() == name)
            .expect(name);
        entity.transform.position = to;
        entities
    }

    #[test]
    fn a_staged_scene_has_nothing_to_flag_and_says_what_each_shot_sees() {
        let report = lint(&world(two_hander()));
        assert_eq!(report.diagnostics, vec![], "{:#?}", report.diagnostics);
        let names: Vec<&str> = report.shots.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["1A", "1B", "1C"], "in the shot list's order");

        // The master holds both figures, small; the single holds its
        // subject's head at the centre of the frame.
        let master = &report.shots[0];
        let seen = |shot: &ShotReport, name: &str| {
            shot.sees
                .iter()
                .find(|f| f.name == name)
                .cloned()
                .unwrap_or_else(|| panic!("{} sees no {name}: {:?}", shot.name, shot.sees))
        };
        assert!(seen(master, "MAYA").centre[0] < 0.0 && seen(master, "KAI").centre[0] > 0.0);
        assert!(seen(master, "MAYA").height < 0.9, "a wide holds her whole");
        let single = &report.shots[1];
        assert_eq!(single.aims_at.as_deref(), Some("MAYA.head"));
        let head = seen(single, "MAYA.head");
        assert!(
            head.centre[0].abs() < 0.03 && head.centre[1].abs() < 0.03,
            "{head:?}"
        );
        assert!(
            seen(single, "MAYA.head").height > seen(master, "MAYA.head").height * 3.0,
            "a 50 on a single holds the head larger than a 24 on the master"
        );
        assert!((single.aspect - 2.39).abs() < 1e-6);
        assert!(
            report.lines().last().unwrap().starts_with("ok:"),
            "{:?}",
            report.lines()
        );
    }

    #[test]
    fn a_camera_in_a_wall_is_inside_it_and_one_facing_away_sees_nothing() {
        let report = lint(&world(moved(two_hander(), "1A", [0.0, 1.6, -3.05])));
        assert!(
            codes(&report).contains(&CAMERA_INSIDE),
            "{:?}",
            report.diagnostics
        );
        let found = report
            .diagnostics
            .iter()
            .find(|d| d.code == CAMERA_INSIDE)
            .unwrap();
        assert_eq!(found.entities, ["1A", "wall-back"]);
        assert_eq!(
            found.line(),
            "warning[camera-inside]: 1A is inside wall-back"
        );

        // Past the wall and aimed further out: the room is behind the lens.
        let mut entities = two_hander();
        let master = entities
            .iter_mut()
            .find(|e| e.name.as_str() == "1A")
            .unwrap();
        master.transform.position = [0.0, 1.6, -6.0];
        master.extra.insert(
            cinema::EXTENSION_NAME.into(),
            json!({"camera": {"aim": [0, 1.6, -20]}}),
        );
        let report = lint(&world(entities));
        assert_eq!(codes(&report), [EMPTY_FRAME], "{:?}", report.diagnostics);
    }

    #[test]
    fn a_figure_in_the_way_hides_the_subject_and_its_own_body_does_not() {
        // The staged single looks past KAI's shoulder: nothing is in the way.
        assert!(!codes(&lint(&world(two_hander()))).contains(&SUBJECT_HIDDEN));
        // Straight behind KAI, his body stands on the line to her head.
        let report = lint(&world(moved(two_hander(), "1B", [1.5, 1.6, 0.0])));
        let hidden: Vec<&Diagnostic> = report
            .diagnostics
            .iter()
            .filter(|d| d.code == SUBJECT_HIDDEN)
            .collect();
        assert_eq!(
            hidden.len(),
            1,
            "once, though it aims and focuses there: {hidden:?}"
        );
        assert_eq!(hidden[0].entities[0], "1B");
        assert_eq!(figure(&hidden[0].entities[1]), "KAI");
        assert_eq!(hidden[0].entities[2], "MAYA.head");
        // A pane of glass in the way hides nothing.
        let mut entities = two_hander();
        let mut glass = cuboid(40, "glass", [0.05, 3.0, 3.0], [0.2, 1.5, 0.0]);
        glass.material = Some(wt::MaterialDef {
            color: [0.8, 0.9, 1.0, 0.2],
            ..Default::default()
        });
        entities.push(glass);
        assert!(!codes(&lint(&world(entities))).contains(&SUBJECT_HIDDEN));
    }

    #[test]
    fn a_reverse_on_the_far_side_of_the_line_crosses_it() {
        // 1B over KAI's other shoulder: the far side of the line from 1C.
        let report = lint(&world(moved(two_hander(), "1B", [1.5, 1.6, -0.6])));
        let crossed: Vec<&Diagnostic> = report
            .diagnostics
            .iter()
            .filter(|d| d.code == LINE_CROSSED)
            .collect();
        assert_eq!(crossed.len(), 1, "{:?}", report.diagnostics);
        assert_eq!(crossed[0].entities, ["1B", "1C"]);
        assert!(
            crossed[0].message.contains("MAYA.head") && crossed[0].message.contains("KAI.head")
        );

        // Two cameras on one subject are not a shot and its reverse, and
        // neither are shots of different scenes.
        let mut entities = moved(two_hander(), "1B", [1.5, 1.6, -0.6]);
        let second = entities
            .iter_mut()
            .find(|e| e.name.as_str() == "1C")
            .unwrap();
        second.extra.insert(
            cinema::EXTENSION_NAME.into(),
            json!({"camera": {"aim_entity": 21}, "shot": {"scene": "2", "order": 3}}),
        );
        assert!(!codes(&lint(&world(entities))).contains(&LINE_CROSSED));
    }

    #[test]
    fn a_target_that_is_gone_and_a_focus_out_of_frame_are_said() {
        let mut entities = two_hander();
        let single = entities
            .iter_mut()
            .find(|e| e.name.as_str() == "1B")
            .unwrap();
        single.extra.insert(
            cinema::EXTENSION_NAME.into(),
            json!({"camera": {"aim": [-0.7, 1.6, 0], "aim_entity": 99, "focus_entity": 2},
                   "shot": {"scene": "1", "order": 2, "in_s": 4, "out_s": 4}}),
        );
        let report = lint(&world(entities));
        let of = |code: &str| {
            report
                .diagnostics
                .iter()
                .find(|d| d.code == code)
                .unwrap_or_else(|| panic!("no {code} in {:?}", report.diagnostics))
        };
        assert!(of(DANGLING_TARGET).message.contains("entity 99"));
        // wall-back's centre is far off the frame of a 35 mm aimed at MAYA.
        assert_eq!(of(FOCUS_OUT_OF_FRAME).entities, ["1B", "wall-back"]);
        assert!(of(SHOT_TIMING).message.contains("no length"));
    }

    #[test]
    fn a_figure_placed_at_its_feet_is_sunk_and_the_hint_is_the_height_that_stands_it() {
        let report = lint(&world(moved(two_hander(), "MAYA", [-0.7, 0.0, 0.0])));
        let sunk = report
            .diagnostics
            .iter()
            .find(|d| d.code == SUNK)
            .expect("sunk");
        assert_eq!(sunk.severity, Severity::Note);
        assert_eq!(sunk.entities, ["MAYA", "floor"]);
        assert_eq!(sunk.message, "MAYA is 0.85 m into floor");
        assert!(
            sunk.hint.contains("y 0.85 stands it on floor"),
            "{}",
            sunk.hint
        );
        // A wall resting on the floor, a post set a little into it, a rock
        // a third of the way down and a tower sunk a metre for effect are
        // as meant: cut by the floor's top, with their centres elsewhere.
        let mut entities = two_hander();
        entities.push(cuboid(50, "post", [0.2, 2.0, 0.2], [3.0, 0.9, 2.0]));
        entities.push(entity(
            51,
            "rock",
            wt::Shape::Sphere { radius: 0.6 },
            [-3.0, 0.2, 2.0],
        ));
        entities.push(cuboid(52, "tower", [1.0, 12.0, 1.0], [3.0, 5.0, -2.0]));
        assert!(!codes(&lint(&world(entities))).contains(&SUNK));
    }

    #[test]
    fn the_same_shape_twice_in_one_place_is_a_duplicate() {
        let mut entities = two_hander();
        entities.push(cuboid(60, "crate", [1.0, 1.0, 1.0], [3.0, 0.5, 2.0]));
        entities.push(cuboid(61, "crate-2", [1.0, 1.0, 1.0], [3.0, 0.5, 2.01]));
        // A sphere in the same box is another thing, and so is a crate beside it.
        entities.push(entity(
            62,
            "ball",
            wt::Shape::Sphere { radius: 0.5 },
            [3.0, 0.5, 2.0],
        ));
        entities.push(cuboid(63, "crate-3", [1.0, 1.0, 1.0], [-3.0, 0.5, 2.0]));
        let report = lint(&world(entities));
        let duplicates: Vec<&Diagnostic> = report
            .diagnostics
            .iter()
            .filter(|d| d.code == DUPLICATE)
            .collect();
        assert_eq!(duplicates.len(), 1, "{:?}", report.diagnostics);
        assert_eq!(duplicates[0].entities, ["crate", "crate-2"]);
    }

    #[test]
    fn what_the_app_keeps_and_does_not_run_is_noted() {
        let mut lamp = cuboid(1, "lamp", [0.2, 1.0, 0.2], [0.0, 0.5, 0.0]);
        let click = wt::TriggerEvent::Click {
            max_distance: 3.0,
            prompt: None,
        };
        let trigger = |action| wt::TriggerDef {
            on: click.clone(),
            action,
            once: false,
            cooldown: None,
            requires_item: None,
        };
        lamp.triggers = vec![
            trigger(wt::TriggerActionDef::Host {
                name: "open_gate".into(),
                args: Default::default(),
            }),
            trigger(wt::TriggerActionDef::Host {
                name: host_action::ADD_SCORE.into(),
                args: Default::default(),
            }),
            // Gen's own tool writes this one, and nothing runs it.
            trigger(wt::TriggerActionDef::Host {
                name: host_action::SPAWN.into(),
                args: Default::default(),
            }),
            trigger(wt::TriggerActionDef::Animate {
                property: "colour".into(),
                to: vec![1.0, 0.0, 0.0],
                duration: 1.0,
            }),
            trigger(wt::TriggerActionDef::Animate {
                property: "position".into(),
                to: vec![2.0],
                duration: 1.0,
            }),
            trigger(wt::TriggerActionDef::Animate {
                property: "scale".into(),
                to: vec![2.0],
                duration: 1.0,
            }),
        ];
        lamp.extra
            .insert("ext-physics".into(), json!({"body": "dynamic"}));
        lamp.extra.insert("ext-provenance".into(), json!({}));
        lamp.extra
            .insert(cinema::EXTENSION_NAME.into(), json!({"camera": {}}));
        let carried = carried_not_run(&lamp);
        let said: Vec<&str> = carried.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(carried.len(), 5, "{said:#?}");

        // A misspelt extension commits like any `ext-*` key: the note is
        // what says the camera it was meant to make is not one.
        let mut shot = wt::WorldEntity::new(2, "1A");
        shot.extra
            .insert("ext-cinematograpy".into(), json!({"camera": {}}));
        shot.extra.insert("ext-mood".into(), json!({}));
        let slips = carried_not_run(&shot);
        assert!(
            slips[0]
                .message
                .contains("not a registered extension — did you mean `ext-cinematography`?"),
            "{}",
            slips[0].message
        );
        assert!(
            slips[1]
                .message
                .contains("`ext-mood`, which is not a registered extension:"),
            "nothing is near enough to guess: {}",
            slips[1].message
        );
        assert_eq!(
            nearest("rotaton", &["position", "rotation", "scale"]),
            Some("rotation")
        );
        assert_eq!(
            nearest("rotation", &["rotation"]),
            None,
            "the word itself is no slip"
        );
        assert_eq!(nearest("colour", &["position", "scale"]), None);
        assert!(said[0].contains("\"open_gate\""), "{said:?}");
        assert_eq!(
            carried[0].hint,
            "this app's host actions are add_score, play_sound, set_state"
        );
        assert!(said[1].contains("\"spawn\""), "{said:?}");
        assert!(said[2].contains("animates \"colour\""));
        assert!(said[3].contains("position to 1 number"));
        assert!(said[4].contains("`ext-physics`"));
        assert!(
            carried
                .iter()
                .all(|d| d.code == CARRIED_NOT_RUN && d.severity == Severity::Note)
        );
    }

    #[test]
    fn an_empty_world_and_a_world_in_millimetres_are_said() {
        let report = lint(&wt::WorldManifest::new("empty"));
        assert_eq!(codes(&report), [NOTHING_TO_DRAW]);
        assert_eq!(report.warnings(), 0, "a note is not a warning");

        let report = lint(&world(vec![cuboid(
            1,
            "chip",
            [0.01, 0.002, 0.01],
            [0.0; 3],
        )]));
        assert_eq!(codes(&report), [TINY_WORLD]);
        assert!(report.to_json()["ok"] == false);
    }

    #[test]
    fn the_opening_view_is_checked_like_a_camera() {
        let mut manifest = world(two_hander());
        manifest.camera = Some(wt::CameraDef {
            position: [0.0, 1.6, 5.75],
            look_at: [0.0, 1.2, 0.0],
            fov_degrees: 30.0,
        });
        assert_eq!(codes(&lint(&manifest)), Vec::<&str>::new());
        manifest.camera = Some(wt::CameraDef {
            position: [0.0, 1.6, 5.75],
            look_at: [0.0, 1.6, 50.0],
            fov_degrees: 30.0,
        });
        let report = lint(&manifest);
        assert_eq!(codes(&report), [EMPTY_FRAME]);
        assert_eq!(
            report.diagnostics[0].message,
            "the opening view has no shape in its frame"
        );
    }

    #[test]
    fn a_child_is_measured_where_its_parents_put_it() {
        // A turntable turned a quarter and scaled by two, with a box a metre
        // along its x: the box is two metres along the world's −z.
        let mut table = wt::WorldEntity::new(1, "table");
        table.transform.position = [10.0, 0.0, 0.0];
        table.transform.rotation_degrees = [0.0, 90.0, 0.0];
        table.transform.scale = [2.0, 2.0, 2.0];
        let mut cup = cuboid(2, "cup", [0.2, 0.2, 0.2], [1.0, 0.0, 0.0]);
        cup.parent = Some(wt::EntityId(1));
        let manifest = world(vec![table, cup]);
        let placed = place(&manifest);
        let [min, max] = placed[1].bounds.unwrap();
        let centre = scaled(add(min, max), 0.5);
        for (got, want) in centre.iter().zip([10.0, 0.0, -2.0]) {
            assert!((got - want).abs() < 1e-6, "{centre:?}");
        }
        assert!(
            (max[0] - min[0] - 0.4).abs() < 1e-6,
            "scaled with its parent"
        );
        // The way back is the way there, undone.
        let back = placed[1].world.inverse().unwrap();
        let p = back.apply(placed[1].world.apply([0.03, -0.02, 0.07]));
        assert!(length(sub(p, [0.03, -0.02, 0.07])) < 1e-9);
    }

    #[test]
    fn every_rule_has_one_code_and_a_finding_names_a_rule() {
        let mut seen = std::collections::HashSet::new();
        for rule in RULES {
            assert!(seen.insert(rule.code), "{} is listed twice", rule.code);
            assert!(
                rule.code
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '-'),
                "{}",
                rule.code
            );
            assert_eq!(super::rule(rule.code), Some(rule));
        }
        assert_eq!(RULES.len(), 12);
    }
}
