//! First-run onboarding (spec 1p/1j).

use bevy::prelude::*;
use bevy::ui::GlobalZIndex;

use crate::theme::{self, Fonts, RADIUS_SM, TEXT, Theme, text_font};

use super::actions::ButtonAction;
#[allow(unused_imports)]
use super::widgets::*;

#[derive(Component)]
pub struct FirstRunRoot;

pub fn spawn_first_run(
    mut commands: Commands,
    fonts: Res<Fonts>,
    theme: Res<Theme>,
    onboarding: Res<crate::Onboarding>,
) {
    build_onboarding(&mut commands, &fonts, &theme, onboarding.step);
}

/// Re-render the onboarding when the step advances.
pub fn refresh_onboarding(
    onboarding: Res<crate::Onboarding>,
    mut commands: Commands,
    fonts: Res<Fonts>,
    theme: Res<Theme>,
    existing: Query<Entity, With<FirstRunRoot>>,
) {
    if !onboarding.is_changed() || onboarding.is_added() {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    build_onboarding(&mut commands, &fonts, &theme, onboarding.step);
}

/// Build the first-run onboarding for `step` (0 photosensitivity, 1 controls,
/// 2 import) — the photosensitivity-first flow from spec 1p.
fn build_onboarding(commands: &mut Commands, fonts: &Fonts, theme: &Theme, step: u8) {
    let accent = theme.accent();
    commands
        .spawn((
            FirstRunRoot,
            full_screen_center(),
            BackgroundColor(theme::BASE.with_alpha(0.92)),
            GlobalZIndex(100),
        ))
        .with_children(|c| {
            label_text(c, fonts, &format!("{} OF 3", step.min(2) + 1), 11.0, theme::text_muted().with_alpha(0.6), false);
            spacer(c, 18.0);

            match step {
                0 => {
                    label_text(c, fonts, "Worlds that move with your music", 34.0, TEXT, true);
                    spacer(c, 14.0);
                    centered_body(c, fonts, "Some songs make light pulse, flash, or strobe. If flashing bothers\nyou — or you're not sure — start gentle. It still looks beautiful.");
                    spacer(c, 28.0);
                    c.spawn(Node { column_gap: Val::Px(12.0), align_items: AlignItems::Center, ..default() })
                        .with_children(|row| {
                            button(row, fonts, "Start gentle", None, ButtonAction::StartGentle, true, accent);
                            button(row, fonts, "Full intensity", None, ButtonAction::FullIntensity, false, accent);
                        });
                    spacer(c, 14.0);
                    label_text(c, fonts, "Reduced flashing · gentler motion · recommended   ·   change anytime in Settings › Comfort", 11.0, theme::text_muted().with_alpha(0.6), false);
                }
                1 => {
                    label_text(c, fonts, "The controls, both ways", 34.0, TEXT, true);
                    spacer(c, 20.0);
                    c.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        ..default()
                    })
                    .with_children(|col| {
                        onboard_control_row(col, fonts, "Move", "W A S D", "LS");
                        onboard_control_row(col, fonts, "Look", "Mouse", "RS");
                        onboard_control_row(col, fonts, "Fly up · down", "Space · Shift", "LB · RB");
                        onboard_control_row(col, fonts, "Fly speed", "Scroll", "-");
                        onboard_control_row(col, fonts, "Send a pulse", "E", "X");
                        onboard_control_row(col, fonts, "Queue · Pause", "Tab · Esc", "View · B");
                    });
                    spacer(c, 14.0);
                    label_text(c, fonts, "Explore holds the mouse for 360° look — Esc frees it", 11.0, theme::text_muted().with_alpha(0.6), false);
                    spacer(c, 26.0);
                    button(c, fonts, "Next", None, ButtonAction::OnboardNext, true, accent);
                }
                _ => {
                    label_text(c, fonts, "Bring your music", 40.0, TEXT, true);
                    spacer(c, 14.0);
                    centered_body(c, fonts, "LocalGPT Verse plays the files already on this computer and imagines a\nworld for every song. Point it at a folder — nothing is uploaded, ever.");
                    spacer(c, 30.0);
                    c.spawn(Node { column_gap: Val::Px(12.0), align_items: AlignItems::Center, ..default() })
                        .with_children(|row| {
                            button(row, fonts, "Choose your music folder…", None, ButtonAction::Start, true, accent);
                            button(row, fonts, "Skip for now", None, ButtonAction::Skip, false, accent);
                        });
                    spacer(c, 18.0);
                    label_text(c, fonts, "MP3 · FLAC · WAV · OGG · AIFF", 11.0, theme::text_muted().with_alpha(0.4), false);
                }
            }

            if step < 2 {
                spacer(c, 26.0);
                button(c, fonts, "Skip setup", None, ButtonAction::Skip, false, accent);
            }
        });
}

/// Centered muted body paragraph used across onboarding steps.
fn centered_body(parent: &mut ChildSpawnerCommands<'_>, fonts: &Fonts, text: &str) {
    parent.spawn((
        Text::new(text.to_string()),
        text_font(fonts.ui.clone(), 14.0),
        TextColor(theme::text_muted()),
        TextLayout {
            justify: Justify::Center,
            ..default()
        },
    ));
}

/// One "action — keyboard — gamepad" row on the controls step.
fn onboard_control_row(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    action: &str,
    keyboard: &str,
    gamepad: &str,
) {
    parent
        .spawn(Node {
            width: Val::Px(440.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|row| {
            label_text(row, fonts, action, 14.0, TEXT, false);
            row.spawn(Node {
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|keys| {
                key_chip(keys, fonts, keyboard);
                key_chip(keys, fonts, gamepad);
            });
        });
}

/// A small key/gamepad chip.
fn key_chip(parent: &mut ChildSpawnerCommands<'_>, fonts: &Fonts, text: &str) {
    parent
        .spawn((
            rounded(
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(5.0)),
                    ..default()
                },
                RADIUS_SM,
            ),
            BackgroundColor(theme::hairline()),
        ))
        .with_children(|c| {
            label_text(c, fonts, text, 12.0, TEXT, false);
        });
}

pub fn despawn_first_run(mut commands: Commands, q: Query<Entity, With<FirstRunRoot>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}
