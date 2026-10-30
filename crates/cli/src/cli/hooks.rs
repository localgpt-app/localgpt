//! Event hooks management CLI
//!
//! List, add, remove, and manage event hooks for automation.
//!
//! Usage:
//!   localgpt hooks list
//!   localgpt hooks add onMessage "notify-send 'New message'"
//!   localgpt hooks remove <name>

use anyhow::{Result, bail};
use clap::Subcommand;
use localgpt_core::config::Config;
use tracing::info;

#[derive(clap::Args, Debug)]
pub struct HooksArgs {
    #[command(subcommand)]
    pub command: HooksCommands,
}

#[derive(Subcommand, Debug)]
pub enum HooksCommands {
    /// List all configured hooks
    List,

    /// Add a new event hook
    Add(AddArgs),

    /// Remove a hook by name
    Remove(RemoveArgs),

    /// Enable a disabled hook
    Enable(EnableArgs),

    /// Disable a hook without removing it
    Disable(DisableArgs),

    /// Test a hook by simulating an event
    Test(TestArgs),
}

#[derive(clap::Args, Debug)]
pub struct AddArgs {
    /// Event type: onMessage, onToolCall, onSessionStart, onSessionEnd, beforeToolCall, afterToolCall
    pub event: String,

    /// Command to execute (shell command)
    pub command: String,

    /// Unique name for this hook (auto-generated if not provided)
    #[arg(short, long)]
    pub name: Option<String>,

    /// Only trigger when this condition is met (e.g., "tool:bash" for beforeToolCall)
    #[arg(short, long)]
    pub filter: Option<String>,
}

#[derive(clap::Args, Debug)]
pub struct RemoveArgs {
    /// Name of the hook to remove
    pub name: String,

    /// Skip confirmation prompt
    #[arg(short, long)]
    pub force: bool,
}

#[derive(clap::Args, Debug)]
pub struct EnableArgs {
    /// Name of the hook to enable
    pub name: String,
}

#[derive(clap::Args, Debug)]
pub struct DisableArgs {
    /// Name of the hook to disable
    pub name: String,
}

#[derive(clap::Args, Debug)]
pub struct TestArgs {
    /// Name of the hook to test
    pub name: String,

    /// JSON payload to pass to the hook (simulated event data)
    #[arg(short, long)]
    pub payload: Option<String>,
}

/// Supported hook events
const VALID_EVENTS: &[&str] = &[
    "onMessage",
    "onToolCall",
    "onSessionStart",
    "onSessionEnd",
    "beforeToolCall",
    "afterToolCall",
    "onHeartbeat",
    "onError",
];

pub fn run(args: HooksArgs) -> Result<()> {
    match args.command {
        HooksCommands::List => list_hooks(),
        HooksCommands::Add(add_args) => add_hook(add_args),
        HooksCommands::Remove(remove_args) => remove_hook(remove_args),
        HooksCommands::Enable(enable_args) => enable_hook(enable_args),
        HooksCommands::Disable(disable_args) => disable_hook(disable_args),
        HooksCommands::Test(test_args) => test_hook(test_args),
    }
}

fn list_hooks() -> Result<()> {
    let config = Config::load()?;

    if config.hooks.hooks.is_empty() {
        println!("No hooks configured.");
        println!();
        println!("Add a hook with: localgpt hooks add <event> <command>");
        println!();
        println!("Available events:");
        for event in VALID_EVENTS {
            println!("  - {}", event);
        }
        return Ok(());
    }

    println!("Configured Hooks");
    println!("================");
    println!();

    for hook in &config.hooks.hooks {
        let status = if hook.enabled { "✓" } else { "✗" };
        println!(
            "  [{}] {} ({})",
            status,
            hook.name,
            if hook.enabled { "enabled" } else { "disabled" }
        );
        println!("      Event:   {}", hook.event);
        println!("      Command: {}", hook.command);
        if let Some(ref filter) = hook.filter {
            println!("      Filter:  {}", filter);
        }
        println!();
    }

    Ok(())
}

fn add_hook(args: AddArgs) -> Result<()> {
    // Validate event type
    if !VALID_EVENTS.contains(&args.event.as_str()) {
        bail!(
            "Invalid event type '{}'. Valid events: {}",
            args.event,
            VALID_EVENTS.join(", ")
        );
    }

    let mut config = Config::load()?;

    let name = args.name.unwrap_or_else(|| {
        format!(
            "hook-{}-{}",
            args.event.to_lowercase(),
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        )
    });

    // Check for duplicate name
    if config.hooks.hooks.iter().any(|h| h.name == name) {
        bail!(
            "A hook named '{}' already exists. Use a different name with --name.",
            name
        );
    }

    let hook = localgpt_core::config::HookConfig {
        name: name.clone(),
        event: args.event,
        command: args.command,
        filter: args.filter,
        enabled: true,
    };

    config.hooks.hooks.push(hook);
    config.save()?;

    info!("Added hook: {}", name);
    println!("Added hook: {}", name);
    println!("Run 'localgpt hooks list' to see all hooks.");

    Ok(())
}

fn remove_hook(args: RemoveArgs) -> Result<()> {
    let mut config = Config::load()?;

    let idx = config
        .hooks
        .hooks
        .iter()
        .position(|h| h.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("Hook '{}' not found", args.name))?;

    if !args.force {
        println!("About to remove hook: {}", args.name);
        println!("Event: {}", config.hooks.hooks[idx].event);
        println!("Command: {}", config.hooks.hooks[idx].command);
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

    config.hooks.hooks.remove(idx);
    config.save()?;

    info!("Removed hook: {}", args.name);
    println!("Removed hook: {}", args.name);

    Ok(())
}

fn enable_hook(args: EnableArgs) -> Result<()> {
    let mut config = Config::load()?;

    let hook = config
        .hooks
        .hooks
        .iter_mut()
        .find(|h| h.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("Hook '{}' not found", args.name))?;

    if hook.enabled {
        println!("Hook '{}' is already enabled.", args.name);
        return Ok(());
    }

    hook.enabled = true;
    config.save()?;

    info!("Enabled hook: {}", args.name);
    println!("Enabled hook: {}", args.name);

    Ok(())
}

fn disable_hook(args: DisableArgs) -> Result<()> {
    let mut config = Config::load()?;

    let hook = config
        .hooks
        .hooks
        .iter_mut()
        .find(|h| h.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("Hook '{}' not found", args.name))?;

    if !hook.enabled {
        println!("Hook '{}' is already disabled.", args.name);
        return Ok(());
    }

    hook.enabled = false;
    config.save()?;

    info!("Disabled hook: {}", args.name);
    println!("Disabled hook: {}", args.name);

    Ok(())
}

fn test_hook(args: TestArgs) -> Result<()> {
    let config = Config::load()?;

    let hook = config
        .hooks
        .hooks
        .iter()
        .find(|h| h.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("Hook '{}' not found", args.name))?;

    println!("Testing hook: {}", hook.name);
    println!("Event: {}", hook.event);
    println!("Command: {}", hook.command);
    println!();

    // Build the command with environment variables
    let payload = args
        .payload
        .unwrap_or_else(|| r#"{"test": true}"#.to_string());

    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(&hook.command)
        .env("LOCALGPT_EVENT", &hook.event)
        .env("LOCALGPT_PAYLOAD", &payload)
        .output();

    match output {
        Ok(output) => {
            println!("Exit code: {:?}", output.status.code());
            if !output.stdout.is_empty() {
                println!("Stdout:\n{}", String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                println!("Stderr:\n{}", String::from_utf8_lossy(&output.stderr));
            }
        }
        Err(e) => {
            println!("Failed to execute hook: {}", e);
        }
    }

    Ok(())
}
