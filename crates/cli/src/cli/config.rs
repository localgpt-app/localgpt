use anyhow::Result;
use clap::{Args, Subcommand};

use localgpt_core::config::Config;

#[derive(Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// Show current configuration
    Show {
        /// Output format: toml (default) or json
        #[arg(short, long, default_value = "toml")]
        format: String,
    },

    /// Get a configuration value
    Get {
        /// Config key (e.g., agent.default_model)
        key: String,
    },

    /// Set a configuration value
    Set {
        /// Config key (e.g., agent.default_model)
        key: String,

        /// Value to set
        value: String,
    },

    /// Show config file path
    Path,

    /// Initialize default config file
    Init {
        /// Overwrite existing config
        #[arg(short, long)]
        force: bool,
    },
}

pub async fn run(args: ConfigArgs) -> Result<()> {
    match args.command {
        ConfigCommands::Show { format } => show_config(&format),
        ConfigCommands::Get { key } => get_config(&key),
        ConfigCommands::Set { key, value } => set_config(&key, &value),
        ConfigCommands::Path => show_path(),
        ConfigCommands::Init { force } => init_config(force),
    }
}

fn show_config(format: &str) -> Result<()> {
    let mut config = Config::load()?;

    // Show resolved paths first (these account for LOCALGPT_PROFILE and env overrides)
    println!("# Resolved paths");
    println!("#   config:    {}", config.paths.config_dir.display());
    println!("#   data:      {}", config.paths.data_dir.display());
    println!("#   workspace: {}", config.paths.workspace.display());
    println!("#   state:     {}", config.paths.state_dir.display());
    println!("#   cache:     {}", config.paths.cache_dir.display());
    println!("#   logs:      {}", config.paths.logs_dir().display());
    if let Some(ref rt) = config.paths.runtime_dir {
        println!("#   runtime:   {}", rt.display());
    }
    println!();

    // Replace raw config values with resolved paths so output is accurate
    config.memory.workspace = config.paths.workspace.display().to_string();
    config.memory.embedding_cache_dir = config
        .paths
        .cache_dir
        .join("embeddings")
        .display()
        .to_string();
    config.logging.path = config.paths.logs_dir().display().to_string();

    match format {
        "json" => {
            let json = serde_json::to_string_pretty(&config)?;
            println!("{}", redact_secrets(&json));
        }
        _ => {
            let toml = toml::to_string_pretty(&config)?;
            println!("{}", redact_secrets(&toml));
        }
    }

    Ok(())
}

fn get_config(key: &str) -> Result<()> {
    let config = Config::load()?;
    let value = config.get_value(key)?;
    println!("{}", value);
    Ok(())
}

fn set_config(key: &str, value: &str) -> Result<()> {
    let mut config = Config::load()?;
    config.set_value(key, value)?;
    config.save()?;
    println!("Set {} = {}", key, value);
    Ok(())
}

fn show_path() -> Result<()> {
    let path = Config::config_path()?;
    println!("{}", path.display());
    Ok(())
}

fn init_config(force: bool) -> Result<()> {
    let path = Config::config_path()?;

    if path.exists() && !force {
        anyhow::bail!(
            "Config file already exists at {}. Use --force to overwrite.",
            path.display()
        );
    }

    // Create parent directories
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Write template config with comments
    std::fs::write(&path, DEFAULT_CONFIG_TEMPLATE)?;

    println!("Created config file at {}", path.display());
    Ok(())
}

/// Redact secret values in serialized config output.
/// Matches lines where the key contains "api_key", "api_token", "secret", or
/// "service_account_key" and replaces the value with "***".
fn redact_secrets(output: &str) -> String {
    const SECRET_KEYS: &[&str] = &[
        "api_key",
        "api_token",
        "secret",
        "service_account_key",
        "auth_token",
    ];

    output
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            // TOML: key = "value" or JSON: "key": "value"
            for key in SECRET_KEYS {
                // TOML format: api_key = "..."
                if trimmed.starts_with(key)
                    && trimmed.contains("= \"")
                    && let Some(eq_pos) = line.find("= \"")
                {
                    return format!("{}= \"***\"", &line[..eq_pos]);
                }
                // JSON format: "api_key": "..."
                let json_key = format!("\"{}\":", key);
                if trimmed.starts_with(&json_key)
                    && trimmed.contains('"')
                    && let Some(colon_pos) = line.find(':')
                {
                    return format!("{}: \"***\"", &line[..colon_pos]);
                }
            }
            line.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const DEFAULT_CONFIG_TEMPLATE: &str = r#"# LocalGPT Configuration

[agent]
# Default model: claude-cli/opus, anthropic/claude-sonnet-4-5, openai/gpt-4o, etc.
default_model = "claude-cli/opus"
context_window = 128000
reserve_tokens = 8000

# Anthropic API (for anthropic/* models)
# [providers.anthropic]
# api_key = "${ANTHROPIC_API_KEY}"

# OpenAI API (for openai/* models)
# [providers.openai]
# api_key = "${OPENAI_API_KEY}"

# Claude CLI (for claude-cli/* models, requires claude CLI installed)
[providers.claude_cli]
command = "claude"

[heartbeat]
enabled = true
interval = "30m"

# Only run during these hours (optional)
# [heartbeat.active_hours]
# start = "09:00"
# end = "22:00"

[memory]
# Workspace directory for memory files (MEMORY.md, HEARTBEAT.md, etc.)
# Default: XDG data dir (~/.local/share/localgpt/workspace)
# Override with: LOCALGPT_WORKSPACE=/path or LOCALGPT_PROFILE=work
# workspace = "~/.local/share/localgpt/workspace"

[server]
enabled = true
port = 31327
bind = "127.0.0.1"

[logging]
level = "info"
"#;
