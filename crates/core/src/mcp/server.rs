//! MCP server — exposes LocalGPT tools to external AI backends.
//!
//! Provides both stdio and (optionally) HTTP transports:
//!
//! - **stdio**: Reads JSON-RPC 2.0 messages from stdin, writes responses to
//!   stdout. Used by `localgpt mcp-server` and `localgpt-gen mcp-server`.
//!
//! - **HTTP** (feature `mcp-http`): Streamable HTTP transport per the MCP spec.
//!   POST `/mcp` accepts JSON-RPC requests; GET `/mcp` opens an SSE stream for
//!   server-initiated notifications. Enable via `--mcp-http` flag.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info};

use super::super::agent::tools::Tool;

// ---------------------------------------------------------------------------
// McpHandler trait — shared dispatch interface for stdio and HTTP transports
// ---------------------------------------------------------------------------

/// Trait for dispatching MCP JSON-RPC requests.
///
/// Both the stdio server loop and the HTTP transport call into this trait.
/// The default implementation (`ToolHandler`) wraps a `Vec<Box<dyn Tool>>`.
#[async_trait]
pub trait McpHandler: Send + Sync {
    /// Server name returned in `initialize` response.
    fn server_name(&self) -> &str;

    /// Handle an `initialize` request.
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

    /// Handle a `tools/list` request.
    fn handle_tools_list(&self) -> Result<Value, Value>;

    /// Handle a `tools/call` request.
    async fn handle_tools_call(&self, params: &Value) -> Result<Value, Value>;

    /// Dispatch a parsed JSON-RPC request. Returns `Some(response)` for
    /// requests (those with an `id`) and `None` for notifications.
    async fn dispatch(&self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));

        // Notifications (no id) — handle but don't respond
        if id.is_none() {
            match method {
                "notifications/initialized" => {
                    info!("MCP server: client initialized");
                }
                "notifications/cancelled" => {
                    debug!("MCP server: received cancellation");
                }
                _ => {
                    debug!("MCP server: ignoring notification '{}'", method);
                }
            }
            return None;
        }

        let id = id.unwrap();

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

        let response_msg = match response {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": error,
            }),
        };

        Some(response_msg)
    }
}

// ---------------------------------------------------------------------------
// ToolHandler — default McpHandler backed by a Vec<Box<dyn Tool>>
// ---------------------------------------------------------------------------

/// Default `McpHandler` implementation backed by a tool vector.
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
                        .unwrap()
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
                "content": [{
                    "type": "text",
                    "text": output,
                }]
            })),
            Err(e) => Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Error: {}", e),
                }],
                "isError": true,
            })),
        }
    }
}

// ---------------------------------------------------------------------------
// Stdio server (always available)
// ---------------------------------------------------------------------------

/// Run the MCP stdio server loop with the given tool set.
///
/// Blocks until stdin is closed (i.e., the MCP client disconnects).
pub async fn run_mcp_stdio_server(tools: Vec<Box<dyn Tool>>, server_name: &str) -> Result<()> {
    let handler = Arc::new(ToolHandler::new(server_name, tools));
    run_mcp_stdio_server_with_handler(handler).await
}

/// Run the MCP stdio server loop with a custom handler.
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

        if let Some(response_msg) = handler.dispatch(&msg).await {
            let mut out = serde_json::to_string(&response_msg)?;
            out.push('\n');
            stdout.write_all(out.as_bytes()).await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}
