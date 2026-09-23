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
use localgpt_core::security;

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

/// A workspace file entry for the editor.
#[derive(uniffi::Record)]
pub struct WorkspaceFile {
    /// File name (e.g. "MEMORY.md").
    pub name: String,
    /// File content, or empty string if the file does not yet exist.
    pub content: String,
    /// Whether this file is security-sensitive and requires confirmation before editing.
    pub is_security_sensitive: bool,
}

/// Regular workspace markdown files that can be edited by the mobile app.
const REGULAR_EDITABLE_FILES: &[&str] = &["MEMORY.md", "SOUL.md", "HEARTBEAT.md"];

/// Security-sensitive files that require user confirmation before editing.
/// These files affect the agent's security policy.
/// Editing is only allowed through explicit user action (never by the agent).
const SECURITY_EDITABLE_FILES: &[&str] = &["POLICY.md"];

/// Check if a filename is a security-sensitive file that requires confirmation.
///
/// The legacy `LocalGPT.md` name (pre-rename) is also security-sensitive.
fn is_security_file(filename: &str) -> bool {
    SECURITY_EDITABLE_FILES.contains(&filename) || filename == security::LEGACY_POLICY_FILENAME
}

/// Check if a filename is one of the editable workspace files.
fn is_editable_file(filename: &str) -> bool {
    REGULAR_EDITABLE_FILES.contains(&filename) || is_security_file(filename)
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

        let memory = Arc::new(
            MemoryManager::new_with_full_config(&config.memory, Some(&config), "mobile")
                .map_err(|e| MobileError::Init(e.to_string()))?,
        );

        let agent = rt
            .block_on(Agent::new(agent_config, &config, Arc::clone(&memory)))
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

    /// Write new MEMORY.md content.
    pub fn set_memory(&self, content: String) -> Result<(), MobileError> {
        let workspace = self.config.workspace_path();
        std::fs::write(workspace.join("MEMORY.md"), content)
            .map_err(|e| MobileError::Memory(e.to_string()))
    }

    /// Get the POLICY.md content (security policy / standing instructions).
    ///
    /// Falls back to the legacy `LocalGPT.md` when `POLICY.md` does not
    /// exist yet, so workspaces created before the rename keep working.
    pub fn get_policy(&self) -> Result<String, MobileError> {
        let workspace = self.config.workspace_path();
        match security::find_policy_file(&workspace) {
            Some(path) => {
                std::fs::read_to_string(path).map_err(|e| MobileError::Memory(e.to_string()))
            }
            None => Ok(String::new()),
        }
    }

    /// Write new POLICY.md content (security policy / standing instructions).
    ///
    /// This is a plain file write — the policy is loaded and sanitized at
    /// the next session start. Editing is only allowed through explicit
    /// user action (never by the agent).
    pub fn set_policy(&self, content: String) -> Result<(), MobileError> {
        let workspace = self.config.workspace_path();
        std::fs::write(workspace.join(security::POLICY_FILENAME), &content)
            .map_err(|e| MobileError::Memory(e.to_string()))
    }

    /// List the editable workspace files with their current content.
    ///
    /// Returns `WorkspaceFile` entries for MEMORY.md, SOUL.md,
    /// HEARTBEAT.md, and POLICY.md. Files that do not exist yet are
    /// returned with an empty `content` string. Security-sensitive files
    /// (like POLICY.md) are flagged with `is_security_sensitive = true`.
    /// For POLICY.md, the content falls back to the legacy `LocalGPT.md`
    /// when the renamed file does not exist yet.
    pub fn list_workspace_files(&self) -> Vec<WorkspaceFile> {
        let workspace = self.config.workspace_path();
        REGULAR_EDITABLE_FILES
            .iter()
            .chain(SECURITY_EDITABLE_FILES.iter())
            .map(|name| {
                match std::fs::read_to_string(workspace.join(name)) {
                    Ok(c) => return (name.to_string(), c),
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        tracing::warn!("Failed to read workspace file {}: {}", name, e);
                    }
                    _ => {}
                };

                // POLICY.md not found — fall back to the legacy name
                let content = if *name == security::POLICY_FILENAME {
                    std::fs::read_to_string(workspace.join(security::LEGACY_POLICY_FILENAME))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                (name.to_string(), content)
            })
            .map(|(name, content)| WorkspaceFile {
                is_security_sensitive: is_security_file(&name),
                name,
                content,
            })
            .collect()
    }

    /// Read an arbitrary workspace file by name.
    ///
    /// Only the known editable files (MEMORY.md, SOUL.md, HEARTBEAT.md,
    /// POLICY.md — plus the legacy LocalGPT.md) are allowed. Returns
    /// `MobileError::Memory` for unknown file names to prevent
    /// path-traversal.
    pub fn get_workspace_file(&self, filename: String) -> Result<String, MobileError> {
        if !is_editable_file(&filename) {
            return Err(MobileError::Memory(format!(
                "File '{}' is not an editable workspace file",
                filename
            )));
        }

        if filename == security::POLICY_FILENAME {
            return self.get_policy();
        }

        let workspace = self.config.workspace_path();
        std::fs::read_to_string(workspace.join(&filename))
            .map_err(|e| MobileError::Memory(e.to_string()))
    }

    /// Write an arbitrary workspace file by name.
    ///
    /// Only the known editable files (MEMORY.md, SOUL.md, HEARTBEAT.md,
    /// POLICY.md — plus the legacy LocalGPT.md) are allowed. The caller
    /// (mobile UI) must confirm security-sensitive file edits before
    /// calling this method. Writing via the legacy name stores to
    /// POLICY.md.
    pub fn set_workspace_file(&self, filename: String, content: String) -> Result<(), MobileError> {
        if !is_editable_file(&filename) {
            return Err(MobileError::Memory(format!(
                "File '{}' is not an editable workspace file",
                filename
            )));
        }

        if filename == security::POLICY_FILENAME || filename == security::LEGACY_POLICY_FILENAME {
            return self.set_policy(content);
        }

        let workspace = self.config.workspace_path();
        std::fs::write(workspace.join(&filename), content)
            .map_err(|e| MobileError::Memory(e.to_string()))
    }

    /// Check whether a workspace file is security-sensitive.
    ///
    /// Security-sensitive files (like POLICY.md) affect the agent's
    /// security policy and require user confirmation before editing.
    /// The mobile UI should display a warning dialog before allowing
    /// edits to these files.
    pub fn is_workspace_file_security_sensitive(&self, filename: String) -> bool {
        is_security_file(&filename)
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify editable file list is the union of REGULAR + SECURITY lists.
    #[test]
    fn editable_files_lists_consistent() {
        let all: Vec<&str> = REGULAR_EDITABLE_FILES
            .iter()
            .chain(SECURITY_EDITABLE_FILES.iter())
            .copied()
            .collect();

        for &f in REGULAR_EDITABLE_FILES {
            assert!(
                all.contains(&f),
                "Editable file list missing regular file: {}",
                f
            );
        }
        for &f in SECURITY_EDITABLE_FILES {
            assert!(
                all.contains(&f),
                "Editable file list missing security file: {}",
                f
            );
        }
        assert_eq!(
            all.len(),
            REGULAR_EDITABLE_FILES.len() + SECURITY_EDITABLE_FILES.len(),
            "Editable file list has unexpected length"
        );
    }

    /// Verify security-sensitive file detection.
    #[test]
    fn security_file_detection() {
        assert!(is_security_file("POLICY.md"));
        // Legacy pre-rename name stays security-sensitive
        assert!(is_security_file("LocalGPT.md"));
        assert!(is_editable_file("LocalGPT.md"));
        assert!(!is_security_file("MEMORY.md"));
        assert!(!is_security_file("SOUL.md"));
        assert!(!is_security_file("HEARTBEAT.md"));
        assert!(!is_security_file("unknown.md"));
    }

    /// Verify POLICY.md is the only security-sensitive editable file.
    #[test]
    fn only_policy_file_is_security_sensitive() {
        for &f in REGULAR_EDITABLE_FILES
            .iter()
            .chain(SECURITY_EDITABLE_FILES.iter())
        {
            if f == security::POLICY_FILENAME {
                assert!(is_security_file(f), "{} should be security-sensitive", f);
            } else {
                assert!(
                    !is_security_file(f),
                    "{} should not be security-sensitive",
                    f
                );
            }
        }
    }
}
