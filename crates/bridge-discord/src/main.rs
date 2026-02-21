//! Discord bridge for LocalGPT
//!
//! Connects to the LocalGPT Bridge Manager to retrieve the Discord bot token,
//! then runs a Discord bot that exposes LocalGPT to Discord servers or DMs.
//!
//! # Pairing
//! On first contact from any user, a 6-digit pairing code is printed to the
//! bridge logs. The user must DM that code back to claim ownership. Only the
//! paired user may subsequently use the bot.
//!
//! # Usage
//! ```bash
//! # 1. Register your Discord bot token with the bridge manager
//! localgpt bridge register --id discord --secret "your-bot-token"
//!
//! # 2. Start the bridge
//! localgpt-bridge-discord
//! ```

use anyhow::Result;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serenity::Client;
use serenity::all::{
    ChannelId, Context, CreateMessage, EditMessage, EventHandler, GatewayIntents, Message,
    MessageId, Ready,
};
use serenity::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tarpc::context;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use localgpt_bridge::connect;
use localgpt_core::agent::{Agent, AgentConfig, StreamEvent, extract_tool_detail};
use localgpt_core::concurrency::TurnGate;
use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;

/// Agent ID for Discord sessions
const DISCORD_AGENT_ID: &str = "discord";

/// Discord message character limit
const MAX_MESSAGE_LENGTH: usize = 2000;

/// Debounce interval for streaming edits (seconds).
/// Discord rate-limits edits so we keep this conservative.
const EDIT_DEBOUNCE_SECS: u64 = 2;

// ── Pairing ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct PairedUser {
    user_id: u64,
    username: String,
    paired_at: String,
}

fn pairing_file_path() -> Result<PathBuf> {
    let paths = localgpt_core::paths::Paths::resolve()?;
    Ok(paths.state_dir.join("discord_paired_user.json"))
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

// ── Bot state ──────────────────────────────────────────────────────────────

struct SessionEntry {
    agent: Agent,
    last_accessed: Instant,
}

struct BotState {
    config: Config,
    /// Sessions keyed by Discord channel ID (or user ID for DMs)
    sessions: Mutex<HashMap<u64, SessionEntry>>,
    memory: MemoryManager,
    turn_gate: TurnGate,
    paired_user: Mutex<Option<PairedUser>>,
    pending_pairing_code: Mutex<Option<String>>,
}

// ── Event handler ─────────────────────────────────────────────────────────

struct Handler {
    state: Arc<BotState>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _ctx: Context, data_about_bot: Ready) {
        info!("Discord bot connected as: {}", data_about_bot.user.name);
    }

    async fn message(&self, ctx: Context, msg: Message) {
        // Ignore bots (including ourselves)
        if msg.author.bot {
            return;
        }

        let content = msg.content.trim().to_string();
        if content.is_empty() {
            return;
        }

        let channel_id = msg.channel_id;
        let author_id = msg.author.id.get();
        let author_name = msg.author.tag();

        // Check pairing
        let is_paired;
        {
            let paired = self.state.paired_user.lock().await;
            is_paired = paired
                .as_ref()
                .map(|p| p.user_id == author_id)
                .unwrap_or(false);
            let has_paired = paired.is_some();
            drop(paired);

            if has_paired && !is_paired {
                let _ = channel_id
                    .say(
                        &ctx.http,
                        "Not authorized. This bot is paired with another user.",
                    )
                    .await;
                return;
            }

            if !has_paired {
                // Initiate or complete pairing flow
                self.handle_pairing(&ctx, channel_id, author_id, &author_name, &content)
                    .await;
                return;
            }
        }

        // Slash commands start with '/'
        if content.starts_with('/') {
            self.handle_command(&ctx, channel_id, &content).await;
        } else {
            self.handle_chat(&ctx, channel_id, &content).await;
        }
    }
}

impl Handler {
    async fn handle_pairing(
        &self,
        ctx: &Context,
        channel_id: ChannelId,
        user_id: u64,
        username: &str,
        text: &str,
    ) {
        let mut pending = self.state.pending_pairing_code.lock().await;

        if let Some(ref code) = *pending {
            if text.trim() == code.as_str() {
                let paired = PairedUser {
                    user_id,
                    username: username.to_string(),
                    paired_at: chrono::Utc::now().to_rfc3339(),
                };

                if let Err(e) = save_paired_user(&paired) {
                    error!("Failed to save Discord pairing: {}", e);
                    let _ = channel_id
                        .say(&ctx.http, "Pairing failed (could not save). Check logs.")
                        .await;
                    return;
                }

                *self.state.paired_user.lock().await = Some(paired);
                *pending = None;

                info!(
                    "Discord bot paired with user {} (ID: {})",
                    username, user_id
                );
                let _ = channel_id
                    .say(
                        &ctx.http,
                        "✅ Paired successfully! You can now chat with LocalGPT.\n\
                         Use `/new` to start a fresh session or `/help` for all commands.",
                    )
                    .await;
            } else {
                let _ = channel_id
                    .say(&ctx.http, "❌ Invalid pairing code. Please try again.")
                    .await;
            }
        } else {
            let code = generate_pairing_code();
            println!("\n========================================");
            println!("  DISCORD PAIRING CODE: {}", code);
            println!("========================================\n");
            info!(
                "Discord pairing code generated for user {} (ID: {})",
                username, user_id
            );

            *pending = Some(code);
            let _ = channel_id
                .say(
                    &ctx.http,
                    "👋 Welcome! A pairing code has been printed to the bridge logs.\n\
                     Please send it here to pair your account.",
                )
                .await;
        }
    }

    async fn handle_command(&self, ctx: &Context, channel_id: ChannelId, text: &str) {
        let parts: Vec<&str> = text.splitn(2, ' ').collect();
        let cmd = parts[0];
        let args = parts.get(1).map(|s| s.trim()).unwrap_or("");

        match cmd {
            "/help" | "/start" => {
                let help = format!(
                    "**LocalGPT Discord Bridge**\n\n{}",
                    localgpt_core::commands::format_help_text(
                        localgpt_core::commands::Interface::Discord
                    )
                );
                send_long_message(ctx, channel_id, None, &help).await;
            }
            "/new" => {
                self.state.sessions.lock().await.remove(&channel_id.get());
                let _ = channel_id
                    .say(
                        &ctx.http,
                        "🆕 Session cleared. Send a message to start a new conversation.",
                    )
                    .await;
            }
            "/status" => {
                let sessions = self.state.sessions.lock().await;
                let status_text = if let Some(entry) = sessions.get(&channel_id.get()) {
                    let status = entry.agent.session_status();
                    let (used, usable, total) = entry.agent.context_usage();
                    let mut t = format!(
                        "**Session active**\n\
                         Model: `{}`\n\
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
                        let cache_pct = (status.search_cached_hits as f64
                            / status.search_queries as f64)
                            * 100.0;
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
                let _ = channel_id.say(&ctx.http, &status_text).await;
            }
            "/compact" => {
                let mut sessions = self.state.sessions.lock().await;
                match sessions.get_mut(&channel_id.get()) {
                    Some(entry) => {
                        entry.last_accessed = Instant::now();
                        match entry.agent.compact_session().await {
                            Ok((before, after)) => {
                                let _ = channel_id
                                    .say(
                                        &ctx.http,
                                        format!("✅ Compacted: {} → {} tokens", before, after),
                                    )
                                    .await;
                            }
                            Err(e) => {
                                let _ = channel_id
                                    .say(&ctx.http, format!("❌ Compact failed: {}", e))
                                    .await;
                            }
                        }
                    }
                    None => {
                        let _ = channel_id.say(&ctx.http, "No active session.").await;
                    }
                }
            }
            "/clear" => {
                let mut sessions = self.state.sessions.lock().await;
                if let Some(entry) = sessions.get_mut(&channel_id.get()) {
                    entry.agent.clear_session();
                    entry.last_accessed = Instant::now();
                    let _ = channel_id
                        .say(&ctx.http, "🗑️ Session history cleared.")
                        .await;
                } else {
                    let _ = channel_id.say(&ctx.http, "No active session.").await;
                }
            }
            "/memory" => {
                if args.is_empty() {
                    let _ = channel_id
                        .say(&ctx.http, "Usage: `/memory <search query>`")
                        .await;
                } else {
                    match self.state.memory.search(args, 5) {
                        Ok(results) => {
                            if results.is_empty() {
                                let _ = channel_id.say(&ctx.http, "No results found.").await;
                            } else {
                                let mut t = format!("**Memory search:** \"{}\"\n\n", args);
                                for (i, r) in results.iter().enumerate() {
                                    t.push_str(&format!(
                                        "{}. `{}` (L{}-{})\n{}\n\n",
                                        i + 1,
                                        r.file,
                                        r.line_start,
                                        r.line_end,
                                        truncate_str(&r.content, 300),
                                    ));
                                }
                                send_long_message(ctx, channel_id, None, &t).await;
                            }
                        }
                        Err(e) => {
                            let _ = channel_id
                                .say(&ctx.http, format!("Search error: {}", e))
                                .await;
                        }
                    }
                }
            }
            "/model" => {
                if args.is_empty() {
                    let sessions = self.state.sessions.lock().await;
                    let current = sessions
                        .get(&channel_id.get())
                        .map(|e| e.agent.model().to_string())
                        .unwrap_or_else(|| self.state.config.agent.default_model.clone());
                    let _ = channel_id
                        .say(
                            &ctx.http,
                            format!("Current model: `{}`\n\nUsage: `/model <name>`", current),
                        )
                        .await;
                } else {
                    let mut sessions = self.state.sessions.lock().await;
                    if let Some(entry) = sessions.get_mut(&channel_id.get()) {
                        match entry.agent.set_model(args) {
                            Ok(()) => {
                                let _ = channel_id
                                    .say(&ctx.http, format!("✅ Switched to model: `{}`", args))
                                    .await;
                            }
                            Err(e) => {
                                let _ = channel_id
                                    .say(&ctx.http, format!("❌ Failed to switch model: {}", e))
                                    .await;
                            }
                        }
                    } else {
                        let _ = channel_id
                            .say(
                                &ctx.http,
                                "No active session. Send a message first, then switch models.",
                            )
                            .await;
                    }
                }
            }
            "/skills" => {
                let workspace_path = self.state.config.workspace_path();
                match localgpt_core::agent::load_skills(&workspace_path) {
                    Ok(skills) => {
                        if skills.is_empty() {
                            let _ = channel_id.say(&ctx.http, "No skills installed.").await;
                        } else {
                            let summary = localgpt_core::agent::get_skills_summary(&skills);
                            let _ = channel_id.say(&ctx.http, &summary).await;
                        }
                    }
                    Err(e) => {
                        let _ = channel_id
                            .say(&ctx.http, format!("Failed to load skills: {}", e))
                            .await;
                    }
                }
            }
            "/unpair" => {
                *self.state.paired_user.lock().await = None;
                if let Ok(path) = pairing_file_path() {
                    let _ = std::fs::remove_file(path);
                }
                self.state.sessions.lock().await.remove(&channel_id.get());
                info!("Discord bot: user unpaired");
                let _ = channel_id
                    .say(
                        &ctx.http,
                        "🔓 Unpaired. Send any message to start a new pairing.",
                    )
                    .await;
            }
            _ => {
                let _ = channel_id
                    .say(
                        &ctx.http,
                        "Unknown command. Use `/help` for available commands.",
                    )
                    .await;
            }
        }
    }

    async fn handle_chat(&self, ctx: &Context, channel_id: ChannelId, text: &str) {
        // Send a placeholder "thinking" message that we'll edit with streamed output
        let thinking_msg = match channel_id
            .send_message(&ctx.http, CreateMessage::new().content("⏳ Thinking..."))
            .await
        {
            Ok(m) => m,
            Err(e) => {
                error!("Failed to send thinking message: {}", e);
                return;
            }
        };

        let _gate_permit = self.state.turn_gate.acquire().await;
        let mut sessions = self.state.sessions.lock().await;

        if let std::collections::hash_map::Entry::Vacant(e) = sessions.entry(channel_id.get()) {
            let agent_config = AgentConfig {
                model: self.state.config.agent.default_model.clone(),
                context_window: self.state.config.agent.context_window,
                reserve_tokens: self.state.config.agent.reserve_tokens,
            };

            match Agent::new(agent_config, &self.state.config, self.state.memory.clone()).await {
                Ok(mut agent) => {
                    if let Err(err) = agent.new_session().await {
                        error!("Failed to create session: {}", err);
                        let _ = thinking_msg
                            .channel_id
                            .edit_message(
                                &ctx.http,
                                thinking_msg.id,
                                EditMessage::new().content(format!("❌ Error: {}", err)),
                            )
                            .await;
                        return;
                    }
                    // Send welcome message on first run
                    if agent.is_brand_new() {
                        let _ = channel_id
                            .say(&ctx.http, localgpt_core::agent::FIRST_RUN_WELCOME)
                            .await;
                    }
                    e.insert(SessionEntry {
                        agent,
                        last_accessed: Instant::now(),
                    });
                }
                Err(err) => {
                    error!("Failed to create agent: {}", err);
                    let _ = thinking_msg
                        .channel_id
                        .edit_message(
                            &ctx.http,
                            thinking_msg.id,
                            EditMessage::new().content(format!("❌ Error: {}", err)),
                        )
                        .await;
                    return;
                }
            }
        }

        let entry = sessions.get_mut(&channel_id.get()).unwrap();
        entry.last_accessed = Instant::now();

        let response = match entry.agent.chat_stream_with_tools(text, Vec::new()).await {
            Ok(event_stream) => {
                let mut full_response = String::new();
                let mut last_edit = Instant::now();
                let mut pinned_stream = std::pin::pin!(event_stream);
                let mut tool_info = String::new();

                while let Some(event) = pinned_stream.next().await {
                    match event {
                        Ok(StreamEvent::Content(delta)) => {
                            full_response.push_str(&delta);
                            if last_edit.elapsed().as_secs() >= EDIT_DEBOUNCE_SECS {
                                let display = format_display(&full_response, &tool_info);
                                let _ = thinking_msg
                                    .channel_id
                                    .edit_message(
                                        &ctx.http,
                                        thinking_msg.id,
                                        EditMessage::new().content(&display),
                                    )
                                    .await;
                                last_edit = Instant::now();
                            }
                        }
                        Ok(StreamEvent::ToolCallStart {
                            name, arguments, ..
                        }) => {
                            let detail = extract_tool_detail(&name, &arguments);
                            let info_line = if let Some(d) = detail {
                                format!("🔧 `{}({})`\n", name, d)
                            } else {
                                format!("🔧 `{}`\n", name)
                            };
                            tool_info.push_str(&info_line);
                            let display = format_display(&full_response, &tool_info);
                            let _ = thinking_msg
                                .channel_id
                                .edit_message(
                                    &ctx.http,
                                    thinking_msg.id,
                                    EditMessage::new().content(&display),
                                )
                                .await;
                            last_edit = Instant::now();
                        }
                        Ok(StreamEvent::ToolCallEnd { name, warnings, .. }) => {
                            if !warnings.is_empty() {
                                for w in &warnings {
                                    tool_info.push_str(&format!(
                                        "⚠️ Suspicious content in `{}`: {}\n",
                                        name, w
                                    ));
                                }
                                let display = format_display(&full_response, &tool_info);
                                let _ = thinking_msg
                                    .channel_id
                                    .edit_message(
                                        &ctx.http,
                                        thinking_msg.id,
                                        EditMessage::new().content(&display),
                                    )
                                    .await;
                                last_edit = Instant::now();
                            }
                        }
                        Ok(StreamEvent::Done) => break,
                        Err(e) => {
                            error!("Stream error: {}", e);
                            full_response.push_str(&format!("\n\n❌ Error: {}", e));
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
            Err(e) => format!("❌ Error: {}", e),
        };

        if let Err(e) = entry.agent.save_session_for_agent(DISCORD_AGENT_ID).await {
            debug!("Failed to save Discord session: {}", e);
        }

        drop(sessions);

        // Final edit (or send new messages) with full response, split if needed
        send_long_message(ctx, channel_id, Some(thinking_msg.id), &response).await;
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn format_display(response: &str, tool_info: &str) -> String {
    let mut display = String::new();
    if !tool_info.is_empty() {
        display.push_str(tool_info);
        display.push('\n');
    }
    display.push_str(response);
    // Truncate for Discord's limit during streaming previews
    if display.len() > MAX_MESSAGE_LENGTH {
        let mut end = MAX_MESSAGE_LENGTH - 3;
        while end > 0 && !display.is_char_boundary(end) {
            end -= 1;
        }
        display.truncate(end);
        display.push_str("...");
    }
    display
}

/// Send (or edit) a potentially long response, splitting into chunks if needed.
async fn send_long_message(
    ctx: &Context,
    channel_id: ChannelId,
    edit_msg_id: Option<MessageId>,
    text: &str,
) {
    let chunks = split_text_chunks(text);

    if let Some(first) = chunks.first() {
        if let Some(mid) = edit_msg_id {
            if let Err(e) = channel_id
                .edit_message(&ctx.http, mid, EditMessage::new().content(*first))
                .await
            {
                warn!("Failed to edit message: {}. Sending as new.", e);
                let _ = channel_id.say(&ctx.http, *first).await;
            }
        } else {
            let _ = channel_id.say(&ctx.http, *first).await;
        }
    }

    for chunk in chunks.iter().skip(1) {
        let _ = channel_id.say(&ctx.http, *chunk).await;
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

fn truncate_str(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

// ── Entry point ────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .init();

    info!("Starting LocalGPT Discord Bridge...");

    // 1. Connect to Bridge Manager
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

    // 3. Fetch Discord bot token
    let token_bytes = match client
        .get_credentials(context::current(), "discord".to_string())
        .await?
    {
        Ok(t) => t,
        Err(e) => {
            error!(
                "Failed to retrieve Discord credentials: {}. Have you run 'localgpt bridge register --id discord ...'?",
                e
            );
            std::process::exit(1);
        }
    };

    let token = String::from_utf8(token_bytes)
        .map_err(|_| anyhow::anyhow!("Invalid UTF-8 in Discord token"))?;
    info!("Successfully retrieved Discord token.");

    // 4. Initialize shared state
    let config = Config::load()?;
    let memory =
        MemoryManager::new_with_full_config(&config.memory, Some(&config), DISCORD_AGENT_ID)?;
    let turn_gate = TurnGate::new();

    let paired_user = load_paired_user();
    if let Some(ref user) = paired_user {
        info!("Paired with user {} (ID: {})", user.username, user.user_id);
    } else {
        info!("No paired user. The first person to DM the bot will be prompted to pair.");
    }

    let state = Arc::new(BotState {
        config,
        sessions: Mutex::new(HashMap::new()),
        memory,
        turn_gate,
        paired_user: Mutex::new(paired_user),
        pending_pairing_code: Mutex::new(None),
    });

    // 5. Start Discord bot
    // Only the intents we actually need: receiving messages + their content
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(&token, intents)
        .event_handler(Handler { state })
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create Discord client: {}", e))?;

    info!("Discord bot started. Listening for messages...");

    client
        .start()
        .await
        .map_err(|e| anyhow::anyhow!("Discord client error: {}", e))?;

    Ok(())
}
