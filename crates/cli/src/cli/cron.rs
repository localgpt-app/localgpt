//! Cron job management CLI
//!
//! List, add, remove, and manage scheduled tasks.
//!
//! Usage:
//!   localgpt cron list
//!   localgpt cron add "0 9 * * *" "Summarize yesterday's work"
//!   localgpt cron remove <name>

use anyhow::{Result, bail};
use clap::Subcommand;
use localgpt_core::config::Config;
use tracing::info;

#[derive(clap::Args, Debug)]
pub struct CronArgs {
    #[command(subcommand)]
    pub command: CronCommands,
}

#[derive(Subcommand, Debug)]
pub enum CronCommands {
    /// List all configured cron jobs
    List,

    /// Add a new cron job
    Add(AddArgs),

    /// Remove a cron job by name
    Remove(RemoveArgs),

    /// Enable a disabled cron job
    Enable(EnableArgs),

    /// Disable a cron job without removing it
    Disable(DisableArgs),

    /// Run a cron job immediately (for testing)
    Run(RunArgs),
}

#[derive(clap::Args, Debug)]
pub struct AddArgs {
    /// Cron expression (e.g., "0 9 * * *" for daily at 9am) or interval (e.g., "every 30m")
    pub schedule: String,

    /// Prompt to send to the agent
    pub prompt: String,

    /// Unique name for this job (auto-generated if not provided)
    #[arg(short, long)]
    pub name: Option<String>,

    /// Optional Telegram channel to send output to
    #[arg(short, long)]
    pub channel: Option<String>,

    /// Timeout for the job (e.g., "5m", "1h")
    #[arg(short, long, default_value = "10m")]
    pub timeout: String,
}

#[derive(clap::Args, Debug)]
pub struct RemoveArgs {
    /// Name of the cron job to remove
    pub name: String,

    /// Skip confirmation prompt
    #[arg(short, long)]
    pub force: bool,
}

#[derive(clap::Args, Debug)]
pub struct EnableArgs {
    /// Name of the cron job to enable
    pub name: String,
}

#[derive(clap::Args, Debug)]
pub struct DisableArgs {
    /// Name of the cron job to disable
    pub name: String,
}

#[derive(clap::Args, Debug)]
pub struct RunArgs {
    /// Name of the cron job to run
    pub name: String,
}

pub fn run(args: CronArgs) -> Result<()> {
    match args.command {
        CronCommands::List => list_jobs(),
        CronCommands::Add(add_args) => add_job(add_args),
        CronCommands::Remove(remove_args) => remove_job(remove_args),
        CronCommands::Enable(enable_args) => enable_job(enable_args),
        CronCommands::Disable(disable_args) => disable_job(disable_args),
        CronCommands::Run(run_args) => run_job(run_args),
    }
}

fn list_jobs() -> Result<()> {
    let config = Config::load()?;

    if config.cron.jobs.is_empty() {
        println!("No cron jobs configured.");
        println!();
        println!("Add a job with: localgpt cron add <schedule> <prompt>");
        return Ok(());
    }

    println!("Configured Cron Jobs");
    println!("====================");
    println!();

    for job in &config.cron.jobs {
        let status = if job.enabled { "✓" } else { "✗" };
        let channel = job.channel.as_deref().unwrap_or("none");
        println!(
            "  [{}] {} ({})",
            status,
            job.name,
            if job.enabled { "enabled" } else { "disabled" }
        );
        println!("      Schedule: {}", job.schedule);
        println!("      Timeout:  {}", job.timeout);
        println!("      Channel:  {}", channel);
        println!("      Prompt:   {}", truncate(&job.prompt, 60));
        println!();
    }

    Ok(())
}

fn add_job(args: AddArgs) -> Result<()> {
    let mut config = Config::load()?;

    let name = args
        .name
        .unwrap_or_else(|| format!("job-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S")));

    // Check for duplicate name
    if config.cron.jobs.iter().any(|j| j.name == name) {
        bail!(
            "A cron job named '{}' already exists. Use a different name with --name.",
            name
        );
    }

    let job = localgpt_core::config::CronJob {
        name: name.clone(),
        schedule: args.schedule,
        prompt: args.prompt,
        channel: args.channel,
        enabled: true,
        timeout: args.timeout,
        mcp_servers: Vec::new(),
    };

    config.cron.jobs.push(job);
    config.save()?;

    info!("Added cron job: {}", name);
    println!("Added cron job: {}", name);
    println!("Run 'localgpt cron list' to see all jobs.");

    Ok(())
}

fn remove_job(args: RemoveArgs) -> Result<()> {
    let mut config = Config::load()?;

    let idx = config
        .cron
        .jobs
        .iter()
        .position(|j| j.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("Cron job '{}' not found", args.name))?;

    if !args.force {
        println!("About to remove cron job: {}", args.name);
        println!("Prompt: {}", config.cron.jobs[idx].prompt);
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

    config.cron.jobs.remove(idx);
    config.save()?;

    info!("Removed cron job: {}", args.name);
    println!("Removed cron job: {}", args.name);

    Ok(())
}

fn enable_job(args: EnableArgs) -> Result<()> {
    let mut config = Config::load()?;

    let job = config
        .cron
        .jobs
        .iter_mut()
        .find(|j| j.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("Cron job '{}' not found", args.name))?;

    if job.enabled {
        println!("Cron job '{}' is already enabled.", args.name);
        return Ok(());
    }

    job.enabled = true;
    config.save()?;

    info!("Enabled cron job: {}", args.name);
    println!("Enabled cron job: {}", args.name);

    Ok(())
}

fn disable_job(args: DisableArgs) -> Result<()> {
    let mut config = Config::load()?;

    let job = config
        .cron
        .jobs
        .iter_mut()
        .find(|j| j.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("Cron job '{}' not found", args.name))?;

    if !job.enabled {
        println!("Cron job '{}' is already disabled.", args.name);
        return Ok(());
    }

    job.enabled = false;
    config.save()?;

    info!("Disabled cron job: {}", args.name);
    println!("Disabled cron job: {}", args.name);

    Ok(())
}

fn run_job(args: RunArgs) -> Result<()> {
    let config = Config::load()?;

    let job = config
        .cron
        .jobs
        .iter()
        .find(|j| j.name == args.name)
        .ok_or_else(|| anyhow::anyhow!("Cron job '{}' not found", args.name))?;

    println!("Running cron job: {}", job.name);
    println!("Prompt: {}", job.prompt);
    println!();
    println!("Note: This requires a running daemon. Use 'localgpt daemon start' first.");
    println!("The job will be picked up by the next heartbeat cycle.");

    // TODO: In the future, this could trigger the job immediately via the daemon's control plane

    Ok(())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..s.floor_char_boundary(max_len - 3)])
    }
}
