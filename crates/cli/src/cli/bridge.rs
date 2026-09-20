//! Bridge management CLI
//!
//! Register, list, and manage local client bridge connections
//!
//! Usage:
//!   localgpt bridge register --id cli --secret <token>
//!   localgpt bridge list
//!   localgpt bridge status cli

use anyhow::Result;
use clap::{Args, Subcommand};
use localgpt_core::paths::Paths;
use localgpt_server::BridgeManager;
use std::fs;
use tracing::info;

#[derive(Args)]
pub struct BridgeArgs {
    #[command(subcommand)]
    pub command: BridgeCommands,
}

#[derive(Subcommand)]
pub enum BridgeCommands {
    /// Register a new bridge with credentials
    Register {
        /// Unique ID for the bridge (e.g., "cli")
        #[arg(long)]
        id: String,

        /// Secret key/token for the bridge
        #[arg(long)]
        secret: String,
    },

    /// List all registered bridges
    List,

    /// Show status of active bridge connections (requires running daemon)
    Status {
        /// Optional bridge ID to filter by
        #[arg(long)]
        id: Option<String>,
    },

    /// Remove a bridge's credentials
    Remove {
        /// Bridge ID to remove
        #[arg(long)]
        id: String,

        /// Skip confirmation prompt
        #[arg(short, long)]
        force: bool,
    },
}

pub async fn run(args: BridgeArgs) -> Result<()> {
    match args.command {
        BridgeCommands::Register { id, secret } => {
            let manager = BridgeManager::new();
            manager.register_bridge(&id, secret.as_bytes()).await?;
            println!("Bridge '{}' registered successfully.", id);
            println!("You may need to restart the daemon for changes to take effect.");
        }
        BridgeCommands::List => {
            list_bridges()?;
        }
        BridgeCommands::Status { id } => {
            show_status(id).await?;
        }
        BridgeCommands::Remove { id, force } => {
            remove_bridge(&id, force)?;
        }
    }
    Ok(())
}

/// List all registered bridges (by checking credential files)
fn list_bridges() -> Result<()> {
    let paths = Paths::resolve()?;
    let bridges_dir = paths.data_dir.join("bridges");

    if !bridges_dir.exists() {
        println!("No bridges registered.");
        println!();
        println!("Register a bridge with: localgpt bridge register --id <name> --secret <token>");
        return Ok(());
    }

    let mut bridges: Vec<String> = Vec::new();
    for entry in fs::read_dir(&bridges_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "enc")
            && let Some(stem) = path.file_stem()
        {
            bridges.push(stem.to_string_lossy().to_string());
        }
    }

    if bridges.is_empty() {
        println!("No bridges registered.");
        return Ok(());
    }

    bridges.sort();

    println!("Registered Bridges");
    println!("==================");
    println!();

    for bridge in &bridges {
        // Check if credentials file exists and is readable
        let cred_path = bridges_dir.join(format!("{}.enc", bridge));
        let size = fs::metadata(&cred_path).map(|m| m.len()).unwrap_or(0);

        println!("  {} ({} bytes encrypted)", bridge, size);
    }

    println!();
    println!("Use 'localgpt bridge status' to see active connections (requires running daemon).");

    Ok(())
}

/// Show status of active bridge connections
async fn show_status(filter_id: Option<String>) -> Result<()> {
    let manager = BridgeManager::new();
    let bridges = manager.get_active_bridges().await;

    let bridges: Vec<_> = if let Some(ref id) = filter_id {
        bridges
            .into_iter()
            .filter(|b| b.bridge_id.as_deref() == Some(id.as_str()))
            .collect()
    } else {
        bridges
    };

    if bridges.is_empty() {
        if let Some(id) = filter_id {
            println!("No active connections for bridge '{}'.", id);
        } else {
            println!("No active bridge connections.");
            println!();
            println!("Note: This command requires a running daemon.");
            println!("Start with: localgpt daemon start");
        }
        return Ok(());
    }

    println!("Active Bridge Connections");
    println!("=========================");
    println!();

    for bridge in &bridges {
        let health_icon = match bridge.health {
            localgpt_server::HealthStatus::Healthy => "✓",
            localgpt_server::HealthStatus::Degraded => "⚠",
            localgpt_server::HealthStatus::Unhealthy => "✗",
        };

        let health_str = format!("{:?}", bridge.health).to_lowercase();

        println!(
            "  [{}] {} ({})",
            health_icon,
            bridge.bridge_id.as_deref().unwrap_or("unknown"),
            health_str
        );
        println!("      Connection ID: {}", bridge.connection_id);
        println!(
            "      Connected:     {}",
            bridge.connected_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!(
            "      Last active:   {}",
            bridge.last_active.format("%Y-%m-%d %H:%M:%S UTC")
        );

        if let Some(pid) = bridge.pid {
            println!("      PID:           {}", pid);
        }
        if let Some(uid) = bridge.uid {
            println!("      UID:           {}", uid);
        }
        if bridge.consecutive_failures > 0 {
            println!("      Failures:      {}", bridge.consecutive_failures);
        }

        println!();
    }

    Ok(())
}

/// Remove a bridge's credentials
fn remove_bridge(id: &str, force: bool) -> Result<()> {
    let paths = Paths::resolve()?;
    let cred_path = paths.data_dir.join("bridges").join(format!("{}.enc", id));

    if !cred_path.exists() {
        anyhow::bail!("Bridge '{}' is not registered.", id);
    }

    if !force {
        println!("About to remove bridge: {}", id);
        println!("This will delete the stored credentials.");
        println!();
        print!("Confirm? [y/N] ");
        use std::io::{self, Write};
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    fs::remove_file(&cred_path)?;
    info!("Removed bridge: {}", id);
    println!("Removed bridge: {}", id);

    Ok(())
}
