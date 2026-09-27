//! Which models this machine can use right now, for the prompt panel's model
//! menu: installed CLI backends, whatever a local Ollama server has pulled,
//! and (with the `local-llm` feature) the GGUF models Gen can run itself.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// CLI backends by executable name, and the model strings that select them
/// (the providers' defaults; see `localgpt_core::config`).
const CLI_BACKENDS: [(&str, &[&str]); 3] = [
    ("claude", &["claude-cli/opus", "claude-cli/sonnet"]),
    ("gemini", &["gemini-cli/gemini-3.1-pro-preview"]),
    ("codex", &["codex-cli/o4-mini"]),
];

/// Where Ollama listens unless the config says otherwise.
pub const DEFAULT_OLLAMA_ENDPOINT: &str = "http://localhost:11434";

/// A local model Gen knows how to run in-process, in preference order.
///
/// Sizes and the runtime caveats come from the runs LocalGPT Verse and MD did
/// on Apple Silicon (see their `scripts/fetch-bonsai.sh`): only standard GGUF
/// quants load in mistral.rs 0.8 — ternary/1-bit packings fail with "Critical
/// failure loading model part 0", which is why PrismML's newer
/// Ternary-Bonsai-2-27B is absent here. Add an entry when a packing is
/// actually verified, not when it is announced.
pub struct KnownModel {
    /// GGUF filename in the shared model directory, without `.gguf`. Also the
    /// `gguf/<name>` model string.
    pub name: &'static str,
    /// Short label for the menu.
    pub label: &'static str,
    /// On-disk size in GiB.
    pub size_gb: f32,
    /// How to get it, shown when it isn't downloaded.
    pub fetch_hint: &'static str,
}

/// Local models in preference order: Bonsai-8B first (the configuration Verse
/// runtime-verified), then the stock Qwen A/B that MD documents as a drop-in.
pub const KNOWN_MODELS: &[KnownModel] = &[
    KnownModel {
        name: "prism-ml_Bonsai-8B-unpacked-Q4_K_M",
        label: "Bonsai-8B Q4_K_M",
        size_gb: 5.2,
        fetch_hint: "scripts/fetch-model.sh (one download, shared with MD and Verse)",
    },
    KnownModel {
        name: "Qwen3-8B-Instruct-Q4_K_M",
        label: "Qwen3-8B-Instruct Q4_K_M",
        size_gb: 4.7,
        fetch_hint: "any standard Q4_K_M GGUF + tokenizer.json in the shared model dir",
    },
];

/// The local model to use for `[gen] default_model = "auto"`: the first known
/// model that is both downloaded and fits this machine, else any other GGUF
/// present that fits. `None` when nothing local is usable, so the caller
/// keeps whatever model was already configured.
#[cfg(feature = "local-llm")]
pub fn auto_local_model() -> Option<String> {
    let present = crate::local_llm::available_models();
    let is_present = |name: &str| {
        let id = format!("{}/{name}", crate::local_llm::PREFIX);
        present.contains(&id).then_some(id)
    };

    for known in KNOWN_MODELS {
        if let Some(id) = is_present(known.name)
            && super::hardware::fits(known.size_gb)
        {
            return Some(id);
        }
    }
    // An unknown GGUF the user dropped in: size it from the file itself.
    present.into_iter().find(|id| match gguf_size_gb(id) {
        Some(size) => super::hardware::fits(size),
        None => true,
    })
}

#[cfg(not(feature = "local-llm"))]
pub fn auto_local_model() -> Option<String> {
    None
}

/// Size of the GGUF behind a `gguf/<name>` id, in GiB.
#[cfg(feature = "local-llm")]
fn gguf_size_gb(model_id: &str) -> Option<f32> {
    let name = model_id.strip_prefix(&format!("{}/", crate::local_llm::PREFIX))?;
    let dir = localgpt_world_agent::shared_llm_dir()?;
    let bytes = std::fs::metadata(dir.join(format!("{name}.gguf")))
        .ok()?
        .len();
    Some(bytes as f32 / (1024.0 * 1024.0 * 1024.0))
}

/// Find `program` on `PATH` (trying `.exe` and `.cmd` on Windows).
pub fn find_on_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| executable_in(&dir, program))
}

fn executable_in(dir: &Path, program: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let names = [
        format!("{program}.exe"),
        format!("{program}.cmd"),
        program.to_string(),
    ];
    #[cfg(not(windows))]
    let names = [program.to_string()];
    names
        .into_iter()
        .map(|name| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// Models worth offering: `current` first, then installed CLI backends, then
/// local Ollama models, then in-process GGUF models. Never fails; missing
/// backends are simply left out.
pub async fn detect_model_options(current: &str, ollama_endpoint: &str) -> Vec<String> {
    let mut options = vec![current.to_string()];
    for (program, models) in CLI_BACKENDS {
        if find_on_path(program).is_some() {
            options.extend(models.iter().map(|model| model.to_string()));
        }
    }
    options.extend(
        ollama_models(ollama_endpoint)
            .await
            .into_iter()
            .map(|model| format!("ollama/{model}")),
    );
    #[cfg(feature = "local-llm")]
    options.extend(crate::local_llm::available_models());
    dedupe(options)
}

/// Models a local Ollama server has pulled; empty when it isn't running.
async fn ollama_models(endpoint: &str) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct Tags {
        models: Vec<Model>,
    }
    #[derive(serde::Deserialize)]
    struct Model {
        name: String,
    }

    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
    else {
        return Vec::new();
    };
    let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));
    let Ok(response) = client.get(url).send().await else {
        return Vec::new();
    };
    response
        .json::<Tags>()
        .await
        .map(|tags| tags.models.into_iter().map(|model| model.name).collect())
        .unwrap_or_default()
}

/// Drop repeats, keeping the first occurrence of each.
fn dedupe(items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|item| seen.insert(item.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn finds_programs_on_path() {
        assert!(find_on_path("sh").is_some());
        assert!(find_on_path("localgpt-gen-no-such-program").is_none());
    }

    #[test]
    fn dedupe_keeps_first_occurrence_order() {
        let items = ["b", "a", "b", "c", "a"].map(String::from).to_vec();
        assert_eq!(dedupe(items), ["b", "a", "c"]);
    }

    #[tokio::test]
    async fn unreachable_ollama_yields_no_models() {
        // Port 9 (discard) is never an Ollama server.
        assert!(ollama_models("http://127.0.0.1:9").await.is_empty());
    }
}
