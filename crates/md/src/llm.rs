//! LLM recipe authoring — PLAN.md M1 (`llm` feature). Ported from LocalGPT
//! Verse's `src/llm.rs` (Apache-2.0), inheriting the constraints Verse
//! runtime-verified on Apple Silicon (2026-09):
//!   - plain instructed-JSON generation with a lenient parse — mistral.rs
//!     0.8's grammar-constrained `generate_structured` hangs on GGUF;
//!   - the ~5 GB Q4_K_M needs the `llm-metal` GPU path to fit in memory
//!     beside a renderer;
//!   - any standard GGUF + matching `tokenizer.json` dropped in the model
//!     directory is picked up unchanged.
//!
//! Every step degrades to `None` (no model → [`RecipeModel::try_load`] is
//! `None`; generation fails or times out → [`RecipeModel::generate`] is
//! `None`): the caller keeps the rule-derived draft, so the app is never
//! broken by a missing LLM tier.
//!
//! Like Verse's module: mistral.rs is async, the app is sync, so each call
//! runs on a dedicated single-threaded tokio runtime.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use mistralrs::{GgufModelBuilder, TextMessageRole};

use localgpt_md::recipe::RegionRecipe;

/// The loaded LLM, ready to author recipes and agent builds. `None` from
/// [`Self::try_load`] when no model file is found — the caller keeps the
/// rule-derived draft.
pub struct RecipeModel {
    /// The GGUF's file name — recorded in the sidecar so a build says which
    /// model authored it (PLAN.md M2's open note).
    name: String,
    model: mistralrs::Model,
}

impl RecipeModel {
    /// Load the first `.gguf` in the model directory (see [`locate_model`]).
    /// Returns `None` (and logs) when no model is present.
    pub fn try_load() -> Option<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| warn!("llm: can't start tokio runtime: {e}"))
            .ok()?;

        let (model_dir, gguf_file, tokenizer_json) = locate_model()?;

        rt.block_on(async move {
            // A local path as the model id makes mistral.rs read from disk
            // instead of fetching from HuggingFace (Verse's finding).
            let model = GgufModelBuilder::new(
                model_dir.to_string_lossy().into_owned(),
                vec![gguf_file.clone()],
            )
            .with_tokenizer_json(tokenizer_json.clone())
            .build()
            .await
            .map_err(|e| warn!("llm: can't build mistral.rs model: {e}"))
            .ok()?;
            info!("llm: recipe model loaded ({gguf_file})");
            Some(RecipeModel {
                name: gguf_file,
                model,
            })
        })
    }

    /// The loaded model's file name (for the sidecar's `model` fields).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Borrow the underlying model — the agent tier (`crate::agent`) runs
    /// its tool-calling session on the same loaded model so we don't pay for
    /// two loads (Verse's `RecipeModel::model_mut`).
    pub fn model_mut(&mut self) -> &mut mistralrs::Model {
        &mut self.model
    }

    /// Author a recipe for one section. `None` on any failure — the caller
    /// keeps the draft. The returned recipe is already clamped, so no
    /// out-of-range value can reach the world.
    pub fn generate(&mut self, heading: &str, body: &str, genre: &str) -> Option<RegionRecipe> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| warn!("llm: can't start tokio runtime: {e}"))
            .ok()?;

        rt.block_on(async {
            let messages = build_prompt(heading, body, genre);
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
                    warn!("llm: recipe generation failed ({e}) — keeping draft");
                    return None;
                }
                Err(_) => {
                    warn!(
                        "llm: recipe generation timed out after {}s — keeping draft",
                        GENERATE_TIMEOUT.as_secs()
                    );
                    return None;
                }
            };

            match RegionRecipe::from_llm_text(&text) {
                Some(recipe) => {
                    info!("llm: recipe for \"{heading}\"");
                    Some(recipe.clamped())
                }
                None => {
                    warn!(
                        "llm: reply wasn't valid JSON ({} bytes) — keeping draft",
                        text.len()
                    );
                    None
                }
            }
        })
    }
}

/// How long one generation may run before giving up (keeping the draft).
/// Verse measured ~25 tok/s CPU-side for this model class; a ~100-token
/// recipe normally lands in seconds — the cap is for degradation.
const GENERATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// The part of a section's prose the prompt carries: enough for the model to
/// see the scene, short enough to keep generation quick.
pub fn prompt_excerpt(body: &str) -> String {
    const MAX_CHARS: usize = 600;
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_CHARS {
        return flat;
    }
    let cut: String = flat.chars().take(MAX_CHARS).collect();
    match cut.rsplit_once(' ') {
        Some((head, _)) => head.to_string(),
        None => cut,
    }
}

/// Where the model lives: `$LOCALGPT_MD_LLM`, then the directory every
/// LocalGPT app shares ([`localgpt_world_agent::shared_llm_dir`], where
/// `scripts/fetch-bonsai.sh` puts it, so one download serves MD, Verse and
/// Gen), then the older per-app spot `assets/llm/`.
/// Returns the `(directory, gguf, tokenizer)` trio `GgufModelBuilder` wants,
/// or `None` (with a warning) when absent.
fn locate_model() -> Option<(PathBuf, String, String)> {
    let shared = localgpt_world_agent::shared_llm_dir();
    let candidates = [
        std::env::var_os("LOCALGPT_MD_LLM").map(PathBuf::from),
        shared.clone(),
        Some(PathBuf::from("assets/llm")),
    ];
    for dir in candidates.into_iter().flatten() {
        if let Some(found) = locate_model_in(&dir) {
            info!("llm: using model in {}", dir.display());
            return Some(found);
        }
    }
    let shared = shared.map_or_else(|| "$LOCALGPT_LLM_DIR".into(), |d| d.display().to_string());
    warn!(
        "llm: no model found (looked in $LOCALGPT_MD_LLM, {shared}, assets/llm) \
         — rule drafts only; run scripts/fetch-bonsai.sh"
    );
    None
}

/// [`locate_model`] over one directory. The rule is shared with Verse and Gen
/// in `localgpt_world_agent::paths`; this alias keeps the call sites and the
/// tests below reading the same as before.
fn locate_model_in(dir: &Path) -> Option<(PathBuf, String, String)> {
    localgpt_world_agent::paths::locate_model_in(dir)
}

/// The prompt: system describes the JSON contract; user carries the section.
fn build_prompt(heading: &str, body: &str, genre: &str) -> mistralrs::RequestBuilder {
    let genre_note = if genre == "deck" {
        "This section is one slide of a presentation: pick ONE focal idea — \
the slide's key message becomes the landmark — and derive the palette from \
the slide's own subject, so each slide's stage feels different from the \
last."
    } else {
        "Match the setting the text describes: a night shore wants deep \
blues and a low glow; a library wants warm ambers and dense blocks like \
shelves."
    };
    let system = format!(
        "You design one region of a calm, walkable 3D world that reflects one \
section of a document. Reply with ONE JSON object only (no prose, no code fences) with \
these optional fields: accent ([r,g,b] in 0..1 — the region's signature colour), \
ground ([r,g,b] in 0..1 — the ground tint), landmark ({{
kind: \"pyramid\"|\"cone\"|\"column\"|\"cube\"|\"orb\"|\"ring\", scale: 0.5..2.5, \
emissive: 0..1}}), props ({{kind: \"blocks\"|\"spheres\"|\"crystals\", count: 3..12}}). \
Let the text decide everything, and make this region's look its OWN — a shore and a \
library must not share a palette. {genre_note} Colours are muted and desaturated \
(dusk light, weathered stone, deep water — never pure primaries like [0,0,1] or \
[1,0,0], never neon) yet still distinct and specific to this text."
    );

    let user = format!(
        "Document genre: {genre}.\nSection heading: \"{heading}\".\nSection text: \"{}\".\n\
         Reply with only the JSON object.",
        prompt_excerpt(body)
    );

    mistralrs::RequestBuilder::new()
        .add_message(TextMessageRole::System, system)
        .add_message(TextMessageRole::User, user)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "localgpt-md-llm-{tag}-{}",
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
        assert!(locate_model_in(&temp_dir("empty")).is_none());
        assert!(locate_model_in(Path::new("/nonexistent/localgpt-md-llm")).is_none());
    }

    #[test]
    fn locate_model_finds_the_gguf_and_tokenizer() {
        let dir = temp_dir("full");
        std::fs::write(dir.join("some-model.gguf"), b"gguf").unwrap();
        std::fs::write(dir.join("tokenizer.json"), b"{}").unwrap();
        assert_eq!(
            locate_model_in(&dir),
            Some((
                dir.clone(),
                "some-model.gguf".into(),
                "tokenizer.json".into()
            ))
        );
    }

    // The shared model directory's rule is tested where it now lives, in
    // `localgpt_world_agent::paths`.

    #[test]
    fn prompt_excerpt_cuts_at_a_word_boundary() {
        assert_eq!(prompt_excerpt("short text"), "short text");
        let long = "word ".repeat(200);
        let cut = prompt_excerpt(&long);
        assert!(cut.chars().count() <= 600);
        assert!(!cut.ends_with(' '));
    }
}
