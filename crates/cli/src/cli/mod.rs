pub mod ask;
pub mod audit;
pub mod bridge;
pub mod cert;
pub mod chat;
pub mod completion;
pub mod config;
pub mod cron;
pub mod daemon;
#[cfg(feature = "desktop")]
pub mod desktop;
pub mod doctor;
pub mod encrypt;
pub mod gen3d;
pub mod hooks;
pub mod init;
pub mod mcp_server;
pub mod md;
pub mod memory;
pub mod paths;
pub mod sandbox;
pub mod search;
pub mod session;
pub mod tool;
pub mod tui;

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

    /// Launch the Ratatui TUI
    Tui(tui::TuiArgs),

    /// Ask a single question
    Ask(ask::AskArgs),

    /// Launch the desktop GUI
    #[cfg(feature = "desktop")]
    Desktop(desktop::DesktopArgs),

    /// Launch 3D scene generation mode (delegates to localgpt-gen)
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

    /// Manage encryption at rest
    Encrypt(encrypt::EncryptArgs),

    /// Manage MCP tool servers
    Tool(tool::ToolArgs),

    /// Manage MCP tool servers (alias for 'tool')
    Plugin(tool::ToolArgs),

    /// Generate shell completion scripts
    Completion(completion::CompletionArgs),

    /// Manage cron jobs
    Cron(cron::CronArgs),

    /// Manage event hooks
    Hooks(hooks::HooksArgs),

    /// Run as MCP server (stdio) — exposes memory tools for external AI backends
    McpServer(mcp_server::McpServerArgs),

    /// Manage sessions (list, branch)
    Session(session::SessionArgs),

    /// Inspect compaction audit log (show, verify, stats)
    Audit(audit::AuditArgs),

    /// Manage TLS certificates (info, regenerate)
    Cert(cert::CertArgs),
}
