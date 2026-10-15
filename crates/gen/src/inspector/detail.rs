//! Detail panel — the selected entity's components; in a live world, the
//! place to change them.
//!
//! Every widget edit becomes one `ModifyEntity` merge patch through the
//! authority, like a canvas gesture (`gen3d::canvas_edit`): while a slider
//! is held or a field is being typed into, the Bevy component is changed
//! directly so the person sees the value; when the pointer is up and
//! nothing has keyboard focus, what moved goes out as one intent holding
//! only those fields, and the document has the last word — a refusal puts
//! the entity back. The values shown are the document's, not the scene's,
//! so an angle keeps the form the author gave it. Without a live world the
//! panel reads the scene and does not write.

use std::collections::HashSet;

use bevy::prelude::*;
use bevy_egui::egui;
use serde_json::{Map, Value, json};

use crate::gen3d::audio::AudioEngine;
use crate::gen3d::behaviors::EntityBehaviors;
use crate::gen3d::canvas_edit::PersonIntent;
use crate::gen3d::commands::BehaviorDef as GenBehaviorDef;
use crate::gen3d::registry::*;
use localgpt_world_types as wt;

use super::{InspectorQueries, InspectorSelection};

const REFUSED: egui::Color32 = egui::Color32::from_rgb(235, 120, 100);
const WARN: egui::Color32 = egui::Color32::from_rgb(230, 180, 90);

/// Where the panel's edits go, in a live world.
pub(crate) struct EditSurface<'a> {
    /// The document the canvas shows: the panel edits its values.
    pub doc: &'a openworldformat::WorldDoc,
    /// Intents for the canvas to commit.
    pub intents: &'a mut Vec<PersonIntent>,
    /// The field gesture in progress.
    pub edit: &'a mut PanelEdit,
    /// The newest commit or refusal, for the panel to show.
    pub status: Option<&'a str>,
    /// Where the fields, combos and buttons are, for a driver with no eyes.
    pub rects: &'a mut super::PanelRects,
}

/// The field gesture in progress: changes to one entity merged into one
/// patch, sent when the pointer is up and nothing is being typed.
#[derive(Resource, Default)]
pub struct PanelEdit {
    pending: Option<Pending>,
    /// The name being typed, until it is committed or abandoned.
    name_draft: Option<(u64, String)>,
    /// Why the last name couldn't be taken.
    name_problem: Option<String>,
}

struct Pending {
    id: u64,
    name: String,
    patch: Map<String, Value>,
    verbs: Vec<&'static str>,
}

impl PanelEdit {
    /// Merge one field into the pending patch for `id` — flushing a patch
    /// for another entity first. `sub` names a key inside an object-valued
    /// field (`transform.position`); the authority merges those.
    #[allow(clippy::too_many_arguments)]
    fn change(
        &mut self,
        out: &mut Vec<PersonIntent>,
        id: u64,
        name: &str,
        top: &str,
        sub: Option<&str>,
        value: Value,
        verb: &'static str,
    ) {
        if self.pending.as_ref().is_some_and(|p| p.id != id) {
            self.flush(out);
        }
        let pending = self.pending.get_or_insert_with(|| Pending {
            id,
            name: name.to_string(),
            patch: Map::new(),
            verbs: Vec::new(),
        });
        match sub {
            Some(sub) => {
                let slot = pending
                    .patch
                    .entry(top.to_string())
                    .or_insert_with(|| json!({}));
                if let Some(object) = slot.as_object_mut() {
                    object.insert(sub.to_string(), value);
                }
            }
            None => {
                pending.patch.insert(top.to_string(), value);
            }
        }
        if !pending.verbs.contains(&verb) {
            pending.verbs.push(verb);
        }
    }

    /// Send what is pending as one intent.
    pub fn flush(&mut self, out: &mut Vec<PersonIntent>) {
        if let Some(p) = self.pending.take() {
            out.push(PersonIntent::Patch {
                id: p.id,
                what: format!("{} {}", p.verbs.join(", "), p.name),
                name: p.name,
                patch: Value::Object(p.patch),
            });
        }
    }

    /// Flush when the gesture is over — the pointer is up and no field has
    /// the keyboard — or when the pending entity is no longer the one shown.
    fn settle(&mut self, ctx: &egui::Context, out: &mut Vec<PersonIntent>, shown: Option<u64>) {
        let Some(p) = &self.pending else {
            return;
        };
        let held = ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input();
        if Some(p.id) != shown || !held {
            self.flush(out);
        }
    }
}

/// What a section needs to draw, and to edit when live.
struct Edit<'a, 'b> {
    entity: Entity,
    id: Option<u64>,
    name: &'a str,
    /// The document's entity, when live.
    doc: Option<&'a wt::WorldEntity>,
    live: Option<&'a mut EditSurface<'b>>,
}

impl Edit<'_, '_> {
    fn editable(&self) -> bool {
        self.live.is_some() && self.doc.is_some()
    }

    /// Record a field change, when live.
    fn change(&mut self, top: &str, sub: Option<&str>, value: Value, verb: &'static str) {
        if let (Some(live), Some(id)) = (self.live.as_mut(), self.id) {
            live.edit
                .change(live.intents, id, self.name, top, sub, value, verb);
        }
    }

    /// Note where a widget is, when live.
    fn note(&mut self, name: impl Into<String>, response: &egui::Response) {
        self.note_rect(name, response.rect);
    }

    fn note_rect(&mut self, name: impl Into<String>, rect: egui::Rect) {
        if let Some(live) = self.live.as_mut() {
            live.rects.note_rect(name, rect);
        }
    }
}

// ---------------------------------------------------------------------------
// Main draw function
// ---------------------------------------------------------------------------

// egui 0.34 deprecates `Panel::show(ctx)` in favour of `show_inside(ui)`, but
// bevy_egui only exposes a `Context` (not a root `Ui`), so the context-level
// `show` is still the correct entry point here.
#[allow(deprecated)]
pub(crate) fn draw_detail(
    ctx: &egui::Context,
    selection: &mut InspectorSelection,
    registry: &NameRegistry,
    q: &mut InspectorQueries,
    mut live: Option<EditSurface<'_>>,
) -> egui::Rect {
    let panel = egui::Panel::right("inspector_detail")
        .default_size(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            let Some(entity) = selection.entity else {
                ui.heading("Inspector");
                ui.separator();
                ui.weak("Select an entity to inspect");
                if let Some(live) = live.as_mut() {
                    live.edit.flush(live.intents);
                    show_status(ui, live.status);
                }
                return;
            };

            // Verify entity still exists in registry
            let Some(name) = registry.get_name(entity).map(str::to_string) else {
                ui.heading("Inspector");
                ui.separator();
                ui.weak("Entity no longer exists");
                if let Some(live) = live.as_mut() {
                    live.edit.flush(live.intents);
                }
                return;
            };
            let id = q.gen_entities.get(entity).ok().map(|g| g.world_id.0);
            // The document's entity, which is what the panel edits.
            let doc_entity = match (&live, id) {
                (Some(live), Some(id)) => live.doc.get(id).cloned(),
                _ => None,
            };

            ui.heading(&name);
            if !selection.others.is_empty() {
                ui.weak(format!(
                    "+{} more selected — fields here change {name}; gestures move all",
                    selection.others.len()
                ));
            }
            if let Some(live) = &live {
                show_status(ui, live.status);
            }
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut edit = Edit {
                        entity,
                        id,
                        name: &name,
                        doc: doc_entity.as_ref(),
                        live: live.as_mut(),
                    };
                    draw_identity_section(ui, &mut edit, registry, q);
                    draw_transform_section(ui, &mut edit, q);
                    draw_shape_section(ui, &mut edit, q);
                    draw_material_section(ui, &mut edit, q);
                    draw_light_section(ui, &mut edit, q);
                    draw_behaviors_section(ui, &mut edit, &q.behaviors_q);
                    draw_audio_section(ui, &name, q.audio_engine.as_deref());
                    draw_mesh_section(ui, entity, &q.gltf_sources);
                    draw_hierarchy_section(ui, &mut edit, registry, q, selection);
                });
            if let Some(live) = live.as_mut() {
                live.edit.settle(ctx, live.intents, id);
            }
        });
    panel.response.rect
}

fn show_status(ui: &mut egui::Ui, status: Option<&str>) {
    if let Some(status) = status {
        if status.starts_with("refused") {
            ui.colored_label(REFUSED, status);
        } else {
            ui.weak(status);
        }
    }
}

/// Three drag values in a row; whether one changed, and where each is.
fn triple(
    ui: &mut egui::Ui,
    v: &mut [f32; 3],
    speed: f64,
    enabled: bool,
) -> (bool, [egui::Rect; 3]) {
    let mut changed = false;
    let mut rects = [egui::Rect::NOTHING; 3];
    ui.horizontal(|ui| {
        for (i, label) in ["x", "y", "z"].iter().enumerate() {
            ui.label(*label);
            let response = ui.add_enabled(
                enabled,
                egui::DragValue::new(&mut v[i]).speed(speed).max_decimals(3),
            );
            changed |= response.changed();
            rects[i] = response.rect;
        }
    });
    (changed, rects)
}

/// Note a triple's three fields as `<name>.x`, `.y`, `.z`.
fn note_triple(edit: &mut Edit, name: &str, rects: [egui::Rect; 3]) {
    for (axis, rect) in ["x", "y", "z"].iter().zip(rects) {
        edit.note_rect(format!("{name}.{axis}"), rect);
    }
}

fn round3(v: [f32; 3]) -> [f32; 3] {
    v.map(|x| (x * 1000.0).round() / 1000.0)
}

fn round2(v: [f32; 3]) -> [f32; 3] {
    v.map(|x| (x * 100.0).round() / 100.0)
}

fn round3_4(v: [f32; 4]) -> [f32; 4] {
    v.map(|x| (x * 1000.0).round() / 1000.0)
}

// ---------------------------------------------------------------------------
// Section: Identity
// ---------------------------------------------------------------------------

fn draw_identity_section(
    ui: &mut egui::Ui,
    edit: &mut Edit,
    registry: &NameRegistry,
    q: &InspectorQueries,
) {
    egui::CollapsingHeader::new("Identity")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("identity_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    match (edit.live.as_mut(), edit.doc, edit.id) {
                        (Some(live), Some(_), Some(id)) => {
                            let mut draft = match &live.edit.name_draft {
                                Some((d, s)) if *d == id => s.clone(),
                                _ => edit.name.to_string(),
                            };
                            let response = ui.text_edit_singleline(&mut draft);
                            live.rects.note("detail.name", &response);
                            if response.lost_focus() {
                                let new = draft.trim().to_string();
                                if new.is_empty() {
                                    live.edit.name_problem = Some("a name can't be empty".into());
                                    draft = edit.name.to_string();
                                } else if new != edit.name {
                                    if live.doc.get_by_name(&new).is_some() {
                                        live.edit.name_problem = Some(format!("{new} is taken"));
                                    } else {
                                        live.edit.name_problem = None;
                                        live.edit.change(
                                            live.intents,
                                            id,
                                            edit.name,
                                            "name",
                                            None,
                                            json!(new),
                                            "rename",
                                        );
                                    }
                                } else {
                                    live.edit.name_problem = None;
                                }
                            }
                            live.edit.name_draft = Some((id, draft));
                            ui.end_row();
                            if let Some(problem) = live.edit.name_problem.clone() {
                                ui.label("");
                                ui.colored_label(REFUSED, problem);
                                ui.end_row();
                            }
                        }
                        _ => {
                            ui.label(edit.name);
                            ui.end_row();
                        }
                    }

                    if let Ok(gen_e) = q.gen_entities.get(edit.entity) {
                        ui.label("Type");
                        ui.label(gen_e.entity_type.as_str());
                        ui.end_row();

                        ui.label("World ID");
                        ui.label(format!("{}", gen_e.world_id.0));
                        ui.end_row();
                    }

                    if let Some(id) = registry.get_id(edit.entity) {
                        ui.label("Entity ID");
                        ui.label(format!("{}", id.0));
                        ui.end_row();
                    }
                });
        });
}

// ---------------------------------------------------------------------------
// Section: Transform
// ---------------------------------------------------------------------------

fn draw_transform_section(ui: &mut egui::Ui, edit: &mut Edit, q: &mut InspectorQueries) {
    let Ok(current) = q.transforms.get(edit.entity).copied() else {
        return;
    };
    let editable = edit.editable();
    // The document's values when live — the angles keep their form — and
    // the scene's otherwise.
    let (mut position, mut rotation, mut scale, mut visible) = match edit.doc {
        Some(doc) => (
            doc.transform.position,
            doc.transform.rotation_degrees,
            doc.transform.scale,
            doc.transform.visible,
        ),
        None => {
            let (x, y, z) = current.rotation.to_euler(EulerRot::XYZ);
            (
                current.translation.to_array(),
                [x.to_degrees(), y.to_degrees(), z.to_degrees()],
                current.scale.to_array(),
                q.visibility_q
                    .get(edit.entity)
                    .map_or(true, |v| *v != Visibility::Hidden),
            )
        }
    };

    egui::CollapsingHeader::new("Transform")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("transform_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Position");
                    let (changed, rects) = triple(ui, &mut position, 0.01, editable);
                    note_triple(edit, "detail.position", rects);
                    if changed {
                        if let Ok(mut t) = q.transforms.get_mut(edit.entity) {
                            t.translation = Vec3::from_array(position);
                        }
                        edit.change(
                            "transform",
                            Some("position"),
                            json!(round3(position)),
                            "move",
                        );
                    }
                    ui.end_row();

                    ui.label("Rotation \u{00B0}");
                    let (changed, rects) = triple(ui, &mut rotation, 0.5, editable);
                    note_triple(edit, "detail.rotation", rects);
                    if changed {
                        if let Ok(mut t) = q.transforms.get_mut(edit.entity) {
                            t.rotation = Quat::from_euler(
                                EulerRot::XYZ,
                                rotation[0].to_radians(),
                                rotation[1].to_radians(),
                                rotation[2].to_radians(),
                            );
                        }
                        edit.change(
                            "transform",
                            Some("rotation_degrees"),
                            json!(round2(rotation)),
                            "turn",
                        );
                    }
                    ui.end_row();

                    ui.label("Scale");
                    let (changed, rects) = triple(ui, &mut scale, 0.01, editable);
                    note_triple(edit, "detail.scale", rects);
                    if changed {
                        let scale = scale.map(|s| s.max(0.001));
                        if let Ok(mut t) = q.transforms.get_mut(edit.entity) {
                            t.scale = Vec3::from_array(scale);
                        }
                        edit.change("transform", Some("scale"), json!(round3(scale)), "scale");
                    }
                    ui.end_row();

                    ui.label("Visible");
                    let response =
                        ui.add_enabled(editable, egui::Checkbox::without_text(&mut visible));
                    edit.note("detail.visible", &response);
                    if response.changed() {
                        if let Ok(mut v) = q.visibility_q.get_mut(edit.entity) {
                            *v = if visible {
                                Visibility::Inherited
                            } else {
                                Visibility::Hidden
                            };
                        }
                        edit.change(
                            "transform",
                            Some("visible"),
                            json!(visible),
                            if visible { "show" } else { "hide" },
                        );
                    }
                    ui.end_row();
                });
        });
}

// ---------------------------------------------------------------------------
// Section: Shape
// ---------------------------------------------------------------------------

/// A number for the document: rounded so the log reads, an integer kept
/// an integer, never NaN.
fn json_number(v: f64, integer: bool) -> Value {
    if integer {
        return Value::from(v.round() as i64);
    }
    let v = (v * 1000.0).round() / 1000.0;
    serde_json::Number::from_f64(v)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// One JSON field as a widget: a number as a drag value, a boolean as a
/// checkbox, a string as text, a short array of numbers in a row, an
/// object as its own grid; anything else shown as it is. A key named
/// `target` or `center` with entity names to offer is a combo of them —
/// an entity reference, written as the name that binds at ingest.
fn json_field(
    ui: &mut egui::Ui,
    key: &str,
    field: &mut Value,
    enabled: bool,
    salt: &str,
    names: &[String],
) -> bool {
    if (key == "target" || key == "center") && !names.is_empty() {
        let current = match field {
            Value::String(s) => s.clone(),
            Value::Number(n) => format!("entity {n}"),
            _ => "\u{2014}".into(),
        };
        let mut pick: Option<String> = None;
        ui.add_enabled_ui(enabled, |ui| {
            egui::ComboBox::from_id_salt(salt)
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    for name in names {
                        if ui.selectable_label(current == *name, name).clicked() {
                            pick = Some(name.clone());
                        }
                    }
                });
        });
        return match pick {
            Some(name) => {
                *field = Value::String(name);
                true
            }
            None => false,
        };
    }
    match field {
        Value::Number(n) => {
            let integer = n.is_i64() || n.is_u64();
            let mut v = n.as_f64().unwrap_or(0.0);
            let changed = ui
                .add_enabled(
                    enabled,
                    egui::DragValue::new(&mut v)
                        .speed(if integer { 1.0 } else { 0.05 })
                        .max_decimals(3),
                )
                .changed();
            if changed {
                *field = json_number(v, integer);
            }
            changed
        }
        Value::Bool(b) => ui
            .add_enabled(enabled, egui::Checkbox::without_text(b))
            .changed(),
        Value::String(s) => ui
            .add_enabled(enabled, egui::TextEdit::singleline(s))
            .changed(),
        Value::Array(items)
            if !items.is_empty() && items.len() <= 4 && items.iter().all(Value::is_number) =>
        {
            let mut changed = false;
            ui.horizontal(|ui| {
                for item in items.iter_mut() {
                    let integer = item.as_i64().is_some() || item.as_u64().is_some();
                    let mut v = item.as_f64().unwrap_or(0.0);
                    if ui
                        .add_enabled(
                            enabled,
                            egui::DragValue::new(&mut v).speed(0.05).max_decimals(3),
                        )
                        .changed()
                    {
                        *item = json_number(v, integer);
                        changed = true;
                    }
                }
            });
            changed
        }
        Value::Object(_) => {
            let mut changed = false;
            ui.vertical(|ui| {
                changed = json_fields(ui, field, enabled, salt, names);
            });
            changed
        }
        other => {
            ui.monospace(other.to_string());
            false
        }
    }
}

/// A JSON object's fields as a grid of widgets, in place — what a
/// parametric shape or a behavior is, to the panel: the document's JSON,
/// field by field, so a new variant edits without a new widget. Returns
/// whether anything changed.
fn json_fields(
    ui: &mut egui::Ui,
    value: &mut Value,
    enabled: bool,
    salt: &str,
    names: &[String],
) -> bool {
    let mut changed = false;
    match value {
        Value::Object(map) => {
            egui::Grid::new(salt)
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for (key, field) in map.iter_mut() {
                        ui.label(key.as_str());
                        changed |=
                            json_field(ui, key, field, enabled, &format!("{salt}.{key}"), names);
                        ui.end_row();
                    }
                });
        }
        other => changed |= json_field(ui, "", other, enabled, salt, names),
    }
    changed
}

/// The one key of an externally tagged enum's JSON (`{"Cuboid": {…}}`)
/// and what is under it.
fn variant_of(value: &Value) -> Option<(String, Value)> {
    value
        .as_object()
        .and_then(|o| o.iter().next())
        .map(|(kind, params)| (kind.clone(), params.clone()))
}

fn draw_shape_section(ui: &mut egui::Ui, edit: &mut Edit, q: &InspectorQueries) {
    let editable = edit.editable();
    let value = if editable {
        match edit.doc.and_then(|d| d.shape.as_ref()) {
            Some(shape) => serde_json::to_value(shape).unwrap_or(Value::Null),
            None => return,
        }
    } else {
        let Ok(shape) = q.shapes.get(edit.entity) else {
            return;
        };
        serde_json::to_value(&shape.shape).unwrap_or(Value::Null)
    };
    let Some((variant, mut params)) = variant_of(&value) else {
        return;
    };

    egui::CollapsingHeader::new(format!("Shape ({variant})"))
        .default_open(true)
        .show(ui, |ui| {
            if json_fields(ui, &mut params, editable, "shape", &[]) {
                // A shape replaces whole: the scene rebuilds the mesh when
                // the commit lands.
                edit.change("shape", None, json!({ variant: params }), "reshape");
            }
        });
}

// ---------------------------------------------------------------------------
// Section: Material
// ---------------------------------------------------------------------------

/// The definition a scene material reads back as, for a panel with no
/// document to show.
fn material_of(material: &StandardMaterial) -> wt::MaterialDef {
    let c = material.base_color.to_srgba();
    let e = material.emissive;
    wt::MaterialDef {
        color: [c.red, c.green, c.blue, c.alpha],
        metallic: material.metallic,
        roughness: material.perceptual_roughness,
        emissive: [e.red, e.green, e.blue, e.alpha],
        alpha_mode: Some(match material.alpha_mode {
            AlphaMode::Opaque => wt::AlphaModeDef::Opaque,
            AlphaMode::Mask(cutoff) => wt::AlphaModeDef::Mask(cutoff),
            AlphaMode::Add => wt::AlphaModeDef::Add,
            AlphaMode::Multiply => wt::AlphaModeDef::Multiply,
            _ => wt::AlphaModeDef::Blend,
        }),
        unlit: Some(material.unlit),
        double_sided: Some(material.double_sided),
        reflectance: Some(material.reflectance),
        ..Default::default()
    }
}

fn alpha_mode_name(mode: Option<&wt::AlphaModeDef>) -> &'static str {
    match mode {
        None | Some(wt::AlphaModeDef::Opaque) => "Opaque",
        Some(wt::AlphaModeDef::Mask(_)) => "Mask",
        Some(wt::AlphaModeDef::Blend) => "Blend",
        Some(wt::AlphaModeDef::Add) => "Add",
        Some(wt::AlphaModeDef::Multiply) => "Multiply",
    }
}

fn alpha_mode_of(name: &str) -> wt::AlphaModeDef {
    match name {
        "Mask" => wt::AlphaModeDef::Mask(0.5),
        "Blend" => wt::AlphaModeDef::Blend,
        "Add" => wt::AlphaModeDef::Add,
        "Multiply" => wt::AlphaModeDef::Multiply,
        _ => wt::AlphaModeDef::Opaque,
    }
}

fn bevy_alpha_mode(def: &wt::AlphaModeDef) -> AlphaMode {
    match def {
        wt::AlphaModeDef::Opaque => AlphaMode::Opaque,
        wt::AlphaModeDef::Mask(cutoff) => AlphaMode::Mask(*cutoff),
        wt::AlphaModeDef::Blend => AlphaMode::Blend,
        wt::AlphaModeDef::Add => AlphaMode::Add,
        wt::AlphaModeDef::Multiply => AlphaMode::Multiply,
    }
}

fn draw_material_section(ui: &mut egui::Ui, edit: &mut Edit, q: &mut InspectorQueries) {
    let handle = q
        .material_handles
        .get(edit.entity)
        .ok()
        .map(|h| h.0.clone());
    let editable = edit.editable();
    let mut def = if editable {
        match edit.doc.and_then(|d| d.material.clone()) {
            Some(def) => def,
            None => return,
        }
    } else {
        let Some(handle) = &handle else {
            return;
        };
        let Some(material) = q.materials.get(handle) else {
            return;
        };
        material_of(material)
    };

    egui::CollapsingHeader::new("Material")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("material_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Color");
                    let mut rgba = def.color;
                    let response = ui
                        .add_enabled_ui(editable, |ui| {
                            ui.color_edit_button_rgba_unmultiplied(&mut rgba)
                        })
                        .inner;
                    edit.note("detail.material.color", &response);
                    if response.changed() {
                        if let Some(handle) = &handle
                            && let Some(mut m) = q.materials.get_mut(handle)
                        {
                            m.base_color = Color::srgba(rgba[0], rgba[1], rgba[2], rgba[3]);
                        }
                        edit.change("material", Some("color"), json!(round3_4(rgba)), "recolor");
                    }
                    ui.end_row();

                    ui.label("Metallic");
                    let response =
                        ui.add_enabled(editable, egui::Slider::new(&mut def.metallic, 0.0..=1.0));
                    edit.note("detail.material.metallic", &response);
                    if response.changed() {
                        if let Some(handle) = &handle
                            && let Some(mut m) = q.materials.get_mut(handle)
                        {
                            m.metallic = def.metallic;
                        }
                        edit.change(
                            "material",
                            Some("metallic"),
                            json!((def.metallic * 1000.0).round() / 1000.0),
                            "reshade",
                        );
                    }
                    ui.end_row();

                    ui.label("Roughness");
                    if ui
                        .add_enabled(editable, egui::Slider::new(&mut def.roughness, 0.0..=1.0))
                        .changed()
                    {
                        if let Some(handle) = &handle
                            && let Some(mut m) = q.materials.get_mut(handle)
                        {
                            m.perceptual_roughness = def.roughness;
                        }
                        edit.change(
                            "material",
                            Some("roughness"),
                            json!((def.roughness * 1000.0).round() / 1000.0),
                            "reshade",
                        );
                    }
                    ui.end_row();

                    ui.label("Reflectance");
                    let mut reflectance = def.reflectance.unwrap_or(0.5);
                    if ui
                        .add_enabled(editable, egui::Slider::new(&mut reflectance, 0.0..=1.0))
                        .changed()
                    {
                        if let Some(handle) = &handle
                            && let Some(mut m) = q.materials.get_mut(handle)
                        {
                            m.reflectance = reflectance;
                        }
                        edit.change(
                            "material",
                            Some("reflectance"),
                            json!((reflectance * 1000.0).round() / 1000.0),
                            "reshade",
                        );
                    }
                    ui.end_row();

                    ui.label("Emissive");
                    let mut e = [def.emissive[0], def.emissive[1], def.emissive[2]];
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        color_swatch(ui, e[0].min(1.0), e[1].min(1.0), e[2].min(1.0), 1.0);
                        for v in e.iter_mut() {
                            changed |= ui
                                .add_enabled(
                                    editable,
                                    egui::DragValue::new(v)
                                        .speed(0.05)
                                        .range(0.0..=100.0)
                                        .max_decimals(2),
                                )
                                .changed();
                        }
                    });
                    if changed {
                        let linear = LinearRgba::new(e[0], e[1], e[2], def.emissive[3]);
                        // The selection glow sits on the material; the
                        // material's own emissive is kept aside, so the
                        // edit goes there and shows once deselected.
                        if let Ok(mut kept) = q.original_emissives.get_mut(edit.entity) {
                            kept.color = linear;
                        } else if let Some(handle) = &handle
                            && let Some(mut m) = q.materials.get_mut(handle)
                        {
                            m.emissive = linear;
                        }
                        edit.change(
                            "material",
                            Some("emissive"),
                            json!(round3_4([e[0], e[1], e[2], def.emissive[3]])),
                            "relight",
                        );
                    }
                    ui.end_row();

                    ui.label("Alpha Mode");
                    let before = alpha_mode_name(def.alpha_mode.as_ref());
                    let mut mode = before;
                    let mut items: Vec<(&str, egui::Rect)> = Vec::new();
                    let combo = ui
                        .add_enabled_ui(editable, |ui| {
                            egui::ComboBox::from_id_salt("alpha_mode")
                                .selected_text(mode)
                                .show_ui(ui, |ui| {
                                    for name in ["Opaque", "Blend", "Mask", "Add", "Multiply"] {
                                        let item = ui.selectable_value(&mut mode, name, name);
                                        items.push((name, item.rect));
                                    }
                                })
                        })
                        .inner;
                    edit.note("detail.alpha_mode", &combo.response);
                    for (name, rect) in items {
                        edit.note_rect(format!("combo.alpha_mode.{name}"), rect);
                    }
                    if mode != before {
                        let new = alpha_mode_of(mode);
                        if let Some(handle) = &handle
                            && let Some(mut m) = q.materials.get_mut(handle)
                        {
                            m.alpha_mode = bevy_alpha_mode(&new);
                        }
                        edit.change(
                            "material",
                            Some("alpha_mode"),
                            serde_json::to_value(new).unwrap_or(Value::Null),
                            "reshade",
                        );
                    }
                    ui.end_row();

                    ui.label("Double Sided");
                    let mut double_sided = def.double_sided.unwrap_or(false);
                    if ui
                        .add_enabled(editable, egui::Checkbox::without_text(&mut double_sided))
                        .changed()
                    {
                        if let Some(handle) = &handle
                            && let Some(mut m) = q.materials.get_mut(handle)
                        {
                            m.double_sided = double_sided;
                        }
                        edit.change(
                            "material",
                            Some("double_sided"),
                            json!(double_sided),
                            "reshade",
                        );
                    }
                    ui.end_row();

                    ui.label("Unlit");
                    let mut unlit = def.unlit.unwrap_or(false);
                    if ui
                        .add_enabled(editable, egui::Checkbox::without_text(&mut unlit))
                        .changed()
                    {
                        if let Some(handle) = &handle
                            && let Some(mut m) = q.materials.get_mut(handle)
                        {
                            m.unlit = unlit;
                        }
                        edit.change("material", Some("unlit"), json!(unlit), "reshade");
                    }
                    ui.end_row();

                    for (label, texture) in [
                        ("Base texture", &def.base_color_texture),
                        ("Metal/rough", &def.metallic_roughness_texture),
                        ("Normal map", &def.normal_map_texture),
                    ] {
                        if let Some(path) = texture {
                            ui.label(label);
                            ui.monospace(path);
                            ui.end_row();
                        }
                    }
                });
        });
}

/// Draw a small colored rectangle as a color preview.
fn color_swatch(ui: &mut egui::Ui, r: f32, g: f32, b: f32, a: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    let color = egui::Color32::from_rgba_unmultiplied(
        (r.clamp(0.0, 1.0) * 255.0) as u8,
        (g.clamp(0.0, 1.0) * 255.0) as u8,
        (b.clamp(0.0, 1.0) * 255.0) as u8,
        (a.clamp(0.0, 1.0) * 255.0) as u8,
    );
    ui.painter().rect_filled(rect, 2, color);
}

// ---------------------------------------------------------------------------
// Section: Light
// ---------------------------------------------------------------------------

/// The definition the scene's light reads back as, for a panel with no
/// document to show.
fn light_of(entity: Entity, q: &InspectorQueries) -> Option<wt::LightDef> {
    let rgba = |color: Color| {
        let c = color.to_srgba();
        [c.red, c.green, c.blue, c.alpha]
    };
    if let Ok(light) = q.point_lights.get(entity) {
        return Some(wt::LightDef {
            light_type: wt::LightType::Point,
            color: rgba(light.color),
            intensity: light.intensity,
            shadows: light.shadow_maps_enabled,
            range: Some(light.range),
            ..Default::default()
        });
    }
    if let Ok(light) = q.spot_lights.get(entity) {
        return Some(wt::LightDef {
            light_type: wt::LightType::Spot,
            color: rgba(light.color),
            intensity: light.intensity,
            shadows: light.shadow_maps_enabled,
            range: Some(light.range),
            inner_angle: Some(light.inner_angle),
            outer_angle: Some(light.outer_angle),
            ..Default::default()
        });
    }
    if let Ok(light) = q.dir_lights.get(entity) {
        return Some(wt::LightDef {
            light_type: wt::LightType::Directional,
            color: rgba(light.color),
            intensity: light.illuminance,
            shadows: light.shadow_maps_enabled,
            ..Default::default()
        });
    }
    None
}

fn draw_light_section(ui: &mut egui::Ui, edit: &mut Edit, q: &mut InspectorQueries) {
    let editable = edit.editable();
    let def = if editable {
        match edit.doc.and_then(|d| d.light.clone()) {
            Some(def) => def,
            None => return,
        }
    } else {
        match light_of(edit.entity, q) {
            Some(def) => def,
            None => return,
        }
    };
    let kind = match def.light_type {
        wt::LightType::Directional => "Directional",
        wt::LightType::Point => "Point",
        wt::LightType::Spot => "Spot",
    };
    let entity = edit.entity;

    egui::CollapsingHeader::new(format!("Light ({kind})"))
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("light_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Color");
                    let mut rgba = def.color;
                    let changed = ui
                        .add_enabled_ui(editable, |ui| {
                            ui.color_edit_button_rgba_unmultiplied(&mut rgba)
                        })
                        .inner
                        .changed();
                    if changed {
                        let color = Color::srgba(rgba[0], rgba[1], rgba[2], rgba[3]);
                        if let Ok(mut l) = q.point_lights.get_mut(entity) {
                            l.color = color;
                        }
                        if let Ok(mut l) = q.spot_lights.get_mut(entity) {
                            l.color = color;
                        }
                        if let Ok(mut l) = q.dir_lights.get_mut(entity) {
                            l.color = color;
                        }
                        edit.change("light", Some("color"), json!(round3_4(rgba)), "relight");
                    }
                    ui.end_row();

                    ui.label(if def.light_type == wt::LightType::Directional {
                        "Illuminance"
                    } else {
                        "Intensity"
                    });
                    let mut intensity = def.intensity;
                    if ui
                        .add_enabled(
                            editable,
                            egui::DragValue::new(&mut intensity)
                                .speed(10.0)
                                .range(0.0..=f32::MAX),
                        )
                        .changed()
                    {
                        if let Ok(mut l) = q.point_lights.get_mut(entity) {
                            l.intensity = intensity;
                        }
                        if let Ok(mut l) = q.spot_lights.get_mut(entity) {
                            l.intensity = intensity;
                        }
                        if let Ok(mut l) = q.dir_lights.get_mut(entity) {
                            l.illuminance = intensity;
                        }
                        edit.change(
                            "light",
                            Some("intensity"),
                            json!(intensity.round()),
                            "relight",
                        );
                    }
                    ui.end_row();

                    if def.light_type != wt::LightType::Directional {
                        ui.label("Range");
                        let mut range = def.range.unwrap_or(20.0);
                        if ui
                            .add_enabled(
                                editable,
                                egui::DragValue::new(&mut range)
                                    .speed(0.1)
                                    .range(0.0..=f32::MAX)
                                    .suffix(" m"),
                            )
                            .changed()
                        {
                            if let Ok(mut l) = q.point_lights.get_mut(entity) {
                                l.range = range;
                            }
                            if let Ok(mut l) = q.spot_lights.get_mut(entity) {
                                l.range = range;
                            }
                            edit.change(
                                "light",
                                Some("range"),
                                json!((range * 100.0).round() / 100.0),
                                "relight",
                            );
                        }
                        ui.end_row();
                    }

                    if def.light_type == wt::LightType::Spot {
                        // The document holds radians, as Bevy does; the
                        // panel shows degrees.
                        for (label, key, value) in [
                            ("Inner Angle", "inner_angle", def.inner_angle.unwrap_or(0.0)),
                            (
                                "Outer Angle",
                                "outer_angle",
                                def.outer_angle.unwrap_or(std::f32::consts::FRAC_PI_4),
                            ),
                        ] {
                            ui.label(label);
                            let mut degrees = value.to_degrees();
                            if ui
                                .add_enabled(
                                    editable,
                                    egui::DragValue::new(&mut degrees)
                                        .speed(0.5)
                                        .range(0.0..=90.0)
                                        .suffix("\u{00B0}"),
                                )
                                .changed()
                            {
                                let radians = degrees.to_radians();
                                if let Ok(mut l) = q.spot_lights.get_mut(entity) {
                                    if key == "inner_angle" {
                                        l.inner_angle = radians;
                                    } else {
                                        l.outer_angle = radians;
                                    }
                                }
                                edit.change(
                                    "light",
                                    Some(key),
                                    json!((radians * 1000.0).round() / 1000.0),
                                    "relight",
                                );
                            }
                            ui.end_row();
                        }
                    }

                    if def.light_type == wt::LightType::Directional {
                        ui.label("Direction");
                        let mut direction = def.direction.unwrap_or([0.0, -1.0, -0.5]);
                        let (changed, rects) = triple(ui, &mut direction, 0.01, editable);
                        note_triple(edit, "detail.light.direction", rects);
                        if changed {
                            // The scene aims the light when the commit lands.
                            edit.change(
                                "light",
                                Some("direction"),
                                json!(round3(direction)),
                                "aim",
                            );
                        }
                        ui.end_row();
                    }

                    ui.label("Shadows");
                    let mut shadows = def.shadows;
                    if ui
                        .add_enabled(editable, egui::Checkbox::without_text(&mut shadows))
                        .changed()
                    {
                        if let Ok(mut l) = q.point_lights.get_mut(entity) {
                            l.shadow_maps_enabled = shadows;
                        }
                        if let Ok(mut l) = q.spot_lights.get_mut(entity) {
                            l.shadow_maps_enabled = shadows;
                        }
                        if let Ok(mut l) = q.dir_lights.get_mut(entity) {
                            l.shadow_maps_enabled = shadows;
                        }
                        edit.change("light", Some("shadows"), json!(shadows), "relight");
                    }
                    ui.end_row();
                });
        });
}

// ---------------------------------------------------------------------------
// Section: Behaviors
// ---------------------------------------------------------------------------

/// The kind and the parameters of a document behavior, from its JSON form
/// (an externally tagged enum: one key, the kind).
fn behavior_kind_and_params(def: &wt::BehaviorDef) -> (String, String) {
    let value = serde_json::to_value(def).unwrap_or(Value::Null);
    match value.as_object().and_then(|o| o.iter().next()) {
        Some((kind, params)) => (kind.clone(), params.to_string()),
        None => ("behavior".into(), value.to_string()),
    }
}

/// Why a behavior's target is a problem: it names an entity that is gone.
fn missing_target(def: &wt::BehaviorDef, doc: &openworldformat::WorldDoc) -> Option<String> {
    let target = match def {
        wt::BehaviorDef::LookAt { target } => target,
        wt::BehaviorDef::Orbit {
            center: Some(center),
            ..
        } => center,
        _ => return None,
    };
    match target {
        wt::EntityRef::Id(id) if !doc.contains(id.0) => Some(format!(
            "targets entity {}, which is gone — pick a new target or remove it",
            id.0
        )),
        wt::EntityRef::Name(name) if doc.get_by_name(name).is_none() => Some(format!(
            "targets {name}, which is gone — pick a new target or remove it"
        )),
        _ => None,
    }
}

fn draw_behaviors_section(
    ui: &mut egui::Ui,
    edit: &mut Edit,
    behaviors_q: &Query<&EntityBehaviors>,
) {
    if edit.editable() {
        let Some(doc) = edit.doc else {
            return;
        };
        if doc.behaviors.is_empty() {
            return;
        }
        // Every entity's name, for a target to pick from.
        let names: Vec<String> = edit
            .live
            .as_ref()
            .map(|live| live.doc.entities().map(|e| e.name.0.clone()).collect())
            .unwrap_or_default();
        egui::CollapsingHeader::new(format!("Behaviors ({})", doc.behaviors.len()))
            .default_open(true)
            .show(ui, |ui| {
                for (n, def) in doc.behaviors.iter().enumerate() {
                    let (kind, _) = behavior_kind_and_params(def);
                    let problem = edit
                        .live
                        .as_ref()
                        .and_then(|live| missing_target(def, live.doc));
                    egui::CollapsingHeader::new(format!("{n}: {kind}"))
                        .id_salt(("behavior", n))
                        .default_open(problem.is_some())
                        .show(ui, |ui| {
                            // The parameters, field by field; a change
                            // sends the whole list back with this one
                            // replaced — `behaviors` patches whole.
                            let mut params = serde_json::to_value(def)
                                .ok()
                                .and_then(|v| variant_of(&v))
                                .map(|(_, p)| p)
                                .unwrap_or(Value::Null);
                            if json_fields(ui, &mut params, true, &format!("behavior{n}"), &names) {
                                let mut all: Vec<Value> = doc
                                    .behaviors
                                    .iter()
                                    .map(|d| serde_json::to_value(d).unwrap_or(Value::Null))
                                    .collect();
                                all[n] = json!({ kind.clone(): params });
                                edit.change(
                                    "behaviors",
                                    None,
                                    Value::Array(all),
                                    "retune a behavior of",
                                );
                            }
                            if let Some(problem) = &problem {
                                ui.colored_label(WARN, format!("\u{26A0} {problem}"));
                            }
                            let remove = ui.button("Remove this behavior");
                            edit.note(format!("detail.behavior.{n}.remove"), &remove);
                            if remove.clicked() {
                                let remaining: Vec<&wt::BehaviorDef> = doc
                                    .behaviors
                                    .iter()
                                    .enumerate()
                                    .filter(|(i, _)| *i != n)
                                    .map(|(_, d)| d)
                                    .collect();
                                edit.change(
                                    "behaviors",
                                    None,
                                    serde_json::to_value(remaining).unwrap_or_else(|_| json!([])),
                                    "drop a behavior of",
                                );
                            }
                        });
                }
            });
        return;
    }

    let Ok(behaviors) = behaviors_q.get(edit.entity) else {
        return;
    };

    if behaviors.behaviors.is_empty() {
        return;
    }

    egui::CollapsingHeader::new(format!("Behaviors ({})", behaviors.behaviors.len()))
        .default_open(true)
        .show(ui, |ui| {
            for beh in &behaviors.behaviors {
                egui::CollapsingHeader::new(format!(
                    "{}: {}",
                    beh.id,
                    behavior_type_name(&beh.def)
                ))
                .default_open(false)
                .show(ui, |ui| {
                    egui::Grid::new(format!("behavior_{}", beh.id))
                        .num_columns(2)
                        .spacing([8.0, 4.0])
                        .show(ui, |ui| {
                            ui.label("ID");
                            ui.label(&beh.id);
                            ui.end_row();

                            ui.label("Type");
                            ui.label(behavior_type_name(&beh.def));
                            ui.end_row();

                            ui.label("Base Pos");
                            ui.label(format!(
                                "[{:.2}, {:.2}, {:.2}]",
                                beh.base_position.x, beh.base_position.y, beh.base_position.z
                            ));
                            ui.end_row();

                            ui.label("Base Scale");
                            ui.label(format!(
                                "[{:.2}, {:.2}, {:.2}]",
                                beh.base_scale.x, beh.base_scale.y, beh.base_scale.z
                            ));
                            ui.end_row();

                            // Show behavior-specific params
                            ui.label("Params");
                            ui.label(format!("{:?}", beh.def));
                            ui.end_row();
                        });
                });
            }
        });
}

/// Get the variant name of a scene BehaviorDef.
fn behavior_type_name(def: &GenBehaviorDef) -> &'static str {
    match def {
        GenBehaviorDef::Orbit { .. } => "orbit",
        GenBehaviorDef::Spin { .. } => "spin",
        GenBehaviorDef::Bob { .. } => "bob",
        GenBehaviorDef::LookAt { .. } => "look_at",
        GenBehaviorDef::Pulse { .. } => "pulse",
        GenBehaviorDef::PathFollow { .. } => "path_follow",
        GenBehaviorDef::Bounce { .. } => "bounce",
    }
}

// ---------------------------------------------------------------------------
// Section: Audio
// ---------------------------------------------------------------------------

fn draw_audio_section(ui: &mut egui::Ui, entity_name: &str, audio_engine: Option<&AudioEngine>) {
    let Some(engine) = audio_engine else {
        return;
    };

    // Try direct key lookup first, then search by attached_to
    let meta = engine.emitter_meta.get(entity_name).or_else(|| {
        engine
            .emitter_meta
            .values()
            .find(|m| m.attached_to.as_deref() == Some(entity_name))
    });
    let Some(meta) = meta else {
        return;
    };

    egui::CollapsingHeader::new("Audio")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("audio_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Sound Type");
                    ui.label(&meta.sound_type);
                    ui.end_row();

                    ui.label("Volume");
                    ui.label(format!("{:.2}", meta.base_volume));
                    ui.end_row();

                    ui.label("Radius");
                    ui.label(format!("{:.1}m", meta.radius));
                    ui.end_row();

                    if let Some(ref attached) = meta.attached_to {
                        ui.label("Attached To");
                        ui.label(attached);
                        ui.end_row();
                    }

                    if let Some(pos) = meta.position {
                        ui.label("Position");
                        ui.label(format!("[{:.2}, {:.2}, {:.2}]", pos[0], pos[1], pos[2]));
                        ui.end_row();
                    }
                });
        });
}

// ---------------------------------------------------------------------------
// Section: Mesh Asset
// ---------------------------------------------------------------------------

fn draw_mesh_section(ui: &mut egui::Ui, entity: Entity, gltf_sources: &Query<&GltfSource>) {
    let Ok(source) = gltf_sources.get(entity) else {
        return;
    };

    egui::CollapsingHeader::new("Mesh Asset")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("mesh_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Path");
                    ui.label(&source.path);
                    ui.end_row();
                });
        });
}

// ---------------------------------------------------------------------------
// Section: Hierarchy
// ---------------------------------------------------------------------------

/// The intent that hangs `child` under `parent` (`None`: at the root) where
/// it stands: a new parent, and the local transform that keeps its world
/// placement — what an editor's drag between tree rows means.
pub(crate) fn reparent_intent(
    q: &InspectorQueries,
    child: Entity,
    id: u64,
    name: &str,
    parent: Option<(Entity, u64, &str)>,
) -> Option<PersonIntent> {
    let child_global = q.globals.get(child).ok()?;
    let local = match parent {
        Some((entity, _, _)) => child_global.reparented_to(q.globals.get(entity).ok()?),
        None => child_global.compute_transform(),
    };
    let (x, y, z) = local.rotation.to_euler(EulerRot::XYZ);
    let patch = json!({
        "parent": parent.map(|(_, id, _)| id),
        "transform": {
            "position": round3(local.translation.to_array()),
            "rotation_degrees": round2([x.to_degrees(), y.to_degrees(), z.to_degrees()]),
            "scale": round3(local.scale.to_array()),
        }
    });
    let what = match parent {
        Some((_, _, parent_name)) => format!("parent {name} to {parent_name}"),
        None => format!("unparent {name}"),
    };
    Some(PersonIntent::Patch {
        id,
        name: name.to_string(),
        patch,
        what,
    })
}

fn draw_hierarchy_section(
    ui: &mut egui::Ui,
    edit: &mut Edit,
    registry: &NameRegistry,
    q: &InspectorQueries,
    selection: &mut InspectorSelection,
) {
    let parent_entity = q
        .parent_q
        .get(edit.entity)
        .ok()
        .map(|p| p.parent())
        .filter(|p| q.gen_entities.contains(*p));
    let parent_name = parent_entity
        .and_then(|p| registry.get_name(p))
        .map(|s| s.to_string());

    let children: Vec<(Entity, String)> = q
        .children_q
        .get(edit.entity)
        .map(|ch| {
            ch.iter()
                .filter_map(|child| {
                    // Only show Gen-managed children
                    q.gen_entities.get(child).ok()?;
                    registry.get_name(child).map(|s| (child, s.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    let editable = edit.editable();
    if parent_name.is_none() && children.is_empty() && !editable {
        return;
    }

    egui::CollapsingHeader::new("Hierarchy")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("hierarchy_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Parent");
                    match (edit.doc, edit.live.as_mut(), edit.id) {
                        (Some(doc), Some(live), Some(id)) => {
                            // Candidates: everything but this entity and
                            // what hangs under it.
                            let under: HashSet<u64> = live
                                .doc
                                .subtree_entities_parent_first(id)
                                .iter()
                                .map(|e| e.id.0)
                                .collect();
                            let mut candidates: Vec<(u64, String)> = live
                                .doc
                                .entities()
                                .filter(|e| !under.contains(&e.id.0))
                                .map(|e| (e.id.0, e.name.0.clone()))
                                .collect();
                            candidates.sort_by_key(|(_, name)| name.to_lowercase());
                            let before: Option<u64> = doc.parent.map(|p| p.0);
                            let mut choice = before;
                            let mut items: Vec<(String, egui::Rect)> = Vec::new();
                            let combo = egui::ComboBox::from_id_salt("parent")
                                .selected_text(
                                    parent_name.clone().unwrap_or_else(|| "— none —".into()),
                                )
                                .show_ui(ui, |ui| {
                                    let none = ui.selectable_value(&mut choice, None, "— none —");
                                    items.push(("— none —".into(), none.rect));
                                    for (candidate, name) in &candidates {
                                        let item = ui.selectable_value(
                                            &mut choice,
                                            Some(*candidate),
                                            name,
                                        );
                                        items.push((name.clone(), item.rect));
                                    }
                                });
                            live.rects.note("detail.parent", &combo.response);
                            for (name, rect) in items {
                                live.rects.note_rect(format!("combo.parent.{name}"), rect);
                            }
                            if choice != before {
                                let parent = choice.and_then(|pid| {
                                    let entity = registry.get_entity_by_id(&wt::EntityId(pid))?;
                                    let name = candidates
                                        .iter()
                                        .find(|(c, _)| *c == pid)
                                        .map(|(_, n)| n.as_str())?;
                                    Some((entity, pid, name))
                                });
                                if (choice.is_none() || parent.is_some())
                                    && let Some(intent) =
                                        reparent_intent(q, edit.entity, id, edit.name, parent)
                                {
                                    live.edit.flush(live.intents);
                                    live.intents.push(intent);
                                }
                            }
                        }
                        _ => {
                            ui.label(parent_name.clone().unwrap_or_else(|| "— none —".into()));
                        }
                    }
                    ui.end_row();

                    if !children.is_empty() {
                        ui.label("Children");
                        ui.vertical(|ui| {
                            for (child, child_name) in &children {
                                if ui.link(child_name).clicked() {
                                    selection.set(Some(*child));
                                }
                            }
                        });
                        ui.end_row();
                    }
                });
        });
}
