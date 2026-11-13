//! Walking the document: ←/→ (also Space, PageUp/PageDown, Home/End) fly the
//! camera between tour waypoints — one per section — and a caption shows
//! where you are. The stop survives rebuilds, so editing the section you're
//! looking at keeps you there.

use bevy::prelude::*;
use localgpt_world_types as wt;

use crate::scene::{CurrentWorld, TourCamera};

pub struct TourPlugin;

impl Plugin for TourPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TourStop>()
            .add_systems(Startup, spawn_caption)
            .add_systems(Update, (navigate, fly, update_caption).chain());
    }
}

/// Index of the waypoint the camera is heading to.
#[derive(Resource, Default)]
struct TourStop(usize);

#[derive(Component)]
enum Caption {
    Title,
    Heading,
    Excerpt,
}

fn waypoints(world: &CurrentWorld) -> &[wt::TourWaypoint] {
    match world.manifest.tours.first() {
        Some(tour) => &tour.waypoints,
        None => &[],
    }
}

fn navigate(keys: Res<ButtonInput<KeyCode>>, world: Res<CurrentWorld>, mut stop: ResMut<TourStop>) {
    let last = waypoints(&world).len().saturating_sub(1);
    let mut next = stop.0.min(last);
    if keys.any_just_pressed([
        KeyCode::ArrowRight,
        KeyCode::ArrowDown,
        KeyCode::Space,
        KeyCode::PageDown,
    ]) {
        next = (next + 1).min(last);
    }
    if keys.any_just_pressed([KeyCode::ArrowLeft, KeyCode::ArrowUp, KeyCode::PageUp]) {
        next = next.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::Home) {
        next = 0;
    }
    if keys.just_pressed(KeyCode::End) {
        next = last;
    }
    // Only write on a real move, so the caption isn't redrawn every frame.
    if next != stop.0 {
        stop.0 = next;
    }
}

fn fly(
    time: Res<Time>,
    world: Res<CurrentWorld>,
    stop: Res<TourStop>,
    mut camera: Query<&mut Transform, With<TourCamera>>,
    mut placed: Local<bool>,
) {
    let Some(waypoint) = waypoints(&world).get(stop.0) else {
        return;
    };
    let Ok(mut transform) = camera.single_mut() else {
        return;
    };
    let target = Transform::from_translation(Vec3::from_array(waypoint.position))
        .looking_at(Vec3::from_array(waypoint.look_at), Vec3::Y);
    if !*placed {
        *transform = target;
        *placed = true;
        return;
    }
    // Exponential ease: frame-rate independent, settles in about a second.
    let k = 1.0 - (-3.0 * time.delta_secs()).exp();
    transform.translation = transform.translation.lerp(target.translation, k);
    transform.rotation = transform.rotation.slerp(target.rotation, k);
}

fn spawn_caption(mut commands: Commands) {
    commands.spawn((
        Caption::Title,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(22.0),
            left: Val::Px(28.0),
            ..default()
        },
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(24.0),
                left: Val::Px(24.0),
                // A definite width, not just `max_width`: otherwise the panel's
                // height is measured before the excerpt wraps and text spills out.
                width: Val::Px(620.0),
                max_width: Val::Vw(90.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(14.0)),
                border_radius: BorderRadius::all(Val::Px(10.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.06, 0.45)),
        ))
        .with_children(|panel| {
            panel.spawn((
                Caption::Heading,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(26.0),
                    ..default()
                },
                TextColor(Color::WHITE),
            ));
            panel.spawn((
                Caption::Excerpt,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.8)),
            ));
            panel.spawn((
                Text::new("←/→ walk   ·   save the file to rebuild"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.5)),
            ));
        });
}

fn update_caption(
    world: Res<CurrentWorld>,
    stop: Res<TourStop>,
    mut captions: Query<(&Caption, &mut Text)>,
) {
    if !world.is_changed() && !stop.is_changed() {
        return;
    }
    let stops = waypoints(&world);
    for (caption, mut text) in &mut captions {
        text.0 = match caption {
            Caption::Title => world.doc.title.clone(),
            Caption::Heading => match stops.get(stop.0) {
                Some(waypoint) => format!(
                    "{} / {}   {}",
                    stop.0 + 1,
                    stops.len(),
                    waypoint.description.as_deref().unwrap_or_default()
                ),
                None => "Nothing here yet: add a ## heading".into(),
            },
            Caption::Excerpt => world
                .doc
                .sections
                .get(stop.0)
                .map(|section| excerpt(&section.body))
                .unwrap_or_default(),
        };
    }
}

/// The start of a section's prose, cut at a word boundary.
fn excerpt(body: &str) -> String {
    const MAX_CHARS: usize = 180;
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_CHARS {
        return flat;
    }
    let cut: String = flat.chars().take(MAX_CHARS).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{cut}…")
}
