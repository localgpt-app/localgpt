//! LocalGPT Gen — AI-driven 3D scene generation binary.
//!
//! This binary runs Bevy on the main thread (required for macOS windowing/GPU)
//! and spawns the LLM agent loop on a background tokio runtime.

use anyhow::Result;
use clap::{Parser, Subcommand};
use futures::StreamExt;
use localgpt_core::agent::tools::extract_tool_detail;
use localgpt_core::agent::{Agent, list_sessions_for_agent, search_sessions_for_agent};
use localgpt_core::commands::Interface;
use std::io::Write as _;
use std::path::{Path, PathBuf};

// Use library modules
use localgpt_gen::character_tools;
use localgpt_gen::gen3d;
use localgpt_gen::mcp_server;

/// Result of handling a slash command.
enum CommandResult {
    /// Continue the interactive loop.
    Continue,
    /// Exit the loop.
    Quit,
    /// Send the message to the agent.
    SendMessage(String),
}

/// Handle slash commands for Gen mode.
async fn handle_gen_command(
    input: &str,
    agent: &mut Agent,
    agent_id: &str,
    workspace: &Path,
) -> CommandResult {
    let parts: Vec<&str> = input.split_whitespace().collect();
    let cmd = parts.first().copied().unwrap_or("");

    match cmd {
        "/quit" | "/exit" | "/q" => CommandResult::Quit,

        "/help" | "/h" | "/?" => {
            println!(
                "\n{}\n",
                localgpt_core::commands::format_help_text(Interface::Gen)
            );
            CommandResult::Continue
        }

        "/model" => {
            if parts.len() < 2 {
                println!("\nCurrent model: {}\n", agent.model());
                return CommandResult::Continue;
            }
            let model = parts[1];
            match agent.set_model(model) {
                Ok(()) => println!("\nSwitched to model: {}\n", model),
                Err(e) => eprintln!("\nError: Failed to switch model: {}\n", e),
            }
            CommandResult::Continue
        }

        "/models" => {
            println!("\nAvailable model prefixes:");
            println!("  claude-cli/*    - Use Claude CLI (e.g., claude-cli/opus)");
            println!("  gpt-*           - OpenAI (requires API key)");
            println!("  claude-*        - Anthropic API (requires API key)");
            println!("  glm-*           - GLM (Z.AI)");
            println!("  ollama/*        - Ollama local (e.g., ollama/llama3)");
            println!("\nCurrent model: {}", agent.model());
            println!("Use /model <name> to switch.\n");
            CommandResult::Continue
        }

        "/status" => {
            let status = agent.session_status();
            println!("\nSession Status:");
            println!("  ID: {}", status.id);
            println!("  Model: {}", agent.model());
            println!("  Messages: {}", status.message_count);
            println!("  Context tokens: ~{}", status.token_count);
            println!("  Compactions: {}", status.compaction_count);
            println!("\nMemory:");
            println!("  Chunks: {}", agent.memory_chunk_count());
            if agent.has_embeddings() {
                println!("  Embeddings: enabled");
            }
            println!();
            CommandResult::Continue
        }

        "/context" => {
            let (used, usable, total) = agent.context_usage();
            let pct = (used as f64 / usable as f64 * 100.0).min(100.0);
            println!("\nContext Window:");
            println!("  Used: {} tokens ({:.1}%)", used, pct);
            println!("  Usable: {} tokens", usable);
            println!("  Total: {} tokens", total);
            if pct > 80.0 {
                println!("\n⚠ Context nearly full. Consider /compact or /new.");
            }
            println!();
            CommandResult::Continue
        }

        "/new" => {
            match agent.save_session_to_memory().await {
                Ok(Some(path)) => println!("\nSession saved to: {}", path.display()),
                Ok(None) => {}
                Err(e) => eprintln!("Warning: Failed to save session to memory: {}", e),
            }
            match agent.new_session().await {
                Ok(()) => println!("New session started. Memory context reloaded.\n"),
                Err(e) => eprintln!("\nError: Failed to create new session: {}\n", e),
            }
            CommandResult::Continue
        }

        "/clear" => {
            agent.clear_session();
            println!("\nSession cleared.\n");
            CommandResult::Continue
        }

        "/compact" => match agent.compact_session().await {
            Ok((before, after)) => {
                println!("\nSession compacted. Token count: {} → {}\n", before, after);
                CommandResult::Continue
            }
            Err(e) => {
                eprintln!("\nError: Failed to compact: {}\n", e);
                CommandResult::Continue
            }
        },

        "/save" => match agent.save_session().await {
            Ok(path) => {
                println!("\nSession saved to: {}\n", path.display());
                CommandResult::Continue
            }
            Err(e) => {
                eprintln!("\nError: Failed to save session: {}\n", e);
                CommandResult::Continue
            }
        },

        "/memory" => {
            if parts.len() < 2 {
                eprintln!("\nError: Usage: /memory <query>\n");
                return CommandResult::Continue;
            }
            let query = parts[1..].join(" ");
            match agent.search_memory(&query).await {
                Ok(results) => {
                    if results.is_empty() {
                        println!(
                            "\nNo results found for '{}'. Try /reindex to rebuild memory index.\n",
                            query
                        );
                    } else {
                        println!("\nMemory search results for '{}':", query);
                        for (i, result) in results.iter().enumerate() {
                            let snippet = extract_snippet(&result.content, &query, 120);
                            println!(
                                "{}. [{}:{}] {}",
                                i + 1,
                                result.file,
                                result.line_start,
                                snippet
                            );
                        }
                        println!();
                    }
                }
                Err(e) => eprintln!("\nError: Memory search failed: {}\n", e),
            }
            CommandResult::Continue
        }

        "/reindex" => match agent.reindex_memory().await {
            Ok((files, chunks, embedded)) => {
                if embedded > 0 {
                    println!(
                        "\nMemory index rebuilt: {} files, {} chunks, {} embeddings\n",
                        files, chunks, embedded
                    );
                } else {
                    println!(
                        "\nMemory index rebuilt: {} files, {} chunks\n",
                        files, chunks
                    );
                }
                CommandResult::Continue
            }
            Err(e) => {
                eprintln!("\nError: Failed to reindex: {}\n", e);
                CommandResult::Continue
            }
        },

        "/export" => {
            let markdown = agent.export_markdown();
            if parts.len() >= 2 {
                let path = parts[1..].join(" ");
                let expanded = shellexpand::tilde(&path).to_string();
                match std::fs::write(&expanded, &markdown) {
                    Ok(()) => println!("\nSession exported to: {}\n", expanded),
                    Err(e) => eprintln!("\nError: Failed to export: {}\n", e),
                }
            } else {
                println!("\n{}", markdown);
            }
            CommandResult::Continue
        }

        "/sessions" => {
            match list_sessions_for_agent(agent_id) {
                Ok(sessions) => {
                    if sessions.is_empty() {
                        println!("\nNo saved sessions found.\n");
                    } else {
                        println!("\nAvailable sessions:");
                        for (i, session) in sessions.iter().take(10).enumerate() {
                            println!(
                                "  {}. {} ({} messages, {})",
                                i + 1,
                                &session.id[..session.id.floor_char_boundary(8)],
                                session.message_count,
                                session.created_at.format("%Y-%m-%d %H:%M")
                            );
                            if !session.preview.is_empty() {
                                if session.preview != session.end_preview {
                                    println!("     B: \"{}\"", session.preview);
                                    println!("     E: \"{}\"", session.end_preview);
                                } else {
                                    println!("     \"{}\"", session.preview);
                                }
                            }
                        }
                        if sessions.len() > 10 {
                            println!("  ... and {} more", sessions.len() - 10);
                        }
                        println!("\nUse /resume <id> to resume a session.\n");
                    }
                }
                Err(e) => eprintln!("\nError: Failed to list sessions: {}\n", e),
            }
            CommandResult::Continue
        }

        "/resume" => {
            if parts.len() < 2 {
                eprintln!("\nError: Usage: /resume <session-id>\n");
                return CommandResult::Continue;
            }
            let session_id = parts[1];
            match list_sessions_for_agent(agent_id) {
                Ok(sessions) => {
                    let matching: Vec<_> = sessions
                        .iter()
                        .filter(|s| s.id.starts_with(session_id))
                        .collect();

                    match matching.len() {
                        0 => eprintln!("\nError: No session found matching '{}'\n", session_id),
                        1 => {
                            let full_id = matching[0].id.clone();
                            match agent.resume_session(&full_id).await {
                                Ok(()) => {
                                    let status = agent.session_status();
                                    println!(
                                        "\nResumed session {} ({} messages)\n",
                                        &full_id[..full_id.floor_char_boundary(8)],
                                        status.message_count
                                    );

                                    for msg in agent.raw_session_messages() {
                                        if msg.message.role == localgpt_core::agent::Role::System {
                                            continue;
                                        }

                                        let role_str = match msg.message.role {
                                            localgpt_core::agent::Role::User => {
                                                "\x1b[36mYou\x1b[0m"
                                            }
                                            localgpt_core::agent::Role::Assistant => {
                                                "\x1b[32mAssistant\x1b[0m"
                                            }
                                            localgpt_core::agent::Role::Tool => {
                                                "\x1b[35mTool\x1b[0m"
                                            }
                                            _ => "",
                                        };

                                        if let Some(ref calls) = msg.message.tool_calls {
                                            for call in calls {
                                                println!(
                                                    "\n{}: \x1b[35m[Tool Call: {}({})]\x1b[0m",
                                                    role_str,
                                                    call.name,
                                                    call.arguments.trim()
                                                );
                                            }
                                        }

                                        if !msg.message.content.is_empty() {
                                            if msg.message.role == localgpt_core::agent::Role::Tool
                                            {
                                                println!(
                                                    "\n{}:\n\x1b[90m{}\x1b[0m",
                                                    role_str,
                                                    msg.message.content.trim()
                                                );
                                            } else {
                                                println!(
                                                    "\n{}:\n{}",
                                                    role_str,
                                                    msg.message.content.trim()
                                                );
                                            }
                                        }
                                    }
                                    println!();
                                }
                                Err(e) => eprintln!("\nError: Failed to resume: {}\n", e),
                            }
                        }
                        _ => eprintln!(
                            "\nError: Multiple sessions match '{}'. Please be more specific.\n",
                            session_id
                        ),
                    }
                }
                Err(e) => eprintln!("\nError: Failed to list sessions: {}\n", e),
            }
            CommandResult::Continue
        }

        "/search" => {
            if parts.len() < 2 {
                eprintln!("\nError: Usage: /search <query>\n");
                return CommandResult::Continue;
            }
            let query = parts[1..].join(" ");
            match search_sessions_for_agent(agent_id, &query) {
                Ok(results) => {
                    if results.is_empty() {
                        println!("\nNo sessions found matching '{}'.\n", query);
                    } else {
                        println!("\nSessions matching '{}':", query);
                        for (i, result) in results.iter().take(10).enumerate() {
                            println!(
                                "  {}. {} ({} matches, {})",
                                i + 1,
                                &result.session_id[..result.session_id.floor_char_boundary(8)],
                                result.match_count,
                                result.created_at.format("%Y-%m-%d")
                            );
                            if !result.message_preview.is_empty() {
                                println!("     \"{}\"", result.message_preview);
                            }
                        }
                        if results.len() > 10 {
                            println!("  ... and {} more", results.len() - 10);
                        }
                        println!("\nUse /resume <id> to resume a session.\n");
                    }
                }
                Err(e) => eprintln!("\nError: Search failed: {}\n", e),
            }
            CommandResult::Continue
        }

        "/skills" => {
            match localgpt_core::agent::load_skills(workspace) {
                Ok(skills) => {
                    // Partition into world skills and other skills
                    let mut worlds = Vec::new();
                    let mut others = Vec::new();
                    for skill in skills {
                        let skill_dir = skill.path.parent().unwrap_or(&skill.path);
                        if skill_dir.join("world.ron").exists() {
                            worlds.push(skill);
                        } else {
                            others.push(skill);
                        }
                    }

                    if !worlds.is_empty() {
                        println!("\nWorlds ({}):", worlds.len());
                        for skill in &worlds {
                            let source = match skill.source {
                                localgpt_core::agent::skills::SkillSource::Workspace => {
                                    "[workspace]"
                                }
                                localgpt_core::agent::skills::SkillSource::Managed => "[managed]",
                                localgpt_core::agent::skills::SkillSource::Bundled => "[bundled]",
                            };
                            println!(
                                "  /{} - {} {}",
                                skill.command_name, skill.description, source
                            );
                        }
                    }

                    if !others.is_empty() {
                        println!("\n{}", localgpt_core::agent::get_skills_summary(&others));
                    }

                    if worlds.is_empty() && others.is_empty() {
                        println!("\nNo skills found.");
                    }
                    println!();
                }
                Err(e) => eprintln!("\nError loading skills: {}\n", e),
            }
            CommandResult::Continue
        }

        _ => {
            // Not a recognized command - send to agent
            CommandResult::SendMessage(input.to_string())
        }
    }
}

/// Extract a snippet from content around a query match.
fn extract_snippet(content: &str, query: &str, max_len: usize) -> String {
    let lower_content = content.to_lowercase();
    let lower_query = query.to_lowercase();

    if let Some(pos) = lower_content.find(&lower_query) {
        let start = pos.saturating_sub(30);
        let end = (pos + query.len() + 30).min(content.len());
        let snippet = &content[start..end];

        let prefix = if start > 0 { "..." } else { "" };
        let suffix = if end < content.len() { "..." } else { "" };

        format!("{}{}{}", prefix, snippet.trim(), suffix)
    } else {
        let truncated = if content.len() > max_len {
            format!("{}...", &content[..max_len])
        } else {
            content.to_string()
        };
        truncated.replace('\n', " ")
    }
}

/// Run a streaming chat with tool call display.
///
/// This mirrors the CLI mode's streaming chat behavior:
/// - Streams response chunks in real-time
/// - Shows tool calls with detail extraction
/// - Displays execution status for each tool
async fn streaming_chat(agent: &mut Agent, input: &str) -> Result<()> {
    print!("\nLocalGPT: ");
    std::io::stdout().flush().ok();

    match agent.chat_stream_with_images(input, vec![]).await {
        Ok(mut stream) => {
            let mut full_response = String::new();
            let mut pending_tool_calls = None;

            // Stream response chunks
            while let Some(result) = stream.next().await {
                match result {
                    Ok(chunk) => {
                        print!("{}", chunk.delta);
                        std::io::stdout().flush().ok();
                        full_response.push_str(&chunk.delta);

                        if chunk.done && chunk.tool_calls.is_some() {
                            pending_tool_calls = chunk.tool_calls;
                        }
                    }
                    Err(e) => {
                        eprintln!("\nStream error: {}", e);
                        break;
                    }
                }
            }

            // Handle tool calls with display
            if let Some(tool_calls) = pending_tool_calls {
                for tc in &tool_calls {
                    let detail = extract_tool_detail(&tc.name, &tc.arguments);
                    if let Some(ref d) = detail {
                        println!("\n[{}: {}]", tc.name, d);
                    } else {
                        println!("\n[{}]", tc.name);
                    }
                }

                // Execute with feedback
                agent
                    .execute_streaming_tool_calls(
                        &full_response,
                        tool_calls,
                        |name, args| {
                            let detail = extract_tool_detail(name, args);
                            if let Some(ref d) = detail {
                                print!("\n> Running: {} ({}) ... ", name, d);
                            } else {
                                print!("\n> Running: {} ... ", name);
                            }
                            std::io::stdout().flush().ok();
                        },
                        |_name, result| match result {
                            Ok(()) => print!("Done."),
                            Err(e) => print!("Failed: {}", e),
                        },
                    )
                    .await?;

                println!();
            } else {
                // No tool calls - finish the stream
                agent.finish_chat_stream(&full_response);
            }

            if let Err(e) = agent.auto_save_session() {
                eprintln!("Warning: Failed to auto-save session: {}", e);
            }
        }
        Err(e) => {
            eprintln!("\nError: {}", e);
        }
    }

    Ok(())
}

#[derive(Parser)]
#[command(name = "localgpt-gen")]
#[command(about = "LocalGPT Gen — AI-driven 3D scene generation")]
struct Cli {
    #[command(subcommand)]
    command: Option<GenSubcommand>,

    /// Initial prompt (interactive mode only)
    prompt: Option<String>,

    /// Agent ID to use
    #[arg(short, long, global = true, default_value = "gen")]
    agent: String,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Load a glTF/GLB scene at startup
    #[arg(short = 's', long, global = true)]
    scene: Option<String>,
}

#[derive(Subcommand)]
enum GenSubcommand {
    /// Run as MCP server (stdio) — Bevy window + gen tools over MCP
    McpServer,
    /// Control an external avatar (headless, no Bevy window)
    Control {
        /// URL of the external app
        url: String,
        /// Initial prompt
        prompt: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging before handing off to Bevy
    // Use "warn" by default for cleaner interactive TUI, "debug" with --verbose
    let log_level = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_writer(std::io::stderr)
        .init();

    // Load config early so both Bevy and agent threads can use it
    let config = localgpt_core::config::Config::load()?;
    let workspace = config.workspace_path();

    // Dispatch based on subcommand
    match cli.command {
        Some(GenSubcommand::Control { url, prompt }) => {
            // Headless bridge mode — no Bevy window
            tracing::info!("Starting Gen in CONTROL mode (headless) -> {}", url);
            let agent_id = cli.agent;
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build tokio runtime");

            rt.block_on(
                async move { run_headless_control_loop(&url, &agent_id, prompt, config).await },
            )
        }

        Some(GenSubcommand::McpServer) => {
            // MCP server mode: Bevy on main thread, MCP stdio server on background thread
            let initial_scene = cli
                .scene
                .as_ref()
                .and_then(|path| gen3d::plugin::resolve_gltf_path(path, &workspace));

            let (bridge, channels) = gen3d::create_gen_channels();
            let bridge_for_mcp = bridge.clone();
            let mcp_config = config.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime for MCP server");

                rt.block_on(async move {
                    if let Err(e) = mcp_server::run_mcp_server(bridge_for_mcp, mcp_config).await {
                        tracing::error!("MCP server error: {}", e);
                    }
                    // MCP client disconnected — exit the process
                    std::process::exit(0);
                });
            });

            // Run Bevy on the main thread
            run_bevy_app(channels, workspace, initial_scene)
        }

        None => {
            // Interactive mode (default)
            let initial_scene = cli
                .scene
                .as_ref()
                .and_then(|path| gen3d::plugin::resolve_gltf_path(path, &workspace));

            let (bridge, channels) = gen3d::create_gen_channels();
            let agent_id = cli.agent;
            let initial_prompt = cli.prompt;
            let bridge_for_agent = bridge.clone();

            // Spawn tokio runtime + agent loop on a background thread
            // (Bevy must own the main thread for windowing/GPU on macOS)
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime for gen agent");

                rt.block_on(async move {
                    if let Err(e) =
                        run_agent_loop(bridge_for_agent, &agent_id, initial_prompt, config).await
                    {
                        tracing::error!("Gen agent loop error: {}", e);
                    }
                });
            });

            // Run Bevy on the main thread
            run_bevy_app(channels, workspace, initial_scene)
        }
    }
}

/// Set up and run the Bevy application on the main thread.
fn run_bevy_app(
    channels: gen3d::GenChannels,
    workspace: std::path::PathBuf,
    initial_scene: Option<PathBuf>,
) -> Result<()> {
    use bevy::prelude::*;

    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "LocalGPT Gen".into(),
                    resolution: bevy::window::WindowResolution::new(1280, 720),
                    ..default()
                }),
                ..default()
            })
            .set(bevy::asset::AssetPlugin {
                file_path: "/".to_string(),
                ..default()
            })
            .disable::<bevy::log::LogPlugin>(),
    );

    gen3d::plugin::setup_gen_app(&mut app, channels, workspace, initial_scene);

    app.run();

    Ok(())
}

/// Run the interactive agent loop in headless control mode.
async fn run_headless_control_loop(
    url: &str,
    agent_id: &str,
    initial_prompt: Option<String>,
    config: localgpt_core::config::Config,
) -> Result<()> {
    use localgpt_core::agent::tools::create_safe_tools;
    use localgpt_core::agent::{Agent, create_spawn_agent_tool};
    use localgpt_core::memory::MemoryManager;
    use rustyline::DefaultEditor;
    use rustyline::error::ReadlineError;
    use std::sync::Arc;

    // Set up memory
    let memory = MemoryManager::new_with_agent(&config.memory, agent_id)?;
    let memory = Arc::new(memory);

    // Create safe tools + character tools pointing to the external URL
    let mut tools = create_safe_tools(&config, Some(memory.clone()))?;
    tools.extend(character_tools::create_avatar_tools());
    tools.extend(vec![create_spawn_agent_tool(
        config.clone(),
        memory.clone(),
    )]);

    // Create agent with combined tools
    let mut agent = Agent::new_with_tools(config.clone(), agent_id, memory, tools)?;
    agent.new_session().await?;

    // Inject instructions for avatar control
    let instructions = r#"
You are controlling an avatar in an external 3D application.
Your goal is to explore the world and execute user commands.

You have access to `avatar_tools` to:
- Get state (`get_avatar_state`)
- Move (`move_avatar`)
- Look (`look_avatar`)
- Teleport (`teleport_avatar`)

Use `get_avatar_state` frequently to understand your position.
"#;
    agent.add_user_message(instructions);

    println!("Connected to external avatar control at {}", url);

    // If initial prompt given, send it
    if let Some(prompt) = initial_prompt {
        println!("\n> {}", prompt);
        streaming_chat(&mut agent, &prompt).await?;
        println!();
    }

    // Interactive loop
    let mut rl = DefaultEditor::new()?;
    loop {
        let readline = rl.readline("Avatar> ");

        let input = match readline {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                break; // Ctrl+D
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        };

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        let _ = rl.add_history_entry(input);

        if input == "/quit" || input == "/exit" || input == "/q" {
            break;
        }

        streaming_chat(&mut agent, input).await?;
        println!();
    }

    Ok(())
}

/// Run the interactive agent loop with Gen tools available.
async fn run_agent_loop(
    bridge: std::sync::Arc<gen3d::GenBridge>,
    agent_id: &str,
    initial_prompt: Option<String>,
    config: localgpt_core::config::Config,
) -> Result<()> {
    use localgpt_core::agent::tools::create_safe_tools;
    use localgpt_core::agent::{Agent, create_spawn_agent_tool};
    use localgpt_core::memory::MemoryManager;
    use rustyline::DefaultEditor;
    use rustyline::error::ReadlineError;
    use std::sync::Arc;

    // Set up memory
    let memory = MemoryManager::new_with_agent(&config.memory, agent_id)?;
    let memory = Arc::new(memory);

    // Create safe tools + gen tools + CLI tools
    let mut tools = create_safe_tools(&config, Some(memory.clone()))?;
    tools.extend(gen3d::tools::create_gen_tools(bridge.clone()));
    tools.extend(localgpt_gen::mcp::avatar_tools::create_character_tools(
        bridge.clone(),
    ));
    tools.extend(localgpt_gen::mcp::interaction_tools::create_interaction_tools(bridge.clone()));
    tools.extend(localgpt_gen::mcp::terrain_tools::create_terrain_tools(
        bridge.clone(),
    ));
    tools.extend(localgpt_gen::mcp::ui_tools::create_ui_tools(bridge.clone()));
    tools.extend(localgpt_gen::mcp::physics_tools::create_physics_tools(
        bridge,
    ));
    tools.extend(localgpt_cli_tools::create_cli_tools(&config)?);
    tools.extend(vec![create_spawn_agent_tool(
        config.clone(),
        memory.clone(),
    )]);

    // Gen mode needs many repeated tool calls to build scenes (e.g., spawning
    // multiple primitives, checking scene_info between steps).  The default
    // loop-detection threshold (3) is too aggressive and causes the agent to
    // abort mid-scene.  Raise it so legitimate scene-building isn't blocked.
    let mut config = config;
    config.agent.max_tool_repeats = config.agent.max_tool_repeats.max(20);
    let workspace = config.workspace_path();

    // Create agent with combined tools
    let mut agent = Agent::new_with_tools(config.clone(), agent_id, memory, tools)?;
    agent.new_session().await?;

    // Display model info (matching CLI format)
    let embedding_status = if agent.has_embeddings() {
        " | Embeddings: enabled"
    } else {
        ""
    };
    println!(
        "LocalGPT Gen v{} | Agent: {} | Model: {} | Memory: {} chunks{}\n",
        env!("CARGO_PKG_VERSION"),
        agent_id,
        agent.model(),
        agent.memory_chunk_count(),
        embedding_status
    );
    println!("Type /help for commands, /quit to exit\n");
    println!("Scene Controls:");
    println!("  WASD          Move (forward/back/strafe)");
    println!("  Space         Move up");
    println!("  Shift         Move down");
    println!("  Right-click   Hold + drag to look around");
    println!("  Scroll wheel  Adjust movement speed");
    println!("  Tab           Toggle free-fly / avatar camera");
    println!("  V             Toggle 1st/3rd person (avatar mode)");
    println!();

    // If initial prompt given, send it
    if let Some(prompt) = initial_prompt {
        println!("\nYou: {}", prompt);
        streaming_chat(&mut agent, &prompt).await?;
        println!();
    }

    // Interactive loop
    let mut rl = DefaultEditor::new()?;
    loop {
        let readline = rl.readline("You: ");

        let input = match readline {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                break; // Ctrl+D
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        };

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        // Add to history
        let _ = rl.add_history_entry(input);

        // Handle slash commands
        if input.starts_with('/') {
            match handle_gen_command(input, &mut agent, agent_id, &workspace).await {
                CommandResult::Continue => continue,
                CommandResult::Quit => break,
                CommandResult::SendMessage(msg) => {
                    streaming_chat(&mut agent, &msg).await?;
                    println!();
                }
            }
        } else {
            streaming_chat(&mut agent, input).await?;
            println!();
        }
    }

    Ok(())
}
