//! World Info bar — compact bottom bar showing global world state, and in
//! a live world the canvas's controls: snapping, review of agent edits,
//! and camera bookmarks (a `bookmarks` tour in the document, so they travel
//! with the world).
//!
//! With the `feathers` feature the bar is `world_bar.rs` (Bevy's own
//! widgets) and only the bookmark helpers here are used.

#![cfg_attr(feature = "feathers", allow(dead_code))]

use bevy::prelude::*;
use bevy_egui::egui;
use serde_json::{Value, json};

use crate::gen3d::audio::AudioEngine;
use crate::gen3d::behaviors::BehaviorState;
use crate::gen3d::canvas_edit::{CanvasSettings, PersonIntent};
use crate::gen3d::herdr::AgentPresence;
use crate::gen3d::plugin::CurrentWorld;
use crate::gen3d::registry::NameRegistry;

use super::{PanelRects, PendingCameraPose};

/// The tour bookmarks are kept in.
pub const BOOKMARKS: &str = "bookmarks";

const REFUSED: egui::Color32 = egui::Color32::from_rgb(235, 120, 100);

/// The live canvas's controls, present when a live world is open.
pub(crate) struct LiveControls<'a> {
    pub live: &'a mut crate::gen3d::live::Live,
    pub settings: Option<&'a mut CanvasSettings>,
    pub intents: &'a mut Vec<PersonIntent>,
    /// The fly camera now, for a bookmark.
    pub camera: Option<Transform>,
    pub pose: &'a mut PendingCameraPose,
    /// herdr's pane, when herdr runs.
    pub presence: Option<&'a AgentPresence>,
    /// The bar's widgets by name, for `GET /widgets`.
    pub rects: &'a mut PanelRects,
    /// The canvas clock, for "waiting 2 m".
    pub now: f64,
}

// egui 0.34 deprecates `Panel::show(ctx)` in favour of `show_inside(ui)`, but
// bevy_egui only exposes a `Context` (not a root `Ui`), so the context-level
// `show` is still the correct entry point here.
#[allow(deprecated)]
pub(crate) fn draw_world_info(
    ctx: &egui::Context,
    registry: &NameRegistry,
    behavior_state: &BehaviorState,
    audio_engine: Option<&AudioEngine>,
    current_world: &CurrentWorld,
    mut controls: Option<LiveControls<'_>>,
) -> egui::Rect {
    let panel = egui::Panel::bottom("inspector_world_info")
        .exact_size(28.0)
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                // World name
                let world_name = current_world.name.as_deref().unwrap_or("(unnamed world)");
                ui.label(egui::RichText::new(world_name).strong().size(12.0));

                ui.separator();

                // Entity count
                ui.label(egui::RichText::new(format!("{} entities", registry.len())).size(12.0));

                ui.separator();

                // Behavior state
                let beh_status = if behavior_state.paused {
                    "paused"
                } else {
                    "running"
                };
                ui.label(
                    egui::RichText::new(format!(
                        "behaviors: {} {:.1}s",
                        beh_status, behavior_state.elapsed
                    ))
                    .size(12.0),
                );

                ui.separator();

                // Audio state
                if let Some(engine) = audio_engine {
                    let audio_status = if engine.active { "ON" } else { "OFF" };
                    let emitter_count = engine.emitter_meta.len();
                    ui.label(
                        egui::RichText::new(format!(
                            "audio: {} {} emitters",
                            audio_status, emitter_count
                        ))
                        .size(12.0),
                    );
                }

                let Some(controls) = controls.as_mut() else {
                    return;
                };
                ui.separator();
                if let Some(settings) = controls.settings.as_deref_mut() {
                    let snap = ui
                        .checkbox(&mut settings.snap, "snap")
                        .on_hover_text("Y toggles; hold Cmd/Ctrl while dragging for the opposite");
                    controls.rects.note("world_info.snap", &snap);
                    let grid = ui.add(
                        egui::DragValue::new(&mut settings.grid_m)
                            .speed(0.05)
                            .range(0.05..=10.0)
                            .suffix(" m"),
                    );
                    controls.rects.note("world_info.grid", &grid);
                    let angle = ui.add(
                        egui::DragValue::new(&mut settings.angle_degrees)
                            .speed(1.0)
                            .range(1.0..=90.0)
                            .suffix("\u{00B0}"),
                    );
                    controls.rects.note("world_info.angle", &angle);
                    let local = ui.checkbox(&mut settings.local_axes, "local").on_hover_text(
                        "L toggles: the handles along the entity's own axes instead of the world's",
                    );
                    controls.rects.note("world_info.local", &local);
                    ui.separator();
                }
                let review = ui
                    .checkbox(&mut controls.live.review, "review")
                    .on_hover_text(
                        "An agent's batch shows on the canvas before it commits: \
                     Enter accepts it, Esc rejects it",
                    );
                controls.rects.note("world_info.review", &review);
                ui.separator();
                bookmarks(ui, controls);
                agent(ui, controls);
                if let Some(status) = controls.live.status() {
                    ui.separator();
                    let text = egui::RichText::new(status).size(12.0);
                    if status.starts_with("refused") {
                        ui.label(text.color(REFUSED));
                    } else {
                        ui.label(text);
                    }
                }
            });
        });
    panel.response.rect
}

/// The bookmarks menu: fly to one, drop one, add the view now.
fn bookmarks(ui: &mut egui::Ui, controls: &mut LiveControls<'_>) {
    let doc = controls.live.doc();
    let tour = doc.tours.iter().find(|t| t.name == BOOKMARKS);
    let count = tour.map_or(0, |t| t.waypoints.len());
    let menu = ui.menu_button(format!("\u{2605} {count}"), |ui| {
        if let Some(tour) = tour {
            for (n, waypoint) in tour.waypoints.iter().enumerate() {
                ui.horizontal(|ui| {
                    let label = waypoint
                        .description
                        .clone()
                        .unwrap_or_else(|| format!("view {}", n + 1));
                    let fly = ui.button(label);
                    controls.rects.note(format!("menu.bookmark.{}", n + 1), &fly);
                    if fly.clicked() {
                        controls.pose.pose = Some((
                            Vec3::from_array(waypoint.position),
                            Vec3::from_array(waypoint.look_at),
                        ));
                    }
                    let drop = ui.small_button("\u{2715}").on_hover_text("Drop this bookmark");
                    controls.rects.note(format!("menu.drop.{}", n + 1), &drop);
                    if drop.clicked() {
                        controls.intents.push(PersonIntent::World {
                            patch: json!({"tours": tours_with(doc, |tours| drop_bookmark(tours, n))}),
                            what: format!("drop bookmark {}", n + 1),
                        });
                    }
                });
            }
            ui.separator();
        }
        let Some(camera) = controls.camera else {
            ui.weak("no camera");
            return;
        };
        let add = ui.button("\u{FF0B} bookmark this view");
        controls.rects.note("menu.bookmark this view", &add);
        if add.clicked() {
            let position = camera.translation;
            let look_at = position + camera.forward() * 5.0;
            controls.intents.push(PersonIntent::World {
                patch: json!({"tours": tours_with(doc, |tours| add_bookmark(tours, position, look_at, count + 1))}),
                what: "bookmark a view".into(),
            });
        }
    });
    controls.rects.note("world_info.bookmarks", &menu.response);
}

/// herdr's pane: a menu to open one in the world folder, then its agent's
/// state — a light when it waits for the person (click: bring the pane to
/// the front). Nothing when herdr isn't running.
fn agent(ui: &mut egui::Ui, controls: &mut LiveControls<'_>) {
    let Some(presence) = controls.presence else {
        return;
    };
    if presence.pane.is_none() && !presence.available() {
        return;
    }
    ui.separator();
    match presence.line(controls.now) {
        Some((line, attention)) => {
            let text = egui::RichText::new(line).size(12.0);
            let text = if attention { text.color(REFUSED) } else { text };
            let response = ui
                .add(egui::Button::new(text).frame(false))
                .on_hover_text("herdr's pane in the world folder; click to bring it to the front");
            if response.clicked() {
                presence.focus();
            }
            controls.rects.note("world_info.agent", &response);
        }
        None => {
            let menu = ui.menu_button("agent", |ui| {
                let shell = ui.button("terminal here");
                controls.rects.note("menu.terminal here", &shell);
                if shell.clicked() {
                    presence.open(None);
                }
                for command in &presence.commands {
                    let item = ui.button(format!("{command} here"));
                    controls.rects.note(format!("menu.{command} here"), &item);
                    if item.clicked() {
                        presence.open(Some(command));
                    }
                }
            });
            controls.rects.note("world_info.agent", &menu.response);
        }
    }
    if let Some(error) = &presence.error {
        ui.label(egui::RichText::new(error).size(11.0).color(REFUSED));
    }
}

/// The document's tours as JSON, after a change — `ModifyWorld` replaces
/// the list whole.
pub(crate) fn tours_with(
    doc: &openworldformat::WorldDoc,
    change: impl FnOnce(&mut Vec<Value>),
) -> Vec<Value> {
    let mut tours: Vec<Value> = doc
        .tours
        .iter()
        .map(|t| serde_json::to_value(t).unwrap_or(Value::Null))
        .collect();
    change(&mut tours);
    tours
}

fn round3(v: Vec3) -> [f32; 3] {
    v.to_array().map(|x| (x * 1000.0).round() / 1000.0)
}

pub(crate) fn add_bookmark(tours: &mut Vec<Value>, position: Vec3, look_at: Vec3, n: usize) {
    let waypoint = json!({
        "position": round3(position),
        "look_at": round3(look_at),
        "description": format!("view {n}"),
        "pause_duration": 0.0,
    });
    match tours.iter_mut().find(|t| t["name"] == BOOKMARKS) {
        Some(tour) => {
            if let Some(waypoints) = tour["waypoints"].as_array_mut() {
                waypoints.push(waypoint);
            }
        }
        None => tours.push(json!({
            "name": BOOKMARKS,
            "description": "views bookmarked in the canvas",
            "waypoints": [waypoint],
        })),
    }
}

pub(crate) fn drop_bookmark(tours: &mut Vec<Value>, n: usize) {
    if let Some(tour) = tours.iter_mut().find(|t| t["name"] == BOOKMARKS)
        && let Some(waypoints) = tour["waypoints"].as_array_mut()
        && n < waypoints.len()
    {
        waypoints.remove(n);
    }
    // An empty bookmarks tour is no tour.
    tours.retain(|t| {
        !(t["name"] == BOOKMARKS && t["waypoints"].as_array().is_some_and(|w| w.is_empty()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_live_in_one_tour_and_leave_when_the_last_goes() {
        let mut tours = vec![json!({"name": "grand", "waypoints": []})];
        add_bookmark(&mut tours, Vec3::new(1.0, 2.0, 3.0), Vec3::ZERO, 1);
        add_bookmark(&mut tours, Vec3::new(4.0, 5.0, 6.0), Vec3::ZERO, 2);
        assert_eq!(tours.len(), 2);
        assert_eq!(tours[1]["waypoints"].as_array().unwrap().len(), 2);
        assert_eq!(tours[1]["waypoints"][1]["description"], "view 2");
        drop_bookmark(&mut tours, 0);
        assert_eq!(tours[1]["waypoints"][0]["position"], json!([4.0, 5.0, 6.0]));
        drop_bookmark(&mut tours, 0);
        assert_eq!(tours.len(), 1, "the other tour stays: {tours:?}");
    }
}
