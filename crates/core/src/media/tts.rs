//! Text-to-Speech (TTS) support for LocalGPT.
//!
//! Uses a provider registry pattern with ordered fallback:
//! 1. OpenAI API (high quality)
//! 2. Edge TTS (free, Microsoft Edge's online TTS)
//! 3. CLI tools (offline fallback: espeak, pico2wave, say)

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// TTS provider trait
#[async_trait]
pub trait TtsProvider: Send + Sync {
    /// Unique identifier for this provider
    fn id(&self) -> &str;

    /// Synthesize speech from text
    ///
    /// # Arguments
    /// * `text` - Text to synthesize
    /// * `config` - Synthesis configuration
    ///
    /// # Returns
    /// * `Ok(Vec<u8>)` - Audio data (typically MP3 or WAV)
    /// * `Err` - If synthesis fails
    async fn synthesize(&self, text: &str, config: &TtsConfig) -> Result<Vec<u8>>;
}

/// Configuration for a single TTS provider
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TtsProviderConfig {
    /// Provider type: "openai", "edge", "cli"
    #[serde(rename = "type")]
    pub provider_type: String,

    /// Voice to use (provider-specific)
    #[serde(default)]
    pub voice: Option<String>,

    /// API key (optional, can use env var)
    #[serde(default)]
    pub api_key: Option<String>,

    /// CLI command (for type = "cli")
    #[serde(default)]
    pub command: Option<String>,

    /// CLI arguments with template variables
    #[serde(default)]
    pub args: Vec<String>,

    /// Output format: "mp3", "wav", "ogg"
    #[serde(default)]
    pub format: Option<String>,
}

/// Global TTS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    /// Enable TTS functionality
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Default voice to use
    #[serde(default = "default_voice")]
    pub voice: String,

    /// Output format: "mp3", "wav", "ogg"
    #[serde(default = "default_format")]
    pub format: String,

    /// Maximum text length to synthesize (characters)
    #[serde(default = "default_max_text_length")]
    pub max_text_length: usize,

    /// Ordered list of providers to try
    #[serde(default)]
    pub providers: Vec<TtsProviderConfig>,
}

fn default_voice() -> String {
    "alloy".to_string() // OpenAI default
}

fn default_format() -> String {
    "mp3".to_string()
}

fn default_max_text_length() -> usize {
    4096
}

fn default_enabled() -> bool {
    true
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            voice: default_voice(),
            format: default_format(),
            max_text_length: default_max_text_length(),
            providers: vec![],
        }
    }
}

/// OpenAI TTS provider
pub struct OpenAiProvider {
    api_key: String,
    voice: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String, voice: Option<String>, model: Option<String>) -> Self {
        Self {
            api_key,
            voice: voice.unwrap_or_else(|| "alloy".to_string()),
            model: model.unwrap_or_else(|| "tts-1".to_string()),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl TtsProvider for OpenAiProvider {
    fn id(&self) -> &str {
        "openai"
    }

    async fn synthesize(&self, text: &str, config: &TtsConfig) -> Result<Vec<u8>> {
        let response = self
            .client
            .post("https://api.openai.com/v1/audio/speech")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&serde_json::json!({
                "model": self.model,
                "input": text,
                "voice": self.voice,
                "response_format": config.format,
            }))
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .context("OpenAI TTS API request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("OpenAI TTS API error ({}): {}", status, body);
        }

        let audio = response
            .bytes()
            .await
            .context("Failed to read OpenAI TTS response")?;

        Ok(audio.to_vec())
    }
}

/// Edge TTS provider (free, uses Microsoft Edge's online TTS)
pub struct EdgeTtsProvider {
    voice: String,
}

impl EdgeTtsProvider {
    pub fn new(voice: Option<String>) -> Self {
        Self {
            voice: voice.unwrap_or_else(|| "en-US-AriaNeural".to_string()),
        }
    }
}

#[async_trait]
impl TtsProvider for EdgeTtsProvider {
    fn id(&self) -> &str {
        "edge"
    }

    async fn synthesize(&self, text: &str, _config: &TtsConfig) -> Result<Vec<u8>> {
        // Use edge-tts CLI (Python package: pip install edge-tts)
        let temp_dir = tempfile::tempdir().context("Failed to create temp directory")?;
        let output_path = temp_dir.path().join("output.mp3");

        let output = Command::new("edge-tts")
            .arg("--text")
            .arg(text)
            .arg("--voice")
            .arg(&self.voice)
            .arg("--write-media")
            .arg(&output_path)
            .output()
            .context("Failed to execute edge-tts command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "edge-tts command failed: {}. Install with: pip install edge-tts",
                stderr
            );
        }

        std::fs::read(&output_path).context("Failed to read edge-tts output file")
    }
}

/// CLI-based TTS provider (espeak, pico2wave, say, etc.)
pub struct CliProvider {
    command: String,
    args: Vec<String>,
}

impl CliProvider {
    pub fn new(command: String, args: Vec<String>) -> Self {
        Self { command, args }
    }

    /// Substitute template variables in arguments
    fn substitute_args(&self, text: &str, output_path: &Path) -> Vec<String> {
        self.args
            .iter()
            .map(|arg| {
                arg.replace("{{text}}", text)
                    .replace("{{output}}", &output_path.display().to_string())
            })
            .collect()
    }
}

#[async_trait]
impl TtsProvider for CliProvider {
    fn id(&self) -> &str {
        "cli"
    }

    async fn synthesize(&self, text: &str, _config: &TtsConfig) -> Result<Vec<u8>> {
        let temp_dir = tempfile::tempdir().context("Failed to create temp directory")?;
        let output_path = temp_dir.path().join("output.wav");

        let args = self.substitute_args(text, &output_path);

        debug!("Running CLI TTS: {} {}", self.command, args.join(" "));

        let output = Command::new(&self.command)
            .args(&args)
            .output()
            .context("Failed to execute CLI TTS command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("CLI TTS command failed: {}", stderr);
        }

        // Check if output file was created
        if output_path.exists() {
            return std::fs::read(&output_path).context("Failed to read CLI TTS output file");
        }

        // Otherwise return stdout as audio (some tools output to stdout)
        Ok(output.stdout)
    }
}

/// TTS registry that manages providers and handles fallback
pub struct TtsRegistry {
    providers: Vec<Arc<dyn TtsProvider>>,
    config: TtsConfig,
}

impl TtsRegistry {
    /// Create a new TTS registry with the given configuration
    pub fn new(config: TtsConfig) -> Self {
        Self {
            providers: Vec::new(),
            config,
        }
    }

    /// Add a provider to the registry
    pub fn add_provider(&mut self, provider: Arc<dyn TtsProvider>) {
        self.providers.push(provider);
    }

    /// Create registry from configuration
    pub fn from_config(config: &TtsConfig, env_vars: &HashMap<String, String>) -> Self {
        let mut registry = Self::new(config.clone());

        for provider_config in &config.providers {
            match provider_config.provider_type.as_str() {
                "openai" => {
                    let api_key = provider_config
                        .api_key
                        .clone()
                        .or_else(|| env_vars.get("OPENAI_API_KEY").cloned());

                    if let Some(key) = api_key {
                        registry.add_provider(Arc::new(OpenAiProvider::new(
                            key,
                            provider_config.voice.clone(),
                            None,
                        )));
                    } else {
                        debug!("Skipping OpenAI TTS provider: no API key configured");
                    }
                }
                "edge" => {
                    // Check if edge-tts is installed
                    if which::which("edge-tts").is_ok() {
                        registry.add_provider(Arc::new(EdgeTtsProvider::new(
                            provider_config.voice.clone(),
                        )));
                    } else {
                        debug!(
                            "Skipping Edge TTS provider: edge-tts not installed (pip install edge-tts)"
                        );
                    }
                }
                "cli" => {
                    if let Some(ref command) = provider_config.command {
                        registry.add_provider(Arc::new(CliProvider::new(
                            command.clone(),
                            provider_config.args.clone(),
                        )));
                    } else {
                        warn!("CLI TTS provider missing 'command' field");
                    }
                }
                other => {
                    warn!("Unknown TTS provider type: {}", other);
                }
            }
        }

        registry
    }

    /// Synthesize speech, trying providers in order
    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        if !self.config.enabled {
            bail!("TTS is disabled in configuration");
        }

        let text_chars = text.chars().count();
        if text_chars > self.config.max_text_length {
            bail!(
                "Text too long: {} chars (max: {})",
                text_chars,
                self.config.max_text_length
            );
        }

        if self.providers.is_empty() {
            bail!("No TTS providers configured");
        }

        let mut last_error = None;

        for provider in &self.providers {
            match provider.synthesize(text, &self.config).await {
                Ok(audio) => {
                    info!(
                        "Synthesized {} bytes of audio via {}",
                        audio.len(),
                        provider.id()
                    );
                    return Ok(audio);
                }
                Err(e) => {
                    warn!("TTS provider {} failed: {}", provider.id(), e);
                    last_error = Some(e);
                }
            }
        }

        bail!(
            "All TTS providers failed. Last error: {}",
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

/// Get default CLI TTS command for the current platform
pub fn default_cli_command() -> Option<(String, Vec<String>)> {
    #[cfg(target_os = "macos")]
    {
        Some((
            "say".to_string(),
            vec![
                "-o".to_string(),
                "{{output}}".to_string(),
                "{{text}}".to_string(),
            ],
        ))
    }

    #[cfg(target_os = "linux")]
    {
        // Try espeak first, then pico2wave
        if which::which("espeak").is_ok() {
            Some((
                "espeak".to_string(),
                vec![
                    "-w".to_string(),
                    "{{output}}".to_string(),
                    "{{text}}".to_string(),
                ],
            ))
        } else if which::which("pico2wave").is_ok() {
            Some((
                "pico2wave".to_string(),
                vec![
                    "-w".to_string(),
                    "{{output}}".to_string(),
                    "{{text}}".to_string(),
                ],
            ))
        } else {
            None
        }
    }

    #[cfg(target_os = "windows")]
    {
        // PowerShell can do TTS on Windows
        Some((
            "powershell".to_string(),
            vec![
                "-Command".to_string(),
                "Add-Type -AssemblyName System.Speech; $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Speak('{{text}}')".to_string(),
            ],
        ))
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tts_config_default() {
        let config = TtsConfig::default();
        assert!(config.enabled);
        assert_eq!(config.voice, "alloy");
        assert_eq!(config.format, "mp3");
        assert_eq!(config.max_text_length, 4096);
        assert!(config.providers.is_empty());
    }

    #[test]
    fn test_registry_no_providers() {
        let registry = TtsRegistry::new(TtsConfig::default());

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(registry.synthesize("test"));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No TTS providers"));
    }

    #[test]
    fn test_registry_text_too_long() {
        let config = TtsConfig {
            max_text_length: 10,
            ..Default::default()
        };
        let registry = TtsRegistry::new(config);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(registry.synthesize("this text is way too long for the limit"));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too long"));
    }

    #[test]
    fn test_registry_text_limit_counts_multibyte_characters() {
        let config = TtsConfig {
            max_text_length: 2,
            ..Default::default()
        };
        let registry = TtsRegistry::new(config);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(registry.synthesize("✅✅"));

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No TTS providers"));
    }

    #[test]
    fn test_registry_text_limit_rejects_multibyte_over_character_limit() {
        let config = TtsConfig {
            max_text_length: 2,
            ..Default::default()
        };
        let registry = TtsRegistry::new(config);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(registry.synthesize("✅✅✅"));

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("too long"));
        assert!(error.contains("3 chars (max: 2)"));
    }

    #[test]
    fn test_cli_provider_substitute_args() {
        let provider = CliProvider::new(
            "echo".to_string(),
            vec![
                "{{text}}".to_string(),
                ">".to_string(),
                "{{output}}".to_string(),
            ],
        );

        let args = provider.substitute_args("hello world", Path::new("/tmp/output.wav"));

        assert_eq!(args[0], "hello world");
        assert_eq!(args[1], ">");
        assert_eq!(args[2], "/tmp/output.wav");
    }

    #[test]
    fn test_registry_from_config() {
        let mut config = TtsConfig::default();
        config.providers.push(TtsProviderConfig {
            provider_type: "cli".to_string(),
            command: Some("echo".to_string()),
            args: vec!["test".to_string()],
            ..Default::default()
        });

        let env_vars = HashMap::new();
        let registry = TtsRegistry::from_config(&config, &env_vars);

        assert!(registry.has_providers());
        assert_eq!(registry.provider_ids(), vec!["cli"]);
    }
}
