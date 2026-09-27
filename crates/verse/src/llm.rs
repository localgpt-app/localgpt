//! LLM-authored scene recipes — PLAN.md M7 (`llm` feature).
//!
//! The top rung of the signal-ownership ladder: takes the MIR features from
//! [`crate::analysis::TrackAnalysis`] (bpm, energy, sections, mood, and the CLAP
//! embedding when present) and asks a local LLM to author a
//! [`crate::recipe::WorldRecipe`] describing how to dress the world up.
//!
//! Generation is **plain instructed-JSON with a lenient parse**
//! ([`RecipeModel::generate`]), not mistral.rs's `generate_structured`: the
//! grammar-constrained path hangs on GGUF in mistral.rs 0.8 — verified against
//! even a two-field schema (see the `llm_generation_probe` test). Safety never
//! depended on the grammar: every recipe field carries a serde default,
//! [`WorldRecipe::clamped`] bounds the free values, and a malformed or
//! truncated reply degrades to the rule recipe.
//!
//! Like [`crate::ml::ClapModel`] / [`crate::demucs::StemModel`], this degrades
//! to `None`: no `llm` feature → module not compiled; feature but no model →
//! [`RecipeModel::try_load`] is `None`; model but generation fails or exceeds
//! [`GENERATE_TIMEOUT`] → [`RecipeModel::generate`] returns `None`. In every
//! case the renderer keeps the rule-derived recipe (today's path). The app is
//! never broken by a missing LLM tier.
//!
//! # Model
//! `GgufModelBuilder` loads the first `.gguf` in the model directory LocalGPT's
//! apps share (`~/.local/share/localgpt/models/llm`, or `$LOCALGPT_LLM_DIR`;
//! `assets/llm/` is still read as a fallback), fetched by
//! `scripts/fetch-bonsai.sh` — Bonsai-8B Q4_K_M by default (Apache-2.0; the
//! 1-bit Q1_0 quant parses in neither mistral.rs 0.8 nor its llama.cpp, and
//! the 5 GB Q4_K_M needs the `llm-metal` feature's GPU path to fit in memory
//! on a 32 GB Mac running the world renderer alongside). Any standard GGUF
//! (e.g. Qwen2.5-7B) + matching `tokenizer.json` dropped into that directory
//! is picked up the same way.
//!
//! # Async
//! mistral.rs's `build()` and generation calls are async. This crate is
//! otherwise sync (the analysis worker is a plain `std::thread`). We run a
//! dedicated single-threaded tokio runtime per generation inside that thread —
//! no async pollution of the rest of the app.

use std::path::PathBuf;

use bevy::log::{info, warn};
use mistralrs::{GgufModelBuilder, TextMessageRole};

use crate::analysis::TrackAnalysis;
use crate::recipe::WorldRecipe;
use crate::theme::moods;

/// The loaded LLM, ready to author recipes. `None` from `try_load` when the
/// model file is missing — the caller keeps the rule-derived recipe.
pub struct RecipeModel {
    model: mistralrs::Model,
}

impl RecipeModel {
    /// Borrow the underlying mistral.rs model. Used by the agent tier
    /// (`crate::agent::run_session`) to run tool-calling on the same loaded
    /// model, so we don't pay for two model loads.
    pub fn model_mut(&mut self) -> &mut mistralrs::Model {
        &mut self.model
    }

    /// Load the GGUF under `assets/llm/` if present. Returns `None` (and logs)
    /// when no model is found — see the module docs for the fallback contract.
    ///
    /// Builds a single-threaded tokio runtime on the calling thread: mistral.rs
    /// is async, and we keep the rest of the app sync.
    pub fn try_load() -> Option<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| warn!("llm: can't start tokio runtime: {e}"))
            .ok()?;

        let (model_id, gguf_file, tokenizer_json) = locate_model()?;

        rt.block_on(async move {
            // `GgufModelBuilder::new` takes `impl ToString`; pass the directory
            // and filenames as strings (mistral.rs treats a local path as the
            // model source, avoiding a HuggingFace fetch).
            let model_id_str = model_id.to_string_lossy().into_owned();
            let model = GgufModelBuilder::new(model_id_str, vec![gguf_file.clone()])
                .with_tokenizer_json(tokenizer_json.clone())
                .build()
                .await
                .map_err(|e| warn!("llm: can't build mistral.rs model: {e}"))
                .ok()?;
            info!("llm: recipe model loaded ({gguf_file})");
            Some(RecipeModel { model })
        })
    }

    /// Author a recipe for one track. `None` on any failure — the caller keeps
    /// the rule-derived recipe. The returned recipe is already [`WorldRecipe::clamped`]
    /// so no out-of-range value can reach the renderer.
    ///
    /// This is **plain instructed-JSON generation with a lenient parse**, not
    /// mistral.rs's `generate_structured`: the grammar-constrained path hangs
    /// on GGUF in mistral.rs 0.8 — verified against even a two-field schema at
    /// ~0 tokens emitted over 3 minutes while plain chat on the same loaded
    /// model runs at ~25 tok/s (see the `llm_generation_probe` test). Runtime
    /// safety never depended on the grammar anyway: every `WorldRecipe` field
    /// has a serde default, [`WorldRecipe::clamped`] bounds the free values,
    /// and a parse failure degrades to the rule recipe.
    pub fn generate(&mut self, analysis: &TrackAnalysis) -> Option<WorldRecipe> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| warn!("llm: can't start tokio runtime: {e}"))
            .ok()?;

        rt.block_on(async {
            let messages = build_prompt(analysis);
            let text = match tokio::time::timeout(GENERATE_TIMEOUT, async {
                self.model
                    .send_chat_request(messages)
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|response| {
                        response
                            .choices
                            .into_iter()
                            .next()
                            .and_then(|c| c.message.content)
                            .ok_or_else(|| "empty completion".to_string())
                    })
            })
            .await
            {
                Ok(Ok(text)) => text,
                Ok(Err(e)) => {
                    warn!("llm: recipe generation failed ({e}) — keeping rule recipe");
                    return None;
                }
                Err(_) => {
                    warn!(
                        "llm: recipe generation timed out after {}s — keeping rule recipe",
                        GENERATE_TIMEOUT.as_secs()
                    );
                    return None;
                }
            };

            match extract_json_object(&text)
                .and_then(|json| serde_json::from_str::<WorldRecipe>(&json).ok())
            {
                Some(recipe) => {
                    info!(
                        "llm: recipe for \"{}\" ({} biomes, {} landmarks)",
                        recipe.world_name,
                        recipe.biomes.len(),
                        recipe.landmarks.len()
                    );
                    Some(recipe.clamped())
                }
                None => {
                    warn!(
                        "llm: recipe wasn't valid JSON ({} bytes of prose) — keeping rule recipe",
                        text.len()
                    );
                    None
                }
            }
        })
    }
}

/// How long one recipe generation may run before the worker gives up on it
/// (and the rule recipe stands). Plain chat on the verified setup runs at
/// ~25 tok/s, so a ~200-token recipe lands in well under 30 s; this cap
/// exists for the day the model path degrades (thermal throttle, swap).
const GENERATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// Pull the first balanced JSON object out of an LLM reply — tolerating code
/// fences and surrounding prose. Returns the object text (without fences) or
/// `None` when no complete `{...}` is present (e.g. the reply was truncated
/// mid-object; the caller keeps the rule recipe).
fn extract_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (i, c) in text[start..].char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + i + c.len_utf8()].to_string());
                }
            }
            _ => {}
        }
    }
    None // unbalanced (truncated) — no usable object
}

/// Find the recipe model on disk. Returns `(model_id, gguf_file, tokenizer_json)`
/// as the trio `GgufModelBuilder` needs, or `None` (with a warning) when no
/// model is present. Looks in the directory every LocalGPT app shares
/// ([`localgpt_world_agent::shared_llm_dir`], where
/// `scripts/fetch-bonsai.sh` puts it, so one
/// download serves Verse, MD and Gen), then in the older per-app spot
/// `assets/llm/`. Takes the first `.gguf` it finds, so the default (Bonsai 8B)
/// and the documented standard-GGUF fallback are both discovered
/// automatically — the user just drops a model in.
///
/// `model_id` is the local directory (mistral.rs treats a local path as the
/// model source, avoiding a HuggingFace fetch); `gguf_file`/`tokenizer_json` are
/// the bare filenames within that directory.
fn locate_model() -> Option<(PathBuf, String, String)> {
    let shared = localgpt_world_agent::shared_llm_dir();
    let legacy = crate::world_assets::asset_root().join("llm");
    for dir in shared.iter().chain(std::iter::once(&legacy)) {
        if let Some(found) = locate_model_in(dir) {
            info!("llm: using model in {}", dir.display());
            return Some(found);
        }
    }
    let shared = shared.map_or_else(|| "$LOCALGPT_LLM_DIR".into(), |d| d.display().to_string());
    warn!(
        "llm: no model in {shared} or {} — rule recipes only (run scripts/fetch-bonsai.sh)",
        legacy.display()
    );
    None
}

/// [`locate_model`] over an explicit directory, so the discovery contract is
/// testable against a temp dir instead of whatever happens to sit in
/// `assets/llm/` on the dev machine (a fetched model must not flip the test).
fn locate_model_in(dir: &PathBuf) -> Option<(PathBuf, String, String)> {
    // First .gguf in the directory wins.
    let gguf = std::fs::read_dir(dir).ok()?.find_map(|entry| {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".gguf") {
            Some(name)
        } else {
            None
        }
    })?;

    let tokenizer = std::fs::read_dir(dir)
        .ok()?
        .find_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "tokenizer.json" {
                Some(name)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "tokenizer.json".to_string());

    Some((dir.clone(), gguf, tokenizer))
}

/// Turn a track's MIR analysis into the system+user prompt for recipe authoring.
/// Kept compact (analysis already carries the hard facts) and explicit about
/// the mood palette so the model modulates *within* the right world.
fn build_prompt(analysis: &TrackAnalysis) -> mistralrs::RequestBuilder {
    let mood_name = moods()
        .get(analysis.mood)
        .map(|m| m.world_name)
        .unwrap_or("UNKNOWN");

    let bpm = if analysis.bpm > 0.0 {
        format!("{:.0}", analysis.bpm)
    } else {
        "unknown".into()
    };
    let mean_energy = analysis.energy.iter().sum::<f32>() / analysis.energy.len().max(1) as f32;
    let energy_band = match (mean_energy * 5.0) as usize {
        0..=1 => "low",
        2..=3 => "medium",
        _ => "high",
    };
    let sections = analysis.sections.len().max(1);

    let system = "You design immersive 3D worlds for a music visualizer. Given a song's \
analysis, reply with ONE JSON object (no prose, no code fences) describing how to dress \
the world, with exactly these fields: \
world_name (string — an inventive, evocative name of YOUR OWN that evokes this \
specific song; never merely repeat the mood name, e.g. for Tide Gardens say \
\"Kelp Cathedral at Blue Hour\"), \
biomes (array of {mood: 0-3, layout: \"spiral\"|\"grid\"|\"rings\", \
density: 0..1, tint: [r,g,b]}), landmarks (array of {kind: \"spire\"|\"gateway\"|\"mass\"|\
\"monument\", at: \"center\"|\"cardinal\"|\"rim\", scale: 0.5..3, emissive: 0..1}), \
atmosphere ({fog_density: 0..1, ambient_tint: [r,g,b], bloom_ceiling: 0..1}), \
section_choreography (array of {at_role: \"intro\"|\"verse\"|\"chorus\"|\"drop\"|\"bridge\"|\
\"outro\", energy_shift: -1..1, palette_wash: bool, motion: \"calm\"|\"drift\"|\"active\"}), \
particles ({kind: \"dust\"|\"ember\"|\"snow\"|\"spark\"|\"spore\", rate: 0..1, drift: number}), \
motion_speed (0.25..2.5), density (0.3..2.0). Modulate WITHIN the given mood — do not pick \
a different base palette (the first biome's mood MUST be the given index). Keep it tasteful \
and performant: 1-2 biomes, 0-3 landmarks, modest density.";

    let user = format!(
        "Mood: {mood_name} (the primary biome's mood index must be {idx}).\n\
Tempo: {bpm} BPM. Mean energy: {energy_band} ({mean_energy:.2}). Sections: {sections}.\n\
Reply with only the JSON object.",
        idx = analysis.mood
    );

    mistralrs::RequestBuilder::new()
        .add_message(TextMessageRole::System, system)
        .add_message(TextMessageRole::User, user)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique temp dir for model-discovery tests; caller drops it.
    fn temp_llm_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "verse-llm-test-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn locate_model_returns_none_when_no_gguf() {
        // Empty dir (and, via locate_model, an absent one) → the no-model
        // graceful-fallback path. Tested against a temp dir so a fetched model
        // on the dev machine can't flip the result.
        assert!(locate_model_in(&temp_llm_dir("empty")).is_none());
        assert!(locate_model_in(&PathBuf::from("/nonexistent/verse-llm")).is_none());
    }

    #[test]
    fn locate_model_finds_the_gguf_and_tokenizer() {
        let dir = temp_llm_dir("full");
        std::fs::write(dir.join("some-model.gguf"), b"gguf").unwrap();
        std::fs::write(dir.join("tokenizer.json"), b"{}").unwrap();
        // A non-gguf file must not win the scan.
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();

        let (model_id, gguf, tokenizer) = locate_model_in(&dir).expect("model found");
        assert_eq!(model_id, dir);
        assert_eq!(gguf, "some-model.gguf");
        assert_eq!(tokenizer, "tokenizer.json");
    }

    // The shared model directory's rule is tested where it now lives, in
    // `localgpt_world_agent::paths`.

    #[test]
    fn locate_model_defaults_the_tokenizer_name_when_absent() {
        // A bare GGUF still resolves; mistral.rs reports the missing file at
        // build time (the fetch script's note covers this).
        let dir = temp_llm_dir("no-tok");
        std::fs::write(dir.join("m.gguf"), b"gguf").unwrap();
        let (_, gguf, tokenizer) = locate_model_in(&dir).expect("model found");
        assert_eq!(gguf, "m.gguf");
        assert_eq!(tokenizer, "tokenizer.json");
    }

    /// Manual runtime probe for the recipe tier — the load-bearing question
    /// "does this model + mistral.rs actually generate, and at what speed?"
    /// measured in three phases so a hang is localized:
    ///
    /// 1. plain chat (no grammar) — model + device path sanity
    /// 2. `generate_structured` with a two-field schema — documents the
    ///    mistral.rs 0.8 GGUF grammar hang (expected to time out; kept so a
    ///    future mistral.rs bump can flip the expectation)
    /// 3. [`RecipeModel::generate`] — the real recipe path (plain JSON +
    ///    lenient parse)
    ///
    /// Run explicitly (loads the multi-GB GGUF):
    /// `cargo test --features llm-metal -- --ignored --nocapture llm_generation_probe`
    #[test]
    #[ignore = "loads the local GGUF; run explicitly (see the doc comment)"]
    fn llm_generation_probe() {
        use mistralrs::TextMessages;

        let Some(mut recipe_model) = RecipeModel::try_load() else {
            panic!("no model in assets/llm — run scripts/fetch-bonsai.sh");
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // Phase 1: plain chat. Must succeed — everything else builds on it.
        let started = std::time::Instant::now();
        let reply = rt
            .block_on(
                recipe_model
                    .model_mut()
                    .chat("In one short sentence, describe a desert at dusk."),
            )
            .expect("plain chat generation");
        println!(
            "phase 1 (plain chat): {:.1}s — {reply}",
            started.elapsed().as_secs_f32()
        );
        assert!(!reply.trim().is_empty());

        // Phase 2: structured with a two-field schema. mistral.rs 0.8's
        // grammar-constrained decoding hangs on GGUF (even this trivial
        // schema emits nothing for minutes), which is why `generate` uses
        // plain generation. Bounded to 60s; failure is the documented state.
        #[derive(serde::Deserialize, schemars::JsonSchema)]
        struct Tiny {
            name: String,
            warmth: f32,
        }
        let started = std::time::Instant::now();
        let tiny: Result<Tiny, String> = rt.block_on(async {
            match tokio::time::timeout(
                std::time::Duration::from_secs(60),
                recipe_model
                    .model_mut()
                    .generate_structured(TextMessages::new().add_message(
                        TextMessageRole::User,
                        "Describe a desert at dusk as JSON with a `name` (string) and \
                     `warmth` (number 0..1).",
                    )),
            )
            .await
            {
                Ok(result) => result.map_err(|e| e.to_string()),
                Err(_) => Err("timed out after 60s".into()),
            }
        });
        match tiny {
            Ok(v) => println!(
                "phase 2 (generate_structured): {:.1}s — name {:?}, warmth {:.2} \
                 (a mistral.rs bump fixed the grammar hang!)",
                started.elapsed().as_secs_f32(),
                v.name,
                v.warmth
            ),
            Err(e) => println!(
                "phase 2 (generate_structured): timed out after {:.0}s — {e} \
                 (the documented mistral.rs 0.8 GGUF hang)",
                started.elapsed().as_secs_f32()
            ),
        }

        // Phase 3: the real recipe path against a real-shaped analysis.
        let analysis = TrackAnalysis {
            version: 2,
            duration: 214.0,
            bpm: 128.0,
            beat_offset: 0.4,
            sections: vec![0.0, 0.22, 0.51, 0.78],
            energy: vec![0.2; 214],
            centroid_hz: Some(1800.0),
            mood: 1,
            mood_id: None,
            loudness_lufs: Some(-14.0),
            pinned_mood: None,
            pinned_mood_id: None,
            pinned_seed: None,
            embedding: None,
            stems: None,
            recipe: None,
            build: None,
        };
        let started = std::time::Instant::now();
        match recipe_model.generate(&analysis) {
            Some(v) => println!(
                "phase 3 (RecipeModel::generate): {:.1}s — {:?} ({} biomes, {} landmarks, \
                 {} choreography moments)",
                started.elapsed().as_secs_f32(),
                v.world_name,
                v.biomes.len(),
                v.landmarks.len(),
                v.section_choreography.len()
            ),
            None => panic!(
                "phase 3 (RecipeModel::generate): no recipe after {:.0}s — the real \
                 path is broken for this model",
                started.elapsed().as_secs_f32()
            ),
        }
    }

    #[test]
    fn extract_json_handles_fences_and_prose() {
        assert_eq!(
            extract_json_object("Sure! ```json\n{\"a\": 1}\n``` hope that helps"),
            Some("{\"a\": 1}".to_string())
        );
        assert_eq!(
            extract_json_object("{\"outer\": {\"inner\": \"}\"}, \"tail\": 2}"),
            Some("{\"outer\": {\"inner\": \"}\"}, \"tail\": 2}".to_string())
        );
        assert_eq!(extract_json_object("no object here"), None);
        // Truncated mid-object (a blown token budget) → None, not a partial.
        assert_eq!(extract_json_object("{\"a\": {\"b\": 1"), None);
        // First object wins when the model rambles after answering.
        assert_eq!(
            extract_json_object("{\"a\": 1} and then I said {\"b\": 2}"),
            Some("{\"a\": 1}".to_string())
        );
    }
}
