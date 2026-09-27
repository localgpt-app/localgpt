//! Playback + beat state.
//!
//! There is no audio engine yet — this module *simulates* the signals the real
//! MIR pipeline (see `idea.md`) will eventually provide: a transport clock, a
//! beat phase, and a slow energy envelope. The UI reads these resources so the
//! HUD is already wired to react to music.

use std::path::PathBuf;

use bevy::prelude::*;

/// One track in the queue.
#[derive(Clone)]
pub struct Track {
    pub title: String,
    pub artist: String,
    /// Album tag, when present — the Library browser's ALBUM column (spec 1i).
    pub album: Option<String>,
    /// Duration in seconds.
    pub duration: f32,
    /// Index into [`crate::theme::moods()`] — the world this song imagines.
    pub mood: usize,
    /// Section label shown under the title, e.g. "Cascade Hour · Slow Light".
    pub section: String,
    /// Audio file on disk. `None` for the built-in demo tracks — those play
    /// silently on the simulated clock.
    pub path: Option<PathBuf>,
    /// Stable identity: the blake3 content hash (also the analysis sidecar
    /// key — ARCHITECTURE R5). `None` for demo tracks. Survives renames,
    /// moves, and queue reordering; used for import dedupe.
    pub id: Option<String>,
}

impl Track {
    /// A demo track (no file) for the built-in queue.
    fn demo(title: &str, artist: &str, duration: f32, mood: usize, section: &str) -> Self {
        Self {
            title: title.into(),
            artist: artist.into(),
            album: None,
            duration,
            mood,
            section: section.into(),
            path: None,
            id: None,
        }
    }
}

/// Repeat mode for the queue.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Repeat {
    /// Play to the end of the order, then stop.
    Off,
    /// Wrap forever (reshuffling each pass when shuffled). The default.
    #[default]
    All,
    /// Replay the current track.
    One,
}

impl Repeat {
    /// Off → All → One → Off (the pill's cycle).
    fn next(self) -> Self {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Repeat::Off => "Repeat off",
            Repeat::All => "Repeat all",
            Repeat::One => "Repeat one",
        }
    }
}

/// The transport: what is playing, where we are, and what's next.
#[derive(Resource)]
pub struct Playback {
    pub queue: Vec<Track>,
    /// Index of the now-playing track in `queue`.
    pub current: usize,
    /// Seconds elapsed in the current track.
    pub elapsed: f32,
    pub playing: bool,
    /// Section boundaries as fractions 0..1 of the track — drawn as notches.
    pub sections: Vec<f32>,
    /// Bumped on queue/current changes (advance, import, reorder) so UI
    /// panels can refresh without diffing the queue every frame.
    pub revision: u64,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// Play order over `queue` indices — identity, or a permutation while
    /// shuffled. `current == order[pos]` is the invariant.
    order: Vec<usize>,
    pos: usize,
    /// Shuffle PRNG state.
    rng: u64,
}

impl Default for Playback {
    fn default() -> Self {
        // Sample queue drawn from the spec mockups (1a / 1l); replaced by the
        // user's own music on import (see `audio::poll_import`).
        let queue = vec![
            Track::demo(
                "Amber Waking",
                "Cascade Hour",
                243.0,
                0,
                "Cascade Hour · Slow Light",
            ),
            Track::demo("Night Bloom", "Lys", 227.0, 2, "Chrome Gardens · Rising"),
            Track::demo(
                "Glass Runner",
                "Nova Dusk",
                202.0,
                3,
                "Auto Camera · Slow Orbit",
            ),
            Track::demo("Static Bloom", "Vel", 195.0, 1, "Velvet Circuit · Surge"),
            Track::demo(
                "Undertow",
                "Saltwater Choir",
                311.0,
                2,
                "Tide Gardens · Ebb",
            ),
            Track::demo("Low Sun", "Miren", 280.0, 0, "Ember Flats · Dusk"),
            Track::demo(
                "Hollow Light",
                "The Quiet Party",
                232.0,
                3,
                "Glass Expanse · Late",
            ),
        ];
        let order = (0..queue.len()).collect();
        Self {
            elapsed: 161.0, // 2:41, matching the hero mockup
            current: 0,
            playing: true,
            sections: vec![0.0, 0.18, 0.42, 0.63, 0.85],
            queue,
            revision: 0,
            shuffle: false,
            repeat: Repeat::default(),
            order,
            pos: 0,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }
}

impl Playback {
    pub fn track(&self) -> &Track {
        &self.queue[self.current % self.queue.len()]
    }
    /// The track queued to play next (honours shuffle + repeat), for the HUD
    /// "next" preview and the crossfade target.
    pub fn next_track(&self) -> &Track {
        &self.queue[self.peek_next() % self.queue.len()]
    }
    pub fn duration(&self) -> f32 {
        self.track().duration
    }
    /// Progress 0..1 through the current track.
    pub fn fraction(&self) -> f32 {
        (self.elapsed / self.duration().max(1.0)).clamp(0.0, 1.0)
    }

    /// Queue index that will play after `current` (repeat-one → itself,
    /// otherwise the next slot in `order`, wrapping).
    fn peek_next(&self) -> usize {
        if self.repeat == Repeat::One || self.order.is_empty() {
            return self.current;
        }
        self.order[(self.pos + 1) % self.order.len()]
    }

    /// Whether a natural end has somewhere to go — false only at the end of
    /// the order with repeat off (so the transport stops instead of wrapping).
    pub fn has_next(&self) -> bool {
        self.repeat != Repeat::Off || self.pos + 1 < self.order.len()
    }

    /// Rebuild `order`/`pos` from the queue + shuffle flag. Call after any
    /// queue mutation (import, reorder) or a shuffle toggle.
    pub fn resequence(&mut self) {
        self.order = (0..self.queue.len()).collect();
        if self.shuffle {
            self.reshuffle(true);
        }
        self.pos = self
            .order
            .iter()
            .position(|&i| i == self.current)
            .unwrap_or(0);
    }

    /// Fisher–Yates over `order`. When `keep_current_front`, the current track
    /// is moved to the front so toggling shuffle mid-song keeps it playing.
    fn reshuffle(&mut self, keep_current_front: bool) {
        let len = self.order.len();
        for i in (1..len).rev() {
            let j = (crate::world_assets::splitmix(&mut self.rng) % (i as u64 + 1)) as usize;
            self.order.swap(i, j);
        }
        if keep_current_front && let Some(p) = self.order.iter().position(|&i| i == self.current) {
            self.order.swap(0, p);
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.resequence();
        self.revision += 1;
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.next();
        self.revision += 1;
    }

    /// Step back, returning its mood index. Walks back through `order` (so a
    /// shuffled queue retraces its own history). Callers implement the
    /// "restart if >3s in" convention with a seek instead.
    pub fn previous(&mut self) -> usize {
        let len = self.order.len().max(1);
        self.pos = (self.pos + len - 1) % len;
        self.current = self.order[self.pos.min(self.order.len().saturating_sub(1))];
        self.elapsed = 0.0;
        self.revision += 1;
        self.track().mood
    }

    /// Advance to the next song, returning its mood index — or `None` when the
    /// queue is exhausted (repeat off at the end), meaning the caller stops.
    pub fn advance(&mut self) -> Option<usize> {
        if self.queue.is_empty() {
            return None;
        }
        if self.repeat == Repeat::One {
            // Replay in place.
            self.elapsed = 0.0;
            self.revision += 1;
            return Some(self.track().mood);
        }
        if self.pos + 1 >= self.order.len() {
            // End of the order.
            if self.repeat == Repeat::Off {
                return None;
            }
            if self.shuffle {
                self.reshuffle(false); // fresh pass; may re-lead with any track
            }
            self.pos = 0;
        } else {
            self.pos += 1;
        }
        self.current = self.order[self.pos];
        self.elapsed = 0.0;
        self.revision += 1;
        Some(self.track().mood)
    }
}

/// Format seconds as `M:SS` (tabular, matching the HUD).
pub fn fmt_time(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Beat + energy signals. `phase` ramps 0→1 each beat; `pulse` spikes to 1 on
/// each beat and decays; `energy` is a slow 0..1 envelope for the song's drive.
#[derive(Resource)]
pub struct Beat {
    pub bpm: f32,
    pub phase: f32,
    pub pulse: f32,
    pub energy: f32,
    /// Live bass-band loudness 0..1 (smoothed from the audio tap). Feeds the
    /// bass-reactive world layers (drifter sway, beacon pulse) even when the
    /// track has no Demucs stems.
    pub bass: f32,
    /// Live high-band loudness 0..1 ("sparkle") from the audio tap — drives
    /// the particle field's emissive breathing.
    pub highs: f32,
    /// Wall-clock accumulator used to shape the simulated energy envelope.
    pub clock: f32,
    /// First-beat offset in seconds (from analysis, PLAN.md M3).
    pub offset: f32,
    /// True once `bpm`/`offset` come from real analysis — the phase is then
    /// derived from the transport clock instead of integrated.
    pub grid: bool,
}

impl Default for Beat {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            phase: 0.0,
            pulse: 0.0,
            energy: 0.6,
            bass: 0.0,
            highs: 0.0,
            clock: 0.0,
            offset: 0.0,
            grid: false,
        }
    }
}

/// Per-stem intensity 0..1 for the current track at the playhead —
/// `[drums, bass, vocals, other]`, sampled from the analysis sidecar's Demucs
/// curves (`ml` feature) and one-pole smoothed. All zeros when the track has
/// no stems; consumers take `max(stem, live band)` so the mixed/live signals
/// degrade into the same behaviours.
#[derive(Resource, Default)]
pub struct StemLevels(pub [f32; 4]);

/// Advance the transport and synthesise the beat/energy signals.
///
/// Signal ownership by source (PLAN.md M1–M3):
/// - no real audio → everything simulated here;
/// - real audio, no analysis → `audio.rs` owns elapsed + energy/pulse (live
///   tap); phase free-runs at the default bpm;
/// - real audio + analysis grid → phase/pulse derive from the transport
///   clock against the measured beat grid (predictive, tight).
pub fn advance_playback(
    time: Res<Time>,
    audio_active: Res<crate::AudioActive>,
    mut playback: ResMut<Playback>,
    mut beat: ResMut<Beat>,
    mut theme: ResMut<crate::theme::Theme>,
) {
    let dt = time.delta_secs();
    let live = audio_active.0;

    // Simulated energy envelope — only while no live tap feeds it.
    beat.clock += dt;
    if !live {
        beat.energy = 0.62 + 0.28 * (beat.clock * 0.35).sin() * (beat.clock * 0.11).cos();
        beat.energy = beat.energy.clamp(0.3, 0.95);
    }

    if !playback.playing {
        // Pulse still decays so a paused world settles.
        beat.pulse = (beat.pulse - dt * 3.0).max(0.0);
        return;
    }
    beat.pulse = (beat.pulse - dt * 4.0).max(0.0);

    // Beat phase.
    if live && beat.grid {
        // Derived from the real clock against the measured grid.
        let prev = beat.phase;
        beat.phase = ((playback.elapsed - beat.offset).max(0.0) * beat.bpm / 60.0).fract();
        if beat.phase < prev {
            beat.pulse = 1.0;
        }
    } else {
        let prev = beat.phase;
        beat.phase = (beat.phase + dt * beat.bpm / 60.0).fract();
        // Spike on wrap only when fully simulated; with a live tap (but no
        // grid yet) real onsets own the pulse.
        if beat.phase < prev && !live {
            beat.pulse = 1.0;
        }
    }

    // Transport — simulated only while no real stream owns the clock.
    if live {
        return;
    }
    playback.elapsed += dt;
    if playback.elapsed >= playback.duration() {
        match playback.advance() {
            Some(mood) => theme.mood = mood,
            None => playback.playing = false, // repeat-off: end of queue
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests tweak a couple of fields on the default (queue-building) transport.
    #![allow(clippy::field_reassign_with_default)]
    use super::*;

    #[test]
    fn fmt_time_formats_and_clamps() {
        assert_eq!(fmt_time(0.0), "0:00");
        assert_eq!(fmt_time(65.0), "1:05");
        assert_eq!(fmt_time(161.0), "2:41");
        assert_eq!(fmt_time(243.0), "4:03");
        assert_eq!(fmt_time(-5.0), "0:00");
    }

    #[test]
    fn fraction_is_clamped_0_to_1() {
        let mut p = Playback::default();
        p.elapsed = 0.0;
        assert_eq!(p.fraction(), 0.0);
        p.elapsed = p.duration() * 2.0;
        assert_eq!(p.fraction(), 1.0);
    }

    /// Move the transport to a specific queue index (as the UI does).
    fn seek_to(p: &mut Playback, idx: usize) {
        p.current = idx;
        p.resequence();
    }

    #[test]
    fn advance_wraps_and_resets_elapsed() {
        let mut p = Playback::default();
        let last = p.queue.len() - 1;
        seek_to(&mut p, last);
        p.elapsed = 99.0;
        let mood = p.advance().unwrap();
        assert_eq!(p.current, 0);
        assert_eq!(p.elapsed, 0.0);
        assert_eq!(mood, p.queue[0].mood);
    }

    #[test]
    fn next_track_wraps_to_first() {
        let mut p = Playback::default();
        let last = p.queue.len() - 1;
        seek_to(&mut p, last);
        assert_eq!(p.next_track().title, p.queue[0].title);
    }

    #[test]
    fn repeat_one_replays_in_place() {
        let mut p = Playback::default();
        seek_to(&mut p, 2);
        p.repeat = Repeat::One;
        assert_eq!(p.advance(), Some(p.queue[2].mood));
        assert_eq!(p.current, 2);
        assert_eq!(p.next_track().title, p.queue[2].title);
    }

    #[test]
    fn repeat_off_stops_at_end() {
        let mut p = Playback::default();
        p.repeat = Repeat::Off;
        let last = p.queue.len() - 1;
        seek_to(&mut p, last);
        assert!(!p.has_next());
        assert_eq!(p.advance(), None);
    }

    #[test]
    fn shuffle_visits_every_track_once_per_pass() {
        let mut p = Playback::default();
        let n = p.queue.len();
        p.toggle_shuffle();
        assert!(p.shuffle);
        let mut seen = vec![p.current];
        for _ in 0..n - 1 {
            p.advance().unwrap();
            seen.push(p.current);
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), n, "one shuffled pass covers every track once");
    }

    #[test]
    fn previous_retraces_shuffled_order() {
        let mut p = Playback::default();
        p.toggle_shuffle();
        let a = p.current;
        p.advance().unwrap();
        let b = p.current;
        assert_ne!(a, b);
        assert_eq!(p.previous(), p.queue[a].mood);
        assert_eq!(p.current, a);
    }
}
