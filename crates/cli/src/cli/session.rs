//! Session management CLI commands

use anyhow::Result;
use clap::{Args, Subcommand};

use localgpt_core::agent::checkpoint::{CheckpointManager, format_checkpoint_time};
use localgpt_core::agent::{Session, get_sessions_dir_for_agent, list_sessions_for_agent};

const DEFAULT_AGENT_ID: &str = "main";

#[derive(Args)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub command: SessionCommands,
}

#[derive(Subcommand)]
pub enum SessionCommands {
    /// List recent sessions
    List {
        /// Agent ID (default: "main")
        #[arg(long, default_value = DEFAULT_AGENT_ID)]
        agent: String,
    },
    /// Create a branch from an existing session
    Branch {
        /// Session ID to branch from
        from_id: String,
        /// Agent ID (default: "main")
        #[arg(long, default_value = DEFAULT_AGENT_ID)]
        agent: String,
    },
    /// List compaction checkpoints for a session
    Checkpoints {
        /// Session ID (uses most recent if omitted)
        #[arg(long)]
        session: Option<String>,
        /// Agent ID (default: "main")
        #[arg(long, default_value = DEFAULT_AGENT_ID)]
        agent: String,
    },
    /// Restore a session from a compaction checkpoint
    Restore {
        /// Checkpoint number to restore from
        checkpoint: u32,
        /// Session ID (uses most recent if omitted)
        #[arg(long)]
        session: Option<String>,
        /// Agent ID (default: "main")
        #[arg(long, default_value = DEFAULT_AGENT_ID)]
        agent: String,
    },
    /// Branch a new session from a compaction checkpoint
    BranchCheckpoint {
        /// Checkpoint number to branch from
        checkpoint: u32,
        /// Session ID containing the checkpoint
        #[arg(long)]
        session: Option<String>,
        /// Agent ID (default: "main")
        #[arg(long, default_value = DEFAULT_AGENT_ID)]
        agent: String,
    },
}

pub async fn run(args: SessionArgs) -> Result<()> {
    match args.command {
        SessionCommands::List { agent } => {
            let sessions = list_sessions_for_agent(&agent)?;
            if sessions.is_empty() {
                println!("No sessions found for agent '{}'.", agent);
            } else {
                println!("Sessions for agent '{}':", agent);
                for (i, info) in sessions.iter().enumerate().take(20) {
                    println!("  {}. {} ({})", i + 1, info.id, info.created_at);
                }
                if sessions.len() > 20 {
                    println!("  ... and {} more", sessions.len() - 20);
                }
            }
        }
        SessionCommands::Branch { from_id, agent } => {
            let original = Session::load_for_agent(&from_id, &agent)?;
            let branched = original.branch();
            let new_id = branched.id().to_string();
            let msg_count = branched.message_count();

            let dir = get_sessions_dir_for_agent(&agent)?;
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{}.jsonl", new_id));
            branched.save_to_path(&path)?;

            println!("Branched session created:");
            println!("  From: {}", from_id);
            println!("  New:  {}", new_id);
            println!("  Messages inherited: {}", msg_count);
        }
        SessionCommands::Checkpoints { session, agent } => {
            let session_id = resolve_session_id(session, &agent)?;
            let mgr = CheckpointManager::from_agent(&agent)?;
            let checkpoints = mgr.list_checkpoints(&session_id)?;

            if checkpoints.is_empty() {
                println!("No checkpoints found for session {}.", session_id);
            } else {
                println!(
                    "Checkpoints for session {} ({} total):",
                    session_id,
                    checkpoints.len()
                );
                for cp in &checkpoints {
                    let time = format_checkpoint_time(cp.created_at);
                    let tokens = if let Some(after) = cp.tokens_after {
                        format!("{} -> {} tokens", cp.tokens_before, after)
                    } else {
                        format!("{} tokens", cp.tokens_before)
                    };
                    println!(
                        "  #{}: {} ({}, {} msgs)",
                        cp.checkpoint_number, time, tokens, cp.message_count
                    );
                }
            }
        }
        SessionCommands::Restore {
            checkpoint,
            session,
            agent,
        } => {
            let session_id = resolve_session_id(session, &agent)?;
            let mgr = CheckpointManager::from_agent(&agent)?;
            let sessions_dir = get_sessions_dir_for_agent(&agent)?;

            mgr.restore_checkpoint(&session_id, checkpoint, &sessions_dir)?;
            println!(
                "Restored session {} from checkpoint #{}.",
                session_id, checkpoint
            );
        }
        SessionCommands::BranchCheckpoint {
            checkpoint,
            session,
            agent,
        } => {
            let session_id = resolve_session_id(session, &agent)?;
            let mgr = CheckpointManager::from_agent(&agent)?;

            let new_id = mgr.branch_from_checkpoint(&session_id, checkpoint, &agent)?;
            println!(
                "Branched from checkpoint #{} of session {}:",
                checkpoint, session_id
            );
            println!("  New session: {}", new_id);
        }
    }

    Ok(())
}

/// Resolve session ID: use provided value or find the most recent session
fn resolve_session_id(session: Option<String>, agent: &str) -> Result<String> {
    if let Some(id) = session {
        return Ok(id);
    }

    let sessions = list_sessions_for_agent(agent)?;
    sessions.first().map(|s| s.id.clone()).ok_or_else(|| {
        anyhow::anyhow!(
            "No sessions found for agent '{}'. Specify --session.",
            agent
        )
    })
}
