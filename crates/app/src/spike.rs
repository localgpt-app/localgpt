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

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::window::WindowOccluded;
use localgpt_gen::gen3d::plugin::FlyCam;
use localgpt_world_types as wt;

pub struct SpikePlugin;

#[derive(Resource)]
struct FrameLog {
    out: BufWriter<File>,
    frames: u64,
}

impl Plugin for SpikePlugin {
    fn build(&self, app: &mut App) {
        if let Ok(path) = std::env::var("LOCALGPT_SPIKE_FRAMES") {
            match File::create(&path) {
                Ok(file) => {
                    app.insert_resource(FrameLog {
                        out: BufWriter::new(file),
                        frames: 0,
                    })
                    .add_systems(Last, log_frame);
                }
                Err(e) => eprintln!("spike: {path}: {e}"),
            }
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
    }
}

fn unix_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
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
