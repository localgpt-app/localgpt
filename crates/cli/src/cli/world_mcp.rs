//! `localgpt world mcp` — an MCP server for the world an app has open.
//!
//! The live-editing design puts the agent outside the window: a world is a
//! folder, an app holds it open and serves a localhost API, and agents change
//! the world by sending batches of ops to that API
//! (`docs/rfcs/authoring/live-editing-poc.md`). Any agent that speaks MCP can
//! do that through this shim, which is why it is the cheapest way to make
//! Claude Code or Codex a first-class editor of a live world: no terminal
//! emulator, no pane, no new protocol.
//!
//! The shim is deliberately thin. It owns no world state and no format
//! semantics: it reads `.live/endpoint.json` to find the open app, and every
//! tool is one HTTP call to it. The authority stays the app — it ingests the
//! batch strictly, binds names to ids, commits whole or refuses with a reason
//! per op, and writes `manifest.json`. A refusal comes back to the agent
//! verbatim, because that is what lets it correct itself.
//!
//! Started by the agent, as MCP servers are, so it inherits nothing: the
//! world folder comes from the argument or the working directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use localgpt_core::agent::ToolSchema;
use localgpt_core::agent::tools::Tool;
use localgpt_world_agent::agent_cli::Endpoint;
use serde_json::{Value, json};

/// How long to wait on the open app before giving up. Localhost, and a call
/// that applies ops to a live scene is still interactive work.
const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// Serve the world in `dir` (or the working directory) over stdio MCP.
pub async fn run(dir: Option<PathBuf>) -> Result<()> {
    let dir = match dir {
        Some(dir) => dir,
        None => {
            std::env::current_dir().context("no world folder given and no working directory")?
        }
    };
    // Fail at startup rather than per call: an agent that gets a server with
    // no world behind it will keep trying tools that cannot work.
    let endpoint = Endpoint::read(&dir).map_err(|e| {
        anyhow::anyhow!(
            "no open world at {} ({e})\n\
             Open it first: localgpt-app --world {}",
            dir.display(),
            dir.display()
        )
    })?;

    let api = Api::new(endpoint, dir);
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(SubmitTool(api.clone())),
        Box::new(UndoTool(api.clone())),
        Box::new(LogTool(api.clone())),
        Box::new(TipsTool(api.clone())),
        Box::new(GotoTool(api.clone())),
        Box::new(VerifyTool(api.clone())),
        Box::new(ScreenshotTool(api.clone())),
        Box::new(SelectionTool(api)),
    ];
    localgpt_core::mcp::server::run_mcp_stdio_server(tools, "localgpt-world").await
}

/// The open app's API.
#[derive(Clone)]
struct Api {
    url: String,
    token: String,
    /// The world's folder, so a tool can point an agent at its guide.
    dir: PathBuf,
    http: reqwest::Client,
}

impl Api {
    fn new(endpoint: Endpoint, dir: PathBuf) -> Self {
        Self {
            url: endpoint.url.trim_end_matches('/').to_string(),
            token: endpoint.token,
            dir,
            http: reqwest::Client::new(),
        }
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<String> {
        let mut req = self
            .http
            .request(method, format!("{}{path}", self.url))
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
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if status.is_success() {
            Ok(text)
        } else {
            // A refusal is the point, not an error to swallow: it carries a
            // reason per op so the agent can fix the batch and retry.
            Err(anyhow::anyhow!("{text}"))
        }
    }
}

/// The guide an agent should read first, named in every tool's description so
/// it is discoverable without a prompt telling it to look.
fn guide_hint(dir: &Path) -> String {
    format!("{}/AGENTS.md describes this world's ops.", dir.display())
}

macro_rules! simple_tool {
    ($name:ident, $tool:literal, $desc:expr, $method:ident, $path:literal) => {
        struct $name(Api);

        #[async_trait]
        impl Tool for $name {
            fn name(&self) -> &str {
                $tool
            }
            fn schema(&self) -> ToolSchema {
                ToolSchema {
                    name: $tool.to_string(),
                    description: $desc.to_string(),
                    parameters: json!({"type": "object", "properties": {}}),
                }
            }
            async fn execute(&self, _arguments: &str) -> Result<String> {
                self.0.call(reqwest::Method::$method, $path, None).await
            }
        }
    };
}

simple_tool!(
    LogTool,
    "world_log",
    "Every batch committed to this world, oldest first: its id, the entry it builds on (parent), revision, author, message and changes — and which entry is the head.",
    GET,
    "/log"
);
simple_tool!(
    TipsTool,
    "world_tips",
    "Where this world's history ends: every branch tip, which one is the head (what manifest.json holds, where a batch lands by default), and which entry the window is showing (`current`). Pass an id from here as world_submit's `at` to build on it.",
    GET,
    "/tips"
);
simple_tool!(
    VerifyTool,
    "world_verify",
    "Check this world's integrity: that folding the log over the base equals manifest.json, and every asset matches its hash.",
    GET,
    "/verify"
);
simple_tool!(
    ScreenshotTool,
    "world_screenshot",
    "Take a screenshot of the open world and return the path to the PNG. Use it to see what an edit actually did.",
    GET,
    "/screenshot"
);
simple_tool!(
    SelectionTool,
    "world_selection",
    "What the person has selected in the app right now — the entities they are looking at.",
    GET,
    "/selection"
);

struct SubmitTool(Api);

#[async_trait]
impl Tool for SubmitTool {
    fn name(&self) -> &str {
        "world_submit"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "world_submit".to_string(),
            description: format!(
                "Change the open world by committing a batch of edit ops. \
                 The batch applies whole or not at all: if any op is refused you get a \
                 reason per op and the world is untouched, so fix the batch and send it \
                 again. Entity names may be used where ids go, and a new entity may omit \
                 its id to be given one. Unknown fields are refused rather than ignored. {}",
                guide_hint(&self.0.dir)
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "ops": {
                        "type": "array",
                        "description": "The edit ops, e.g. [{\"SpawnEntity\": {\"entity\": {\"name\": \"lamp\"}}}]",
                        "items": {"type": "object"}
                    },
                    "message": {
                        "type": "string",
                        "description": "What this batch is for — a commit message, kept in the world's history"
                    },
                    "author": {
                        "type": "string",
                        "description": "Who is making the change. Defaults to the agent's own name."
                    },
                    "at": {
                        "type": "string",
                        "description": "The id of the entry to build on (from world_tips or world_log); leave it out to build on the head. An earlier entry starts a branch and leaves the head as it was; the reply's `id` is the branch's new tip, to pass as `at` next time."
                    }
                },
                "required": ["ops"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("the arguments aren't JSON: {e}"))?;
        let ops = args
            .get("ops")
            .filter(|ops| ops.is_array())
            .ok_or_else(|| anyhow::anyhow!("`ops` must be an array of edit ops"))?
            .clone();
        let mut body = json!({"ops": ops});
        // The author rides in the batch, as the API expects; the app decides
        // what to do with it and the git commit carries it.
        for key in ["author", "message", "at"] {
            if let Some(value) = args.get(key).and_then(Value::as_str) {
                body[key] = json!(value);
            }
        }
        self.0.call(reqwest::Method::POST, "/ops", Some(body)).await
    }
}

struct UndoTool(Api);

#[async_trait]
impl Tool for UndoTool {
    fn name(&self) -> &str {
        "world_undo"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "world_undo".to_string(),
            description: "Take back the head's newest edit by appending its inverse. \
                 The log never rewinds, so an undo is itself a committed batch."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "author": {
                        "type": "string",
                        "description": "Who is undoing — recorded as the undo's author."
                    },
                    "at": {
                        "type": "string",
                        "description": "Undo on the path to this entry (from world_tips) instead of the head's. Off main the undo is a branch commit; the head stays."
                    }
                }
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments).unwrap_or(json!({}));
        let mut body = json!({});
        for key in ["author", "at"] {
            if let Some(value) = args.get(key).and_then(Value::as_str) {
                body[key] = json!(value);
            }
        }
        self.0
            .call(reqwest::Method::POST, "/undo", Some(body))
            .await
    }
}

struct GotoTool(Api);

#[async_trait]
impl Tool for GotoTool {
    fn name(&self) -> &str {
        "world_goto"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "world_goto".to_string(),
            description: "Show an entry of this world's history in the app's window — the person \
                 sees it. Nothing is written: this moves the view, not the world. Use it to \
                 show a branch you made, or to look at an earlier state before a screenshot."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tip": {
                        "type": ["string", "null"],
                        "description": "The entry's id (from world_tips or world_log), or null for the world before any entry."
                    }
                },
                "required": ["tip"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)
            .map_err(|e| anyhow::anyhow!("the arguments aren't JSON: {e}"))?;
        let tip = match args.get("tip") {
            Some(tip @ (Value::String(_) | Value::Null)) => tip.clone(),
            _ => anyhow::bail!("`tip` is an entry id, or null for the base"),
        };
        self.0
            .call(reqwest::Method::POST, "/goto", Some(json!({"tip": tip})))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api() -> Api {
        Api::new(
            Endpoint {
                url: "http://127.0.0.1:51734/".into(),
                token: "t".into(),
                pid: None,
            },
            PathBuf::from("/worlds/harbour.world"),
        )
    }

    #[test]
    fn a_trailing_slash_in_the_endpoint_does_not_double_up() {
        assert_eq!(api().url, "http://127.0.0.1:51734");
    }

    #[test]
    fn every_tool_is_named_and_described_for_an_agent_that_has_not_been_told_about_worlds() {
        let tools: Vec<Box<dyn Tool>> = vec![
            Box::new(SubmitTool(api())),
            Box::new(UndoTool(api())),
            Box::new(LogTool(api())),
            Box::new(TipsTool(api())),
            Box::new(GotoTool(api())),
            Box::new(VerifyTool(api())),
            Box::new(ScreenshotTool(api())),
            Box::new(SelectionTool(api())),
        ];
        let mut names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "world_goto",
                "world_log",
                "world_screenshot",
                "world_selection",
                "world_submit",
                "world_tips",
                "world_undo",
                "world_verify"
            ]
        );
        for tool in &tools {
            let schema = tool.schema();
            assert_eq!(schema.name, tool.name());
            assert!(
                schema.description.len() > 40,
                "{} needs a description an agent can act on",
                tool.name()
            );
            assert!(schema.parameters.get("type").is_some());
        }
    }

    #[tokio::test]
    async fn submit_requires_ops_and_says_so_rather_than_sending_nothing() {
        let tool = SubmitTool(api());
        for bad in [
            r#"{}"#,
            r#"{"ops": "not an array"}"#,
            r#"{"message": "hi"}"#,
        ] {
            let err = tool.execute(bad).await.expect_err(bad);
            assert!(err.to_string().contains("`ops`"), "{bad} -> {err}");
        }
        let err = tool.execute("not json").await.expect_err("not json");
        assert!(err.to_string().contains("aren't JSON"), "{err}");
    }

    #[tokio::test]
    async fn a_world_nothing_has_open_fails_at_startup_not_per_call() {
        // An agent handed a server with no world behind it would keep trying
        // tools that cannot work, so this refuses up front and says how to
        // open the world.
        let err = run(Some(PathBuf::from("/definitely/not/a/world")))
            .await
            .expect_err("no open world");
        let text = err.to_string();
        assert!(text.contains("no open world"), "{text}");
        assert!(
            text.contains("localgpt-app --world"),
            "it should say how to open one: {text}"
        );
    }

    #[test]
    fn the_submit_tool_points_an_agent_at_the_worlds_guide() {
        // An agent that has never heard of this format should be able to find
        // the rules from the tool description alone.
        let description = SubmitTool(api()).schema().description;
        assert!(
            description.contains("/worlds/harbour.world/AGENTS.md"),
            "{description}"
        );
    }
}
