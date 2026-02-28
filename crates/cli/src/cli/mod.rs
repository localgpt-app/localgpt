pub mod ask;
pub mod bridge;
pub mod chat;
pub mod config;
pub mod daemon;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod doctor;
#[cfg(feature = "gen")]
pub mod gen3d;
pub mod init;
pub mod md;
pub mod memory;
pub mod paths;
pub mod sandbox;
pub mod search;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "localgpt")]
#[command(author, version, about = "A lightweight, local-only AI assistant")]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Path to config file
    #[arg(short, long, global = true, env = "LOCALGPT_CONFIG")]
    pub config: Option<String>,

    /// Agent ID to use (default: "main", OpenClaw-compatible)
    #[arg(
        short,
        long,
        global = true,
        default_value = "main",
        env = "LOCALGPT_AGENT"
    )]
    pub agent: String,

    /// Profile name for complete isolation (suffixes all XDG dirs: ~/.config/localgpt-{profile}, etc.)
    #[arg(short, long, global = true, env = "LOCALGPT_PROFILE")]
    pub profile: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start an interactive chat session
    Chat(chat::ChatArgs),

    /// Ask a single question
    Ask(ask::AskArgs),

    /// Launch the desktop GUI
    #[cfg(feature = "desktop")]
    Desktop(desktop::DesktopArgs),

    /// Launch 3D scene generation mode (Bevy renderer)
    #[cfg(feature = "gen")]
    Gen(gen3d::GenArgs),

    /// Manage the daemon
    Daemon(daemon::DaemonArgs),

    /// Memory operations
    Memory(memory::MemoryArgs),

    /// Configuration management
    Config(config::ConfigArgs),

    /// LocalGPT.md policy management
    Md(md::MdArgs),

    /// Show resolved XDG directory paths
    Paths,

    /// Shell sandbox management
    Sandbox(sandbox::SandboxArgs),

    /// Test and manage web search
    Search(search::SearchArgs),

    /// Initialize configuration and keys
    Init(init::InitArgs),

    /// Manage bridges and credentials
    Bridge(bridge::BridgeArgs),

    /// Run diagnostics to validate setup
    Doctor(doctor::DoctorArgs),
}
