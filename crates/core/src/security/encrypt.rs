//! Encryption at rest for sessions and config secrets.
//!
//! Uses XChaCha20-Poly1305 (AEAD) with a 32-byte random key stored on disk.
//! Key material is zeroized on drop to prevent leaking in memory.
//!
//! Wire format: `[nonce (24 bytes) || ciphertext || Poly1305 tag (16 bytes)]`
//! Config format: `enc:<base64>` prefix for encrypted string values.

use anyhow::{Context, Result, bail};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use std::path::Path;
use zeroize::Zeroizing;

/// Minimum ciphertext length: 24-byte nonce + 16-byte tag
const MIN_CIPHERTEXT_LEN: usize = 24 + 16;

/// Prefix for encrypted config values
pub const ENC_PREFIX: &str = "enc:";

/// Encryption key with automatic zeroization on drop.
pub struct EncryptionKey {
    raw: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptionKey")
            .field("raw", &"[REDACTED]")
            .finish()
    }
}

impl EncryptionKey {
    /// Generate a new random 32-byte key.
    pub fn generate() -> Self {
        let mut key = Zeroizing::new([0u8; 32]);
        rand::fill(key.as_mut());
        Self { raw: key }
    }

    /// Load key from a file on disk.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read encryption key: {}", path.display()))?;
        if bytes.len() != 32 {
            bail!(
                "Invalid encryption key: expected 32 bytes, got {}",
                bytes.len()
            );
        }
        let mut key = Zeroizing::new([0u8; 32]);
        key.copy_from_slice(&bytes);
        Ok(Self { raw: key })
    }

    /// Save key to disk with 0o600 permissions.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.raw.as_ref())?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// Encrypt plaintext. Returns `nonce || ciphertext || tag`.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = XChaCha20Poly1305::new(self.raw.as_ref().into());
        let mut nonce_bytes = [0u8; 24];
        rand::fill(&mut nonce_bytes);
        let nonce = *XNonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        let mut output = Vec::with_capacity(24 + ciphertext.len());
        output.extend_from_slice(&nonce);
        output.extend_from_slice(&ciphertext);
        Ok(output)
    }

    /// Decrypt data from `nonce || ciphertext || tag` format.
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < MIN_CIPHERTEXT_LEN {
            bail!(
                "Ciphertext too short: {} bytes (minimum {})",
                data.len(),
                MIN_CIPHERTEXT_LEN
            );
        }

        let (nonce_bytes, ciphertext) = data.split_at(24);
        let nonce = XNonce::from_slice(nonce_bytes);
        let cipher = XChaCha20Poly1305::new(self.raw.as_ref().into());

        cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| anyhow::anyhow!("Decryption failed: invalid key or tampered data"))
    }

    /// Encrypt a string and return `enc:<base64>` format for config files.
    pub fn encrypt_config_value(&self, plaintext: &str) -> Result<String> {
        let encrypted = self.encrypt(plaintext.as_bytes())?;
        Ok(format!(
            "{}{}",
            ENC_PREFIX,
            base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &encrypted)
        ))
    }

    /// Decrypt an `enc:<base64>` config value. Returns the plaintext string.
    /// If the value doesn't start with `enc:`, returns it unchanged.
    pub fn decrypt_config_value(&self, value: &str) -> Result<String> {
        if let Some(encoded) = value.strip_prefix(ENC_PREFIX) {
            let data = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)
                .context("Invalid base64 in encrypted config value")?;
            let plaintext = self.decrypt(&data)?;
            String::from_utf8(plaintext).context("Decrypted config value is not valid UTF-8")
        } else {
            Ok(value.to_string())
        }
    }
}

/// Check if an encryption key exists at the given path.
pub fn key_exists(path: &Path) -> bool {
    path.is_file()
}

/// Try to load an encryption key, returning None if the file doesn't exist.
pub fn try_load_key(path: &Path) -> Result<Option<EncryptionKey>> {
    if !key_exists(path) {
        return Ok(None);
    }
    Ok(Some(EncryptionKey::load(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_key_generation_is_32_bytes() {
        let key = EncryptionKey::generate();
        assert_eq!(key.raw.len(), 32);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = EncryptionKey::generate();
        let plaintext = b"Hello, LocalGPT! This is sensitive session data.";

        let encrypted = key.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext); // Must not be plaintext
        assert!(encrypted.len() > plaintext.len()); // nonce + tag overhead

        let decrypted = key.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_empty_data() {
        let key = EncryptionKey::generate();
        let encrypted = key.encrypt(b"").unwrap();
        let decrypted = key.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, b"");
    }

    #[test]
    fn test_tampered_ciphertext_fails() {
        let key = EncryptionKey::generate();
        let mut encrypted = key.encrypt(b"secret").unwrap();

        // Tamper with the ciphertext (flip a byte after the nonce)
        if encrypted.len() > 25 {
            encrypted[25] ^= 0xFF;
        }

        let result = key.decrypt(&encrypted);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("tampered"));
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = EncryptionKey::generate();
        let key2 = EncryptionKey::generate();

        let encrypted = key1.encrypt(b"secret").unwrap();
        let result = key2.decrypt(&encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_too_short_ciphertext_fails() {
        let key = EncryptionKey::generate();
        let result = key.decrypt(&[0u8; 10]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn test_config_value_encrypt_decrypt() {
        let key = EncryptionKey::generate();
        let secret = "sk-abc123-my-api-key";

        let encrypted = key.encrypt_config_value(secret).unwrap();
        assert!(encrypted.starts_with("enc:"));
        assert_ne!(encrypted, secret);

        let decrypted = key.decrypt_config_value(&encrypted).unwrap();
        assert_eq!(decrypted, secret);
    }

    #[test]
    fn test_config_value_passthrough_unencrypted() {
        let key = EncryptionKey::generate();
        let plain = "sk-plaintext-key";

        let result = key.decrypt_config_value(plain).unwrap();
        assert_eq!(result, plain); // Unchanged
    }

    #[test]
    fn test_save_and_load_key() {
        let tmp = TempDir::new().unwrap();
        let key_path = tmp.path().join("encryption.key");

        let key = EncryptionKey::generate();
        key.save(&key_path).unwrap();

        assert!(key_path.exists());

        // Verify permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&key_path).unwrap().permissions();
            assert_eq!(perms.mode() & 0o777, 0o600);
        }

        let loaded = EncryptionKey::load(&key_path).unwrap();

        // Roundtrip: encrypt with original, decrypt with loaded
        let encrypted = key.encrypt(b"test data").unwrap();
        let decrypted = loaded.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, b"test data");
    }

    #[test]
    fn test_key_exists_and_try_load() {
        let tmp = TempDir::new().unwrap();
        let key_path = tmp.path().join("encryption.key");

        assert!(!key_exists(&key_path));
        assert!(try_load_key(&key_path).unwrap().is_none());

        let key = EncryptionKey::generate();
        key.save(&key_path).unwrap();

        assert!(key_exists(&key_path));
        assert!(try_load_key(&key_path).unwrap().is_some());
    }

    #[test]
    fn test_invalid_key_file_wrong_size() {
        let tmp = TempDir::new().unwrap();
        let key_path = tmp.path().join("bad.key");
        std::fs::write(&key_path, [0u8; 16]).unwrap(); // Wrong size

        let result = EncryptionKey::load(&key_path);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("expected 32"));
    }

    #[test]
    fn test_each_encryption_produces_unique_ciphertext() {
        let key = EncryptionKey::generate();
        let plaintext = b"same input";

        let enc1 = key.encrypt(plaintext).unwrap();
        let enc2 = key.encrypt(plaintext).unwrap();

        // Different nonces → different ciphertexts
        assert_ne!(enc1, enc2);

        // But both decrypt to the same plaintext
        assert_eq!(key.decrypt(&enc1).unwrap(), plaintext);
        assert_eq!(key.decrypt(&enc2).unwrap(), plaintext);
    }
}
