//! MCP relay server — lets an external MCP client use the existing GenBridge.
//!
//! When `localgpt-gen` runs interactively, this relay listens on a TCP port.
//! A separate `localgpt-gen mcp-server --connect <port>` process connects
//! and forwards MCP stdio ↔ TCP, so tool calls go to the existing Bevy window
//! instead of spawning a new one.
//!
//! Protocol: JSON-RPC 2.0 (MCP) over newline-delimited TCP.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use localgpt_world_agent::mcp::{Tool, call_tool};

use crate::gen3d::GenBridge;

/// Default port for the MCP relay server.
pub const MCP_RELAY_PORT: u16 = 9878;

/// File where the relay port is written for discovery.
pub fn relay_port_file() -> PathBuf {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("localgpt");
    let _ = std::fs::create_dir_all(&runtime_dir);
    runtime_dir.join("gen-mcp-relay.port")
}

/// Write the relay port to the discovery file.
fn write_relay_port(port: u16) {
    let path = relay_port_file();
    if let Err(e) = std::fs::write(&path, port.to_string()) {
        tracing::warn!("Failed to write relay port file: {}", e);
    }
}

/// Read the relay port from the discovery file.
pub fn read_relay_port() -> Option<u16> {
    let path = relay_port_file();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Clean up the relay port file.
pub fn cleanup_relay_port() {
    let _ = std::fs::remove_file(relay_port_file());
}

/// Shared tool set for the relay server.
/// Tools are created once and shared across all client connections.
/// `Tool: Send + Sync` is a trait bound, so `Vec<Box<dyn Tool>>` is automatically Send+Sync.
struct RelayTools {
    tools: Vec<Box<dyn Tool>>,
}

impl RelayTools {
    fn list_schemas(&self) -> Vec<serde_json::Value> {
        self.tools
            .iter()
            .map(|t| {
                let schema = t.schema();
                serde_json::json!({
                    "name": schema.name,
                    "description": schema.description,
                    "inputSchema": schema.parameters,
                })
            })
            .collect()
    }

    async fn call_tool(&self, name: &str, arguments: &serde_json::Value) -> Result<String, String> {
        let tool = self
            .tools
            .iter()
            .find(|t| t.name() == name)
            .ok_or_else(|| format!("Unknown tool: {}", name))?;
        // Through the shared `call_tool`, so the relay refuses what the
        // stdio and HTTP servers refuse.
        call_tool(tool.as_ref(), arguments)
            .await
            .map_err(|e| e.to_string())
    }
}

/// Start the MCP relay server on a TCP port.
///
/// Creates the gen tools over the given GenBridge and serves them over TCP
/// as newline-delimited JSON-RPC 2.0 (MCP). Returns the port.
pub async fn start_mcp_relay(bridge: Arc<GenBridge>) -> anyhow::Result<u16> {
    let tools = crate::mcp_server::create_mcp_tools(bridge);

    // Try the default port, then fall back to any available port
    let listener = match TcpListener::bind(("127.0.0.1", MCP_RELAY_PORT)).await {
        Ok(l) => l,
        Err(_) => TcpListener::bind("127.0.0.1:0").await?,
    };

    let port = listener.local_addr()?.port();
    write_relay_port(port);
    tracing::info!("MCP relay listening on 127.0.0.1:{}", port);
    serve_relay(listener, tools);
    Ok(port)
}

/// The other end: MCP stdio ↔ TCP. An MCP client spawns this process as
/// `localgpt-gen mcp-server --connect` — it reads from stdin, forwards to the
/// window's relay, and writes the answers to stdout, so tool calls reach the
/// window that is open instead of a second one.
pub async fn run_stdio_relay(port: u16) -> anyhow::Result<()> {
    use tokio::net::TcpStream;

    let stream = TcpStream::connect(("127.0.0.1", port)).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to connect to the MCP relay on port {}: {}\n\
             Make sure a LocalGPT Gen window is running with --mcp-relay.",
            port,
            e
        )
    })?;

    let (tcp_reader, mut tcp_writer) = stream.into_split();
    let mut tcp_lines = BufReader::new(tcp_reader).lines();
    let mut stdin_lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    loop {
        tokio::select! {
            // stdin → TCP: the client's requests
            line = stdin_lines.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let mut buf = line;
                        buf.push('\n');
                        if tcp_writer.write_all(buf.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => break, // stdin closed
                    Err(_) => break,
                }
            }
            // TCP → stdout: the window's answers
            line = tcp_lines.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let mut buf = line;
                        buf.push('\n');
                        if stdout.write_all(buf.as_bytes()).await.is_err() {
                            break;
                        }
                        stdout.flush().await.ok();
                    }
                    Ok(None) => break, // TCP closed
                    Err(_) => break,
                }
            }
        }
    }

    Ok(())
}

fn serve_relay(listener: TcpListener, tools: Vec<Box<dyn Tool>>) {
    let relay = Arc::new(RelayTools { tools });

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    tracing::info!("MCP relay: client connected from {}", addr);
                    let relay_clone = relay.clone();
                    tokio::spawn(handle_relay_client(stream, relay_clone));
                }
                Err(e) => {
                    tracing::warn!("MCP relay accept error: {}", e);
                }
            }
        }
    });
}

/// Handle a single relay client: read JSON-RPC requests, dispatch to tools, write responses.
async fn handle_relay_client(stream: tokio::net::TcpStream, relay: Arc<RelayTools>) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    let idle_timeout = std::time::Duration::from_secs(300); // 5 min idle timeout

    loop {
        let line = match tokio::time::timeout(idle_timeout, lines.next_line()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => break, // Client closed connection
            Ok(Err(_)) => break,   // Read error
            Err(_) => {
                tracing::info!("MCP relay: client idle timeout");
                break;
            }
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err = error_response(serde_json::Value::Null, -32700, &e.to_string());
                let _ = write_response(&mut writer, &err).await;
                continue;
            }
        };

        let response = process_request(&request, &relay).await;

        // Notifications (null response) don't get a reply
        if response.is_null() {
            continue;
        }

        if write_response(&mut writer, &response).await.is_err() {
            break;
        }
    }

    tracing::info!("MCP relay: client disconnected");
}

async fn write_response(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    response: &serde_json::Value,
) -> std::io::Result<()> {
    let mut buf = match serde_json::to_string(response) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to serialize MCP response: {}", e);
            r#"{"jsonrpc":"2.0","error":{"code":-32603,"message":"Internal serialization error"}}"#
                .to_string()
        }
    };
    buf.push('\n');
    writer.write_all(buf.as_bytes()).await
}

async fn process_request(request: &serde_json::Value, relay: &RelayTools) -> serde_json::Value {
    let method = request["method"].as_str().unwrap_or("");
    let id = request
        .get("id")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    match method {
        "initialize" => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "localgpt-gen-relay",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        }),

        "notifications/initialized" | "initialized" => serde_json::Value::Null,

        "tools/list" => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": relay.list_schemas() }
        }),

        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            let args = request["params"]
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::json!({}));

            match relay.call_tool(name, &args).await {
                Ok(result) => serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": result }]
                    }
                }),
                Err(e) => serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": format!("Error: {e:#}") }],
                        "isError": true
                    }
                }),
            }
        }

        "ping" => serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": {} }),

        _ => {
            if id.is_null() {
                serde_json::Value::Null // notification
            } else {
                error_response(id, -32601, &format!("Unknown method: {}", method))
            }
        }
    }
}

fn error_response(id: serde_json::Value, code: i32, message: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen3d::create_gen_channels;

    /// The relay has its own dispatch, so it has to refuse what the stdio
    /// and HTTP servers refuse. Left to the tool, `preset` would be dropped
    /// and the call would set the default gravity, Earth's.
    #[tokio::test]
    async fn the_relay_refuses_an_argument_a_tool_does_not_take() {
        let (bridge, mut channels) = create_gen_channels();
        let relay = RelayTools {
            tools: crate::mcp_server::create_mcp_tools(bridge),
        };
        let request = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": "gen_set_gravity", "arguments": {"preset": "mars"}}
        });
        // A call that reached the tool would wait here for the scene's
        // answer, which this test never gives.
        let answer = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            process_request(&request, &relay),
        )
        .await
        .expect("a refusal does not wait for the scene");
        assert_eq!(answer["result"]["isError"], true, "{answer}");
        let said = answer["result"]["content"][0]["text"]
            .as_str()
            .expect("the refusal");
        assert!(said.contains("`preset`"), "{said}");
        assert!(said.contains("strength"), "{said}");
        assert!(
            channels.cmd_rx.try_recv().is_err(),
            "nothing may reach the scene"
        );
    }
}
