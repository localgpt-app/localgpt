//! Doctor diagnostics command
//!
//! Validates LocalGPT setup and diagnoses common issues.

use anyhow::Result;
use clap::Args;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Check result status
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

/// Result of a single check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    /// Check name
    pub name: &'static str,
    /// Status
    pub status: CheckStatus,
    /// Human-readable message
    pub message: String,
    /// Hint for fixing the issue (if applicable)
    pub fix_hint: Option<String>,
}

impl CheckResult {
    fn pass(name: &'static str, message: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Pass,
            message: message.into(),
            fix_hint: None,
        }
    }

    fn warn(name: &'static str, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Warn,
            message: message.into(),
            fix_hint: Some(hint.into()),
        }
    }

    fn fail(name: &'static str, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            name,
            status: CheckStatus::Fail,
            message: message.into(),
            fix_hint: Some(hint.into()),
        }
    }
}

#[derive(Args)]
pub struct DoctorArgs {
    /// Auto-fix issues where possible
    #[arg(short, long)]
    pub fix: bool,

    /// Output results as JSON
    #[arg(long)]
    pub json: bool,
}

pub async fn run(args: DoctorArgs) -> Result<()> {
    let mut results = Vec::new();

    // Load config (check #1)
    let config_result = check_config_file(args.fix);
    results.push(config_result.clone());

    let config = localgpt_core::config::Config::load().ok();

    // Check #2: Workspace directory
    results.push(check_workspace_dir(config.as_ref(), args.fix));

    // Check #3: Memory database
    results.push(check_memory_database(config.as_ref(), args.fix).await);

    // Check #4: Embedding model
    results.push(check_embedding_model(config.as_ref()).await);

    // Check #5: Default provider reachable
    results.push(check_default_provider(config.as_ref()).await);

    // Check #6: API keys configured
    results.push(check_api_keys(config.as_ref()));

    // Check #7: Telegram token valid (if enabled)
    results.push(check_telegram_token(config.as_ref()).await);

    // Check #8: MCP servers connectable (if configured)
    results.push(check_mcp_servers(config.as_ref()).await);

    // Check #9: No stale PID file
    results.push(check_stale_pid_file(args.fix));

    // Check #10: Disk space adequate
    results.push(check_disk_space());

    // Check #11: Cron expressions valid (if configured)
    results.push(check_cron_expressions(config.as_ref()));

    // Check #12: Server port available (if enabled)
    results.push(check_server_port(config.as_ref()).await);

    // Output results
    if args.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        print_results(&results);
    }

    // Exit with error code if any check failed
    let has_failures = results.iter().any(|r| r.status == CheckStatus::Fail);
    if has_failures {
        std::process::exit(1);
    }

    Ok(())
}

fn print_results(results: &[CheckResult]) {
    println!("LocalGPT Doctor");
    println!("===============\n");

    let mut passed = 0;
    let mut warned = 0;
    let mut failed = 0;

    for result in results {
        let symbol = match result.status {
            CheckStatus::Pass => {
                passed += 1;
                "\x1b[32m✓\x1b[0m" // Green checkmark
            }
            CheckStatus::Warn => {
                warned += 1;
                "\x1b[33m⚠\x1b[0m" // Yellow warning
            }
            CheckStatus::Fail => {
                failed += 1;
                "\x1b[31m✗\x1b[0m" // Red X
            }
        };

        println!("{} {}", symbol, result.message);

        if let Some(ref hint) = result.fix_hint {
            println!("  \x1b[90m→ Hint: {}\x1b[0m", hint);
        }
    }

    println!(
        "\nResults: {} passed, {} warning{}, {} failed",
        passed,
        warned,
        if warned == 1 { "" } else { "s" },
        failed
    );
}

/// Check #1: Config file exists and parses
fn check_config_file(fix: bool) -> CheckResult {
    let paths = match localgpt_core::paths::Paths::resolve() {
        Ok(p) => p,
        Err(e) => {
            return CheckResult::fail(
                "Config file",
                format!("Cannot resolve paths: {}", e),
                "Check XDG environment variables",
            );
        }
    };

    let config_path = paths.config_file();

    if !config_path.exists() {
        if fix {
            // Create default config
            if let Err(e) = std::fs::create_dir_all(config_path.parent().unwrap()) {
                return CheckResult::fail(
                    "Config file",
                    "Config file does not exist",
                    format!("Failed to create config directory: {}", e),
                );
            }
            let default_config = r#"# LocalGPT configuration
[agent]
default_model = "claude-cli/opus"

[server]
enabled = true
port = 31327

[heartbeat]
enabled = false
"#;
            if let Err(e) = std::fs::write(&config_path, default_config) {
                return CheckResult::fail(
                    "Config file",
                    "Config file does not exist",
                    format!("Failed to create default config: {}", e),
                );
            }
            return CheckResult::pass(
                "Config file",
                format!("Created default config at {}", config_path.display()),
            );
        }
        return CheckResult::fail(
            "Config file",
            "Config file does not exist",
            format!("Run 'localgpt init' or create {}", config_path.display()),
        );
    }

    // Try to parse config
    match localgpt_core::config::Config::load() {
        Ok(_) => CheckResult::pass(
            "Config file",
            format!("Config valid at {}", config_path.display()),
        ),
        Err(e) => CheckResult::fail(
            "Config file",
            format!("Config parse error: {}", e),
            "Fix syntax errors in config.toml",
        ),
    }
}

/// Check #2: Workspace directory exists and writable
fn check_workspace_dir(config: Option<&localgpt_core::config::Config>, fix: bool) -> CheckResult {
    let workspace = match config {
        Some(c) => c.paths.workspace.clone(),
        None => {
            // Fallback to default
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".localgpt/workspace")
        }
    };

    if !workspace.exists() {
        if fix {
            if let Err(e) = std::fs::create_dir_all(&workspace) {
                return CheckResult::fail(
                    "Workspace directory",
                    "Workspace directory does not exist",
                    format!("Failed to create: {}", e),
                );
            }
            return CheckResult::pass(
                "Workspace directory",
                format!("Created workspace at {}", workspace.display()),
            );
        }
        return CheckResult::fail(
            "Workspace directory",
            format!(
                "Workspace directory does not exist: {}",
                workspace.display()
            ),
            "Run 'localgpt init' or create the directory manually",
        );
    }

    // Check if writable
    let test_file = workspace.join(".write_test");
    match std::fs::write(&test_file, b"test") {
        Ok(_) => {
            let _ = std::fs::remove_file(&test_file);
            CheckResult::pass(
                "Workspace directory",
                format!("Workspace directory writable at {}", workspace.display()),
            )
        }
        Err(e) => CheckResult::fail(
            "Workspace directory",
            format!("Workspace directory not writable: {}", e),
            "Check directory permissions",
        ),
    }
}

/// Check #3: Memory database opens without error
async fn check_memory_database(
    config: Option<&localgpt_core::config::Config>,
    _fix: bool,
) -> CheckResult {
    let config = match config {
        Some(c) => c,
        None => {
            return CheckResult::warn(
                "Memory database",
                "Cannot check without valid config",
                "Fix config file first",
            );
        }
    };

    // Try to create memory manager
    match localgpt_core::memory::MemoryManager::new_with_full_config(
        &config.memory,
        Some(config),
        "main",
    ) {
        Ok(mm) => {
            // Get stats
            match mm.stats() {
                Ok(stats) => CheckResult::pass(
                    "Memory database",
                    format!("Memory database OK ({} chunks indexed)", stats.total_chunks),
                ),
                Err(_) => CheckResult::pass("Memory database", "Memory database OK"),
            }
        }
        Err(e) => CheckResult::fail(
            "Memory database",
            format!("Memory database error: {}", e),
            "Try 'localgpt memory reindex' to rebuild",
        ),
    }
}

/// Check #4: Embedding model available
async fn check_embedding_model(config: Option<&localgpt_core::config::Config>) -> CheckResult {
    let config = match config {
        Some(c) => c,
        None => {
            return CheckResult::warn(
                "Embedding model",
                "Cannot check without valid config",
                "Fix config file first",
            );
        }
    };

    let provider = config.memory.embedding_provider.to_lowercase();

    match provider.as_str() {
        "local" => CheckResult::pass("Embedding model", "Local embeddings configured (fastembed)"),
        "openai" => {
            if std::env::var("OPENAI_API_KEY").is_ok() {
                CheckResult::pass(
                    "Embedding model",
                    "OpenAI embeddings configured (API key set)",
                )
            } else {
                CheckResult::fail(
                    "Embedding model",
                    "OpenAI embeddings configured but OPENAI_API_KEY not set",
                    "Set OPENAI_API_KEY environment variable",
                )
            }
        }
        "none" => CheckResult::pass("Embedding model", "Embeddings disabled (FTS5 only)"),
        _ => CheckResult::warn(
            "Embedding model",
            format!("Unknown embedding provider: {}", provider),
            "Check memory.embedding_provider config",
        ),
    }
}

/// Check #5: Default provider reachable
async fn check_default_provider(config: Option<&localgpt_core::config::Config>) -> CheckResult {
    let config = match config {
        Some(c) => c,
        None => {
            return CheckResult::warn(
                "Default provider",
                "Cannot check without valid config",
                "Fix config file first",
            );
        }
    };

    let model = &config.agent.default_model;

    // Check if it's a CLI-based provider
    if model.starts_with("claude-cli/") {
        // Check if claude CLI is installed
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new("claude")
                .arg("--version")
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) if output.status.success() => CheckResult::pass(
                "Default provider",
                format!("Claude CLI available ({})", model),
            ),
            _ => CheckResult::fail(
                "Default provider",
                format!("Claude CLI not found for {}", model),
                "Install claude CLI: npm install -g @anthropic-ai/claude-cli",
            ),
        }
    } else if model.starts_with("codex-cli/") {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new("codex")
                .arg("--version")
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) if output.status.success() => CheckResult::pass(
                "Default provider",
                format!("Codex CLI available ({})", model),
            ),
            _ => CheckResult::fail(
                "Default provider",
                format!("Codex CLI not found for {}", model),
                "Install codex CLI",
            ),
        }
    } else if model.starts_with("gemini-cli/") {
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new("gemini")
                .arg("--version")
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) if output.status.success() => CheckResult::pass(
                "Default provider",
                format!("Gemini CLI available ({})", model),
            ),
            _ => CheckResult::fail(
                "Default provider",
                format!("Gemini CLI not found for {}", model),
                "Install gemini CLI",
            ),
        }
    } else {
        // API-based providers - just check if we have config
        CheckResult::pass(
            "Default provider",
            format!("API provider configured ({})", model),
        )
    }
}

/// Check #6: API keys configured for enabled providers
fn check_api_keys(config: Option<&localgpt_core::config::Config>) -> CheckResult {
    let config = match config {
        Some(c) => c,
        None => {
            return CheckResult::warn(
                "API keys",
                "Cannot check without valid config",
                "Fix config file first",
            );
        }
    };

    let mut missing_keys = Vec::new();

    // Check OpenAI
    if config.providers.openai.is_some() && std::env::var("OPENAI_API_KEY").is_err() {
        missing_keys.push("OPENAI_API_KEY");
    }

    // Check Anthropic
    if config.providers.anthropic.is_some() && std::env::var("ANTHROPIC_API_KEY").is_err() {
        missing_keys.push("ANTHROPIC_API_KEY");
    }

    if missing_keys.is_empty() {
        CheckResult::pass("API keys", "All required API keys configured")
    } else {
        CheckResult::fail(
            "API keys",
            format!("Missing API keys: {}", missing_keys.join(", ")),
            "Set the required environment variables",
        )
    }
}

/// Check #7: Telegram token valid (if enabled)
async fn check_telegram_token(config: Option<&localgpt_core::config::Config>) -> CheckResult {
    let config = match config {
        Some(c) => c,
        None => return CheckResult::pass("Telegram token", "Telegram not configured"),
    };

    let telegram = match &config.telegram {
        Some(t) if t.enabled => t,
        _ => return CheckResult::pass("Telegram token", "Telegram not enabled"),
    };

    // Check if token is set
    if telegram.api_token.is_empty() {
        return CheckResult::fail(
            "Telegram token",
            "Telegram enabled but api_token not set",
            "Set telegram.api_token in config.toml",
        );
    }

    // Try to validate token by calling getMe
    let token = &telegram.api_token;
    let url = format!("https://api.telegram.org/bot{}/getMe", token);

    match reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            CheckResult::pass("Telegram token", "Telegram token valid")
        }
        Ok(resp) => CheckResult::fail(
            "Telegram token",
            format!("Telegram API returned: {}", resp.status()),
            "Check that the bot token is correct",
        ),
        Err(e) => CheckResult::warn(
            "Telegram token",
            format!("Cannot verify token: {}", e),
            "Check network connectivity",
        ),
    }
}

/// Check #8: MCP servers connectable (if configured)
async fn check_mcp_servers(config: Option<&localgpt_core::config::Config>) -> CheckResult {
    let config = match config {
        Some(c) => c,
        None => return CheckResult::pass("MCP servers", "Cannot check without valid config"),
    };

    if config.mcp.servers.is_empty() {
        return CheckResult::pass("MCP servers", "No MCP servers configured");
    }

    // Check if each server command exists
    let mut failed_servers = Vec::new();

    for server in &config.mcp.servers {
        // Check if the command exists
        if let Some(ref cmd) = server.command {
            let cmd_name = cmd.split_whitespace().next().unwrap_or(cmd);
            if which::which(cmd_name).is_err() {
                failed_servers.push(server.name.clone());
            }
        }
    }

    if failed_servers.is_empty() {
        CheckResult::pass(
            "MCP servers",
            format!("{} MCP server(s) configured", config.mcp.servers.len()),
        )
    } else {
        CheckResult::warn(
            "MCP servers",
            format!(
                "MCP server command(s) not found: {}",
                failed_servers.join(", ")
            ),
            "Check command path and ensure the MCP server is installed",
        )
    }
}

/// Check #9: No stale PID file
fn check_stale_pid_file(fix: bool) -> CheckResult {
    let paths = match localgpt_core::paths::Paths::resolve() {
        Ok(p) => p,
        Err(_) => return CheckResult::pass("PID file", "Cannot check PID file"),
    };

    let pid_file = paths.pid_file();

    if !pid_file.exists() {
        return CheckResult::pass("PID file", "No PID file (daemon not running)");
    }

    let pid = match std::fs::read_to_string(&pid_file) {
        Ok(p) => p.trim().to_string(),
        Err(_) => {
            return CheckResult::warn(
                "PID file",
                "Cannot read PID file",
                "Remove stale PID file manually",
            );
        }
    };

    // Check if process is running
    #[cfg(unix)]
    {
        use std::process::Command;
        let running = Command::new("kill")
            .args(["-0", &pid])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if running {
            CheckResult::pass("PID file", format!("Daemon running (PID: {})", pid))
        } else if fix {
            let _ = std::fs::remove_file(&pid_file);
            CheckResult::pass("PID file", "Removed stale PID file")
        } else {
            CheckResult::warn(
                "PID file",
                "Stale PID file (daemon not running)",
                "Run with --fix to remove, or 'localgpt daemon start' to start",
            )
        }
    }

    #[cfg(not(unix))]
    {
        let _ = fix;
        CheckResult::pass("PID file", format!("PID file exists ({})", pid))
    }
}

/// Check #10: Disk space adequate (>100MB free)
fn check_disk_space() -> CheckResult {
    #[cfg(unix)]
    {
        let paths = match localgpt_core::paths::Paths::resolve() {
            Ok(p) => p,
            Err(_) => return CheckResult::pass("Disk space", "Cannot check disk space"),
        };
        let data_dir = paths.data_dir;
        use std::fs;
        if fs::metadata(&data_dir).is_ok() {
            // Use statvfs to get free space via nix
            let path = std::ffi::CString::new(data_dir.to_string_lossy().to_string()).unwrap();
            // SAFETY: `path` is a valid NUL-terminated CString. `stat` is zeroed
            // which is a valid initial state for statvfs. The syscall writes filesystem
            // statistics into `stat` and returns 0 on success.
            unsafe {
                let mut stat: nix::libc::statvfs = std::mem::zeroed();
                if nix::libc::statvfs(path.as_ptr(), &mut stat) == 0 {
                    // statvfs field widths vary by platform; normalize before math.
                    let free_bytes = (stat.f_bavail as u128) * (stat.f_frsize as u128);
                    let free_mb = free_bytes / ((1024_u128) * (1024_u128));

                    if free_mb < 100 {
                        return CheckResult::warn(
                            "Disk space",
                            format!("Low disk space: {} MB free", free_mb),
                            "Free up disk space to ensure proper operation",
                        );
                    }
                    return CheckResult::pass("Disk space", format!("{} MB free", free_mb));
                }
            }
        }
    }

    CheckResult::pass("Disk space", "Disk space check skipped")
}

/// Check #11: Cron expressions valid (if configured)
fn check_cron_expressions(config: Option<&localgpt_core::config::Config>) -> CheckResult {
    let config = match config {
        Some(c) => c,
        None => return CheckResult::pass("Cron expressions", "Cannot check without valid config"),
    };

    if config.cron.jobs.is_empty() {
        return CheckResult::pass("Cron expressions", "No cron jobs configured");
    }

    let mut invalid = Vec::new();

    for job in &config.cron.jobs {
        if !job.enabled {
            continue;
        }
        // Try to parse the cron expression
        if job.schedule.parse::<croner::Cron>().is_err() {
            invalid.push(format!("{}: {}", job.name, job.schedule));
        }
    }

    if invalid.is_empty() {
        let count = config.cron.jobs.iter().filter(|j| j.enabled).count();
        CheckResult::pass(
            "Cron expressions",
            format!("{} cron job(s) configured", count),
        )
    } else {
        CheckResult::fail(
            "Cron expressions",
            format!("Invalid cron expression(s): {}", invalid.join(", ")),
            "Fix cron schedule syntax (use standard cron format)",
        )
    }
}

/// Check #12: Server port available (if enabled)
async fn check_server_port(config: Option<&localgpt_core::config::Config>) -> CheckResult {
    let config = match config {
        Some(c) => c,
        None => return CheckResult::pass("Server port", "Cannot check without valid config"),
    };

    if !config.server.enabled {
        return CheckResult::pass("Server port", "HTTP server not enabled");
    }

    let addr = format!("{}:{}", config.server.bind, config.server.port);

    // Try to bind to the port
    match tokio::net::TcpListener::bind(&addr).await {
        Ok(_) => CheckResult::pass(
            "Server port",
            format!("Port {} available", config.server.port),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => CheckResult::warn(
            "Server port",
            format!("Port {} already in use", config.server.port),
            "Stop the existing process or change server.port in config",
        ),
        Err(e) => CheckResult::warn(
            "Server port",
            format!("Cannot check port: {}", e),
            "Check server.bind and server.port config",
        ),
    }
}
