//! OpenAI-compatible HTTP API
//!
//! Provides `/v1/chat/completions` and `/v1/models` endpoints that match
//! the OpenAI wire format, enabling integration with tools like Cursor,
//! Continue, Open WebUI, LibreChat, and the Python `openai` library.

use anyhow::Result;
use axum::{
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Json, Response,
        sse::{Event, Sse},
    },
};
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};
use uuid::Uuid;

use localgpt_core::agent::{
    Agent, AgentConfig, LLMResponse, LLMResponseContent, Message, Role, StreamEvent, ToolCall,
    ToolSchema,
};
use localgpt_core::config::Config;

use crate::http::AppState;

// ============================================================================
// Request/Response Types (OpenAI Wire Format)
// ============================================================================

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OaiMessage>,
    #[serde(default)]
    pub stream: bool,
    pub max_tokens: Option<usize>,
    pub temperature: Option<f64>,
    pub tools: Option<Vec<OaiToolDef>>,
    /// Map of tool_choice options: "auto", "none", or {"type": "function", "function": {"name": "..."}}
    pub tool_choice: Option<Value>,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct OaiMessage {
    pub role: String,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<OaiToolCallResponse>>,
    pub tool_call_id: Option<String>,
    /// For assistant messages with tool calls, the content might be null or string
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OaiToolCallResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: Option<String>,
    pub function: OaiFunctionCall,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct OaiFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct OaiToolDef {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OaiFunctionDef,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct OaiFunctionDef {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<OaiUsage>,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: usize,
    pub message: OaiResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OaiResponseMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OaiToolCallResponse>>,
}

#[derive(Debug, Serialize)]
pub struct OaiUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChunkChoice {
    pub index: usize,
    pub delta: ChunkDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Default)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OaiToolCallChunk>>,
}

#[derive(Debug, Serialize)]
pub struct OaiToolCallChunk {
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub tool_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<OaiFunctionCallChunk>,
}

#[derive(Debug, Serialize)]
pub struct OaiFunctionCallChunk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub object: &'static str,
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub owned_by: String,
}

// ============================================================================
// Message Conversion
// ============================================================================

/// Convert OpenAI messages to LocalGPT Message format
fn convert_messages(oai_messages: &[OaiMessage]) -> Result<Vec<Message>> {
    let mut messages = Vec::new();

    for msg in oai_messages {
        let role = match msg.role.to_lowercase().as_str() {
            "system" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool" => Role::Tool,
            other => {
                // Default unknown roles to user
                debug!("Unknown role '{}', treating as user", other);
                Role::User
            }
        };

        // Convert tool calls from OpenAI format to LocalGPT format
        let tool_calls = msg.tool_calls.as_ref().map(|calls| {
            calls
                .iter()
                .map(|tc| ToolCall {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    arguments: tc.function.arguments.clone(),
                })
                .collect()
        });

        messages.push(Message {
            role,
            content: msg.content.clone().unwrap_or_default(),
            tool_calls,
            tool_call_id: msg.tool_call_id.clone(),
            images: Vec::new(),
        });
    }

    Ok(messages)
}

/// Convert OpenAI tool definitions to LocalGPT ToolSchema
fn convert_tools(oai_tools: &[OaiToolDef]) -> Vec<ToolSchema> {
    oai_tools
        .iter()
        .map(|t| ToolSchema {
            name: t.function.name.clone(),
            description: t.function.description.clone().unwrap_or_default(),
            parameters: t.function.parameters.clone().unwrap_or(json!({})),
        })
        .collect()
}

/// Get current Unix timestamp
fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Generate a unique completion ID
fn generate_completion_id() -> String {
    format!("chatcmpl-{}", Uuid::new_v4().simple())
}

// ============================================================================
// Handlers
// ============================================================================

/// Handle POST /v1/chat/completions
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, (StatusCode, String)> {
    if req.stream {
        return chat_completions_stream(state, req)
            .await
            .map(|r| r.into_response());
    }

    chat_completions_non_stream(state, req)
        .await
        .map(|r| r.into_response())
}

/// Non-streaming chat completion
async fn chat_completions_non_stream(
    state: Arc<AppState>,
    req: ChatCompletionRequest,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let messages = convert_messages(&req.messages)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid messages: {}", e)))?;

    let tools = req.tools.as_ref().map(|t| convert_tools(t));

    // Create a fresh agent for this request
    let agent_config = AgentConfig {
        model: req.model.clone(),
        context_window: state.config.agent.context_window,
        reserve_tokens: state.config.agent.reserve_tokens,
    };

    let memory = Arc::new(state.memory.clone());
    let mut agent = Agent::new(agent_config, &state.config, memory)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create agent: {}", e),
            )
        })?;

    info!("OpenAI API: non-streaming request for model {}", req.model);

    // Call the provider
    let response = agent
        .chat_with_messages(&messages, tools.as_deref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("LLM error: {}", e),
            )
        })?;

    // Convert response
    let completion = to_completion_response(response, &req.model);

    Ok(Json(completion))
}

/// Streaming chat completion (SSE)
async fn chat_completions_stream(
    state: Arc<AppState>,
    req: ChatCompletionRequest,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Extract the last user message for streaming
    let last_message = req
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .and_then(|m| m.content.clone())
        .unwrap_or_default();

    let model = req.model.clone();
    let completion_id = generate_completion_id();
    let created = unix_timestamp();

    // Create a fresh agent for this request
    let agent_config = AgentConfig {
        model: model.clone(),
        context_window: state.config.agent.context_window,
        reserve_tokens: state.config.agent.reserve_tokens,
    };

    let memory = Arc::new(state.memory.clone());

    info!("OpenAI API: streaming request for model {}", model);

    // The agent must live for the duration of the stream, so we create the stream
    // in an async_stream that owns both the agent and the inner event stream.
    let event_stream = create_sse_stream_owned(
        agent_config,
        state.config.clone(),
        memory,
        last_message,
        completion_id,
        created,
        model,
    );

    Ok(Sse::new(event_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text(""),
    ))
}

/// Create an SSE stream that owns its agent and handles the full lifecycle.
fn create_sse_stream_owned(
    agent_config: AgentConfig,
    config: Config,
    memory: Arc<localgpt_core::memory::MemoryManager>,
    message: String,
    completion_id: String,
    created: u64,
    model: String,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::try_stream! {
        // Create agent inside the stream so it lives for the stream's duration
        let mut agent = match Agent::new(agent_config, &config, memory).await {
            Ok(a) => a,
            Err(e) => {
                warn!("Failed to create agent for streaming: {}", e);
                yield Event::default().data("[DONE]");
                return;
            }
        };

        let event_stream = match agent.chat_stream_with_tools(&message, Vec::new()).await {
            Ok(s) => s,
            Err(e) => {
                warn!("Failed to start stream: {}", e);
                yield Event::default().data("[DONE]");
                return;
            }
        };

        let mut stream = std::pin::pin!(event_stream);

        // Send initial chunk with role
        let initial = ChatCompletionChunk {
            id: completion_id.clone(),
            object: "chat.completion.chunk",
            created,
            model: model.clone(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: ChunkDelta {
                    role: Some("assistant".to_string()),
                    content: None,
                    tool_calls: None,
                },
                finish_reason: None,
            }],
        };
        yield Event::default().json_data(initial).unwrap();

        let mut tool_call_index: usize = 0;

        while let Some(event) = stream.next().await {
            match event {
                Ok(StreamEvent::Content(text)) => {
                    let chunk = ChatCompletionChunk {
                        id: completion_id.clone(),
                        object: "chat.completion.chunk",
                        created,
                        model: model.clone(),
                        choices: vec![ChunkChoice {
                            index: 0,
                            delta: ChunkDelta {
                                role: None,
                                content: Some(text),
                                tool_calls: None,
                            },
                            finish_reason: None,
                        }],
                    };
                    yield Event::default().json_data(chunk).unwrap();
                }
                Ok(StreamEvent::ToolCallStart { name, id, arguments: _ }) => {
                    let chunk = ChatCompletionChunk {
                        id: completion_id.clone(),
                        object: "chat.completion.chunk",
                        created,
                        model: model.clone(),
                        choices: vec![ChunkChoice {
                            index: 0,
                            delta: ChunkDelta {
                                role: None,
                                content: None,
                                tool_calls: Some(vec![OaiToolCallChunk {
                                    index: tool_call_index,
                                    id: Some(id),
                                    tool_type: Some("function".to_string()),
                                    function: Some(OaiFunctionCallChunk {
                                        name: Some(name),
                                        arguments: None,
                                    }),
                                }]),
                            },
                            finish_reason: None,
                        }],
                    };
                    yield Event::default().json_data(chunk).unwrap();
                    tool_call_index += 1;
                }
                Ok(StreamEvent::ToolCallEnd { .. }) => {
                    // Tool call finished - the output will be processed internally
                    // We don't need to send anything special for the end
                }
                Ok(StreamEvent::Done) => {
                    // Send final chunk with finish_reason
                    let finish_chunk = ChatCompletionChunk {
                        id: completion_id.clone(),
                        object: "chat.completion.chunk",
                        created,
                        model: model.clone(),
                        choices: vec![ChunkChoice {
                            index: 0,
                            delta: ChunkDelta::default(),
                            finish_reason: Some("stop".to_string()),
                        }],
                    };
                    yield Event::default().json_data(finish_chunk).unwrap();
                    break;
                }
                Err(e) => {
                    warn!("Stream error: {}", e);
                    break;
                }
            }
        }

        // Send [DONE] marker
        yield Event::default().data("[DONE]");
    }
}

/// Handle GET /v1/models
pub async fn list_models(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut models = Vec::new();

    // Add the default model
    let default_model = &state.config.agent.default_model;
    models.push(ModelInfo {
        id: default_model.clone(),
        object: "model",
        created: 0,
        owned_by: "localgpt".to_string(),
    });

    // Add fallback models
    for model in &state.config.agent.fallback_models {
        models.push(ModelInfo {
            id: model.clone(),
            object: "model",
            created: 0,
            owned_by: "localgpt".to_string(),
        });
    }

    // Add configured provider models
    if let Some(ollama) = &state.config.providers.ollama {
        models.push(ModelInfo {
            id: format!("ollama/{}", ollama.model),
            object: "model",
            created: 0,
            owned_by: "ollama".to_string(),
        });
    }

    Ok(Json(ModelsResponse {
        object: "list",
        data: models,
    }))
}

// ============================================================================
// Response Conversion
// ============================================================================

/// Convert LocalGPT LLMResponse to OpenAI ChatCompletionResponse
fn to_completion_response(response: LLMResponse, model: &str) -> ChatCompletionResponse {
    let (content, tool_calls, finish_reason) = match response.content {
        LLMResponseContent::Text(text) => (Some(text), None, "stop"),
        LLMResponseContent::ToolCalls { calls, text } => {
            let oai_calls: Vec<OaiToolCallResponse> = calls
                .iter()
                .map(|c| OaiToolCallResponse {
                    id: c.id.clone(),
                    tool_type: Some("function".to_string()),
                    function: OaiFunctionCall {
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                    },
                })
                .collect();
            (text, Some(oai_calls), "tool_calls")
        }
    };

    let usage = response.usage.map(|u| OaiUsage {
        prompt_tokens: u.input_tokens,
        completion_tokens: u.output_tokens,
        total_tokens: u.total(),
    });

    ChatCompletionResponse {
        id: generate_completion_id(),
        object: "chat.completion",
        created: unix_timestamp(),
        model: model.to_string(),
        choices: vec![Choice {
            index: 0,
            message: OaiResponseMessage {
                role: "assistant".to_string(),
                content,
                tool_calls,
            },
            finish_reason: Some(finish_reason.to_string()),
        }],
        usage,
    }
}
