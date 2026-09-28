//! The LocalGPT desktop app — one window, every mode.
//!
//! Phase 1 of the one-app consolidation (docs/world-strategy.md §13.1): the
//! shell is Gen's — a Bevy window, an egui prompt panel, an agent loop, a
//! world viewport — with Markdown as the first second-class input: `--md
//! doc.md` compiles the document with MD's pure lib (the rule-derived draft
//! plus any ```world fences) and opens it as a world in the same viewport,
//! tour and all, where the full toolbelt still works on it.
//!
//! ```sh
//! cargo run -p localgpt-app                          # the shell
//! cargo run -p localgpt-app -- --md notes.md         # a document as a world
//! cargo run -p localgpt-app -- --md notes.md --no-author   # rules only, no model
//! cargo run -p localgpt-app -- --world place.json    # any world manifest
//! ```
//!
//! Verse (playback + worlds) and the assistant (chat + memory) are the next
//! modes; nothing here forecloses them. What this crate deliberately does NOT
//! do is duplicate Gen's entry path — everything heavy comes from the gen
//! lib, and this is the thin assembly.

mod authoring;
mod document;

use std::path::PathBuf;
use std::sync::Arc;

use bevy::prelude::*;
use localgpt_core::agent::Agent;
use localgpt_core::agent::tools::create_safe_tools;
use localgpt_core::memory::MemoryManager;
use localgpt_gen::desktop::{self, ChatEvent, PanelSettings};
use localgpt_gen::gen3d;

/// Gen's agent id. The app is Gen desktop's successor, so it reads and writes
/// the same memory index and session history rather than starting a parallel
/// one; when the gen binary retires, this is the only id left.
const AGENT_ID: &str = "gen";

#[derive(Debug, Default)]
struct Args {
    md: Option<PathBuf>,
    world: Option<String>,
    tools: Option<String>,
    /// Build documents from rules and fences only, never asking a model.
    no_author: bool,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--md" => args.md = iter.next().map(PathBuf::from),
            "--world" => args.world = iter.next(),
            "--tools" => args.tools = iter.next(),
            "--no-author" => args.no_author = true,
            other => {
                eprintln!(
                    "localgpt-app: unknown argument {other:?} (--md, --world, --tools, --no-author)"
                );
                std::process::exit(2);
            }
        }
    }
    args
}

/// Logging, before Bevy starts: Bevy's own LogPlugin is disabled (as in Gen),
/// so without this every warning and error from Gen's internals — a world that
/// failed to load, a missing model — would vanish. Launched from a terminal it
/// goes to stderr; from Finder there is no terminal, so it goes to
/// `<logs>/localgpt-app.log`, Gen's `gen-desktop.log` pattern. `RUST_LOG`
/// overrides the default of `warn`.
fn init_logging() {
    use std::io::IsTerminal;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let log_file = (!std::io::stderr().is_terminal())
        .then(|| {
            let dir = localgpt_core::paths::Paths::resolve().ok()?.logs_dir();
            std::fs::create_dir_all(&dir).ok()?;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("localgpt-app.log"))
                .ok()
        })
        .flatten();
    match log_file {
        Some(file) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(std::sync::Mutex::new(file))
            .init(),
        None => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init(),
    }
}

fn main() -> anyhow::Result<()> {
    let args = parse_args();
    init_logging();

    // Like Gen launched from Finder: the login shell's PATH, so CLI model
    // backends installed there are found.
    #[cfg(unix)]
    if let Some(login_path) = desktop::shell_env::login_shell_path() {
        let current = std::env::var("PATH").unwrap_or_default();
        let merged = desktop::shell_env::merge_paths(&current, &login_path);
        // SAFETY: program start, before threads.
        unsafe { std::env::set_var("PATH", merged) };
    }

    // Gen's in-memory config, not Config::load(): a desktop app must open on a
    // machine with nothing on disk and must not write the assistant's
    // config.toml or read its workspace. Same path as `localgpt-gen`.
    let config = localgpt_gen::config::gen_config();
    let workspace = config.workspace_path();

    // The initial world: an explicit --world arg, or a Markdown document
    // compiled in-process. Both land in the same place — GenInitialWorld —
    // and `--md x --world y` prefers the document (the more specific ask).
    // A failure to import falls back to the raw path so the reason shows up
    // in the window's log rather than killing the launch.
    let import_rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let import = |arg: &str| {
        import_rt
            .block_on(gen3d::world_import::prepare(arg, &workspace))
            .map(|w| w.dir.to_string_lossy().into_owned())
            .unwrap_or_else(|e| {
                eprintln!("localgpt-app: could not import {arg}: {e}");
                arg.to_string()
            })
    };
    // A document: MD's pipeline builds the first world (camera included — it
    // places the view once), and the live Document drives every rebuild after.
    let mut live_document = None;
    let initial_world = if let Some(md) = &args.md {
        let text = std::fs::read_to_string(md)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", md.display()))?;
        let (doc, world) = document::compile(&text, md);
        let dir = document::world_dir(&workspace, md);
        document::write_world(&world, &dir)?;
        eprintln!(
            "localgpt-app: {} -> {} ({} entities, {} sections)",
            md.display(),
            dir.display(),
            world.entities.len(),
            doc.sections.len(),
        );
        let mut live = document::Document::new(md.clone(), dir.clone(), text, &doc, &world);
        // The model authors each section's place in the background; the draft
        // above is on screen meanwhile, and each build replaces its section's
        // draft as it lands. Same model as the prompt panel.
        if !args.no_author {
            let manifest = localgpt_md::assets::read_manifest_from_disk();
            live = live.with_authoring(authoring::Worker::spawn(&config, manifest), &doc);
        }
        live_document = Some(live);
        Some(dir.to_string_lossy().into_owned())
    } else {
        args.world.as_ref().map(|world| import(world))
    };

    let (bridge, channels) = gen3d::create_gen_channels();
    let (panel_channels, agent_channels) = desktop::create_chat_channels();

    let tool_profile = match args.tools.as_deref() {
        Some("core") => gen3d::tool_profile::ToolProfile::Core,
        Some("standard") => gen3d::tool_profile::ToolProfile::Standard,
        Some("full") | None => gen3d::tool_profile::ToolProfile::Full,
        Some(other) => {
            eprintln!("localgpt-app: --tools {other:?} (core, standard, full)");
            std::process::exit(2);
        }
    };

    // The agent on a background thread: Bevy owns the main thread (macOS).
    let agent_config = config.clone();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                agent_channels
                    .sink
                    .send(ChatEvent::Failed(format!("no tokio runtime: {e}")));
                return;
            }
        };
        if let Err(e) = rt.block_on(run_agent(
            agent_config,
            bridge,
            agent_channels,
            tool_profile,
        )) {
            eprintln!("localgpt-app: agent stopped: {e:#}");
        }
    });

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "LocalGPT".into(),
                    resolution: bevy::window::WindowResolution::new(1440, 860),
                    present_mode: bevy::window::PresentMode::AutoVsync,
                    composite_alpha_mode: bevy::window::CompositeAlphaMode::Auto,
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
    gen3d::plugin::setup_gen_app(&mut app, channels, workspace, None);
    app.insert_resource(gen3d::plugin::GenInitialWorld {
        path: initial_world,
    });
    app.add_plugins(desktop::PromptPanelPlugin::new(
        panel_channels,
        PanelSettings {
            open: true,
            focus_input: true,
            settings_file: localgpt_gen::settings::settings_path(),
        },
    ));
    app.add_plugins(document::DocumentPlugin);
    if let Some(live) = live_document {
        app.insert_resource(live);
    }
    app.run();
    Ok(())
}

/// The panel-driven agent: one turn per prompt, everything else identical to
/// Gen's desktop loop in shape (safe tools + memory writes + the gen tools,
/// filtered by the tool profile). Streaming is folded into a single Delta —
/// per-token streaming is Gen's refinment to make, not this crate's to
/// duplicate.
async fn run_agent(
    config: localgpt_core::config::Config,
    bridge: Arc<gen3d::GenBridge>,
    mut panel: localgpt_gen::desktop::AgentChannels,
    profile: gen3d::tool_profile::ToolProfile,
) -> anyhow::Result<()> {
    // See Gen's run_agent_loop for why new_with_full_config: the agent must
    // stay in this workspace, not fall back to the assistant's.
    let memory = Arc::new(MemoryManager::new_with_full_config(
        &config.memory,
        Some(&config),
        AGENT_ID,
    )?);
    let mut tools = create_safe_tools(&config, Some(memory.clone()))?;
    tools.extend(localgpt_core::mcp::memory_tools::create_memory_write_tools(
        config.workspace_path(),
    ));
    tools.extend(gen3d::tools::create_gen_tools(bridge));
    let tools = gen3d::tool_profile::apply_tool_profile(tools, profile);

    let mut agent = Agent::new_with_tools(config.clone(), AGENT_ID, memory, tools)?;
    panel.sink.send(ChatEvent::Ready {
        model: config.agent.default_model.clone(),
    });

    while let Some(prompt) = panel.prompt_rx.recv().await {
        if prompt.trim().is_empty() {
            continue;
        }
        panel.sink.send(ChatEvent::Prompt {
            text: prompt.clone(),
            from: None,
        });
        match agent.chat_saving_session(&prompt, AGENT_ID).await {
            Ok(reply) => {
                if !reply.is_empty() {
                    panel.sink.send(ChatEvent::Delta(reply));
                }
                panel.sink.send(ChatEvent::TurnFinished { error: None });
            }
            Err(e) => {
                panel.sink.send(ChatEvent::TurnFinished {
                    error: Some(format!("{e:#}")),
                });
            }
        }
    }
    Ok(())
}
