use serde::{Deserialize, Serialize};

/// Current bridge protocol version.
/// Increment the minor version for backward-compatible additions,
/// and the major version for breaking changes.
pub const BRIDGE_PROTOCOL_VERSION: &str = "1.1";

#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum BridgeError {
    #[error("Bridge not registered")]
    NotRegistered,
    #[error("Authentication failed: {0}")]
    AuthFailed(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Unsupported protocol version: {0}")]
    UnsupportedVersion(String),
    #[error("Not supported: {0}")]
    NotSupported(String),
}

#[tarpc::service]
pub trait BridgeService {
    /// Returns the server's protocol version string (e.g. "1.1").
    /// Clients should call this first to detect version mismatches.
    async fn get_version() -> String;

    /// Liveness check. Returns `true` if the server is healthy.
    async fn ping() -> bool;

    /// Retrieve encrypted credentials for the given bridge ID.
    async fn get_credentials(bridge_id: String) -> Result<Vec<u8>, BridgeError>;

    // -- Agent session RPCs (added in 1.1) --

    /// Send a chat message to an agent session and return the full response.
    /// Creates a new session if one doesn't exist for `session_id`.
    async fn chat(session_id: String, message: String) -> Result<String, BridgeError>;

    /// Create or reset an agent session. Returns a confirmation message.
    async fn new_session(session_id: String) -> Result<String, BridgeError>;

    /// Get the status of an agent session (model, message count, token usage).
    async fn session_status(session_id: String) -> Result<String, BridgeError>;

    /// Set the model for an agent session.
    async fn set_model(session_id: String, model: String) -> Result<String, BridgeError>;

    /// Compact session history to reduce token usage.
    async fn compact_session(session_id: String) -> Result<String, BridgeError>;

    /// Clear session history.
    async fn clear_session(session_id: String) -> Result<String, BridgeError>;

    /// Search memory with a query. Returns formatted results.
    async fn memory_search(query: String, limit: u32) -> Result<String, BridgeError>;

    /// Get memory statistics.
    async fn memory_stats() -> Result<String, BridgeError>;
}
