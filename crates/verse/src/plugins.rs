//! Composition root — what the app is made of.
//!
//! Every unit of the app registers itself here rather than in one flat builder
//! chain in `main`. Two things this buys, beyond readability:
//!
//! - **Conditional composition is trivial.** A `#[cfg]` on a plugin is just a
//!   `#[cfg]`; a `#[cfg]` on a chained `.add_systems()` call breaks its
//!   receiver, which is why the optional tiers used to be bolted on after the
//!   chain instead of sitting with the rest of the app.
//! - **Ownership is legible.** Each plugin lists the resources and systems one
//!   subsystem owns, so "what would I have to undo to remove this?" has an
//!   answer you can read.
//!
//! # Plugins are not scopes
//!
//! These are compile-time composition: `App::add_plugins` is add-only and Bevy
//! has no plugin or system removal. Runtime lifecycle — the things that must be
//! releasable — lives in [`crate::scope`] instead, and the two are deliberately
//! not mixed. [`crate::analysis::mount_analysis`] is mounted from `main` after
//! these plugins are built, because it owns a worker thread that has to be
//! joinable; a plugin could not give it that.
//!
//! # Splitting the in-world systems is safe
//!
//! The systems that ran in one large tuple gated on `AppState::InWorld` were
//! never `.chain()`ed at the top level — the nesting only worked around Bevy's
//! 20-element tuple cap. They are unordered with respect to each other, so
//! distributing them across plugins preserves the previous behavior. The tuples
//! that *are* chained (the button dispatcher, the palette/animate pair, the
//! audio sync sequence, the transport pipeline, and the folder-pick/import
//! pair) each stay whole inside one plugin.

use bevy::prelude::*;

use crate::hud::HudActivity;
use crate::playback::{Beat, Playback};
use crate::theme::{Fonts, Theme};
use crate::{
    AppState, AudioActive, CameraMode, Comfort, Onboarding, OverlayStack, Paused, Photo, QueueOpen,
    SeekRequest, Volume, WorldClock, WorldIntensity,
};
use crate::{analysis, audio, hud, overlays, playback, recipe, theme, world, world_assets};

/// Everything LocalGPT Verse adds on top of `DefaultPlugins`.
pub struct VersePlugins;

impl Plugin for VersePlugins {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            AppStatePlugin,
            ThemePlugin,
            SettingsPlugin,
            WorldPlugin,
            WorldAssetsPlugin,
            AudioPlugin,
            PlaybackPlugin,
            HudPlugin,
            OverlaysPlugin,
            DiagnosticsPlugin,
            // `VERSE_EXPORT_WORLD=<dir>`: each track's world in the shared
            // LocalGPT world format. Dormant without the variable.
            crate::world_manifest::WorldExportPlugin,
        ));

        #[cfg(feature = "llm")]
        app.add_plugins(AgentPlugin);
    }
}

/// Top-level screen state, the cross-cutting resources `main` defines, and the
/// input and photo-mode systems that read them.
struct AppStatePlugin;

impl Plugin for AppStatePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>()
            .init_resource::<CameraMode>()
            .init_resource::<Paused>()
            .init_resource::<Photo>()
            .add_systems(
                Update,
                crate::input_first_run.run_if(in_state(AppState::FirstRun)),
            )
            .add_systems(
                Update,
                (crate::input_in_world, crate::photo_capture).run_if(in_state(AppState::InWorld)),
            );
    }
}

/// Palette, type roles, and the fonts every UI system reads.
struct ThemePlugin;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(theme::BASE))
            .init_resource::<Theme>();

        // Fonts must exist before any schedule runs: some `OnEnter` systems
        // read them, and the state machine's initial transition fires before a
        // `PreStartup` system would. `DefaultPlugins` is added before this
        // group, so the `AssetServer` is already available here.
        let asset_server = app.world().resource::<AssetServer>().clone();
        app.insert_resource(Fonts::load(&asset_server));
    }
}

/// Persisted user settings — loaded at startup, saved debounced.
struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Comfort>()
            .add_systems(Startup, crate::apply_loaded_settings)
            .add_systems(Update, crate::save_settings_debounced);
    }
}

/// The procedural backdrop: particles, palette wash, sway, and the cameras.
struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldIntensity>()
            .init_resource::<WorldClock>()
            .init_resource::<recipe::ActiveRecipe>()
            .init_resource::<world::PaletteWash>()
            .init_resource::<world::HeldRing>()
            .init_resource::<world::SectionFeel>()
            .init_resource::<world::ParticleField>()
            // Continuous timbre + mood blend — read by the palette wash.
            .init_resource::<analysis::Timbre>()
            .init_resource::<analysis::MoodBlend>()
            // The agent's background override (M7) — always present because
            // the palette wash reads it in every feature config.
            .init_resource::<crate::agent_types::EnvOverride>()
            .add_systems(Startup, world::setup_world)
            .add_systems(Update, (crate::ease_world_clock, world::spawn_particles))
            .add_systems(Update, world::ground_waves)
            // Choreography first (it may trigger a wash), then the wash writes
            // hues before the beat-glow rescale reads them.
            .add_systems(
                Update,
                (
                    world::sync_section_moment,
                    world::palette_wash,
                    world::animate_world,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                world::sync_held_ring.run_if(in_state(AppState::InWorld)),
            )
            .add_systems(
                Update,
                world::sync_world_title.run_if(in_state(AppState::InWorld)),
            )
            .add_systems(
                Update,
                world::update_cursor_grab.run_if(in_state(AppState::InWorld)),
            )
            .add_systems(
                Update,
                world::camera_control
                    .run_if(in_state(AppState::InWorld))
                    .run_if(crate::not_paused),
            );
    }
}

/// The manifest-driven glTF prop pack placed over the backdrop.
struct WorldAssetsPlugin;

impl Plugin for WorldAssetsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<world_assets::WorldAssets>()
            .init_resource::<world_assets::WorldLayout>()
            .init_resource::<world_assets::AssetEmbeddings>()
            .add_systems(Startup, world_assets::load_asset_manifest)
            .add_systems(
                Update,
                (
                    world_assets::populate_world_props,
                    world_assets::rise_props,
                    world_assets::animate_props,
                    world_assets::pulse_beacons,
                )
                    .run_if(in_state(AppState::InWorld)),
            );

        // M5→M6 (ml feature): lazily embed the manifest's asset texts with
        // the CLAP text tower so placement can weight by the track embedding.
        // Runs in every state (onboarding is idle time anyway), a few assets
        // per frame.
        #[cfg(feature = "ml")]
        app.init_resource::<world_assets::TextModelState>()
            .add_systems(Update, world_assets::sync_asset_embeddings);
    }
}

/// Real playback through kira: the library import, the transport sync, and the
/// live tap that feeds the beat.
struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioActive>()
            .init_resource::<SeekRequest>()
            .init_resource::<Volume>()
            .init_resource::<audio::AudioPlayer>()
            .init_resource::<audio::AudioTap>()
            .init_resource::<audio::ImportState>()
            .add_systems(Startup, (audio::init_audio, crate::auto_import))
            // The import poll runs everywhere (the scan can start during
            // onboarding). The folder pick is drained first so a freshly chosen
            // path kicks off the scan before `poll_import` runs in the same
            // frame.
            .add_systems(
                Update,
                (overlays::poll_folder_pick, audio::poll_import).chain(),
            )
            // The player syncs only in-world ("audio starts at 0s" on
            // materialize). Chained — each stage feeds the next within a frame.
            .add_systems(
                Update,
                (
                    audio::sync_track_playback,
                    audio::sync_pause,
                    audio::apply_seek,
                    audio::apply_volume,
                    audio::sync_clock,
                )
                    .chain()
                    .run_if(in_state(AppState::InWorld)),
            );
    }
}

/// Transport and beat. The analysis store itself is mounted separately — see
/// the module docs.
struct PlaybackPlugin;

impl Plugin for PlaybackPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Playback>()
            .init_resource::<Beat>()
            .init_resource::<playback::StemLevels>()
            // Ordered: analysis applies the grid/sections/mood on a track
            // change, the stem sampler reads its curves at the playhead, the
            // live tap feeds energy/onsets/bands, then advance decays the
            // pulse, derives phase, and (when simulated) moves the clock.
            .add_systems(
                Update,
                (
                    analysis::sync_analysis,
                    analysis::update_stem_levels,
                    audio::update_beat_from_tap,
                    playback::advance_playback,
                )
                    .chain()
                    .run_if(in_state(AppState::InWorld)),
            );
    }
}

/// The chrome: now-playing cluster, beat-reactive transport, and the
/// three-depth fade.
struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudActivity>()
            .add_systems(OnEnter(AppState::InWorld), hud::setup_hud)
            .add_systems(
                Update,
                (
                    hud::hud_depth,
                    hud::apply_hud_alpha,
                    hud::update_hud_accent,
                    hud::update_hud_content,
                    hud::update_mode_tabs,
                    hud::update_reticle,
                    hud::update_section_notches,
                    hud::update_transport,
                    hud::seek_strip_scrub,
                    hud::transport_clicks,
                    hud::mode_tab_clicks,
                    hud::chip_clicks,
                )
                    .run_if(in_state(AppState::InWorld)),
            );
    }
}

/// Onboarding, pause, queue, settings, credits, and library — plus the button
/// dispatcher every one of them consumes.
struct OverlaysPlugin;

impl Plugin for OverlaysPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QueueOpen>()
            .init_resource::<OverlayStack>()
            .init_resource::<Onboarding>()
            .init_resource::<overlays::LibraryFilter>()
            .init_resource::<overlays::FolderPickRx>()
            .add_message::<overlays::UiAction>()
            .add_systems(OnEnter(AppState::FirstRun), overlays::spawn_first_run)
            .add_systems(OnExit(AppState::FirstRun), overlays::despawn_first_run)
            // Button presses broadcast as messages; the focused handlers run
            // after the dispatcher within the frame.
            .add_systems(
                Update,
                (
                    overlays::dispatch_buttons,
                    (
                        overlays::onboarding_actions,
                        overlays::overlay_actions,
                        overlays::world_actions,
                        overlays::comfort_actions,
                        overlays::app_actions,
                        overlays::queue_actions,
                        overlays::library_actions,
                    ),
                )
                    .chain(),
            )
            .add_systems(Update, overlays::overlay_scroll)
            .add_systems(
                Update,
                overlays::refresh_onboarding.run_if(in_state(AppState::FirstRun)),
            )
            .add_systems(
                Update,
                (
                    overlays::sync_pause_overlay,
                    overlays::sync_queue_overlay,
                    overlays::sync_settings_overlay,
                    overlays::sync_credits_overlay,
                    overlays::sync_library_overlay,
                    overlays::update_intensity_knob,
                    overlays::update_comfort_toggles,
                )
                    .run_if(in_state(AppState::InWorld)),
            );
    }
}

/// Env-gated development surfaces: the smoke walkthrough and the perf stress
/// run. Both are no-ops unless their variable is set.
struct DiagnosticsPlugin;

impl Plugin for DiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        // `VERSE_SMOKE=1 cargo run` drives the app through every UI surface
        // (world → queue → pause) then exits — a headful boot check.
        if std::env::var("VERSE_SMOKE").is_ok() {
            app.add_systems(Update, crate::smoke_drive);
        }

        // `VERSE_STRESS=5000 cargo run` fills the world with prop instances
        // and logs frame rates, then exits after 30 s. Skips onboarding so the
        // measurement starts immediately (needs the asset pack).
        if let Some(stress) = world_assets::StressTest::from_env() {
            app.insert_resource(stress);
            app.insert_state(AppState::InWorld);
            app.add_systems(
                Update,
                (world_assets::stress_spawn, world_assets::stress_report)
                    .chain()
                    .run_if(in_state(AppState::InWorld)),
            );
        }
    }
}

/// M7 LLM scene-construction agent. Bevy owns the executor (channels + name
/// registries); the matching bridge is published as a resource so the analysis
/// worker can be handed it at construction.
#[cfg(feature = "llm")]
struct AgentPlugin;

#[cfg(feature = "llm")]
impl Plugin for AgentPlugin {
    fn build(&self, app: &mut App) {
        let (bridge, channels) = crate::agent::create_channels();
        app.insert_resource(crate::agent::AgentBridgeHandle(bridge));
        app.insert_resource(crate::agent::AgentExecutor::new(channels));
        // Ordered: live-issued commands drain first, the cached-build replay
        // (track change) clears/rebuilds on top, and the scope sync reveals
        // the current track's scene last — so freshly spawned or replayed
        // entities are scoped within the same frame.
        app.add_systems(
            Update,
            (
                crate::agent::drain_agent_commands,
                crate::agent::replay_cached_build,
                crate::agent::sync_agent_scene_scope,
            )
                .chain(),
        );
    }
}
