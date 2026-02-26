use anyhow::Result;
use clap::{Args, Subcommand};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::task::JoinSet;

#[cfg(unix)]
use daemonize::Daemonize;

use localgpt_core::concurrency::TurnGate;
use localgpt_core::config::Config;
use localgpt_core::heartbeat::HeartbeatRunner;
use localgpt_core::memory::MemoryManager;
use localgpt_server::Server;
use std::time::Duration;

/// Agent ID used for bridge CLI sessions.
const BRIDGE_CLI_AGENT_ID: &str = "bridge-cli";

/// Synchronously stop the daemon (for use before Tokio runtime starts)
pub fn stop_sync() -> Result<()> {
    let pid_file = get_pid_file()?;

    if !pid_file.exists() {
        println!("Daemon is not running");
        return Ok(());
    }

    let pid = fs::read_to_string(&pid_file)?.trim().to_string();

    if !is_process_running(&pid) {
        println!("Daemon is not running (stale PID file)");
        fs::remove_file(&pid_file)?;
        return Ok(());
    }

    println!("Stopping daemon (PID: {})...", pid);

    // Send SIGTERM
    #[cfg(unix)]
    {
        use std::process::Command;
        Command::new("kill").args(["-TERM", &pid]).status()?;
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("taskkill").args(["/PID", &pid]).status()?;
    }

    // Wait for process to stop (up to 5 seconds)
    for _ in 0..50 {
        if !is_process_running(&pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    if is_process_running(&pid) {
        anyhow::bail!("Failed to stop daemon (PID: {})", pid);
    }

    println!("Daemon stopped");
    fs::remove_file(&pid_file).ok();

    Ok(())
}

/// Fork and daemonize BEFORE starting the Tokio runtime.
/// This avoids the macOS fork-safety issue with ObjC/Swift runtime.
#[cfg(unix)]
pub fn daemonize_and_run(agent_id: &str) -> Result<()> {
    let config = Config::load()?;

    // Check if already running
    let pid_file = get_pid_file()?;
    if pid_file.exists() {
        let pid = fs::read_to_string(&pid_file)?;
        if is_process_running(&pid) {
            anyhow::bail!("Daemon already running (PID: {})", pid.trim());
        }
        fs::remove_file(&pid_file)?;
    }

    let log_file = get_log_file(config.logging.retention_days)?;

    // Print startup info before daemonizing
    println!(
        "Starting LocalGPT daemon in background (agent: {})...",
        agent_id
    );
    println!("  PID file: {}", pid_file.display());
    println!("  Log file: {}", log_file.display());
    if config.server.enabled {
        println!(
            "  Server: http://{}:{}",
            config.server.bind, config.server.port
        );
    }
    println!("\nUse 'localgpt daemon status' to check status");
    println!("Use 'localgpt daemon stop' to stop\n");

    if let Some(notice) = localgpt_core::config::check_openclaw_detected() {
        println!("{}\n", notice);
    }

    // Fork BEFORE starting Tokio
    // Use append mode to preserve previous logs within the same day
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)?;
    let stderr = stdout.try_clone()?;

    let daemonize = Daemonize::new()
        .pid_file(&pid_file)
        .working_directory(std::env::current_dir()?)
        .stdout(stdout)
        .stderr(stderr);

    match daemonize.start() {
        Ok(_) => {
            // Now in the child process - safe to start Tokio
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(run_daemon_server(config, agent_id))
        }
        Err(e) => anyhow::bail!("Failed to daemonize: {}", e),
    }
}

/// Run the daemon server (called after fork in background mode)
async fn run_daemon_server(config: Config, agent_id: &str) -> Result<()> {
    // Initialize logging in the daemon process
    // Disable ANSI colors since we're writing to a file
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .with_ansi(false)
        .init();

    let memory = MemoryManager::new_with_full_config(&config.memory, Some(&config), agent_id)?;
    let _watcher = memory.start_watcher()?;

    // Create config watcher for hot-reload support
    let config_watcher = match Config::load() {
        Ok(cfg) => match localgpt_core::config::ConfigWatcher::start(cfg) {
            Ok(w) => {
                println!("  Config hot-reload: enabled");
                Some(Arc::new(w))
            }
            Err(e) => {
                tracing::warn!("Failed to start config watcher: {}", e);
                None
            }
        },
        Err(_) => None,
    };

    // Set up SIGHUP handler for manual config reload
    if let Some(ref watcher) = config_watcher {
        localgpt_core::config::spawn_sighup_handler(watcher.clone());
    }

    println!("Daemon started successfully");

    run_daemon_services(&config, agent_id, config_watcher).await?;

    println!("\nShutting down...");
    let pid_file = get_pid_file()?;
    fs::remove_file(&pid_file).ok();

    Ok(())
}

/// Run daemon services (server and/or heartbeat)
async fn run_daemon_services(
    config: &Config,
    agent_id: &str,
    // Config watcher is available for services that need hot-reload support
    // Services can subscribe to config changes via config_watcher.subscribe()
    _config_watcher: Option<Arc<localgpt_core::config::ConfigWatcher>>,
) -> Result<()> {
    // Create shared turn gate for heartbeat + HTTP concurrency control
    let turn_gate = TurnGate::new();

    // Collect all running JoinHandles
    let mut handles = JoinSet::new();

    // Note: Services that need hot-reload should subscribe to config_watcher.subscribe()
    // and update their internal state when a new config is received.
    // For simplicity, most services currently use the config passed at startup.

    // Run session pruning at startup
    if config.agent.session_max_age > 0 || config.agent.session_max_count > 0 {
        let paths = localgpt_core::paths::Paths::resolve()?;
        let max_age = if config.agent.session_max_age > 0 {
            Some(Duration::from_secs(config.agent.session_max_age))
        } else {
            None
        };
        let max_count = if config.agent.session_max_count > 0 {
            Some(config.agent.session_max_count)
        } else {
            None
        };

        match localgpt_core::agent::prune_all_agents(&paths.state_dir, max_age, max_count) {
            Ok(result) if result.deleted > 0 => {
                println!(
                    "  Session pruning: deleted {} old sessions (freed {} bytes)",
                    result.deleted, result.freed_bytes
                );
            }
            Ok(_) => {
                println!("  Session pruning: no old sessions to delete");
            }
            Err(e) => {
                tracing::warn!("Session pruning failed: {}", e);
            }
        }

        // Spawn periodic session pruning task (every hour)
        let state_dir = paths.state_dir.clone();
        handles.spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60 * 60));
            loop {
                interval.tick().await;
                if let Err(e) =
                    localgpt_core::agent::prune_all_agents(&state_dir, max_age, max_count)
                {
                    tracing::warn!("Periodic session pruning failed: {}", e);
                }
            }
        });
    }

    // Spawn heartbeat in background if enabled
    if config.heartbeat.enabled {
        let heartbeat_config = config.clone();
        let heartbeat_agent_id = agent_id.to_string();
        let heartbeat_gate = turn_gate.clone();
        println!(
            "  Heartbeat: enabled (interval: {})",
            config.heartbeat.interval
        );
        handles.spawn(async move {
            // Create tool factory that provides CLI tools to heartbeat
            let tool_factory: localgpt_core::heartbeat::ToolFactory =
                Box::new(|config: &localgpt_core::config::Config| {
                    crate::tools::create_cli_tools(config)
                });

            let runner = match HeartbeatRunner::new_with_gate_and_tools(
                &heartbeat_config,
                &heartbeat_agent_id,
                Some(heartbeat_gate),
                Some(tool_factory),
            ) {
                Ok(runner) => runner,
                Err(e) => {
                    tracing::error!("Failed to create heartbeat runner: {}", e);
                    return;
                }
            };
            tracing::info!("Heartbeat runner created");
            if let Err(e) = runner.run().await {
                tracing::error!("Heartbeat runner error: {}", e);
            }
        });
    } else {
        println!("  Heartbeat: disabled");
    }

    // Spawn Telegram bot in background if configured
    if config.telegram.as_ref().is_some_and(|t| t.enabled) {
        let tg_config = config.clone();
        let tg_gate = turn_gate.clone();
        println!("  Telegram: enabled");
        handles.spawn(async move {
            // Create tool factory that provides CLI tools to Telegram
            let tool_factory: localgpt_server::telegram::ToolFactory =
                Box::new(|config: &localgpt_core::config::Config| {
                    crate::tools::create_cli_tools(config)
                });

            let bot = localgpt_server::telegram::run_telegram_bot(
                &tg_config,
                tg_gate,
                Some(tool_factory),
            );
            tracing::info!("Telegram bot created");
            if let Err(e) = bot.await {
                tracing::error!("Telegram bot error: {}", e);
            }
        });
    } else {
        println!("  Telegram: disabled");
    }

    // Spawn cron scheduler if any jobs are configured
    if !config.cron.jobs.is_empty() {
        let cron_config = config.clone();
        let scheduler = localgpt_core::cron::CronScheduler::new(&config.cron.jobs);
        let job_count = config.cron.jobs.iter().filter(|j| j.enabled).count();
        println!("  Cron: {} job(s) scheduled", job_count);
        handles.spawn(async move {
            // Create tool factory that provides CLI tools to cron jobs
            let tool_factory: localgpt_core::cron::ToolFactory =
                Box::new(|config: &localgpt_core::config::Config| {
                    crate::tools::create_cli_tools(config).unwrap_or_default()
                });

            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                scheduler.tick(&cron_config, Some(&tool_factory)).await;
            }
        });
    } else {
        println!("  Cron: no jobs configured");
    }

    if config.server.enabled {
        let bridge_memory =
            MemoryManager::new_with_full_config(&config.memory, Some(config), BRIDGE_CLI_AGENT_ID)?;
        let bridge_manager =
            localgpt_server::BridgeManager::new_with_agent_support(config.clone(), bridge_memory);

        // Spawn Server
        let server_config = config.clone();
        let server_gate = turn_gate.clone();
        let server_bridge_manager = bridge_manager.clone();
        println!(
            "  Server: http://{}:{}",
            server_config.server.bind, server_config.server.port
        );
        handles.spawn(async move {
            match Server::new_daemon(&server_config, server_gate, server_bridge_manager) {
                Err(e) => {
                    tracing::error!("Failed to create HTTP server: {}", e);
                }
                Ok(server) => {
                    if let Err(e) = server.run().await {
                        tracing::error!("HTTP server error: {}", e);
                    }
                }
            }
        });

        // Spawn Bridge Manager
        let paths = localgpt_core::paths::Paths::resolve()?;
        let bridge_socket = paths.bridge_socket_name();
        println!("  Bridge: enabled (socket: {})", bridge_socket);
        handles.spawn(async move {
            if let Err(e) = bridge_manager.serve(&bridge_socket).await {
                tracing::error!("Bridge server error: {}", e);
            }
        });
    } else {
        println!("  Server: disabled");
    }

    tokio::signal::ctrl_c().await?;

    println!("  Server: shutting down");
    handles.shutdown().await;

    Ok(())
}

#[derive(Args)]
pub struct DaemonArgs {
    #[command(subcommand)]
    pub command: DaemonCommands,
}

#[derive(Subcommand)]
pub enum DaemonCommands {
    /// Start the daemon
    Start {
        /// Run in foreground (don't daemonize)
        #[arg(short, long)]
        foreground: bool,
    },

    /// Stop the daemon
    Stop,

    /// Restart the daemon (stop then start)
    Restart {
        /// Run in foreground (don't daemonize)
        #[arg(short, long)]
        foreground: bool,
    },

    /// Show daemon status
    Status,

    /// Run heartbeat once (for testing)
    Heartbeat,
}

pub async fn run(args: DaemonArgs, agent_id: &str) -> Result<()> {
    match args.command {
        DaemonCommands::Start { foreground } => start_daemon(foreground, agent_id).await,
        DaemonCommands::Stop => stop_daemon().await,
        DaemonCommands::Restart { foreground } => restart_daemon(foreground, agent_id).await,
        DaemonCommands::Status => show_status().await,
        DaemonCommands::Heartbeat => run_heartbeat_once(agent_id).await,
    }
}

async fn start_daemon(foreground: bool, agent_id: &str) -> Result<()> {
    let config = Config::load()?;

    // Check if already running
    let pid_file = get_pid_file()?;
    if pid_file.exists() {
        let pid = fs::read_to_string(&pid_file)?;
        if is_process_running(&pid) {
            anyhow::bail!("Daemon already running (PID: {})", pid.trim());
        }
        fs::remove_file(&pid_file)?;
    }

    // Background mode on Unix is handled by daemonize_and_run() before Tokio starts
    // This function only handles foreground mode and non-Unix platforms
    #[cfg(unix)]
    if !foreground {
        // This shouldn't be reached - background mode is handled in main()
        anyhow::bail!("Background mode should be handled before Tokio starts");
    }

    #[cfg(not(unix))]
    if !foreground {
        println!(
            "Note: Background daemonization not supported on this platform. Running in foreground."
        );
    }

    println!(
        "Starting LocalGPT daemon in foreground (agent: {})...",
        agent_id
    );

    if let Some(notice) = localgpt_core::config::check_openclaw_detected() {
        println!("{}\n", notice);
    }

    // Write PID file for foreground mode
    fs::write(&pid_file, std::process::id().to_string())?;

    // Initialize components
    let memory = MemoryManager::new_with_full_config(&config.memory, Some(&config), agent_id)?;
    let _watcher = memory.start_watcher()?;

    // Create config watcher for hot-reload support
    let config_watcher = match localgpt_core::config::ConfigWatcher::start(config.clone()) {
        Ok(w) => {
            println!("  Config hot-reload: enabled");
            Some(Arc::new(w))
        }
        Err(e) => {
            tracing::warn!("Failed to start config watcher: {}", e);
            None
        }
    };

    // Set up SIGHUP handler for manual config reload
    if let Some(ref watcher) = config_watcher {
        localgpt_core::config::spawn_sighup_handler(watcher.clone());
    }

    println!("Daemon started successfully");

    run_daemon_services(&config, agent_id, config_watcher).await?;

    println!("\nShutting down...");
    fs::remove_file(&pid_file).ok();

    Ok(())
}

async fn stop_daemon() -> Result<()> {
    let pid_file = get_pid_file()?;

    if !pid_file.exists() {
        println!("Daemon is not running");
        return Ok(());
    }

    let pid = fs::read_to_string(&pid_file)?.trim().to_string();

    if !is_process_running(&pid) {
        println!("Daemon is not running (stale PID file)");
        fs::remove_file(&pid_file)?;
        return Ok(());
    }

    // Send SIGTERM
    #[cfg(unix)]
    {
        use std::process::Command;
        Command::new("kill").args(["-TERM", &pid]).status()?;
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("taskkill").args(["/PID", &pid]).status()?;
    }

    println!("Sent stop signal to daemon (PID: {})", pid);
    fs::remove_file(&pid_file)?;

    Ok(())
}

async fn restart_daemon(foreground: bool, agent_id: &str) -> Result<()> {
    // Stop the daemon if running
    let pid_file = get_pid_file()?;
    if pid_file.exists() {
        let pid = fs::read_to_string(&pid_file)?.trim().to_string();
        if is_process_running(&pid) {
            println!("Stopping daemon (PID: {})...", pid);

            #[cfg(unix)]
            {
                use std::process::Command;
                Command::new("kill").args(["-TERM", &pid]).status()?;
            }

            #[cfg(windows)]
            {
                use std::process::Command;
                Command::new("taskkill").args(["/PID", &pid]).status()?;
            }

            // Wait for process to stop (up to 5 seconds)
            for _ in 0..50 {
                if !is_process_running(&pid) {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            if is_process_running(&pid) {
                anyhow::bail!("Failed to stop daemon (PID: {})", pid);
            }

            println!("Daemon stopped");
        }
        fs::remove_file(&pid_file).ok();
    }

    // For background mode on Unix, we need to exit and let main() handle daemonization
    #[cfg(unix)]
    if !foreground {
        println!("\nTo start daemon in background, run: localgpt daemon start");
        println!("(Background restart requires re-running the command due to fork requirements)");
        return Ok(());
    }

    // Start in foreground mode
    println!();
    start_daemon(foreground, agent_id).await
}

async fn show_status() -> Result<()> {
    let config = Config::load()?;
    let pid_file = get_pid_file()?;

    let running = if pid_file.exists() {
        let pid = fs::read_to_string(&pid_file)?;
        is_process_running(&pid)
    } else {
        false
    };

    println!("LocalGPT Daemon Status");
    println!("----------------------");
    println!("Running: {}", if running { "yes" } else { "no" });

    if running {
        let pid = fs::read_to_string(&pid_file)?;
        println!("PID: {}", pid.trim());
    }

    if running {
        println!("\nConfiguration (Active):");
    } else {
        println!("\nConfiguration (Inactive):");
    }
    println!("  Heartbeat enabled: {}", config.heartbeat.enabled);
    if config.heartbeat.enabled {
        println!("  Heartbeat interval: {}", config.heartbeat.interval);
        if let Some(timeout) = &config.heartbeat.timeout {
            println!("  Heartbeat timeout: {}", timeout);
        }
    }
    println!("  Cron enabled: {}", !config.cron.jobs.is_empty());
    if !config.cron.jobs.is_empty() {
        println!("  Cron jobs: {}", config.cron.jobs.len());
    }
    let telegram_enabled = config.telegram.as_ref().map_or(false, |t| t.enabled);
    println!("  Telegram enabled: {}", telegram_enabled);
    println!("  HTTP Server enabled: {}", config.server.enabled);
    if config.server.enabled {
        println!(
            "  HTTP Server address: http://{}:{}",
            config.server.bind, config.server.port
        );
    }

    if let Ok(paths) = localgpt_core::paths::Paths::resolve() {
        println!("  Workspace: {}", paths.workspace.display());
        println!("  Locks directory: {}", paths.locks_dir().display());
        println!("  Logs directory: {}", paths.logs_dir().display());
        if let Ok(log_file) = get_log_file(0) {
            println!("  Current log: {}", log_file.display());
        }
        println!("  Bridge socket: {}", paths.bridge_socket_name());
    }

    Ok(())
}

async fn run_heartbeat_once(agent_id: &str) -> Result<()> {
    let config = Config::load()?;

    // Create tool factory to provide CLI tools
    let tool_factory: localgpt_core::heartbeat::ToolFactory =
        Box::new(|config: &localgpt_core::config::Config| crate::tools::create_cli_tools(config));

    let runner =
        HeartbeatRunner::new_with_gate_and_tools(&config, agent_id, None, Some(tool_factory))?;

    println!("Running heartbeat (agent: {})...", agent_id);
    let result = runner.run_once().await?;

    if result == "HEARTBEAT_OK" {
        println!("Heartbeat completed: No tasks needed attention");
    } else {
        println!("Heartbeat response:\n{}", result);
    }

    Ok(())
}

fn get_pid_file() -> Result<PathBuf> {
    let paths = localgpt_core::paths::Paths::resolve()?;
    Ok(paths.pid_file())
}

fn get_log_file(retention_days: u32) -> Result<PathBuf> {
    let paths = localgpt_core::paths::Paths::resolve()?;
    let logs_dir = paths.logs_dir();
    fs::create_dir_all(&logs_dir)?;

    // Prune old logs only if retention_days > 0
    if retention_days > 0 {
        prune_old_logs(&logs_dir, retention_days as i64);
    }

    // Use date-based log files (like OpenClaw)
    let date = chrono::Local::now().format("%Y-%m-%d");
    Ok(logs_dir.join(format!("localgpt-{}.log", date)))
}

/// Prune log files older than `keep_days` days
fn prune_old_logs(logs_dir: &std::path::Path, keep_days: i64) {
    let cutoff = chrono::Local::now() - chrono::Duration::days(keep_days);
    let cutoff_date = cutoff.format("%Y-%m-%d").to_string();

    if let Ok(entries) = fs::read_dir(logs_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Match localgpt-YYYY-MM-DD.log pattern
            if name_str.starts_with("localgpt-")
                && name_str.ends_with(".log")
                && let Some(date_part) = name_str
                    .strip_prefix("localgpt-")
                    .and_then(|s| s.strip_suffix(".log"))
                && date_part < cutoff_date.as_str()
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

fn is_process_running(pid: &str) -> bool {
    let pid = pid.trim();

    #[cfg(unix)]
    {
        use std::process::Command;
        Command::new("kill")
            .args(["-0", pid])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[cfg(windows)]
    {
        use std::process::Command;
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid)])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(pid))
            .unwrap_or(false)
    }
}
