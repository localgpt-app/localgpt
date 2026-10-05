//! A person editing the live canvas — select, move, turn, scale, delete,
//! duplicate, undo — each gesture one batch through the authority the agents
//! use, committed at the entry on screen.
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
//! | click | select (Esc, or a click on nothing: deselect) |
//! | drag | move along the ground; with Alt held, up and down |
//! | arrows, PgUp / PgDn | nudge 0.25 m, along the axis nearest the view / vertically |
//! | `,` `.` | turn 15° left / right |
//! | `-` `=` | scale down / up by 10% |
//! | Delete / Backspace | delete (children go with it) |
//! | Cmd/Ctrl + D | duplicate beside it |
//! | Cmd/Ctrl + Z | undo on the branch on screen |
//!
//! W A S D, Space and Shift stay the camera's and the right button looks
//! around, so none of these collide with flying.

use bevy::camera::primitives::Aabb;
use bevy::ecs::system::SystemParam;
use bevy::math::bounding::Aabb3d;
use bevy::picking::events::{Click, Drag, DragEnd, DragStart, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy_egui::EguiContexts;

use super::avatar;
use super::plugin::FlyCam;
use super::registry::{GenEntity, NameRegistry};
use crate::inspector::{InspectorSelection, UiHovered};
use localgpt_world_types as wt;

/// How long a run of key presses waits for the next one before it commits.
const KEY_GESTURE_IDLE_SECS: f32 = 0.6;
/// One nudge.
const NUDGE_METERS: f32 = 0.25;
/// One turn.
const TURN_DEGREES: f32 = 15.0;
/// One scale step.
const SCALE_STEP: f32 = 1.1;
const SELECTED: Color = Color::srgb(0.35, 0.8, 1.0);

/// A person's edits in the canvas.
pub struct CanvasEditPlugin;

/// The person-edit systems; the canvas commits after them in the same frame.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PersonEdits;

impl Plugin for CanvasEditPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PersonIntent>()
            .init_resource::<CanvasGesture>()
            .add_systems(
                Update,
                (
                    keep_selection,
                    (pointer_edits, key_edits).run_if(avatar::in_freefly_mode),
                )
                    .chain()
                    .in_set(PersonEdits),
            )
            .add_systems(Update, draw_selection.after(PersonEdits));
    }
}

/// What a person asked of the world, for the canvas to commit at the entry
/// on screen.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum PersonIntent {
    /// A gesture's result for one entity: where the canvas left it, and how
    /// far it was turned and scaled. Turns and scales travel as deltas
    /// because the document holds the angles, and only it can add to them
    /// without changing their form.
    Transform {
        id: u64,
        name: String,
        /// The local position the canvas left it at, when the gesture moved it.
        position: Option<[f32; 3]>,
        /// Degrees about the vertical axis, counter-clockwise seen from above.
        yaw_degrees: f32,
        /// Uniform factor.
        scale_by: f32,
    },
    /// Delete an entity and its children.
    Delete { id: u64, name: String },
    /// A copy of an entity beside it, under a free name.
    Duplicate { id: u64 },
    /// Undo on the path to the entry on screen.
    Undo,
}

/// The gesture in progress, if any.
#[derive(Resource, Default)]
struct CanvasGesture(Option<Gesture>);

/// One entity being moved, turned or scaled, and what has changed so far.
struct Gesture {
    entity: Entity,
    id: u64,
    name: String,
    moved: bool,
    yaw_degrees: f32,
    scale_by: f32,
    /// Quiet time before a run of key presses commits; a drag commits on
    /// release instead.
    idle: Timer,
    drag: Option<Grab>,
}

impl Gesture {
    fn new(entity: Entity, id: u64, name: String) -> Self {
        Self {
            entity,
            id,
            name,
            moved: false,
            yaw_degrees: 0.0,
            scale_by: 1.0,
            idle: Timer::from_seconds(KEY_GESTURE_IDLE_SECS, TimerMode::Once),
            drag: None,
        }
    }

    /// The intent this gesture commits: where the canvas left the entity.
    fn finish(self, transforms: &Query<&mut Transform>) -> PersonIntent {
        let position = if self.moved {
            transforms
                .get(self.entity)
                .ok()
                .map(|t| t.translation.to_array())
        } else {
            None
        };
        PersonIntent::Transform {
            id: self.id,
            name: self.name,
            position,
            yaw_degrees: self.yaw_degrees,
            scale_by: self.scale_by,
        }
    }
}

/// A drag: the plane the entity slides on, and where on it it was grabbed.
struct Grab {
    /// The entity's world position when the drag began; on the plane.
    origin: Vec3,
    normal: Vec3,
    /// From the grabbed point to the entity's origin, so it doesn't jump to
    /// the pointer.
    offset: Vec3,
    vertical: bool,
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

    /// The world ray under a point of the window.
    fn ray(&self, at: Vec2) -> Option<Ray3d> {
        let (camera, transform) = self.camera.single().ok()?;
        camera.viewport_to_world(transform, at).ok()
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
    if gesture.0.as_ref().is_some_and(|g| Some(g.entity) != entity)
        && let Some(done) = gesture.0.take()
    {
        intents.write(done.finish(&scene.transforms));
    }
}

/// The gesture on `entity`, committing one on another entity first.
fn gesture_on<'g>(
    gesture: &'g mut CanvasGesture,
    entity: Entity,
    scene: &Scene,
    intents: &mut MessageWriter<PersonIntent>,
) -> Option<&'g mut Gesture> {
    flush_unless(gesture, Some(entity), scene, intents);
    if gesture.0.is_none() {
        let (id, name) = scene.identify(entity)?;
        gesture.0 = Some(Gesture::new(entity, id, name));
    }
    gesture.0.as_mut()
}

/// Keep selecting the same world entity when the scene replaces it under
/// the selection — a seek or an undo can despawn it and spawn it again.
fn keep_selection(
    mut selection: ResMut<InspectorSelection>,
    gen_entities: Query<&GenEntity>,
    registry: Res<NameRegistry>,
    mut last: Local<Option<u64>>,
) {
    let Some(entity) = selection.entity else {
        *last = None;
        return;
    };
    if let Ok(found) = gen_entities.get(entity) {
        *last = Some(found.world_id.0);
        return;
    }
    let again = last.and_then(|id| registry.get_entity_by_id(&wt::EntityId(id)));
    if again.is_none() {
        *last = None;
    }
    if selection.entity != again {
        selection.entity = again;
    }
}

/// Clicks select; drags move.
#[allow(clippy::too_many_arguments)]
fn pointer_edits(
    mut clicks: MessageReader<Pointer<Click>>,
    mut starts: MessageReader<Pointer<DragStart>>,
    mut drags: MessageReader<Pointer<Drag>>,
    mut ends: MessageReader<Pointer<DragEnd>>,
    keys: Res<ButtonInput<KeyCode>>,
    hovered: Option<Res<UiHovered>>,
    mut selection: ResMut<InspectorSelection>,
    mut gesture: ResMut<CanvasGesture>,
    mut intents: MessageWriter<PersonIntent>,
    mut scene: Scene,
) {
    // A panel under the pointer (the inspector) has the pointer, not the world.
    if hovered.is_some() {
        clicks.clear();
        starts.clear();
        drags.clear();
        ends.clear();
        return;
    }

    for click in clicks.read() {
        if click.event.button != PointerButton::Primary {
            continue;
        }
        if let Some(owner) = scene.owner(click.entity) {
            flush_unless(&mut gesture, Some(owner), &scene, &mut intents);
            if selection.entity != Some(owner) {
                selection.entity = Some(owner);
            }
        } else if scene.windows.contains(click.entity) {
            // Nothing under the pointer but the window itself.
            flush_unless(&mut gesture, None, &scene, &mut intents);
            if selection.entity.is_some() {
                selection.entity = None;
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
        if selection.entity != Some(owner) {
            selection.entity = Some(owner);
        }
        if let Some(g) = gesture_on(&mut gesture, owner, &scene, &mut intents) {
            g.drag = Some(Grab {
                origin,
                normal,
                offset: origin - hit,
                vertical,
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
        }
        let entity = g.entity;
        g.moved = true;
        scene.place(entity, target);
    }

    for _ in ends.read() {
        if gesture.0.as_ref().is_some_and(|g| g.drag.is_some())
            && let Some(done) = gesture.0.take()
        {
            intents.write(done.finish(&scene.transforms));
        }
    }
}

/// Keys edit the selection; a run of presses is one gesture.
fn key_edits(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut egui: EguiContexts,
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
            if selection.entity.is_some() {
                selection.entity = None;
            }
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

fn key_edit(
    keys: &ButtonInput<KeyCode>,
    command: bool,
    entity: Entity,
    selection: &mut InspectorSelection,
    gesture: &mut CanvasGesture,
    intents: &mut MessageWriter<PersonIntent>,
    scene: &mut Scene,
) {
    if keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        // The delete supersedes a gesture on the same entity.
        if gesture.0.as_ref().is_some_and(|g| g.entity == entity) {
            gesture.0 = None;
        }
        flush_unless(gesture, None, scene, intents);
        if let Some((id, name)) = scene.identify(entity) {
            intents.write(PersonIntent::Delete { id, name });
        }
        selection.entity = None;
        return;
    }
    if command {
        if keys.just_pressed(KeyCode::KeyD) {
            flush_unless(gesture, None, scene, intents);
            if let Some((id, _)) = scene.identify(entity) {
                intents.write(PersonIntent::Duplicate { id });
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
            nudge += step * NUDGE_METERS;
        }
    }
    let turn = if keys.just_pressed(KeyCode::Comma) {
        TURN_DEGREES
    } else if keys.just_pressed(KeyCode::Period) {
        -TURN_DEGREES
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

    let local_nudge = scene.local_vector(entity, nudge);
    let Some(g) = gesture_on(gesture, entity, scene, intents) else {
        return;
    };
    g.idle.reset();
    g.moved |= nudge != Vec3::ZERO;
    g.yaw_degrees += turn;
    g.scale_by *= scale;
    if let Ok(mut transform) = scene.transforms.get_mut(entity) {
        transform.translation += local_nudge;
        // About the parent's vertical, which is what the committed angles say.
        transform.rotation = Quat::from_rotation_y(turn.to_radians()) * transform.rotation;
        transform.scale *= scale;
    }
}

/// A wire box around the selection — around everything under it, since its
/// children move with it.
fn draw_selection(
    mut gizmos: Gizmos,
    selection: Res<InspectorSelection>,
    globals: Query<&GlobalTransform>,
    bounds: Query<&Aabb>,
    children: Query<&Children>,
) {
    let Some(root) = selection.entity else {
        return;
    };
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
            SELECTED,
        );
    } else if let Ok(global) = globals.get(root) {
        // Nothing with a mesh — a light, an empty: mark where it is.
        gizmos.sphere(
            Isometry3d::from_translation(global.translation()),
            0.25,
            SELECTED,
        );
    }
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
}
