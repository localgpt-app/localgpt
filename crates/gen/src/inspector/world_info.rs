//! World Info bar — compact bottom bar showing global world state.

use bevy_egui::egui;

use crate::gen3d::audio::AudioEngine;
use crate::gen3d::behaviors::BehaviorState;
use crate::gen3d::plugin::CurrentWorld;
use crate::gen3d::registry::NameRegistry;

// egui 0.34 deprecates `Panel::show(ctx)` in favour of `show_inside(ui)`, but
// bevy_egui only exposes a `Context` (not a root `Ui`), so the context-level
// `show` is still the correct entry point here.
#[allow(deprecated)]
pub fn draw_world_info(
    ctx: &egui::Context,
    registry: &NameRegistry,
    behavior_state: &BehaviorState,
    audio_engine: Option<&AudioEngine>,
    current_world: &CurrentWorld,
) {
    egui::Panel::bottom("inspector_world_info")
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
            });
        });
}
