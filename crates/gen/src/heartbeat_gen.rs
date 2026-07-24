//! Gen-aware heartbeat integration.
//!
//! Provides a ToolFactory that creates gen tools for a headless Bevy instance,
//! allowing the heartbeat runner to process gen experiments from HEARTBEAT.md.
//!
//! The factory:
//! 1. Creates GenChannels (bridge between agent and Bevy)
//! 2. Boots headless Bevy on a dedicated thread
//! 3. Returns all gen tool sets pointing at the headless bridge
//! 4. Bevy thread shuts down when tools/bridge are dropped

use std::sync::Arc;

use localgpt_core::agent::tools::Tool;
use localgpt_core::config::Config;
use localgpt_core::heartbeat::ToolFactory;

use crate::gen3d;

/// Boot a headless Bevy instance and return gen tools connected to it.
///
/// Shared implementation for both factory functions.
fn boot_headless_and_create_tools(config: &Config) -> anyhow::Result<Vec<Box<dyn Tool>>> {
    let (bridge, channels) = gen3d::create_gen_channels();

    let workspace = config.workspace_path();
    let completion_flag = gen3d::headless::HeadlessCompletionFlag::default();
    let flag = completion_flag.clone();

    std::thread::spawn(move || {
        use bevy::prelude::*;

        let mut app = App::new();

        app.add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    ..default()
                })
                .set(bevy::render::RenderPlugin {
                    render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                        bevy::render::settings::WgpuSettings {
                            backends: Some(bevy::render::settings::Backends::all()),
                            ..default()
                        },
                    )),
                    ..default()
                })
                .set(bevy::asset::AssetPlugin {
                    file_path: "/".to_string(),
                    ..default()
                })
                .disable::<bevy::log::LogPlugin>(),
        );

        app.insert_resource(flag);
        app.add_systems(Update, gen3d::headless::headless_completion_detector);

        gen3d::plugin::setup_gen_app(&mut app, channels, workspace, None);

        app.run();
    });

    // Wait for Bevy to initialize
    // TODO(C3): Replace with channel-based readiness signal
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Create all gen tools pointing at the headless bridge
    let mut tools: Vec<Box<dyn Tool>> = Vec::new();
    tools.extend(gen3d::tools::create_gen_tools(bridge.clone()));
    tools.extend(crate::mcp::avatar_tools::create_character_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::interaction_tools::create_interaction_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::terrain_tools::create_terrain_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::ui_tools::create_ui_tools(bridge.clone()));
    tools.extend(crate::mcp::physics_tools::create_physics_tools(
        bridge.clone(),
    ));
    tools.extend(crate::mcp::multifile_tools::create_multifile_tools(
        bridge.clone(),
    ));

    Ok(tools)
}

/// Create a ToolFactory that provides gen tools backed by a headless Bevy instance.
///
/// This is passed to `HeartbeatRunner::new_with_gate_and_tools()` so the heartbeat
/// can dispatch gen experiments.
pub fn create_headless_gen_tool_factory() -> ToolFactory {
    Box::new(|config: &Config| -> anyhow::Result<Vec<Box<dyn Tool>>> {
        let tools = boot_headless_and_create_tools(config)?;
        Ok(tools)
    })
}

/// Create a ToolFactory that provides gen tools + experiment queue tools.
///
/// This is the full factory for heartbeat-driven gen experiments.
pub fn create_full_gen_tool_factory() -> ToolFactory {
    Box::new(|config: &Config| -> anyhow::Result<Vec<Box<dyn Tool>>> {
        let mut tools = boot_headless_and_create_tools(config)?;

        let tracker = Arc::new(crate::experiment::ExperimentTracker::new(
            &config.paths.state_dir,
        ));
        tools.extend(crate::mcp::experiment_tools::create_experiment_tools(
            tracker,
        ));

        Ok(tools)
    })
}
