//! CLAP zero-shot mood + per-track audio embeddings — PLAN.md M5 (`ml` feature).
//!
//! The model is LAION CLAP (`larger_clap_music_and_speech`, Xenova's quantized
//! ONNX export), fetched by `scripts/fetch-clap.sh` into `assets/ml/`. Only the
//! **audio** branch runs at runtime: the four mood text embeddings are
//! precomputed offline (same model's text branch) and bundled in the binary,
//! so a track's mood = argmax cosine( audio_embed, mood_text_embed ). The
//! 512-d audio embedding is also stored in the track's sidecar — the hook the
//! asset-selection tier reuses (PLAN §1.2, M6 hook).
//!
//! Everything degrades to the M4 rule mapper: no feature → module not
//! compiled; feature but no model file → `ClapModel::try_load` is `None`.
//!
//! Frontend (must match HF `ClapFeatureExtractor`, validated against
//! `transformers.audio_utils` in tests): 48 kHz mono, reflect-center-padded
//! STFT (1024 window / 480 hop, periodic Hann), power spectrum, Slaney mel
//! filterbank (64 mels, 50 Hz–14 kHz), 10·log10. A 10 s window → [1001, 64].

use std::path::Path;
use std::sync::LazyLock;

use bevy::log::{info, warn};
use realfft::RealFftPlanner;

/// CLAP analysis rate / window shape (preprocessor_config.json).
pub const SR: u32 = 48_000;
pub const WINDOW_SAMPLES: usize = 480_000; // 10 s
pub const FRAMES: usize = 1001;
pub const MELS: usize = 64;
pub const EMB_DIM: usize = 512;
const N_FFT: usize = 1024;
const HOP: usize = 480;
const FMIN: f64 = 50.0;
const FMAX: f64 = 14_000.0;
/// Sample positions (fractions of the track) whose windows are embedded and
/// averaged — one window per song is too mood-local (intro/drop/bridge).
const WINDOW_FRACS: [f32; 3] = [0.25, 0.5, 0.75];

// ---------------------------------------------------------------------------
// Slaney mel filterbank (librosa-compatible: slaney scale + slaney area norm)
// ---------------------------------------------------------------------------

fn hz_to_mel(f: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1000.0;
    let min_log_mel = MIN_LOG_HZ / F_SP;
    let logstep = 6.4f64.ln() / 27.0;
    if f < MIN_LOG_HZ {
        f / F_SP
    } else {
        min_log_mel + (f / MIN_LOG_HZ).ln() / logstep
    }
}

fn mel_to_hz(m: f64) -> f64 {
    const F_SP: f64 = 200.0 / 3.0;
    let min_log_mel = 1000.0 / F_SP;
    let logstep = 6.4f64.ln() / 27.0;
    if m < min_log_mel {
        m * F_SP
    } else {
        1000.0 * (logstep * (m - min_log_mel)).exp()
    }
}

/// The 64×513 filterbank (row-major, [mel][fft_bin]).
fn mel_filterbank() -> Vec<f32> {
    let n_bins = N_FFT / 2 + 1;
    let fftfreqs: Vec<f64> = (0..n_bins)
        .map(|k| k as f64 * SR as f64 / N_FFT as f64)
        .collect();
    let lo = hz_to_mel(FMIN);
    let hi = hz_to_mel(FMAX);
    let freqs: Vec<f64> = (0..MELS + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (MELS + 1) as f64))
        .collect();

    let mut weights = vec![0.0f32; MELS * n_bins];
    for i in 0..MELS {
        let enorm = 2.0 / (freqs[i + 2] - freqs[i]); // slaney area norm
        for (k, &f) in fftfreqs.iter().enumerate() {
            let lower = (f - freqs[i]) / (freqs[i + 1] - freqs[i]);
            let upper = (freqs[i + 2] - f) / (freqs[i + 2] - freqs[i + 1]);
            let w = lower.min(upper).max(0.0) * enorm;
            weights[i * n_bins + k] = w as f32;
        }
    }
    weights
}

// ---------------------------------------------------------------------------
// Log-mel frontend
// ---------------------------------------------------------------------------

/// Periodic Hann (matches numpy `hanning(N+1)[:-1]`).
fn hann() -> Vec<f32> {
    (0..N_FFT)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos())
        .collect()
}

/// Reflect-pad `n` samples on both sides (numpy `mode="reflect"`).
fn reflect_pad(samples: &[f32], n: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(samples.len() + 2 * n);
    for i in (1..=n).rev() {
        out.push(samples[i.min(samples.len() - 1)]);
    }
    out.extend_from_slice(samples);
    for i in (samples.len().saturating_sub(n + 1)..samples.len().saturating_sub(1)).rev() {
        out.push(samples[i]);
    }
    out
}

/// 48 kHz mono → log-mel [FRAMES, MELS] row-major (10·log10 power).
/// Input shorter than 10 s is zero-padded; longer is truncated.
pub fn log_mel(samples: &[f32]) -> Vec<f32> {
    let filters = mel_filterbank();
    let window = hann();

    let mut buf = samples.to_vec();
    buf.resize(WINDOW_SAMPLES, 0.0);
    let padded = reflect_pad(&buf, N_FFT / 2);

    let mut planner = RealFftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(N_FFT);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();

    let n_bins = N_FFT / 2 + 1;
    let mut power = vec![0.0f32; n_bins];
    let mut out = vec![0.0f32; FRAMES * MELS];

    for frame in 0..FRAMES {
        let start = frame * HOP;
        if start + N_FFT > padded.len() {
            break;
        }
        for (i, w) in window.iter().enumerate() {
            input[i] = padded[start + i] * w;
        }
        if fft.process(&mut input, &mut spectrum).is_err() {
            break;
        }
        for (b, c) in spectrum.iter().enumerate() {
            power[b] = c.norm_sqr();
        }
        for m in 0..MELS {
            let row = &filters[m * n_bins..(m + 1) * n_bins];
            let mut e = 0.0f32;
            for b in 0..n_bins {
                e += row[b] * power[b];
            }
            out[frame * MELS + m] = 10.0 * e.max(1e-10).log10();
        }
    }
    out
}

/// Resample mono to 48 kHz (rubato sinc); passthrough when already there.
pub fn to_48k(samples: Vec<f32>, src_sr: u32) -> Option<Vec<f32>> {
    if src_sr == SR {
        return Some(samples);
    }
    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };
    let chunk = 4096usize;
    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    let mut resampler =
        SincFixedIn::<f32>::new(SR as f64 / src_sr as f64, 2.0, params, chunk, 1).ok()?;
    let mut out = Vec::new();
    for part in samples.chunks(chunk) {
        let mut frame = part.to_vec();
        frame.resize(chunk, 0.0);
        let waves = resampler.process(&[frame], None).ok()?;
        out.extend_from_slice(&waves[0]);
    }
    // Trim the sinc tail padding back to the true resampled length.
    out.truncate(samples.len() * SR as usize / src_sr as usize + 1024);
    Some(out)
}

// ---------------------------------------------------------------------------
// Mood text embeddings (precomputed offline via the CLAP text branch)
// ---------------------------------------------------------------------------

struct MoodEmbeds {
    /// Each text embedding paired with the mood index it votes for.
    ///
    /// The pairing is resolved through the prompt's stable id rather than left
    /// implicit in row order: the file is generated offline and the mood list
    /// lives in code, so a reordering would otherwise silently make every
    /// zero-shot classification name the wrong world.
    embeds: Vec<(usize, [f32; EMB_DIM])>,
}

static MOOD_EMBEDS: LazyLock<MoodEmbeds> = LazyLock::new(|| {
    #[derive(serde::Deserialize)]
    struct Prompt {
        /// Stable mood id. Absent in files generated before ids existed, which
        /// then fall back to `mood`.
        #[serde(default)]
        id: Option<String>,
        mood: usize,
    }
    #[derive(serde::Deserialize)]
    struct File {
        prompts: Vec<Prompt>,
        embeddings: Vec<Vec<f32>>,
    }
    let text = include_str!("../assets/ml/mood_text_embeddings.json");
    let parsed: File = serde_json::from_str(text).expect("mood embeddings JSON");
    assert_eq!(
        parsed.prompts.len(),
        parsed.embeddings.len(),
        "mood_text_embeddings.json: {} prompts but {} embeddings — every \
         embedding must name the mood it votes for",
        parsed.prompts.len(),
        parsed.embeddings.len(),
    );
    let embeds = parsed
        .embeddings
        .iter()
        .zip(&parsed.prompts)
        .map(|(e, prompt)| {
            let mut v = [0.0f32; EMB_DIM];
            v.copy_from_slice(e);
            let mood = crate::theme::resolve_mood(
                crate::theme::moods(),
                prompt.id.as_deref(),
                prompt.mood,
            );
            (mood, v)
        })
        .collect();
    MoodEmbeds { embeds }
});

/// argmax cosine(embedding, mood_text_embedding) — embedding already L2-normed,
/// text embeddings were normed at precompute time, so a dot product suffices.
pub fn mood_for(embedding: &[f32]) -> usize {
    MOOD_EMBEDS
        .embeds
        .iter()
        .map(|(mood, t)| {
            (
                *mood,
                embedding
                    .iter()
                    .zip(t.iter())
                    .map(|(a, b)| a * b)
                    .sum::<f32>(),
            )
        })
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(mood, _)| mood)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------

/// The CLAP audio branch. `None` from `try_load` when the model file is
/// missing — the caller keeps the rule-based mood (graceful fallback).
pub struct ClapModel {
    session: ort::session::Session,
}

impl ClapModel {
    /// Load `assets/ml/audio_model_quantized.onnx` if present.
    pub fn try_load() -> Option<Self> {
        let path = crate::world_assets::asset_root().join("ml/audio_model_quantized.onnx");
        if !path.exists() {
            warn!(
                "ml: {} not found — rule-based moods (run scripts/fetch-clap.sh)",
                path.display()
            );
            return None;
        }
        let session = ort::session::Session::builder()
            .ok()?
            .commit_from_file(&path)
            .map_err(|e| warn!("ml: can't load CLAP: {e}"))
            .ok()?;
        info!("ml: CLAP audio model loaded");
        Some(Self { session })
    }

    /// 512-d embedding for one 10 s window of 48 kHz mono.
    fn embed_window(&mut self, samples: &[f32]) -> Option<Vec<f32>> {
        let mel = log_mel(samples);
        let input = ort::value::Tensor::from_array(([1usize, 1, FRAMES, MELS], mel)).ok()?;
        let outputs = self
            .session
            .run(ort::inputs!["input_features" => input])
            .map_err(|e| warn!("ml: CLAP inference failed: {e}"))
            .ok()?;
        let (_, flat) = outputs["audio_embeds"].try_extract_tensor::<f32>().ok()?;
        let mut emb = flat.to_vec();
        let norm = emb.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
        for v in &mut emb {
            *v /= norm;
        }
        Some(std::mem::take(&mut emb))
    }

    /// Decode + embed a track: average of three windows (quarter/half/three-
    /// quarter), then the zero-shot mood. `None` keeps the rule-based mood.
    pub fn analyze_track(&mut self, path: &Path) -> Option<(Vec<f32>, usize)> {
        let (native, src_sr) = crate::analysis::decode_mono(path, u32::MAX)?;
        let samples = to_48k(native, src_sr)?;

        let mut acc = vec![0.0f32; EMB_DIM];
        let mut n = 0;
        for frac in WINDOW_FRACS {
            let center = (samples.len() as f32 * frac) as usize;
            let start = center.saturating_sub(WINDOW_SAMPLES / 2);
            let window: Vec<f32> = (0..WINDOW_SAMPLES)
                .map(|i| samples.get(start + i).copied().unwrap_or(0.0))
                .collect();
            let emb = self.embed_window(&window)?;
            for (a, e) in acc.iter_mut().zip(emb.iter()) {
                *a += e;
            }
            n += 1;
        }
        if n == 0 {
            return None;
        }
        let norm = acc.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
        for v in &mut acc {
            *v /= norm;
        }
        let mood = mood_for(&acc);
        Some((acc, mood))
    }
}

// ---------------------------------------------------------------------------
// The text tower — asset-selection ranking (PLAN.md M5→M6 hook)
// ---------------------------------------------------------------------------

/// The CLAP text branch, loaded at runtime for asset selection: short phrases
/// (asset names/tags, the recipe's `intent` descriptions) embed into the same
/// 512-d space as the audio tower, so "this landmark is a rain-slick monolith"
/// ranks "obsidian shard" above "limestone boulder" without a keyword in
/// common.
///
/// `fetch-clap.sh` already pulls `text_model_quantized.onnx` + `tokenizer.json`
/// (they were fetched for the offline mood-embedding precompute); this is the
/// first runtime use. Same graceful-degradation contract as [`ClapModel`]:
/// absent files or a tokenizer failure → `None`, and every caller keeps the
/// keyword/rule path.
pub struct TextEmbedder {
    session: ort::session::Session,
    tokenizer: tokenizers::Tokenizer,
}

impl TextEmbedder {
    /// Load the text branch + tokenizer from `assets/ml/` if present.
    pub fn try_load() -> Option<Self> {
        let dir = crate::world_assets::asset_root().join("ml");
        let model_path = dir.join("text_model_quantized.onnx");
        let tok_path = dir.join("tokenizer.json");
        if !model_path.exists() || !tok_path.exists() {
            warn!(
                "ml: text tower not found under {} — keyword asset matching (run scripts/fetch-clap.sh)",
                dir.display()
            );
            return None;
        }
        let session = ort::session::Session::builder()
            .ok()?
            .commit_from_file(&model_path)
            .map_err(|e| warn!("ml: can't load CLAP text model: {e}"))
            .ok()?;
        let tokenizer = tokenizers::Tokenizer::from_file(&tok_path)
            .map_err(|e| warn!("ml: can't load CLAP tokenizer: {e}"))
            .ok()?;
        info!("ml: CLAP text model loaded (asset selection active)");
        Some(Self { session, tokenizer })
    }

    /// L2-normed 512-d embedding for a short English phrase. Errors are
    /// strings so callers can log once and fall back per-asset.
    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>, String> {
        const MAX_TOKENS: usize = 77; // CLAP text context
        let encoding = self
            .tokenizer
            .encode(text, true)
            .map_err(|e| format!("tokenize: {e}"))?;
        let ids: Vec<i64> = encoding
            .get_ids()
            .iter()
            .take(MAX_TOKENS)
            .map(|&i| i as i64)
            .collect();
        let n = ids.len().max(1);
        let input_ids = ort::value::Tensor::from_array(([1usize, n], ids))
            .map_err(|e| format!("input_ids tensor: {e}"))?;
        // Xenova's export takes input_ids only (no attention_mask input).
        let outputs = self
            .session
            .run(ort::inputs!["input_ids" => input_ids])
            .map_err(|e| format!("inference: {e}"))?;
        let (_, flat) = outputs["text_embeds"]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("text_embeds output: {e}"))?;
        let mut emb = flat.to_vec();
        let norm = emb.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
        for v in &mut emb {
            *v /= norm;
        }
        Ok(emb)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Defined in `analysis` (ungated — placement uses it too); its cosine
    // checks live here beside the embeddings they compare.
    use crate::analysis::dot_normed;

    /// The same deterministic signal the Python reference fixture used.
    fn test_signal() -> Vec<f32> {
        (0..WINDOW_SAMPLES)
            .map(|i| {
                let t = i as f64 / SR as f64;
                let mut v = 0.30 * (2.0 * std::f64::consts::PI * 220.0 * t).sin()
                    + 0.15 * (2.0 * std::f64::consts::PI * 880.0 * t).sin();
                if t % 0.5 < 0.01 {
                    v += 0.5 * (2.0 * std::f64::consts::PI * 3000.0 * t).sin();
                }
                v as f32
            })
            .collect()
    }

    #[test]
    fn every_mood_gets_exactly_one_text_embedding() {
        // A zero-shot vote is only meaningful if each world is represented once:
        // a missing prompt makes a world unreachable, a duplicate biases toward
        // it. Both are silent without this check.
        //
        // The file covers the four *base* quadrants; the extended variants
        // (Cinder Reach, …) are selected from the base by mean energy in the
        // analysis worker, not by a text vote — they have no embeddings here.
        let mut voted: Vec<usize> = MOOD_EMBEDS.embeds.iter().map(|(mood, _)| *mood).collect();
        voted.sort_unstable();
        let expected: Vec<usize> = (0..crate::theme::ASSET_BASE_MOODS).collect();
        assert_eq!(
            voted, expected,
            "mood_text_embeddings.json does not cover the base moods one-to-one"
        );
    }

    #[test]
    fn embeddings_pair_with_moods_by_id_not_row_order() {
        // Each prompt's id is what decides the mood it votes for, so the pairing
        // has to agree with resolving that id directly.
        #[derive(serde::Deserialize)]
        struct Prompt {
            id: String,
        }
        #[derive(serde::Deserialize)]
        struct File {
            prompts: Vec<Prompt>,
        }
        let parsed: File =
            serde_json::from_str(include_str!("../assets/ml/mood_text_embeddings.json")).unwrap();

        for (prompt, (mood, _)) in parsed.prompts.iter().zip(&MOOD_EMBEDS.embeds) {
            assert_eq!(
                crate::theme::moods()[*mood].id,
                prompt.id,
                "embedding paired with the wrong world"
            );
        }
    }

    #[test]
    fn mel_matches_python_reference() {
        let mel = log_mel(&test_signal());
        let bytes = include_bytes!("testdata/clap_ref_mel.f32le");
        let expected: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        assert_eq!(mel.len(), expected.len());
        let max_diff = mel
            .iter()
            .zip(expected.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max_diff < 0.5, "mel max diff {max_diff} dB");
    }

    #[test]
    fn embedding_matches_python_reference() {
        let Some(mut model) = ClapModel::try_load() else {
            return; // model not fetched — nothing to compare
        };
        let emb = model.embed_window(&test_signal()).expect("embedding");
        let meta: serde_json::Value =
            serde_json::from_str(include_str!("testdata/clap_ref_meta.json")).unwrap();
        let expected: Vec<f32> = meta["embedding"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        let cos: f32 = emb.iter().zip(expected.iter()).map(|(a, b)| a * b).sum();
        assert!(cos > 0.995, "cosine vs reference {cos}");
    }

    #[test]
    fn mood_index_in_range() {
        let emb = vec![0.0f32; EMB_DIM];
        assert!(mood_for(&emb) < crate::theme::moods().len());
    }

    #[test]
    fn dot_normed_is_cosine_for_unit_vectors() {
        let a = vec![1.0f32, 0.0, 0.0];
        let b = vec![0.0f32, 1.0, 0.0];
        assert_eq!(dot_normed(&a, &a), 1.0);
        assert_eq!(dot_normed(&a, &b), 0.0);
        assert_eq!(dot_normed(&a, &[]), 0.0, "mismatched lengths are neutral");
        let c = vec![0.6f32, 0.8, 0.0];
        assert!((dot_normed(&a, &c) - 0.6).abs() < 1e-6);
    }

    /// Manual probe for the text tower (the model files sit under assets/ml
    /// after fetch-clap.sh): verifies the ONNX I/O names and that semantically
    /// close phrases rank closer than far ones in the shared space.
    /// `cargo test --features ml -- --ignored --nocapture text_embedding_probe`
    #[test]
    #[ignore = "loads the CLAP text model; run explicitly"]
    fn text_embedding_probe() {
        let Some(mut embedder) = TextEmbedder::try_load() else {
            panic!("no text model — run scripts/fetch-clap.sh");
        };
        let ocean = embedder
            .embed("ocean waves, coral reef, tidal pools")
            .expect("embed");
        let neon = embedder
            .embed("neon city at night, chrome, electric lights")
            .expect("embed");
        let coral = embedder.embed("coral branch").expect("embed");
        let monolith = embedder.embed("dark monolith slab").expect("embed");
        let oc = dot_normed(&ocean, &coral);
        let on = dot_normed(&ocean, &monolith);
        let nc = dot_normed(&neon, &coral);
        let nn = dot_normed(&neon, &monolith);
        eprintln!(
            "ocean→coral {oc:.3} · ocean→monolith {on:.3} · neon→coral {nc:.3} · neon→monolith {nn:.3}"
        );
        assert!(
            oc > on,
            "ocean should sit closer to coral ({oc:.3}) than to monolith ({on:.3})"
        );
    }

    /// Manual real-track check:
    /// `VERSE_TEST_TRACK=<file> cargo test --features ml real_track -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_track_embedding_and_mood() {
        let Ok(path) = std::env::var("VERSE_TEST_TRACK") else {
            return;
        };
        let mut model = ClapModel::try_load().expect("model file (scripts/fetch-clap.sh)");
        let (emb, mood) = model
            .analyze_track(Path::new(&path))
            .expect("analysis failed");
        eprintln!(
            "{path}\n  mood = {} · embedding[..4] = {:?}",
            crate::theme::moods()[mood].world_name,
            &emb[..4]
        );
    }
}
