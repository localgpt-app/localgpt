use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use std::path::{Path, PathBuf};

use localgpt_core::paths::Paths;
use localgpt_core::security::encrypt::{self, ENC_PREFIX, EncryptionKey};

#[derive(Args)]
pub struct EncryptArgs {
    #[command(subcommand)]
    pub command: EncryptCommand,
}

#[derive(Subcommand)]
pub enum EncryptCommand {
    /// Generate encryption key and encrypt existing sessions and config secrets
    Enable,
    /// Decrypt all data and remove the encryption key
    Disable,
    /// Show encryption status
    Status,
    /// Generate new key and re-encrypt all data
    Rotate,
}

pub async fn run(args: EncryptArgs) -> Result<()> {
    let paths = Paths::resolve()?;

    match args.command {
        EncryptCommand::Enable => cmd_enable(&paths),
        EncryptCommand::Disable => cmd_disable(&paths),
        EncryptCommand::Status => cmd_status(&paths),
        EncryptCommand::Rotate => cmd_rotate(&paths),
    }
}

fn key_path(paths: &Paths) -> PathBuf {
    paths.data_dir.join("encryption.key")
}

// ── enable ──

fn cmd_enable(paths: &Paths) -> Result<()> {
    let kp = key_path(paths);
    if encrypt::key_exists(&kp) {
        bail!(
            "Encryption is already enabled (key exists at {}). Use `localgpt encrypt rotate` to change keys.",
            kp.display()
        );
    }

    let key = EncryptionKey::generate();
    key.save(&kp)?;
    println!("Generated encryption key: {}", kp.display());

    let sessions = encrypt_sessions(paths, &key)?;
    let secrets = encrypt_config_secrets(paths, &key)?;

    println!(
        "Encryption enabled: {} sessions encrypted, {} config secrets wrapped",
        sessions, secrets
    );
    Ok(())
}

// ── disable ──

fn cmd_disable(paths: &Paths) -> Result<()> {
    let kp = key_path(paths);
    let key = EncryptionKey::load(&kp)
        .map_err(|_| anyhow::anyhow!("Encryption is not enabled (no key at {})", kp.display()))?;

    let sessions = decrypt_sessions(paths, &key)?;
    let secrets = decrypt_config_secrets(paths, &key)?;

    std::fs::remove_file(&kp)?;
    println!(
        "Encryption disabled: {} sessions decrypted, {} config secrets unwrapped, key removed",
        sessions, secrets
    );
    Ok(())
}

// ── status ──

fn cmd_status(paths: &Paths) -> Result<()> {
    let kp = key_path(paths);
    let enabled = encrypt::key_exists(&kp);

    println!(
        "Encryption: {}",
        if enabled { "enabled" } else { "disabled" }
    );
    if enabled {
        println!("Key: {}", kp.display());
    }

    let (plain_sessions, enc_sessions) = count_session_files(paths);
    println!(
        "Sessions: {} encrypted, {} plaintext",
        enc_sessions, plain_sessions
    );

    let enc_secrets = count_config_secrets(paths);
    println!("Config secrets: {} encrypted", enc_secrets);

    Ok(())
}

// ── rotate ──

fn cmd_rotate(paths: &Paths) -> Result<()> {
    let kp = key_path(paths);
    let old_key = EncryptionKey::load(&kp).map_err(|_| {
        anyhow::anyhow!(
            "Cannot rotate: encryption is not enabled (no key at {})",
            kp.display()
        )
    })?;

    let new_key = EncryptionKey::generate();

    // Re-encrypt sessions: decrypt with old → encrypt with new
    let sessions = reencrypt_sessions(paths, &old_key, &new_key)?;

    // Re-encrypt config secrets
    let secrets = reencrypt_config_secrets(paths, &old_key, &new_key)?;

    // Save new key (overwrites old)
    new_key.save(&kp)?;

    println!(
        "Key rotated: {} sessions re-encrypted, {} config secrets re-wrapped",
        sessions, secrets
    );
    Ok(())
}

// ── Session helpers ──

fn find_session_files(paths: &Paths, ext: &str) -> Vec<PathBuf> {
    let agents_dir = paths.state_dir.join("agents");
    let mut files = Vec::new();
    if let Ok(agents) = std::fs::read_dir(&agents_dir) {
        for agent in agents.filter_map(|e| e.ok()) {
            let sessions_dir = agent.path().join("sessions");
            if let Ok(sessions) = std::fs::read_dir(&sessions_dir) {
                for entry in sessions.filter_map(|e| e.ok()) {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some(ext)
                        || (ext == "enc" && path.to_string_lossy().ends_with(".jsonl.enc"))
                    {
                        files.push(path);
                    }
                }
            }
        }
    }
    files
}

fn encrypt_sessions(paths: &Paths, key: &EncryptionKey) -> Result<usize> {
    let files = find_session_files(paths, "jsonl");
    let mut count = 0;
    for path in &files {
        let content = std::fs::read(path)?;
        let encrypted = key.encrypt(&content)?;
        let enc_path = path.with_extension("jsonl.enc");
        std::fs::write(&enc_path, &encrypted)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&enc_path, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::remove_file(path)?;
        count += 1;
    }
    Ok(count)
}

fn decrypt_sessions(paths: &Paths, key: &EncryptionKey) -> Result<usize> {
    let files = find_session_files(paths, "enc");
    let mut count = 0;
    for path in &files {
        let data = std::fs::read(path)?;
        let plaintext = key.decrypt(&data)?;
        // .jsonl.enc → .jsonl
        let plain_path =
            Path::new(&path.to_string_lossy().replace(".jsonl.enc", ".jsonl")).to_path_buf();
        std::fs::write(&plain_path, &plaintext)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&plain_path, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::remove_file(path)?;
        count += 1;
    }
    Ok(count)
}

fn reencrypt_sessions(
    paths: &Paths,
    old_key: &EncryptionKey,
    new_key: &EncryptionKey,
) -> Result<usize> {
    let files = find_session_files(paths, "enc");
    let mut count = 0;
    for path in &files {
        let data = std::fs::read(path)?;
        let plaintext = old_key.decrypt(&data)?;
        let reencrypted = new_key.encrypt(&plaintext)?;
        std::fs::write(path, &reencrypted)?;
        count += 1;
    }
    Ok(count)
}

fn count_session_files(paths: &Paths) -> (usize, usize) {
    let plain = find_session_files(paths, "jsonl").len();
    let enc = find_session_files(paths, "enc").len();
    (plain, enc)
}

// ── Config helpers ──

fn encrypt_config_secrets(paths: &Paths, key: &EncryptionKey) -> Result<usize> {
    let config_path = paths.config_file();
    if !config_path.exists() {
        return Ok(0);
    }

    let content = std::fs::read_to_string(&config_path)?;
    let (new_content, count) = transform_config_secrets(&content, |value| {
        if value.starts_with(ENC_PREFIX) {
            Ok(value.to_string()) // Already encrypted
        } else if looks_like_secret(value) {
            key.encrypt_config_value(value)
        } else {
            Ok(value.to_string())
        }
    })?;

    if count > 0 {
        std::fs::write(&config_path, new_content)?;
    }
    Ok(count)
}

fn decrypt_config_secrets(paths: &Paths, key: &EncryptionKey) -> Result<usize> {
    let config_path = paths.config_file();
    if !config_path.exists() {
        return Ok(0);
    }

    let content = std::fs::read_to_string(&config_path)?;
    let (new_content, count) =
        transform_config_secrets(&content, |value| key.decrypt_config_value(value))?;

    if count > 0 {
        std::fs::write(&config_path, new_content)?;
    }
    Ok(count)
}

fn reencrypt_config_secrets(
    paths: &Paths,
    old_key: &EncryptionKey,
    new_key: &EncryptionKey,
) -> Result<usize> {
    let config_path = paths.config_file();
    if !config_path.exists() {
        return Ok(0);
    }

    let content = std::fs::read_to_string(&config_path)?;
    let (new_content, count) = transform_config_secrets(&content, |value| {
        if let Some(encoded) = value.strip_prefix(ENC_PREFIX) {
            let data = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encoded)?;
            let plaintext = old_key.decrypt(&data)?;
            let plain_str = String::from_utf8(plaintext)?;
            new_key.encrypt_config_value(&plain_str)
        } else {
            Ok(value.to_string())
        }
    })?;

    if count > 0 {
        std::fs::write(&config_path, new_content)?;
    }
    Ok(count)
}

fn count_config_secrets(paths: &Paths) -> usize {
    let config_path = paths.config_file();
    if !config_path.exists() {
        return 0;
    }
    std::fs::read_to_string(&config_path)
        .map(|c| c.matches(ENC_PREFIX).count())
        .unwrap_or(0)
}

/// Check if a TOML string value looks like an API key/secret.
fn looks_like_secret(value: &str) -> bool {
    let v = value.trim();
    // Skip empty, booleans, numbers, paths, URLs
    if v.is_empty() || v.starts_with('/') || v.starts_with('~') || v.starts_with("http") {
        return false;
    }
    // Common API key patterns
    v.starts_with("sk-")
        || v.starts_with("key-")
        || v.starts_with("xai-")
        || v.starts_with("gsk_")
        || v.contains("api_key")
        || v.contains("token")
        || (v.len() > 20
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
}

/// Transform secret values in a TOML config file.
/// Operates on raw text to preserve comments and formatting.
/// Returns (new_content, count_of_transformed_values).
fn transform_config_secrets(
    content: &str,
    transform: impl Fn(&str) -> Result<String>,
) -> Result<(String, usize)> {
    let secret_keys = ["api_key", "api_token", "token", "secret", "password"];

    let mut result = String::with_capacity(content.len());
    let mut count = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        let is_secret_line = secret_keys
            .iter()
            .any(|k| trimmed.starts_with(k) || trimmed.starts_with(&format!("# {}", k)));

        if is_secret_line && let Some((key_part, value_part)) = trimmed.split_once('=') {
            let value = value_part.trim().trim_matches('"');
            if !value.is_empty() && value != "\"\"" {
                let transformed = transform(value)?;
                if transformed != value {
                    let indent = &line[..line.len() - trimmed.len()];
                    result.push_str(&format!(
                        "{}{}= \"{}\"\n",
                        indent,
                        key_part.trim_end(),
                        transformed
                    ));
                    count += 1;
                    continue;
                }
            }
        }

        result.push_str(line);
        result.push('\n');
    }

    Ok((result, count))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_paths() -> (TempDir, Paths) {
        let tmp = TempDir::new().unwrap();
        let paths = Paths::from_root(tmp.path());
        paths.ensure_dirs().unwrap();
        (tmp, paths)
    }

    #[test]
    fn test_enable_creates_key_and_encrypts_session() {
        let (tmp, paths) = setup_test_paths();

        // Create a test session file
        let sessions_dir = paths.sessions_dir("main");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let session_file = sessions_dir.join("test.jsonl");
        std::fs::write(&session_file, r#"{"type":"session","id":"test"}"#).unwrap();

        cmd_enable(&paths).unwrap();

        // Key should exist
        assert!(key_path(&paths).exists());

        // Plaintext session should be gone, encrypted should exist
        assert!(!session_file.exists());
        assert!(sessions_dir.join("test.jsonl.enc").exists());

        let _ = tmp; // prevent early drop
    }

    #[test]
    fn test_disable_decrypts_and_removes_key() {
        let (tmp, paths) = setup_test_paths();

        // Enable first
        let sessions_dir = paths.sessions_dir("main");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(
            sessions_dir.join("test.jsonl"),
            r#"{"type":"session","id":"test"}"#,
        )
        .unwrap();
        cmd_enable(&paths).unwrap();

        // Now disable
        cmd_disable(&paths).unwrap();

        // Key should be gone
        assert!(!key_path(&paths).exists());

        // Plaintext session should be restored
        assert!(sessions_dir.join("test.jsonl").exists());
        assert!(!sessions_dir.join("test.jsonl.enc").exists());

        // Content should match original
        let content = std::fs::read_to_string(sessions_dir.join("test.jsonl")).unwrap();
        assert!(content.contains("test"));

        let _ = tmp;
    }

    #[test]
    fn test_status_reports_counts() {
        let (_tmp, paths) = setup_test_paths();

        // No encryption
        let (plain, enc) = count_session_files(&paths);
        assert_eq!(plain, 0);
        assert_eq!(enc, 0);

        // Create sessions and enable
        let sessions_dir = paths.sessions_dir("main");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(sessions_dir.join("a.jsonl"), "data").unwrap();
        std::fs::write(sessions_dir.join("b.jsonl"), "data").unwrap();

        cmd_enable(&paths).unwrap();

        let (plain, enc) = count_session_files(&paths);
        assert_eq!(plain, 0);
        assert_eq!(enc, 2);
    }

    #[test]
    fn test_rotate_preserves_data() {
        let (_tmp, paths) = setup_test_paths();
        let original_data = r#"{"type":"session","id":"rotate-test"}"#;

        let sessions_dir = paths.sessions_dir("main");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(sessions_dir.join("test.jsonl"), original_data).unwrap();

        cmd_enable(&paths).unwrap();
        let key1_bytes = std::fs::read(key_path(&paths)).unwrap();

        cmd_rotate(&paths).unwrap();
        let key2_bytes = std::fs::read(key_path(&paths)).unwrap();

        // Key should have changed
        assert_ne!(key1_bytes, key2_bytes);

        // Data should still be recoverable
        cmd_disable(&paths).unwrap();
        let recovered = std::fs::read_to_string(sessions_dir.join("test.jsonl")).unwrap();
        assert_eq!(recovered, original_data);
    }

    #[test]
    fn test_enable_when_already_enabled() {
        let (_tmp, paths) = setup_test_paths();
        cmd_enable(&paths).unwrap();
        let result = cmd_enable(&paths);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already enabled"));
    }

    #[test]
    fn test_disable_when_not_enabled() {
        let (_tmp, paths) = setup_test_paths();
        let result = cmd_disable(&paths);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not enabled"));
    }

    #[test]
    fn test_looks_like_secret() {
        assert!(looks_like_secret("sk-abc123def456"));
        assert!(looks_like_secret("gsk_test_key_value"));
        assert!(looks_like_secret("xai-something-long"));
        assert!(!looks_like_secret(""));
        assert!(!looks_like_secret("/path/to/file"));
        assert!(!looks_like_secret("https://api.example.com"));
        assert!(!looks_like_secret("~/.config/file"));
        assert!(!looks_like_secret("true"));
    }

    #[test]
    fn test_transform_config_secrets() {
        let config = r#"
[providers.openai]
api_key = "sk-test123"
base_url = "https://api.openai.com/v1"

[custom_service]
api_token = "bot123456:ABC"
"#;
        let (transformed, count) = transform_config_secrets(config, |value| {
            if value.starts_with(ENC_PREFIX) {
                Ok(value.to_string())
            } else {
                Ok(format!("enc:ENCRYPTED_{}", value))
            }
        })
        .unwrap();

        assert_eq!(count, 2);
        assert!(transformed.contains("enc:ENCRYPTED_sk-test123"));
        assert!(transformed.contains("enc:ENCRYPTED_bot123456:ABC"));
        // URL should be unchanged
        assert!(transformed.contains("https://api.openai.com/v1"));
    }
}
