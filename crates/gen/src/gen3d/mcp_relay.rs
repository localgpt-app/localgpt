//! MCP relay server — lets an external MCP client use a window's GenBridge.
//!
//! A window with Gen's tools — `localgpt gen --mcp-relay`, or the desktop
//! app with MCP switched on in its bar — listens on a localhost port and
//! writes the port where a client finds it. A separate `localgpt gen
//! mcp-server --connect` process, the MCP client's, carries its stdio to
//! that port (`localgpt_world_agent::mcp::relay`, which `localgpt` runs
//! without linking Gen), so tool calls go to the window that is open instead
//! of spawning a new one.
//!
//! Protocol: JSON-RPC 2.0 (MCP) over newline-delimited TCP.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

use localgpt_world_agent::mcp::relay;
use localgpt_world_agent::mcp::{Tool, error_result, tool_result};

use crate::gen3d::GenBridge;

/// Default port for the MCP relay server.
pub const MCP_RELAY_PORT: u16 = relay::DEFAULT_PORT;

/// A relay serving a window's tools. Dropping it stops the relay — the
/// listener and every client's connection — and forgets its port.
pub struct McpRelay {
    port: u16,
    task: tokio::task::JoinHandle<()>,
}

impl McpRelay {
    /// The localhost port it listens on.
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for McpRelay {
    fn drop(&mut self) {
        self.task.abort();
        relay::clear_port(self.port);
    }
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

    /// The `result` of a `tools/call`. Through the shared `tool_result`, so
    /// the relay refuses what the stdio and HTTP servers refuse and hands
    /// back a picture the way they do.
    async fn call_tool(&self, name: &str, arguments: &serde_json::Value) -> serde_json::Value {
        match self.tools.iter().find(|t| t.name() == name) {
            Some(tool) => tool_result(tool.as_ref(), arguments).await,
            None => error_result(&format!("Unknown tool: {name}")),
        }
    }
}

/// Start the MCP relay server on a TCP port, on the current tokio runtime.
///
/// Creates the gen tools over the given GenBridge and serves them over TCP
/// as newline-delimited JSON-RPC 2.0 (MCP), on [`MCP_RELAY_PORT`] or, when
/// that is taken, any free port. It serves until the handle is dropped.
pub async fn start_mcp_relay(bridge: Arc<GenBridge>) -> anyhow::Result<McpRelay> {
    let tools = crate::mcp_server::create_mcp_tools(bridge);

    // Try the default port, then fall back to any available port
    let listener = match TcpListener::bind(("127.0.0.1", MCP_RELAY_PORT)).await {
        Ok(l) => l,
        Err(_) => TcpListener::bind("127.0.0.1:0").await?,
    };

    let port = listener.local_addr()?.port();
    if let Err(e) = relay::write_port(port) {
        tracing::warn!("Failed to write relay port file: {}", e);
    }
    tracing::info!("MCP relay listening on 127.0.0.1:{}", port);
    let task = tokio::spawn(serve_relay(listener, tools));
    Ok(McpRelay { port, task })
}

/// Accept clients until the relay is stopped. Each client's task is in a set
/// this future owns, so stopping it (aborting its task) ends them too.
async fn serve_relay(listener: TcpListener, tools: Vec<Box<dyn Tool>>) {
    let relay = Arc::new(RelayTools { tools });
    let mut clients = tokio::task::JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, addr)) => {
                    tracing::info!("MCP relay: client connected from {}", addr);
                    clients.spawn(handle_relay_client(stream, relay.clone()));
                }
                Err(e) => {
                    tracing::warn!("MCP relay accept error: {}", e);
                }
            },
            // Clients that left, so the set holds only the connected.
            Some(_) = clients.join_next(), if !clients.is_empty() => {}
        }
    }
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

            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": relay.call_tool(name, &args).await,
            })
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

    /// The desktop app switches its relay off by dropping the handle: the
    /// listener goes, and so does a client that was connected.
    #[tokio::test]
    async fn dropping_the_handle_ends_the_listener_and_its_clients() {
        use std::time::Duration;
        use tokio::io::AsyncReadExt;

        // Its own listener, not `start_mcp_relay`: a test must not write the
        // port file a window on this machine may be using.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (bridge, _channels) = create_gen_channels();
        let tools = crate::mcp_server::create_mcp_tools(bridge);
        let relay = McpRelay {
            port,
            task: tokio::spawn(serve_relay(listener, tools)),
        };
        assert_eq!(relay.port(), port);

        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        client
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n")
            .await
            .unwrap();
        let mut answer = [0u8; 256];
        let n = tokio::time::timeout(Duration::from_secs(5), client.read(&mut answer))
            .await
            .expect("a ping is answered")
            .unwrap();
        assert!(String::from_utf8_lossy(&answer[..n]).contains("\"id\":1"));

        drop(relay);
        let end = tokio::time::timeout(Duration::from_secs(5), client.read(&mut answer))
            .await
            .expect("the connection ends");
        assert!(matches!(end, Ok(0) | Err(_)), "{end:?}");
        assert!(
            tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_err(),
            "nothing listens"
        );
    }
}
