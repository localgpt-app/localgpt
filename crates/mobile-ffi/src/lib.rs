//! LocalGPT Mobile — UniFFI bindings for iOS and Android.
//!
//! This crate exposes a minimal, thread-safe surface from `localgpt-core`
//! to Swift (iOS) and Kotlin (Android) via UniFFI proc-macros.
//!
//! All async work is dispatched onto a per-instance tokio runtime so that
//! the foreign caller never needs to provide an executor.

uniffi::setup_scaffolding!();

use std::sync::Arc;

use localgpt_core::agent::{Agent, AgentConfig, AgentHandle};
use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;

// ---------------------------------------------------------------------------
// Types exposed to foreign code
// ---------------------------------------------------------------------------

/// A search result returned from memory search.
#[derive(uniffi::Record)]
pub struct SearchResult {
    pub file: String,
    pub content: String,
    pub score: f64,
}

/// Session status information.
#[derive(uniffi::Record)]
pub struct SessionStatus {
    pub model: String,
    pub tokens_used: u64,
    pub tokens_available: u64,
}

// ---------------------------------------------------------------------------
// The main entry point: LocalGPTClient
// ---------------------------------------------------------------------------

/// Thread-safe client for interacting with LocalGPT from mobile apps.
///
/// Create one instance at app launch with `LocalGPTClient::new()` and keep
/// it alive for the lifetime of the app. All methods are safe to call from
/// any thread.
#[derive(uniffi::Object)]
pub struct LocalGPTClient {
    handle: AgentHandle,
    config: Config,
    runtime: tokio::runtime::Runtime,
}

#[uniffi::export]
impl LocalGPTClient {
    /// Create a new client rooted at the given directory.
    ///
    /// `data_dir` should be the app's document or library directory
    /// (e.g. `NSDocumentDirectory` on iOS, `Context.filesDir` on Android).
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Result<Arc<Self>, MobileError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .map_err(|e| MobileError::Init(e.to_string()))?;

        let config =
            Config::load_from_dir(&data_dir).map_err(|e| MobileError::Init(e.to_string()))?;

        // Mobile builds exclude claude-cli feature (no subprocess support).
        // If config has claude-cli/* model, override to API-based default.
        let model = if config.agent.default_model.starts_with("claude-cli/") {
            // Default to Anthropic API for mobile (requires api_key in config)
            "anthropic/claude-sonnet-4-6".to_string()
        } else {
            config.agent.default_model.clone()
        };

        let agent_config = AgentConfig {
            model,
            context_window: config.agent.context_window,
            reserve_tokens: config.agent.reserve_tokens,
        };

        let memory = MemoryManager::new_with_full_config(&config.memory, Some(&config), "mobile")
            .map_err(|e| MobileError::Init(e.to_string()))?;

        let agent = rt
            .block_on(Agent::new(agent_config, &config, memory))
            .map_err(|e| MobileError::Init(e.to_string()))?;

        let handle = AgentHandle::new(agent);

        // Start a fresh session
        rt.block_on(handle.new_session())
            .map_err(|e| MobileError::Init(e.to_string()))?;

        Ok(Arc::new(Self {
            handle,
            config,
            runtime: rt,
        }))
    }

    /// Send a chat message and return the full response.
    pub fn chat(&self, message: String) -> Result<String, MobileError> {
        self.runtime
            .block_on(self.handle.chat(&message))
            .map_err(|e| MobileError::Chat(e.to_string()))
    }

    /// Search memory files.
    pub fn memory_search(
        &self,
        query: String,
        max_results: u32,
    ) -> Result<Vec<SearchResult>, MobileError> {
        let chunks = self
            .runtime
            .block_on(self.handle.memory_search(&query, max_results as usize))
            .map_err(|e| MobileError::Memory(e.to_string()))?;

        Ok(chunks
            .into_iter()
            .map(|c| SearchResult {
                file: c.file,
                content: c.content,
                score: c.score,
            })
            .collect())
    }

    /// Read a memory file by name (e.g. "MEMORY.md").
    pub fn memory_get(&self, filename: String) -> Result<String, MobileError> {
        self.runtime
            .block_on(self.handle.memory_get(&filename))
            .map_err(|e| MobileError::Memory(e.to_string()))
    }

    /// Get the SOUL.md content (persona/tone guidance).
    pub fn get_soul(&self) -> Result<String, MobileError> {
        self.runtime
            .block_on(self.handle.memory_get("SOUL.md"))
            .map_err(|e| MobileError::Memory(e.to_string()))
    }

    /// Write new SOUL.md content.
    pub fn set_soul(&self, content: String) -> Result<(), MobileError> {
        let workspace = self.config.workspace_path();
        std::fs::write(workspace.join("SOUL.md"), content)
            .map_err(|e| MobileError::Memory(e.to_string()))
    }

    /// Get the MEMORY.md content.
    pub fn get_memory(&self) -> Result<String, MobileError> {
        self.runtime
            .block_on(self.handle.memory_get("MEMORY.md"))
            .map_err(|e| MobileError::Memory(e.to_string()))
    }

    /// Get the HEARTBEAT.md content.
    pub fn get_heartbeat(&self) -> Result<String, MobileError> {
        self.runtime
            .block_on(self.handle.memory_get("HEARTBEAT.md"))
            .map_err(|e| MobileError::Memory(e.to_string()))
    }

    /// Write new HEARTBEAT.md content.
    pub fn set_heartbeat(&self, content: String) -> Result<(), MobileError> {
        let workspace = self.config.workspace_path();
        std::fs::write(workspace.join("HEARTBEAT.md"), content)
            .map_err(|e| MobileError::Memory(e.to_string()))
    }

    /// Get the current model name.
    pub fn get_model(&self) -> String {
        self.runtime.block_on(self.handle.model())
    }

    /// Switch to a different model.
    pub fn set_model(&self, model: String) -> Result<(), MobileError> {
        self.runtime
            .block_on(self.handle.set_model(&model))
            .map_err(|e| MobileError::Config(e.to_string()))
    }

    /// Get session status.
    pub fn session_status(&self) -> SessionStatus {
        let (used, usable, _total) = self.runtime.block_on(self.handle.context_usage());
        SessionStatus {
            model: self.runtime.block_on(self.handle.model()),
            tokens_used: used as u64,
            tokens_available: usable as u64,
        }
    }

    /// Start a fresh session.
    pub fn new_session(&self) -> Result<(), MobileError> {
        self.runtime
            .block_on(self.handle.new_session())
            .map_err(|e| MobileError::Chat(e.to_string()))
    }

    /// Compact the current session to free context space.
    pub fn compact_session(&self) -> Result<(), MobileError> {
        self.runtime
            .block_on(self.handle.compact_session())
            .map(|_| ())
            .map_err(|e| MobileError::Chat(e.to_string()))
    }

    /// Clear session history.
    pub fn clear_session(&self) {
        self.runtime.block_on(self.handle.clear_session());
    }

    /// Check if this is a brand new workspace (first run).
    /// Mobile apps can use this to display a custom welcome message.
    pub fn is_brand_new(&self) -> bool {
        self.runtime.block_on(self.handle.is_brand_new())
    }

    /// Configure an API key for a provider.
    pub fn configure_provider(&self, provider: String, api_key: String) -> Result<(), MobileError> {
        let workspace = self.config.workspace_path();
        // Write a minimal config snippet that the user can expand
        let snippet = format!("[providers.{}]\napi_key = \"{}\"\n", provider, api_key);
        let path = workspace
            .parent()
            .unwrap_or(&workspace)
            .join("provider_keys")
            .join(format!("{}.toml", provider));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| MobileError::Config(e.to_string()))?;
        }
        std::fs::write(&path, snippet).map_err(|e| MobileError::Config(e.to_string()))
    }

    /// List available provider names.
    pub fn list_providers(&self) -> Vec<String> {
        vec![
            "anthropic".to_string(),
            "openai".to_string(),
            "ollama".to_string(),
            "glm".to_string(),
        ]
    }
}

// ---------------------------------------------------------------------------
// Standalone functions
// ---------------------------------------------------------------------------

/// Get the first-run welcome message text.
/// Mobile apps can display this when `is_brand_new()` returns true.
#[uniffi::export]
pub fn get_welcome_message() -> String {
    localgpt_core::agent::FIRST_RUN_WELCOME.to_string()
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileError {
    #[error("Initialization error: {0}")]
    Init(String),
    #[error("Chat error: {0}")]
    Chat(String),
    #[error("Memory error: {0}")]
    Memory(String),
    #[error("Config error: {0}")]
    Config(String),
}
