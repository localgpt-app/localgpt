use serde::{Deserialize, Serialize};

/// Current bridge protocol version.
/// Increment the minor version for backward-compatible additions,
/// and the major version for breaking changes.
pub const BRIDGE_PROTOCOL_VERSION: &str = "1.0";

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
}

#[tarpc::service]
pub trait BridgeService {
    /// Returns the server's protocol version string (e.g. "1.0").
    /// Clients should call this first to detect version mismatches.
    async fn get_version() -> String;

    /// Liveness check. Returns `true` if the server is healthy.
    async fn ping() -> bool;

    /// Retrieve encrypted credentials for the given bridge ID.
    async fn get_credentials(bridge_id: String) -> Result<Vec<u8>, BridgeError>;
}
