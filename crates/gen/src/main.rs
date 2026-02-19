//! LocalGPT Gen — AI-driven 3D scene generation binary.
//!
//! This binary runs Bevy on the main thread (required for macOS windowing/GPU)
//! and spawns the LLM agent loop on a background tokio runtime.

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

mod gen3d;

#[derive(Parser)]
#[command(name = "localgpt-gen")]
#[command(about = "LocalGPT Gen — AI-driven 3D scene generation")]
struct Cli {
    /// Initial prompt to send (optional — starts interactive if omitted)
    prompt: Option<String>,

    /// Agent ID to use
    #[arg(short, long, default_value = "gen")]
    agent: String,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Load a glTF/GLB scene at startup
    #[arg(short = 's', long)]
    scene: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging before handing off to Bevy
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .init();

    // Load config early so both Bevy and agent threads can use it
    let config = localgpt_core::config::Config::load()?;
    let workspace = config.workspace_path();

    // Resolve initial scene path if provided
    let initial_scene = cli
        .scene
        .as_ref()
        .and_then(|path| gen3d::plugin::resolve_gltf_path(path, &workspace));

    // Create the channel pair
    let (bridge, channels) = gen3d::create_gen_channels();

    // Clone values for the background thread
    let agent_id = cli.agent;
    let initial_prompt = cli.prompt;
    let bridge_for_agent = bridge.clone();

    // Spawn tokio runtime + agent loop on a background thread
    // (Bevy must own the main thread for windowing/GPU on macOS)
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to build tokio runtime for gen agent");

        rt.block_on(async move {
            if let Err(e) =
                run_agent_loop(bridge_for_agent, &agent_id, initial_prompt, config).await
            {
                tracing::error!("Gen agent loop error: {}", e);
            }
        });
    });

    // Run Bevy on the main thread
    run_bevy_app(channels, workspace, initial_scene)
}

/// Set up and run the Bevy application on the main thread.
fn run_bevy_app(
    channels: gen3d::GenChannels,
    workspace: std::path::PathBuf,
    initial_scene: Option<PathBuf>,
) -> Result<()> {
    use bevy::prelude::*;

    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "LocalGPT Gen".into(),
                    resolution: bevy::window::WindowResolution::new(1280, 720),
                    ..default()
                }),
                ..default()
            })
            .set(bevy::asset::AssetPlugin {
                file_path: "/".to_string(),
                ..default()
            })
            .disable::<bevy::log::LogPlugin>(),
    );

    gen3d::plugin::setup_gen_app(&mut app, channels, workspace, initial_scene);

    app.run();

    Ok(())
}

/// Run the interactive agent loop with Gen tools available.
async fn run_agent_loop(
    bridge: std::sync::Arc<gen3d::GenBridge>,
    agent_id: &str,
    initial_prompt: Option<String>,
    config: localgpt_core::config::Config,
) -> Result<()> {
    use localgpt_core::agent::Agent;
    use localgpt_core::agent::tools::create_safe_tools;
    use localgpt_core::memory::MemoryManager;
    use rustyline::DefaultEditor;
    use rustyline::error::ReadlineError;
    use std::sync::Arc;

    // Set up memory
    let memory = MemoryManager::new_with_agent(&config.memory, agent_id)?;
    let memory = Arc::new(memory);

    // Create safe tools + gen tools
    let mut tools = create_safe_tools(&config, Some(memory.clone()))?;
    tools.extend(gen3d::tools::create_gen_tools(bridge));

    // Create agent with combined tools
    let mut agent = Agent::new_with_tools(config.clone(), agent_id, memory, tools)?;
    agent.new_session().await?;

    // If initial prompt given, send it
    if let Some(prompt) = initial_prompt {
        println!("\n> {}", prompt);
        let response = agent.chat(&prompt).await?;
        println!("\n{}\n", response);
    }

    // Interactive loop
    let mut rl = DefaultEditor::new()?;
    loop {
        let readline = rl.readline("> ");

        let input = match readline {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                break; // Ctrl+D
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        };

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        // Add to history
        let _ = rl.add_history_entry(input);

        if input == "/quit" || input == "/exit" || input == "/q" {
            break;
        }

        let response = agent.chat(input).await?;
        println!("\n{}\n", response);
    }

    Ok(())
}
