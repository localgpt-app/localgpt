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
use localgpt_core::text::{prefix_chars, prefix_chars_with_ellipsis};
use std::io::{IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};

// Use library modules
use localgpt_gen::character_tools;
use localgpt_gen::desktop::{AgentChannels, ChatEvent, ChatSink, PanelChannels, PanelSettings};
use localgpt_gen::gen3d;
use localgpt_gen::mcp_server;

/// Host-side net hooks passed into the agent loop.
/// Degrades to a unit option when the `multiplayer` feature is off.
#[cfg(feature = "multiplayer")]
type AgentNetHooksOpt = Option<localgpt_gen::net::host::AgentNetHooks>;
#[cfg(not(feature = "multiplayer"))]
type AgentNetHooksOpt = Option<()>;

/// Result of handling a slash command.
enum CommandResult {
    /// Continue the interactive loop.
    Continue,
    /// Exit the loop.
    Quit,
    /// Send the message to the agent.
    SendMessage(String),
}

fn short_session_id(id: &str) -> String {
    prefix_chars(id, 8)
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
            #[cfg(feature = "local-llm")]
            {
                println!("  gguf/*          - Run a GGUF model in-process (gguf/default)");
                for model in localgpt_gen::local_llm::available_models() {
                    println!("                    {model}");
                }
            }
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
                                short_session_id(&session.id),
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
                                        short_session_id(&full_id),
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
                                short_session_id(&result.session_id),
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
                                    let prompt_preview =
                                        localgpt_gen::experiment::prompt_preview(&exp.prompt, 50);
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
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if query.is_empty() {
        return prefix_chars_with_ellipsis(&normalized, max_len);
    }

    let lower_content = normalized.to_lowercase();
    let lower_query = query.to_lowercase();

    if let Some(pos) = lower_content.find(&lower_query) {
        let match_start = lowercase_byte_to_original_byte(&normalized, pos);
        let match_end = lowercase_byte_to_original_byte(&normalized, pos + lower_query.len());
        let query_chars = query.chars().count();
        let context_chars = max_len.saturating_sub(query_chars) / 2;
        let start = retreat_chars(&normalized, match_start, context_chars);
        let end = advance_chars(&normalized, match_end, context_chars);
        let snippet = &normalized[start..end];

        let prefix = if start > 0 { "..." } else { "" };
        let suffix = if end < normalized.len() { "..." } else { "" };

        format!("{}{}{}", prefix, snippet.trim(), suffix)
    } else {
        prefix_chars_with_ellipsis(&normalized, max_len)
    }
}

fn lowercase_byte_to_original_byte(value: &str, lowercase_byte: usize) -> usize {
    let mut lowered_len = 0;
    for (original_idx, ch) in value.char_indices() {
        if lowered_len >= lowercase_byte {
            return original_idx;
        }
        lowered_len += ch.to_lowercase().map(char::len_utf8).sum::<usize>();
    }
    value.len()
}

fn retreat_chars(value: &str, byte_index: usize, char_count: usize) -> usize {
    let mut index = byte_index.min(value.len());
    for _ in 0..char_count {
        if index == 0 {
            break;
        }
        index = value[..index]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0);
    }
    index
}

fn advance_chars(value: &str, byte_index: usize, char_count: usize) -> usize {
    let mut index = byte_index.min(value.len());
    for _ in 0..char_count {
        let Some((offset, ch)) = value[index..].char_indices().next() else {
            break;
        };
        index += offset + ch.len_utf8();
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_snippet_handles_multibyte_context() {
        let content = format!("{} marker {}", "✅".repeat(12), "界".repeat(12));

        let snippet = extract_snippet(&content, "marker", 12);

        assert!(snippet.contains("marker"));
        assert!(snippet.starts_with("..."));
        assert!(snippet.ends_with("..."));
    }

    #[test]
    fn extract_snippet_maps_lowercase_offsets_back_to_original_text() {
        let content = format!("{} marker", "İ".repeat(8));

        let snippet = extract_snippet(&content, "marker", 12);

        assert!(snippet.contains("marker"));
    }

    #[test]
    fn extract_snippet_truncates_multibyte_fallback_by_characters() {
        let snippet = extract_snippet(&"✅".repeat(4), "missing", 2);

        assert_eq!(snippet, "✅✅...");
    }

    #[test]
    fn extract_snippet_flattens_whitespace() {
        let snippet = extract_snippet("first\nsecond\tthird", "missing", 50);

        assert_eq!(snippet, "first second third");
    }
}

/// Run a streaming chat with tool call display.
///
/// This mirrors the CLI mode's streaming chat behavior:
/// - Streams response chunks in real-time
/// - Shows tool calls with detail extraction
/// - Displays execution status for each tool
///
/// Returns the accumulated assistant response text (possibly partial on
/// stream errors) so collaborative clients can receive the reply.
///
/// With a `sink`, the turn is also reported to the in-window prompt panel.
async fn streaming_chat(agent: &mut Agent, input: &str, sink: Option<&ChatSink>) -> Result<String> {
    let (response, _model_error) = streaming_chat_reporting(agent, input, sink).await?;
    Ok(response)
}

/// Like [`streaming_chat`], but also reports a model/stream error (which
/// `streaming_chat` only prints) so callers can mark the turn as failed —
/// used for collaborative jobs, whose scaffolds show success or failure.
async fn streaming_chat_reporting(
    agent: &mut Agent,
    input: &str,
    sink: Option<&ChatSink>,
) -> Result<(String, Option<String>)> {
    let result = run_streaming_turn(agent, input, sink).await;
    if let Some(sink) = sink {
        let error = match &result {
            Ok((_, model_error)) => model_error.clone(),
            Err(e) => Some(e.to_string()),
        };
        sink.send(ChatEvent::TurnFinished { error });
    }
    result
}

async fn run_streaming_turn(
    agent: &mut Agent,
    input: &str,
    sink: Option<&ChatSink>,
) -> Result<(String, Option<String>)> {
    print!("\nLocalGPT: ");
    std::io::stdout().flush().ok();

    let mut full_response = String::new();
    let mut model_error = None;

    match agent.chat_stream_with_images(input, vec![]).await {
        Ok(mut stream) => {
            let mut pending_tool_calls = None;

            // Stream response chunks
            while let Some(result) = stream.next().await {
                match result {
                    Ok(chunk) => {
                        print!("{}", chunk.delta);
                        std::io::stdout().flush().ok();
                        full_response.push_str(&chunk.delta);
                        if let Some(sink) = sink
                            && !chunk.delta.is_empty()
                        {
                            sink.send(ChatEvent::Delta(chunk.delta.clone()));
                        }

                        if chunk.done && chunk.tool_calls.is_some() {
                            pending_tool_calls = chunk.tool_calls;
                        }
                    }
                    Err(e) => {
                        eprintln!("\nStream error: {}", e);
                        model_error = Some(e.to_string());
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
                let start_sink = sink.cloned();
                let end_sink = sink.cloned();
                let (follow_up, _warnings) = agent
                    .execute_streaming_tool_calls(
                        &full_response,
                        tool_calls,
                        move |name, args| {
                            let detail = extract_tool_detail(name, args);
                            if let Some(ref d) = detail {
                                print!("\n> Running: {} ({}) ... ", name, d);
                            } else {
                                print!("\n> Running: {} ... ", name);
                            }
                            std::io::stdout().flush().ok();
                            if let Some(sink) = &start_sink {
                                sink.send(ChatEvent::ToolStarted {
                                    name: name.to_string(),
                                    detail,
                                });
                            }
                        },
                        move |name, result| {
                            match result {
                                Ok(()) => print!("Done."),
                                Err(e) => print!("Failed: {}", e),
                            }
                            if let Some(sink) = &end_sink {
                                sink.send(ChatEvent::ToolFinished {
                                    name: name.to_string(),
                                    error: result.err().map(str::to_string),
                                });
                            }
                        },
                    )
                    .await?;

                // The model's reply after its tool calls.
                if !follow_up.trim().is_empty() {
                    print!("\nLocalGPT: {}", follow_up);
                    if let Some(sink) = sink {
                        sink.send(ChatEvent::Delta(follow_up.clone()));
                    }
                    if !full_response.is_empty() {
                        full_response.push_str("\n\n");
                    }
                    full_response.push_str(&follow_up);
                }

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
            model_error = Some(e.to_string());
        }
    }

    Ok((full_response, model_error))
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

    /// Run as a desktop app: type prompts in a panel inside the window
    /// instead of the terminal. Automatic when Gen isn't started from a
    /// terminal (for example, from the macOS app bundle).
    #[arg(long)]
    desktop: bool,

    /// Host a collaborative session on the LAN (listen server + mDNS
    /// announcement). Others join as read-only viewers with --join.
    #[cfg(feature = "multiplayer")]
    #[arg(long, group = "net_mode")]
    host: bool,

    /// Session name shown in mDNS discovery (with --host).
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host")]
    session_name: Option<String>,

    /// Join a collaborative session. Pass the host address (host:port or
    /// bare host — default port 9879), or omit the value to discover
    /// sessions on the LAN via mDNS.
    #[cfg(feature = "multiplayer")]
    #[arg(long, num_args = 0..=1, default_missing_value = None, group = "net_mode")]
    join: Option<Option<String>>,

    /// UDP port for hosted sessions (with --host).
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host", default_value_t = 9879)]
    port: u16,

    /// Area-of-interest radius in 64-unit chunks (with --join): the host
    /// streams full detail within this many chunks of your camera; farther
    /// chunks show as low-poly impostors.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "join", default_value_t = 2, value_parser = clap::value_parser!(u8).range(0..=8))]
    view_radius: u8,

    /// Disable static mesh baking on the client (with --join).
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "join")]
    no_bake: bool,

    /// Session PIN shown on the host's console (with --join). Prompted for
    /// if the host requires pairing and this is omitted.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "join")]
    pin: Option<String>,

    /// Host an OPEN session: no PIN, public netcode key — anyone on the LAN
    /// with localgpt-gen can join (with --host). Trusted networks only.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host")]
    open: bool,

    /// What connected clients' prompts may do (with --host). `safe`
    /// (default): a separate agent with scene-editing tools only — no
    /// shell, files, memory, web, or disk writes. `full`: the host's own
    /// agent with all of its tools, including shell access.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host", value_enum, default_value_t = RemoteTools::Safe)]
    remote_tools: RemoteTools,
}

/// Tool access for prompts from collaborative clients (`--remote-tools`).
#[cfg(feature = "multiplayer")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum RemoteTools {
    Safe,
    Full,
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

    // `gguf/<name>` models run in-process (see src/local_llm.rs); register
    // the provider before any agent is created.
    #[cfg(feature = "local-llm")]
    localgpt_gen::local_llm::register();

    // Desktop mode: the interactive app (or a --join viewer) without a
    // terminal to type in, so prompts come from a panel in the window.
    // Automatic when stdin isn't a terminal (a double-click, the macOS app
    // bundle, or a viewer launched from the panel's Join); --desktop forces it.
    let launched_from_terminal = std::io::stdin().is_terminal();
    let desktop = cli.command.is_none() && (cli.desktop || !launched_from_terminal);
    if cli.desktop && cli.command.is_some() {
        anyhow::bail!("--desktop only applies to interactive mode (no subcommand)");
    }

    // Apps opened from Finder inherit launchd's minimal PATH, which hides CLI
    // backends (claude, gemini, codex) installed with Homebrew, npm, or into
    // ~/.local/bin. Adopt the login shell's PATH before anything spawns them.
    // A failure is logged once logging is set up, below.
    #[cfg(unix)]
    let login_path_failed = desktop
        && !launched_from_terminal
        && match localgpt_gen::desktop::shell_env::login_shell_path() {
            Some(login_path) => {
                let current = std::env::var("PATH").unwrap_or_default();
                let merged = localgpt_gen::desktop::shell_env::merge_paths(&current, &login_path);
                // SAFETY: called at program start before any threads are spawned.
                unsafe { std::env::set_var("PATH", merged) };
                false
            }
            None => true,
        };
    #[cfg(not(unix))]
    let login_path_failed = false;

    // Initialize logging before handing off to Bevy.
    // Use "warn" by default for cleaner interactive TUI, "debug" with --verbose.
    //
    // For the interactive Bevy+REPL mode (no subcommand), we grab a rustyline
    // editor up front so its ExternalPrinter can route tracing output through
    // the REPL — async warnings (Bevy render, Ollama, etc.) no longer clobber
    // the `You:` prompt mid-typing. Other subcommands (headless, mcp-server,
    // control) have no REPL to protect, so they log straight to stderr.
    // Desktop mode has no terminal at all, so it logs to a file.
    let log_level = if cli.verbose { "debug" } else { "warn" };
    let mut repl_editor: Option<rustyline::DefaultEditor> = None;
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    if desktop {
        match open_desktop_log() {
            Some(file) => tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .init(),
            None => tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(std::io::stderr)
                .init(),
        }
    } else if cli.command.is_none() {
        let wired = rustyline::DefaultEditor::new()
            .ok()
            .and_then(|mut ed| ed.create_external_printer().ok().map(|p| (ed, p)));
        if let Some((ed, printer)) = wired {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(localgpt_gen::tracing_printer::SharedPrinter::new(printer))
                .init();
            repl_editor = Some(ed);
        } else {
            // No tty / headless stdin — fall back to stderr.
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(std::io::stderr)
                .init();
        }
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(std::io::stderr)
            .init();
    }

    if login_path_failed {
        tracing::warn!(
            "Couldn't read your login shell's PATH; CLI backends installed outside the \
             system PATH may not be found"
        );
    }

    // Load config early so both Bevy and agent threads can use it
    let config = localgpt_core::config::Config::load()?;
    let workspace = config.workspace_path();

    // Multiplayer client mode (--join): slim viewer app, no gen subsystems.
    #[cfg(feature = "multiplayer")]
    if cli.join.is_some() {
        if cli.command.is_some() {
            anyhow::bail!("--join cannot be combined with a subcommand");
        }
        return run_join_mode(&cli, desktop);
    }
    #[cfg(feature = "multiplayer")]
    if cli.host && cli.command.is_some() {
        anyhow::bail!("--host cannot be combined with a subcommand");
    }

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
                run_bevy_app(channels, workspace, initial_scene, None)
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

            // The in-window prompt panel: open at startup in desktop mode,
            // one F2 away otherwise.
            let (panel_channels, agent_channels) = localgpt_gen::desktop::create_chat_channels();
            let panel = Some((
                panel_channels,
                PanelSettings {
                    open: desktop,
                    focus_input: desktop,
                    config_file: Some(config.paths.config_file()),
                },
            ));

            // The host plugin is always installed so a session can be started
            // from the prompt panel at any time; `--host` just starts one
            // before the first frame. Until then it stays dormant.
            #[cfg(feature = "multiplayer")]
            let (host_options, agent_net): (
                localgpt_gen::net::host::NetHostOptions,
                AgentNetHooksOpt,
            ) = {
                let (mut opts, hooks) = localgpt_gen::net::host::create_host_channels();
                opts.session_name = cli
                    .session_name
                    .clone()
                    .unwrap_or_else(localgpt_gen::net::default_session_name);
                opts.port = cli.port;
                opts.open = cli.open;
                opts.full_access = cli.host && cli.remote_tools == RemoteTools::Full;
                opts.autostart = cli.host;
                if opts.full_access {
                    eprintln!(
                        "WARNING: --remote-tools full — connected clients' prompts run with this \
                         agent's full tool access, including shell commands on this machine."
                    );
                }
                (opts, Some(hooks))
            };
            #[cfg(not(feature = "multiplayer"))]
            let agent_net: AgentNetHooksOpt = None;

            // Enable MCP relay when explicitly requested or when using a CLI backend
            // (claude-cli, gemini-cli, codex-cli spawn subprocesses that need MCP access)
            let model = &config.agent.default_model;
            let enable_relay = cli.mcp_relay
                || model.starts_with("claude-cli/")
                || model.starts_with("gemini-cli/")
                || model.starts_with("codex-cli/");

            // Spawn tokio runtime + agent loop + MCP relay on a background thread
            // (Bevy must own the main thread for windowing/GPU on macOS).
            // Move the REPL editor into the agent thread — it was created up
            // front so its ExternalPrinter could be wired into tracing.
            let editor_for_agent = repl_editor.take();
            let failure_sink = agent_channels.sink.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime for gen agent");

                let outcome = rt.block_on(async move {
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

                    run_agent_loop(
                        bridge_for_agent,
                        &agent_id,
                        initial_prompt,
                        relay_config,
                        editor_for_agent,
                        agent_net,
                        agent_channels,
                        desktop,
                    )
                    .await
                });

                if let Err(e) = outcome {
                    tracing::error!("Gen agent loop error: {}", e);
                    if desktop {
                        // No terminal to print to: say what went wrong in the
                        // panel and leave the window open to read it.
                        eprintln!("Gen agent loop error: {e}");
                        failure_sink.send(ChatEvent::Failed(format!("{e:#}")));
                        return;
                    }
                }

                // REPL exited (/quit, Ctrl+D, or error). The Bevy window is
                // still blocking the main thread, so the process would hang
                // here without an explicit exit. Clean up the relay port
                // (duplicated from the main-thread cleanup below because we
                // won't reach it) and terminate.
                if enable_relay {
                    gen3d::mcp_relay::cleanup_relay_port();
                }
                std::process::exit(0);
            });

            // Run Bevy on the main thread (blocks until window closes)
            #[cfg(feature = "multiplayer")]
            let result = run_host_bevy_app(channels, workspace, initial_scene, host_options, panel);
            #[cfg(not(feature = "multiplayer"))]
            let result = run_bevy_app(channels, workspace, initial_scene, panel);

            // Clean up relay port file so stale ports aren't discovered
            if enable_relay {
                gen3d::mcp_relay::cleanup_relay_port();
            }

            result
        }
    }
}

/// The prompt panel's channels and startup settings, when the app has one.
type PanelSetup = Option<(PanelChannels, PanelSettings)>;

fn add_prompt_panel(app: &mut bevy::prelude::App, panel: PanelSetup) {
    if let Some((channels, settings)) = panel {
        app.add_plugins(localgpt_gen::desktop::PromptPanelPlugin::new(
            channels, settings,
        ));
    }
}

/// Set up and run the Bevy application on the main thread.
fn run_bevy_app(
    channels: gen3d::GenChannels,
    workspace: std::path::PathBuf,
    initial_scene: Option<PathBuf>,
    panel: PanelSetup,
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
    add_prompt_panel(&mut app, panel);

    app.run();

    Ok(())
}

/// Set up and run the host Bevy application: the full gen app plus the
/// listen-server plugin (authoritative ECS + replication + mDNS).
#[cfg(feature = "multiplayer")]
fn run_host_bevy_app(
    channels: gen3d::GenChannels,
    workspace: std::path::PathBuf,
    initial_scene: Option<PathBuf>,
    options: localgpt_gen::net::host::NetHostOptions,
    panel: PanelSetup,
) -> Result<()> {
    use bevy::prelude::*;

    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    // host_lifecycle retitles it once a session starts.
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
    app.add_plugins(localgpt_gen::net::host::NetHostPlugin {
        options: std::sync::Mutex::new(Some(options)),
    });
    add_prompt_panel(&mut app, panel);

    app.run();

    Ok(())
}

/// Run the collaborative client app: a slim viewer with its own camera that
/// renders replicated state and forwards prompts to the host.
#[cfg(feature = "multiplayer")]
fn run_client_app(
    server_addr: std::net::SocketAddr,
    prompt_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    view_radius: u8,
    bake: bool,
    connect_token: Option<Vec<u8>>,
    panel_prompt_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
) -> Result<()> {
    use bevy::prelude::*;

    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "LocalGPT Gen — Client".into(),
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

    app.add_plugins(localgpt_gen::net::client::NetClientPlugin {
        options: std::sync::Mutex::new(Some(localgpt_gen::net::client::NetClientOptions {
            server_addr,
            prompt_rx,
            view_radius,
            bake,
            connect_token,
        })),
    });
    if let Some(prompt_tx) = panel_prompt_tx {
        app.add_plugins(localgpt_gen::desktop::ClientPanelPlugin::new(
            prompt_tx,
            server_addr,
        ));
    }

    app.run();

    Ok(())
}

/// `--join` mode: resolve the host address (mDNS browse when unspecified),
/// start the prompt REPL, and run the client app.
///
/// In `desktop` mode (no terminal — typically a viewer the host-or-join
/// panel launched) prompts come from an in-window panel instead of the
/// REPL, and an already-paired connect token may arrive in
/// [`localgpt_gen::net::JOIN_TOKEN_ENV`].
#[cfg(feature = "multiplayer")]
fn run_join_mode(cli: &Cli, desktop: bool) -> Result<()> {
    use std::net::SocketAddr;

    let addr: SocketAddr = match cli.join.clone().flatten() {
        Some(spec) => localgpt_gen::net::parse_peer_addr(&spec)?,
        None => {
            eprintln!("Browsing the LAN for collaborative sessions…");
            let sessions = localgpt_gen::net::mdns::browse_sessions(
                std::time::Duration::from_secs(3),
                localgpt_gen::net::PROTOCOL_ID,
            )?;
            if sessions.is_empty() {
                anyhow::bail!(
                    "No sessions found. Host one with `localgpt-gen --host`, then join it,\n\
                     or connect directly: localgpt-gen --join 192.168.1.5:9879"
                );
            }
            for session in &sessions {
                eprintln!("  found: {} — {}", session.session_name, session.addr);
            }
            let chosen = &sessions[0];
            eprintln!("Joining '{}' at {}", chosen.session_name, chosen.addr);
            chosen.addr
        }
    };

    let connect_token = match std::env::var(localgpt_gen::net::JOIN_TOKEN_ENV) {
        // Paired already by the panel that launched us; don't pair twice.
        Ok(token) if !token.is_empty() => {
            use base64::Engine as _;
            // SAFETY: called before any threads are spawned.
            unsafe { std::env::remove_var(localgpt_gen::net::JOIN_TOKEN_ENV) };
            Some(
                base64::engine::general_purpose::STANDARD
                    .decode(token.trim())
                    .map_err(|e| {
                        anyhow::anyhow!("invalid {}: {e}", localgpt_gen::net::JOIN_TOKEN_ENV)
                    })?,
            )
        }
        _ if desktop => pair_for_join_without_terminal(addr, cli.pin.as_deref())?,
        _ => pair_for_join(addr, cli.pin.as_deref())?,
    };

    let (prompt_tx, prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    if desktop {
        return run_client_app(
            addr,
            prompt_rx,
            cli.view_radius,
            !cli.no_bake,
            connect_token,
            Some(prompt_tx),
        );
    }

    // Prompt REPL on a background thread — lines flow to the net systems.
    std::thread::spawn(move || {
        let Ok(mut rl) = rustyline::DefaultEditor::new() else {
            eprintln!("Failed to open input editor — prompts unavailable");
            return;
        };
        loop {
            match rl.readline("You: ") {
                Ok(line) => {
                    let text = line.trim().to_string();
                    if !text.is_empty() {
                        let _ = prompt_tx.send(text);
                    }
                }
                Err(_) => {
                    // Ctrl+D or editor error — the Bevy window keeps running;
                    // exiting the process is the only way out of the REPL.
                    std::process::exit(0);
                }
            }
        }
    });

    run_client_app(
        addr,
        prompt_rx,
        cli.view_radius,
        !cli.no_bake,
        connect_token,
        None,
    )
}

/// Pairing for a viewer with no terminal to type a PIN into: open sessions
/// need nothing, PIN sessions need `--pin` (the panel pairs on the viewer's
/// behalf instead, see `JOIN_TOKEN_ENV`).
#[cfg(feature = "multiplayer")]
fn pair_for_join_without_terminal(
    addr: std::net::SocketAddr,
    pin_arg: Option<&str>,
) -> Result<Option<Vec<u8>>> {
    let info = localgpt_gen::net::pairing::fetch_info(&format!("http://{addr}"))
        .map_err(|e| anyhow::anyhow!("Couldn't reach the host at tcp://{addr} ({e})"))?;
    if info.pairing_required && pin_arg.is_none() {
        anyhow::bail!(
            "This session needs a PIN. Join it from Gen's Collaborate panel, or pass --pin."
        );
    }
    pair_for_join(addr, pin_arg)
}

/// Obtain a connect token from the host's pairing endpoint (TCP, same port
/// number as the session). Returns `None` for `--open` sessions.
#[cfg(feature = "multiplayer")]
fn pair_for_join(addr: std::net::SocketAddr, pin_arg: Option<&str>) -> Result<Option<Vec<u8>>> {
    use localgpt_gen::net::pairing::{PairError, fetch_info, pair_with_host};

    let http_base = format!("http://{addr}");
    let info = fetch_info(&http_base).map_err(|e| {
        anyhow::anyhow!(
            "Couldn't reach the host's session endpoint at tcp://{addr} ({e}).\n\
             Is the host running the same localgpt-gen version, and is TCP port {} open?",
            addr.port()
        )
    })?;
    if info.protocol_id != localgpt_gen::net::PROTOCOL_ID {
        anyhow::bail!(
            "Host speaks protocol {} but this client speaks {} — use matching localgpt-gen versions",
            info.protocol_id,
            localgpt_gen::net::PROTOCOL_ID
        );
    }
    if !info.pairing_required {
        eprintln!("Joining an OPEN session (no PIN).");
        return Ok(None);
    }

    let interactive = pin_arg.is_none();
    let mut editor = None;
    for attempt in 1..=3 {
        let pin = match pin_arg {
            Some(pin) => pin.to_string(),
            None => {
                let rl = match &mut editor {
                    Some(rl) => rl,
                    None => editor.insert(rustyline::DefaultEditor::new()?),
                };
                rl.readline("Session PIN (shown on the host's console): ")?
            }
        };
        match pair_with_host(&http_base, addr, &pin) {
            Ok(token) => {
                eprintln!("Paired with host.");
                let bytes = token
                    .try_into_bytes()
                    .map_err(|e| anyhow::anyhow!("connect token: {e}"))?;
                return Ok(Some(bytes.to_vec()));
            }
            Err(PairError::WrongPin) if interactive && attempt < 3 => {
                eprintln!("Wrong PIN — try again.");
            }
            Err(e) => anyhow::bail!("Pairing failed: {e}"),
        }
    }
    anyhow::bail!("Pairing failed: wrong PIN")
}

/// The desktop-mode log (`<state dir>/logs/gen-desktop.log`, appended to),
/// since there's no terminal to log to. `None` if it can't be opened.
fn open_desktop_log() -> Option<std::fs::File> {
    let dir = localgpt_core::paths::Paths::resolve().ok()?.logs_dir();
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("gen-desktop.log"))
        .ok()
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
                render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                    bevy::render::settings::WgpuSettings {
                        // Allow software rendering on headless servers
                        backends: Some(bevy::render::settings::Backends::all()),
                        ..default()
                    },
                )),
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

    let response_preview = localgpt_gen::experiment::prompt_preview(&response, 200);
    tracing::info!("Agent response: {}", response_preview);

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
        streaming_chat(&mut agent, &prompt, None).await?;
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

        streaming_chat(&mut agent, input, None).await?;
        println!();
    }

    Ok(())
}

/// Which agent runs prompts from collaborative clients.
#[cfg(feature = "multiplayer")]
enum RemoteWorker {
    /// `--remote-tools full`: the host's own agent.
    Host,
    /// Default: a separate scene-only agent.
    Scoped(Box<Agent>),
    /// Remote prompts are refused with this reason.
    Disabled(String),
}

/// Briefing for the scene-only agent that serves remote collaborators.
#[cfg(feature = "multiplayer")]
const REMOTE_AGENT_PROMPT: &str = "You are building inside a shared 3D world on behalf of \
collaborators connected over the network. You can only use scene-editing tools; you cannot \
run commands, read or write files, browse the web, or access anyone's memory or notes. \
Politely decline requests that need those abilities.";

/// Build the scene-only agent for remote prompts (`--remote-tools safe`).
///
/// Isolation: scene tools only, each path-scoped (see
/// `localgpt_gen::net::remote_scope`); a separate memory workspace so the
/// host's MEMORY.md / daily logs never enter a conversation remote users
/// steer; a fresh LLM session. For Claude CLI the built-in tools are
/// disabled and MCP points at a dedicated relay serving the same scoped
/// tools. Subprocess backends whose built-in tools can't be restricted
/// (Gemini CLI, Codex CLI) are refused.
#[cfg(feature = "multiplayer")]
async fn build_scoped_remote_agent(
    bridge: std::sync::Arc<gen3d::GenBridge>,
    config: &localgpt_core::config::Config,
) -> Result<Agent> {
    use localgpt_core::memory::MemoryManager;
    use localgpt_gen::net::remote_scope::create_remote_scene_tools;

    let model = config.agent.default_model.clone();
    if model.starts_with("gemini-cli") || model.starts_with("codex") {
        anyhow::bail!(
            "the {model} backend has built-in shell/file tools that can't be restricted. \
             Use an API provider or claude-cli, or host with --remote-tools full \
             (gives remote users full tool access)"
        );
    }

    let mut remote_config = config.clone();
    remote_config.paths.workspace = remote_config.paths.data_dir.join("gen-remote-workspace");
    remote_config.memory.embedding_provider = "none".to_string();

    if model.starts_with("claude-cli") {
        let port =
            gen3d::mcp_relay::start_scoped_relay(create_remote_scene_tools(bridge.clone())).await?;
        let gen_binary =
            std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("localgpt-gen"));
        let mcp_config = serde_json::json!({
            "mcpServers": {
                "localgpt-gen": {
                    "command": gen_binary.to_string_lossy(),
                    "args": ["mcp-server", "--connect", port.to_string()]
                }
            }
        });
        let cli = remote_config.providers.claude_cli.get_or_insert_with(|| {
            localgpt_core::config::ClaudeCliConfig {
                command: "claude".to_string(),
                model: model.clone(),
                effort: "max".to_string(),
                mcp_config_override: None,
                builtin_tools: None,
            }
        });
        cli.mcp_config_override = Some(mcp_config.to_string());
        // No Bash/Read/Write/…: only the scoped MCP tools remain.
        cli.builtin_tools = Some(String::new());
    }

    let memory = std::sync::Arc::new(MemoryManager::new_with_full_config(
        &remote_config.memory,
        Some(&remote_config),
        "gen-remote",
    )?);
    let tools = create_remote_scene_tools(bridge);
    let mut agent = Agent::new_with_tools(remote_config, "gen-remote", memory, tools)?;
    // Also resets any CLI session the provider resumed at construction.
    agent.new_session().await?;
    agent.add_user_message(REMOTE_AGENT_PROMPT);
    Ok(agent)
}

/// Input sources merged into the agent loop.
enum AgentInput {
    /// Line typed at the local REPL.
    Local(String),
    /// Prompt typed in the in-window panel.
    Panel(String),
    /// REPL closed (Ctrl+D or error).
    Eof,
    /// Queued prompt job dispatched from a connected collaborative client.
    #[cfg(feature = "multiplayer")]
    Remote(localgpt_gen::net::host::RemoteJob),
    /// A collaborative session went live; build the remote-prompt worker.
    #[cfg(feature = "multiplayer")]
    HostingStarted { full_access: bool },
}

/// What the agent loop should do with a slash command typed in the panel.
enum PanelCommand {
    /// Handled (or explained) in the panel.
    Handled,
    /// Quit Gen.
    Quit,
}

/// Slash commands typed in the prompt panel. [`handle_gen_command`] prints
/// its results, which nobody sees without a terminal, so the commands that
/// make sense in the window are handled here and the rest are explained.
async fn panel_command(input: &str, agent: &mut Agent, sink: &ChatSink) -> PanelCommand {
    let mut words = input.split_whitespace();
    let command = words.next().unwrap_or_default();
    let argument = words.collect::<Vec<_>>().join(" ");
    match command {
        "/quit" | "/exit" | "/q" => PanelCommand::Quit,
        "/model" if argument.is_empty() => {
            sink.send(ChatEvent::Notice(format!(
                "Current model: {}",
                agent.model()
            )));
            PanelCommand::Handled
        }
        "/model" => {
            match agent.set_model(&argument) {
                Ok(()) => {
                    sink.send(ChatEvent::Ready {
                        model: agent.model().to_string(),
                    });
                    sink.send(ChatEvent::Notice(format!("Now using {}.", agent.model())));
                }
                Err(e) => sink.send(ChatEvent::Warning(format!(
                    "Couldn't switch to {argument}: {e}"
                ))),
            }
            PanelCommand::Handled
        }
        "/new" => {
            if let Err(e) = agent.save_session_to_memory().await {
                tracing::warn!("Failed to save session to memory: {e}");
            }
            match agent.new_session().await {
                Ok(()) => sink.send(ChatEvent::Notice(
                    "Started a new conversation. The world stays as it is.".into(),
                )),
                Err(e) => sink.send(ChatEvent::Warning(format!(
                    "Couldn't start a new conversation: {e}"
                ))),
            }
            PanelCommand::Handled
        }
        "/clear" => {
            agent.clear_session();
            sink.send(ChatEvent::Notice(
                "Cleared the conversation. The world stays as it is.".into(),
            ));
            PanelCommand::Handled
        }
        _ => {
            sink.send(ChatEvent::Notice(format!(
                "{command} prints its results in a terminal. Here you can use /model <name>, \
                 /new, /clear, and /quit, and press G for the world gallery. Run localgpt-gen \
                 from a terminal for the rest."
            )));
            PanelCommand::Handled
        }
    }
}

/// Which CLI backend a model string selects, if any (matching the checks
/// that set up the MCP relay in [`run_agent_loop`]).
fn cli_family(model: &str) -> Option<&'static str> {
    if model.starts_with("claude-cli") {
        Some("claude-cli")
    } else if model.starts_with("gemini-cli") {
        Some("gemini-cli")
    } else if model.starts_with("codex") {
        Some("codex")
    } else {
        None
    }
}

/// A first-run hint when the configured model is a CLI backend whose
/// program can't be found, so desktop users learn why nothing happens
/// before they type.
fn missing_cli_backend_hint(config: &localgpt_core::config::Config) -> Option<String> {
    use localgpt_gen::desktop::models::find_on_path;

    let model = config.agent.default_model.as_str();
    let providers = &config.providers;
    let (label, program) = if model.starts_with("claude-cli") {
        let command = providers.claude_cli.as_ref().map(|c| c.command.clone());
        ("Claude CLI", command.unwrap_or_else(|| "claude".into()))
    } else if model.starts_with("gemini-cli") {
        let command = providers.gemini_cli.as_ref().map(|c| c.command.clone());
        ("Gemini CLI", command.unwrap_or_else(|| "gemini".into()))
    } else if model.starts_with("codex-cli") || model == "codex" {
        let command = providers.codex_cli.as_ref().map(|c| c.command.clone());
        ("Codex CLI", command.unwrap_or_else(|| "codex".into()))
    } else {
        return None;
    };
    let found = Path::new(&program).is_file() || find_on_path(&program).is_some();
    (!found).then(|| {
        format!(
            "Your model is {model}, but Gen can't find the {label} (`{program}`). Install it \
             and sign in, then restart Gen, or pick another model from the menu above."
        )
    })
}

/// Run the interactive agent loop with Gen tools available.
///
/// The REPL runs on a dedicated blocking thread (rustyline reads are
/// synchronous) and feeds a merged event channel, so remote prompts from
/// connected clients can be interleaved with local input. When hosting,
/// every turn (local or remote) is echoed to connected clients as
/// [`localgpt_gen::net::protocol::HostChat`] messages.
///
/// Prompts from the in-window panel join the same stream, and every turn is
/// reported back to it. In `desktop` mode there's no terminal, so the REPL
/// isn't started and the panel is the only local input.
#[allow(clippy::too_many_arguments)]
async fn run_agent_loop(
    bridge: std::sync::Arc<gen3d::GenBridge>,
    agent_id: &str,
    initial_prompt: Option<String>,
    config: localgpt_core::config::Config,
    editor: Option<rustyline::DefaultEditor>,
    net_hooks: AgentNetHooksOpt,
    panel: AgentChannels,
    desktop: bool,
) -> Result<()> {
    use localgpt_core::agent::tools::create_safe_tools;
    use localgpt_core::agent::{Agent, create_spawn_agent_tool};
    use localgpt_core::memory::MemoryManager;
    use rustyline::error::ReadlineError;
    use std::sync::Arc;

    #[cfg(not(feature = "multiplayer"))]
    let _ = &net_hooks;

    #[cfg(feature = "multiplayer")]
    let remote_bridge = bridge.clone();

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
                builtin_tools: None,
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

    // Worker for prompts from collaborative clients. Built when a session
    // actually starts (`--host`, or Start hosting in the panel) — see
    // `AgentInput::HostingStarted` below.
    #[cfg(feature = "multiplayer")]
    let mut remote_worker = RemoteWorker::Disabled("not hosting".into());

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
    println!("  F2            Toggle the prompt panel");
    println!();

    // Tell the prompt panel what's running and what it could switch to.
    let sink = panel.sink.clone();
    sink.send(ChatEvent::Ready {
        model: agent.model().to_string(),
    });
    if let Some(hint) = missing_cli_backend_hint(&config) {
        if desktop {
            tracing::warn!("{hint}");
        } else {
            eprintln!("{hint}");
        }
        sink.send(ChatEvent::Warning(hint));
    }
    {
        let sink = sink.clone();
        let current = agent.model().to_string();
        let ollama_endpoint = config
            .providers
            .ollama
            .as_ref()
            .map(|ollama| ollama.endpoint.clone())
            .unwrap_or_else(|| localgpt_gen::desktop::models::DEFAULT_OLLAMA_ENDPOINT.to_string());
        // A CLI backend only works in a session that started with one of its
        // family: the MCP relay and its tool config are set up at startup.
        let startup_family = cli_family(&current);
        tokio::spawn(async move {
            let mut options =
                localgpt_gen::desktop::models::detect_model_options(&current, &ollama_endpoint)
                    .await;
            options.retain(|model| {
                let family = cli_family(model);
                family.is_none() || family == startup_family
            });
            sink.send(ChatEvent::ModelOptions(options));
        });
    }

    // If initial prompt given, send it
    if let Some(prompt) = initial_prompt {
        println!("\nYou: {}", prompt);
        sink.send(ChatEvent::Prompt {
            text: prompt.clone(),
            from: None,
        });
        streaming_chat(&mut agent, &prompt, Some(&sink)).await?;
        println!();
    }

    // Set up the merged input stream: REPL lines on a blocking thread
    // (terminal mode only), prompts from the window's panel, plus remote
    // client prompts when hosting.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<AgentInput>();
    {
        let mut prompt_rx = panel.prompt_rx;
        let panel_tx = event_tx.clone();
        tokio::spawn(async move {
            while let Some(prompt) = prompt_rx.recv().await {
                if panel_tx.send(AgentInput::Panel(prompt)).is_err() {
                    break;
                }
            }
        });
    }
    let repl_event_tx = event_tx.clone();
    let start_repl = !desktop;
    std::thread::spawn(move || {
        if !start_repl {
            return;
        }
        // Reuse the editor created in main() so tracing's ExternalPrinter
        // stays wired to its pipe. Fall back to a fresh editor if one
        // wasn't provided.
        let mut rl = match editor {
            Some(ed) => ed,
            None => match rustyline::DefaultEditor::new() {
                Ok(ed) => ed,
                Err(e) => {
                    eprintln!("Failed to open input editor: {e}");
                    let _ = repl_event_tx.send(AgentInput::Eof);
                    return;
                }
            },
        };
        loop {
            match rl.readline("You: ") {
                Ok(line) => {
                    let _ = rl.add_history_entry(&line);
                    if repl_event_tx.send(AgentInput::Local(line)).is_err() {
                        return;
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    println!("^C");
                }
                Err(ReadlineError::Eof) => {
                    let _ = repl_event_tx.send(AgentInput::Eof);
                    return;
                }
                Err(err) => {
                    eprintln!("Error: {:?}", err);
                    let _ = repl_event_tx.send(AgentInput::Eof);
                    return;
                }
            }
        }
    });

    // Chat is only echoed to clients while a session is live; the sender
    // waits here until hosting starts.
    #[cfg(feature = "multiplayer")]
    let mut idle_chat_tx = None;
    #[cfg(feature = "multiplayer")]
    let mut chat_tx: Option<
        tokio::sync::mpsc::UnboundedSender<localgpt_gen::net::protocol::HostChat>,
    > = None;
    #[cfg(feature = "multiplayer")]
    let job_events_tx = match net_hooks {
        Some(hooks) => {
            // Forward dispatched remote jobs into the merged input stream.
            // The host's queue hands out one job at a time, so this loop is
            // the (single) worker of the §2 inference queue.
            let mut job_rx = hooks.job_rx;
            let remote_tx = event_tx.clone();
            tokio::spawn(async move {
                while let Some(job) = job_rx.recv().await {
                    if remote_tx.send(AgentInput::Remote(job)).is_err() {
                        break;
                    }
                }
            });
            // Hosting started (from --host or the panel): build the worker.
            let mut control_rx = hooks.control_rx;
            let control_tx = event_tx.clone();
            tokio::spawn(async move {
                while let Some(localgpt_gen::net::host::HostControlEvent::HostingStarted {
                    full_access,
                }) = control_rx.recv().await
                {
                    if control_tx
                        .send(AgentInput::HostingStarted { full_access })
                        .is_err()
                    {
                        break;
                    }
                }
            });
            idle_chat_tx = Some(hooks.chat_tx);
            Some(hooks.job_events_tx)
        }
        None => None,
    };

    // Interactive loop over the merged event stream.
    loop {
        let Some(event) = event_rx.recv().await else {
            break;
        };
        let (input, from_panel) = match event {
            AgentInput::Local(line) => (line, false),
            AgentInput::Panel(line) => (line, true),
            AgentInput::Eof => break, // Ctrl+D
            #[cfg(feature = "multiplayer")]
            AgentInput::HostingStarted { full_access } => {
                remote_worker = if full_access {
                    let note = "Guests' prompts run on this agent with all of its tools.";
                    eprintln!("Remote prompts: {note}");
                    sink.send(ChatEvent::Warning(note.into()));
                    RemoteWorker::Host
                } else {
                    match build_scoped_remote_agent(remote_bridge.clone(), &config).await {
                        Ok(remote) => {
                            eprintln!(
                                "Remote prompts: scene-editing tools only (--remote-tools full \
                                 to change)"
                            );
                            sink.send(ChatEvent::Notice(
                                "Hosting. Guests' prompts run on a separate agent that can only \
                                 edit the scene."
                                    .into(),
                            ));
                            RemoteWorker::Scoped(Box::new(remote))
                        }
                        Err(e) => {
                            eprintln!("Remote prompts disabled: {e}");
                            sink.send(ChatEvent::Warning(format!(
                                "Hosting, but guests' prompts are disabled: {e}"
                            )));
                            RemoteWorker::Disabled(e.to_string())
                        }
                    }
                };
                if chat_tx.is_none() {
                    chat_tx = idle_chat_tx.take();
                }
                continue;
            }
            #[cfg(feature = "multiplayer")]
            AgentInput::Remote(job) => {
                use localgpt_gen::net::host::JobEvent;
                println!("\n[client job #{}] {}", job.job_id, job.display);
                if let Some(tx) = &job_events_tx {
                    let _ = tx.send(JobEvent::Started(job.job_id));
                }
                sink.send(ChatEvent::Prompt {
                    text: job.display.clone(),
                    from: Some(format!("Guest · job #{}", job.job_id)),
                });
                // A failed remote job must not take the host's REPL down.
                let result = match &mut remote_worker {
                    RemoteWorker::Host => {
                        streaming_chat_reporting(&mut agent, &job.agent_prompt, Some(&sink)).await
                    }
                    RemoteWorker::Scoped(remote) => {
                        streaming_chat_reporting(remote, &job.agent_prompt, Some(&sink)).await
                    }
                    RemoteWorker::Disabled(reason) => {
                        sink.send(ChatEvent::TurnFinished {
                            error: Some(reason.clone()),
                        });
                        Ok((String::new(), Some(reason.clone())))
                    }
                };
                let error = match result {
                    Ok((_, Some(model_error))) => Some(model_error),
                    Ok((reply, None)) => {
                        if let Some(chat_tx) = &chat_tx
                            && !reply.is_empty()
                        {
                            let _ = chat_tx.send(localgpt_gen::net::protocol::HostChat {
                                speaker: "host".to_string(),
                                text: reply,
                            });
                        }
                        None
                    }
                    Err(e) => {
                        eprintln!("Remote job #{} failed: {e}", job.job_id);
                        Some(e.to_string())
                    }
                };
                if let Some(tx) = &job_events_tx {
                    let _ = tx.send(JobEvent::Finished {
                        job_id: job.job_id,
                        error,
                    });
                }
                println!();
                continue;
            }
        };
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        // Who the panel should say sent this prompt (None = the panel itself).
        let sender = (!from_panel).then(|| "Terminal".to_string());

        // Handle slash commands (local only — collaborative clients can't
        // run them). Their output is printed, so the panel handles the few
        // that make sense without a terminal and explains the rest.
        if input.starts_with('/') {
            if from_panel {
                match panel_command(input, &mut agent, &sink).await {
                    PanelCommand::Handled => continue,
                    PanelCommand::Quit => break,
                }
            }
            let result = handle_gen_command(input, &mut agent, agent_id, &workspace).await;
            // Keep the panel's model label right after /model and friends.
            sink.send(ChatEvent::Ready {
                model: agent.model().to_string(),
            });
            match result {
                CommandResult::Continue => continue,
                CommandResult::Quit => break,
                CommandResult::SendMessage(msg) => {
                    sink.send(ChatEvent::Prompt {
                        text: msg.clone(),
                        from: sender,
                    });
                    let reply = streaming_chat(&mut agent, &msg, Some(&sink))
                        .await
                        .unwrap_or_else(|e| {
                            eprintln!("\nError: {e}");
                            String::new()
                        });
                    #[cfg(feature = "multiplayer")]
                    if let Some(chat_tx) = &chat_tx {
                        let _ = chat_tx.send(localgpt_gen::net::protocol::HostChat {
                            speaker: "host-user".to_string(),
                            text: msg.clone(),
                        });
                        if !reply.is_empty() {
                            let _ = chat_tx.send(localgpt_gen::net::protocol::HostChat {
                                speaker: "host".to_string(),
                                text: reply,
                            });
                        }
                    }
                    #[cfg(not(feature = "multiplayer"))]
                    let _ = &reply;
                    println!();
                }
            }
        } else {
            sink.send(ChatEvent::Prompt {
                text: input.to_string(),
                from: sender,
            });
            // A failed turn is reported (terminal and panel) but doesn't end
            // the session.
            let reply = streaming_chat(&mut agent, input, Some(&sink))
                .await
                .unwrap_or_else(|e| {
                    eprintln!("\nError: {e}");
                    String::new()
                });
            #[cfg(feature = "multiplayer")]
            if let Some(chat_tx) = &chat_tx {
                let _ = chat_tx.send(localgpt_gen::net::protocol::HostChat {
                    speaker: "host-user".to_string(),
                    text: input.to_string(),
                });
                if !reply.is_empty() {
                    let _ = chat_tx.send(localgpt_gen::net::protocol::HostChat {
                        speaker: "host".to_string(),
                        text: reply,
                    });
                }
            }
            #[cfg(not(feature = "multiplayer"))]
            let _ = &reply;
            println!();
        }
    }

    Ok(())
}
