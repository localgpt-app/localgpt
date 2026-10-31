//! The world-in-the-page measurement
//! (`docs/rfcs/authoring/the-world-in-the-page.md`): what it takes to measure
//! the world drawn by Bevy against the world drawn in the page. Nothing here
//! runs unless its variable is set; `scripts/world-in-the-page/run.sh` sets
//! them.
//!
//! - `LOCALGPT_SPIKE_FRAMES=<file>`: every Bevy frame as `unix_ms,dt_ms`,
//!   and the window's occlusion changes as `# occluded <bool> <unix_ms>`.
//! - `LOCALGPT_SPIKE_WEB_WORLD=1`: the world camera draws nothing; the page
//!   draws the world instead.
//! - `LOCALGPT_SPIKE_MANIFEST=1`: a song's world is also written as
//!   `manifest.json`, the JSON the format's web viewer reads.
//! - `LOCALGPT_SPIKE_MONITOR=fastest`: the window moves to the display with
//!   the highest refresh rate — a ProMotion screen beside a 60 Hz one.
//! - `LOCALGPT_SPIKE_PLAY=<s>` and `LOCALGPT_SPIKE_EXIT=<s>`: for a run with
//!   no page (a platform with no web view yet): `<s>` seconds in, mute the
//!   song and play it, as the page would; and quit `<s>` seconds in. The
//!   frame log marks both (`# play`, `# exit`), and names the displays and
//!   the window's size first (`# monitor`, `# window`).
//! - `LOCALGPT_SPIKE_CASCADES=<n>`: the directional lights' shadows in `n`
//!   cascades, where Bevy's default is four; three.js draws one map.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::window::WindowOccluded;
use localgpt_gen::gen3d::plugin::FlyCam;
use localgpt_world_types as wt;

use crate::timeline::TimelineAsk;

pub struct SpikePlugin;

#[derive(Resource)]
struct FrameLog {
    out: BufWriter<File>,
    frames: u64,
    displays_named: bool,
}

/// Seconds since start, from a variable.
fn secs_var(name: &str) -> Option<f64> {
    std::env::var(name).ok()?.parse().ok()
}

#[derive(Resource)]
struct PlayAt(f64);

#[derive(Resource)]
struct ExitAt(f64);

impl Plugin for SpikePlugin {
    fn build(&self, app: &mut App) {
        if let Ok(path) = std::env::var("LOCALGPT_SPIKE_FRAMES") {
            match File::create(&path) {
                Ok(file) => {
                    app.insert_resource(FrameLog {
                        out: BufWriter::new(file),
                        frames: 0,
                        displays_named: false,
                    })
                    .add_systems(Last, (name_displays, log_frame).chain());
                }
                Err(e) => eprintln!("spike: {path}: {e}"),
            }
        }
        if let Some(at) = secs_var("LOCALGPT_SPIKE_PLAY") {
            app.insert_resource(PlayAt(at)).add_systems(Update, play_at);
        }
        if let Some(at) = secs_var("LOCALGPT_SPIKE_EXIT") {
            app.insert_resource(ExitAt(at)).add_systems(Update, exit_at);
        }
        if std::env::var("LOCALGPT_SPIKE_WEB_WORLD").is_ok_and(|v| v == "1") {
            // With no camera presenting, nothing waits on vsync and the loop
            // would spin a core: tick at the display's 120 Hz instead, so
            // the app's own systems cost what they cost when Bevy draws.
            let tick = bevy::winit::UpdateMode::reactive(std::time::Duration::from_micros(8_333));
            app.insert_resource(bevy::winit::WinitSettings {
                focused_mode: tick,
                unfocused_mode: tick,
            })
            .add_systems(Last, world_camera_off);
        }
        if std::env::var("LOCALGPT_SPIKE_MONITOR").is_ok_and(|v| v == "fastest") {
            app.add_systems(Update, to_fastest_monitor);
        }
        if let Some(n) = secs_var("LOCALGPT_SPIKE_CASCADES") {
            app.insert_resource(Cascades(n.max(1.0) as usize))
                .add_systems(Update, cascades);
        }
    }
}

/// `LOCALGPT_SPIKE_CASCADES=<n>`: each directional light's shadow in `n`
/// cascades over 200 m, where Bevy's default is four over 1 km — three.js
/// draws the format's one shadow-casting light into a single map.
#[derive(Resource)]
struct Cascades(usize);

fn cascades(
    n: Res<Cascades>,
    lights: Query<Entity, (With<DirectionalLight>, Without<CascadesSet>)>,
    mut commands: Commands,
) {
    for light in &lights {
        let config = bevy::light::CascadeShadowConfigBuilder {
            num_cascades: n.0,
            maximum_distance: 200.0,
            first_cascade_far_bound: if n.0 == 1 { 200.0 } else { 10.0 },
            ..default()
        }
        .build();
        // A light the world's load replaces later this frame is gone by the
        // time commands apply: `try_insert` lets that one be.
        commands.entity(light).try_insert((config, CascadesSet));
    }
}

#[derive(Component)]
struct CascadesSet;

/// Once the monitors are known: centre the window on the fastest one.
fn to_fastest_monitor(
    monitors: Query<(Entity, &bevy::window::Monitor)>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some((entity, monitor)) = monitors
        .iter()
        .max_by_key(|(_, m)| m.refresh_rate_millihertz.unwrap_or(0))
    else {
        return;
    };
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    window.position =
        bevy::window::WindowPosition::Centered(bevy::window::MonitorSelection::Entity(entity));
    eprintln!(
        "spike: the window moves to {} ({} mHz)",
        monitor.name.as_deref().unwrap_or("a display"),
        monitor.refresh_rate_millihertz.unwrap_or(0)
    );
    *done = true;
}

fn unix_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

/// Once the monitors are known: each display's name, size and refresh rate,
/// and the window's size in pixels, as comments at the top of the log.
fn name_displays(
    monitors: Query<&bevy::window::Monitor>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut log: ResMut<FrameLog>,
) {
    if log.displays_named || monitors.is_empty() {
        return;
    }
    for m in &monitors {
        let _ = writeln!(
            log.out,
            "# monitor {} {}x{} {} mHz",
            m.name.as_deref().unwrap_or("?").replace(' ', "_"),
            m.physical_width,
            m.physical_height,
            m.refresh_rate_millihertz.unwrap_or(0)
        );
    }
    if let Ok(w) = windows.single() {
        let _ = writeln!(
            log.out,
            "# window {}x{} scale {}",
            w.physical_width(),
            w.physical_height(),
            w.scale_factor()
        );
    }
    log.displays_named = true;
}

/// The page's two asks, made here when there is no page: mute the song and
/// play it.
fn play_at(
    at: Res<PlayAt>,
    time: Res<Time<Real>>,
    asks: Option<ResMut<Messages<TimelineAsk>>>,
    log: Option<ResMut<FrameLog>>,
    mut done: Local<bool>,
) {
    if *done || time.elapsed_secs_f64() < at.0 {
        return;
    }
    let Some(mut asks) = asks else {
        return;
    };
    asks.write(TimelineAsk::Mute(vec!["soundtrack".into()]));
    asks.write(TimelineAsk::Play(true));
    if let Some(mut log) = log {
        let _ = writeln!(log.out, "# play {:.0}", unix_ms());
    }
    *done = true;
}

fn exit_at(
    at: Res<ExitAt>,
    time: Res<Time<Real>>,
    log: Option<ResMut<FrameLog>>,
    mut exit: MessageWriter<AppExit>,
    mut done: Local<bool>,
) {
    if *done || time.elapsed_secs_f64() < at.0 {
        return;
    }
    if let Some(mut log) = log {
        let _ = writeln!(log.out, "# exit {:.0}", unix_ms());
        let _ = log.out.flush();
    }
    exit.write(AppExit::Success);
    *done = true;
}

fn log_frame(
    time: Res<Time<Real>>,
    mut occluded: MessageReader<WindowOccluded>,
    mut log: ResMut<FrameLog>,
) {
    let now = unix_ms();
    for e in occluded.read() {
        let _ = writeln!(log.out, "# occluded {} {now:.0}", e.occluded);
    }
    let _ = writeln!(log.out, "{now:.3},{:.3}", time.delta_secs_f64() * 1000.0);
    log.frames += 1;
    if log.frames.is_multiple_of(30) {
        let _ = log.out.flush();
    }
}

fn world_camera_off(mut cameras: Query<&mut Camera, With<FlyCam>>) {
    for mut camera in &mut cameras {
        if camera.is_active {
            camera.is_active = false;
        }
    }
}

/// The song's world as the web viewer reads it, beside its `world.ron`.
pub fn write_manifest(dir: &Path, world: &wt::WorldManifest) {
    if std::env::var_os("LOCALGPT_SPIKE_MANIFEST").is_none() {
        return;
    }
    let written = serde_json::to_string(world)
        .map_err(anyhow::Error::from)
        .and_then(|json| Ok(std::fs::write(dir.join("manifest.json"), json)?));
    if let Err(e) = written {
        eprintln!("spike: manifest.json: {e}");
    }
}
