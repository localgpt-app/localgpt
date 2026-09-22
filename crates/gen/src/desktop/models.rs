//! Which models this machine can use right now, for the prompt panel's model
//! menu: installed CLI backends, plus whatever a local Ollama server has
//! pulled.

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
/// local Ollama models. Never fails; missing backends are simply left out.
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
