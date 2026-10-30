//! CLI commands for TLS certificate management.
//!
//! Available when the `tls` feature is enabled on localgpt-server.

use clap::{Args, Subcommand};

#[derive(Args)]
pub struct CertArgs {
    #[command(subcommand)]
    pub command: CertCommand,
}

#[derive(Subcommand)]
pub enum CertCommand {
    /// Show certificate info (expiry, SANs, paths)
    Info,

    /// Force certificate regeneration
    Regenerate,
}

pub fn run(args: &CertArgs, config: &localgpt_core::Config) -> anyhow::Result<()> {
    let cert_dir = std::path::Path::new(&config.server.tls_cert_dir);

    match &args.command {
        CertCommand::Info => {
            let meta_path = cert_dir.join("meta.json");
            if !meta_path.exists() {
                println!("No TLS certificates found at {}", cert_dir.display());
                println!(
                    "Run 'localgpt cert regenerate' or enable server.tls_enabled to generate."
                );
                return Ok(());
            }

            let content = std::fs::read_to_string(&meta_path)?;
            let meta: serde_json::Value = serde_json::from_str(&content)?;

            println!("TLS Certificate Info");
            println!("====================");
            println!("Directory: {}", cert_dir.display());
            println!();

            if let Some(sans) = meta.get("sans").and_then(|v| v.as_array()) {
                println!(
                    "SANs: {}",
                    sans.iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }

            if let Some(ts) = meta.get("server_expires_at").and_then(|v| v.as_i64()) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let days_left = (ts - now) / 86400;
                println!("Server cert expires in: {} days", days_left);
            }

            if let Some(ts) = meta.get("ca_expires_at").and_then(|v| v.as_i64()) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let days_left = (ts - now) / 86400;
                println!("CA cert expires in: {} days", days_left);
            }

            if let Some(ts) = meta.get("generated_at").and_then(|v| v.as_i64()) {
                let dt = chrono::DateTime::from_timestamp(ts, 0)
                    .map(|d| d.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                println!("Generated at: {}", dt);
            }

            println!();
            println!("Files:");
            for name in ["ca.pem", "ca-key.pem", "server.pem", "server-key.pem"] {
                let path = cert_dir.join(name);
                let exists = if path.exists() { "OK" } else { "MISSING" };
                println!("  {} [{}]", path.display(), exists);
            }

            Ok(())
        }
        CertCommand::Regenerate => {
            #[cfg(feature = "tls")]
            {
                let paths = localgpt_server::tls::certs::CertPaths::new(cert_dir);
                localgpt_server::tls::certs::generate_certs(&paths)?;
                println!("Certificates regenerated at {}", cert_dir.display());
                Ok(())
            }
            #[cfg(not(feature = "tls"))]
            {
                anyhow::bail!(
                    "TLS feature not enabled. Build with --features tls to use cert commands."
                );
            }
        }
    }
}
