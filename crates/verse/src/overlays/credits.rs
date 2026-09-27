//! Credits & Licenses — real provenance (spec 1o).

use bevy::prelude::*;
use bevy::ui::GlobalZIndex;

use crate::theme::{self, Fonts, RADIUS_MD, RADIUS_SM, TEXT, Theme, text_font};
use crate::{Overlay, OverlayStack};

use super::actions::ButtonAction;
#[allow(unused_imports)]
use super::widgets::*;

#[derive(Component)]
pub struct CreditsRoot;

pub fn sync_credits_overlay(
    stack: Res<OverlayStack>,
    mut commands: Commands,
    fonts: Res<Fonts>,
    theme: Res<Theme>,
    assets: Res<crate::world_assets::WorldAssets>,
    existing: Query<Entity, With<CreditsRoot>>,
    mut last: Local<bool>,
) {
    let open = stack.is_open(Overlay::Credits);
    if open == *last {
        return;
    }
    *last = open;
    if open && existing.is_empty() {
        spawn_credits(&mut commands, &fonts, &theme, assets.manifest.as_ref());
    } else if !open {
        for e in &existing {
            commands.entity(e).despawn();
        }
    }
}

fn spawn_credits(
    commands: &mut Commands,
    fonts: &Fonts,
    theme: &Theme,
    manifest: Option<&crate::world_assets::AssetManifest>,
) {
    let accent = theme.accent();
    commands
        .spawn((
            CreditsRoot,
            full_screen_center(),
            BackgroundColor(theme::veil_panel()),
            GlobalZIndex(96),
        ))
        .with_children(|c| {
            c.spawn((
                rounded(
                    Node {
                        width: Val::Px(680.0),
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
                card.spawn(Node {
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|h| {
                    h.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(3.0),
                        ..default()
                    })
                    .with_children(|t| {
                        label_text(t, fonts, "Credits & Licenses", 22.0, TEXT, true);
                        label_text(
                            t,
                            fonts,
                            "Everyone whose work is in these worlds.",
                            11.5,
                            theme::text_muted(),
                            false,
                        );
                    });
                    button(
                        h,
                        fonts,
                        "Done",
                        Some("Esc"),
                        ButtonAction::CloseCredits,
                        true,
                        accent,
                    );
                });

                spacer(card, 14.0);
                card.spawn(Node {
                    column_gap: Val::Px(8.0),
                    row_gap: Val::Px(8.0),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|f| {
                    for (name, active) in [
                        ("All", true),
                        ("3D assets", false),
                        ("Sounds", false),
                        ("Open-source software", false),
                    ] {
                        nav_tab(f, fonts, name, active, accent);
                    }
                });

                spacer(card, 16.0);
                // Rows live in a scroll region — the full pack (50+ assets)
                // plus software credits overflows any fixed card.
                card.spawn((
                    Scrollable,
                    ScrollPosition::default(),
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        max_height: Val::Px(540.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|rows| {
                    // Real per-asset CC0/CC-BY attribution from the bundled
                    // asset manifest (PLAN.md M6); falls back to a note.
                    match manifest {
                        Some(m) if !m.assets.is_empty() => {
                            section_label(rows, fonts, &format!("3D ASSETS · {}", m.assets.len()));
                            for a in &m.assets {
                                let detail = format!(
                                    "{} · {}",
                                    a.tier_label(),
                                    theme::moods()[a.mood % theme::moods().len()].world_name
                                );
                                credit_row(
                                    rows, fonts, &a.name, &detail, &a.author, &a.license, accent,
                                );
                            }
                        }
                        _ => {
                            section_label(rows, fonts, "3D ASSETS");
                            label_text(
                                rows,
                                fonts,
                                "No asset packs bundled yet — worlds are procedural for now.",
                                11.5,
                                theme::text_muted(),
                                false,
                            );
                        }
                    }

                    spacer(rows, 14.0);
                    section_label(
                        rows,
                        fonts,
                        &format!("OPEN-SOURCE SOFTWARE · {}", SOFTWARE_CREDITS.len()),
                    );
                    for (name, detail, author, license) in SOFTWARE_CREDITS {
                        credit_row(rows, fonts, name, detail, author, license, accent);
                    }
                });
            });
        });
}

/// The app's real open-source dependencies, shown in Credits. Kept honest by
/// hand (the loud few; the full tree is in `Cargo.lock`).
const SOFTWARE_CREDITS: &[(&str, &str, &str, &str)] = &[
    (
        "Bevy Engine",
        "the engine LocalGPT Verse runs on",
        "Bevy contributors",
        "MIT / Apache-2.0",
    ),
    (
        "Kira",
        "audio mixer, clocks & tweens",
        "Andrew Minnich",
        "MIT / Apache-2.0",
    ),
    (
        "Symphonia",
        "audio decoding (MP3/FLAC/OGG…)",
        "Philip Deljanov",
        "MPL-2.0",
    ),
    (
        "Lofty",
        "music tag reading",
        "Serial-ATA",
        "MIT / Apache-2.0",
    ),
    (
        "RealFFT / RustFFT",
        "spectral analysis",
        "Henrik Enquist",
        "MIT / Apache-2.0",
    ),
    (
        "BLAKE3",
        "content-hash cache keys",
        "BLAKE3 team",
        "CC0 / Apache-2.0",
    ),
    ("rfd", "native folder picker", "PolyMeilex", "MIT"),
    #[cfg(feature = "ml")]
    (
        "ONNX Runtime",
        "local ML inference (optional `ml` feature)",
        "Microsoft",
        "MIT",
    ),
    #[cfg(feature = "ml")]
    (
        "rubato",
        "audio resampling (optional `ml` feature)",
        "Henrik Enquist",
        "MIT",
    ),
    #[cfg(feature = "ml")]
    (
        "CLAP (LAION, Xenova ONNX)",
        "zero-shot music understanding (optional `ml` feature)",
        "LAION / Xenova",
        "CC-BY-NC-4.0 weights",
    ),
];

/// An uppercase section header inside the credits list.
fn section_label(parent: &mut ChildSpawnerCommands<'_>, fonts: &Fonts, text: &str) {
    parent.spawn((
        Text::new(text.to_string()),
        text_font(fonts.ui_semibold.clone(), 10.5),
        TextColor(theme::text_muted().with_alpha(0.7)),
        Node {
            margin: UiRect::bottom(Val::Px(6.0)),
            ..default()
        },
    ));
}

/// One attribution row: title/detail, author, license chip, and a link.
fn credit_row(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    title: &str,
    detail: &str,
    author: &str,
    license: &str,
    accent: Color,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(14.0),
            padding: UiRect::vertical(Val::Px(8.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: Val::Px(2.0),
                ..default()
            })
            .with_children(|c| {
                label_text(c, fonts, title, 13.5, TEXT, false);
                label_text(c, fonts, detail, 11.0, theme::text_muted(), false);
            });
            label_text(row, fonts, author, 12.0, theme::text_muted(), false);
            row.spawn((
                rounded(
                    Node {
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                        ..default()
                    },
                    RADIUS_SM,
                ),
                BackgroundColor(theme::hairline()),
            ))
            .with_children(|chip| {
                label_text(chip, fonts, license, 10.5, theme::TEXT_DIM, false);
            });
            label_text(
                row,
                fonts,
                "source & license ↗",
                10.5,
                accent.with_alpha(0.9),
                false,
            );
        });
}
