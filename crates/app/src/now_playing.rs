//! The song a world performs, as a small transport at the bottom of the
//! window: title and artist, where the song is, and play/pause. Shown
//! whenever the loaded world has a soundtrack — a song opened with `--song`,
//! or any world that carries one.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use localgpt_gen::gen3d::audio::AudioEngine;
use localgpt_world_bevy::modulation::Soundtrack;

/// The transport's own state: whether the listener paused the song, and
/// where it was last heard (the engine reports no position while paused).
#[derive(Resource, Default)]
struct Transport {
    paused: bool,
    last_position: f32,
}

pub struct NowPlayingPlugin;

impl Plugin for NowPlayingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Transport>()
            .add_systems(EguiPrimaryContextPass, now_playing);
    }
}

fn now_playing(
    mut contexts: EguiContexts,
    soundtrack: Res<Soundtrack>,
    audio: Option<ResMut<AudioEngine>>,
    mut transport: ResMut<Transport>,
) {
    let Some(song) = soundtrack.def.as_ref() else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if let Some(position) = soundtrack.position {
        transport.last_position = position;
    }
    let playable = song.path.is_some() && audio.is_some();
    let mut toggle = false;
    egui::Area::new(egui::Id::new("now_playing"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -16.0])
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    if playable {
                        let label = if transport.paused { "Play" } else { "Pause" };
                        toggle = ui.button(label).clicked();
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new(song.title.as_deref().unwrap_or("Untitled"))
                                .strong(),
                        );
                        if let Some(artist) = &song.artist {
                            ui.label(
                                egui::RichText::new(artist)
                                    .small()
                                    .color(ui.visuals().weak_text_color()),
                            );
                        }
                    });
                    if song.duration > 0.0 {
                        let position = transport.last_position.min(song.duration);
                        ui.add(
                            egui::ProgressBar::new(position / song.duration)
                                .desired_width(200.0)
                                .text(format!("{} / {}", clock(position), clock(song.duration))),
                        );
                    }
                    if !playable {
                        ui.label(
                            egui::RichText::new("no audio with this world")
                                .small()
                                .color(ui.visuals().weak_text_color()),
                        );
                    }
                });
            });
        });
    if toggle && let Some(mut audio) = audio {
        transport.paused = !transport.paused;
        audio.pause_soundtrack(transport.paused);
    }
}

/// `m:ss`.
fn clock(seconds: f32) -> String {
    let whole = seconds.max(0.0) as u32;
    format!("{}:{:02}", whole / 60, whole % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_reads_minutes_and_seconds() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(59.9), "0:59");
        assert_eq!(clock(61.0), "1:01");
        assert_eq!(clock(754.2), "12:34");
        assert_eq!(clock(-3.0), "0:00");
    }
}
