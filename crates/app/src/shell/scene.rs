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
use crate::timeline::ShotTime;
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

/// The value at a dotted path, or its default, as the page reads it
/// (`valueAt`): `transform.position.1`; `shape.radius`, the shape's own
/// kind skipped; `camera.aim.0` and `shot.in_s`, in the cinematography
/// extension.
fn read(entity: &Value, path: &str) -> Option<Value> {
    let found = (|| {
        let mut parts = path.split('.');
        let first = parts.next()?;
        let mut here = match first {
            "shape" => entity.get("shape")?.as_object()?.values().next()?,
            "camera" | "shot" => entity.get("ext-cinematography")?.get(first)?,
            _ => entity.get(first)?,
        };
        for part in parts {
            here = match part.parse::<usize>() {
                Ok(i) => here.get(i)?,
                Err(_) => here.get(part)?,
            };
        }
        (!here.is_null()).then(|| here.clone())
    })();
    found.or_else(|| read_default(entity, path))
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
        // A sound and the cinematography extension are replaced whole.
        ["audio", key] => {
            let mut audio = entity.get("audio").filter(|v| !v.is_null())?.clone();
            audio.as_object_mut()?.insert(key.to_string(), value);
            Some(json!({ "audio": audio }))
        }
        [part @ ("camera" | "shot"), key, rest @ ..] => {
            let mut cine = entity.get("ext-cinematography")?.clone();
            let block = cine.get_mut(*part)?.as_object_mut()?;
            match rest {
                [] => {
                    block.insert(key.to_string(), value);
                }
                [axis] => {
                    let axis: usize = axis.parse().ok()?;
                    *block.get_mut(*key)?.as_array_mut()?.get_mut(axis)? = value;
                }
                _ => return None,
            }
            Some(json!({ "ext-cinematography": cine }))
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
        [_, "inner_angle" | "outer_angle"] => format!(
            "{} the cone of {names}",
            if number(Some(value)) > number(was) {
                "widen"
            } else {
                "narrow"
            }
        ),
        [_, "focal_length_mm"] => {
            format!("{} mm lens on {names}", number(Some(value)).round())
        }
        [_, "aim", ..] => format!("aim {names}"),
        ["audio", "radius"] => format!("{names} heard to {} m", number(Some(value)).round()),
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

/// How a number field shows and takes its value.
#[derive(Clone, Copy)]
struct Field {
    /// Shown units a point of drag.
    speed: f64,
    /// Shown = written × this: a cone in degrees over radians.
    scale: f64,
    unit: &'static str,
    range: Option<(f64, f64)>,
    digits: usize,
}

impl Field {
    fn by(speed: f64) -> Self {
        Self {
            speed,
            scale: 1.0,
            unit: "",
            range: None,
            digits: 3,
        }
    }

    fn unit(self, unit: &'static str) -> Self {
        Self { unit, ..self }
    }

    fn range(self, lo: f64, hi: f64) -> Self {
        Self {
            range: Some((lo, hi)),
            ..self
        }
    }

    fn digits(self, digits: usize) -> Self {
        Self { digits, ..self }
    }

    fn degrees(self) -> Self {
        Self {
            scale: 180.0 / std::f64::consts::PI,
            ..self
        }
    }
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
    /// A shot's in and out points while a drag holds them: its id, in, out.
    timing: Option<(u64, f64, f64)>,
    /// A timeline lane asked to be shown (a modulation's "timeline" link),
    /// for the shell to pass on.
    reveal: Option<String>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            search: String::new(),
            closed: HashSet::new(),
            hidden: HashSet::new(),
            held: None,
            naming: None,
            timing: None,
            reveal: None,
        }
    }

    /// The lane the inspector asked the timeline to show, once.
    pub fn take_reveal(&mut self) -> Option<String> {
        self.reveal.take()
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
        // Keyed by the thing, as the page's is: another one starts at its
        // top.
        egui::ScrollArea::vertical()
            .id_salt(("inspector", state.selection.first()))
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
            nothing(ui, entities, state);
            return;
        };
        let (glyph, kind) = kind_of(primary);
        let id = id_of(primary).unwrap_or_default();
        let shot = primary
            .pointer("/ext-cinematography/shot")
            .is_some_and(|v| !v.is_null());
        ui.horizontal(|ui| {
            ui.label(RichText::new(glyph).color(theme::MUTED));
            ui.strong(&kind);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Fly to")
                    .on_hover_text("The camera goes to it")
                    .clicked()
                {
                    end.ask(FromPage::Focus { id });
                }
                if shot && ui.small_button("Look through").clicked() {
                    end.ask(FromPage::ShotLook { id: Some(id) });
                }
            });
        });
        // Its id, what it is, what holds it and what it holds.
        let children = entities
            .values()
            .filter(|e| e.get("parent").and_then(Value::as_u64) == Some(id))
            .count();
        let parent = primary
            .get("parent")
            .and_then(Value::as_u64)
            .and_then(|p| Some((p, name_of(entities.get(&p)?))));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let faint = |text: String| RichText::new(text).small().color(theme::FAINT);
            ui.label(faint(format!("#{id} · {kind}")));
            if let Some((parent, name)) = parent {
                ui.label(faint("· in".into()));
                if ui
                    .link(RichText::new(name).small().color(theme::MUTED))
                    .clicked()
                {
                    end.ask(FromPage::Select { ids: vec![parent] });
                }
            }
            if children > 0 {
                ui.label(faint(format!("· holds {children}")));
            }
        });
        if chosen.len() > 1 {
            let others: Vec<String> = chosen[1..].iter().map(|e| name_of(e)).collect();
            let and = match others.as_slice() {
                [one] => one.clone(),
                [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
                [] => String::new(),
            };
            ui.label(
                RichText::new(format!(
                    "And {and}: a change here goes to all {}.",
                    chosen.len()
                ))
                .color(theme::ACCENT),
            );
        }
        if !editable {
            ui.label(
                RichText::new(
                    "This world was built from a file; open a .world package to change it here.",
                )
                .color(theme::MUTED),
            );
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

            part(ui, "Transform");
            egui::Grid::new("transform").num_columns(4).show(ui, |ui| {
                for (label, key, field) in [
                    ("Position", "position", Field::by(0.01).unit(" m")),
                    (
                        "Rotation",
                        "rotation_degrees",
                        Field::by(1.0).unit("°").digits(0),
                    ),
                    ("Scale", "scale", Field::by(0.01).range(0.01, f64::MAX)),
                ] {
                    ui.label(label);
                    for axis in 0..3 {
                        let path = format!("transform.{key}.{axis}");
                        self.number(ui, &chosen, &path, field, end);
                    }
                    ui.end_row();
                }
            });
            self.toggle(ui, &chosen, "transform.visible", "Visible", true, end);

            if let Some((shape, fields)) = primary
                .get("shape")
                .and_then(Value::as_object)
                .and_then(|o| o.iter().next())
            {
                part(ui, "Shape");
                egui::Grid::new("shape").num_columns(2).show(ui, |ui| {
                    text_row(ui, "Kind", shape);
                    if let Some(fields) = fields.as_object() {
                        for key in fields.keys().filter(|k| fields[*k].is_number()) {
                            ui.label(key.replace('_', " "));
                            // A prism's sides are counted, not measured.
                            let field = if key == "sides" {
                                Field::by(1.0).digits(0).range(3.0, 64.0)
                            } else {
                                Field::by(0.01).unit(" m").range(0.01, f64::MAX)
                            };
                            self.number(ui, &chosen, &format!("shape.{key}"), field, end);
                            ui.end_row();
                        }
                    }
                });
            }
            if let Some(material) = primary.get("material").filter(|v| !v.is_null()) {
                part(ui, "Material");
                self.color(ui, &chosen, "material.color", end);
                egui::Grid::new("material").num_columns(2).show(ui, |ui| {
                    for (label, key) in [("Roughness", "roughness"), ("Metallic", "metallic")] {
                        ui.label(label);
                        let field = Field::by(0.01).range(0.0, 1.0).digits(2);
                        self.number(ui, &chosen, &format!("material.{key}"), field, end);
                        ui.end_row();
                    }
                    if let Some(glow) = rgba(material.get("emissive"))
                        && glow[..3].iter().any(|c| *c > 0.0)
                    {
                        // Its colour picked as a colour, written back at its
                        // strength (an emissive brighter than 1), as the
                        // page's `atStrength` does.
                        ui.label("Glow");
                        ui.horizontal(|ui| {
                            let strength = glow[..3].iter().fold(1.0f32, |m, c| m.max(*c));
                            let colour = [glow[0], glow[1], glow[2]].map(|c| c / strength);
                            let mut picked = colour;
                            if ui.color_edit_button_rgb(&mut picked).changed() && picked != colour {
                                let at = |c: f32| (c * strength * 1e4).round() / 1e4;
                                let value =
                                    json!([at(picked[0]), at(picked[1]), at(picked[2]), glow[3]]);
                                self.commit(&chosen, "material.emissive", value, end);
                            }
                            ui.label(RichText::new(format!("× {strength:.1}")).color(theme::MUTED));
                        });
                        ui.end_row();
                    }
                    if let Some(texture) =
                        material.get("base_color_texture").and_then(Value::as_str)
                    {
                        text_row(ui, "Texture", texture);
                    }
                });
            }
            if let Some(light) = primary.get("light").filter(|v| !v.is_null()) {
                part(ui, "Light");
                let kind = light
                    .get("light_type")
                    .and_then(Value::as_str)
                    .unwrap_or("point");
                self.color(ui, &chosen, "light.color", end);
                egui::Grid::new("light").num_columns(2).show(ui, |ui| {
                    text_row(ui, "Kind", kind);
                    ui.label("Intensity");
                    let unit = if kind == "directional" { " lux" } else { " lm" };
                    let field = Field::by(10.0).unit(unit).digits(0).range(0.0, f64::MAX);
                    self.number(ui, &chosen, "light.intensity", field, end);
                    ui.end_row();
                    if light.get("range").is_some_and(|v| !v.is_null()) {
                        ui.label("Range");
                        let field = Field::by(0.1).unit(" m").digits(1).range(0.0, f64::MAX);
                        self.number(ui, &chosen, "light.range", field, end);
                        ui.end_row();
                    }
                    if let Some(outer) = light.get("outer_angle").and_then(Value::as_f64) {
                        // The inner cone stays inside the outer, and a spot
                        // opens to a right angle at most.
                        let inner = light
                            .get("inner_angle")
                            .and_then(Value::as_f64)
                            .unwrap_or(0.0);
                        let degrees = 180.0 / std::f64::consts::PI;
                        ui.label("Cone");
                        ui.horizontal(|ui| {
                            let field = Field::by(0.5).unit("°").digits(0).degrees();
                            self.number(
                                ui,
                                &chosen,
                                "light.inner_angle",
                                field.range(0.0, outer * degrees),
                                end,
                            );
                            ui.weak("to");
                            self.number(
                                ui,
                                &chosen,
                                "light.outer_angle",
                                field.range(inner * degrees, 90.0),
                                end,
                            );
                        });
                        ui.end_row();
                    }
                });
                self.toggle(ui, &chosen, "light.shadows", "Shadows", false, end);
            }
            if let Some(camera) = primary
                .pointer("/ext-cinematography/camera")
                .filter(|v| !v.is_null())
            {
                part(ui, "Camera");
                egui::Grid::new("camera").num_columns(2).show(ui, |ui| {
                    ui.label("Lens");
                    let field = Field::by(1.0).unit(" mm").digits(0).range(1.0, f64::MAX);
                    self.number(ui, &chosen, "camera.focal_length_mm", field, end);
                    ui.end_row();
                    let sensor = camera
                        .get("sensor_mm")
                        .and_then(Value::as_array)
                        .map(|s| {
                            s.iter()
                                .filter_map(Value::as_f64)
                                .map(|v| v.to_string())
                                .collect::<Vec<_>>()
                                .join(" × ")
                        })
                        .unwrap_or_else(|| "24.89 × 18.66".into());
                    text_row(ui, "Sensor", &format!("{sensor} mm"));
                    if camera.get("aim").is_some_and(Value::is_array) {
                        ui.label("Aims at");
                        ui.horizontal(|ui| {
                            for axis in 0..3 {
                                let field = Field::by(0.05).unit(" m").digits(1);
                                self.number(ui, &chosen, &format!("camera.aim.{axis}"), field, end);
                            }
                        });
                        ui.end_row();
                    }
                });
            }
            if let Some(shot) = primary
                .pointer("/ext-cinematography/shot")
                .filter(|v| !v.is_null())
            {
                part(ui, "Shot");
                egui::Grid::new("shot").num_columns(2).show(ui, |ui| {
                    let size = shot.get("size").and_then(Value::as_str).unwrap_or("—");
                    text_row(ui, "Size", size);
                    if let Some(scene) = shot.get("scene").and_then(Value::as_str) {
                        text_row(ui, "Scene", scene);
                    }
                    ui.label("On the clock");
                    self.shot_times(ui, id, shot, end);
                    ui.end_row();
                    if let Some(says) = shot.get("description").and_then(Value::as_str) {
                        text_row(ui, "Says", says);
                    }
                });
            }
            if let Some(mesh) = primary.pointer("/mesh_asset/path").and_then(Value::as_str) {
                part(ui, "Mesh");
                egui::Grid::new("mesh").num_columns(2).show(ui, |ui| {
                    ui.label("File");
                    ui.label(RichText::new(mesh).monospace().color(theme::MUTED));
                    ui.end_row();
                });
            }
            let behaviors = primary
                .get("behaviors")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !behaviors.is_empty() {
                part(ui, "Behaviors");
                for (i, behavior) in behaviors.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let (kind, said) = behavior_words(behavior);
                        ui.label(RichText::new(&kind).color(theme::BEHAVIOR));
                        ui.add(
                            egui::Label::new(RichText::new(said).color(theme::MUTED)).truncate(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Remove").clicked() {
                                let mut rest = behaviors.clone();
                                rest.remove(i);
                                let name = name_of(primary);
                                end.ask(FromPage::EntityPatch {
                                    edits: vec![SceneEdit {
                                        id,
                                        name: name.clone(),
                                        patch: json!({ "behaviors": rest }),
                                    }],
                                    what: format!("remove {kind} from {name}"),
                                });
                            }
                        });
                    });
                }
            }
        });
        // What the song moves edits nothing here: its link to the lane
        // works on a world built from a file too.
        if let Some(modulations) = primary.get("modulations").and_then(Value::as_array)
            && !modulations.is_empty()
        {
            {
                part(ui, "Moved by the song");
                // Each one's lane on the timeline, where the timeline has it.
                let lanes: Vec<Option<String>> = (0..modulations.len())
                    .map(|i| {
                        let track = state.scene.as_deref()?.tracks.get(&id)?.get(i)?;
                        let shown = state.timeline.as_deref()?.tracks.iter().any(|t| {
                            matches!(t, crate::timeline::TrackView::Modulation { id, .. } if id == track)
                        });
                        shown.then(|| track.clone())
                    })
                    .collect();
                for (modulation, lane) in modulations.iter().zip(lanes) {
                    ui.horizontal(|ui| {
                        let target = modulation
                            .get("target")
                            .and_then(Value::as_str)
                            .unwrap_or("?");
                        ui.label(target.replace('_', " "));
                        ui.weak("←");
                        let signal = modulation
                            .get("signal")
                            .map(signal_name)
                            .unwrap_or_default();
                        ui.label(RichText::new(signal).color(theme::MODULATION));
                        if let Some([lo, hi]) = modulation
                            .get("range")
                            .and_then(Value::as_array)
                            .and_then(|r| Some([r.first()?.as_f64()?, r.get(1)?.as_f64()?]))
                        {
                            ui.label(RichText::new(format!("{lo}–{hi}")).color(theme::MUTED));
                        }
                        if let Some(lane) = lane {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button("timeline ↗")
                                        .on_hover_text("Its track on the timeline")
                                        .clicked()
                                    {
                                        self.reveal = Some(lane);
                                    }
                                },
                            );
                        }
                    });
                }
            }
        }
        ui.add_enabled_ui(editable, |ui| {
            if let Some(audio) = primary.get("audio").filter(|v| !v.is_null()) {
                part(ui, "Sound");
                egui::Grid::new("sound").num_columns(2).show(ui, |ui| {
                    let kind = audio
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("ambient");
                    text_row(ui, "Kind", kind);
                    if let Some(source) = audio.get("source") {
                        text_row(ui, "Source", &source_name(source));
                    }
                    ui.label("Volume");
                    let volume = audio.get("volume").and_then(Value::as_f64).unwrap_or(1.0);
                    let field = Field::by(0.01).digits(2).range(0.0, volume.max(1.0));
                    self.number(ui, &chosen, "audio.volume", field, end);
                    ui.end_row();
                    if audio.get("radius").is_some_and(|v| !v.is_null()) {
                        ui.label("Heard to");
                        let field = Field::by(0.5).unit(" m").digits(0).range(0.0, f64::MAX);
                        self.number(ui, &chosen, "audio.radius", field, end);
                        ui.end_row();
                    }
                });
            }
        });
    }

    /// A switch on a field that is true or false, one entry a change.
    fn toggle(
        &mut self,
        ui: &mut egui::Ui,
        chosen: &[&Value],
        path: &str,
        label: &str,
        default: bool,
        end: &ShellEnd,
    ) {
        let was = read(chosen[0], path)
            .and_then(|v| v.as_bool())
            .unwrap_or(default);
        let mut on = was;
        if ui.checkbox(&mut on, label).changed() {
            self.commit(chosen, path, json!(on), end);
        }
    }

    /// A shot's in and out points: set through the timeline's own edit
    /// (`shot.time`), one entry when a drag lets go or a number is typed.
    fn shot_times(&mut self, ui: &mut egui::Ui, id: u64, shot: &Value, end: &ShellEnd) {
        let get = |key: &str| shot.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        let (from, to) = (get("in_s"), get("out_s"));
        let (mut a, mut b) = self
            .timing
            .filter(|(at, _, _)| *at == id)
            .map_or((from, to), |(_, a, b)| (a, b));
        ui.horizontal(|ui| {
            let left = ui.add(
                egui::DragValue::new(&mut a)
                    .speed(0.05)
                    .suffix(" s")
                    .range(0.0..=(b - 0.1).max(0.0)),
            );
            ui.weak("to");
            let right = ui.add(
                egui::DragValue::new(&mut b)
                    .speed(0.05)
                    .suffix(" s")
                    .range((a + 0.1)..=f64::MAX),
            );
            let held = left.dragged() || right.dragged();
            let typed = (left.changed() || right.changed()) && !held;
            if held {
                self.timing = Some((id, a, b));
            }
            if typed || left.drag_stopped() || right.drag_stopped() {
                self.timing = None;
                if (a, b) != (from, to) {
                    end.ask(FromPage::ShotTimes {
                        edits: vec![ShotTime {
                            id,
                            in_s: a as f32,
                            out_s: b as f32,
                        }],
                    });
                }
            }
        });
    }
}

/// A part of the inspector: its title over its rows.
fn part(ui: &mut egui::Ui, title: &str) {
    ui.add_space(4.0);
    ui.separator();
    ui.label(RichText::new(title).strong().color(theme::MUTED));
}

/// A row that says, without an edit.
fn text_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(label);
    ui.add(egui::Label::new(RichText::new(value).color(theme::MUTED)).wrap());
    ui.end_row();
}

/// A colour, as a small square.
fn swatch(ui: &mut egui::Ui, [r, g, b, _]: [f32; 4]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 2.0, egui::Rgba::from_rgb(r, g, b));
}

fn rgba(value: Option<&Value>) -> Option<[f32; 4]> {
    let a = value?.as_array()?;
    Some([
        a.first()?.as_f64()? as f32,
        a.get(1)?.as_f64()? as f32,
        a.get(2)?.as_f64()? as f32,
        a.get(3).and_then(Value::as_f64).unwrap_or(1.0) as f32,
    ])
}

/// A behavior as the page's card heads it: its kind, and its fields in a
/// line ("speed 30, radius 3").
fn behavior_words(behavior: &Value) -> (String, String) {
    let Some((kind, fields)) = behavior.as_object().and_then(|o| o.iter().next()) else {
        return ("behavior".into(), String::new());
    };
    let said = fields
        .as_object()
        .map(|f| {
            f.iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| format!("{} {}", k.replace('_', " "), short(v)))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    (kind.clone(), said)
}

/// A value in a line: a number to two places, a list of them, a name.
fn short(value: &Value) -> String {
    match value {
        Value::Number(n) => {
            let n = n.as_f64().unwrap_or(0.0);
            if n.fract() == 0.0 {
                format!("{n:.0}")
            } else {
                format!("{n:.2}")
            }
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(short).collect::<Vec<_>>().join(", ")
        ),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// What a modulation reads, as the page names it.
fn signal_name(signal: &Value) -> String {
    if let Some(name) = signal.as_str() {
        return name.to_string();
    }
    let Some((kind, value)) = signal.as_object().and_then(|o| o.iter().next()) else {
        return "signal".into();
    };
    match kind.as_str() {
        "stem" => format!("{} stem", value.as_str().unwrap_or("a")),
        "oscillator" => format!(
            "sine {} Hz",
            value.get("frequency").map(short).unwrap_or_default()
        ),
        _ => kind.clone(),
    }
}

/// A sound's source, as the page names it: a file, or a synthesized kind
/// with its settings.
fn source_name(source: &Value) -> String {
    if let Some(file) = source.as_str() {
        return file.to_string();
    }
    let Some((kind, fields)) = source.as_object().and_then(|o| o.iter().next()) else {
        return "—".into();
    };
    let said = fields
        .as_object()
        .map(|f| {
            f.iter()
                .map(|(k, v)| format!("{} {}", k.replace('_', " "), short(v)))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    if said.is_empty() {
        kind.clone()
    } else {
        format!("{kind} · {said}")
    }
}

/// Nothing selected: the world in a few lines, as the page's inspector has
/// it — what it holds, its sky, its song, its shots and tours.
fn nothing(ui: &mut egui::Ui, entities: &BTreeMap<u64, &Value>, state: &State) {
    if let Some(view) = state.scene.as_deref() {
        let world = &view.world;
        ui.strong(&world.name);
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for entity in entities.values() {
            *counts.entry(class_of(entity)).or_default() += 1;
        }
        ui.horizontal_wrapped(|ui| {
            for (class, n) in &counts {
                let word = if *n == 1 {
                    class.to_string()
                } else {
                    format!("{class}s")
                };
                ui.label(RichText::new(format!("{n} {word}")).color(theme::MUTED));
            }
        });
        if let Some(sky) = world.sky {
            ui.horizontal(|ui| {
                swatch(ui, sky);
                let [r, g, b, _] = sky.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
                ui.label(RichText::new(format!("sky #{r:02x}{g:02x}{b:02x}")).color(theme::MUTED));
            });
        }
        if let Some(song) = &world.song {
            ui.label(RichText::new(format!("♪ {song}")).color(theme::MUTED));
        }
        let shots = entities
            .values()
            .filter(|e| {
                e.pointer("/ext-cinematography/shot")
                    .is_some_and(|v| !v.is_null())
            })
            .count();
        let tours = world.tours;
        ui.label(
            RichText::new(format!(
                "{shots} shots · {tours} tour{}",
                if tours == 1 { "" } else { "s" }
            ))
            .color(theme::MUTED),
        );
        ui.add_space(6.0);
    }
    ui.label(
        RichText::new(
            "Select a thing in the world or above to see what it is made of; shift-click adds \
             to the selection.",
        )
        .color(theme::FAINT),
    );
}

/// What a thing is, for the world's counts: a light, a camera, a model, a
/// shape or a group.
fn class_of(entity: &Value) -> &'static str {
    let (_, kind) = kind_of(entity);
    if kind.ends_with("light") {
        "light"
    } else {
        match kind.as_str() {
            "camera" => "camera",
            "model" => "model",
            "group" => "group",
            _ => "shape",
        }
    }
}

impl Scene {
    /// A number field: a drag previews on the canvas and lets go as one
    /// entry; a typed value is one entry. Each selected thing that has the
    /// field moves by as much as the shown one.
    fn number(
        &mut self,
        ui: &mut egui::Ui,
        chosen: &[&Value],
        path: &str,
        field: Field,
        end: &ShellEnd,
    ) {
        let primary = chosen[0];
        let Some(written) = read(primary, path).and_then(|v| v.as_f64()) else {
            ui.weak("—");
            return;
        };
        let held = match &self.held {
            Some(held) if held.path == path => held
                .edits
                .first()
                .and_then(|e| patch_value(&e.patch, path))
                .unwrap_or(written),
            _ => written,
        };
        // Shown in the field's own units (a cone in degrees, written in
        // radians), written back in the format's.
        let mut shown = held * field.scale;
        let mut drag = egui::DragValue::new(&mut shown)
            .speed(field.speed)
            .max_decimals(field.digits)
            .suffix(field.unit);
        if let Some((lo, hi)) = field.range {
            drag = drag.range(lo..=hi);
        }
        let response = ui.add(drag);
        let value = shown / field.scale;
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
        [part @ ("camera" | "shot"), key, rest @ ..] => {
            let mut at = patch.get("ext-cinematography")?.get(*part)?.get(*key)?;
            if let [axis] = rest {
                at = at.get(axis.parse::<usize>().ok()?)?;
            }
            at.as_f64()
        }
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
        assert_eq!(
            read(&lamp, "shape.height"),
            Some(json!(1.5)),
            "the shape's kind skipped"
        );
        let camera = json!({
            "id": 9,
            "name": "1A",
            "transform": { "position": [0.0, 1.6, 6.0] },
            "ext-cinematography": {
                "camera": { "focal_length_mm": 24.0, "aim": [0.0, 1.0, 0.0] },
                "shot": { "in_s": 0.0, "out_s": 11.4 }
            }
        });
        assert_eq!(read(&camera, "camera.focal_length_mm"), Some(json!(24.0)));
        assert_eq!(read(&camera, "camera.aim.1"), Some(json!(1.0)));
        assert_eq!(read(&camera, "shot.out_s"), Some(json!(11.4)));
        // The extension is replaced whole: the shot rides along.
        let patch = patch_at(&camera, "camera.aim.1", json!(1.5)).expect("a camera");
        assert_eq!(
            patch,
            json!({ "ext-cinematography": {
                "camera": { "focal_length_mm": 24.0, "aim": [0.0, 1.5, 0.0] },
                "shot": { "in_s": 0.0, "out_s": 11.4 }
            } })
        );
        assert_eq!(patch_value(&patch, "camera.aim.1"), Some(1.5));
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
    fn behaviors_signals_and_sounds_read_as_the_page_says_them() {
        let pulse = json!({ "Pulse": { "frequency": 2.0, "max_scale": 1.1, "min_scale": 0.9 } });
        assert_eq!(
            behavior_words(&pulse),
            (
                "Pulse".to_string(),
                "frequency 2, max scale 1.10, min scale 0.90".to_string()
            )
        );
        assert_eq!(
            signal_name(&json!({ "oscillator": { "frequency": 0.25 } })),
            "sine 0.25 Hz"
        );
        assert_eq!(signal_name(&json!({ "stem": "drums" })), "drums stem");
        assert_eq!(signal_name(&json!("energy")), "energy");
        assert_eq!(
            source_name(&json!({ "Fire": { "crackle": 0.5, "intensity": 0.7 } })),
            "Fire · crackle 0.50, intensity 0.70"
        );
        assert_eq!(source_name(&json!("sounds/rain.ogg")), "sounds/rain.ogg");
        assert_eq!(
            describe("camera.focal_length_mm", "1A", &json!(50.0), None),
            "50 mm lens on 1A"
        );
        assert_eq!(
            describe("light.outer_angle", "spot", &json!(0.8), Some(&json!(0.6))),
            "widen the cone of spot"
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
