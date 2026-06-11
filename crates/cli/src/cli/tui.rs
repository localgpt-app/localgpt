use anyhow::Result;
use clap::Args;
use crossterm::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures::StreamExt;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::io;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::debug;

use localgpt_core::agent::{
    Agent, AgentConfig, create_spawn_agent_tool, get_last_session_id_for_agent,
};
use localgpt_core::concurrency::WorkspaceLock;
use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;
use localgpt_core::text::prefix_chars;

fn short_session_id(id: &str) -> String {
    prefix_chars(id, 8)
}

#[derive(Args)]
pub struct TuiArgs {
    /// Model to use (overrides config)
    #[arg(short, long)]
    pub model: Option<String>,

    /// Session ID to resume
    #[arg(short, long)]
    pub session: Option<String>,

    /// Resume the most recent session
    #[arg(long)]
    pub resume: bool,
}

pub async fn run(args: TuiArgs, agent_id: &str) -> Result<()> {
    // 1. Setup Agent
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
    debug!("TUI Agent initialized with tools: {:?}", agent.tool_names());

    let _workspace_lock = WorkspaceLock::new()?;

    // Load or create session
    let session_id = if let Some(id) = args.session {
        Some(id)
    } else if args.resume {
        get_last_session_id_for_agent(agent_id)?
    } else {
        None
    };

    if let Some(session_id) = session_id {
        if let Err(e) = agent.resume_session(&session_id).await {
            eprintln!("Failed to load session {}: {}", session_id, e);
            agent.new_session().await?;
        }
    } else {
        agent.new_session().await?;
    }

    // 2. Setup Terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 3. Run App
    let app_result = run_app(&mut terminal, &mut agent, agent_id).await;

    // 4. Restore Terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = app_result {
        eprintln!("TUI Error: {:?}", e);
    } else if let Err(e) = agent.auto_save_session() {
        eprintln!("Warning: Failed to auto-save session: {}", e);
    }

    Ok(())
}

#[derive(Clone)]
enum AppMessage {
    Text {
        role: String,
        content: String,
    },
    ToolCall {
        name: String,
        arguments: String,
        output: Option<String>,
        is_expanded: bool,
    },
}

struct App {
    header: String,
    input: String,
    messages: Vec<AppMessage>,
    is_generating: bool,
    selected_index: Option<usize>,
    cursor_position: usize,
}

impl App {
    fn new(agent_id: &str, model: &str, chunk_count: usize, has_embeddings: bool) -> Self {
        let embedding_status = if has_embeddings {
            " | Embeddings: enabled"
        } else {
            ""
        };

        let header = format!(
            "LocalGPT v{} | Agent: {} | Model: {} | Memory: {} chunks{}",
            env!("CARGO_PKG_VERSION"),
            agent_id,
            model,
            chunk_count,
            embedding_status
        );

        Self {
            header,
            input: String::new(),
            messages: vec![AppMessage::Text {
                role: "System".to_string(),
                content: "Use Up/Down to select, Ctrl+O to expand tools. Esc/Ctrl+C to quit."
                    .to_string(),
            }],
            is_generating: false,
            selected_index: None,
            cursor_position: 0,
        }
    }
}

enum AppEvent {
    Input(Event),
    Tick,
}

fn load_agent_messages_into_app(app: &mut App, agent: &Agent) {
    let mut pending_tool_outputs: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    for msg in agent.raw_session_messages() {
        if msg.message.role == localgpt_core::agent::Role::System {
            // Skip the huge internal system prompt from showing up in UI
            continue;
        }

        if msg.message.role == localgpt_core::agent::Role::Tool {
            if let Some(ref id) = msg.message.tool_call_id {
                pending_tool_outputs.insert(id.clone(), msg.message.content.clone());
            }
            continue;
        }

        let role = match msg.message.role {
            localgpt_core::agent::Role::User => "You",
            localgpt_core::agent::Role::Assistant => "Assistant",
            localgpt_core::agent::Role::System => "System",
            localgpt_core::agent::Role::Tool => "Tool",
        };

        if let Some(ref calls) = msg.message.tool_calls {
            for call in calls {
                app.messages.push(AppMessage::ToolCall {
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                    output: pending_tool_outputs.remove(&call.id), // Might be None if not yet processed or tracked differently in history
                    is_expanded: false,
                });
            }
        }

        if !msg.message.content.is_empty() {
            app.messages.push(AppMessage::Text {
                role: role.to_string(),
                content: msg.message.content.clone(),
            });
        }
    }

    // Now re-iterate the app messages. In LocalGPT's history, Assistant messages emit tool_calls,
    // and then the *next* messages are Role::Tool with the outputs.
    // Let's patch those historical outputs in.
    let raw_msgs = agent.raw_session_messages();
    for i in 0..app.messages.len() {
        if let AppMessage::ToolCall {
            ref name,
            ref arguments,
            ref mut output,
            ..
        } = app.messages[i]
            && output.is_none()
        {
            // Find matching tool call in raw history to get ID
            if let Some(msg) = raw_msgs.iter().find(|m| {
                m.message.tool_calls.as_ref().is_some_and(|calls| {
                    calls
                        .iter()
                        .any(|c| &c.name == name && &c.arguments == arguments)
                })
            }) && let Some(calls) = &msg.message.tool_calls
                && let Some(call) = calls
                    .iter()
                    .find(|c| &c.name == name && &c.arguments == arguments)
            {
                // Find subsequent Role::Tool message with this ID
                if let Some(tool_msg) = raw_msgs.iter().find(|m| {
                    m.message.role == localgpt_core::agent::Role::Tool
                        && m.message.tool_call_id.as_ref() == Some(&call.id)
                }) {
                    *output = Some(tool_msg.message.content.clone());
                }
            }
        }
    }
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    agent: &mut Agent,
    agent_id: &str,
) -> Result<()> {
    let mut app = App::new(
        agent_id,
        agent.model(),
        agent.memory_chunk_count(),
        agent.has_embeddings(),
    );

    // Load history messages if resuming (use raw_session_messages to avoid printing the appended security block)
    load_agent_messages_into_app(&mut app, agent);

    app.selected_index = if app.messages.is_empty() {
        None
    } else {
        Some(app.messages.len() - 1)
    };

    let (tx, mut rx) = mpsc::channel(100);

    // Input thread
    let tick_rate = std::time::Duration::from_millis(50);
    let _input_task = tokio::spawn(async move {
        loop {
            if crossterm::event::poll(tick_rate).unwrap_or(false)
                && let Ok(evt) = crossterm::event::read()
            {
                let _ = tx.send(AppEvent::Input(evt)).await;
            }
            let _ = tx.send(AppEvent::Tick).await;
        }
    });

    loop {
        terminal.draw(|f| ui(f, &app))?;

        if let Some(event) = rx.recv().await {
            match event {
                AppEvent::Input(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    // Check for Ctrl+C to quit
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('c')
                    {
                        return Ok(());
                    }

                    // Check for Ctrl+A
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('a')
                    {
                        if !app.is_generating {
                            app.cursor_position = 0;
                        }
                        continue;
                    }

                    // Check for Ctrl+E
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('e')
                    {
                        if !app.is_generating {
                            app.cursor_position = app.input.chars().count();
                        }
                        continue;
                    }

                    // Check for Ctrl+O
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char('o')
                    {
                        if let Some(idx) = app.selected_index
                            && let Some(AppMessage::ToolCall { is_expanded, .. }) =
                                app.messages.get_mut(idx)
                        {
                            *is_expanded = !*is_expanded;
                        }
                        continue;
                    }

                    match key.code {
                        KeyCode::Esc => {
                            return Ok(());
                        }
                        KeyCode::Up => {
                            if let Some(idx) = app.selected_index
                                && idx > 0
                            {
                                app.selected_index = Some(idx - 1);
                            } else if !app.messages.is_empty() {
                                app.selected_index = Some(app.messages.len() - 1);
                            }
                        }
                        KeyCode::Down => {
                            if let Some(idx) = app.selected_index
                                && idx + 1 < app.messages.len()
                            {
                                app.selected_index = Some(idx + 1);
                            }
                        }
                        KeyCode::Enter => {
                            if app.is_generating {
                                continue;
                            }
                            let input = app.input.trim().to_string();
                            app.input.clear();
                            app.cursor_position = 0;

                            if !input.is_empty() {
                                if input.starts_with('/') {
                                    app.messages.push(AppMessage::Text {
                                        role: "You".to_string(),
                                        content: input.clone(),
                                    });

                                    let parts: Vec<&str> = input.split_whitespace().collect();
                                    let cmd = parts[0];

                                    match cmd {
                                        "/quit" | "/exit" | "/q" => return Ok(()),
                                        "/help" | "/h" | "/?" => {
                                            let help_text =
                                                localgpt_core::commands::format_help_text(
                                                    localgpt_core::commands::Interface::Cli,
                                                );
                                            app.messages.push(AppMessage::Text {
                                                role: "System".to_string(),
                                                content: help_text,
                                            });
                                        }
                                        "/skills" => {
                                            match localgpt_core::agent::load_skills(
                                                std::path::Path::new("."),
                                            ) {
                                                Ok(skills) => {
                                                    app.messages.push(AppMessage::Text { role: "System".to_string(), content: localgpt_core::agent::get_skills_summary(&skills) });
                                                }
                                                Err(e) => {
                                                    app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!(
                                                            "Failed to load skills: {}",
                                                            e
                                                        ),
                                                    });
                                                }
                                            }
                                        }
                                        "/sessions" => {
                                            match localgpt_core::agent::list_sessions_for_agent(
                                                agent_id,
                                            ) {
                                                Ok(sessions) => {
                                                    if sessions.is_empty() {
                                                        app.messages.push(AppMessage::Text {
                                                            role: "System".to_string(),
                                                            content: "No saved sessions found."
                                                                .to_string(),
                                                        });
                                                    } else {
                                                        let mut out =
                                                            String::from("Available sessions:\n");
                                                        for (i, session) in
                                                            sessions.iter().take(10).enumerate()
                                                        {
                                                            let short_id =
                                                                short_session_id(&session.id);
                                                            out.push_str(&format!(
                                                                "  {}. {} ({} messages, {})\n",
                                                                i + 1,
                                                                short_id,
                                                                session.message_count,
                                                                session
                                                                    .created_at
                                                                    .format("%Y-%m-%d %H:%M")
                                                            ));
                                                            if !session.preview.is_empty() {
                                                                if session.preview
                                                                    != session.end_preview
                                                                {
                                                                    out.push_str(&format!(
                                                                        "     B: \"{}\"\n",
                                                                        session.preview
                                                                    ));
                                                                    out.push_str(&format!(
                                                                        "     E: \"{}\"\n",
                                                                        session.end_preview
                                                                    ));
                                                                } else {
                                                                    out.push_str(&format!(
                                                                        "     \"{}\"\n",
                                                                        session.preview
                                                                    ));
                                                                }
                                                            }
                                                        }
                                                        if sessions.len() > 10 {
                                                            out.push_str(&format!(
                                                                "  ... and {} more\n",
                                                                sessions.len() - 10
                                                            ));
                                                        }
                                                        out.push_str("\nUse /resume <id> to resume a session.");
                                                        app.messages.push(AppMessage::Text {
                                                            role: "System".to_string(),
                                                            content: out,
                                                        });
                                                    }
                                                }
                                                Err(e) => app.messages.push(AppMessage::Text {
                                                    role: "System".to_string(),
                                                    content: format!(
                                                        "Failed to list sessions: {}",
                                                        e
                                                    ),
                                                }),
                                            }
                                        }
                                        "/search" => {
                                            if parts.len() < 2 {
                                                app.messages.push(AppMessage::Text {
                                                    role: "System".to_string(),
                                                    content: "Usage: /search <query>".to_string(),
                                                });
                                            } else {
                                                let query = parts[1..].join(" ");
                                                match localgpt_core::agent::search_sessions_for_agent(agent_id, &query) {
                                                    Ok(results) => {
                                                        if results.is_empty() {
                                                            app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("No sessions found matching '{}'.", query) });
                                                        } else {
                                                            let mut out = format!("Sessions matching '{}':\n", query);
                                                            for (i, result) in results.iter().take(10).enumerate() {
                                                                let short_id = short_session_id(&result.session_id);
                                                                out.push_str(&format!("  {}. {} ({} matches, {})\n", i + 1, short_id, result.match_count, result.created_at.format("%Y-%m-%d")));
                                                                if !result.message_preview.is_empty() {
                                                                    out.push_str(&format!("     \"{}\"\n", result.message_preview));
                                                                }
                                                            }
                                                            app.messages.push(AppMessage::Text { role: "System".to_string(), content: out });
                                                        }
                                                    }
                                                    Err(e) => app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("Search failed: {}", e) }),
                                                }
                                            }
                                        }
                                        "/resume" => {
                                            if parts.len() < 2 {
                                                app.messages.push(AppMessage::Text {
                                                    role: "System".to_string(),
                                                    content: "Usage: /resume <session-id>"
                                                        .to_string(),
                                                });
                                            } else {
                                                let session_id = parts[1];
                                                match localgpt_core::agent::list_sessions_for_agent(
                                                    agent_id,
                                                ) {
                                                    Ok(sessions) => {
                                                        let matching: Vec<_> = sessions
                                                            .iter()
                                                            .filter(|s| {
                                                                s.id.starts_with(session_id)
                                                            })
                                                            .collect();
                                                        match matching.len() {
                                                            0 => app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("No session found matching '{}'", session_id) }),
                                                            1 => {
                                                                let full_id = matching[0].id.clone();
                                                                match agent.resume_session(&full_id).await {
                                                                    Ok(()) => {
                                                                        let status = agent.session_status();
                                                                        let short_id = short_session_id(&full_id);
                                                                        app.messages.clear();
                                                                        load_agent_messages_into_app(&mut app, agent);
                                                                        app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("Resumed session {} ({} messages)", short_id, status.message_count) });
                                                                    }
                                                                    Err(e) => app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("Failed to resume: {}", e) }),
                                                                }
                                                            }
                                                            _ => app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("Multiple sessions match '{}'. Please be more specific.", session_id) }),
                                                        }
                                                    }
                                                    Err(e) => app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!(
                                                            "Failed to list sessions: {}",
                                                            e
                                                        ),
                                                    }),
                                                }
                                            }
                                        }
                                        "/clear" => {
                                            agent.clear_session();
                                            app.messages.push(AppMessage::Text {
                                                role: "System".to_string(),
                                                content: "Session cleared.".to_string(),
                                            });
                                        }
                                        "/new" => {
                                            match agent.save_session_to_memory().await {
                                                Ok(Some(path)) => {
                                                    app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!(
                                                            "Session saved to: {}",
                                                            path.display()
                                                        ),
                                                    });
                                                }
                                                Ok(None) => {}
                                                Err(e) => {
                                                    app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!("Warning: Failed to save session to memory: {}", e),
                                                    });
                                                }
                                            }
                                            match agent.new_session().await {
                                                Ok(_) => {
                                                    app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: "New session started.".to_string(),
                                                    });
                                                }
                                                Err(e) => {
                                                    app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!(
                                                            "Failed to create new session: {}",
                                                            e
                                                        ),
                                                    });
                                                }
                                            }
                                        }
                                        "/model" => {
                                            if parts.len() < 2 {
                                                app.messages.push(AppMessage::Text {
                                                    role: "System".to_string(),
                                                    content: format!(
                                                        "Current model: {}",
                                                        agent.model()
                                                    ),
                                                });
                                            } else {
                                                let model = parts[1];
                                                if let Err(e) = agent.set_model(model) {
                                                    app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!(
                                                            "Failed to switch model: {}",
                                                            e
                                                        ),
                                                    });
                                                } else {
                                                    app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!(
                                                            "Switched to model: {}",
                                                            model
                                                        ),
                                                    });
                                                }
                                            }
                                        }
                                        "/compact" => {
                                            match agent.compact_session().await {
                                                Ok((before, after)) => {
                                                    app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("Session compacted. Token count: {} -> {}", before, after) });
                                                }
                                                Err(e) => {
                                                    app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!(
                                                            "Failed to compact: {}",
                                                            e
                                                        ),
                                                    });
                                                }
                                            }
                                        }
                                        "/memory" => {
                                            if parts.len() < 2 {
                                                app.messages.push(AppMessage::Text {
                                                    role: "System".to_string(),
                                                    content: "Usage: /memory <query>".to_string(),
                                                });
                                            } else {
                                                let query = parts[1..].join(" ");
                                                match agent.search_memory(&query).await {
                                                    Ok(results) => {
                                                        if results.is_empty() {
                                                            app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("No results found for '{}'. Try /reindex to rebuild memory index.", query) });
                                                        } else {
                                                            let mut out = format!(
                                                                "Memory search results for '{}':\n",
                                                                query
                                                            );
                                                            for (i, result) in
                                                                results.iter().enumerate()
                                                            {
                                                                let snippet = crate::cli::chat::extract_snippet(&result.content, &query, 120);
                                                                out.push_str(&format!(
                                                                    "{}. [{}:{}] {}\n",
                                                                    i + 1,
                                                                    result.file,
                                                                    result.line_start,
                                                                    snippet
                                                                ));
                                                            }
                                                            app.messages.push(AppMessage::Text {
                                                                role: "System".to_string(),
                                                                content: out,
                                                            });
                                                        }
                                                    }
                                                    Err(e) => app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!(
                                                            "Memory search failed: {}",
                                                            e
                                                        ),
                                                    }),
                                                }
                                            }
                                        }
                                        "/reindex" => match agent.reindex_memory().await {
                                            Ok((files, chunks, embedded)) => {
                                                if embedded > 0 {
                                                    app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("Memory index rebuilt: {} files, {} chunks, {} embeddings", files, chunks, embedded) });
                                                } else {
                                                    app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("Memory index rebuilt: {} files, {} chunks", files, chunks) });
                                                }
                                            }
                                            Err(e) => app.messages.push(AppMessage::Text {
                                                role: "System".to_string(),
                                                content: format!("Failed to reindex: {}", e),
                                            }),
                                        },
                                        "/save" => match agent.save_session().await {
                                            Ok(path) => app.messages.push(AppMessage::Text {
                                                role: "System".to_string(),
                                                content: format!(
                                                    "Session saved to: {}",
                                                    path.display()
                                                ),
                                            }),
                                            Err(e) => app.messages.push(AppMessage::Text {
                                                role: "System".to_string(),
                                                content: format!("Failed to save session: {}", e),
                                            }),
                                        },
                                        "/status" => {
                                            let status = agent.session_status();
                                            let mut out = String::from("Session Status:\n");
                                            out.push_str(&format!("  ID: {}\n", status.id));
                                            out.push_str(&format!("  Model: {}\n", agent.model()));
                                            out.push_str(&format!(
                                                "  Messages: {}\n",
                                                status.message_count
                                            ));
                                            out.push_str(&format!(
                                                "  Context tokens: ~{}\n",
                                                status.token_count
                                            ));
                                            out.push_str(&format!(
                                                "  Compactions: {}\n\n",
                                                status.compaction_count
                                            ));
                                            out.push_str("Memory:\n");
                                            out.push_str(&format!(
                                                "  Chunks: {}\n",
                                                agent.memory_chunk_count()
                                            ));
                                            if agent.has_embeddings() {
                                                out.push_str("  Embeddings: enabled\n");
                                            }
                                            if status.api_input_tokens > 0
                                                || status.api_output_tokens > 0
                                            {
                                                out.push_str("\nAPI Usage:\n");
                                                out.push_str(&format!(
                                                    "  Input tokens: {}\n",
                                                    status.api_input_tokens
                                                ));
                                                out.push_str(&format!(
                                                    "  Output tokens: {}\n",
                                                    status.api_output_tokens
                                                ));
                                                out.push_str(&format!(
                                                    "  Total tokens: {}\n",
                                                    status.api_input_tokens
                                                        + status.api_output_tokens
                                                ));
                                            }
                                            if status.search_queries > 0 {
                                                let cache_pct = (status.search_cached_hits as f64
                                                    / status.search_queries as f64)
                                                    * 100.0;
                                                out.push_str("\nSearch:\n");
                                                out.push_str(&format!(
                                                    "  Queries: {}\n",
                                                    status.search_queries
                                                ));
                                                out.push_str(&format!(
                                                    "  Cached hits: {} ({:.0}%)\n",
                                                    status.search_cached_hits, cache_pct
                                                ));
                                                out.push_str(&format!(
                                                    "  Estimated cost: ${:.3}",
                                                    status.search_cost_usd
                                                ));
                                            }
                                            app.messages.push(AppMessage::Text {
                                                role: "System".to_string(),
                                                content: out,
                                            });
                                        }
                                        "/models" => {
                                            let mut out =
                                                String::from("Available model prefixes:\n");
                                            out.push_str("  claude-cli/*    - Use Claude CLI (e.g., claude-cli/opus, claude-cli/sonnet)\n");
                                            out.push_str(
                                                "  gpt-*           - OpenAI (requires API key)\n",
                                            );
                                            out.push_str("  claude-*        - Anthropic API (requires API key)\n");
                                            out.push_str("  ollama/*        - Ollama local (e.g., ollama/llama3)\n");
                                            out.push_str(
                                                "  <other>         - Defaults to Ollama\n",
                                            );
                                            out.push_str(&format!(
                                                "\nCurrent model: {}\n",
                                                agent.model()
                                            ));
                                            out.push_str("Use /model <name> to switch.");
                                            app.messages.push(AppMessage::Text {
                                                role: "System".to_string(),
                                                content: out,
                                            });
                                        }
                                        "/context" => {
                                            let (used, usable, total) = agent.context_usage();
                                            let pct =
                                                (used as f64 / usable as f64 * 100.0).min(100.0);
                                            let mut out = String::from("Context Window:\n");
                                            out.push_str(&format!(
                                                "  Used: {} tokens ({:.1}%)\n",
                                                used, pct
                                            ));
                                            out.push_str(&format!("  Usable: {} tokens\n", usable));
                                            out.push_str(&format!("  Total: {} tokens\n", total));
                                            out.push_str(&format!(
                                                "  Reserve: {} tokens\n",
                                                total - usable
                                            ));
                                            if pct > 80.0 {
                                                out.push_str("\n⚠ Context nearly full. Consider /compact or /new.");
                                            }
                                            app.messages.push(AppMessage::Text {
                                                role: "System".to_string(),
                                                content: out,
                                            });
                                        }
                                        "/export" => {
                                            let markdown = agent.export_markdown();
                                            if parts.len() >= 2 {
                                                let path = parts[1..].join(" ");
                                                let expanded =
                                                    shellexpand::tilde(&path).to_string();
                                                match std::fs::write(&expanded, &markdown) {
                                                    Ok(()) => app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!(
                                                            "Session exported to: {}",
                                                            expanded
                                                        ),
                                                    }),
                                                    Err(e) => app.messages.push(AppMessage::Text {
                                                        role: "System".to_string(),
                                                        content: format!("Failed to export: {}", e),
                                                    }),
                                                }
                                            } else {
                                                app.messages.push(AppMessage::Text {
                                                    role: "System".to_string(),
                                                    content: markdown,
                                                });
                                            }
                                        }
                                        _ => {
                                            app.messages.push(AppMessage::Text {
                                                role: "System".to_string(),
                                                content: format!(
                                                    "Unknown command: {}. Type /help for commands.",
                                                    cmd
                                                ),
                                            });
                                        }
                                    }
                                    app.selected_index = Some(app.messages.len() - 1);
                                    continue;
                                }

                                app.messages.push(AppMessage::Text {
                                    role: "You".to_string(),
                                    content: input.clone(),
                                });
                                app.is_generating = true;
                                // Placeholder for assistant message
                                app.messages.push(AppMessage::Text {
                                    role: "Assistant".to_string(),
                                    content: String::new(),
                                });
                                app.selected_index = Some(app.messages.len() - 1);
                                terminal.draw(|f| ui(f, &app))?;

                                // Stream response
                                match agent.chat_stream_with_tools(&input, Vec::new()).await {
                                    Ok(stream) => {
                                        tokio::pin!(stream);
                                        while let Some(evt) = stream.next().await {
                                            match evt {
                                                Ok(localgpt_core::agent::StreamEvent::Content(text)) => {
                                                    if let Some(AppMessage::Text { content, .. }) = app.messages.last_mut() {
                                                        content.push_str(&text);
                                                    } else {
                                                        app.messages.push(AppMessage::Text { role: "Assistant".to_string(), content: text });
                                                        app.selected_index = Some(app.messages.len() - 1);
                                                    }
                                                    terminal.draw(|f| ui(f, &app))?;
                                                }
                                                Ok(localgpt_core::agent::StreamEvent::ToolCallStart { name, arguments, .. }) => {
                                                    app.messages.push(AppMessage::ToolCall {
                                                        name: name.clone(),
                                                        arguments: arguments.to_string(),
                                                        output: None,
                                                        is_expanded: false,
                                                    });
                                                    app.selected_index = Some(app.messages.len() - 1);
                                                    terminal.draw(|f| ui(f, &app))?;
                                                }
                                                Ok(localgpt_core::agent::StreamEvent::ToolCallEnd { name: _, output, .. }) => {
                                                    if let Some(AppMessage::ToolCall { output: out, .. }) = app.messages.last_mut() {
                                                        *out = Some(output);
                                                    }
                                                    terminal.draw(|f| ui(f, &app))?;
                                                }
                                                Ok(localgpt_core::agent::StreamEvent::Done) => {}
                                                Ok(localgpt_core::agent::StreamEvent::ApprovalRequired { .. }) => {}
                                                Err(e) => {
                                                    app.messages.push(AppMessage::Text { role: "System".to_string(), content: format!("[Error: {}]", e) });
                                                    terminal.draw(|f| ui(f, &app))?;
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        app.messages.push(AppMessage::Text {
                                            role: "System".to_string(),
                                            content: format!("Error starting stream: {}", e),
                                        });
                                    }
                                }
                                app.is_generating = false;
                                app.selected_index = Some(app.messages.len() - 1);

                                if let Err(e) = agent.auto_save_session() {
                                    app.messages.push(AppMessage::Text {
                                        role: "System".to_string(),
                                        content: format!(
                                            "Warning: Failed to auto-save session: {}",
                                            e
                                        ),
                                    });
                                }
                            }
                        }
                        KeyCode::Char(c) => {
                            if !app.is_generating {
                                // insert char at cursor_position
                                let byte_index = app
                                    .input
                                    .char_indices()
                                    .map(|(i, _)| i)
                                    .nth(app.cursor_position)
                                    .unwrap_or(app.input.len());
                                app.input.insert(byte_index, c);
                                app.cursor_position += 1;
                            }
                        }
                        KeyCode::Backspace => {
                            if !app.is_generating && app.cursor_position > 0 {
                                app.cursor_position -= 1;
                                let byte_index = app
                                    .input
                                    .char_indices()
                                    .map(|(i, _)| i)
                                    .nth(app.cursor_position)
                                    .unwrap_or(app.input.len());
                                app.input.remove(byte_index);
                            }
                        }
                        KeyCode::Delete => {
                            if !app.is_generating && app.cursor_position < app.input.chars().count()
                            {
                                let byte_index = app
                                    .input
                                    .char_indices()
                                    .map(|(i, _)| i)
                                    .nth(app.cursor_position)
                                    .unwrap_or(app.input.len());
                                app.input.remove(byte_index);
                            }
                        }
                        KeyCode::Left => {
                            if !app.is_generating && app.cursor_position > 0 {
                                app.cursor_position -= 1;
                            }
                        }
                        KeyCode::Right => {
                            if !app.is_generating && app.cursor_position < app.input.chars().count()
                            {
                                app.cursor_position += 1;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {} // Tick or other events
            }
        }
    }
}

fn ui(f: &mut ratatui::Frame, app: &App) {
    let inner_width = f.area().width.max(1) as usize;
    let input_char_count = app.input.chars().count();
    let input_lines = (input_char_count / inner_width) + 1;
    let input_height = input_lines.max(1) as u16 + 2;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(input_height)])
        .split(f.area());

    let mut lines: Vec<Line> = Vec::new();

    for (i, msg) in app.messages.iter().enumerate() {
        let is_selected = app.selected_index == Some(i);
        let prefix = if is_selected { "> " } else { "  " };
        let style = if is_selected {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        match msg {
            AppMessage::Text { role, content } => {
                let role_color = match role.as_str() {
                    "You" => Color::Cyan,
                    "Assistant" => Color::Green,
                    "System" => Color::DarkGray,
                    _ => Color::Reset,
                };

                // Very basic wrapping
                let mut first = true;
                for line in content.split('\n') {
                    if first {
                        lines.push(Line::from(vec![
                            Span::styled(prefix, style),
                            Span::styled(
                                format!("{}: ", role),
                                Style::default().fg(role_color).add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(line),
                        ]));
                        first = false;
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled("  ", style),
                            Span::raw(format!("  {}", line)),
                        ]));
                    }
                }
            }
            AppMessage::ToolCall {
                name,
                arguments,
                output,
                is_expanded,
            } => {
                let status = if output.is_some() {
                    "[Done]"
                } else {
                    "[Running...]"
                };
                let expand_hint = if *is_expanded { "[-] " } else { "[+] " };

                lines.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(expand_hint, Style::default().fg(Color::Yellow)),
                    Span::styled(
                        format!("Tool: {} {}", name, status),
                        Style::default().fg(Color::Magenta),
                    ),
                ]));

                if *is_expanded {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled("Input: ", Style::default().fg(Color::DarkGray)),
                        Span::raw(arguments.trim()),
                    ]));

                    if let Some(out) = output {
                        lines.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled("Output: ", Style::default().fg(Color::DarkGray)),
                        ]));
                        for out_line in out.trim().lines() {
                            lines.push(Line::from(vec![Span::raw("      "), Span::raw(out_line)]));
                        }
                    }
                }
            }
        }
    }

    let title = if app.is_generating {
        format!(" {} (Generating...) ", app.header)
    } else {
        format!(" {} ", app.header)
    };

    // Calculate how many terminal lines the wrapped text will actually take.
    // Ratatui handles wrapping natively, but we need to compute scroll offset.
    let wrap_width = chunks[0].width.max(1) as usize;
    let mut total_wrapped_lines = 0;

    for line in &lines {
        // A simple heuristic for wrapped line count: total chars / width
        let char_count: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        let wrapped_count = (char_count / wrap_width) + 1;
        total_wrapped_lines += wrapped_count;
    }

    let max_lines = chunks[0].height.saturating_sub(2);
    let scroll_y = (total_wrapped_lines as u16).saturating_sub(max_lines);

    let messages_block = Paragraph::new(lines)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::TOP | Borders::BOTTOM),
        )
        .wrap(ratatui::widgets::Wrap { trim: false })
        .scroll((scroll_y, 0));

    f.render_widget(messages_block, chunks[0]);

    let input_block = Paragraph::new(app.input.as_str())
        .block(
            Block::default()
                .title(" Input ")
                .borders(Borders::TOP | Borders::BOTTOM),
        )
        .wrap(ratatui::widgets::Wrap { trim: false });
    f.render_widget(input_block, chunks[1]);

    if !app.is_generating {
        let cursor_x = chunks[1].x + (app.cursor_position as u16 % inner_width as u16);
        let cursor_y = chunks[1].y + 1 + (app.cursor_position as u16 / inner_width as u16);
        f.set_cursor_position((cursor_x, cursor_y));
    }
}
