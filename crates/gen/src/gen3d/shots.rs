//! Shot awareness in the live canvas: look through any shot, the shot
//! list with a thumbnail per shot, revise a shot's place by drag.
//!
//! The canvas holds a `.world` whose camera entities carry
//! `ext-cinematography` (the previs crate stages them). This module is
//! what a person does with that in the window:
//!
//! - **Look through** — press a shot's number (1–9), or ask with
//!   [`LookThrough`], and the free camera becomes the shot's camera:
//!   placed by the entity's transform (`aim` wins), vertical FOV and
//!   frame aspect from the extension's derived frame ([`shot_camera`],
//!   the same math the board and the conformance suite use),
//!   letterboxed to the frame. Esc returns the free camera as it was.
//! - **The shot list** — [`LiveShots`], the cameras with a `shot` in
//!   `shot.order` (ties by entity id), each with a thumbnail rendered
//!   through the offscreen path ([`ShotThumbs`]): what the app's
//!   timeline draws as its shot track. A reorder ([`reorder_ops`])
//!   goes through the authority as `ModifyEntity` patches on the
//!   ext-cinematography block — no new op kinds, one history entry,
//!   undoable like any edit.
//!
//! None of it owns state a panel could get out of step with: the list
//! is a snapshot the canvas publishes when the document on screen
//! changes (the same cadence as [`super::live::LiveHistory`]), and a
//! panel asks with messages — [`LookThrough`], [`ShotOps`] — which the
//! canvas serves, so a panel and the API cannot behave differently.
//!
//! [`shot_camera`]: localgpt_world_bevy::camera::shot_camera

use bevy::prelude::*;
use localgpt_world_bevy::camera::{self, ShotCamera};
use localgpt_world_types as wt;
use serde_json::{Value, json};

use super::offscreen;
use super::plugin::FlyCam;

/// The plugin: the canvas's shot systems. Registered by
/// [`super::live::setup_live`] — a world without the live authority
/// has no shot list.
pub struct ShotsPlugin;

impl Plugin for ShotsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LiveShots>()
            .init_resource::<ShotView>()
            .init_resource::<ShotThumbs>()
            .init_resource::<ThumbsDirty>()
            .add_message::<LookThrough>()
            .add_message::<ShotOps>()
            .add_systems(
                Update,
                (
                    look_through_keys.run_if(super::avatar::in_freefly_mode),
                    apply_look_through,
                    drive_shot_view,
                    sync_shot_thumbs,
                )
                    .chain(),
            );
    }
}

/// The shot list's snapshot: the shots of the document on screen, in
/// `shot.order`, published by the canvas when that document changes.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct LiveShots {
    pub shots: Vec<ShotEntry>,
    /// The document the shots came from — what resolves a shot's view
    /// (`aim_entity`, origins through parents) when the canvas looks
    /// through it or renders its thumbnail.
    pub manifest: wt::WorldManifest,
}

impl Default for LiveShots {
    fn default() -> Self {
        Self {
            shots: Vec::new(),
            manifest: wt::WorldManifest::new(""),
        }
    }
}

/// One shot: the camera entity as the document has it, and the derived
/// row (name, size, lens, frame, clock) a shot track draws.
#[derive(Debug, Clone, PartialEq)]
pub struct ShotEntry {
    pub entity: wt::WorldEntity,
    pub row: localgpt_previs::ShotRow,
}

/// Set whenever the canvas applies ops — a commit, a seek, a replay
/// step — so thumbnails re-render even when the shot list itself is
/// unchanged (a prop moved inside a frame).
#[derive(Resource, Default)]
pub struct ThumbsDirty(pub bool);

/// Ask the canvas to look through a shot's camera entity (`None`: back
/// to the free camera — Esc). A panel and the number keys send these.
#[derive(Message, Debug, Clone)]
pub struct LookThrough(pub Option<u64>);

/// A shot-list edit from a panel (a drag to reorder), committed by
/// the canvas through the authority at the entry on screen — the same
/// path a person's gesture takes.
#[derive(Message, Debug, Clone)]
pub struct ShotOps {
    pub ops: Value,
    pub what: String,
}

/// The look-through state: which shot owns the camera, and the free
/// camera as it was, to hand back on Esc.
#[derive(Resource, Default)]
pub struct ShotView {
    /// The world entity being looked through.
    pub active: Option<u64>,
    saved: Option<SavedCamera>,
}

struct SavedCamera {
    transform: Transform,
    projection: Projection,
    viewport: Option<bevy::camera::Viewport>,
}

/// Run condition for the fly camera's own systems: it owns the camera
/// only while no shot does.
pub fn not_looking_through(view: Option<Res<ShotView>>) -> bool {
    view.is_none_or(|v| v.active.is_none())
}

/// The thumbnail size for a frame aspect: 384 px wide, the frame's
/// shape, capped so a portrait frame doesn't run away.
fn thumb_size(frame_aspect: f32) -> (u32, u32) {
    const WIDTH: u32 = 384;
    let height = (WIDTH as f32 / frame_aspect.max(0.1)).round() as u32;
    (WIDTH, height.clamp(1, 768))
}

// ---------------------------------------------------------------------------
// Look-through
// ---------------------------------------------------------------------------

/// The keys: a shot's number looks through it, Esc hands the camera
/// back. (A panel's shot cards are the mouse form of the same asks.)
fn look_through_keys(
    keys: Res<ButtonInput<KeyCode>>,
    shots: Res<LiveShots>,
    view: Res<ShotView>,
    mut looks: MessageWriter<LookThrough>,
) {
    if keys.just_pressed(KeyCode::Escape) && view.active.is_some() {
        looks.write(LookThrough(None));
        return;
    }
    const DIGITS: [KeyCode; 9] = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    for (index, key) in DIGITS.into_iter().enumerate() {
        if keys.just_pressed(key)
            && let Some(entry) = shots.shots.get(index)
        {
            looks.write(LookThrough(Some(entry.row.entity_id)));
        }
    }
}

/// The camera the shot asks for, applied: transform and projection
/// from the extension's math, the viewport letterboxed to the frame.
fn apply_shot(
    shot: &ShotCamera,
    transform: &mut Transform,
    projection: &mut Projection,
    camera: &mut Camera,
    window: Option<&Window>,
) {
    *transform = shot.transform;
    *projection = Projection::Perspective(shot.projection.clone());
    camera.viewport = window.and_then(|w| {
        camera::letterbox(w.physical_size(), shot.frame.aspect as f32).map(|(pos, size)| {
            bevy::camera::Viewport {
                physical_position: pos,
                physical_size: size,
                ..default()
            }
        })
    });
}

/// Hand the camera back as it was.
fn restore_free_camera(
    view: &mut ShotView,
    camera: &mut Query<(&mut Transform, &mut Projection, &mut Camera), With<FlyCam>>,
) {
    view.active = None;
    if let (Some(saved), Ok((mut transform, mut projection, mut camera))) =
        (view.saved.take(), camera.single_mut())
    {
        *transform = saved.transform;
        *projection = saved.projection;
        camera.viewport = saved.viewport;
    }
}

/// Serve the asks: entering saves the free camera once, leaving
/// restores it.
fn apply_look_through(
    mut looks: MessageReader<LookThrough>,
    mut view: ResMut<ShotView>,
    shots: Res<LiveShots>,
    mut camera: Query<(&mut Transform, &mut Projection, &mut Camera), With<FlyCam>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    for look in looks.read() {
        let Some(id) = look.0 else {
            restore_free_camera(&mut view, &mut camera);
            continue;
        };
        let shot = shots
            .shots
            .iter()
            .find(|e| e.row.entity_id == id)
            .and_then(|e| camera::shot_camera_in(&e.entity, &shots.manifest));
        let (Some(shot), Ok((mut transform, mut projection, mut camera))) =
            (shot, camera.single_mut())
        else {
            continue;
        };
        if view.active.is_none() {
            view.saved = Some(SavedCamera {
                transform: *transform,
                projection: projection.clone(),
                viewport: camera.viewport.clone(),
            });
        }
        view.active = Some(id);
        apply_shot(
            &shot,
            &mut transform,
            &mut projection,
            &mut camera,
            windows.single().ok(),
        );
    }
}

/// While a shot owns the camera it is re-derived every frame: the doc
/// may move the shot (an agent revising it, a seek), and the window
/// may resize under the letterbox. A shot the document on screen no
/// longer has hands the camera back.
fn drive_shot_view(
    mut view: ResMut<ShotView>,
    shots: Res<LiveShots>,
    mut camera: Query<(&mut Transform, &mut Projection, &mut Camera), With<FlyCam>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
) {
    let Some(id) = view.active else {
        return;
    };
    let shot = shots
        .shots
        .iter()
        .find(|e| e.row.entity_id == id)
        .and_then(|e| camera::shot_camera_in(&e.entity, &shots.manifest));
    match shot {
        Some(shot) => {
            if let Ok((mut transform, mut projection, mut camera)) = camera.single_mut() {
                apply_shot(
                    &shot,
                    &mut transform,
                    &mut projection,
                    &mut camera,
                    windows.single().ok(),
                );
            }
        }
        None => restore_free_camera(&mut view, &mut camera),
    }
}

// ---------------------------------------------------------------------------
// Thumbnails
// ---------------------------------------------------------------------------

/// One thumbnail camera per shot, rendered through the offscreen path
/// into an image; active for a couple of frames whenever something the
/// frame shows could have changed.
#[derive(Resource, Default)]
pub struct ShotThumbs {
    thumbs: Vec<Thumb>,
}

impl ShotThumbs {
    /// The rendered thumbnail of a shot's camera entity, when there is one.
    pub fn image_of(&self, entity_id: u64) -> Option<Handle<Image>> {
        self.thumbs
            .iter()
            .find(|t| t.entity_id == entity_id)
            .map(|t| t.image.clone())
    }
}

struct Thumb {
    /// The shot's world entity id.
    entity_id: u64,
    /// The offscreen camera.
    camera: Entity,
    image: Handle<Image>,
    /// The document entity the last render was placed from.
    doc_entity: wt::WorldEntity,
    /// Where the camera stood for it — a shot that tracks an actor
    /// moves when the actor does, with its own entity unchanged.
    placed: Transform,
    frames_left: u8,
}

fn sync_shot_thumbs(
    mut commands: Commands,
    shots: Res<LiveShots>,
    mut dirty: ResMut<ThumbsDirty>,
    mut thumbs: ResMut<ShotThumbs>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<&mut Camera>,
) {
    if shots.is_changed() {
        let want: Vec<u64> = shots.shots.iter().map(|e| e.row.entity_id).collect();
        thumbs.thumbs.retain(|t| {
            let keep = want.contains(&t.entity_id);
            if !keep {
                commands.entity(t.camera).despawn();
            }
            keep
        });
        for entry in &shots.shots {
            let id = entry.row.entity_id;
            // The view resolved in the document on screen: a shot that
            // tracks an actor stands where the actor is now.
            let Some(shot) = camera::shot_camera_in(&entry.entity, &shots.manifest) else {
                continue;
            };
            match thumbs.thumbs.iter_mut().find(|t| t.entity_id == id) {
                // Placed as before: nothing to re-place (what moved
                // inside the frame is the dirty flag's business).
                Some(thumb)
                    if thumb.doc_entity == entry.entity && thumb.placed == shot.transform => {}
                // The shot changed, or its actor moved it: re-place, re-render.
                Some(thumb) => {
                    thumb.doc_entity = entry.entity.clone();
                    thumb.placed = shot.transform;
                    thumb.frames_left = thumb.frames_left.max(2);
                    commands
                        .entity(thumb.camera)
                        .insert((shot.transform, Projection::Perspective(shot.projection)));
                }
                None => {
                    let (w, h) = thumb_size(shot.frame.aspect as f32);
                    let image = offscreen::create_offscreen_image(&mut images, w, h);
                    let camera = commands
                        .spawn((
                            Camera3d::default(),
                            Camera {
                                is_active: false,
                                ..default()
                            },
                            bevy::camera::RenderTarget::Image(image.clone().into()),
                            Projection::Perspective(shot.projection),
                            shot.transform,
                            Name::new(format!("shot thumb {}", entry.row.shot)),
                        ))
                        .id();
                    thumbs.thumbs.push(Thumb {
                        entity_id: id,
                        camera,
                        image,
                        doc_entity: entry.entity.clone(),
                        placed: shot.transform,
                        frames_left: 3,
                    });
                }
            }
        }
    }
    // Anything in a frame changed: re-render them all.
    if dirty.0 {
        dirty.0 = false;
        for thumb in &mut thumbs.thumbs {
            thumb.frames_left = thumb.frames_left.max(2);
        }
    }
    for thumb in &mut thumbs.thumbs {
        if thumb.frames_left > 0 {
            thumb.frames_left -= 1;
            if let Ok(mut camera) = cameras.get_mut(thumb.camera) {
                camera.is_active = thumb.frames_left > 0;
            }
        }
    }
}

/// The `ModifyEntity` ops a drag makes: the shots whose place changed,
/// each a patch carrying its full ext-cinematography block with
/// `shot.order` renumbered 1..=n (an `ext-*` patch replaces the whole
/// extension value, so the camera block rides along unchanged).
/// `zone` is the gap the row was dropped on: 0 before the first row,
/// `shots.len()` after the last. `None` when the drag changes nothing.
pub fn reorder_ops(entries: &[ShotEntry], from: usize, zone: usize) -> Option<Value> {
    let n = entries.len();
    if from >= n || zone > n || zone == from || zone == from + 1 {
        return None;
    }
    let mut ids: Vec<u64> = entries.iter().map(|e| e.row.entity_id).collect();
    let id = ids.remove(from);
    ids.insert(if zone > from { zone - 1 } else { zone }, id);
    let ops: Vec<Value> = ids
        .iter()
        .enumerate()
        .filter_map(|(place, id)| {
            let order = place as u64 + 1;
            let entry = entries.iter().find(|e| e.row.entity_id == *id)?;
            (u64::from(entry.row.order) != order).then(|| {
                json!({"ModifyEntity": {"id": id, "patch": {
                    wt::cinematography::EXTENSION_NAME: ext_block_with_order(&entry.entity, order),
                }}})
            })
        })
        .collect();
    (!ops.is_empty()).then(|| json!(ops))
}

/// An entity's ext-cinematography block with `shot.order` set, the
/// rest of the block carried over unchanged.
fn ext_block_with_order(entity: &wt::WorldEntity, order: u64) -> Value {
    let mut ext = entity
        .extra
        .get(wt::cinematography::EXTENSION_NAME)
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(obj) = ext.as_object_mut() {
        let shot = obj.entry("shot").or_insert_with(|| json!({}));
        if let Some(shot) = shot.as_object_mut() {
            shot.insert("order".into(), json!(order));
        }
    }
    ext
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shot entry with just an order and a camera block, as the
    /// strip would publish it.
    fn entry(id: u64, name: &str, order: u32) -> ShotEntry {
        let entity: wt::WorldEntity = serde_json::from_value(json!({
            "id": id,
            "name": name,
            "transform": {"position": [0.0, 1.6, 6.0]},
            "ext-cinematography": {
                "camera": {"focal_length_mm": 50, "aim": [0.0, 1.6, 0.0]},
                "shot": {"scene": "1", "order": order, "size": "MS"},
            },
        }))
        .unwrap();
        let row = localgpt_previs::shots::shot_list(&wt::WorldManifest {
            entities: vec![entity.clone()],
            ..wt::WorldManifest::new("test")
        })
        .remove(0);
        ShotEntry { entity, row }
    }

    /// The (id, order) pairs a reorder op patches, in op order.
    fn patched(ops: &Value) -> Vec<(u64, u64)> {
        ops.as_array()
            .unwrap()
            .iter()
            .map(|op| {
                let m = &op["ModifyEntity"];
                let order = m["patch"]["ext-cinematography"]["shot"]["order"]
                    .as_u64()
                    .unwrap();
                (m["id"].as_u64().unwrap(), order)
            })
            .collect()
    }

    #[test]
    fn a_drag_renumbers_only_the_shots_whose_place_changed() {
        let entries = vec![entry(1, "1A", 1), entry(2, "1B", 2), entry(3, "1C", 3)];
        // Drag the first after the last: 1B, 1C, 1A — all three move.
        let ops = reorder_ops(&entries, 0, 3).unwrap();
        assert_eq!(patched(&ops), [(2, 1), (3, 2), (1, 3)]);
        // The patch carries the whole extension block, camera included.
        assert_eq!(
            ops[0]["ModifyEntity"]["patch"]["ext-cinematography"]["camera"]["focal_length_mm"],
            json!(50)
        );
        // Drag the last up one: only 1B and 1C change places.
        let ops = reorder_ops(&entries, 2, 1).unwrap();
        assert_eq!(patched(&ops), [(3, 2), (2, 3)]);
    }

    #[test]
    fn a_drag_back_where_it_was_changes_nothing() {
        let entries = vec![entry(1, "1A", 1), entry(2, "1B", 2)];
        assert!(reorder_ops(&entries, 0, 0).is_none());
        assert!(
            reorder_ops(&entries, 0, 1).is_none(),
            "the gap after itself"
        );
        assert!(reorder_ops(&entries, 1, 2).is_none());
        assert!(reorder_ops(&entries, 5, 0).is_none(), "not a row");
        assert!(reorder_ops(&entries, 0, 3).is_none(), "not a gap");
    }

    #[test]
    fn thumbnail_size_follows_the_frame_and_is_bounded() {
        assert_eq!(thumb_size(2.39), (384, 161));
        assert_eq!(thumb_size(1.0), (384, 384));
        // A portrait frame is capped rather than towering.
        assert_eq!(thumb_size(0.5), (384, 768));
        assert_eq!(thumb_size(0.0).1, 768);
    }
}
