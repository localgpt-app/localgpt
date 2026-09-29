//! Gen's agent loop, for every binary with Gen's shell.
//!
//! `localgpt-gen` and the one-window `localgpt-app` run the same loop: the
//! prompt panel's slash commands (`/model` switches and remembers, `/new`,
//! `/clear`), streamed replies with each tool call shown, the REPL when there
//! is a terminal, the MCP relay a CLI backend reaches the window through, and
//! prompts from collaborative guests when hosting. It lived in `main.rs`, so
//! the app ran a thinner copy that had none of those.

use anyhow::Result;
use futures::StreamExt;
use localgpt_core::agent::tools::extract_tool_detail;
use localgpt_core::agent::{Agent, list_sessions_for_agent, search_sessions_for_agent};
use localgpt_core::commands::Interface;
use localgpt_core::text::{prefix_chars, prefix_chars_with_ellipsis};
use std::io::Write as _;
use std::path::Path;

use crate::desktop::{AgentChannels, ChatEvent, ChatSink};
use crate::gen3d;

/// Host-side net hooks passed into the agent loop.
/// Degrades to a unit option when the `multiplayer` feature is off.
#[cfg(feature = "multiplayer")]
pub type AgentNetHooksOpt = Option<crate::net::host::AgentNetHooks>;
#[cfg(not(feature = "multiplayer"))]
pub type AgentNetHooksOpt = Option<()>;

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
    config: &localgpt_core::config::Config,
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
            let (model, remember_as) = match resolve_model_choice(parts[1]) {
                Ok(pair) => pair,
                Err(e) => {
                    eprintln!("\nError: {e}\n");
                    return CommandResult::Continue;
                }
            };
            match agent.set_model(&model) {
                Ok(()) => match persist_gen_model(&remember_as) {
                    Ok(()) => {
                        println!("\nSwitched to model: {model} (remembered as Gen's default)\n")
                    }
                    Err(e) => println!("\nSwitched to model: {model} (this session only: {e})\n"),
                },
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
                for model in crate::local_llm::available_models() {
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
                    // The caller already has the config; loading a second one
                    // here used to create config.toml, and its fallback was a
                    // literal "~/..." that would have made a ./~/ directory.
                    let tracker =
                        crate::experiment::ExperimentTracker::new(&config.paths.state_dir);
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
                                        crate::experiment::prompt_preview(&exp.prompt, 50);
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

    /// "auto" is remembered as "auto" so it keeps re-picking; anything else
    /// passes through untouched.
    #[test]
    fn model_choices_pass_through_except_auto() {
        let (model, remembered) = resolve_model_choice("claude-cli/sonnet").unwrap();
        assert_eq!(model, "claude-cli/sonnet");
        assert_eq!(remembered, "claude-cli/sonnet");

        match resolve_model_choice("auto") {
            // With a usable local model, "auto" resolves to it but is stored
            // as "auto".
            Ok((model, remembered)) => {
                assert_eq!(remembered, "auto");
                assert!(model.starts_with("gguf/"), "{model}");
            }
            // Without one, the error says what to do about it.
            Err(e) => {
                let message = e.to_string();
                assert!(message.contains("no local model"), "{message}");
            }
        }
    }

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
pub async fn streaming_chat(
    agent: &mut Agent,
    input: &str,
    sink: Option<&ChatSink>,
) -> Result<String> {
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

/// MCP stdio ↔ TCP relay: bridges Claude CLI's MCP stdio to an open window's
/// TCP relay server (`localgpt-gen` interactive, or the LocalGPT app). This
/// process is spawned by Claude CLI when `--connect` is passed — it reads
/// from stdin, forwards to the TCP relay, and writes responses to stdout.
pub async fn run_mcp_stdio_relay(port: u16) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    let stream = TcpStream::connect(("127.0.0.1", port)).await.map_err(|e| {
        anyhow::anyhow!(
            "Failed to connect to the MCP relay on port {}: {}\n\
             Make sure a LocalGPT window with a scene is running (localgpt-gen, or the app).",
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

/// Turn what the user picked into `(model to switch to, value to remember)`.
/// `"auto"` resolves to a concrete local model now but is remembered as
/// `"auto"`, so it keeps re-picking as models come and go from the shared
/// directory.
fn resolve_model_choice(choice: &str) -> Result<(String, String)> {
    let choice = choice.trim();
    if choice.eq_ignore_ascii_case("auto") {
        let model = crate::desktop::models::auto_local_model().ok_or_else(|| {
            anyhow::anyhow!(
                "no local model this machine can run ({}). Download one first: {}",
                crate::desktop::hardware::summary(),
                crate::desktop::models::KNOWN_MODELS[0].fetch_hint
            )
        })?;
        return Ok((model, "auto".to_string()));
    }
    Ok((choice.to_string(), choice.to_string()))
}

/// Remember a model as Gen's default, so a switch in the model menu (or at
/// the REPL) survives a restart. Writes Gen's own settings file — the
/// assistant's config is neither read nor touched.
fn persist_gen_model(model: &str) -> Result<()> {
    crate::settings::update(|settings| {
        settings.default_model = Some(model.to_string());
    })
    .map(|_| ())
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
/// `crate::net::remote_scope`); a separate memory workspace so the
/// host's MEMORY.md / daily logs never enter a conversation remote users
/// steer; a fresh LLM session. For Claude CLI the built-in tools are
/// disabled and MCP points at a dedicated relay serving the same scoped
/// tools. Subprocess backends whose built-in tools can't be restricted
/// (Gemini CLI, Codex CLI) are refused.
#[cfg(feature = "multiplayer")]
async fn build_scoped_remote_agent(
    bridge: std::sync::Arc<gen3d::GenBridge>,
    config: &localgpt_core::config::Config,
    tool_profile: gen3d::tool_profile::ToolProfile,
) -> Result<Agent> {
    use crate::net::remote_scope::create_remote_scene_tools;
    use localgpt_core::memory::MemoryManager;

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
        let port = gen3d::mcp_relay::start_scoped_relay(create_remote_scene_tools(
            bridge.clone(),
            tool_profile,
        ))
        .await?;
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
    let tools = create_remote_scene_tools(bridge, tool_profile);
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
    Remote(crate::net::host::RemoteJob),
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
            let (model, remember_as) = match resolve_model_choice(&argument) {
                Ok(pair) => pair,
                Err(e) => {
                    sink.send(ChatEvent::Warning(format!("{e}")));
                    return PanelCommand::Handled;
                }
            };
            match agent.set_model(&model) {
                Ok(()) => {
                    sink.send(ChatEvent::Ready {
                        model: agent.model().to_string(),
                    });
                    let remembered = match persist_gen_model(&remember_as) {
                        Ok(()) => "Gen will use it next time too".to_string(),
                        Err(e) => {
                            tracing::warn!("couldn't save Gen's settings: {e}");
                            "this session only — couldn't save Gen's settings".to_string()
                        }
                    };
                    sink.send(ChatEvent::Notice(format!(
                        "Now using {} ({remembered}).",
                        agent.model()
                    )));
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

/// Whether `config`'s model is a CLI backend (Claude, Gemini or Codex CLI).
/// Those run as subprocesses and reach the window's tools through the MCP
/// relay — `<this binary> mcp-server --connect` — rather than through the
/// agent's tool calls, so a binary running this loop with one must start the
/// relay ([`gen3d::mcp_relay::start_mcp_relay`]) and answer that command
/// ([`run_mcp_stdio_relay`]).
pub fn uses_cli_backend(config: &localgpt_core::config::Config) -> bool {
    cli_family(&config.agent.default_model).is_some()
}

/// A first-run hint when the configured model is a CLI backend whose
/// program can't be found, so desktop users learn why nothing happens
/// before they type.
fn missing_cli_backend_hint(config: &localgpt_core::config::Config) -> Option<String> {
    use crate::desktop::models::find_on_path;

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
/// [`crate::net::host::HostChat`] messages.
///
/// Prompts from the in-window panel join the same stream, and every turn is
/// reported back to it. In `desktop` mode there's no terminal, so the REPL
/// isn't started and the panel is the only local input.
#[allow(clippy::too_many_arguments)]
pub async fn run_agent_loop(
    bridge: std::sync::Arc<gen3d::GenBridge>,
    agent_id: &str,
    initial_prompt: Option<String>,
    config: localgpt_core::config::Config,
    editor: Option<rustyline::DefaultEditor>,
    net_hooks: AgentNetHooksOpt,
    panel: AgentChannels,
    desktop: bool,
    tool_profile: gen3d::tool_profile::ToolProfile,
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
    // new_with_full_config, not new_with_agent: the latter drops config.paths
    // and re-resolves from the environment, which would silently put Gen back
    // in the assistant's workspace.
    let memory = MemoryManager::new_with_full_config(&config.memory, Some(&config), agent_id)?;
    let memory = Arc::new(memory);

    // Create safe tools + gen tools + CLI tools. memory_save / memory_log
    // come too — GEN_MEMORY_PROMPT instructs the model to use them, and
    // without them it fell back to write_file on the same files.
    let mut tools = create_safe_tools(&config, Some(memory.clone()))?;
    tools.extend(localgpt_core::mcp::memory_tools::create_memory_write_tools(
        config.workspace_path(),
    ));
    tools.extend(gen3d::tools::create_gen_tools(bridge.clone()));
    tools.extend(crate::mcp::avatar_tools::create_character_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::interaction_tools::create_interaction_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::terrain_tools::create_terrain_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::ui_tools::create_ui_tools(bridge.clone()));
    tools.extend(crate::mcp::physics_tools::create_physics_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::multifile_tools::create_multifile_tools(bridge));
    tools.extend(localgpt_cli_tools::create_cli_tools(&config)?);
    tools = gen3d::tool_profile::apply_tool_profile(tools, tool_profile);
    if tool_profile != gen3d::tool_profile::ToolProfile::Full {
        eprintln!(
            "Tool profile '{tool_profile}': {} tools active (smaller schema for the model's context)",
            tools.len()
        );
    }
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
    // config so it uses `<this binary> mcp-server --connect` to relay tool
    // calls to the EXISTING Bevy window instead of spawning a new one — this
    // binary, so the app relays through itself and Gen through Gen.
    if uses_cli_backend(&config) {
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
            .unwrap_or_else(|| crate::desktop::models::DEFAULT_OLLAMA_ENDPOINT.to_string());
        // A CLI backend only works in a session that started with one of its
        // family: the MCP relay and its tool config are set up at startup.
        let startup_family = cli_family(&current);
        tokio::spawn(async move {
            let mut options =
                crate::desktop::models::detect_model_options(&current, &ollama_endpoint).await;
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
    let mut chat_tx: Option<tokio::sync::mpsc::UnboundedSender<crate::net::host::HostChat>> = None;
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
                while let Some(crate::net::host::HostControlEvent::HostingStarted { full_access }) =
                    control_rx.recv().await
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
                    match build_scoped_remote_agent(remote_bridge.clone(), &config, tool_profile)
                        .await
                    {
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
                use crate::net::host::JobEvent;
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
                            let _ = chat_tx.send(crate::net::host::HostChat {
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
            let result = handle_gen_command(input, &mut agent, agent_id, &workspace, &config).await;
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
                        let _ = chat_tx.send(crate::net::host::HostChat {
                            speaker: "host-user".to_string(),
                            text: msg.clone(),
                        });
                        if !reply.is_empty() {
                            let _ = chat_tx.send(crate::net::host::HostChat {
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
                let _ = chat_tx.send(crate::net::host::HostChat {
                    speaker: "host-user".to_string(),
                    text: input.to_string(),
                });
                if !reply.is_empty() {
                    let _ = chat_tx.send(crate::net::host::HostChat {
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
