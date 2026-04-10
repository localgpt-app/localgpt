//! Job execution: runs a prompt in a fresh agent session.

use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use crate::agent::{Agent, AgentConfig, filter_silent_reply};
use crate::config::Config;
use crate::memory::MemoryManager;

/// Execute a cron job by running the prompt in a fresh agent session.
/// Returns the agent's text response.
///
/// `mcp_server_filter`: optional allowlist of MCP server names (empty = all).
pub async fn run_job(
    config: &Config,
    job_name: &str,
    prompt: &str,
    extra_tools: Option<Vec<Box<dyn crate::agent::Tool>>>,
    mcp_server_filter: &[String],
) -> Result<String> {
    let agent_id = format!("cron-{}", job_name);
    info!("Cron job '{}' starting (agent: {})", job_name, agent_id);

    let memory = MemoryManager::new_with_full_config(&config.memory, Some(config), &agent_id)?;
    let memory = Arc::new(memory);

    let agent_config = AgentConfig {
        model: config.agent.default_model.clone(),
        context_window: config.agent.context_window,
        reserve_tokens: config.agent.reserve_tokens,
    };

    // Apply MCP server filter if specified
    let config = if !mcp_server_filter.is_empty() {
        let mut filtered = config.clone();
        filtered.mcp.servers = config.mcp.filter_servers(mcp_server_filter);
        info!(
            "Cron job '{}': filtered MCP servers to {} of {}",
            job_name,
            filtered.mcp.servers.len(),
            config.mcp.servers.len()
        );
        filtered
    } else {
        config.clone()
    };

    let mut agent = Agent::new(agent_config, &config, memory).await?;

    if let Some(tools) = extra_tools {
        agent.extend_tools(tools);
    }

    let response = agent.chat(prompt).await?;
    let response = filter_silent_reply(response);

    info!(
        "Cron job '{}' finished ({} chars)",
        job_name,
        response.len()
    );
    Ok(response)
}
