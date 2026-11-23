//! The MCP relay's client half: an MCP client's stdio, carried to a window's
//! relay over TCP.
//!
//! A window with Gen's tools — `localgpt gen --mcp-relay`, or the desktop
//! app with its relay switched on — listens on a localhost port and writes
//! the port to [`port_file`]. An MCP client cannot start a process that is
//! already running, so it starts this bridge instead (`localgpt gen
//! mcp-server --connect`): each line read from stdin goes to the window,
//! each line the window answers comes out on stdout, and the tool calls
//! reach the window that is open rather than a second one.
//!
//! Nothing here knows a tool or a scene, which is why it lives here and not
//! with the tools: `localgpt` runs it without linking a renderer. The server
//! half, which holds the tools, is Gen's (`gen3d::mcp_relay`).
//!
//! Protocol: JSON-RPC 2.0 (MCP), one message per line, both ways.

use std::path::PathBuf;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// The port a window's relay tries first.
pub const DEFAULT_PORT: u16 = 9878;

/// Where a window's relay writes its port, for a bridge started without one
/// to find it: `<runtime>/localgpt/gen-mcp-relay.port`
/// ([`crate::paths::runtime_dir`]).
pub fn port_file() -> PathBuf {
    crate::paths::runtime_dir().join("gen-mcp-relay.port")
}

/// Say that a relay listens on `port`. The file names one window: the one
/// that wrote it last.
pub fn write_port(port: u16) -> std::io::Result<()> {
    let path = port_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, port.to_string())
}

/// The port of the relay that wrote [`port_file`] last, if any did.
pub fn read_port() -> Option<u16> {
    std::fs::read_to_string(port_file())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// A relay on `port` stopped: forget it, unless another window has written
/// its own port since.
pub fn clear_port(port: u16) {
    if read_port() == Some(port) {
        let _ = std::fs::remove_file(port_file());
    }
}

/// Bridge stdin and stdout to the relay on `port` until either side closes.
pub async fn run_stdio_bridge(port: u16) -> anyhow::Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    bridge(port, stdin, tokio::io::stdout()).await
}

/// [`run_stdio_bridge`] over any pair of streams.
async fn bridge<R, W>(port: u16, input: R, mut output: W) -> anyhow::Result<()>
where
    R: tokio::io::AsyncBufRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "no MCP relay answers on port {port} ({e}).\n\
                 Switch on the relay of a window with Gen's tools: \
                 `localgpt gen --mcp-relay`, or MCP in the desktop app's bar."
            )
        })?;
    let (tcp_reader, mut tcp_writer) = stream.into_split();
    let mut tcp_lines = BufReader::new(tcp_reader).lines();
    let mut input_lines = input.lines();
    let mut asking = true;

    loop {
        tokio::select! {
            // The client's requests, to the window.
            line = input_lines.next_line(), if asking => match line {
                Ok(Some(mut line)) => {
                    line.push('\n');
                    if tcp_writer.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                }
                // The client has asked all it will: the window hears so, and
                // its answers to what was asked still come out, until it
                // hangs up.
                _ => {
                    asking = false;
                    let _ = tcp_writer.shutdown().await;
                }
            },
            // The window's answers, to the client.
            line = tcp_lines.next_line() => {
                let Ok(Some(mut line)) = line else { break };
                line.push('\n');
                if output.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                output.flush().await.ok();
            }
        }
    }
    Ok(())
}

/// The relay's port: the one asked for, else the one [`port_file`] names.
pub fn port_or_discovered(asked: Option<u16>) -> anyhow::Result<u16> {
    asked.or_else(read_port).ok_or_else(|| {
        anyhow::anyhow!(
            "no MCP relay is running ({} names none).\n\
             Switch on the relay of a window with Gen's tools — `localgpt gen --mcp-relay`, \
             or MCP in the desktop app's bar — or pass the port: --connect <PORT>.",
            port_file().display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    /// A line in comes out at the window, the window's line comes out of the
    /// bridge, and the bridge ends when its input does.
    #[tokio::test]
    async fn the_bridge_carries_lines_both_ways() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let window = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();
            let asked = lines.next_line().await.unwrap().unwrap();
            writer
                .write_all(format!("{{\"answer\":{asked}}}\n").as_bytes())
                .await
                .unwrap();
            // The bridge's input ends next: it hangs up, and so do we.
            assert!(lines.next_line().await.unwrap().is_none());
        });

        let (mut client, bridge_in) = tokio::io::duplex(1024);
        let (bridge_out, mut seen) = tokio::io::duplex(1024);
        let running = tokio::spawn(bridge(port, BufReader::new(bridge_in), bridge_out));
        // Asked, and done asking at once, as `echo … | localgpt gen
        // mcp-server --connect` is: the answer still comes out.
        client.write_all(b"{\"id\":1}\n").await.unwrap();
        drop(client);

        let mut answer = Vec::new();
        seen.read_to_end(&mut answer).await.unwrap();
        assert_eq!(answer, b"{\"answer\":{\"id\":1}}\n");

        running.await.unwrap().unwrap();
        window.await.unwrap();
    }

    #[tokio::test]
    async fn a_port_nothing_answers_on_says_how_to_start_one() {
        // Port 1 on loopback, as headless's stale-endpoint test: nothing
        // listens there. A port freshly freed by a dropped `bind(:0)`
        // stays in the ephemeral range the OS keeps handing to new
        // listeners, so on a busy machine — a workspace test run —
        // another binder can be given it before the connect, and the
        // bridge then connects where nothing was meant to answer.
        let port = 1u16;
        let (_client, input) = tokio::io::duplex(64);
        let err = bridge(port, BufReader::new(input), tokio::io::sink())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains(&format!("port {port}")), "{err}");
        assert!(err.contains("--mcp-relay"), "{err}");
    }

    #[test]
    fn an_asked_port_wins_over_the_file() {
        assert_eq!(port_or_discovered(Some(4242)).unwrap(), 4242);
    }
}
