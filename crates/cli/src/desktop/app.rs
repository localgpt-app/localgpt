//! Main eframe application

use eframe::egui;

use super::state::{Panel, UiState};
use super::views::{ChatView, SessionsView, StatusView, chat::show_toolbar};
use super::worker::WorkerHandle;

/// The main desktop application
pub struct DesktopApp {
    state: UiState,
    worker: WorkerHandle,
}

impl DesktopApp {
    /// Create a new desktop app
    pub fn new(cc: &eframe::CreationContext<'_>, agent_id: Option<String>) -> Self {
        // Configure fonts and visuals
        Self::configure_style(&cc.egui_ctx);

        // Start the background worker
        let worker = WorkerHandle::start(agent_id).expect("Failed to start worker");

        Self {
            state: UiState::new(),
            worker,
        }
    }

    fn configure_style(ctx: &egui::Context) {
        let mut style = (*ctx.global_style()).clone();

        // Use slightly larger text
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(20.0, egui::FontFamily::Proportional),
        );

        // Rounded corners
        style.visuals.window_corner_radius = egui::CornerRadius::same(8);
        style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(4);
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);

        ctx.set_global_style(style);
    }

    /// Process all pending worker messages
    fn process_worker_messages(&mut self) {
        while let Some(msg) = self.worker.try_recv() {
            self.state.handle_worker_message(msg);
        }
    }
}

impl eframe::App for DesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Process worker messages
        self.process_worker_messages();

        // Request repaint while loading or streaming
        if self.state.is_loading || !self.state.streaming_content.is_empty() {
            ui.ctx().request_repaint();
        }

        // Top panel with toolbar
        egui::Panel::top("toolbar").show_inside(ui, |ui| {
            show_toolbar(ui, &mut self.state);
        });

        // Main content
        egui::CentralPanel::default().show_inside(ui, |ui| {
            let msg = match self.state.active_panel {
                Panel::Chat => ChatView::show(ui, &mut self.state),
                Panel::Sessions => SessionsView::show(ui, &mut self.state),
                Panel::Status => StatusView::show(ui, &mut self.state),
            };

            // Send any UI messages to worker
            if let Some(msg) = msg
                && let Err(e) = self.worker.send(msg)
            {
                self.state.error = Some(format!("Failed to send to worker: {}", e));
            }
        });
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        // Could save window position, etc.
    }
}
