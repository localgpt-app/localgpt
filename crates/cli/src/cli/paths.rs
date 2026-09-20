//! CLI subcommand: `localgpt paths`
//!
//! Prints all resolved XDG-compliant paths for debugging and scripting.

use anyhow::Result;

use localgpt_core::paths::Paths;

pub fn run() -> Result<()> {
    let paths = Paths::resolve()?;

    println!("LocalGPT Paths (XDG Base Directory)");
    println!("====================================");
    println!();
    println!("Config:     {}", paths.config_dir.display());
    println!("  config.toml:    {}", paths.config_file().display());
    println!();
    println!("Data:       {}", paths.data_dir.display());
    println!("  workspace:      {}", paths.workspace.display());
    println!("  device key:     {}", paths.device_key().display());
    println!("  skills:         {}", paths.managed_skills_dir().display());
    println!();
    println!("State:      {}", paths.state_dir.display());
    println!("  sessions:       {}", paths.sessions_dir("main").display());
    println!("  logs:           {}", paths.logs_dir().display());
    println!();
    println!("Cache:      {}", paths.cache_dir.display());
    println!("  search index:   {}", paths.search_index("main").display());
    println!(
        "  embeddings:     {}",
        paths.embedding_cache_dir().display()
    );
    println!();
    match paths.runtime_dir {
        Some(ref dir) => {
            println!("Runtime:    {}", dir.display());
            println!("  PID file:       {}", paths.pid_file().display());
            println!("  workspace lock: {}", paths.workspace_lock().display());
        }
        None => {
            println!("Runtime:    (not available)");
            println!("  PID file:       {}", paths.pid_file().display());
            println!("  workspace lock: {}", paths.workspace_lock().display());
        }
    }

    Ok(())
}
