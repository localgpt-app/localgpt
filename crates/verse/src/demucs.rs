//! Demucs stem separation — PLAN.md M7 (`ml` feature, via `stem-splitter-core`).
//!
//! Splits a track into four stems — **drums / bass / vocals / other** — via
//! HT-Demucs, wrapped by [`stem_splitter_core`] (MIT OR Apache-2.0, so
//! commercial-friendly, unlike CLAP's CC-BY-NC). Each stem gets its own
//! per-second RMS energy curve, so the renderer can drive four independent
//! visual layers (drums=ground pulse, bass=deep core glow, vocals=floating
//! motes, lead=hero landmarks) instead of one mixed envelope.
//!
//! Like [`crate::ml::ClapModel`], this degrades to `None` — no `ml` feature →
//! `StemModel` not compiled; the model absent or `split_file` failing →
//! [`StemModel::analyze_track`] is `None`. In every case the analysis keeps the
//! mixed energy envelope from M3 and the world renders as it does today. The
//! app is never broken by a missing stem tier.
//!
//! This is a lower rung than the LLM recipe: stems are an *input* feature the
//! recipe LLM may consume (so it can, e.g., tell a sparse-vocal bridge from a
//! full drop), independent of whether the recipe tier is present.
//!
//! # Pipeline
//! `stem_splitter_core::split_file` writes 4 stem WAVs into a temp dir (and
//! fetches the htdemucs ONNX on first use into its own cache). We then decode
//! each WAV mono via [`crate::analysis::decode_mono`] and reduce to a per-second
//! RMS envelope aligned to the M3 `energy` curve.

// The StemEnergy type is referenced unconditionally from analysis.rs's
// TrackAnalysis (it serializes into the sidecar), so it lives outside the
// feature gate. Only the stem-splitter-core-dependent StemModel is gated.

/// Per-second RMS energy per stem, 0..1. Same length as the M3 mixed `energy`
/// curve (one value per second of audio) so they align in the renderer.
pub type StemEnergy = [Vec<f32>; 4];

/// drum / bass / vocal / other. Index order matches [`StemEnergy`]. Read by
/// the renderer's stem-driven layers; unused until then.
#[allow(dead_code)]
pub const STEM_NAMES: [&str; 4] = ["drums", "bass", "vocals", "other"];

#[cfg(feature = "ml")]
mod gated {
    use std::path::{Path, PathBuf};

    use bevy::log::{info, warn};

    /// The Demucs stem separator (backed by `stem-splitter-core`). Stateless
    /// from our side — the model session lives inside the crate, fetched lazily
    /// on the first `split_file`.
    pub struct StemModel {
        /// Where split_file writes the 4 stem WAVs. One per worker invocation,
        /// cleared between tracks so disk doesn't grow.
        out_dir: PathBuf,
        /// htdemucs model name stem-splitter-core downloads + caches. The
        /// canonical 4-stem v4 model.
        model_name: String,
    }

    impl StemModel {
        /// Construct the separator. Always succeeds under the `ml` feature —
        /// the actual model fetch happens lazily inside `split_file`, mirroring
        /// stem-splitter-core's own design (and our `try_load` convention:
        /// feature on → ready; the per-track call decides success).
        pub fn try_load() -> Option<Self> {
            let out_dir = crate::analysis::cache_dir()
                .map(|d| d.join("stems"))
                .unwrap_or_else(|| std::env::temp_dir().join("verse-stems"));
            std::fs::create_dir_all(&out_dir).ok();
            info!(
                "demucs: stem-splitter-core ready (out_dir {})",
                out_dir.display()
            );
            Some(Self {
                out_dir,
                // stem-splitter-core's manifest ships model names; "htdemucs"
                // is the standard 4-stem v4 model (drums/bass/vocals/other).
                model_name: "htdemucs".to_string(),
            })
        }

        /// Separate a track into four per-second RMS energy curves. `None`
        /// keeps the mixed envelope — separation or decode failed. Writes the
        /// 4 stem WAVs into `out_dir`, decodes each, then deletes them.
        pub fn analyze_track(&self, path: &Path) -> Option<super::StemEnergy> {
            let opts = stem_splitter_core::SplitOptions {
                output_dir: self.out_dir.to_string_lossy().into_owned(),
                model_name: self.model_name.clone(),
                manifest_url_override: None,
            };
            let result = match stem_splitter_core::split_file(&path.to_string_lossy(), opts) {
                Ok(r) => r,
                Err(e) => {
                    warn!("demucs: split_file failed for {}: {e}", path.display());
                    return None;
                }
            };
            info!(
                "demucs: separated {} → 4 stems",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("?")
            );

            // SplitResult holds WAV paths in a fixed order we map to STEM_NAMES.
            // Index order must match StemEnergy = [drums, bass, vocals, other].
            let stem_paths = [
                result.drums_path.as_str(),
                result.bass_path.as_str(),
                result.vocals_path.as_str(),
                result.other_path.as_str(),
            ];

            // Per-second RMS aligned to the M3 energy curve length. decode_mono
            // returns (samples, sample_rate) at a chosen rate; ANALYSIS_SR keeps
            // the curves consistent with the mixed envelope.
            let analysis_sr = crate::analysis::ANALYSIS_SR;
            let mut curves: super::StemEnergy = Default::default();
            let mut max_secs = 0usize;
            for (i, stem_path) in stem_paths.iter().enumerate() {
                match crate::analysis::decode_mono(Path::new(stem_path), analysis_sr) {
                    Some((samples, sr)) => {
                        let curve = per_second_rms(&samples, sr);
                        max_secs = max_secs.max(curve.len());
                        curves[i] = curve;
                    }
                    None => {
                        warn!("demucs: can't decode stem {stem_path}");
                        // Leave this stem empty; caller treats missing data as 0.
                    }
                }
            }

            // Pad all curves to the same length so the renderer can index in lockstep.
            for curve in curves.iter_mut() {
                curve.resize(max_secs, 0.0);
            }

            // Best-effort cleanup of the stem WAVs (the cache dir is reused,
            // but don't leave per-track files accumulating).
            for stem_path in stem_paths {
                let _ = std::fs::remove_file(stem_path);
            }

            // If every stem failed to decode, signal None so the caller keeps
            // the mixed envelope rather than 4 empty curves.
            if max_secs == 0 {
                warn!("demucs: no stems decoded — mixed energy only");
                return None;
            }
            Some(curves)
        }
    }

    /// Reduce mono samples to a per-second RMS envelope, 0..1.
    fn per_second_rms(samples: &[f32], sr: u32) -> Vec<f32> {
        if samples.is_empty() || sr == 0 {
            return Vec::new();
        }
        let win = sr.max(1) as usize;
        let mut out = Vec::with_capacity(samples.len() / win + 1);
        for chunk in samples.chunks(win) {
            let sum_sq: f32 = chunk.iter().map(|s| s * s).sum();
            let rms = (sum_sq / chunk.len() as f32).sqrt();
            // Map RMS (~0..0.5 for typical audio) into 0..1 with a gentle curve.
            out.push((rms * 2.0).min(1.0));
        }
        out
    }
}

#[cfg(feature = "ml")]
pub use gated::StemModel;
