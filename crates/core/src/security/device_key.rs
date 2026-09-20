//! Device-local key for encrypting bridge credentials.
//!
//! A 32-byte random key stored at `~/.local/share/localgpt/localgpt.device.key`
//! (0600 permissions on Unix), outside the workspace so the agent's tool
//! sandbox cannot reach it. The key is pure randomness from the OS CSPRNG
//! via the `rand` crate — it is not derived from anything.
//!
//! The bridge server (`localgpt-server`) derives per-bridge ChaCha20Poly1305
//! keys from this master key to encrypt credentials at rest.

use anyhow::{Context, Result};
use rand::RngExt;
use std::fs;
use std::path::Path;

const DEVICE_KEY_FILENAME: &str = "localgpt.device.key";
const DEVICE_KEY_LEN: usize = 32;

/// Ensure a device key exists in the data directory.
///
/// If the key file does not exist, generates 32 random bytes and writes
/// them with 0600 permissions (Unix). If the key already exists, this
/// is a no-op.
pub fn ensure_device_key(data_dir: &Path) -> Result<()> {
    let key_path = data_dir.join(DEVICE_KEY_FILENAME);
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

/// Read the device key from the data directory.
///
/// Returns an error if the key file does not exist or has an unexpected
/// length.
pub fn read_device_key(data_dir: &Path) -> Result<[u8; DEVICE_KEY_LEN]> {
    let key_path = data_dir.join(DEVICE_KEY_FILENAME);
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
}
