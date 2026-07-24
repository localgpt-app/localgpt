//! Cryptographic signing and verification for `LocalGPT.md`.
//!
//! Uses HMAC-SHA256 with a device-local 32-byte key to sign the user's
//! security policy file. The key is stored at `~/.local/share/localgpt/localgpt.device.key`
//! (0600 permissions on Unix) and is never exposed to the agent's tool
//! sandbox.
//!
//! # Key Management
//!
//! - **Generation**: [`ensure_device_key`] creates a 32-byte random key
//!   on first run. The key is not derived from anything — pure randomness
//!   from the OS CSPRNG via the `rand` crate.
//! - **Storage**: Outside the workspace directory, in the state directory.
//!   The agent's `read_file` and `memory_search` tools cannot reach it.
//! - **Permissions**: 0600 on Unix (owner read/write only). No enforcement
//!   on Windows — the file is still outside the workspace.
//!
//! # Signing Flow
//!
//! 1. Read `LocalGPT.md` content.
//! 2. Compute `content_sha256 = SHA-256(content)` (quick tamper check).
//! 3. Compute `hmac_sha256 = HMAC-SHA256(device_key, content)`.
//! 4. Write manifest to `.localgpt_manifest.json` in the workspace.

use anyhow::{Context, Result};
use hmac::{Hmac, KeyInit, Mac};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

type HmacSha256 = Hmac<Sha256>;

/// Filename of the signature manifest in the workspace directory.
pub const MANIFEST_FILENAME: &str = ".localgpt_manifest.json";

const DEVICE_KEY_FILENAME: &str = "localgpt.device.key";
const DEVICE_KEY_LEN: usize = 32;

/// Signature manifest for `LocalGPT.md`.
///
/// Stored as `.localgpt_manifest.json` in the workspace directory.
/// The `signed_by` field is always `"cli"` or `"gui"` — never `"agent"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    /// Schema version. Currently `1`.
    pub version: u8,
    /// HMAC-SHA256 of the policy content using the device key. Hex-encoded.
    pub hmac_sha256: String,
    /// ISO 8601 timestamp of when the policy was signed.
    pub signed_at: String,
    /// Who triggered the signing: `"cli"` or `"gui"`.
    pub signed_by: String,
    /// Plain SHA-256 of the content for quick tamper detection. Hex-encoded.
    pub content_sha256: String,
}

/// Ensure a device key exists in the state directory.
///
/// If the key file does not exist, generates 32 random bytes and writes
/// them with 0600 permissions (Unix). If the key already exists, this
/// is a no-op.
pub fn ensure_device_key(state_dir: &Path) -> Result<()> {
    let key_path = state_dir.join(DEVICE_KEY_FILENAME);
    if key_path.exists() {
        return Ok(());
    }

    // Generate 32 bytes from the OS CSPRNG
    let mut key = [0u8; DEVICE_KEY_LEN];
    rand::rng().fill(&mut key);

    fs::write(&key_path, key).context("Failed to write device key")?;

    // Set permissions to 0600 on Unix (skip on iOS/Android - sandbox doesn't allow)
    #[cfg(all(unix, not(target_os = "ios"), not(target_os = "android")))]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600));
    }

    tracing::info!("Generated device key at {}", key_path.display());
    Ok(())
}

/// Read the device key from the state directory.
///
/// Returns an error if the key file does not exist or has an unexpected
/// length.
pub fn read_device_key(state_dir: &Path) -> Result<[u8; DEVICE_KEY_LEN]> {
    let key_path = state_dir.join(DEVICE_KEY_FILENAME);
    let bytes = fs::read(&key_path).context("Failed to read device key. Run `localgpt init`.")?;

    if bytes.len() != DEVICE_KEY_LEN {
        anyhow::bail!(
            "Device key has unexpected length {} (expected {})",
            bytes.len(),
            DEVICE_KEY_LEN
        );
    }

    let mut key = [0u8; DEVICE_KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// Compute the hex-encoded SHA-256 digest of content.
pub fn content_sha256(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex_encode(&hasher.finalize())
}

/// Compute the hex-encoded HMAC-SHA256 of content using the device key.
pub fn compute_hmac(key: &[u8; DEVICE_KEY_LEN], content: &str) -> Result<String> {
    let mut mac =
        HmacSha256::new_from_slice(key).context("HMAC key initialization should not fail")?;
    mac.update(content.as_bytes());
    Ok(hex_encode(&mac.finalize().into_bytes()))
}

/// Sign the policy file and write the manifest to the workspace.
///
/// Reads `LocalGPT.md` from the workspace, computes SHA-256 and
/// HMAC-SHA256, and writes `.localgpt_manifest.json`.
///
/// # Arguments
///
/// * `state_dir` — Path to `~/.local/state/localgpt/` (contains `.device_key`).
/// * `workspace` — Path to `~/.local/share/localgpt/workspace/` (contains `LocalGPT.md`).
/// * `signed_by` — Who triggered the signing: `"cli"` or `"gui"`.
pub fn sign_policy(state_dir: &Path, workspace: &Path, signed_by: &str) -> Result<Manifest> {
    let policy_path = workspace.join(super::localgpt::POLICY_FILENAME);
    let content = fs::read_to_string(&policy_path)
        .with_context(|| format!("Failed to read {}", policy_path.display()))?;

    let key = read_device_key(state_dir)?;
    let sha256 = content_sha256(&content);
    let hmac = compute_hmac(&key, &content)?;
    let now = chrono::Utc::now().to_rfc3339();

    let manifest = Manifest {
        version: 1,
        hmac_sha256: hmac,
        signed_at: now,
        signed_by: signed_by.to_string(),
        content_sha256: sha256,
    };

    let manifest_path = workspace.join(MANIFEST_FILENAME);
    let json = serde_json::to_string_pretty(&manifest).context("Failed to serialize manifest")?;
    fs::write(&manifest_path, json).context("Failed to write manifest")?;

    Ok(manifest)
}

/// Verify that the policy file matches its signed manifest.
///
/// Returns `true` if both the SHA-256 and HMAC-SHA256 match.
/// Returns `false` on any mismatch (content modified after signing).
pub fn verify_signature(state_dir: &Path, workspace: &Path) -> Result<bool> {
    let policy_path = workspace.join(super::localgpt::POLICY_FILENAME);
    let content = fs::read_to_string(&policy_path)
        .with_context(|| format!("Failed to read {}", policy_path.display()))?;

    let manifest = read_manifest(workspace)?;
    let key = read_device_key(state_dir)?;

    // Quick check: content SHA-256
    let sha256 = content_sha256(&content);
    if sha256 != manifest.content_sha256 {
        return Ok(false);
    }

    // Full check: HMAC-SHA256
    let hmac = compute_hmac(&key, &content)?;
    Ok(hmac == manifest.hmac_sha256)
}

/// Read and parse the manifest file from the workspace.
pub fn read_manifest(workspace: &Path) -> Result<Manifest> {
    let manifest_path = workspace.join(MANIFEST_FILENAME);
    let json = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Failed to read {}", manifest_path.display()))?;
    serde_json::from_str(&json).context("Failed to parse manifest JSON")
}

/// Hex-encode a byte slice.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn device_key_generation() {
        let tmp = tempfile::tempdir().unwrap();
        ensure_device_key(tmp.path()).unwrap();

        let key_path = tmp.path().join(DEVICE_KEY_FILENAME);
        assert!(key_path.exists());

        let bytes = fs::read(&key_path).unwrap();
        assert_eq!(bytes.len(), DEVICE_KEY_LEN);
    }

    #[test]
    fn device_key_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        ensure_device_key(tmp.path()).unwrap();
        let key1 = read_device_key(tmp.path()).unwrap();

        // Second call should not overwrite
        ensure_device_key(tmp.path()).unwrap();
        let key2 = read_device_key(tmp.path()).unwrap();

        assert_eq!(key1, key2);
    }

    #[cfg(unix)]
    #[test]
    fn device_key_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        ensure_device_key(tmp.path()).unwrap();

        let key_path = tmp.path().join(DEVICE_KEY_FILENAME);
        let perms = fs::metadata(&key_path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[test]
    fn sign_and_verify_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let state_dir = tmp.path().join("state");
        let workspace = tmp.path().join("workspace");
        fs::create_dir_all(&state_dir).unwrap();
        fs::create_dir_all(&workspace).unwrap();

        // Create device key
        ensure_device_key(&state_dir).unwrap();

        // Create policy file
        let policy_content = "# Security Policy\n\n- Do not access /etc/passwd\n";
        fs::write(
            workspace.join(super::super::localgpt::POLICY_FILENAME),
            policy_content,
        )
        .unwrap();

        // Sign
        let manifest = sign_policy(&state_dir, &workspace, "cli").unwrap();
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.signed_by, "cli");
        assert!(!manifest.hmac_sha256.is_empty());
        assert!(!manifest.content_sha256.is_empty());

        // Verify
        assert!(verify_signature(&state_dir, &workspace).unwrap());
    }

    #[test]
    fn tampered_content_fails_verification() {
        let tmp = tempfile::tempdir().unwrap();
        let state_dir = tmp.path().join("state");
        let workspace = tmp.path().join("workspace");
        fs::create_dir_all(&state_dir).unwrap();
        fs::create_dir_all(&workspace).unwrap();

        ensure_device_key(&state_dir).unwrap();

        let policy_path = workspace.join(super::super::localgpt::POLICY_FILENAME);
        fs::write(&policy_path, "Original content").unwrap();

        sign_policy(&state_dir, &workspace, "cli").unwrap();

        // Tamper with the content
        fs::write(&policy_path, "Tampered content").unwrap();

        // Verification should fail
        assert!(!verify_signature(&state_dir, &workspace).unwrap());
    }

    #[test]
    fn content_sha256_deterministic() {
        let hash1 = content_sha256("hello world");
        let hash2 = content_sha256("hello world");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // 32 bytes = 64 hex chars
    }

    #[test]
    fn hmac_deterministic() {
        let key = [42u8; DEVICE_KEY_LEN];
        let hmac1 = compute_hmac(&key, "test data").unwrap();
        let hmac2 = compute_hmac(&key, "test data").unwrap();
        assert_eq!(hmac1, hmac2);
    }

    #[test]
    fn hmac_differs_for_different_content() {
        let key = [42u8; DEVICE_KEY_LEN];
        let hmac1 = compute_hmac(&key, "content A").unwrap();
        let hmac2 = compute_hmac(&key, "content B").unwrap();
        assert_ne!(hmac1, hmac2);
    }

    #[test]
    fn hmac_differs_for_different_keys() {
        let key1 = [1u8; DEVICE_KEY_LEN];
        let key2 = [2u8; DEVICE_KEY_LEN];
        let hmac1 = compute_hmac(&key1, "same content").unwrap();
        let hmac2 = compute_hmac(&key2, "same content").unwrap();
        assert_ne!(hmac1, hmac2);
    }
}
