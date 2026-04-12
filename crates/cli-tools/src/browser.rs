//! Browser automation tool via Chrome DevTools Protocol (CDP).
//!
//! Uses headless Chrome with direct WebSocket JSON-RPC — no external CDP crate.
//! Chrome is lazily launched on first tool invocation and reused for subsequent calls.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tracing::debug;

use localgpt_core::agent::providers::ToolSchema;
use localgpt_core::agent::tools::Tool;

/// Default timeout for CDP commands (30 seconds).
const CDP_TIMEOUT: Duration = Duration::from_secs(30);

/// Browser tool — provides navigate, screenshot, and text extraction.
pub struct BrowserTool {
    port: u16,
    state: Arc<Mutex<BrowserState>>,
}

enum BrowserState {
    NotStarted,
    Running {
        _process: tokio::process::Child, // kept alive for kill_on_drop
        ws_url: String,
    },
}

impl BrowserTool {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            state: Arc::new(Mutex::new(BrowserState::NotStarted)),
        }
    }

    /// Ensure Chrome is running and return the WebSocket debug URL.
    async fn ensure_browser(&self) -> Result<String> {
        let mut state = self.state.lock().await;
        match &*state {
            BrowserState::Running { ws_url, .. } => Ok(ws_url.clone()),
            BrowserState::NotStarted => {
                let chrome_path = detect_chrome()?;
                debug!("Launching headless Chrome: {}", chrome_path);

                let user_data = std::env::temp_dir().join("localgpt-chrome");
                let process = tokio::process::Command::new(&chrome_path)
                    .args([
                        "--headless=new",
                        "--disable-gpu",
                        "--no-sandbox",
                        "--disable-dev-shm-usage",
                        &format!("--remote-debugging-port={}", self.port),
                        &format!("--user-data-dir={}", user_data.display()),
                        "--no-first-run",
                        "--no-default-browser-check",
                        "about:blank",
                    ])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .spawn()
                    .context("Failed to launch Chrome")?;

                // Wait for Chrome to start accepting connections
                let ws_url = wait_for_chrome(self.port).await?;
                debug!("Chrome ready at {}", ws_url);

                *state = BrowserState::Running {
                    _process: process,
                    ws_url: ws_url.clone(),
                };
                Ok(ws_url)
            }
        }
    }

    async fn do_navigate(&self, url: &str) -> Result<String> {
        let ws_url = self.ensure_browser().await?;
        let mut cdp = CdpConnection::connect(&ws_url).await?;

        cdp.send("Page.enable", json!({})).await?;
        cdp.send("Page.navigate", json!({"url": url})).await?;

        // Wait for page load
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Extract page content
        let result = cdp
            .send(
                "Runtime.evaluate",
                json!({"expression": "document.title + '\\n\\n' + document.body?.innerText?.substring(0, 10000) || ''"}),
            )
            .await?;

        let text = result["result"]["value"]
            .as_str()
            .unwrap_or("(empty page)")
            .to_string();

        Ok(format!("URL: {}\n\n{}", url, text))
    }

    async fn do_screenshot(&self, url: Option<&str>) -> Result<String> {
        let ws_url = self.ensure_browser().await?;
        let mut cdp = CdpConnection::connect(&ws_url).await?;

        if let Some(url) = url {
            cdp.send("Page.enable", json!({})).await?;
            cdp.send("Page.navigate", json!({"url": url})).await?;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }

        let result = cdp
            .send("Page.captureScreenshot", json!({"format": "png"}))
            .await?;

        let data = result["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No screenshot data returned"))?;

        Ok(format!("Screenshot captured ({} bytes base64)", data.len()))
    }

    async fn do_text(&self, selector: Option<&str>) -> Result<String> {
        let ws_url = self.ensure_browser().await?;
        let mut cdp = CdpConnection::connect(&ws_url).await?;

        let expr = if let Some(sel) = selector {
            format!(
                "document.querySelector('{}')?.innerText || '(element not found)'",
                sel.replace('\'', "\\'")
            )
        } else {
            "document.body?.innerText || '(empty)'".to_string()
        };

        let result = cdp
            .send("Runtime.evaluate", json!({"expression": expr}))
            .await?;

        Ok(result["result"]["value"]
            .as_str()
            .unwrap_or("(no text)")
            .to_string())
    }

    async fn do_click(&self, selector: &str) -> Result<String> {
        let ws_url = self.ensure_browser().await?;
        let mut cdp = CdpConnection::connect(&ws_url).await?;

        let expr = format!(
            "(() => {{ const el = document.querySelector('{}'); if (el) {{ el.click(); return 'clicked'; }} return 'element not found'; }})()",
            selector.replace('\'', "\\'")
        );

        let result = cdp
            .send("Runtime.evaluate", json!({"expression": expr}))
            .await?;

        Ok(result["result"]["value"]
            .as_str()
            .unwrap_or("unknown")
            .to_string())
    }
}

#[async_trait]
impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn permission_level(&self) -> localgpt_core::agent::tools::PermissionLevel {
        localgpt_core::agent::tools::PermissionLevel::Elevated
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "browser".to_string(),
            description: "Interact with web pages via headless Chrome. Actions: navigate (load URL and get text), screenshot (capture page as PNG), text (extract text by CSS selector), click (click element by selector).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["navigate", "screenshot", "text", "click"],
                        "description": "Action to perform"
                    },
                    "url": {
                        "type": "string",
                        "description": "URL to navigate to (required for navigate/screenshot)"
                    },
                    "selector": {
                        "type": "string",
                        "description": "CSS selector (for text/click actions)"
                    }
                },
                "required": ["action"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let action = args["action"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing action"))?;

        match action {
            "navigate" => {
                let url = args["url"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("navigate requires 'url'"))?;
                self.do_navigate(url).await
            }
            "screenshot" => {
                let url = args["url"].as_str();
                self.do_screenshot(url).await
            }
            "text" => {
                let selector = args["selector"].as_str();
                self.do_text(selector).await
            }
            "click" => {
                let selector = args["selector"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("click requires 'selector'"))?;
                self.do_click(selector).await
            }
            other => bail!(
                "Unknown browser action: '{}'. Use: navigate, screenshot, text, click",
                other
            ),
        }
    }
}

impl Drop for BrowserTool {
    fn drop(&mut self) {
        // kill_on_drop handles Chrome cleanup
    }
}

// ── CDP WebSocket connection ──

struct CdpConnection {
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    next_id: u32,
}

impl CdpConnection {
    async fn connect(ws_url: &str) -> Result<Self> {
        let (ws, _) = tokio::time::timeout(CDP_TIMEOUT, tokio_tungstenite::connect_async(ws_url))
            .await
            .map_err(|_| anyhow::anyhow!("CDP WebSocket connection timed out"))??;

        Ok(Self { ws, next_id: 1 })
    }

    async fn send(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let msg = json!({
            "id": id,
            "method": method,
            "params": params
        });

        self.ws
            .send(Message::Text(msg.to_string().into()))
            .await
            .context("Failed to send CDP command")?;

        // Read responses until we get our ID
        loop {
            let response = tokio::time::timeout(CDP_TIMEOUT, self.ws.next())
                .await
                .map_err(|_| anyhow::anyhow!("CDP response timed out for {}", method))?
                .ok_or_else(|| anyhow::anyhow!("CDP WebSocket closed"))??;

            if let Message::Text(text) = response {
                let parsed: Value = serde_json::from_str(text.as_ref())?;
                if parsed["id"].as_u64() == Some(id as u64) {
                    if let Some(error) = parsed.get("error") {
                        bail!("CDP error for {}: {}", method, error);
                    }
                    return Ok(parsed["result"].clone());
                }
                // Skip events (no matching id)
            }
        }
    }
}

// ── Chrome detection and startup ──

/// Detect Chrome/Chromium binary path.
pub fn detect_chrome() -> Result<String> {
    let candidates = if cfg!(target_os = "macos") {
        vec![
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
    } else {
        vec![]
    };

    // Check well-known paths first
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }

    // Try PATH
    for name in &["google-chrome", "chromium", "chromium-browser", "chrome"] {
        if which::which(name).is_ok() {
            return Ok(name.to_string());
        }
    }

    bail!(
        "Chrome/Chromium not found. Install Google Chrome or Chromium.\n\
         Searched: {:?} + PATH lookup for google-chrome, chromium, chrome",
        candidates
    )
}

/// Wait for Chrome to start accepting CDP connections.
async fn wait_for_chrome(port: u16) -> Result<String> {
    let url = format!("http://127.0.0.1:{}/json/version", port);
    let client = reqwest::Client::new();

    for attempt in 0..20 {
        tokio::time::sleep(Duration::from_millis(250)).await;

        if let Ok(resp) = client.get(&url).send().await
            && let Ok(info) = resp.json::<Value>().await
            && let Some(ws_url) = info["webSocketDebuggerUrl"].as_str()
        {
            return Ok(ws_url.to_string());
        }

        if attempt > 0 && attempt % 5 == 0 {
            debug!("Waiting for Chrome to start (attempt {})", attempt);
        }
    }

    bail!("Chrome failed to start within 5 seconds on port {}", port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_tool_schema() {
        let tool = BrowserTool::new(9222);
        assert_eq!(tool.name(), "browser");
        let schema = tool.schema();
        assert_eq!(schema.name, "browser");
        let params = &schema.parameters;
        assert!(params["properties"]["action"].is_object());
        assert!(params["properties"]["url"].is_object());
        assert!(params["properties"]["selector"].is_object());
        assert_eq!(params["required"][0], "action");
    }

    #[test]
    fn test_detect_chrome_path() {
        // This test validates detect_chrome doesn't panic
        // It may succeed or fail depending on Chrome installation
        let result = detect_chrome();
        if let Ok(path) = &result {
            assert!(!path.is_empty());
        }
        // Both Ok and Err are valid — we just check it doesn't crash
    }

    #[tokio::test]
    async fn test_browser_missing_action() {
        let tool = BrowserTool::new(19222); // Use unlikely port
        let result = tool.execute(r#"{"url": "https://example.com"}"#).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Missing action"));
    }

    #[tokio::test]
    async fn test_browser_unknown_action() {
        let tool = BrowserTool::new(19222);
        let result = tool.execute(r#"{"action": "fly"}"#).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Unknown browser action")
        );
    }

    #[tokio::test]
    async fn test_browser_navigate_missing_url() {
        let tool = BrowserTool::new(19222);
        let result = tool.execute(r#"{"action": "navigate"}"#).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("requires 'url'"));
    }

    #[tokio::test]
    async fn test_browser_click_missing_selector() {
        let tool = BrowserTool::new(19222);
        let result = tool.execute(r#"{"action": "click"}"#).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("requires 'selector'")
        );
    }
}
