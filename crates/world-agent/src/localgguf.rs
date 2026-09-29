//! The one local-GGUF load-and-complete path, shared by the apps.
//!
//! MD's and Verse's `llm.rs` each carried a copy of this — locate the model
//! ([`paths::locate_model_in`], one directory at a time), build it with
//! `GgufModelBuilder` on a single-threaded runtime (mistral.rs is async, the
//! apps are sync), then run one chat completion under a timeout. The
//! constraints were runtime-verified by Verse on Apple Silicon (2026-09) and
//! inherited by MD:
//!
//! - a local path as the model id makes mistral.rs read from disk instead of
//!   fetching from HuggingFace;
//! - plain instructed-JSON generation, never mistral.rs 0.8's
//!   grammar-constrained `generate_structured`, which hangs on GGUF;
//! - the ~5 GB Q4_K_M needs the `llm-metal` GPU path to fit beside a
//!   renderer.
//!
//! What stays with each app: the directory candidates (an env override of
//! its own, then the shared download), the prompt, and what a failed or
//! unparseable reply falls back to.

use std::path::PathBuf;
use std::time::Duration;

use mistralrs::{GgufModelBuilder, Model, RequestBuilder};
use tracing::{info, warn};

use crate::paths;

/// A GGUF loaded into mistral.rs, ready for completions.
pub struct LocalGguf {
    /// The GGUF's file name — the thing to record beside what the model
    /// authored, so a build says which model made it.
    name: String,
    model: Model,
}

/// Why a [`LocalGguf::complete`] gave up. The caller logs this with its own
/// nouns — MD keeps its draft, Verse its rule recipe — so the variants carry
/// the facts, not the message.
#[derive(Debug)]
pub enum CompletionError {
    /// The request failed, or the reply came back empty.
    Failed(String),
    /// The request outlived its timeout; the model may be degraded (thermal
    /// throttle, swap).
    TimedOut(Duration),
}

impl LocalGguf {
    /// Load the first `.gguf` found in `dirs`, in order. `None` (and a
    /// warning naming the directories) when there is no model anywhere —
    /// the caller keeps its rule-derived output.
    pub fn try_load(dirs: &[PathBuf]) -> Option<Self> {
        let (dir, gguf, tokenizer) = locate(dirs)?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| warn!("llm: can't start a tokio runtime: {e}"))
            .ok()?;
        rt.block_on(async move {
            let model =
                GgufModelBuilder::new(dir.to_string_lossy().into_owned(), vec![gguf.clone()])
                    .with_tokenizer_json(tokenizer)
                    .build()
                    .await
                    .map_err(|e| warn!("llm: can't build the mistral.rs model: {e}"))
                    .ok()?;
            info!("llm: model loaded ({gguf})");
            Some(LocalGguf { name: gguf, model })
        })
    }

    /// The loaded model's file name (for the sidecar's `model` fields).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Borrow the underlying model — a tool-calling session
    /// ([`run_session`]) runs on the same loaded model rather than paying
    /// for a second load.
    pub fn model_mut(&mut self) -> &mut Model {
        &mut self.model
    }

    /// One chat completion, under `timeout`. The single-threaded runtime is
    /// built per call, as mistral.rs requires async and the apps are sync.
    pub fn complete(
        &mut self,
        request: RequestBuilder,
        timeout: Duration,
    ) -> Result<String, CompletionError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| CompletionError::Failed(format!("can't start a tokio runtime: {e}")))?;
        rt.block_on(async {
            match tokio::time::timeout(timeout, async {
                self.model
                    .send_chat_request(request)
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|response| {
                        response
                            .choices
                            .into_iter()
                            .next()
                            .and_then(|choice| choice.message.content)
                            .ok_or_else(|| "empty completion".to_string())
                    })
            })
            .await
            {
                Ok(Ok(text)) => Ok(text),
                Ok(Err(e)) => Err(CompletionError::Failed(e)),
                Err(_) => Err(CompletionError::TimedOut(timeout)),
            }
        })
    }
}

/// The first model trio ([`paths::locate_model_in`]) across `dirs`, in
/// order. `None`, with a warning that names the directories, when there is
/// none — the caller's rule-derived output stands.
pub fn locate(dirs: &[PathBuf]) -> Option<(PathBuf, String, String)> {
    for dir in dirs {
        if let Some(found) = paths::locate_model_in(dir) {
            info!("llm: using the model in {}", dir.display());
            return Some(found);
        }
    }
    let looked = dirs
        .iter()
        .map(|dir| dir.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    warn!(
        "llm: no model found (looked in {looked}) — the rule-derived output stands; \
         run scripts/fetch-model.sh"
    );
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "localgpt-localgguf-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// No directory, a nonexistent one and an empty one all find nothing —
    /// the contract the apps' fallbacks lean on. (A real load is not tested
    /// here: it needs a ~5 GB model, and the apps' own generation probes
    /// cover it on machines that have one.)
    #[test]
    fn locate_finds_nothing_where_there_is_no_model() {
        assert!(locate(&[]).is_none());
        assert!(locate(&[temp_dir("empty")]).is_none());
        assert!(locate(&[PathBuf::from("/nonexistent/localgguf")]).is_none());
    }

    #[test]
    fn locate_takes_the_first_directory_that_has_one() {
        let (empty, full) = (temp_dir("empty-first"), temp_dir("full"));
        std::fs::write(full.join("some-model.gguf"), b"gguf").unwrap();
        std::fs::write(full.join("tokenizer.json"), b"{}").unwrap();
        assert_eq!(
            locate(&[empty, full.clone()]),
            Some((full, "some-model.gguf".into(), "tokenizer.json".into()))
        );
    }
}
