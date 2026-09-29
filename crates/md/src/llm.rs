//! LLM recipe authoring — PLAN.md M1 (`llm` feature). The load-and-complete
//! plumbing is [`localgpt_world_agent::LocalGguf`] (ported out of this module
//! and Verse's twin of it); what lives here is MD's half — the model
//! directory candidates, the prompt for one section, and the parse into a
//! [`RegionRecipe`] — inheriting the constraints Verse runtime-verified on
//! Apple Silicon (2026-09):
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

use std::path::PathBuf;

use bevy::prelude::*;
use mistralrs::TextMessageRole;

use localgpt_md::recipe::RegionRecipe;
use localgpt_world_agent::{CompletionError, LocalGguf};

/// The loaded LLM, ready to author recipes and agent builds. `None` from
/// [`Self::try_load`] when no model file is found — the caller keeps the
/// rule-derived draft.
pub struct RecipeModel {
    gguf: LocalGguf,
}

impl RecipeModel {
    /// Load the first model in MD's candidates: `$LOCALGPT_MD_LLM`, then the
    /// directory every LocalGPT app shares (where `scripts/fetch-model.sh`
    /// puts it), then the older per-app spot `assets/llm/`.
    pub fn try_load() -> Option<Self> {
        let candidates = [
            std::env::var_os("LOCALGPT_MD_LLM").map(PathBuf::from),
            localgpt_world_agent::shared_llm_dir(),
            Some(PathBuf::from("assets/llm")),
        ];
        Some(Self {
            gguf: LocalGguf::try_load(&candidates.into_iter().flatten().collect::<Vec<_>>())?,
        })
    }

    /// The loaded model's file name (for the sidecar's `model` fields).
    pub fn name(&self) -> &str {
        self.gguf.name()
    }

    /// Borrow the underlying model — the agent tier (`crate::agent`) runs
    /// its tool-calling session on the same loaded model so we don't pay for
    /// two loads.
    pub fn model_mut(&mut self) -> &mut mistralrs::Model {
        self.gguf.model_mut()
    }

    /// Author a recipe for one section. `None` on any failure — the caller
    /// keeps the draft. The returned recipe is already clamped, so no
    /// out-of-range value can reach the world.
    pub fn generate(&mut self, heading: &str, body: &str, genre: &str) -> Option<RegionRecipe> {
        let text = match self
            .gguf
            .complete(build_prompt(heading, body, genre), GENERATE_TIMEOUT)
        {
            Ok(text) => text,
            Err(CompletionError::Failed(e)) => {
                warn!("llm: recipe generation failed ({e}) — keeping draft");
                return None;
            }
            Err(CompletionError::TimedOut(timeout)) => {
                warn!(
                    "llm: recipe generation timed out after {}s — keeping draft",
                    timeout.as_secs()
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

    #[test]
    fn prompt_excerpt_cuts_at_a_word_boundary() {
        assert_eq!(prompt_excerpt("short text"), "short text");
        let long = "word ".repeat(200);
        let cut = prompt_excerpt(&long);
        assert!(cut.chars().count() <= 600);
        assert!(!cut.ends_with(' '));
    }

    // The model-directory rule (shared download first, any GGUF + matching
    // tokenizer picked up) is tested where it lives, in
    // `localgpt_world_agent`'s `paths` and `localgguf`.
}
