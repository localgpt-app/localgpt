use anyhow::Result;
use clap::Args;
use localgpt_core::paths::Paths;
use localgpt_core::security::ensure_device_key;

#[derive(Args)]
pub struct InitArgs {}

pub fn run(_args: InitArgs) -> Result<()> {
    let paths = Paths::resolve()?;

    // Ensure directories exist
    paths.ensure_dirs()?;

    // Generate device key in data directory
    ensure_device_key(&paths.data_dir)?;

    println!("Initialized LocalGPT configuration.");
    println!("  Data: {}", paths.data_dir.display());
    println!("  State: {}", paths.state_dir.display());
    println!("  Device Key: {}", paths.device_key().display());

    Ok(())
}
