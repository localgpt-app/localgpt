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
#[cfg(not(feature = "claude-cli"))]
use tracing::debug;
#[cfg(feature = "claude-cli")]
use tracing::{debug, info};

use crate::config::Config;

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
    ToolCalls(Vec<ToolCall>),
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
            content: LLMResponseContent::ToolCalls(calls),
            usage: None,
        }
    }

    pub fn tool_calls_with_usage(calls: Vec<ToolCall>, usage: Usage) -> Self {
        Self {
            content: LLMResponseContent::ToolCalls(calls),
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

#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Get provider name
    fn name(&self) -> String;

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
            LLMResponseContent::ToolCalls(calls) => {
                Ok(Box::pin(futures::stream::once(async move {
                    Ok(StreamChunk {
                        delta: String::new(),
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
        // Short aliases → latest 4.5 models
        "opus" => "anthropic/claude-opus-4-5".to_string(),
        "sonnet" => "anthropic/claude-sonnet-4-5".to_string(),
        "gpt" => "openai/gpt-4o".to_string(),
        "gpt-mini" => "openai/gpt-4o-mini".to_string(),
        "glm" => "glm/glm-4.7".to_string(),
        "grok" => "xai/grok-3-mini".to_string(),
        _ => model.to_string(),
    }
}

/// Map OpenClaw model ID to actual API model ID
/// See: https://docs.anthropic.com/en/docs/about-claude/models
fn normalize_model_id(provider: &str, model_id: &str) -> String {
    match provider {
        "anthropic" => {
            match model_id.to_lowercase().as_str() {
                // Claude 4.5 models only
                "claude-opus-4-5" | "opus" => "claude-opus-4-5-20251101".to_string(),
                "claude-sonnet-4-5" | "sonnet" => "claude-sonnet-4-5-20250929".to_string(),
                // Default to Opus 4.5 for any other input
                _ => "claude-opus-4-5-20251101".to_string(),
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
            let anthropic_config = config.providers.anthropic.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Anthropic provider not configured.\n\
                    Set ANTHROPIC_API_KEY env var or add to ~/.localgpt/config.toml:\n\n\
                    [providers.anthropic]\n\
                    api_key = \"sk-ant-...\""
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

        "openai" => {
            let openai_config = config.providers.openai.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "OpenAI provider not configured.\n\
                    Set OPENAI_API_KEY env var or add to ~/.localgpt/config.toml:\n\n\
                    [providers.openai]\n\
                    api_key = \"sk-...\""
                )
            })?;

            Ok(Box::new(OpenAIProvider::new(
                &openai_config.api_key,
                &openai_config.base_url,
                &model_id,
            )?))
        }

        "xai" => {
            let xai_config = config.providers.xai.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "xAI provider not configured.\n\
                    Set XAI_API_KEY env var or add to ~/.localgpt/config.toml:\n\n\
                    [providers.xai]\n\
                    api_key = \"xai-...\""
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

        "ollama" => {
            let ollama_config = config.providers.ollama.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "Ollama provider not configured.\n\
                    Add to ~/.localgpt/config.toml:\n\n\
                    [providers.ollama]\n\
                    endpoint = \"http://localhost:11434\""
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
                    Set GLM_API_KEY env var or add to ~/.localgpt/config.toml:\n\n\
                    [providers.glm]\n\
                    api_key = \"your-glm-api-key\""
                )
            })?;

            Ok(Box::new(OpenAIProvider::new(
                &glm_config.api_key,
                &glm_config.base_url,
                &model_id,
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
                - ollama/llama3, ollama/mistral\n\n\
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
                return Ok(LLMResponse {
                    content: LLMResponseContent::ToolCalls(parsed_calls),
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
            return Ok(LLMResponse {
                content: LLMResponseContent::ToolCalls(parsed_calls),
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
            return Ok(LLMResponse {
                content: LLMResponseContent::ToolCalls(tool_calls),
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
                return Ok(LLMResponse {
                    content: LLMResponseContent::ToolCalls(calls),
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
                LLMResponseContent::ToolCalls(calls) => {
                    Ok(Box::pin(futures::stream::once(async move {
                        Ok(StreamChunk {
                            delta: String::new(),
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
        "opus" | "opus-4.5" | "opus-4" | "claude-opus-4-5" => "opus",
        "sonnet" | "sonnet-4.5" | "sonnet-4.1" | "claude-sonnet-4-5" => "sonnet",
        "haiku" | "haiku-3.5" | "claude-haiku-3-5" => "haiku",
        other => other,
    }
    .to_string()
}

#[cfg(feature = "claude-cli")]
/// Check if a message is the synthetic security block appended by `messages_for_api_call`.
fn is_security_block(msg: &Message) -> bool {
    msg.role == Role::User
        && msg
            .content
            .contains(crate::security::HARDCODED_SECURITY_SUFFIX)
}

#[cfg(feature = "claude-cli")]
fn build_prompt_from_messages(messages: &[Message]) -> String {
    // Get the last *real* user message as the prompt, skipping the security block
    messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User && !is_security_block(m))
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

#[cfg(feature = "claude-cli")]
fn extract_system_prompt(messages: &[Message]) -> Option<String> {
    let system = messages
        .iter()
        .find(|m| m.role == Role::System)
        .map(|m| m.content.clone());

    // For Claude CLI, fold the security block into the system prompt
    // since the CLI only accepts a single prompt + system prompt
    let security = messages
        .iter()
        .rev()
        .find(|m| is_security_block(m))
        .map(|m| m.content.clone());

    match (system, security) {
        (Some(sys), Some(sec)) => Some(format!("{}\n\n{}", sys, sec)),
        (Some(sys), None) => Some(sys),
        (None, Some(sec)) => Some(sec),
        (None, None) => None,
    }
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
                                                        .map(|s| if s.len() > 60 { format!("{}...", &s[..57]) } else { s.to_string() }),
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
        assert!(matches!(resp.content, LLMResponseContent::ToolCalls(_)));
        assert!(resp.usage.is_none());
    }

    #[test]
    fn test_resolve_model_alias() {
        assert_eq!(resolve_model_alias("opus"), "anthropic/claude-opus-4-5");
        assert_eq!(resolve_model_alias("sonnet"), "anthropic/claude-sonnet-4-5");
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
