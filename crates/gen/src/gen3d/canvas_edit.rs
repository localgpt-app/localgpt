//! A person editing the live canvas — select, move, turn, scale, delete,
//! duplicate, group, undo — each gesture one batch through the authority
//! the agents use, committed at the entry on screen.
//!
//! The canvas's rule is that the scene changes only through ops, and a
//! gesture bends it in exactly one place: while a drag or a run of key
//! presses is in progress, the entity's `Transform` is moved directly so the
//! person sees it follow, and when the gesture ends its result is sent as one
//! intent. The canvas then applies what the authority committed — the same
//! value — or, if the authority refuses, puts the entity back from the
//! document. The document has the last word either way, and the history gets
//! one entry per gesture, not one per frame or per key press.
//!
//! Committed at the entry on screen, an edit made while the canvas shows an
//! earlier point in the history starts a branch there: forking from the
//! history is seeking back and editing.
//!
//! | Input | Edit |
//! |---|---|
//! | click | select (Esc, or a click on nothing: deselect); Shift- or Cmd/Ctrl-click adds to the selection |
//! | drag | move along the ground; with Alt held, up and down |
//! | drag on nothing, or Shift-drag anywhere | box-select what the box encloses (Shift adds to the selection) |
//! | drag a handle | move along one axis (the arrows), turn about one (the rings), scale (the cube) |
//! | Alt-drag an arrow | scale along the entity's own matching axis |
//! | `L` | the handles along the world's axes, or the entity's own |
//! | Alt + right-drag | orbit the camera around the selection |
//! | arrows, PgUp / PgDn | nudge one grid step, along the axis nearest the view / vertically |
//! | `,` `.` | turn one angle step left / right |
//! | `-` `=` | scale down / up by 10% |
//! | `Y` | snapping on / off — or hold Cmd/Ctrl while dragging for the opposite |
//! | Delete / Backspace | delete (children go with it) |
//! | Cmd/Ctrl + D | duplicate beside it |
//! | Cmd/Ctrl + G | group the selection under a new empty at its centroid |
//! | Cmd/Ctrl + Z | undo your own newest change on the branch on screen; with Alt held, anyone's |
//! | Cmd/Ctrl + Shift + Z | redo your own newest undo |
//!
//! A multi-selection moves, turns, scales, deletes and duplicates together,
//! as one entry. The panels (`crate::inspector`) edit through the same
//! stream: a field, a parent, a bookmark arrives here as an intent and goes
//! out as a batch the way a gesture does.
//!
//! W A S D, Space and Q stay the camera's and the right button looks
//! around, so none of these collide with flying — and Shift is free to be
//! the selection's modifier.

use bevy::camera::primitives::Aabb;
use bevy::ecs::system::SystemParam;
use bevy::math::bounding::Aabb3d;
use bevy::picking::events::{Click, Drag, DragEnd, DragStart, Pointer};
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerButton, PointerId, PointerLocation};
use bevy::prelude::*;
use serde_json::Value;

use super::avatar;
use super::plugin::FlyCam;
use super::registry::{GenEntity, NameRegistry};
use crate::inspector::{InspectorSelection, PanelIntents};
use localgpt_world_types as wt;

/// How long a run of key presses waits for the next one before it commits.
const KEY_GESTURE_IDLE_SECS: f32 = 0.6;
/// One scale step.
const SCALE_STEP: f32 = 1.1;
/// How far a press must travel before it is a selection box, not a click.
const BOX_THRESHOLD_PX: f32 = 3.0;
const SELECTED: Color = Color::srgb(0.35, 0.8, 1.0);
const SELECTED_TOO: Color = Color::srgb(0.25, 0.55, 0.72);
const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];
const AXIS_COLORS: [Color; 3] = [
    Color::srgb(0.95, 0.3, 0.3),
    Color::srgb(0.4, 0.9, 0.4),
    Color::srgb(0.35, 0.5, 1.0),
];
const SCALE_HANDLE: Color = Color::srgb(0.9, 0.9, 0.9);
const HOT: Color = Color::WHITE;
const BOX_FILL: Color = Color::srgba(0.35, 0.7, 1.0, 0.16);
const BOX_LINE: Color = Color::srgb(0.35, 0.7, 1.0);

/// A person's edits in the canvas.
pub struct CanvasEditPlugin;

/// The person-edit systems; the canvas commits after them in the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PersonEdits;

impl Plugin for CanvasEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PersonIntent>()
            .add_message::<PersonPreview>()
            .init_resource::<CanvasGesture>()
            .init_resource::<CanvasSettings>()
            .init_resource::<Handles>()
            .init_resource::<CanvasMarquee>()
            .add_systems(
                Update,
                (
                    keep_selection,
                    panel_intents,
                    // In this order, and only in this order: `pointer_edits`
                    // leaves a press alone when `handle_edits` has taken it
                    // (`handles.pressed`), and a tuple inside a chain is not
                    // chained itself — unordered, Bevy once ran the pointer
                    // system first, and a press on a ring began a box too.
                    (handle_edits, pointer_edits, key_edits)
                        .chain()
                        .run_if(avatar::in_freefly_mode),
                )
                    .chain()
                    .in_set(PersonEdits),
            )
            .add_systems(Update, (draw_selection, draw_handles).after(PersonEdits));
        // The box being drawn: a layer, not a widget (picking ignores it).
        app.add_systems(Startup, spawn_marquee)
            .add_systems(Update, draw_marquee.after(PersonEdits));
    }
}

/// Snapping, for the canvas to apply and the panels to show.
#[derive(Resource)]
pub struct CanvasSettings {
    /// Drags land on the grid and on the angle step; `Y` toggles, and a
    /// Cmd/Ctrl held during a drag means the opposite.
    pub snap: bool,
    /// The grid, and one nudge.
    pub grid_m: f32,
    /// The angle step, and one turn.
    pub angle_degrees: f32,
    /// The handles along the entity's own axes rather than the world's;
    /// `L` toggles.
    pub local_axes: bool,
}

impl Default for CanvasSettings {
    fn default() -> Self {
        Self {
            snap: false,
            grid_m: 0.25,
            angle_degrees: 15.0,
            local_axes: false,
        }
    }
}

/// What a person asked of the world, for the canvas to commit at the entry
/// on screen.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum PersonIntent {
    /// A gesture's result: for each entity it took, where the canvas left
    /// it, and how far the gesture turned and scaled. Turns and scales
    /// travel as deltas because the document holds the angles, and only it
    /// can add to them without changing their form.
    Transform(Vec<Moved>),
    /// Delete entities and their children.
    Delete(Vec<Named>),
    /// Copies beside the originals, under free names.
    Duplicate(Vec<u64>),
    /// Gather entities under a new empty at their centroid.
    Group(Vec<Named>),
    /// Hang entities under another — `None`: at the top — each standing
    /// where it stood, or, with `keep_offset`, carried by its new parent at
    /// the offset it had from the old one.
    Reparent {
        moved: Vec<Named>,
        parent: Option<Named>,
        keep_offset: bool,
    },
    /// A field edit from a panel: a merge patch for each entity it
    /// changes, as `ModifyEntity` takes it — several when several are
    /// selected, one entry all the same — and what to call the change.
    Patch { edits: Vec<Patched>, what: String },
    /// A scene-wide edit from a panel: a `ModifyWorld` patch.
    World { patch: Value, what: String },
    /// Undo on the path to the entry on screen: the person's own newest
    /// change, or anyone's.
    Undo { own: bool },
    /// Take the person's own newest undo back.
    Redo,
}

/// One entity a panel's edit changes: its merge patch, and its name for
/// the history to read.
#[derive(Debug, Clone, PartialEq)]
pub struct Patched {
    pub id: u64,
    pub name: String,
    pub patch: Value,
}

/// A panel's edit while it is held — a drag on a field — shown on the
/// canvas ahead of the document: each entity as its patch would leave it,
/// nothing committed. The [`PersonIntent::Patch`] the drag lets go as
/// commits it; a preview of the values the document holds puts the scene
/// back.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct PersonPreview(pub Vec<Patched>);

/// One entity a gesture took.
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub id: u64,
    pub name: String,
    /// The local position the canvas left it at, when the gesture moved it.
    pub position: Option<[f32; 3]>,
    /// The rotation the gesture applied, in the parent's frame — about the
    /// vertical from the keys, about any world axis from the rings.
    /// Identity when it didn't turn.
    pub turn: Quat,
    /// Factors along the entity's own axes: the same on all three from the
    /// cube or the keys, one of them from an Alt-drag of an arrow.
    pub scale_by: [f32; 3],
}

/// An entity by id, with its name for the history to read.
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub id: u64,
    pub name: String,
}

/// The gesture in progress, if any.
#[derive(Resource, Default)]
pub struct CanvasGesture(Option<Gesture>);

impl CanvasGesture {
    /// A drag is held: the entity is where the pointer has it, not yet
    /// where the document says.
    pub fn dragging(&self) -> bool {
        self.0.as_ref().is_some_and(|g| g.drag.is_some())
    }
}

/// The entities being moved, turned or scaled — the one the gesture is on
/// first, the rest of the selection after — and what has changed so far.
struct Gesture {
    grabbed: Vec<Grabbed>,
    moved: bool,
    turned: bool,
    scale_by: Vec3,
    /// Quiet time before a run of key presses commits; a drag commits on
    /// release instead.
    idle: Timer,
    drag: Option<Grab>,
}

struct Grabbed {
    entity: Entity,
    id: u64,
    name: String,
    /// The world origin when the drag began: the others follow the
    /// primary's displacement from theirs.
    start: Vec3,
    /// The local rotation when the gesture began: what it turned is the
    /// difference at the end.
    start_rotation: Quat,
}

impl Gesture {
    fn new(grabbed: Vec<Grabbed>) -> Self {
        Self {
            grabbed,
            moved: false,
            turned: false,
            scale_by: Vec3::ONE,
            idle: Timer::from_seconds(KEY_GESTURE_IDLE_SECS, TimerMode::Once),
            drag: None,
        }
    }

    fn primary(&self) -> Entity {
        self.grabbed[0].entity
    }

    /// Remember where everything stands as a drag begins.
    fn begin_drag(&mut self, scene: &Scene) {
        for g in &mut self.grabbed {
            if let Ok(global) = scene.globals.get(g.entity) {
                g.start = global.translation();
            }
        }
    }

    /// Put the primary's origin at a world position; the others follow by
    /// the same displacement.
    fn move_to(&mut self, scene: &mut Scene, target: Vec3) {
        let delta = target - self.grabbed[0].start;
        for g in &self.grabbed {
            scene.place(g.entity, g.start + delta);
        }
        self.moved = true;
    }

    /// Nudge everything by a world vector.
    fn nudge(&mut self, scene: &mut Scene, world: Vec3) {
        for g in &self.grabbed {
            let local = scene.local_vector(g.entity, world);
            if let Ok(mut transform) = scene.transforms.get_mut(g.entity) {
                transform.translation += local;
            }
        }
        self.moved = true;
    }

    /// Turn everything by `degrees` about a world `axis` through its own
    /// origin — the axis read in the parent's frame, which is what the
    /// committed angles say — and scale it by `factor` along its own axes.
    fn turn_and_scale(&mut self, scene: &mut Scene, axis: Vec3, degrees: f32, factor: Vec3) {
        for g in &self.grabbed {
            let local_axis = scene.local_vector(g.entity, axis).normalize_or(axis);
            if let Ok(mut transform) = scene.transforms.get_mut(g.entity) {
                transform.rotation =
                    Quat::from_axis_angle(local_axis, degrees.to_radians()) * transform.rotation;
                transform.scale *= factor;
            }
        }
        if degrees != 0.0 {
            self.turned = true;
        }
        self.scale_by *= factor;
    }

    /// The intent this gesture commits: where the canvas left each entity.
    fn finish(self, transforms: &Query<&mut Transform>) -> PersonIntent {
        PersonIntent::Transform(
            self.grabbed
                .iter()
                .map(|g| {
                    let now = transforms.get(g.entity).ok();
                    Moved {
                        id: g.id,
                        name: g.name.clone(),
                        position: if self.moved {
                            now.map(|t| t.translation.to_array())
                        } else {
                            None
                        },
                        turn: if self.turned {
                            now.map_or(Quat::IDENTITY, |t| t.rotation * g.start_rotation.inverse())
                        } else {
                            Quat::IDENTITY
                        },
                        scale_by: self.scale_by.to_array(),
                    }
                })
                .collect(),
        )
    }
}

/// A drag: the plane the entity slides on and where on it it was grabbed —
/// or the handle that was grabbed instead.
struct Grab {
    /// The primary's world position when the drag began; on the plane.
    origin: Vec3,
    normal: Vec3,
    /// From the grabbed point to the entity's origin, so it doesn't jump to
    /// the pointer.
    offset: Vec3,
    vertical: bool,
    handle: Option<HandleGrab>,
}

/// The handles around the selection: three axis arrows, three rings, one
/// about each axis, a cube for uniform scale. An arrow grabbed with Alt
/// held is a stretch: scale along that one axis of the entity's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    Axis(usize),
    Ring(usize),
    Scale,
    Stretch(usize),
}

/// A handle being dragged, and where the drag began.
struct HandleGrab {
    handle: Handle,
    /// The frame's axes at the grab: a turn mid-drag doesn't move the rail.
    axes: [Vec3; 3],
    /// Along the axis: the line parameter under the pointer at the grab.
    start_s: f32,
    /// On a ring: the direction from the origin to the grab.
    start_dir: Vec3,
    /// On the cube: the screen distance from the origin to the grab.
    start_px: f32,
    /// What the ring has applied so far, in degrees, so each frame applies
    /// the rest.
    applied_angle: f32,
    /// What the cube, or a stretch, has applied so far.
    applied_scale: f32,
}

/// A box being drawn over the viewport, to select what it encloses.
#[derive(Resource, Default)]
pub struct CanvasMarquee(Option<Marquee>);

impl CanvasMarquee {
    /// The box on screen now, `[x0, y0, x1, y1]` in window points — for the
    /// live API, so a test driver sees what a person sees.
    pub(crate) fn rect(&self) -> Option<[f32; 4]> {
        self.0
            .as_ref()
            .filter(|m| m.moved)
            .map(|m| [m.from.x, m.from.y, m.to.x, m.to.y])
    }
}

struct Marquee {
    from: Vec2,
    to: Vec2,
    /// Add to the selection rather than replace it.
    extend: bool,
    /// The pointer has moved since the press: a box, not a click.
    moved: bool,
}

/// Where the handles are this frame, and which one is under the pointer.
#[derive(Resource, Default)]
pub struct Handles {
    frame: Option<HandleFrame>,
    hot: Option<Handle>,
    /// The button went down on a handle: the picking events of that press
    /// (a click on what was behind it) are not the world's.
    pressed: bool,
}

impl Handles {
    /// Where the handles are this frame, for whoever has to find them
    /// without eyes — the live API tells a test driver.
    pub(crate) fn frame(&self) -> Option<HandleFrame> {
        self.frame
    }

    /// The handle under the pointer, by name.
    pub(crate) fn hot_name(&self) -> Option<&'static str> {
        self.hot.map(|handle| match handle {
            Handle::Axis(0) | Handle::Stretch(0) => "x",
            Handle::Axis(1) | Handle::Stretch(1) => "y",
            Handle::Axis(_) | Handle::Stretch(_) => "z",
            Handle::Ring(0) => "ring_x",
            Handle::Ring(1) => "ring_y",
            Handle::Ring(_) => "ring_z",
            Handle::Scale => "scale",
        })
    }
}

/// The handles' geometry: sized by the distance to the camera so they take
/// about the same screen space from anywhere, along the world's axes or,
/// turned by the entity's rotation, its own.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HandleFrame {
    pub(crate) origin: Vec3,
    pub(crate) length: f32,
    pub(crate) ring_radius: f32,
    pub(crate) scale_at: Vec3,
    /// The arrows' directions and the rings' normals.
    pub(crate) axes: [Vec3; 3],
    /// What turns the world's axes into `axes`: identity for the world's.
    pub(crate) rotation: Quat,
    /// The axes are the entity's own.
    pub(crate) local: bool,
    /// How near the pointer's ray must pass, in world units.
    threshold: f32,
}

impl HandleFrame {
    fn around(origin: Vec3, camera: Vec3, local: Option<Quat>) -> Self {
        let distance = (camera - origin).length().max(0.5);
        let length = (0.22 * distance).clamp(0.4, 12.0);
        let rotation = local.unwrap_or(Quat::IDENTITY);
        Self {
            origin,
            length,
            // Outside the arrows' tips: a ring inside them crosses every
            // arrow at its own radius, and there the ring took the grab.
            ring_radius: 1.15 * length,
            scale_at: origin + rotation * (Vec3::ONE.normalize() * 0.9 * length),
            axes: AXES.map(|axis| rotation * axis),
            rotation,
            local: local.is_some(),
            threshold: 0.035 * distance,
        }
    }
}

/// The scene as the edit systems need it.
#[derive(SystemParam)]
struct Scene<'w, 's> {
    gen_entities: Query<'w, 's, &'static GenEntity>,
    /// Every world entity, for a box to look through.
    entities: Query<'w, 's, Entity, With<GenEntity>>,
    parents: Query<'w, 's, &'static ChildOf>,
    children: Query<'w, 's, &'static Children>,
    bounds: Query<'w, 's, &'static Aabb>,
    globals: Query<'w, 's, &'static GlobalTransform>,
    transforms: Query<'w, 's, &'static mut Transform>,
    registry: Res<'w, NameRegistry>,
    camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<FlyCam>>,
    windows: Query<'w, 's, (), With<Window>>,
    /// The pointers picking tracks — the mouse's place, in the same
    /// coordinates the picking events carry, whether a hand or a test
    /// driver moved it.
    pointers: Query<'w, 's, (&'static PointerId, &'static PointerLocation)>,
    /// What picking finds under each pointer, so a press on nothing is
    /// known as one without waiting for an event that may never come
    /// (picking sends none when the canvas renders into an image).
    hover: Option<Res<'w, HoverMap>>,
}

impl Scene<'_, '_> {
    /// The world entity a picked mesh belongs to: itself, or its nearest
    /// ancestor that is one (a glTF's meshes are children of its entity).
    fn owner(&self, mut entity: Entity) -> Option<Entity> {
        loop {
            if self.gen_entities.contains(entity) {
                return Some(entity);
            }
            entity = self.parents.get(entity).ok()?.parent();
        }
    }

    fn identify(&self, entity: Entity) -> Option<(u64, String)> {
        let id = self.gen_entities.get(entity).ok()?.world_id.0;
        let name = self.registry.get_name(entity).unwrap_or("entity");
        Some((id, name.to_string()))
    }

    fn named(&self, entity: Entity) -> Option<Named> {
        self.identify(entity).map(|(id, name)| Named { id, name })
    }

    /// Whether anything pickable is under the mouse pointer.
    fn under_pointer(&self) -> bool {
        self.hover
            .as_ref()
            .and_then(|map| map.get(&PointerId::Mouse))
            .is_some_and(|hits| !hits.is_empty())
    }

    /// Where the mouse pointer is in the window, if it is in it.
    fn cursor(&self) -> Option<Vec2> {
        self.pointers
            .iter()
            .find(|(id, _)| **id == PointerId::Mouse)
            .and_then(|(_, location)| location.location().map(|l| l.position))
    }

    /// The world ray under a point of the window.
    fn ray(&self, at: Vec2) -> Option<Ray3d> {
        let (camera, transform) = self.camera.single().ok()?;
        camera.viewport_to_world(transform, at).ok()
    }

    /// Where a world point is in the window.
    fn to_viewport(&self, world: Vec3) -> Option<Vec2> {
        let (camera, transform) = self.camera.single().ok()?;
        camera.world_to_viewport(transform, world).ok()
    }

    /// The camera's horizontal directions, snapped to the nearest world
    /// axis, so a nudge moves along X or Z exactly: (forward, right).
    fn nudge_axes(&self) -> (Vec3, Vec3) {
        let Ok((_, camera)) = self.camera.single() else {
            return (Vec3::NEG_Z, Vec3::X);
        };
        (
            snap_to_axis(camera.forward().as_vec3()),
            snap_to_axis(camera.right().as_vec3()),
        )
    }

    /// A world-space vector in the entity's parent's frame.
    fn local_vector(&self, entity: Entity, world: Vec3) -> Vec3 {
        match self.parent_global(entity) {
            Some(parent) => parent.affine().inverse().transform_vector3(world),
            None => world,
        }
    }

    /// Put an entity's origin at a world position, through its parent's frame.
    fn place(&mut self, entity: Entity, world: Vec3) {
        let local = match self.parent_global(entity) {
            Some(parent) => parent.affine().inverse().transform_point3(world),
            None => world,
        };
        if let Ok(mut transform) = self.transforms.get_mut(entity) {
            transform.translation = local;
        }
    }

    fn parent_global(&self, entity: Entity) -> Option<GlobalTransform> {
        let parent = self.parents.get(entity).ok()?.parent();
        self.globals.get(parent).ok().copied()
    }

    /// The entity's own rotation in the world, for handles along its axes.
    fn world_rotation(&self, entity: Entity) -> Option<Quat> {
        self.globals
            .get(entity)
            .ok()
            .map(|g| g.compute_transform().rotation)
    }

    /// Whether a world entity is under another in the set.
    fn has_ancestor_in(&self, mut entity: Entity, set: &[Entity]) -> bool {
        while let Ok(child_of) = self.parents.get(entity) {
            entity = child_of.parent();
            if set.contains(&entity) {
                return true;
            }
        }
        false
    }

    /// The world entities a box between two window points encloses whole —
    /// every corner of what they draw inside it, or their origin when they
    /// draw nothing — without the ones whose parent is enclosed too, since
    /// those move with it. By id, so the order is the world's.
    fn enclosed_by(&self, a: Vec2, b: Vec2) -> Vec<Entity> {
        let (low, high) = (a.min(b), a.max(b));
        let inside = |p: Vec2| p.cmpge(low).all() && p.cmple(high).all();
        let mut found: Vec<(u64, Entity)> = Vec::new();
        for root in self.entities.iter() {
            let mut corners_seen = false;
            let mut all_inside = true;
            let mut stack = vec![root];
            let mut budget = 4096;
            while let Some(entity) = stack.pop() {
                budget -= 1;
                if budget == 0 {
                    break;
                }
                if let (Ok(aabb), Ok(global)) = (self.bounds.get(entity), self.globals.get(entity))
                {
                    for corner in corners(aabb) {
                        corners_seen = true;
                        match self.to_viewport(global.transform_point(corner)) {
                            Some(p) if inside(p) => {}
                            _ => all_inside = false,
                        }
                    }
                }
                if let Ok(kids) = self.children.get(entity) {
                    stack.extend(kids.iter());
                }
            }
            let enclosed = if corners_seen {
                all_inside
            } else {
                self.globals
                    .get(root)
                    .ok()
                    .and_then(|g| self.to_viewport(g.translation()))
                    .is_some_and(inside)
            };
            if enclosed && let Some((id, _)) = self.identify(root) {
                found.push((id, root));
            }
        }
        found.sort_by_key(|(id, _)| *id);
        let all: Vec<Entity> = found.iter().map(|(_, e)| *e).collect();
        all.iter()
            .copied()
            .filter(|e| !self.has_ancestor_in(*e, &all))
            .collect()
    }
}

/// Select what a box enclosed: in place of the selection, or added to it.
fn select_boxed(selection: &mut InspectorSelection, enclosed: Vec<Entity>, extend: bool) {
    if !extend {
        selection.clear();
    }
    for entity in enclosed {
        if selection.is_empty() {
            selection.set(Some(entity));
        } else if !selection.contains(entity) {
            selection.toggle(entity);
        }
    }
}

/// Commit the gesture in progress, if it is not on `entity`.
fn flush_unless(
    gesture: &mut CanvasGesture,
    entity: Option<Entity>,
    scene: &Scene,
    intents: &mut MessageWriter<PersonIntent>,
) {
    if gesture
        .0
        .as_ref()
        .is_some_and(|g| Some(g.primary()) != entity)
        && let Some(done) = gesture.0.take()
    {
        intents.write(done.finish(&scene.transforms));
    }
}

/// The gesture on `entity` — and on the rest of the selection, when
/// `entity` is in it — committing one on another entity first.
fn gesture_on<'g>(
    gesture: &'g mut CanvasGesture,
    entity: Entity,
    selection: &InspectorSelection,
    scene: &Scene,
    intents: &mut MessageWriter<PersonIntent>,
) -> Option<&'g mut Gesture> {
    flush_unless(gesture, Some(entity), scene, intents);
    if gesture.0.is_none() {
        let mut grabbed = Vec::new();
        let mut take = |entity: Entity| {
            if let Some((id, name)) = scene.identify(entity) {
                let start = scene
                    .globals
                    .get(entity)
                    .map(|g| g.translation())
                    .unwrap_or_default();
                let start_rotation = scene
                    .transforms
                    .get(entity)
                    .map_or(Quat::IDENTITY, |t| t.rotation);
                grabbed.push(Grabbed {
                    entity,
                    id,
                    name,
                    start,
                    start_rotation,
                });
            }
        };
        take(entity);
        if selection.contains(entity) {
            for other in selection.all().filter(|e| *e != entity) {
                take(other);
            }
        }
        if grabbed.is_empty() {
            return None;
        }
        gesture.0 = Some(Gesture::new(grabbed));
    }
    gesture.0.as_mut()
}

/// Keep selecting the same world entities when the scene replaces them
/// under the selection — a seek or an undo can despawn one and spawn it
/// again.
fn keep_selection(
    mut selection: ResMut<InspectorSelection>,
    gen_entities: Query<&GenEntity>,
    registry: Res<NameRegistry>,
    mut last: Local<Vec<(Entity, u64)>>,
) {
    let current: Vec<Entity> = selection.all().collect();
    if current.is_empty() {
        last.clear();
        return;
    }
    let mut next = Vec::with_capacity(current.len());
    let mut remembered = Vec::with_capacity(current.len());
    for entity in &current {
        if let Ok(found) = gen_entities.get(*entity) {
            next.push(*entity);
            remembered.push((*entity, found.world_id.0));
            continue;
        }
        // Gone from the scene: the entity that holds its id now, if any.
        if let Some((_, id)) = last.iter().find(|(e, _)| e == entity)
            && let Some(again) = registry.get_entity_by_id(&wt::EntityId(*id))
        {
            next.push(again);
            remembered.push((again, *id));
        }
    }
    *last = remembered;
    if next != current {
        selection.entity = next.first().copied();
        selection.others = next.get(1..).map(<[Entity]>::to_vec).unwrap_or_default();
    }
}

/// The panels' intents, into the same stream as the gestures. An intent
/// identical to the one before it in the same frame is the same ask sent
/// twice, not a second one.
fn panel_intents(mut panel: ResMut<PanelIntents>, mut intents: MessageWriter<PersonIntent>) {
    if panel.0.is_empty() {
        return;
    }
    let mut last: Option<PersonIntent> = None;
    for intent in panel.0.drain(..) {
        if last.as_ref() == Some(&intent) {
            continue;
        }
        intents.write(intent.clone());
        last = Some(intent);
    }
}

/// Whether a drag snaps right now: the setting, or its opposite while
/// Cmd/Ctrl is held.
fn snapping(settings: &CanvasSettings, keys: &ButtonInput<KeyCode>) -> bool {
    settings.snap
        != keys.any_pressed([
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
        ])
}

/// The handles: hover, grab, and the drag along an axis, around a ring
/// or out from the cube. Runs before the picking-driven edits so a press on
/// a handle is a handle's, not a press on what is behind it.
#[allow(clippy::too_many_arguments)]
fn handle_edits(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<CanvasSettings>,
    mut handles: ResMut<Handles>,
    selection: Res<InspectorSelection>,
    mut gesture: ResMut<CanvasGesture>,
    mut intents: MessageWriter<PersonIntent>,
    mut scene: Scene,
) {
    handles.frame = None;
    let Some(primary) = selection.entity else {
        handles.hot = None;
        return;
    };
    let (Ok(global), Ok((_, camera))) = (scene.globals.get(primary), scene.camera.single()) else {
        return;
    };
    let local = settings
        .local_axes
        .then(|| scene.world_rotation(primary))
        .flatten();
    let frame = HandleFrame::around(global.translation(), camera.translation(), local);
    handles.frame = Some(frame);
    let cursor = scene.cursor();
    let ray = cursor.and_then(|c| scene.ray(c));
    let snap = snapping(&settings, &keys);

    // A handle drag in progress: follow the pointer, or finish on release.
    let on_handle = gesture
        .0
        .as_ref()
        .is_some_and(|g| g.drag.as_ref().is_some_and(|d| d.handle.is_some()));
    if on_handle {
        if !mouse.pressed(MouseButton::Left) {
            if let Some(done) = gesture.0.take() {
                intents.write(done.finish(&scene.transforms));
            }
            return;
        }
        let (Some(ray), Some(g)) = (ray, gesture.0.as_mut()) else {
            return;
        };
        let Some(grab) = g.drag.as_ref() else {
            return;
        };
        let Some(hg) = grab.handle.as_ref() else {
            return;
        };
        let origin = grab.origin;
        let kind = hg.handle;
        let axes = hg.axes;
        let (start_s, start_dir, start_px) = (hg.start_s, hg.start_dir, hg.start_px);
        let (applied_angle, applied_scale) = (hg.applied_angle, hg.applied_scale);
        let mut target = None;
        let mut angle = applied_angle;
        let mut scale = applied_scale;
        // The axis a ring turns about; the keys' vertical otherwise.
        let mut turn_axis = Vec3::Y;
        // The one axis of the entity's own a stretch scales along.
        let mut stretch_axis = None;
        match kind {
            Handle::Axis(i) => {
                let axis = axes[i];
                let s = line_param_nearest_ray(origin, axis, ray);
                let mut along = s - start_s;
                if snap && settings.local_axes {
                    // No grid lines run along a turned axis: grid steps of
                    // travel instead.
                    along = snap_to(along, settings.grid_m);
                }
                let mut t = origin + axis * along;
                if snap && !settings.local_axes {
                    t[i] = snap_to(t[i], settings.grid_m);
                }
                target = Some(t);
            }
            Handle::Stretch(i) => {
                stretch_axis = Some(i);
                let s = line_param_nearest_ray(origin, axes[i], ray);
                scale = (s / start_s.max(0.05)).max(0.05);
                if snap {
                    scale = ((scale * 4.0).round().max(1.0)) / 4.0;
                }
            }
            Handle::Ring(i) => {
                turn_axis = axes[i];
                if let Some(hit) = hit_plane(ray, origin, turn_axis) {
                    let dir = (hit - origin).normalize_or_zero();
                    if dir != Vec3::ZERO {
                        angle = signed_angle_about(turn_axis, start_dir, dir).to_degrees();
                        if snap {
                            angle = snap_to(angle, settings.angle_degrees);
                        }
                    }
                }
            }
            Handle::Scale => {
                if let (Some(c), Some(center)) = (cursor, scene.to_viewport(origin)) {
                    scale = ((c - center).length() / start_px.max(1.0)).max(0.05);
                    if snap {
                        scale = ((scale * 4.0).round().max(1.0)) / 4.0;
                    }
                }
            }
        }
        if let Some(t) = target {
            g.move_to(&mut scene, t);
        }
        if angle != applied_angle || scale != applied_scale {
            let by = scale / applied_scale;
            let factor = match stretch_axis {
                Some(i) => {
                    let mut f = Vec3::ONE;
                    f[i] = by;
                    f
                }
                None => Vec3::splat(by),
            };
            g.turn_and_scale(&mut scene, turn_axis, angle - applied_angle, factor);
        }
        if let Some(hg) = g.drag.as_mut().and_then(|d| d.handle.as_mut()) {
            hg.applied_angle = angle;
            hg.applied_scale = scale;
        }
        return;
    }

    // Hover, and a press that grabs.
    if gesture.dragging() {
        handles.hot = None;
        return;
    }
    let hot = ray.and_then(|ray| hit_handle(&frame, ray));
    handles.hot = hot;
    let (Some(kind), Some(ray)) = (hot, ray) else {
        return;
    };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    // An arrow taken with Alt held stretches instead of moving.
    let kind = match kind {
        Handle::Axis(i) if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) => {
            Handle::Stretch(i)
        }
        other => other,
    };
    let Some(g) = gesture_on(&mut gesture, primary, &selection, &scene, &mut intents) else {
        return;
    };
    g.begin_drag(&scene);
    let origin = frame.origin;
    let mut hg = HandleGrab {
        handle: kind,
        axes: frame.axes,
        start_s: 0.0,
        start_dir: Vec3::X,
        start_px: 1.0,
        applied_angle: 0.0,
        applied_scale: 1.0,
    };
    match kind {
        Handle::Axis(i) | Handle::Stretch(i) => {
            hg.start_s = line_param_nearest_ray(origin, frame.axes[i], ray);
        }
        Handle::Ring(i) => {
            // Some direction in the ring's plane, should the grab miss it.
            let across = frame.axes[(i + 1) % 3];
            hg.start_dir = hit_plane(ray, origin, frame.axes[i])
                .map(|hit| (hit - origin).normalize_or(across))
                .unwrap_or(across);
        }
        Handle::Scale => {
            hg.start_px = cursor
                .zip(scene.to_viewport(origin))
                .map(|(c, center)| (c - center).length())
                .unwrap_or(1.0);
        }
    }
    g.drag = Some(Grab {
        origin,
        normal: Vec3::Y,
        offset: Vec3::ZERO,
        vertical: false,
        handle: Some(hg),
    });
    handles.pressed = true;
}

/// Clicks select; drags move.
#[allow(clippy::too_many_arguments)]
fn pointer_edits(
    mut clicks: MessageReader<Pointer<Click>>,
    mut starts: MessageReader<Pointer<DragStart>>,
    mut drags: MessageReader<Pointer<Drag>>,
    mut ends: MessageReader<Pointer<DragEnd>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    settings: Res<CanvasSettings>,
    mut handles: ResMut<Handles>,
    mut marquee: ResMut<CanvasMarquee>,
    mut selection: ResMut<InspectorSelection>,
    mut gesture: ResMut<CanvasGesture>,
    mut intents: MessageWriter<PersonIntent>,
    mut scene: Scene,
) {
    let left = mouse.pressed(MouseButton::Left);
    // A box being drawn follows the pointer wherever it goes — across a
    // panel too — and ends on release. Its press's picking events are not
    // the world's: a drag of what was under the press, a click on what is
    // under the release. A press that never travelled is a click, below.
    if let Some(m) = marquee.0.as_mut() {
        if let Some(at) = scene.cursor()
            && (at - m.from).length() > BOX_THRESHOLD_PX
        {
            m.to = at;
            m.moved = true;
        }
        if left {
            drain_pointer(&mut clicks, &mut starts, &mut drags, &mut ends);
            return;
        }
        let m = marquee.0.take().expect("checked just above");
        if m.moved {
            let enclosed = scene.enclosed_by(m.from, m.to);
            select_boxed(&mut selection, enclosed, m.extend);
            drain_pointer(&mut clicks, &mut starts, &mut drags, &mut ends);
            return;
        }
    }
    // A press on a handle was the handle's.
    if handles.pressed {
        drain_pointer(&mut clicks, &mut starts, &mut drags, &mut ends);
        if handles.pressed && !left {
            handles.pressed = false;
        }
        return;
    }
    // Shift, or Cmd/Ctrl as the outliner also takes it.
    let extend = keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    let snap = snapping(&settings, &keys);

    // A press on nothing, or with Shift on anything, begins a box. Read
    // from the button and the hover map rather than picking's events, so
    // the sky counts as nothing whether or not a window is behind it.
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if mouse.just_pressed(MouseButton::Left)
        && !gesture.dragging()
        && (shift || !scene.under_pointer())
        && let Some(at) = scene.cursor()
    {
        flush_unless(&mut gesture, None, &scene, &mut intents);
        marquee.0 = Some(Marquee {
            from: at,
            to: at,
            extend,
            moved: false,
        });
    }

    for click in clicks.read() {
        if click.event.button != PointerButton::Primary {
            continue;
        }
        // The release that ends a drag reports a click on whatever is under
        // the pointer too. That click is the drag's, not a choice: taken as
        // one, it collapsed a multi-selection to the entity just moved.
        if gesture
            .0
            .as_ref()
            .is_some_and(|g| g.drag.is_some() && g.moved)
        {
            continue;
        }
        if let Some(owner) = scene.owner(click.entity) {
            flush_unless(&mut gesture, Some(owner), &scene, &mut intents);
            if extend {
                selection.toggle(owner);
            } else if !selection.contains(owner) || selection.len() > 1 {
                selection.set(Some(owner));
            }
        } else if scene.windows.contains(click.entity) {
            // Nothing under the pointer but the window itself.
            flush_unless(&mut gesture, None, &scene, &mut intents);
            if !selection.is_empty() {
                selection.clear();
            }
        }
    }

    for start in starts.read() {
        if start.event.button != PointerButton::Primary || marquee.0.is_some() {
            continue;
        }
        let Some(owner) = scene.owner(start.entity) else {
            continue;
        };
        let (Some(ray), Ok(global)) = (
            scene.ray(start.pointer_location.position),
            scene.globals.get(owner),
        ) else {
            continue;
        };
        let origin = global.translation();
        let vertical = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
        // Along the ground, or up and down on a wall facing the camera.
        let normal = if vertical {
            let toward_camera = -ray.direction.as_vec3();
            Vec3::new(toward_camera.x, 0.0, toward_camera.z).normalize_or(Vec3::Z)
        } else {
            Vec3::Y
        };
        let Some(hit) = hit_plane(ray, origin, normal) else {
            continue;
        };
        if !selection.contains(owner) {
            if extend {
                selection.toggle(owner);
            } else {
                selection.set(Some(owner));
            }
        }
        if let Some(g) = gesture_on(&mut gesture, owner, &selection, &scene, &mut intents) {
            g.begin_drag(&scene);
            g.drag = Some(Grab {
                origin,
                normal,
                offset: origin - hit,
                vertical,
                handle: None,
            });
        }
    }

    for drag in drags.read() {
        if marquee.0.is_some() {
            continue;
        }
        let Some(g) = gesture.0.as_mut() else {
            continue;
        };
        let Some(grab) = &g.drag else {
            continue;
        };
        if grab.handle.is_some() {
            continue;
        }
        let Some(hit) = scene
            .ray(drag.pointer_location.position)
            .and_then(|ray| hit_plane(ray, grab.origin, grab.normal))
        else {
            continue;
        };
        let mut target = hit + grab.offset;
        if grab.vertical {
            target.x = grab.origin.x;
            target.z = grab.origin.z;
            if snap {
                target.y = snap_to(target.y, settings.grid_m);
            }
        } else if snap {
            target.x = snap_to(target.x, settings.grid_m);
            target.z = snap_to(target.z, settings.grid_m);
        }
        g.move_to(&mut scene, target);
    }

    for _ in ends.read() {
        if gesture
            .0
            .as_ref()
            .is_some_and(|g| g.drag.as_ref().is_some_and(|d| d.handle.is_none()))
            && let Some(done) = gesture.0.take()
        {
            intents.write(done.finish(&scene.transforms));
        }
    }
}

/// Forget this frame's picking events: they belong to a panel, a handle or
/// a box, not to the world.
fn drain_pointer(
    clicks: &mut MessageReader<Pointer<Click>>,
    starts: &mut MessageReader<Pointer<DragStart>>,
    drags: &mut MessageReader<Pointer<Drag>>,
    ends: &mut MessageReader<Pointer<DragEnd>>,
) {
    clicks.clear();
    starts.clear();
    drags.clear();
    ends.clear();
}

/// The box being drawn, as a `bevy_ui` node over the scene: a layer the
/// pointer passes through, sized from the marquee each frame. It is the
/// viewport's own feedback, like a handle, so it is drawn in the world's
/// frame rather than in the page.
#[derive(Component)]
struct MarqueeNode;

fn spawn_marquee(mut commands: Commands) {
    commands.spawn((
        MarqueeNode,
        Name::new("Selection box"),
        Node {
            position_type: PositionType::Absolute,
            display: Display::None,
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(BOX_FILL),
        BorderColor::all(BOX_LINE),
        ZIndex(90),
        bevy::picking::Pickable::IGNORE,
    ));
}

fn draw_marquee(marquee: Res<CanvasMarquee>, mut node: Query<&mut Node, With<MarqueeNode>>) {
    let Ok(mut node) = node.single_mut() else {
        return;
    };
    match marquee.rect() {
        Some([x0, y0, x1, y1]) => {
            node.display = Display::Flex;
            node.left = Val::Px(x0.min(x1));
            node.top = Val::Px(y0.min(y1));
            node.width = Val::Px((x1 - x0).abs());
            node.height = Val::Px((y1 - y0).abs());
        }
        None if node.display != Display::None => node.display = Display::None,
        None => {}
    }
}

/// Keys edit the selection; a run of presses is one gesture. Typing into a
/// panel never reaches here: the page holds the keys while it has focus.
fn key_edits(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut settings: ResMut<CanvasSettings>,
    mut selection: ResMut<InspectorSelection>,
    mut gesture: ResMut<CanvasGesture>,
    mut intents: MessageWriter<PersonIntent>,
    mut scene: Scene,
) {
    let command = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    if keys.just_pressed(KeyCode::Escape) {
        flush_unless(&mut gesture, None, &scene, &mut intents);
        if !selection.is_empty() {
            selection.clear();
        }
    }
    if keys.just_pressed(KeyCode::KeyY) && !command {
        settings.snap = !settings.snap;
    }
    if keys.just_pressed(KeyCode::KeyL) && !command {
        settings.local_axes = !settings.local_axes;
    }
    if command && keys.just_pressed(KeyCode::KeyZ) {
        flush_unless(&mut gesture, None, &scene, &mut intents);
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
        intents.write(if shift {
            PersonIntent::Redo
        } else {
            PersonIntent::Undo { own: !alt }
        });
    }
    if let Some(entity) = selection.entity {
        key_edit(
            &keys,
            command,
            entity,
            &settings,
            &mut selection,
            &mut gesture,
            &mut intents,
            &mut scene,
        );
    }

    // A run of presses commits once the keys go quiet. A drag in progress
    // commits on release instead.
    if let Some(g) = gesture.0.as_mut()
        && g.drag.is_none()
        && g.idle.tick(time.delta()).just_finished()
        && let Some(done) = gesture.0.take()
    {
        intents.write(done.finish(&scene.transforms));
    }
}

#[allow(clippy::too_many_arguments)]
fn key_edit(
    keys: &ButtonInput<KeyCode>,
    command: bool,
    entity: Entity,
    settings: &CanvasSettings,
    selection: &mut InspectorSelection,
    gesture: &mut CanvasGesture,
    intents: &mut MessageWriter<PersonIntent>,
    scene: &mut Scene,
) {
    if keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        // The delete supersedes a gesture on the same entity.
        if gesture.0.as_ref().is_some_and(|g| g.primary() == entity) {
            gesture.0 = None;
        }
        flush_unless(gesture, None, scene, intents);
        let named: Vec<Named> = selection.all().filter_map(|e| scene.named(e)).collect();
        if !named.is_empty() {
            intents.write(PersonIntent::Delete(named));
        }
        selection.clear();
        return;
    }
    if command {
        if keys.just_pressed(KeyCode::KeyD) {
            flush_unless(gesture, None, scene, intents);
            let ids: Vec<u64> = selection
                .all()
                .filter_map(|e| scene.identify(e).map(|(id, _)| id))
                .collect();
            if !ids.is_empty() {
                intents.write(PersonIntent::Duplicate(ids));
            }
        }
        if keys.just_pressed(KeyCode::KeyG) {
            flush_unless(gesture, None, scene, intents);
            let named: Vec<Named> = selection.all().filter_map(|e| scene.named(e)).collect();
            if named.len() > 1 {
                intents.write(PersonIntent::Group(named));
            }
        }
        return;
    }

    let (forward, right) = scene.nudge_axes();
    let mut nudge = Vec3::ZERO;
    for (key, step) in [
        (KeyCode::ArrowUp, forward),
        (KeyCode::ArrowDown, -forward),
        (KeyCode::ArrowRight, right),
        (KeyCode::ArrowLeft, -right),
        (KeyCode::PageUp, Vec3::Y),
        (KeyCode::PageDown, Vec3::NEG_Y),
    ] {
        if keys.just_pressed(key) {
            nudge += step * settings.grid_m;
        }
    }
    let turn = if keys.just_pressed(KeyCode::Comma) {
        settings.angle_degrees
    } else if keys.just_pressed(KeyCode::Period) {
        -settings.angle_degrees
    } else {
        0.0
    };
    let scale = if keys.just_pressed(KeyCode::Equal) {
        SCALE_STEP
    } else if keys.just_pressed(KeyCode::Minus) {
        1.0 / SCALE_STEP
    } else {
        1.0
    };
    if nudge == Vec3::ZERO && turn == 0.0 && scale == 1.0 {
        return;
    }

    let Some(g) = gesture_on(gesture, entity, selection, scene, intents) else {
        return;
    };
    g.idle.reset();
    if nudge != Vec3::ZERO {
        g.nudge(scene, nudge);
    }
    if turn != 0.0 || scale != 1.0 {
        g.turn_and_scale(scene, Vec3::Y, turn, Vec3::splat(scale));
    }
}

/// A wire box around each selected entity — around everything under it,
/// since its children move with it. The primary draws brighter.
fn draw_selection(
    mut gizmos: Gizmos,
    selection: Res<InspectorSelection>,
    globals: Query<&GlobalTransform>,
    bounds: Query<&Aabb>,
    children: Query<&Children>,
) {
    for (n, root) in selection.all().enumerate() {
        let color = if n == 0 { SELECTED } else { SELECTED_TOO };
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        let mut found = false;
        let mut stack = vec![root];
        // Bounded, so a pathological hierarchy can't stall a frame.
        let mut budget = 4096;
        while let Some(entity) = stack.pop() {
            budget -= 1;
            if budget == 0 {
                break;
            }
            if let (Ok(aabb), Ok(global)) = (bounds.get(entity), globals.get(entity)) {
                for corner in corners(aabb) {
                    let at = global.transform_point(corner);
                    min = min.min(at);
                    max = max.max(at);
                }
                found = true;
            }
            if let Ok(kids) = children.get(entity) {
                stack.extend(kids.iter());
            }
        }
        if found {
            let pad = Vec3::splat(0.03);
            gizmos.aabb_3d(
                Aabb3d {
                    min: (min - pad).into(),
                    max: (max + pad).into(),
                },
                Transform::IDENTITY,
                color,
            );
        } else if let Ok(global) = globals.get(root) {
            // Nothing with a mesh — a light, an empty: mark where it is.
            gizmos.sphere(
                Isometry3d::from_translation(global.translation()),
                0.25,
                color,
            );
        }
    }
}

/// The handles around the primary selection: axis arrows, a ring about
/// each axis in the axis's color, the scale cube; the one under the
/// pointer, or being dragged, lights up.
fn draw_handles(mut gizmos: Gizmos, handles: Res<Handles>, gesture: Res<CanvasGesture>) {
    let Some(frame) = handles.frame else {
        return;
    };
    let active = gesture
        .0
        .as_ref()
        .and_then(|g| g.drag.as_ref())
        .and_then(|d| d.handle.as_ref())
        .map(|h| h.handle);
    let hot = active.or(handles.hot);
    for (i, axis) in frame.axes.iter().enumerate() {
        let lit = matches!(hot, Some(Handle::Axis(j) | Handle::Stretch(j)) if j == i);
        let color = if lit { HOT } else { AXIS_COLORS[i] };
        gizmos.arrow(
            frame.origin + *axis * frame.length * 0.15,
            frame.origin + *axis * frame.length,
            color,
        );
    }
    for (i, facing) in ring_facings().iter().enumerate() {
        gizmos.circle(
            Isometry3d::new(frame.origin, frame.rotation * *facing),
            frame.ring_radius,
            if hot == Some(Handle::Ring(i)) {
                HOT
            } else {
                AXIS_COLORS[i]
            },
        );
    }
    let half = Vec3::splat(frame.length * 0.05);
    gizmos.aabb_3d(
        Aabb3d {
            min: (frame.scale_at - half).into(),
            max: (frame.scale_at + half).into(),
        },
        Transform::IDENTITY,
        if hot == Some(Handle::Scale) {
            HOT
        } else {
            SCALE_HANDLE
        },
    );
}

/// The handle a ray touches, nearest first.
fn hit_handle(frame: &HandleFrame, ray: Ray3d) -> Option<Handle> {
    let mut best: Option<(f32, Handle)> = None;
    let mut consider = |distance: f32, handle: Handle, threshold: f32| {
        if distance < threshold && best.is_none_or(|(d, _)| distance < d) {
            best = Some((distance, handle));
        }
    };
    for (i, axis) in frame.axes.iter().enumerate() {
        let (distance, _) = ray_segment_distance(
            ray,
            frame.origin + *axis * frame.length * 0.15,
            frame.origin + *axis * frame.length,
        );
        consider(distance, Handle::Axis(i), frame.threshold);
    }
    consider(
        ray_point_distance(ray, frame.scale_at),
        Handle::Scale,
        frame.threshold * 1.4,
    );
    for (i, axis) in frame.axes.iter().enumerate() {
        if let Some(hit) = hit_plane(ray, frame.origin, *axis) {
            let radius = (hit - frame.origin).length();
            consider(
                (radius - frame.ring_radius).abs(),
                Handle::Ring(i),
                frame.threshold,
            );
        }
    }
    best.map(|(_, handle)| handle)
}

/// The rotation that lays a gizmo circle (drawn about +Z) in each ring's
/// plane: about X, about Y, about Z.
fn ring_facings() -> [Quat; 3] {
    [
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        Quat::IDENTITY,
    ]
}

fn corners(aabb: &Aabb) -> [Vec3; 8] {
    let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
    [
        c + Vec3::new(-h.x, -h.y, -h.z),
        c + Vec3::new(h.x, -h.y, -h.z),
        c + Vec3::new(-h.x, h.y, -h.z),
        c + Vec3::new(h.x, h.y, -h.z),
        c + Vec3::new(-h.x, -h.y, h.z),
        c + Vec3::new(h.x, -h.y, h.z),
        c + Vec3::new(-h.x, h.y, h.z),
        c + Vec3::new(h.x, h.y, h.z),
    ]
}

fn hit_plane(ray: Ray3d, origin: Vec3, normal: Vec3) -> Option<Vec3> {
    let plane = InfinitePlane3d::new(normal);
    ray.intersect_plane(origin, plane)
        .map(|distance| ray.get_point(distance))
}

/// The world axis nearest a direction, in the horizontal plane.
fn snap_to_axis(v: Vec3) -> Vec3 {
    if v.x.abs() > v.z.abs() {
        Vec3::X * v.x.signum()
    } else {
        Vec3::Z * if v.z == 0.0 { -1.0 } else { v.z.signum() }
    }
}

/// The nearest multiple of `step`.
fn snap_to(value: f32, step: f32) -> f32 {
    if step <= 0.0 {
        return value;
    }
    (value / step).round() * step
}

/// The distance from a ray to a point, measured along the ray forward of
/// its origin.
fn ray_point_distance(ray: Ray3d, point: Vec3) -> f32 {
    let d = ray.direction.as_vec3();
    let v = point - ray.origin;
    let s = v.dot(d).max(0.0);
    (v - d * s).length()
}

/// The closest distance between a ray and a segment, and where on the
/// segment it is (0 at `a`, 1 at `b`).
fn ray_segment_distance(ray: Ray3d, a: Vec3, b: Vec3) -> (f32, f32) {
    let d = ray.direction.as_vec3();
    let e = b - a;
    let r = ray.origin - a;
    let dd = d.dot(d);
    let de = d.dot(e);
    let ee = e.dot(e).max(1e-9);
    let dr = d.dot(r);
    let er = e.dot(r);
    let denom = dd * ee - de * de;
    let mut t = if denom.abs() < 1e-6 {
        (er / ee).clamp(0.0, 1.0)
    } else {
        ((dd * er - de * dr) / denom).clamp(0.0, 1.0)
    };
    // The ray's nearest point to that spot, forward of its origin; then the
    // segment's nearest point to it, so both clamps hold.
    let s = ((a + e * t - ray.origin).dot(d) / dd).max(0.0);
    t = ((ray.origin + d * s - a).dot(e) / ee).clamp(0.0, 1.0);
    let p = ray.origin + d * s;
    let q = a + e * t;
    ((p - q).length(), t)
}

/// The parameter `s` of the point `origin + s·axis` nearest a ray — where
/// along a handle's axis the pointer is.
fn line_param_nearest_ray(origin: Vec3, axis: Vec3, ray: Ray3d) -> f32 {
    let d = ray.direction.as_vec3();
    let w = origin - ray.origin;
    let aa = axis.dot(axis);
    let ab = axis.dot(d);
    let bb = d.dot(d);
    let aw = axis.dot(w);
    let bw = d.dot(w);
    let denom = aa * bb - ab * ab;
    if denom.abs() < 1e-6 {
        // Looking straight down the axis: no place along it is nearer.
        return 0.0;
    }
    (ab * bw - bb * aw) / denom
}

/// The rotation about `axis` that takes direction `from` to direction `to`,
/// in radians — right-handed, so about +Y it is counter-clockwise seen from
/// above.
fn signed_angle_about(axis: Vec3, from: Vec3, to: Vec3) -> f32 {
    from.cross(to).dot(axis).atan2(from.dot(to))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nudge_follows_the_axis_nearest_the_view() {
        // Looking down -Z (Bevy's default forward): forward is -Z, right +X.
        assert_eq!(snap_to_axis(Vec3::new(0.1, -0.5, -0.9)), Vec3::NEG_Z);
        assert_eq!(snap_to_axis(Vec3::new(0.9, 0.2, 0.3)), Vec3::X);
        assert_eq!(snap_to_axis(Vec3::new(-0.7, 0.0, 0.69)), Vec3::NEG_X);
        // Straight down has no horizontal part: forward, not nothing.
        assert_eq!(snap_to_axis(Vec3::NEG_Y), Vec3::NEG_Z);
    }

    #[test]
    fn a_drag_plane_is_hit_where_the_ray_crosses_it() {
        let ray = Ray3d::new(
            Vec3::new(0.0, 5.0, 5.0),
            Dir3::new(Vec3::new(0.0, -1.0, -1.0)).unwrap(),
        );
        let hit = hit_plane(ray, Vec3::new(3.0, 1.0, -2.0), Vec3::Y).unwrap();
        assert!((hit - Vec3::new(0.0, 1.0, 1.0)).length() < 1e-5, "{hit}");
        // Parallel to the plane: no hit, not a point at infinity.
        let flat = Ray3d::new(Vec3::Y, Dir3::X);
        assert!(hit_plane(flat, Vec3::ZERO, Vec3::Y).is_none());
    }

    #[test]
    fn a_box_has_its_eight_corners() {
        let aabb = Aabb::from_min_max(Vec3::new(-1.0, 0.0, -2.0), Vec3::new(1.0, 3.0, 2.0));
        let c = corners(&aabb);
        let min = c.iter().fold(Vec3::splat(f32::MAX), |m, p| m.min(*p));
        let max = c.iter().fold(Vec3::splat(f32::MIN), |m, p| m.max(*p));
        assert_eq!(
            (min, max),
            (Vec3::new(-1.0, 0.0, -2.0), Vec3::new(1.0, 3.0, 2.0))
        );
    }

    #[test]
    fn snapping_lands_on_the_grid_and_the_angle_step() {
        assert_eq!(snap_to(1.13, 0.25), 1.25);
        assert_eq!(snap_to(-0.3, 0.25), -0.25);
        assert_eq!(snap_to(37.0, 15.0), 30.0);
        assert_eq!(snap_to(38.0, 15.0), 45.0);
        // No step: the value as it is.
        assert_eq!(snap_to(1.13, 0.0), 1.13);
    }

    #[test]
    fn the_pointer_is_found_along_a_handles_axis() {
        // A ray from above and in front, passing through x = 2 on the X axis.
        let ray = Ray3d::new(
            Vec3::new(2.0, 5.0, 5.0),
            Dir3::new(Vec3::new(0.0, -1.0, -1.0)).unwrap(),
        );
        let s = line_param_nearest_ray(Vec3::ZERO, Vec3::X, ray);
        assert!((s - 2.0).abs() < 1e-5, "{s}");
        // Looking down the axis itself says nowhere in particular.
        let along = Ray3d::new(Vec3::new(-5.0, 0.0, 0.0), Dir3::X);
        assert_eq!(line_param_nearest_ray(Vec3::ZERO, Vec3::X, along), 0.0);
    }

    #[test]
    fn a_ray_measures_its_distance_to_a_segment_and_a_point() {
        let ray = Ray3d::new(Vec3::new(0.0, 1.0, 5.0), Dir3::NEG_Z);
        // The ray passes 1 above the X axis segment's midpoint.
        let (distance, t) =
            ray_segment_distance(ray, Vec3::new(-2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0));
        assert!((distance - 1.0).abs() < 1e-5, "{distance}");
        assert!((t - 0.5).abs() < 1e-5, "{t}");
        // Past the segment's end, the end is the nearest point.
        let (distance, t) =
            ray_segment_distance(ray, Vec3::new(-6.0, 0.0, 0.0), Vec3::new(-3.0, 0.0, 0.0));
        assert!(
            (distance - (1.0f32 + 9.0).sqrt()).abs() < 1e-4,
            "{distance}"
        );
        assert!((t - 1.0).abs() < 1e-5, "{t}");
        // Behind the ray's origin counts from the origin, not from behind it.
        assert!((ray_point_distance(ray, Vec3::new(0.0, 1.0, 9.0)) - 4.0).abs() < 1e-5);
        assert!((ray_point_distance(ray, Vec3::new(0.0, 4.0, -2.0)) - 3.0).abs() < 1e-5);
    }

    #[test]
    fn the_ring_turns_counter_clockwise_seen_from_above() {
        let quarter = signed_angle_about(Vec3::Y, Vec3::Z, Vec3::X);
        assert!(
            (quarter - std::f32::consts::FRAC_PI_2).abs() < 1e-5,
            "{quarter}"
        );
        // Applying it as a rotation about Y takes `from` to `to`.
        let turned = Quat::from_rotation_y(quarter) * Vec3::Z;
        assert!((turned - Vec3::X).length() < 1e-5, "{turned}");
        let back = signed_angle_about(Vec3::Y, Vec3::X, Vec3::Z);
        assert!((back + std::f32::consts::FRAC_PI_2).abs() < 1e-5, "{back}");
        // The same rule about X: the angle applied about X takes Y to Z.
        let about_x = signed_angle_about(Vec3::X, Vec3::Y, Vec3::Z);
        let turned = Quat::from_axis_angle(Vec3::X, about_x) * Vec3::Y;
        assert!((turned - Vec3::Z).length() < 1e-5, "{turned}");
    }

    #[test]
    fn each_ring_lies_in_its_own_plane() {
        // A circle gizmo is drawn about +Z; faced for ring i, its normal is
        // axis i.
        for (i, facing) in ring_facings().iter().enumerate() {
            let normal = *facing * Vec3::Z;
            assert!(
                (normal.abs() - AXES[i]).length() < 1e-5,
                "ring {i} faces {normal}"
            );
        }
    }

    #[test]
    fn local_axes_turn_the_frame_with_the_entity() {
        let camera = Vec3::new(0.0, 5.0, 10.0);
        let world = HandleFrame::around(Vec3::ZERO, camera, None);
        assert!(!world.local);
        assert_eq!(world.axes, AXES);
        // An entity turned a quarter about Y: its own X points down -Z.
        let turned = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let local = HandleFrame::around(Vec3::ZERO, camera, Some(turned));
        assert!(local.local);
        assert!(
            (local.axes[0] - Vec3::NEG_Z).length() < 1e-5,
            "{:?}",
            local.axes
        );
        assert!(
            (local.axes[1] - Vec3::Y).length() < 1e-5,
            "{:?}",
            local.axes
        );
        assert!(
            (local.axes[2] - Vec3::X).length() < 1e-5,
            "{:?}",
            local.axes
        );
        // The X arrow is found where it now lies, not where the world's is.
        let on_x = local.axes[0] * local.length;
        let ray = Ray3d::new(on_x + Vec3::Y * 10.0, Dir3::NEG_Y);
        assert_eq!(hit_handle(&local, ray), Some(Handle::Axis(0)));
        let where_world_x_was = Ray3d::new(Vec3::X * local.length + Vec3::Y * 10.0, Dir3::NEG_Y);
        assert_eq!(hit_handle(&local, where_world_x_was), Some(Handle::Axis(2)));
    }

    #[test]
    fn the_nearest_handle_wins_and_nothing_far_away_does() {
        let frame = HandleFrame::around(Vec3::ZERO, Vec3::new(0.0, 5.0, 10.0), None);
        // Straight down onto the end of the X arrow.
        let x_end = Vec3::X * frame.length;
        let ray = Ray3d::new(x_end + Vec3::Y * 10.0, Dir3::NEG_Y);
        assert_eq!(hit_handle(&frame, ray), Some(Handle::Axis(0)));
        // Straight down onto the ring about Y, away from any arrow.
        let on_ring = Vec3::new(1.0, 0.0, 1.0).normalize() * frame.ring_radius;
        let ray = Ray3d::new(on_ring + Vec3::Y * 10.0, Dir3::NEG_Y);
        assert_eq!(hit_handle(&frame, ray), Some(Handle::Ring(1)));
        // Along -X onto the ring about X, where no other ring or arrow is.
        let on_x_ring = Vec3::new(0.0, 1.0, -1.0).normalize() * frame.ring_radius;
        let ray = Ray3d::new(on_x_ring + Vec3::X * 10.0, Dir3::NEG_X);
        assert_eq!(hit_handle(&frame, ray), Some(Handle::Ring(0)));
        // Onto the cube.
        let ray = Ray3d::new(frame.scale_at + Vec3::Y * 10.0, Dir3::NEG_Y);
        assert_eq!(hit_handle(&frame, ray), Some(Handle::Scale));
        // Far from everything.
        let ray = Ray3d::new(Vec3::new(50.0, 10.0, 50.0), Dir3::NEG_Y);
        assert_eq!(hit_handle(&frame, ray), None);
    }
}
