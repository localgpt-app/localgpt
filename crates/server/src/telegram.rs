//! Telegram bot interface for LocalGPT
//!
//! Provides a Telegram bot that allows interacting with LocalGPT remotely.
//! Uses a one-time pairing code mechanism to restrict access to the owner.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use teloxide::prelude::*;
use teloxide::types::{MessageId, ParseMode};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use localgpt_core::agent::{Agent, AgentConfig, StreamEvent, extract_tool_detail, tools::Tool};
use localgpt_core::concurrency::TurnGate;
use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;

/// Agent ID for Telegram sessions
const TELEGRAM_AGENT_ID: &str = "telegram";

/// Maximum Telegram message length
const MAX_MESSAGE_LENGTH: usize = 4096;

/// Debounce interval for message edits (seconds)
const EDIT_DEBOUNCE_SECS: u64 = 2;

/// Factory function type for creating additional tools for the Telegram agent.
/// This allows the caller (e.g., CLI daemon) to inject dangerous tools like bash, file I/O.
pub type ToolFactory = Box<dyn Fn(&Config) -> Result<Vec<Box<dyn Tool>>> + Send + Sync>;

#[derive(Debug, Serialize, Deserialize)]
struct PairedUser {
    user_id: u64,
    username: Option<String>,
    paired_at: String,
}

struct SessionEntry {
    agent: Agent,
    last_accessed: Instant,
}

struct BotState {
    config: Config,
    sessions: Mutex<HashMap<i64, SessionEntry>>,
    memory: MemoryManager,
    turn_gate: TurnGate,
    paired_user: Mutex<Option<PairedUser>>,
    pending_pairing_code: Mutex<Option<String>>,
    tool_factory: Option<ToolFactory>,
}

fn pairing_file_path() -> Result<PathBuf> {
    let paths = localgpt_core::paths::Paths::resolve()?;
    Ok(paths.pairing_file())
}

fn load_paired_user() -> Option<PairedUser> {
    let path = pairing_file_path().ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_paired_user(user: &PairedUser) -> Result<()> {
    let path = pairing_file_path()?;
    let content = serde_json::to_string_pretty(user)?;
    std::fs::write(path, content)?;
    Ok(())
}

fn generate_pairing_code() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Simple LCG-based 6-digit code (not cryptographic, but fine for pairing)
    let code = ((seed.wrapping_mul(6364136223846793005).wrapping_add(1)) % 900000 + 100000) as u32;
    format!("{:06}", code)
}

pub async fn run_telegram_bot(config: &Config, turn_gate: TurnGate, tool_factory: Option<ToolFactory>) -> Result<()> {
    let telegram_config = config
        .telegram
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Telegram config not found"))?;

    if !telegram_config.enabled {
        return Ok(());
    }

    let token = &telegram_config.api_token;
    if token.is_empty() || token.starts_with("${") {
        anyhow::bail!("Telegram API token not configured or not expanded");
    }

    let bot = Bot::new(token);

    let memory =
        MemoryManager::new_with_full_config(&config.memory, Some(config), TELEGRAM_AGENT_ID)?;

    let paired_user = load_paired_user();
    if let Some(ref user) = paired_user {
        info!(
            "Telegram bot: paired with user {} (ID: {})",
            user.username.as_deref().unwrap_or("unknown"),
            user.user_id
        );
    } else {
        info!("Telegram bot: no paired user. Send any message to start pairing.");
    }

    let state = Arc::new(BotState {
        config: config.clone(),
        sessions: Mutex::new(HashMap::new()),
        memory,
        turn_gate,
        paired_user: Mutex::new(paired_user),
        pending_pairing_code: Mutex::new(None),
        tool_factory,
    });

    // Register bot commands so Telegram clients show the "/" menu
    let commands: Vec<teloxide::types::BotCommand> = localgpt_core::commands::COMMANDS
        .iter()
        .filter(|c| c.supports(localgpt_core::commands::Interface::Telegram))
        .map(|c| teloxide::types::BotCommand::new(c.name, c.description))
        .collect();
    if let Err(e) = bot.set_my_commands(commands).await {
        warn!("Failed to set bot commands: {}", e);
    }

    info!("Starting Telegram bot...");

    let handler = Update::filter_message().endpoint(handle_message);

    Dispatcher::builder(bot, handler)
        .default_handler(|_upd| async {})
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

async fn handle_message(bot: Bot, msg: Message, state: Arc<BotState>) -> ResponseResult<()> {
    let text = match msg.text() {
        Some(t) => t.to_string(),
        None => return Ok(()),
    };

    let user = match msg.from {
        Some(ref u) => u,
        None => return Ok(()),
    };

    let user_id = user.id.0;
    let chat_id = msg.chat.id;

    // Check pairing
    {
        let paired = state.paired_user.lock().await;
        if let Some(ref pu) = *paired {
            if pu.user_id != user_id {
                bot.send_message(
                    chat_id,
                    "Not authorized. This bot is paired with another user.",
                )
                .await?;
                return Ok(());
            }
        } else {
            // Not paired yet - handle pairing flow
            drop(paired);
            return handle_pairing(bot, msg, &state, user_id, &text).await;
        }
    }

    // Handle slash commands
    if text.starts_with('/') {
        return handle_command(&bot, chat_id, &state, &text).await;
    }

    // Regular chat message
    handle_chat(&bot, chat_id, &state, &text).await
}

async fn handle_pairing(
    bot: Bot,
    msg: Message,
    state: &Arc<BotState>,
    user_id: u64,
    text: &str,
) -> ResponseResult<()> {
    let chat_id = msg.chat.id;
    let mut pending = state.pending_pairing_code.lock().await;

    if let Some(ref code) = *pending {
        // User is entering the pairing code
        if text.trim() == code.as_str() {
            // Pairing successful
            let username = msg.from.as_ref().and_then(|u| u.username.clone());
            let paired = PairedUser {
                user_id,
                username: username.clone(),
                paired_at: chrono::Utc::now().to_rfc3339(),
            };

            if let Err(e) = save_paired_user(&paired) {
                error!("Failed to save pairing: {}", e);
                bot.send_message(chat_id, "Pairing failed (could not save). Check logs.")
                    .await?;
                return Ok(());
            }

            *state.paired_user.lock().await = Some(paired);
            *pending = None;

            info!(
                "Telegram bot: paired with user {} (ID: {})",
                username.as_deref().unwrap_or("unknown"),
                user_id
            );

            bot.send_message(chat_id,
                "Paired successfully! You can now chat with LocalGPT.\n\nUse /new to start a fresh session, /status to see session info.",
            )
            .await?;
        } else {
            bot.send_message(chat_id, "Invalid pairing code. Please try again.")
                .await?;
        }
    } else {
        // Generate new pairing code
        let code = generate_pairing_code();
        println!("\n========================================");
        println!("  TELEGRAM PAIRING CODE: {}", code);
        println!("========================================\n");
        info!(
            "Telegram pairing code generated for user {} (ID: {})",
            msg.from
                .as_ref()
                .and_then(|u| u.username.as_deref())
                .unwrap_or("unknown"),
            user_id
        );

        *pending = Some(code);

        bot.send_message(chat_id,
            "Welcome! A pairing code has been printed to the daemon logs/stdout.\nPlease enter the code to pair this bot with your account.",
        )
        .await?;
    }

    Ok(())
}

async fn handle_command(
    bot: &Bot,
    chat_id: ChatId,
    state: &Arc<BotState>,
    text: &str,
) -> ResponseResult<()> {
    let parts: Vec<&str> = text.splitn(2, ' ').collect();
    let cmd = parts[0];
    let args = parts.get(1).map(|s| s.trim()).unwrap_or("");

    match cmd {
        "/start" | "/help" => {
            let help = format!(
                "LocalGPT Telegram Bot\n\n{}",
                localgpt_core::commands::format_help_text(
                    localgpt_core::commands::Interface::Telegram
                )
            );
            bot.send_message(chat_id, &help).await?;
        }
        "/new" => {
            let mut sessions = state.sessions.lock().await;
            sessions.remove(&chat_id.0);
            bot.send_message(
                chat_id,
                "Session cleared. Send a message to start a new conversation.",
            )
            .await?;
        }
        "/status" => {
            let sessions = state.sessions.lock().await;
            let status_text = if let Some(entry) = sessions.get(&chat_id.0) {
                let status = entry.agent.session_status();
                let (used, usable, total) = entry.agent.context_usage();
                let mut text = format!(
                    "Session active\n\
                     Model: {}\n\
                     Messages: {}\n\
                     Tokens: {} / {} (window: {})\n\
                     Compactions: {}\n\
                     Idle: {}s",
                    entry.agent.model(),
                    status.message_count,
                    used,
                    usable,
                    total,
                    status.compaction_count,
                    entry.last_accessed.elapsed().as_secs()
                );
                if status.search_queries > 0 {
                    let cache_pct =
                        (status.search_cached_hits as f64 / status.search_queries as f64) * 100.0;
                    text.push_str(&format!(
                        "\nSearch: {} queries ({} cached, {:.0}%) · ${:.3}",
                        status.search_queries,
                        status.search_cached_hits,
                        cache_pct,
                        status.search_cost_usd
                    ));
                }
                text
            } else {
                "No active session. Send a message to start one.".to_string()
            };
            bot.send_message(chat_id, &status_text).await?;
        }
        "/compact" => {
            let mut sessions = state.sessions.lock().await;
            match sessions.get_mut(&chat_id.0) {
                Some(entry) => {
                    entry.last_accessed = Instant::now();
                    match entry.agent.compact_session().await {
                        Ok((before, after)) => {
                            bot.send_message(
                                chat_id,
                                format!("Compacted: {} -> {} tokens", before, after),
                            )
                            .await?;
                        }
                        Err(e) => {
                            bot.send_message(chat_id, format!("Compact failed: {}", e))
                                .await?;
                        }
                    }
                }
                None => {
                    bot.send_message(chat_id, "No active session.").await?;
                }
            }
        }
        "/clear" => {
            let mut sessions = state.sessions.lock().await;
            if let Some(entry) = sessions.get_mut(&chat_id.0) {
                entry.agent.clear_session();
                entry.last_accessed = Instant::now();
                bot.send_message(chat_id, "Session history cleared.")
                    .await?;
            } else {
                bot.send_message(chat_id, "No active session.").await?;
            }
        }
        "/memory" => {
            if args.is_empty() {
                bot.send_message(chat_id, "Usage: /memory <search query>")
                    .await?;
            } else {
                match state.memory.search(args, 5) {
                    Ok(results) => {
                        if results.is_empty() {
                            bot.send_message(chat_id, "No results found.").await?;
                        } else {
                            let mut text = format!("Memory search: \"{}\"\n\n", args);
                            for (i, r) in results.iter().enumerate() {
                                text.push_str(&format!(
                                    "{}. {} (L{}-{})\n{}\n\n",
                                    i + 1,
                                    r.file,
                                    r.line_start,
                                    r.line_end,
                                    truncate_str(&r.content, 300),
                                ));
                            }
                            send_long_message(bot, chat_id, None, &text).await;
                        }
                    }
                    Err(e) => {
                        bot.send_message(chat_id, format!("Search error: {}", e))
                            .await?;
                    }
                }
            }
        }
        "/model" => {
            if args.is_empty() {
                let sessions = state.sessions.lock().await;
                let current = sessions
                    .get(&chat_id.0)
                    .map(|e| e.agent.model().to_string())
                    .unwrap_or_else(|| state.config.agent.default_model.clone());
                bot.send_message(
                    chat_id,
                    format!("Current model: {}\n\nUsage: /model <name>", current),
                )
                .await?;
            } else {
                let mut sessions = state.sessions.lock().await;
                if let Some(entry) = sessions.get_mut(&chat_id.0) {
                    match entry.agent.set_model(args) {
                        Ok(()) => {
                            bot.send_message(chat_id, format!("Switched to model: {}", args))
                                .await?;
                        }
                        Err(e) => {
                            bot.send_message(chat_id, format!("Failed to switch model: {}", e))
                                .await?;
                        }
                    }
                } else {
                    bot.send_message(
                        chat_id,
                        "No active session. Send a message first, then switch models.",
                    )
                    .await?;
                }
            }
        }
        "/skills" => {
            let workspace_path = state.config.workspace_path();
            match localgpt_core::agent::load_skills(&workspace_path) {
                Ok(skills) => {
                    if skills.is_empty() {
                        bot.send_message(chat_id, "No skills installed.").await?;
                    } else {
                        let summary = localgpt_core::agent::get_skills_summary(&skills);
                        bot.send_message(chat_id, &summary).await?;
                    }
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("Failed to load skills: {}", e))
                        .await?;
                }
            }
        }
        "/unpair" => {
            *state.paired_user.lock().await = None;
            if let Ok(path) = pairing_file_path() {
                let _ = std::fs::remove_file(path);
            }
            let mut sessions = state.sessions.lock().await;
            sessions.remove(&chat_id.0);
            info!("Telegram bot: user unpaired");
            bot.send_message(
                chat_id,
                "Unpaired. Send any message to start a new pairing.",
            )
            .await?;
        }
        _ => {
            bot.send_message(
                chat_id,
                "Unknown command. Use /help for available commands.",
            )
            .await?;
        }
    }

    Ok(())
}

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Find a char boundary
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

/// Escape text for Telegram HTML parse mode.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Convert markdown to Telegram-compatible HTML.
/// Handles: code blocks, inline code, bold, italic, links, headers.
/// Unrecognized markup passes through as escaped HTML.
fn markdown_to_html(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut code_block_content = String::new();

    for line in text.lines() {
        if in_code_block {
            if line.starts_with("```") {
                // Close code block
                let lang_attr = if code_block_lang.is_empty() {
                    String::new()
                } else {
                    format!(" class=\"language-{}\"", escape_html(&code_block_lang))
                };
                result.push_str(&format!(
                    "<pre><code{}>{}</code></pre>\n",
                    lang_attr,
                    escape_html(&code_block_content)
                ));
                code_block_content.clear();
                code_block_lang.clear();
                in_code_block = false;
            } else {
                if !code_block_content.is_empty() {
                    code_block_content.push('\n');
                }
                code_block_content.push_str(line);
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("```") {
            in_code_block = true;
            code_block_lang = rest.trim().to_string();
            continue;
        }

        // Headers → bold
        let line = if let Some(rest) = line.strip_prefix("### ") {
            format!("<b>{}</b>", escape_html(rest))
        } else if let Some(rest) = line.strip_prefix("## ") {
            format!("<b>{}</b>", escape_html(rest))
        } else if let Some(rest) = line.strip_prefix("# ") {
            format!("<b>{}</b>", escape_html(rest))
        } else {
            convert_inline_markdown(line)
        };

        result.push_str(&line);
        result.push('\n');
    }

    // Handle unclosed code block
    if in_code_block {
        result.push_str(&format!(
            "<pre><code>{}</code></pre>\n",
            escape_html(&code_block_content)
        ));
    }

    result
}

/// Convert inline markdown elements: `code`, **bold**, *italic*, [links](url)
fn convert_inline_markdown(line: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Inline code: `...`
        if chars[i] == '`'
            && let Some(end) = chars[i + 1..].iter().position(|&c| c == '`')
        {
            let code: String = chars[i + 1..i + 1 + end].iter().collect();
            result.push_str(&format!("<code>{}</code>", escape_html(&code)));
            i += end + 2;
            continue;
        }

        // Bold: **...**
        if i + 1 < len
            && chars[i] == '*'
            && chars[i + 1] == '*'
            && let Some(end) = find_closing(&chars, i + 2, &['*', '*'])
        {
            let inner: String = chars[i + 2..end].iter().collect();
            result.push_str(&format!("<b>{}</b>", escape_html(&inner)));
            i = end + 2;
            continue;
        }

        // Italic: *...*
        if chars[i] == '*'
            && let Some(end) = chars[i + 1..].iter().position(|&c| c == '*')
        {
            let inner: String = chars[i + 1..i + 1 + end].iter().collect();
            result.push_str(&format!("<i>{}</i>", escape_html(&inner)));
            i += end + 2;
            continue;
        }

        // Link: [text](url)
        if chars[i] == '['
            && let Some(close_bracket) = chars[i + 1..].iter().position(|&c| c == ']')
        {
            let text_end = i + 1 + close_bracket;
            if text_end + 1 < len
                && chars[text_end + 1] == '('
                && let Some(close_paren) = chars[text_end + 2..].iter().position(|&c| c == ')')
            {
                let text: String = chars[i + 1..text_end].iter().collect();
                let url: String = chars[text_end + 2..text_end + 2 + close_paren]
                    .iter()
                    .collect();
                result.push_str(&format!(
                    "<a href=\"{}\">{}</a>",
                    escape_html(&url),
                    escape_html(&text)
                ));
                i = text_end + 2 + close_paren + 1;
                continue;
            }
        }

        // Regular character
        match chars[i] {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            c => result.push(c),
        }
        i += 1;
    }

    result
}

/// Find closing delimiter (e.g., ** for bold) starting from `start`.
fn find_closing(chars: &[char], start: usize, delim: &[char]) -> Option<usize> {
    let dlen = delim.len();
    if start + dlen > chars.len() {
        return None;
    }
    for i in start..chars.len() - dlen + 1 {
        if chars[i..i + dlen] == *delim {
            return Some(i);
        }
    }
    None
}

async fn handle_chat(
    bot: &Bot,
    chat_id: ChatId,
    state: &Arc<BotState>,
    text: &str,
) -> ResponseResult<()> {
    // Send initial "thinking" message
    let thinking_msg = bot.send_message(chat_id, "Thinking...").await?;
    let msg_id = thinking_msg.id;

    // Acquire turn gate
    let _gate_permit = state.turn_gate.acquire().await;

    // Get or create agent session, then stream response
    let mut sessions = state.sessions.lock().await;

    if let std::collections::hash_map::Entry::Vacant(e) = sessions.entry(chat_id.0) {
        let agent_config = AgentConfig {
            model: state.config.agent.default_model.clone(),
            context_window: state.config.agent.context_window,
            reserve_tokens: state.config.agent.reserve_tokens,
        };

        match Agent::new(agent_config, &state.config, state.memory.clone()).await {
            Ok(mut agent) => {
                // Extend agent with additional tools from factory if provided (e.g., CLI tools from daemon)
                if let Some(ref factory) = state.tool_factory {
                    match factory(&state.config) {
                        Ok(extra_tools) => {
                            agent.extend_tools(extra_tools);
                        }
                        Err(err) => {
                            error!("Failed to create additional tools: {}", err);
                        }
                    }
                }

                if let Err(err) = agent.new_session().await {
                    error!("Failed to create session: {}", err);
                    let _ = bot
                        .edit_message_text(chat_id, msg_id, format!("Error: {}", err))
                        .await;
                    return Ok(());
                }
                e.insert(SessionEntry {
                    agent,
                    last_accessed: Instant::now(),
                });
            }
            Err(err) => {
                error!("Failed to create agent: {}", err);
                let _ = bot
                    .edit_message_text(chat_id, msg_id, format!("Error: {}", err))
                    .await;
                return Ok(());
            }
        }
    }

    let entry = sessions.get_mut(&chat_id.0).unwrap();
    entry.last_accessed = Instant::now();

    // Use streaming with tools
    let response = match entry.agent.chat_stream_with_tools(text, Vec::new()).await {
        Ok(event_stream) => {
            use futures::StreamExt;

            let mut full_response = String::new();
            let mut last_edit = Instant::now();
            let mut pinned_stream = std::pin::pin!(event_stream);
            let mut tool_info = String::new();

            while let Some(event) = pinned_stream.next().await {
                match event {
                    Ok(StreamEvent::Content(delta)) => {
                        full_response.push_str(&delta);

                        // Debounced edit
                        if last_edit.elapsed().as_secs() >= EDIT_DEBOUNCE_SECS {
                            let display = format_display(&full_response, &tool_info);
                            let _ = bot.edit_message_text(chat_id, msg_id, &display).await;
                            last_edit = Instant::now();
                        }
                    }
                    Ok(StreamEvent::ToolCallStart {
                        name, arguments, ..
                    }) => {
                        let detail = extract_tool_detail(&name, &arguments);
                        let info_line = if let Some(d) = detail {
                            format!("🔧 {}({})\n", name, d)
                        } else {
                            format!("🔧 {}\n", name)
                        };
                        tool_info.push_str(&info_line);

                        let display = format_display(&full_response, &tool_info);
                        let _ = bot.edit_message_text(chat_id, msg_id, &display).await;
                        last_edit = Instant::now();
                    }
                    Ok(StreamEvent::ToolCallEnd { name, warnings, .. }) => {
                        if !warnings.is_empty() {
                            for w in &warnings {
                                tool_info.push_str(&format!(
                                    "\u{26a0} Suspicious content in {}: {}\n",
                                    name, w
                                ));
                            }
                            let display = format_display(&full_response, &tool_info);
                            let _ = bot.edit_message_text(chat_id, msg_id, &display).await;
                            last_edit = Instant::now();
                        }
                    }
                    Ok(StreamEvent::Done) => break,
                    Err(e) => {
                        error!("Stream error: {}", e);
                        full_response.push_str(&format!("\n\nError: {}", e));
                        break;
                    }
                }
            }

            if full_response.is_empty() {
                "(no response)".to_string()
            } else {
                full_response
            }
        }
        Err(e) => format!("Error: {}", e),
    };

    // Save session before releasing lock
    if let Err(e) = entry.agent.save_session_for_agent(TELEGRAM_AGENT_ID).await {
        debug!("Failed to save telegram session: {}", e);
    }

    drop(sessions);

    // Final edit with complete response
    send_long_message(bot, chat_id, Some(msg_id), &response).await;

    Ok(())
}

fn format_display(response: &str, tool_info: &str) -> String {
    let mut display = String::new();
    if !tool_info.is_empty() {
        display.push_str(tool_info);
        display.push('\n');
    }
    display.push_str(response);

    // Truncate for Telegram limit
    if display.len() > MAX_MESSAGE_LENGTH {
        display.truncate(MAX_MESSAGE_LENGTH - 3);
        display.push_str("...");
    }

    display
}

/// Send/edit agent response as HTML-converted markdown.
async fn send_or_edit_html(bot: &Bot, chat_id: ChatId, msg_id: Option<MessageId>, text: &str) {
    let html = markdown_to_html(text);
    let result = if let Some(mid) = msg_id {
        bot.edit_message_text(chat_id, mid, &html)
            .parse_mode(ParseMode::Html)
            .await
    } else {
        bot.send_message(chat_id, &html)
            .parse_mode(ParseMode::Html)
            .await
    };

    // Fallback to plain text on conversion issues
    if result.is_err() {
        if let Some(mid) = msg_id {
            let _ = bot.edit_message_text(chat_id, mid, text).await;
        } else {
            let _ = bot.send_message(chat_id, text).await;
        }
    }
}

async fn send_long_message(bot: &Bot, chat_id: ChatId, edit_msg_id: Option<MessageId>, text: &str) {
    if text.len() <= MAX_MESSAGE_LENGTH {
        send_or_edit_html(bot, chat_id, edit_msg_id, text).await;
        return;
    }

    // Split into chunks at char boundaries
    let chunks = split_text_chunks(text);

    // First chunk: edit existing message or send new
    if let Some(first) = chunks.first() {
        send_or_edit_html(bot, chat_id, edit_msg_id, first).await;
    }

    // Remaining chunks as new messages
    for chunk in chunks.iter().skip(1) {
        send_or_edit_html(bot, chat_id, None, chunk).await;
    }
}

fn split_text_chunks(text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + MAX_MESSAGE_LENGTH).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks
}
