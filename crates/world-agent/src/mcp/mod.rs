//! The agent-facing tool surface: the [`Tool`] trait and an MCP server over
//! a set of tools.
//!
//! Behind the `mcp` feature. `localgpt world mcp` (the shim) and
//! `localgpt-gen mcp-server` both serve a `Vec<Box<dyn Tool>>` over stdio
//! with [`run_mcp_stdio_server`]; `mcp-http` adds the streamable HTTP
//! transport ([`http`]). A tool is a name, a JSON schema and an async call;
//! nothing here knows which agent is on the other end.

#[cfg(feature = "mcp-http")]
pub mod http;

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info};

/// A tool as an MCP client lists it: its name, what it does, and the JSON
/// schema of its arguments.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// One tool: a name, its schema, and an async call taking the arguments as
/// a JSON string.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, arguments: &str) -> Result<String>;

    /// MCP tool annotations (`readOnlyHint`, `destructiveHint`, …), when the
    /// tool states them.
    fn annotations(&self) -> Option<Value> {
        None
    }
}

// ---------------------------------------------------------------------------
// McpHandler — the dispatch shared by the stdio and HTTP transports
// ---------------------------------------------------------------------------

/// Dispatches MCP JSON-RPC requests. [`ToolHandler`] is the implementation
/// over a tool vector; a server with its own `initialize` answer implements
/// this itself.
#[async_trait]
pub trait McpHandler: Send + Sync {
    /// The name returned in the `initialize` response.
    fn server_name(&self) -> &str;

    fn handle_initialize(&self) -> Result<Value, Value> {
        Ok(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": self.server_name(),
                "version": env!("CARGO_PKG_VERSION"),
            }
        }))
    }

    fn handle_tools_list(&self) -> Result<Value, Value>;

    async fn handle_tools_call(&self, params: &Value) -> Result<Value, Value>;

    /// Dispatch one parsed JSON-RPC message: `Some(response)` for a request
    /// (one with an `id`), `None` for a notification.
    async fn dispatch(&self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));

        let Some(id) = id else {
            match method {
                "notifications/initialized" => info!("MCP server: client initialized"),
                "notifications/cancelled" => debug!("MCP server: received cancellation"),
                _ => debug!("MCP server: ignoring notification '{}'", method),
            }
            return None;
        };

        let response = match method {
            "initialize" => self.handle_initialize(),
            "tools/list" => self.handle_tools_list(),
            "tools/call" => self.handle_tools_call(&params).await,
            "ping" => Ok(json!({})),
            _ => Err(json!({
                "code": -32601,
                "message": format!("Method not found: {}", method),
            })),
        };

        Some(match response {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
        })
    }
}

/// The handler over a tool vector.
pub struct ToolHandler {
    server_name: String,
    tools: Vec<Box<dyn Tool>>,
}

impl ToolHandler {
    pub fn new(server_name: impl Into<String>, tools: Vec<Box<dyn Tool>>) -> Self {
        Self {
            server_name: server_name.into(),
            tools,
        }
    }
}

#[async_trait]
impl McpHandler for ToolHandler {
    fn server_name(&self) -> &str {
        &self.server_name
    }

    fn handle_tools_list(&self) -> Result<Value, Value> {
        let tool_defs: Vec<Value> = self
            .tools
            .iter()
            .map(|t| {
                let schema = t.schema();
                let mut def = json!({
                    "name": schema.name,
                    "description": schema.description,
                    "inputSchema": schema.parameters,
                });
                if let Some(annotations) = t.annotations() {
                    def.as_object_mut()
                        .expect("a JSON object")
                        .insert("annotations".into(), annotations);
                }
                def
            })
            .collect();
        Ok(json!({ "tools": tool_defs }))
    }

    async fn handle_tools_call(&self, params: &Value) -> Result<Value, Value> {
        let tool_name = params.get("name").and_then(|n| n.as_str()).ok_or_else(|| {
            json!({
                "code": -32602,
                "message": "Missing 'name' parameter",
            })
        })?;
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| {
                json!({
                    "code": -32602,
                    "message": format!("Unknown tool: {}", tool_name),
                })
            })?;
        let args_str = serde_json::to_string(&arguments).unwrap_or_default();
        match tool.execute(&args_str).await {
            Ok(output) => Ok(json!({
                "content": [{ "type": "text", "text": output }]
            })),
            Err(e) => Ok(json!({
                "content": [{ "type": "text", "text": format!("Error: {}", e) }],
                "isError": true,
            })),
        }
    }
}

// ---------------------------------------------------------------------------
// The stdio server
// ---------------------------------------------------------------------------

/// Serve `tools` over stdio until stdin closes (the MCP client went away)
/// or Ctrl+C.
pub async fn run_mcp_stdio_server(tools: Vec<Box<dyn Tool>>, server_name: &str) -> Result<()> {
    let handler = Arc::new(ToolHandler::new(server_name, tools));
    run_mcp_stdio_server_with_handler(handler).await
}

/// [`run_mcp_stdio_server`] with a handler of the caller's own.
pub async fn run_mcp_stdio_server_with_handler(handler: Arc<dyn McpHandler>) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    info!("MCP server '{}' ready (stdio)", handler.server_name());

    loop {
        line.clear();
        let bytes_read = tokio::select! {
            result = reader.read_line(&mut line) => result?,
            _ = tokio::signal::ctrl_c() => {
                info!("MCP server: received Ctrl+C, shutting down");
                break;
            }
        };
        if bytes_read == 0 {
            info!("MCP server: stdin closed, shutting down");
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                debug!("MCP server: ignoring non-JSON line: {}", e);
                continue;
            }
        };
        if let Some(response) = handler.dispatch(&msg).await {
            let mut out = serde_json::to_string(&response)?;
            out.push('\n');
            stdout.write_all(out.as_bytes()).await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    #[async_trait]
    impl Tool for Echo {
        fn name(&self) -> &str {
            "echo"
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "echo".into(),
                description: "says it back".into(),
                parameters: json!({"type": "object", "properties": {"text": {"type": "string"}}}),
            }
        }

        async fn execute(&self, arguments: &str) -> Result<String> {
            let args: Value = serde_json::from_str(arguments)?;
            Ok(args["text"].as_str().unwrap_or_default().to_string())
        }

        fn annotations(&self) -> Option<Value> {
            Some(json!({"readOnlyHint": true}))
        }
    }

    #[tokio::test]
    async fn the_handler_lists_calls_and_refuses_what_it_does_not_know() {
        let handler = ToolHandler::new("test", vec![Box::new(Echo) as Box<dyn Tool>]);

        let init = handler
            .dispatch(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize"}))
            .await
            .unwrap();
        assert_eq!(init["result"]["serverInfo"]["name"], "test");

        let list = handler
            .dispatch(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
            .await
            .unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert_eq!(tools[0]["name"], "echo");
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);

        let call = handler
            .dispatch(&json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {"name": "echo", "arguments": {"text": "hi"}}
            }))
            .await
            .unwrap();
        assert_eq!(call["result"]["content"][0]["text"], "hi");

        let unknown = handler
            .dispatch(&json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": {"name": "nope", "arguments": {}}
            }))
            .await
            .unwrap();
        assert_eq!(unknown["error"]["code"], -32602);

        // A notification gets no reply.
        assert!(
            handler
                .dispatch(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
                .await
                .is_none()
        );
    }
}
