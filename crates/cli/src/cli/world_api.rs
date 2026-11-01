//! The open app's API, as `localgpt world` and its MCP shim call it.
//!
//! An app holding a world serves a localhost API and advertises it in the
//! folder's `.live/endpoint.json`: the URL, and the bearer token every call
//! carries (`crates/gen/src/gen3d/live.rs`). This is the CLI's one client of
//! it, so the commands and the shim send the same calls the same way — and
//! the token is read from the file here, never typed into a command line.

use std::path::{Path, PathBuf};

use anyhow::Result;
use localgpt_world_agent::agent_env::Endpoint;
use localgpt_world_agent::headless;
use serde_json::Value;

/// How long to wait on the open app before giving up. Localhost, and a call
/// that applies ops to a live scene is still interactive work.
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The open app's API.
#[derive(Clone)]
pub struct Api {
    pub(crate) url: String,
    token: String,
    /// The world's folder, so a tool can point an agent at its guide.
    dir: PathBuf,
    http: reqwest::Client,
}

/// What the app answered: its status, and its reply as JSON (a reply that
/// isn't JSON arrives as a string).
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub status: u16,
    pub body: Value,
}

impl Answer {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

impl Api {
    pub fn new(endpoint: Endpoint, dir: PathBuf) -> Self {
        Self {
            url: endpoint.url.trim_end_matches('/').to_string(),
            token: endpoint.token,
            dir,
            http: reqwest::Client::new(),
        }
    }

    /// The app holding `dir`, when one answers there. `None` when no app has
    /// it open — or the one that did is gone, whose endpoint file `holder`
    /// clears, so a crash never locks a world.
    pub fn holding(dir: &Path) -> Option<Self> {
        headless::holder(dir)?;
        let endpoint = Endpoint::read(dir).ok()?;
        Some(Self::new(endpoint, dir.to_path_buf()))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> Result<(u16, String)> {
        let mut url = format!("{}{path}", self.url);
        for (n, (key, value)) in query.iter().enumerate() {
            url += &format!("{}{key}={}", if n == 0 { '?' } else { '&' }, escaped(value));
        }
        let mut req = self
            .http
            .request(method, url)
            .bearer_auth(&self.token)
            .timeout(CALL_TIMEOUT);
        if let Some(body) = body {
            req = req.json(&body);
        }
        let response = req.send().await.map_err(|e| {
            // The app closing is the common case, and "connection refused" is
            // not a useful thing to hand a model.
            anyhow::anyhow!("the app holding this world didn't answer ({e}) — is it still open?")
        })?;
        let status = response.status().as_u16();
        Ok((status, response.text().await.unwrap_or_default()))
    }

    /// The reply's text, or a refusal's as the error — the shim's shape,
    /// since a tool result is text and a refusal must reach the agent
    /// verbatim: it carries a reason per op, so the agent can fix the batch.
    pub async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<String> {
        let (status, text) = self.send(method, path, &[], body).await?;
        if (200..300).contains(&status) {
            Ok(text)
        } else {
            Err(anyhow::anyhow!("{text}"))
        }
    }

    /// The reply as JSON, whatever its status — the commands' shape, since
    /// they tell a proposal (202) from a commit and a refusal from a busy
    /// canvas.
    pub async fn ask(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, &str)],
        body: Option<Value>,
    ) -> Result<Answer> {
        let (status, text) = self.send(method, path, query, body).await?;
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        Ok(Answer { status, body })
    }

    pub async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Answer> {
        self.ask(reqwest::Method::GET, path, query, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Answer> {
        self.ask(reqwest::Method::POST, path, &[], Some(body)).await
    }
}

/// A query value as a URL carries it: an entry id's `sha256:` stays as it
/// is, and the app decodes anything escaped.
fn escaped(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b':' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_value_is_escaped_but_an_id_reads_as_itself() {
        assert_eq!(escaped("sha256:0a1b"), "sha256:0a1b");
        assert_eq!(escaped("a b&c"), "a%20b%26c");
    }

    #[test]
    fn a_trailing_slash_in_the_endpoint_does_not_double_up() {
        let api = Api::new(
            Endpoint {
                url: "http://127.0.0.1:51734/".into(),
                token: "t".into(),
                pid: None,
            },
            PathBuf::from("/worlds/harbour.world"),
        );
        assert_eq!(api.url, "http://127.0.0.1:51734");
    }

    #[test]
    fn a_folder_no_app_holds_has_no_api() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Api::holding(dir.path()).is_none());
    }
}
