//! Settings — the Comfort group with live toggles (spec 1n).

use bevy::prelude::*;
use bevy::ui::GlobalZIndex;

use crate::theme::{self, Fonts, RADIUS_MD, RADIUS_PILL, TEXT, Theme};
use crate::{Comfort, Overlay, OverlayStack};

use super::actions::{ButtonAction, UiButton};
#[allow(unused_imports)]
use super::widgets::*;

#[derive(Component)]
pub struct SettingsRoot;

/// Which comfort field a toggle controls (for live visual updates).
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum ComfortField {
    ReduceFlashing,
    GentlerMotion,
}

/// On the toggle track (a `Button`).
#[derive(Component)]
pub struct ComfortToggle(pub ComfortField);
/// On the toggle knob.
#[derive(Component)]
pub struct ComfortKnob(pub ComfortField);

fn comfort_get(comfort: &Comfort, field: ComfortField) -> bool {
    match field {
        ComfortField::ReduceFlashing => comfort.reduce_flashing,
        ComfortField::GentlerMotion => comfort.gentler_motion,
    }
}

fn switch_bg(on: bool, accent: Color) -> Color {
    if on {
        accent.with_alpha(0.9)
    } else {
        theme::hairline()
    }
}

fn knob_left(on: bool) -> Val {
    Val::Px(if on { 22.0 } else { 4.0 })
}

pub fn sync_settings_overlay(
    stack: Res<OverlayStack>,
    mut commands: Commands,
    fonts: Res<Fonts>,
    theme: Res<Theme>,
    comfort: Res<Comfort>,
    existing: Query<Entity, With<SettingsRoot>>,
    mut last: Local<bool>,
) {
    let open = stack.is_open(Overlay::Settings);
    if open == *last {
        return;
    }
    *last = open;
    if open && existing.is_empty() {
        spawn_settings(&mut commands, &fonts, &theme, &comfort);
    } else if !open {
        for e in &existing {
            commands.entity(e).despawn();
        }
    }
}

fn spawn_settings(commands: &mut Commands, fonts: &Fonts, theme: &Theme, comfort: &Comfort) {
    let accent = theme.accent();
    commands
        .spawn((
            SettingsRoot,
            full_screen_center(),
            BackgroundColor(theme::veil_panel()),
            GlobalZIndex(95),
        ))
        .with_children(|c| {
            c.spawn((
                rounded(
                    Node {
                        width: Val::Px(640.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(34.0)),
                        row_gap: Val::Px(4.0),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    RADIUS_MD,
                ),
                BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.98)),
                BorderColor::all(theme::hairline()),
            ))
            .with_children(|card| {
                label_text(card, fonts, "SETTINGS", 13.0, theme::text_muted(), false);
                spacer(card, 16.0);

                // Section nav — Comfort is the active group; About & Credits opens Credits.
                card.spawn(Node {
                    column_gap: Val::Px(8.0),
                    row_gap: Val::Px(8.0),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|nav| {
                    for (name, active) in [
                        ("Graphics", false),
                        ("Comfort", true),
                        ("Sound", false),
                        ("Worlds", false),
                        ("Storage", false),
                    ] {
                        nav_tab(nav, fonts, name, active, accent);
                    }
                    button(
                        nav,
                        fonts,
                        "About & Credits",
                        None,
                        ButtonAction::OpenCredits,
                        false,
                        accent,
                    );
                });

                spacer(card, 22.0);
                label_text(card, fonts, "Comfort", 20.0, TEXT, true);
                label_text(
                    card,
                    fonts,
                    "LocalGPT Verse should feel good to be in. These apply instantly.",
                    12.5,
                    theme::text_muted(),
                    false,
                );
                spacer(card, 12.0);

                comfort_toggle(
                    card,
                    fonts,
                    "Reduce flashing",
                    "Caps strobing and beat-flash effects across every world.",
                    ComfortField::ReduceFlashing,
                    ButtonAction::ToggleReduceFlashing,
                    comfort.reduce_flashing,
                    accent,
                );
                hairline_row(card);
                comfort_toggle(
                    card,
                    fonts,
                    "Gentler world motion",
                    "The world sways less; scene changes take their time.",
                    ComfortField::GentlerMotion,
                    ButtonAction::ToggleGentlerMotion,
                    comfort.gentler_motion,
                    accent,
                );
                hairline_row(card);
                static_row(
                    card,
                    fonts,
                    "Camera bob while walking",
                    "Off keeps the camera perfectly level",
                    "Off",
                );
                hairline_row(card);
                static_row(
                    card,
                    fonts,
                    "Field of view",
                    "Wider can ease motion sickness",
                    "90°",
                );
                hairline_row(card);
                static_row(
                    card,
                    fonts,
                    "Interface size",
                    "TV is made for across-the-room",
                    "Comfortable",
                );

                spacer(card, 24.0);
                card.spawn(Node {
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|row| {
                    button(
                        row,
                        fonts,
                        "Restore comfort defaults",
                        None,
                        ButtonAction::RestoreComfort,
                        false,
                        accent,
                    );
                    button(
                        row,
                        fonts,
                        "Done",
                        Some("Esc"),
                        ButtonAction::CloseSettings,
                        true,
                        accent,
                    );
                });
            });
        });
}

/// A comfort on/off row: label + description on the left, a switch on the right.
#[allow(clippy::too_many_arguments)]
fn comfort_toggle(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    label: &str,
    desc: &str,
    field: ComfortField,
    action: ButtonAction,
    on: bool,
    accent: Color,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(16.0),
            padding: UiRect::vertical(Val::Px(10.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: Val::Px(3.0),
                ..default()
            })
            .with_children(|c| {
                label_text(c, fonts, label, 14.0, TEXT, false);
                label_text(c, fonts, desc, 11.5, theme::text_muted(), false);
            });
            // The switch is a button; its visual is kept in sync by
            // `update_comfort_toggles`.
            let bg = switch_bg(on, accent);
            row.spawn((
                Button,
                UiButton {
                    action,
                    primary: false,
                    base: bg,
                },
                ComfortToggle(field),
                rounded(
                    Node {
                        width: Val::Px(42.0),
                        height: Val::Px(24.0),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    RADIUS_PILL,
                ),
                BackgroundColor(bg),
                BorderColor::all(theme::hairline()),
            ))
            .with_children(|s| {
                s.spawn((
                    ComfortKnob(field),
                    rounded(
                        Node {
                            position_type: PositionType::Absolute,
                            top: Val::Px(3.0),
                            left: knob_left(on),
                            width: Val::Px(16.0),
                            height: Val::Px(16.0),
                            ..default()
                        },
                        RADIUS_PILL,
                    ),
                    BackgroundColor(TEXT),
                ));
            });
        });
}

/// Keep comfort switches in sync with the `Comfort` resource.
pub fn update_comfort_toggles(
    comfort: Res<Comfort>,
    theme: Res<Theme>,
    mut tracks: Query<(&ComfortToggle, &mut BackgroundColor, &mut UiButton)>,
    mut knobs: Query<(&ComfortKnob, &mut Node)>,
) {
    if !comfort.is_changed() && !theme.is_changed() {
        return;
    }
    let accent = theme.accent();
    for (toggle, mut bg, mut btn) in &mut tracks {
        let c = switch_bg(comfort_get(&comfort, toggle.0), accent);
        bg.0 = c;
        btn.base = c;
    }
    for (knob, mut node) in &mut knobs {
        node.left = knob_left(comfort_get(&comfort, knob.0));
    }
}

/// A display-only settings row: label + description on the left, value on the right.
fn static_row(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    label: &str,
    desc: &str,
    value: &str,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(16.0),
            padding: UiRect::vertical(Val::Px(10.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: Val::Px(3.0),
                ..default()
            })
            .with_children(|c| {
                label_text(c, fonts, label, 14.0, TEXT, false);
                label_text(c, fonts, desc, 11.5, theme::text_muted(), false);
            });
            label_text(row, fonts, value, 13.0, theme::text_muted(), false);
        });
}

/// A full-width hairline divider.
fn hairline_row(parent: &mut ChildSpawnerCommands<'_>) {
    parent.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(1.0),
            ..default()
        },
        BackgroundColor(theme::hairline().with_alpha(0.5)),
    ));
}
