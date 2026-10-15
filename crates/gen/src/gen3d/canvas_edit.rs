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
//! earlier point in the history starts a branch there: forking from the rail
//! is seeking back and editing.
//!
//! | Input | Edit |
//! |---|---|
//! | click | select (Esc, or a click on nothing: deselect); shift-click adds to the selection |
//! | drag | move along the ground; with Alt held, up and down |
//! | drag a handle | move along one axis (the arrows), turn (the ring), scale (the cube) |
//! | arrows, PgUp / PgDn | nudge one grid step, along the axis nearest the view / vertically |
//! | `,` `.` | turn one angle step left / right |
//! | `-` `=` | scale down / up by 10% |
//! | `G` | snapping on / off — or hold Cmd/Ctrl while dragging for the opposite |
//! | Delete / Backspace | delete (children go with it) |
//! | Cmd/Ctrl + D | duplicate beside it |
//! | Cmd/Ctrl + G | group the selection under a new empty at its centroid |
//! | Cmd/Ctrl + Z | undo on the branch on screen |
//!
//! A multi-selection moves, turns, scales, deletes and duplicates together,
//! as one entry. The panels (`crate::inspector`) edit through the same
//! stream: a field, a parent, a bookmark arrives here as an intent and goes
//! out as a batch the way a gesture does.
//!
//! W A S D, Space and Shift stay the camera's and the right button looks
//! around, so none of these collide with flying.

use bevy::camera::primitives::Aabb;
use bevy::ecs::system::SystemParam;
use bevy::math::bounding::Aabb3d;
use bevy::picking::events::{Click, Drag, DragEnd, DragStart, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;
use serde_json::Value;

use super::avatar;
use super::plugin::FlyCam;
use super::registry::{GenEntity, NameRegistry};
use crate::inspector::{InspectorSelection, PanelIntents, UiHovered};
use localgpt_world_types as wt;

/// How long a run of key presses waits for the next one before it commits.
const KEY_GESTURE_IDLE_SECS: f32 = 0.6;
/// One scale step.
const SCALE_STEP: f32 = 1.1;
const SELECTED: Color = Color::srgb(0.35, 0.8, 1.0);
const SELECTED_TOO: Color = Color::srgb(0.25, 0.55, 0.72);
const AXES: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::Z];
const AXIS_COLORS: [Color; 3] = [
    Color::srgb(0.95, 0.3, 0.3),
    Color::srgb(0.4, 0.9, 0.4),
    Color::srgb(0.35, 0.5, 1.0),
];
const RING: Color = Color::srgb(0.95, 0.85, 0.3);
const SCALE_HANDLE: Color = Color::srgb(0.9, 0.9, 0.9);
const HOT: Color = Color::WHITE;

/// A person's edits in the canvas.
pub struct CanvasEditPlugin;

/// The person-edit systems; the canvas commits after them in the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PersonEdits;

impl Plugin for CanvasEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PersonIntent>()
            .init_resource::<CanvasGesture>()
            .init_resource::<CanvasSettings>()
            .init_resource::<Handles>()
            .add_systems(
                Update,
                (
                    keep_selection,
                    panel_intents,
                    (handle_edits, pointer_edits, key_edits).run_if(avatar::in_freefly_mode),
                )
                    .chain()
                    .in_set(PersonEdits),
            )
            .add_systems(Update, (draw_selection, draw_handles).after(PersonEdits));
    }
}

/// Snapping, for the canvas to apply and the panels to show.
#[derive(Resource)]
pub struct CanvasSettings {
    /// Drags land on the grid and on the angle step; `G` toggles, and a
    /// Cmd/Ctrl held during a drag means the opposite.
    pub snap: bool,
    /// The grid, and one nudge.
    pub grid_m: f32,
    /// The angle step, and one turn.
    pub angle_degrees: f32,
}

impl Default for CanvasSettings {
    fn default() -> Self {
        Self {
            snap: false,
            grid_m: 0.25,
            angle_degrees: 15.0,
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
    /// A field edit from a panel: a merge patch for one entity, as
    /// `ModifyEntity` takes it, and what to call the change.
    Patch {
        id: u64,
        name: String,
        patch: Value,
        what: String,
    },
    /// A scene-wide edit from a panel: a `ModifyWorld` patch.
    World { patch: Value, what: String },
    /// Undo on the path to the entry on screen.
    Undo,
}

/// One entity a gesture took.
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub id: u64,
    pub name: String,
    /// The local position the canvas left it at, when the gesture moved it.
    pub position: Option<[f32; 3]>,
    /// Degrees about the vertical axis, counter-clockwise seen from above.
    pub yaw_degrees: f32,
    /// Uniform factor.
    pub scale_by: f32,
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
    yaw_degrees: f32,
    scale_by: f32,
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
}

impl Gesture {
    fn new(grabbed: Vec<Grabbed>) -> Self {
        Self {
            grabbed,
            moved: false,
            yaw_degrees: 0.0,
            scale_by: 1.0,
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

    /// Turn everything by `degrees` about its own vertical — the parent's,
    /// which is what the committed angles say — and scale it by `factor`.
    fn turn_and_scale(&mut self, scene: &mut Scene, degrees: f32, factor: f32) {
        for g in &self.grabbed {
            if let Ok(mut transform) = scene.transforms.get_mut(g.entity) {
                transform.rotation =
                    Quat::from_rotation_y(degrees.to_radians()) * transform.rotation;
                transform.scale *= factor;
            }
        }
        self.yaw_degrees += degrees;
        self.scale_by *= factor;
    }

    /// The intent this gesture commits: where the canvas left each entity.
    fn finish(self, transforms: &Query<&mut Transform>) -> PersonIntent {
        PersonIntent::Transform(
            self.grabbed
                .iter()
                .map(|g| Moved {
                    id: g.id,
                    name: g.name.clone(),
                    position: if self.moved {
                        transforms
                            .get(g.entity)
                            .ok()
                            .map(|t| t.translation.to_array())
                    } else {
                        None
                    },
                    yaw_degrees: self.yaw_degrees,
                    scale_by: self.scale_by,
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

/// The handles around the selection: three axis arrows, a ring about the
/// vertical, a cube for uniform scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handle {
    Axis(usize),
    Ring,
    Scale,
}

/// A handle being dragged, and where the drag began.
struct HandleGrab {
    handle: Handle,
    /// Along the axis: the line parameter under the pointer at the grab.
    start_s: f32,
    /// On the ring: the direction from the origin to the grab.
    start_dir: Vec3,
    /// On the cube: the screen distance from the origin to the grab.
    start_px: f32,
    /// What the ring has applied so far, so each frame applies the rest.
    applied_yaw: f32,
    /// What the cube has applied so far.
    applied_scale: f32,
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

/// The handles' geometry: sized by the distance to the camera so they take
/// about the same screen space from anywhere.
#[derive(Debug, Clone, Copy)]
struct HandleFrame {
    origin: Vec3,
    length: f32,
    ring_radius: f32,
    scale_at: Vec3,
    /// How near the pointer's ray must pass, in world units.
    threshold: f32,
}

impl HandleFrame {
    fn around(origin: Vec3, camera: Vec3) -> Self {
        let distance = (camera - origin).length().max(0.5);
        let length = (0.22 * distance).clamp(0.4, 12.0);
        Self {
            origin,
            length,
            ring_radius: 0.8 * length,
            scale_at: origin + Vec3::ONE.normalize() * 0.9 * length,
            threshold: 0.035 * distance,
        }
    }
}

/// The scene as the edit systems need it.
#[derive(SystemParam)]
struct Scene<'w, 's> {
    gen_entities: Query<'w, 's, &'static GenEntity>,
    parents: Query<'w, 's, &'static ChildOf>,
    globals: Query<'w, 's, &'static GlobalTransform>,
    transforms: Query<'w, 's, &'static mut Transform>,
    registry: Res<'w, NameRegistry>,
    camera: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<FlyCam>>,
    windows: Query<'w, 's, (), With<Window>>,
    primary_window: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
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

    /// Where the pointer is in the window, if it is in it.
    fn cursor(&self) -> Option<Vec2> {
        self.primary_window.single().ok()?.cursor_position()
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
                grabbed.push(Grabbed {
                    entity,
                    id,
                    name,
                    start,
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

/// The panels' intents, into the same stream as the gestures. egui may run
/// a second pass in a frame (its multi-pass mode, for sizing), and a click
/// is a click in both: an intent identical to the one before it is the
/// same ask, not a second one.
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

/// The handles: hover, grab, and the drag along an axis, around the ring
/// or out from the cube. Runs before the picking-driven edits so a press on
/// a handle is a handle's, not a press on what is behind it.
#[allow(clippy::too_many_arguments)]
fn handle_edits(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    hovered: Option<Res<UiHovered>>,
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
    let frame = HandleFrame::around(global.translation(), camera.translation());
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
        let (start_s, start_dir, start_px) = (hg.start_s, hg.start_dir, hg.start_px);
        let (applied_yaw, applied_scale) = (hg.applied_yaw, hg.applied_scale);
        let mut target = None;
        let mut yaw = applied_yaw;
        let mut scale = applied_scale;
        match kind {
            Handle::Axis(i) => {
                let axis = AXES[i];
                let s = line_param_nearest_ray(origin, axis, ray);
                let mut t = origin + axis * (s - start_s);
                if snap {
                    t[i] = snap_to(t[i], settings.grid_m);
                }
                target = Some(t);
            }
            Handle::Ring => {
                if let Some(hit) = hit_plane(ray, origin, Vec3::Y) {
                    let dir = (hit - origin).normalize_or_zero();
                    if dir != Vec3::ZERO {
                        yaw = signed_angle_about_y(start_dir, dir).to_degrees();
                        if snap {
                            yaw = snap_to(yaw, settings.angle_degrees);
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
        if yaw != applied_yaw || scale != applied_scale {
            g.turn_and_scale(&mut scene, yaw - applied_yaw, scale / applied_scale);
        }
        if let Some(hg) = g.drag.as_mut().and_then(|d| d.handle.as_mut()) {
            hg.applied_yaw = yaw;
            hg.applied_scale = scale;
        }
        return;
    }

    // Hover, and a press that grabs.
    if hovered.is_some() || gesture.dragging() {
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
    let Some(g) = gesture_on(&mut gesture, primary, &selection, &scene, &mut intents) else {
        return;
    };
    g.begin_drag(&scene);
    let origin = frame.origin;
    let mut hg = HandleGrab {
        handle: kind,
        start_s: 0.0,
        start_dir: Vec3::X,
        start_px: 1.0,
        applied_yaw: 0.0,
        applied_scale: 1.0,
    };
    match kind {
        Handle::Axis(i) => hg.start_s = line_param_nearest_ray(origin, AXES[i], ray),
        Handle::Ring => {
            hg.start_dir = hit_plane(ray, origin, Vec3::Y)
                .map(|hit| (hit - origin).normalize_or(Vec3::X))
                .unwrap_or(Vec3::X);
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
    hovered: Option<Res<UiHovered>>,
    settings: Res<CanvasSettings>,
    mut handles: ResMut<Handles>,
    mut selection: ResMut<InspectorSelection>,
    mut gesture: ResMut<CanvasGesture>,
    mut intents: MessageWriter<PersonIntent>,
    mut scene: Scene,
) {
    // A panel under the pointer (the inspector) has the pointer, not the
    // world; and a press on a handle was the handle's.
    if hovered.is_some() || handles.pressed {
        clicks.clear();
        starts.clear();
        drags.clear();
        ends.clear();
        if handles.pressed && !mouse.pressed(MouseButton::Left) {
            handles.pressed = false;
        }
        return;
    }
    let extend = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let snap = snapping(&settings, &keys);

    for click in clicks.read() {
        if click.event.button != PointerButton::Primary {
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
        if start.event.button != PointerButton::Primary {
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

/// Keys edit the selection; a run of presses is one gesture.
#[allow(clippy::too_many_arguments)]
fn key_edits(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut egui: EguiContexts,
    mut settings: ResMut<CanvasSettings>,
    mut selection: ResMut<InspectorSelection>,
    mut gesture: ResMut<CanvasGesture>,
    mut intents: MessageWriter<PersonIntent>,
    mut scene: Scene,
) {
    // Typing into a panel is not editing the world.
    let typing = egui
        .ctx_mut()
        .is_ok_and(|ctx| ctx.egui_wants_keyboard_input());
    if !typing {
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
        if keys.just_pressed(KeyCode::KeyG) && !command {
            settings.snap = !settings.snap;
        }
        if command && keys.just_pressed(KeyCode::KeyZ) {
            flush_unless(&mut gesture, None, &scene, &mut intents);
            intents.write(PersonIntent::Undo);
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
        g.turn_and_scale(scene, turn, scale);
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

/// The handles around the primary selection: axis arrows, the yaw ring,
/// the scale cube; the one under the pointer, or being dragged, lights up.
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
    for (i, axis) in AXES.iter().enumerate() {
        let color = if hot == Some(Handle::Axis(i)) {
            HOT
        } else {
            AXIS_COLORS[i]
        };
        gizmos.arrow(
            frame.origin + *axis * frame.length * 0.15,
            frame.origin + *axis * frame.length,
            color,
        );
    }
    gizmos.circle(
        Isometry3d::new(
            frame.origin,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        ),
        frame.ring_radius,
        if hot == Some(Handle::Ring) { HOT } else { RING },
    );
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
    for (i, axis) in AXES.iter().enumerate() {
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
    if let Some(hit) = hit_plane(ray, frame.origin, Vec3::Y) {
        let radius = (hit - frame.origin).length();
        consider(
            (radius - frame.ring_radius).abs(),
            Handle::Ring,
            frame.threshold,
        );
    }
    best.map(|(_, handle)| handle)
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

/// The rotation about +Y that takes direction `from` to direction `to`,
/// in radians, counter-clockwise seen from above.
fn signed_angle_about_y(from: Vec3, to: Vec3) -> f32 {
    from.cross(to).dot(Vec3::Y).atan2(from.dot(to))
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
        let quarter = signed_angle_about_y(Vec3::Z, Vec3::X);
        assert!(
            (quarter - std::f32::consts::FRAC_PI_2).abs() < 1e-5,
            "{quarter}"
        );
        // Applying it as a rotation about Y takes `from` to `to`.
        let turned = Quat::from_rotation_y(quarter) * Vec3::Z;
        assert!((turned - Vec3::X).length() < 1e-5, "{turned}");
        let back = signed_angle_about_y(Vec3::X, Vec3::Z);
        assert!((back + std::f32::consts::FRAC_PI_2).abs() < 1e-5, "{back}");
    }

    #[test]
    fn the_nearest_handle_wins_and_nothing_far_away_does() {
        let frame = HandleFrame::around(Vec3::ZERO, Vec3::new(0.0, 5.0, 10.0));
        // Straight down onto the end of the X arrow.
        let x_end = Vec3::X * frame.length;
        let ray = Ray3d::new(x_end + Vec3::Y * 10.0, Dir3::NEG_Y);
        assert_eq!(hit_handle(&frame, ray), Some(Handle::Axis(0)));
        // Straight down onto the ring, away from any arrow.
        let on_ring = Vec3::new(1.0, 0.0, 1.0).normalize() * frame.ring_radius;
        let ray = Ray3d::new(on_ring + Vec3::Y * 10.0, Dir3::NEG_Y);
        assert_eq!(hit_handle(&frame, ray), Some(Handle::Ring));
        // Onto the cube.
        let ray = Ray3d::new(frame.scale_at + Vec3::Y * 10.0, Dir3::NEG_Y);
        assert_eq!(hit_handle(&frame, ray), Some(Handle::Scale));
        // Far from everything.
        let ray = Ray3d::new(Vec3::new(50.0, 10.0, 50.0), Dir3::NEG_Y);
        assert_eq!(hit_handle(&frame, ray), None);
    }
}
