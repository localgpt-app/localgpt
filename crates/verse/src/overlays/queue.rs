//! Slide-in queue panel (spec 1l). Rows run from the now-playing track
//! downward; ↑/↓ buttons reorder the upcoming tracks (spec's "drag to
//! reorder", adapted to the buttons the overlay kit already has).

use bevy::prelude::*;
use bevy::ui::GlobalZIndex;

use crate::QueueOpen;
use crate::playback::{Playback, fmt_time};
use crate::theme::{self, Fonts, RADIUS_SM, TEXT, Theme, text_font};

#[allow(unused_imports)]
use super::widgets::*;

use super::actions::{ButtonAction, UiButton};

#[derive(Component)]
pub struct QueueRoot;

pub fn sync_queue_overlay(
    queue_open: Res<QueueOpen>,
    mut commands: Commands,
    fonts: Res<Fonts>,
    theme: Res<Theme>,
    playback: Res<Playback>,
    existing: Query<Entity, With<QueueRoot>>,
    mut shown: Local<Option<(usize, u64)>>,
) {
    let state = if queue_open.0 {
        Some((playback.current, playback.revision))
    } else {
        None
    };
    if *shown == state {
        return;
    }
    *shown = state;
    for e in &existing {
        commands.entity(e).despawn();
    }
    if queue_open.0 {
        spawn_queue(&mut commands, &fonts, &theme, &playback);
    }
}

fn spawn_queue(commands: &mut Commands, fonts: &Fonts, theme: &Theme, playback: &Playback) {
    let accent = theme.accent();
    let len = playback.queue.len();
    let current = playback.current % len.max(1);
    let total: f32 = playback.queue.iter().map(|t| t.duration).sum();
    commands
        .spawn((
            QueueRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(0.0),
                right: Val::Px(0.0),
                width: Val::Px(380.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(26.0)),
                row_gap: Val::Px(6.0),
                border: UiRect::left(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::veil_panel()),
            BorderColor::all(theme::hairline()),
            GlobalZIndex(80),
        ))
        .with_children(|panel| {
            // Header.
            panel
                .spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Baseline,
                    width: Val::Percent(100.0),
                    ..default()
                })
                .with_children(|hdr| {
                    label_text(hdr, fonts, "Queue", 22.0, TEXT, true);
                    label_text(hdr, fonts, "Clear", 12.0, theme::text_muted(), false);
                });
            label_text(
                panel,
                fonts,
                &format!(
                    "{} songs · {} min",
                    playback.queue.len(),
                    (total / 60.0).round() as i32
                ),
                11.5,
                theme::text_muted(),
                false,
            );

            // Shuffle + repeat pills (spec 1l).
            panel
                .spawn(Node {
                    column_gap: Val::Px(8.0),
                    margin: UiRect::top(Val::Px(10.0)),
                    ..default()
                })
                .with_children(|row| {
                    pill(
                        row,
                        fonts,
                        "Shuffle",
                        ButtonAction::ToggleShuffle,
                        playback.shuffle,
                        accent,
                    );
                    pill(
                        row,
                        fonts,
                        playback.repeat.label(),
                        ButtonAction::CycleRepeat,
                        playback.repeat != crate::playback::Repeat::Off,
                        accent,
                    );
                });

            panel.spawn(Node {
                height: Val::Px(14.0),
                ..default()
            });

            // The queue as it will play: now-playing first, then upcoming.
            // Long libraries scroll (the footer stays pinned below).
            panel
                .spawn((
                    Scrollable,
                    ScrollPosition::default(),
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        flex_grow: 1.0,
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|rows| {
                    for k in 0..len {
                        let idx = (current + k) % len;
                        let track = &playback.queue[idx];
                        let tag = match k {
                            0 => Some(("NOW", accent)),
                            1 => Some(("NEXT", theme::text_muted())),
                            _ => None,
                        };
                        queue_row(
                            rows,
                            fonts,
                            tag,
                            &track.title,
                            &track.artist,
                            fmt_time(track.duration),
                            k == 0,
                            idx,
                            k > 0,
                        );
                    }
                });
            label_text(
                panel,
                fonts,
                "↑ ↓ to reorder · the next world is prepared quietly",
                11.0,
                theme::text_muted().with_alpha(0.6),
                false,
            );
        });
}

#[allow(clippy::too_many_arguments)]
fn queue_row(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    tag: Option<(&str, Color)>,
    title: &str,
    artist: &str,
    dur: String,
    current: bool,
    idx: usize,
    reorderable: bool,
) {
    parent
        .spawn((
            rounded(
                Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(12.0),
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(9.0)),
                    ..default()
                },
                RADIUS_SM,
            ),
            BackgroundColor(if current {
                theme::hairline().with_alpha(0.08)
            } else {
                Color::NONE
            }),
        ))
        .with_children(|row| {
            // Tag column.
            row.spawn(Node {
                width: Val::Px(42.0),
                ..default()
            })
            .with_children(|c| {
                if let Some((t, col)) = tag {
                    label_text(c, fonts, t, 9.5, col, false);
                }
            });
            // Title + artist.
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: Val::Px(2.0),
                ..default()
            })
            .with_children(|col| {
                label_text(col, fonts, title, 14.0, TEXT, false);
                label_text(col, fonts, artist, 11.5, theme::text_muted(), false);
            });
            label_text(row, fonts, &dur, 12.0, theme::text_muted(), false);
            // Reorder buttons for upcoming tracks (the now-playing one stays).
            if reorderable {
                for dir in [-1i32, 1] {
                    move_button(row, fonts, if dir < 0 { "↑" } else { "↓" }, idx, dir);
                }
            }
        });
}

/// A transport pill (Shuffle / Repeat). Tinted with the world accent when
/// active, muted when off.
fn pill(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    label: &str,
    action: ButtonAction,
    active: bool,
    accent: Color,
) {
    let base = if active {
        accent.with_alpha(0.22)
    } else {
        theme::veil_hud()
    };
    let text = if active { accent } else { theme::text_muted() };
    parent
        .spawn((
            Button,
            UiButton {
                action,
                primary: false,
                base,
            },
            rounded(
                Node {
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                RADIUS_SM,
            ),
            BackgroundColor(base),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(label.to_string()),
                text_font(fonts.ui_medium.clone(), 11.5),
                TextColor(text),
            ));
        });
}

/// A small ↑/↓ reorder button broadcasting `QueueMove { idx, dir }`.
fn move_button(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    glyph: &str,
    idx: usize,
    dir: i32,
) {
    let base = theme::veil_hud();
    parent
        .spawn((
            Button,
            UiButton {
                action: ButtonAction::QueueMove { idx, dir },
                primary: false,
                base,
            },
            rounded(
                Node {
                    width: Val::Px(22.0),
                    height: Val::Px(22.0),
                    margin: UiRect::left(Val::Px(3.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                RADIUS_SM,
            ),
            BackgroundColor(base),
        ))
        .with_children(|b| {
            b.spawn((
                Text::new(glyph.to_string()),
                text_font(fonts.ui.clone(), 12.0),
                TextColor(TEXT),
            ));
        });
}
