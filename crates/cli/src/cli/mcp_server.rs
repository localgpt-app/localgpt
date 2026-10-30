//! CLI subcommand: `localgpt mcp-server`
//!
//! Runs LocalGPT as an MCP server over stdio, exposing memory and web tools
//! to external AI backends (Claude CLI, Gemini CLI, Codex, VS Code, Zed, etc.).

use anyhow::Result;
use clap::Args;
use std::sync::Arc;

use localgpt_core::config::Config;
use localgpt_core::mcp::memory_tools::create_memory_write_tools;
use localgpt_core::mcp::server::run_mcp_stdio_server;

#[derive(Args)]
pub struct McpServerArgs {
    /// Agent ID for memory indexing
    #[arg(short, long, default_value = "mcp")]
    agent: String,
}

pub async fn run(args: McpServerArgs) -> Result<()> {
    let config = Config::load()?;
    let workspace = config.workspace_path();

    // Read-side tools: memory_search, memory_get, web_fetch, web_search
    let memory = localgpt_core::memory::MemoryManager::new_with_agent(&config.memory, &args.agent)?;
    let memory = Arc::new(memory);
    let mut tools = localgpt_core::agent::tools::create_safe_tools(&config, Some(memory))?;

    // Write-side tools: memory_save, memory_log
    tools.extend(create_memory_write_tools(workspace));

    run_mcp_stdio_server(tools, "localgpt").await
}
