//! The review bar on Bevy's own widgets.
//!
//! With `--review`, an agent's batch shows on the canvas before it
//! commits, and this bar at the top of the window says who proposes what
//! and takes the decision — Accept, Reject, or Enter and Esc. Without the
//! `feathers` feature the egui bar in `live.rs` (`review_bar`) does the
//! same; `GET /proposals` reports the buttons' places from the widget
//! seam here (`review.accept`, `review.reject`) and from egui's
//! rectangles there, under one key.
//!
//! The first surface written as a scene (`bsn!`): feathers' bundle
//! constructors are deprecated, and the egui evaluation's amendment was to
//! start the next surface on scenes.

use bevy::feathers::constants::fonts;
use bevy::feathers::controls::{ButtonVariant, FeathersButton};
use bevy::feathers::font_styles::InheritableFont;
use bevy::feathers::theme::{InheritableThemeTextColor, ThemedText};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;

use super::live::{Live, ReviewDecision};
use crate::ui::widgets::Widget;

const PANEL: Color = Color::srgba(0.12, 0.13, 0.17, 0.97);
const AMBER: Color = Color::srgb(0.90, 0.71, 0.35);

pub struct ReviewUiPlugin;

impl Plugin for ReviewUiPlugin {
    fn build(&self, app: &mut App) {
        // After the world side has taken the batch: the bar is built in the
        // frame the proposal lands, and its buttons are in the seam before
        // the proposer's next request is answered.
        app.add_systems(Startup, spawn_bar).add_systems(
            Update,
            (show_proposal.after(super::live::live_world), review_keys),
        );
    }
}

/// The bar's root.
#[derive(Component)]
struct ReviewBar;

fn spawn_bar(mut commands: Commands, assets: Res<AssetServer>) {
    let font: Handle<Font> = assets.load(fonts::REGULAR);
    commands.spawn((
        ReviewBar,
        Name::new("Review bar"),
        Widget::new("panel.review"),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            min_height: Val::Px(30.0),
            display: Display::None,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::Wrap,
            align_items: AlignItems::Center,
            column_gap: Val::Px(10.0),
            row_gap: Val::Px(4.0),
            padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(PANEL),
        ZIndex(50),
        InheritableThemeTextColor(tokens::TEXT_MAIN),
        InheritableFont {
            font,
            font_size: FontSize::Px(13.0),
            weight: FontWeight::NORMAL,
        },
    ));
}

/// The bar follows the proposal on the canvas: shown while one is pending,
/// rebuilt when another arrives (by its number), hidden otherwise.
fn show_proposal(
    live: Res<Live>,
    mut bar: Query<(Entity, &mut Node), With<ReviewBar>>,
    mut shown: Local<Option<u64>>,
    mut commands: Commands,
) {
    let Ok((root, mut node)) = bar.single_mut() else {
        return;
    };
    let Some(proposal) = live.proposal() else {
        if node.display != Display::None {
            node.display = Display::None;
        }
        *shown = None;
        return;
    };
    if node.display != Display::Flex {
        node.display = Display::Flex;
    }
    if *shown == Some(proposal.n) {
        return;
    }
    *shown = Some(proposal.n);
    let author = format!("{} proposes:", proposal.author);
    let changes = proposal.changes.join(", ");
    let mut parts: Vec<Box<dyn SceneList>> = vec![
        Box::new(bsn_list![(Text({author}) ThemedText)]),
        Box::new(bsn_list![(Text({changes}) ThemedText)]),
    ];
    for warning in &proposal.warnings {
        let warning = format!("! {warning}");
        parts.push(Box::new(bsn_list![(Text({warning}) TextColor(AMBER))]));
    }
    parts.push(Box::new(bsn_list![
        decision_button("Accept (Enter)", "review.accept", true),
        decision_button("Reject (Esc)", "review.reject", false),
    ]));
    commands.entity(root).despawn_children();
    commands
        .entity(root)
        .queue_spawn_related_scenes::<Children>(parts);
}

/// Accept or Reject: a feathers button whose activation is the decision.
fn decision_button(caption: &'static str, widget: &'static str, accept: bool) -> impl Scene {
    let variant = if accept {
        ButtonVariant::Primary
    } else {
        ButtonVariant::Normal
    };
    bsn! {
        @FeathersButton {
            @caption: {Box::new(bsn_list![(Text(caption) ThemedText)]) as Box<dyn SceneList>},
            @variant: {variant},
        }
        Widget({widget.to_string()})
        on(move |_: On<Activate>, mut decisions: MessageWriter<ReviewDecision>| {
            decisions.write(ReviewDecision(accept));
        })
    }
}

/// Enter accepts, Esc rejects, while a proposal is pending.
fn review_keys(
    live: Res<Live>,
    keys: Res<ButtonInput<KeyCode>>,
    mut decisions: MessageWriter<ReviewDecision>,
) {
    if live.proposal().is_none() {
        return;
    }
    if keys.just_pressed(KeyCode::Enter) {
        decisions.write(ReviewDecision(true));
    }
    if keys.just_pressed(KeyCode::Escape) {
        decisions.write(ReviewDecision(false));
    }
}
