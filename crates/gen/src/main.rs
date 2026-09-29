//! LocalGPT Gen — AI-driven 3D scene generation binary.
//!
//! This binary runs Bevy on the main thread (required for macOS windowing/GPU)
//! and spawns the LLM agent loop on a background tokio runtime.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

// Use library modules
use localgpt_gen::agent_loop::{
    AgentNetHooksOpt, run_agent_loop, run_mcp_stdio_relay, streaming_chat,
};
use localgpt_gen::character_tools;
use localgpt_gen::desktop::{ChatEvent, PanelChannels, PanelSettings};
use localgpt_gen::gen3d;
use localgpt_gen::mcp_server;

#[derive(Parser)]
#[command(name = "localgpt-gen")]
#[command(about = "LocalGPT Gen — AI-driven 3D scene generation")]
struct Cli {
    #[command(subcommand)]
    command: Option<GenSubcommand>,

    /// Initial prompt (interactive mode only)
    prompt: Option<String>,

    /// Agent ID to use
    #[arg(short, long, global = true, default_value = "gen")]
    agent: String,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Load a glTF/GLB scene at startup
    #[arg(short = 's', long, global = true)]
    scene: Option<String>,

    /// Open a LocalGPT world at startup: a world folder, a world name from
    /// {workspace}/skills/, a .json or .ron manifest, or an http(s) URL to one
    /// (for example a world on localgpt.world). Manifests and URLs are
    /// imported, with their assets, into {workspace}/skills/<name>/ first.
    #[arg(long, global = true, value_name = "PATH|URL")]
    world: Option<String>,

    /// Tool profile for the generation agent: "core" keeps only the 22
    /// scene-editing essentials (~3.8k tokens of schema, for local models
    /// with small context windows — the full toolbelt is 77 gen tools,
    /// ~14k tokens per request), "standard" adds meshes/exports/audio/
    /// terrain and the WorldGen pipeline (~9.4k), "full" (default) keeps
    /// everything. Overrides [gen] tool_profile in config.toml.
    #[arg(long, global = true, value_name = "core|standard|full")]
    tools: Option<String>,

    /// Enable MCP relay server for external MCP clients.
    /// Auto-enabled when using claude-cli/* models.
    #[arg(long, global = true)]
    mcp_relay: bool,

    /// Run as a desktop app: type prompts in a panel inside the window
    /// instead of the terminal. Automatic when Gen isn't started from a
    /// terminal (for example, from the macOS app bundle).
    #[arg(long)]
    desktop: bool,

    /// Host a collaborative session on the LAN (listen server + mDNS
    /// announcement). Others join as read-only viewers with --join.
    #[cfg(feature = "multiplayer")]
    #[arg(long, group = "net_mode")]
    host: bool,

    /// Session name shown in mDNS discovery (with --host).
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host")]
    session_name: Option<String>,

    /// Join a collaborative session. Pass the host address (host:port or
    /// bare host — default port 9879), or omit the value to discover
    /// sessions on the LAN via mDNS.
    #[cfg(feature = "multiplayer")]
    #[arg(long, num_args = 0..=1, default_missing_value = None, group = "net_mode")]
    join: Option<Option<String>>,

    /// Port for hosted sessions (with --host).
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host", default_value_t = 9879)]
    port: u16,

    /// Session PIN shown on the host's console (with --join). Prompted for
    /// if the host requires pairing and this is omitted.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "join")]
    pin: Option<String>,

    /// Host an OPEN session: no PIN, public netcode key — anyone on the LAN
    /// with localgpt-gen can join (with --host). Trusted networks only.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host")]
    open: bool,

    /// What connected clients' prompts may do (with --host). `safe`
    /// (default): a separate agent with scene-editing tools only — no
    /// shell, files, memory, web, or disk writes. `full`: the host's own
    /// agent with all of its tools, including shell access.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host", value_enum, default_value_t = RemoteTools::Safe)]
    remote_tools: RemoteTools,

    /// Let browsers join as guests (with --host): serve a join page and a
    /// WebSocket endpoint on the session port, and print an invite link.
    /// Guests watch, walk, chat and prompt the room's AI; they can't edit
    /// directly.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host")]
    web: bool,

    /// Let browser guests EDIT directly, not just prompt (with --web):
    /// they join as editors and can select, move, rotate, scale and
    /// delete entities from the join page.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "web")]
    web_edit: bool,

    /// Reach guests beyond the LAN through a relay (with --host): the
    /// relay's URL (http(s)://host:port of a localgpt-relay). Prints an
    /// internet invite link.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host")]
    relay: Option<String>,

    /// Replay a previous web session's op log before guests join (with
    /// --host --web). Pass a session name (reads
    /// <workspace>/sessions/<name>/ops.jsonl) or a path to an ops.jsonl.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "web")]
    resume: Option<String>,

    /// Replay an op log as a time-lapse in the window: the world rebuilds
    /// itself batch by batch. No agent, no session. Pass an ops.jsonl path.
    #[arg(long)]
    replay: Option<String>,

    /// Replay pace in batches per second (with --replay).
    #[arg(long, requires = "replay", default_value_t = 4.0)]
    replay_speed: f32,
}

/// Tool access for prompts from collaborative clients (`--remote-tools`).
#[cfg(feature = "multiplayer")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum RemoteTools {
    Safe,
    Full,
}

#[derive(Subcommand)]
enum GenSubcommand {
    /// Run as MCP server (stdio) — Bevy window + gen tools over MCP
    McpServer {
        /// Run headless (no window) — for CI or batch generation via MCP
        #[arg(long)]
        headless: bool,

        /// Connect to an existing gen process's MCP relay instead of starting Bevy.
        /// Pass the relay port (e.g., 9878) or omit to auto-discover.
        #[arg(long)]
        connect: Option<Option<u16>>,

        /// Also start a streamable HTTP MCP server on this port.
        /// External tools can POST JSON-RPC to http://127.0.0.1:<port>/mcp.
        #[arg(long)]
        mcp_http: Option<u16>,
    },
    /// Control an external avatar (headless, no Bevy window)
    Control {
        /// URL of the external app
        url: String,
        /// Initial prompt
        prompt: Option<String>,
    },
    /// Headless generation — generate a world without opening a window
    Headless {
        /// Generation prompt (required)
        #[arg(long)]
        prompt: String,

        /// Output world skill directory (default: auto-named in workspace/skills/)
        #[arg(long)]
        output: Option<String>,

        /// Capture a thumbnail after generation (default: true)
        #[arg(long, default_value = "true")]
        screenshot: bool,

        /// Screenshot width in pixels
        #[arg(long, default_value = "1280")]
        screenshot_width: u32,

        /// Screenshot height in pixels
        #[arg(long, default_value = "720")]
        screenshot_height: u32,

        /// Max generation time before abort (seconds, default: 300)
        #[arg(long, default_value = "300")]
        timeout: u64,

        /// Override LLM model for this run
        #[arg(long)]
        model: Option<String>,

        /// Style hint prepended to prompt
        #[arg(long)]
        style: Option<String>,
    },
}

fn main() -> Result<()> {
    // On Linux, default to X11 (XWayland) to avoid wgpu "Invalid surface" errors
    // on Wayland compositors. winit 0.29+ selects Wayland when WAYLAND_DISPLAY is set.
    // Users can set LOCALGPT_WAYLAND=1 to keep native Wayland.
    #[cfg(target_os = "linux")]
    if std::env::var("LOCALGPT_WAYLAND").is_err() {
        // SAFETY: called at program start before any threads are spawned.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }

    let cli = Cli::parse();

    // `gguf/<name>` models run in-process (see src/local_llm.rs); register
    // the provider before any agent is created.
    #[cfg(feature = "local-llm")]
    localgpt_gen::local_llm::register();

    // Desktop mode: the interactive app (or a --join viewer) without a
    // terminal to type in, so prompts come from a panel in the window.
    // Automatic when stdin isn't a terminal (a double-click, the macOS app
    // bundle, or a viewer launched from the panel's Join); --desktop forces it.
    let launched_from_terminal = std::io::stdin().is_terminal();
    let desktop = cli.command.is_none() && (cli.desktop || !launched_from_terminal);
    if cli.desktop && cli.command.is_some() {
        anyhow::bail!("--desktop only applies to interactive mode (no subcommand)");
    }

    // Apps opened from Finder inherit launchd's minimal PATH, which hides CLI
    // backends (claude, gemini, codex) installed with Homebrew, npm, or into
    // ~/.local/bin. Adopt the login shell's PATH before anything spawns them.
    // A failure is logged once logging is set up, below.
    #[cfg(unix)]
    let login_path_failed = desktop
        && !launched_from_terminal
        && match localgpt_gen::desktop::shell_env::login_shell_path() {
            Some(login_path) => {
                let current = std::env::var("PATH").unwrap_or_default();
                let merged = localgpt_gen::desktop::shell_env::merge_paths(&current, &login_path);
                // SAFETY: called at program start before any threads are spawned.
                unsafe { std::env::set_var("PATH", merged) };
                false
            }
            None => true,
        };
    #[cfg(not(unix))]
    let login_path_failed = false;

    // Initialize logging before handing off to Bevy.
    // Use "warn" by default for cleaner interactive TUI, "debug" with --verbose.
    //
    // For the interactive Bevy+REPL mode (no subcommand), we grab a rustyline
    // editor up front so its ExternalPrinter can route tracing output through
    // the REPL — async warnings (Bevy render, Ollama, etc.) no longer clobber
    // the `You:` prompt mid-typing. Other subcommands (headless, mcp-server,
    // control) have no REPL to protect, so they log straight to stderr.
    // Desktop mode has no terminal at all, so it logs to a file.
    let log_level = if cli.verbose { "debug" } else { "warn" };
    let mut repl_editor: Option<rustyline::DefaultEditor> = None;
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    if desktop {
        match open_desktop_log() {
            Some(file) => tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .init(),
            None => tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(std::io::stderr)
                .init(),
        }
    } else if cli.command.is_none() {
        let wired = rustyline::DefaultEditor::new()
            .ok()
            .and_then(|mut ed| ed.create_external_printer().ok().map(|p| (ed, p)));
        if let Some((ed, printer)) = wired {
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(localgpt_gen::tracing_printer::SharedPrinter::new(printer))
                .init();
            repl_editor = Some(ed);
        } else {
            // No tty / headless stdin — fall back to stderr.
            tracing_subscriber::fmt()
                .with_env_filter(env_filter)
                .with_writer(std::io::stderr)
                .init();
        }
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_writer(std::io::stderr)
            .init();
    }

    if login_path_failed {
        tracing::warn!(
            "Couldn't read your login shell's PATH; CLI backends installed outside the \
             system PATH may not be found"
        );
    }

    // Gen's configuration is built in memory from its own settings — no
    // config.toml is read, and none is created.
    let (config, settings) = localgpt_gen::config::gen_config_and_settings();
    let workspace = config.workspace_path();

    // Tool profile: --tools flag > Gen's settings > full.
    let tool_profile = gen3d::tool_profile::ToolProfile::resolve(
        cli.tools.as_deref(),
        settings.tool_profile.as_deref(),
    )?;

    // --replay: time-lapse an op log in the window — no agent, no session.
    if let Some(replay_path) = cli.replay.as_deref() {
        let entries = gen3d::replay::load_op_log(replay_path)?;
        eprintln!(
            "[replay] {} batches from {} ({}x speed)",
            entries.len(),
            replay_path,
            cli.replay_speed
        );
        let (_bridge, channels) = gen3d::create_gen_channels();
        return run_bevy_app(
            channels,
            workspace,
            None,
            None,
            None,
            Some((entries, cli.replay_speed)),
        );
    }

    // Multiplayer client mode (--join): slim viewer app, no gen subsystems.
    #[cfg(feature = "multiplayer")]
    if cli.join.is_some() {
        if cli.command.is_some() {
            anyhow::bail!("--join cannot be combined with a subcommand");
        }
        return run_join_mode(&cli, desktop, &config);
    }
    #[cfg(feature = "multiplayer")]
    if cli.host && cli.command.is_some() {
        anyhow::bail!("--host cannot be combined with a subcommand");
    }

    // Dispatch based on subcommand
    match cli.command {
        Some(GenSubcommand::Control { url, prompt }) => {
            // Headless bridge mode — no Bevy window
            tracing::info!("Starting Gen in CONTROL mode (headless) -> {}", url);
            let agent_id = cli.agent;
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build tokio runtime");

            rt.block_on(
                async move { run_headless_control_loop(&url, &agent_id, prompt, config).await },
            )
        }

        Some(GenSubcommand::Headless {
            prompt,
            output,
            screenshot,
            screenshot_width,
            screenshot_height,
            timeout,
            model,
            style,
        }) => {
            // Headless generation mode — no window, generate and exit
            let headless_config = gen3d::headless::HeadlessConfig {
                prompt,
                output,
                screenshot,
                screenshot_width,
                screenshot_height,
                timeout_secs: timeout,
                agent_id: cli.agent,
                model,
                style,
            };

            tracing::info!("Starting headless generation: {:?}", headless_config.prompt);

            let (bridge, channels) = gen3d::create_gen_channels();
            let completion_flag = gen3d::headless::HeadlessCompletionFlag::default();
            let flag_for_agent = completion_flag.clone();
            let flag_for_timeout = completion_flag.clone();
            let agent_config = config.clone();

            // Spawn timeout watchdog
            let timeout_secs = headless_config.timeout_secs;
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(timeout_secs));
                if !flag_for_timeout.is_done() {
                    tracing::error!("Headless generation timed out after {}s", timeout_secs);
                    flag_for_timeout.complete_failure();
                }
            });

            // Spawn agent loop on background thread
            let bridge_for_agent = bridge.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime for headless gen");

                rt.block_on(async move {
                    match run_headless_agent(
                        bridge_for_agent,
                        headless_config,
                        agent_config,
                        tool_profile,
                    )
                    .await
                    {
                        Ok(()) => flag_for_agent.complete_success(),
                        Err(e) => {
                            tracing::error!("Headless generation failed: {}", e);
                            flag_for_agent.complete_failure();
                        }
                    }
                });
            });

            // Run headless Bevy on the main thread
            let result = run_headless_bevy_app(channels, workspace, completion_flag.clone(), None);

            // Map exit code based on success/failure
            if !completion_flag.is_success() {
                std::process::exit(1);
            }

            result
        }

        Some(GenSubcommand::McpServer {
            headless,
            connect,
            mcp_http,
        }) => {
            // --connect mode: relay stdio MCP to an existing gen process's TCP relay
            if let Some(port_opt) = connect {
                let port = port_opt
                    .or_else(gen3d::mcp_relay::read_relay_port)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No relay port specified and no running gen process found.\n\
                             Start localgpt-gen first, or pass --connect <port>."
                        )
                    })?;

                tracing::info!("Connecting to existing gen process relay on port {}", port);

                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime");

                return rt.block_on(run_mcp_stdio_relay(port));
            }

            // MCP server mode: Bevy on main thread, MCP stdio server on background thread
            let initial_scene = if headless {
                None
            } else {
                cli.scene
                    .as_ref()
                    .and_then(|path| gen3d::plugin::resolve_gltf_path(path, &workspace))
            };
            let initial_world = startup_world(cli.world.as_deref(), &workspace)?;

            let (bridge, channels) = gen3d::create_gen_channels();
            let bridge_for_mcp = bridge.clone();
            let mcp_config = config.clone();

            // Optionally start the MCP HTTP server on a separate thread
            if let Some(http_port) = mcp_http {
                let bridge_for_http = bridge.clone();
                let http_config = config.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to build tokio runtime for MCP HTTP server");

                    rt.block_on(async move {
                        if let Err(e) =
                            mcp_server::run_mcp_http_server(bridge_for_http, http_config, http_port)
                                .await
                        {
                            tracing::error!("MCP HTTP server error: {}", e);
                        }
                    });
                });
            }

            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime for MCP server");

                rt.block_on(async move {
                    if let Err(e) = mcp_server::run_mcp_server(bridge_for_mcp, mcp_config).await {
                        tracing::error!("MCP server error: {}", e);
                    }
                    // MCP client disconnected — exit the process
                    std::process::exit(0);
                });
            });

            // Run Bevy on the main thread (headless or windowed)
            if headless {
                let completion_flag = gen3d::headless::HeadlessCompletionFlag::default();
                run_headless_bevy_app(channels, workspace, completion_flag, initial_world)
            } else {
                run_bevy_app(
                    channels,
                    workspace,
                    initial_scene,
                    initial_world,
                    None,
                    None,
                )
            }
        }

        None => {
            // Interactive mode (default)
            let initial_scene = cli
                .scene
                .as_ref()
                .and_then(|path| gen3d::plugin::resolve_gltf_path(path, &workspace));
            let initial_world = startup_world(cli.world.as_deref(), &workspace)?;

            let (bridge, channels) = gen3d::create_gen_channels();
            let agent_id = cli.agent;
            let initial_prompt = cli.prompt;
            let bridge_for_agent = bridge.clone();
            let bridge_for_relay = bridge.clone();
            let relay_config = config.clone();

            // The in-window prompt panel: open at startup in desktop mode,
            // one F2 away otherwise.
            let (panel_channels, agent_channels) = localgpt_gen::desktop::create_chat_channels();
            let panel = Some((
                panel_channels,
                PanelSettings {
                    open: desktop,
                    focus_input: desktop,
                    settings_file: localgpt_gen::settings::settings_path(),
                },
            ));

            // The host plugin is always installed so a session can be started
            // from the prompt panel at any time; `--host` just starts one
            // before the first frame. Until then it stays dormant.
            #[cfg(feature = "multiplayer")]
            let (host_options, agent_net): (
                localgpt_gen::net::host::NetHostOptions,
                AgentNetHooksOpt,
            ) = {
                let (mut opts, hooks) = localgpt_gen::net::host::create_host_channels();
                opts.session_name = cli
                    .session_name
                    .clone()
                    .unwrap_or_else(localgpt_gen::net::default_session_name);
                opts.port = cli.port;
                opts.open = cli.open;
                opts.full_access = cli.host && cli.remote_tools == RemoteTools::Full;
                opts.autostart = cli.host;
                opts.web = cli.web;
                opts.web_edit = cli.web_edit;
                opts.resume = cli.resume.clone();
                opts.relay = cli.relay.clone();
                if opts.full_access {
                    eprintln!(
                        "WARNING: --remote-tools full — connected clients' prompts run with this \
                         agent's full tool access, including shell commands on this machine."
                    );
                }
                (opts, Some(hooks))
            };
            #[cfg(not(feature = "multiplayer"))]
            let agent_net: AgentNetHooksOpt = None;

            // Enable MCP relay when explicitly requested or when using a CLI backend
            // (claude-cli, gemini-cli, codex-cli spawn subprocesses that need MCP access)
            let enable_relay = cli.mcp_relay || localgpt_gen::agent_loop::uses_cli_backend(&config);

            // Spawn tokio runtime + agent loop + MCP relay on a background thread
            // (Bevy must own the main thread for windowing/GPU on macOS).
            // Move the REPL editor into the agent thread — it was created up
            // front so its ExternalPrinter could be wired into tracing.
            let editor_for_agent = repl_editor.take();
            let failure_sink = agent_channels.sink.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime for gen agent");

                let outcome = rt.block_on(async move {
                    if enable_relay {
                        // Start the MCP relay server so external CLI tools (claude, codex, gemini)
                        // can connect to the existing Bevy window instead of spawning a new one.
                        match gen3d::mcp_relay::start_mcp_relay(
                            bridge_for_relay,
                            &relay_config,
                        )
                        .await
                        {
                            Ok(port) => {
                                eprintln!(
                                    "MCP relay active on port {} (external MCP clients can connect to this window)",
                                    port
                                );
                            }
                            Err(e) => {
                                tracing::warn!("MCP relay failed to start: {} (external MCP clients won't be able to connect)", e);
                            }
                        }
                    }

                    run_agent_loop(
                        bridge_for_agent,
                        &agent_id,
                        initial_prompt,
                        relay_config,
                        editor_for_agent,
                        agent_net,
                        agent_channels,
                        desktop,
                        tool_profile,
                    )
                    .await
                });

                if let Err(e) = outcome {
                    tracing::error!("Gen agent loop error: {}", e);
                    if desktop {
                        // No terminal to print to: say what went wrong in the
                        // panel and leave the window open to read it.
                        eprintln!("Gen agent loop error: {e}");
                        failure_sink.send(ChatEvent::Failed(format!("{e:#}")));
                        return;
                    }
                }

                // REPL exited (/quit, Ctrl+D, or error). The Bevy window is
                // still blocking the main thread, so the process would hang
                // here without an explicit exit. Clean up the relay port
                // (duplicated from the main-thread cleanup below because we
                // won't reach it) and terminate.
                if enable_relay {
                    gen3d::mcp_relay::cleanup_relay_port();
                }
                std::process::exit(0);
            });

            // Run Bevy on the main thread (blocks until window closes)
            #[cfg(feature = "multiplayer")]
            let result = run_host_bevy_app(
                channels,
                workspace,
                initial_scene,
                initial_world,
                host_options,
                panel,
            );
            #[cfg(not(feature = "multiplayer"))]
            let result = run_bevy_app(
                channels,
                workspace,
                initial_scene,
                initial_world,
                panel,
                None,
            );

            // Clean up relay port file so stale ports aren't discovered
            if enable_relay {
                gen3d::mcp_relay::cleanup_relay_port();
            }

            result
        }
    }
}

/// The prompt panel's channels and startup settings, when the app has one.
type PanelSetup = Option<(PanelChannels, PanelSettings)>;

fn add_prompt_panel(app: &mut bevy::prelude::App, panel: PanelSetup) {
    if let Some((channels, settings)) = panel {
        app.add_plugins(localgpt_gen::desktop::PromptPanelPlugin::new(
            channels, settings,
        ));
    }
}

/// `--world`: the world folder to open on the first frame, importing a loose
/// manifest or a URL into the workspace first (see `gen3d::world_import`).
fn startup_world(arg: Option<&str>, workspace: &Path) -> Result<Option<String>> {
    use gen3d::world_import::Outcome;

    let Some(arg) = arg else {
        return Ok(None);
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let world = rt.block_on(gen3d::world_import::prepare(arg, workspace))?;
    match &world.outcome {
        Outcome::Opened => {}
        Outcome::Imported { assets, missing } => {
            eprintln!(
                "Imported {arg} into {} ({assets} asset{})",
                world.dir.display(),
                if *assets == 1 { "" } else { "s" }
            );
            for asset in missing {
                eprintln!("  missing asset: {asset}");
            }
        }
        Outcome::AlreadyImported => eprintln!(
            "Opening {} — imported earlier; delete it to import {arg} again",
            world.dir.display()
        ),
    }
    Ok(Some(world.dir.to_string_lossy().into_owned()))
}

/// Set up and run the Bevy application on the main thread.
fn run_bevy_app(
    channels: gen3d::GenChannels,
    workspace: std::path::PathBuf,
    initial_scene: Option<PathBuf>,
    initial_world: Option<String>,
    panel: PanelSetup,
    replay: Option<(
        std::collections::VecDeque<localgpt_world_sync::OpLogEntry>,
        f32,
    )>,
) -> Result<()> {
    use bevy::prelude::*;

    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "LocalGPT Gen".into(),
                    resolution: bevy::window::WindowResolution::new(1280, 720),
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

    gen3d::plugin::setup_gen_app(&mut app, channels, workspace, initial_scene);
    app.insert_resource(gen3d::plugin::GenInitialWorld {
        path: initial_world,
    });
    if let Some((entries, speed)) = replay {
        gen3d::replay::setup_replay(&mut app, entries, speed);
    }
    add_prompt_panel(&mut app, panel);

    app.run();

    Ok(())
}

/// Set up and run the host Bevy application: the full gen app plus the
/// listen-server plugin (authoritative ECS + replication + mDNS).
#[cfg(feature = "multiplayer")]
fn run_host_bevy_app(
    channels: gen3d::GenChannels,
    workspace: std::path::PathBuf,
    initial_scene: Option<PathBuf>,
    initial_world: Option<String>,
    options: localgpt_gen::net::host::NetHostOptions,
    panel: PanelSetup,
) -> Result<()> {
    use bevy::prelude::*;

    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    // host_lifecycle retitles it once a session starts.
                    title: "LocalGPT Gen".into(),
                    resolution: bevy::window::WindowResolution::new(1280, 720),
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

    gen3d::plugin::setup_gen_app(&mut app, channels, workspace, initial_scene);
    app.insert_resource(gen3d::plugin::GenInitialWorld {
        path: initial_world,
    });
    app.add_plugins(localgpt_gen::net::host::NetHostPlugin {
        options: std::sync::Mutex::new(Some(options)),
    });
    add_prompt_panel(&mut app, panel);

    app.run();

    Ok(())
}

/// Run the ops client app: the full gen scene driven by the room over one
/// WebSocket (see `net/ops_client.rs`).
#[cfg(feature = "multiplayer")]
fn run_ops_client_app(
    server_addr: std::net::SocketAddr,
    pin: Option<String>,
    prompt_rx: tokio::sync::mpsc::UnboundedReceiver<String>,
    panel_prompt_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    workspace: std::path::PathBuf,
) -> Result<()> {
    use bevy::prelude::*;

    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "LocalGPT Gen — Client".into(),
                    resolution: bevy::window::WindowResolution::new(1280, 720),
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

    // The full gen scene runtime (camera, behaviors, audio) with no agent:
    // the room drives the scene through the ops applier.
    let (_bridge, channels) = gen3d::create_gen_channels();
    gen3d::plugin::setup_gen_app(&mut app, channels, workspace, None);
    app.add_plugins(localgpt_gen::net::ops_client::OpsClientPlugin {
        options: std::sync::Mutex::new(Some(localgpt_gen::net::ops_client::OpsClientOptions {
            server_addr,
            pin,
            name: guest_display_name(),
            prompt_rx,
        })),
    });
    if let Some(prompt_tx) = panel_prompt_tx {
        app.add_plugins(localgpt_gen::desktop::ClientPanelPlugin::new(
            prompt_tx,
            server_addr,
        ));
    }

    app.run();

    Ok(())
}

/// The guest's display name (env user, else "guest").
#[cfg(feature = "multiplayer")]
fn guest_display_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "guest".to_string())
}

/// What `/session-info` returns.
#[cfg(feature = "multiplayer")]
#[derive(serde::Deserialize)]
struct SessionInfoWire {
    protocol: u32,
    name: String,
    secret_required: bool,
}

/// Fetch the host's session info (protocol, name, whether a secret is
/// needed) before joining.
#[cfg(feature = "multiplayer")]
fn fetch_session_info(addr: &std::net::SocketAddr) -> Result<SessionInfoWire> {
    let url = format!("http://{addr}/session-info");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let resp = reqwest::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Couldn't reach the host's session endpoint at {url} ({e}). Is the host running, and is TCP port {} open?",
                    addr.port()
                )
            })?;
        if !resp.status().is_success() {
            anyhow::bail!("session endpoint answered HTTP {}", resp.status());
        }
        Ok(resp.json::<SessionInfoWire>().await?)
    })
}

/// The session PIN from --pin, the panel's env hand-off, or an interactive
/// prompt (three attempts isn't meaningful against a bearer secret — the
/// host refuses and closes on a wrong one).
#[cfg(feature = "multiplayer")]
fn resolve_session_pin(pin_arg: Option<&str>, desktop: bool) -> Result<String> {
    if let Some(pin) = pin_arg {
        return Ok(localgpt_gen::net::normalize_pin(pin));
    }
    if let Ok(pin) = std::env::var(localgpt_gen::net::JOIN_PIN_ENV) {
        // SAFETY: called before any threads are spawned.
        unsafe { std::env::remove_var(localgpt_gen::net::JOIN_PIN_ENV) };
        let pin = localgpt_gen::net::normalize_pin(&pin);
        if !pin.is_empty() {
            return Ok(pin);
        }
    }
    if desktop {
        anyhow::bail!(
            "This session needs a PIN. Join it from Gen's Collaborate panel, or pass --pin."
        );
    }
    let mut rl = rustyline::DefaultEditor::new()?;
    let pin = rl.readline("Session PIN (shown on the host's console): ")?;
    Ok(localgpt_gen::net::normalize_pin(&pin))
}

/// `--join` mode: resolve the host address (mDNS browse when unspecified),
/// fetch session info, and run the ops client app.
#[cfg(feature = "multiplayer")]
fn run_join_mode(cli: &Cli, desktop: bool, config: &localgpt_core::config::Config) -> Result<()> {
    use std::net::SocketAddr;

    let addr: SocketAddr = match cli.join.clone().flatten() {
        Some(spec) => localgpt_gen::net::parse_peer_addr(&spec)?,
        None => {
            eprintln!("Browsing the LAN for collaborative sessions…");
            let sessions = localgpt_gen::net::mdns::browse_sessions(
                std::time::Duration::from_secs(3),
                localgpt_gen::net::PROTOCOL_ID,
            )?;
            if sessions.is_empty() {
                anyhow::bail!(
                    "No sessions found. Host one with `localgpt-gen --host`, then join it,\n\
                     or connect directly: localgpt-gen --join 192.168.1.5:9879"
                );
            }
            for session in &sessions {
                eprintln!("  found: {} — {}", session.session_name, session.addr);
            }
            let chosen = &sessions[0];
            eprintln!("Joining '{}' at {}", chosen.session_name, chosen.addr);
            chosen.addr
        }
    };

    let info = fetch_session_info(&addr)?;
    if info.protocol != localgpt_world_sync::PROTOCOL_VERSION {
        anyhow::bail!(
            "Host speaks session protocol {} but this client speaks {} — use matching localgpt-gen versions",
            info.protocol,
            localgpt_world_sync::PROTOCOL_VERSION
        );
    }
    let pin = if info.secret_required {
        Some(resolve_session_pin(cli.pin.as_deref(), desktop)?)
    } else {
        eprintln!("Joining '{}' (open session).", info.name);
        None
    };

    let (prompt_tx, prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let workspace = config.workspace_path();
    if desktop {
        return run_ops_client_app(addr, pin, prompt_rx, Some(prompt_tx), workspace);
    }

    // Prompt REPL on a background thread — lines flow to the ops client.
    std::thread::spawn(move || {
        let Ok(mut rl) = rustyline::DefaultEditor::new() else {
            eprintln!("Failed to open input editor — prompts unavailable");
            return;
        };
        loop {
            match rl.readline("You: ") {
                Ok(line) => {
                    let text = line.trim().to_string();
                    if !text.is_empty() {
                        let _ = prompt_tx.send(text);
                    }
                }
                Err(_) => {
                    // Ctrl+D or editor error — the Bevy window keeps running;
                    // exiting the process is the only way out of the REPL.
                    std::process::exit(0);
                }
            }
        }
    });

    run_ops_client_app(addr, pin, prompt_rx, None, workspace)
}

/// The desktop-mode log (`<state dir>/logs/gen-desktop.log`, appended to),
/// since there's no terminal to log to. `None` if it can't be opened.
fn open_desktop_log() -> Option<std::fs::File> {
    let dir = localgpt_core::paths::Paths::resolve().ok()?.logs_dir();
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("gen-desktop.log"))
        .ok()
}

/// Set up and run headless Bevy (no window) on the main thread.
///
/// Headless mode uses DefaultPlugins without a primary window, and adds
/// the completion detector system that exits when the agent is done.
fn run_headless_bevy_app(
    channels: gen3d::GenChannels,
    workspace: std::path::PathBuf,
    completion_flag: gen3d::headless::HeadlessCompletionFlag,
    initial_world: Option<String>,
) -> Result<()> {
    use bevy::prelude::*;

    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None, // No window
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(bevy::render::RenderPlugin {
                render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                    bevy::render::settings::WgpuSettings {
                        // Allow software rendering on headless servers
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

    // Insert completion flag and add detector system
    app.insert_resource(completion_flag);
    app.add_systems(Update, gen3d::headless::headless_completion_detector);

    gen3d::plugin::setup_gen_app(&mut app, channels, workspace, None);
    app.insert_resource(gen3d::plugin::GenInitialWorld {
        path: initial_world,
    });

    app.run();

    Ok(())
}

/// Run the headless generation agent — generates a world and exits.
async fn run_headless_agent(
    bridge: std::sync::Arc<gen3d::GenBridge>,
    headless_config: gen3d::headless::HeadlessConfig,
    config: localgpt_core::config::Config,
    tool_profile: gen3d::tool_profile::ToolProfile,
) -> Result<()> {
    use localgpt_core::agent::Agent;
    use localgpt_core::agent::tools::create_safe_tools;
    use localgpt_core::memory::MemoryManager;
    use std::sync::Arc;

    let agent_id = &headless_config.agent_id;

    // Set up memory
    // new_with_full_config, not new_with_agent: the latter drops config.paths
    // and re-resolves from the environment, which would silently put Gen back
    // in the assistant's workspace.
    let memory = MemoryManager::new_with_full_config(&config.memory, Some(&config), agent_id)?;
    let memory = Arc::new(memory);

    // Create safe tools + gen tools (no CLI tools needed in headless).
    // memory_save / memory_log come too: GEN_MEMORY_PROMPT below tells the
    // model to use them, so they have to exist (they did only in the MCP
    // server path, leaving the prompt pointing at missing tools).
    let mut tools = create_safe_tools(&config, Some(memory.clone()))?;
    tools.extend(localgpt_core::mcp::memory_tools::create_memory_write_tools(
        config.workspace_path(),
    ));
    tools.extend(gen3d::tools::create_gen_tools(bridge.clone()));
    tools.extend(localgpt_gen::mcp::avatar_tools::create_character_tools(
        bridge.clone(),
    ));
    tools.extend(localgpt_gen::mcp::interaction_tools::create_interaction_tools(bridge.clone()));
    tools.extend(localgpt_gen::mcp::terrain_tools::create_terrain_tools(
        bridge.clone(),
    ));
    tools.extend(localgpt_gen::mcp::ui_tools::create_ui_tools(bridge.clone()));
    tools.extend(localgpt_gen::mcp::physics_tools::create_physics_tools(
        bridge.clone(),
    ));
    tools.extend(localgpt_gen::mcp::multifile_tools::create_multifile_tools(
        bridge.clone(),
    ));
    tools = gen3d::tool_profile::apply_tool_profile(tools, tool_profile);
    if tool_profile != gen3d::tool_profile::ToolProfile::Full {
        tracing::info!(
            "tool profile '{tool_profile}': {} tools active",
            tools.len()
        );
    }

    // Configure agent
    let mut config = config;
    config.agent.max_tool_repeats = config.agent.max_tool_repeats.max(20);

    if let Some(ref model) = headless_config.model {
        config.agent.default_model = model.clone();
    }

    // Create agent
    let mut agent = Agent::new_with_tools(config.clone(), agent_id, memory, tools)?;
    agent.new_session().await?;

    // Inject gen-specific memory guidance
    agent.add_user_message(gen3d::system_prompt::GEN_MEMORY_PROMPT);

    // Build effective prompt
    let effective_prompt = format!(
        "{}\n\n{}",
        gen3d::system_prompt::HEADLESS_EXPERIMENT_PROMPT,
        headless_config.effective_prompt()
    );

    eprintln!("Generating: {}", headless_config.prompt);

    // Generate the world
    let response = agent.chat(&effective_prompt).await?;

    let response_preview = localgpt_gen::experiment::prompt_preview(&response, 200);
    tracing::info!("Agent response: {}", response_preview);

    // Save the world
    let world_name = headless_config
        .output
        .as_deref()
        .map(|p| {
            std::path::Path::new(p)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|| localgpt_gen::experiment::prompt_to_slug(&headless_config.prompt));

    let save_prompt = format!(
        "Save this world with gen_save_world. Name: \"{}\"",
        world_name
    );
    let _save_response = agent.chat(&save_prompt).await?;

    eprintln!("World saved: {}", world_name);

    Ok(())
}

/// Run the interactive agent loop in headless control mode.
async fn run_headless_control_loop(
    url: &str,
    agent_id: &str,
    initial_prompt: Option<String>,
    config: localgpt_core::config::Config,
) -> Result<()> {
    use localgpt_core::agent::tools::create_safe_tools;
    use localgpt_core::agent::{Agent, create_spawn_agent_tool};
    use localgpt_core::memory::MemoryManager;
    use rustyline::DefaultEditor;
    use rustyline::error::ReadlineError;
    use std::sync::Arc;

    // Set up memory
    // new_with_full_config, not new_with_agent: the latter drops config.paths
    // and re-resolves from the environment, which would silently put Gen back
    // in the assistant's workspace.
    let memory = MemoryManager::new_with_full_config(&config.memory, Some(&config), agent_id)?;
    let memory = Arc::new(memory);

    // Create safe tools + character tools pointing to the external URL
    let mut tools = create_safe_tools(&config, Some(memory.clone()))?;
    tools.extend(character_tools::create_avatar_tools());
    tools.extend(vec![create_spawn_agent_tool(
        config.clone(),
        memory.clone(),
    )]);

    // Create agent with combined tools
    let mut agent = Agent::new_with_tools(config.clone(), agent_id, memory, tools)?;
    agent.new_session().await?;

    // Inject instructions for avatar control
    let instructions = r#"
You are controlling an avatar in an external 3D application.
Your goal is to explore the world and execute user commands.

You have access to `avatar_tools` to:
- Get state (`get_avatar_state`)
- Move (`move_avatar`)
- Look (`look_avatar`)
- Teleport (`teleport_avatar`)

Use `get_avatar_state` frequently to understand your position.
"#;
    agent.add_user_message(instructions);

    println!("Connected to external avatar control at {}", url);

    // If initial prompt given, send it
    if let Some(prompt) = initial_prompt {
        println!("\n> {}", prompt);
        streaming_chat(&mut agent, &prompt, None).await?;
        println!();
    }

    // Interactive loop
    let mut rl = DefaultEditor::new()?;
    loop {
        let readline = rl.readline("Avatar> ");

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

        let _ = rl.add_history_entry(input);

        if input == "/quit" || input == "/exit" || input == "/q" {
            break;
        }

        streaming_chat(&mut agent, input, None).await?;
        println!();
    }

    Ok(())
}
