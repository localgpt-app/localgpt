//! CLI subcommand: `localgpt md`
//!
//! Manages the workspace security policy (LocalGPT.md): signing, verification,
//! audit log inspection, and security posture reporting.

use anyhow::Result;
use clap::{Args, Subcommand};

use localgpt_core::config::Config;
use localgpt_core::security;
use localgpt_core::text::prefix_chars;

#[derive(Args)]
pub struct MdArgs {
    #[command(subcommand)]
    pub command: MdCommands,
}

#[derive(Subcommand)]
pub enum MdCommands {
    /// Sign LocalGPT.md with device key
    Sign,

    /// Verify LocalGPT.md signature
    Verify,

    /// Show security audit log
    Audit {
        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Filter by action type (e.g., write_blocked, tamper_detected)
        #[arg(long)]
        filter: Option<String>,
    },

    /// Show current security posture
    Status,
}

fn hash_preview(hash: &str) -> String {
    prefix_chars(hash, 16)
}

pub async fn run(args: MdArgs) -> Result<()> {
    match args.command {
        MdCommands::Sign => sign_policy().await,
        MdCommands::Verify => verify_policy().await,
        MdCommands::Audit { json, filter } => show_audit(json, filter).await,
        MdCommands::Status => show_status().await,
    }
}

async fn sign_policy() -> Result<()> {
    let config = Config::load()?;
    let workspace = config.workspace_path();
    let data_dir = &config.paths.data_dir;
    let state_dir = &config.paths.state_dir;

    // Ensure device key exists
    security::ensure_device_key(data_dir)?;

    // Check policy file exists
    let policy_path = workspace.join(security::POLICY_FILENAME);
    if !policy_path.exists() {
        anyhow::bail!(
            "No {} found at {}. Create it first.",
            security::POLICY_FILENAME,
            policy_path.display()
        );
    }

    // Sign
    let manifest = security::sign_policy(data_dir, &workspace, "cli")?;

    // Write audit entry
    security::append_audit_entry(
        state_dir,
        security::AuditAction::Signed,
        &manifest.content_sha256,
        "cli",
    )?;

    println!(
        "Signed {} (sha256: {} | hmac: {})",
        security::POLICY_FILENAME,
        hash_preview(&manifest.content_sha256),
        hash_preview(&manifest.hmac_sha256)
    );

    Ok(())
}

async fn verify_policy() -> Result<()> {
    let config = Config::load()?;
    let workspace = config.workspace_path();
    let data_dir = &config.paths.data_dir;
    let state_dir = &config.paths.state_dir;

    let result = security::load_and_verify_policy(&workspace, data_dir);

    match result {
        security::PolicyVerification::Valid(content) => {
            let char_count = content.len();
            println!("Policy: VALID ({} chars)", char_count);

            // Write audit entry
            let sha = security::content_sha256(&content);
            security::append_audit_entry(state_dir, security::AuditAction::Verified, &sha, "cli")?;
        }
        security::PolicyVerification::Unsigned => {
            println!("Policy: UNSIGNED");
            println!("  Run `localgpt md sign` to activate.");
        }
        security::PolicyVerification::TamperDetected => {
            println!("Policy: TAMPER DETECTED");
            println!("  The file was modified after signing. Re-sign with `localgpt md sign`.");

            security::append_audit_entry(
                state_dir,
                security::AuditAction::TamperDetected,
                "",
                "cli",
            )?;
        }
        security::PolicyVerification::Missing => {
            println!("Policy: MISSING");
            println!(
                "  No {} found. Using hardcoded security only.",
                security::POLICY_FILENAME
            );
        }
        security::PolicyVerification::ManifestCorrupted => {
            println!("Policy: MANIFEST CORRUPTED");
            println!("  Re-sign with `localgpt md sign`.");
        }
        security::PolicyVerification::SuspiciousContent(warnings) => {
            println!("Policy: REJECTED (suspicious content)");
            for w in &warnings {
                println!("  - {}", w);
            }
        }
    }

    Ok(())
}

async fn show_audit(json_output: bool, filter: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let state_dir = &config.paths.state_dir;

    let mut entries = security::read_audit_log(state_dir)?;

    // Apply filter if specified
    if let Some(ref filter_action) = filter {
        entries.retain(|e| {
            let action_str = serde_json::to_string(&e.action).unwrap_or_default();
            // action_str is like "\"write_blocked\"", strip quotes
            let action_str = action_str.trim_matches('"');
            action_str == filter_action
        });
    }

    if entries.is_empty() {
        if filter.is_some() {
            println!("No audit log entries matching filter.");
        } else {
            println!("No audit log entries.");
        }
        return Ok(());
    }

    // Verify chain integrity (on full log, not filtered)
    let broken = security::verify_audit_chain(state_dir)?;

    if json_output {
        let output = serde_json::to_string_pretty(&entries)?;
        println!("{}", output);
    } else {
        let label = if let Some(ref f) = filter {
            format!(
                "Security Audit Log ({} entries, filter: {}):",
                entries.len(),
                f
            )
        } else {
            format!("Security Audit Log ({} entries):", entries.len())
        };
        println!("{}", label);
        println!();

        for (i, entry) in entries.iter().enumerate() {
            let chain_status = if broken.contains(&i) {
                " [CHAIN BROKEN]"
            } else {
                ""
            };
            let detail_str = entry
                .detail
                .as_deref()
                .map(|d| format!(" — {}", d))
                .unwrap_or_default();
            println!(
                "  {} {:?} (source: {}, sha256: {}){}{}",
                entry.ts,
                entry.action,
                entry.source,
                hash_preview(&entry.content_sha256),
                chain_status,
                detail_str,
            );
        }

        println!();
        if broken.is_empty() {
            println!("Chain integrity: INTACT");
        } else {
            println!("Chain integrity: BROKEN at {} position(s)", broken.len());
        }
    }

    Ok(())
}

async fn show_status() -> Result<()> {
    let config = Config::load()?;
    let workspace = config.workspace_path();
    let data_dir = &config.paths.data_dir;
    let state_dir = &config.paths.state_dir;

    println!("Security Status:");

    // Policy file
    let policy_path = workspace.join(security::POLICY_FILENAME);
    if policy_path.exists() {
        let result = security::load_and_verify_policy(&workspace, data_dir);
        let status = match result {
            security::PolicyVerification::Valid(_) => "Valid (signed and verified)",
            security::PolicyVerification::Unsigned => "Unsigned (run `localgpt md sign`)",
            security::PolicyVerification::TamperDetected => "TAMPER DETECTED",
            security::PolicyVerification::Missing => "Missing",
            security::PolicyVerification::ManifestCorrupted => "Manifest corrupted",
            security::PolicyVerification::SuspiciousContent(_) => "Rejected (suspicious content)",
        };

        // Get signed_at from manifest if available
        let signed_at = security::read_manifest(&workspace)
            .ok()
            .map(|m| m.signed_at)
            .unwrap_or_else(|| "N/A".to_string());

        println!("  Policy:     {} (exists)", policy_path.display());
        println!("  Signature:  {} (signed: {})", status, signed_at);
    } else {
        println!("  Policy:     Not created");
    }

    // Device key
    let key_path = config.paths.device_key();
    if key_path.exists() {
        println!("  Device Key: Present");
    } else {
        println!("  Device Key: Missing (run `localgpt init`)");
    }

    // Audit log
    let entries = security::read_audit_log(state_dir)?;
    if entries.is_empty() {
        println!("  Audit Log:  Empty");
    } else {
        let broken = security::verify_audit_chain(state_dir)?;
        let chain_status = if broken.is_empty() {
            "chain intact"
        } else {
            "CHAIN BROKEN"
        };
        println!("  Audit Log:  {} entries, {}", entries.len(), chain_status);
    }

    // Protected files
    println!(
        "  Protected:  {} workspace files, {} external paths",
        security::PROTECTED_FILES.len(),
        security::PROTECTED_EXTERNAL_PATHS.len()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_preview_preserves_short_hash() {
        assert_eq!(hash_preview("abc123"), "abc123");
    }

    #[test]
    fn hash_preview_truncates_ascii_hash() {
        assert_eq!(hash_preview("0123456789abcdefextra"), "0123456789abcdef");
    }

    #[test]
    fn hash_preview_truncates_multibyte_without_panicking() {
        assert_eq!(
            hash_preview("✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅"),
            "✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅✅"
        );
    }
}
