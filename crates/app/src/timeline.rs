//! The timeline's half in the app: world time — the shots, the song and
//! what the song moves — on one clock, which the page draws across the
//! window and plays (`ui/src/timeline`, drawn first in `ui/design/`).
//!
//! **The clock** is the song's position when the world ships its audio and
//! a device plays it; otherwise the app keeps it, advancing while it plays.
//! Either way it is the time the world is shown at: the modulations read
//! their signals there — held while paused, so moving the playhead shows
//! the world at that moment — and while a shot is looked through, moving
//! the playhead cuts to the shot under it.
//!
//! **The tracks** are a view of the world's document, never a copy of it:
//! the live canvas's document on screen, or the world the scene loaded from
//! its folder, rebuilt when either changes ([`TimelineView`]). A trim or a
//! move goes back as the [`ShotOps`] a reorder sends — one entry in the
//! history per edit, through the authority the agents use, undone like any
//! other.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use base64::Engine as _;
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::renderer::RenderDevice;
use localgpt_gen::gen3d::audio::{AudioEngine, sync_soundtrack_clock};
use localgpt_gen::gen3d::behaviors::{BehaviorInstance, EntityBehaviors};
use localgpt_gen::gen3d::canvas_edit::PersonIntent;
use localgpt_gen::gen3d::live::{LiveReview, PendingReview, ReviewDecision};
use localgpt_gen::gen3d::plugin::CurrentWorld;
use localgpt_gen::gen3d::registry::GenEntity;
use localgpt_gen::gen3d::shots::{
    LiveShots, LookThrough, ShotOps, ShotThumbs, ShotView, ThumbsDirty,
};
use localgpt_previs::ShotRow;
use localgpt_world_bevy::modulation::{Modulated, Soundtrack};
use localgpt_world_types as wt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::peaks;

/// Modulation signals are sampled this often for the page, a second.
const SIGNAL_RATE: f32 = 16.0;

pub struct TimelinePlugin {
    /// A live canvas's package, when that is what is open: its document is
    /// the timeline's, and a shot edit goes to its authority.
    pub canvas: Option<PathBuf>,
}

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Source {
            canvas: self.canvas.clone(),
            read: None,
            poll: Timer::from_seconds(0.5, TimerMode::Repeating),
        })
        .init_resource::<Timeline>()
        .init_resource::<WorldClock>()
        .init_resource::<Thumbs>()
        .add_message::<TimelineAsk>()
        .add_systems(
            Update,
            (
                read_world.in_set(WorldRead),
                answer_asks,
                hide_tracks,
                drive_clock,
                follow_cut,
                capture_thumbs,
            )
                .chain()
                .after(sync_soundtrack_clock),
        );
    }
}

/// Where the world's document is read, each frame: what reads it from the
/// timeline goes after.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldRead;

// ---------------------------------------------------------------------------
// What the page draws
// ---------------------------------------------------------------------------

/// The world's clock and its tracks, in seconds — the shapes
/// `ui/src/timeline/model.ts` names.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TimelineView {
    /// `None`: the world keeps no time — no soundtrack, no shots, no tour.
    pub clock: Option<ClockView>,
    pub tracks: Vec<TrackView>,
    /// The signals the modulations read, sampled on the clock, by name.
    /// Many modulations read one signal, so each goes once.
    pub signals: BTreeMap<String, Signal>,
    /// Shots can be trimmed and moved: a live canvas, whose authority takes
    /// the edit.
    pub editable: bool,
    /// An agent's change, waiting for the person.
    pub proposal: Option<ProposalView>,
}

/// The soundtrack's length and beat grid, or a shot list's or a tour's
/// running time when there is no song.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClockView {
    pub duration: f32,
    /// `0` when there is no beat grid.
    pub bpm: f32,
    pub beat_offset: f32,
    /// Section starts as fractions of the duration, from `0`.
    pub sections: Vec<f32>,
}

/// Samples of a signal in `0..=1`, `rate` a second.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Signal {
    pub values: Vec<f32>,
    pub rate: f32,
}

/// A track. The page adds what it alone knows: selection, its own toggles'
/// state, and the samples each modulation's `source` names.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TrackView {
    Shots {
        id: String,
        name: String,
        shots: Vec<Clip>,
        toggles: Vec<&'static str>,
    },
    Camera {
        id: String,
        name: String,
        tour: String,
        keys: Vec<CameraKey>,
        toggles: Vec<&'static str>,
    },
    Soundtrack {
        id: String,
        name: String,
        title: String,
        artist: String,
        licence: String,
        peaks: Signal,
        toggles: Vec<&'static str>,
    },
    Stem {
        id: String,
        name: String,
        envelope: Signal,
    },
    Modulation {
        id: String,
        /// The entity it drives.
        name: String,
        target: &'static str,
        signal: String,
        /// The key of its samples in [`TimelineView::signals`].
        source: String,
        range: [f32; 2],
        toggles: Vec<&'static str>,
    },
    Behavior {
        id: String,
        name: String,
        what: &'static str,
        period: f32,
        from: f32,
        to: f32,
        toggles: Vec<&'static str>,
    },
    Ambience {
        id: String,
        name: String,
        from: f32,
        to: f32,
        fade: [f32; 2],
        toggles: Vec<&'static str>,
    },
}

/// A shot on its track: a camera entity with `ext-cinematography`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Clip {
    /// The camera entity's id.
    pub id: String,
    pub name: String,
    pub size: String,
    /// Focal length, mm.
    pub lens: f32,
    /// `in_s` and `out_s`.
    pub from: f32,
    pub to: f32,
}

/// A tour waypoint at the time the camera reaches it, held for `hold`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CameraKey {
    pub t: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProposalView {
    pub author: String,
    /// What it changes, in the history's words.
    pub what: String,
    /// The shots it would move or add, where it would put them.
    pub shots: Vec<Clip>,
}

/// The timeline view of `world`. `peaks` is its song's waveform once
/// decoded; until then the song is drawn by its energy curve.
pub fn view_of(
    world: &wt::WorldManifest,
    peaks: Option<&[f32]>,
    editable: bool,
    review: Option<&PendingReview>,
) -> TimelineView {
    let rows = localgpt_previs::shots::shot_list(world);
    let Some(clock) = clock_of(world, &rows) else {
        return TimelineView {
            editable,
            ..TimelineView::default()
        };
    };
    let duration = clock.duration;
    let mut tracks = Vec::new();
    let mut signals = BTreeMap::new();

    if !rows.is_empty() {
        tracks.push(TrackView::Shots {
            id: "shots".into(),
            name: "Shots".into(),
            shots: rows.iter().map(clip).collect(),
            toggles: if editable { vec!["lock"] } else { Vec::new() },
        });
    }
    for tour in world.tours.iter().filter(|tour| tour.waypoints.len() > 1) {
        tracks.push(TrackView::Camera {
            id: format!("tour:{}", tour.name),
            name: "Camera".into(),
            tour: tour.name.clone(),
            keys: tour_keys(tour),
            toggles: if editable { vec!["lock"] } else { Vec::new() },
        });
    }
    if let Some(song) = song_of(world) {
        tracks.push(TrackView::Soundtrack {
            id: "soundtrack".into(),
            name: "Soundtrack".into(),
            title: song.title.clone().unwrap_or_else(|| "Untitled".into()),
            artist: song.artist.clone().unwrap_or_default(),
            licence: song.license.clone().unwrap_or_default(),
            peaks: match peaks {
                Some(peaks) => Signal {
                    values: peaks.to_vec(),
                    rate: peaks::RATE,
                },
                None => Signal {
                    values: song.energy.iter().map(|&v| round3(v)).collect(),
                    rate: 1.0,
                },
            },
            toggles: if song.path.is_some() {
                vec!["mute", "solo"]
            } else {
                Vec::new()
            },
        });
        if let Some(stems) = &song.stems {
            for (key, name, curve) in [
                ("drums", "Drums", &stems.drums),
                ("bass", "Bass", &stems.bass),
                ("vocals", "Vocals", &stems.vocals),
                ("other", "Other", &stems.other),
            ] {
                if !curve.is_empty() {
                    tracks.push(TrackView::Stem {
                        id: format!("stem:{key}"),
                        name: name.into(),
                        envelope: Signal {
                            values: curve.iter().map(|&v| round3(v)).collect(),
                            rate: 1.0,
                        },
                    });
                }
            }
        }
    }

    // What the song moves: each modulation reads its signal at the clock,
    // through the same reading the renderer uses (live bands aside). Things
    // moved alike — every boulder breathing with the energy over one range
    // — are one track, named for the first of them.
    let performing = Soundtrack {
        def: world.soundtrack.clone(),
        ..Soundtrack::default()
    };
    let mut alike: Vec<(TrackView, usize)> = Vec::new();
    let mut add = |track: TrackView| {
        let id = track_id(&track).to_string();
        match alike.iter_mut().find(|(other, _)| track_id(other) == id) {
            Some((_, count)) => *count += 1,
            None => alike.push((track, 1)),
        }
    };
    for entity in &world.entities {
        for def in entity.modulations.iter().filter(|def| def.is_valid()) {
            let source = signal_key(&def.signal);
            signals
                .entry(source.clone())
                .or_insert_with(|| sample(&performing, &def.signal, duration));
            let track = TrackView::Modulation {
                id: modulation_track(def),
                name: entity.name.as_str().to_string(),
                target: target_label(def.target),
                signal: signal_label(&def.signal),
                source,
                range: def.range,
                toggles: vec!["hide"],
            };
            add(track);
        }
    }
    for entity in &world.entities {
        for behavior in &entity.behaviors {
            if let Some((what, period)) = cycle(behavior) {
                let track = TrackView::Behavior {
                    id: behavior_track(behavior).unwrap_or_default(),
                    name: entity.name.as_str().to_string(),
                    what,
                    period: round2(period),
                    from: 0.0,
                    to: duration,
                    toggles: vec!["hide"],
                };
                add(track);
            }
        }
    }
    for (mut track, count) in alike {
        if let (TrackView::Modulation { name, .. } | TrackView::Behavior { name, .. }, 2..) =
            (&mut track, count)
        {
            *name = format!("{name} +{}", count - 1);
        }
        tracks.push(track);
    }
    for name in heard_names(world) {
        tracks.push(TrackView::Ambience {
            id: format!("ambience:{name}"),
            name,
            from: 0.0,
            to: duration,
            fade: [0.0, 0.0],
            toggles: vec!["mute", "solo"],
        });
    }

    let proposal = review.map(|review| ProposalView {
        author: review.author.clone(),
        what: review.changes.join(" · "),
        // The shots it moves or adds; one it leaves alone isn't drawn twice.
        shots: review
            .shots
            .iter()
            .filter(|row| {
                !rows.iter().any(|now| {
                    now.entity_id == row.entity_id
                        && (now.in_s - row.in_s).abs() < 0.005
                        && (now.out_s - row.out_s).abs() < 0.005
                })
            })
            .map(clip)
            .collect(),
    });

    TimelineView {
        clock: Some(clock),
        tracks,
        signals,
        editable,
        proposal,
    }
}

/// The track a modulation is drawn on: things moved alike share one.
fn modulation_track(def: &wt::ModulationDef) -> String {
    let (target, source) = (target_label(def.target), signal_key(&def.signal));
    format!("mod:{target}:{source}:{}:{}", def.range[0], def.range[1])
}

/// The track a behavior is drawn on, when it has a cycle to draw.
fn behavior_track(behavior: &wt::BehaviorDef) -> Option<String> {
    let (what, period) = cycle(behavior)?;
    Some(format!("behavior:{what}:{}", round2(period)))
}

fn track_id(track: &TrackView) -> &str {
    match track {
        TrackView::Shots { id, .. }
        | TrackView::Camera { id, .. }
        | TrackView::Soundtrack { id, .. }
        | TrackView::Stem { id, .. }
        | TrackView::Modulation { id, .. }
        | TrackView::Behavior { id, .. }
        | TrackView::Ambience { id, .. } => id,
    }
}

fn song_of(world: &wt::WorldManifest) -> Option<&wt::SoundtrackDef> {
    world
        .soundtrack
        .as_ref()
        .filter(|song| song.duration.is_finite() && song.duration > 0.0)
}

fn clock_of(world: &wt::WorldManifest, rows: &[ShotRow]) -> Option<ClockView> {
    if let Some(song) = song_of(world) {
        return Some(ClockView {
            duration: song.duration,
            bpm: if song.bpm.is_finite() {
                song.bpm.max(0.0)
            } else {
                0.0
            },
            beat_offset: song.beat_offset,
            sections: if song.sections.is_empty() {
                vec![0.0]
            } else {
                song.sections.clone()
            },
        });
    }
    // No song: the shot list's running time, or the longest tour's.
    let shots = rows.iter().map(|row| row.out_s as f32).fold(0.0, f32::max);
    let tours = world
        .tours
        .iter()
        .filter_map(|tour| tour_keys(tour).last().map(|k| k.t + k.hold.unwrap_or(0.0)))
        .fold(0.0, f32::max);
    let end = shots.max(tours);
    (end > 0.0).then(|| ClockView {
        duration: end,
        bpm: 0.0,
        beat_offset: 0.0,
        sections: vec![0.0],
    })
}

fn clip(row: &ShotRow) -> Clip {
    Clip {
        id: row.entity_id.to_string(),
        name: row.shot.clone(),
        size: row.size.clone(),
        lens: round2(row.focal_length_mm as f32),
        from: row.in_s as f32,
        to: row.out_s as f32,
    }
}

/// When the camera reaches each stop: the travel at the tour's speed, the
/// stop's pause after it, and no travel at all between teleports.
fn tour_keys(tour: &wt::TourDef) -> Vec<CameraKey> {
    let speed = if tour.speed > 0.0 { tour.speed } else { 1.0 };
    let mut t = 0.0f32;
    let mut keys = Vec::with_capacity(tour.waypoints.len());
    for (i, stop) in tour.waypoints.iter().enumerate() {
        if i > 0 {
            let before = &tour.waypoints[i - 1];
            t += before.pause_duration.max(0.0);
            if !matches!(tour.mode, wt::TourMode::Teleport) {
                let d: f32 = (0..3)
                    .map(|k| (stop.position[k] - before.position[k]).powi(2))
                    .sum::<f32>()
                    .sqrt();
                t += d / speed;
            }
        }
        keys.push(CameraKey {
            t: round2(t),
            hold: (stop.pause_duration > 0.0).then_some(stop.pause_duration),
        });
    }
    keys
}

/// A behavior's name and how long one cycle of it takes; `None` for one
/// that has no cycle (look-at).
fn cycle(behavior: &wt::BehaviorDef) -> Option<(&'static str, f32)> {
    let per = |rate: f32, of: f32| (rate.abs() > 1e-6).then(|| of / rate.abs());
    match behavior {
        wt::BehaviorDef::Orbit { speed, .. } => per(*speed, 360.0).map(|p| ("orbit", p)),
        wt::BehaviorDef::Spin { speed, .. } => per(*speed, 360.0).map(|p| ("spin", p)),
        wt::BehaviorDef::Bob { frequency, .. } => per(*frequency, 1.0).map(|p| ("bob", p)),
        wt::BehaviorDef::Pulse { frequency, .. } => per(*frequency, 1.0).map(|p| ("pulse", p)),
        wt::BehaviorDef::PathFollow {
            waypoints,
            speed,
            mode,
            ..
        } => {
            let leg = |a: &[f32; 3], b: &[f32; 3]| {
                ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
            };
            let mut length: f32 = waypoints.windows(2).map(|w| leg(&w[0], &w[1])).sum();
            match mode {
                wt::PathMode::Loop => {
                    if let (Some(first), Some(last)) = (waypoints.first(), waypoints.last()) {
                        length += leg(last, first);
                    }
                }
                wt::PathMode::PingPong => length *= 2.0,
                wt::PathMode::Once => {}
            }
            per(*speed, length)
                .filter(|p| *p > 0.0)
                .map(|p| ("path", p))
        }
        wt::BehaviorDef::Bounce {
            height, gravity, ..
        } => (*height > 0.0 && *gravity > 0.0)
            .then(|| ("bounce", 2.0 * (2.0 * height / gravity).sqrt())),
        wt::BehaviorDef::LookAt { .. } => None,
    }
}

fn signal_key(signal: &wt::SignalSource) -> String {
    match signal {
        wt::SignalSource::Energy => "energy".into(),
        wt::SignalSource::Beat => "beat".into(),
        wt::SignalSource::Bass => "bass".into(),
        wt::SignalSource::Highs => "highs".into(),
        wt::SignalSource::Stem(stem) => format!("stem:{}", stem_name(*stem)),
        wt::SignalSource::Oscillator { frequency } => format!("sine:{frequency}"),
        wt::SignalSource::Constant(value) => format!("constant:{value}"),
    }
}

fn signal_label(signal: &wt::SignalSource) -> String {
    match signal {
        wt::SignalSource::Stem(stem) => format!("{} stem", stem_name(*stem)),
        wt::SignalSource::Oscillator { frequency } => format!("sine {frequency} Hz"),
        wt::SignalSource::Constant(value) => format!("constant {value}"),
        other => signal_key(other),
    }
}

fn stem_name(stem: wt::StemKind) -> &'static str {
    match stem {
        wt::StemKind::Drums => "drums",
        wt::StemKind::Bass => "bass",
        wt::StemKind::Vocals => "vocals",
        wt::StemKind::Other => "other",
    }
}

fn target_label(target: wt::ModulationTarget) -> &'static str {
    match target {
        wt::ModulationTarget::Scale => "scale",
        wt::ModulationTarget::OffsetY => "height",
        wt::ModulationTarget::Emissive => "emissive",
        wt::ModulationTarget::LightIntensity => "light",
        wt::ModulationTarget::Opacity => "opacity",
    }
}

/// A signal sampled over the clock, as the renderer reads it.
fn sample(performing: &Soundtrack, signal: &wt::SignalSource, duration: f32) -> Signal {
    let n = (duration * SIGNAL_RATE).ceil() as usize + 1;
    Signal {
        values: (0..n)
            .map(|i| {
                round3(
                    performing
                        .signal(signal, i as f32 / SIGNAL_RATE)
                        .unwrap_or(0.0),
                )
            })
            .collect(),
        rate: SIGNAL_RATE,
    }
}

/// The names the world's sound plays under, as Gen's loader starts it:
/// ambience layers (an ambient entity with no radius is one, its
/// `ambience_` prefix dropped) and emitters.
fn heard_names(world: &wt::WorldManifest) -> Vec<String> {
    let mut names: Vec<String> = world
        .ambience
        .iter()
        .map(|layer| layer.name.clone())
        .collect();
    for entity in &world.entities {
        let Some(audio) = &entity.audio else {
            continue;
        };
        let name = entity.name.as_str();
        let name = if audio.kind == wt::AudioKind::Ambient && audio.radius.is_none() {
            name.strip_prefix("ambience_").unwrap_or(name)
        } else {
            name
        };
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    names
}

fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn round3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}

// ---------------------------------------------------------------------------
// Shot edits
// ---------------------------------------------------------------------------

/// A shot's new place on the clock, as the page lets go of a drag.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ShotTime {
    pub id: u64,
    pub in_s: f32,
    pub out_s: f32,
}

/// The edit new in and out points make: a `ModifyEntity` patch for each
/// shot that moved, carrying its whole ext-cinematography block with
/// `shot.in_s` and `shot.out_s` set (an `ext-*` patch replaces the
/// extension's value, so the camera block rides along unchanged), and the
/// history's line for it. `None` when nothing moves.
pub fn shot_edit(world: &wt::WorldManifest, times: &[ShotTime]) -> Option<(Value, String)> {
    let ext_name = wt::cinematography::EXTENSION_NAME;
    let mut ops = Vec::new();
    // (name, was, now) per shot that moved.
    let mut moved: Vec<(String, [f64; 2], [f64; 2])> = Vec::new();
    for time in times {
        let (new_in, new_out) = (round_s(time.in_s), round_s(time.out_s));
        if !(new_in >= 0.0 && new_out > new_in) {
            continue;
        }
        let Some(entity) = world.entities.iter().find(|e| e.id.0 == time.id) else {
            continue;
        };
        let Some(mut ext) = entity.extra.get(ext_name).cloned() else {
            continue;
        };
        let Some(shot) = ext.get_mut("shot").and_then(Value::as_object_mut) else {
            continue;
        };
        let seconds = |k: &str| shot.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        let was = [seconds("in_s"), seconds("out_s")];
        if (was[0] - new_in).abs() < 0.005 && (was[1] - new_out).abs() < 0.005 {
            continue;
        }
        shot.insert("in_s".into(), json!(new_in));
        shot.insert("out_s".into(), json!(new_out));
        ops.push(json!({"ModifyEntity": {"id": time.id, "patch": {ext_name: ext}}}));
        moved.push((entity.name.as_str().to_string(), was, [new_in, new_out]));
    }
    (!ops.is_empty()).then(|| (json!(ops), describe(&moved)))
}

/// A recut of the song — its sections as the page lets go of them, each a
/// fraction of the song from 0 — as the `ModifyWorld` patch that sets them
/// (the authority merges it into the soundtrack, so nothing else of the
/// song changes) and the history's line for it. `None` when the world has
/// no song, the list isn't a song's sections, or nothing moves.
pub fn recut(world: &wt::WorldManifest, sections: &[f32]) -> Option<(Value, String)> {
    let song = song_of(world)?;
    // To the ten-thousandth, as f64: an f32 would write 0.55 as 0.5500000119.
    let new: Vec<f64> = sections
        .iter()
        .map(|&f| (f64::from(f) * 10_000.0).round() / 10_000.0)
        .collect();
    let valid = new.first() == Some(&0.0)
        && new.windows(2).all(|pair| pair[0] < pair[1])
        && new.iter().all(|f| (0.0..1.0).contains(f));
    let old: Vec<f64> = if song.sections.is_empty() {
        vec![0.0]
    } else {
        song.sections.iter().map(|&f| f64::from(f)).collect()
    };
    let apart = |a: f64, b: f64| (a - b).abs() > 1e-4;
    let same = old.len() == new.len() && old.iter().zip(&new).all(|(a, b)| !apart(*a, *b));
    if !valid || same {
        return None;
    }
    let at = |f: f64| timecode(f * f64::from(song.duration));
    // The first place the two lists part: what moved, came or went.
    let parts = |a: &[f64], b: &[f64]| (0..a.len().min(b.len())).find(|&i| apart(a[i], b[i]));
    let what = match new.len() as isize - old.len() as isize {
        0 => {
            let moved: Vec<usize> = (0..new.len()).filter(|&i| apart(new[i], old[i])).collect();
            match moved[..] {
                [i] => format!("§{} to {}", i + 1, at(new[i])),
                _ => "recut the song".into(),
            }
        }
        1 => format!(
            "a section at {}",
            at(new[parts(&new, &old).unwrap_or(old.len())])
        ),
        -1 => {
            let gone = parts(&new, &old).unwrap_or(new.len());
            format!("§{} and §{} join", gone, gone + 1)
        }
        _ => "recut the song".into(),
    };
    Some((json!({"soundtrack": {"sections": new}}), what))
}

/// A tour stop's hold as a drag lets go of it, as the `ModifyWorld` patch
/// that replaces the tours with it — a list goes whole — and the history's
/// line. The stops after it come later by as much; the travel between them
/// is the tour's speed, and stays. `None` when there is no such stop or its
/// hold doesn't change.
pub fn held_longer(
    world: &wt::WorldManifest,
    tour: &str,
    stop: usize,
    seconds: f32,
) -> Option<(Value, String)> {
    let seconds = round2(seconds.max(0.0));
    let mut tours = world.tours.clone();
    let waypoint = tours
        .iter_mut()
        .find(|t| t.name == tour)?
        .waypoints
        .get_mut(stop)?;
    if (waypoint.pause_duration - seconds).abs() < 0.005 {
        return None;
    }
    waypoint.pause_duration = seconds;
    let what = format!("{tour} holds {seconds} s at stop {}", stop + 1);
    Some((json!({"tours": tours}), what))
}

/// Times as the format writes them: to the hundredth of a second.
fn round_s(t: f32) -> f64 {
    (f64::from(t) * 100.0).round() / 100.0
}

/// The history's line for a timeline edit.
fn describe(moved: &[(String, [f64; 2], [f64; 2])]) -> String {
    let changed = |a: f64, b: f64| (a - b).abs() >= 0.005;
    // A cut rolled: one shot's out and the next one's in, to the same place.
    if let [(a, a_was, a_now), (b, b_was, b_now)] = moved
        && !changed(a_was[0], a_now[0])
        && !changed(b_was[1], b_now[1])
        && !changed(a_now[1], b_now[0])
    {
        return format!("cut {a} | {b} to {}", timecode(a_now[1]));
    }
    moved
        .iter()
        .map(|(name, was, now)| {
            let (start, end) = (changed(was[0], now[0]), changed(was[1], now[1]));
            if start && end && !changed(now[0] - was[0], now[1] - was[1]) {
                format!("move {name} to {}", timecode(now[0]))
            } else if start && !end {
                format!("trim {name} in to {}", timecode(now[0]))
            } else if end && !start {
                format!("trim {name} out to {}", timecode(now[1]))
            } else {
                format!("time {name} {}–{}", timecode(now[0]), timecode(now[1]))
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// `m:ss.cc`.
fn timecode(t: f64) -> String {
    let hundredths = (t.max(0.0) * 100.0).round() as u64;
    let (seconds, cc) = (hundredths / 100, hundredths % 100);
    format!("{}:{:02}.{cc:02}", seconds / 60, seconds % 60)
}

// ---------------------------------------------------------------------------
// The world the timeline is of
// ---------------------------------------------------------------------------

/// Where the timeline's world comes from: the canvas's document, or the
/// folder the scene was loaded from, read again when its `world.ron` is
/// written (a document's rebuild writes it).
#[derive(Resource)]
struct Source {
    canvas: Option<PathBuf>,
    /// The folder last read, and when its `world.ron` was written then.
    read: Option<(PathBuf, Option<SystemTime>)>,
    poll: Timer,
}

/// A waveform on its way from the decoding thread, by the file it is of.
type Decoding = (PathBuf, Mutex<Receiver<Option<Vec<f32>>>>);

/// The timeline's state in the app: the view the page draws, the world it
/// was built from, and what the page asked of it.
#[derive(Resource, Default)]
pub struct Timeline {
    /// `None` until a world is open.
    pub view: Option<TimelineView>,
    /// Counts the view's rebuilds, so the page gets it only when it changed.
    pub rev: u64,
    world: Option<wt::WorldManifest>,
    /// The world's folder: its song is in `assets/`.
    dir: Option<PathBuf>,
    editable: bool,
    review: Option<PendingReview>,
    /// The song's waveform, by the file it was decoded from.
    peaks: Option<(PathBuf, Vec<f32>)>,
    decoding: Option<Decoding>,
    /// The heard tracks the page silenced, by track id.
    muted: Vec<String>,
    /// The mutes are to be applied again: asked for, or the tracks changed.
    remute: bool,
    /// The seen tracks the page hid, by track id.
    hidden: Vec<String>,
    /// The hidden are to be gone over again.
    rehide: bool,
}

impl Timeline {
    /// The world's document the view was built from: the canvas's on
    /// screen, or the folder the scene loaded.
    pub fn world(&self) -> Option<&wt::WorldManifest> {
        self.world.as_ref()
    }

    /// The world edits here: a live canvas.
    pub fn editable(&self) -> bool {
        self.editable
    }

    fn rebuild(&mut self) {
        let file = self.song_file();
        let peaks = self
            .peaks
            .as_ref()
            .filter(|(decoded, _)| Some(decoded) == file.as_ref())
            .map(|(_, peaks)| peaks.as_slice());
        self.view = self
            .world
            .as_ref()
            .map(|world| view_of(world, peaks, self.editable, self.review.as_ref()));
        self.rev += 1;
        let heard: Vec<String> = self.heard().map(|(id, _)| id.to_string()).collect();
        self.muted.retain(|id| heard.contains(id));
        self.remute = true;
        let tracks: Vec<String> = self
            .view
            .iter()
            .flat_map(|view| &view.tracks)
            .map(|track| track_id(track).to_string())
            .collect();
        self.hidden.retain(|id| tracks.contains(id));
        self.rehide = true;
    }

    fn duration(&self) -> Option<f32> {
        self.view
            .as_ref()
            .and_then(|view| view.clock.as_ref())
            .map(|clock| clock.duration)
    }

    fn song_file(&self) -> Option<PathBuf> {
        let path = self.world.as_ref().and_then(song_of)?.path.as_ref()?;
        Some(self.dir.as_ref()?.join("assets").join(path))
    }

    /// The heard tracks: their ids, and the audio each is — the song, or a
    /// layer or emitter by name.
    fn heard(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.view
            .iter()
            .flat_map(|view| &view.tracks)
            .filter_map(|track| match track {
                TrackView::Soundtrack { id, toggles, .. } if !toggles.is_empty() => {
                    Some((id.as_str(), None))
                }
                TrackView::Ambience { id, name, .. } => Some((id.as_str(), Some(name.as_str()))),
                _ => None,
            })
    }

    /// The shot under `t`; where shots overlap (a master under its
    /// coverage), the one that started last.
    fn shot_at(&self, t: f32) -> Option<u64> {
        let view = self.view.as_ref()?;
        view.tracks.iter().find_map(|track| match track {
            TrackView::Shots { shots, .. } => shots
                .iter()
                .filter(|shot| shot.from <= t && t < shot.to)
                .max_by(|a, b| a.from.total_cmp(&b.from))
                .and_then(|shot| shot.id.parse().ok()),
            _ => None,
        })
    }

    /// Start decoding the song's waveform, unless it is decoded or on its
    /// way.
    fn decode_song(&mut self) {
        let Some(file) = self.song_file() else {
            return;
        };
        let known = |held: Option<&PathBuf>| held == Some(&file);
        if known(self.peaks.as_ref().map(|(f, _)| f))
            || known(self.decoding.as_ref().map(|(f, _)| f))
        {
            return;
        }
        let (tx, rx) = channel();
        let path = file.clone();
        let spawned = std::thread::Builder::new()
            .name("song-peaks".into())
            .spawn(move || {
                let _ = tx.send(peaks::decode(&path));
            });
        if spawned.is_ok() {
            self.decoding = Some((file, Mutex::new(rx)));
        }
    }
}

fn read_world(
    time: Res<Time>,
    mut source: ResMut<Source>,
    shots: Option<Res<LiveShots>>,
    review: Option<Res<LiveReview>>,
    current: Option<Res<CurrentWorld>>,
    mut timeline: ResMut<Timeline>,
    mut clock: ResMut<WorldClock>,
) {
    // A waveform decoded in the background.
    if let Some((file, rx)) = &timeline.decoding {
        let received = rx
            .lock()
            .map_or(Err(TryRecvError::Disconnected), |rx| rx.try_recv());
        match received {
            Ok(peaks) => {
                let file = file.clone();
                timeline.decoding = None;
                timeline.peaks = peaks.map(|peaks| (file, peaks));
                timeline.rebuild();
            }
            Err(TryRecvError::Disconnected) => timeline.decoding = None,
            Err(TryRecvError::Empty) => {}
        }
    }

    let had_clock = timeline.duration().is_some();
    if let Some(dir) = source.canvas.clone() {
        let changed = shots.as_ref().is_some_and(|s| s.is_changed())
            || review.as_ref().is_some_and(|r| r.is_changed());
        let Some(shots) = shots.filter(|_| changed) else {
            return;
        };
        timeline.world = Some(shots.manifest.clone());
        timeline.review = review.and_then(|review| review.pending.clone());
        timeline.dir = Some(dir);
        timeline.editable = true;
        timeline.decode_song();
        timeline.rebuild();
        // The package opens once; its clock starts when its document lands.
        if !had_clock {
            clock.reset(&timeline);
        }
        return;
    }

    let path = current.as_ref().and_then(|current| current.path.clone());
    let moved = source.read.as_ref().map(|(dir, _)| dir) != path.as_ref();
    if !source.poll.tick(time.delta()).just_finished() && !moved {
        return;
    }
    let Some(dir) = path else {
        source.read = None;
        if timeline.world.take().is_some() {
            timeline.rebuild();
        }
        return;
    };
    let file = dir.join("world.ron");
    let written = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
    if !moved && source.read.as_ref().is_some_and(|(_, at)| *at == written) {
        return;
    }
    source.read = Some((dir.clone(), written));
    let Some(world) = read_ron(&file) else {
        return;
    };
    timeline.world = Some(world);
    timeline.dir = Some(dir);
    timeline.editable = false;
    timeline.review = None;
    timeline.decode_song();
    timeline.rebuild();
    // Another world: its clock starts over. A document's rebuild is the
    // same world, and keeps where it was.
    if moved || !had_clock {
        clock.reset(&timeline);
    }
}

/// A world folder's `world.ron`, as Gen's loader reads it.
fn read_ron(file: &Path) -> Option<wt::WorldManifest> {
    let text = std::fs::read_to_string(file).ok()?;
    match ron::from_str::<wt::WorldManifest>(&text) {
        Ok(mut world) => {
            world.expand_instances();
            Some(world)
        }
        Err(e) => {
            tracing::debug!("timeline: {}: {e}", file.display());
            None
        }
    }
}

// ---------------------------------------------------------------------------
// The clock
// ---------------------------------------------------------------------------

/// Where the world is in time, and whether it moves.
#[derive(Resource, Debug, Default)]
pub struct WorldClock {
    /// Seconds: where the playhead is.
    pub at: f32,
    pub playing: bool,
    /// The span playback repeats, when one is set.
    pub looping: Option<[f32; 2]>,
    /// The song was moved here and isn't heard there yet, and for how
    /// long: until it is, what the engine reports is where it was.
    seeking: Option<(f32, f32)>,
    /// How long a song has played under a paused clock.
    hushing: f32,
}

impl WorldClock {
    /// A world just opened: at the start, playing if it has a song (it
    /// performs it), still if not.
    fn reset(&mut self, timeline: &Timeline) {
        *self = Self {
            playing: timeline.world.as_ref().and_then(song_of).is_some(),
            ..Self::default()
        };
    }
}

/// The page's asks of the timeline.
#[derive(Message, Debug, Clone)]
pub enum TimelineAsk {
    Seek(f32),
    Play(bool),
    Loop(Option<[f32; 2]>),
    /// Look through a shot's camera, by entity id; `None`, the free camera.
    Look(Option<u64>),
    /// New in and out points for shots, as one edit.
    Shots(Vec<ShotTime>),
    /// Accept or reject the agent's proposal.
    Review(bool),
    /// The heard tracks to silence, by id; the rest are heard.
    Mute(Vec<String>),
    /// The seen tracks to hide, by id: what they move stands still.
    Hide(Vec<String>),
    /// The song's sections as a recut lets go of them: where each starts,
    /// as a fraction of the song, from 0.
    Sections(Vec<f32>),
    /// How long the camera waits at a tour's stop, as a drag lets go of it.
    Hold {
        tour: String,
        stop: usize,
        hold: f32,
    },
}

/// Whether the song is what keeps time: the world ships its audio, and an
/// engine plays it.
fn song_keeps_time(soundtrack: &Soundtrack, audio: Option<&AudioEngine>) -> bool {
    audio.is_some_and(|audio| audio.active)
        && soundtrack
            .def
            .as_ref()
            .is_some_and(|song| song.path.is_some())
}

#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn answer_asks(
    mut asks: MessageReader<TimelineAsk>,
    mut timeline: ResMut<Timeline>,
    mut clock: ResMut<WorldClock>,
    soundtrack: Res<Soundtrack>,
    mut audio: Option<ResMut<AudioEngine>>,
    mut looks: Option<ResMut<Messages<LookThrough>>>,
    mut shot_ops: Option<ResMut<Messages<ShotOps>>>,
    mut decisions: Option<ResMut<Messages<ReviewDecision>>>,
    mut intents: Option<ResMut<Messages<PersonIntent>>>,
) {
    let on_song = song_keeps_time(&soundtrack, audio.as_deref());
    for ask in asks.read() {
        match ask {
            TimelineAsk::Seek(t) => {
                let Some(duration) = timeline.duration() else {
                    continue;
                };
                let t = t.clamp(0.0, duration);
                clock.at = t;
                if on_song && let Some(audio) = audio.as_deref_mut() {
                    audio.seek_soundtrack(t);
                    clock.seeking = Some((t, 0.0));
                }
            }
            TimelineAsk::Play(playing) => {
                clock.playing = *playing;
                clock.hushing = 0.0;
                if on_song && let Some(audio) = audio.as_deref_mut() {
                    audio.pause_soundtrack(!playing);
                }
            }
            TimelineAsk::Loop(span) => clock.looping = span.filter(|[from, to]| to > from),
            TimelineAsk::Look(id) => {
                if let Some(looks) = looks.as_mut() {
                    looks.write(LookThrough(*id));
                }
            }
            TimelineAsk::Shots(times) => {
                if !timeline.editable {
                    tracing::warn!("timeline: shots are edited on a live canvas only");
                    continue;
                }
                let edit = timeline.world.as_ref().and_then(|w| shot_edit(w, times));
                if let (Some((ops, what)), Some(shot_ops)) = (edit, shot_ops.as_mut()) {
                    shot_ops.write(ShotOps { ops, what });
                }
            }
            TimelineAsk::Review(accept) => {
                if let Some(decisions) = decisions.as_mut() {
                    decisions.write(ReviewDecision(*accept));
                }
            }
            TimelineAsk::Mute(ids) => {
                timeline.muted = ids.clone();
                timeline.remute = true;
            }
            TimelineAsk::Hide(ids) => {
                timeline.hidden = ids.clone();
                timeline.rehide = true;
            }
            // The world's own fields: a person's scene-wide edit, which the
            // canvas commits through the authority like a gesture.
            TimelineAsk::Sections(_) | TimelineAsk::Hold { .. } => {
                if !timeline.editable {
                    tracing::warn!(
                        "timeline: the song and the tour are edited on a live canvas only"
                    );
                    continue;
                }
                let edit = timeline.world.as_ref().and_then(|world| match ask {
                    TimelineAsk::Sections(sections) => recut(world, sections),
                    TimelineAsk::Hold { tour, stop, hold } => {
                        held_longer(world, tour, *stop, *hold)
                    }
                    _ => None,
                });
                if let (Some((patch, what)), Some(intents)) = (edit, intents.as_mut()) {
                    intents.write(PersonIntent::World { patch, what });
                }
            }
        }
    }
    if timeline.remute
        && let Some(audio) = audio.as_deref_mut()
    {
        timeline.remute = false;
        let muted = |id: &str| timeline.muted.iter().any(|m| m == id);
        for (id, name) in timeline.heard() {
            match name {
                None => audio.mute_soundtrack(muted(id)),
                Some(name) => {
                    audio.mute(name, muted(id));
                }
            }
        }
    }
}

/// What a hidden track's things keep while it is hidden: their own lists,
/// as the world gives them, and what was left on them — so a list the
/// world replaces meanwhile (an edit, a rebuild) is the one restored.
#[derive(Component)]
struct HeldModulations {
    all: Vec<wt::ModulationDef>,
    shown: Vec<wt::ModulationDef>,
}

#[derive(Component)]
struct HeldBehaviors {
    all: Vec<BehaviorInstance>,
    /// The ids of the instances left on the entity.
    shown: Vec<String>,
}

/// A hidden modulation or behavior track stands its things still: its
/// modulations stop driving them, its behaviors stop moving them, and a
/// thing a behavior let go of stands where the world puts it. Nothing is
/// written — like a mute, it is the person's view.
fn hide_tracks(
    mut commands: Commands,
    mut timeline: ResMut<Timeline>,
    modulated: Query<(Entity, Ref<Modulated>, Option<&HeldModulations>)>,
    mut behaving: Query<(
        Entity,
        &GenEntity,
        &mut EntityBehaviors,
        Option<&HeldBehaviors>,
        &mut Transform,
    )>,
) {
    let again = std::mem::take(&mut timeline.rehide);
    let timeline = &*timeline;
    let hidden = |track: &str| timeline.hidden.iter().any(|id| id == track);

    for (entity, component, held) in &modulated {
        if !again && !component.is_changed() {
            continue;
        }
        let current: Vec<wt::ModulationDef> = component.defs().cloned().collect();
        let all = match held {
            Some(held) if held.shown == current => held.all.clone(),
            _ => current.clone(),
        };
        let shown: Vec<wt::ModulationDef> = all
            .iter()
            .filter(|def| !hidden(&modulation_track(def)))
            .cloned()
            .collect();
        if shown.len() == all.len() {
            if held.is_some() {
                if current != all {
                    commands.entity(entity).insert(modulated_from(&all));
                }
                commands.entity(entity).remove::<HeldModulations>();
            }
            continue;
        }
        if shown != current {
            commands.entity(entity).insert(modulated_from(&shown));
        }
        if held.is_none_or(|held| held.shown != shown) {
            commands
                .entity(entity)
                .insert(HeldModulations { all, shown });
        }
    }

    for (entity, tag, mut behaviors, held, mut transform) in &mut behaving {
        if !again && !behaviors.is_changed() {
            continue;
        }
        let current: Vec<String> = behaviors.behaviors.iter().map(|b| b.id.clone()).collect();
        let all = match held {
            Some(held) if held.shown == current => held.all.clone(),
            _ => behaviors.behaviors.clone(),
        };
        let shown: Vec<BehaviorInstance> = all
            .iter()
            .filter(|b| behavior_track(&(&b.def).into()).is_none_or(|track| !hidden(&track)))
            .cloned()
            .collect();
        if shown.len() == all.len() {
            if held.is_some() {
                if current.len() != all.len() {
                    behaviors.behaviors = all;
                }
                commands.entity(entity).remove::<HeldBehaviors>();
            }
            continue;
        }
        let ids: Vec<String> = shown.iter().map(|b| b.id.clone()).collect();
        if ids != current {
            behaviors.behaviors = shown;
            let authored = timeline
                .world
                .as_ref()
                .and_then(|world| world.entities.iter().find(|e| e.id == tag.world_id));
            if let Some(authored) = authored {
                *transform = localgpt_world_bevy::transform(&authored.transform);
            }
        }
        if held.is_none_or(|held| held.shown != ids) {
            commands
                .entity(entity)
                .insert(HeldBehaviors { all, shown: ids });
        }
    }
}

/// The component for these modulations; one that drives nothing when there
/// are none, so the entity stays in the query that brings them back.
fn modulated_from(defs: &[wt::ModulationDef]) -> Modulated {
    let mut probe = wt::WorldEntity::new(0, "");
    probe.modulations = defs.to_vec();
    localgpt_world_bevy::modulation::modulated(&probe).unwrap_or_default()
}

/// The clock `dt` later, when the app keeps it: it repeats the loop span
/// when one is set, else the whole clock, as a song loops.
fn advance(at: f32, dt: f32, duration: f32, looping: Option<[f32; 2]>) -> f32 {
    let next = at + dt;
    match looping {
        Some([from, to]) if to > from && next >= to => from + (next - to) % (to - from),
        _ if duration > 0.0 && next >= duration => next % duration,
        _ => next,
    }
}

/// Keep the clock, and show the world at it: the song's position while it
/// plays and is heard, the app's own otherwise. Runs after Gen's
/// [`sync_soundtrack_clock`], whose `position` (the song's, or none while
/// paused) this replaces with the clock's, so the modulations hold where
/// the playhead is.
fn drive_clock(
    time: Res<Time>,
    timeline: Res<Timeline>,
    mut clock: ResMut<WorldClock>,
    mut soundtrack: ResMut<Soundtrack>,
    mut audio: Option<ResMut<AudioEngine>>,
) {
    let Some(duration) = timeline.duration() else {
        return;
    };
    let dt = time.delta_secs();
    let on_song = song_keeps_time(&soundtrack, audio.as_deref());
    if on_song && let Some(audio) = audio.as_deref_mut() {
        let heard = audio.soundtrack_position();
        if clock.playing {
            match (heard, clock.seeking) {
                (Some(at), Some((to, waited))) if (at - to).abs() > 0.3 && waited < 1.0 => {
                    clock.seeking = Some((to, waited + dt));
                }
                (Some(at), _) => {
                    clock.seeking = None;
                    clock.at = at;
                }
                (None, _) => {}
            }
            if let Some([from, to]) = clock.looping
                && clock.at >= to
                && clock.seeking.is_none()
            {
                audio.seek_soundtrack(from);
                clock.at = from;
                clock.seeking = Some((from, 0.0));
            }
        } else if heard.is_some() {
            // A song started under a paused clock (its world loaded
            // again): it waits too.
            clock.hushing += dt;
            if clock.hushing > 0.5 {
                clock.hushing = 0.0;
                audio.pause_soundtrack(true);
            }
        }
    } else if clock.playing {
        clock.at = advance(clock.at, dt, duration, clock.looping);
    }
    clock.at = clock.at.clamp(0.0, duration);
    if soundtrack.position != Some(clock.at) {
        soundtrack.position = Some(clock.at);
    }
}

/// While a shot is looked through, moving the playhead cuts to the shot
/// under it — playing, the camera follows the cut list.
fn follow_cut(
    clock: Res<WorldClock>,
    timeline: Res<Timeline>,
    view: Option<Res<ShotView>>,
    looks: Option<ResMut<Messages<LookThrough>>>,
    mut last: Local<Option<f32>>,
) {
    let moved = last.is_some_and(|t| (t - clock.at).abs() > 1e-4);
    *last = Some(clock.at);
    let (Some(view), Some(mut looks)) = (view, looks) else {
        return;
    };
    if !moved {
        return;
    }
    if let Some(active) = view.active
        && let Some(id) = timeline.shot_at(clock.at)
        && id != active
    {
        looks.write(LookThrough(Some(id)));
    }
}

// ---------------------------------------------------------------------------
// Thumbnails
// ---------------------------------------------------------------------------

/// Each shot's newest frame for the page, as a JPEG data URL — read back
/// from the thumbnail the canvas renders offscreen whenever it renders one.
#[derive(Resource, Default)]
pub struct Thumbs {
    /// By the shot's entity id: the revision it arrived at, and the URL.
    pub urls: BTreeMap<u64, (u64, String)>,
    pub rev: u64,
    inbox: Arc<Mutex<Vec<(u64, Captured)>>>,
    /// Blank captures asked again since the last frame that drew, and when
    /// to ask next.
    retries: u32,
    retry_at: Option<f32>,
}

/// What a read-back came to. A thumbnail camera's first frames can draw
/// nothing while its pipelines compile (it renders to a format the window
/// doesn't), so a frame with no pixel written is asked for again.
enum Captured {
    Frame(String),
    Blank,
}

/// How many blank captures are asked again, a fifth of a second apart.
const RETRIES: u32 = 40;

fn capture_thumbs(
    mut commands: Commands,
    time: Res<Time>,
    images: Option<Res<Assets<Image>>>,
    rendered: Option<ResMut<ShotThumbs>>,
    dirty: Option<ResMut<ThumbsDirty>>,
    mut thumbs: ResMut<Thumbs>,
) {
    let arrived: Vec<(u64, Captured)> = thumbs
        .inbox
        .lock()
        .map(|mut inbox| inbox.drain(..).collect())
        .unwrap_or_default();
    let now = time.elapsed_secs();
    for (id, captured) in arrived {
        match captured {
            Captured::Frame(url) => {
                thumbs.rev += 1;
                let rev = thumbs.rev;
                thumbs.urls.insert(id, (rev, url));
                thumbs.retries = 0;
            }
            Captured::Blank if thumbs.retries < RETRIES && thumbs.retry_at.is_none() => {
                thumbs.retries += 1;
                thumbs.retry_at = Some(now + 0.2);
            }
            Captured::Blank => {}
        }
    }
    if let Some(at) = thumbs.retry_at
        && now >= at
        && let Some(mut dirty) = dirty
    {
        thumbs.retry_at = None;
        dirty.0 = true;
    }
    let (Some(mut rendered), Some(images)) = (rendered, images) else {
        return;
    };
    // The camera has drawn its frame: read the texture back as it holds it.
    for (id, image) in rendered.take_rendered() {
        let Some(size) = images.get(&image).map(|image| image.size()) else {
            continue;
        };
        let inbox = Arc::clone(&thumbs.inbox);
        commands.spawn(Readback::texture(image)).observe(
            move |done: On<ReadbackComplete>, mut commands: Commands| {
                if let Some(captured) = read_back(&done.data, size)
                    && let Ok(mut inbox) = inbox.lock()
                {
                    inbox.push((id, captured));
                }
                // A readback repeats every frame while it is there.
                commands.entity(done.entity).despawn();
            },
        );
    }
}

/// A thumbnail's bytes as the GPU copied them (sRGB RGBA, each row padded
/// to the copy alignment) to a JPEG data URL — or blank, when no pixel was
/// ever written.
fn read_back(data: &[u8], size: UVec2) -> Option<Captured> {
    let (width, height) = (size.x as usize, size.y as usize);
    let row = width * 4;
    let stride = RenderDevice::align_copy_bytes_per_row(row);
    if width == 0 || height == 0 || data.len() < stride * (height - 1) + row {
        return None;
    }
    let mut rgb = Vec::with_capacity(width * height * 3);
    let mut written = false;
    for y in 0..height {
        for pixel in data[y * stride..y * stride + row].as_chunks::<4>().0 {
            written |= pixel[3] != 0;
            rgb.extend_from_slice(&pixel[..3]);
        }
    }
    if !written {
        return Some(Captured::Blank);
    }
    let frame = image::RgbImage::from_raw(size.x, size.y, rgb)?;
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 78)
        .encode_image(&frame)
        .ok()?;
    Some(Captured::Frame(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera entity with a shot from `in_s` to `out_s`.
    fn shot(id: u64, name: &str, order: u32, in_s: f64, out_s: f64) -> wt::WorldEntity {
        serde_json::from_value(json!({
            "id": id,
            "name": name,
            "transform": {"position": [0.0, 1.6, 6.0]},
            "ext-cinematography": {
                "camera": {"focal_length_mm": 35, "aim": [0.0, 1.6, 0.0]},
                "shot": {"scene": "1", "order": order, "size": "MS", "in_s": in_s, "out_s": out_s},
            },
        }))
        .unwrap()
    }

    fn song() -> wt::SoundtrackDef {
        wt::SoundtrackDef {
            path: Some("music/tidewater.mp3".into()),
            title: Some("Tidewater".into()),
            artist: Some("Starter pack".into()),
            license: Some("CC0-1.0".into()),
            duration: 192.0,
            bpm: 92.0,
            beat_offset: 0.4,
            sections: vec![0.0, 0.25, 0.6],
            energy: vec![0.2, 0.5, 0.9],
            stems: Some(wt::StemCurves {
                drums: vec![0.1, 0.8],
                ..Default::default()
            }),
        }
    }

    fn world(
        entities: Vec<wt::WorldEntity>,
        soundtrack: Option<wt::SoundtrackDef>,
    ) -> wt::WorldManifest {
        wt::WorldManifest {
            entities,
            soundtrack,
            ..wt::WorldManifest::new("harbour")
        }
    }

    fn kinds(view: &TimelineView) -> Vec<&'static str> {
        view.tracks
            .iter()
            .map(|track| match track {
                TrackView::Shots { .. } => "shots",
                TrackView::Camera { .. } => "camera",
                TrackView::Soundtrack { .. } => "soundtrack",
                TrackView::Stem { .. } => "stem",
                TrackView::Modulation { .. } => "modulation",
                TrackView::Behavior { .. } => "behavior",
                TrackView::Ambience { .. } => "ambience",
            })
            .collect()
    }

    #[test]
    fn a_song_is_the_clock_and_its_curves_draw_until_the_waveform_does() {
        let mut lantern: wt::WorldEntity = serde_json::from_value(
            json!({"id": 7, "name": "lantern", "transform": {"position": [0.0, 2.0, 0.0]}}),
        )
        .unwrap();
        lantern.modulations = vec![wt::ModulationDef {
            target: wt::ModulationTarget::Emissive,
            signal: wt::SignalSource::Beat,
            range: [0.4, 2.0],
            smoothing: 0.0,
        }];
        lantern.behaviors = vec![wt::BehaviorDef::Spin {
            axis: [0.0, 1.0, 0.0],
            speed: 60.0,
        }];
        let mut buoy = lantern.clone();
        buoy.id = wt::EntityId(8);
        buoy.name = wt::EntityName::new("buoy");
        let harbour = world(
            vec![shot(1, "1A", 1, 0.0, 12.0), lantern, buoy],
            Some(song()),
        );

        let view = view_of(&harbour, None, true, None);
        let clock = view.clock.as_ref().expect("the song keeps time");
        assert_eq!(
            (clock.duration, clock.bpm, clock.beat_offset),
            (192.0, 92.0, 0.4)
        );
        assert_eq!(clock.sections, vec![0.0, 0.25, 0.6]);
        assert_eq!(
            kinds(&view),
            ["shots", "soundtrack", "stem", "modulation", "behavior"]
        );
        let TrackView::Soundtrack { peaks, toggles, .. } = &view.tracks[1] else {
            panic!("the soundtrack");
        };
        assert_eq!(
            (peaks.rate, peaks.values.len()),
            (1.0, 3),
            "its energy, a value a second"
        );
        assert_eq!(
            toggles,
            &["mute", "solo"],
            "its audio ships: it can be silenced"
        );
        let TrackView::Behavior { what, period, .. } = &view.tracks[4] else {
            panic!("the behavior");
        };
        assert_eq!(
            (*what, *period),
            ("spin", 6.0),
            "60° a second turns once in 6 s"
        );

        // The modulation names its signal; the samples go once, by name.
        let TrackView::Modulation {
            source,
            target,
            name,
            ..
        } = &view.tracks[3]
        else {
            panic!("the modulation");
        };
        assert_eq!((source.as_str(), *target), ("beat", "emissive"));
        assert_eq!(
            name, "lantern +1",
            "the buoy pulses alike: one track for both"
        );
        let beat = &view.signals["beat"];
        assert_eq!(beat.rate, SIGNAL_RATE);
        assert_eq!(beat.values.len(), (192.0 * SIGNAL_RATE) as usize + 1);
        assert!(
            beat.values.iter().any(|&v| v > 0.9),
            "a pulse on every beat"
        );

        // Once decoded, the waveform draws the song.
        let view = view_of(&harbour, Some(&[0.5; 80]), true, None);
        let TrackView::Soundtrack { peaks, .. } = &view.tracks[1] else {
            panic!("the soundtrack");
        };
        assert_eq!((peaks.rate, peaks.values.len()), (peaks::RATE, 80));
    }

    #[test]
    fn without_a_song_the_shots_or_a_tour_keep_time_or_nothing_does() {
        let previs = world(
            vec![shot(1, "1A", 1, 0.0, 12.0), shot(2, "1B", 2, 12.0, 27.5)],
            None,
        );
        let view = view_of(&previs, None, false, None);
        let clock = view.clock.as_ref().expect("the shot list keeps time");
        assert_eq!(
            (clock.duration, clock.bpm),
            (27.5, 0.0),
            "its last out, no beat grid"
        );
        let TrackView::Shots { shots, toggles, .. } = &view.tracks[0] else {
            panic!("the shots");
        };
        assert_eq!(shots.len(), 2);
        assert_eq!(
            (shots[1].name.as_str(), shots[1].from, shots[1].to),
            ("1B", 12.0, 27.5)
        );
        assert!(toggles.is_empty(), "not a canvas: nothing to lock");

        let mut toured = world(Vec::new(), None);
        toured.tours = vec![wt::TourDef {
            name: "dusk".into(),
            description: None,
            waypoints: [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 0.0, 20.0]]
                .into_iter()
                .map(|position| wt::TourWaypoint {
                    position,
                    look_at: [0.0; 3],
                    description: None,
                    pause_duration: 2.0,
                })
                .collect(),
            speed: 5.0,
            mode: wt::TourMode::Fly,
            autostart: false,
            loop_tour: false,
            pov: None,
        }];
        let view = view_of(&toured, None, false, None);
        let TrackView::Camera { keys, .. } = &view.tracks[0] else {
            panic!("the tour");
        };
        let times: Vec<f32> = keys.iter().map(|k| k.t).collect();
        assert_eq!(
            times,
            [0.0, 4.0, 10.0],
            "10 m at 5 m/s after a 2 s stop, then 20 m"
        );
        assert_eq!(
            view.clock.unwrap().duration,
            12.0,
            "the last stop's pause ends it"
        );

        let still = world(Vec::new(), None);
        assert_eq!(view_of(&still, None, false, None).clock, None);
    }

    #[test]
    fn a_proposal_draws_only_the_shots_it_moves() {
        let doc = world(
            vec![
                shot(1, "3A", 1, 100.0, 115.2),
                shot(2, "3B", 2, 115.2, 140.0),
            ],
            Some(song()),
        );
        let proposed = world(
            vec![
                shot(1, "3A", 1, 100.0, 115.2),
                shot(2, "3B", 2, 120.42, 145.22),
            ],
            Some(song()),
        );
        let review = PendingReview {
            n: 1,
            author: "claude".into(),
            changes: vec!["modify 3B".into()],
            shots: localgpt_previs::shots::shot_list(&proposed),
        };
        let view = view_of(&doc, None, true, Some(&review));
        let proposal = view.proposal.expect("waiting for the person");
        assert_eq!(
            (proposal.author.as_str(), proposal.what.as_str()),
            ("claude", "modify 3B")
        );
        assert_eq!(proposal.shots.len(), 1, "3A stays where it is");
        assert_eq!(
            (proposal.shots[0].name.as_str(), proposal.shots[0].from),
            ("3B", 120.42)
        );
    }

    #[test]
    fn a_trim_or_a_move_patches_the_shot_block_and_says_what_it_did() {
        let doc = world(
            vec![
                shot(1, "2B", 1, 51.84, 82.56),
                shot(2, "3A", 2, 82.56, 100.0),
            ],
            Some(song()),
        );
        let time = |id, in_s, out_s| ShotTime { id, in_s, out_s };

        let (ops, what) = shot_edit(&doc, &[time(2, 87.78, 105.22)]).expect("a move");
        assert_eq!(what, "move 3A to 1:27.78");
        let op = &ops[0]["ModifyEntity"];
        assert_eq!(op["id"], 2);
        let block = &op["patch"]["ext-cinematography"];
        assert_eq!(
            (
                block["shot"]["in_s"].as_f64(),
                block["shot"]["out_s"].as_f64()
            ),
            (Some(87.78), Some(105.22))
        );
        assert_eq!(
            block["camera"]["focal_length_mm"], 35,
            "the camera rides along"
        );
        assert_eq!(block["shot"]["size"], "MS");

        // The cut between them, rolled back to bar 31: both, one entry.
        let (ops, what) = shot_edit(&doc, &[time(1, 51.84, 78.66), time(2, 78.66, 100.0)]).unwrap();
        assert_eq!(ops.as_array().unwrap().len(), 2);
        assert_eq!(what, "cut 2B | 3A to 1:18.66");

        let (_, what) = shot_edit(&doc, &[time(1, 49.22, 82.56)]).unwrap();
        assert_eq!(what, "trim 2B in to 0:49.22");

        // Nothing moved, a shot turned inside out, or no such shot: no edit.
        assert!(shot_edit(&doc, &[time(1, 51.84, 82.56)]).is_none());
        assert!(shot_edit(&doc, &[time(1, 60.0, 50.0)]).is_none());
        assert!(shot_edit(&doc, &[time(9, 1.0, 2.0)]).is_none());
    }

    #[test]
    fn the_clock_repeats_its_loop_or_the_whole_of_it() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
        assert!(near(advance(10.0, 0.5, 192.0, None), 10.5));
        assert!(
            near(advance(191.8, 0.5, 192.0, None), 0.3),
            "round to the start"
        );
        let span = Some([60.0, 90.0]);
        assert!(
            near(advance(89.9, 0.2, 192.0, span), 60.1),
            "back to the loop's start"
        );
        assert!(near(advance(70.0, 0.2, 192.0, span), 70.2));
        assert!(
            near(advance(5.0, 0.2, 192.0, span), 5.2),
            "before the loop, it plays on to it"
        );
    }

    #[test]
    fn the_shot_under_the_playhead_is_the_one_that_started_last() {
        let mut timeline = Timeline {
            world: Some(world(
                vec![
                    shot(1, "master", 1, 0.0, 60.0),
                    shot(2, "single", 2, 20.0, 30.0),
                ],
                None,
            )),
            ..Timeline::default()
        };
        timeline.rebuild();
        assert_eq!(timeline.shot_at(10.0), Some(1));
        assert_eq!(
            timeline.shot_at(25.0),
            Some(2),
            "the single inside the master"
        );
        assert_eq!(timeline.shot_at(30.0), Some(1), "out points are exclusive");
        assert_eq!(timeline.shot_at(61.0), None);
    }

    fn tour(name: &str, stops: usize) -> wt::TourDef {
        wt::TourDef {
            name: name.into(),
            description: None,
            waypoints: (0..stops)
                .map(|i| wt::TourWaypoint {
                    position: [i as f32 * 10.0, 0.0, 0.0],
                    look_at: [0.0; 3],
                    description: None,
                    pause_duration: 2.0,
                })
                .collect(),
            speed: 5.0,
            mode: wt::TourMode::Fly,
            autostart: false,
            loop_tour: false,
            pov: None,
        }
    }

    #[test]
    fn a_recut_sets_only_the_sections_and_says_which_moved_came_or_joined() {
        // Tidewater here starts its sections at 0, 0.25 and 0.6 of 192 s.
        let harbour = world(Vec::new(), Some(song()));
        let (patch, what) = recut(&harbour, &[0.0, 0.25, 0.55]).expect("a recut");
        assert_eq!(
            patch,
            json!({"soundtrack": {"sections": [0.0, 0.25, 0.55]}})
        );
        assert_eq!(what, "§3 to 1:45.60");
        let said = |sections: &[f32]| recut(&harbour, sections).map(|(_, what)| what);
        assert_eq!(
            said(&[0.0, 0.25, 0.4, 0.6]).as_deref(),
            Some("a section at 1:16.80")
        );
        assert_eq!(said(&[0.0, 0.6]).as_deref(), Some("§1 and §2 join"));
        assert_eq!(said(&[0.0, 0.25]).as_deref(), Some("§2 and §3 join"));
        // Not a song's sections, or nothing moved, or no song: no edit.
        assert_eq!(said(&[0.1, 0.5]), None, "they start at 0");
        assert_eq!(said(&[0.0, 0.6, 0.25]), None, "in order");
        assert_eq!(said(&[0.0, 0.25, 0.6]), None, "as it was");
        assert_eq!(recut(&world(Vec::new(), None), &[0.0, 0.5]), None);
    }

    #[test]
    fn a_stop_held_longer_replaces_the_tours_with_only_its_hold_changed() {
        let mut harbour = world(Vec::new(), Some(song()));
        harbour.tours = vec![tour("dusk", 3), tour("dawn", 2)];
        let (patch, what) = held_longer(&harbour, "dusk", 1, 4.5).expect("a hold");
        assert_eq!(what, "dusk holds 4.5 s at stop 2");
        let tours: Vec<wt::TourDef> = serde_json::from_value(patch["tours"].clone()).unwrap();
        assert_eq!(tours.len(), 2, "the list goes whole");
        let pauses: Vec<f32> = tours[0]
            .waypoints
            .iter()
            .map(|w| w.pause_duration)
            .collect();
        assert_eq!(pauses, [2.0, 4.5, 2.0]);
        assert_eq!(tours[1], harbour.tours[1], "the other tour as it was");
        assert!(
            held_longer(&harbour, "dusk", 1, 2.0).is_none(),
            "the same hold"
        );
        assert!(
            held_longer(&harbour, "dusk", 7, 1.0).is_none(),
            "no such stop"
        );
        assert!(
            held_longer(&harbour, "noon", 0, 1.0).is_none(),
            "no such tour"
        );
        assert_eq!(
            held_longer(&harbour, "dusk", 0, -3.0).unwrap().1,
            "dusk holds 0 s at stop 1"
        );
    }

    /// Hiding the tracks of a lantern's spin and its glow stands it still,
    /// where the world puts it; showing them gives it both back.
    #[test]
    fn a_hidden_track_stands_its_things_still_until_it_is_shown() {
        let mut lantern: wt::WorldEntity = serde_json::from_value(
            json!({"id": 7, "name": "lantern", "transform": {"position": [0.0, 2.0, 0.0]}}),
        )
        .unwrap();
        lantern.modulations = vec![wt::ModulationDef {
            target: wt::ModulationTarget::Emissive,
            signal: wt::SignalSource::Beat,
            range: [0.4, 2.0],
            smoothing: 0.0,
        }];
        lantern.behaviors = vec![wt::BehaviorDef::Spin {
            axis: [0.0, 1.0, 0.0],
            speed: 60.0,
        }];
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Soundtrack>()
            .add_plugins(TimelinePlugin {
                canvas: Some(PathBuf::from("harbour.world")),
            })
            .insert_resource(LiveShots {
                shots: Vec::new(),
                manifest: world(vec![lantern.clone()], Some(song())),
            });
        let spun = Transform::from_xyz(0.0, 2.0, 0.0).with_rotation(Quat::from_rotation_y(1.0));
        let thing = app
            .world_mut()
            .spawn((
                GenEntity {
                    entity_type: localgpt_gen::gen3d::registry::GenEntityType::Primitive,
                    world_id: wt::EntityId(7),
                },
                EntityBehaviors {
                    behaviors: vec![BehaviorInstance {
                        id: "b1".into(),
                        def: (&lantern.behaviors[0]).into(),
                        base_position: Vec3::new(0.0, 2.0, 0.0),
                        base_scale: Vec3::ONE,
                    }],
                },
                localgpt_world_bevy::modulation::modulated(&lantern).unwrap(),
                spun,
            ))
            .id();
        app.update();

        let spin = behavior_track(&lantern.behaviors[0]).unwrap();
        let glow = modulation_track(&lantern.modulations[0]);
        app.world_mut()
            .write_message(TimelineAsk::Hide(vec![spin, glow]));
        app.update();
        let entity = app.world().entity(thing);
        assert!(
            entity
                .get::<EntityBehaviors>()
                .unwrap()
                .behaviors
                .is_empty(),
            "the spin stops"
        );
        assert_eq!(
            entity.get::<Modulated>().unwrap().defs().count(),
            0,
            "the glow stops"
        );
        assert_eq!(
            *entity.get::<Transform>().unwrap(),
            localgpt_world_bevy::transform(&lantern.transform),
            "it stands where the world puts it"
        );

        app.world_mut().write_message(TimelineAsk::Hide(Vec::new()));
        app.update();
        let entity = app.world().entity(thing);
        assert_eq!(
            entity.get::<EntityBehaviors>().unwrap().behaviors.len(),
            1,
            "the spin is back"
        );
        assert_eq!(
            entity.get::<Modulated>().unwrap().defs().count(),
            1,
            "and the glow"
        );
        assert!(
            entity.get::<HeldBehaviors>().is_none() && entity.get::<HeldModulations>().is_none()
        );
    }

    /// The plugin on a canvas, with no window and no audio: the page's asks
    /// become the canvas's own messages, and the clock keeps itself.
    #[test]
    fn on_a_canvas_a_drag_let_go_is_one_edit_and_a_seek_moves_the_clock() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Soundtrack>()
            .add_message::<ShotOps>()
            .add_message::<LookThrough>()
            .add_message::<ReviewDecision>()
            .add_message::<PersonIntent>()
            .add_plugins(TimelinePlugin {
                canvas: Some(PathBuf::from("harbour.world")),
            })
            .insert_resource(LiveShots {
                shots: Vec::new(),
                manifest: world(
                    vec![
                        shot(1, "2B", 1, 51.84, 82.56),
                        shot(2, "3A", 2, 82.56, 100.0),
                    ],
                    Some(song()),
                ),
            });
        app.update();
        let view = app
            .world()
            .resource::<Timeline>()
            .view
            .clone()
            .expect("the canvas's document");
        assert!(view.editable);
        assert!(
            app.world().resource::<WorldClock>().playing,
            "a world with a song performs it"
        );

        app.world_mut()
            .write_message(TimelineAsk::Shots(vec![ShotTime {
                id: 2,
                in_s: 87.78,
                out_s: 105.22,
            }]));
        app.world_mut().write_message(TimelineAsk::Play(false));
        app.world_mut().write_message(TimelineAsk::Seek(30.0));
        app.update();
        let edits: Vec<ShotOps> = app
            .world_mut()
            .resource_mut::<Messages<ShotOps>>()
            .drain()
            .collect();
        assert_eq!(edits.len(), 1, "one drag, one edit");
        assert_eq!(edits[0].what, "move 3A to 1:27.78");
        let clock = app.world().resource::<WorldClock>();
        assert!(!clock.playing);
        assert_eq!(clock.at, 30.0, "paused where it was sent");
        assert_eq!(
            app.world().resource::<Soundtrack>().position,
            Some(30.0),
            "the world is shown there"
        );

        // A seek past the end lands on it.
        app.world_mut().write_message(TimelineAsk::Seek(500.0));
        app.update();
        assert_eq!(app.world().resource::<WorldClock>().at, 192.0);

        // Looking through a shot, and the person's word on a proposal, are
        // the canvas's own messages.
        app.world_mut().write_message(TimelineAsk::Look(Some(1)));
        app.world_mut().write_message(TimelineAsk::Review(true));
        app.update();
        let looks: Vec<LookThrough> = app
            .world_mut()
            .resource_mut::<Messages<LookThrough>>()
            .drain()
            .collect();
        assert_eq!(
            looks.iter().map(|look| look.0).collect::<Vec<_>>(),
            [Some(1)]
        );
        let decisions: Vec<ReviewDecision> = app
            .world_mut()
            .resource_mut::<Messages<ReviewDecision>>()
            .drain()
            .collect();
        assert_eq!(decisions.iter().map(|d| d.0).collect::<Vec<_>>(), [true]);

        // A recut is the person's scene-wide edit, for the canvas to commit.
        app.world_mut()
            .write_message(TimelineAsk::Sections(vec![0.0, 0.25, 0.55]));
        app.update();
        let intents: Vec<PersonIntent> = app
            .world_mut()
            .resource_mut::<Messages<PersonIntent>>()
            .drain()
            .collect();
        assert_eq!(
            intents,
            [PersonIntent::World {
                patch: json!({"soundtrack": {"sections": [0.0, 0.25, 0.55]}}),
                what: "§3 to 1:45.60".into(),
            }]
        );
    }
}
