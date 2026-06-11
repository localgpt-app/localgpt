use anyhow::Result;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tarpc::context;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, MessageId, ParseMode, ThreadId};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use localgpt_bridge::connect;
use localgpt_core::agent::{Agent, AgentConfig, StreamEvent, extract_tool_detail};
use localgpt_core::concurrency::TurnGate;
use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;
use localgpt_core::text::{ellipsize_chars, prefix_chars_cow};

/// Agent ID for Telegram sessions
const TELEGRAM_AGENT_ID: &str = "telegram";

/// Maximum Telegram message length (Telegram API limit)
const MAX_MESSAGE_LENGTH: usize = 4096;

/// Debounce interval for streaming edits (seconds)
const EDIT_DEBOUNCE_SECS: u64 = 2;

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
    bot_info: teloxide::types::Me,
}

fn pairing_file_path() -> Result<PathBuf> {
    let paths = localgpt_core::paths::Paths::resolve()?;
    Ok(paths.pairing_file())
}

fn load_paired_user() -> Option<PairedUser> {
    let path = pairing_file_path().ok()?;
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_paired_user(user: &PairedUser) -> Result<()> {
    let path = pairing_file_path()?;
    let content = serde_json::to_string_pretty(user)?;
    std::fs::write(path, content)?;
    Ok(())
}

/// Generate a 6-digit pairing code using a cryptographically secure RNG.
fn generate_pairing_code() -> String {
    let code: u32 = rand::random_range(100_000..=999_999);
    format!("{:06}", code)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .init();

    info!("Starting LocalGPT Telegram Bridge...");

    // 1. Connect to Bridge Manager to get credentials
    let paths = localgpt_core::paths::Paths::resolve()?;
    let socket_path = paths.bridge_socket_name();

    info!("Connecting to bridge socket: {}", socket_path);
    let client = connect(&socket_path).await?;

    // 2. Verify protocol version
    match client.get_version(context::current()).await {
        Ok(v) => {
            if !v.starts_with("1.") {
                anyhow::bail!("Unsupported bridge protocol version '{}'. Expected 1.x", v);
            }
            info!("Bridge protocol version: {}", v);
        }
        Err(e) => {
            warn!("Could not retrieve bridge version (old server?): {}", e);
        }
    }

    // 3. Fetch Telegram token
    let token_bytes = match client
        .get_credentials(context::current(), "telegram".to_string())
        .await?
    {
        Ok(t) => t,
        Err(e) => {
            error!(
                "Failed to retrieve Telegram credentials: {}. Have you run 'localgpt bridge register --id telegram ...'?",
                e
            );
            std::process::exit(1);
        }
    };

    let token = String::from_utf8(token_bytes)
        .map_err(|_| anyhow::anyhow!("Invalid UTF-8 in Telegram token"))?;
    info!("Successfully retrieved Telegram token.");

    // 4. Initialize Bot & State
    let config = Config::load()?;
    let bot = Bot::new(token);
    let bot_info = bot.get_me().await?;
    info!("Bot identity: @{}", bot_info.username());

    let memory =
        MemoryManager::new_with_full_config(&config.memory, Some(&config), TELEGRAM_AGENT_ID)?;
    let turn_gate = TurnGate::new();

    let paired_user = load_paired_user();
    if let Some(ref user) = paired_user {
        info!(
            "Paired with user {} (ID: {})",
            user.username.as_deref().unwrap_or("unknown"),
            user.user_id
        );
    } else {
        info!("No paired user. Send any message to start pairing.");
    }

    let state = Arc::new(BotState {
        config: config.clone(),
        sessions: Mutex::new(HashMap::new()),
        memory,
        turn_gate,
        paired_user: Mutex::new(paired_user),
        pending_pairing_code: Mutex::new(None),
        bot_info,
    });

    // 5. Register slash commands so clients show the "/" menu
    let commands: Vec<teloxide::types::BotCommand> = localgpt_core::commands::COMMANDS
        .iter()
        .filter(|c| c.supports(localgpt_core::commands::Interface::Telegram))
        .map(|c| teloxide::types::BotCommand::new(c.name, c.description))
        .collect();

    if let Err(e) = bot.set_my_commands(commands).await {
        warn!("Failed to set bot commands: {}", e);
    }

    info!("Telegram bot started.");

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

    let chat_id = msg.chat.id;
    let user = match msg.from {
        Some(ref u) => u,
        None => return Ok(()),
    };
    let user_id = user.id.0;

    // Skip self-messages to prevent infinite loops in groups
    if user.id == state.bot_info.id {
        debug!("Skipping self-message from bot");
        return Ok(());
    }

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
            drop(paired);

            return handle_pairing(bot, chat_id, msg.from, &state, user_id, &text).await;
        }
    }

    // Mention-based activation in groups
    if msg.chat.is_group() || msg.chat.is_supergroup() {
        let bot_name = state.bot_info.username();
        let mention = format!("@{}", bot_name);

        let is_mentioned = text.contains(&mention);
        let is_reply_to_bot = msg
            .reply_to_message()
            .and_then(|m| m.from.as_ref())
            .map(|u| u.id == state.bot_info.id)
            .unwrap_or(false);

        if !is_mentioned && !is_reply_to_bot {
            return Ok(());
        }
    }

    // Extract thread_id for forum topic support (only in forum supergroups)
    let thread_id = if is_forum_chat(&msg.chat) {
        msg.thread_id
    } else {
        None
    };

    if text.starts_with('/') {
        return handle_command(&bot, chat_id, thread_id, &state, &text).await;
    }

    handle_chat(&bot, chat_id, thread_id, msg.id, &state, &text).await
}

async fn handle_pairing(
    bot: Bot,
    chat_id: ChatId,
    from: Option<teloxide::types::User>,
    state: &Arc<BotState>,
    user_id: u64,
    text: &str,
) -> ResponseResult<()> {
    let mut pending = state.pending_pairing_code.lock().await;

    if let Some(ref code) = *pending {
        if text.trim() == code.as_str() {
            let username = from.as_ref().and_then(|u| u.username.clone());
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
                "Paired with user {} (ID: {})",
                username.as_deref().unwrap_or("unknown"),
                user_id
            );
            bot.send_message(
                chat_id,
                "Paired successfully! You can now chat with LocalGPT.\n\nUse /new to start a fresh session, /status to see session info.",
            )
            .await?;
        } else {
            bot.send_message(chat_id, "Invalid pairing code. Please try again.")
                .await?;
        }
    } else {
        let code = generate_pairing_code();
        println!("\n========================================");
        println!("  TELEGRAM PAIRING CODE: {}", code);
        println!("========================================\n");
        info!(
            "Pairing code generated for user {} (ID: {})",
            from.as_ref()
                .and_then(|u| u.username.as_deref())
                .unwrap_or("unknown"),
            user_id
        );

        *pending = Some(code);
        bot.send_message(
            chat_id,
            "Welcome! A pairing code has been printed to the bridge logs.\nPlease enter it here to pair your account.",
        )
        .await?;
    }

    Ok(())
}

async fn handle_command(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    state: &Arc<BotState>,
    text: &str,
) -> ResponseResult<()> {
    let parts: Vec<&str> = text.splitn(2, ' ').collect();
    let cmd = parts[0];
    let args = parts.get(1).map(|s| s.trim()).unwrap_or("");

    match cmd {
        "/start" | "/help" => {
            let help = format!(
                "LocalGPT Telegram Bridge\n\n{}",
                localgpt_core::commands::format_help_text(
                    localgpt_core::commands::Interface::Telegram
                )
            );
            send_in_thread(bot, chat_id, thread_id, &help).await?;
        }
        "/new" => {
            state.sessions.lock().await.remove(&chat_id.0);
            send_in_thread(
                bot,
                chat_id,
                thread_id,
                "Session cleared. Send a message to start a new conversation.",
            )
            .await?;
        }
        "/status" => {
            let sessions = state.sessions.lock().await;
            let status_text = if let Some(entry) = sessions.get(&chat_id.0) {
                let status = entry.agent.session_status();
                let (used, usable, total) = entry.agent.context_usage();
                let mut t = format!(
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
                    t.push_str(&format!(
                        "\nSearch: {} queries ({} cached, {:.0}%) · ${:.3}",
                        status.search_queries,
                        status.search_cached_hits,
                        cache_pct,
                        status.search_cost_usd
                    ));
                }
                t
            } else {
                "No active session. Send a message to start one.".to_string()
            };
            send_in_thread(bot, chat_id, thread_id, &status_text).await?;
        }
        "/compact" => {
            let mut sessions = state.sessions.lock().await;
            match sessions.get_mut(&chat_id.0) {
                Some(entry) => {
                    entry.last_accessed = Instant::now();
                    match entry.agent.compact_session().await {
                        Ok((before, after)) => {
                            send_in_thread(
                                bot,
                                chat_id,
                                thread_id,
                                &format!("Compacted: {} → {} tokens", before, after),
                            )
                            .await?;
                        }
                        Err(e) => {
                            send_in_thread(
                                bot,
                                chat_id,
                                thread_id,
                                &format!("Compact failed: {}", e),
                            )
                            .await?;
                        }
                    }
                }
                None => {
                    send_in_thread(bot, chat_id, thread_id, "No active session.").await?;
                }
            }
        }
        "/clear" => {
            let mut sessions = state.sessions.lock().await;
            if let Some(entry) = sessions.get_mut(&chat_id.0) {
                entry.agent.clear_session();
                entry.last_accessed = Instant::now();
                send_in_thread(bot, chat_id, thread_id, "Session history cleared.").await?;
            } else {
                send_in_thread(bot, chat_id, thread_id, "No active session.").await?;
            }
        }
        "/memory" => {
            if args.is_empty() {
                send_in_thread(bot, chat_id, thread_id, "Usage: /memory <search query>").await?;
            } else {
                match state.memory.search(args, 5) {
                    Ok(results) => {
                        if results.is_empty() {
                            send_in_thread(bot, chat_id, thread_id, "No results found.").await?;
                        } else {
                            let mut t = format!("Memory search: \"{}\"\n\n", args);
                            for (i, r) in results.iter().enumerate() {
                                t.push_str(&format!(
                                    "{}. {} (L{}-{})\n{}\n\n",
                                    i + 1,
                                    r.file,
                                    r.line_start,
                                    r.line_end,
                                    truncate_str(&r.content, 300),
                                ));
                            }
                            send_long_message(bot, chat_id, thread_id, None, &t).await;
                        }
                    }
                    Err(e) => {
                        send_in_thread(bot, chat_id, thread_id, &format!("Search error: {}", e))
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
                send_in_thread(
                    bot,
                    chat_id,
                    thread_id,
                    &format!("Current model: {}\n\nUsage: /model <name>", current),
                )
                .await?;
            } else {
                let mut sessions = state.sessions.lock().await;
                if let Some(entry) = sessions.get_mut(&chat_id.0) {
                    match entry.agent.set_model(args) {
                        Ok(()) => {
                            send_in_thread(
                                bot,
                                chat_id,
                                thread_id,
                                &format!("Switched to model: {}", args),
                            )
                            .await?;
                        }
                        Err(e) => {
                            send_in_thread(
                                bot,
                                chat_id,
                                thread_id,
                                &format!("Failed to switch model: {}", e),
                            )
                            .await?;
                        }
                    }
                } else {
                    send_in_thread(
                        bot,
                        chat_id,
                        thread_id,
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
                        send_in_thread(bot, chat_id, thread_id, "No skills installed.").await?;
                    } else {
                        let summary = localgpt_core::agent::get_skills_summary(&skills);
                        send_in_thread(bot, chat_id, thread_id, &summary).await?;
                    }
                }
                Err(e) => {
                    send_in_thread(
                        bot,
                        chat_id,
                        thread_id,
                        &format!("Failed to load skills: {}", e),
                    )
                    .await?;
                }
            }
        }
        "/topic" => {
            if args.is_empty() {
                send_in_thread(bot, chat_id, thread_id, "Usage: /topic <name>").await?;
            } else if !is_forum_chat_remote(bot, chat_id).await {
                send_in_thread(
                    bot,
                    chat_id,
                    thread_id,
                    "This chat is not a forum group. /topic only works in forum-enabled groups.",
                )
                .await?;
            } else {
                match bot.create_forum_topic(chat_id, args).await {
                    Ok(topic) => {
                        send_in_thread(
                            bot,
                            chat_id,
                            Some(topic.thread_id),
                            &format!(
                                "Topic '{}' created. Send messages here to chat with LocalGPT.",
                                args
                            ),
                        )
                        .await?;
                    }
                    Err(e) => {
                        send_in_thread(
                            bot,
                            chat_id,
                            thread_id,
                            &format!("Failed to create topic: {}", e),
                        )
                        .await?;
                    }
                }
            }
        }
        "/unpair" => {
            *state.paired_user.lock().await = None;
            if let Ok(path) = pairing_file_path() {
                let _ = std::fs::remove_file(path);
            }
            state.sessions.lock().await.remove(&chat_id.0);
            info!("Telegram bot: user unpaired");
            send_in_thread(
                bot,
                chat_id,
                thread_id,
                "Unpaired. Send any message to start a new pairing.",
            )
            .await?;
        }
        _ => {
            send_in_thread(
                bot,
                chat_id,
                thread_id,
                "Unknown command. Use /help for available commands.",
            )
            .await?;
        }
    }

    Ok(())
}

/// Check if a Chat is a forum-enabled supergroup.
fn is_forum_chat(chat: &teloxide::types::Chat) -> bool {
    use teloxide::types::{ChatKind, PublicChatKind};
    if let ChatKind::Public(public) = &chat.kind
        && let PublicChatKind::Supergroup(sg) = &public.kind
    {
        return sg.is_forum;
    }
    false
}

/// Send a message within a forum topic thread (or normally if no thread).
async fn send_in_thread(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    text: &str,
) -> ResponseResult<Message> {
    let mut req = bot.send_message(chat_id, text);
    if let Some(tid) = thread_id {
        req = req.message_thread_id(tid);
    }
    req.await
}

/// Check if a chat is a forum group via API call (used for /topic validation).
async fn is_forum_chat_remote(bot: &Bot, chat_id: ChatId) -> bool {
    use teloxide::types::{ChatFullInfoKind, ChatFullInfoPublicKind};
    match bot.get_chat(chat_id).await {
        Ok(chat) => {
            if let ChatFullInfoKind::Public(public) = &chat.kind
                && let ChatFullInfoPublicKind::Supergroup(sg) = &public.kind
            {
                return sg.is_forum;
            }
            false
        }
        Err(_) => false,
    }
}

async fn handle_chat(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    user_msg_id: MessageId,
    state: &Arc<BotState>,
    text: &str,
) -> ResponseResult<()> {
    // Send typing indicator and acknowledgment reaction
    let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;
    // React with "eyes" emoji to acknowledge the message is being processed
    let _ = bot
        .set_message_reaction(chat_id, user_msg_id)
        .reaction(vec![teloxide::types::ReactionType::Emoji {
            emoji: "\u{1F440}".to_owned(),
        }])
        .send()
        .await;

    let _gate_permit = state.turn_gate.acquire().await;
    let mut sessions = state.sessions.lock().await;

    if let std::collections::hash_map::Entry::Vacant(e) = sessions.entry(chat_id.0) {
        let agent_config = AgentConfig {
            model: state.config.agent.default_model.clone(),
            context_window: state.config.agent.context_window,
            reserve_tokens: state.config.agent.reserve_tokens,
        };

        match Agent::new(agent_config, &state.config, Arc::new(state.memory.clone())).await {
            Ok(mut agent) => {
                if let Err(err) = agent.new_session().await {
                    send_in_thread(bot, chat_id, thread_id, &format!("Error: {}", err)).await?;
                    return Ok(());
                }
                // Send welcome message on first run
                if agent.is_brand_new() {
                    let html = markdown_to_html(localgpt_core::agent::FIRST_RUN_WELCOME);
                    let mut req = bot.send_message(chat_id, html).parse_mode(ParseMode::Html);
                    if let Some(tid) = thread_id {
                        req = req.message_thread_id(tid);
                    }
                    let _ = req.await;
                }
                e.insert(SessionEntry {
                    agent,
                    last_accessed: Instant::now(),
                });
            }
            Err(err) => {
                error!("Failed to create agent: {}", err);
                send_in_thread(bot, chat_id, thread_id, &format!("Error: {}", err)).await?;
                return Ok(());
            }
        }
    }

    let entry = sessions.get_mut(&chat_id.0).unwrap();
    entry.last_accessed = Instant::now();

    let mut msg_id: Option<MessageId> = None;

    let response = match entry.agent.chat_stream_with_tools(text, Vec::new()).await {
        Ok(event_stream) => {
            let mut full_response = String::new();
            let mut last_edit = Instant::now();
            let mut last_typing = Instant::now();
            let mut pinned_stream = std::pin::pin!(event_stream);
            let mut tool_info = String::new();

            while let Some(event) = pinned_stream.next().await {
                // Periodically send typing indicator (every 5 seconds) if we haven't finished
                if last_typing.elapsed().as_secs() >= 5 {
                    let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;
                    last_typing = Instant::now();
                }

                match event {
                    Ok(StreamEvent::Content(delta)) => {
                        full_response.push_str(&delta);
                        if last_edit.elapsed().as_secs() >= EDIT_DEBOUNCE_SECS || msg_id.is_none() {
                            let display = format_display(&full_response, &tool_info);
                            if let Some(id) = msg_id {
                                let _ = bot.edit_message_text(chat_id, id, &display).await;
                            } else {
                                let sent =
                                    send_in_thread(bot, chat_id, thread_id, &display).await?;
                                msg_id = Some(sent.id);
                            }
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
                        if let Some(id) = msg_id {
                            let _ = bot.edit_message_text(chat_id, id, &display).await;
                        } else {
                            let sent = send_in_thread(bot, chat_id, thread_id, &display).await?;
                            msg_id = Some(sent.id);
                        }
                        last_edit = Instant::now();
                    }
                    Ok(StreamEvent::ToolCallEnd { name, warnings, .. }) => {
                        if !warnings.is_empty() {
                            for w in &warnings {
                                tool_info.push_str(&format!(
                                    "⚠ Suspicious content in {}: {}\n",
                                    name, w
                                ));
                            }
                            let display = format_display(&full_response, &tool_info);
                            if let Some(id) = msg_id {
                                let _ = bot.edit_message_text(chat_id, id, &display).await;
                            } else {
                                let sent =
                                    send_in_thread(bot, chat_id, thread_id, &display).await?;
                                msg_id = Some(sent.id);
                            }
                            last_edit = Instant::now();
                        }
                    }
                    Ok(StreamEvent::Done) => break,
                    Ok(StreamEvent::ApprovalRequired { .. }) => {}
                    Err(e) => {
                        error!("Stream error: {}", e);
                        full_response.push_str(&format!("\n\nError: {}", e));
                        break;
                    }
                }
            }

            if full_response.is_empty() && tool_info.is_empty() {
                "(no response)".to_string()
            } else {
                full_response
            }
        }
        Err(e) => format!("Error: {}", e),
    };

    if let Err(e) = entry.agent.save_session_for_agent(TELEGRAM_AGENT_ID).await {
        debug!("Failed to save telegram session: {}", e);
    }

    drop(sessions);

    // Final render with HTML formatting, split into chunks if needed
    send_long_message(bot, chat_id, thread_id, msg_id, &response).await;

    // Clear the acknowledgment reaction
    let _ = bot
        .set_message_reaction(chat_id, user_msg_id)
        .reaction(Vec::<teloxide::types::ReactionType>::new())
        .send()
        .await;

    Ok(())
}

fn format_display(response: &str, tool_info: &str) -> String {
    let mut display = String::new();
    if !tool_info.is_empty() {
        display.push_str(tool_info);
        display.push('\n');
    }
    display.push_str(response);

    ellipsize_chars(&display, MAX_MESSAGE_LENGTH)
}

/// Send (or edit) a potentially long response, splitting into chunks if needed.
async fn send_long_message(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    edit_msg_id: Option<MessageId>,
    text: &str,
) {
    if text.chars().count() <= MAX_MESSAGE_LENGTH {
        send_or_edit_html(bot, chat_id, thread_id, edit_msg_id, text).await;
        return;
    }

    let chunks = split_text_chunks(text);

    if let Some(first) = chunks.first() {
        send_or_edit_html(bot, chat_id, thread_id, edit_msg_id, first).await;
    }
    for chunk in chunks.iter().skip(1) {
        send_or_edit_html(bot, chat_id, thread_id, None, chunk).await;
    }
}

fn split_text_chunks(text: &str) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = text.len();
        for (char_count, (idx, _)) in text[start..].char_indices().enumerate() {
            if char_count == MAX_MESSAGE_LENGTH {
                end = start + idx;
                break;
            }
        }
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks
}

/// Send or edit a message using HTML parse mode, falling back to plain text on error.
async fn send_or_edit_html(
    bot: &Bot,
    chat_id: ChatId,
    thread_id: Option<ThreadId>,
    msg_id: Option<MessageId>,
    text: &str,
) {
    let html = markdown_to_html(text);
    let result = if let Some(mid) = msg_id {
        bot.edit_message_text(chat_id, mid, &html)
            .parse_mode(ParseMode::Html)
            .await
    } else {
        let mut req = bot.send_message(chat_id, &html).parse_mode(ParseMode::Html);
        if let Some(tid) = thread_id {
            req = req.message_thread_id(tid);
        }
        req.await
    };

    if result.is_err() {
        // Fallback: plain text
        if let Some(mid) = msg_id {
            let _ = bot.edit_message_text(chat_id, mid, text).await;
        } else {
            let _ = send_in_thread(bot, chat_id, thread_id, text).await;
        }
    }
}

fn truncate_str(s: &str, max: usize) -> std::borrow::Cow<'_, str> {
    prefix_chars_cow(s, max)
}

/// Escape text for Telegram HTML parse mode.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn safe_link_url(url: &str) -> Option<&str> {
    let url = url.trim();
    if url.chars().any(char::is_control) {
        return None;
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Some(url)
    } else {
        None
    }
}

/// Convert markdown to Telegram-compatible HTML.
/// Handles: fenced code blocks, inline code, bold, italic, links, headers.
fn markdown_to_html(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut code_block_content = String::new();

    for line in text.lines() {
        if in_code_block {
            if line.starts_with("```") {
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

        let converted = if let Some(rest) = line.strip_prefix("### ") {
            format!("<b>{}</b>", escape_html(rest))
        } else if let Some(rest) = line.strip_prefix("## ") {
            format!("<b>{}</b>", escape_html(rest))
        } else if let Some(rest) = line.strip_prefix("# ") {
            format!("<b>{}</b>", escape_html(rest))
        } else {
            convert_inline_markdown(line)
        };

        result.push_str(&converted);
        result.push('\n');
    }

    // Handle unclosed code block
    if in_code_block && !code_block_content.is_empty() {
        result.push_str(&format!(
            "<pre><code>{}</code></pre>\n",
            escape_html(&code_block_content)
        ));
    }

    result
}

/// Convert inline markdown: `code`, **bold**, *italic*, [text](url)
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
                let link_text: String = chars[i + 1..text_end].iter().collect();
                let url: String = chars[text_end + 2..text_end + 2 + close_paren]
                    .iter()
                    .collect();
                if let Some(url) = safe_link_url(&url) {
                    result.push_str(&format!(
                        "<a href=\"{}\">{}</a>",
                        escape_html(url),
                        escape_html(&link_text)
                    ));
                } else {
                    result.push_str(&format!(
                        "{} ({})",
                        escape_html(&link_text),
                        escape_html(&url)
                    ));
                }
                i = text_end + 2 + close_paren + 1;
                continue;
            }
        }

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

fn find_closing(chars: &[char], start: usize, delim: &[char]) -> Option<usize> {
    let dlen = delim.len();
    if start + dlen > chars.len() {
        return None;
    }
    for i in start..=chars.len() - dlen {
        if chars[i..i + dlen] == *delim {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_display_respects_telegram_character_limit() {
        let display = format_display(&"✅".repeat(MAX_MESSAGE_LENGTH + 1), "");

        assert_eq!(display.chars().count(), MAX_MESSAGE_LENGTH);
        assert!(display.ends_with("..."));
    }

    #[test]
    fn split_text_chunks_splits_by_characters() {
        let text = "✅".repeat(MAX_MESSAGE_LENGTH + 1);
        let chunks = split_text_chunks(&text);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), MAX_MESSAGE_LENGTH);
        assert_eq!(chunks[1], "✅");
    }

    #[test]
    fn truncate_str_limits_multibyte_text_by_characters() {
        assert_eq!(truncate_str(&"✅".repeat(301), 300), "✅".repeat(300));
    }

    #[test]
    fn markdown_to_html_escapes_link_attribute_quotes() {
        let html = markdown_to_html(r#"[docs](https://example.com/?q="bad"&x='tag')"#);

        assert_eq!(
            html,
            "<a href=\"https://example.com/?q=&quot;bad&quot;&amp;x=&#39;tag&#39;\">docs</a>\n"
        );
    }

    #[test]
    fn markdown_to_html_does_not_link_unsafe_url_schemes() {
        let html = markdown_to_html("[docs](javascript:alert(1))");

        assert_eq!(html, "docs (javascript:alert(1))\n");
        assert!(!html.contains("<a href="));
    }
}
