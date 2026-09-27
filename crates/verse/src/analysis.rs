//! Offline track analysis — PLAN.md M3 (beats/sections/energy) + M4 (mood).
//!
//! A background worker decodes each track (symphonia), downmixes to low-rate
//! mono, and computes: an onset-novelty curve (spectral flux via realfft), a
//! tempo + beat grid (autocorrelation + phase fit), section boundaries
//! (feature-distance peaks), a per-second energy curve, and a mood via the
//! valence/arousal quadrant mapping from `idea.md`. Results are cached as one
//! JSON sidecar per track, keyed by blake3 content hash (rename/move-proof),
//! in the per-user data dir — the user's music folder is never written to.
//!
//! "Keep this world" (pause overlay) pins a mood into the sidecar; the pin
//! wins over the computed mood whenever the track plays again.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use bevy::prelude::*;
use realfft::RealFftPlanner;
use serde::{Deserialize, Serialize};

use crate::playback::{Beat, Playback};
use crate::theme::Theme;

/// Analysis sample rate — plenty for onsets/brightness, cheap to decode into.
/// Sample rate the DSP analysis runs at (and Demucs stem RMS is reduced to).
pub(crate) const ANALYSIS_SR: u32 = 11_025;
/// Cap the analyzed span; longer tracks are judged by their first 8 minutes.
const MAX_ANALYSIS_SECS: f32 = 480.0;
const FFT_SIZE: usize = 1024;
const HOP: usize = 256;
/// How many upcoming tracks to pre-analyze beyond the current one.
const LOOKAHEAD: usize = 4;

// ---------------------------------------------------------------------------
// Sidecar
// ---------------------------------------------------------------------------

/// Sidecar schema stamp. New fields are `#[serde(default)]`, so older
/// sidecars still load (missing data simply reads as `None`) — no forced
/// re-analysis, no pin loss. Bump only on an incompatible change.
const SIDECAR_VERSION: u32 = 2;

/// Reference loudness for playback normalization (streaming/broadcast norm).
pub const TARGET_LUFS: f32 = -14.0;

/// Cosine similarity of two L2-normed vectors (a dot product). Shared by the
/// CLAP zero-shot mood vote (ml) and the embedding-weighted asset placement —
/// lives here, ungated, so placement compiles in every feature config.
/// NaN-safe: a mismatched or empty pair scores 0 (neutral).
pub(crate) fn dot_normed(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

/// One track's analysis, serialized as a JSON sidecar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackAnalysis {
    pub version: u32,
    /// Analyzed duration in seconds.
    pub duration: f32,
    /// Beats per minute; `0.0` means no reliable grid was found.
    pub bpm: f32,
    /// First-beat offset in seconds.
    pub beat_offset: f32,
    /// Section boundaries as fractions 0..1 (always starts with 0).
    pub sections: Vec<f32>,
    /// Per-second loudness envelope, normalized 0..1.
    pub energy: Vec<f32>,
    /// Mean spectral centroid in Hz (the track's timbral brightness). The
    /// continuous-timbre signal: the palette wash nudges hue by it and the
    /// mood blend weighs it. Absent in older sidecars (reads as neutral).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub centroid_hz: Option<f32>,
    /// Mood index into [`crate::theme::moods()`] from the quadrant mapping.
    ///
    /// Kept as the compatibility representation; read through
    /// [`TrackAnalysis::mood_index`], which prefers `mood_id`.
    pub mood: usize,
    /// Stable id of `mood` ([`crate::theme::WorldMood::id`]).
    ///
    /// Written since sidecar v2; absent in older files, which then resolve by
    /// index. Both are written so a sidecar stays readable by a build that
    /// predates ids — old sidecars load as `None` with no `SIDECAR_VERSION`
    /// bump needed, the same contract as `recipe` and `build`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mood_id: Option<String>,
    /// Integrated loudness (LUFS) for playback normalization; `None` when
    /// measurement failed (silence/too short).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loudness_lufs: Option<f32>,
    /// "Keep this world": a pinned mood that overrides `mood` (PLAN.md §5.3).
    /// Read through [`TrackAnalysis::pinned_mood_index`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_mood: Option<usize>,
    /// Stable id of `pinned_mood`, on the same contract as `mood_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_mood_id: Option<String>,
    /// The pinned layout seed — with `pinned_mood`, the full world identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_seed: Option<u64>,
    /// CLAP 512-d audio embedding (PLAN.md M5, `ml` feature). Present only
    /// when the model was available; `mood` then comes from the zero-shot
    /// tagger instead of the rule mapper. Also the hook for embedding-based
    /// asset selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,

    /// Demucs per-stem energy curves — drums/bass/vocals/other (PLAN.md M7,
    /// `ml` feature, same `ort` runtime as CLAP). Each entry is a per-second
    /// RMS envelope aligned to `energy`. `None` when the model is absent or
    /// unconfirmed (src/demucs.rs); the renderer then falls back to the mixed
    /// `energy` envelope. MIT-licensed model (shippable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stems: Option<crate::demucs::StemEnergy>,

    /// LLM-authored scene recipe (PLAN.md M7, `llm` feature). The top rung of
    /// the signal-ownership ladder; `None` when the model is absent or
    /// generation failed. The renderer keeps the rule-derived path when `None`
    /// or [`crate::recipe::WorldRecipe::is_empty`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe: Option<crate::recipe::WorldRecipe>,

    /// Agent-authored scene *build* — the ordered tool-calls Bonsai issued
    /// (PLAN.md M7, `llm` feature, agent path). Cached so the world is rebuilt
    /// deterministically on replay (no LLM re-run) and inspectable for debug.
    /// `None` when the agent feature/model is absent or the session was empty.
    /// Same serde-default contract as `recipe`: old sidecars without this field
    /// load as `None` (no `SIDECAR_VERSION` bump needed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<crate::agent_types::SceneBuild>,
}

impl TrackAnalysis {
    /// The live mood index, preferring the stable id over the stored position.
    ///
    /// Use this rather than reading `mood` directly: the raw field is only
    /// meaningful next to the mood list that was live when it was written.
    pub fn mood_index(&self) -> usize {
        crate::theme::resolve_mood(crate::theme::moods(), self.mood_id.as_deref(), self.mood)
    }

    /// The pinned mood as a live index, when "Keep this world" is set.
    pub fn pinned_mood_index(&self) -> Option<usize> {
        let pinned = self.pinned_mood?;
        Some(crate::theme::resolve_mood(
            crate::theme::moods(),
            self.pinned_mood_id.as_deref(),
            pinned,
        ))
    }

    /// Set the computed mood, keeping the index and its stable id in step.
    ///
    /// Only the CLAP tier revises a mood after analysis, so this has no caller
    /// without `ml`.
    #[cfg_attr(not(feature = "ml"), allow(dead_code))]
    pub fn set_mood(&mut self, index: usize) {
        self.mood = index;
        self.mood_id = Some(crate::theme::mood_id(index).to_string());
    }

    /// Set (or clear) the pinned mood, keeping both representations in step.
    pub fn set_pinned_mood(&mut self, index: Option<usize>) {
        self.pinned_mood = index;
        self.pinned_mood_id = index.map(|i| crate::theme::mood_id(i).to_string());
    }
}

pub(crate) fn cache_dir() -> Option<PathBuf> {
    let dir = dirs::data_local_dir()?
        .join("localgpt-verse")
        .join("analysis");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Content hash of the file (chunked blake3) — the sidecar filename and the
/// track's stable identity (`Track.id`, ARCHITECTURE R5).
pub(crate) fn cache_key(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(hasher.finalize().to_hex()[..32].to_string())
}

fn sidecar_path(key: &str) -> Option<PathBuf> {
    Some(cache_dir()?.join(format!("{key}.json")))
}

fn load_sidecar(key: &str) -> Option<TrackAnalysis> {
    let text = std::fs::read_to_string(sidecar_path(key)?).ok()?;
    serde_json::from_str(&text).ok()
}

fn save_sidecar(key: &str, analysis: &TrackAnalysis) {
    let Some(path) = sidecar_path(key) else {
        return;
    };
    match serde_json::to_string(analysis) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                warn!("Can't write analysis sidecar {}: {e}", path.display());
            }
        }
        Err(e) => warn!("Can't serialize analysis: {e}"),
    }
}

/// A track's analysis without the app: its sidecar, else the rules analysis
/// run now and cached — the worker's path minus the `ml` and LLM tiers.
pub(crate) fn analysis_for(key: &str, path: &Path) -> Option<TrackAnalysis> {
    if let Some(analysis) = load_sidecar(key) {
        return Some(analysis);
    }
    let analysis = analyze(path)?;
    save_sidecar(key, &analysis);
    Some(analysis)
}

/// The world a track imagines: the pinned mood ("Keep this world"), else the
/// analysed one.
pub(crate) fn world_mood(a: &TrackAnalysis) -> usize {
    a.pinned_mood_index().unwrap_or_else(|| a.mood_index())
}

/// Per-track layout seed: the pin wins, else the recipe's authored seed (M7,
/// when non-zero), else a deterministic default from the path (same song →
/// same place until re-rolled).
pub(crate) fn layout_seed(a: &TrackAnalysis, path: &Path) -> u64 {
    a.pinned_seed
        .or_else(|| a.recipe.as_ref().filter(|r| r.seed != 0).map(|r| r.seed))
        .unwrap_or_else(|| crate::world_assets::path_seed(path))
}

/// The recipe the world is placed from. A pinned seed overrides the recipe's
/// so "Keep this world" stays deterministic even with an LLM recipe.
pub(crate) fn world_recipe(a: &TrackAnalysis) -> Option<crate::recipe::WorldRecipe> {
    a.recipe.as_ref().map(|r| {
        let mut r = r.clone();
        if let Some(seed) = a.pinned_seed {
            r.seed = seed;
        }
        r
    })
}

// ---------------------------------------------------------------------------
// Decode (symphonia) → low-rate mono
// ---------------------------------------------------------------------------

/// Decode to mono at ~`max_sr` by boxcar decimation (native rate when
/// `max_sr` exceeds the source rate). Returns samples and the actual rate.
/// Used by the CLAP resampler (`ml` feature); loudness callers use
/// [`decode_mono_full`].
#[cfg_attr(not(feature = "ml"), allow(dead_code))]
pub(crate) fn decode_mono(path: &Path, max_sr: u32) -> Option<(Vec<f32>, u32)> {
    decode_mono_full(path, max_sr, false).map(|(s, sr, _)| (s, sr))
}

/// Decode to decimated mono; optionally measure integrated loudness (EBU
/// R128) on the *pre-decimation* mono at the source rate, where K-weighting
/// is accurate.
fn decode_mono_full(
    path: &Path,
    max_sr: u32,
    measure_loudness: bool,
) -> Option<(Vec<f32>, u32, Option<f32>)> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;
    let mut format = probed.format;
    let track = format.default_track()?;
    let track_id = track.id;
    let src_sr = track.codec_params.sample_rate?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .ok()?;

    let mut meter = if measure_loudness {
        ebur128::EbuR128::new(1, src_sr, ebur128::Mode::I).ok()
    } else {
        None
    };
    let mut meter_chunk: Vec<f32> = Vec::new();

    let decim = (src_sr / max_sr).max(1) as usize;
    let out_sr = src_sr / decim as u32;
    let max_samples = (MAX_ANALYSIS_SECS * out_sr as f32) as usize;

    let mut out: Vec<f32> = Vec::new();
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut acc = 0.0f32;
    let mut acc_n = 0usize;

    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            continue;
        };
        let spec = *decoded.spec();
        let needed = decoded.capacity() as u64;
        let buf = match &mut sample_buf {
            Some(b) if b.capacity() >= decoded.capacity() * spec.channels.count() => b,
            _ => sample_buf.insert(SampleBuffer::new(needed, spec)),
        };
        buf.copy_interleaved_ref(decoded);
        let channels = spec.channels.count().max(1);
        meter_chunk.clear();
        let mut hit_cap = false;
        for frame in buf.samples().chunks_exact(channels) {
            let mono: f32 = frame.iter().sum::<f32>() / channels as f32;
            if meter.is_some() {
                meter_chunk.push(mono);
            }
            acc += mono;
            acc_n += 1;
            if acc_n == decim {
                out.push(acc / decim as f32);
                acc = 0.0;
                acc_n = 0;
                if out.len() >= max_samples {
                    hit_cap = true;
                    break;
                }
            }
        }
        // Feed the loudness meter its (pre-decimation) mono for this packet.
        if let Some(m) = meter.as_mut() {
            let _ = m.add_frames_f32(&meter_chunk);
        }
        if hit_cap {
            break;
        }
    }
    if out.len() < out_sr as usize {
        return None; // under a second of audio — not worth analyzing
    }
    // -inf/NaN (silence, or too little gated content) → no measurement.
    let loudness = meter
        .and_then(|m| m.loudness_global().ok())
        .map(|l| l as f32)
        .filter(|l| l.is_finite() && *l > -70.0);
    Some((out, out_sr, loudness))
}

// ---------------------------------------------------------------------------
// DSP
// ---------------------------------------------------------------------------

/// Per-frame spectral features from a hop-by-hop FFT sweep.
struct SpectralSweep {
    /// Positive spectral flux per frame (onset novelty).
    novelty: Vec<f32>,
    /// Spectral centroid per frame, in Hz.
    centroid: Vec<f32>,
    /// Frames per second.
    rate: f32,
}

fn spectral_sweep(samples: &[f32], sr: u32) -> SpectralSweep {
    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let window: Vec<f32> = (0..FFT_SIZE)
        .map(|i| {
            let x = i as f32 / (FFT_SIZE - 1) as f32;
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * x).cos() // Hann
        })
        .collect();

    let bins = FFT_SIZE / 2 + 1;
    let hz_per_bin = sr as f32 / FFT_SIZE as f32;
    let mut prev_mag = vec![0.0f32; bins];
    let mut novelty = Vec::new();
    let mut centroid = Vec::new();

    let mut start = 0;
    while start + FFT_SIZE <= samples.len() {
        for (i, s) in input.iter_mut().enumerate() {
            *s = samples[start + i] * window[i];
        }
        if fft.process(&mut input, &mut spectrum).is_err() {
            break;
        }
        let mut flux = 0.0f32;
        let mut num = 0.0f32;
        let mut den = 0.0f32;
        for (i, c) in spectrum.iter().enumerate() {
            let mag = c.norm();
            flux += (mag - prev_mag[i]).max(0.0);
            num += i as f32 * hz_per_bin * mag;
            den += mag;
            prev_mag[i] = mag;
        }
        novelty.push(flux);
        centroid.push(if den > 1e-6 { num / den } else { 0.0 });
        start += HOP;
    }

    SpectralSweep {
        novelty,
        centroid,
        rate: sr as f32 / HOP as f32,
    }
}

/// Tempo from the autocorrelation of the novelty curve, with a mild prior
/// toward 90–140 BPM. Returns `(bpm, lag_frames)`, or `None` if the signal
/// has no periodicity worth trusting.
fn estimate_tempo(novelty: &[f32], rate: f32) -> Option<(f32, usize)> {
    if novelty.len() < (rate * 8.0) as usize {
        return None; // need ~8s of signal
    }
    let mean = novelty.iter().sum::<f32>() / novelty.len() as f32;
    let sig: Vec<f32> = novelty.iter().map(|v| (v - mean).max(0.0)).collect();

    let min_lag = (rate * 60.0 / 180.0) as usize; // 180 BPM
    let max_lag = ((rate * 60.0 / 55.0) as usize).min(sig.len() / 2); // 55 BPM
    if max_lag <= min_lag + 1 {
        return None;
    }
    // Prior-weighted autocorrelation across the whole lag range (prior is a
    // log-normal centred near 115 BPM, damping octave errors).
    let scores: Vec<f32> = (min_lag..=max_lag)
        .map(|lag| {
            let mut s = 0.0f32;
            for i in 0..sig.len() - lag {
                s += sig[i] * sig[i + lag];
            }
            s /= (sig.len() - lag) as f32;
            let bpm = 60.0 * rate / lag as f32;
            let prior = (-((bpm / 115.0).ln().powi(2)) / 0.45).exp();
            s * prior
        })
        .collect();

    let (peak_i, &peak) = scores
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())?;
    if peak <= 1e-9 {
        return None;
    }
    let lag = min_lag + peak_i;
    // Parabolic interpolation of the peak → sub-frame lag precision, so e.g.
    // a true 120 BPM (21.53 frames) isn't quantized to the 117.5/123 the
    // integer lags allow.
    let frac = if peak_i > 0 && peak_i + 1 < scores.len() {
        let (l, r) = (scores[peak_i - 1], scores[peak_i + 1]);
        let denom = l - 2.0 * peak + r;
        if denom.abs() > 1e-12 {
            (0.5 * (l - r) / denom).clamp(-0.5, 0.5)
        } else {
            0.0
        }
    } else {
        0.0
    };
    // Integer lag drives the beat-offset comb; the refined lag gives the bpm.
    Some((60.0 * rate / (lag as f32 + frac), lag))
}

/// Best beat phase for a fixed lag: the offset whose comb of grid points
/// collects the most novelty. Returns seconds.
fn estimate_beat_offset(novelty: &[f32], lag: usize, rate: f32) -> f32 {
    let mut best_phase = 0usize;
    let mut best_score = f32::MIN;
    for phase in 0..lag {
        let mut score = 0.0;
        let mut i = phase;
        while i < novelty.len() {
            score += novelty[i];
            i += lag;
        }
        if score > best_score {
            best_score = score;
            best_phase = phase;
        }
    }
    best_phase as f32 / rate
}

/// Section boundaries as fractions of the duration. Adjacent 4s windows of
/// (novelty, centroid, energy) means; boundaries where the feature distance
/// peaks, ≥15s apart, at most 5 (+ the implicit 0.0).
fn estimate_sections(sweep: &SpectralSweep, samples: &[f32], sr: u32, duration: f32) -> Vec<f32> {
    let win_frames = (4.0 * sweep.rate) as usize;
    if win_frames == 0 || sweep.novelty.len() < win_frames * 3 {
        return vec![0.0];
    }
    let win_samples = 4 * sr as usize;

    // Per-window features, each dimension normalized afterwards.
    let n_windows = sweep.novelty.len() / win_frames;
    let mut feats: Vec<[f32; 3]> = Vec::with_capacity(n_windows);
    for w in 0..n_windows {
        let f0 = w * win_frames;
        let nov: f32 = sweep.novelty[f0..f0 + win_frames].iter().sum::<f32>() / win_frames as f32;
        let cen: f32 = sweep.centroid[f0..f0 + win_frames].iter().sum::<f32>() / win_frames as f32;
        let s0 = (w * win_samples).min(samples.len());
        let s1 = ((w + 1) * win_samples).min(samples.len());
        let rms = if s1 > s0 {
            (samples[s0..s1].iter().map(|s| s * s).sum::<f32>() / (s1 - s0) as f32).sqrt()
        } else {
            0.0
        };
        feats.push([nov, cen, rms]);
    }
    for dim in 0..3 {
        let max = feats.iter().map(|f| f[dim]).fold(1e-9f32, f32::max);
        for f in &mut feats {
            f[dim] /= max;
        }
    }

    let scores: Vec<f32> = feats
        .windows(2)
        .map(|p| {
            let d = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
            (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
        })
        .collect();

    let min_gap_windows = 4; // 15s+ apart (windows are 4s)
    let picked = pick_peaks(&scores, min_gap_windows, 5);

    let mut sections = vec![0.0f32];
    for idx in picked {
        // Boundary between window idx and idx+1.
        let t = ((idx + 1) * win_frames) as f32 / sweep.rate;
        let frac = (t / duration.max(1.0)).clamp(0.0, 0.99);
        sections.push(frac);
    }
    sections.sort_by(|a, b| a.partial_cmp(b).unwrap());
    sections.dedup_by(|a, b| (*a - *b).abs() < 0.02);
    sections
}

/// Indices of up to `count` peaks above mean+0.5σ, greedily by height with a
/// minimum index separation.
fn pick_peaks(scores: &[f32], min_gap: usize, count: usize) -> Vec<usize> {
    if scores.is_empty() {
        return Vec::new();
    }
    let mean = scores.iter().sum::<f32>() / scores.len() as f32;
    let var = scores.iter().map(|s| (s - mean) * (s - mean)).sum::<f32>() / scores.len() as f32;
    let threshold = mean + 0.5 * var.sqrt();

    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].partial_cmp(&scores[a]).unwrap());

    let mut picked: Vec<usize> = Vec::new();
    for idx in order {
        if scores[idx] < threshold || picked.len() >= count {
            break;
        }
        if picked.iter().all(|&p| p.abs_diff(idx) >= min_gap) {
            picked.push(idx);
        }
    }
    picked.sort_unstable();
    picked
}

/// Per-second RMS loudness normalized to 0..1 by the 95th percentile.
fn energy_curve(samples: &[f32], sr: u32) -> Vec<f32> {
    let sec = sr as usize;
    if sec == 0 {
        return Vec::new();
    }
    let mut curve: Vec<f32> = samples
        .chunks(sec)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len().max(1) as f32).sqrt())
        .collect();
    let mut sorted = curve.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p95 = sorted[(sorted.len() as f32 * 0.95) as usize % sorted.len()].max(1e-6);
    for v in &mut curve {
        *v = (*v / p95).clamp(0.0, 1.0);
    }
    curve
}

/// The M4 quadrant mapping: arousal (tempo + loudness) × brightness
/// (spectral centroid) → one of the four moods. Documented in PLAN.md.
fn map_mood(bpm: f32, mean_energy: f32, mean_centroid_hz: f32) -> usize {
    let tempo_norm = ((bpm - 70.0) / 90.0).clamp(0.0, 1.0); // 70..160 BPM
    let arousal = 0.6 * tempo_norm + 0.4 * mean_energy;
    let bright = (mean_centroid_hz / 2500.0).clamp(0.0, 1.0);
    // Eight octants: the four base quadrants split by mean energy — the base
    // world for assertive tracks, its variant (same neighbourhood, different
    // hour: smoldering / bleached / still / first-light) for restrained ones.
    // The extended four borrow their base's asset set (ASSET_BASE_MOODS).
    match (arousal > 0.55, bright > 0.45, mean_energy > 0.5) {
        (true, true, true) => 1,    // VELVET CIRCUIT
        (true, true, false) => 5,   // MIRAGE CIRCUIT
        (true, false, true) => 0,   // EMBER FLATS
        (true, false, false) => 4,  // CINDER REACH
        (false, true, true) => 3,   // GLASS EXPANSE
        (false, true, false) => 7,  // DAWN EXPANSE
        (false, false, true) => 2,  //   TIDE GARDENS
        (false, false, false) => 6, //  ABYSS TERRACES
    }
}

/// Full pipeline for one file.
fn analyze(path: &Path) -> Option<TrackAnalysis> {
    let (samples, sr, loudness_lufs) = decode_mono_full(path, ANALYSIS_SR, true)?;
    let duration = samples.len() as f32 / sr as f32;
    let sweep = spectral_sweep(&samples, sr);

    let (bpm, beat_offset) = match estimate_tempo(&sweep.novelty, sweep.rate) {
        Some((bpm, lag)) => (bpm, estimate_beat_offset(&sweep.novelty, lag, sweep.rate)),
        None => (0.0, 0.0),
    };
    let sections = estimate_sections(&sweep, &samples, sr, duration);
    let energy = energy_curve(&samples, sr);
    let mean_energy = energy.iter().sum::<f32>() / energy.len().max(1) as f32;
    let mean_centroid = sweep.centroid.iter().sum::<f32>() / sweep.centroid.len().max(1) as f32;
    let mood = map_mood(bpm, mean_energy, mean_centroid);

    Some(TrackAnalysis {
        version: SIDECAR_VERSION,
        duration,
        bpm,
        beat_offset,
        sections,
        energy,
        centroid_hz: Some(mean_centroid),
        mood,
        mood_id: Some(crate::theme::mood_id(mood).to_string()),
        loudness_lufs,
        pinned_mood: None,
        pinned_mood_id: None,
        pinned_seed: None,
        embedding: None,
        stems: None,
        recipe: None,
        build: None,
    })
}

// ---------------------------------------------------------------------------
// Worker + Bevy plumbing
// ---------------------------------------------------------------------------

type WorkerResult = (String, TrackAnalysis);

/// Continuous timbre of the current track: normalized spectral brightness
/// (0 = dark timbre, 1 = bright), from the sidecar's mean centroid. The
/// palette wash nudges its hues by it — a warm-vintage track and a glassy
/// one in the same mood no longer paint identical worlds.
#[derive(Resource)]
pub struct Timbre(pub f32);

impl Default for Timbre {
    fn default() -> Self {
        Self(0.5)
    }
}

/// Continuous position between the discrete moods: which neighbour palette
/// the current world blends toward, and by how much. A track sitting near a
/// mapper decision boundary takes on some of the world it *almost* landed in
/// (up to 50% at the boundary itself, easing to nothing 0.12 away).
#[derive(Resource, Default)]
pub struct MoodBlend {
    /// The neighbour mood's index into [`crate::theme::moods()`].
    pub toward: usize,
    /// Blend amount 0..~0.5.
    pub amount: f32,
}

/// Analysis results by track id (blake3 content hash = sidecar key), plus the
/// worker channels. Id-keying makes analyses rename/move-proof in memory the
/// same way the sidecars are on disk (ARCHITECTURE R5).
#[derive(Resource)]
pub struct AnalysisStore {
    map: HashMap<String, TrackAnalysis>,
    pending: HashSet<String>,
    tx: Sender<(String, PathBuf)>,
    rx: Mutex<Receiver<WorkerResult>>,
    /// Kept so [`shutdown`](AnalysisStore::shutdown) can join the worker rather
    /// than merely dropping its channel. `None` only after shutdown took it.
    worker: Option<std::thread::JoinHandle<()>>,
    /// Set by [`shutdown`](AnalysisStore::shutdown); the worker's clone checks
    /// it between tracks, between model passes, and (per turn) inside the
    /// agent loop.
    cancel: WorkerCancel,
}

/// Cooperative cancellation for the analysis worker.
///
/// `shutdown` sets it; the worker checks it between tracks and between model
/// passes, and the agent session checks it between LLM turns. A generation
/// already inside mistral.rs cannot be interrupted — the checks bound how much
/// *more* work starts, never the turn in flight (which is what
/// [`SHUTDOWN_TIMEOUT`]'s detach backstop is for).
#[derive(Clone, Default)]
pub struct WorkerCancel(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl WorkerCancel {
    fn cancel(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    /// Whether shutdown was requested. Checked cooperatively; never blocks.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// What the analysis worker needs from the app.
///
/// Passed in at construction so the worker owns no globals: it holds exactly
/// what it was handed, and dropping the store drops those handles with it.
#[derive(Default)]
pub struct WorkerDeps {
    /// The agent bridge for the M7 recipe tier. `None` leaves the tier off.
    #[cfg(feature = "llm")]
    pub agent_bridge: Option<std::sync::Arc<crate::agent::AgentBridge>>,
}

impl WorkerDeps {
    /// Collect the worker's dependencies from what the plugins published.
    ///
    /// Under `llm` this reads the bridge that `AgentPlugin` inserted, so the
    /// worker is handed a live handle instead of reaching for a global. Absent
    /// resource means the tier stays off, which is the same graceful
    /// degradation as a missing model file.
    #[cfg_attr(not(feature = "llm"), allow(unused_variables))]
    pub fn from_world(world: &World) -> Self {
        Self {
            #[cfg(feature = "llm")]
            agent_bridge: world
                .get_resource::<crate::agent::AgentBridgeHandle>()
                .map(|handle| handle.0.clone()),
        }
    }
}

impl AnalysisStore {
    /// Start the analysis worker and return the store that owns it.
    ///
    /// Construction is explicit rather than `Default` because it spawns a
    /// thread that can come to own the CLAP, demucs, and recipe models —
    /// hundreds of MB of state that the caller must be able to release. Pair
    /// every `spawn` with [`shutdown`](AnalysisStore::shutdown), or mount it in
    /// a [`crate::scope::Scope`] that does.
    ///
    /// The models load on the first track that needs one, not here; a library
    /// whose sidecars are already written never loads any. See [`crate::tier`].
    pub fn spawn(deps: WorkerDeps) -> Self {
        let (req_tx, req_rx) = channel::<(String, PathBuf)>();
        let (res_tx, res_rx) = channel::<WorkerResult>();
        let cancel = WorkerCancel::default();
        let worker_cancel = cancel.clone();
        let worker = std::thread::spawn(move || {
            // Moved in so the worker owns its dependencies for its whole life.
            // Without `llm` the struct is empty; destructuring consumes it so
            // there is still one construction path and no unused binding.
            #[cfg(not(feature = "llm"))]
            let WorkerDeps {} = deps;
            // The tiers load on the first track that actually needs them, not
            // here — a library whose sidecars are already written never loads a
            // model at all. See `crate::tier`.
            //
            // M5: one CLAP model per worker thread (stays unavailable without
            // the model file — rules keep running either way).
            #[cfg(feature = "ml")]
            let mut clap = crate::tier::Tier::<crate::ml::ClapModel>::Cold;
            // M7 stem tier (rides the same `ort` runtime as CLAP). Below the
            // recipe tier: stems are an input feature the recipe LLM consumes.
            #[cfg(feature = "ml")]
            let mut stems = crate::tier::Tier::<crate::demucs::StemModel>::Cold;
            // M7 recipe tier — top rung. `llm` feature + model file required.
            #[cfg(feature = "llm")]
            let mut recipe_model = crate::tier::Tier::<crate::llm::RecipeModel>::Cold;
            for (id, path) in req_rx {
                // Shutdown drains the request channel first, so a request
                // still in flight when cancel landed is dropped here rather
                // than starting minutes of model work nothing will read.
                if worker_cancel.is_cancelled() {
                    break;
                }
                let analysis = match load_sidecar(&id) {
                    Some(a) => a,
                    None => {
                        let started = std::time::Instant::now();
                        let Some(a) = analyze(&path) else {
                            warn!("Analysis failed for {}", path.display());
                            continue;
                        };
                        info!(
                            "Analyzed {} — {:.0} BPM, {} sections ({:.1}s)",
                            path.display(),
                            a.bpm,
                            a.sections.len(),
                            started.elapsed().as_secs_f32()
                        );
                        save_sidecar(&id, &a);
                        a
                    }
                };
                // M5 upgrade pass: a sidecar with no embedding gets one (plus
                // the zero-shot mood) even if rules analyzed it earlier.
                // The `embedding.is_none()` test comes first so a track that
                // already has one never triggers the 78 MB load.
                //
                // CLAP votes among the four base quadrants; the extended
                // variants are the same quadrant split by mean energy (the
                // mapper's third axis), folded in here so both mappers agree.
                #[cfg(feature = "ml")]
                let analysis = if analysis.embedding.is_none()
                    && let Some(model) = clap.get_or_load(crate::ml::ClapModel::try_load)
                {
                    match model.analyze_track(&path) {
                        Some((embedding, mood)) => {
                            info!("CLAP embedded {} → mood {mood}", path.display());
                            let mut a = analysis;
                            let variant = mood_variant(mood, mean_energy_of(&a));
                            a.set_mood(variant);
                            a.embedding = Some(embedding);
                            save_sidecar(&id, &a);
                            a
                        }
                        None => analysis,
                    }
                } else {
                    analysis
                };
                // M7 stem upgrade pass: a sidecar with no stem curves gets them
                // (separate rung from CLAP; reuses the same `ort` runtime).
                #[cfg(feature = "ml")]
                let analysis = if analysis.stems.is_none()
                    && let Some(model) = stems.get_or_load(crate::demucs::StemModel::try_load)
                {
                    match model.analyze_track(&path) {
                        Some(curves) => {
                            info!("Demucs separated {} → 4 stems", path.display());
                            let mut a = analysis;
                            a.stems = Some(curves);
                            save_sidecar(&id, &a);
                            a
                        }
                        None => analysis,
                    }
                } else {
                    analysis
                };
                // M7 recipe upgrade pass — the top rung. Only when the LLM
                // feature + model are present, the sidecar lacks a recipe, and
                // shutdown hasn't been requested (a cancelled pass would be a
                // multi-minute generation nothing will read).
                #[cfg(feature = "llm")]
                let analysis = if analysis.recipe.is_none()
                    && !worker_cancel.is_cancelled()
                    && let Some(model) = recipe_model.get_or_load(crate::llm::RecipeModel::try_load)
                {
                    match model.generate(&analysis) {
                        Some(recipe) => {
                            info!(
                                "LLM authored recipe \"{}\" for {}",
                                recipe.world_name,
                                path.display()
                            );
                            let mut a = analysis;
                            a.recipe = Some(recipe);
                            save_sidecar(&id, &a);
                            a
                        }
                        None => analysis,
                    }
                } else {
                    analysis
                };
                // M7 agent path — the gen-style alternative to the static
                // recipe. The model calls tools (spawn_primitive, place_asset,
                // set_light, ...) to construct the world entity-by-entity,
                // issuing commands through the bridge handed to this worker at
                // construction, which the Bevy executor drains each frame.
                // Skipped when:
                //   - no bridge was supplied (feature off / plugin absent)
                //   - the model is absent
                //   - a build is already cached (build.is_none() gate) — replay
                //     handles it instead, no LLM re-run.
                // The resulting SceneBuild is persisted to the sidecar so the
                // world is rebuilt deterministically on replay and the command
                // stream is inspectable for debugging.
                //
                // Same tier as the recipe pass above, so whichever runs first
                // pays the load and the other reuses it. The manifest is read
                // from disk (the worker can't reach the Bevy-side resource) so
                // the agent's `place_asset` tool carries the real asset
                // vocabulary; absent manifest → primitives only. The cancel
                // handle threads through so a session in flight stops at the
                // next turn boundary (see `run_session`).
                #[cfg(feature = "llm")]
                let analysis = if analysis.build.is_none()
                    && !worker_cancel.is_cancelled()
                    && let Some(bridge) = deps.agent_bridge.clone()
                    && let Some(model) = recipe_model.get_or_load(crate::llm::RecipeModel::try_load)
                {
                    let m = model.model_mut();
                    let manifest = crate::world_assets::read_manifest_from_disk();
                    match crate::agent::run_session(
                        m,
                        bridge,
                        &analysis,
                        &id,
                        manifest.as_ref(),
                        &worker_cancel,
                    ) {
                        Some(build) => {
                            if let Some(d) = &build.description {
                                info!("Agent described its world for {}: {d}", path.display());
                            }
                            info!(
                                "Agent built {} commands for {}",
                                build.commands.len(),
                                path.display()
                            );
                            let mut a = analysis;
                            a.build = Some(build);
                            save_sidecar(&id, &a);
                            a
                        }
                        None => analysis,
                    }
                } else {
                    analysis
                };
                if res_tx.send((id, analysis)).is_err() {
                    return;
                }
            }
        });
        Self {
            map: HashMap::new(),
            pending: HashSet::new(),
            tx: req_tx,
            rx: Mutex::new(res_rx),
            worker: Some(worker),
            cancel,
        }
    }

    /// How long [`AnalysisStore::shutdown`] waits for the worker before
    /// detaching it. Generous for the cooperative cases (idle, mid-decode,
    /// between LLM turns); a single LLM generation already inside mistral.rs
    /// can exceed it — that is exactly the case the detach covers.
    const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

    /// Stop the worker, waiting at most [`SHUTDOWN_TIMEOUT`].
    ///
    /// Cooperative first: the [`WorkerCancel`] flag makes a worker that is
    /// idle, decoding, or between model passes exit promptly, and a running
    /// agent session stop at its next turn boundary. A generation already
    /// inside mistral.rs cannot be interrupted, so after the bounded wait the
    /// worker is *detached* — it finishes its turn, fails its (closed) result
    /// channel, and releases the models on its own. The `Err` returned on a
    /// detach is a diagnostic (the scope machinery logs it); teardown proceeds
    /// either way.
    pub fn shutdown(mut self) -> Result<(), String> {
        let Some(worker) = self.worker.take() else {
            return Ok(());
        };
        self.cancel.cancel();
        // Dropping `self` closes the request channel (ending the loop) and the
        // result channel (so an in-flight send returns Err and the worker
        // exits instead of writing into a dead store).
        drop(self);

        // Join on a helper thread so the wait can be bounded: std has no
        // timed join, and an unbounded one is what this whole method exists
        // to avoid. The join's Result travels through the channel; on timeout
        // the helper (and worker) are simply left to finish unnoticed.
        let (done_tx, done_rx) = channel::<Result<(), String>>();
        std::thread::spawn(move || {
            let _ = done_tx.send(
                worker
                    .join()
                    .map_err(|_| "analysis worker panicked".to_string()),
            );
        });
        match done_rx.recv_timeout(Self::SHUTDOWN_TIMEOUT) {
            Ok(result) => result,
            Err(_) => Err(format!(
                "analysis worker detached: still finishing an LLM turn after {}s — \
                 it exits and releases its models on its own",
                Self::SHUTDOWN_TIMEOUT.as_secs()
            )),
        }
    }
}

impl AnalysisStore {
    /// The full analysis for a track id, when it's been computed. Used by the
    /// agent replay path to read a cached `SceneBuild` without re-running the LLM.
    #[allow(dead_code)]
    pub fn get(&self, id: &str) -> Option<&TrackAnalysis> {
        self.map.get(id)
    }

    /// First-beat offset for a track, when analyzed and a grid was found.
    /// Used to land the materialize sequence on the first downbeat.
    pub fn beat_offset_for(&self, id: &str) -> Option<f32> {
        self.map
            .get(id)
            .filter(|a| a.bpm > 0.0)
            .map(|a| a.beat_offset)
    }

    /// The track's Demucs stem curves, when the `ml` separation pass ran.
    /// `[drums, bass, vocals, other]`, per-second, aligned to `energy`.
    pub fn stems_for(&self, id: &str) -> Option<&crate::demucs::StemEnergy> {
        self.map.get(id).and_then(|a| a.stems.as_ref())
    }

    /// Normalization gain in **decibels** for `id` toward [`TARGET_LUFS`],
    /// clamped to ±12 dB so a mismeasured quiet track can't blast. `0.0` (no
    /// change) when the track isn't analyzed yet or loudness was unmeasurable.
    pub fn norm_db_for(&self, id: Option<&str>) -> f32 {
        id.and_then(|id| self.map.get(id))
            .and_then(|a| a.loudness_lufs)
            .map(|lufs| (TARGET_LUFS - lufs).clamp(-12.0, 12.0))
            .unwrap_or(0.0)
    }

    fn request(&mut self, id: &str, path: &Path) {
        if self.map.contains_key(id) || self.pending.contains(id) {
            return;
        }
        self.pending.insert(id.to_string());
        let _ = self.tx.send((id.to_string(), path.to_path_buf()));
    }

    /// Toggle the "Keep this world" pin for `id` at `mood`. Returns the new
    /// pin state, or `None` when the track has no analysis yet.
    pub fn toggle_pin(&mut self, id: &str, mood: usize, seed: u64) -> Option<bool> {
        let analysis = self.map.get_mut(id)?;
        let pinned = if analysis.pinned_mood.is_some() {
            analysis.set_pinned_mood(None);
            analysis.pinned_seed = None;
            false
        } else {
            analysis.set_pinned_mood(Some(mood));
            analysis.pinned_seed = Some(seed);
            true
        };
        save_sidecar(id, analysis);
        Some(pinned)
    }
}

/// The scope name the analysis worker mounts under.
pub const ANALYSIS_SCOPE: &str = "analysis";

/// Mount the analysis worker as a disposable unit.
///
/// Unmounting `ANALYSIS_SCOPE` removes the store and joins the worker, which
/// releases the models it holds. This is the app's demonstration of the
/// registration-carries-its-undo rule: the resource and the thread are
/// registered together, so they cannot be released apart.
pub fn mount_analysis(world: &mut World, deps: WorkerDeps) {
    crate::scope::mount(world, ANALYSIS_SCOPE, |scope, world| {
        world.insert_resource(AnalysisStore::spawn(deps));
        scope.defer("AnalysisStore + worker thread", |world| {
            let Some(store) = world.remove_resource::<AnalysisStore>() else {
                return Ok(());
            };
            store.shutdown()
        });
    });
}

/// Request analysysis for the current + next few tracks, drain worker results,
/// and apply the current track's analysis to the transport/beat/theme exactly
/// once per (track, availability) state.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn sync_analysis(
    mut store: ResMut<AnalysisStore>,
    mut playback: ResMut<Playback>,
    mut beat: ResMut<Beat>,
    mut theme: ResMut<Theme>,
    mut layout: ResMut<crate::world_assets::WorldLayout>,
    mut active_recipe: ResMut<crate::recipe::ActiveRecipe>,
    mut timbre: ResMut<Timbre>,
    mut blend: ResMut<MoodBlend>,
    mut applied: Local<Option<(Option<String>, bool)>>,
) {
    if playback.queue.is_empty() {
        return;
    }

    // Keep the current and next few tracks in flight ("the next world is
    // prepared quietly" — queue panel). A deeper lookahead means back-to-back
    // skips still land on analyzed tracks (sections/downbeat/mood ready).
    let idx = playback.current % playback.queue.len();
    let requests: Vec<(String, PathBuf)> = (0..LOOKAHEAD)
        .map(|ahead| (idx + ahead) % playback.queue.len())
        .filter_map(|i| match (&playback.queue[i].id, &playback.queue[i].path) {
            (Some(id), Some(path)) => Some((id.clone(), path.clone())),
            _ => None,
        })
        .collect();
    for (id, path) in requests {
        store.request(&id, &path);
    }

    // Drain results.
    let mut arrived = Vec::new();
    {
        let rx = store.rx.lock().unwrap();
        while let Ok(r) = rx.try_recv() {
            arrived.push(r);
        }
    }
    for (id, analysis) in arrived {
        store.pending.remove(&id);
        store.map.insert(id, analysis);
    }

    // Apply to the live signals when the current track (keyed by content id,
    // so a queue replacement or reorder is caught) or its analysis
    // availability changes.
    let current = &playback.queue[idx];
    let current_id = current.id.clone();
    let current_path = current.path.clone();
    let has = current_id
        .as_ref()
        .is_some_and(|id| store.map.contains_key(id));
    if applied.as_ref() == Some(&(current_id.clone(), has)) {
        return;
    }
    *applied = Some((current_id.clone(), has));

    match current_id.as_ref().and_then(|id| store.map.get(id)) {
        Some(a) => {
            playback.sections = a.sections.clone();
            if a.bpm > 0.0 {
                beat.bpm = a.bpm;
                beat.offset = a.beat_offset;
                beat.grid = true;
            } else {
                beat.grid = false;
            }
            let mood = world_mood(a);
            layout.seed = layout_seed(a, current_path.as_ref().unwrap());
            playback.queue[idx].mood = mood;
            theme.mood = mood;
            // Continuous timbre + mood blend (the "almost landed in"
            // neighbourhood). Distances use the same axes/thresholds as
            // `map_mood` so the blend is the mapper's own uncertainty.
            timbre.0 = (a.centroid_hz.unwrap_or(1250.0) / 2500.0).clamp(0.0, 1.0);
            let mean_energy = mean_energy_of(a);
            let tempo_norm = ((a.bpm - 70.0) / 90.0).clamp(0.0, 1.0);
            let arousal = 0.6 * tempo_norm + 0.4 * mean_energy;
            let base = mood % crate::theme::ASSET_BASE_MOODS;
            let (toward_base, dist) = if (bright_dist(timbre.0)).abs() < (arousal - 0.55).abs() {
                // Closer to the brightness boundary → flip bright/dark.
                (base ^ 1, (bright_dist(timbre.0)).abs())
            } else {
                // Closer to the arousal boundary → flip driving/calm.
                (base ^ 2, (arousal - 0.55).abs())
            };
            blend.toward = toward_base;
            blend.amount = ((0.12 - dist) / 0.12).clamp(0.0, 1.0) * 0.5;
            // M7: push the track's recipe (if any) to the live resource the
            // renderer reads. The choreography is resolved against this
            // analysis' measured sections/energy here — the renderer only
            // sees indices.
            active_recipe.recipe = world_recipe(a);
            active_recipe.moments = a
                .recipe
                .as_ref()
                .map(|r| resolve_section_moments(a, &r.section_choreography))
                .unwrap_or_default();
        }
        None => {
            // Demo track (no path) keeps its authored mock sections; a real
            // file with analysis still pending shows a clean bar until it lands.
            if let Some(path) = &current_path {
                playback.sections.clear();
                layout.seed = crate::world_assets::path_seed(path);
            }
            beat.grid = false;
            active_recipe.recipe = None;
            active_recipe.moments.clear();
            timbre.0 = 0.5;
            blend.amount = 0.0;
        }
    }
}

/// Signed distance of a normalized brightness from the mapper's 0.45
/// boundary (kept as a helper so the blend and `map_mood` can't drift).
fn bright_dist(bright: f32) -> f32 {
    bright - 0.45
}

/// Resolve the recipe's section choreography onto measured segment indices
/// (0-based, aligned with the boundaries in `TrackAnalysis::sections` — see
/// `world::sync_section_moment` for the segmentation). Positional heuristics
/// anchored by the energy curve: intro/outro take the ends, chorus/drop claim
/// the loudest remaining segments, verse the quietest, bridge the middle of
/// what's left. Each segment hosts at most one moment, so a role never
/// silently overwrites another.
fn resolve_section_moments(
    analysis: &TrackAnalysis,
    choreography: &[crate::recipe::SectionMoment],
) -> Vec<(usize, crate::recipe::SectionMoment)> {
    use crate::recipe::SectionRole as R;
    let n = analysis.sections.len().max(1);
    let energy: Vec<f32> = (0..n).map(|i| segment_energy(analysis, i)).collect();

    let mut taken: Vec<usize> = Vec::new();
    let mut out = Vec::new();
    for m in choreography {
        let remaining: Vec<usize> = (0..n).filter(|i| !taken.contains(i)).collect();
        let Some(&first_free) = remaining.first() else {
            break; // more moments than segments — the rest have nowhere to land
        };
        let idx = match m.at_role {
            R::Intro => 0,
            R::Outro => n - 1,
            R::Chorus | R::Drop => remaining
                .iter()
                .max_by(|a, b| energy[**a].total_cmp(&energy[**b]))
                .copied()
                .unwrap_or(first_free),
            R::Verse => remaining
                .iter()
                .min_by(|a, b| energy[**a].total_cmp(&energy[**b]))
                .copied()
                .unwrap_or(first_free),
            R::Bridge => remaining[remaining.len() / 2],
        };
        taken.push(idx);
        out.push((idx, m.clone()));
    }
    out
}

/// Mean of the per-second energy curve (0.0 when empty).
fn mean_energy_of(a: &TrackAnalysis) -> f32 {
    a.energy.iter().sum::<f32>() / a.energy.len().max(1) as f32
}

/// The CLAP zero-shot vote lands on a base quadrant; the extended variant of
/// the same neighbourhood applies when the track's energy is restrained —
/// the mapper's third axis (`map_mood`'s `mean_energy > 0.5`), so the rule
/// and CLAP paths resolve to the same eight worlds.
// Called by the CLAP vote under `ml`, and by its own test. Without this the
// default-feature *binary* build sees no caller and warns; Verse's own CI hid
// that by only ever linting with `--tests`.
#[cfg(any(feature = "ml", test))]
fn mood_variant(base: usize, mean_energy: f32) -> usize {
    if mean_energy <= 0.5 {
        (base % crate::theme::ASSET_BASE_MOODS) + crate::theme::ASSET_BASE_MOODS
    } else {
        base % crate::theme::ASSET_BASE_MOODS
    }
}

/// Per-stem level 0..1 at `t` seconds (`[drums, bass, vocals, other]`). A
/// curve shorter than the playhead holds its last value — a truncated
/// analysis, not silence; an empty curve reads as 0.
pub(crate) fn sample_stems(stems: &crate::demucs::StemEnergy, t: f32) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (slot, curve) in out.iter_mut().zip(stems.iter()) {
        if !curve.is_empty() {
            let idx = (t.max(0.0).floor() as usize).min(curve.len() - 1);
            *slot = curve[idx].clamp(0.0, 1.0);
        }
    }
    out
}

/// Sample the current track's Demucs stem curves at the playhead into
/// [`crate::playback::StemLevels`], one-pole smoothed so the per-second
/// resolution doesn't step. Without stems (no `ml` feature, or a sidecar
/// written without them) the levels ease to zero and consumers fall back to
/// the live-tap band envelopes.
pub fn update_stem_levels(
    store: Res<AnalysisStore>,
    playback: Res<Playback>,
    time: Res<Time>,
    mut levels: ResMut<crate::playback::StemLevels>,
) {
    let target = playback
        .queue
        .get(playback.current % playback.queue.len().max(1))
        .and_then(|t| t.id.as_deref())
        .and_then(|id| store.stems_for(id))
        .map(|s| sample_stems(s, playback.elapsed))
        .unwrap_or([0.0; 4]);
    let k = (time.delta_secs() * 6.0).min(1.0);
    for (level, t) in levels.0.iter_mut().zip(target) {
        *level += (t - *level) * k;
    }
}

/// Mean energy of segment `i` (the fraction window between boundary `i` and
/// `i+1`), from the per-second energy curve. 0.0 when the curve is empty.
fn segment_energy(analysis: &TrackAnalysis, i: usize) -> f32 {
    let e = &analysis.energy;
    if e.is_empty() {
        return 0.0;
    }
    let dur = analysis.duration.max(1.0);
    let start_frac = analysis.sections.get(i).copied().unwrap_or(0.0);
    let end_frac = analysis
        .sections
        .get(i + 1)
        .copied()
        .unwrap_or(1.0)
        .max(start_frac);
    let start = start_frac * dur;
    let end = end_frac * dur;
    let s0 = (start.floor() as usize).min(e.len() - 1);
    let s1 = ((end.ceil() as usize).max(s0 + 1)).min(e.len());
    let slice = &e[s0..s1];
    if slice.is_empty() {
        0.0
    } else {
        slice.iter().sum::<f32>() / slice.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipe::{SectionMoment, SectionRole};

    /// Synthetic novelty with impulses every `period` frames.
    fn click_novelty(period: usize, phase: usize, len: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; len];
        let mut i = phase;
        while i < len {
            v[i] = 1.0;
            i += period;
        }
        v
    }

    #[test]
    fn tempo_recovers_120_bpm() {
        // 43.066 frames/sec (11025/256); 120 BPM → beat every 0.5s ≈ 21.5 fr.
        let rate = ANALYSIS_SR as f32 / HOP as f32;
        let period = (rate * 0.5).round() as usize;
        let novelty = click_novelty(period, 3, (rate * 60.0) as usize);
        let (bpm, _lag) = estimate_tempo(&novelty, rate).expect("tempo found");
        assert!((bpm - 120.0).abs() < 3.0, "got {bpm}");
    }

    #[test]
    fn beat_offset_recovers_phase() {
        let rate = ANALYSIS_SR as f32 / HOP as f32;
        let period = (rate * 0.5).round() as usize;
        let phase = 10usize;
        let novelty = click_novelty(period, phase, (rate * 30.0) as usize);
        let offset = estimate_beat_offset(&novelty, period, rate);
        assert!((offset - phase as f32 / rate).abs() < 0.02, "got {offset}");
    }

    #[test]
    fn peaks_respect_gap_and_count() {
        let scores = vec![0.0, 0.0, 5.0, 0.0, 0.0, 8.0, 0.0, 0.1];
        let picked = pick_peaks(&scores, 2, 2);
        assert_eq!(picked, vec![2, 5]);
    }

    #[test]
    fn mood_octants() {
        // The four base quadrants (assertive energy).
        assert_eq!(map_mood(150.0, 0.8, 4000.0), 1); // driving + bright → VELVET
        assert_eq!(map_mood(150.0, 0.8, 500.0), 0); //  driving + dark → EMBER
        assert_eq!(map_mood(70.0, 0.6, 4000.0), 3); //  calm + bright → GLASS
        assert_eq!(map_mood(70.0, 0.6, 500.0), 2); //   calm + dark → TIDE
        // …and their restrained variants (same neighbourhood, different hour).
        assert_eq!(map_mood(150.0, 0.3, 4000.0), 5); // driving + bright → MIRAGE
        assert_eq!(map_mood(150.0, 0.3, 500.0), 4); //  driving + dark → CINDER
        assert_eq!(map_mood(70.0, 0.3, 4000.0), 7); //  calm + bright → DAWN
        assert_eq!(map_mood(70.0, 0.3, 500.0), 6); //   calm + dark → ABYSS
    }

    #[test]
    fn extended_moods_borrow_their_base_quadrant() {
        // The asset-borrowing invariant the extended moods rely on.
        for extended in 4..8 {
            let base = extended % crate::theme::ASSET_BASE_MOODS;
            assert!(base < crate::theme::ASSET_BASE_MOODS);
            // Same bright/dark family: base^1 flips brightness, base^2 the
            // drive — both stay inside the asset-tagged four.
            assert!(base ^ 1 < 4);
            assert!(base ^ 2 < 4);
        }
    }

    fn moment(role: SectionRole) -> SectionMoment {
        SectionMoment {
            at_role: role,
            ..Default::default()
        }
    }

    fn choreo_analysis() -> TrackAnalysis {
        // 40 s, 4 segments (0.0/0.25/0.5/0.75): quiet, loud, quiet, mid.
        TrackAnalysis {
            version: SIDECAR_VERSION,
            duration: 40.0,
            bpm: 120.0,
            beat_offset: 0.0,
            sections: vec![0.0, 0.25, 0.5, 0.75],
            energy: {
                let mut e = vec![0.1; 10]; // 0–10 s: quiet intro
                e.extend(vec![0.9; 10]); //  10–20 s: loud
                e.extend(vec![0.2; 10]); //  20–30 s: quiet
                e.extend(vec![0.5; 10]); //  30–40 s: mid
                e
            },
            centroid_hz: None,
            mood: 0,
            mood_id: None,
            loudness_lufs: None,
            pinned_mood: None,
            pinned_mood_id: None,
            pinned_seed: None,
            embedding: None,
            stems: None,
            recipe: None,
            build: None,
        }
    }

    #[test]
    fn chorus_lands_on_the_loudest_segment() {
        let a = choreo_analysis();
        let out = resolve_section_moments(&a, &[moment(SectionRole::Chorus)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, 1, "segment 1 (10–20 s) is the loudest");
    }

    #[test]
    fn intro_outro_take_the_ends_and_never_collide() {
        let a = choreo_analysis();
        let out = resolve_section_moments(
            &a,
            &[moment(SectionRole::Intro), moment(SectionRole::Outro)],
        );
        assert_eq!(out[0].0, 0);
        assert_eq!(out[1].0, 3);
    }

    #[test]
    fn verse_and_drop_split_quiet_and_loud() {
        let a = choreo_analysis();
        let out =
            resolve_section_moments(&a, &[moment(SectionRole::Drop), moment(SectionRole::Verse)]);
        assert_eq!(out[0].0, 1, "drop → loudest remaining (segment 1)");
        assert_eq!(out[1].0, 0, "verse → quietest remaining (segment 0)");
    }

    #[test]
    fn more_moments_than_segments_stop_cleanly() {
        let a = choreo_analysis();
        let roles = [
            SectionRole::Intro,
            SectionRole::Chorus,
            SectionRole::Verse,
            SectionRole::Bridge,
            SectionRole::Outro,
        ];
        let out =
            resolve_section_moments(&a, &roles.iter().map(|r| moment(*r)).collect::<Vec<_>>());
        assert_eq!(out.len(), 4, "one moment per segment, no duplicates");
        let mut idxs: Vec<usize> = out.iter().map(|(i, _)| *i).collect();
        idxs.sort_unstable();
        idxs.dedup();
        assert_eq!(idxs.len(), out.len(), "no two moments share a segment");
    }

    #[test]
    fn empty_sections_collapse_to_one_segment() {
        let mut a = choreo_analysis();
        a.sections = vec![0.0];
        let out = resolve_section_moments(
            &a,
            &[moment(SectionRole::Intro), moment(SectionRole::Chorus)],
        );
        assert_eq!(out.len(), 1, "second moment has nowhere to land");
        assert_eq!(out[0].0, 0);
    }

    #[test]
    fn mood_variant_splits_base_by_energy() {
        assert_eq!(mood_variant(1, 0.8), 1, "assertive stays the base quadrant");
        assert_eq!(mood_variant(1, 0.3), 5, "restrained lands in the variant");
        assert_eq!(mood_variant(2, 0.3), 6);
        // Out-of-range bases (a manifest grown past 4) degrade to base range.
        assert_eq!(mood_variant(9, 0.8), 1);
    }

    #[test]
    fn stems_sample_by_second_and_hold_their_last_value() {
        let stems = [vec![0.1, 0.9], vec![], vec![0.5; 200], vec![0.0, 0.7]];
        let at = |t: f32| sample_stems(&stems, t);
        assert_eq!(at(0.0), [0.1, 0.0, 0.5, 0.0], "floor(t) indexes each curve");
        assert_eq!(at(1.4), [0.9, 0.0, 0.5, 0.7]);
        // Past the end of a short curve: hold last, not silence.
        assert_eq!(at(9.9), [0.9, 0.0, 0.5, 0.7]);
        // Negative time (seek before 0) reads the first value.
        assert_eq!(at(-3.0)[0], 0.1);
    }

    #[test]
    fn stems_sample_clamps_out_of_range_values() {
        let stems = [vec![2.0, -1.0], vec![0.0; 3], vec![0.0; 3], vec![0.0; 3]];
        assert_eq!(sample_stems(&stems, 0.0)[0], 1.0);
        assert_eq!(sample_stems(&stems, 1.0)[0], 0.0);
    }

    #[test]
    fn energy_curve_normalized() {
        let sr = 100u32;
        let mut samples = vec![0.1f32; 500];
        samples.extend(vec![0.9f32; 500]);
        let curve = energy_curve(&samples, sr);
        assert_eq!(curve.len(), 10);
        assert!(curve.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(curve[9] > curve[0]);
    }

    // -----------------------------------------------------------------------
    // Worker lifecycle
    // -----------------------------------------------------------------------

    #[test]
    fn unmounting_removes_the_store_and_joins_the_worker() {
        let mut world = World::new();

        mount_analysis(&mut world, WorkerDeps::default());
        assert!(world.contains_resource::<AnalysisStore>());
        assert!(
            world
                .resource::<crate::scope::Scopes>()
                .is_mounted(ANALYSIS_SCOPE)
        );

        crate::scope::unmount(&mut world, ANALYSIS_SCOPE);

        // The disposer joined the worker before returning, so by here the
        // thread has exited and the models it owned are released.
        assert!(!world.contains_resource::<AnalysisStore>());
        assert!(
            !world
                .resource::<crate::scope::Scopes>()
                .is_mounted(ANALYSIS_SCOPE)
        );
    }

    #[test]
    fn the_worker_can_be_remounted_after_unmounting() {
        let mut world = World::new();

        for _ in 0..3 {
            mount_analysis(&mut world, WorkerDeps::default());
            assert!(world.contains_resource::<AnalysisStore>());
            crate::scope::unmount(&mut world, ANALYSIS_SCOPE);
            assert!(!world.contains_resource::<AnalysisStore>());
        }
    }

    #[test]
    fn shutdown_joins_a_worker_with_no_requests_in_flight() {
        let store = AnalysisStore::spawn(WorkerDeps::default());
        assert!(store.shutdown().is_ok());
    }

    #[test]
    fn a_cancel_flag_is_unset_until_cancelled_and_shared_across_clones() {
        let cancel = WorkerCancel::default();
        let worker_view = cancel.clone();
        assert!(!worker_view.is_cancelled());

        cancel.cancel();
        assert!(worker_view.is_cancelled(), "the worker's view sees it");
    }

    #[test]
    fn shutdown_is_prompt_for_an_idle_worker() {
        // The cooperative path: no work in flight, so the worker notices the
        // flag and exits well inside the 15s bound. A slow path (mid-LLM-turn
        // detach) needs a real model and is covered by the bounded-join
        // design rather than a unit test.
        let store = AnalysisStore::spawn(WorkerDeps::default());
        let started = std::time::Instant::now();
        assert!(store.shutdown().is_ok());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "idle shutdown took {:?} — the cancel flag isn't being observed",
            started.elapsed()
        );
    }

    // -----------------------------------------------------------------------
    // Sidecar mood compatibility
    // -----------------------------------------------------------------------

    /// A sidecar as written before mood ids existed. Every field that has no
    /// serde default must be present, which is what makes this a real
    /// regression test rather than a shape test.
    const SIDECAR_WITHOUT_ID: &str = r#"{
        "version": 2, "duration": 180.0, "bpm": 120.0, "beat_offset": 0.1,
        "sections": [0.0, 0.5], "energy": [0.1, 0.2], "mood": 2
    }"#;

    #[test]
    fn a_sidecar_without_an_id_still_loads_and_resolves_by_index() {
        let analysis: TrackAnalysis = serde_json::from_str(SIDECAR_WITHOUT_ID).unwrap();
        assert_eq!(analysis.mood_id, None);
        assert_eq!(analysis.mood_index(), 2);
        assert_eq!(analysis.pinned_mood_index(), None);
    }

    #[test]
    fn a_written_sidecar_carries_both_representations() {
        let mut analysis: TrackAnalysis = serde_json::from_str(SIDECAR_WITHOUT_ID).unwrap();
        analysis.set_mood(1);
        analysis.set_pinned_mood(Some(3));

        let json = serde_json::to_string(&analysis).unwrap();
        let reloaded: TrackAnalysis = serde_json::from_str(&json).unwrap();

        assert_eq!(reloaded.mood, 1);
        assert_eq!(reloaded.mood_id.as_deref(), Some(crate::theme::mood_id(1)));
        assert_eq!(reloaded.mood_index(), 1);
        assert_eq!(reloaded.pinned_mood_index(), Some(3));
    }

    #[test]
    fn the_id_wins_when_it_disagrees_with_the_stored_index() {
        let mut analysis: TrackAnalysis = serde_json::from_str(SIDECAR_WITHOUT_ID).unwrap();
        // What a reordering looks like from the reader's side: the index says
        // one world, the id says another.
        analysis.mood = 0;
        analysis.mood_id = Some(crate::theme::mood_id(3).to_string());

        assert_eq!(analysis.mood_index(), 3);
    }

    #[test]
    fn clearing_a_pin_clears_both_representations() {
        let mut analysis: TrackAnalysis = serde_json::from_str(SIDECAR_WITHOUT_ID).unwrap();
        analysis.set_pinned_mood(Some(2));
        analysis.set_pinned_mood(None);

        assert_eq!(analysis.pinned_mood, None);
        assert_eq!(analysis.pinned_mood_id, None);
        assert_eq!(analysis.pinned_mood_index(), None);
    }
}
