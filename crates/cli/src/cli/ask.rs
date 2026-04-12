use anyhow::Result;
use clap::Args;
use std::sync::Arc;

use futures::StreamExt;
use localgpt_core::agent::{
    Agent, AgentConfig, StreamEvent, create_spawn_agent_tool, extract_tool_detail,
};
use localgpt_core::concurrency::WorkspaceLock;
use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;
use std::io::Write;

#[derive(Args)]
pub struct AskArgs {
    /// The question or task to perform
    pub question: String,

    /// Model to use (overrides config)
    #[arg(short, long)]
    pub model: Option<String>,

    /// Output format: text (default) or json
    #[arg(short, long, default_value = "text")]
    pub format: String,
}

pub async fn run(args: AskArgs, agent_id: &str) -> Result<()> {
    let config = Config::load()?;
    let memory = Arc::new(MemoryManager::new_with_full_config(
        &config.memory,
        Some(&config),
        agent_id,
    )?);

    let agent_config = AgentConfig {
        model: args.model.unwrap_or(config.agent.default_model.clone()),
        context_window: config.agent.context_window,
        reserve_tokens: config.agent.reserve_tokens,
    };

    let mut agent = Agent::new(agent_config, &config, Arc::clone(&memory)).await?;
    agent.extend_tools(localgpt_cli_tools::create_cli_tools(&config)?);
    agent.extend_tools(vec![create_spawn_agent_tool(config.clone(), memory)]);
    agent.new_session().await?;

    let workspace_lock = WorkspaceLock::new()?;
    let _lock_guard = workspace_lock.acquire()?;

    if args.format.as_str() == "json" {
        let response = agent.chat(&args.question).await?;
        let output = serde_json::json!({
            "question": args.question,
            "response": response,
            "model": agent.model(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        let event_stream = agent
            .chat_stream_with_tools(&args.question, Vec::new())
            .await?;
        let mut pinned_stream = std::pin::pin!(event_stream);
        let mut full_response = String::new();
        let mut stdout = std::io::stdout();

        while let Some(event) = pinned_stream.next().await {
            match event {
                Ok(StreamEvent::Content(content)) => {
                    print!("{}", content);
                    let _ = stdout.flush();
                    full_response.push_str(&content);
                }
                Ok(StreamEvent::ToolCallStart {
                    name, arguments, ..
                }) => {
                    let detail = extract_tool_detail(&name, &arguments);
                    if let Some(ref d) = detail {
                        print!("\n> Running tool: {} ({}) ... ", name, d);
                    } else {
                        print!("\n> Running tool: {} ... ", name);
                    }
                    let _ = std::io::stdout().flush();
                }
                Ok(StreamEvent::ToolCallEnd { warnings, .. }) => {
                    println!("Done.");
                    let _ = std::io::stdout().flush();
                    if !warnings.is_empty() {
                        for warning in warnings {
                            eprintln!("  \u{26a0} Warning: {}", warning);
                        }
                    }
                }
                Ok(StreamEvent::Done) => {
                    // LLM text stream finished (this turn)
                }
                Ok(StreamEvent::ApprovalRequired { .. }) => {}
                Err(e) => {
                    eprintln!("\nError: {}", e);
                    break;
                }
            }
        }
        println!();
    }

    Ok(())
}
