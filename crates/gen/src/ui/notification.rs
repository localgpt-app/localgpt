//! Transient notification messages.
//!
//! Implements Spec 4.5: `gen_add_notification` — Temporary messages with animation.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Notification display style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[serde(rename_all = "snake_case")]
pub enum NotificationStyle {
    #[default]
    Toast,
    Banner,
    Achievement,
}

/// Notification position on screen.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[serde(rename_all = "snake_case")]
pub enum NotificationPosition {
    #[default]
    Top,
    Center,
    Bottom,
}

/// Built-in icon types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Reflect)]
#[serde(rename_all = "snake_case")]
pub enum NotificationIcon {
    #[default]
    None,
    Star,
    Coin,
    Key,
    Heart,
    Warning,
}

/// Parameters for notification creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationParams {
    /// Notification text.
    pub text: String,
    /// Display style.
    #[serde(default)]
    pub style: NotificationStyle,
    /// Position on screen.
    #[serde(default)]
    pub position: NotificationPosition,
    /// Display duration in seconds.
    #[serde(default = "default_duration")]
    pub duration: f32,
    /// Text color (hex).
    #[serde(default = "default_notification_color")]
    pub color: String,
    /// Icon type.
    #[serde(default)]
    pub icon: NotificationIcon,
    /// Sound to play.
    #[serde(default)]
    pub sound: Option<String>,
}

fn default_duration() -> f32 {
    3.0
}
fn default_notification_color() -> String {
    "#ffffff".to_string()
}

impl Default for NotificationParams {
    fn default() -> Self {
        Self {
            text: String::new(),
            style: NotificationStyle::default(),
            position: NotificationPosition::default(),
            duration: default_duration(),
            color: default_notification_color(),
            icon: NotificationIcon::default(),
            sound: None,
        }
    }
}

/// Component for notification.
#[derive(Component, Reflect)]
#[reflect(Component)]
pub struct Notification {
    /// Notification text.
    pub text: String,
    /// Display style.
    pub style: NotificationStyle,
    /// Screen position.
    pub position: NotificationPosition,
    /// Animation phase.
    pub phase: NotificationPhase,
    /// Elapsed time.
    pub elapsed: f32,
    /// Duration.
    pub duration: f32,
    /// Stack offset.
    pub stack_offset: f32,
    /// Alpha.
    pub alpha: f32,
}

/// Animation phases for notification.
#[derive(Debug, Clone, Copy, Default, Reflect)]
pub enum NotificationPhase {
    #[default]
    EnterIn,
    Hold,
    Exit,
}

/// Event emitted when a notification is spawned.
#[derive(Message, Reflect)]
pub struct NotificationEvent {
    pub text: String,
    pub style: NotificationStyle,
    pub position: NotificationPosition,
    pub icon: NotificationIcon,
}

/// Resource tracking active notifications.
#[derive(Resource, Default, Reflect)]
#[reflect(Resource)]
pub struct NotificationQueue {
    /// Active notifications.
    pub notifications: Vec<Entity>,
}

/// System to spawn notifications from events.
pub fn notification_spawn_system(
    mut events: MessageReader<NotificationEvent>,
    mut commands: Commands,
    mut queue: ResMut<NotificationQueue>,
) {
    for event in events.read() {
        let icon_text = get_notification_icon_text(event.icon);
        let display_text = if icon_text.is_empty() {
            event.text.clone()
        } else {
            format!("{} {}", icon_text, event.text)
        };
        let entity = commands
            .spawn((
                Notification {
                    text: event.text.clone(),
                    style: event.style,
                    position: event.position,
                    phase: NotificationPhase::EnterIn,
                    elapsed: 0.0,
                    duration: default_duration(),
                    stack_offset: 0.0,
                    alpha: 0.0,
                },
                notification_position_node(event.position),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
                Text::new(display_text),
                TextColor(Color::WHITE),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                Name::new("Notification"),
            ))
            .id();

        queue.notifications.push(entity);

        // Limit stack size
        while queue.notifications.len() > 4 {
            if let Some(&oldest) = queue.notifications.first() {
                commands.entity(oldest).despawn();
                queue.notifications.remove(0);
            }
        }
    }
}

/// System to animate notifications.
#[allow(clippy::type_complexity)]
pub fn notification_animation_system(
    time: Res<Time>,
    mut commands: Commands,
    mut notification_query: Query<(
        Entity,
        &mut Notification,
        Option<&mut TextColor>,
        Option<&mut BackgroundColor>,
        Option<&mut Node>,
    )>,
    mut queue: ResMut<NotificationQueue>,
) {
    for (entity, mut notification, text_color, bg_color, node) in notification_query.iter_mut() {
        notification.elapsed += time.delta_secs();

        match notification.phase {
            NotificationPhase::EnterIn => {
                // Slide in
                notification.alpha = (notification.elapsed / 0.3).min(1.0);
                if notification.alpha >= 1.0 {
                    notification.phase = NotificationPhase::Hold;
                    notification.elapsed = 0.0;
                }
            }
            NotificationPhase::Hold => {
                notification.alpha = 1.0;
                if notification.elapsed >= notification.duration {
                    notification.phase = NotificationPhase::Exit;
                    notification.elapsed = 0.0;
                }
            }
            NotificationPhase::Exit => {
                // Fade out
                notification.alpha = (1.0 - notification.elapsed / 0.3).max(0.0);

                if notification.alpha <= 0.0 {
                    commands.entity(entity).despawn();
                    queue.notifications.retain(|&e| e != entity);
                }
            }
        }

        // Apply alpha to text and background
        if let Some(mut tc) = text_color {
            tc.0 = tc.0.with_alpha(notification.alpha);
        }
        if let Some(mut bg) = bg_color {
            bg.0 = bg.0.with_alpha(notification.alpha * 0.7);
        }

        // Apply stack offset to node position
        if let Some(mut n) = node {
            let base_top = match notification.position {
                NotificationPosition::Top => Val::Px(20.0),
                NotificationPosition::Center => Val::Percent(50.0),
                NotificationPosition::Bottom => Val::Auto,
            };
            if !matches!(notification.position, NotificationPosition::Bottom) {
                n.top = match base_top {
                    Val::Px(px) => Val::Px(px + notification.stack_offset * 50.0),
                    Val::Percent(p) => Val::Percent(p),
                    _ => base_top,
                };
            }
        }
    }

    // Update stack positions
    let mut offset = 0.0;
    for entity in &queue.notifications {
        if let Ok((_, mut notif, _, _, _)) = notification_query.get_mut(*entity) {
            notif.stack_offset = offset;
            offset += 1.0;
        }
    }
}

/// Get icon text for notification icon.
pub fn get_notification_icon_text(icon: NotificationIcon) -> &'static str {
    match icon {
        NotificationIcon::None => "",
        NotificationIcon::Star => "★",
        NotificationIcon::Coin => "●",
        NotificationIcon::Key => "🔑",
        NotificationIcon::Heart => "❤",
        NotificationIcon::Warning => "⚠",
    }
}

/// Build a positioned `Node` for notification screen placement.
pub fn notification_position_node(position: NotificationPosition) -> Node {
    let mut node = Node {
        position_type: PositionType::Absolute,
        padding: UiRect::all(Val::Px(8.0)),
        max_width: Val::Px(300.0),
        ..default()
    };
    match position {
        NotificationPosition::Top => {
            node.top = Val::Px(20.0);
            node.right = Val::Px(20.0);
        }
        NotificationPosition::Center => {
            node.top = Val::Percent(50.0);
            node.left = Val::Percent(50.0);
        }
        NotificationPosition::Bottom => {
            node.bottom = Val::Px(20.0);
            node.right = Val::Px(20.0);
        }
    }
    node
}

/// Plugin for notification systems.
pub struct NotificationPlugin;

impl Plugin for NotificationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NotificationQueue>()
            .add_message::<NotificationEvent>()
            .add_systems(Update, notification_spawn_system)
            .add_systems(Update, notification_animation_system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_params_default() {
        let params = NotificationParams::default();
        assert_eq!(params.style, NotificationStyle::Toast);
        assert_eq!(params.position, NotificationPosition::Top);
        assert_eq!(params.duration, 3.0);
        assert_eq!(params.color, "#ffffff");
        assert_eq!(params.icon, NotificationIcon::None);
        assert!(params.sound.is_none());
    }

    #[test]
    fn test_notification_icon_text() {
        assert_eq!(get_notification_icon_text(NotificationIcon::None), "");
        assert_eq!(get_notification_icon_text(NotificationIcon::Star), "★");
        assert_eq!(get_notification_icon_text(NotificationIcon::Coin), "●");
        assert_eq!(get_notification_icon_text(NotificationIcon::Warning), "⚠");
    }

    #[test]
    fn test_notification_queue_default() {
        let queue = NotificationQueue::default();
        assert!(queue.notifications.is_empty());
    }

    #[test]
    fn test_notification_style_variants() {
        assert_eq!(NotificationStyle::default(), NotificationStyle::Toast);
        assert_ne!(NotificationStyle::Banner, NotificationStyle::Achievement);
    }

    #[test]
    fn test_notification_position_variants() {
        assert_eq!(NotificationPosition::default(), NotificationPosition::Top);
        assert_ne!(NotificationPosition::Center, NotificationPosition::Bottom);
    }

    #[test]
    fn test_notification_phase_default() {
        assert!(matches!(
            NotificationPhase::default(),
            NotificationPhase::EnterIn
        ));
    }

    #[test]
    fn test_notification_icon_all_variants() {
        assert_eq!(get_notification_icon_text(NotificationIcon::Key), "🔑");
        assert_eq!(get_notification_icon_text(NotificationIcon::Heart), "❤");
    }

    #[test]
    fn test_notification_component() {
        let notif = Notification {
            text: "Achievement unlocked!".to_string(),
            style: NotificationStyle::Achievement,
            position: NotificationPosition::Center,
            phase: NotificationPhase::Hold,
            elapsed: 1.0,
            duration: 3.0,
            stack_offset: 0.0,
            alpha: 1.0,
        };
        assert_eq!(notif.text, "Achievement unlocked!");
        assert_eq!(notif.style, NotificationStyle::Achievement);
        assert_eq!(notif.alpha, 1.0);
    }

    #[test]
    fn test_notification_position_node_top() {
        let node = notification_position_node(NotificationPosition::Top);
        assert_eq!(node.position_type, PositionType::Absolute);
        assert_eq!(node.top, Val::Px(20.0));
        assert_eq!(node.right, Val::Px(20.0));
        assert_eq!(node.max_width, Val::Px(300.0));
    }

    #[test]
    fn test_notification_position_node_center() {
        let node = notification_position_node(NotificationPosition::Center);
        assert_eq!(node.position_type, PositionType::Absolute);
        assert_eq!(node.top, Val::Percent(50.0));
        assert_eq!(node.left, Val::Percent(50.0));
    }

    #[test]
    fn test_notification_position_node_bottom() {
        let node = notification_position_node(NotificationPosition::Bottom);
        assert_eq!(node.position_type, PositionType::Absolute);
        assert_eq!(node.bottom, Val::Px(20.0));
        assert_eq!(node.right, Val::Px(20.0));
    }
}
