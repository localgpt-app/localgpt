//! Egui web application

use eframe::egui;

/// The main web application
#[derive(Default)]
pub struct WebApp {
    message_input: String,
    messages: Vec<Message>,
    session_id: Option<String>,
    status: Status,
}

#[derive(Clone)]
struct Message {
    role: String,
    content: String,
}

#[derive(Default)]
struct Status {
    connected: bool,
    model: String,
    version: String,
}

impl WebApp {
    /// Create a new web app
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            message_input: String::new(),
            messages: Vec::new(),
            session_id: None,
            status: Status {
                connected: true,
                model: "claude-cli/opus".to_string(),
                version: "0.3.0".to_string(),
            },
        }
    }
}

impl eframe::App for WebApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top panel with toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("LocalGPT");
                ui.separator();
                
                // Status indicator
                let status_color = if self.status.connected {
                    egui::Color32::GREEN
                } else {
                    egui::Color32::RED
                };
                ui.colored_label(status_color, "●");
                
                ui.label(format!("Model: {}", self.status.model));
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("New Session").clicked() {
                        self.messages.clear();
                        self.session_id = None;
                    }
                    
                    if let Some(ref id) = self.session_id {
                        // Safe truncation that respects UTF-8 character boundaries
                        let truncated: String = id.chars().take(8).collect();
                        ui.label(format!("Session: {}...", truncated));
                    }
                });
            });
        });

        // Main chat area
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if self.messages.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(100.0);
                            ui.heading("Welcome to LocalGPT");
                            ui.label("This is a Proof of Concept egui web UI");
                            ui.add_space(10.0);
                            ui.label("Type a message below to start chatting");
                            ui.add_space(10.0);
                            ui.label("🚧 Note: This is a static demo without backend connection");
                        });
                    } else {
                        for msg in &self.messages {
                            self.render_message(ui, msg);
                        }
                    }
                });
        });

        // Bottom input panel
        egui::TopBottomPanel::bottom("input").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let input_response = ui.add(
                    egui::TextEdit::multiline(&mut self.message_input)
                        .desired_width(f32::INFINITY)
                        .desired_rows(1)
                        .hint_text("Type a message..."),
                );

                let enter_without_shift = input_response.has_focus() 
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);

                if ui.button("Send").clicked() || enter_without_shift {
                    self.send_message();
                    input_response.request_focus();
                }
            });
        });
    }
}

impl WebApp {
    fn render_message(&self, ui: &mut egui::Ui, msg: &Message) {
        let bg_color = if msg.role == "user" {
            egui::Color32::from_rgb(40, 40, 60)
        } else {
            egui::Color32::from_rgb(30, 50, 40)
        };

        egui::Frame::none()
            .fill(bg_color)
            .inner_margin(egui::Margin::same(10.0))
            .corner_radius(egui::CornerRadius::same(5.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let role_color = if msg.role == "user" {
                        egui::Color32::LIGHT_BLUE
                    } else {
                        egui::Color32::LIGHT_GREEN
                    };
                    ui.colored_label(role_color, &msg.role);
                });
                ui.label(&msg.content);
            });

        ui.add_space(8.0);
    }

    fn send_message(&mut self) {
        if self.message_input.trim().is_empty() {
            return;
        }

        // Add user message
        self.messages.push(Message {
            role: "user".to_string(),
            content: self.message_input.clone(),
        });

        // Add a mock response
        self.messages.push(Message {
            role: "assistant".to_string(),
            content: format!(
                "This is a PoC demo. Your message was: \"{}\"\n\n\
                 In the full implementation, this would connect to the LocalGPT backend \
                 via WebSocket or HTTP API to send your message and stream the response.",
                self.message_input
            ),
        });

        self.message_input.clear();
    }
}
