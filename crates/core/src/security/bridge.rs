use anyhow::Result;
use chacha20poly1305::{
    ChaCha20Poly1305, Key, Nonce,
    aead::{Aead, KeyInit},
};
use hmac::{Hmac, Mac};
use localgpt_bridge::peer_identity::{PeerIdentity, get_peer_identity};
use localgpt_bridge::{BridgeError, BridgeServer, BridgeService};
use rand::RngExt;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use tarpc::context;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::paths::Paths;
use crate::security::signing::read_device_key;

/// Manages bridge processes and their credentials.
#[derive(Clone)]
pub struct BridgeManager {
    // In-memory cache of decrypted credentials
    credentials: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl BridgeManager {
    pub fn new() -> Self {
        Self {
            credentials: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new bridge secret.
    /// Encrypts and saves to disk, and updates cache.
    pub async fn register_bridge(&self, bridge_id: &str, secret: &[u8]) -> Result<()> {
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
                Ok(id) => id,
                Err(e) => {
                    error!("Peer identity verification failed: {}", e);
                    continue;
                }
            };

            info!("Accepted connection from: {:?}", identity);

            let handler = ConnectionHandler {
                manager: manager.clone(),
                identity,
            };

            tokio::spawn(async move {
                if let Err(e) = localgpt_bridge::handle_connection(conn, handler).await {
                    error!("Connection handling error: {:?}", e);
                }
            });
        }
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
}

impl BridgeService for ConnectionHandler {
    async fn get_version(self, _: context::Context) -> String {
        localgpt_bridge::BRIDGE_PROTOCOL_VERSION.to_string()
    }

    async fn ping(self, _: context::Context) -> bool {
        true
    }

    async fn get_credentials(
        self,
        _: context::Context,
        bridge_id: String,
    ) -> Result<Vec<u8>, BridgeError> {
        self.manager
            .get_credentials_for(&bridge_id, &self.identity)
            .await
    }
}
