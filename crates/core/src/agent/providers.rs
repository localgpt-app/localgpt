use anyhow::Result;
use async_trait::async_trait;
use futures::Stream;
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::pin::Pin;
#[cfg(feature = "claude-cli")]
use std::process::Stdio;
#[cfg(feature = "claude-cli")]
use std::sync::Mutex as StdMutex;
#[cfg(feature = "claude-cli")]
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, info};

use crate::config::Config;
use crate::paths::DEFAULT_CONFIG_DIR_STR;

/// Image attachment for multimodal messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageAttachment {
    /// Base64-encoded image data
    pub data: String,
    /// MIME type (e.g., "image/png", "image/jpeg")
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Optional image attachments (for multimodal messages)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageAttachment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// Token usage statistics from API response
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

pub struct LLMResponse {
    pub content: LLMResponseContent,
    pub usage: Option<Usage>,
}

pub enum LLMResponseContent {
    Text(String),
    ToolCalls {
        calls: Vec<ToolCall>,
        /// Reasoning or interleaved assistant text emitted alongside the tool calls
        text: Option<String>,
    },
}

impl LLMResponse {
    pub fn text(content: String) -> Self {
        Self {
            content: LLMResponseContent::Text(content),
            usage: None,
        }
    }

    pub fn text_with_usage(content: String, usage: Usage) -> Self {
        Self {
            content: LLMResponseContent::Text(content),
            usage: Some(usage),
        }
    }

    pub fn tool_calls(calls: Vec<ToolCall>) -> Self {
        Self {
            content: LLMResponseContent::ToolCalls { calls, text: None },
            usage: None,
        }
    }

    pub fn tool_calls_with_usage(calls: Vec<ToolCall>, usage: Usage) -> Self {
        Self {
            content: LLMResponseContent::ToolCalls { calls, text: None },
            usage: Some(usage),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StreamChunk {
    pub delta: String,
    pub done: bool,
    /// Tool calls accumulated during streaming (only set when done=true)
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Events emitted during streaming with tools
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Text content chunk
    Content(String),
    /// Tool call started
    ToolCallStart {
        name: String,
        id: String,
        arguments: String,
    },
    /// Tool call completed
    ToolCallEnd {
        name: String,
        id: String,
        output: String,
        warnings: Vec<String>,
    },
    /// Stream completed
    Done,
}

pub type StreamResult = Pin<Box<dyn Stream<Item = Result<StreamChunk>> + Send>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokenUpdate {
    pub provider: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
}

/// Common OAuth configuration for providers
pub struct OAuthConfig {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub expires_at: Option<u64>,
    pub base_url: String,
}

#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Get provider name
    fn name(&self) -> String;

    /// Whether the provider has refreshed its credentials and needs the caller
    /// to persist the updated token to disk.
    fn token_update(&self) -> Option<OAuthTokenUpdate> {
        None
    }

    async fn chat(&self, messages: &[Message], tools: Option<&[ToolSchema]>)
    -> Result<LLMResponse>;

    async fn summarize(&self, text: &str) -> Result<String>;

    /// Whether this provider supports native, server-side web search.
    fn supports_native_search(&self) -> bool {
        false
    }

    /// Provider-native tool definitions to include with regular tool schemas.
    fn native_tool_definitions(&self) -> Vec<Value> {
        Vec::new()
    }

    /// Reset provider session state (e.g., clear cached CLI session ID).
    /// Called when starting a new conversation via `/new`.
    /// Default: no-op (most providers are stateless).
    fn reset_session(&self) {}

    /// Stream chat response (default: falls back to non-streaming)
    async fn chat_stream(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<StreamResult> {
        // Default implementation: single chunk with full response
        let resp = self.chat(messages, tools).await?;
        match resp.content {
            LLMResponseContent::Text(text) => Ok(Box::pin(futures::stream::once(async move {
                Ok(StreamChunk {
                    delta: text,
                    done: true,
                    tool_calls: None,
                })
            }))),
            LLMResponseContent::ToolCalls { calls, text } => {
                let delta = text.unwrap_or_default();
                Ok(Box::pin(futures::stream::once(async move {
                    Ok(StreamChunk {
                        delta,
                        done: true,
                        tool_calls: Some(calls),
                    })
                })))
            }
        }
    }
}

/// Resolve model alias to provider/model format (OpenClaw-compatible)
fn resolve_model_alias(model: &str) -> String {
    // OpenClaw-compatible aliases
    match model.to_lowercase().as_str() {
        // Short aliases → latest 4.6 models
        "opus" => "anthropic/claude-opus-4-6".to_string(),
        "sonnet" => "anthropic/claude-sonnet-4-6".to_string(),
        "gpt" => "openai/gpt-4o".to_string(),
        "gpt-mini" => "openai/gpt-4o-mini".to_string(),
        "glm" => "glm/glm-4.7".to_string(),
        "grok" => "xai/grok-3-mini".to_string(),
        "codex" => "codex-cli/o4-mini".to_string(),
        _ => model.to_string(),
    }
}

/// Map OpenClaw model ID to actual API model ID
/// See: https://docs.anthropic.com/en/docs/about-claude/models
fn normalize_model_id(provider: &str, model_id: &str) -> String {
    match provider {
        "anthropic" => {
            match model_id.to_lowercase().as_str() {
                // Claude 4.6 models (latest)
                "claude-opus-4-6" | "opus" | "opus-4.6" => "claude-opus-4-6".to_string(),
                "claude-sonnet-4-6" | "sonnet" | "sonnet-4.6" => "claude-sonnet-4-6".to_string(),
                // Claude 4.5 models (still supported)
                "claude-opus-4-5" | "opus-4.5" => "claude-opus-4-5-20251101".to_string(),
                "claude-sonnet-4-5" | "sonnet-4.5" => "claude-sonnet-4-5-20250929".to_string(),
                "claude-haiku-4-5" | "haiku" | "haiku-4.5" => {
                    "claude-haiku-4-5-20251001".to_string()
                }
                // Default to Opus 4.6 (latest)
                _ => "claude-opus-4-6".to_string(),
            }
        }
        _ => model_id.to_string(),
    }
}

pub fn create_provider(model: &str, config: &Config) -> Result<Box<dyn LLMProvider>> {
    #[cfg(feature = "claude-cli")]
    let workspace = config.workspace_path();

    // Resolve aliases first (e.g., "opus" → "anthropic/claude-opus-4-5")
    let model = resolve_model_alias(model);

    // Parse provider/model format (OpenClaw-compatible)
    let (provider, model_id) = if let Some(pos) = model.find('/') {
        let (p, m) = model.split_at(pos);
        (p.to_lowercase(), m[1..].to_string()) // Skip the '/'
    } else if model.starts_with("gpt-") || model.starts_with("o1") {
        ("openai".to_string(), model.clone())
    } else if model.starts_with("claude-") {
        ("anthropic".to_string(), model.clone())
    } else if model.starts_with("glm-") {
        ("glm".to_string(), model.clone())
    } else if model.starts_with("grok-") {
        ("xai".to_string(), model.clone())
    } else if model.starts_with("gemini-") {
        ("gemini".to_string(), model.clone())
    } else {
        // Default to anthropic for unknown models, or ollama if configured
        if config.providers.ollama.is_some() {
            ("ollama".to_string(), model.clone())
        } else if config.providers.anthropic.is_some() {
            ("anthropic".to_string(), model.clone())
        } else {
            ("unknown".to_string(), model.clone())
        }
    };

    match provider.as_str() {
        "anthropic" => {
            // Prefer OAuth config if available
            if let Some(oauth_config) = &config.providers.anthropic_oauth {
                let full_model = normalize_model_id("anthropic", &model_id);
                Ok(Box::new(AnthropicOAuthProvider::new(
                    OAuthConfig {
                        access_token: oauth_config.access_token.clone(),
                        refresh_token: oauth_config.refresh_token.clone(),
                        client_id: oauth_config.client_id.clone(),
                        client_secret: oauth_config.client_secret.clone(),
                        expires_at: oauth_config.expires_at,
                        base_url: oauth_config.base_url.clone(),
                    },
                    &full_model,
                    config.agent.max_tokens,
                )?))
            } else {
                let anthropic_config = config.providers.anthropic.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Anthropic provider not configured.\n\
                        Set ANTHROPIC_API_KEY env var or add to {}/config.toml:\n\n\
                        [providers.anthropic]\n\
                        api_key = \"sk-ant-...\"\n\n\
                        Or use OAuth subscription credentials:\n\n\
                        [providers.anthropic_oauth]\n\
                        access_token = \"${{ANTHROPIC_OAUTH_TOKEN}}\"",
                        DEFAULT_CONFIG_DIR_STR
                    )
                })?;

                let full_model = normalize_model_id("anthropic", &model_id);
                Ok(Box::new(AnthropicProvider::new(
                    &anthropic_config.api_key,
                    &anthropic_config.base_url,
                    &full_model,
                    config.agent.max_tokens,
                )?))
            }
        }

        "openai" => {
            // Prefer OAuth config if available
            if let Some(oauth_config) = &config.providers.openai_oauth {
                Ok(Box::new(OpenAIOAuthProvider::new(
                    OAuthConfig {
                        access_token: oauth_config.access_token.clone(),
                        refresh_token: oauth_config.refresh_token.clone(),
                        client_id: oauth_config.client_id.clone(),
                        client_secret: oauth_config.client_secret.clone(),
                        expires_at: oauth_config.expires_at,
                        base_url: oauth_config.base_url.clone(),
                    },
                    &model_id,
                )?))
            } else {
                let openai_config = config.providers.openai.as_ref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "OpenAI provider not configured.\n\
                        Set OPENAI_API_KEY env var or add to {}/config.toml:\n\n\
                        [providers.openai]\n\
                        api_key = \"sk-...\"\n\n\
                        Or use OAuth credentials:\n\n\
                        [providers.openai_oauth]\n\
                        access_token = \"${{OPENAI_OAUTH_TOKEN}}\"",
                        DEFAULT_CONFIG_DIR_STR
                    )
                })?;

                Ok(Box::new(OpenAIProvider::new(
                    &openai_config.api_key,
                    &openai_config.base_url,
                    &model_id,
                )?))
            }
        }

        "xai" => {
            let xai_config = config.providers.xai.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "xAI provider not configured.\n\
                    Set XAI_API_KEY env var or add to {}/config.toml:\n\n\
                    [providers.xai]\n\
                    api_key = \"xai-...\"",
                    DEFAULT_CONFIG_DIR_STR
                )
            })?;

            Ok(Box::new(XaiProvider::new(
                &xai_config.api_key,
                &xai_config.base_url,
                &model_id,
            )?))
        }

        #[cfg(feature = "claude-cli")]
        "claude-cli" => {
            let cli_config = config.providers.claude_cli.as_ref();
            let command = cli_config.map(|c| c.command.as_str()).unwrap_or("claude");
            Ok(Box::new(ClaudeCliProvider::new(
                command, &model_id, workspace,
            )?))
        }
        #[cfg(not(feature = "claude-cli"))]
        "claude-cli" => {
            anyhow::bail!(
                "Claude CLI provider is not available in this build.\n\
                 The 'claude-cli' feature is required for subprocess-based providers."
            )
        }

        #[cfg(feature = "gemini-cli")]
        "gemini-cli" => {
            let cli_config = config.providers.gemini_cli.as_ref();
            let command = cli_config.map(|c| c.command.as_str()).unwrap_or("gemini");
            Ok(Box::new(GeminiCliProvider::new(
                command, &model_id, workspace,
            )?))
        }
        #[cfg(not(feature = "gemini-cli"))]
        "gemini-cli" => {
            anyhow::bail!(
                "Gemini CLI provider is not available in this build.\n\
                 The 'gemini-cli' feature is required for subprocess-based providers."
            )
        }

        #[cfg(feature = "codex-cli")]
        "codex-cli" => {
            let cli_config = config.providers.codex_cli.as_ref();
            let command = cli_config.map(|c| c.command.as_str()).unwrap_or("codex");
            Ok(Box::new(CodexCliProvider::new(
                command, &model_id, workspace,
            )?))
        }
        #[cfg(not(feature = "codex-cli"))]
        "codex-cli" => {
            anyhow::bail!(
                "Codex CLI provider is not available in this build.\n\
                 The 'codex-cli' feature is required for subprocess-based providers."
            )
        }

        "ollama" => {
            let ollama_config = config.providers.ollama.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Ollama provider not configured.\n\
                    Add to {}/config.toml:\n\n\
                    [providers.ollama]\n\
                    endpoint = \"http://localhost:11434\"",
                    DEFAULT_CONFIG_DIR_STR
                )
            })?;

            Ok(Box::new(OllamaProvider::new(
                &ollama_config.endpoint,
                &model_id,
            )?))
        }

        "glm" => {
            let glm_config = config.providers.glm.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "GLM provider not configured.\n\
                    Set GLM_API_KEY env var or add to {}/config.toml:\n\n\
                    [providers.glm]\n\
                    api_key = \"your-glm-api-key\"",
                    DEFAULT_CONFIG_DIR_STR
                )
            })?;

            Ok(Box::new(OpenAIProvider::new(
                &glm_config.api_key,
                &glm_config.base_url,
                &model_id,
            )?))
        }

        "gemini" => {
            let gemini_config = config.providers.gemini_oauth.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Gemini OAuth provider not configured.\n\
                    Add to {}/config.toml:\n\n\
                    [providers.gemini_oauth]\n\
                    access_token = \"${{GEMINI_OAUTH_TOKEN}}\"\n\
                    # Optional: refresh_token for automatic token renewal\n\
                    # Optional: project_id for enterprise/subscription plans",
                    DEFAULT_CONFIG_DIR_STR
                )
            })?;

            Ok(Box::new(GeminiOAuthProvider::new(
                OAuthConfig {
                    access_token: gemini_config.access_token.clone(),
                    refresh_token: gemini_config.refresh_token.clone(),
                    client_id: gemini_config.client_id.clone(),
                    client_secret: gemini_config.client_secret.clone(),
                    expires_at: gemini_config.expires_at,
                    base_url: gemini_config.base_url.clone(),
                },
                &model_id,
                gemini_config.project_id.as_deref(),
            )?))
        }

        "github" => {
            let github_config = config.providers.github_copilot.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "GitHub Copilot provider not configured.\n\
                    Run `localgpt auth github` to authenticate.",
                )
            })?;

            Ok(Box::new(GitHubCopilotProvider::new(
                &github_config.access_token,
                github_config.refresh_token.clone(),
                github_config.client_id.clone(),
                github_config.client_secret.clone(),
                github_config.expires_at,
                &model_id,
            )?))
        }

        "openai-compat" | "openai_compat" => {
            let compat_config = config.providers.openai_compatible.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "OpenAI-compatible provider not configured.\n\
                    Add to {}/config.toml:\n\n\
                    [providers.openai_compatible]\n\
                    base_url = \"https://openrouter.ai/api/v1\"\n\
                    api_key = \"${{YOUR_API_KEY}}\"\n\
                    # Optional extra headers:\n\
                    # extra_headers = {{ \"HTTP-Referer\" = \"https://localgpt.app\" }}",
                    DEFAULT_CONFIG_DIR_STR
                )
            })?;

            Ok(Box::new(OpenAICompatibleProvider::new(
                &compat_config.base_url,
                &compat_config.api_key,
                &model_id,
                compat_config.extra_headers.clone(),
            )?))
        }

        _ => {
            // Fallback: try Claude CLI if configured
            #[cfg(feature = "claude-cli")]
            if let Some(cli_config) = &config.providers.claude_cli {
                return Ok(Box::new(ClaudeCliProvider::new(
                    &cli_config.command,
                    &cli_config.model,
                    workspace,
                )?));
            }

            anyhow::bail!(
                "Unknown provider '{}' for model '{}'.\n\n\
                Supported formats (OpenClaw-compatible):\n  \
                - anthropic/claude-opus-4-5, anthropic/claude-sonnet-4-5\n  \
                - openai/gpt-4o, openai/gpt-4o-mini\n  \
                - xai/grok-3-mini\n  \
                - glm/glm-4.7\n  \
                - claude-cli/opus, claude-cli/sonnet\n  \
                - gemini-cli/gemini-3.1-pro-preview\n  \
                - ollama/llama3, ollama/mistral\n  \
                - openai-compat/<model> (OpenRouter, DeepSeek, Groq, etc.)\n\n\
                Or use aliases: opus, sonnet, haiku, gpt, gpt-mini, grok, glm",
                provider,
                model
            )
        }
    }
}

// OpenAI Provider
pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl OpenAIProvider {
    pub fn new(api_key: &str, base_url: &str, model: &str) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
        })
    }

    fn format_tools(&self, tools: &[ToolSchema]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect()
    }

    fn format_messages(&self, messages: &[Message]) -> Vec<Value> {
        messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };

                // Handle multimodal content for user messages with images
                let content: Value = if m.role == Role::User && !m.images.is_empty() {
                    let mut content_parts: Vec<Value> = Vec::new();

                    // Add images first (OpenAI uses data URLs)
                    for img in &m.images {
                        content_parts.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{};base64,{}", img.media_type, img.data)
                            }
                        }));
                    }

                    // Add text content
                    if !m.content.is_empty() {
                        content_parts.push(json!({
                            "type": "text",
                            "text": m.content
                        }));
                    }

                    json!(content_parts)
                } else {
                    json!(m.content)
                };

                let mut msg = json!({
                    "role": role,
                    "content": content
                });

                if let Some(ref tool_calls) = m.tool_calls {
                    msg["tool_calls"] = json!(
                        tool_calls
                            .iter()
                            .map(|tc| {
                                json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.arguments
                                    }
                                })
                            })
                            .collect::<Vec<_>>()
                    );
                }

                if let Some(ref tool_call_id) = m.tool_call_id {
                    msg["tool_call_id"] = json!(tool_call_id);
                }

                msg
            })
            .collect()
    }
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    fn name(&self) -> String {
        "openai".to_string()
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        let mut body = json!({
            "model": self.model,
            "messages": self.format_messages(messages)
        });

        if let Some(tools) = tools
            && !tools.is_empty()
        {
            body["tools"] = json!(self.format_tools(tools));
        }

        debug!("OpenAI request: {}", serde_json::to_string_pretty(&body)?);

        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let response_body: Value = response.json().await?;
        debug!(
            "OpenAI response: {}",
            serde_json::to_string_pretty(&response_body)?
        );

        // Check for errors
        if let Some(error) = response_body.get("error") {
            anyhow::bail!("OpenAI API error: {}", error);
        }

        let choice = response_body["choices"]
            .get(0)
            .ok_or_else(|| anyhow::anyhow!("No choices in response"))?;

        let message = &choice["message"];

        // Parse usage
        let usage = response_body.get("usage").map(|u| Usage {
            input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        });

        // Check for tool calls
        if let Some(tool_calls) = message.get("tool_calls")
            && let Some(calls) = tool_calls.as_array()
        {
            let parsed_calls: Vec<ToolCall> = calls
                .iter()
                .map(|tc| ToolCall {
                    id: tc["id"].as_str().unwrap_or("").to_string(),
                    name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                    arguments: tc["function"]["arguments"]
                        .as_str()
                        .unwrap_or("{}")
                        .to_string(),
                })
                .collect();

            if !parsed_calls.is_empty() {
                let text = message["content"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                return Ok(LLMResponse {
                    content: LLMResponseContent::ToolCalls {
                        calls: parsed_calls,
                        text,
                    },
                    usage,
                });
            }
        }

        let content = message["content"].as_str().unwrap_or("").to_string();

        Ok(LLMResponse {
            content: LLMResponseContent::Text(content),
            usage,
        })
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!(
                "Summarize the following conversation concisely, preserving key information and context:\n\n{}",
                text
            ),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];

        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response type"),
        }
    }
}

// OpenAI-Compatible Provider (OpenRouter, DeepSeek, Groq, vLLM, LiteLLM, Together AI, etc.)
pub struct OpenAICompatibleProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    extra_headers: std::collections::HashMap<String, String>,
}

impl OpenAICompatibleProvider {
    pub fn new(
        base_url: &str,
        api_key: &str,
        model: &str,
        extra_headers: std::collections::HashMap<String, String>,
    ) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            extra_headers,
        })
    }

    fn format_tools(&self, tools: &[ToolSchema]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect()
    }

    fn format_messages(&self, messages: &[Message]) -> Vec<Value> {
        messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };

                // Handle multimodal content for user messages with images
                let content: Value = if m.role == Role::User && !m.images.is_empty() {
                    let mut content_parts: Vec<Value> = Vec::new();

                    // Add images first (OpenAI uses data URLs)
                    for img in &m.images {
                        content_parts.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{};base64,{}", img.media_type, img.data)
                            }
                        }));
                    }

                    // Add text content
                    if !m.content.is_empty() {
                        content_parts.push(json!({
                            "type": "text",
                            "text": m.content
                        }));
                    }

                    json!(content_parts)
                } else {
                    json!(m.content)
                };

                let mut msg = json!({
                    "role": role,
                    "content": content
                });

                if let Some(ref tool_calls) = m.tool_calls {
                    msg["tool_calls"] = json!(
                        tool_calls
                            .iter()
                            .map(|tc| {
                                json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.arguments
                                    }
                                })
                            })
                            .collect::<Vec<_>>()
                    );
                }

                if let Some(ref tool_call_id) = m.tool_call_id {
                    msg["tool_call_id"] = json!(tool_call_id);
                }

                msg
            })
            .collect()
    }
}

#[async_trait]
impl LLMProvider for OpenAICompatibleProvider {
    fn name(&self) -> String {
        format!("openai_compatible({})", self.base_url)
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        let mut body = json!({
            "model": self.model,
            "messages": self.format_messages(messages)
        });

        if let Some(tools) = tools
            && !tools.is_empty()
        {
            body["tools"] = json!(self.format_tools(tools));
        }

        debug!(
            "OpenAI-Compatible request to {}: {}",
            self.base_url,
            serde_json::to_string_pretty(&body)?
        );

        let mut request = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json");

        // Add extra headers from config
        for (key, value) in &self.extra_headers {
            request = request.header(key, value);
        }

        let response = request.json(&body).send().await?;

        let response_body: Value = response.json().await?;
        debug!(
            "OpenAI-Compatible response: {}",
            serde_json::to_string_pretty(&response_body)?
        );

        // Check for errors
        if let Some(error) = response_body.get("error") {
            anyhow::bail!(
                "OpenAI-Compatible API error from {}: {}",
                self.base_url,
                error
            );
        }

        let choice = response_body["choices"]
            .get(0)
            .ok_or_else(|| anyhow::anyhow!("No choices in response from {}", self.base_url))?;

        let message = &choice["message"];

        // Parse usage
        let usage = response_body.get("usage").map(|u| Usage {
            input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        });

        // Check for tool calls
        if let Some(tool_calls) = message.get("tool_calls")
            && let Some(calls) = tool_calls.as_array()
        {
            let parsed_calls: Vec<ToolCall> = calls
                .iter()
                .map(|tc| ToolCall {
                    id: tc["id"].as_str().unwrap_or("").to_string(),
                    name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                    arguments: tc["function"]["arguments"]
                        .as_str()
                        .unwrap_or("{}")
                        .to_string(),
                })
                .collect();

            if !parsed_calls.is_empty() {
                let text = message["content"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                return Ok(LLMResponse {
                    content: LLMResponseContent::ToolCalls {
                        calls: parsed_calls,
                        text,
                    },
                    usage,
                });
            }
        }

        let content = message["content"].as_str().unwrap_or("").to_string();

        Ok(LLMResponse {
            content: LLMResponseContent::Text(content),
            usage,
        })
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!(
                "Summarize the following conversation concisely, preserving key information and context:\n\n{}",
                text
            ),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];

        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response type"),
        }
    }
}

// xAI Provider (Responses API + native web_search passthrough)
pub struct XaiProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl XaiProvider {
    pub fn new(api_key: &str, base_url: &str, model: &str) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
        })
    }

    fn format_tools(&self, tools: &[ToolSchema]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters
                })
            })
            .collect()
    }

    fn format_text_message(role: &str, content: &str) -> Value {
        json!({
            "role": role,
            "content": content
        })
    }

    fn format_message_with_images(role: &str, content: &str, images: &[ImageAttachment]) -> Value {
        let mut parts: Vec<Value> = Vec::new();

        if !content.is_empty() {
            parts.push(json!({
                "type": "input_text",
                "text": content
            }));
        }

        for image in images {
            parts.push(json!({
                "type": "input_image",
                "image_url": format!("data:{};base64,{}", image.media_type, image.data)
            }));
        }

        json!({
            "role": role,
            "content": parts
        })
    }

    fn format_input(&self, messages: &[Message]) -> Vec<Value> {
        let mut formatted = Vec::new();

        for message in messages {
            match message.role {
                Role::System | Role::User | Role::Assistant => {
                    let role = match message.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => unreachable!(),
                    };

                    if let Some(tool_calls) = message.tool_calls.as_ref()
                        && !tool_calls.is_empty()
                    {
                        if !message.content.is_empty() {
                            formatted.push(Self::format_text_message(role, &message.content));
                        }

                        for tool_call in tool_calls {
                            formatted.push(json!({
                                "type": "function_call",
                                "call_id": tool_call.id,
                                "name": tool_call.name,
                                "arguments": tool_call.arguments
                            }));
                        }
                    } else if message.images.is_empty() {
                        formatted.push(Self::format_text_message(role, &message.content));
                    } else {
                        formatted.push(Self::format_message_with_images(
                            role,
                            &message.content,
                            &message.images,
                        ));
                    }
                }
                Role::Tool => {
                    if let Some(tool_call_id) = message.tool_call_id.as_ref() {
                        formatted.push(json!({
                            "type": "function_call_output",
                            "call_id": tool_call_id,
                            "output": message.content
                        }));
                    }
                }
            }
        }

        formatted
    }

    fn parse_tool_calls(output: &[Value]) -> Vec<ToolCall> {
        output
            .iter()
            .filter(|item| item["type"] == "function_call")
            .map(|item| {
                let arguments = if let Some(args) = item["arguments"].as_str() {
                    args.to_string()
                } else {
                    serde_json::to_string(&item["arguments"]).unwrap_or_else(|_| "{}".to_string())
                };

                ToolCall {
                    id: item["call_id"]
                        .as_str()
                        .or_else(|| item["id"].as_str())
                        .unwrap_or("")
                        .to_string(),
                    name: item["name"].as_str().unwrap_or("").to_string(),
                    arguments,
                }
            })
            .filter(|call| !call.id.is_empty() && !call.name.is_empty())
            .collect()
    }

    fn parse_output_text(response_body: &Value) -> String {
        let mut text = String::new();

        if let Some(output) = response_body["output"].as_array() {
            for item in output {
                if item["type"] != "message" {
                    continue;
                }

                if let Some(content_parts) = item["content"].as_array() {
                    for part in content_parts {
                        let part_type = part["type"].as_str().unwrap_or("");
                        if (part_type == "output_text" || part_type == "text")
                            && let Some(chunk) = part["text"].as_str()
                        {
                            text.push_str(chunk);
                        }
                    }
                } else if let Some(content) = item["content"].as_str() {
                    text.push_str(content);
                }
            }
        }

        if text.is_empty()
            && let Some(top_level_text) = response_body["output_text"].as_str()
        {
            return top_level_text.to_string();
        }

        text
    }
}

#[async_trait]
impl LLMProvider for XaiProvider {
    fn name(&self) -> String {
        "xai".to_string()
    }

    fn supports_native_search(&self) -> bool {
        true
    }

    fn native_tool_definitions(&self) -> Vec<Value> {
        vec![json!({
            "type": "web_search"
        })]
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        let mut body = json!({
            "model": self.model,
            "input": self.format_input(messages)
        });

        let mut all_tools = Vec::new();
        if let Some(tool_schemas) = tools {
            let client_has_web_search = tool_schemas.iter().any(|t| t.name == "web_search");
            if !client_has_web_search {
                all_tools.extend(self.native_tool_definitions());
            }
            if !tool_schemas.is_empty() {
                all_tools.extend(self.format_tools(tool_schemas));
            }
        }
        if !all_tools.is_empty() {
            body["tools"] = json!(all_tools);
        }

        debug!("xAI request: {}", serde_json::to_string_pretty(&body)?);

        let response = self
            .client
            .post(format!("{}/responses", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let response_body: Value = response.json().await?;
        debug!(
            "xAI response: {}",
            serde_json::to_string_pretty(&response_body)?
        );

        // Check for errors
        if let Some(error) = response_body.get("error") {
            anyhow::bail!("xAI API error: {}", error);
        }

        let output = response_body["output"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let usage = response_body.get("usage").map(|u| Usage {
            input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
        });

        let parsed_calls = Self::parse_tool_calls(&output);
        if !parsed_calls.is_empty() {
            let text = {
                let t = Self::parse_output_text(&response_body);
                if t.is_empty() { None } else { Some(t) }
            };
            return Ok(LLMResponse {
                content: LLMResponseContent::ToolCalls {
                    calls: parsed_calls,
                    text,
                },
                usage,
            });
        }

        let content = Self::parse_output_text(&response_body);

        Ok(LLMResponse {
            content: LLMResponseContent::Text(content),
            usage,
        })
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!(
                "Summarize the following conversation concisely, preserving key information and context:\n\n{}",
                text
            ),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];

        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response type"),
        }
    }
}

// Anthropic Provider
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: usize,
}

impl AnthropicProvider {
    pub fn new(api_key: &str, base_url: &str, model: &str, max_tokens: usize) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            model: model.to_string(),
            max_tokens,
        })
    }

    fn format_tools(&self, tools: &[ToolSchema]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters
                })
            })
            .collect()
    }

    fn format_messages(&self, messages: &[Message]) -> (Option<String>, Vec<Value>) {
        let mut system_prompt = None;
        let mut formatted = Vec::new();

        for m in messages {
            match m.role {
                Role::System => {
                    system_prompt = Some(m.content.clone());
                }
                Role::User => {
                    // Handle multimodal content if images are present
                    if m.images.is_empty() {
                        formatted.push(json!({
                            "role": "user",
                            "content": m.content
                        }));
                    } else {
                        // Build content array with text and images
                        let mut content_parts: Vec<Value> = Vec::new();

                        // Add images first
                        for img in &m.images {
                            content_parts.push(json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": img.media_type,
                                    "data": img.data
                                }
                            }));
                        }

                        // Add text content
                        if !m.content.is_empty() {
                            content_parts.push(json!({
                                "type": "text",
                                "text": m.content
                            }));
                        }

                        formatted.push(json!({
                            "role": "user",
                            "content": content_parts
                        }));
                    }
                }
                Role::Assistant => {
                    if let Some(ref tool_calls) = m.tool_calls {
                        let tool_use: Vec<Value> = tool_calls.iter().map(|tc| {
                            json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": serde_json::from_str::<Value>(&tc.arguments).unwrap_or(json!({}))
                            })
                        }).collect();
                        formatted.push(json!({
                            "role": "assistant",
                            "content": tool_use
                        }));
                    } else {
                        formatted.push(json!({
                            "role": "assistant",
                            "content": m.content
                        }));
                    }
                }
                Role::Tool => {
                    if let Some(ref tool_call_id) = m.tool_call_id {
                        formatted.push(json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": tool_call_id,
                                "content": m.content
                            }]
                        }));
                    }
                }
            }
        }

        (system_prompt, formatted)
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    fn name(&self) -> String {
        "anthropic".to_string()
    }

    fn supports_native_search(&self) -> bool {
        true
    }

    fn native_tool_definitions(&self) -> Vec<Value> {
        vec![json!({
            "type": "web_search_20250305",
            "name": "web_search"
        })]
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        let (system_prompt, formatted_messages) = self.format_messages(messages);

        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": formatted_messages
        });

        if let Some(system) = system_prompt {
            body["system"] = json!(system);
        }

        let mut all_tools = Vec::new();
        if let Some(tool_schemas) = tools {
            let client_has_web_search = tool_schemas.iter().any(|t| t.name == "web_search");
            if !client_has_web_search {
                all_tools.extend(self.native_tool_definitions());
            }
            if !tool_schemas.is_empty() {
                all_tools.extend(self.format_tools(tool_schemas));
            }
        }
        if !all_tools.is_empty() {
            body["tools"] = json!(all_tools);
        }

        debug!(
            "Anthropic request: {}",
            serde_json::to_string_pretty(&body)?
        );

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let response_body: Value = response.json().await?;
        debug!(
            "Anthropic response: {}",
            serde_json::to_string_pretty(&response_body)?
        );

        // Check for errors
        if let Some(error) = response_body.get("error") {
            anyhow::bail!("Anthropic API error: {}", error);
        }

        let content = response_body["content"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("No content in response"))?;

        // Parse usage (Anthropic uses input_tokens/output_tokens directly)
        let usage = response_body.get("usage").map(|u| Usage {
            input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
        });

        // Check for tool use
        let tool_calls: Vec<ToolCall> = content
            .iter()
            .filter(|c| c["type"] == "tool_use")
            .map(|c| ToolCall {
                id: c["id"].as_str().unwrap_or("").to_string(),
                name: c["name"].as_str().unwrap_or("").to_string(),
                arguments: serde_json::to_string(&c["input"]).unwrap_or("{}".to_string()),
            })
            .collect();

        if !tool_calls.is_empty() {
            let text = {
                let t = content
                    .iter()
                    .filter(|c| c["type"] == "text")
                    .map(|c| c["text"].as_str().unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("");
                if t.is_empty() { None } else { Some(t) }
            };
            return Ok(LLMResponse {
                content: LLMResponseContent::ToolCalls {
                    calls: tool_calls,
                    text,
                },
                usage,
            });
        }

        // Get text content
        let text = content
            .iter()
            .filter(|c| c["type"] == "text")
            .map(|c| c["text"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("");

        Ok(LLMResponse {
            content: LLMResponseContent::Text(text),
            usage,
        })
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!(
                "Summarize the following conversation concisely, preserving key information and context:\n\n{}",
                text
            ),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];

        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response type"),
        }
    }

    async fn chat_stream(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<StreamResult> {
        let (system_prompt, formatted_messages) = self.format_messages(messages);

        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": formatted_messages,
            "stream": true
        });

        if let Some(system) = system_prompt {
            body["system"] = json!(system);
        }

        let mut all_tools = Vec::new();
        if let Some(tool_schemas) = tools {
            let client_has_web_search = tool_schemas.iter().any(|t| t.name == "web_search");
            if !client_has_web_search {
                all_tools.extend(self.native_tool_definitions());
            }
            if !tool_schemas.is_empty() {
                all_tools.extend(self.format_tools(tool_schemas));
            }
        }
        if !all_tools.is_empty() {
            body["tools"] = json!(all_tools);
        }

        debug!(
            "Anthropic streaming request: {}",
            serde_json::to_string_pretty(&body)?
        );

        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        // Check for error status
        if !response.status().is_success() {
            let error_body = response.text().await?;
            anyhow::bail!("Anthropic API error: {}", error_body);
        }

        // Anthropic streams Server-Sent Events (SSE)
        // We need to track tool_use blocks and accumulate their JSON input
        let stream = async_stream::stream! {
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();

            // Track tool calls being accumulated
            let mut pending_tool_calls: Vec<ToolCall> = Vec::new();
            let mut current_tool_id: Option<String> = None;
            let mut current_tool_name: Option<String> = None;
            let mut current_tool_input: String = String::new();

            while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));

                        // Process complete SSE events (lines starting with "data: ")
                        while let Some(pos) = buffer.find("\n\n") {
                            let event = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();

                            // Parse SSE event
                            for line in event.lines() {
                                if let Some(data) = line.strip_prefix("data: ") {
                                    if data == "[DONE]" {
                                        // Return any accumulated tool calls
                                        let tool_calls = if pending_tool_calls.is_empty() {
                                            None
                                        } else {
                                            Some(pending_tool_calls.clone())
                                        };
                                        yield Ok(StreamChunk {
                                            delta: String::new(),
                                            done: true,
                                            tool_calls,
                                        });
                                        continue;
                                    }

                                    if let Ok(json) = serde_json::from_str::<Value>(data) {
                                        let event_type = json["type"].as_str().unwrap_or("");

                                        match event_type {
                                            // Text content delta
                                            "content_block_delta" => {
                                                // Check if it's text or tool input
                                                if let Some(delta) = json["delta"]["text"].as_str() {
                                                    yield Ok(StreamChunk {
                                                        delta: delta.to_string(),
                                                        done: false,
                                                        tool_calls: None,
                                                    });
                                                } else if let Some(input_delta) = json["delta"]["partial_json"].as_str() {
                                                    // Accumulate tool input JSON
                                                    current_tool_input.push_str(input_delta);
                                                }
                                            }

                                            // Tool use block started
                                            "content_block_start" => {
                                                if let Some(content_block) = json.get("content_block")
                                                    && content_block["type"] == "tool_use" {
                                                        current_tool_id = content_block["id"].as_str().map(|s| s.to_string());
                                                        current_tool_name = content_block["name"].as_str().map(|s| s.to_string());
                                                        current_tool_input.clear();
                                                    }
                                            }

                                            // Content block finished
                                            "content_block_stop" => {
                                                // If we were accumulating a tool call, finalize it
                                                if let (Some(id), Some(name)) = (current_tool_id.take(), current_tool_name.take()) {
                                                    pending_tool_calls.push(ToolCall {
                                                        id,
                                                        name,
                                                        arguments: std::mem::take(&mut current_tool_input),
                                                    });
                                                }
                                            }

                                            // Message complete
                                            "message_stop" => {
                                                let tool_calls = if pending_tool_calls.is_empty() {
                                                    None
                                                } else {
                                                    Some(pending_tool_calls.clone())
                                                };
                                                yield Ok(StreamChunk {
                                                    delta: String::new(),
                                                    done: true,
                                                    tool_calls,
                                                });
                                            }

                                            // Error
                                            "error" => {
                                                let error_msg = json["error"]["message"]
                                                    .as_str()
                                                    .unwrap_or("Unknown error");
                                                yield Err(anyhow::anyhow!("Anthropic error: {}", error_msg));
                                            }

                                            _ => {} // Ignore other events
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        yield Err(anyhow::anyhow!("Stream error: {}", e));
                        break;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

// Ollama Provider (for local models)
pub struct OllamaProvider {
    client: Client,
    endpoint: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(endpoint: &str, model: &str) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            endpoint: endpoint.to_string(),
            model: model.to_string(),
        })
    }
}

#[async_trait]
impl LLMProvider for OllamaProvider {
    fn name(&self) -> String {
        "ollama".to_string()
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        let formatted_messages: Vec<Value> = messages
            .iter()
            .map(|m| {
                let mut msg = json!({
                    "role": match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    },
                    "content": m.content
                });
                // Include tool_call_id for tool role messages
                if m.role == Role::Tool
                    && let Some(ref id) = m.tool_call_id {
                        msg["tool_call_id"] = json!(id);
                    }
                // Include tool_calls for assistant messages that had them
                if m.role == Role::Assistant
                    && let Some(ref calls) = m.tool_calls {
                        let tc: Vec<Value> = calls.iter().map(|c| json!({
                            "function": {
                                "name": c.name,
                                "arguments": serde_json::from_str::<Value>(&c.arguments).unwrap_or(json!({}))
                            }
                        })).collect();
                        msg["tool_calls"] = json!(tc);
                    }
                msg
            })
            .collect();

        let mut body = json!({
            "model": self.model,
            "messages": formatted_messages,
            "stream": false
        });

        // Send tool schemas if provided
        if let Some(tool_schemas) = tools
            && !tool_schemas.is_empty()
        {
            let tools_json: Vec<Value> = tool_schemas
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters
                        }
                    })
                })
                .collect();
            body["tools"] = json!(tools_json);
        }

        debug!("Ollama request: {}", serde_json::to_string_pretty(&body)?);

        let response = self
            .client
            .post(format!("{}/api/chat", self.endpoint))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        // If Ollama returns 400 (model doesn't support tools), retry without tools
        if response.status() == reqwest::StatusCode::BAD_REQUEST && body.get("tools").is_some() {
            debug!("Ollama returned 400 with tools, retrying without tools");
            let mut body_no_tools = body.clone();
            body_no_tools.as_object_mut().map(|o| o.remove("tools"));
            let retry_response = self
                .client
                .post(format!("{}/api/chat", self.endpoint))
                .header("Content-Type", "application/json")
                .json(&body_no_tools)
                .send()
                .await?;
            let response_body: Value = retry_response.json().await?;
            let content = response_body["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let usage = if response_body.get("prompt_eval_count").is_some() {
                Some(Usage {
                    input_tokens: response_body["prompt_eval_count"].as_u64().unwrap_or(0),
                    output_tokens: response_body["eval_count"].as_u64().unwrap_or(0),
                })
            } else {
                None
            };
            return Ok(LLMResponse {
                content: LLMResponseContent::Text(content),
                usage,
            });
        }

        let response_body: Value = response.json().await?;
        debug!(
            "Ollama response: {}",
            serde_json::to_string_pretty(&response_body)?
        );

        // Ollama returns token counts in prompt_eval_count and eval_count
        let usage = if response_body.get("prompt_eval_count").is_some() {
            Some(Usage {
                input_tokens: response_body["prompt_eval_count"].as_u64().unwrap_or(0),
                output_tokens: response_body["eval_count"].as_u64().unwrap_or(0),
            })
        } else {
            None
        };

        // Check for tool calls in response
        if let Some(tool_calls) = response_body["message"]["tool_calls"].as_array()
            && !tool_calls.is_empty()
        {
            let calls: Vec<ToolCall> = tool_calls
                .iter()
                .enumerate()
                .filter_map(|(i, tc)| {
                    let name = tc["function"]["name"].as_str()?.to_string();
                    let arguments = if tc["function"]["arguments"].is_object() {
                        serde_json::to_string(&tc["function"]["arguments"]).ok()?
                    } else {
                        tc["function"]["arguments"].as_str()?.to_string()
                    };
                    Some(ToolCall {
                        id: format!("call_{}", i),
                        name,
                        arguments,
                    })
                })
                .collect();

            if !calls.is_empty() {
                let text = response_body["message"]["content"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                return Ok(LLMResponse {
                    content: LLMResponseContent::ToolCalls {
                        calls,
                        text,
                    },
                    usage,
                });
            }
        }

        let content = response_body["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        Ok(LLMResponse {
            content: LLMResponseContent::Text(content),
            usage,
        })
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!(
                "Summarize the following conversation concisely, preserving key information and context:\n\n{}",
                text
            ),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];

        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response type"),
        }
    }

    async fn chat_stream(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<StreamResult> {
        // For tool-enabled requests, use non-streaming to properly handle tool calls
        if tools.is_some() && tools.map(|t| !t.is_empty()).unwrap_or(false) {
            let resp = self.chat(messages, tools).await?;
            return match resp.content {
                LLMResponseContent::Text(text) => Ok(Box::pin(futures::stream::once(async move {
                    Ok(StreamChunk {
                        delta: text,
                        done: true,
                        tool_calls: None,
                    })
                }))),
                LLMResponseContent::ToolCalls { calls, text } => {
                    let delta = text.unwrap_or_default();
                    Ok(Box::pin(futures::stream::once(async move {
                        Ok(StreamChunk {
                            delta,
                            done: true,
                            tool_calls: Some(calls),
                        })
                    })))
                }
            };
        }

        let formatted_messages: Vec<Value> = messages
            .iter()
            .map(|m| {
                json!({
                    "role": match m.role {
                        Role::System => "system",
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        Role::Tool => "tool",
                    },
                    "content": m.content
                })
            })
            .collect();

        let body = json!({
            "model": self.model,
            "messages": formatted_messages,
            "stream": true
        });

        debug!(
            "Ollama streaming request: {}",
            serde_json::to_string_pretty(&body)?
        );

        let response = self
            .client
            .post(format!("{}/api/chat", self.endpoint))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        // Ollama streams newline-delimited JSON
        let stream = async_stream::stream! {
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk) = byte_stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        buffer.push_str(&String::from_utf8_lossy(&bytes));

                        // Process complete lines
                        while let Some(pos) = buffer.find('\n') {
                            let line = buffer[..pos].to_string();
                            buffer = buffer[pos + 1..].to_string();

                            if line.is_empty() {
                                continue;
                            }

                            if let Ok(json) = serde_json::from_str::<Value>(&line) {
                                let content = json["message"]["content"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string();
                                let done = json["done"].as_bool().unwrap_or(false);

                                yield Ok(StreamChunk {
                                    delta: content,
                                    done,
                                    tool_calls: None,
                                });
                            }
                        }
                    }
                    Err(e) => {
                        yield Err(anyhow::anyhow!("Stream error: {}", e));
                        break;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

#[cfg(feature = "claude-cli")]
/// Claude CLI Provider - invokes the `claude` CLI command
/// No tool support (text in → text out only)
/// No streaming (CLI output is collected then returned)
pub struct ClaudeCliProvider {
    command: String,
    model: String,
    /// Working directory for CLI execution
    workspace: std::path::PathBuf,
    /// Session key for the session store (e.g., "main")
    session_key: String,
    /// LocalGPT session ID (for session store tracking)
    localgpt_session_id: String,
    /// CLI session ID for multi-turn conversations (interior mutability for &self methods)
    cli_session_id: StdMutex<Option<String>>,
}

#[cfg(feature = "claude-cli")]
/// Provider name for CLI session storage
const CLAUDE_CLI_PROVIDER: &str = "claude-cli";

#[cfg(feature = "claude-cli")]
impl ClaudeCliProvider {
    pub fn new(command: &str, model: &str, workspace: std::path::PathBuf) -> Result<Self> {
        // Load existing CLI session from session store
        let session_key = "main".to_string();
        let existing_session = load_cli_session_from_store(&session_key, CLAUDE_CLI_PROVIDER);

        if let Some(ref sid) = existing_session {
            debug!("Loaded existing Claude CLI session: {}", sid);
        }

        Ok(Self {
            command: command.to_string(),
            model: normalize_claude_model(model),
            workspace,
            session_key,
            localgpt_session_id: uuid::Uuid::new_v4().to_string(),
            cli_session_id: StdMutex::new(existing_session),
        })
    }

    /// Execute Claude CLI command, retrying with a new session if the existing one is not found
    async fn execute_cli_command(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        existing_session: Option<&str>,
    ) -> Result<(std::process::Output, bool)> {
        use std::process::Command;

        // First attempt: try with existing session if available
        if let Some(cli_sid) = existing_session {
            let args = self.build_cli_args(prompt, system_prompt, Some(cli_sid), false);

            debug!(
                "Claude CLI (resume): {} {:?} (cwd: {:?})",
                self.command, args, self.workspace
            );

            let output = tokio::task::spawn_blocking({
                let command = self.command.clone();
                let args = args.clone();
                let workspace = self.workspace.clone();
                move || {
                    Command::new(&command)
                        .args(&args)
                        .current_dir(&workspace)
                        .output()
                }
            })
            .await??;

            if output.status.success() {
                return Ok((output, false));
            }

            // Check if the error is "session not found" - if so, retry with new session
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No conversation found")
                || stderr.contains("session")
                    && (stderr.contains("not found") || stderr.contains("does not exist"))
            {
                info!(
                    "Claude CLI session {} not found, creating new session",
                    cli_sid
                );
                // Clear the invalid session from our state
                if let Ok(mut cli_session) = self.cli_session_id.lock() {
                    *cli_session = None;
                }
            } else {
                // Some other error - propagate it
                anyhow::bail!("Claude CLI failed: {}", stderr);
            }
        }

        // Create new session
        let args = self.build_cli_args(prompt, system_prompt, None, true);

        debug!(
            "Claude CLI (new): {} {:?} (cwd: {:?})",
            self.command, args, self.workspace
        );

        let output = tokio::task::spawn_blocking({
            let command = self.command.clone();
            let args = args.clone();
            let workspace = self.workspace.clone();
            move || {
                Command::new(&command)
                    .args(&args)
                    .current_dir(&workspace)
                    .output()
            }
        })
        .await??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Claude CLI failed: {}", stderr);
        }

        Ok((output, true))
    }

    /// Build CLI arguments for a command
    fn build_cli_args(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        resume_session: Option<&str>,
        is_new_session: bool,
    ) -> Vec<String> {
        self.build_cli_args_with_format(
            prompt,
            system_prompt,
            resume_session,
            is_new_session,
            "json",
        )
    }

    /// Build CLI arguments with a specific output format
    fn build_cli_args_with_format(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        resume_session: Option<&str>,
        is_new_session: bool,
        output_format: &str,
    ) -> Vec<String> {
        let mut args = vec![
            "-p".to_string(),
            "--output-format".to_string(),
            output_format.to_string(),
            "--dangerously-skip-permissions".to_string(),
        ];

        // Claude CLI requires --verbose when using stream-json with --print
        // Also include partial messages for better visibility into internal process
        if output_format == "stream-json" {
            args.push("--verbose".to_string());
            args.push("--include-partial-messages".to_string());
        }

        // Model (only on new sessions)
        if is_new_session {
            args.push("--model".to_string());
            args.push(self.model.clone());
        }

        // System prompt (new sessions only)
        // Use --system-prompt to SET the prompt (not --append-system-prompt which appends)
        if is_new_session && let Some(sys) = system_prompt {
            args.push("--system-prompt".to_string());
            args.push(sys.to_string());
        }

        // CLI session handling
        if let Some(cli_sid) = resume_session {
            args.push("--resume".to_string());
            args.push(cli_sid.to_string());
        } else {
            // New CLI session - generate UUID
            let new_cli_session = uuid::Uuid::new_v4().to_string();
            args.push("--session-id".to_string());
            args.push(new_cli_session);
        }

        // Add prompt as final argument
        args.push(prompt.to_string());

        args
    }
}

#[cfg(feature = "claude-cli")]
/// Load CLI session ID from session store
fn load_cli_session_from_store(session_key: &str, provider: &str) -> Option<String> {
    use super::session_store::SessionStore;

    let store = SessionStore::load().ok()?;
    store.get_cli_session_id(session_key, provider)
}

#[cfg(feature = "claude-cli")]
/// Save CLI session ID to session store
fn save_cli_session_to_store(
    session_key: &str,
    session_id: &str,
    provider: &str,
    cli_session_id: &str,
) -> Result<()> {
    use super::session_store::SessionStore;

    let mut store = SessionStore::load()?;
    store.set_cli_session_id(session_key, session_id, provider, cli_session_id)?;
    Ok(())
}

#[cfg(feature = "claude-cli")]
fn normalize_claude_model(model: &str) -> String {
    match model.to_lowercase().as_str() {
        "opus" | "opus-4.6" | "opus-4.5" | "opus-4" | "claude-opus-4-6" | "claude-opus-4-5" => {
            "opus"
        }
        "sonnet" | "sonnet-4.6" | "sonnet-4.5" | "sonnet-4.1" | "claude-sonnet-4-6"
        | "claude-sonnet-4-5" => "sonnet",
        "haiku" | "haiku-4.5" | "haiku-3.5" | "claude-haiku-4-5" | "claude-haiku-3-5" => "haiku",
        other => other,
    }
    .to_string()
}

#[cfg(feature = "claude-cli")]
fn build_prompt_from_messages(messages: &[Message]) -> String {
    // Get the last user message as the prompt.
    // The security block is now concatenated into it by messages_for_api_call().
    messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

#[cfg(feature = "claude-cli")]
fn extract_system_prompt(messages: &[Message]) -> Option<String> {
    // The security block is now concatenated into the last user/tool message
    // by messages_for_api_call(), so no need to fold it here.
    messages
        .iter()
        .find(|m| m.role == Role::System)
        .map(|m| m.content.clone())
}

#[cfg(feature = "claude-cli")]
/// Parse Claude CLI JSON output, returning (response_text, session_id)
fn parse_claude_cli_output(stdout: &str) -> Result<(String, Option<String>)> {
    // Claude CLI outputs JSON with message content and session info
    if let Ok(json) = serde_json::from_str::<Value>(stdout) {
        // Extract response text (try multiple field names)
        let text = json
            .get("result")
            .or_else(|| json.get("message"))
            .or_else(|| json.get("content"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| stdout.trim().to_string());

        // Extract session ID (try multiple field names per OpenClaw pattern)
        let session_id = json
            .get("session_id")
            .or_else(|| json.get("sessionId"))
            .or_else(|| json.get("conversation_id"))
            .or_else(|| json.get("conversationId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        return Ok((text, session_id));
    }

    // Fallback: return raw output, no session
    Ok((stdout.trim().to_string(), None))
}

#[cfg(feature = "claude-cli")]
#[async_trait]
impl LLMProvider for ClaudeCliProvider {
    fn name(&self) -> String {
        "claude-cli".to_string()
    }

    fn reset_session(&self) {
        if let Ok(mut cli_session) = self.cli_session_id.lock() {
            *cli_session = None;
        }
        // Clear from session store on disk
        if let Ok(mut store) = super::session_store::SessionStore::load() {
            let _ = store.update(&self.session_key, &self.localgpt_session_id, |entry| {
                entry.clear_cli_session_ids();
            });
        }
        info!("Claude CLI session reset (next call will start fresh)");
    }

    async fn chat(
        &self,
        messages: &[Message],
        _tools: Option<&[ToolSchema]>, // Ignored - no tool support
    ) -> Result<LLMResponse> {
        // Build prompt from messages (last user message)
        let prompt = build_prompt_from_messages(messages);
        let system_prompt = extract_system_prompt(messages);

        // Get current CLI session state
        let current_cli_session = self
            .cli_session_id
            .lock()
            .map_err(|e| anyhow::anyhow!("Session lock poisoned: {}", e))?
            .clone();

        // Try to execute with current session, fall back to new session if not found
        let (output, used_new_session) = self
            .execute_cli_command(
                &prompt,
                system_prompt.as_deref(),
                current_cli_session.as_deref(),
            )
            .await?;

        // Parse JSON output and extract session ID
        let stdout = String::from_utf8_lossy(&output.stdout);
        let (response, new_session_id) = parse_claude_cli_output(&stdout)?;

        // Update CLI session ID for next turn and persist to session store
        if let Some(ref new_cli_sid) = new_session_id {
            let mut cli_session = self
                .cli_session_id
                .lock()
                .map_err(|e| anyhow::anyhow!("Session lock poisoned: {}", e))?;
            *cli_session = Some(new_cli_sid.clone());

            // Persist to session store for cross-restart continuity
            if let Err(e) = save_cli_session_to_store(
                &self.session_key,
                &self.localgpt_session_id,
                CLAUDE_CLI_PROVIDER,
                new_cli_sid,
            ) {
                debug!("Failed to persist CLI session: {}", e);
            }

            if used_new_session {
                info!("Created new Claude CLI session: {}", new_cli_sid);
            }
        }

        // Claude CLI doesn't report token usage
        Ok(LLMResponse::text(response))
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!(
                "Summarize the following conversation concisely:\n\n{}",
                text
            ),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];

        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response type"),
        }
    }

    async fn chat_stream(
        &self,
        messages: &[Message],
        _tools: Option<&[ToolSchema]>,
    ) -> Result<StreamResult> {
        // Build prompt from messages (last user message)
        let prompt = build_prompt_from_messages(messages);
        let system_prompt = extract_system_prompt(messages);

        // Get current CLI session state
        let current_cli_session = self
            .cli_session_id
            .lock()
            .map_err(|e| anyhow::anyhow!("Session lock poisoned: {}", e))?
            .clone();

        // Determine if we're resuming or starting new
        let (resume_session, is_new_session) = if let Some(ref sid) = current_cli_session {
            (Some(sid.clone()), false)
        } else {
            (None, true)
        };

        // Build args with stream-json format
        let args = self.build_cli_args_with_format(
            &prompt,
            system_prompt.as_deref(),
            resume_session.as_deref(),
            is_new_session,
            "stream-json",
        );

        debug!(
            "Claude CLI streaming: {} {:?} (cwd: {:?})",
            self.command, args, self.workspace
        );

        // Spawn the CLI process with piped stdout
        let mut child = tokio::process::Command::new(&self.command)
            .args(&args)
            .current_dir(&self.workspace)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn Claude CLI: {}", e))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture stdout"))?;

        // Clone session state for the stream closure
        let cli_session_id = self.cli_session_id.lock().ok().and_then(|g| g.clone());
        let session_key = self.session_key.clone();
        let localgpt_session_id = self.localgpt_session_id.clone();
        let cli_session_mutex = std::sync::Arc::new(StdMutex::new(cli_session_id));

        // Create the stream
        let stream = async_stream::stream! {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            let mut accumulated_text = String::new();
            let mut session_id_captured: Option<String> = None;
            let mut last_text_len = 0;
            let mut shown_tool_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut pending_tools: std::collections::HashMap<String, String> = std::collections::HashMap::new();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.is_empty() {
                    continue;
                }

                // Parse the JSON line
                if let Ok(json) = serde_json::from_str::<Value>(&line) {
                    let event_type = json["type"].as_str().unwrap_or("");

                    match event_type {
                        // System init - show model info
                        "system" => {
                            if json.get("subtype").and_then(|v| v.as_str()) == Some("init")
                                && let Some(model) = json.get("model").and_then(|v| v.as_str()) {
                                    let tools_count = json.get("tools")
                                        .and_then(|v| v.as_array())
                                        .map(|a| a.len())
                                        .unwrap_or(0);
                                    yield Ok(StreamChunk {
                                        delta: format!("[Model: {} | Tools: {}]\n", model, tools_count),
                                        done: false,
                                        tool_calls: None,
                                    });
                                }
                        }

                        // Assistant message with content (streaming updates)
                        "assistant" => {
                            // Extract text and tool_use from message.content array
                            if let Some(content_array) = json["message"]["content"].as_array() {
                                for block in content_array {
                                    if block["type"] == "text" {
                                        if let Some(text) = block["text"].as_str() {
                                            accumulated_text = text.to_string();
                                        }
                                    } else if block["type"] == "tool_use" {
                                        // Show tool call as it happens
                                        let tool_id = block["id"].as_str().unwrap_or("").to_string();
                                        let tool_name = block["name"].as_str().unwrap_or("unknown");

                                        // Only show each tool call once
                                        if !shown_tool_ids.contains(&tool_id) {
                                            shown_tool_ids.insert(tool_id.clone());
                                            pending_tools.insert(tool_id, tool_name.to_string());

                                            // Format tool details
                                            let detail = if let Some(input) = block.get("input") {
                                                match tool_name {
                                                    "Bash" => input.get("command")
                                                        .and_then(|v| v.as_str())
                                                        .map(|s| if s.len() > 60 { format!("{}...", &s[..s.floor_char_boundary(57)]) } else { s.to_string() }),
                                                    "Read" | "Edit" | "Write" => input.get("file_path")
                                                        .or_else(|| input.get("path"))
                                                        .and_then(|v| v.as_str())
                                                        .map(|s| s.to_string()),
                                                    "Grep" | "Glob" => input.get("pattern")
                                                        .and_then(|v| v.as_str())
                                                        .map(|s| format!("\"{}\"", s)),
                                                    "WebFetch" => input.get("url")
                                                        .and_then(|v| v.as_str())
                                                        .map(|s| s.to_string()),
                                                    "Task" => input.get("description")
                                                        .and_then(|v| v.as_str())
                                                        .map(|s| s.to_string()),
                                                    _ => None,
                                                }
                                            } else {
                                                None
                                            };

                                            let tool_msg = if let Some(d) = detail {
                                                format!("\n[{}: {}]", tool_name, d)
                                            } else {
                                                format!("\n[{}]", tool_name)
                                            };

                                            yield Ok(StreamChunk {
                                                delta: tool_msg,
                                                done: false,
                                                tool_calls: None,
                                            });
                                        }
                                    }
                                }
                            }

                            // Calculate delta (new text since last update)
                            if accumulated_text.len() > last_text_len {
                                let delta = accumulated_text[last_text_len..].to_string();
                                last_text_len = accumulated_text.len();
                                yield Ok(StreamChunk {
                                    delta,
                                    done: false,
                                    tool_calls: None,
                                });
                            }
                        }

                        // Tool result - show completion
                        "user" => {
                            if let Some(content_array) = json["message"]["content"].as_array() {
                                for block in content_array {
                                    if block["type"] == "tool_result" {
                                        let tool_id = block["tool_use_id"].as_str().unwrap_or("");
                                        let is_error = block.get("is_error")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false);

                                        // Get tool name from pending_tools
                                        let _tool_name = pending_tools.remove(tool_id);

                                        let status = if is_error { "failed" } else { "done" };
                                        yield Ok(StreamChunk {
                                            delta: format!(" [{}]\n", status),
                                            done: false,
                                            tool_calls: None,
                                        });
                                    }
                                }
                            }
                        }

                        // Result event contains session_id and final result
                        "result" => {
                            // Capture session_id for resume
                            if let Some(sid) = json.get("session_id").and_then(|v| v.as_str()) {
                                session_id_captured = Some(sid.to_string());

                                // Update the session state
                                if let Ok(mut guard) = cli_session_mutex.lock() {
                                    *guard = Some(sid.to_string());
                                }

                                // Persist to session store
                                if let Err(e) = save_cli_session_to_store(
                                    &session_key,
                                    &localgpt_session_id,
                                    CLAUDE_CLI_PROVIDER,
                                    sid,
                                ) {
                                    debug!("Failed to persist CLI session: {}", e);
                                }

                                debug!("Claude CLI session captured: {}", sid);
                            }

                            // Get final result text (may have more content than accumulated)
                            if let Some(result_text) = json.get("result").and_then(|v| v.as_str()) {
                                // Emit any remaining text not yet sent
                                if result_text.len() > last_text_len {
                                    let delta = result_text[last_text_len..].to_string();
                                    if !delta.is_empty() {
                                        yield Ok(StreamChunk {
                                            delta,
                                            done: false,
                                            tool_calls: None,
                                        });
                                    }
                                }
                            }

                            // Signal completion
                            yield Ok(StreamChunk {
                                delta: String::new(),
                                done: true,
                                tool_calls: None,
                            });
                        }

                        // Handle errors
                        "error" => {
                            let error_msg = json.get("error")
                                .and_then(|e| e.get("message"))
                                .and_then(|m| m.as_str())
                                .unwrap_or("Unknown CLI error");
                            yield Err(anyhow::anyhow!("Claude CLI error: {}", error_msg));
                        }

                        _ => {
                            // Ignore other event types (e.g., "system", "tool_use", etc.)
                            debug!("Ignoring CLI stream event type: {}", event_type);
                        }
                    }
                }
            }

            // Wait for the process to complete
            match child.wait().await {
                Ok(status) if !status.success() => {
                    // Try to read stderr for error details
                    if let Some(mut stderr) = child.stderr.take() {
                        let mut error_buf = String::new();
                        use tokio::io::AsyncReadExt;
                        let _ = stderr.read_to_string(&mut error_buf).await;
                        if !error_buf.is_empty() {
                            yield Err(anyhow::anyhow!("Claude CLI failed: {}", error_buf));
                        }
                    }
                }
                Err(e) => {
                    yield Err(anyhow::anyhow!("Failed to wait for CLI process: {}", e));
                }
                _ => {}
            }

            // Update the provider's session state if we captured a new session ID
            if let Some(ref new_sid) = session_id_captured {
                info!("Claude CLI streaming session: {}", new_sid);
            }
        };

        Ok(Box::pin(stream))
    }
}

#[cfg(feature = "gemini-cli")]
/// Gemini CLI Provider - invokes the `gemini` CLI command
/// No tool support (text in → text out only)
/// Streaming is emulated via non-streaming call for now
pub struct GeminiCliProvider {
    command: String,
    model: String,
    /// Working directory for CLI execution
    workspace: std::path::PathBuf,
    /// Session key for the session store (e.g., "main")
    session_key: String,
    /// LocalGPT session ID (for session store tracking)
    localgpt_session_id: String,
    /// CLI session ID for multi-turn conversations (interior mutability for &self methods)
    cli_session_id: StdMutex<Option<String>>,
}

#[cfg(feature = "gemini-cli")]
/// Provider name for CLI session storage
const GEMINI_CLI_PROVIDER: &str = "gemini-cli";

#[cfg(feature = "gemini-cli")]
impl GeminiCliProvider {
    pub fn new(command: &str, model: &str, workspace: std::path::PathBuf) -> Result<Self> {
        // Load existing CLI session from session store
        let session_key = "main".to_string();
        let existing_session = load_cli_session_from_store(&session_key, GEMINI_CLI_PROVIDER);

        if let Some(ref sid) = existing_session {
            debug!("Loaded existing Gemini CLI session: {}", sid);
        }

        Ok(Self {
            command: command.to_string(),
            model: model.to_string(),
            workspace,
            session_key,
            localgpt_session_id: uuid::Uuid::new_v4().to_string(),
            cli_session_id: StdMutex::new(existing_session),
        })
    }

    /// Execute Gemini CLI command
    async fn execute_cli_command(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        existing_session: Option<&str>,
    ) -> Result<(std::process::Output, bool)> {
        use std::process::Command;

        // First attempt: try with existing session if available
        if let Some(cli_sid) = existing_session {
            let args = self.build_cli_args(prompt, system_prompt, Some(cli_sid), false);

            debug!(
                "Gemini CLI (resume): {} {:?} (cwd: {:?})",
                self.command, args, self.workspace
            );

            let output = tokio::task::spawn_blocking({
                let command = self.command.clone();
                let args = args.clone();
                let workspace = self.workspace.clone();
                move || {
                    Command::new(&command)
                        .args(&args)
                        .current_dir(&workspace)
                        .output()
                }
            })
            .await??;

            if output.status.success() {
                return Ok((output, false));
            }

            // Check if the error is related to session not found
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.to_lowercase().contains("session")
                && (stderr.to_lowercase().contains("found")
                    || stderr.to_lowercase().contains("exist"))
            {
                info!(
                    "Gemini CLI session {} not found, creating new session",
                    cli_sid
                );
                // Clear the invalid session from our state
                if let Ok(mut cli_session) = self.cli_session_id.lock() {
                    *cli_session = None;
                }
            } else {
                // Some other error - propagate it
                anyhow::bail!("Gemini CLI failed: {}", stderr);
            }
        }

        // Create new session
        let args = self.build_cli_args(prompt, system_prompt, None, true);

        debug!(
            "Gemini CLI (new): {} {:?} (cwd: {:?})",
            self.command, args, self.workspace
        );

        let output = tokio::task::spawn_blocking({
            let command = self.command.clone();
            let args = args.clone();
            let workspace = self.workspace.clone();
            move || {
                Command::new(&command)
                    .args(&args)
                    .current_dir(&workspace)
                    .output()
            }
        })
        .await??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Gemini CLI failed: {}", stderr);
        }

        Ok((output, true))
    }

    /// Build CLI arguments for a command
    fn build_cli_args(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        resume_session: Option<&str>,
        is_new_session: bool,
    ) -> Vec<String> {
        let mut args = vec![
            "-p".to_string(),
            // Prepend system prompt to user prompt if new session (Gemini CLI lacks --system-prompt)
            if is_new_session && let Some(sys) = system_prompt {
                format!("System Instruction: {}\n\nUser Request: {}", sys, prompt)
            } else {
                prompt.to_string()
            },
            "--output-format".to_string(),
            "json".to_string(),   // Always use json for now
            "--yolo".to_string(), // Auto-accept actions (skip permissions)
        ];

        // Model (only needed on new sessions, but good to be explicit)
        if is_new_session {
            args.push("--model".to_string());
            args.push(self.model.clone());
        }

        // CLI session handling
        if let Some(cli_sid) = resume_session {
            args.push("--resume".to_string());
            args.push(cli_sid.to_string());
        }

        args
    }
}

#[cfg(feature = "gemini-cli")]
/// Parse Gemini CLI JSON output, returning (response_text, session_id)
fn parse_gemini_cli_output(stdout: &str) -> Result<(String, Option<String>)> {
    // Gemini CLI outputs JSON: { "session_id": "...", "response": "..." }
    if let Ok(json) = serde_json::from_str::<Value>(stdout) {
        let text = json
            .get("response")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| stdout.trim().to_string());

        let session_id = json
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        return Ok((text, session_id));
    }

    // Fallback
    Ok((stdout.trim().to_string(), None))
}

#[cfg(feature = "gemini-cli")]
#[async_trait]
impl LLMProvider for GeminiCliProvider {
    fn name(&self) -> String {
        "gemini-cli".to_string()
    }

    fn reset_session(&self) {
        if let Ok(mut cli_session) = self.cli_session_id.lock() {
            *cli_session = None;
        }
        // Clear from session store on disk
        if let Ok(mut store) = super::session_store::SessionStore::load() {
            let _ = store.update(&self.session_key, &self.localgpt_session_id, |entry| {
                entry.clear_cli_session_ids();
            });
        }
        info!("Gemini CLI session reset");
    }

    async fn chat(
        &self,
        messages: &[Message],
        _tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        let prompt = build_prompt_from_messages(messages);
        let system_prompt = extract_system_prompt(messages);

        let current_cli_session = self
            .cli_session_id
            .lock()
            .map_err(|e| anyhow::anyhow!("Session lock poisoned: {}", e))?
            .clone();

        let (output, used_new_session) = self
            .execute_cli_command(
                &prompt,
                system_prompt.as_deref(),
                current_cli_session.as_deref(),
            )
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let (response, new_session_id) = parse_gemini_cli_output(&stdout)?;

        if let Some(ref new_cli_sid) = new_session_id {
            let mut cli_session = self
                .cli_session_id
                .lock()
                .map_err(|e| anyhow::anyhow!("Session lock poisoned: {}", e))?;
            *cli_session = Some(new_cli_sid.clone());

            if let Err(e) = save_cli_session_to_store(
                &self.session_key,
                &self.localgpt_session_id,
                GEMINI_CLI_PROVIDER,
                new_cli_sid,
            ) {
                debug!("Failed to persist CLI session: {}", e);
            }

            if used_new_session {
                info!("Created new Gemini CLI session: {}", new_cli_sid);
            }
        }

        Ok(LLMResponse::text(response))
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!("Summarize concisely:\n\n{}", text),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];
        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response"),
        }
    }

    async fn chat_stream(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<StreamResult> {
        // Emulate streaming by calling chat() and yielding one chunk
        let response = self.chat(messages, tools).await?;

        let text = match response.content {
            LLMResponseContent::Text(t) => t,
            _ => return Err(anyhow::anyhow!("Unexpected non-text response")),
        };

        let stream = async_stream::stream! {
            yield Ok(StreamChunk {
                delta: text,
                done: true,
                tool_calls: None,
            });
        };

        Ok(Box::pin(stream))
    }
}

#[cfg(feature = "codex-cli")]
/// Parse Codex CLI JSON output, returning (response_text, session_id)
fn parse_codex_cli_output(stdout: &str) -> Result<(String, Option<String>)> {
    if let Ok(json) = serde_json::from_str::<Value>(stdout) {
        let text = json
            .get("response")
            .and_then(|v| v.as_str())
            .or_else(|| json.get("text").and_then(|v| v.as_str()))
            .or_else(|| json.get("content").and_then(|v| v.as_str()))
            .map(|s| s.to_string())
            .unwrap_or_else(|| stdout.trim().to_string());

        let session_id = json
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        return Ok((text, session_id));
    }

    Ok((stdout.trim().to_string(), None))
}

#[cfg(feature = "codex-cli")]
pub struct CodexCliProvider {
    command: String,
    model: String,
    workspace: std::path::PathBuf,
    session_key: String,
    localgpt_session_id: String,
    cli_session_id: StdMutex<Option<String>>,
}

#[cfg(feature = "codex-cli")]
const CODEX_CLI_PROVIDER: &str = "codex-cli";

#[cfg(feature = "codex-cli")]
impl CodexCliProvider {
    pub fn new(command: &str, model: &str, workspace: std::path::PathBuf) -> Result<Self> {
        let session_key = "main".to_string();
        let existing_session = load_cli_session_from_store(&session_key, CODEX_CLI_PROVIDER);

        if let Some(ref sid) = existing_session {
            debug!("Loaded existing Codex CLI session: {}", sid);
        }

        Ok(Self {
            command: command.to_string(),
            model: model.to_string(),
            workspace,
            session_key,
            localgpt_session_id: uuid::Uuid::new_v4().to_string(),
            cli_session_id: StdMutex::new(existing_session),
        })
    }

    async fn execute_cli_command(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        existing_session: Option<&str>,
    ) -> Result<(std::process::Output, bool)> {
        use std::process::Command;

        let mut args = vec!["-q".to_string(), "--json".to_string()];

        if !self.model.is_empty() {
            args.push("--model".to_string());
            args.push(self.model.clone());
        }

        if let Some(sys) = system_prompt {
            args.push("--system-prompt".to_string());
            args.push(sys.to_string());
        }

        if let Some(sid) = existing_session {
            args.push("--session".to_string());
            args.push(sid.to_string());
        }

        args.push(prompt.to_string());

        debug!(
            "Codex CLI: {} {:?} (cwd: {:?})",
            self.command, args, self.workspace
        );

        let output = tokio::task::spawn_blocking({
            let command = self.command.clone();
            let args = args.clone();
            let workspace = self.workspace.clone();
            move || {
                Command::new(&command)
                    .args(&args)
                    .current_dir(&workspace)
                    .output()
            }
        })
        .await??;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Codex CLI failed: {}", stderr);
        }

        Ok((output, existing_session.is_none()))
    }
}

#[cfg(feature = "codex-cli")]
#[async_trait]
impl LLMProvider for CodexCliProvider {
    fn name(&self) -> String {
        "codex-cli".to_string()
    }

    fn reset_session(&self) {
        if let Ok(mut cli_session) = self.cli_session_id.lock() {
            *cli_session = None;
        }
        if let Ok(mut store) = super::session_store::SessionStore::load() {
            let _ = store.update(&self.session_key, &self.localgpt_session_id, |entry| {
                entry.clear_cli_session_ids();
            });
        }
        info!("Codex CLI session reset");
    }

    async fn chat(
        &self,
        messages: &[Message],
        _tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        let prompt = build_prompt_from_messages(messages);
        let system_prompt = extract_system_prompt(messages);

        let current_cli_session = self
            .cli_session_id
            .lock()
            .map_err(|e| anyhow::anyhow!("Session lock poisoned: {}", e))?
            .clone();

        let (output, used_new_session) = self
            .execute_cli_command(
                &prompt,
                system_prompt.as_deref(),
                current_cli_session.as_deref(),
            )
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let (response, new_session_id) = parse_codex_cli_output(&stdout)?;

        if let Some(ref new_cli_sid) = new_session_id {
            let mut cli_session = self
                .cli_session_id
                .lock()
                .map_err(|e| anyhow::anyhow!("Session lock poisoned: {}", e))?;
            *cli_session = Some(new_cli_sid.clone());

            if let Err(e) = save_cli_session_to_store(
                &self.session_key,
                &self.localgpt_session_id,
                CODEX_CLI_PROVIDER,
                new_cli_sid,
            ) {
                debug!("Failed to persist CLI session: {}", e);
            }

            if used_new_session {
                info!("Created new Codex CLI session: {}", new_cli_sid);
            }
        }

        Ok(LLMResponse::text(response))
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!("Summarize concisely:\n\n{}", text),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];
        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response"),
        }
    }

    async fn chat_stream(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<StreamResult> {
        let response = self.chat(messages, tools).await?;

        let text = match response.content {
            LLMResponseContent::Text(t) => t,
            _ => return Err(anyhow::anyhow!("Unexpected non-text response")),
        };

        let stream = async_stream::stream! {
            yield Ok(StreamChunk {
                delta: text,
                done: true,
                tool_calls: None,
            });
        };

        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
#[path = "./test/unit/openaiprovider_tool_test.rs"]
mod providers_test;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_total() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
        };
        assert_eq!(usage.total(), 150);
    }

    #[test]
    fn test_usage_default() {
        let usage = Usage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert_eq!(usage.total(), 0);
    }

    #[test]
    fn test_llm_response_constructors() {
        // Text response
        let resp = LLMResponse::text("hello".to_string());
        assert!(matches!(resp.content, LLMResponseContent::Text(_)));
        assert!(resp.usage.is_none());

        // Text with usage
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 5,
        };
        let resp = LLMResponse::text_with_usage("hello".to_string(), usage);
        assert!(matches!(resp.content, LLMResponseContent::Text(_)));
        assert!(resp.usage.is_some());
        assert_eq!(resp.usage.unwrap().total(), 15);

        // Tool calls
        let calls = vec![ToolCall {
            id: "1".to_string(),
            name: "test".to_string(),
            arguments: "{}".to_string(),
        }];
        let resp = LLMResponse::tool_calls(calls);
        assert!(matches!(resp.content, LLMResponseContent::ToolCalls { .. }));
        assert!(resp.usage.is_none());
    }

    #[test]
    fn test_resolve_model_alias() {
        assert_eq!(resolve_model_alias("opus"), "anthropic/claude-opus-4-6");
        assert_eq!(resolve_model_alias("sonnet"), "anthropic/claude-sonnet-4-6");
        assert_eq!(resolve_model_alias("gpt"), "openai/gpt-4o");
        assert_eq!(resolve_model_alias("gpt-mini"), "openai/gpt-4o-mini");
        assert_eq!(resolve_model_alias("grok"), "xai/grok-3-mini");
        assert_eq!(
            resolve_model_alias("custom-model"),
            "custom-model".to_string()
        );
    }

    #[test]
    fn test_xai_native_search_definition() {
        let provider = XaiProvider::new("test-key", "https://api.x.ai/v1", "grok-3-mini")
            .expect("provider should construct");
        assert!(provider.supports_native_search());
        let defs = provider.native_tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["type"], "web_search");
    }

    #[test]
    fn test_xai_parse_tool_calls_from_responses_output() {
        let output = vec![json!({
            "type": "function_call",
            "id": "fc_1",
            "call_id": "call_1",
            "name": "memory_search",
            "arguments": "{\"query\":\"rust\"}"
        })];

        let calls = XaiProvider::parse_tool_calls(&output);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].name, "memory_search");
        assert_eq!(calls[0].arguments, "{\"query\":\"rust\"}");
    }

    #[test]
    fn test_xai_parse_output_text_from_responses_output() {
        let body = json!({
            "output": [{
                "type": "message",
                "content": [
                    {"type": "output_text", "text": "Hello"},
                    {"type": "output_text", "text": " world"}
                ]
            }]
        });

        let text = XaiProvider::parse_output_text(&body);
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn test_xai_format_input_includes_tool_output_item() {
        let provider = XaiProvider::new("test-key", "https://api.x.ai/v1", "grok-3-mini")
            .expect("provider should construct");

        let messages = vec![
            Message {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "memory_search".to_string(),
                    arguments: "{\"query\":\"rust\"}".to_string(),
                }]),
                tool_call_id: None,
                images: Vec::new(),
            },
            Message {
                role: Role::Tool,
                content: "result".to_string(),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
                images: Vec::new(),
            },
        ];

        let formatted = provider.format_input(&messages);
        assert_eq!(formatted.len(), 2);
        assert_eq!(formatted[0]["type"], "function_call");
        assert_eq!(formatted[1]["type"], "function_call_output");
        assert_eq!(formatted[1]["call_id"], "call_1");
        assert_eq!(formatted[1]["output"], "result");
    }
}

// Anthropic OAuth Provider (for Claude Pro/Max subscription plans)
pub struct AnthropicOAuthProvider {
    client: Client,
    access_token: std::sync::Arc<std::sync::RwLock<String>>,
    refresh_token: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    expires_at: std::sync::Arc<std::sync::RwLock<Option<u64>>>,
    base_url: String,
    model: String,
    max_tokens: usize,
}

impl AnthropicOAuthProvider {
    pub fn new(config: OAuthConfig, model: &str, max_tokens: usize) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            access_token: std::sync::Arc::new(std::sync::RwLock::new(config.access_token)),
            refresh_token: config.refresh_token,
            client_id: config.client_id,
            client_secret: config.client_secret,
            expires_at: std::sync::Arc::new(std::sync::RwLock::new(config.expires_at)),
            base_url: config.base_url,
            model: model.to_string(),
            max_tokens,
        })
    }

    async fn refresh_access_token(&self) -> Result<()> {
        let refresh_token = self
            .refresh_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No refresh token available"))?;
        let client_id = self
            .client_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client ID available"))?;
        let client_secret = self
            .client_secret
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client secret available"))?;

        debug!("Refreshing Anthropic OAuth access token...");

        let params = [
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];

        let response = self
            .client
            .post(format!("{}/oauth/token", self.base_url))
            .form(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Failed to refresh Anthropic access token: {}", error_text);
        }

        let json: Value = response.json().await?;
        let new_access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No access_token in refresh response"))?;
        let expires_in = json["expires_in"].as_u64().unwrap_or(3600);

        {
            let mut token_guard = self
                .access_token
                .write()
                .map_err(|_| anyhow::anyhow!("Failed to acquire write lock on access_token"))?;
            *token_guard = new_access_token.to_string();
        }

        {
            let mut expires_guard = self
                .expires_at
                .write()
                .map_err(|_| anyhow::anyhow!("Failed to acquire write lock on expires_at"))?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            *expires_guard = Some(now + expires_in);
        }

        info!("Anthropic OAuth access token refreshed successfully");
        Ok(())
    }

    async fn ensure_valid_token(&self) -> Result<()> {
        let should_refresh = {
            let expires_at = self
                .expires_at
                .read()
                .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on expires_at"))?;
            if let Some(expiry) = *expires_at {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now + 300 >= expiry
            } else {
                false
            }
        };

        if should_refresh && self.refresh_token.is_some() {
            self.refresh_access_token().await?;
        }

        Ok(())
    }

    fn format_tools(&self, tools: &[ToolSchema]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters
                })
            })
            .collect()
    }

    fn format_messages(&self, messages: &[Message]) -> (Option<String>, Vec<Value>) {
        let mut system_prompt = None;
        let mut formatted = Vec::new();

        for m in messages {
            match m.role {
                Role::System => {
                    system_prompt = Some(m.content.clone());
                }
                Role::User => {
                    if m.images.is_empty() {
                        formatted.push(json!({
                            "role": "user",
                            "content": m.content
                        }));
                    } else {
                        let mut content_parts: Vec<Value> = Vec::new();
                        for img in &m.images {
                            content_parts.push(json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": img.media_type,
                                    "data": img.data
                                }
                            }));
                        }
                        if !m.content.is_empty() {
                            content_parts.push(json!({
                                "type": "text",
                                "text": m.content
                            }));
                        }
                        formatted.push(json!({
                            "role": "user",
                            "content": content_parts
                        }));
                    }
                }
                Role::Assistant => {
                    if let Some(ref tool_calls) = m.tool_calls {
                        let tool_use: Vec<Value> = tool_calls.iter().map(|tc| {
                            json!({
                                "type": "tool_use",
                                "id": tc.id,
                                "name": tc.name,
                                "input": serde_json::from_str::<Value>(&tc.arguments).unwrap_or(json!({}))
                            })
                        }).collect();
                        formatted.push(json!({
                            "role": "assistant",
                            "content": tool_use
                        }));
                    } else {
                        formatted.push(json!({
                            "role": "assistant",
                            "content": m.content
                        }));
                    }
                }
                Role::Tool => {
                    if let Some(ref tool_call_id) = m.tool_call_id {
                        formatted.push(json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": tool_call_id,
                                "content": m.content
                            }]
                        }));
                    }
                }
            }
        }

        (system_prompt, formatted)
    }
}

#[async_trait]
impl LLMProvider for AnthropicOAuthProvider {
    fn name(&self) -> String {
        "anthropic-oauth".to_string()
    }

    fn token_update(&self) -> Option<OAuthTokenUpdate> {
        let access_token = self.access_token.read().ok()?.clone();
        let expires_at = *self.expires_at.read().ok()?;
        Some(OAuthTokenUpdate {
            provider: "anthropic".to_string(),
            access_token,
            refresh_token: self.refresh_token.clone(),
            expires_at,
        })
    }

    fn supports_native_search(&self) -> bool {
        true
    }

    fn native_tool_definitions(&self) -> Vec<Value> {
        vec![json!({
            "type": "web_search_20250305",
            "name": "web_search"
        })]
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        self.ensure_valid_token().await?;

        let (system_prompt, formatted_messages) = self.format_messages(messages);

        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": formatted_messages
        });

        if let Some(system) = system_prompt {
            body["system"] = json!(system);
        }

        let mut all_tools = Vec::new();
        if let Some(tool_schemas) = tools {
            let client_has_web_search = tool_schemas.iter().any(|t| t.name == "web_search");
            if !client_has_web_search {
                all_tools.extend(self.native_tool_definitions());
            }
            if !tool_schemas.is_empty() {
                all_tools.extend(self.format_tools(tool_schemas));
            }
        }
        if !all_tools.is_empty() {
            body["tools"] = json!(all_tools);
        }

        debug!(
            "Anthropic OAuth request: {}",
            serde_json::to_string_pretty(&body)?
        );

        let current_access_token = self
            .access_token
            .read()
            .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on access_token"))?
            .clone();
        let response = self
            .client
            .post(format!("{}/v1/messages", self.base_url))
            .header("Authorization", format!("Bearer {}", current_access_token))
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let response =
            if status == reqwest::StatusCode::UNAUTHORIZED && self.refresh_token.is_some() {
                debug!("Anthropic OAuth returned 401 Unauthorized, attempting to refresh token...");
                self.refresh_access_token().await?;
                let new_access_token = self
                    .access_token
                    .read()
                    .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on access_token"))?
                    .clone();

                self.client
                    .post(format!("{}/v1/messages", self.base_url))
                    .header("Authorization", format!("Bearer {}", new_access_token))
                    .header("anthropic-version", "2023-06-01")
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await?
            } else {
                response
            };

        let response_body: Value = response.json().await?;
        debug!(
            "Anthropic OAuth response: {}",
            serde_json::to_string_pretty(&response_body)?
        );

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("Anthropic OAuth API error: {}", error);
        }

        let content = response_body["content"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("No content in response"))?;

        let usage = response_body.get("usage").map(|u| Usage {
            input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
        });

        let tool_calls: Vec<ToolCall> = content
            .iter()
            .filter(|c| c["type"] == "tool_use")
            .map(|c| ToolCall {
                id: c["id"].as_str().unwrap_or("").to_string(),
                name: c["name"].as_str().unwrap_or("").to_string(),
                arguments: serde_json::to_string(&c["input"]).unwrap_or("{}".to_string()),
            })
            .collect();

        if !tool_calls.is_empty() {
            let text = {
                let t = content
                    .iter()
                    .filter(|c| c["type"] == "text")
                    .map(|c| c["text"].as_str().unwrap_or(""))
                    .collect::<Vec<_>>()
                    .join("");
                if t.is_empty() { None } else { Some(t) }
            };
            return Ok(LLMResponse {
                content: LLMResponseContent::ToolCalls {
                    calls: tool_calls,
                    text,
                },
                usage,
            });
        }

        let text = content
            .iter()
            .filter(|c| c["type"] == "text")
            .map(|c| c["text"].as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("");

        Ok(LLMResponse {
            content: LLMResponseContent::Text(text),
            usage,
        })
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!(
                "Summarize the following conversation concisely, preserving key information and context:\n\n{}",
                text
            ),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];

        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response type"),
        }
    }
}

// Gemini OAuth Provider (for Google AI subscription plans)
pub struct GeminiOAuthProvider {
    client: Client,
    access_token: std::sync::Arc<std::sync::RwLock<String>>,
    refresh_token: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    expires_at: std::sync::Arc<std::sync::RwLock<Option<u64>>>,
    base_url: String,
    model: String,
    project_id: Option<String>,
}

impl GeminiOAuthProvider {
    pub fn new(config: OAuthConfig, model: &str, project_id: Option<&str>) -> Result<Self> {
        let client = Client::builder()
            .http1_only()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;

        Ok(Self {
            client,
            access_token: std::sync::Arc::new(std::sync::RwLock::new(config.access_token)),
            refresh_token: config.refresh_token,
            client_id: config.client_id,
            client_secret: config.client_secret,
            expires_at: std::sync::Arc::new(std::sync::RwLock::new(config.expires_at)),
            base_url: config.base_url,
            model: model.to_string(),
            project_id: project_id.map(|s| s.to_string()),
        })
    }

    async fn refresh_access_token(&self) -> Result<()> {
        let refresh_token = self
            .refresh_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No refresh token available"))?;
        let client_id = self
            .client_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client ID available"))?;
        let client_secret = self
            .client_secret
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client secret available"))?;

        debug!("Refreshing Gemini OAuth access token...");

        let params = [
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];

        let response = self
            .client
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Failed to refresh Gemini access token: {}", error_text);
        }

        let json: Value = response.json().await?;
        let new_access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No access_token in refresh response"))?;
        let expires_in = json["expires_in"].as_u64().unwrap_or(3600);

        {
            let mut token_guard = self
                .access_token
                .write()
                .map_err(|_| anyhow::anyhow!("Failed to acquire write lock on access_token"))?;
            *token_guard = new_access_token.to_string();
        }

        {
            let mut expires_guard = self
                .expires_at
                .write()
                .map_err(|_| anyhow::anyhow!("Failed to acquire write lock on expires_at"))?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            *expires_guard = Some(now + expires_in);
        }

        info!("Gemini OAuth access token refreshed successfully");
        Ok(())
    }

    async fn ensure_valid_token(&self) -> Result<()> {
        let should_refresh = {
            let expires_at = self
                .expires_at
                .read()
                .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on expires_at"))?;
            if let Some(expiry) = *expires_at {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                // Refresh if token expires in less than 5 minutes
                now + 300 >= expiry
            } else {
                false
            }
        };

        if should_refresh && self.refresh_token.is_some() {
            self.refresh_access_token().await?;
        }

        Ok(())
    }

    fn format_messages(&self, messages: &[Message]) -> Vec<Value> {
        let mut formatted = Vec::new();
        let mut system_instruction = None;

        for m in messages {
            match m.role {
                Role::System => {
                    system_instruction = Some(m.content.clone());
                }
                Role::User => {
                    let mut parts = Vec::new();
                    if !m.content.is_empty() {
                        parts.push(json!({"text": m.content}));
                    }
                    for img in &m.images {
                        parts.push(json!({
                            "inline_data": {
                                "mime_type": img.media_type,
                                "data": img.data
                            }
                        }));
                    }
                    formatted.push(json!({
                        "role": "user",
                        "parts": parts
                    }));
                }
                Role::Assistant => {
                    if let Some(ref tool_calls) = m.tool_calls {
                        let function_calls: Vec<Value> = tool_calls
                            .iter()
                            .map(|tc| {
                                json!({
                                    "function_call": {
                                        "name": tc.name,
                                        "args": serde_json::from_str::<Value>(&tc.arguments)
                                            .unwrap_or(json!({}))
                                    }
                                })
                            })
                            .collect();
                        formatted.push(json!({
                            "role": "model",
                            "parts": function_calls
                        }));
                    } else {
                        formatted.push(json!({
                            "role": "model",
                            "parts": [{"text": m.content}]
                        }));
                    }
                }
                Role::Tool => {
                    if let Some(ref tool_call_id) = m.tool_call_id {
                        formatted.push(json!({
                            "role": "function",
                            "parts": [{
                                "function_response": {
                                    "name": tool_call_id,
                                    "response": {"result": m.content}
                                }
                            }]
                        }));
                    }
                }
            }
        }

        // Prepend system instruction if present
        if let Some(system) = system_instruction {
            formatted.insert(
                0,
                json!({
                    "role": "user",
                    "parts": [{"text": system}]
                }),
            );
        }

        formatted
    }

    fn format_tools(&self, tools: &[ToolSchema]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "function_declarations": [{
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }]
                })
            })
            .collect()
    }
}

#[async_trait]
impl LLMProvider for GeminiOAuthProvider {
    fn name(&self) -> String {
        "gemini-oauth".to_string()
    }

    fn token_update(&self) -> Option<OAuthTokenUpdate> {
        let access_token = self.access_token.read().ok()?.clone();
        let expires_at = *self.expires_at.read().ok()?;
        Some(OAuthTokenUpdate {
            provider: "gemini".to_string(),
            access_token,
            refresh_token: self.refresh_token.clone(),
            expires_at,
        })
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        self.ensure_valid_token().await?;

        let formatted_messages = self.format_messages(messages);

        let mut body = json!({
            "contents": formatted_messages,
        });

        if let Some(tool_schemas) = tools
            && !tool_schemas.is_empty()
        {
            body["tools"] = json!(self.format_tools(tool_schemas));
        }

        debug!(
            "Gemini OAuth request: {}",
            serde_json::to_string_pretty(&body)?
        );

        // Build the URL based on whether we have a project_id
        let url = if let Some(ref project_id) = self.project_id {
            format!(
                "{}/v1beta/projects/{}/models/{}:generateContent",
                self.base_url, project_id, self.model
            )
        } else {
            format!(
                "{}/v1beta/models/{}:generateContent",
                self.base_url, self.model
            )
        };

        let current_access_token = self
            .access_token
            .read()
            .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on access_token"))?
            .clone();
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", current_access_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let mut status = response.status();

        // Handle 401 Unauthorized - try to refresh and retry once
        let response =
            if status == reqwest::StatusCode::UNAUTHORIZED && self.refresh_token.is_some() {
                debug!("Gemini OAuth returned 401 Unauthorized, attempting to refresh token...");
                self.refresh_access_token().await?;
                let new_access_token = self
                    .access_token
                    .read()
                    .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on access_token"))?
                    .clone();

                let retry_response = self
                    .client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", new_access_token))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await?;

                status = retry_response.status();
                retry_response
            } else {
                response
            };

        let response_text = response.text().await?;
        debug!("Gemini OAuth response ({}): {}", status, response_text);

        let response_body: Value = match serde_json::from_str(&response_text) {
            Ok(val) => val,
            Err(e) => {
                anyhow::bail!(
                    "Failed to decode Gemini response body (Status: {}): {}\n\nRaw response:\n{}",
                    status,
                    e,
                    response_text
                );
            }
        };

        if let Some(error) = response_body.get("error") {
            anyhow::bail!("Gemini OAuth API error: {}", error);
        }

        let candidates = response_body["candidates"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("No candidates in response"))?;

        if candidates.is_empty() {
            anyhow::bail!("Empty candidates in response");
        }

        let parts = candidates[0]["content"]["parts"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("No parts in candidate"))?;

        // Check for function calls (tool use)
        let tool_calls: Vec<ToolCall> = parts
            .iter()
            .filter_map(|p| p.get("function_call"))
            .enumerate()
            .map(|(i, fc)| ToolCall {
                id: format!("call_{}", i),
                name: fc["name"].as_str().unwrap_or("").to_string(),
                arguments: serde_json::to_string(&fc["args"]).unwrap_or("{}".to_string()),
            })
            .collect();

        if !tool_calls.is_empty() {
            let text = {
                let t = parts
                    .iter()
                    .filter_map(|p| p.get("text"))
                    .filter_map(|t| t.as_str())
                    .collect::<Vec<_>>()
                    .join("");
                if t.is_empty() { None } else { Some(t) }
            };
            return Ok(LLMResponse {
                content: LLMResponseContent::ToolCalls {
                    calls: tool_calls,
                    text,
                },
                usage: None,
            });
        }

        // Get text content
        let text = parts
            .iter()
            .filter_map(|p| p.get("text"))
            .filter_map(|t| t.as_str())
            .collect::<Vec<_>>()
            .join("");

        Ok(LLMResponse {
            content: LLMResponseContent::Text(text),
            usage: None,
        })
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!(
                "Summarize the following conversation concisely, preserving key information and context:\n\n{}",
                text
            ),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];

        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response type"),
        }
    }
}

// OpenAI OAuth Provider (for ChatGPT Plus/Pro/Team subscription plans)
pub struct OpenAIOAuthProvider {
    client: Client,
    access_token: std::sync::Arc<std::sync::RwLock<String>>,
    refresh_token: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    expires_at: std::sync::Arc<std::sync::RwLock<Option<u64>>>,
    base_url: String,
    model: String,
}

impl OpenAIOAuthProvider {
    pub fn new(config: OAuthConfig, model: &str) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            access_token: std::sync::Arc::new(std::sync::RwLock::new(config.access_token)),
            refresh_token: config.refresh_token,
            client_id: config.client_id,
            client_secret: config.client_secret,
            expires_at: std::sync::Arc::new(std::sync::RwLock::new(config.expires_at)),
            base_url: config.base_url,
            model: model.to_string(),
        })
    }

    async fn refresh_access_token(&self) -> Result<()> {
        let refresh_token = self
            .refresh_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No refresh token available"))?;
        let client_id = self
            .client_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client ID available"))?;
        let client_secret = self
            .client_secret
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client secret available"))?;

        debug!("Refreshing OpenAI OAuth access token...");

        let params = [
            ("client_id", client_id.as_str()),
            ("client_secret", client_secret.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];

        let response = self
            .client
            .post(format!("{}/oauth/token", self.base_url))
            .form(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Failed to refresh OpenAI access token: {}", error_text);
        }

        let json: Value = response.json().await?;
        let new_access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No access_token in refresh response"))?;
        let expires_in = json["expires_in"].as_u64().unwrap_or(3600);

        {
            let mut token_guard = self
                .access_token
                .write()
                .map_err(|_| anyhow::anyhow!("Failed to acquire write lock on access_token"))?;
            *token_guard = new_access_token.to_string();
        }

        {
            let mut expires_guard = self
                .expires_at
                .write()
                .map_err(|_| anyhow::anyhow!("Failed to acquire write lock on expires_at"))?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            *expires_guard = Some(now + expires_in);
        }

        info!("OpenAI OAuth access token refreshed successfully");
        Ok(())
    }

    async fn ensure_valid_token(&self) -> Result<()> {
        let should_refresh = {
            let expires_at = self
                .expires_at
                .read()
                .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on expires_at"))?;
            if let Some(expiry) = *expires_at {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now + 300 >= expiry
            } else {
                false
            }
        };

        if should_refresh && self.refresh_token.is_some() {
            self.refresh_access_token().await?;
        }

        Ok(())
    }

    fn format_tools(&self, tools: &[ToolSchema]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect()
    }

    fn format_messages(&self, messages: &[Message]) -> Vec<Value> {
        messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };

                // Handle multimodal content for user messages with images
                let content: Value = if m.role == Role::User && !m.images.is_empty() {
                    let mut content_parts: Vec<Value> = Vec::new();

                    // Add images first (OpenAI uses data URLs)
                    for img in &m.images {
                        content_parts.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:{};base64,{}", img.media_type, img.data)
                            }
                        }));
                    }

                    // Add text content
                    if !m.content.is_empty() {
                        content_parts.push(json!({
                            "type": "text",
                            "text": m.content
                        }));
                    }

                    json!(content_parts)
                } else {
                    json!(m.content)
                };

                let mut msg = json!({
                    "role": role,
                    "content": content
                });

                if let Some(ref tool_calls) = m.tool_calls {
                    msg["tool_calls"] = json!(
                        tool_calls
                            .iter()
                            .map(|tc| {
                                json!({
                                    "id": tc.id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.arguments
                                    }
                                })
                            })
                            .collect::<Vec<_>>()
                    );
                }

                if let Some(ref tool_call_id) = m.tool_call_id {
                    msg["tool_call_id"] = json!(tool_call_id);
                }

                msg
            })
            .collect()
    }
}

#[async_trait]
impl LLMProvider for OpenAIOAuthProvider {
    fn name(&self) -> String {
        "openai-oauth".to_string()
    }

    fn token_update(&self) -> Option<OAuthTokenUpdate> {
        let access_token = self.access_token.read().ok()?.clone();
        let expires_at = *self.expires_at.read().ok()?;
        Some(OAuthTokenUpdate {
            provider: "openai".to_string(),
            access_token,
            refresh_token: self.refresh_token.clone(),
            expires_at,
        })
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        self.ensure_valid_token().await?;

        let mut body = json!({
            "model": self.model,
            "messages": self.format_messages(messages)
        });

        if let Some(tools) = tools
            && !tools.is_empty()
        {
            body["tools"] = json!(self.format_tools(tools));
        }

        debug!(
            "OpenAI OAuth request: {}",
            serde_json::to_string_pretty(&body)?
        );

        let current_access_token = self
            .access_token
            .read()
            .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on access_token"))?
            .clone();
        let response = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", current_access_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let response =
            if status == reqwest::StatusCode::UNAUTHORIZED && self.refresh_token.is_some() {
                debug!("OpenAI OAuth returned 401 Unauthorized, attempting to refresh token...");
                self.refresh_access_token().await?;
                let new_access_token = self
                    .access_token
                    .read()
                    .map_err(|_| anyhow::anyhow!("Failed to acquire read lock on access_token"))?
                    .clone();

                self.client
                    .post(format!("{}/chat/completions", self.base_url))
                    .header("Authorization", format!("Bearer {}", new_access_token))
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .send()
                    .await?
            } else {
                response
            };

        let response_body: Value = response.json().await?;
        debug!(
            "OpenAI OAuth response: {}",
            serde_json::to_string_pretty(&response_body)?
        );

        // Check for errors
        if let Some(error) = response_body.get("error") {
            anyhow::bail!("OpenAI OAuth API error: {}", error);
        }

        let choice = response_body["choices"]
            .get(0)
            .ok_or_else(|| anyhow::anyhow!("No choices in response"))?;

        let message = &choice["message"];

        // Parse usage
        let usage = response_body.get("usage").map(|u| Usage {
            input_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
        });

        // Check for tool calls
        if let Some(tool_calls) = message.get("tool_calls")
            && let Some(calls) = tool_calls.as_array()
        {
            let parsed_calls: Vec<ToolCall> = calls
                .iter()
                .map(|tc| ToolCall {
                    id: tc["id"].as_str().unwrap_or("").to_string(),
                    name: tc["function"]["name"].as_str().unwrap_or("").to_string(),
                    arguments: tc["function"]["arguments"]
                        .as_str()
                        .unwrap_or("{}")
                        .to_string(),
                })
                .collect();

            if !parsed_calls.is_empty() {
                let text = message["content"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                return Ok(LLMResponse {
                    content: LLMResponseContent::ToolCalls {
                        calls: parsed_calls,
                        text,
                    },
                    usage,
                });
            }
        }

        let content = message["content"].as_str().unwrap_or("").to_string();

        Ok(LLMResponse {
            content: LLMResponseContent::Text(content),
            usage,
        })
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let messages = vec![Message {
            role: Role::User,
            content: format!(
                "Summarize the following conversation concisely, preserving key information and context:\n\n{}",
                text
            ),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }];

        match self.chat(&messages, None).await?.content {
            LLMResponseContent::Text(summary) => Ok(summary),
            _ => anyhow::bail!("Unexpected response type"),
        }
    }
}

// GitHub Copilot Provider
pub struct GitHubCopilotProvider {
    client: Client,
    github_token: std::sync::Arc<std::sync::RwLock<String>>,
    github_refresh_token: Option<String>,
    github_client_id: Option<String>,
    github_client_secret: Option<String>,
    github_expires_at: std::sync::Arc<std::sync::RwLock<Option<u64>>>,
    copilot_token: std::sync::Arc<std::sync::RwLock<Option<String>>>,
    copilot_expires_at: std::sync::Arc<std::sync::RwLock<u64>>,
    base_url: std::sync::Arc<std::sync::RwLock<String>>,
    model: String,
}

impl GitHubCopilotProvider {
    pub fn new(
        github_token: &str,
        github_refresh_token: Option<String>,
        github_client_id: Option<String>,
        github_client_secret: Option<String>,
        github_expires_at: Option<u64>,
        model: &str,
    ) -> Result<Self> {
        Ok(Self {
            client: Client::new(),
            github_token: std::sync::Arc::new(std::sync::RwLock::new(github_token.to_string())),
            github_refresh_token,
            github_client_id,
            github_client_secret,
            github_expires_at: std::sync::Arc::new(std::sync::RwLock::new(github_expires_at)),
            copilot_token: std::sync::Arc::new(std::sync::RwLock::new(None)),
            copilot_expires_at: std::sync::Arc::new(std::sync::RwLock::new(0)),
            base_url: std::sync::Arc::new(std::sync::RwLock::new(
                "https://api.individual.githubcopilot.com".to_string(),
            )),
            model: model.to_string(),
        })
    }

    async fn refresh_github_token(&self) -> Result<()> {
        let refresh_token = self
            .github_refresh_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No refresh token available"))?;
        let client_id = self
            .github_client_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client ID available"))?;

        debug!("Refreshing GitHub OAuth access token...");

        let mut params = vec![
            ("client_id", client_id.as_str()),
            ("refresh_token", refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ];
        if let Some(ref secret) = self.github_client_secret {
            params.push(("client_secret", secret.as_str()));
        }

        let response = self
            .client
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .form(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Failed to refresh GitHub access token: {}", error_text);
        }

        let json: Value = response.json().await?;
        let new_access_token = json["access_token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No access_token in refresh response"))?;
        let expires_in = json["expires_in"].as_u64().unwrap_or(3600);

        {
            let mut token_guard = self
                .github_token
                .write()
                .map_err(|_| anyhow::anyhow!("Lock error"))?;
            *token_guard = new_access_token.to_string();
        }

        {
            let mut expires_guard = self
                .github_expires_at
                .write()
                .map_err(|_| anyhow::anyhow!("Lock error"))?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            *expires_guard = Some(now + expires_in);
        }

        info!("GitHub OAuth access token refreshed successfully");
        Ok(())
    }

    async fn ensure_valid_github_token(&self) -> Result<()> {
        let should_refresh = {
            let expires_at = self
                .github_expires_at
                .read()
                .map_err(|_| anyhow::anyhow!("Lock error"))?;
            if let Some(expiry) = *expires_at {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now + 300 >= expiry
            } else {
                false
            }
        };

        if should_refresh && self.github_refresh_token.is_some() {
            self.refresh_github_token().await?;
        }

        Ok(())
    }

    async fn refresh_copilot_token(&self) -> Result<()> {
        self.ensure_valid_github_token().await?;
        let github_token = self
            .github_token
            .read()
            .map_err(|_| anyhow::anyhow!("Lock error"))?
            .clone();

        debug!("Exchanging GitHub token for Copilot API token...");

        let response = self
            .client
            .get("https://api.github.com/copilot_internal/v2/token")
            .header("Authorization", format!("Bearer {}", github_token))
            .header("User-Agent", "LocalGPT/1.0")
            .header("Accept", "application/json")
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("GitHub Copilot token exchange failed: {}", error_text);
        }

        let json: Value = response.json().await?;
        let token = json["token"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No token in Copilot response"))?;
        let expires_at = json["expires_at"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("No expires_at in Copilot response"))?;

        // GitHub returns expires_at in seconds, but we defensively check
        let expires_at_ms = if expires_at > 10_000_000_000 {
            expires_at
        } else {
            expires_at * 1000
        };

        {
            let mut token_guard = self
                .copilot_token
                .write()
                .map_err(|_| anyhow::anyhow!("Lock error"))?;
            *token_guard = Some(token.to_string());
        }
        {
            let mut expires_guard = self
                .copilot_expires_at
                .write()
                .map_err(|_| anyhow::anyhow!("Lock error"))?;
            *expires_guard = expires_at_ms;
        }

        // Derive base URL from token if possible (proxy-ep=...)
        if let Some(pos) = token.find("proxy-ep=") {
            let rest = &token[pos + 9..];
            let end = rest.find(';').unwrap_or(rest.len());
            let proxy_url = &rest[..end];
            let api_url = proxy_url.replace("proxy.", "api.");
            let mut base_url_guard = self
                .base_url
                .write()
                .map_err(|_| anyhow::anyhow!("Lock error"))?;
            *base_url_guard = api_url;
        }

        info!("GitHub Copilot API token acquired successfully");
        Ok(())
    }

    async fn ensure_valid_token(&self) -> Result<String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let needs_refresh = {
            let expires_at = self
                .copilot_expires_at
                .read()
                .map_err(|_| anyhow::anyhow!("Lock error"))?;
            let token = self
                .copilot_token
                .read()
                .map_err(|_| anyhow::anyhow!("Lock error"))?;
            token.is_none() || now + 300_000 >= *expires_at // 5 min margin
        };

        if needs_refresh {
            self.refresh_copilot_token().await?;
        }

        self.copilot_token
            .read()
            .map_err(|_| anyhow::anyhow!("Lock error"))?
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Token missing after refresh"))
    }
}

#[async_trait]
impl LLMProvider for GitHubCopilotProvider {
    fn name(&self) -> String {
        "github-copilot".to_string()
    }

    fn token_update(&self) -> Option<OAuthTokenUpdate> {
        let access_token = self.github_token.read().ok()?.clone();
        let expires_at = *self.github_expires_at.read().ok()?;
        Some(OAuthTokenUpdate {
            provider: "github".to_string(),
            access_token,
            refresh_token: self.github_refresh_token.clone(),
            expires_at,
        })
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        let token = self.ensure_valid_token().await?;
        let base_url = self
            .base_url
            .read()
            .map_err(|_| anyhow::anyhow!("Lock error"))?
            .clone();

        // GitHub Copilot uses OpenAI-compatible API
        let openai_provider = OpenAIOAuthProvider::new(
            OAuthConfig {
                access_token: token,
                refresh_token: None,
                client_id: None,
                client_secret: None,
                expires_at: None,
                base_url,
            },
            &self.model,
        )?;

        openai_provider.chat(messages, tools).await
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let token = self.ensure_valid_token().await?;
        let base_url = self
            .base_url
            .read()
            .map_err(|_| anyhow::anyhow!("Lock error"))?
            .clone();

        let openai_provider = OpenAIOAuthProvider::new(
            OAuthConfig {
                access_token: token,
                refresh_token: None,
                client_id: None,
                client_secret: None,
                expires_at: None,
                base_url,
            },
            &self.model,
        )?;

        openai_provider.summarize(text).await
    }
}
