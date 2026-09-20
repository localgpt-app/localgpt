//! Detail panel — shows all components of the selected entity.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::gen3d::audio::AudioEngine;
use crate::gen3d::behaviors::EntityBehaviors;
use crate::gen3d::commands::BehaviorDef;
use crate::gen3d::registry::*;

use super::{InspectorQueries, InspectorSelection};

/// Get the variant name of a BehaviorDef.
fn behavior_type_name(def: &BehaviorDef) -> &'static str {
    match def {
        BehaviorDef::Orbit { .. } => "orbit",
        BehaviorDef::Spin { .. } => "spin",
        BehaviorDef::Bob { .. } => "bob",
        BehaviorDef::LookAt { .. } => "look_at",
        BehaviorDef::Pulse { .. } => "pulse",
        BehaviorDef::PathFollow { .. } => "path_follow",
        BehaviorDef::Bounce { .. } => "bounce",
    }
}

// ---------------------------------------------------------------------------
// Main draw function
// ---------------------------------------------------------------------------

// egui 0.34 deprecates `Panel::show(ctx)` in favour of `show_inside(ui)`, but
// bevy_egui only exposes a `Context` (not a root `Ui`), so the context-level
// `show` is still the correct entry point here.
#[allow(deprecated)]
pub fn draw_detail(
    ctx: &egui::Context,
    selection: &InspectorSelection,
    registry: &NameRegistry,
    q: &InspectorQueries,
) {
    egui::Panel::right("inspector_detail")
        .default_size(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            let Some(entity) = selection.entity else {
                ui.heading("Inspector");
                ui.separator();
                ui.weak("Select an entity to inspect");
                return;
            };

            // Verify entity still exists in registry
            let Some(name) = registry.get_name(entity) else {
                ui.heading("Inspector");
                ui.separator();
                ui.weak("Entity no longer exists");
                return;
            };

            ui.heading(name);
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    draw_identity_section(ui, entity, name, registry, &q.gen_entities);
                    draw_transform_section(ui, entity, &q.transforms, &q.visibility_q);
                    draw_shape_section(ui, entity, &q.shapes);
                    draw_material_section(ui, entity, &q.material_handles, &q.materials);
                    draw_light_section(ui, entity, &q.point_lights, &q.dir_lights, &q.spot_lights);
                    draw_behaviors_section(ui, entity, &q.behaviors_q);
                    draw_audio_section(ui, name, q.audio_engine.as_deref());
                    draw_mesh_section(ui, entity, &q.gltf_sources);
                    draw_hierarchy_section(
                        ui,
                        entity,
                        registry,
                        &q.children_q,
                        &q.parent_q,
                        &q.gen_entities,
                    );
                });
        });
}

// ---------------------------------------------------------------------------
// Section: Identity
// ---------------------------------------------------------------------------

fn draw_identity_section(
    ui: &mut egui::Ui,
    entity: Entity,
    name: &str,
    registry: &NameRegistry,
    gen_entities: &Query<&GenEntity>,
) {
    egui::CollapsingHeader::new("Identity")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("identity_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    ui.label(name);
                    ui.end_row();

                    if let Ok(gen_e) = gen_entities.get(entity) {
                        ui.label("Type");
                        ui.label(gen_e.entity_type.as_str());
                        ui.end_row();

                        ui.label("World ID");
                        ui.label(format!("{}", gen_e.world_id.0));
                        ui.end_row();
                    }

                    if let Some(id) = registry.get_id(entity) {
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

fn draw_transform_section(
    ui: &mut egui::Ui,
    entity: Entity,
    transforms: &Query<&Transform>,
    visibility_q: &Query<&Visibility>,
) {
    let Ok(transform) = transforms.get(entity) else {
        return;
    };

    egui::CollapsingHeader::new("Transform")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("transform_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    let pos = transform.translation;
                    ui.label("Position");
                    ui.label(format!("[{:.3}, {:.3}, {:.3}]", pos.x, pos.y, pos.z));
                    ui.end_row();

                    let (x, y, z) = transform.rotation.to_euler(EulerRot::XYZ);
                    ui.label("Rotation");
                    ui.label(format!(
                        "[{:.1}, {:.1}, {:.1}]",
                        x.to_degrees(),
                        y.to_degrees(),
                        z.to_degrees()
                    ));
                    ui.end_row();

                    let scale = transform.scale;
                    ui.label("Scale");
                    ui.label(format!("[{:.3}, {:.3}, {:.3}]", scale.x, scale.y, scale.z));
                    ui.end_row();

                    if let Ok(vis) = visibility_q.get(entity) {
                        ui.label("Visible");
                        ui.label(format!("{:?}", vis));
                        ui.end_row();
                    }
                });
        });
}

// ---------------------------------------------------------------------------
// Section: Shape
// ---------------------------------------------------------------------------

fn draw_shape_section(ui: &mut egui::Ui, entity: Entity, shapes: &Query<&ParametricShape>) {
    let Ok(shape) = shapes.get(entity) else {
        return;
    };

    egui::CollapsingHeader::new("Shape")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("shape_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Variant");
                    ui.label(format!("{:?}", shape.shape));
                    ui.end_row();
                });
        });
}

// ---------------------------------------------------------------------------
// Section: Material
// ---------------------------------------------------------------------------

fn draw_material_section(
    ui: &mut egui::Ui,
    entity: Entity,
    material_handles: &Query<&MeshMaterial3d<StandardMaterial>>,
    materials: &Assets<StandardMaterial>,
) {
    let Ok(mat_handle) = material_handles.get(entity) else {
        return;
    };

    let Some(material) = materials.get(&mat_handle.0) else {
        return;
    };

    egui::CollapsingHeader::new("Material")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("material_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    let color = material.base_color;
                    ui.label("Base Color");
                    ui.horizontal(|ui| {
                        let c = color.to_srgba();
                        color_swatch(ui, c.red, c.green, c.blue, c.alpha);
                        ui.label(format!(
                            "[{:.2}, {:.2}, {:.2}, {:.2}]",
                            c.red, c.green, c.blue, c.alpha
                        ));
                    });
                    ui.end_row();

                    ui.label("Metallic");
                    ui.label(format!("{:.3}", material.metallic));
                    ui.end_row();

                    ui.label("Roughness");
                    ui.label(format!("{:.3}", material.perceptual_roughness));
                    ui.end_row();

                    ui.label("Reflectance");
                    ui.label(format!("{:.3}", material.reflectance));
                    ui.end_row();

                    let e = material.emissive;
                    ui.label("Emissive");
                    ui.horizontal(|ui| {
                        color_swatch(ui, e.red, e.green, e.blue, 1.0);
                        ui.label(format!("[{:.2}, {:.2}, {:.2}]", e.red, e.green, e.blue));
                    });
                    ui.end_row();

                    ui.label("Alpha Mode");
                    ui.label(format!("{:?}", material.alpha_mode));
                    ui.end_row();

                    ui.label("Double Sided");
                    ui.label(format!("{}", material.double_sided));
                    ui.end_row();

                    ui.label("Unlit");
                    ui.label(format!("{}", material.unlit));
                    ui.end_row();
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

fn draw_light_section(
    ui: &mut egui::Ui,
    entity: Entity,
    point_lights: &Query<&PointLight>,
    dir_lights: &Query<&DirectionalLight>,
    spot_lights: &Query<&SpotLight>,
) {
    if let Ok(light) = point_lights.get(entity) {
        egui::CollapsingHeader::new("Light (Point)")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("light_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Color");
                        let c = light.color.to_srgba();
                        ui.horizontal(|ui| {
                            color_swatch(ui, c.red, c.green, c.blue, 1.0);
                            ui.label(format!("[{:.2}, {:.2}, {:.2}]", c.red, c.green, c.blue));
                        });
                        ui.end_row();

                        ui.label("Intensity");
                        ui.label(format!("{:.1}", light.intensity));
                        ui.end_row();

                        ui.label("Range");
                        ui.label(format!("{:.2}", light.range));
                        ui.end_row();

                        ui.label("Shadows");
                        ui.label(format!("{}", light.shadow_maps_enabled));
                        ui.end_row();
                    });
            });
    }

    if let Ok(light) = dir_lights.get(entity) {
        egui::CollapsingHeader::new("Light (Directional)")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("dirlight_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Color");
                        let c = light.color.to_srgba();
                        ui.horizontal(|ui| {
                            color_swatch(ui, c.red, c.green, c.blue, 1.0);
                            ui.label(format!("[{:.2}, {:.2}, {:.2}]", c.red, c.green, c.blue));
                        });
                        ui.end_row();

                        ui.label("Illuminance");
                        ui.label(format!("{:.1}", light.illuminance));
                        ui.end_row();

                        ui.label("Shadows");
                        ui.label(format!("{}", light.shadow_maps_enabled));
                        ui.end_row();
                    });
            });
    }

    if let Ok(light) = spot_lights.get(entity) {
        egui::CollapsingHeader::new("Light (Spot)")
            .default_open(true)
            .show(ui, |ui| {
                egui::Grid::new("spotlight_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Color");
                        let c = light.color.to_srgba();
                        ui.horizontal(|ui| {
                            color_swatch(ui, c.red, c.green, c.blue, 1.0);
                            ui.label(format!("[{:.2}, {:.2}, {:.2}]", c.red, c.green, c.blue));
                        });
                        ui.end_row();

                        ui.label("Intensity");
                        ui.label(format!("{:.1}", light.intensity));
                        ui.end_row();

                        ui.label("Range");
                        ui.label(format!("{:.2}", light.range));
                        ui.end_row();

                        ui.label("Inner Angle");
                        ui.label(format!("{:.1}\u{00B0}", light.inner_angle.to_degrees()));
                        ui.end_row();

                        ui.label("Outer Angle");
                        ui.label(format!("{:.1}\u{00B0}", light.outer_angle.to_degrees()));
                        ui.end_row();

                        ui.label("Shadows");
                        ui.label(format!("{}", light.shadow_maps_enabled));
                        ui.end_row();
                    });
            });
    }
}

// ---------------------------------------------------------------------------
// Section: Behaviors
// ---------------------------------------------------------------------------

fn draw_behaviors_section(
    ui: &mut egui::Ui,
    entity: Entity,
    behaviors_q: &Query<&EntityBehaviors>,
) {
    let Ok(behaviors) = behaviors_q.get(entity) else {
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

fn draw_hierarchy_section(
    ui: &mut egui::Ui,
    entity: Entity,
    registry: &NameRegistry,
    children_q: &Query<&Children>,
    parent_q: &Query<&ChildOf>,
    gen_entities: &Query<&GenEntity>,
) {
    let parent_name = parent_q
        .get(entity)
        .ok()
        .and_then(|p| registry.get_name(p.parent()))
        .map(|s| s.to_string());

    let children_names: Vec<String> = children_q
        .get(entity)
        .map(|ch| {
            ch.iter()
                .filter_map(|child| {
                    // Only show Gen-managed children
                    gen_entities.get(child).ok()?;
                    registry.get_name(child).map(|s| s.to_string())
                })
                .collect()
        })
        .unwrap_or_default();

    if parent_name.is_none() && children_names.is_empty() {
        return;
    }

    egui::CollapsingHeader::new("Hierarchy")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("hierarchy_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    if let Some(ref parent) = parent_name {
                        ui.label("Parent");
                        ui.label(parent);
                        ui.end_row();
                    }

                    if !children_names.is_empty() {
                        ui.label("Children");
                        ui.vertical(|ui| {
                            for child_name in &children_names {
                                ui.label(child_name);
                            }
                        });
                        ui.end_row();
                    }
                });
        });
}
