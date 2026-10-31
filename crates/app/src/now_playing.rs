//! The song a world performs, for the page's transport: title and artist,
//! where the song is, and whether it plays. There whenever the loaded world
//! has a soundtrack — a song opened with `--song`, or any world that
//! carries one. The page pauses and plays it with [`ToggleSong`].

use bevy::prelude::*;
use localgpt_gen::gen3d::audio::AudioEngine;
use localgpt_world_bevy::modulation::Soundtrack;

/// The transport's own state: whether the listener paused the song, and
/// where it was last heard (the engine reports no position while paused).
#[derive(Resource, Default)]
pub struct Transport {
    paused: bool,
    last_position: f32,
}

/// Pause the song, or play it again.
#[derive(Message, Debug, Clone, Copy)]
pub struct ToggleSong;

/// What the page's transport shows.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct NowPlaying {
    pub title: String,
    pub artist: Option<String>,
    /// Seconds.
    pub duration: f32,
    /// Seconds, where the song was last heard.
    pub position: f32,
    pub playing: bool,
    /// The world ships its audio and the engine is up: there is something
    /// to pause.
    pub playable: bool,
}

pub struct NowPlayingPlugin;

impl Plugin for NowPlayingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Transport>()
            .add_message::<ToggleSong>()
            .add_systems(Update, (follow_song, toggle).chain());
    }
}

fn follow_song(soundtrack: Res<Soundtrack>, mut transport: ResMut<Transport>) {
    if let Some(position) = soundtrack.position
        && transport.last_position != position
    {
        transport.last_position = position;
    }
}

fn toggle(
    mut asks: MessageReader<ToggleSong>,
    soundtrack: Res<Soundtrack>,
    audio: Option<ResMut<AudioEngine>>,
    mut transport: ResMut<Transport>,
) {
    // Two asks in one frame are a play and a pause: nothing changes.
    if asks.read().count().is_multiple_of(2) {
        return;
    }
    let ships_audio = soundtrack
        .def
        .as_ref()
        .is_some_and(|song| song.path.is_some());
    if ships_audio && let Some(mut audio) = audio {
        transport.paused = !transport.paused;
        audio.pause_soundtrack(transport.paused);
    }
}

/// The transport's snapshot, while the world has a soundtrack.
pub fn now_playing(
    soundtrack: &Soundtrack,
    transport: &Transport,
    audio: bool,
) -> Option<NowPlaying> {
    let song = soundtrack.def.as_ref()?;
    let playable = song.path.is_some() && audio;
    Some(NowPlaying {
        title: song.title.clone().unwrap_or_else(|| "Untitled".into()),
        artist: song.artist.clone(),
        duration: song.duration,
        position: transport.last_position.clamp(0.0, song.duration.max(0.0)),
        playing: playable && !transport.paused,
        playable,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use localgpt_world_types as wt;

    fn song(path: Option<&str>) -> Soundtrack {
        Soundtrack {
            def: Some(wt::SoundtrackDef {
                path: path.map(str::to_string),
                title: Some("Tidewater".into()),
                artist: None,
                license: None,
                duration: 192.0,
                bpm: 92.0,
                beat_offset: 0.4,
                sections: vec![0.0, 0.5],
                energy: Vec::new(),
                stems: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_world_without_its_audio_has_nothing_to_pause() {
        let transport = Transport::default();
        let shown = now_playing(&song(None), &transport, true).expect("a soundtrack");
        assert!(!shown.playable && !shown.playing);
        let shown = now_playing(&song(Some("music/tide.mp3")), &transport, true).unwrap();
        assert!(shown.playable && shown.playing);
        // No audio engine (a machine without an output device): nothing plays.
        let shown = now_playing(&song(Some("music/tide.mp3")), &transport, false).unwrap();
        assert!(!shown.playable);
        assert!(now_playing(&Soundtrack::default(), &transport, true).is_none());
    }

    #[test]
    fn the_position_stays_on_the_song() {
        let transport = Transport {
            paused: true,
            last_position: 400.0,
        };
        let shown = now_playing(&song(Some("music/tide.mp3")), &transport, true).unwrap();
        assert_eq!(shown.position, 192.0);
        assert!(!shown.playing, "paused");
    }
}
