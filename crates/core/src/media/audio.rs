//! Audio transcription (Speech-to-Text) support for LocalGPT.
//!
//! Uses a provider registry pattern with ordered fallback:
//! 1. Groq API (fast, free Whisper)
//! 2. OpenAI API (fallback)
//! 3. CLI tools (offline fallback)

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Maximum audio file size (25MB)
const DEFAULT_MAX_BYTES: usize = 26_214_400;

/// Default language hint for transcription
const DEFAULT_LANGUAGE: &str = "en";

/// STT provider trait
#[async_trait]
pub trait SttProvider: Send + Sync {
    /// Unique identifier for this provider
    fn id(&self) -> &str;

    /// Transcribe audio data
    ///
    /// # Arguments
    /// * `audio` - Raw audio bytes
    /// * `mime_type` - MIME type of the audio (e.g., "audio/ogg", "audio/mp3")
    /// * `config` - Transcription configuration
    ///
    /// # Returns
    /// * `Ok(String)` - Transcribed text
    /// * `Err` - If transcription fails
    async fn transcribe(&self, audio: &[u8], mime_type: &str, config: &SttConfig)
    -> Result<String>;
}

/// Configuration for a single STT provider
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SttProviderConfig {
    /// Provider type: "groq", "openai", "cli"
    #[serde(rename = "type")]
    pub provider_type: String,

    /// Model to use (provider-specific)
    #[serde(default)]
    pub model: Option<String>,

    /// API key (optional, can use env var)
    #[serde(default)]
    pub api_key: Option<String>,

    /// CLI command (for type = "cli")
    #[serde(default)]
    pub command: Option<String>,

    /// CLI arguments with template variables
    #[serde(default)]
    pub args: Vec<String>,
}

/// Global STT configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    /// Enable STT functionality
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Language hint for transcription
    #[serde(default = "default_language")]
    pub language: String,

    /// Maximum audio file size in bytes
    #[serde(default = "default_max_bytes")]
    pub max_bytes: usize,

    /// Ordered list of providers to try
    #[serde(default)]
    pub providers: Vec<SttProviderConfig>,
}

fn default_enabled() -> bool {
    true
}

fn default_language() -> String {
    DEFAULT_LANGUAGE.to_string()
}

fn default_max_bytes() -> usize {
    DEFAULT_MAX_BYTES
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            language: DEFAULT_LANGUAGE.to_string(),
            max_bytes: DEFAULT_MAX_BYTES,
            providers: vec![],
        }
    }
}

/// Groq API STT provider (OpenAI-compatible)
pub struct GroqProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl GroqProvider {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| "whisper-large-v3".to_string()),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl SttProvider for GroqProvider {
    fn id(&self) -> &str {
        "groq"
    }

    async fn transcribe(
        &self,
        audio: &[u8],
        mime_type: &str,
        config: &SttConfig,
    ) -> Result<String> {
        let file_part = multipart::Part::bytes(audio.to_vec())
            .file_name("audio.ogg")
            .mime_str(mime_type)?;

        let mut form = multipart::Form::new()
            .part("file", file_part)
            .text("model", self.model.clone());

        if !config.language.is_empty() {
            form = form.text("language", config.language.clone());
        }

        let response = self
            .client
            .post("https://api.groq.com/openai/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .context("Groq API request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Groq API error ({}): {}", status, body);
        }

        let result: TranscriptionResponse = response
            .json()
            .await
            .context("Failed to parse Groq API response")?;

        Ok(result.text)
    }
}

/// OpenAI API STT provider
pub struct OpenAiProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| "gpt-4o-mini-transcribe".to_string()),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl SttProvider for OpenAiProvider {
    fn id(&self) -> &str {
        "openai"
    }

    async fn transcribe(
        &self,
        audio: &[u8],
        mime_type: &str,
        config: &SttConfig,
    ) -> Result<String> {
        let file_part = multipart::Part::bytes(audio.to_vec())
            .file_name("audio.ogg")
            .mime_str(mime_type)?;

        let mut form = multipart::Form::new()
            .part("file", file_part)
            .text("model", self.model.clone());

        if !config.language.is_empty() {
            form = form.text("language", config.language.clone());
        }

        let response = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .context("OpenAI API request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("OpenAI API error ({}): {}", status, body);
        }

        let result: TranscriptionResponse = response
            .json()
            .await
            .context("Failed to parse OpenAI API response")?;

        Ok(result.text)
    }
}

/// CLI-based STT provider (whisper, sherpa-onnx, etc.)
pub struct CliProvider {
    command: String,
    args: Vec<String>,
}

impl CliProvider {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self { command, args }
    }

    /// Substitute template variables in arguments
    fn substitute_args(&self, input_path: &Path, output_dir: &Path, language: &str) -> Vec<String> {
        self.args
            .iter()
            .map(|arg| {
                arg.replace("{{input}}", &input_path.display().to_string())
                    .replace("{{output_dir}}", &output_dir.display().to_string())
                    .replace("{{language}}", language)
            })
            .collect()
    }
}

#[async_trait]
impl SttProvider for CliProvider {
    fn id(&self) -> &str {
        "cli"
    }

    async fn transcribe(
        &self,
        audio: &[u8],
        _mime_type: &str,
        config: &SttConfig,
    ) -> Result<String> {
        // Write audio to temp file
        let temp_dir = tempfile::tempdir().context("Failed to create temp directory")?;
        let input_path = temp_dir.path().join("audio");
        std::fs::write(&input_path, audio).context("Failed to write audio temp file")?;

        let args = self.substitute_args(&input_path, temp_dir.path(), &config.language);

        debug!("Running CLI STT: {} {}", self.command, args.join(" "));

        let output = Command::new(&self.command)
            .args(&args)
            .output()
            .context("Failed to execute CLI STT command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("CLI STT command failed: {}", stderr);
        }

        // Try to read from stdout first
        let stdout = String::from_utf8(output.stdout)?;
        if !stdout.trim().is_empty() {
            return Ok(stdout.trim().to_string());
        }

        // Otherwise, look for generated .txt file
        for entry in std::fs::read_dir(temp_dir.path())? {
            let entry = entry?;
            if let Some(ext) = entry.path().extension()
                && ext == "txt"
            {
                let text = std::fs::read_to_string(entry.path())?;
                return Ok(text.trim().to_string());
            }
        }

        bail!("CLI STT produced no output")
    }
}

/// Response from transcription APIs
#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

/// STT registry that manages providers and handles fallback
pub struct SttRegistry {
    providers: Vec<Arc<dyn SttProvider>>,
    config: SttConfig,
}

impl SttRegistry {
    /// Create a new STT registry with the given configuration
    pub fn new(config: SttConfig) -> Self {
        Self {
            providers: Vec::new(),
            config,
        }
    }

    /// Add a provider to the registry
    pub fn add_provider(&mut self, provider: Arc<dyn SttProvider>) {
        self.providers.push(provider);
    }

    /// Create registry from configuration
    pub fn from_config(config: &SttConfig, env_vars: &HashMap<String, String>) -> Self {
        let mut registry = Self::new(config.clone());

        for provider_config in &config.providers {
            match provider_config.provider_type.as_str() {
                "groq" => {
                    let api_key = provider_config
                        .api_key
                        .clone()
                        .or_else(|| env_vars.get("GROQ_API_KEY").cloned())
                        .or_else(|| env_vars.get("GROQ_API_TOKEN").cloned());

                    if let Some(key) = api_key {
                        registry.add_provider(Arc::new(GroqProvider::new(
                            key,
                            provider_config.model.clone(),
                        )));
                    } else {
                        debug!("Skipping Groq provider: no API key configured");
                    }
                }
                "openai" => {
                    let api_key = provider_config
                        .api_key
                        .clone()
                        .or_else(|| env_vars.get("OPENAI_API_KEY").cloned());

                    if let Some(key) = api_key {
                        registry.add_provider(Arc::new(OpenAiProvider::new(
                            key,
                            provider_config.model.clone(),
                        )));
                    } else {
                        debug!("Skipping OpenAI provider: no API key configured");
                    }
                }
                "cli" => {
                    if let Some(ref command) = provider_config.command {
                        registry.add_provider(Arc::new(CliProvider::new(
                            command.clone(),
                            provider_config.args.clone(),
                        )));
                    } else {
                        warn!("CLI provider missing 'command' field");
                    }
                }
                other => {
                    warn!("Unknown STT provider type: {}", other);
                }
            }
        }

        registry
    }

    /// Transcribe audio, trying providers in order
    pub async fn transcribe(&self, audio: &[u8], mime_type: &str) -> Result<String> {
        if !self.config.enabled {
            bail!("STT is disabled in configuration");
        }

        if audio.len() > self.config.max_bytes {
            bail!(
                "Audio file too large: {} bytes (max: {})",
                audio.len(),
                self.config.max_bytes
            );
        }

        if self.providers.is_empty() {
            bail!("No STT providers configured");
        }

        let mut last_error = None;

        for provider in &self.providers {
            match provider.transcribe(audio, mime_type, &self.config).await {
                Ok(text) => {
                    info!(
                        "Transcribed audio via {} ({} chars)",
                        provider.id(),
                        text.len()
                    );
                    return Ok(text);
                }
                Err(e) => {
                    warn!("STT provider {} failed: {}", provider.id(), e);
                    last_error = Some(e);
                }
            }
        }

        bail!(
            "All STT providers failed. Last error: {}",
            last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error"))
        )
    }

    /// Check if any providers are available
    pub fn has_providers(&self) -> bool {
        !self.providers.is_empty()
    }

    /// Get list of configured provider IDs
    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.id()).collect()
    }
}

/// Get MIME type from file extension
pub fn mime_type_from_path(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("ogg") => "audio/ogg",
        Some("oga") => "audio/ogg",
        Some("opus") => "audio/opus",
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("webm") => "audio/webm",
        Some("flac") => "audio/flac",
        _ => "audio/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stt_config_default() {
        let config = SttConfig::default();
        assert!(config.enabled);
        assert_eq!(config.language, "en");
        assert_eq!(config.max_bytes, DEFAULT_MAX_BYTES);
        assert!(config.providers.is_empty());
    }

    #[test]
    fn test_mime_type_from_path() {
        assert_eq!(mime_type_from_path(Path::new("audio.ogg")), "audio/ogg");
        assert_eq!(mime_type_from_path(Path::new("audio.mp3")), "audio/mpeg");
        assert_eq!(mime_type_from_path(Path::new("audio.m4a")), "audio/mp4");
        assert_eq!(mime_type_from_path(Path::new("audio.wav")), "audio/wav");
        assert_eq!(
            mime_type_from_path(Path::new("audio.unknown")),
            "audio/octet-stream"
        );
    }

    #[test]
    fn test_cli_provider_substitute_args() {
        let provider = CliProvider::new(
            "whisper".to_string(),
            vec![
                "{{input}}".to_string(),
                "--output_dir".to_string(),
                "{{output_dir}}".to_string(),
                "--language".to_string(),
                "{{language}}".to_string(),
            ],
        );

        let args =
            provider.substitute_args(Path::new("/tmp/audio.ogg"), Path::new("/tmp/output"), "en");

        assert_eq!(args[0], "/tmp/audio.ogg");
        assert_eq!(args[1], "--output_dir");
        assert_eq!(args[2], "/tmp/output");
        assert_eq!(args[3], "--language");
        assert_eq!(args[4], "en");
    }

    #[test]
    fn test_registry_no_providers() {
        let registry = SttRegistry::new(SttConfig::default());

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(registry.transcribe(b"test".as_slice(), "audio/ogg"));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No STT providers"));
    }

    #[test]
    fn test_registry_file_too_large() {
        let config = SttConfig {
            max_bytes: 10,
            ..Default::default()
        };
        let registry = SttRegistry::new(config);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(registry.transcribe(b"test audio data".as_slice(), "audio/ogg"));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too large"));
    }

    #[test]
    fn test_registry_from_config() {
        let mut config = SttConfig::default();
        config.providers.push(SttProviderConfig {
            provider_type: "cli".to_string(),
            command: Some("echo".to_string()),
            args: vec!["test".to_string()],
            ..Default::default()
        });

        let env_vars = HashMap::new();
        let registry = SttRegistry::from_config(&config, &env_vars);

        assert!(registry.has_providers());
        assert_eq!(registry.provider_ids(), vec!["cli"]);
    }

    #[test]
    fn test_registry_skips_provider_without_api_key() {
        let mut config = SttConfig::default();
        config.providers.push(SttProviderConfig {
            provider_type: "groq".to_string(),
            ..Default::default()
        });

        let env_vars = HashMap::new();
        let registry = SttRegistry::from_config(&config, &env_vars);

        assert!(!registry.has_providers());
    }
}
