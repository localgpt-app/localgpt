//! Real audio playback — PLAN.md M1.
//!
//! kira (mixer/clocks/tweens on cpal) + symphonia (decode) feed the same
//! `Playback` resource the UI already reads; the simulated transport in
//! `playback.rs` stays as the fallback. Degrades gracefully twice over:
//! no audio device → silent world, simulated clock; a track with no file
//! (the built-in demo queue) → same.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::prelude::*;
use kira::effect::{Effect, EffectBuilder};
use kira::info::Info;
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{FromFileError, PlaybackState};
use kira::track::{TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Easing, Frame, Tween};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::tag::Accessor;

use crate::AudioActive;
use crate::playback::{Beat, Playback, Track};
use crate::theme::Theme;

/// A tween with the given duration (start immediately, linear).
fn tween_ms(ms: u64) -> Tween {
    Tween {
        duration: Duration::from_millis(ms),
        ..Default::default()
    }
}

/// A tween with an easing curve.
fn tween_ease(secs: f32, easing: Easing) -> Tween {
    Tween {
        duration: Duration::from_secs_f32(secs.max(0.05)),
        easing,
        ..Default::default()
    }
}

/// Crossfade length for a track of `duration` seconds — 6s for full songs
/// (spec 1r: "6–10s"), scaled down for short material so the fade never eats
/// the whole track.
pub(crate) fn crossfade_secs(duration: f32) -> f32 {
    (duration * 0.25).clamp(1.0, 6.0)
}

// ---------------------------------------------------------------------------
// Player
// ---------------------------------------------------------------------------

struct AudioInner {
    /// Owns the audio thread — never read after setup, must stay alive.
    #[allow(dead_code)]
    manager: AudioManager<DefaultBackend>,
    /// All music plays on this sub-track so the tap effect hears it.
    track: TrackHandle,
    handle: Option<StreamingSoundHandle<FromFileError>>,
    /// The previous song, still fading out under the new one (crossfade).
    fading_out: Option<StreamingSoundHandle<FromFileError>>,
    /// Fade-in to apply to the next stream started (set when crossfading).
    fade_in_next: Option<f32>,
    /// Path the current handle was started for — path-keyed (not index) so a
    /// queue reorder doesn't restart the playing track (ARCHITECTURE R5).
    playing: Option<PathBuf>,
    /// A seek in flight: hold `elapsed` at the target until the stream's
    /// readback catches up (or the frame budget runs out), so the progress
    /// bar doesn't snap backwards for a few frames.
    seek_hold: Option<(f32, u8)>,
    /// One-time "clock live" log for smoke verification.
    clock_logged: bool,
}

/// The audio engine. `Mutex` because `AudioManager` is `Send` but not `Sync`
/// (kira parks the cpal stream on its own thread); locks are held for
/// microseconds a few times per frame.
#[derive(Resource, Default)]
pub struct AudioPlayer(Mutex<Option<AudioInner>>);

/// Lock-free signals shared with the audio thread (see [`TapEffect`]).
#[derive(Resource, Default, Clone)]
pub struct AudioTap(Arc<TapShared>);

/// Create the audio device. On failure the app runs silent (simulated clock).
pub fn init_audio(player: ResMut<AudioPlayer>, tap: Res<AudioTap>) {
    let mut guard = player.0.lock().unwrap();
    match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
        Ok(mut manager) => {
            // Music sub-track with the analysis tap effect (PLAN.md M2).
            let builder = TrackBuilder::new().with_effect(TapBuilder(tap.0.clone()));
            match manager.add_sub_track(builder) {
                Ok(track) => {
                    *guard = Some(AudioInner {
                        manager,
                        track,
                        handle: None,
                        fading_out: None,
                        fade_in_next: None,
                        playing: None,
                        seek_hold: None,
                        clock_logged: false,
                    });
                    info!("Audio device ready");
                }
                Err(e) => warn!("Audio mixer setup failed — running silent ({e})"),
            }
        }
        Err(e) => warn!("No audio device — running silent ({e})"),
    }
}

/// Start/stop streams so the playing sound always matches `Playback.current`.
pub fn sync_track_playback(
    player: Res<AudioPlayer>,
    mut playback: ResMut<Playback>,
    mut audio_active: ResMut<AudioActive>,
) {
    let mut guard = player.0.lock().unwrap();
    let Some(inner) = guard.as_mut() else {
        audio_active.0 = false;
        return;
    };
    if playback.queue.is_empty() {
        audio_active.0 = false;
        return;
    }

    let idx = playback.current % playback.queue.len();
    let want = playback.queue[idx].path.clone();
    let switching = inner.playing != want;

    if switching {
        if let Some(handle) = &mut inner.handle {
            // Skip smear — a quick fade instead of a hard cut (spec 1r: 400ms).
            handle.stop(tween_ms(400));
        }
        inner.handle = None;
        inner.playing = want.clone();

        if let Some(path) = &want {
            match StreamingSoundData::from_file(path) {
                Ok(sound) => {
                    // Trust the decoder's duration over the tag header.
                    let duration = sound.duration().as_secs_f32();
                    if duration > 1.0 {
                        playback.queue[idx].duration = duration;
                    }
                    // Crossfade entry: ramp in on the equal-power counterpart
                    // of the outgoing stream's fade (see sync_clock).
                    let sound = match inner.fade_in_next.take() {
                        Some(secs) => sound.fade_in_tween(tween_ease(secs, Easing::OutPowi(2))),
                        None => sound,
                    };
                    match inner.track.play(sound) {
                        Ok(mut handle) => {
                            // A track can start while paused (pause → skip):
                            // hold it silently until resume.
                            if !playback.playing {
                                handle.pause(tween_ms(0));
                            }
                            let t = &playback.queue[idx];
                            info!("Now playing: {} — {}", t.title, t.artist);
                            inner.handle = Some(handle);
                        }
                        Err(e) => warn!("Can't play {}: {e}", path.display()),
                    }
                }
                Err(e) => warn!("Can't decode {}: {e}", path.display()),
            }
        }
    }
    audio_active.0 = inner.handle.is_some();
}

/// Pause/resume with the spec's time-dilation tweens (220ms in / 320ms out).
///
/// Follows the transport itself (`playback.playing`) rather than the pause
/// *menu*, so the HUD's play/pause button freezes the sound in place without
/// opening the overlay (Esc still does). Acts only on actual value flips
/// (`Local` tracks the last state): re-issuing `pause()` restarts its fade
/// tween, so a writer that flipped every frame would otherwise keep the sound
/// in a never-finishing fade.
pub fn sync_pause(
    player: Res<AudioPlayer>,
    playback: Res<Playback>,
    mut last: Local<Option<bool>>,
) {
    let is_paused = !playback.playing;
    if *last == Some(is_paused) {
        return;
    }
    let was = last.replace(is_paused);
    if was.is_none() && !is_paused {
        return; // startup default — nothing to do
    }
    let mut guard = player.0.lock().unwrap();
    let Some(inner) = guard.as_mut() else { return };
    // Both streams breathe together during a crossfade.
    for handle in inner.handle.iter_mut().chain(inner.fading_out.iter_mut()) {
        if is_paused {
            handle.pause(tween_ms(220));
        } else {
            handle.resume(tween_ms(320));
        }
    }
}

/// The audio clock owns `elapsed` while a stream is live, advances the queue
/// when a track ends, and starts the natural song→song crossfade (spec 1h/1r):
/// as A approaches its end, it fades out (≈cos curve) while B fades in
/// (≈sin) — an equal-power pair — and the world morph "lands" on B's first
/// downbeat via the materialize sequence keyed to B's beat grid.
pub fn sync_clock(
    player: Res<AudioPlayer>,
    mut playback: ResMut<Playback>,
    mut theme: ResMut<Theme>,
) {
    let mut guard = player.0.lock().unwrap();
    let Some(inner) = guard.as_mut() else { return };

    // Drop the outgoing stream once its fade completes.
    if inner
        .fading_out
        .as_ref()
        .is_some_and(|f| f.state() == PlaybackState::Stopped)
    {
        inner.fading_out = None;
    }

    let Some(handle) = &mut inner.handle else {
        return;
    };
    let pos = handle.position() as f32;
    match &mut inner.seek_hold {
        Some((target, frames_left)) => {
            if (pos - *target).abs() < 0.3 || *frames_left == 0 {
                inner.seek_hold = None;
                playback.elapsed = pos;
            } else {
                *frames_left -= 1;
                playback.elapsed = *target;
            }
        }
        None => playback.elapsed = pos,
    }
    let pos = playback.elapsed;
    if !inner.clock_logged && pos > 0.25 {
        inner.clock_logged = true;
        info!("Audio clock live: {pos:.2}s");
    }

    if handle.state() == PlaybackState::Stopped {
        // Hard end (no crossfade ran: single-track queue, or a very short
        // fade window was missed) — mirror the simulated end-of-track path.
        inner.handle = None;
        inner.playing = None;
        match playback.advance() {
            Some(mood) => theme.mood = mood,
            None => playback.playing = false, // repeat-off: queue exhausted
        }
        return;
    }

    // Begin the crossfade when A enters its final `cross` seconds. Single-
    // track queues (and the last track with repeat off) skip it — the same
    // stream can't overlap itself and there may be nothing to cross to.
    let duration = playback.duration();
    let cross = crossfade_secs(duration);
    let remaining = duration - pos;
    if inner.fading_out.is_none()
        && playback.queue.len() > 1
        && playback.has_next()
        && duration > 2.0
        && remaining > 0.05
        && remaining <= cross
    {
        if let Some(mut old) = inner.handle.take() {
            old.stop(tween_ease(cross, Easing::InPowi(2)));
            inner.fading_out = Some(old);
        }
        inner.playing = None;
        inner.fade_in_next = Some(cross);
        info!(
            "Crossfading to: {} ({cross:.1}s)",
            playback.next_track().title
        );
        if let Some(mood) = playback.advance() {
            theme.mood = mood;
        }
    }
}

/// Consume a pending [`crate::SeekRequest`]: seek the live stream (holding
/// the UI clock at the target until the readback catches up) or, on the
/// simulated path, just move the clock.
pub fn apply_seek(
    player: Res<AudioPlayer>,
    mut seek: ResMut<crate::SeekRequest>,
    mut playback: ResMut<Playback>,
) {
    let Some(target) = seek.0.take() else { return };
    let duration = playback.duration();
    let target = target.clamp(0.0, (duration - 0.1).max(0.0));
    playback.elapsed = target;

    let mut guard = player.0.lock().unwrap();
    if let Some(inner) = guard.as_mut()
        && let Some(handle) = &mut inner.handle
    {
        handle.seek_to(target as f64);
        inner.seek_hold = Some((target, 30));
    }
}

/// Drive the music sub-track's volume from the user fader combined with the
/// current track's loudness normalization (both in dB). Re-tweens only on a
/// meaningful change so it doesn't restart the tween every frame.
pub fn apply_volume(
    player: Res<AudioPlayer>,
    volume: Res<crate::Volume>,
    analysis: Res<crate::analysis::AnalysisStore>,
    playback: Res<Playback>,
    mut last_db: Local<Option<f32>>,
) {
    if playback.queue.is_empty() {
        return;
    }
    let id = playback.track().id.as_deref();
    let norm_db = analysis.norm_db_for(id);
    // Amplitude fader in dB: 1.0 → 0 dB, 0.5 → -6 dB, 0 → silence.
    let user_db = if volume.0 <= 0.001 {
        -80.0
    } else {
        20.0 * volume.0.log10()
    };
    let total_db = (norm_db + user_db).clamp(-80.0, 12.0);
    if last_db.is_some_and(|d| (d - total_db).abs() < 0.1) {
        return;
    }
    *last_db = Some(total_db);

    let mut guard = player.0.lock().unwrap();
    if let Some(inner) = guard.as_mut() {
        inner
            .track
            .set_volume(kira::Decibels(total_db), tween_ms(120));
    }
}

// ---------------------------------------------------------------------------
// Library import
// ---------------------------------------------------------------------------

/// Background folder scan feeding tracks into the queue as they're found.
#[derive(Resource, Default)]
pub struct ImportState {
    rx: Option<Mutex<Receiver<Track>>>,
    /// True once imported tracks have replaced the demo queue.
    pub imported_any: bool,
    count: usize,
}

const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "ogg", "oga", "m4a", "aac", "aiff", "aif",
];

/// Whether an import would take this file (by extension).
pub(crate) fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| AUDIO_EXTS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Spawn a scan thread over `folder`; tracks stream in via `poll_import`.
pub fn start_import(folder: PathBuf, import: &mut ImportState) {
    info!("Importing music from {}", folder.display());
    let (tx, rx) = channel();
    import.rx = Some(Mutex::new(rx));
    import.count = 0;
    std::thread::spawn(move || {
        for entry in walkdir::WalkDir::new(&folder)
            .follow_links(true)
            .sort_by_file_name() // deterministic queue order across platforms
            .into_iter()
            .flatten()
        {
            let path = entry.path();
            if !entry.file_type().is_file() || !is_audio(path) {
                continue;
            }
            if let Some(track) = read_track(path)
                && tx.send(track).is_err()
            {
                return; // receiver dropped — app shutting down
            }
        }
    });
}

/// Read tags + duration for one file. `None` skips it (unreadable/too short).
pub(crate) fn read_track(path: &Path) -> Option<Track> {
    let tagged = lofty::read_from_path(path).ok()?;
    let duration = tagged.properties().duration().as_secs_f32();
    if duration < 1.0 {
        return None;
    }
    let tag = tagged.primary_tag().or_else(|| tagged.tags().first());
    let title = tag
        .and_then(|t| t.title().map(|s| s.into_owned()))
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".into())
        });
    let artist = tag
        .and_then(|t| t.artist().map(|s| s.into_owned()))
        .unwrap_or_else(|| "Unknown Artist".into());
    let album = tag.and_then(|t| t.album().map(|s| s.into_owned()));
    Some(Track {
        title,
        artist,
        album,
        duration,
        mood: path_mood(path),
        section: "Your library".into(),
        path: Some(path.to_path_buf()),
        // Content hash = stable identity (dedupe, sidecar key, reorder-proof).
        id: crate::analysis::cache_key(path),
    })
}

/// Deterministic mood per file until the real mapper lands (PLAN.md M4):
/// same file → same world.
pub(crate) fn path_mood(path: &Path) -> usize {
    let bytes = path.as_os_str().as_encoded_bytes();
    let hash = bytes.iter().fold(0usize, |acc, &b| {
        acc.wrapping_mul(31).wrapping_add(b as usize)
    });
    hash % crate::theme::moods().len()
}

/// Drain the scan channel into the queue. The first batch replaces the demo
/// queue and restarts playback on the user's own music.
pub fn poll_import(
    mut import: ResMut<ImportState>,
    mut playback: ResMut<Playback>,
    mut theme: ResMut<Theme>,
    player: Res<AudioPlayer>,
) {
    let Some(rx_mutex) = &import.rx else { return };

    let mut batch = Vec::new();
    let mut done = false;
    {
        let rx = rx_mutex.lock().unwrap();
        loop {
            match rx.try_recv() {
                Ok(track) => {
                    batch.push(track);
                    if batch.len() >= 64 {
                        break; // bound per-frame work
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    done = true;
                    break;
                }
            }
        }
    }

    if !batch.is_empty() {
        // Dedupe by content id — the same song reachable through two paths
        // (symlinked folders, stray copies) imports once (ARCHITECTURE R5).
        let mut seen: std::collections::HashSet<String> =
            playback.queue.iter().filter_map(|t| t.id.clone()).collect();
        batch.retain(|t| match &t.id {
            Some(id) => seen.insert(id.clone()),
            None => true,
        });
        if !batch.is_empty() {
            if !import.imported_any {
                import.imported_any = true;
                playback.queue.clear();
                playback.current = 0;
                playback.elapsed = 0.0;
                playback.playing = true;
                theme.mood = batch[0].mood;
                // Force the player to restart on the new queue.
                if let Some(inner) = player.0.lock().unwrap().as_mut() {
                    for handle in inner.handle.iter_mut().chain(inner.fading_out.iter_mut()) {
                        handle.stop(tween_ms(200));
                    }
                    inner.handle = None;
                    inner.fading_out = None;
                    inner.fade_in_next = None;
                    inner.playing = None;
                }
            }
            import.count += batch.len();
            playback.queue.append(&mut batch);
            playback.resequence(); // keep order/pos valid as the queue grows
            playback.revision += 1;
        }
    }

    if done {
        info!("Library import finished: {} tracks", import.count);
        import.rx = None;
    }
}

// ---------------------------------------------------------------------------
// Live tap (PLAN.md M2) — allocation-free DSP on the audio thread
// ---------------------------------------------------------------------------
//
// Full FFT analysis belongs to the offline pass (M3); the live tap only needs
// two cheap signals: a smoothed loudness envelope ("energy") and bass-onset
// impulses ("pulse"). One-pole filters are real-time safe and good enough —
// bass onsets track the beat for most music.

/// f32s shared across threads as atomic bit patterns.
#[derive(Default)]
pub struct TapShared {
    /// Normalized loudness 0..1 (fast RMS over slow RMS).
    energy: AtomicU32,
    /// Onset impulse latch; the app consumes (zeroes) it.
    pulse: AtomicU32,
    /// Bass-band level 0..1 (absolute fast RMS of the low-passed signal) —
    /// feeds the bass-reactive world layers.
    bass: AtomicU32,
    /// High-band loudness 0..1 ("sparkle", fast over slow RMS above ~3.5 kHz)
    /// — drives the particle field's emissive breathing.
    highs: AtomicU32,
}

impl TapShared {
    fn store(atomic: &AtomicU32, v: f32) {
        atomic.store(v.to_bits(), Ordering::Relaxed);
    }
    fn load(atomic: &AtomicU32) -> f32 {
        f32::from_bits(atomic.load(Ordering::Relaxed))
    }
}

struct TapBuilder(Arc<TapShared>);

impl EffectBuilder for TapBuilder {
    type Handle = ();
    fn build(self) -> (Box<dyn Effect>, Self::Handle) {
        (
            Box::new(TapEffect {
                shared: self.0,
                lp: 0.0,
                lp_all: 0.0,
                sq_fast: 0.0,
                sq_slow: 0.0,
                low_fast: 0.0,
                low_slow: 0.0,
                hi_fast: 0.0,
                hi_slow: 0.0,
                cooldown: 0.0,
                pulse_latch: 0.0,
            }),
            (),
        )
    }
}

struct TapEffect {
    shared: Arc<TapShared>,
    /// One-pole low-pass state (~120 Hz) isolating the bass band.
    lp: f32,
    /// One-pole low-pass state (~3.5 kHz) whose complement is the high band.
    lp_all: f32,
    /// Fast/slow mean-square envelopes of the full signal (~25ms / ~1.2s).
    sq_fast: f32,
    sq_slow: f32,
    /// Fast/slow envelopes of the bass band (~15ms / ~700ms).
    low_fast: f32,
    low_slow: f32,
    /// Fast/slow envelopes of the high band (~25ms / ~0.6s).
    hi_fast: f32,
    hi_slow: f32,
    /// Seconds until another onset may fire (debounce).
    cooldown: f32,
    pulse_latch: f32,
}

/// One-pole smoothing coefficient for time-constant `tau` at step `dt`.
fn one_pole(dt: f32, tau: f32) -> f32 {
    1.0 - (-dt / tau).exp()
}

impl Effect for TapEffect {
    fn process(&mut self, input: &mut [Frame], dt: f64, _info: &Info) {
        let dt = dt as f32;
        let k_lp = one_pole(dt, 1.0 / (2.0 * std::f32::consts::PI * 120.0));
        let k_fast = one_pole(dt, 0.025);
        let k_slow = one_pole(dt, 1.2);
        let k_lfast = one_pole(dt, 0.015);
        let k_lslow = one_pole(dt, 0.7);
        let k_hslow = one_pole(dt, 0.6);
        let k_all = one_pole(dt, 1.0 / (2.0 * std::f32::consts::PI * 3500.0));

        for frame in input.iter() {
            let mono = (frame.left + frame.right) * 0.5;
            // Bass band via one-pole low-pass; high band via its ~3.5 kHz
            // complement (mono − lowpassed).
            self.lp += k_lp * (mono - self.lp);
            self.lp_all += k_all * (mono - self.lp_all);
            let low_sq = self.lp * self.lp;
            let high = mono - self.lp_all;
            let high_sq = high * high;
            let sq = mono * mono;

            self.sq_fast += k_fast * (sq - self.sq_fast);
            self.sq_slow += k_slow * (sq - self.sq_slow);
            self.low_fast += k_lfast * (low_sq - self.low_fast);
            self.low_slow += k_lslow * (low_sq - self.low_slow);
            self.hi_fast += k_fast * (high_sq - self.hi_fast);
            self.hi_slow += k_hslow * (high_sq - self.hi_slow);

            self.cooldown = (self.cooldown - dt).max(0.0);
            // Onset: the bass envelope jumps well above its running average.
            if self.cooldown == 0.0 && self.low_slow > 1e-7 && self.low_fast > self.low_slow * 2.2 {
                self.pulse_latch = 1.0;
                self.cooldown = 0.18; // ≥180ms between onsets (≈ ≤ 333 BPM)
            }
        }

        // Publish once per buffer (a few hundred frames), not per frame.
        let energy = if self.sq_slow > 1e-8 {
            ((self.sq_fast / (self.sq_slow * 2.5)).sqrt()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Absolute-ish bass level (RMS of the isolated band), scaled into
        // 0..1 for typical mastered loudness.
        let bass = (self.low_fast.max(0.0).sqrt() * 3.0).clamp(0.0, 1.0);
        let highs = if self.hi_slow > 1e-8 {
            ((self.hi_fast / (self.hi_slow * 2.5)).sqrt()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        TapShared::store(&self.shared.energy, energy);
        TapShared::store(&self.shared.bass, bass);
        TapShared::store(&self.shared.highs, highs);
        if self.pulse_latch > 0.0 {
            TapShared::store(&self.shared.pulse, self.pulse_latch);
            self.pulse_latch = 0.0;
        }
    }
}

/// Feed the live tap into `Beat` while real audio plays. The pulse latch is
/// consumed here; `advance_playback` handles decay (and the beat-grid phase
/// once analysis lands, PLAN.md M3).
pub fn update_beat_from_tap(
    tap: Res<AudioTap>,
    audio_active: Res<AudioActive>,
    time: Res<Time>,
    mut beat: ResMut<Beat>,
) {
    if !audio_active.0 {
        return;
    }
    // Smooth the published energy a little more UI-side.
    let target = TapShared::load(&tap.0.energy);
    let k = (time.delta_secs() * 8.0).min(1.0);
    beat.energy = (beat.energy + (target - beat.energy) * k).clamp(0.0, 1.0);
    // Same for the band envelopes (a touch faster — they carry rhythm).
    let kb = (time.delta_secs() * 10.0).min(1.0);
    let bass = TapShared::load(&tap.0.bass);
    let highs = TapShared::load(&tap.0.highs);
    beat.bass = (beat.bass + (bass - beat.bass) * kb).clamp(0.0, 1.0);
    beat.highs = (beat.highs + (highs - beat.highs) * kb).clamp(0.0, 1.0);

    // Consume the onset latch.
    let latch = TapShared::load(&tap.0.pulse);
    if latch > 0.0 {
        TapShared::store(&tap.0.pulse, 0.0);
        beat.pulse = beat.pulse.max(latch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_mood_is_stable_and_in_range() {
        let p = Path::new("/music/artist/song.flac");
        assert_eq!(path_mood(p), path_mood(p));
        for path in ["/a.mp3", "/b.mp3", "/c/d.flac", "/e/f/g.wav"] {
            assert!(path_mood(Path::new(path)) < crate::theme::moods().len());
        }
    }

    #[test]
    fn crossfade_scales_with_duration() {
        assert_eq!(crossfade_secs(240.0), 6.0); // full songs cap at 6s
        assert_eq!(crossfade_secs(4.0), 1.0); //   shorts floor at 1s
        assert_eq!(crossfade_secs(10.0), 2.5); //  quarter of the track between
    }

    #[test]
    fn path_mood_varies_across_paths() {
        // Not all paths land on one mood (would make the hash pointless).
        let moods: std::collections::HashSet<usize> = (0..32)
            .map(|i| path_mood(Path::new(&format!("/music/track-{i}.mp3"))))
            .collect();
        assert!(moods.len() > 1);
    }
}
