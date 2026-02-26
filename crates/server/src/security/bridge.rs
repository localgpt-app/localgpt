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
use tarpc::context;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use localgpt_core::agent::{Agent, AgentConfig};
use localgpt_core::config::Config;
use localgpt_core::memory::MemoryManager;
use localgpt_core::paths::Paths;
use localgpt_core::security::read_device_key;

/// Agent ID used for bridge CLI sessions.
const BRIDGE_CLI_AGENT_ID: &str = "bridge-cli";

/// Status and health info for a connected bridge.
#[derive(Debug, Clone, Serialize)]
pub struct BridgeStatus {
    pub connection_id: String,
    pub bridge_id: Option<String>,
    pub connected_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub pid: Option<i32>,
    pub uid: Option<u32>,
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
}

impl BridgeManager {
    pub fn new() -> Self {
        Self {
            credentials: Arc::new(RwLock::new(HashMap::new())),
            active_bridges: Arc::new(RwLock::new(HashMap::new())),
            agent_support: None,
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
            let preview: String = result.content.chars().take(200).collect();
            let preview = preview.replace('\n', " ");
            output.push_str(&format!(
                "   {}{}\n",
                preview,
                if result.content.len() > 200 {
                    "..."
                } else {
                    ""
                }
            ));
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
