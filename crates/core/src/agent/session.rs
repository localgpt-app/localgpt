//! Session management with Pi-compatible JSONL format
//!
//! JSONL format matches Pi's SessionManager for OpenClaw compatibility:
//! - Header: {type: "session", version, id, timestamp, cwd}
//! - Messages: {type: "message", message: {role, content, ...}}

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use uuid::Uuid;

use super::providers::{LLMProvider, Message, Role, ToolCall, Usage};
use crate::text::ellipsize_chars;

/// Current session format version (matches Pi)
pub const CURRENT_SESSION_VERSION: u32 = 1;

/// Maximum accepted length for externally supplied session IDs.
pub const MAX_SESSION_ID_LEN: usize = 128;

/// Validate a session ID before it is used as a filename.
pub fn is_valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= MAX_SESSION_ID_LEN
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn validate_session_id(session_id: &str) -> Result<()> {
    if is_valid_session_id(session_id) {
        Ok(())
    } else {
        anyhow::bail!(
            "Invalid session ID: use 1-{} ASCII letters, numbers, hyphens, or underscores",
            MAX_SESSION_ID_LEN
        )
    }
}

/// Session state (internal representation)
#[derive(Debug, Clone)]
pub struct Session {
    id: String,
    created_at: DateTime<Utc>,
    cwd: String,
    messages: Vec<SessionMessage>,
    system_context: Option<String>,
    token_count: usize,
    compaction_count: u32,
    memory_flush_compaction_count: u32,
    /// True if this session was interrupted mid-turn (daemon crash/restart)
    pub aborted_last_run: bool,
}

/// Message with metadata for persistence
#[derive(Debug, Clone)]
pub struct SessionMessage {
    pub message: Message,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api: Option<String>,
    pub usage: Option<MessageUsage>,
    pub stop_reason: Option<String>,
    pub timestamp: u64,
}

/// Per-message usage tracking (Pi-compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageUsage {
    pub input: u64,
    pub output: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u64>,
    pub total_tokens: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<MessageCost>,
}

/// Cost breakdown (Pi-compatible)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCost {
    pub input: f64,
    pub output: f64,
    pub total: f64,
}

impl From<&Usage> for MessageUsage {
    fn from(usage: &Usage) -> Self {
        Self {
            input: usage.input_tokens,
            output: usage.output_tokens,
            cache_read: None,
            cache_write: None,
            total_tokens: usage.total(),
            cost: None, // Cost calculation not implemented
        }
    }
}

impl SessionMessage {
    pub fn new(message: Message) -> Self {
        Self {
            message,
            provider: None,
            model: None,
            api: None,
            usage: None,
            stop_reason: None,
            timestamp: Utc::now().timestamp_millis() as u64,
        }
    }

    pub fn with_metadata(
        message: Message,
        provider: Option<&str>,
        model: Option<&str>,
        usage: Option<&Usage>,
        stop_reason: Option<&str>,
    ) -> Self {
        Self {
            message,
            provider: provider.map(|s| s.to_string()),
            model: model.map(|s| s.to_string()),
            api: provider.map(|p| format!("{}-messages", p)), // e.g., "anthropic-messages"
            usage: usage.map(MessageUsage::from),
            stop_reason: stop_reason.map(|s| s.to_string()),
            timestamp: Utc::now().timestamp_millis() as u64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionStatus {
    pub id: String,
    pub message_count: usize,
    pub token_count: usize,
    pub compaction_count: u32,
    pub api_input_tokens: u64,
    pub api_output_tokens: u64,
    pub api_cost_usd: f64,
    pub search_queries: u64,
    pub search_cached_hits: u64,
    pub search_cost_usd: f64,
}

impl Session {
    pub fn new() -> Self {
        Self::new_with_cwd(
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
        )
    }

    pub fn new_with_id(id: impl Into<String>) -> Result<Self> {
        let id = id.into();
        validate_session_id(&id)?;

        Ok(Self::new_with_id_and_cwd(
            id,
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
        ))
    }

    pub fn new_with_cwd(cwd: String) -> Self {
        Self::new_with_id_and_cwd(Uuid::new_v4().to_string(), cwd)
    }

    fn new_with_id_and_cwd(id: String, cwd: String) -> Self {
        Self {
            id,
            created_at: Utc::now(),
            cwd,
            messages: Vec::new(),
            system_context: None,
            token_count: 0,
            compaction_count: 0,
            memory_flush_compaction_count: 0,
            aborted_last_run: false,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Create a branched copy of this session with a new ID.
    /// Inherits all messages and context but gets a fresh identity.
    pub fn branch(&self) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            cwd: self.cwd.clone(),
            messages: self.messages.clone(),
            system_context: self.system_context.clone(),
            token_count: self.token_count,
            compaction_count: self.compaction_count,
            memory_flush_compaction_count: self.memory_flush_compaction_count,
            aborted_last_run: false,
        }
    }

    pub fn token_count(&self) -> usize {
        self.token_count
    }

    pub fn compaction_count(&self) -> u32 {
        self.compaction_count
    }

    pub fn should_memory_flush(&self) -> bool {
        self.memory_flush_compaction_count <= self.compaction_count
    }

    pub fn mark_memory_flushed(&mut self) {
        self.memory_flush_compaction_count = self.compaction_count + 1;
    }

    pub fn set_system_context(&mut self, context: String) {
        self.system_context = Some(context);
        self.recalculate_tokens();
    }

    /// Add a message without metadata
    pub fn add_message(&mut self, message: Message) {
        let tokens = estimate_tokens(&message.content);
        self.token_count += tokens;
        self.messages.push(SessionMessage::new(message));
    }

    /// Add a message with provider/model metadata
    pub fn add_message_with_metadata(
        &mut self,
        message: Message,
        provider: Option<&str>,
        model: Option<&str>,
        usage: Option<&Usage>,
        stop_reason: Option<&str>,
    ) {
        let tokens = estimate_tokens(&message.content);
        self.token_count += tokens;
        self.messages.push(SessionMessage::with_metadata(
            message,
            provider,
            model,
            usage,
            stop_reason,
        ));
    }

    pub fn messages_for_llm(&self) -> Vec<Message> {
        let mut messages = Vec::new();

        if let Some(ref context) = self.system_context {
            messages.push(Message {
                role: Role::System,
                content: context.clone(),
                tool_calls: None,
                tool_call_id: None,
                images: Vec::new(),
            });
        }

        messages.extend(self.messages.iter().map(|sm| sm.message.clone()));
        messages
    }

    pub fn messages(&self) -> Vec<&Message> {
        self.messages.iter().map(|sm| &sm.message).collect()
    }

    /// Get raw session messages with metadata (for API responses)
    pub fn raw_messages(&self) -> &[SessionMessage] {
        &self.messages
    }

    pub fn user_assistant_messages(&self) -> Vec<Message> {
        self.messages
            .iter()
            .filter(|sm| matches!(sm.message.role, Role::User | Role::Assistant))
            .map(|sm| sm.message.clone())
            .collect()
    }

    pub async fn compact(&mut self, provider: &dyn LLMProvider) -> Result<()> {
        let keep_count = 4;
        if self.messages.len() <= keep_count {
            return Ok(());
        }

        let to_summarize = &self.messages[..self.messages.len() - keep_count];

        let text: String = to_summarize
            .iter()
            .map(|sm| format!("{:?}: {}", sm.message.role, sm.message.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let summary = provider.summarize(&text).await?;

        let mut new_messages = vec![SessionMessage::new(Message {
            role: Role::System,
            content: format!("Previous conversation summary:\n\n{}", summary),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        })];

        new_messages.extend(self.messages[self.messages.len() - keep_count..].to_vec());

        self.messages = new_messages;
        self.compaction_count += 1;
        self.recalculate_tokens();

        Ok(())
    }

    fn recalculate_tokens(&mut self) {
        self.token_count = 0;

        if let Some(ref context) = self.system_context {
            self.token_count += estimate_tokens(context);
        }

        for sm in &self.messages {
            self.token_count += estimate_tokens(&sm.message.content);
        }
    }

    /// Save session in Pi-compatible JSONL format
    pub fn save(&self) -> Result<PathBuf> {
        validate_session_id(&self.id)?;
        let dir = get_sessions_dir()?;
        fs::create_dir_all(&dir)?;

        let path = dir.join(format!("{}.jsonl", self.id));
        self.save_to_path(&path)?;
        Ok(path)
    }

    pub fn save_for_agent(&self, agent_id: &str) -> Result<PathBuf> {
        validate_session_id(&self.id)?;
        let dir = get_sessions_dir_for_agent(agent_id)?;
        fs::create_dir_all(&dir)?;

        let path = dir.join(format!("{}.jsonl", self.id));
        self.save_to_path(&path)?;
        Ok(path)
    }

    /// Save session with optional encryption at rest.
    pub fn save_to_path_encrypted(
        &self,
        path: &PathBuf,
        encryption_key: Option<&crate::security::encrypt::EncryptionKey>,
    ) -> Result<()> {
        if let Some(key) = encryption_key {
            // Build JSONL content in memory, then encrypt and write as a single blob
            let content = self.build_jsonl_content()?;
            let encrypted = key.encrypt(content.as_bytes())?;

            let enc_path = path.with_extension("jsonl.enc");
            std::fs::write(&enc_path, &encrypted)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&enc_path, std::fs::Permissions::from_mode(0o600));
            }

            // Remove unencrypted file if it exists (migration)
            if path.exists() && path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                let _ = std::fs::remove_file(path);
            }
            return Ok(());
        }

        self.save_to_path(path)
    }

    pub fn save_to_path(&self, path: &PathBuf) -> Result<()> {
        let mut file = File::create(path)?;

        // Restrict permissions: session files may contain sensitive data
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            file.set_permissions(perms)?;
        }

        // Write Pi-compatible header
        let header = json!({
            "type": "session",
            "version": CURRENT_SESSION_VERSION,
            "id": self.id,
            "timestamp": self.created_at.to_rfc3339(),
            "cwd": self.cwd,
            // LocalGPT extensions (ignored by Pi but preserved)
            "compactionCount": self.compaction_count,
            "memoryFlushCompactionCount": self.memory_flush_compaction_count
        });
        writeln!(file, "{}", serde_json::to_string(&header)?)?;

        // Write system context as a system message
        if let Some(ref context) = self.system_context {
            let system_msg = self.format_message_entry(&SessionMessage::new(Message {
                role: Role::System,
                content: context.clone(),
                tool_calls: None,
                tool_call_id: None,
                images: Vec::new(),
            }));
            writeln!(file, "{}", serde_json::to_string(&system_msg)?)?;
        }

        // Write messages in Pi format
        for sm in &self.messages {
            let entry = self.format_message_entry(sm);
            writeln!(file, "{}", serde_json::to_string(&entry)?)?;
        }

        Ok(())
    }

    /// Build JSONL content as a String (used for encryption).
    fn build_jsonl_content(&self) -> Result<String> {
        let mut content = String::new();

        let header = json!({
            "type": "session",
            "version": CURRENT_SESSION_VERSION,
            "id": self.id,
            "timestamp": self.created_at.to_rfc3339(),
            "cwd": self.cwd,
            "compactionCount": self.compaction_count,
            "memoryFlushCompactionCount": self.memory_flush_compaction_count
        });
        content.push_str(&serde_json::to_string(&header)?);
        content.push('\n');

        if let Some(ref context) = self.system_context {
            let system_msg = self.format_message_entry(&SessionMessage::new(Message {
                role: Role::System,
                content: context.clone(),
                tool_calls: None,
                tool_call_id: None,
                images: Vec::new(),
            }));
            content.push_str(&serde_json::to_string(&system_msg)?);
            content.push('\n');
        }

        for sm in &self.messages {
            let entry = self.format_message_entry(sm);
            content.push_str(&serde_json::to_string(&entry)?);
            content.push('\n');
        }

        Ok(content)
    }

    /// Load session with optional decryption. Tries .jsonl.enc first, then .jsonl.
    pub fn load_from_path_encrypted(
        base_path: &PathBuf,
        session_id: &str,
        encryption_key: Option<&crate::security::encrypt::EncryptionKey>,
    ) -> Result<Self> {
        validate_session_id(session_id)?;
        let enc_path = base_path.with_extension("jsonl.enc");

        // Try encrypted file first
        if enc_path.exists() {
            let key = encryption_key.ok_or_else(|| {
                anyhow::anyhow!(
                    "Session {} is encrypted but no encryption key is available. \
                     Run `localgpt encrypt enable` or set security.encryption = true",
                    session_id
                )
            })?;

            let data = std::fs::read(&enc_path)?;
            let plaintext = key.decrypt(&data)?;
            let content = String::from_utf8(plaintext)
                .map_err(|_| anyhow::anyhow!("Decrypted session is not valid UTF-8"))?;

            return Self::load_from_string(&content, session_id);
        }

        // Fall back to unencrypted .jsonl
        if base_path.exists() {
            return Self::load_from_path(base_path, session_id);
        }

        anyhow::bail!("Session not found: {}", session_id)
    }

    /// Load session from an in-memory JSONL string.
    fn load_from_string(content: &str, session_id: &str) -> Result<Self> {
        validate_session_id(session_id)?;
        let mut session = Session {
            id: session_id.to_string(),
            created_at: Utc::now(),
            cwd: ".".to_string(),
            messages: Vec::new(),
            system_context: None,
            token_count: 0,
            compaction_count: 0,
            memory_flush_compaction_count: 0,
            aborted_last_run: false,
        };

        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let entry: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            session.parse_entry(&entry);
        }

        session.recalculate_tokens();
        Ok(session)
    }

    /// Format a message in Pi-compatible format
    fn format_message_entry(&self, sm: &SessionMessage) -> serde_json::Value {
        let role = match sm.message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::System => "system",
            Role::Tool => "toolResult",
        };

        // Build content array (Pi format)
        let mut content = Vec::new();

        // Add text content
        if !sm.message.content.is_empty() {
            content.push(json!({
                "type": "text",
                "text": sm.message.content
            }));
        }

        // Add images as image_url entries
        for img in &sm.message.images {
            content.push(json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{};base64,{}", img.media_type, img.data)
                }
            }));
        }

        // Build message object
        let mut message = json!({
            "role": role,
            "content": content
        });

        // Add tool calls if present
        if let Some(ref tool_calls) = sm.message.tool_calls {
            let tc: Vec<serde_json::Value> = tool_calls
                .iter()
                .map(|tc| {
                    json!({
                        "id": tc.id,
                        "name": tc.name,
                        "arguments": tc.arguments
                    })
                })
                .collect();
            message["toolCalls"] = json!(tc);
        }

        // Add tool call ID if present
        if let Some(ref id) = sm.message.tool_call_id {
            message["toolCallId"] = json!(id);
        }

        // Add metadata if available
        if let Some(ref provider) = sm.provider {
            message["provider"] = json!(provider);
        }
        if let Some(ref model) = sm.model {
            message["model"] = json!(model);
        }
        if let Some(ref api) = sm.api {
            message["api"] = json!(api);
        }
        if let Some(ref usage) = sm.usage {
            message["usage"] = serde_json::to_value(usage).unwrap_or(json!(null));
        }
        if let Some(ref reason) = sm.stop_reason {
            message["stopReason"] = json!(reason);
        }
        message["timestamp"] = json!(sm.timestamp);

        json!({
            "type": "message",
            "message": message
        })
    }

    /// Load session (supports both old and Pi formats)
    pub fn load(session_id: &str) -> Result<Self> {
        validate_session_id(session_id)?;
        let dir = get_sessions_dir()?;
        let path = dir.join(format!("{}.jsonl", session_id));

        if !path.exists() {
            anyhow::bail!("Session not found: {}", session_id);
        }

        Self::load_from_path(&path, session_id)
    }

    /// Load a session for a specific agent ID.
    pub fn load_for_agent(session_id: &str, agent_id: &str) -> Result<Self> {
        validate_session_id(session_id)?;
        let dir = get_sessions_dir_for_agent(agent_id)?;
        let path = dir.join(format!("{}.jsonl", session_id));

        if !path.exists() {
            anyhow::bail!("Session not found: {}", session_id);
        }

        Self::load_from_path(&path, session_id)
    }

    /// Get the number of messages in this session.
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Check if the last message is from the user (incomplete turn).
    pub fn is_incomplete_turn(&self) -> bool {
        self.messages
            .last()
            .map(|m| m.message.role == Role::User)
            .unwrap_or(false)
    }

    /// Inject a recovery message for sessions interrupted mid-turn.
    pub fn inject_recovery_message(&mut self) {
        if self.is_incomplete_turn() {
            self.add_message(Message {
                role: Role::System,
                content: "[System] The previous session was interrupted by a daemon restart. \
                          Please continue from where you left off. Review the conversation \
                          history above for context."
                    .to_string(),
                tool_calls: None,
                tool_call_id: None,
                images: Vec::new(),
            });
        }
    }

    fn load_from_path(path: &PathBuf, session_id: &str) -> Result<Self> {
        validate_session_id(session_id)?;
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut session = Session {
            id: session_id.to_string(),
            created_at: Utc::now(),
            cwd: ".".to_string(),
            messages: Vec::new(),
            system_context: None,
            token_count: 0,
            compaction_count: 0,
            memory_flush_compaction_count: 0,
            aborted_last_run: false,
        };

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };
            session.parse_entry(&entry);
        }

        session.recalculate_tokens();
        Ok(session)
    }

    /// Parse a single JSONL entry into this session.
    fn parse_entry(&mut self, entry: &serde_json::Value) {
        match entry["type"].as_str() {
            Some("session") => {
                if let Some(ts) = entry["timestamp"].as_str()
                    && let Ok(dt) = DateTime::parse_from_rfc3339(ts)
                {
                    self.created_at = dt.with_timezone(&Utc);
                }
                if let Some(cwd) = entry["cwd"].as_str() {
                    self.cwd = cwd.to_string();
                }
                if let Some(count) = entry["compactionCount"].as_u64() {
                    self.compaction_count = count as u32;
                }
                if let Some(count) = entry["memoryFlushCompactionCount"].as_u64() {
                    self.memory_flush_compaction_count = count as u32;
                }
            }
            Some("message") => {
                if let Some(msg_obj) = entry.get("message")
                    && let Some(sm) = Self::parse_pi_message(msg_obj)
                {
                    if sm.message.role == Role::System && self.system_context.is_none() {
                        self.system_context = Some(sm.message.content);
                    } else {
                        self.messages.push(sm);
                    }
                }
            }
            _ => {}
        }
    }

    /// Parse Pi format message
    fn parse_pi_message(msg: &serde_json::Value) -> Option<SessionMessage> {
        let role = match msg["role"].as_str()? {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "system" => Role::System,
            "toolResult" | "tool" => Role::Tool,
            _ => return None,
        };

        // Extract text content from content array
        let content = if let Some(arr) = msg["content"].as_array() {
            arr.iter()
                .filter_map(|item| {
                    if item["type"].as_str() == Some("text") {
                        item["text"].as_str().map(|s| s.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("")
        } else if let Some(s) = msg["content"].as_str() {
            s.to_string()
        } else {
            String::new()
        };

        // Parse tool calls
        let tool_calls = msg["toolCalls"].as_array().map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    Some(ToolCall {
                        id: tc["id"].as_str()?.to_string(),
                        name: tc["name"].as_str()?.to_string(),
                        arguments: tc["arguments"].as_str().unwrap_or("{}").to_string(),
                    })
                })
                .collect()
        });

        let tool_call_id = msg["toolCallId"].as_str().map(|s| s.to_string());

        // Parse usage
        let usage = serde_json::from_value(msg["usage"].clone()).ok();

        Some(SessionMessage {
            message: Message {
                role,
                content,
                tool_calls,
                tool_call_id,
                images: Vec::new(), // TODO: parse images from content array
            },
            provider: msg["provider"].as_str().map(|s| s.to_string()),
            model: msg["model"].as_str().map(|s| s.to_string()),
            api: msg["api"].as_str().map(|s| s.to_string()),
            usage,
            stop_reason: msg["stopReason"].as_str().map(|s| s.to_string()),
            timestamp: msg["timestamp"].as_u64().unwrap_or(0),
        })
    }

    pub fn status(&self) -> SessionStatus {
        SessionStatus {
            id: self.id.clone(),
            message_count: self.messages.len(),
            token_count: self.token_count,
            compaction_count: self.compaction_count,
            api_input_tokens: 0,
            api_output_tokens: 0,
            api_cost_usd: 0.0,
            search_queries: 0,
            search_cached_hits: 0,
            search_cost_usd: 0.0,
        }
    }

    pub fn status_with_usage(
        &self,
        input_tokens: u64,
        output_tokens: u64,
        cost_usd: f64,
        search_queries: u64,
        search_cached_hits: u64,
        search_cost_usd: f64,
    ) -> SessionStatus {
        SessionStatus {
            id: self.id.clone(),
            message_count: self.messages.len(),
            token_count: self.token_count,
            compaction_count: self.compaction_count,
            api_input_tokens: input_tokens,
            api_output_tokens: output_tokens,
            api_cost_usd: cost_usd,
            search_queries,
            search_cached_hits,
            search_cost_usd,
        }
    }

    pub fn auto_save(&self) -> Result<()> {
        if self.messages.is_empty() {
            return Ok(());
        }
        self.save()?;
        Ok(())
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// Default agent ID (matches OpenClaw's default)
pub const DEFAULT_AGENT_ID: &str = "main";

fn get_sessions_dir() -> Result<PathBuf> {
    get_sessions_dir_for_agent(DEFAULT_AGENT_ID)
}

pub fn get_sessions_dir_for_agent(agent_id: &str) -> Result<PathBuf> {
    let paths = crate::paths::Paths::resolve()?;
    Ok(paths.sessions_dir(agent_id))
}

pub fn get_state_dir() -> Result<PathBuf> {
    let paths = crate::paths::Paths::resolve()?;
    Ok(paths.state_dir)
}

/// Scan session files for an agent and recover any that were interrupted mid-turn.
/// Sets `aborted_last_run = false` and injects a recovery system message.
/// Returns the number of sessions recovered.
pub fn recover_orphaned_sessions(agent_id: &str) -> Result<usize> {
    let sessions_dir = get_sessions_dir_for_agent(agent_id)?;
    if !sessions_dir.exists() {
        return Ok(0);
    }

    let mut recovered = 0;

    for entry in fs::read_dir(&sessions_dir)? {
        let path = entry?.path();
        if path.extension().map(|e| e != "jsonl").unwrap_or(true) {
            continue;
        }

        let session_id = match path.file_stem().and_then(|s| s.to_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };

        if !is_valid_session_id(&session_id) {
            continue;
        }

        if let Ok(mut session) = Session::load_from_path(&path, &session_id)
            && session.aborted_last_run
        {
            tracing::info!("Recovering orphaned session: {}", session.id);
            session.aborted_last_run = false;
            session.inject_recovery_message();
            session.save_to_path(&path)?;
            recovered += 1;
        }
    }

    Ok(recovered)
}

fn estimate_tokens(text: &str) -> usize {
    text.len() / 4
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub message_count: usize,
    pub file_size: u64,
    pub preview: String,
    pub end_preview: String,
}

pub fn list_sessions() -> Result<Vec<SessionInfo>> {
    list_sessions_for_agent(DEFAULT_AGENT_ID)
}

pub fn list_sessions_for_agent(agent_id: &str) -> Result<Vec<SessionInfo>> {
    let sessions_dir = get_sessions_dir_for_agent(agent_id)?;

    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();

    for entry in fs::read_dir(&sessions_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map(|e| e != "jsonl").unwrap_or(true) {
            continue;
        }

        let filename = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

        if !is_valid_session_id(filename) {
            continue;
        }

        let metadata = fs::metadata(&path)?;
        let file_size = metadata.len();

        if let Ok(file) = File::open(&path) {
            let reader = BufReader::new(file);
            let mut lines = reader.lines();

            if let Some(Ok(first_line)) = lines.next()
                && let Ok(header) = serde_json::from_str::<serde_json::Value>(&first_line)
            {
                // Pi format header
                if header["type"].as_str() == Some("session") {
                    let created_at = header["timestamp"]
                        .as_str()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(Utc::now);

                    let mut message_count = 0;
                    let mut preview = String::new();
                    let mut end_preview = String::new();

                    for line in lines.map_while(Result::ok) {
                        message_count += 1;
                        if let Ok(entry) = serde_json::from_str::<serde_json::Value>(&line)
                            && entry["type"].as_str() == Some("message")
                            && let Some(msg_obj) = entry.get("message")
                        {
                            let role = msg_obj["role"].as_str().unwrap_or("");
                            if role == "user" || role == "assistant" {
                                let content = if let Some(arr) = msg_obj["content"].as_array() {
                                    arr.iter()
                                        .filter_map(|item| {
                                            if item["type"].as_str() == Some("text") {
                                                item["text"].as_str().map(|s| s.to_string())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("")
                                } else if let Some(s) = msg_obj["content"].as_str() {
                                    s.to_string()
                                } else {
                                    String::new()
                                };

                                let clean_text = content.replace('\n', " ").trim().to_string();
                                if !clean_text.is_empty() {
                                    let formatted = ellipsize_chars(&clean_text, 60);

                                    if preview.is_empty() {
                                        preview = formatted.clone();
                                    }
                                    // Always update end_preview so it holds the last one
                                    end_preview = formatted;
                                }
                            }
                        }
                    }

                    sessions.push(SessionInfo {
                        id: filename.to_string(),
                        created_at,
                        message_count,
                        file_size,
                        preview,
                        end_preview,
                    });
                }
            }
        }
    }

    sessions.sort_by_key(|s| std::cmp::Reverse(s.created_at));
    Ok(sessions)
}

pub fn get_last_session_id() -> Result<Option<String>> {
    get_last_session_id_for_agent(DEFAULT_AGENT_ID)
}

pub fn get_last_session_id_for_agent(agent_id: &str) -> Result<Option<String>> {
    let sessions = list_sessions_for_agent(agent_id)?;
    Ok(sessions.first().map(|s| s.id.clone()))
}

#[derive(Debug, Clone)]
pub struct SessionSearchResult {
    pub session_id: String,
    pub created_at: DateTime<Utc>,
    pub message_preview: String,
    pub match_count: usize,
}

pub fn search_sessions(query: &str) -> Result<Vec<SessionSearchResult>> {
    search_sessions_for_agent(DEFAULT_AGENT_ID, query)
}

pub fn search_sessions_for_agent(agent_id: &str, query: &str) -> Result<Vec<SessionSearchResult>> {
    let sessions_dir = get_sessions_dir_for_agent(agent_id)?;

    if !sessions_dir.exists() {
        return Ok(Vec::new());
    }

    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    for entry in fs::read_dir(&sessions_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map(|e| e == "jsonl").unwrap_or(false)
            && let Some(filename) = path.file_stem().and_then(|s| s.to_str())
            && is_valid_session_id(filename)
            && let Ok(content) = fs::read_to_string(&path)
        {
            let content_lower = content.to_lowercase();
            let match_count = content_lower.matches(&query_lower).count();

            if match_count > 0 {
                let preview = extract_match_preview(&content, &query_lower, 100);

                let created_at = fs::metadata(&path)
                    .and_then(|m| m.created())
                    .map(DateTime::<Utc>::from)
                    .unwrap_or_else(|_| Utc::now());

                results.push(SessionSearchResult {
                    session_id: filename.to_string(),
                    created_at,
                    message_preview: preview,
                    match_count,
                });
            }
        }
    }

    results.sort_by_key(|r| std::cmp::Reverse(r.match_count));
    Ok(results)
}

fn extract_match_preview(content: &str, query_lower: &str, max_len: usize) -> String {
    if let Some((match_start, match_end)) = find_case_insensitive_char_range(content, query_lower) {
        let chars: Vec<char> = content.chars().collect();
        let half_len = max_len / 2;
        let start = match_start.saturating_sub(half_len);
        let end = (match_end + half_len).min(chars.len());

        let cleaned: String = chars[start..end]
            .iter()
            .map(|c| if c.is_whitespace() { ' ' } else { *c })
            .collect();

        let trimmed = cleaned.trim();
        let prefix = if start > 0 { "..." } else { "" };
        let suffix = if end < content.len() { "..." } else { "" };

        format!("{}{}{}", prefix, trimmed, suffix)
    } else {
        String::new()
    }
}

fn find_case_insensitive_char_range(content: &str, query_lower: &str) -> Option<(usize, usize)> {
    if query_lower.is_empty() {
        return Some((0, 0));
    }

    let chars: Vec<char> = content.chars().collect();
    for start in 0..chars.len() {
        let mut folded = String::new();
        for (offset, ch) in chars[start..].iter().enumerate() {
            folded.extend(ch.to_lowercase());
            if folded == query_lower {
                return Some((start, start + offset + 1));
            }
            if !query_lower.starts_with(&folded) {
                break;
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct PanicSummarizer;

    #[async_trait]
    impl LLMProvider for PanicSummarizer {
        fn name(&self) -> String {
            "panic-summarizer".to_string()
        }

        async fn chat(
            &self,
            _messages: &[Message],
            _tools: Option<&[super::super::providers::ToolSchema]>,
        ) -> Result<super::super::providers::LLMResponse> {
            unreachable!("session compaction test does not call chat")
        }

        async fn summarize(&self, _text: &str) -> Result<String> {
            panic!("summarize should not be called when there is no history to compact")
        }
    }

    fn user_message(content: &str) -> Message {
        Message {
            role: Role::User,
            content: content.to_string(),
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        }
    }

    #[test]
    fn test_session_new() {
        let session = Session::new();
        assert!(!session.id().is_empty());
        assert_eq!(session.token_count(), 0);
        assert_eq!(session.compaction_count(), 0);
    }

    #[test]
    fn test_session_new_with_id() {
        let session = Session::new_with_id("http-session_123").unwrap();
        assert_eq!(session.id(), "http-session_123");
    }

    #[test]
    fn test_session_rejects_invalid_ids() {
        assert!(Session::new_with_id("").is_err());
        assert!(Session::new_with_id("../escape").is_err());
        assert!(Session::new_with_id("has space").is_err());
        assert!(Session::new_with_id("unicode-\u{2603}").is_err());
    }

    #[test]
    fn test_session_save_load_preserves_custom_id() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("custom-session_1.jsonl");
        let session = Session::new_with_id("custom-session_1").unwrap();

        session.save_to_path(&path).unwrap();
        let loaded = Session::load_from_path(&path, "custom-session_1").unwrap();

        assert_eq!(loaded.id(), "custom-session_1");
    }

    #[test]
    fn test_message_usage_from() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        };
        let msg_usage = MessageUsage::from(&usage);
        assert_eq!(msg_usage.input, 100);
        assert_eq!(msg_usage.output, 50);
        assert_eq!(msg_usage.total_tokens, 150);
    }

    #[test]
    fn test_extract_match_preview_handles_unicode_case_expansion() {
        let content = format!("{}needle suffix", "İ".repeat(100));
        let preview = extract_match_preview(&content, "needle", 20);

        assert!(preview.contains("needle"));
        assert!(preview.starts_with("..."));
    }

    #[tokio::test]
    async fn compact_skips_when_only_keep_window_exists() {
        let mut session = Session::new();
        for i in 0..4 {
            session.add_message(user_message(&format!("message {i}")));
        }

        session.compact(&PanicSummarizer).await.unwrap();

        assert_eq!(session.message_count(), 4);
        assert_eq!(session.compaction_count(), 0);
        assert_eq!(session.messages()[0].content, "message 0");
    }
}
