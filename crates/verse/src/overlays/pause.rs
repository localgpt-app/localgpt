//! Pause overlay with world-intensity slider (spec 1k).

use bevy::prelude::*;
use bevy::ui::GlobalZIndex;

use crate::playback::{Playback, fmt_time};
use crate::theme::{self, Fonts, RADIUS_MD, RADIUS_PILL, TEXT, Theme};
use crate::{Paused, WorldIntensity};

use super::actions::ButtonAction;
#[allow(unused_imports)]
use super::widgets::*;

#[derive(Component)]
pub struct PauseRoot;
#[derive(Component)]
pub struct IntensityKnob;

#[allow(clippy::too_many_arguments)]
pub fn sync_pause_overlay(
    paused: Res<Paused>,
    mut commands: Commands,
    fonts: Res<Fonts>,
    theme: Res<Theme>,
    playback: Res<Playback>,
    intensity: Res<WorldIntensity>,
    active_recipe: Res<crate::recipe::ActiveRecipe>,
    analysis: Res<crate::analysis::AnalysisStore>,
    existing: Query<Entity, With<PauseRoot>>,
) {
    if !paused.is_changed() {
        return;
    }
    if paused.0 && existing.is_empty() {
        spawn_pause(
            &mut commands,
            &fonts,
            &theme,
            &playback,
            intensity.0,
            active_recipe.get(),
            &analysis,
        );
    } else if !paused.0 {
        for e in &existing {
            commands.entity(e).despawn();
        }
    }
}

fn spawn_pause(
    commands: &mut Commands,
    fonts: &Fonts,
    theme: &Theme,
    playback: &Playback,
    intensity: f32,
    recipe: Option<&crate::recipe::WorldRecipe>,
    analysis: &crate::analysis::AnalysisStore,
) {
    let accent = theme.accent();
    let track = playback.track();
    // The eyebrow names the world: the LLM recipe's authored name when it has
    // one (capped), else the mood's — same rule as the HUD's now-playing slot.
    let world_name = match recipe
        .map(|r| r.world_name.trim())
        .filter(|n| !n.is_empty())
    {
        Some(name) => {
            let capped: String = name.chars().take(39).collect();
            if name.chars().count() > 39 {
                format!("{capped}…")
            } else {
                capped
            }
        }
        None => theme.current().world_name.to_string(),
    };
    // Tier status: which imagination tier authored this world, and the agent's
    // closing description when it left one. Absent = the rule-derived path
    // (no line — nothing to claim).
    let build = track
        .id
        .as_deref()
        .and_then(|id| analysis.get(id))
        .and_then(|a| a.build.as_ref());
    let author_line = match (recipe, build) {
        (Some(_), Some(b)) => Some(format!(
            "imagined by Bonsai · agent scene: {} parts",
            b.commands.len()
        )),
        (Some(_), None) => Some("world imagined by Bonsai".to_string()),
        (None, Some(b)) => Some(format!("agent scene: {} parts", b.commands.len())),
        (None, None) => None,
    };
    let description = build.and_then(|b| b.description.as_deref()).map(|d| {
        let capped: String = d.chars().take(90).collect();
        if d.chars().count() > 90 {
            format!("“{capped}…”")
        } else {
            format!("“{capped}”")
        }
    });
    commands
        .spawn((
            PauseRoot,
            full_screen_center(),
            BackgroundColor(theme::veil_panel()),
            GlobalZIndex(90),
        ))
        .with_children(|c| {
            // Card.
            c.spawn((
                rounded(
                    Node {
                        width: Val::Px(640.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(Val::Px(34.0)),
                        row_gap: Val::Px(6.0),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    RADIUS_MD,
                ),
                BackgroundColor(Color::srgba(0.06, 0.06, 0.09, 0.75)),
                BorderColor::all(theme::hairline()),
            ))
            .with_children(|card| {
                label_text(
                    card,
                    fonts,
                    &format!("{world_name} · PAUSED"),
                    12.5,
                    theme::text_muted(),
                    false,
                );
                if let Some(line) = &author_line {
                    label_text(
                        card,
                        fonts,
                        line,
                        11.0,
                        theme::TEXT_DIM.with_alpha(0.7),
                        false,
                    );
                }
                if let Some(desc) = &description {
                    label_text(card, fonts, desc, 12.0, theme::text_muted(), false);
                }
                card.spawn(Node {
                    height: Val::Px(4.0),
                    ..default()
                });
                label_text(card, fonts, &track.title, 30.0, TEXT, true);
                label_text(
                    card,
                    fonts,
                    &format!(
                        "{} · {} of {}",
                        track.section,
                        fmt_time(playback.elapsed),
                        fmt_time(track.duration)
                    ),
                    12.5,
                    theme::text_muted(),
                    false,
                );
                card.spawn(Node {
                    height: Val::Px(22.0),
                    ..default()
                });

                // Primary + secondary actions.
                card.spawn(Node {
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|row| {
                    button(
                        row,
                        fonts,
                        "Resume",
                        Some("Esc"),
                        ButtonAction::Resume,
                        true,
                        accent,
                    );
                    button(
                        row,
                        fonts,
                        "Build a different world",
                        Some("R"),
                        ButtonAction::BuildWorld,
                        false,
                        accent,
                    );
                    button(
                        row,
                        fonts,
                        "Keep this world",
                        None,
                        ButtonAction::KeepWorld,
                        false,
                        accent,
                    );
                });

                card.spawn(Node {
                    height: Val::Px(24.0),
                    ..default()
                });

                // World intensity slider.
                card.spawn(Node {
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|col| {
                    col.spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        width: Val::Percent(100.0),
                        ..default()
                    })
                    .with_children(|hdr| {
                        label_text(hdr, fonts, "World intensity", 12.5, TEXT, false);
                        label_text(hdr, fonts, "← →", 12.5, theme::text_muted(), false);
                    });
                    // Track + knob.
                    col.spawn((
                        rounded(
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(4.0),
                                ..default()
                            },
                            RADIUS_PILL,
                        ),
                        BackgroundColor(theme::hairline()),
                    ))
                    .with_children(|tk| {
                        tk.spawn((
                            IntensityKnob,
                            rounded(
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Percent(intensity * 100.0),
                                    top: Val::Px(-5.0),
                                    width: Val::Px(14.0),
                                    height: Val::Px(14.0),
                                    margin: UiRect::left(Val::Px(-7.0)),
                                    ..default()
                                },
                                RADIUS_PILL,
                            ),
                            BackgroundColor(accent),
                        ));
                    });
                    col.spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        width: Val::Percent(100.0),
                        ..default()
                    })
                    .with_children(|lbl| {
                        label_text(lbl, fonts, "calm", 11.0, theme::text_muted(), false);
                        label_text(lbl, fonts, "lively", 11.0, theme::text_muted(), false);
                        label_text(lbl, fonts, "intense", 11.0, theme::text_muted(), false);
                    });
                    label_text(
                        col,
                        fonts,
                        "also caps flashing & camera motion — see Comfort in Settings",
                        10.5,
                        theme::text_muted().with_alpha(0.6),
                        false,
                    );
                });

                card.spawn(Node {
                    height: Val::Px(24.0),
                    ..default()
                });

                // Footer actions.
                card.spawn(Node {
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|row| {
                    button(
                        row,
                        fonts,
                        "Photo mode",
                        Some("P"),
                        ButtonAction::PhotoMode,
                        false,
                        accent,
                    );
                    button(
                        row,
                        fonts,
                        "Settings",
                        None,
                        ButtonAction::OpenSettings,
                        false,
                        accent,
                    );
                    button(
                        row,
                        fonts,
                        "Quit LocalGPT Verse",
                        Some("Q"),
                        ButtonAction::Quit,
                        false,
                        accent,
                    );
                });
                card.spawn(Node {
                    height: Val::Px(12.0),
                    ..default()
                });
                label_text(
                    card,
                    fonts,
                    "time holds its breath while you're here",
                    11.0,
                    theme::text_muted().with_alpha(0.6),
                    false,
                );
            });
        });
}

/// Live-update the knob position while paused.
pub fn update_intensity_knob(
    intensity: Res<WorldIntensity>,
    mut q: Query<&mut Node, With<IntensityKnob>>,
) {
    if !intensity.is_changed() {
        return;
    }
    for mut n in &mut q {
        n.left = Val::Percent(intensity.0 * 100.0);
    }
}
