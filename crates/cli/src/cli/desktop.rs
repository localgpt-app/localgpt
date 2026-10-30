//! Desktop GUI launch command

use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct DesktopArgs {
    // No additional args needed for now
    // Could add --theme, --size, etc. in future
}

pub fn run(_args: DesktopArgs, agent_id: &str) -> Result<()> {
    use crate::desktop::DesktopApp;

    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0])
            .with_title("LocalGPT"),
        ..Default::default()
    };

    let agent_id = agent_id.to_string();

    eframe::run_native(
        "LocalGPT",
        native_options,
        Box::new(move |cc| Ok(Box::new(DesktopApp::new(cc, Some(agent_id.clone()))))),
    )
    .map_err(|e| anyhow::anyhow!("Failed to run desktop app: {}", e))
}
