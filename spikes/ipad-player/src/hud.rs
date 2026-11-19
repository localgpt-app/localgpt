//! What the spike reads out, sized for an iPad: frame rate, the device's
//! heat and battery, the song, the first draw's steps. Every ten seconds the
//! same as a log line and a row of `Documents/player-metrics.csv`, which the
//! Files app shows and `devicectl device copy from` fetches. Three buttons a
//! finger can hit: the song, the look, the view.
//!
//! The buttons are nodes with an `Interaction`, which Bevy 0.20 deprecates
//! for picking's `Hovered` and the UI's `Pressed` but still updates from
//! touches; it needs no picking backend, which is all a spike asks of it.
#![allow(deprecated)]

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::camera::Orbit;
use crate::player::{Audio, Look, Package, Pipelines, Timeline};

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Frames>()
            .add_systems(Startup, spawn)
            .add_systems(Update, (count, readout, press));
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Line {
    Title,
    Frames,
    Device,
    Song,
    Draw,
    Life,
}

#[derive(Component, Clone, Copy)]
enum Action {
    PlayPause,
    Look,
    Home,
}

/// The text inside an [`Action`]'s button.
#[derive(Component, Clone, Copy)]
struct Label(Action);

/// Frame times since the last readout, and when the log last wrote.
#[derive(Resource, Default)]
struct Frames {
    times: Vec<f32>,
    since: f32,
    last_log: f32,
}

const PANEL: Color = Color::srgba(0.04, 0.05, 0.08, 0.62);
const INK: Color = Color::srgb(0.93, 0.94, 0.96);
const BUTTON: Color = Color::srgba(0.12, 0.14, 0.2, 0.78);
const PRESSED: Color = Color::srgba(0.3, 0.42, 0.75, 0.9);
/// Clear of the rounded corners and the home indicator, in points.
const INSET: f32 = 28.0;

fn spawn(mut commands: Commands, package: Res<Package>) {
    // ASCII only: the default font has no dash.
    let title = match &package.manifest.soundtrack {
        Some(song) => format!(
            "{} by {}",
            song.title.as_deref().unwrap_or("Untitled"),
            song.artist.as_deref().unwrap_or("unknown")
        ),
        None => package.manifest.meta.name.clone(),
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(INSET),
                left: Val::Px(INSET),
                padding: UiRect::axes(Val::Px(16.0), Val::Px(12.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            },
            BackgroundColor(PANEL),
        ))
        .with_children(|panel| {
            let lines = [
                (Line::Title, title, 22.0),
                (Line::Frames, String::new(), 17.0),
                (Line::Device, String::new(), 17.0),
                (Line::Song, String::new(), 17.0),
                (Line::Draw, String::new(), 17.0),
                (Line::Life, String::new(), 17.0),
            ];
            for (line, text, size) in lines {
                panel.spawn((
                    line,
                    Text::new(text),
                    TextFont {
                        font_size: FontSize::Px(size),
                        ..default()
                    },
                    TextColor(INK),
                ));
            }
        });
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(INSET + 8.0),
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(16.0),
            ..default()
        })
        .with_children(|row| {
            for action in [Action::PlayPause, Action::Look, Action::Home] {
                row.spawn((
                    action,
                    Interaction::default(),
                    Node {
                        min_width: Val::Px(140.0),
                        height: Val::Px(56.0),
                        padding: UiRect::horizontal(Val::Px(20.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(BUTTON),
                ))
                .with_child((
                    Label(action),
                    Text::new(""),
                    TextFont {
                        font_size: FontSize::Px(20.0),
                        ..default()
                    },
                    TextColor(INK),
                ));
            }
        });
}

fn count(time: Res<Time>, mut frames: ResMut<Frames>) {
    let dt = time.delta_secs();
    frames.times.push(dt);
    frames.since += dt;
}

/// Twice a second the readout; every ten seconds the log and the CSV. The
/// `Audio` parameter keeps this on the main thread, where UIKit's device is.
#[allow(clippy::too_many_arguments)]
fn readout(
    mut frames: ResMut<Frames>,
    timeline: Res<Timeline>,
    audio: NonSend<Audio>,
    look: Res<Look>,
    pipelines: Res<Pipelines>,
    window: Query<&Window, With<PrimaryWindow>>,
    mut lines: Query<(&Line, &mut Text)>,
    mut labels: Query<(&Label, &mut Text), Without<Line>>,
) {
    if frames.since < 0.5 {
        return;
    }
    let fps = frames.times.len() as f32 / frames.since;
    let worst_ms = frames.times.iter().copied().fold(0.0, f32::max) * 1000.0;
    frames.times.clear();
    frames.since = 0.0;

    let t = timeline.launched.elapsed().as_secs_f32();
    let thermal = crate::ios::thermal();
    let battery = crate::ios::battery();
    let (width, height, scale) = window
        .single()
        .map(|w| (w.physical_width(), w.physical_height(), w.scale_factor()))
        .unwrap_or((0, 0, 0.0));
    let song = audio.engine.as_ref().and_then(|e| e.soundtrack_position());
    let seconds = |s: Option<f32>| s.map_or("-".to_string(), |s| format!("{s:.1}s"));

    let look = *look;
    let frames_line = format!(
        "{fps:.1} fps, worst frame {worst_ms:.1} ms, {width}x{height} at {scale}x, {look:?}"
    );
    let battery_text = battery.map_or("battery unknown".to_string(), |(percent, state)| {
        format!("battery {percent:.0}% {state}")
    });
    let device_line = format!("thermal {thermal}, {battery_text}");
    let song_line = match (&audio.song, song) {
        (Ok(length), Some(at)) => format!(
            "song {at:.1}s of {length:.1}s{}, session {}",
            if audio.paused { " (paused)" } else { "" },
            match &audio.session {
                Ok(()) => "playback".to_string(),
                Err(e) => format!("failed: {e}"),
            }
        ),
        (Err(e), _) => format!("song failed: {e}"),
        (Ok(_), None) => "song not playing".to_string(),
    };
    let draw_line = format!(
        "first frame {}, glTF loaded {}, pipelines ready {} ({} of {} waiting)",
        seconds(timeline.first_frame),
        seconds(timeline.assets_loaded),
        seconds(timeline.pipelines_ready),
        pipelines.waiting(),
        pipelines.total(),
    );
    let life_line = format!(
        "life cycle {}, {} suspensions",
        timeline
            .lifecycle
            .map_or("-".to_string(), |state| format!("{state:?}")),
        timeline.suspensions,
    );
    for (line, mut text) in &mut lines {
        match line {
            Line::Title => {}
            Line::Frames => text.0.clone_from(&frames_line),
            Line::Device => text.0.clone_from(&device_line),
            Line::Song => text.0.clone_from(&song_line),
            Line::Draw => text.0.clone_from(&draw_line),
            Line::Life => text.0.clone_from(&life_line),
        }
    }
    for (label, mut text) in &mut labels {
        text.0 = match label.0 {
            Action::PlayPause if audio.paused => "Play",
            Action::PlayPause => "Pause",
            Action::Look if look == Look::Full => "Lite",
            Action::Look => "Full",
            Action::Home => "Reset view",
        }
        .to_string();
    }

    if t - frames.last_log < 10.0 {
        return;
    }
    frames.last_log = t;
    eprintln!(
        "player: {t:.0}s | {frames_line} | {device_line} | {song_line} | {draw_line} | {life_line}"
    );
    let row = format!(
        "{t:.1},{fps:.2},{worst_ms:.2},{thermal},{},{},{look:?},{},{},{width},{height}\n",
        battery.map_or(String::new(), |(percent, _)| format!("{percent:.0}")),
        battery.map_or("", |(_, state)| state),
        song.map_or(String::new(), |s| format!("{s:.2}")),
        pipelines.waiting(),
    );
    if let Some(path) = metrics_file() {
        let new = !path.exists();
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            if new {
                let _ = file.write_all(
                    b"seconds,fps,worst_ms,thermal,battery_percent,battery_state,look,song_seconds,pipelines_waiting,width,height\n",
                );
            }
            let _ = file.write_all(row.as_bytes());
        }
    }
}

/// `Documents/` in the app's container, which `HOME` names on iOS.
fn metrics_file() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let documents = PathBuf::from(home).join("Documents");
    std::fs::create_dir_all(&documents).ok()?;
    Some(documents.join("player-metrics.csv"))
}

fn press(
    mut buttons: Query<(&Interaction, &Action, &mut BackgroundColor), Changed<Interaction>>,
    mut audio: NonSendMut<Audio>,
    mut look: ResMut<Look>,
    mut orbits: Query<&mut Orbit>,
) {
    for (interaction, action, mut colour) in &mut buttons {
        if *interaction != Interaction::Pressed {
            colour.0 = BUTTON;
            continue;
        }
        colour.0 = PRESSED;
        match action {
            Action::PlayPause => {
                let paused = !audio.paused;
                if let Some(engine) = audio.engine.as_mut() {
                    engine.pause_soundtrack(paused);
                }
                audio.paused = paused;
            }
            Action::Look => {
                *look = match *look {
                    Look::Full => Look::Lite,
                    Look::Lite => Look::Full,
                };
            }
            Action::Home => {
                for mut orbit in &mut orbits {
                    orbit.home();
                }
            }
        }
    }
}
