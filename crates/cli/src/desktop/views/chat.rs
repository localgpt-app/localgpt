//! Chat view - message display and input

use eframe::egui::{self, Color32, RichText, ScrollArea, TextEdit, Ui};

use crate::desktop::state::{ChatMessage, MessageRole, Panel, ToolStatus, UiMessage, UiState};

pub struct ChatView;

impl ChatView {
    pub fn show(ui: &mut Ui, state: &mut UiState) -> Option<UiMessage> {
        let mut message_to_send = None;

        // Main chat area
        let available_height = ui.available_height() - 60.0; // Reserve space for input

        // Messages scroll area
        ScrollArea::vertical()
            .id_salt("chat_messages")
            .max_height(available_height)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                // Show messages
                for msg in &state.messages {
                    Self::render_message(ui, msg);
                    ui.add_space(8.0);
                }

                // Show streaming content if any
                if !state.streaming_content.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Assistant")
                                .strong()
                                .color(Color32::from_rgb(100, 149, 237)),
                        );
                    });
                    ui.label(&state.streaming_content);
                    ui.add_space(8.0);
                }

                // Show active tools
                for tool in &state.active_tools {
                    ui.horizontal(|ui| match &tool.status {
                        ToolStatus::Running => {
                            ui.spinner();
                            if let Some(ref detail) = tool.detail {
                                ui.label(format!("Running: {}: {}", tool.name, detail));
                            } else {
                                ui.label(format!("Running: {}", tool.name));
                            }
                        }
                        ToolStatus::Completed(preview) => {
                            ui.label(RichText::new("Done").color(Color32::from_rgb(46, 204, 113)));
                            ui.label(format!("{}: {}", tool.name, preview));
                        }
                        ToolStatus::Error(err) => {
                            ui.label(RichText::new("Error").color(Color32::from_rgb(231, 76, 60)));
                            ui.label(format!("{}: {}", tool.name, err));
                        }
                    });
                }

                // Show pending approval dialog
                if state.pending_approval.is_some() {
                    let tools = state.pending_approval.clone().unwrap();
                    ui.add_space(10.0);
                    ui.group(|ui| {
                        ui.label(RichText::new("Tools pending approval:").strong());
                        for tool in &tools {
                            ui.label(format!("  - {}", tool.name));
                        }
                        ui.horizontal(|ui| {
                            if ui.button("Approve").clicked() {
                                message_to_send = Some(UiMessage::ApproveTools(tools.clone()));
                                state.pending_approval = None;
                            }
                            if ui.button("Deny").clicked() {
                                message_to_send = Some(UiMessage::DenyTools);
                                state.pending_approval = None;
                            }
                        });
                    });
                }

                // Scroll to bottom if requested
                if state.scroll_to_bottom {
                    ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                    state.scroll_to_bottom = false;
                }
            });

        // Error display
        if state.error.is_some() {
            let error = state.error.clone().unwrap();
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Error: ").color(Color32::from_rgb(231, 76, 60)));
                ui.label(&error);
                if ui.small_button("Dismiss").clicked() {
                    state.clear_error();
                }
            });
        }

        ui.add_space(10.0);

        // Input area
        ui.horizontal(|ui| {
            let input_response = ui.add_sized(
                [ui.available_width() - 70.0, 35.0],
                TextEdit::singleline(&mut state.input)
                    .hint_text("Type a message or /help for commands..."),
            );

            let can_send = !state.input.trim().is_empty() && !state.is_loading;
            let send_clicked = ui
                .add_enabled(can_send, egui::Button::new("Send"))
                .clicked();

            // Send on Enter or button click
            let enter_pressed =
                input_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

            if (send_clicked || enter_pressed) && can_send {
                let content = state.input.trim().to_string();
                state.input.clear();

                if let Some(cmd) = Self::parse_slash_command(&content, state) {
                    message_to_send = Some(cmd);
                } else {
                    state.add_user_message(content.clone());
                    state.is_loading = true;
                    message_to_send = Some(UiMessage::Chat(content));
                }
            }
        });

        // Loading indicator
        if state.is_loading && state.streaming_content.is_empty() && state.active_tools.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Thinking...");
            });
        }

        message_to_send
    }

    /// Parse a slash command from user input.
    /// Returns `Some(UiMessage)` if a command was recognized, `None` if it should be sent as chat.
    fn parse_slash_command(input: &str, state: &mut UiState) -> Option<UiMessage> {
        if !input.starts_with('/') {
            return None;
        }

        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match cmd {
            "/new" => Some(UiMessage::NewSession),
            "/model" => {
                if arg.is_empty() {
                    // Show current model
                    state.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: format!("Current model: {}", state.model),
                        tool_info: None,
                    });
                    state.scroll_to_bottom = true;
                    None // No message to send to worker
                } else {
                    Some(UiMessage::SetModel(arg.to_string()))
                }
            }
            "/compact" => Some(UiMessage::Compact),
            "/memory" => {
                if arg.is_empty() {
                    state.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Usage: /memory <query>".to_string(),
                        tool_info: None,
                    });
                    state.scroll_to_bottom = true;
                    None
                } else {
                    Some(UiMessage::SearchMemory(arg.to_string()))
                }
            }
            "/save" => Some(UiMessage::Save),
            "/help" => Some(UiMessage::ShowHelp),
            "/status" => Some(UiMessage::ShowStatus),
            "/resume" => {
                if arg.is_empty() {
                    state.messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: "Usage: /resume <session-id>".to_string(),
                        tool_info: None,
                    });
                    state.scroll_to_bottom = true;
                    None
                } else {
                    Some(UiMessage::ResumeSession(arg.to_string()))
                }
            }
            "/sessions" => {
                state.active_panel = Panel::Sessions;
                Some(UiMessage::RefreshSessions)
            }
            _ => {
                state.messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: format!(
                        "Unknown command: {}. Type /help for available commands.",
                        cmd
                    ),
                    tool_info: None,
                });
                state.scroll_to_bottom = true;
                None
            }
        }
    }

    fn render_message(ui: &mut Ui, msg: &ChatMessage) {
        let (label, color) = match msg.role {
            MessageRole::User => ("You", Color32::from_rgb(52, 152, 219)),
            MessageRole::Assistant => ("Assistant", Color32::from_rgb(100, 149, 237)),
            MessageRole::System => ("System", Color32::from_rgb(149, 165, 166)),
        };

        ui.horizontal(|ui| {
            ui.label(RichText::new(label).strong().color(color));
        });

        // Render content with basic markdown-like formatting
        ui.label(&msg.content);

        // Show tool info if any
        if let Some(ref tool_info) = msg.tool_info {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("[{}]", tool_info.name))
                        .small()
                        .color(Color32::GRAY),
                );
            });
        }
    }
}

/// Top toolbar with panel tabs
pub fn show_toolbar(ui: &mut Ui, state: &mut UiState) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.active_panel, Panel::Chat, "Chat");
        ui.selectable_value(&mut state.active_panel, Panel::Sessions, "Sessions");
        ui.selectable_value(&mut state.active_panel, Panel::Status, "Status");

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !state.model.is_empty() {
                ui.label(RichText::new(&state.model).small().color(Color32::GRAY));
            }
        });
    });
    ui.separator();
}
