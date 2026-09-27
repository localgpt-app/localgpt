//! Shared overlay widgets: pills, labels, layout primitives.

use bevy::prelude::*;

use crate::theme::{self, Fonts, RADIUS_PILL, TEXT, text_font};

use super::actions::{ButtonAction, UiButton};

pub(super) fn rounded(mut node: Node, radius: f32) -> Node {
    node.border_radius = BorderRadius::all(Val::Px(radius));
    node
}

/// Marks the scrollable content region of an overlay (credits, queue) —
/// `overlay_scroll` maps the mouse wheel onto its `ScrollPosition`.
#[derive(Component)]
pub struct Scrollable;

/// Spawn a pill button into `parent`.
pub(super) fn button(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    label: &str,
    key: Option<&str>,
    action: ButtonAction,
    primary: bool,
    accent: Color,
) {
    let base = if primary {
        accent.with_alpha(0.92)
    } else {
        theme::veil_hud()
    };
    let fg = if primary { theme::BASE } else { TEXT };
    parent
        .spawn((
            Button,
            UiButton {
                action,
                primary,
                base,
            },
            rounded(
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(9.0),
                    padding: UiRect::axes(Val::Px(18.0), Val::Px(11.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                RADIUS_PILL,
            ),
            BackgroundColor(base),
            BorderColor::all(if primary {
                Color::NONE
            } else {
                theme::hairline()
            }),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_string()),
                text_font(fonts.ui_semibold.clone(), 13.5),
                TextColor(fg),
            ));
            if let Some(k) = key {
                b.spawn((
                    rounded(
                        Node {
                            min_width: Val::Px(18.0),
                            height: Val::Px(18.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            padding: UiRect::horizontal(Val::Px(5.0)),
                            ..default()
                        },
                        5.0,
                    ),
                    BackgroundColor(if primary {
                        theme::BASE.with_alpha(0.15)
                    } else {
                        theme::hairline()
                    }),
                ))
                .with_children(|c| {
                    c.spawn((
                        Text::new(k.to_string()),
                        text_font(fonts.ui_semibold.clone(), 10.5),
                        TextColor(fg.with_alpha(0.7)),
                    ));
                });
            }
        });
}

pub(super) fn label_text(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    text: &str,
    size: f32,
    color: Color,
    display: bool,
) {
    parent.spawn((
        Text::new(text.to_string()),
        text_font(
            if display {
                fonts.display.clone()
            } else {
                fonts.ui.clone()
            },
            size,
        ),
        TextColor(color),
    ));
}

pub(super) fn full_screen_center() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    }
}

/// A fixed-height vertical spacer.
pub(super) fn spacer(parent: &mut ChildSpawnerCommands<'_>, h: f32) {
    parent.spawn(Node {
        height: Val::Px(h),
        ..default()
    });
}

/// A non-interactive nav/filter pill (active = accent-tinted).
pub(super) fn nav_tab(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    label: &str,
    active: bool,
    accent: Color,
) {
    parent
        .spawn((
            rounded(
                Node {
                    padding: UiRect::axes(Val::Px(14.0), Val::Px(7.0)),
                    ..default()
                },
                RADIUS_PILL,
            ),
            BackgroundColor(if active {
                accent.with_alpha(0.18)
            } else {
                theme::veil_hud()
            }),
        ))
        .with_children(|t| {
            label_text(
                t,
                fonts,
                label,
                12.5,
                if active { TEXT } else { theme::text_muted() },
                false,
            );
        });
}
