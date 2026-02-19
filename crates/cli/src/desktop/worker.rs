//! Background worker that owns the Agent
//!
//! The worker runs in a separate thread with its own tokio runtime.
//! It receives commands from the UI and sends back status updates.

use std::pin::pin;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use anyhow::Result;
use futures::StreamExt;

use localgpt_core::agent::{
    Agent, AgentConfig, DEFAULT_AGENT_ID, StreamEvent, ToolCall, extract_tool_detail,
    list_sessions_for_agent,
};
use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;

use super::state::{UiMessage, WorkerMessage};

/// Handle to the background worker
pub struct WorkerHandle {
    /// Send commands to the worker
    pub tx: Sender<UiMessage>,
    /// Receive updates from the worker
    pub rx: Receiver<WorkerMessage>,
    /// Thread handle
    _thread: JoinHandle<()>,
}

impl WorkerHandle {
    /// Start the background worker
    pub fn start(agent_id: Option<String>) -> Result<Self> {
        let (ui_tx, ui_rx) = mpsc::channel::<UiMessage>();
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerMessage>();

        let agent_id = agent_id.unwrap_or_else(|| DEFAULT_AGENT_ID.to_string());

        let thread = thread::spawn(move || {
            // Create tokio runtime for this thread
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime");

            rt.block_on(async {
                if let Err(e) = worker_loop(agent_id, ui_rx, worker_tx).await {
                    eprintln!("Worker error: {}", e);
                }
            });
        });

        Ok(Self {
            tx: ui_tx,
            rx: worker_rx,
            _thread: thread,
        })
    }

    /// Send a message to the worker
    pub fn send(&self, msg: UiMessage) -> Result<()> {
        self.tx.send(msg)?;
        Ok(())
    }

    /// Try to receive a message from the worker (non-blocking)
    pub fn try_recv(&self) -> Option<WorkerMessage> {
        self.rx.try_recv().ok()
    }
}

async fn worker_loop(
    agent_id: String,
    rx: Receiver<UiMessage>,
    tx: Sender<WorkerMessage>,
) -> Result<()> {
    // Initialize agent
    let config = Config::load()?;
    let memory = MemoryManager::new_with_full_config(&config.memory, Some(&config), &agent_id)?;

    let agent_config = AgentConfig {
        model: config.agent.default_model.clone(),
        context_window: config.agent.context_window,
        reserve_tokens: config.agent.reserve_tokens,
    };

    let mut agent = Agent::new(agent_config, &config, memory).await?;
    agent.extend_tools(crate::tools::create_cli_tools(&config)?);
    agent.new_session().await?;

    // Send ready message
    let _ = tx.send(WorkerMessage::Ready {
        model: agent.model().to_string(),
        memory_chunks: agent.memory_chunk_count(),
        has_embeddings: agent.has_embeddings(),
    });

    // Send initial session list
    if let Ok(sessions) = list_sessions_for_agent(&agent_id) {
        let _ = tx.send(WorkerMessage::Sessions(sessions));
    }

    // Send initial status
    let _ = tx.send(WorkerMessage::Status(agent.session_status()));

    // Track tools requiring approval
    let approval_tools: Vec<String> = agent.approval_required_tools().to_vec();

    // Main loop
    while let Ok(msg) = rx.recv() {
        let mut should_auto_save = false;

        match msg {
            UiMessage::Chat(message) => {
                // Stream response with tool support
                match agent.chat_stream_with_tools(&message, Vec::new()).await {
                    Ok(stream) => {
                        let mut stream = pin!(stream);
                        let mut pending_tools: Vec<ToolCall> = Vec::new();

                        while let Some(result) = stream.next().await {
                            match result {
                                Ok(event) => match event {
                                    StreamEvent::Content(text) => {
                                        let _ = tx.send(WorkerMessage::ContentChunk(text));
                                    }
                                    StreamEvent::ToolCallStart {
                                        name,
                                        id,
                                        arguments,
                                    } => {
                                        // Check if this tool requires approval
                                        if approval_tools.contains(&name) {
                                            // Collect for approval
                                            pending_tools.push(ToolCall {
                                                id,
                                                name,
                                                arguments: String::new(),
                                            });
                                        } else {
                                            let detail = extract_tool_detail(&name, &arguments);
                                            let _ = tx.send(WorkerMessage::ToolCallStart {
                                                name,
                                                id,
                                                detail,
                                            });
                                        }
                                    }
                                    StreamEvent::ToolCallEnd {
                                        name,
                                        id,
                                        output,
                                        warnings,
                                    } => {
                                        let _ = tx.send(WorkerMessage::ToolCallEnd {
                                            name,
                                            id,
                                            output,
                                            warnings,
                                        });
                                    }
                                    StreamEvent::Done => {
                                        if !pending_tools.is_empty() {
                                            let _ = tx.send(WorkerMessage::ToolsPendingApproval(
                                                pending_tools.clone(),
                                            ));
                                            pending_tools.clear();
                                        } else {
                                            let _ = tx.send(WorkerMessage::Done);
                                        }
                                        should_auto_save = true;
                                    }
                                },
                                Err(e) => {
                                    let _ = tx.send(WorkerMessage::Error(e.to_string()));
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(WorkerMessage::Error(e.to_string()));
                    }
                }
            }
            UiMessage::NewSession => match agent.new_session().await {
                Ok(()) => {
                    let status = agent.session_status();
                    let _ = tx.send(WorkerMessage::SessionChanged {
                        id: status.id.clone(),
                        message_count: status.message_count,
                    });
                    let _ = tx.send(WorkerMessage::Status(status));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMessage::Error(e.to_string()));
                }
            },
            UiMessage::ResumeSession(session_id) => match agent.resume_session(&session_id).await {
                Ok(()) => {
                    let status = agent.session_status();
                    let _ = tx.send(WorkerMessage::SessionChanged {
                        id: status.id.clone(),
                        message_count: status.message_count,
                    });
                    let _ = tx.send(WorkerMessage::Status(status));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMessage::Error(e.to_string()));
                }
            },
            UiMessage::ApproveTools(_tools) => {
                // Tool approval is handled in chat loop
                // For now, just send done
                let _ = tx.send(WorkerMessage::Done);
            }
            UiMessage::DenyTools => {
                let _ = tx.send(WorkerMessage::Done);
            }
            UiMessage::RefreshSessions => {
                if let Ok(sessions) = list_sessions_for_agent(&agent_id) {
                    let _ = tx.send(WorkerMessage::Sessions(sessions));
                }
            }
            UiMessage::RefreshStatus => {
                let _ = tx.send(WorkerMessage::Status(agent.session_status()));
            }
            UiMessage::SetModel(name) => match agent.set_model(&name) {
                Ok(()) => {
                    let _ = tx.send(WorkerMessage::SystemMessage(format!(
                        "Model set to: {}",
                        agent.model()
                    )));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMessage::SystemMessage(format!(
                        "Failed to set model: {}",
                        e
                    )));
                }
            },
            UiMessage::Compact => match agent.compact_session().await {
                Ok((before, after)) => {
                    let _ = tx.send(WorkerMessage::SystemMessage(format!(
                        "Session compacted: {} -> {} tokens",
                        before, after
                    )));
                    let _ = tx.send(WorkerMessage::Status(agent.session_status()));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMessage::SystemMessage(format!(
                        "Compact failed: {}",
                        e
                    )));
                }
            },
            UiMessage::SearchMemory(query) => match agent.search_memory(&query).await {
                Ok(results) => {
                    if results.is_empty() {
                        let _ = tx.send(WorkerMessage::SystemMessage(
                            "No memory results found.".to_string(),
                        ));
                    } else {
                        let text = results
                            .iter()
                            .enumerate()
                            .map(|(i, chunk)| {
                                let preview: String = chunk.content.chars().take(150).collect();
                                let preview = preview.replace('\n', " ");
                                format!(
                                    "{}. {} (lines {}-{}, score: {:.3})\n   {}",
                                    i + 1,
                                    chunk.file,
                                    chunk.line_start,
                                    chunk.line_end,
                                    chunk.score,
                                    preview,
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        let _ = tx.send(WorkerMessage::SystemMessage(format!(
                            "Memory search results for \"{}\":\n{}",
                            query, text
                        )));
                    }
                }
                Err(e) => {
                    let _ = tx.send(WorkerMessage::SystemMessage(format!(
                        "Memory search failed: {}",
                        e
                    )));
                }
            },
            UiMessage::Save => match agent.save_session().await {
                Ok(path) => {
                    let _ = tx.send(WorkerMessage::SystemMessage(format!(
                        "Session saved to: {}",
                        path.display()
                    )));
                }
                Err(e) => {
                    let _ = tx.send(WorkerMessage::SystemMessage(format!("Save failed: {}", e)));
                }
            },
            UiMessage::ShowHelp => {
                let help_text = "\
Available commands:
  /new              Start a new session
  /model [name]     Show or set the current model
  /compact          Compact session history
  /memory <query>   Search memory files
  /save             Save current session to disk
  /status           Show session status
  /sessions         Show saved sessions
  /resume <id>      Resume a session by ID
  /help             Show this help text";
                let _ = tx.send(WorkerMessage::SystemMessage(help_text.to_string()));
            }
            UiMessage::ShowStatus => {
                let status = agent.session_status();
                let text = format!(
                    "Session: {}\nMessages: {}\nTokens: {} context / {} API in / {} API out\nCompactions: {}",
                    &status.id[..8.min(status.id.len())],
                    status.message_count,
                    status.token_count,
                    status.api_input_tokens,
                    status.api_output_tokens,
                    status.compaction_count,
                );
                let _ = tx.send(WorkerMessage::SystemMessage(text));
                let _ = tx.send(WorkerMessage::Status(status));
            }
        }

        // Auto-save session after chat completes
        if should_auto_save && let Err(e) = agent.auto_save_session() {
            eprintln!("Warning: Failed to auto-save session: {}", e);
        }
    }

    Ok(())
}
