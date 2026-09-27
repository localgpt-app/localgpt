//! The in-world HUD.
//!
//! Implements the "one system, three states" chrome from the spec (1a / 1e /
//! 1r): **Visible** during input and for a few seconds after, **Minimized** to
//! a breathing hairline once idle, then **Hidden** entirely — world only. Any
//! input wakes it. The single variable is the accent, sampled from the world.
//!
//! The Visible state carries the full "now playing" transport from spec 1a:
//! corner affordances (Library / Queue), a now-playing cluster, a centred
//! control cluster (shuffle · prev · −15s · play/pause · +15s · next · repeat
//! over a scrubber), and a next-up + Explore/Drift toggle. When the HUD
//! minimises the cluster fades out and only a full-width progress hairline
//! remains (spec 1e).

use bevy::prelude::*;

use crate::playback::{Beat, Playback, Repeat, fmt_time};
use crate::theme::{self, Fonts, RADIUS_PILL, RADIUS_SM, TEXT, Theme, text_font};
use crate::{CameraMode, Comfort};

// ---------------------------------------------------------------------------
// Layout constants — the centred scrubber (spec 1a). Fixed widths keep the
// seek math a pure function of the window width (see `seek_strip_scrub`).
// ---------------------------------------------------------------------------

/// Whole control-cluster width (also the centred column width).
const CLUSTER_W: f32 = 560.0;
/// Elapsed / total time label widths flanking the bar.
const TIME_W_LEFT: f32 = 46.0;
const TIME_W_RIGHT: f32 = 44.0;
/// The little equaliser to the right of the bar.
const WAVE_W: f32 = 58.0;
/// Gap between scrubber-row items.
const SCRUB_GAP: f32 = 12.0;
/// The seekable bar width = cluster − times − waveform − 3 gaps.
const BAR_W: f32 = CLUSTER_W - TIME_W_LEFT - TIME_W_RIGHT - WAVE_W - 3.0 * SCRUB_GAP;
/// The bar's left edge, measured from the cluster's left edge.
const BAR_LEFT_IN_CLUSTER: f32 = TIME_W_LEFT + SCRUB_GAP;

// The play/pause/skip icons are built from nodes (see `triangle` / `bar`) so
// they're crisp, monochrome, and accent-tintable. Shuffle / repeat use glyphs,
// picked for what the Windows system-font fallback actually renders monochrome:
// the media-control symbols (U+23xx) and geometric shapes (U+25xx) come back as
// colour emoji, and the rotational-loop arrows (↻ ⟳ …) render as tofu. `⇄`
// (crossed arrows → shuffle) and `∞` (endless loop → repeat) both resolve to a
// monochrome glyph.
const ICON_SHUFFLE: &str = "⇄";
const ICON_REPEAT: &str = "∞";

// ---------------------------------------------------------------------------
// HUD depth + activity
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HudDepth {
    Visible,
    Minimized,
    Hidden,
}

/// Idle tracking + the two eased global alphas that drive the fades.
#[derive(Resource)]
pub struct HudActivity {
    pub idle: f32,
    pub depth: HudDepth,
    /// Alpha for "full" chrome — 1 only while Visible.
    pub full: f32,
    /// Alpha for "minimal" chrome — 1 while Visible or Minimized.
    pub minimal: f32,
    /// Set true for one wake to force the timers back to zero.
    pub force_visible: bool,
}

impl Default for HudActivity {
    fn default() -> Self {
        Self {
            idle: 0.0,
            depth: HudDepth::Visible,
            full: 1.0,
            minimal: 1.0,
            force_visible: true,
        }
    }
}

impl HudActivity {
    /// Any input calls this — snap awake.
    pub fn wake(&mut self) {
        self.idle = 0.0;
        self.force_visible = true;
    }
}

// ---------------------------------------------------------------------------
// Fade plumbing — Bevy UI has no opacity inheritance, so each coloured node
// carries its intrinsic colour and the group whose global alpha scales it.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum FadeGroup {
    /// Visible only — the full cluster.
    Full,
    /// Visible + Minimized (kept for text that lingers into Minimized).
    Minimal,
    /// Minimized only — the breathing hairline that replaces the cluster once
    /// it fades (spec 1e). Alpha = `minimal − full`, so it crossfades in as the
    /// full cluster fades out and vanishes entirely when Visible or Hidden.
    MinimalOnly,
}

#[derive(Clone, Copy)]
pub enum FadeTarget {
    Bg,
    Text,
}

#[derive(Component)]
pub struct Fade {
    pub group: FadeGroup,
    pub base: Color,
    pub target: FadeTarget,
}

/// Marks a node whose `Fade.base` tracks the current accent.
#[derive(Component)]
pub struct AccentTint;

// Dynamic-content markers. `pub(crate)` because they appear in the signatures
// of the `pub` systems below (scheduled from `main`).
#[derive(Component)]
pub(crate) struct WorldNameText;
#[derive(Component)]
pub(crate) struct TrackTitleText;
#[derive(Component)]
pub(crate) struct SectionText;
#[derive(Component)]
pub(crate) struct TimeElapsedText;
#[derive(Component)]
pub(crate) struct TimeTotalText;
#[derive(Component)]
pub(crate) struct NextTrackText;
#[derive(Component)]
pub(crate) struct ProgressFill;
#[derive(Component)]
pub(crate) struct ProgressPlayhead;
#[derive(Component)]
pub(crate) struct ProgressBar;
#[derive(Component)]
pub(crate) struct SectionNotch;
/// The full-width breadcrumb fill shown only while Minimized (spec 1e).
#[derive(Component)]
pub(crate) struct MiniFill;
/// Invisible strip over the scrubber bar — click/drag to seek.
#[derive(Component)]
pub(crate) struct SeekStrip;
#[derive(Component)]
pub(crate) struct ModeTab(CameraMode);
#[derive(Component)]
pub(crate) struct Reticle;

// Transport cluster (spec 1a).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Transport {
    Shuffle,
    Prev,
    Back,
    PlayPause,
    Fwd,
    Next,
    Repeat,
}

#[derive(Component)]
pub(crate) struct TransportButton(Transport);
/// The ▶ triangle group on the play/pause button — shown while paused.
#[derive(Component)]
pub(crate) struct PlayIcon;
/// The ⏸ two-bar group on the play/pause button — shown while playing.
#[derive(Component)]
pub(crate) struct PauseIcon;
/// The shuffle glyph — tinted to accent when shuffle is on.
#[derive(Component)]
pub(crate) struct ShuffleIcon;
/// The repeat glyph — tinted to accent when repeat ≠ off.
#[derive(Component)]
pub(crate) struct RepeatIcon;
/// The small "1" badge on the repeat icon — shown only for repeat-one.
#[derive(Component)]
pub(crate) struct RepeatOneBadge;

#[derive(Component)]
struct HudRoot;

/// Which corner chip — used to route clicks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChipKind {
    Library,
    Queue,
}

#[derive(Component)]
pub(crate) struct HudChip(ChipKind);

// A pill/rounded Node helper (border_radius is a Node field in 0.19).
fn rounded(mut node: Node, radius: f32) -> Node {
    node.border_radius = BorderRadius::all(Val::Px(radius));
    node
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

pub fn setup_hud(
    mut commands: Commands,
    fonts: Res<Fonts>,
    theme: Res<Theme>,
    playback: Res<Playback>,
    active_recipe: Res<crate::recipe::ActiveRecipe>,
) {
    let accent = theme.accent();
    let track = playback.track();
    let world_name = world_label(&theme, active_recipe.get());

    commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            // The HUD never eats clicks meant for the world.
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            // --- Top-left: Library --------------------------------------------
            root.spawn(Node {
                position_type: PositionType::Absolute,
                top: Val::Px(26.0),
                left: Val::Px(30.0),
                ..default()
            })
            .with_children(|row| {
                chip(row, &fonts, "‹  Library", "L", ChipKind::Library);
            });

            // --- Top-right: Queue ---------------------------------------------
            root.spawn(Node {
                position_type: PositionType::Absolute,
                top: Val::Px(26.0),
                right: Val::Px(30.0),
                ..default()
            })
            .with_children(|row| {
                chip(row, &fonts, "Queue", "Tab", ChipKind::Queue);
            });

            // --- Bottom-left now-playing cluster ------------------------------
            root.spawn(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(58.0),
                left: Val::Px(30.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(7.0),
                max_width: Val::Px(430.0),
                ..default()
            })
            .with_children(|col| {
                // Eyebrow: accent dot + world name.
                col.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(9.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        AccentTint,
                        rounded(
                            Node {
                                width: Val::Px(10.0),
                                height: Val::Px(10.0),
                                ..default()
                            },
                            RADIUS_PILL,
                        ),
                        BackgroundColor(accent),
                        Fade {
                            group: FadeGroup::Full,
                            base: accent,
                            target: FadeTarget::Bg,
                        },
                    ));
                    row.spawn((
                        WorldNameText,
                        Text::new(world_name),
                        text_font(fonts.ui_semibold.clone(), 11.5),
                        TextColor(theme::text_muted()),
                        Fade {
                            group: FadeGroup::Full,
                            base: theme::text_muted(),
                            target: FadeTarget::Text,
                        },
                    ));
                });
                // Title — the hero line (Marcellus), lingers into Minimized.
                col.spawn((
                    TrackTitleText,
                    Text::new(track.title.clone()),
                    text_font(fonts.display.clone(), 34.0),
                    TextColor(TEXT),
                    Fade {
                        group: FadeGroup::Minimal,
                        base: TEXT,
                        target: FadeTarget::Text,
                    },
                ));
                // Section subtitle.
                col.spawn((
                    SectionText,
                    Text::new(track.section.clone()),
                    text_font(fonts.ui.clone(), 13.0),
                    TextColor(theme::TEXT_DIM.with_alpha(0.72)),
                    Fade {
                        group: FadeGroup::Full,
                        base: theme::TEXT_DIM.with_alpha(0.72),
                        target: FadeTarget::Text,
                    },
                ));
            });

            // --- Bottom-right: next-up + Explore/Drift ------------------------
            root.spawn(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(58.0),
                right: Val::Px(30.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Px(12.0),
                ..default()
            })
            .with_children(|col| {
                next_pill(col, &fonts, &playback, accent);
                // Mode toggle (Explore | Drift).
                col.spawn((
                    rounded(
                        Node {
                            padding: UiRect::all(Val::Px(4.0)),
                            column_gap: Val::Px(4.0),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        RADIUS_PILL,
                    ),
                    BackgroundColor(theme::veil_hud()),
                    BorderColor::all(theme::hairline()),
                    Fade {
                        group: FadeGroup::Full,
                        base: theme::veil_hud(),
                        target: FadeTarget::Bg,
                    },
                ))
                .with_children(|tabs| {
                    mode_tab(tabs, &fonts, "Explore", CameraMode::Explore, accent, true);
                    mode_tab(tabs, &fonts, "Drift", CameraMode::Drift, accent, false);
                });
            });

            // --- Bottom-centre: hints · transport · scrubber ------------------
            root.spawn(Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(30.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-CLUSTER_W / 2.0)),
                width: Val::Px(CLUSTER_W),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(14.0),
                ..default()
            })
            .with_children(|col| {
                // Control hints.
                col.spawn((
                    Text::new("W A S D  move   ·   E  pulse   ·   Tab  queue   ·   Esc  pause"),
                    text_font(fonts.ui.clone(), 11.5),
                    TextColor(theme::text_muted()),
                    Fade {
                        group: FadeGroup::Full,
                        base: theme::text_muted(),
                        target: FadeTarget::Text,
                    },
                ));

                // Transport row.
                col.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(14.0),
                    ..default()
                })
                .with_children(|row| {
                    transport_button(row, &fonts, Transport::Shuffle, Btn::Dim, accent);
                    transport_button(row, &fonts, Transport::Prev, Btn::Solid, accent);
                    transport_button(row, &fonts, Transport::Back, Btn::Ghost, accent);
                    transport_button(row, &fonts, Transport::PlayPause, Btn::Primary, accent);
                    transport_button(row, &fonts, Transport::Fwd, Btn::Ghost, accent);
                    transport_button(row, &fonts, Transport::Next, Btn::Solid, accent);
                    transport_button(row, &fonts, Transport::Repeat, Btn::Dim, accent);
                });

                // Scrubber row: elapsed · bar · total · waveform.
                col.spawn(Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(SCRUB_GAP),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((
                        TimeElapsedText,
                        time_node(TIME_W_LEFT),
                        Text::new(fmt_time(playback.elapsed)),
                        text_font(fonts.ui_medium.clone(), 13.5),
                        TextColor(TEXT),
                        Fade {
                            group: FadeGroup::Full,
                            base: TEXT,
                            target: FadeTarget::Text,
                        },
                    ));
                    // The bar (fill + notches + playhead + seek strip).
                    row.spawn((
                        ProgressBar,
                        rounded(
                            Node {
                                width: Val::Px(BAR_W),
                                height: Val::Px(4.0),
                                ..default()
                            },
                            RADIUS_PILL,
                        ),
                        BackgroundColor(theme::hairline()),
                        Fade {
                            group: FadeGroup::Full,
                            base: theme::hairline(),
                            target: FadeTarget::Bg,
                        },
                    ))
                    .with_children(|bar| {
                        bar.spawn((
                            ProgressFill,
                            AccentTint,
                            rounded(
                                Node {
                                    width: Val::Percent(playback.fraction() * 100.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                RADIUS_PILL,
                            ),
                            BackgroundColor(accent),
                            Fade {
                                group: FadeGroup::Full,
                                base: accent,
                                target: FadeTarget::Bg,
                            },
                        ));
                        for f in &playback.sections {
                            spawn_notch(bar, *f);
                        }
                        bar.spawn((
                            ProgressPlayhead,
                            AccentTint,
                            rounded(
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Percent(playback.fraction() * 100.0),
                                    top: Val::Px(-4.5),
                                    width: Val::Px(13.0),
                                    height: Val::Px(13.0),
                                    margin: UiRect::left(Val::Px(-6.5)),
                                    ..default()
                                },
                                RADIUS_PILL,
                            ),
                            BackgroundColor(accent),
                            Fade {
                                group: FadeGroup::Full,
                                base: accent,
                                target: FadeTarget::Bg,
                            },
                        ));
                        // Tall invisible hit strip over the bar (click/drag).
                        bar.spawn((
                            SeekStrip,
                            Button,
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                right: Val::Px(0.0),
                                top: Val::Px(-9.0),
                                bottom: Val::Px(-9.0),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ));
                    });
                    row.spawn((
                        TimeTotalText,
                        time_node(TIME_W_RIGHT),
                        Text::new(fmt_time(track.duration)),
                        text_font(fonts.ui.clone(), 13.5),
                        TextColor(theme::text_muted()),
                        Fade {
                            group: FadeGroup::Full,
                            base: theme::text_muted(),
                            target: FadeTarget::Text,
                        },
                    ));
                    waveform(row, accent);
                });
            });

            // --- Minimized breadcrumb: full-width hairline (spec 1e) ----------
            root.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(0.0),
                    left: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Px(2.0),
                    ..default()
                },
                BackgroundColor(theme::hairline()),
                Fade {
                    group: FadeGroup::MinimalOnly,
                    base: theme::hairline(),
                    target: FadeTarget::Bg,
                },
            ))
            .with_children(|bar| {
                bar.spawn((
                    MiniFill,
                    AccentTint,
                    Node {
                        width: Val::Percent(playback.fraction() * 100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(accent),
                    Fade {
                        group: FadeGroup::MinimalOnly,
                        base: accent,
                        target: FadeTarget::Bg,
                    },
                ));
            });

            // --- Explore reticle (survives into Hidden) ----------------------
            root.spawn((
                Reticle,
                rounded(
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(50.0),
                        top: Val::Percent(50.0),
                        width: Val::Px(5.0),
                        height: Val::Px(5.0),
                        margin: UiRect::new(Val::Px(-2.5), Val::ZERO, Val::Px(-2.5), Val::ZERO),
                        ..default()
                    },
                    RADIUS_PILL,
                ),
                BackgroundColor(TEXT.with_alpha(0.5)),
            ));
        });
}

/// A fixed-width, tabular time label node.
fn time_node(w: f32) -> Node {
    Node {
        width: Val::Px(w),
        ..default()
    }
}

/// The little equaliser bars to the right of the scrubber (decorative, accent).
fn waveform(parent: &mut ChildSpawnerCommands<'_>, accent: Color) {
    parent
        .spawn(Node {
            width: Val::Px(WAVE_W),
            height: Val::Px(15.0),
            align_items: AlignItems::FlexEnd,
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(2.5),
            ..default()
        })
        .with_children(|w| {
            // Height / opacity pattern lifted from the 1a mockup.
            for (h, a) in [
                (6.0, 0.40),
                (10.0, 0.55),
                (15.0, 0.90),
                (8.0, 0.50),
                (12.0, 0.65),
                (5.0, 0.35),
                (9.0, 0.50),
            ] {
                let col = accent.with_alpha(a);
                w.spawn((
                    AccentTint,
                    rounded(
                        Node {
                            width: Val::Px(3.0),
                            height: Val::Px(h),
                            ..default()
                        },
                        1.0,
                    ),
                    BackgroundColor(col),
                    Fade {
                        group: FadeGroup::Full,
                        base: col,
                        target: FadeTarget::Bg,
                    },
                ));
            }
        });
}

/// The bottom-right "next up" pill: accent swatch + NEXT label + track.
fn next_pill(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    playback: &Playback,
    accent: Color,
) {
    let next = playback.next_track();
    parent
        .spawn((
            rounded(
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(11.0),
                    padding: UiRect::new(Val::Px(9.0), Val::Px(16.0), Val::Px(8.0), Val::Px(8.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                RADIUS_PILL,
            ),
            BackgroundColor(theme::veil_hud()),
            BorderColor::all(theme::hairline()),
            Fade {
                group: FadeGroup::Full,
                base: theme::veil_hud(),
                target: FadeTarget::Bg,
            },
        ))
        .with_children(|pill| {
            pill.spawn((
                AccentTint,
                rounded(
                    Node {
                        width: Val::Px(28.0),
                        height: Val::Px(28.0),
                        ..default()
                    },
                    RADIUS_PILL,
                ),
                BackgroundColor(accent),
                Fade {
                    group: FadeGroup::Full,
                    base: accent,
                    target: FadeTarget::Bg,
                },
            ));
            pill.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(1.0),
                ..default()
            })
            .with_children(|col| {
                col.spawn((
                    Text::new("NEXT"),
                    text_font(fonts.ui_semibold.clone(), 9.0),
                    TextColor(theme::text_muted()),
                    Fade {
                        group: FadeGroup::Full,
                        base: theme::text_muted(),
                        target: FadeTarget::Text,
                    },
                ));
                col.spawn((
                    NextTrackText,
                    Text::new(format!("{} — {}", next.title, next.artist)),
                    text_font(fonts.ui_medium.clone(), 13.0),
                    TextColor(TEXT),
                    Fade {
                        group: FadeGroup::Full,
                        base: TEXT,
                        target: FadeTarget::Text,
                    },
                ));
            });
        });
}

/// Visual style of a transport button.
#[derive(Clone, Copy)]
enum Btn {
    /// Big accent-filled play/pause.
    Primary,
    /// Prev / next — veil fill with a hairline ring.
    Solid,
    /// −15s / +15s — bare, bright glyph.
    Ghost,
    /// Shuffle / repeat — bare, dim glyph (tints to accent when active).
    Dim,
}

/// Spawn one transport button and its icon.
fn transport_button(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    action: Transport,
    style: Btn,
    accent: Color,
) {
    let (dia, bg, fg, border, accent_bg) = match style {
        Btn::Primary => (56.0, accent, theme::BASE, false, true),
        Btn::Solid => (44.0, theme::veil_hud(), TEXT, true, false),
        Btn::Ghost => (40.0, Color::NONE, TEXT.with_alpha(0.80), false, false),
        Btn::Dim => (34.0, Color::NONE, theme::text_muted(), false, false),
    };

    let mut btn = parent.spawn((
        TransportButton(action),
        Button,
        rounded(
            Node {
                width: Val::Px(dia),
                height: Val::Px(dia),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: if border {
                    UiRect::all(Val::Px(1.0))
                } else {
                    UiRect::ZERO
                },
                ..default()
            },
            RADIUS_PILL,
        ),
        BackgroundColor(bg),
        Fade {
            group: FadeGroup::Full,
            base: bg,
            target: FadeTarget::Bg,
        },
    ));
    if border {
        btn.insert(BorderColor::all(theme::hairline()));
    }
    if accent_bg {
        btn.insert(AccentTint);
    }
    btn.with_children(|b| build_icon(b, fonts, action, fg));
}

/// Build a transport button's icon: node shapes for the transport triangles /
/// bars, monochrome arrow glyphs for shuffle / repeat.
fn build_icon(b: &mut ChildSpawnerCommands<'_>, fonts: &Fonts, action: Transport, fg: Color) {
    match action {
        Transport::Shuffle => glyph_icon(b, fonts, ICON_SHUFFLE, 15.0, fg, ShuffleIcon),
        Transport::Repeat => {
            // `∞` + a small "1" badge shown only for repeat-one.
            b.spawn((
                Node {
                    align_items: AlignItems::Center,
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|row| {
                row.spawn((
                    RepeatIcon,
                    Text::new(ICON_REPEAT.to_string()),
                    text_font(fonts.ui_medium.clone(), 16.0),
                    TextColor(fg),
                    Fade {
                        group: FadeGroup::Full,
                        base: fg,
                        target: FadeTarget::Text,
                    },
                    Pickable::IGNORE,
                ));
                row.spawn((
                    RepeatOneBadge,
                    Visibility::Hidden,
                    Text::new("1"),
                    text_font(fonts.ui_semibold.clone(), 9.5),
                    TextColor(fg),
                    Fade {
                        group: FadeGroup::Full,
                        base: fg,
                        target: FadeTarget::Text,
                    },
                    Pickable::IGNORE,
                ));
            });
        }
        Transport::Prev => icon_row(b, 2.0, |r| {
            bar(r, 3.0, 13.0, fg);
            triangle(r, 13.0, false, fg);
        }),
        Transport::Next => icon_row(b, 2.0, |r| {
            triangle(r, 13.0, true, fg);
            bar(r, 3.0, 13.0, fg);
        }),
        Transport::Back => icon_row(b, 1.0, |r| {
            triangle(r, 12.0, false, fg);
            triangle(r, 12.0, false, fg);
        }),
        Transport::Fwd => icon_row(b, 1.0, |r| {
            triangle(r, 12.0, true, fg);
            triangle(r, 12.0, true, fg);
        }),
        Transport::PlayPause => {
            // Pause — two bars, shown while playing.
            b.spawn((
                PauseIcon,
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(5.0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|g| {
                bar(g, 4.5, 18.0, fg);
                bar(g, 4.5, 18.0, fg);
            });
            // Play — a triangle, shown while paused (nudged for optical centre).
            b.spawn((
                PlayIcon,
                Visibility::Hidden,
                Node {
                    align_items: AlignItems::Center,
                    margin: UiRect::left(Val::Px(3.0)),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|g| triangle(g, 19.0, true, fg));
        }
    }
}

/// A single-glyph icon carrying a marker component (shuffle / repeat).
fn glyph_icon(
    b: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    glyph: &str,
    size: f32,
    fg: Color,
    marker: impl Component,
) {
    b.spawn((
        marker,
        Text::new(glyph.to_string()),
        text_font(fonts.ui_medium.clone(), size),
        TextColor(fg),
        Fade {
            group: FadeGroup::Full,
            base: fg,
            target: FadeTarget::Text,
        },
        // Let the press fall through to the parent button.
        Pickable::IGNORE,
    ));
}

/// A horizontal container for multi-part icons (bar + triangle, etc.).
fn icon_row(
    b: &mut ChildSpawnerCommands<'_>,
    gap: f32,
    build: impl FnOnce(&mut ChildSpawnerCommands<'_>),
) {
    b.spawn((
        Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(gap),
            ..default()
        },
        Pickable::IGNORE,
    ))
    .with_children(build);
}

/// A rounded vertical bar (pause halves, prev/next stops).
fn bar(parent: &mut ChildSpawnerCommands<'_>, w: f32, h: f32, color: Color) {
    parent.spawn((
        rounded(
            Node {
                width: Val::Px(w),
                height: Val::Px(h),
                ..default()
            },
            1.0,
        ),
        BackgroundColor(color),
        Fade {
            group: FadeGroup::Full,
            base: color,
            target: FadeTarget::Bg,
        },
        Pickable::IGNORE,
    ));
}

/// A filled isosceles triangle, built from centre-aligned vertical bars that
/// taper toward the apex. Pure flex layout — no `Transform` rotation, which
/// Bevy's UI layout overwrites (B0004) — so it stays crisp and monochrome
/// (the media-symbol glyphs render as colour emoji here). `point_right` sets
/// the apex direction; `h` is the base height, the depth is ≈0.6·h.
fn triangle(parent: &mut ChildSpawnerCommands<'_>, h: f32, point_right: bool, color: Color) {
    // Thin bars butted together (no gap) so the interior reads as a solid fill
    // and only the tapered top/bottom edges step.
    let bw = 1.25;
    let depth = h * 0.62;
    let n = ((depth / bw).round() as usize).clamp(5, 11);
    parent
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|row| {
            for i in 0..n {
                // Tall at the base, shrinking to a point at the apex.
                let step = if point_right {
                    (n - i) as f32
                } else {
                    (i + 1) as f32
                };
                let bh = (h * step / n as f32).max(1.5);
                row.spawn((
                    Node {
                        width: Val::Px(bw),
                        height: Val::Px(bh),
                        ..default()
                    },
                    BackgroundColor(color),
                    Fade {
                        group: FadeGroup::Full,
                        base: color,
                        target: FadeTarget::Bg,
                    },
                    Pickable::IGNORE,
                ));
            }
        });
}

/// A small corner chip: `label` + a keycap.
fn chip(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    label: &str,
    key: &str,
    kind: ChipKind,
) {
    parent
        .spawn((
            HudChip(kind),
            Button,
            rounded(
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(7.0),
                    padding: UiRect::axes(Val::Px(11.0), Val::Px(6.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                RADIUS_SM,
            ),
            BackgroundColor(theme::veil_hud()),
            BorderColor::all(theme::hairline()),
            Fade {
                group: FadeGroup::Full,
                base: theme::veil_hud(),
                target: FadeTarget::Bg,
            },
        ))
        .with_children(|c| {
            c.spawn((
                Text::new(label.to_string()),
                text_font(fonts.ui_medium.clone(), 12.5),
                TextColor(TEXT),
                Fade {
                    group: FadeGroup::Full,
                    base: TEXT,
                    target: FadeTarget::Text,
                },
            ));
            c.spawn((
                keycap_node(),
                BackgroundColor(theme::hairline()),
                Fade {
                    group: FadeGroup::Full,
                    base: theme::hairline(),
                    target: FadeTarget::Bg,
                },
            ))
            .with_children(|k| {
                k.spawn((
                    Text::new(key.to_string()),
                    text_font(fonts.ui_semibold.clone(), 10.5),
                    TextColor(theme::text_muted()),
                    Fade {
                        group: FadeGroup::Full,
                        base: theme::text_muted(),
                        target: FadeTarget::Text,
                    },
                ));
            });
        });
}

fn keycap_node() -> Node {
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
    )
}

fn mode_tab(
    parent: &mut ChildSpawnerCommands<'_>,
    fonts: &Fonts,
    label: &str,
    mode: CameraMode,
    accent: Color,
    active: bool,
) {
    let bg = if active {
        accent.with_alpha(0.18)
    } else {
        Color::NONE
    };
    parent
        .spawn((
            ModeTab(mode),
            Button, // clickable — switches camera feel (see `mode_tab_clicks`)
            rounded(
                Node {
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(7.0)),
                    ..default()
                },
                RADIUS_PILL,
            ),
            BackgroundColor(bg),
            Fade {
                group: FadeGroup::Full,
                base: bg,
                target: FadeTarget::Bg,
            },
        ))
        .with_children(|t| {
            t.spawn((
                Text::new(label.to_string()),
                text_font(fonts.ui_semibold.clone(), 12.5),
                TextColor(if active { TEXT } else { theme::text_muted() }),
                Fade {
                    group: FadeGroup::Full,
                    base: if active { TEXT } else { theme::text_muted() },
                    target: FadeTarget::Text,
                },
            ));
        });
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Grow the idle timer and resolve the HUD depth from it.
pub fn hud_depth(time: Res<Time>, mode: Res<CameraMode>, mut activity: ResMut<HudActivity>) {
    if activity.force_visible {
        activity.idle = 0.0;
        activity.force_visible = false;
    } else {
        activity.idle += time.delta_secs();
    }

    let hidden_after = match *mode {
        CameraMode::Explore => 12.0,
        CameraMode::Drift => 8.0,
    };
    activity.depth = if activity.idle < 4.0 {
        HudDepth::Visible
    } else if activity.idle < hidden_after {
        HudDepth::Minimized
    } else {
        HudDepth::Hidden
    };

    // Ease the two global alphas. Rising (wake) is fast (~180ms); falling
    // (sleep) is slower (~600ms).
    let dt = time.delta_secs();
    let target_full = matches!(activity.depth, HudDepth::Visible) as u8 as f32;
    let target_min = matches!(activity.depth, HudDepth::Visible | HudDepth::Minimized) as u8 as f32;
    activity.full = ease(activity.full, target_full, dt);
    activity.minimal = ease(activity.minimal, target_min, dt);
}

fn ease(current: f32, target: f32, dt: f32) -> f32 {
    let rate = if target > current { 10.0 } else { 3.0 }; // ~180ms vs ~600ms
    let step = (target - current) * (dt * rate).min(1.0);
    (current + step).clamp(0.0, 1.0)
}

/// Apply the eased alphas to every faded node.
#[allow(clippy::type_complexity)]
pub fn apply_hud_alpha(
    activity: Res<HudActivity>,
    mut q: Query<(
        &Fade,
        Option<&mut BackgroundColor>,
        Option<&mut TextColor>,
        Option<&mut BorderColor>,
    )>,
) {
    for (fade, bg, text, border) in &mut q {
        let g = match fade.group {
            FadeGroup::Full => activity.full,
            FadeGroup::Minimal => activity.minimal,
            FadeGroup::MinimalOnly => (activity.minimal - activity.full).clamp(0.0, 1.0),
        };
        let col = fade.base.with_alpha(fade.base.alpha() * g);
        match fade.target {
            FadeTarget::Bg => {
                if let Some(mut bg) = bg {
                    bg.0 = col;
                }
            }
            FadeTarget::Text => {
                if let Some(mut t) = text {
                    t.0 = col;
                }
            }
        }
        // HUD borders are all hairline; fade them with the same group so the
        // chrome disappears completely (no ghost outlines when Hidden).
        if let Some(mut b) = border {
            let h = theme::hairline();
            *b = BorderColor::all(h.with_alpha(h.alpha() * g));
        }
    }
}

/// Keep accent-tinted nodes in sync with the current world's accent.
pub fn update_hud_accent(theme: Res<Theme>, mut q: Query<&mut Fade, With<AccentTint>>) {
    if !theme.is_changed() {
        return;
    }
    let accent = theme.accent();
    for mut fade in &mut q {
        fade.base = accent.with_alpha(fade.base.alpha());
    }
}

/// The eyebrow label for the now-playing cluster: the LLM recipe's world name
/// when it authored one (M7 — the one piece of the recipe's free text the
/// user sees), capped at 40 chars; else the mood's name.
fn world_label(theme: &Theme, recipe: Option<&crate::recipe::WorldRecipe>) -> String {
    match recipe
        .map(|r| r.world_name.trim())
        .filter(|n| !n.is_empty())
    {
        Some(name) => char_cap(name, 40),
        None => theme.current().world_name.to_string(),
    }
}

/// Truncate to at most `max` chars on a char (not byte) boundary.
fn char_cap(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let capped: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{capped}…")
}

/// Sync dynamic text + progress geometry to the transport.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_hud_content(
    playback: Res<Playback>,
    beat: Res<Beat>,
    comfort: Res<Comfort>,
    theme: Res<Theme>,
    active_recipe: Res<crate::recipe::ActiveRecipe>,
    mut sets: ParamSet<(
        Query<&mut Text, With<WorldNameText>>,
        Query<&mut Text, With<TrackTitleText>>,
        Query<&mut Text, With<SectionText>>,
        Query<&mut Text, With<TimeElapsedText>>,
        Query<&mut Text, With<TimeTotalText>>,
        Query<&mut Text, With<NextTrackText>>,
    )>,
    mut fill_q: Query<
        &mut Node,
        (
            With<ProgressFill>,
            Without<ProgressPlayhead>,
            Without<MiniFill>,
        ),
    >,
    mut head_q: Query<&mut Node, (With<ProgressPlayhead>, Without<MiniFill>)>,
    mut mini_q: Query<
        &mut Node,
        (
            With<MiniFill>,
            Without<ProgressFill>,
            Without<ProgressPlayhead>,
        ),
    >,
) {
    let track = playback.track();
    if let Ok(mut t) = sets.p0().single_mut() {
        *t = Text::new(world_label(&theme, active_recipe.get()));
    }
    if let Ok(mut t) = sets.p1().single_mut() {
        *t = Text::new(track.title.clone());
    }
    if let Ok(mut t) = sets.p2().single_mut() {
        *t = Text::new(track.section.clone());
    }
    if let Ok(mut t) = sets.p3().single_mut() {
        *t = Text::new(fmt_time(playback.elapsed));
    }
    if let Ok(mut t) = sets.p4().single_mut() {
        *t = Text::new(fmt_time(track.duration));
    }
    if let Ok(mut t) = sets.p5().single_mut() {
        let next = playback.next_track();
        *t = Text::new(format!("{} — {}", next.title, next.artist));
    }

    let frac = playback.fraction() * 100.0;
    if let Ok(mut n) = fill_q.single_mut() {
        n.width = Val::Percent(frac);
    }
    if let Ok(mut n) = mini_q.single_mut() {
        n.width = Val::Percent(frac);
    }
    if let Ok(mut n) = head_q.single_mut() {
        n.left = Val::Percent(frac);
        // Beat pulse: grow the dot on the beat, capped when reduce-flashing is
        // on. Driven by node size, not `Transform` — UI layout owns a node's
        // Transform (B0004), so scaling it is a silent no-op. Keep it centred
        // vertically on the 4px bar and horizontally over `left`.
        let pulse = if comfort.reduce_flashing {
            0.0
        } else {
            beat.pulse
        };
        let size = 13.0 * (1.0 + pulse * 0.45);
        n.width = Val::Px(size);
        n.height = Val::Px(size);
        n.top = Val::Px(2.0 - size / 2.0);
        n.margin = UiRect::left(Val::Px(-size / 2.0));
    }
}

/// Reflect transport state onto the buttons: swap the play/pause glyphs by
/// visibility, and tint shuffle / repeat to accent when active (via `Fade.base`
/// so the alpha fade still applies). Runs on any transport or theme change.
#[allow(clippy::type_complexity)]
pub fn update_transport(
    playback: Res<Playback>,
    theme: Res<Theme>,
    mut vis: ParamSet<(
        Query<&mut Visibility, With<PlayIcon>>,
        Query<&mut Visibility, With<PauseIcon>>,
        Query<&mut Visibility, With<RepeatOneBadge>>,
    )>,
    mut shuffle_q: Query<
        &mut Fade,
        (
            With<ShuffleIcon>,
            Without<RepeatIcon>,
            Without<RepeatOneBadge>,
        ),
    >,
    mut repeat_q: Query<&mut Fade, (With<RepeatIcon>, Without<RepeatOneBadge>)>,
    mut badge_q: Query<&mut Fade, (With<RepeatOneBadge>, Without<RepeatIcon>)>,
) {
    if !playback.is_changed() && !theme.is_changed() {
        return;
    }
    let accent = theme.accent();
    let playing = playback.playing;

    if let Ok(mut v) = vis.p0().single_mut() {
        *v = if playing {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    if let Ok(mut v) = vis.p1().single_mut() {
        *v = if playing {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if let Ok(mut fade) = shuffle_q.single_mut() {
        fade.base = if playback.shuffle {
            accent
        } else {
            theme::text_muted()
        };
    }
    if let Ok(mut fade) = repeat_q.single_mut() {
        fade.base = if playback.repeat == Repeat::Off {
            theme::text_muted()
        } else {
            accent
        };
    }
    // The "1" badge shows only for repeat-one, tinted like the glyph.
    if let Ok(mut v) = vis.p2().single_mut() {
        *v = if playback.repeat == Repeat::One {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if let Ok(mut fade) = badge_q.single_mut() {
        fade.base = if playback.repeat == Repeat::One {
            accent
        } else {
            theme::text_muted()
        };
    }
}

/// Route transport-button presses to the transport — mirrors the keyboard
/// bindings in `input_in_world`. Play/pause freezes in place (no menu).
pub fn transport_clicks(
    q: Query<(&TransportButton, &Interaction), Changed<Interaction>>,
    mut playback: ResMut<Playback>,
    mut theme: ResMut<Theme>,
    mut seek: ResMut<crate::SeekRequest>,
    mut activity: ResMut<HudActivity>,
) {
    for (btn, interaction) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match btn.0 {
            Transport::Shuffle => playback.toggle_shuffle(),
            Transport::Prev => {
                // Restart if >3s in (the usual convention), else step back.
                if playback.elapsed > 3.0 {
                    seek.0 = Some(0.0);
                } else {
                    theme.mood = playback.previous();
                }
            }
            Transport::Back => seek.0 = Some((playback.elapsed - 15.0).max(0.0)),
            Transport::PlayPause => playback.playing = !playback.playing,
            Transport::Fwd => seek.0 = Some(playback.elapsed + 15.0),
            Transport::Next => match playback.advance() {
                Some(mood) => theme.mood = mood,
                None => playback.playing = false, // repeat-off: end of queue
            },
            Transport::Repeat => playback.cycle_repeat(),
        }
        activity.wake();
    }
}

/// Highlight the active camera-mode tab.
pub fn update_mode_tabs(
    mode: Res<CameraMode>,
    theme: Res<Theme>,
    mut q: Query<(&ModeTab, &mut Fade, &Children)>,
    mut text_q: Query<&mut Fade, (With<Text>, Without<ModeTab>)>,
) {
    if !mode.is_changed() && !theme.is_changed() {
        return;
    }
    let accent = theme.accent();
    for (tab, mut fade, children) in &mut q {
        let active = tab.0 == *mode;
        fade.base = if active {
            accent.with_alpha(0.18)
        } else {
            Color::NONE
        };
        for child in children {
            if let Ok(mut tfade) = text_q.get_mut(*child) {
                tfade.base = if active { TEXT } else { theme::text_muted() };
            }
        }
    }
}

/// Show the reticle only in Explore.
pub fn update_reticle(mode: Res<CameraMode>, mut q: Query<&mut Visibility, With<Reticle>>) {
    if !mode.is_changed() {
        return;
    }
    for mut v in &mut q {
        *v = if matches!(*mode, CameraMode::Explore) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

/// Clicking an Explore/Drift tab switches the camera feel (in addition to `F`).
pub fn mode_tab_clicks(
    mut mode: ResMut<CameraMode>,
    q: Query<(&ModeTab, &Interaction), Changed<Interaction>>,
) {
    for (tab, interaction) in &q {
        if *interaction == Interaction::Pressed {
            *mode = tab.0;
        }
    }
}

/// Clicking a corner chip toggles its panel (Library / Queue).
pub fn chip_clicks(
    mut stack: ResMut<crate::OverlayStack>,
    mut queue_open: ResMut<crate::QueueOpen>,
    q: Query<(&HudChip, &Interaction), Changed<Interaction>>,
) {
    for (chip, interaction) in &q {
        if *interaction == Interaction::Pressed {
            match chip.0 {
                ChipKind::Library => stack.toggle(crate::Overlay::Library),
                ChipKind::Queue => queue_open.0 = !queue_open.0,
            }
        }
    }
}

/// One section notch on the progress bar.
fn spawn_notch(parent: &mut ChildSpawnerCommands<'_>, fraction: f32) {
    parent.spawn((
        SectionNotch,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(fraction * 100.0),
            top: Val::Px(-3.0),
            width: Val::Px(2.0),
            height: Val::Px(10.0),
            ..default()
        },
        BackgroundColor(theme::hairline().with_alpha(0.4)),
        Fade {
            group: FadeGroup::Full,
            base: theme::hairline().with_alpha(0.4),
            target: FadeTarget::Bg,
        },
    ));
}

/// Respawn the section notches when the analyzed sections change (they were
/// previously spawned once at setup and went stale on every track change).
pub fn update_section_notches(
    playback: Res<Playback>,
    mut commands: Commands,
    bar_q: Query<Entity, With<ProgressBar>>,
    notch_q: Query<Entity, With<SectionNotch>>,
    mut last: Local<Option<Vec<f32>>>,
) {
    if last.as_ref() == Some(&playback.sections) {
        return;
    }
    *last = Some(playback.sections.clone());
    for e in &notch_q {
        commands.entity(e).despawn();
    }
    let Ok(bar) = bar_q.single() else { return };
    commands.entity(bar).with_children(|parent| {
        for f in &playback.sections {
            spawn_notch(parent, *f);
        }
    });
}

/// Click/drag on the seek strip scrubs the song. The bar is a fixed-width,
/// window-centred element, so its screen rect is a pure function of the window
/// width (see the layout constants). Only active while the HUD is Visible.
pub fn seek_strip_scrub(
    strip_q: Query<&Interaction, With<SeekStrip>>,
    window_q: Query<&Window, With<bevy::window::PrimaryWindow>>,
    playback: Res<Playback>,
    mut activity: ResMut<HudActivity>,
    mut seek: ResMut<crate::SeekRequest>,
    mut last_sent: Local<Option<f32>>,
) {
    let pressed = strip_q.iter().any(|i| *i == Interaction::Pressed);
    if !pressed {
        *last_sent = None;
        return;
    }
    // A mouse click isn't caught by the keyboard/motion wake path — wake here
    // so a press always brings the HUD back, but only actually seek when the
    // scrubber was already Visible (else the bar isn't on screen to aim at).
    let visible = activity.full >= 0.5;
    activity.wake();
    if !visible {
        *last_sent = None;
        return;
    }
    let Ok(window) = window_q.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    // The bar's left edge = window centre − half the cluster + the bar's offset
    // within the cluster.
    let bar_left = window.width() * 0.5 - CLUSTER_W * 0.5 + BAR_LEFT_IN_CLUSTER;
    let frac = ((cursor.x - bar_left) / BAR_W).clamp(0.0, 1.0);
    // Throttle: only send when the target moved meaningfully (drag-scrub would
    // otherwise issue a decoder seek every frame).
    if last_sent.is_some_and(|f| (f - frac).abs() < 0.005) {
        return;
    }
    *last_sent = Some(frac);
    seek.0 = Some(frac * playback.duration());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SeekRequest;
    use crate::playback::Repeat;

    /// Build a minimal app with the transport resources and one pressed
    /// button, run `transport_clicks` once, and hand back the world. This
    /// exercises the button→action wiring end-to-end without a real pointer.
    fn press(action: Transport) -> App {
        let mut app = App::new();
        app.insert_resource(Playback::default())
            .insert_resource(Theme::default())
            .insert_resource(SeekRequest::default())
            .insert_resource(HudActivity::default())
            .add_systems(Update, transport_clicks);
        app.world_mut()
            .spawn((TransportButton(action), Interaction::Pressed));
        app.update();
        app
    }

    #[test]
    fn play_pause_button_freezes_transport() {
        // Default is playing; a press stops it (and never touches the menu).
        let app = press(Transport::PlayPause);
        assert!(!app.world().resource::<Playback>().playing);
    }

    #[test]
    fn shuffle_button_toggles_shuffle() {
        let app = press(Transport::Shuffle);
        assert!(app.world().resource::<Playback>().shuffle);
    }

    #[test]
    fn repeat_button_cycles_from_all_to_one() {
        let app = press(Transport::Repeat);
        assert_eq!(app.world().resource::<Playback>().repeat, Repeat::One);
    }

    #[test]
    fn next_button_advances_the_queue() {
        let app = press(Transport::Next);
        assert_eq!(app.world().resource::<Playback>().current, 1);
    }

    #[test]
    fn forward_button_requests_a_plus_15s_seek() {
        // Default elapsed is 161s → +15 = 176s.
        let app = press(Transport::Fwd);
        let seek = app.world().resource::<SeekRequest>().0;
        assert!(seek.is_some_and(|s| (s - 176.0).abs() < 0.01));
    }

    #[test]
    fn back_button_requests_a_minus_15s_seek() {
        // Default elapsed is 161s → −15 = 146s.
        let app = press(Transport::Back);
        let seek = app.world().resource::<SeekRequest>().0;
        assert!(seek.is_some_and(|s| (s - 146.0).abs() < 0.01));
    }

    #[test]
    fn previous_button_restarts_when_past_3s() {
        // Default elapsed is 161s (>3s), so previous seeks to 0 rather than
        // stepping back a track.
        let app = press(Transport::Prev);
        assert_eq!(app.world().resource::<Playback>().current, 0);
        assert_eq!(app.world().resource::<SeekRequest>().0, Some(0.0));
    }
}
