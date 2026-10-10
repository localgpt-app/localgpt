//! Shot awareness in the live canvas: look through any shot, a shot
//! strip, revise a shot's place by drag.
//!
//! The canvas holds a `.world` whose camera entities carry
//! `ext-cinematography` (the previs crate stages them). This module is
//! what a person does with that in the window:
//!
//! - **Look through** — click a shot card (or press its number, 1–9)
//!   and the free camera becomes the shot's camera: placed by the
//!   entity's transform (`aim` wins), vertical FOV and frame aspect
//!   from the extension's derived frame ([`shot_camera`], the same
//!   math the board and the conformance suite use), letterboxed to the
//!   frame. Esc returns the free camera as it was.
//! - **The strip** — an egui window listing the cameras with a `shot`,
//!   in `shot.order` (ties by entity id): name, size, lens, and a
//!   thumbnail rendered through the offscreen path. A drag to reorder
//!   goes through the authority as `ModifyEntity` patches on the
//!   ext-cinematography block — no new op kinds, one history entry,
//!   undoable like any edit.
//! - **Boards** — buttons open the authority's `GET /board` in the
//!   browser: the tip on screen as a printable board, or two tips
//!   (the one on screen vs the head) side by side.
//!
//! The strip owns no state: it draws [`LiveShots`], a snapshot the
//! canvas publishes when the document on screen changes (the same
//! cadence as the history rail's [`super::live::LiveHistory`]), and
//! asks with messages — [`LookThrough`], [`ShotOps`] — which the
//! canvas serves, so the strip and the API cannot behave differently.
//!
//! [`shot_camera`]: localgpt_world_bevy::camera::shot_camera

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiTextureHandle, egui};
use localgpt_world_bevy::camera::{self, ShotCamera};
use localgpt_world_types as wt;
use serde_json::{Value, json};

use super::live::{LiveEndpoint, LiveHistory};
use super::offscreen;
use super::plugin::FlyCam;

/// The plugin: the canvas's shot systems. Registered by
/// [`super::live::setup_live`] — a world without the live authority
/// has no shot strip.
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
                    look_through_keys.run_if(
                        super::avatar::in_freefly_mode.and_then(crate::inspector::not_ui_hovered),
                    ),
                    apply_look_through,
                    drive_shot_view,
                    sync_shot_thumbs,
                    shot_strip_ui,
                )
                    .chain(),
            );
    }
}

/// The strip's snapshot: the shots of the document on screen, in
/// `shot.order`, published by the canvas when that document changes.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct LiveShots {
    pub shots: Vec<ShotEntry>,
}

/// One shot: the camera entity as the document has it, and the derived
/// row (name, size, lens, frame, clock) the strip draws.
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
/// to the free camera — Esc). The strip and the number keys send these.
#[derive(Message, Debug, Clone)]
pub struct LookThrough(pub Option<u64>);

/// A shot-list edit from the strip (a drag to reorder), committed by
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
/// back. (The strip's cards are the mouse form of the same asks.)
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
            .and_then(|e| camera::shot_camera(&e.entity));
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
        .and_then(|e| camera::shot_camera(&e.entity));
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
/// into an image egui shows; active for a couple of frames whenever
/// something the frame shows could have changed.
#[derive(Resource, Default)]
pub struct ShotThumbs {
    thumbs: Vec<Thumb>,
}

struct Thumb {
    /// The shot's world entity id.
    entity_id: u64,
    /// The offscreen camera.
    camera: Entity,
    image: Handle<Image>,
    texture: Option<egui::TextureId>,
    /// The document entity the last render was placed from.
    doc_entity: wt::WorldEntity,
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
            if let Some(thumb) = thumbs.thumbs.iter().find(|t| t.entity_id == id)
                && thumb.doc_entity == entry.entity
            {
                continue;
            }
            let Some(shot) = camera::shot_camera(&entry.entity) else {
                continue;
            };
            match thumbs.thumbs.iter_mut().find(|t| t.entity_id == id) {
                // The shot itself changed: re-place, re-render.
                Some(thumb) => {
                    thumb.doc_entity = entry.entity.clone();
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
                        texture: None,
                        doc_entity: entry.entity.clone(),
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

// ---------------------------------------------------------------------------
// The strip
// ---------------------------------------------------------------------------

/// One drop gap: before the first row, or after row `zone - 1`.
fn strip_drop_zone(ui: &mut egui::Ui, zone: usize, drop: &mut Option<(usize, usize)>) {
    let (_inner, payload) = ui.dnd_drop_zone::<usize, _>(
        egui::Frame::new().inner_margin(egui::Margin::symmetric(4, 1)),
        |ui| {
            ui.allocate_space(egui::vec2(ui.available_width(), 4.0));
        },
    );
    if let Some(from) = payload {
        *drop = Some((*from, zone));
    }
}

/// The strip: the shots in order, click or number to look through,
/// drag to reorder, boards at the bottom.
#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn shot_strip_ui(
    mut contexts: EguiContexts,
    shots: Res<LiveShots>,
    view: Res<ShotView>,
    mut thumbs: ResMut<ShotThumbs>,
    mut looks: MessageWriter<LookThrough>,
    mut edits: MessageWriter<ShotOps>,
    history: Option<Res<LiveHistory>>,
    endpoint: Option<Res<LiveEndpoint>>,
) {
    if shots.shots.is_empty() {
        return;
    }
    // Register the thumbnail textures before borrowing the context.
    for thumb in &mut thumbs.thumbs {
        thumb.texture.get_or_insert_with(|| {
            contexts.add_image(EguiTextureHandle::Strong(thumb.image.clone()))
        });
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    egui::Window::new("Shots")
        .default_width(320.0)
        .show(ctx, |ui| {
            let mut drop = None;
            strip_drop_zone(ui, 0, &mut drop);
            for index in 0..shots.shots.len() {
                shot_row(ui, &shots, index, &thumbs, &view, &mut looks);
                strip_drop_zone(ui, index + 1, &mut drop);
            }
            if let Some((from, zone)) = drop
                && let Some(ops) = reorder_ops(&shots.shots, from, zone)
            {
                let name = &shots.shots[from].row.shot;
                edits.write(ShotOps {
                    ops,
                    what: format!("reorder shot {name}"),
                });
            }
            ui.separator();
            ui.label(
                egui::RichText::new(
                    "click to look through · Esc back · 1–9 jump · drag to reorder",
                )
                .small()
                .color(egui::Color32::GRAY),
            );
            if let Some(endpoint) = &endpoint {
                board_buttons(ui, endpoint, history.as_deref());
            }
        });
}

/// One shot's card: thumbnail, name and size, lens and clock,
/// description; a drag source for reordering, a click to look through.
fn shot_row(
    ui: &mut egui::Ui,
    shots: &LiveShots,
    index: usize,
    thumbs: &ShotThumbs,
    view: &ShotView,
    looks: &mut MessageWriter<LookThrough>,
) {
    let entry = &shots.shots[index];
    let row = &entry.row;
    let active = view.active == Some(row.entity_id);
    let fill = if active {
        egui::Color32::from_rgba_premultiplied(60, 80, 120, 200)
    } else {
        egui::Color32::from_rgba_premultiplied(40, 40, 40, 200)
    };
    let aspect = row.frame_aspect as f32;
    let size = egui::vec2(120.0, 120.0 / aspect);
    let texture = thumbs
        .thumbs
        .iter()
        .find(|t| t.entity_id == row.entity_id)
        .and_then(|t| t.texture);

    let inner = ui.dnd_drag_source(egui::Id::new(("shot-row", row.entity_id)), index, |ui| {
        egui::Frame::new()
            .fill(fill)
            .corner_radius(4.0)
            .inner_margin(6.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(texture) = texture {
                        ui.add(egui::Image::new(egui::load::SizedTexture::new(
                            texture, size,
                        )));
                    } else {
                        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                        ui.painter()
                            .rect_filled(rect, 2.0, egui::Color32::from_gray(24));
                    }
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.strong(format!("{}. {}", index + 1, row.shot));
                            ui.label(
                                egui::RichText::new(&row.size).color(egui::Color32::LIGHT_BLUE),
                            );
                        });
                        ui.label(format!(
                            "{} mm · {:.2}:1 · {}–{} s",
                            localgpt_previs::shots::focal(row.focal_length_mm),
                            row.frame_aspect,
                            trim_seconds(row.in_s),
                            trim_seconds(row.out_s),
                        ));
                        if !row.description.is_empty() {
                            ui.label(
                                egui::RichText::new(&row.description)
                                    .small()
                                    .color(egui::Color32::GRAY),
                            );
                        }
                    });
                });
            });
    });
    if inner.response.interact(egui::Sense::click()).clicked() {
        looks.write(LookThrough(Some(row.entity_id)));
    }
}

/// `4.50` → `4.5`, for the clock on a card.
fn trim_seconds(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The board buttons: this tip's board, or two tips side by side in
/// the browser (the authority's `GET /board`).
fn board_buttons(ui: &mut egui::Ui, endpoint: &LiveEndpoint, history: Option<&LiveHistory>) {
    let current = history.and_then(|h| h.current.as_deref());
    let head = history.and_then(|h| h.rows.iter().find(|r| r.head).map(|r| r.id.as_str()));
    ui.horizontal(|ui| {
        if ui.button("Board ↗").clicked() {
            open_in_browser(&board_url(endpoint, current, None));
        }
        // The treatment on screen against main — the everyday compare.
        let comparable = head.is_some() && head != current;
        if ui
            .add_enabled(comparable, egui::Button::new("Compare with head ↗"))
            .on_hover_text("the entry on screen vs the head, side by side")
            .clicked()
        {
            open_in_browser(&board_url(endpoint, current, head));
        }
    });
}

/// A `/board` URL on the authority, token included (the API takes it
/// as a query parameter, which is what a browser can send).
fn board_url(endpoint: &LiveEndpoint, tip: Option<&str>, vs: Option<&str>) -> String {
    let mut url = format!("{}/board?token={}", endpoint.url, endpoint.token);
    if let Some(tip) = tip {
        url.push_str(&format!("&tip={tip}"));
    }
    if let Some(vs) = vs {
        url.push_str(&format!("&vs={vs}"));
    }
    url
}

/// Open a URL in the person's browser, best effort; the address is
/// logged either way (the authority is localhost, the token already
/// sits in `.live/endpoint.json`).
fn open_in_browser(url: &str) {
    eprintln!("[shots] {url}");
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener).arg(url).spawn();
}

// ---------------------------------------------------------------------------
// Reordering
// ---------------------------------------------------------------------------

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
