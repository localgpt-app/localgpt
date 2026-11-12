//! The models' MCP tools, offered only for the models that are installed.
//!
//! `localgpt world mcp` adds them beside the world's own tools, writing what
//! they make into the world's `assets/`, so an op can reference it; `localgpt
//! audio mcp` serves them alone, writing into the directory it was started
//! in. The models run on a blocking thread, so a long separation never stalls
//! the server.

use std::path::{Path, PathBuf};

use localgpt_world_agent::mcp::Tool;

use crate::catalog::MODELS;
use crate::install::installed as is_installed;

/// What the tools work against.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Where the models are installed ([`crate::models_dir`]).
    pub models: PathBuf,
    /// Relative input paths resolve here: the world's folder, or the working
    /// directory.
    pub base: PathBuf,
    /// Where what a tool makes is written (a world's `assets/`). Paths in a
    /// tool's answer are relative to it, as a world's ops name assets.
    pub out: PathBuf,
}

impl ToolContext {
    /// An input path an agent gave, resolved against [`Self::base`].
    pub fn input(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base.join(path)
        }
    }

    /// `path` as the answer names it: relative to [`Self::out`] when inside it.
    pub fn named(&self, path: &Path) -> String {
        path.strip_prefix(&self.out)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }
}

/// The tools of every installed model.
pub fn installed(context: &ToolContext) -> Vec<Box<dyn Tool>> {
    MODELS
        .iter()
        .filter(|model| is_installed(&context.models, model))
        .filter_map(|model| tool_for(model.id, context))
        .collect()
}

/// The tool of the model with this id.
fn tool_for(id: &str, context: &ToolContext) -> Option<Box<dyn Tool>> {
    match id {
        "notes" => Some(Box::new(crate::notes::tool::NotesTool::new(
            context.clone(),
        ))),
        "tags" => Some(Box::new(crate::tags::TagsTool::new(context))),
        _ => None,
    }
}

/// Run a model's blocking work off the async server.
#[allow(dead_code)] // every model's tool calls it
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(work).await?
}
