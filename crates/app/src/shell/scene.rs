//! The world's things: an outliner over an inspector (`crates/app/ui/src/
//! scene/` in the page). The tree is read from the world's document as the
//! app sends it (`SceneView.entities`, each entity as the format writes
//! it); selecting, focusing and the eye ask the app; on a live canvas a
//! field held is previewed on the canvas and a field let go of is one entry
//! in the history, through the authority the agents use.

use std::collections::{BTreeMap, HashSet};

use eframe::egui::{self, RichText};
use serde_json::{Value, json};

use super::state::State;
use super::theme;
use crate::scene::SceneEdit;
use crate::webui::FromPage;
use crate::webui::link::ShellEnd;

/// What a field is missing reads as, as the page's inspector has it.
fn default_of(path: &str) -> Option<Value> {
    Some(match path {
        "transform.rotation_degrees" => json!([0.0, 0.0, 0.0]),
        "transform.scale" => json!([1.0, 1.0, 1.0]),
        "transform.visible" => json!(true),
        "material.roughness" => json!(0.5),
        "material.metallic" => json!(0.0),
        _ => return None,
    })
}

/// The value at a dotted path (`transform.position.1`), or its default.
fn read(entity: &Value, path: &str) -> Option<Value> {
    let mut here = entity;
    for part in path.split('.') {
        here = match part.parse::<usize>() {
            Ok(i) => here.get(i)?,
            Err(_) => match here.get(part) {
                Some(v) => v,
                None => return read_default(entity, path),
            },
        };
    }
    if here.is_null() {
        return read_default(entity, path);
    }
    Some(here.clone())
}

fn read_default(entity: &Value, path: &str) -> Option<Value> {
    let parts: Vec<&str> = path.split('.').collect();
    if let Some((last, rest)) = parts.split_last()
        && let Ok(axis) = last.parse::<usize>()
    {
        let whole = rest.join(".");
        let array = entity
            .pointer(&format!("/{}", rest.join("/")))
            .filter(|v| v.is_array())
            .cloned()
            .or_else(|| default_of(&whole))?;
        return array.get(axis).cloned();
    }
    default_of(path)
}

/// The merge patch that sets `path` to `value` on `entity`, as
/// `ModifyEntity` takes it; none when the entity has no such field.
fn patch_at(entity: &Value, path: &str, value: Value) -> Option<Value> {
    let parts: Vec<&str> = path.split('.').collect();
    match parts.as_slice() {
        ["name"] => Some(json!({ "name": value })),
        ["transform", "visible"] => Some(json!({ "transform": { "visible": value } })),
        ["transform", key, axis] => {
            let axis: usize = axis.parse().ok()?;
            let mut now = read(entity, &format!("transform.{key}"))?;
            let slot = now.get_mut(axis)?;
            *slot = value;
            Some(json!({ "transform": { *key: now } }))
        }
        ["shape", key] => {
            let (kind, fields) = entity.get("shape")?.as_object()?.iter().next()?;
            let mut fields = fields.clone();
            fields.get(*key)?.as_f64()?;
            fields[*key] = value;
            Some(json!({ "shape": { kind.as_str(): fields } }))
        }
        [part @ ("material" | "light"), key] => {
            entity.get(*part).filter(|v| !v.is_null())?;
            Some(json!({ *part: { *key: value } }))
        }
        _ => None,
    }
}

/// The history's words for an edit, as the page writes them.
fn describe(path: &str, names: &str, value: &Value, was: Option<&Value>) -> String {
    let parts: Vec<&str> = path.split('.').collect();
    let number = |v: Option<&Value>| v.and_then(Value::as_f64).unwrap_or(0.0);
    match parts.as_slice() {
        ["transform", "position", ..] => format!("move {names}"),
        ["transform", "rotation_degrees", ..] => format!("turn {names}"),
        ["transform", "scale", ..] => format!("scale {names}"),
        ["transform", "visible"] => format!(
            "{} {names}",
            if value.as_bool() == Some(true) {
                "show"
            } else {
                "hide"
            }
        ),
        ["shape", ..] => format!("resize {names}"),
        [_, "color"] => format!("recolour {names}"),
        [_, "emissive"] => format!("recolour the glow of {names}"),
        [_, "intensity"] => format!(
            "{} {names}",
            if number(Some(value)) > number(was) {
                "brighten"
            } else {
                "dim"
            }
        ),
        [_, "shadows"] => format!(
            "{} {names}",
            if value.as_bool() == Some(true) {
                "shadows from"
            } else {
                "no shadows from"
            }
        ),
        ["name"] => format!("rename {names}"),
        [_, key] => format!(
            "{} {:.2} on {names}",
            key.replace('_', " "),
            number(Some(value))
        ),
        _ => format!("edit {names}"),
    }
}

fn id_of(entity: &Value) -> Option<u64> {
    entity.get("id")?.as_u64()
}

fn name_of(entity: &Value) -> String {
    entity
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("thing")
        .to_string()
}

/// What a thing is, for its row: a light, a camera, a model, a shape.
fn kind_of(entity: &Value) -> (&'static str, String) {
    if let Some(light) = entity.get("light").filter(|v| !v.is_null()) {
        let kind = light
            .get("light_type")
            .and_then(Value::as_str)
            .unwrap_or("light");
        return ("☀", format!("{kind} light"));
    }
    if entity
        .get("ext-cinematography")
        .is_some_and(|v| !v.is_null())
        || entity.get("camera").is_some_and(|v| !v.is_null())
    {
        return ("🎥", "camera".into());
    }
    if entity.get("mesh_asset").is_some_and(|v| !v.is_null()) {
        return ("▣", "model".into());
    }
    if let Some((shape, _)) = entity
        .get("shape")
        .and_then(Value::as_object)
        .and_then(|o| o.iter().next())
    {
        return ("◆", shape.to_lowercase());
    }
    ("○", "group".into())
}

/// A field being held (a drag on a number): its path, where it started, and
/// the edits shown on the canvas so far.
struct Held {
    path: String,
    from: Value,
    edits: Vec<SceneEdit>,
}

pub struct Scene {
    search: String,
    closed: HashSet<u64>,
    hidden: HashSet<u64>,
    held: Option<Held>,
    /// The name being typed, for the selected thing.
    naming: Option<(u64, String)>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            search: String::new(),
            closed: HashSet::new(),
            hidden: HashSet::new(),
            held: None,
            naming: None,
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, state: &State, end: &ShellEnd) {
        let Some(view) = state.scene.as_deref() else {
            ui.weak("No world open.");
            return;
        };
        let entities: BTreeMap<u64, &Value> = view
            .entities
            .iter()
            .filter_map(|e| Some((id_of(e)?, e)))
            .collect();
        let mut children: BTreeMap<Option<u64>, Vec<u64>> = BTreeMap::new();
        for (id, entity) in &entities {
            let parent = entity
                .get("parent")
                .and_then(Value::as_u64)
                .filter(|p| entities.contains_key(p));
            children.entry(parent).or_default().push(*id);
        }
        ui.horizontal(|ui| {
            ui.strong(format!("{} things", entities.len()));
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Find by name or kind")
                    .desired_width(f32::INFINITY),
            );
        });
        let needle = self.search.trim().to_lowercase();
        let height = (ui.available_height() * 0.5).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("outliner")
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if needle.is_empty() {
                    self.rows(ui, &entities, &children, None, 0, state, end);
                } else {
                    for (id, entity) in &entities {
                        let (_, kind) = kind_of(entity);
                        if name_of(entity).to_lowercase().contains(&needle)
                            || kind.contains(&needle)
                        {
                            self.row(ui, *id, entity, 0, false, state, end);
                        }
                    }
                }
            });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("inspector")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.inspector(ui, &entities, state, view.editable, end)
            });
    }

    #[allow(clippy::too_many_arguments)]
    fn rows(
        &mut self,
        ui: &mut egui::Ui,
        entities: &BTreeMap<u64, &Value>,
        children: &BTreeMap<Option<u64>, Vec<u64>>,
        parent: Option<u64>,
        depth: usize,
        state: &State,
        end: &ShellEnd,
    ) {
        let Some(ids) = children.get(&parent) else {
            return;
        };
        for id in ids {
            let Some(entity) = entities.get(id) else {
                continue;
            };
            let has_children = children.contains_key(&Some(*id));
            self.row(ui, *id, entity, depth, has_children, state, end);
            if has_children && !self.closed.contains(id) {
                self.rows(ui, entities, children, Some(*id), depth + 1, state, end);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        ui: &mut egui::Ui,
        id: u64,
        entity: &Value,
        depth: usize,
        has_children: bool,
        state: &State,
        end: &ShellEnd,
    ) {
        let selected = state.selection.contains(&id);
        let (glyph, kind) = kind_of(entity);
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 14.0);
            if has_children {
                let open = !self.closed.contains(&id);
                if ui.small_button(if open { "▾" } else { "▸" }).clicked() {
                    if open {
                        self.closed.insert(id);
                    } else {
                        self.closed.remove(&id);
                    }
                }
            } else {
                ui.add_space(18.0);
            }
            let hidden = self.hidden.contains(&id);
            let text = RichText::new(format!("{glyph} {}", name_of(entity))).color(if hidden {
                theme::FAINT
            } else {
                theme::INK
            });
            let response = ui.selectable_label(selected, text).on_hover_text(kind);
            if response.clicked() {
                let extend = ui.input(|i| i.modifiers.shift || i.modifiers.command);
                let mut ids = state.selection.clone();
                if extend {
                    if let Some(at) = ids.iter().position(|s| *s == id) {
                        ids.remove(at);
                    } else {
                        ids.push(id);
                    }
                } else {
                    ids = vec![id];
                }
                end.ask(FromPage::Select { ids });
            }
            if response.double_clicked() {
                end.ask(FromPage::Focus { id });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let eye = if hidden { "◌" } else { "◉" };
                if ui
                    .small_button(eye)
                    .on_hover_text(if hidden {
                        "Show in the window"
                    } else {
                        "Hide in the window; the document keeps it"
                    })
                    .clicked()
                {
                    if hidden {
                        self.hidden.remove(&id);
                    } else {
                        self.hidden.insert(id);
                    }
                    end.ask(FromPage::EntityHide {
                        ids: self.hidden.iter().copied().collect(),
                    });
                }
            });
        });
    }

    fn inspector(
        &mut self,
        ui: &mut egui::Ui,
        entities: &BTreeMap<u64, &Value>,
        state: &State,
        editable: bool,
        end: &ShellEnd,
    ) {
        let chosen: Vec<&Value> = state
            .selection
            .iter()
            .filter_map(|id| entities.get(id).copied())
            .collect();
        let Some(primary) = chosen.first().copied() else {
            ui.weak("Select a thing — in the world or above — to see its fields.");
            return;
        };
        let (_, kind) = kind_of(primary);
        let id = id_of(primary).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.strong(&kind);
            if chosen.len() > 1 {
                ui.weak(format!("and {} more", chosen.len() - 1));
            }
            if ui.small_button("Fly to").clicked() {
                end.ask(FromPage::Focus { id });
            }
        });
        if !editable {
            ui.weak("Read only: this world is not a live canvas.");
        }
        ui.add_enabled_ui(editable, |ui| {
            // The name, renamed when the typing is done.
            let name = name_of(primary);
            let typed = match &mut self.naming {
                Some((at, text)) if *at == id => text,
                _ => {
                    self.naming = Some((id, name.clone()));
                    &mut self.naming.as_mut().expect("just set").1
                }
            };
            let response = ui.add(egui::TextEdit::singleline(typed).desired_width(f32::INFINITY));
            if response.lost_focus() && *typed != name && !typed.trim().is_empty() {
                let value = json!(typed.trim());
                self.commit(&chosen[..1], "name", value, end);
            }

            egui::Grid::new("transform").num_columns(4).show(ui, |ui| {
                for (label, key, speed) in [
                    ("Position", "position", 0.05),
                    ("Turn", "rotation_degrees", 1.0),
                    ("Scale", "scale", 0.01),
                ] {
                    ui.label(label);
                    for axis in 0..3 {
                        let path = format!("transform.{key}.{axis}");
                        self.number(ui, &chosen, &path, speed, end);
                    }
                    ui.end_row();
                }
            });
            let visible = read(primary, "transform.visible")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let mut shown = visible;
            if ui.checkbox(&mut shown, "Visible").changed() {
                self.commit(&chosen, "transform.visible", json!(shown), end);
            }

            if let Some((shape, fields)) = primary
                .get("shape")
                .and_then(Value::as_object)
                .and_then(|o| o.iter().next())
            {
                ui.separator();
                ui.label(RichText::new(shape).color(theme::MUTED));
                egui::Grid::new("shape").num_columns(2).show(ui, |ui| {
                    if let Some(fields) = fields.as_object() {
                        for key in fields.keys() {
                            if fields[key].is_number() {
                                ui.label(key.replace('_', " "));
                                self.number(ui, &chosen, &format!("shape.{key}"), 0.01, end);
                                ui.end_row();
                            }
                        }
                    }
                });
            }
            if primary.get("material").is_some_and(|v| !v.is_null()) {
                ui.separator();
                ui.label(RichText::new("Material").color(theme::MUTED));
                self.color(ui, &chosen, "material.color", end);
                egui::Grid::new("material").num_columns(2).show(ui, |ui| {
                    for key in ["roughness", "metallic"] {
                        ui.label(key);
                        self.number(ui, &chosen, &format!("material.{key}"), 0.01, end);
                        ui.end_row();
                    }
                });
            }
            if primary.get("light").is_some_and(|v| !v.is_null()) {
                ui.separator();
                ui.label(RichText::new("Light").color(theme::MUTED));
                self.color(ui, &chosen, "light.color", end);
                egui::Grid::new("light").num_columns(2).show(ui, |ui| {
                    ui.label("intensity");
                    self.number(ui, &chosen, "light.intensity", 10.0, end);
                    ui.end_row();
                });
                let shadows = read(primary, "light.shadows")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let mut on = shadows;
                if ui.checkbox(&mut on, "Shadows").changed() {
                    self.commit(&chosen, "light.shadows", json!(on), end);
                }
            }
        });
    }

    /// A number field: a drag previews on the canvas and lets go as one
    /// entry; a typed value is one entry. Each selected thing that has the
    /// field moves by as much as the shown one.
    fn number(
        &mut self,
        ui: &mut egui::Ui,
        chosen: &[&Value],
        path: &str,
        speed: f64,
        end: &ShellEnd,
    ) {
        let primary = chosen[0];
        let Some(written) = read(primary, path).and_then(|v| v.as_f64()) else {
            ui.weak("—");
            return;
        };
        let mut value = match &self.held {
            Some(held) if held.path == path => held
                .edits
                .first()
                .and_then(|e| patch_value(&e.patch, path))
                .unwrap_or(written),
            _ => written,
        };
        let response = ui.add(
            egui::DragValue::new(&mut value)
                .speed(speed)
                .max_decimals(3),
        );
        if response.changed() {
            let edits = self.edits(chosen, path, json!(value), false);
            if response.dragged() {
                end.ask(FromPage::EntityPreview {
                    edits: edits.clone(),
                });
                let from = self
                    .held
                    .take()
                    .filter(|h| h.path == path)
                    .map_or_else(|| json!(written), |h| h.from);
                self.held = Some(Held {
                    path: path.to_string(),
                    from,
                    edits,
                });
            } else {
                self.send_patch(path, edits, &json!(value), Some(&json!(written)), end);
            }
        }
        if response.drag_stopped()
            && let Some(held) = self.held.take()
            && held.path == path
        {
            let now = held
                .edits
                .first()
                .and_then(|e| patch_value(&e.patch, path))
                .map(|v| json!(v));
            if let Some(now) = now {
                self.send_patch(path, held.edits, &now, Some(&held.from), end);
            }
        }
    }

    fn color(&mut self, ui: &mut egui::Ui, chosen: &[&Value], path: &str, end: &ShellEnd) {
        let Some(rgba) = read(chosen[0], path).and_then(|v| {
            let a = v.as_array()?;
            Some([
                a.first()?.as_f64()? as f32,
                a.get(1)?.as_f64()? as f32,
                a.get(2)?.as_f64()? as f32,
                a.get(3).and_then(Value::as_f64).unwrap_or(1.0) as f32,
            ])
        }) else {
            return;
        };
        let mut picked = rgba;
        ui.horizontal(|ui| {
            ui.label("colour");
            let response = ui.color_edit_button_rgba_unmultiplied(&mut picked);
            if response.changed() && picked != rgba {
                let value = json!(picked.map(|c| (c * 1000.0).round() / 1000.0));
                self.commit(chosen, path, value, end);
            }
        });
    }

    /// Each chosen thing's patch for `path` set to `value`: the same value
    /// for all, or for a number, the shown one's value moved by as much.
    fn edits(&self, chosen: &[&Value], path: &str, value: Value, absolute: bool) -> Vec<SceneEdit> {
        let primary = chosen[0];
        let by = match (value.as_f64(), absolute) {
            (Some(v), false) => v - read(primary, path).and_then(|w| w.as_f64()).unwrap_or(v),
            _ => 0.0,
        };
        chosen
            .iter()
            .filter_map(|entity| {
                let own = if std::ptr::eq(*entity, primary) || absolute || !value.is_number() {
                    value.clone()
                } else {
                    let mine = read(entity, path)?.as_f64()?;
                    json!(((mine + by) * 1e4).round() / 1e4)
                };
                let own = match own.as_f64() {
                    Some(v) => json!((v * 1e4).round() / 1e4),
                    None => own,
                };
                Some(SceneEdit {
                    id: id_of(entity)?,
                    name: name_of(entity),
                    patch: patch_at(entity, path, own)?,
                })
            })
            .collect()
    }

    fn commit(&mut self, chosen: &[&Value], path: &str, value: Value, end: &ShellEnd) {
        let was = read(chosen[0], path);
        let edits = self.edits(chosen, path, value.clone(), true);
        self.send_patch(path, edits, &value, was.as_ref(), end);
    }

    fn send_patch(
        &mut self,
        path: &str,
        edits: Vec<SceneEdit>,
        value: &Value,
        was: Option<&Value>,
        end: &ShellEnd,
    ) {
        if edits.is_empty() {
            return;
        }
        let names = edits
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let what = describe(path, &names, value, was);
        end.ask(FromPage::EntityPatch { edits, what });
    }
}

/// What a patch sets `path` to, for a number.
fn patch_value(patch: &Value, path: &str) -> Option<f64> {
    let parts: Vec<&str> = path.split('.').collect();
    match parts.as_slice() {
        ["transform", key, axis] => patch.pointer(&format!("/transform/{key}/{axis}"))?.as_f64(),
        ["shape", key] => patch
            .get("shape")?
            .as_object()?
            .values()
            .next()?
            .get(*key)?
            .as_f64(),
        [part, key] => patch.get(*part)?.get(*key)?.as_f64(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamp() -> Value {
        json!({
            "id": 7,
            "name": "lamp",
            "transform": { "position": [1.0, 2.0, 3.0], "visible": true },
            "shape": { "Cylinder": { "radius": 0.1, "height": 1.5 } },
            "material": { "color": [1.0, 0.8, 0.5, 1.0], "roughness": 0.4 },
            "light": null
        })
    }

    #[test]
    fn a_field_reads_its_value_or_the_pages_default() {
        let lamp = lamp();
        assert_eq!(read(&lamp, "transform.position.1"), Some(json!(2.0)));
        assert_eq!(
            read(&lamp, "transform.scale.0"),
            Some(json!(1.0)),
            "the default scale"
        );
        assert_eq!(read(&lamp, "material.metallic"), Some(json!(0.0)));
        assert_eq!(read(&lamp, "shape.Cylinder.height"), Some(json!(1.5)));
    }

    #[test]
    fn a_patch_sets_the_field_and_keeps_its_neighbours() {
        let lamp = lamp();
        assert_eq!(
            patch_at(&lamp, "transform.position.1", json!(4.0)),
            Some(json!({ "transform": { "position": [1.0, 4.0, 3.0] } }))
        );
        assert_eq!(
            patch_at(&lamp, "shape.height", json!(2.0)),
            Some(json!({ "shape": { "Cylinder": { "radius": 0.1, "height": 2.0 } } }))
        );
        assert_eq!(
            patch_at(&lamp, "light.intensity", json!(10.0)),
            None,
            "it has no light"
        );
        assert_eq!(
            patch_value(
                &json!({ "transform": { "position": [1.0, 4.0, 3.0] } }),
                "transform.position.1"
            ),
            Some(4.0)
        );
    }

    #[test]
    fn several_things_move_by_as_much_as_the_one_shown() {
        let a = lamp();
        let mut b = lamp();
        b["id"] = json!(8);
        b["name"] = json!("lantern");
        b["transform"]["position"] = json!([5.0, 0.5, 0.0]);
        let scene = Scene::new();
        let edits = scene.edits(&[&a, &b], "transform.position.1", json!(3.0), false);
        assert_eq!(edits.len(), 2);
        assert_eq!(
            edits[1].patch,
            json!({ "transform": { "position": [5.0, 1.5, 0.0] } })
        );
        assert_eq!(
            describe("transform.position.1", "lamp, lantern", &json!(3.0), None),
            "move lamp, lantern"
        );
    }
}
