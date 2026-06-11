use anyhow::Result;
use chacha20poly1305::{
    ChaCha20Poly1305, Key, Nonce,
    aead::{Aead, KeyInit},
};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use localgpt_bridge::peer_identity::{PeerIdentity, get_peer_identity};
use localgpt_bridge::{BridgeError, BridgeServer, BridgeService};
use rand::RngExt;
use serde::Serialize;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tarpc::context;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use localgpt_core::agent::{Agent, AgentConfig};
use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;
use localgpt_core::paths::Paths;
use localgpt_core::security::read_device_key;
use localgpt_core::text::single_line_prefix_with_ellipsis;

/// Agent ID used for bridge CLI sessions.
const BRIDGE_CLI_AGENT_ID: &str = "bridge-cli";

/// Health status of a bridge connection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Bridge is actively communicating
    Healthy,
    /// Bridge hasn't been seen recently (warning)
    Degraded,
    /// Bridge is unresponsive (critical)
    Unhealthy,
}

/// Status and health info for a connected bridge.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    pub connection_id: String,
    pub bridge_id: Option<String>,
    pub connected_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub pid: Option<i32>,
    pub uid: Option<u32>,
    /// Current health status based on last_active time
    pub health: HealthStatus,
    /// Number of consecutive health check failures
    pub consecutive_failures: u32,
}

/// Configuration for bridge health monitoring
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// How often to check bridge health (default: 30s)
    pub check_interval: Duration,
    /// Time without activity before marking as degraded (default: 60s)
    pub degraded_threshold: Duration,
    /// Time without activity before marking as unhealthy (default: 120s)
    pub unhealthy_threshold: Duration,
    /// Whether to log warnings for unhealthy bridges
    pub log_warnings: bool,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(30),
            degraded_threshold: Duration::from_secs(60),
            unhealthy_threshold: Duration::from_secs(120),
            log_warnings: true,
        }
    }
}

/// Shared agent session for bridge CLI connections.
struct AgentSession {
    agent: Agent,
}

/// Optional agent support for handling chat/memory RPCs.
struct AgentSupport {
    config: Config,
    memory: Arc<MemoryManager>,
    sessions: tokio::sync::Mutex<HashMap<String, AgentSession>>,
}

/// Manages bridge processes and their credentials.
#[derive(Clone)]
pub struct BridgeManager {
    // In-memory cache of decrypted credentials
    credentials: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    // Active connections: connection_id -> info
    active_bridges: Arc<RwLock<HashMap<String, BridgeStatus>>>,
    // Optional agent support for CLI bridge
    agent_support: Option<Arc<AgentSupport>>,
    // Health check configuration
    health_config: HealthCheckConfig,
}

impl BridgeManager {
    pub fn new() -> Self {
        Self {
            credentials: Arc::new(RwLock::new(HashMap::new())),
            active_bridges: Arc::new(RwLock::new(HashMap::new())),
            agent_support: None,
            health_config: HealthCheckConfig::default(),
        }
    }

    /// Create a BridgeManager with agent support for handling chat/memory RPCs.
    /// This is used by the daemon when serving bridge CLI connections.
    pub fn new_with_agent_support(config: Config, memory: MemoryManager) -> Self {
        Self {
            credentials: Arc::new(RwLock::new(HashMap::new())),
            active_bridges: Arc::new(RwLock::new(HashMap::new())),
            agent_support: Some(Arc::new(AgentSupport {
                config,
                memory: Arc::new(memory),
                sessions: tokio::sync::Mutex::new(HashMap::new()),
            })),
            health_config: HealthCheckConfig::default(),
        }
    }

    /// Create with custom health check configuration
    pub fn with_health_config(config: HealthCheckConfig) -> Self {
        Self {
            credentials: Arc::new(RwLock::new(HashMap::new())),
            active_bridges: Arc::new(RwLock::new(HashMap::new())),
            agent_support: None,
            health_config: config,
        }
    }

    /// Start the background health check task
    pub fn start_health_checker(&self) -> tokio::task::JoinHandle<()> {
        let manager = self.clone();
        let interval = self.health_config.check_interval;

        tokio::spawn(async move {
            let mut timer = tokio::time::interval(interval);
            loop {
                timer.tick().await;
                manager.check_bridge_health().await;
            }
        })
    }

    /// Check health of all bridges and update their status
    async fn check_bridge_health(&self) {
        let now = Utc::now();
        let config = &self.health_config;
        let mut bridges = self.active_bridges.write().await;

        for (_id, status) in bridges.iter_mut() {
            let elapsed = (now - status.last_active)
                .to_std()
                .unwrap_or(Duration::ZERO);

            let previous_health = status.health;
            let previous_failures = status.consecutive_failures;

            // Determine health based on elapsed time since last activity
            if elapsed > config.unhealthy_threshold {
                status.health = HealthStatus::Unhealthy;
                status.consecutive_failures += 1;
            } else if elapsed > config.degraded_threshold {
                status.health = HealthStatus::Degraded;
                status.consecutive_failures += 1;
            } else {
                status.health = HealthStatus::Healthy;
                status.consecutive_failures = 0;
            }

            // Log warnings on state changes or continued unhealthy state
            if config.log_warnings {
                if status.health != previous_health {
                    match status.health {
                        HealthStatus::Degraded => {
                            warn!(
                                "Bridge {} (connection {}) is degraded - no activity for {:?}",
                                status.bridge_id.as_deref().unwrap_or("unknown"),
                                status.connection_id,
                                elapsed
                            );
                        }
                        HealthStatus::Unhealthy => {
                            error!(
                                "Bridge {} (connection {}) is unhealthy - no activity for {:?}",
                                status.bridge_id.as_deref().unwrap_or("unknown"),
                                status.connection_id,
                                elapsed
                            );
                        }
                        HealthStatus::Healthy => {
                            info!(
                                "Bridge {} (connection {}) is now healthy",
                                status.bridge_id.as_deref().unwrap_or("unknown"),
                                status.connection_id
                            );
                        }
                    }
                } else if status.health == HealthStatus::Unhealthy
                    && status.consecutive_failures > previous_failures
                    && status.consecutive_failures % 3 == 0
                {
                    // Log every 3rd consecutive failure
                    error!(
                        "Bridge {} (connection {}) still unhealthy (failures: {})",
                        status.bridge_id.as_deref().unwrap_or("unknown"),
                        status.connection_id,
                        status.consecutive_failures
                    );
                }
            }
        }
    }

    /// Return status of all active bridge connections.
    pub async fn get_active_bridges(&self) -> Vec<BridgeStatus> {
        self.active_bridges.read().await.values().cloned().collect()
    }

    async fn add_connection(&self, id: &str, identity: &PeerIdentity) {
        let status = BridgeStatus {
            connection_id: id.to_string(),
            bridge_id: None,
            connected_at: Utc::now(),
            last_active: Utc::now(),
            pid: identity.pid,
            uid: identity.uid,
            health: HealthStatus::Healthy,
            consecutive_failures: 0,
        };
        self.active_bridges
            .write()
            .await
            .insert(id.to_string(), status);
    }

    async fn update_active(&self, id: &str, bridge_id: Option<String>) {
        let mut active = self.active_bridges.write().await;
        if let Some(status) = active.get_mut(id) {
            status.last_active = Utc::now();
            status.health = HealthStatus::Healthy;
            status.consecutive_failures = 0;
            if bridge_id.is_some() {
                status.bridge_id = bridge_id;
            }
        }
    }

    async fn remove_connection(&self, id: &str) {
        self.active_bridges.write().await.remove(id);
    }

    /// Register a new bridge secret.
    /// Encrypts and saves to disk, and updates cache.
    pub async fn register_bridge(&self, bridge_id: &str, secret: &[u8]) -> Result<()> {
        validate_bridge_id(bridge_id)?;

        let paths = Paths::resolve()?;
        let bridges_dir = paths.data_dir.join("bridges");
        std::fs::create_dir_all(&bridges_dir)?;

        // 1. Get Master Key
        let master_key = read_device_key(&paths.data_dir)?;

        // 2. Derive Bridge Key = HMAC-SHA256(MasterKey, "bridge-key:" + bridge_id)
        let bridge_key = derive_bridge_key(&master_key, bridge_id)?;

        // 3. Encrypt Secret
        let cipher = ChaCha20Poly1305::new(&bridge_key);

        // Generate nonce manually to avoid rand_core version mismatch
        let mut nonce_bytes = [0u8; 12];
        rand::rng().fill(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, secret)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        // 4. Save to file: [Nonce (12 bytes)][Ciphertext]
        let mut file_content = nonce_bytes.to_vec();
        file_content.extend(ciphertext);

        let file_path = bridges_dir.join(format!("{}.enc", bridge_id));
        std::fs::write(&file_path, file_content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file_path, std::fs::Permissions::from_mode(0o600))?;
        }

        // 5. Update Cache
        let mut creds = self.credentials.write().await;
        creds.insert(bridge_id.to_string(), secret.to_vec());

        info!("Registered credentials for bridge: {}", bridge_id);
        Ok(())
    }

    /// Retrieve credentials if the identity is authorized.
    /// Loads from disk if not in cache.
    pub async fn get_credentials_for(
        &self,
        bridge_id: &str,
        identity: &PeerIdentity,
    ) -> Result<Vec<u8>, BridgeError> {
        if let Err(e) = validate_bridge_id(bridge_id) {
            error!("Invalid bridge ID: {}", e);
            return Err(BridgeError::AuthFailed("Invalid bridge ID".to_string()));
        }

        // Verify identity (Basic check for now)
        // TODO: Implement stricter checks based on OS user or code signature
        info!(
            "Checking access for bridge: {} from {:?}",
            bridge_id, identity
        );

        // Check cache first
        {
            let creds = self.credentials.read().await;
            if let Some(secret) = creds.get(bridge_id) {
                return Ok(secret.clone());
            }
        }

        // Load from disk
        match self.load_credentials_from_disk(bridge_id).await {
            Ok(secret) => {
                // Cache it
                let mut creds = self.credentials.write().await;
                creds.insert(bridge_id.to_string(), secret.clone());
                Ok(secret)
            }
            Err(e) => {
                error!("Failed to load credentials for {}: {}", bridge_id, e);
                Err(BridgeError::NotRegistered)
            }
        }
    }

    async fn load_credentials_from_disk(&self, bridge_id: &str) -> Result<Vec<u8>> {
        let paths = Paths::resolve()?;
        let file_path = paths
            .data_dir
            .join("bridges")
            .join(format!("{}.enc", bridge_id));

        if !file_path.exists() {
            anyhow::bail!("Credential file not found");
        }

        let file_content = std::fs::read(&file_path)?;
        if file_content.len() < 12 {
            anyhow::bail!("Invalid credential file format (too short)");
        }

        let (nonce_bytes, ciphertext) = file_content.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        // Derive Key
        let master_key = read_device_key(&paths.data_dir)?;
        let bridge_key = derive_bridge_key(&master_key, bridge_id)?;

        // Decrypt
        let cipher = ChaCha20Poly1305::new(&bridge_key);
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

        Ok(plaintext)
    }

    /// Start the bridge server listening on the given socket path.
    pub async fn serve(self, socket_path: &str) -> anyhow::Result<()> {
        let listener = BridgeServer::bind(socket_path)?;
        let manager = self.clone();

        info!("BridgeManager listening on {}", socket_path);

        loop {
            let conn = match listener.accept().await {
                Ok(c) => c,
                Err(e) => {
                    error!("Accept failed: {}", e);
                    continue;
                }
            };

            // Verify peer identity
            let identity_result = {
                #[cfg(unix)]
                {
                    get_peer_identity(&conn)
                }
                #[cfg(windows)]
                {
                    get_peer_identity(&conn)
                }
            };

            let identity = match identity_result {
                Ok(id) => {
                    // Enforce UID matching (same-user only)
                    #[cfg(unix)]
                    {
                        // SAFETY: getuid() is always safe to call — it has no arguments,
                        // requires no preconditions, and simply returns the real user ID.
                        let current_uid = unsafe { libc::getuid() };
                        if let Some(peer_uid) = id.uid.filter(|&uid| uid != current_uid) {
                            error!(
                                "Rejected connection from UID {} (expected {})",
                                peer_uid, current_uid
                            );
                            continue;
                        }
                    }
                    id
                }
                Err(e) => {
                    error!("Peer identity verification failed: {}", e);
                    continue;
                }
            };

            info!("Accepted connection from: {:?}", identity);

            let connection_id = Uuid::new_v4().to_string();
            manager.add_connection(&connection_id, &identity).await;

            let handler = ConnectionHandler {
                manager: manager.clone(),
                identity,
                connection_id: connection_id.clone(),
            };

            let connection_manager = manager.clone();
            tokio::spawn(async move {
                if let Err(e) = localgpt_bridge::handle_connection(conn, handler).await {
                    debug!("Connection handling finished/error: {:?}", e);
                }
                connection_manager.remove_connection(&connection_id).await;
            });
        }
    }
}

impl Default for BridgeManager {
    fn default() -> Self {
        Self::new()
    }
}

fn derive_bridge_key(master_key: &[u8; 32], bridge_id: &str) -> Result<Key> {
    type HmacSha256 = Hmac<Sha256>;
    // Disambiguate Mac vs KeyInit
    let mut mac = <HmacSha256 as Mac>::new_from_slice(master_key)
        .map_err(|e| anyhow::anyhow!("HMAC init failed: {}", e))?;

    mac.update(b"bridge-key:");
    mac.update(bridge_id.as_bytes());

    let result = mac.finalize().into_bytes();
    // ChaCha20Poly1305 key is 32 bytes, which matches SHA256 output size.
    Ok(*Key::from_slice(&result))
}

/// Per-connection handler that implements the BridgeService trait.
#[derive(Clone)]
struct ConnectionHandler {
    manager: BridgeManager,
    identity: PeerIdentity,
    connection_id: String,
}

impl BridgeService for ConnectionHandler {
    async fn get_version(self, _: context::Context) -> String {
        self.manager.update_active(&self.connection_id, None).await;
        localgpt_bridge::BRIDGE_PROTOCOL_VERSION.to_string()
    }

    async fn ping(self, _: context::Context) -> bool {
        self.manager.update_active(&self.connection_id, None).await;
        true
    }

    async fn get_credentials(
        self,
        _: context::Context,
        bridge_id: String,
    ) -> Result<Vec<u8>, BridgeError> {
        self.manager
            .update_active(&self.connection_id, Some(bridge_id.clone()))
            .await;
        self.manager
            .get_credentials_for(&bridge_id, &self.identity)
            .await
    }

    async fn chat(
        self,
        _: context::Context,
        session_id: String,
        message: String,
    ) -> Result<String, BridgeError> {
        self.manager.update_active(&self.connection_id, None).await;
        let support = self
            .manager
            .agent_support
            .as_ref()
            .ok_or_else(|| BridgeError::NotSupported("Agent support not available".into()))?;

        let mut sessions = support.sessions.lock().await;

        // Create session if it doesn't exist, using entry API to avoid unwrap
        if let std::collections::hash_map::Entry::Vacant(entry) = sessions.entry(session_id.clone())
        {
            let agent_config = AgentConfig {
                model: support.config.agent.default_model.clone(),
                context_window: support.config.agent.context_window,
                reserve_tokens: support.config.agent.reserve_tokens,
            };
            let mut agent = Agent::new(agent_config, &support.config, Arc::clone(&support.memory))
                .await
                .map_err(|e| BridgeError::Internal(format!("Failed to create agent: {}", e)))?;
            agent
                .new_session()
                .await
                .map_err(|e| BridgeError::Internal(format!("Failed to init session: {}", e)))?;
            entry.insert(AgentSession { agent });
        }

        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| BridgeError::Internal("Session unexpectedly missing".into()))?;
        let response = session
            .agent
            .chat(&message)
            .await
            .map_err(|e| BridgeError::Internal(format!("Chat error: {}", e)))?;

        if let Err(e) = session
            .agent
            .save_session_for_agent(BRIDGE_CLI_AGENT_ID)
            .await
        {
            warn!("Failed to save bridge-cli session: {}", e);
        }

        Ok(response)
    }

    async fn new_session(
        self,
        _: context::Context,
        session_id: String,
    ) -> Result<String, BridgeError> {
        self.manager.update_active(&self.connection_id, None).await;
        let support = self
            .manager
            .agent_support
            .as_ref()
            .ok_or_else(|| BridgeError::NotSupported("Agent support not available".into()))?;

        let mut sessions = support.sessions.lock().await;

        let agent_config = AgentConfig {
            model: support.config.agent.default_model.clone(),
            context_window: support.config.agent.context_window,
            reserve_tokens: support.config.agent.reserve_tokens,
        };
        let mut agent = Agent::new(agent_config, &support.config, Arc::clone(&support.memory))
            .await
            .map_err(|e| BridgeError::Internal(format!("Failed to create agent: {}", e)))?;
        agent
            .new_session()
            .await
            .map_err(|e| BridgeError::Internal(format!("Failed to init session: {}", e)))?;

        let model = agent.model().to_string();
        let chunks = agent.memory_chunk_count();
        sessions.insert(session_id, AgentSession { agent });

        Ok(format!(
            "New session created. Model: {} | Memory: {} chunks",
            model, chunks
        ))
    }

    async fn session_status(
        self,
        _: context::Context,
        session_id: String,
    ) -> Result<String, BridgeError> {
        self.manager.update_active(&self.connection_id, None).await;
        let support = self
            .manager
            .agent_support
            .as_ref()
            .ok_or_else(|| BridgeError::NotSupported("Agent support not available".into()))?;

        let sessions = support.sessions.lock().await;
        let session = sessions
            .get(&session_id)
            .ok_or_else(|| BridgeError::Internal("No active session".into()))?;

        let status = session.agent.session_status();
        let mut output = String::new();
        output.push_str(&format!("Session ID: {}\n", status.id));
        output.push_str(&format!("Model: {}\n", session.agent.model()));
        output.push_str(&format!("Messages: {}\n", status.message_count));
        output.push_str(&format!("Context tokens: ~{}\n", status.token_count));
        output.push_str(&format!("Compactions: {}\n", status.compaction_count));
        output.push_str(&format!(
            "Memory chunks: {}",
            session.agent.memory_chunk_count()
        ));

        if status.api_input_tokens > 0 || status.api_output_tokens > 0 {
            output.push_str(&format!(
                "\nAPI tokens: {} in / {} out",
                status.api_input_tokens, status.api_output_tokens
            ));
        }

        Ok(output)
    }

    async fn set_model(
        self,
        _: context::Context,
        session_id: String,
        model: String,
    ) -> Result<String, BridgeError> {
        self.manager.update_active(&self.connection_id, None).await;
        let support = self
            .manager
            .agent_support
            .as_ref()
            .ok_or_else(|| BridgeError::NotSupported("Agent support not available".into()))?;

        let mut sessions = support.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| BridgeError::Internal("No active session".into()))?;

        session
            .agent
            .set_model(&model)
            .map_err(|e| BridgeError::Internal(format!("Failed to set model: {}", e)))?;

        Ok(format!("Switched to model: {}", model))
    }

    async fn compact_session(
        self,
        _: context::Context,
        session_id: String,
    ) -> Result<String, BridgeError> {
        self.manager.update_active(&self.connection_id, None).await;
        let support = self
            .manager
            .agent_support
            .as_ref()
            .ok_or_else(|| BridgeError::NotSupported("Agent support not available".into()))?;

        let mut sessions = support.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| BridgeError::Internal("No active session".into()))?;

        let (before, after) = session
            .agent
            .compact_session()
            .await
            .map_err(|e| BridgeError::Internal(format!("Failed to compact: {}", e)))?;

        Ok(format!(
            "Session compacted. Token count: {} → {}",
            before, after
        ))
    }

    async fn clear_session(
        self,
        _: context::Context,
        session_id: String,
    ) -> Result<String, BridgeError> {
        self.manager.update_active(&self.connection_id, None).await;
        let support = self
            .manager
            .agent_support
            .as_ref()
            .ok_or_else(|| BridgeError::NotSupported("Agent support not available".into()))?;

        let mut sessions = support.sessions.lock().await;
        let session = sessions
            .get_mut(&session_id)
            .ok_or_else(|| BridgeError::Internal("No active session".into()))?;

        session.agent.clear_session();
        Ok("Session cleared.".into())
    }

    async fn memory_search(
        self,
        _: context::Context,
        query: String,
        limit: u32,
    ) -> Result<String, BridgeError> {
        self.manager.update_active(&self.connection_id, None).await;
        let support = self
            .manager
            .agent_support
            .as_ref()
            .ok_or_else(|| BridgeError::NotSupported("Agent support not available".into()))?;

        let results = support
            .memory
            .search(&query, limit as usize)
            .map_err(|e| BridgeError::Internal(format!("Memory search failed: {}", e)))?;

        if results.is_empty() {
            return Ok(format!("No results found for '{}'", query));
        }

        let mut output = format!("Found {} results for '{}':\n", results.len(), query);
        for (i, result) in results.iter().enumerate() {
            output.push_str(&format!(
                "\n{}. {} (lines {}-{})\n",
                i + 1,
                result.file,
                result.line_start,
                result.line_end
            ));
            output.push_str(&format!("   Score: {:.3}\n", result.score));
            output.push_str(&format!("   {}\n", memory_result_preview(&result.content)));
        }

        Ok(output)
    }

    async fn memory_stats(self, _: context::Context) -> Result<String, BridgeError> {
        self.manager.update_active(&self.connection_id, None).await;
        let support = self
            .manager
            .agent_support
            .as_ref()
            .ok_or_else(|| BridgeError::NotSupported("Agent support not available".into()))?;

        let stats = support
            .memory
            .stats()
            .map_err(|e| BridgeError::Internal(format!("Failed to get stats: {}", e)))?;

        let mut output = String::new();
        output.push_str("Memory Statistics\n");
        output.push_str("-----------------\n");
        output.push_str(&format!("Workspace: {}\n", stats.workspace));
        output.push_str(&format!("Total files: {}\n", stats.total_files));
        output.push_str(&format!("Total chunks: {}\n", stats.total_chunks));
        output.push_str(&format!("Index size: {} KB\n", stats.index_size_kb));
        output.push_str("\nFiles:\n");
        for file in &stats.files {
            output.push_str(&format!(
                "  {} ({} chunks, {} lines)\n",
                file.name, file.chunks, file.lines
            ));
        }

        Ok(output)
    }
}

fn validate_bridge_id(id: &str) -> Result<()> {
    if id.is_empty() {
        anyhow::bail!("Bridge ID cannot be empty");
    }
    if id.len() > 64 {
        anyhow::bail!("Bridge ID is too long (max 64 chars)");
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!("Bridge ID contains invalid characters. Allowed: a-z, A-Z, 0-9, -, _");
    }
    Ok(())
}

fn memory_result_preview(content: &str) -> String {
    single_line_prefix_with_ellipsis(content, 200)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_status_serialization() {
        let healthy = HealthStatus::Healthy;
        assert_eq!(serde_json::to_string(&healthy).unwrap(), "\"healthy\"");

        let degraded = HealthStatus::Degraded;
        assert_eq!(serde_json::to_string(&degraded).unwrap(), "\"degraded\"");

        let unhealthy = HealthStatus::Unhealthy;
        assert_eq!(serde_json::to_string(&unhealthy).unwrap(), "\"unhealthy\"");
    }

    #[test]
    fn test_health_check_config_default() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.check_interval, Duration::from_secs(30));
        assert_eq!(config.degraded_threshold, Duration::from_secs(60));
        assert_eq!(config.unhealthy_threshold, Duration::from_secs(120));
        assert!(config.log_warnings);
    }

    #[test]
    fn memory_result_preview_preserves_short_multibyte_text() {
        assert_eq!(memory_result_preview(&"✅".repeat(100)), "✅".repeat(100));
    }

    #[test]
    fn memory_result_preview_truncates_by_characters() {
        let preview = memory_result_preview(&"✅".repeat(201));

        assert_eq!(preview.chars().filter(|&c| c == '✅').count(), 200);
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn memory_result_preview_flattens_newlines() {
        assert_eq!(memory_result_preview("one\ntwo"), "one two");
    }

    #[tokio::test]
    async fn test_bridge_status_initial_health() {
        let manager = BridgeManager::new();
        let identity = PeerIdentity {
            pid: Some(1234),
            uid: Some(1000),
            gid: Some(1000),
        };

        manager.add_connection("test-conn", &identity).await;

        let bridges = manager.get_active_bridges().await;
        assert_eq!(bridges.len(), 1);
        assert_eq!(bridges[0].health, HealthStatus::Healthy);
        assert_eq!(bridges[0].consecutive_failures, 0);
    }

    #[tokio::test]
    async fn test_update_active_resets_health() {
        let manager = BridgeManager::new();
        let identity = PeerIdentity {
            pid: Some(1234),
            uid: Some(1000),
            gid: Some(1000),
        };

        manager.add_connection("test-conn", &identity).await;

        // Simulate bridge going unhealthy
        {
            let mut bridges = manager.active_bridges.write().await;
            let status = bridges.get_mut("test-conn").unwrap();
            status.health = HealthStatus::Unhealthy;
            status.consecutive_failures = 5;
        }

        // Update active should reset health
        manager
            .update_active("test-conn", Some("telegram".to_string()))
            .await;

        let bridges = manager.get_active_bridges().await;
        assert_eq!(bridges[0].health, HealthStatus::Healthy);
        assert_eq!(bridges[0].consecutive_failures, 0);
        assert_eq!(bridges[0].bridge_id, Some("telegram".to_string()));
    }

    #[tokio::test]
    async fn test_health_check_degraded() {
        let config = HealthCheckConfig {
            check_interval: Duration::from_secs(30),
            degraded_threshold: Duration::from_secs(5),
            unhealthy_threshold: Duration::from_secs(10),
            log_warnings: false,
        };
        let manager = BridgeManager::with_health_config(config);
        let identity = PeerIdentity {
            pid: Some(1234),
            uid: Some(1000),
            gid: Some(1000),
        };

        manager.add_connection("test-conn", &identity).await;

        // Simulate time passing by setting last_active to the past
        {
            let mut bridges = manager.active_bridges.write().await;
            let status = bridges.get_mut("test-conn").unwrap();
            // Set last_active to 7 seconds ago (past degraded threshold of 5s)
            status.last_active = Utc::now() - chrono::Duration::seconds(7);
        }

        // Run health check
        manager.check_bridge_health().await;

        let bridges = manager.get_active_bridges().await;
        assert_eq!(bridges[0].health, HealthStatus::Degraded);
        assert_eq!(bridges[0].consecutive_failures, 1);
    }

    #[tokio::test]
    async fn test_health_check_unhealthy() {
        let config = HealthCheckConfig {
            check_interval: Duration::from_secs(30),
            degraded_threshold: Duration::from_secs(5),
            unhealthy_threshold: Duration::from_secs(10),
            log_warnings: false,
        };
        let manager = BridgeManager::with_health_config(config);
        let identity = PeerIdentity {
            pid: Some(1234),
            uid: Some(1000),
            gid: Some(1000),
        };

        manager.add_connection("test-conn", &identity).await;

        // Simulate time passing by setting last_active to the past
        {
            let mut bridges = manager.active_bridges.write().await;
            let status = bridges.get_mut("test-conn").unwrap();
            // Set last_active to 15 seconds ago (past unhealthy threshold of 10s)
            status.last_active = Utc::now() - chrono::Duration::seconds(15);
        }

        // Run health check
        manager.check_bridge_health().await;

        let bridges = manager.get_active_bridges().await;
        assert_eq!(bridges[0].health, HealthStatus::Unhealthy);
        assert_eq!(bridges[0].consecutive_failures, 1);
    }

    #[tokio::test]
    async fn test_health_check_consecutive_failures() {
        let config = HealthCheckConfig {
            check_interval: Duration::from_secs(30),
            degraded_threshold: Duration::from_secs(5),
            unhealthy_threshold: Duration::from_secs(10),
            log_warnings: false,
        };
        let manager = BridgeManager::with_health_config(config);
        let identity = PeerIdentity {
            pid: Some(1234),
            uid: Some(1000),
            gid: Some(1000),
        };

        manager.add_connection("test-conn", &identity).await;

        // Simulate bridge that stays unhealthy
        {
            let mut bridges = manager.active_bridges.write().await;
            let status = bridges.get_mut("test-conn").unwrap();
            status.last_active = Utc::now() - chrono::Duration::seconds(15);
        }

        // Run health check 3 times
        manager.check_bridge_health().await;
        manager.check_bridge_health().await;
        manager.check_bridge_health().await;

        let bridges = manager.get_active_bridges().await;
        assert_eq!(bridges[0].consecutive_failures, 3);
    }

    #[tokio::test]
    async fn test_health_check_healthy_resets_failures() {
        let config = HealthCheckConfig {
            check_interval: Duration::from_secs(30),
            degraded_threshold: Duration::from_secs(5),
            unhealthy_threshold: Duration::from_secs(10),
            log_warnings: false,
        };
        let manager = BridgeManager::with_health_config(config);
        let identity = PeerIdentity {
            pid: Some(1234),
            uid: Some(1000),
            gid: Some(1000),
        };

        manager.add_connection("test-conn", &identity).await;

        // Start with some failures
        {
            let mut bridges = manager.active_bridges.write().await;
            let status = bridges.get_mut("test-conn").unwrap();
            status.consecutive_failures = 5;
            status.health = HealthStatus::Unhealthy;
        }

        // Bridge becomes active again (last_active is now)
        // Run health check - should reset to healthy
        manager.check_bridge_health().await;

        let bridges = manager.get_active_bridges().await;
        assert_eq!(bridges[0].health, HealthStatus::Healthy);
        assert_eq!(bridges[0].consecutive_failures, 0);
    }

    #[tokio::test]
    async fn test_remove_connection() {
        let manager = BridgeManager::new();
        let identity = PeerIdentity {
            pid: Some(1234),
            uid: Some(1000),
            gid: Some(1000),
        };

        manager.add_connection("test-conn", &identity).await;
        assert_eq!(manager.get_active_bridges().await.len(), 1);

        manager.remove_connection("test-conn").await;
        assert_eq!(manager.get_active_bridges().await.len(), 0);
    }

    #[test]
    fn test_validate_bridge_id() {
        assert!(validate_bridge_id("telegram").is_ok());
        assert!(validate_bridge_id("discord-bot").is_ok());
        assert!(validate_bridge_id("whatsapp_2").is_ok());
        assert!(validate_bridge_id("bridge123").is_ok());

        assert!(validate_bridge_id("").is_err());
        assert!(validate_bridge_id(&"x".repeat(65)).is_err());
        assert!(validate_bridge_id("bridge!@#").is_err());
        assert!(validate_bridge_id("bridge name").is_err());
    }
}
