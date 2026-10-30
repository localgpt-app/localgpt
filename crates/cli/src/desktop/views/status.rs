//! Status view - show model, memory, and session stats

use eframe::egui::{Color32, ProgressBar, RichText, Ui};
use localgpt_core::text::prefix_chars;

use crate::desktop::state::{UiMessage, UiState};

pub struct StatusView;

impl StatusView {
    pub fn show(ui: &mut Ui, state: &mut UiState) -> Option<UiMessage> {
        let mut message_to_send = None;

        ui.heading("Status");
        ui.add_space(10.0);

        // Refresh button
        if ui.button("Refresh").clicked() {
            message_to_send = Some(UiMessage::RefreshStatus);
        }

        ui.add_space(10.0);

        // Model info
        ui.group(|ui| {
            ui.label(RichText::new("Model").strong());
            ui.label(&state.model);
        });

        ui.add_space(10.0);

        // Memory info
        ui.group(|ui| {
            ui.label(RichText::new("Memory").strong());
            ui.label(format!("Chunks: {}", state.memory_chunks));
            ui.horizontal(|ui| {
                ui.label("Embeddings:");
                if state.has_embeddings {
                    ui.label(RichText::new("enabled").color(Color32::from_rgb(46, 204, 113)));
                } else {
                    ui.label(RichText::new("disabled").color(Color32::GRAY));
                }
            });
        });

        ui.add_space(10.0);

        // Session info
        if let Some(ref status) = state.status {
            ui.group(|ui| {
                let short_id = prefix_chars(&status.id, 8);
                ui.label(RichText::new("Session").strong());
                ui.label(format!("ID: {short_id}..."));
                ui.label(format!("Messages: {}", status.message_count));
                ui.label(format!("Compactions: {}", status.compaction_count));

                // Token usage bar
                ui.add_space(5.0);
                ui.label("Context usage:");
                let token_pct = status.token_count as f32 / 128000.0; // Assume 128k context
                ui.add(
                    ProgressBar::new(token_pct.min(1.0))
                        .text(format!("~{} tokens", status.token_count)),
                );

                if token_pct > 0.8 {
                    ui.label(
                        RichText::new("Context nearly full. Consider starting a new session.")
                            .color(Color32::from_rgb(231, 76, 60))
                            .small(),
                    );
                }
            });

            ui.add_space(10.0);

            // API usage
            if status.api_input_tokens > 0 || status.api_output_tokens > 0 {
                ui.group(|ui| {
                    ui.label(RichText::new("API Usage (Session)").strong());
                    ui.label(format!("Input: {} tokens", status.api_input_tokens));
                    ui.label(format!("Output: {} tokens", status.api_output_tokens));
                    ui.label(format!(
                        "Total: {} tokens",
                        status.api_input_tokens + status.api_output_tokens
                    ));
                });
            }

            if status.search_queries > 0 {
                ui.add_space(10.0);
                ui.group(|ui| {
                    ui.label(RichText::new("Web Search (Session)").strong());
                    ui.label(format!("Queries: {}", status.search_queries));
                    let cache_pct =
                        (status.search_cached_hits as f32 / status.search_queries as f32) * 100.0;
                    ui.label(format!(
                        "Cached: {} ({:.0}%)",
                        status.search_cached_hits, cache_pct
                    ));
                    ui.label(format!("Estimated cost: ${:.3}", status.search_cost_usd));
                });
            }
        }

        message_to_send
    }
}
