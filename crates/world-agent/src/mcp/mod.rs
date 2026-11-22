//! The agent-facing tool surface: the [`Tool`] trait and an MCP server over
//! a set of tools.
//!
//! Behind the `mcp` feature. `localgpt world mcp` (the shim) and
//! `localgpt-gen mcp-server` both serve a `Vec<Box<dyn Tool>>` over stdio
//! with [`run_mcp_stdio_server`]; `mcp-http` adds the streamable HTTP
//! transport ([`http`]). A tool is a name, a JSON schema and an async call;
//! nothing here knows which agent is on the other end. A server answers a
//! call through [`call_tool`], which refuses an argument the schema does
//! not name. [`relay`] is the client half of a window's MCP relay: stdio
//! carried to the port it listens on (`localgpt gen mcp-server --connect`).

#[cfg(feature = "mcp-http")]
pub mod http;
pub mod relay;

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

/// A picture a tool hands back beside its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolImage {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// The largest picture a tool puts in its answer. A client holds a whole
/// result in memory and in its model's context; past this, the path is the
/// answer.
pub const MAX_INLINE_IMAGE_BYTES: usize = 3 << 20;

/// What a tool answers: its text, and any pictures beside it.
///
/// A path is only an answer to an agent that shares the server's files — not
/// to one outside the server's container, and it costs a second call
/// everywhere else — so a tool that makes a picture can hand back the
/// picture itself, as MCP's `image` content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolOutput {
    pub text: String,
    pub images: Vec<ToolImage>,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            images: Vec::new(),
        }
    }

    /// The answer with the PNG at `path` beside its text. A picture over
    /// [`MAX_INLINE_IMAGE_BYTES`], or one that cannot be read, is left where
    /// it is and the text says why, so the call still answers.
    pub fn with_png(mut self, path: &std::path::Path) -> Self {
        // The size first: a picture too large to send is not read to find
        // that out.
        let read = std::fs::metadata(path).and_then(|file| {
            if file.len() > MAX_INLINE_IMAGE_BYTES as u64 {
                Ok(Err(file.len()))
            } else {
                std::fs::read(path).map(Ok)
            }
        });
        match read {
            Ok(Ok(bytes)) => self.images.push(ToolImage {
                mime_type: "image/png".into(),
                bytes,
            }),
            Ok(Err(size)) => {
                self.text += &format!(
                    "\nThe picture is {size} bytes, over the {MAX_INLINE_IMAGE_BYTES} a result carries: read it from its path, or ask for a smaller one."
                );
            }
            Err(e) => {
                self.text +=
                    &format!("\nThe picture could not be read back ({e}): read it from its path.");
            }
        }
        self
    }

    /// The answer as the `content` of an MCP `tools/call` result: the text,
    /// then each picture as an `image` block.
    pub fn content(&self) -> Vec<Value> {
        use base64::Engine as _;
        let mut content = vec![json!({ "type": "text", "text": self.text })];
        content.extend(self.images.iter().map(|image| {
            json!({
                "type": "image",
                "data": base64::engine::general_purpose::STANDARD.encode(&image.bytes),
                "mimeType": image.mime_type,
            })
        }));
        content
    }
}

/// One tool: a name, its schema, and an async call taking the arguments as
/// a JSON string.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, arguments: &str) -> Result<String>;

    /// The call's whole answer. The default is [`execute`](Self::execute)'s
    /// text; a tool that can hand back a picture overrides this.
    async fn call(&self, arguments: &str) -> Result<ToolOutput> {
        Ok(ToolOutput::text(self.execute(arguments).await?))
    }

    /// MCP tool annotations (`readOnlyHint`, `destructiveHint`, …), when the
    /// tool states them.
    fn annotations(&self) -> Option<Value> {
        None
    }
}

/// Answer a client's call to `tool`: run it, unless the call carries an
/// argument the tool's schema does not name.
///
/// A tool reads the arguments it knows and takes its defaults for the rest,
/// so a misnamed argument would be dropped and the call would succeed
/// without doing what it asked. The refusal lists the arguments the tool
/// takes, which is what lets an agent correct the call. Every server
/// answers `tools/call` through here ([`tool_result`]).
pub async fn call_tool(tool: &dyn Tool, arguments: &Value) -> Result<ToolOutput> {
    let schema = tool.schema();
    let takes: Vec<&str> = schema
        .parameters
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|properties| properties.keys())
        .map(String::as_str)
        .collect();
    let unknown: Vec<String> = arguments
        .as_object()
        .into_iter()
        .flat_map(|given| given.keys())
        .filter(|name| !takes.contains(&name.as_str()))
        .map(|name| format!("`{name}`"))
        .collect();
    if let [rest @ .., last] = unknown.as_slice() {
        let unknown = if rest.is_empty() {
            last.clone()
        } else {
            format!("{} or {last}", rest.join(", "))
        };
        let takes = if takes.is_empty() {
            "It takes no arguments.".to_string()
        } else {
            format!("Its arguments: {}.", takes.join(", "))
        };
        anyhow::bail!(
            "{} was not run: it has no argument {unknown}. {takes}",
            tool.name()
        );
    }
    tool.call(&serde_json::to_string(arguments)?).await
}

/// The `result` of a `tools/call` on `tool`: its answer as content, or its
/// failure — a refused argument among them — as an error result the agent
/// reads and corrects. The stdio and HTTP servers and Gen's relay all
/// answer with this, so a picture reaches a client the same way through
/// each.
pub async fn tool_result(tool: &dyn Tool, arguments: &Value) -> Value {
    match call_tool(tool, arguments).await {
        Ok(output) => json!({ "content": output.content() }),
        // With its causes (`{:#}`): "opening x.wav: No such file or
        // directory", not just "opening x.wav". The reason is what lets
        // an agent correct itself.
        Err(e) => error_result(&format!("{e:#}")),
    }
}

/// A failure as a `tools/call` result.
pub fn error_result(reason: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": format!("Error: {reason}") }],
        "isError": true,
    })
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
        Ok(tool_result(tool.as_ref(), &arguments).await)
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

    /// A tool that takes the arguments named, and says that it ran.
    struct Takes(&'static [&'static str]);

    #[async_trait]
    impl Tool for Takes {
        fn name(&self) -> &str {
            "takes"
        }

        fn schema(&self) -> ToolSchema {
            let properties: serde_json::Map<String, Value> = self
                .0
                .iter()
                .map(|name| (name.to_string(), json!({"type": "string"})))
                .collect();
            ToolSchema {
                name: "takes".into(),
                description: "takes what it is told to".into(),
                parameters: json!({"type": "object", "properties": properties}),
            }
        }

        async fn execute(&self, _arguments: &str) -> Result<String> {
            Ok("ran".into())
        }
    }

    #[tokio::test]
    async fn a_call_runs_with_the_arguments_its_tool_takes() {
        let tool = Takes(&["text", "volume"]);
        for arguments in [
            json!({"text": "hi", "volume": "low"}),
            json!({"text": "hi"}),
            json!({}),
            Value::Null,
        ] {
            let said = call_tool(&tool, &arguments).await.expect("a call it takes");
            assert_eq!(said, ToolOutput::text("ran"), "{arguments}");
        }
    }

    /// A tool that answers with a picture when asked to.
    struct Shot(std::path::PathBuf);

    #[async_trait]
    impl Tool for Shot {
        fn name(&self) -> &str {
            "shot"
        }

        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "shot".into(),
                description: "takes a picture".into(),
                parameters: json!({"type": "object", "properties": {"inline": {"type": "boolean"}}}),
            }
        }

        async fn execute(&self, _arguments: &str) -> Result<String> {
            Ok(self.0.display().to_string())
        }

        async fn call(&self, arguments: &str) -> Result<ToolOutput> {
            let args: Value = serde_json::from_str(arguments)?;
            let output = ToolOutput::text(self.execute(arguments).await?);
            Ok(if args["inline"] == true {
                output.with_png(&self.0)
            } else {
                output
            })
        }
    }

    #[tokio::test]
    async fn a_picture_rides_beside_the_text_when_a_tool_hands_one_back() {
        let dir = std::env::temp_dir().join(format!("localgpt-mcp-shot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shot.png");
        std::fs::write(&path, b"\x89PNG not really").unwrap();
        let tool = Shot(path.clone());

        // Unasked, the answer is the text alone, as it always was.
        let plain = tool_result(&tool, &json!({})).await;
        assert_eq!(plain["content"].as_array().unwrap().len(), 1);
        assert_eq!(plain["content"][0]["text"], path.display().to_string());

        let with = tool_result(&tool, &json!({"inline": true})).await;
        let content = with["content"].as_array().unwrap();
        assert_eq!(content.len(), 2, "{with}");
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["mimeType"], "image/png");
        assert_eq!(
            content[1]["data"], "iVBORyBub3QgcmVhbGx5",
            "base64, unwrapped"
        );

        // A picture too large for a result, or gone, still answers: the
        // path, and why the picture is not beside it. The large one is
        // sparse — `with_png` decides on the file's size and never reads
        // its bytes, so the boundary is checked without asking a nearly
        // full temp disk for 3 MiB of real zeros.
        let big = std::fs::File::create(&path).unwrap();
        big.set_len(MAX_INLINE_IMAGE_BYTES as u64 + 1).unwrap();
        drop(big);
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            MAX_INLINE_IMAGE_BYTES as u64 + 1,
            "the size check sees the boundary"
        );
        let large = tool_result(&tool, &json!({"inline": true})).await;
        assert_eq!(large["content"].as_array().unwrap().len(), 1);
        assert!(
            large["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("read it from its path"),
            "{large}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
        let gone = tool_result(&tool, &json!({"inline": true})).await;
        assert!(gone.get("isError").is_none(), "{gone}");
        assert!(
            gone["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("could not be read back"),
            "{gone}"
        );
    }

    /// A tool takes its defaults for what it is not given, so a misnamed
    /// argument would be a success that did not do what was asked.
    #[tokio::test]
    async fn a_call_with_an_argument_its_tool_does_not_take_is_refused() {
        let refused = |tool: Takes, arguments: Value| async move {
            call_tool(&tool, &arguments)
                .await
                .expect_err("the tool must not run")
                .to_string()
        };
        assert_eq!(
            refused(
                Takes(&["text", "volume"]),
                json!({"text": "hi", "txt": "ho"})
            )
            .await,
            "takes was not run: it has no argument `txt`. Its arguments: text, volume."
        );
        assert_eq!(
            refused(Takes(&["text"]), json!({"a": 1, "b": 2, "c": 3})).await,
            "takes was not run: it has no argument `a`, `b` or `c`. Its arguments: text."
        );
        assert_eq!(
            refused(Takes(&[]), json!({"text": "hi"})).await,
            "takes was not run: it has no argument `text`. It takes no arguments."
        );
    }

    /// The refusal reaches the client as a tool's own failure does, so the
    /// agent reads it and can send the call again.
    #[tokio::test]
    async fn the_handler_answers_a_refused_call_as_an_error_result() {
        let handler = ToolHandler::new("test", vec![Box::new(Echo) as Box<dyn Tool>]);
        let call = handler
            .dispatch(&json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "echo", "arguments": {"txt": "hi"}}
            }))
            .await
            .unwrap();
        assert_eq!(call["result"]["isError"], true, "{call}");
        assert_eq!(
            call["result"]["content"][0]["text"],
            "Error: echo was not run: it has no argument `txt`. Its arguments: text."
        );
    }
}
