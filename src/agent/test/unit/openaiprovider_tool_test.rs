//! Tests for the default `chat_stream` fallback on `LLMProvider`.
//!
//! The default implementation delegates to `chat()` and wraps the result
//! in a single `StreamChunk`. These tests verify that:
//!   1. Tool schemas are forwarded (not dropped)
//!   2. Text responses produce a valid stream chunk
//!   3. ToolCalls responses produce a stream chunk with tool_calls populated

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;

use super::*;

/// Mock provider that returns a configured response from chat(),
/// used to test the default chat_stream fallback on LLMProvider.
struct MockProvider {
    response: std::sync::Mutex<Option<LLMResponse>>,
    /// Captures whether tools were forwarded to chat()
    received_tools: std::sync::Mutex<bool>,
}

impl MockProvider {
    fn returning_text(text: &str) -> Self {
        Self {
            response: std::sync::Mutex::new(Some(LLMResponse::text(text.to_string()))),
            received_tools: std::sync::Mutex::new(false),
        }
    }

    fn returning_tool_calls(calls: Vec<ToolCall>) -> Self {
        Self {
            response: std::sync::Mutex::new(Some(LLMResponse::tool_calls(calls))),
            received_tools: std::sync::Mutex::new(false),
        }
    }
}

#[async_trait]
impl LLMProvider for MockProvider {
    async fn chat(
        &self,
        _messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        if let Some(t) = tools {
            if !t.is_empty() {
                *self.received_tools.lock().unwrap() = true;
            }
        }
        self.response
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| anyhow::anyhow!("MockProvider exhausted"))
    }

    async fn summarize(&self, _text: &str) -> Result<String> {
        Ok(String::new())
    }
}

#[tokio::test]
async fn test_default_chat_stream_forwards_tools() {
    let provider = MockProvider::returning_text("hello");
    let messages = vec![Message {
        role: Role::User,
        content: "test".to_string(),
        tool_calls: None,
        tool_call_id: None,
        images: Vec::new(),
    }];
    let tools = vec![ToolSchema {
        name: "bash".to_string(),
        description: "Execute a command".to_string(),
        parameters: serde_json::json!({"type": "object"}),
    }];

    let _stream = provider
        .chat_stream(&messages, Some(&tools))
        .await
        .expect("chat_stream should succeed");

    assert!(
        *provider.received_tools.lock().unwrap(),
        "Default chat_stream must forward tools to chat()"
    );
}

#[tokio::test]
async fn test_default_chat_stream_returns_text_as_stream_chunk() {
    let provider = MockProvider::returning_text("hello world");
    let messages = vec![Message {
        role: Role::User,
        content: "test".to_string(),
        tool_calls: None,
        tool_call_id: None,
        images: Vec::new(),
    }];

    let mut stream = provider
        .chat_stream(&messages, None)
        .await
        .expect("chat_stream should succeed");

    let chunk = stream.next().await.expect("stream should yield a chunk");
    let chunk = chunk.expect("chunk should be Ok");

    assert_eq!(chunk.delta, "hello world");
    assert!(chunk.done);
    assert!(chunk.tool_calls.is_none());
}

#[tokio::test]
async fn test_default_chat_stream_returns_tool_calls_as_stream_chunk() {
    let calls = vec![ToolCall {
        id: "call_1".to_string(),
        name: "bash".to_string(),
        arguments: "{\"command\":\"pwd\"}".to_string(),
    }];
    let provider = MockProvider::returning_tool_calls(calls);
    let messages = vec![Message {
        role: Role::User,
        content: "test".to_string(),
        tool_calls: None,
        tool_call_id: None,
        images: Vec::new(),
    }];

    let mut stream = provider
        .chat_stream(&messages, None)
        .await
        .expect("chat_stream should succeed");

    let chunk = stream.next().await.expect("stream should yield a chunk");
    let chunk = chunk.expect("chunk should be Ok");

    assert!(chunk.done);
    assert!(
        chunk.delta.is_empty(),
        "tool call chunk should have empty delta"
    );
    let tool_calls = chunk.tool_calls.expect("chunk should contain tool_calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].name, "bash");
    assert_eq!(tool_calls[0].arguments, "{\"command\":\"pwd\"}");
}
