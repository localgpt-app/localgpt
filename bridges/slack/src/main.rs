//! Slack bridge for LocalGPT
//!
//! Connects to the LocalGPT Bridge Manager to retrieve Slack credentials,
//! then runs a Slack bot via Socket Mode that exposes LocalGPT to Slack
//! workspaces.
//!
//! # Setup
//! ```bash
//! # 1. Register bot_token and app_token (JSON) with the bridge manager
//! localgpt bridge register --id slack --secret '{"bot_token":"xoxb-...","app_token":"xapp-..."}'
//!
//! # 2. Start the bridge
//! localgpt-bridge-slack
//! ```

use anyhow::Result;
use futures::StreamExt;
use serde::Deserialize;
use slack_morphism::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use localgpt_core::agent::{Agent, AgentConfig, StreamEvent, extract_tool_detail};
use localgpt_core::concurrency::TurnGate;
use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;
use localgpt_core::text::ellipsize_chars;

/// Agent ID for Slack sessions
const SLACK_AGENT_ID: &str = "slack";

/// Slack message character limit
const MAX_MESSAGE_LENGTH: usize = 4000;

/// Default debounce for streaming edits (milliseconds)
const DEFAULT_EDIT_THROTTLE_MS: u64 = 1500;

// ── Config ────────────────────────────────────────────────────────────────

/// Activation mode for group channels
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum MentionMode {
    Always,
    #[default]
    Mention,
}

/// DM access policy
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum DmPolicy {
    #[default]
    Open,
    Allowlist,
}

/// Group/channel access policy
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
enum GroupPolicy {
    #[default]
    Open,
    Allowlist,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct SlackConfig {
    bot_token: String,
    app_token: String,
    dm_policy: DmPolicy,
    group_policy: GroupPolicy,
    mention_mode: MentionMode,
    allowlist: Vec<String>,
    channel_allowlist: Vec<String>,
    thread_replies: bool,
    edit_throttle_ms: u64,
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            bot_token: String::new(),
            app_token: String::new(),
            dm_policy: DmPolicy::Open,
            group_policy: GroupPolicy::Open,
            mention_mode: MentionMode::Mention,
            allowlist: Vec::new(),
            channel_allowlist: Vec::new(),
            thread_replies: true,
            edit_throttle_ms: DEFAULT_EDIT_THROTTLE_MS,
        }
    }
}

// ── State ─────────────────────────────────────────────────────────────────

struct SessionEntry {
    agent: Agent,
    last_accessed: Instant,
}

struct BotState {
    config: Config,
    slack_config: SlackConfig,
    sessions: Mutex<HashMap<String, SessionEntry>>,
    memory: MemoryManager,
    turn_gate: TurnGate,
    bot_user_id: String,
    client: Arc<SlackClient<SlackClientHyperHttpsConnector>>,
    bot_token: SlackApiToken,
}

// ── Access control ────────────────────────────────────────────────────────

fn is_allowed(cfg: &SlackConfig, user_id: &str, channel_id: &str, is_dm: bool) -> bool {
    if is_dm {
        match cfg.dm_policy {
            DmPolicy::Open => true,
            DmPolicy::Allowlist => cfg.allowlist.contains(&user_id.to_string()),
        }
    } else {
        match cfg.group_policy {
            GroupPolicy::Open => true,
            GroupPolicy::Allowlist => cfg.channel_allowlist.contains(&channel_id.to_string()),
        }
    }
}

fn is_mentioned(bot_user_id: &str, text: &str) -> bool {
    // Slack mentions look like <@U12345> in the raw text
    let mention_tag = format!("<@{}>", bot_user_id);
    text.contains(&mention_tag)
}

fn strip_mention(bot_user_id: &str, text: &str) -> String {
    let mention_tag = format!("<@{}>", bot_user_id);
    text.replace(&mention_tag, "").trim().to_string()
}

// ── Slack API helpers ─────────────────────────────────────────────────────

async fn post_message(
    state: &BotState,
    channel: &str,
    thread_ts: Option<&SlackTs>,
    text: &str,
) -> Option<SlackTs> {
    let session = state.client.open_session(&state.bot_token);
    let channel_id: SlackChannelId = channel.into();

    let content = SlackMessageContent::new().with_text(truncate(text, MAX_MESSAGE_LENGTH));
    let mut req = SlackApiChatPostMessageRequest::new(channel_id, content);

    if let Some(ts) = thread_ts {
        req = req.with_thread_ts(ts.clone());
    }

    match session.chat_post_message(&req).await {
        Ok(resp) => Some(resp.ts),
        Err(e) => {
            error!("Failed to post Slack message: {}", e);
            None
        }
    }
}

async fn edit_message(state: &BotState, channel: &str, ts: &SlackTs, text: &str) {
    let session = state.client.open_session(&state.bot_token);
    let channel_id: SlackChannelId = channel.into();

    let content = SlackMessageContent::new().with_text(truncate(text, MAX_MESSAGE_LENGTH));
    let req = SlackApiChatUpdateRequest::new(channel_id, content, ts.clone());

    if let Err(e) = session.chat_update(&req).await {
        debug!("Failed to edit Slack message: {}", e);
    }
}

fn truncate(s: &str, max: usize) -> String {
    ellipsize_chars(s, max)
}

// ── Event handling ────────────────────────────────────────────────────────

async fn handle_message_event(state: Arc<BotState>, msg: SlackMessageEvent) {
    // Skip bot's own messages and subtypes (edits, deletes, etc.)
    if msg.sender.bot_id.is_some() {
        return;
    }
    if msg.subtype.is_some() {
        return;
    }

    let user_id = match msg.sender.user {
        Some(ref u) => u.to_string(),
        None => return,
    };

    let channel_id = msg
        .origin
        .channel
        .as_ref()
        .map(|c| c.to_string())
        .unwrap_or_default();
    if channel_id.is_empty() {
        return;
    }

    let text = msg
        .content
        .as_ref()
        .and_then(|c| c.text.as_deref())
        .unwrap_or("");
    if text.is_empty() {
        return;
    }

    // Detect DM: Slack DMs have channel IDs starting with 'D'
    let is_dm = channel_id.starts_with('D');

    // Access control
    if !is_allowed(&state.slack_config, &user_id, &channel_id, is_dm) {
        return;
    }

    // Mention gate for group channels
    if !is_dm
        && state.slack_config.mention_mode == MentionMode::Mention
        && !is_mentioned(&state.bot_user_id, text)
    {
        return;
    }

    let text = strip_mention(&state.bot_user_id, text);

    // Thread support: reply in the original message's thread or start a new one
    let thread_ts = if state.slack_config.thread_replies {
        msg.origin
            .thread_ts
            .clone()
            .or_else(|| Some(msg.origin.ts.clone()))
    } else {
        None
    };

    // Post placeholder
    let placeholder_ts = post_message(&state, &channel_id, thread_ts.as_ref(), "...").await;
    let placeholder_ts = match placeholder_ts {
        Some(ts) => ts,
        None => return,
    };

    // Session key: user_id for DMs, channel_id for groups
    let session_key = if is_dm {
        user_id.clone()
    } else {
        channel_id.clone()
    };

    // Run agent with streaming
    run_agent_streaming(
        state,
        &session_key,
        &text,
        &channel_id,
        &placeholder_ts,
        thread_ts,
    )
    .await;
}

async fn run_agent_streaming(
    state: Arc<BotState>,
    session_key: &str,
    input: &str,
    channel_id: &str,
    placeholder_ts: &SlackTs,
    _thread_ts: Option<SlackTs>,
) {
    let _gate_permit = state.turn_gate.acquire().await;
    let mut sessions = state.sessions.lock().await;

    // Create session if needed
    if !sessions.contains_key(session_key) {
        let agent_config = AgentConfig {
            model: state.config.agent.default_model.clone(),
            context_window: state.config.agent.context_window,
            reserve_tokens: state.config.agent.reserve_tokens,
        };

        match Agent::new(agent_config, &state.config, Arc::new(state.memory.clone())).await {
            Ok(mut agent) => {
                if let Err(err) = agent.new_session().await {
                    edit_message(
                        &state,
                        channel_id,
                        placeholder_ts,
                        &format!("Error: {}", err),
                    )
                    .await;
                    return;
                }
                sessions.insert(
                    session_key.to_string(),
                    SessionEntry {
                        agent,
                        last_accessed: Instant::now(),
                    },
                );
            }
            Err(err) => {
                error!("Failed to create agent: {}", err);
                edit_message(
                    &state,
                    channel_id,
                    placeholder_ts,
                    &format!("Error: {}", err),
                )
                .await;
                return;
            }
        }
    }

    let entry = sessions.get_mut(session_key).unwrap();
    entry.last_accessed = Instant::now();

    let throttle = Duration::from_millis(state.slack_config.edit_throttle_ms);

    let response = match entry.agent.chat_stream_with_tools(input, Vec::new()).await {
        Ok(event_stream) => {
            let mut full_response = String::new();
            let mut last_edit = Instant::now();
            let mut pinned_stream = std::pin::pin!(event_stream);
            let mut tool_info = String::new();

            while let Some(event) = pinned_stream.next().await {
                match event {
                    Ok(StreamEvent::Content(delta)) => {
                        full_response.push_str(&delta);
                        if last_edit.elapsed() >= throttle || full_response.len() < 50 {
                            let display = format_display(&full_response, &tool_info);
                            edit_message(&state, channel_id, placeholder_ts, &display).await;
                            last_edit = Instant::now();
                        }
                    }
                    Ok(StreamEvent::ToolCallStart {
                        name, arguments, ..
                    }) => {
                        let detail = extract_tool_detail(&name, &arguments);
                        let info_line = if let Some(d) = detail {
                            format!(":wrench: {}({})\n", name, d)
                        } else {
                            format!(":wrench: {}\n", name)
                        };
                        tool_info.push_str(&info_line);
                        let display = format_display(&full_response, &tool_info);
                        edit_message(&state, channel_id, placeholder_ts, &display).await;
                        last_edit = Instant::now();
                    }
                    Ok(StreamEvent::Done) => break,
                    Err(e) => {
                        error!("Stream error: {}", e);
                        full_response.push_str(&format!("\n\nError: {}", e));
                        break;
                    }
                    _ => {}
                }
            }

            if full_response.is_empty() && tool_info.is_empty() {
                "(no response)".to_string()
            } else {
                format_display(&full_response, &tool_info)
            }
        }
        Err(e) => format!("Error: {}", e),
    };

    // Save session
    if let Err(e) = entry.agent.save_session_for_agent(SLACK_AGENT_ID).await {
        debug!("Failed to save slack session: {}", e);
    }

    drop(sessions);

    // Final edit
    edit_message(&state, channel_id, placeholder_ts, &response).await;
}

fn format_display(response: &str, tool_info: &str) -> String {
    let mut display = String::new();
    if !tool_info.is_empty() {
        display.push_str(tool_info);
        display.push('\n');
    }
    display.push_str(response);
    truncate(&display, MAX_MESSAGE_LENGTH)
}

// ── Main ──────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .init();

    info!("Starting LocalGPT Slack Bridge...");

    // 1. Connect to Bridge Manager to get credentials
    let paths = localgpt_core::paths::Paths::resolve()?;
    let socket_path = paths.bridge_socket_name();

    info!("Connecting to bridge socket: {}", socket_path);
    let client = localgpt_bridge::connect(&socket_path).await?;

    // 2. Verify protocol version
    match client.get_version(tarpc::context::current()).await {
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

    // 3. Fetch Slack credentials (JSON with bot_token and app_token)
    let cred_bytes = match client
        .get_credentials(tarpc::context::current(), "slack".to_string())
        .await?
    {
        Ok(t) => t,
        Err(e) => {
            error!(
                "Failed to retrieve Slack credentials: {}. \
                 Have you run 'localgpt bridge register --id slack --secret '{{\"bot_token\":\"xoxb-...\",\"app_token\":\"xapp-...\"}}'?",
                e
            );
            std::process::exit(1);
        }
    };

    let cred_str = String::from_utf8(cred_bytes)
        .map_err(|_| anyhow::anyhow!("Invalid UTF-8 in Slack credentials"))?;

    let slack_config: SlackConfig = serde_json::from_str(&cred_str).unwrap_or_else(|_| {
        // If not JSON, treat as plain bot_token (minimal setup)
        SlackConfig {
            bot_token: cred_str.clone(),
            ..Default::default()
        }
    });

    if slack_config.bot_token.is_empty() {
        anyhow::bail!("Slack bot_token is empty");
    }
    if slack_config.app_token.is_empty() {
        anyhow::bail!("Slack app_token is empty (required for Socket Mode)");
    }

    info!("Successfully retrieved Slack credentials.");

    // 4. Initialize Slack client and authenticate
    let slack_client = Arc::new(SlackClient::new(SlackClientHyperConnector::new()?));
    let bot_token = SlackApiToken::new(SlackApiTokenValue::from(slack_config.bot_token.clone()));

    let session = slack_client.open_session(&bot_token);
    let auth = session.auth_test().await?;
    let bot_user_id = auth.user_id.to_string();
    info!(
        "Authenticated as bot user: {} ({})",
        auth.user.as_deref().unwrap_or("unknown"),
        bot_user_id
    );

    // 5. Build shared state
    let config = Config::load()?;
    let memory =
        MemoryManager::new_with_full_config(&config.memory, Some(&config), SLACK_AGENT_ID)?;

    let state = Arc::new(BotState {
        config: config.clone(),
        slack_config: slack_config.clone(),
        sessions: Mutex::new(HashMap::new()),
        memory,
        turn_gate: TurnGate::new(),
        bot_user_id,
        client: slack_client.clone(),
        bot_token: bot_token.clone(),
    });

    // 6. Set up Socket Mode listener with user_state for shared BotState
    let app_token = SlackApiToken::new(SlackApiTokenValue::from(slack_config.app_token.clone()));

    fn error_handler(
        err: Box<dyn std::error::Error + Send + Sync>,
        _client: Arc<SlackClient<SlackClientHyperHttpsConnector>>,
        _states: SlackClientEventsUserState,
    ) -> http::StatusCode {
        error!("Slack Socket Mode error: {:?}", err);
        http::StatusCode::OK
    }

    async fn push_events_fn(
        event: SlackPushEventCallback,
        _client: Arc<SlackClient<SlackClientHyperHttpsConnector>>,
        states: SlackClientEventsUserState,
    ) -> UserCallbackResult<()> {
        let bot_state: Option<Arc<BotState>> = {
            let guard = states.read().await;
            guard.get_user_state::<Arc<BotState>>().cloned()
        };

        if let Some(state) = bot_state
            && let SlackEventCallbackBody::Message(msg) = event.event
        {
            handle_message_event(state, msg).await;
        }
        Ok(())
    }

    let callbacks = SlackSocketModeListenerCallbacks::new().with_push_events(push_events_fn);

    let listener_environment = Arc::new(
        SlackClientEventsListenerEnvironment::new(slack_client.clone())
            .with_error_handler(error_handler)
            .with_user_state(state),
    );

    let socket_config = SlackClientSocketModeConfig::new();
    let socket_listener =
        SlackClientSocketModeListener::new(&socket_config, listener_environment, callbacks);

    info!("Slack bot started. Listening for messages via Socket Mode.");

    socket_listener.listen_for(&app_token).await?;
    socket_listener.serve().await;

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_deserialize_defaults() {
        let json = r#"{"bot_token":"xoxb-test","app_token":"xapp-test"}"#;
        let cfg: SlackConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.bot_token, "xoxb-test");
        assert_eq!(cfg.app_token, "xapp-test");
        assert_eq!(cfg.dm_policy, DmPolicy::Open);
        assert_eq!(cfg.group_policy, GroupPolicy::Open);
        assert_eq!(cfg.mention_mode, MentionMode::Mention);
        assert!(cfg.thread_replies);
        assert_eq!(cfg.edit_throttle_ms, DEFAULT_EDIT_THROTTLE_MS);
    }

    #[test]
    fn test_mention_detection() {
        assert!(is_mentioned("U12345", "Hello <@U12345> how are you?"));
        assert!(!is_mentioned("U12345", "Hello everyone"));
        assert!(!is_mentioned("U12345", "Hello <@U99999>"));
    }

    #[test]
    fn test_strip_mention() {
        assert_eq!(
            strip_mention("U12345", "Hello <@U12345> world"),
            "Hello  world"
        );
        assert_eq!(strip_mention("U12345", "<@U12345> do this"), "do this");
        assert_eq!(strip_mention("U12345", "no mention"), "no mention");
    }

    #[test]
    fn test_access_control_dm_open() {
        let cfg = SlackConfig {
            dm_policy: DmPolicy::Open,
            ..Default::default()
        };
        assert!(is_allowed(&cfg, "any_user", "D123", true));
    }

    #[test]
    fn test_access_control_dm_allowlist() {
        let cfg = SlackConfig {
            dm_policy: DmPolicy::Allowlist,
            allowlist: vec!["U123".to_string()],
            ..Default::default()
        };
        assert!(is_allowed(&cfg, "U123", "D456", true));
        assert!(!is_allowed(&cfg, "U999", "D456", true));
    }

    #[test]
    fn test_access_control_group_open() {
        let cfg = SlackConfig {
            group_policy: GroupPolicy::Open,
            ..Default::default()
        };
        assert!(is_allowed(&cfg, "any_user", "C123", false));
    }

    #[test]
    fn test_access_control_group_allowlist() {
        let cfg = SlackConfig {
            group_policy: GroupPolicy::Allowlist,
            channel_allowlist: vec!["C123".to_string()],
            ..Default::default()
        };
        assert!(is_allowed(&cfg, "any_user", "C123", false));
        assert!(!is_allowed(&cfg, "any_user", "C999", false));
    }

    #[test]
    fn test_thread_ts_logic() {
        // thread_replies=true should use origin ts as thread_ts
        let cfg = SlackConfig {
            thread_replies: true,
            ..Default::default()
        };
        assert!(cfg.thread_replies);
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 100), "short");
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_limits_multibyte_by_characters() {
        let truncated = truncate(&"✅".repeat(MAX_MESSAGE_LENGTH + 1), MAX_MESSAGE_LENGTH);

        assert_eq!(truncated.chars().count(), MAX_MESSAGE_LENGTH);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_format_display() {
        let display = format_display("response text", "");
        assert_eq!(display, "response text");

        let display = format_display("response text", ":wrench: tool_a\n");
        assert!(display.starts_with(":wrench: tool_a"));
        assert!(display.contains("response text"));
    }
}
