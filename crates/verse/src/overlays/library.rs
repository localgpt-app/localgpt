//! Library / Home — the music browser (spec 1i).
//!
//! A left sidebar (wordmark, search field, Import folder, Settings, and the
//! worlds as browsable "playlists") beside a track table (# / TITLE / ALBUM /
//! WORLD / LENGTH). Rows play on click; Play / Shuffle start the shown set.
//! Adapts the spec's named playlists to LocalGPT Verse's own axis — the worlds each
//! song is imagined into — since that is the grouping the app actually has.

use bevy::prelude::*;
use bevy::ui::GlobalZIndex;

use crate::playback::{Playback, fmt_time};
use crate::theme::{self, Fonts, RADIUS_SM, TEXT, Theme, text_font};
use crate::{Overlay, OverlayStack};

use super::actions::{ButtonAction, UiButton};
#[allow(unused_imports)]
use super::widgets::*;

/// The Library's current filter: a world index, or `None` for all music.
#[derive(Resource, Default)]
pub struct LibraryFilter(pub Option<usize>);

#[derive(Component)]
pub struct LibraryRoot;

#[allow(clippy::too_many_arguments)]
pub fn sync_library_overlay(
    stack: Res<OverlayStack>,
    filter: Res<LibraryFilter>,
    mut commands: Commands,
    fonts: Res<Fonts>,
    theme: Res<Theme>,
    playback: Res<Playback>,
    existing: Query<Entity, With<LibraryRoot>>,
    mut shown: Local<Option<(u64, Option<usize>)>>,
) {
    let open = stack.is_open(Overlay::Library);
    // Re-render on open/close, import (revision), and filter change.
    let state = open.then_some((playback.revision, filter.0));
    if *shown == state {
        return;
    }
    *shown = state;
    for e in &existing {
        commands.entity(e).despawn();
    }
    if open {
        spawn_library(&mut commands, &fonts, &theme, &playback, filter.0);
    }
}

fn spawn_library(
    commands: &mut Commands,
    fonts: &Fonts,
    theme: &Theme,
    playback: &Playback,
    filter: Option<usize>,
) {
    let accent = theme.accent();
    commands
        .spawn((
            LibraryRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.03, 0.05, 0.97)),
            GlobalZIndex(85),
        ))
        .with_children(|root| {
            sidebar(root, fonts, playback, filter, accent);
            main_panel(root, fonts, playback, filter, accent);
        });
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------
fn sidebar(
    root: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    playback: &Playback,
    filter: Option<usize>,
    accent: Color,
) {
    root.spawn((
        Node {
            width: Val::Px(248.0),
            height: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(26.0)),
            row_gap: Val::Px(6.0),
            border: UiRect::right(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(theme::veil_panel()),
        BorderColor::all(theme::hairline()),
    ))
    .with_children(|side| {
        label_text(side, fonts, "LOCALGPT VERSE", 20.0, TEXT, true);
        spacer(side, 14.0);

        // Search field (visual affordance; browse by world below to filter).
        side.spawn((
            rounded(
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                RADIUS_SM,
            ),
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.03)),
            BorderColor::all(theme::hairline()),
        ))
        .with_children(|f| {
            label_text(
                f,
                fonts,
                "Search your music",
                12.0,
                theme::text_muted(),
                false,
            );
        });
        spacer(side, 6.0);
        nav_row(side, fonts, "Import folder", ButtonAction::ImportFolder);
        nav_row(side, fonts, "Settings", ButtonAction::OpenSettings);
        spacer(side, 16.0);

        label_text(side, fonts, "WORLDS", 10.5, theme::text_muted(), true);
        spacer(side, 4.0);
        filter_row(
            side,
            fonts,
            "All music",
            playback.queue.len(),
            ButtonAction::FilterAll,
            filter.is_none(),
            accent,
        );
        for (i, mood) in theme::moods().iter().enumerate() {
            let count = playback.queue.iter().filter(|t| t.mood == i).count();
            filter_row(
                side,
                fonts,
                mood.world_name,
                count,
                ButtonAction::FilterWorld(i),
                filter == Some(i),
                accent,
            );
        }

        // Footer note (spec 1i privacy line).
        side.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        label_text(
            side,
            fonts,
            "Worlds are imagined on this computer. Your music never leaves it.",
            10.5,
            theme::text_muted().with_alpha(0.7),
            false,
        );
    });
}

/// A plain sidebar nav item (Import folder / Settings).
fn nav_row(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    label: &str,
    action: ButtonAction,
) {
    let base = Color::NONE;
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
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(7.0)),
                    ..default()
                },
                RADIUS_SM,
            ),
            BackgroundColor(base),
        ))
        .with_children(|b| {
            label_text(b, fonts, label, 13.0, theme::TEXT_DIM, false);
        });
}

/// A world filter row: name + track count, tinted when selected.
fn filter_row(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    label: &str,
    count: usize,
    action: ButtonAction,
    selected: bool,
    accent: Color,
) {
    let base = if selected {
        accent.with_alpha(0.16)
    } else {
        Color::NONE
    };
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
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(7.0)),
                    ..default()
                },
                RADIUS_SM,
            ),
            BackgroundColor(base),
        ))
        .with_children(|row| {
            let col = if selected { accent } else { TEXT };
            label_text(row, fonts, label, 13.0, col, false);
            label_text(
                row,
                fonts,
                &count.to_string(),
                11.5,
                theme::text_muted(),
                false,
            );
        });
}

// ---------------------------------------------------------------------------
// Main panel — header + track table
// ---------------------------------------------------------------------------
fn main_panel(
    root: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    playback: &Playback,
    filter: Option<usize>,
    accent: Color,
) {
    // Visible rows: (queue index, track) after the world filter.
    let rows: Vec<(usize, &crate::playback::Track)> = playback
        .queue
        .iter()
        .enumerate()
        .filter(|(_, t)| filter.is_none_or(|w| t.mood == w))
        .collect();
    let total: f32 = rows.iter().map(|(_, t)| t.duration).sum();
    let title = filter.map_or("All music", |w| {
        theme::moods()[w % theme::moods().len()].world_name
    });
    let first = rows.first().map(|(i, _)| *i);

    root.spawn(Node {
        flex_grow: 1.0,
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(Val::Px(34.0)),
        row_gap: Val::Px(6.0),
        ..default()
    })
    .with_children(|main| {
        // Header row: title/meta on the left, Done on the right.
        main.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::FlexStart,
            ..default()
        })
        .with_children(|hdr| {
            hdr.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|t| {
                label_text(t, fonts, title, 30.0, TEXT, true);
                label_text(
                    t,
                    fonts,
                    &format!(
                        "{} songs · {} min",
                        rows.len(),
                        (total / 60.0).round() as i32
                    ),
                    12.0,
                    theme::text_muted(),
                    false,
                );
            });
            button(
                hdr,
                fonts,
                "Done",
                Some("L"),
                ButtonAction::CloseLibrary,
                false,
                accent,
            );
        });

        spacer(main, 14.0);
        // Play + Shuffle.
        main.spawn(Node {
            column_gap: Val::Px(10.0),
            ..default()
        })
        .with_children(|actions| {
            let play = first.map_or(ButtonAction::FilterAll, ButtonAction::PlayIndex);
            button(
                actions,
                fonts,
                "▶  Play — enter the world",
                None,
                play,
                true,
                accent,
            );
            button(
                actions,
                fonts,
                "⤮  Shuffle",
                None,
                ButtonAction::LibraryShuffle,
                false,
                accent,
            );
        });
        spacer(main, 16.0);

        // Column header.
        table_header(main, fonts);
        // Scrollable rows.
        main.spawn((
            Scrollable,
            ScrollPosition::default(),
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .with_children(|list| {
            if rows.is_empty() {
                label_text(
                    list,
                    fonts,
                    "No songs yet — Import folder to begin.",
                    13.0,
                    theme::text_muted(),
                    false,
                );
            }
            for (n, (idx, track)) in rows.iter().enumerate() {
                track_row(
                    list,
                    fonts,
                    n + 1,
                    *idx,
                    track,
                    *idx == playback.current,
                    accent,
                );
            }
        });

        label_text(
            main,
            fonts,
            "World previews are a feeling, not a spoiler — every build is a little different.",
            11.0,
            theme::text_muted().with_alpha(0.6),
            false,
        );
    });
}

fn table_header(parent: &mut ChildSpawnerCommands<'_>, fonts: &Fonts) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(theme::hairline()),
        ))
        .with_children(|h| {
            let muted = theme::text_muted();
            col(h, fonts, "#", 28.0, muted);
            col_grow(h, fonts, "TITLE", muted);
            col(h, fonts, "ALBUM", 180.0, muted);
            col(h, fonts, "WORLD", 150.0, muted);
            col(h, fonts, "LENGTH", 60.0, muted);
        });
}

#[allow(clippy::too_many_arguments)]
fn track_row(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    num: usize,
    idx: usize,
    track: &crate::playback::Track,
    now_playing: bool,
    accent: Color,
) {
    let base = if now_playing {
        accent.with_alpha(0.10)
    } else {
        Color::NONE
    };
    parent
        .spawn((
            Button,
            UiButton {
                action: ButtonAction::PlayIndex(idx),
                primary: false,
                base,
            },
            rounded(
                Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(12.0),
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                    ..default()
                },
                RADIUS_SM,
            ),
            BackgroundColor(base),
        ))
        .with_children(|row| {
            let world = theme::moods()[track.mood % theme::moods().len()].world_name;
            col(row, fonts, &num.to_string(), 28.0, theme::text_muted());
            // TITLE (+ artist beneath).
            row.spawn(Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                ..default()
            })
            .with_children(|c| {
                label_text(
                    c,
                    fonts,
                    &track.title,
                    14.0,
                    if now_playing { accent } else { TEXT },
                    false,
                );
                label_text(c, fonts, &track.artist, 11.0, theme::text_muted(), false);
            });
            col(
                row,
                fonts,
                track.album.as_deref().unwrap_or("—"),
                180.0,
                theme::text_muted(),
            );
            col(row, fonts, world, 150.0, theme::TEXT_DIM);
            col(
                row,
                fonts,
                &fmt_time(track.duration),
                60.0,
                theme::text_muted(),
            );
        });
}

/// A fixed-width table cell.
fn col(parent: &mut ChildSpawnerCommands<'_>, fonts: &Fonts, s: &str, w: f32, color: Color) {
    parent
        .spawn(Node {
            width: Val::Px(w),
            ..default()
        })
        .with_children(|c| {
            c.spawn((
                Text::new(s.to_string()),
                text_font(fonts.ui.clone(), 12.0),
                TextColor(color),
            ));
        });
}

/// A growing table cell (the TITLE column header).
fn col_grow(parent: &mut ChildSpawnerCommands<'_>, fonts: &Fonts, s: &str, color: Color) {
    parent
        .spawn(Node {
            flex_grow: 1.0,
            ..default()
        })
        .with_children(|c| {
            c.spawn((
                Text::new(s.to_string()),
                text_font(fonts.ui.clone(), 12.0),
                TextColor(color),
            ));
        });
}

// ---------------------------------------------------------------------------
// Interaction
// ---------------------------------------------------------------------------
/// Library actions: play a row / the Play button, shuffle the shown set, or
/// switch the world filter (spec 1i).
pub fn library_actions(
    mut actions: MessageReader<super::actions::UiAction>,
    mut playback: ResMut<Playback>,
    mut theme: ResMut<Theme>,
    mut stack: ResMut<OverlayStack>,
    mut filter: ResMut<LibraryFilter>,
) {
    for super::actions::UiAction(action) in actions.read() {
        match action {
            ButtonAction::FilterWorld(i) => filter.0 = Some(*i),
            ButtonAction::FilterAll => filter.0 = None,
            ButtonAction::PlayIndex(i) if *i < playback.queue.len() => {
                play_index(&mut playback, &mut theme, &mut stack, *i);
            }
            ButtonAction::LibraryShuffle => {
                if !playback.shuffle {
                    playback.toggle_shuffle();
                }
                // Start from the first track of the current filter.
                let first = playback
                    .queue
                    .iter()
                    .position(|t| filter.0.is_none_or(|w| t.mood == w));
                if let Some(i) = first {
                    play_index(&mut playback, &mut theme, &mut stack, i);
                }
            }
            _ => {}
        }
    }
}

fn play_index(playback: &mut Playback, theme: &mut Theme, stack: &mut OverlayStack, idx: usize) {
    playback.current = idx;
    playback.elapsed = 0.0;
    playback.playing = true;
    playback.resequence();
    playback.revision += 1;
    theme.mood = playback.track().mood;
    stack.close(Overlay::Library);
}
