//! LocalGPT Verse — a desktop app that imagines a 3D world for every song.
//!
//! The app is [`run`] (`src/main.rs` is only that). The library half is for
//! other LocalGPT apps: [`song_world`] turns a song file into its world in
//! the shared format — tags, analysis, mood and props, soundtrack curves and
//! modulations — without a window, which is how the one-window LocalGPT app
//! opens a song.

#[cfg(feature = "llm")]
mod agent;
mod agent_types;
mod analysis;
mod audio;
mod demucs;
mod hud;
#[cfg(feature = "llm")]
mod llm;
#[cfg(feature = "ml")]
mod ml;
mod mood_pack;
mod overlays;
mod playback;
mod plugins;
mod recipe;
mod scope;
mod settings;
mod theme;
mod tier;
mod world;
mod world_assets;
mod world_manifest;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

use hud::HudActivity;
use playback::{Beat, Playback};
use theme::Theme;

// ---------------------------------------------------------------------------
// Shared app state & resources
// ---------------------------------------------------------------------------

/// Top-level screen.
#[derive(States, Default, Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum AppState {
    /// First run — "Bring your music".
    #[default]
    FirstRun,
    /// Playing — HUD over a live world.
    InWorld,
}

/// Camera feel. Also stored inside each HUD mode tab.
#[derive(
    Resource, Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize,
)]
pub enum CameraMode {
    /// First-person fly cam: pointer-locked 360° look, WASD + Space/Shift.
    #[default]
    Explore,
    /// Hands-off cinematic auto-orbit.
    Drift,
}

/// Whether the pause overlay is up (and the world is time-dilated).
#[derive(Resource, Default)]
pub struct Paused(pub bool);

/// Whether the queue panel is sliding in.
#[derive(Resource, Default)]
pub struct QueueOpen(pub bool);

/// A modal overlay screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    Settings,
    Credits,
    Library,
}

/// The modal overlays as a stack — Esc closes the top one (ARCHITECTURE R4;
/// replaces three independent bools with hand-ordered priority). The queue is
/// a side panel (`QueueOpen`) and `Paused` is transport state; both separate.
#[derive(Resource, Default)]
pub struct OverlayStack(Vec<Overlay>);

impl OverlayStack {
    pub fn open(&mut self, overlay: Overlay) {
        if !self.0.contains(&overlay) {
            self.0.push(overlay);
        }
    }
    pub fn close(&mut self, overlay: Overlay) {
        self.0.retain(|o| *o != overlay);
    }
    pub fn toggle(&mut self, overlay: Overlay) {
        if self.is_open(overlay) {
            self.close(overlay);
        } else {
            self.open(overlay);
        }
    }
    /// Close the topmost overlay; `None` if nothing was open.
    pub fn pop(&mut self) -> Option<Overlay> {
        self.0.pop()
    }
    pub fn is_open(&self, overlay: Overlay) -> bool {
        self.0.contains(&overlay)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// Photo mode: frames to wait (HUD hidden) before capturing a clean shot.
#[derive(Resource, Default)]
pub struct Photo {
    pub pending: Option<u8>,
    /// GPU readback occasionally returns an all-black frame on some machines
    /// (ARCHITECTURE R8) — bounded re-requests paper over it.
    pub retries: u8,
    /// Output path for the in-flight request; retries overwrite it so a
    /// flaky readback never litters black files.
    pub path: Option<String>,
}

impl Photo {
    /// Request a photo — the HUD hides and a screenshot lands a few frames
    /// later (enough for the chrome to clear and the frame to settle).
    pub fn request(&mut self) {
        self.pending = Some(8);
        self.retries = 0;
        self.path = None;
    }
}

/// First-run onboarding step (0 = photosensitivity, 1 = controls, 2 = import).
#[derive(Resource, Default)]
pub struct Onboarding {
    pub step: u8,
}

/// World-intensity slider value (0..1), shown in the pause overlay.
#[derive(Resource)]
pub struct WorldIntensity(pub f32);

impl Default for WorldIntensity {
    fn default() -> Self {
        Self(0.5)
    }
}

/// Comfort settings — the reduce-flashing gate the spec insists on.
#[derive(
    Resource, Default, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
pub struct Comfort {
    pub reduce_flashing: bool,
    pub gentler_motion: bool,
}

/// True while a real audio stream owns the transport clock (see `audio.rs`).
/// When false, `playback::advance_playback` simulates it.
#[derive(Resource, Default)]
pub struct AudioActive(pub bool);

/// A pending transport seek, in seconds. Written by the seek strip / arrow
/// keys, consumed by `audio::apply_seek` (which also handles the silent
/// simulated path).
#[derive(Resource, Default)]
pub struct SeekRequest(pub Option<f32>);

/// User playback volume, 0..1 (a perceptual fader; see `audio::apply_volume`).
/// Combined with per-track loudness normalization on the music sub-track.
#[derive(Resource)]
pub struct Volume(pub f32);

impl Default for Volume {
    fn default() -> Self {
        Self(0.85)
    }
}

/// The world's timescale (1.0 playing, eases to ~0.05 when paused).
#[derive(Resource)]
pub struct WorldClock {
    pub speed: f32,
}

impl Default for WorldClock {
    fn default() -> Self {
        Self { speed: 1.0 }
    }
}

/// A song's world in the LocalGPT world format: its tags, its analysis
/// (decoded once and cached per user), and the world Verse builds from them —
/// the rule-derived mood world, props from the CC0 pack when it is present,
/// the soundtrack's curves and the modulations that perform them. The same
/// world `VERSE_EXPORT_WORLD` writes. `soundtrack.path` is set only for a
/// starter-pack track whose licence allows shipping it; for anything else
/// the caller decides whether to attach the audio. `None` when the file is
/// not audio Verse can read, or cannot be analysed.
pub fn song_world(path: &std::path::Path) -> Option<localgpt_world_types::WorldManifest> {
    if !audio::is_audio(path) {
        return None;
    }
    let track = audio::read_track(path)?;
    let id = track.id.clone()?;
    let analysis = analysis::analysis_for(&id, path)?;
    let assets = world_assets::read_manifest_from_disk();
    Some(world_manifest::export_world(
        &track,
        &analysis,
        assets.as_ref(),
        &world_assets::AssetEmbeddings::default(),
    ))
}

/// Run the Verse app.
pub fn run() {
    // Batch export: every track's world, no window (see `world_manifest`).
    if std::env::var_os("VERSE_EXPORT_ONLY").is_some() {
        std::process::exit(world_manifest::export_only());
    }
    let mut window = Window {
        title: "LocalGPT Verse".to_string(),
        resolution: (1280, 800).into(),
        ..default()
    };
    // Stress mode measures true frame cost — vsync hides real throughput
    // behind display pacing (and background throttling skews it further).
    if world_assets::StressTest::from_env().is_some() {
        window.present_mode = bevy::window::PresentMode::AutoNoVsync;
    }

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(window),
                ..default()
            })
            // Pin the asset server to the one root the direct-filesystem
            // readers also use (see `world_assets::asset_root`). Bevy's own
            // default is `<exe dir>/assets` — but only when `CARGO_MANIFEST_DIR`
            // is absent from the *environment*, which is true for a packaged
            // app and false under `cargo run`. Leaving it implicit lets the two
            // halves resolve differently; an absolute `file_path` settles it,
            // since Bevy joins this onto its base path and joining an absolute
            // path discards the base.
            .set(AssetPlugin {
                file_path: world_assets::asset_root().to_string_lossy().into_owned(),
                ..default()
            }),
    )
    // What the app is made of; see `plugins`.
    .add_plugins(plugins::VersePlugins);

    // The analysis worker is the app's one long-lived owner of heavyweight
    // state (the CLAP, demucs, and recipe models), so it mounts in a scope
    // whose disposer joins it rather than abandoning it. Mounted here rather
    // than from a plugin because plugins are add-only: runtime lifecycle needs
    // something that can be unwound. See `scope`.
    let deps = analysis::WorkerDeps::from_world(app.world());
    analysis::mount_analysis(app.world_mut(), deps);

    // `VERSE_PACK=1 cargo run` mounts a demo world pack, exercising runtime
    // mount/withdraw against the live registry. See `mood_pack`.
    mood_pack::mount_demo_pack_if_requested(app.world_mut());

    // `VERSE_SCOPES=1 cargo run` lists what is mounted and what each unit
    // will unwind, so a registration that fails to dispose is findable.
    if std::env::var("VERSE_SCOPES").is_ok() {
        scope::describe(app.world());
    }

    app.run();
}

/// Walk the app through its screens for the smoke test, then exit cleanly.
/// Set `VERSE_SHOT=<dir>` to also save PNG screenshots of each surface.
#[allow(clippy::too_many_arguments)]
fn smoke_drive(
    time: Res<Time>,
    state: Res<State<AppState>>,
    mut next: ResMut<NextState<AppState>>,
    mut queue_open: ResMut<QueueOpen>,
    mut paused: ResMut<Paused>,
    mut stack: ResMut<OverlayStack>,
    mut comfort: ResMut<Comfort>,
    mut photo: ResMut<Photo>,
    mut onboarding: ResMut<Onboarding>,
    mut seek: ResMut<SeekRequest>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    mut phase: Local<u8>,
) {
    let t = time.elapsed_secs();
    let dir = std::env::var("VERSE_SHOT").ok();

    // Single settled capture of the loaded world (avoids the multi-screenshot
    // readback flakiness): straight to the world, one shot at 6s, exit at 8s.
    if std::env::var("VERSE_ONESHOT").is_ok() {
        if t > 0.5 && *state.get() == AppState::FirstRun {
            next.set(AppState::InWorld);
        }
        if t > 4.0 && *phase == 0 {
            *phase = 1;
            // Seek probe: jumping near the end should trigger the crossfade
            // within a second (observable in the logs).
            seek.0 = Some(8.0);
        }
        if t > 6.0 && *phase == 1 {
            *phase = 2;
            shot(&mut commands, &dir, "verse-world.png");
        }
        if t > 8.0 {
            exit.write(AppExit::Success);
        }
        return;
    }

    // --- Onboarding walk (FirstRun) ---
    if t > 0.4 && *phase == 0 {
        *phase = 1;
        shot(&mut commands, &dir, "verse-onboard-1.png");
    }
    if t > 0.8 {
        onboarding.step = 1;
    }
    if t > 1.2 && *phase == 1 {
        *phase = 2;
        shot(&mut commands, &dir, "verse-onboard-2.png");
    }
    if t > 1.6 {
        onboarding.step = 2;
    }
    if t > 2.0 && *phase == 2 {
        *phase = 3;
        shot(&mut commands, &dir, "verse-onboard-3.png");
    }
    if t > 2.4 && *state.get() == AppState::FirstRun {
        next.set(AppState::InWorld); // spawns the HUD
    }

    // --- In-world walk ---
    if t > 3.0 && *phase == 3 {
        *phase = 4;
        shot(&mut commands, &dir, "verse-hud.png");
    }
    if t > 3.3 {
        stack.open(Overlay::Library); // spawns the library
    }
    if t > 3.9 && *phase == 4 {
        *phase = 5;
        shot(&mut commands, &dir, "verse-library.png");
        stack.close(Overlay::Library);
    }
    if t > 4.3 {
        queue_open.0 = true; // spawns the queue panel
    }
    if t > 4.7 {
        paused.0 = true; // spawns the pause overlay
    }
    if t > 5.3 && *phase == 5 {
        *phase = 6;
        shot(&mut commands, &dir, "verse-overlays.png");
    }
    if t > 5.7 {
        queue_open.0 = false;
        paused.0 = false; // Settings lifts pause (as the real button does)
        stack.open(Overlay::Settings);
        comfort.reduce_flashing = true; // show a toggle in the "on" state
    }
    if t > 6.3 && *phase == 6 {
        *phase = 7;
        shot(&mut commands, &dir, "verse-settings.png");
    }
    if t > 6.7 {
        stack.open(Overlay::Credits);
    }
    if t > 7.3 && *phase == 7 {
        *phase = 8;
        shot(&mut commands, &dir, "verse-credits.png");
    }
    if t > 7.5 {
        stack.clear();
    }
    // Request the photo only once the chrome has been closed for a while, so
    // the capture lands on a stable, clean frame.
    if t > 8.3 && *phase == 8 {
        *phase = 9;
        photo.request();
    }
    if t > 9.3 {
        exit.write(AppExit::Success);
    }
}

/// Save a screenshot to `<dir>/<name>` when the smoke test requests one.
fn shot(commands: &mut Commands, dir: &Option<String>, name: &str) {
    if let Some(d) = dir {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(format!("{d}/{name}")));
    }
}

fn not_paused(paused: Res<Paused>) -> bool {
    !paused.0
}

/// Dev/smoke hook: `VERSE_IMPORT=<dir>` imports a folder at startup,
/// skipping the folder picker.
fn auto_import(mut import: ResMut<audio::ImportState>) {
    if let Ok(dir) = std::env::var("VERSE_IMPORT") {
        audio::start_import(std::path::PathBuf::from(dir), &mut import);
        return;
    }
    // No override → load the bundled CC0 starter pack (if present) so the user
    // has something to play immediately, before importing their own folder.
    // Flows through the same scan as user music (tags → analysis → worlds).
    let starter = world_assets::asset_root().join("music");
    if starter.join("music.json").exists() {
        audio::start_import(starter, &mut import);
    }
}

/// Load persisted settings at startup and override the default resources
/// (Comfort/Volume/WorldIntensity/CameraMode/Onboarding) with the saved
/// values. Also stores the loaded [`settings::AppSettings`] as a resource so
/// the debounced saver can diff against it, and re-imports the last folder
/// (unless `VERSE_IMPORT` is set, which takes precedence).
#[allow(clippy::too_many_arguments)]
fn apply_loaded_settings(
    mut comfort: ResMut<Comfort>,
    mut volume: ResMut<Volume>,
    mut intensity: ResMut<WorldIntensity>,
    mut camera: ResMut<CameraMode>,
    mut onboarding: ResMut<Onboarding>,
    mut import: ResMut<audio::ImportState>,
    mut commands: Commands,
) {
    let loaded = settings::load().unwrap_or_default();
    *comfort = loaded.comfort;
    volume.0 = loaded.volume;
    intensity.0 = loaded.world_intensity;
    *camera = loaded.camera_mode;
    // A returning user who finished onboarding skips it; a fresh user (no file)
    // starts at step 0.
    if loaded.onboarding_done {
        onboarding.step = u8::MAX;
    }
    // Re-import the most recently opened folder (env override wins).
    if std::env::var("VERSE_IMPORT").is_err()
        && let Some(folder) = loaded.last_folders.last()
        && folder.exists()
    {
        audio::start_import(folder.clone(), &mut import);
    }
    commands.insert_resource(SettingsSnapshot(loaded));
}

/// The last-saved settings, held as a resource so [`save_settings_debounced`]
/// can diff the live resource values against it and write only on change.
#[derive(Resource)]
struct SettingsSnapshot(settings::AppSettings);

/// Debounced settings writer: compares the current Comfort/Volume/etc. against
/// the snapshot, and if any value changed, writes once per second at most.
/// Cheap when idle (a few field compares; no disk I/O).
#[allow(clippy::too_many_arguments)]
fn save_settings_debounced(
    comfort: Res<Comfort>,
    volume: Res<Volume>,
    intensity: Res<WorldIntensity>,
    camera: Res<CameraMode>,
    onboarding: Res<Onboarding>,
    time: Res<Time>,
    mut snapshot: ResMut<SettingsSnapshot>,
    mut due_at: Local<Option<f64>>,
) {
    let now = time.elapsed_secs_f64();
    // Pending folder imports (queued by the UI handlers) also count as a change
    // and need to be folded in when the write fires.
    let pending = settings::drain_pending_folders();
    // Coalesce bursts (slider drag, rapid toggles): schedule a write 1s after
    // the first change, keep pushing it out while changes continue.
    let changed = snapshot.0.comfort != *comfort
        || snapshot.0.volume != volume.0
        || snapshot.0.world_intensity != intensity.0
        || snapshot.0.camera_mode != *camera
        || snapshot.0.onboarding_done != (onboarding.step == u8::MAX)
        || !pending.is_empty();
    if changed && due_at.is_none() {
        *due_at = Some(now + 1.0);
    }
    let Some(due) = *due_at else { return };
    if now < due {
        return;
    }
    *due_at = None;
    // Snapshot current values, fold in any newly-imported folders, and persist.
    snapshot.0.comfort = *comfort;
    snapshot.0.volume = volume.0;
    snapshot.0.world_intensity = intensity.0;
    snapshot.0.camera_mode = *camera;
    snapshot.0.onboarding_done = onboarding.step == u8::MAX;
    for folder in &pending {
        snapshot.0.last_folders.retain(|p| p != folder);
        snapshot.0.last_folders.push(folder.clone());
        // Bound the history (keeps the file tidy across many imports).
        if snapshot.0.last_folders.len() > 8 {
            snapshot.0.last_folders.remove(0);
        }
    }
    settings::save(&snapshot.0);
}

/// Photo mode: force the HUD hidden, then capture a clean screenshot to
/// `verse-photos/`. Runs for a few frames so the chrome fully fades first.
fn photo_capture(
    mut photo: ResMut<Photo>,
    mut activity: ResMut<HudActivity>,
    mut commands: Commands,
) {
    let Some(n) = photo.pending else {
        return;
    };
    // Snap the HUD fully hidden for a clean frame.
    activity.idle = 100.0;
    activity.force_visible = false;
    activity.full = 0.0;
    activity.minimal = 0.0;

    if n == 0 {
        let path = photo.path.get_or_insert_with(photo_path).clone();
        info!("LocalGPT Verse photo saved to {path}");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path))
            .observe(photo_black_check);
        photo.pending = None;
    } else {
        photo.pending = Some(n - 1);
    }
}

/// GPU readback sometimes hands back an all-black frame (ARCHITECTURE R8).
/// Sample the captured image; if it's black, re-request the photo (bounded).
fn photo_black_check(event: On<ScreenshotCaptured>, mut photo: ResMut<Photo>) {
    let Some(data) = event.image.data.as_ref() else {
        return;
    };
    // Sample every ~97th RGBA pixel; ignore alpha (opaque even on black frames).
    let black = data
        .as_chunks::<4>()
        .0
        .iter()
        .step_by(97)
        .all(|px| px[0] < 8 && px[1] < 8 && px[2] < 8);
    if black && photo.retries < 3 {
        photo.retries += 1;
        photo.pending = Some(8);
        warn!("Photo readback was black — retrying ({}/3)", photo.retries);
    }
}

/// A timestamped path under `verse-photos/` (created on demand).
fn photo_path() -> String {
    let dir = "verse-photos";
    let _ = std::fs::create_dir_all(dir);
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{dir}/verse-{ms}.png")
}

/// Ease the world timescale toward its target (time-dilation while frozen).
///
/// Keyed on the transport itself (`playback.playing`) so the HUD play/pause
/// button — which pauses without opening the menu — still dilates time. The
/// menu-pause (Esc) also clears `playing`, so both routes freeze the world.
fn ease_world_clock(time: Res<Time>, playback: Res<Playback>, mut clock: ResMut<WorldClock>) {
    let frozen = !playback.playing;
    let target = if frozen { 0.05 } else { 1.0 };
    let rate = if frozen { 6.0 } else { 4.0 };
    clock.speed += (target - clock.speed) * (time.delta_secs() * rate).min(1.0);
}

/// First-run: any confirm key drops you into the world.
fn input_first_run(keys: Res<ButtonInput<KeyCode>>, mut next: ResMut<NextState<AppState>>) {
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
        next.set(AppState::InWorld);
    }
}

/// In-world keybindings (Esc pause · Tab queue · H hide · E pulse · F mode).
#[allow(clippy::too_many_arguments)]
fn input_in_world(
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<bevy::input::mouse::AccumulatedMouseMotion>,
    window_q: Query<&bevy::window::CursorOptions, With<bevy::window::PrimaryWindow>>,
    mut paused: ResMut<Paused>,
    mut queue_open: ResMut<QueueOpen>,
    mut stack: ResMut<OverlayStack>,
    mut photo: ResMut<Photo>,
    mut mode: ResMut<CameraMode>,
    mut activity: ResMut<HudActivity>,
    mut beat: ResMut<Beat>,
    mut playback: ResMut<Playback>,
    mut theme: ResMut<Theme>,
    mut intensity: ResMut<WorldIntensity>,
    mut seek: ResMut<SeekRequest>,
    mut volume: ResMut<Volume>,
) {
    // Mouse movement wakes the HUD only while the pointer is free — when
    // Explore holds the lock, the mouse is the camera, not a HUD affordance.
    let pointer_locked = window_q
        .single()
        .map(|c| c.grab_mode == bevy::window::CursorGrabMode::Locked)
        .unwrap_or(false);
    let mut wake =
        keys.get_just_pressed().len() > 0 || (motion.delta != Vec2::ZERO && !pointer_locked);

    // Volume: -/= (and numpad) nudge the fader; the apply system tweens it.
    if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        volume.0 = (volume.0 - 0.05).max(0.0);
    }
    if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        volume.0 = (volume.0 + 0.05).min(1.0);
    }

    if keys.just_pressed(KeyCode::Escape) {
        // Close the topmost overlay first; only pause when nothing else is up.
        if stack.pop().is_none() {
            paused.0 = !paused.0;
            playback.playing = !paused.0;
        }
    }
    if keys.just_pressed(KeyCode::Tab) {
        queue_open.0 = !queue_open.0;
    }
    if keys.just_pressed(KeyCode::KeyL) {
        stack.toggle(Overlay::Library);
    }
    if keys.just_pressed(KeyCode::KeyH) {
        // Jump straight to Hidden.
        activity.idle = 100.0;
        activity.force_visible = false;
        wake = false;
    }
    if keys.just_pressed(KeyCode::KeyE) {
        beat.pulse = 1.0;
    }
    if keys.just_pressed(KeyCode::KeyN) || keys.just_pressed(KeyCode::MediaTrackNext) {
        // Next track (the audio player follows `current` and fades over).
        match playback.advance() {
            Some(mood) => theme.mood = mood,
            None => playback.playing = false, // repeat-off: end of queue
        }
    }
    if keys.just_pressed(KeyCode::KeyB) || keys.just_pressed(KeyCode::MediaTrackPrevious) {
        // Previous: restart if >3s in (the usual convention), else go back.
        if playback.elapsed > 3.0 {
            seek.0 = Some(0.0);
        } else {
            let mood = playback.previous();
            theme.mood = mood;
        }
    }
    if keys.just_pressed(KeyCode::MediaPlayPause) {
        // Transport freeze in place — no menu (Esc opens the pause menu).
        playback.playing = !playback.playing;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        *mode = match *mode {
            CameraMode::Explore => CameraMode::Drift,
            CameraMode::Drift => CameraMode::Explore,
        };
    }
    if keys.just_pressed(KeyCode::KeyP) {
        // Photo mode — clear the chrome and capture the world.
        photo.request();
        paused.0 = false;
        playback.playing = true;
        queue_open.0 = false;
        stack.clear();
    }
    if paused.0 {
        if keys.just_pressed(KeyCode::ArrowLeft) {
            intensity.0 = (intensity.0 - 0.05).max(0.0);
        }
        if keys.just_pressed(KeyCode::ArrowRight) {
            intensity.0 = (intensity.0 + 0.05).min(1.0);
        }
    } else {
        // Unpaused, the arrows scrub the song (±5s).
        if keys.just_pressed(KeyCode::ArrowLeft) {
            seek.0 = Some((playback.elapsed - 5.0).max(0.0));
        }
        if keys.just_pressed(KeyCode::ArrowRight) {
            seek.0 = Some(playback.elapsed + 5.0);
        }
    }

    if wake {
        activity.wake();
    }
}
