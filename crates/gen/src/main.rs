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

        "/effort" => {
            if parts.len() < 2 {
                match agent.effort() {
                    Some(level) => println!("\nCurrent effort: {}\n", level),
                    None => println!("\nEffort levels not supported by current provider\n"),
                }
                return CommandResult::Continue;
            }
            match agent.set_effort(parts[1]) {
                Ok(()) => println!("\nEffort set to: {}\n", parts[1]),
                Err(e) => eprintln!("\nError: {}\n", e),
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

        "/gallery" => {
            let subcommand = parts.get(1).copied().unwrap_or("list");
            match subcommand {
                "list" | "ls" => {
                    let summary = gen3d::gallery::gallery_summary(workspace);
                    println!("\n{}\n", summary);
                }
                "refresh" => {
                    let entries = gen3d::gallery::scan_world_gallery(workspace);
                    println!("\nRefreshed: {} worlds found.\n", entries.len());
                }
                _ => {
                    println!("\nUsage:");
                    println!("  /gallery         List all worlds");
                    println!("  /gallery list    List all worlds");
                    println!("  /gallery refresh Rescan skills/");
                    println!("  Press G in the viewport to toggle the gallery overlay\n");
                }
            }
            CommandResult::Continue
        }

        "/experiments" | "/exp" => {
            let subcommand = parts.get(1).copied().unwrap_or("list");
            match subcommand {
                "list" | "ls" => {
                    let config = localgpt_core::config::Config::load().ok();
                    let state_dir = config
                        .as_ref()
                        .map(|c| c.paths.state_dir.clone())
                        .unwrap_or_else(|| PathBuf::from("~/.local/state/localgpt"));
                    let tracker = localgpt_gen::experiment::ExperimentTracker::new(&state_dir);
                    match tracker.read_all() {
                        Ok(exps) => {
                            if exps.is_empty() {
                                println!("\nNo experiments found.\n");
                            } else {
                                println!("\n{} experiments:", exps.len());
                                for exp in exps.iter().rev().take(20) {
                                    let status = format!("{}", exp.status);
                                    let entities = exp
                                        .entity_count
                                        .map(|n| format!("{} entities", n))
                                        .unwrap_or_default();
                                    let prompt_preview = if exp.prompt.len() > 50 {
                                        &exp.prompt[..50]
                                    } else {
                                        &exp.prompt
                                    };
                                    println!(
                                        "  [{}] {} — {} {}",
                                        status, exp.id, prompt_preview, entities
                                    );
                                }
                                println!();
                            }
                        }
                        Err(e) => eprintln!("\nError reading experiments: {}\n", e),
                    }
                }
                _ => {
                    println!("\nUsage:");
                    println!("  /experiments        List recent experiments");
                    println!("  /experiments list   List recent experiments\n");
                }
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

    /// Enable MCP relay server for external MCP clients.
    /// Auto-enabled when using claude-cli/* models.
    #[arg(long, global = true)]
    mcp_relay: bool,
}

#[derive(Subcommand)]
enum GenSubcommand {
    /// Run as MCP server (stdio) — Bevy window + gen tools over MCP
    McpServer {
        /// Run headless (no window) — for CI or batch generation via MCP
        #[arg(long)]
        headless: bool,

        /// Connect to an existing gen process's MCP relay instead of starting Bevy.
        /// Pass the relay port (e.g., 9878) or omit to auto-discover.
        #[arg(long)]
        connect: Option<Option<u16>>,

        /// Also start a streamable HTTP MCP server on this port.
        /// External tools can POST JSON-RPC to http://127.0.0.1:<port>/mcp.
        #[arg(long)]
        mcp_http: Option<u16>,
    },
    /// Control an external avatar (headless, no Bevy window)
    Control {
        /// URL of the external app
        url: String,
        /// Initial prompt
        prompt: Option<String>,
    },
    /// Headless generation — generate a world without opening a window
    Headless {
        /// Generation prompt (required)
        #[arg(long)]
        prompt: String,

        /// Output world skill directory (default: auto-named in workspace/skills/)
        #[arg(long)]
        output: Option<String>,

        /// Capture a thumbnail after generation (default: true)
        #[arg(long, default_value = "true")]
        screenshot: bool,

        /// Screenshot width in pixels
        #[arg(long, default_value = "1280")]
        screenshot_width: u32,

        /// Screenshot height in pixels
        #[arg(long, default_value = "720")]
        screenshot_height: u32,

        /// Max generation time before abort (seconds, default: 300)
        #[arg(long, default_value = "300")]
        timeout: u64,

        /// Override LLM model for this run
        #[arg(long)]
        model: Option<String>,

        /// Style hint prepended to prompt
        #[arg(long)]
        style: Option<String>,
    },
}

fn main() -> Result<()> {
    // On Linux, default to X11 (XWayland) to avoid wgpu "Invalid surface" errors
    // on Wayland compositors. winit 0.29+ selects Wayland when WAYLAND_DISPLAY is set.
    // Users can set LOCALGPT_WAYLAND=1 to keep native Wayland.
    #[cfg(target_os = "linux")]
    if std::env::var("LOCALGPT_WAYLAND").is_err() {
        // SAFETY: called at program start before any threads are spawned.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }

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

        Some(GenSubcommand::Headless {
            prompt,
            output,
            screenshot,
            screenshot_width,
            screenshot_height,
            timeout,
            model,
            style,
        }) => {
            // Headless generation mode — no window, generate and exit
            let headless_config = gen3d::headless::HeadlessConfig {
                prompt,
                output,
                screenshot,
                screenshot_width,
                screenshot_height,
                timeout_secs: timeout,
                agent_id: cli.agent,
                model,
                style,
            };

            tracing::info!("Starting headless generation: {:?}", headless_config.prompt);

            let (bridge, channels) = gen3d::create_gen_channels();
            let completion_flag = gen3d::headless::HeadlessCompletionFlag::default();
            let flag_for_agent = completion_flag.clone();
            let flag_for_timeout = completion_flag.clone();
            let agent_config = config.clone();

            // Spawn timeout watchdog
            let timeout_secs = headless_config.timeout_secs;
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(timeout_secs));
                if !flag_for_timeout.is_done() {
                    tracing::error!("Headless generation timed out after {}s", timeout_secs);
                    flag_for_timeout.complete_failure();
                }
            });

            // Spawn agent loop on background thread
            let bridge_for_agent = bridge.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime for headless gen");

                rt.block_on(async move {
                    match run_headless_agent(bridge_for_agent, headless_config, agent_config).await
                    {
                        Ok(()) => flag_for_agent.complete_success(),
                        Err(e) => {
                            tracing::error!("Headless generation failed: {}", e);
                            flag_for_agent.complete_failure();
                        }
                    }
                });
            });

            // Run headless Bevy on the main thread
            let result = run_headless_bevy_app(channels, workspace, completion_flag.clone());

            // Map exit code based on success/failure
            if !completion_flag.is_success() {
                std::process::exit(1);
            }

            result
        }

        Some(GenSubcommand::McpServer {
            headless,
            connect,
            mcp_http,
        }) => {
            // --connect mode: relay stdio MCP to an existing gen process's TCP relay
            if let Some(port_opt) = connect {
                let port = port_opt
                    .or_else(gen3d::mcp_relay::read_relay_port)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No relay port specified and no running gen process found.\n\
                             Start localgpt-gen first, or pass --connect <port>."
                        )
                    })?;

                tracing::info!("Connecting to existing gen process relay on port {}", port);

                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime");

                return rt.block_on(run_mcp_stdio_relay(port));
            }

            // MCP server mode: Bevy on main thread, MCP stdio server on background thread
            let initial_scene = if headless {
                None
            } else {
                cli.scene
                    .as_ref()
                    .and_then(|path| gen3d::plugin::resolve_gltf_path(path, &workspace))
            };

            let (bridge, channels) = gen3d::create_gen_channels();
            let bridge_for_mcp = bridge.clone();
            let mcp_config = config.clone();

            // Optionally start the MCP HTTP server on a separate thread
            if let Some(http_port) = mcp_http {
                let bridge_for_http = bridge.clone();
                let http_config = config.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to build tokio runtime for MCP HTTP server");

                    rt.block_on(async move {
                        if let Err(e) =
                            mcp_server::run_mcp_http_server(bridge_for_http, http_config, http_port)
                                .await
                        {
                            tracing::error!("MCP HTTP server error: {}", e);
                        }
                    });
                });
            }

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

            // Run Bevy on the main thread (headless or windowed)
            if headless {
                let completion_flag = gen3d::headless::HeadlessCompletionFlag::default();
                run_headless_bevy_app(channels, workspace, completion_flag)
            } else {
                run_bevy_app(channels, workspace, initial_scene)
            }
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
            let bridge_for_relay = bridge.clone();
            let relay_config = config.clone();

            // Enable MCP relay when explicitly requested or when using a CLI backend
            // (claude-cli, gemini-cli, codex-cli spawn subprocesses that need MCP access)
            let model = &config.agent.default_model;
            let enable_relay = cli.mcp_relay
                || model.starts_with("claude-cli/")
                || model.starts_with("gemini-cli/")
                || model.starts_with("codex-cli/");

            // Spawn tokio runtime + agent loop + MCP relay on a background thread
            // (Bevy must own the main thread for windowing/GPU on macOS)
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime for gen agent");

                rt.block_on(async move {
                    if enable_relay {
                        // Start the MCP relay server so external CLI tools (claude, codex, gemini)
                        // can connect to the existing Bevy window instead of spawning a new one.
                        match gen3d::mcp_relay::start_mcp_relay(
                            bridge_for_relay,
                            &relay_config,
                        )
                        .await
                        {
                            Ok(port) => {
                                eprintln!(
                                    "MCP relay active on port {} (external MCP clients can connect to this window)",
                                    port
                                );
                            }
                            Err(e) => {
                                tracing::warn!("MCP relay failed to start: {} (external MCP clients won't be able to connect)", e);
                            }
                        }
                    }

                    if let Err(e) =
                        run_agent_loop(bridge_for_agent, &agent_id, initial_prompt, relay_config)
                            .await
                    {
                        tracing::error!("Gen agent loop error: {}", e);
                    }
                });
            });

            // Run Bevy on the main thread (blocks until window closes)
            let result = run_bevy_app(channels, workspace, initial_scene);

            // Clean up relay port file so stale ports aren't discovered
            if enable_relay {
                gen3d::mcp_relay::cleanup_relay_port();
            }

            result
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
                    present_mode: bevy::window::PresentMode::AutoVsync,
                    composite_alpha_mode: bevy::window::CompositeAlphaMode::Auto,
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

/// Set up and run headless Bevy (no window) on the main thread.
///
/// Headless mode uses DefaultPlugins without a primary window, and adds
/// the completion detector system that exits when the agent is done.
fn run_headless_bevy_app(
    channels: gen3d::GenChannels,
    workspace: std::path::PathBuf,
    completion_flag: gen3d::headless::HeadlessCompletionFlag,
) -> Result<()> {
    use bevy::prelude::*;

    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None, // No window
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(bevy::render::RenderPlugin {
                render_creation: bevy::render::settings::RenderCreation::Automatic(
                    bevy::render::settings::WgpuSettings {
                        // Allow software rendering on headless servers
                        backends: Some(bevy::render::settings::Backends::all()),
                        ..default()
                    },
                ),
                ..default()
            })
            .set(bevy::asset::AssetPlugin {
                file_path: "/".to_string(),
                ..default()
            })
            .disable::<bevy::log::LogPlugin>(),
    );

    // Insert completion flag and add detector system
    app.insert_resource(completion_flag);
    app.add_systems(Update, gen3d::headless::headless_completion_detector);

    gen3d::plugin::setup_gen_app(&mut app, channels, workspace, None);

    app.run();

    Ok(())
}

/// Run the headless generation agent — generates a world and exits.
async fn run_headless_agent(
    bridge: std::sync::Arc<gen3d::GenBridge>,
    headless_config: gen3d::headless::HeadlessConfig,
    config: localgpt_core::config::Config,
) -> Result<()> {
    use localgpt_core::agent::Agent;
    use localgpt_core::agent::tools::create_safe_tools;
    use localgpt_core::memory::MemoryManager;
    use std::sync::Arc;

    let agent_id = &headless_config.agent_id;

    // Set up memory
    let memory = MemoryManager::new_with_agent(&config.memory, agent_id)?;
    let memory = Arc::new(memory);

    // Create safe tools + gen tools (no CLI tools needed in headless)
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
        bridge.clone(),
    ));
    tools.extend(localgpt_gen::mcp::multifile_tools::create_multifile_tools(
        bridge.clone(),
    ));

    // Configure agent
    let mut config = config;
    config.agent.max_tool_repeats = config.agent.max_tool_repeats.max(20);

    if let Some(ref model) = headless_config.model {
        config.agent.default_model = model.clone();
    }

    // Create agent
    let mut agent = Agent::new_with_tools(config.clone(), agent_id, memory, tools)?;
    agent.new_session().await?;

    // Inject gen-specific memory guidance
    agent.add_user_message(gen3d::system_prompt::GEN_MEMORY_PROMPT);

    // Build effective prompt
    let effective_prompt = format!(
        "{}\n\n{}",
        gen3d::system_prompt::HEADLESS_EXPERIMENT_PROMPT,
        headless_config.effective_prompt()
    );

    eprintln!("Generating: {}", headless_config.prompt);

    // Generate the world
    let response = agent.chat(&effective_prompt).await?;

    tracing::info!("Agent response: {}", &response[..response.len().min(200)]);

    // Save the world
    let world_name = headless_config
        .output
        .as_deref()
        .map(|p| {
            std::path::Path::new(p)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|| localgpt_gen::experiment::prompt_to_slug(&headless_config.prompt));

    let save_prompt = format!(
        "Save this world with gen_save_world. Name: \"{}\"",
        world_name
    );
    let _save_response = agent.chat(&save_prompt).await?;

    eprintln!("World saved: {}", world_name);

    Ok(())
}

/// MCP stdio ↔ TCP relay: bridges Claude CLI's MCP stdio to the existing gen
/// process's TCP relay server. This process is spawned by Claude CLI when
/// `--connect` is passed — it reads from stdin, forwards to the TCP relay,
/// and writes responses to stdout.
async fn run_mcp_stdio_relay(port: u16) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let stream = TcpStream::connect(("127.0.0.1", port)).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to connect to gen MCP relay on port {}: {}\n\
             Make sure localgpt-gen is running in interactive mode.",
            port,
            e
        )
    })?;

    let (tcp_reader, mut tcp_writer) = stream.into_split();
    let mut tcp_lines = BufReader::new(tcp_reader).lines();

    let stdin = tokio::io::stdin();
    let mut stdin_lines = BufReader::new(stdin).lines();

    let mut stdout = tokio::io::stdout();

    // Bidirectional relay: stdin → TCP, TCP → stdout
    loop {
        tokio::select! {
            // stdin → TCP (requests from Claude CLI)
            line = stdin_lines.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let mut buf = line;
                        buf.push('\n');
                        if tcp_writer.write_all(buf.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) => break, // stdin closed
                    Err(_) => break,
                }
            }
            // TCP → stdout (responses from gen process)
            line = tcp_lines.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let mut buf = line;
                        buf.push('\n');
                        if stdout.write_all(buf.as_bytes()).await.is_err() {
                            break;
                        }
                        stdout.flush().await.ok();
                    }
                    Ok(None) => break, // TCP closed
                    Err(_) => break,
                }
            }
        }
    }

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
        bridge.clone(),
    ));
    tools.extend(localgpt_gen::mcp::multifile_tools::create_multifile_tools(
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

    // When using a CLI backend (claude-cli, codex, gemini-cli), override its MCP
    // config so it uses `localgpt-gen mcp-server --connect` to relay tool calls
    // to the EXISTING Bevy window instead of spawning a new one.
    let is_cli_backend = config.agent.default_model.starts_with("claude-cli")
        || config.agent.default_model.starts_with("codex")
        || config.agent.default_model.starts_with("gemini-cli");

    if is_cli_backend {
        let gen_binary =
            std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("localgpt-gen"));
        let mcp_config = serde_json::json!({
            "mcpServers": {
                "localgpt-gen": {
                    "command": gen_binary.to_string_lossy(),
                    "args": ["mcp-server", "--connect"]
                }
            }
        });

        eprintln!(
            "CLI backend detected ({}). Gen tools will route to this window via MCP relay.",
            config.agent.default_model
        );

        let cli_config = config.providers.claude_cli.get_or_insert_with(|| {
            localgpt_core::config::ClaudeCliConfig {
                command: "claude".to_string(),
                model: config.agent.default_model.clone(),
                effort: "max".to_string(),
                mcp_config_override: None,
            }
        });
        cli_config.mcp_config_override = Some(mcp_config.to_string());
    }

    let workspace = config.workspace_path();

    // Create agent with combined tools
    let mut agent = Agent::new_with_tools(config.clone(), agent_id, memory, tools)?;
    agent.new_session().await?;

    // Inject gen-specific memory guidance so the LLM learns creative preferences
    agent.add_user_message(gen3d::system_prompt::GEN_MEMORY_PROMPT);

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
    println!("  WASD/Arrows   Move (forward/back/strafe)");
    println!("  Space         Move up / Jump (player mode)");
    println!("  Shift         Move down / Run (player mode)");
    println!("  Right-click   Hold + drag to look around");
    println!("  Scroll wheel  Adjust movement speed");
    println!("  Tab           Toggle free-fly / avatar camera");
    println!("  V             Toggle 1st/3rd person (avatar mode)");
    println!("  F1            Toggle inspector overlay");
    println!("  G             Toggle gallery overlay");
    println!("  E             Interact with NPC / object");
    println!("  1-5           Select dialogue choice");
    println!("  Escape        Deselect entity (inspector)");
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
