//! CLAP zero-shot mood + per-track audio embeddings — PLAN.md M5 (`ml` feature).
//!
//! The model is LAION CLAP (`larger_clap_music_and_speech`, Xenova's quantized
//! ONNX export), run by [`localgpt_audio_models::clap`]: the `match` model
//! every LocalGPT app shares, frontend and towers included. Verse reads its
//! files from the installed `match` model (`localgpt audio install match`, or
//! the desktop app), else from its own `assets/ml/` (`cargo xtask
//! fetch-clap`). What is Verse's own is here: only the **audio** tower runs
//! per track, and the four mood text embeddings are precomputed offline (same
//! model's text tower) and bundled in the binary, so a track's mood =
//! argmax cosine( audio_embed, mood_text_embed ). The 512-d audio embedding is
//! also stored in the track's sidecar — the hook the asset-selection tier
//! reuses (PLAN §1.2, M6 hook), with the text tower embedding the assets
//! ([`text_model`]).
//!
//! Everything degrades to the M4 rule mapper: no feature → module not
//! compiled; feature but no model file → `ClapModel::try_load` is `None`.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use bevy::log::{info, warn};
use localgpt_audio_models::catalog::MATCH;
use localgpt_audio_models::clap::{self, Clap};
use localgpt_audio_models::decode::Audio;

const EMB_DIM: usize = clap::DIMENSIONS;

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
// Where the model is
// ---------------------------------------------------------------------------

/// The directory holding the CLAP files `needed`: the installed `match`
/// model when it is complete, else Verse's own `assets/ml/` when it holds
/// them.
fn model_dir(needed: &[&str]) -> Option<PathBuf> {
    if let Some(models) = localgpt_audio_models::models_dir()
        && localgpt_audio_models::installed(&models, &MATCH)
    {
        return Some(models.join(MATCH.id));
    }
    let legacy = crate::world_assets::asset_root().join("ml");
    needed
        .iter()
        .all(|file| legacy.join(file).is_file())
        .then_some(legacy)
}

// ---------------------------------------------------------------------------
// The audio tower — a track's embedding and mood
// ---------------------------------------------------------------------------

/// The CLAP audio tower. `None` from `try_load` when the model is missing —
/// the caller keeps the rule-based mood (graceful fallback).
pub struct ClapModel {
    clap: Clap,
}

impl ClapModel {
    /// The installed `match` model's audio tower, else `assets/ml/`'s.
    pub fn try_load() -> Option<Self> {
        let Some(dir) = model_dir(&[clap::AUDIO_MODEL]) else {
            warn!(
                "ml: no CLAP model — rule-based moods (localgpt audio install match, or cargo xtask fetch-clap)"
            );
            return None;
        };
        // Only the audio tower opens: a track needs nothing else.
        let clap = Clap::load(&dir)
            .and_then(|mut clap| clap.open_audio().map(|()| clap))
            .map_err(|e| warn!("ml: can't load CLAP: {e:#}"))
            .ok()?;
        info!("ml: CLAP audio model loaded from {}", dir.display());
        Some(Self { clap })
    }

    /// Decode + embed a track: average of three windows (quarter/half/three-
    /// quarter), then the zero-shot mood. `None` keeps the rule-based mood.
    pub fn analyze_track(&mut self, path: &Path) -> Option<(Vec<f32>, usize)> {
        let (samples, sample_rate) = crate::analysis::decode_mono(path, u32::MAX)?;
        let audio = Audio {
            sample_rate,
            channels: vec![samples],
        };
        let embedding = self
            .clap
            .embed_audio(&audio)
            .map_err(|e| warn!("ml: CLAP could not embed {}: {e:#}", path.display()))
            .ok()?;
        let mood = mood_for(&embedding);
        Some((embedding, mood))
    }
}

// ---------------------------------------------------------------------------
// The text tower — asset-selection ranking (PLAN.md M5→M6 hook)
// ---------------------------------------------------------------------------

/// The CLAP text tower, for asset selection: short phrases (asset names and
/// tiers) embed into the same 512-d space as the audio tower, so a track whose
/// audio sits near "coral" favours coral assets. Same graceful-degradation
/// contract as [`ClapModel`]: absent files or a tokenizer failure → `None`,
/// and every caller keeps the keyword/rule path.
pub fn text_model() -> Option<Clap> {
    let Some(dir) = model_dir(&[clap::TEXT_MODEL, clap::TOKENIZER]) else {
        warn!(
            "ml: no CLAP text tower — keyword asset matching (localgpt audio install match, or cargo xtask fetch-clap)"
        );
        return None;
    };
    let clap = Clap::load(&dir)
        .and_then(|mut clap| clap.open_text().map(|()| clap))
        .map_err(|e| warn!("ml: can't load the CLAP text model: {e:#}"))
        .ok()?;
    info!("ml: CLAP text model loaded (asset selection active)");
    Some(clap)
}

#[cfg(test)]
mod tests {
    use super::*;
    // Defined in `analysis` (ungated — placement uses it too); its cosine
    // checks live here beside the embeddings they compare.
    use crate::analysis::dot_normed;

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

    #[test]
    fn the_text_tower_reproduces_the_mood_embeddings() {
        // The mood embeddings were precomputed offline from the same export,
        // so the text tower run here (tokenizer, I/O names, session) has to
        // give them back. Skipped where no model is installed.
        let Some(mut model) = text_model() else {
            return;
        };
        #[derive(serde::Deserialize)]
        struct Prompt {
            prompt: String,
        }
        #[derive(serde::Deserialize)]
        struct File {
            prompts: Vec<Prompt>,
            embeddings: Vec<Vec<f32>>,
        }
        let parsed: File =
            serde_json::from_str(include_str!("../assets/ml/mood_text_embeddings.json")).unwrap();
        for (prompt, expected) in parsed.prompts.iter().zip(&parsed.embeddings) {
            let embedding = model.embed_text(&prompt.prompt).expect("embed");
            let cos = dot_normed(&embedding, expected);
            assert!(cos > 0.99, "{:?}: cosine {cos}", prompt.prompt);
        }
    }

    /// Manual probe for the text tower (the installed `match` model, or
    /// assets/ml after cargo xtask fetch-clap): that semantically close
    /// phrases rank closer than far ones in the shared space.
    /// `cargo test --features ml -- --ignored --nocapture text_embedding_probe`
    #[test]
    #[ignore = "loads the CLAP text model; run explicitly"]
    fn text_embedding_probe() {
        let Some(mut embedder) = text_model() else {
            panic!("no text model — localgpt audio install match");
        };
        let ocean = embedder
            .embed_text("ocean waves, coral reef, tidal pools")
            .expect("embed");
        let neon = embedder
            .embed_text("neon city at night, chrome, electric lights")
            .expect("embed");
        let coral = embedder.embed_text("coral branch").expect("embed");
        let monolith = embedder.embed_text("dark monolith slab").expect("embed");
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
        let mut model =
            ClapModel::try_load().expect("no CLAP model (localgpt audio install match)");
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
