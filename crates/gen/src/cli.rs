//! Gen's command line: the flags and the `mcp-server` subcommand that
//! `localgpt-gen` runs, and that the desktop app runs as `localgpt-app gen …`
//! (which `localgpt gen …` hands off to). One parser and one dispatch, so the
//! two front ends cannot drift.
//!
//! Bevy runs on the main thread (macOS windowing and GPU want it); the tool
//! server runs on a tokio thread beside it. No model runs here and no key is
//! held: an agent outside the window drives the scene through MCP —
//! `--mcp-relay` beside the window with `mcp-server --connect` as the
//! agent's process, or `mcp-server` alone as the agent's own process — or
//! through a live package's API (`--live`).

use anyhow::Result;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::gen3d;
use crate::mcp_server;

#[derive(Parser)]
#[command(name = "localgpt-gen", version)]
#[command(about = "LocalGPT Gen — the world builder, driven by agents outside the window")]
struct Cli {
    #[command(subcommand)]
    command: Option<GenSubcommand>,

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

    /// Serve the MCP relay beside the window, so an MCP client (a coding
    /// agent, an editor) drives this window through `mcp-server --connect`.
    #[arg(long, global = true)]
    mcp_relay: bool,

    /// Host a collaborative session on the LAN (listen server + mDNS
    /// announcement). Others join as viewers with --join.
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

    /// Let browsers join as guests (with --host): serve a join page and a
    /// WebSocket endpoint on the session port, and print an invite link.
    /// Guests watch, walk and chat; they can't edit directly.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host")]
    web: bool,

    /// Let browser guests EDIT directly (with --web): they join as editors
    /// and can select, move, rotate, scale and delete entities from the join
    /// page.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "web")]
    web_edit: bool,

    /// Reach guests beyond the LAN through a relay (with --host): the
    /// relay's URL (http(s)://host:port of a localgpt-relay). Prints an
    /// internet invite link.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "host")]
    relay: Option<String>,

    /// Replay a previous session before guests join (with --host --web).
    /// Pass a session name (its package under
    /// <workspace>/sessions/<name>/), a package directory, an ops.jsonl
    /// path, or a .world archive.
    #[cfg(feature = "multiplayer")]
    #[arg(long, requires = "web")]
    resume: Option<String>,

    /// Zip a session's package into the transport form (a .world file)
    /// and exit. Pass a session name or a package directory; --out picks
    /// the destination (default: <name>.world in the current directory).
    #[arg(long)]
    export_session: Option<String>,

    /// Destination for --export-session.
    #[arg(long, requires = "export_session")]
    out: Option<PathBuf>,

    /// Replay a session as a time-lapse in the window: the world rebuilds
    /// itself batch by batch. Pass a session name, a package directory, an
    /// ops.jsonl path, or a .world archive.
    #[arg(long)]
    replay: Option<String>,

    /// Replay pace in batches per second (with --replay).
    #[arg(long, requires = "replay", default_value_t = 4.0)]
    replay_speed: f32,

    /// Open a folder holding a world's manifest.json as a live canvas:
    /// agents change it from outside through ops sent to a local API, as
    /// the folder's AGENTS.md describes.
    #[arg(long)]
    live: Option<PathBuf>,

    /// With --live: an agent's batch shows on the canvas before it
    /// commits, and you accept (Enter) or reject (Esc) each one.
    #[arg(long, requires = "live")]
    review: bool,
}

#[derive(Subcommand)]
enum GenSubcommand {
    /// Run as an MCP server (stdio) — the Bevy window plus the gen tools
    /// over MCP, as the agent's own process
    McpServer {
        /// Run headless (no window) — for CI or batch generation via MCP
        #[arg(long)]
        headless: bool,

        /// Connect to a running window's MCP relay (`--mcp-relay`) instead
        /// of starting Bevy. Pass the relay port (e.g., 9878) or omit to
        /// auto-discover.
        #[arg(long)]
        connect: Option<Option<u16>>,

        /// Also serve the MCP streamable HTTP transport on this port:
        /// JSON-RPC at http://127.0.0.1:<port>/mcp.
        #[arg(long)]
        mcp_http: Option<u16>,
    },
}

fn prog_name(arg0: Option<&OsString>) -> String {
    let Some(arg0) = arg0.and_then(|s| s.to_str()) else {
        return "localgpt-gen".into();
    };
    if arg0.contains(' ') {
        return arg0.to_string();
    }
    Path::new(arg0)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("localgpt-gen")
        .to_string()
}

fn command_for(prog: &str) -> clap::Command {
    Cli::command().bin_name(prog)
}

fn try_parse_from_args<I, T>(args: I) -> std::result::Result<(String, Cli), clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let raw: Vec<OsString> = args.into_iter().map(Into::into).collect();
    let prog = prog_name(raw.first());
    let matches = command_for(&prog).try_get_matches_from(raw)?;
    let cli = Cli::from_arg_matches(&matches)?;
    Ok((prog, cli))
}

/// Run Gen from the process's own arguments: `localgpt-gen`'s `main`.
pub fn main() -> Result<()> {
    run_from(std::env::args_os())
}

/// Run Gen from `args`, the first being the name the usage shows
/// (`localgpt-gen`, or `localgpt gen` when the app runs it). Call it at
/// program start, on the main thread, before any other thread is spawned:
/// Bevy wants the main thread, and on Linux this edits the environment.
pub fn run_from<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    // On Linux, default to X11 (XWayland) to avoid wgpu "Invalid surface" errors
    // on Wayland compositors. winit 0.29+ selects Wayland when WAYLAND_DISPLAY is set.
    // Users can set LOCALGPT_WAYLAND=1 to keep native Wayland.
    #[cfg(target_os = "linux")]
    if std::env::var("LOCALGPT_WAYLAND").is_err() {
        // SAFETY: called at program start before any threads are spawned.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
    }

    let (prog, cli) = try_parse_from_args(args).unwrap_or_else(|e| e.exit());

    // Logs go to stderr: stdout is the transport while `mcp-server` serves.
    let log_level = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .with_writer(std::io::stderr)
        .init();

    // Gen's configuration is its directories: no config file is read, and
    // none is created.
    let config = crate::config::gen_config();
    let workspace = config.workspace.clone();

    // --export-session: zip a session's package into the transport form.
    if let Some(session) = cli.export_session.as_deref() {
        let dir = crate::net::web::resolve_op_log_path(&workspace, session);
        anyhow::ensure!(
            dir.is_dir(),
            "no session package at {} — pass a session name or a package directory",
            dir.display()
        );
        let dest = cli.out.clone().unwrap_or_else(|| {
            let slug = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "session".into());
            std::path::PathBuf::from(format!("{slug}.world"))
        });
        localgpt_world_agent::session::export_zip(&dir, &dest)?;
        eprintln!(
            "exported {} → {} ({} KB)",
            dir.display(),
            dest.display(),
            std::fs::metadata(&dest)
                .map(|m| m.len() / 1024)
                .unwrap_or(0)
        );
        return Ok(());
    }

    // --replay: time-lapse a session in the window — no agent, no session.
    if let Some(replay_path) = cli.replay.as_deref() {
        let resolved = crate::net::web::resolve_op_log_path(&workspace, replay_path);
        let entries = gen3d::replay::load_op_log(resolved.to_string_lossy().as_ref())?;
        eprintln!(
            "[replay] {} batches from {} ({}x speed)",
            entries.len(),
            resolved.display(),
            cli.replay_speed
        );
        let (_bridge, channels) = gen3d::create_gen_channels();
        return run_bevy_app(
            channels,
            workspace,
            None,
            None,
            Some((entries, cli.replay_speed)),
            None,
        );
    }

    // --live: a canvas for a world agents change from outside.
    if let Some(dir) = cli.live.clone() {
        let (_bridge, channels) = gen3d::create_gen_channels();
        return run_bevy_app(
            channels,
            workspace,
            None,
            None,
            None,
            Some((dir, cli.review)),
        );
    }

    // Multiplayer client mode (--join): slim viewer app, no gen subsystems.
    #[cfg(feature = "multiplayer")]
    if cli.join.is_some() {
        if cli.command.is_some() {
            anyhow::bail!("--join cannot be combined with a subcommand");
        }
        return run_join_mode(&cli, &workspace);
    }
    #[cfg(feature = "multiplayer")]
    if cli.host && cli.command.is_some() {
        anyhow::bail!("--host cannot be combined with a subcommand");
    }

    match cli.command {
        Some(GenSubcommand::McpServer {
            headless,
            connect,
            mcp_http,
        }) => {
            // --connect: this process is the stdio end of a running window's
            // relay, nothing more.
            if let Some(port_opt) = connect {
                let port = port_opt
                    .or_else(gen3d::mcp_relay::read_relay_port)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No relay port specified and no running gen process found.\n\
                             Start `{prog} --mcp-relay` first, or pass --connect <port>."
                        )
                    })?;

                tracing::info!("Connecting to the gen window's relay on port {}", port);

                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to build tokio runtime");

                return rt.block_on(gen3d::mcp_relay::run_stdio_relay(port));
            }

            // MCP server mode: Bevy on the main thread, the stdio server on a
            // background thread.
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

            // Optionally serve the HTTP transport on a thread of its own.
            if let Some(http_port) = mcp_http {
                let bridge_for_http = bridge.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to build tokio runtime for MCP HTTP server");

                    rt.block_on(async move {
                        if let Err(e) =
                            mcp_server::run_mcp_http_server(bridge_for_http, http_port).await
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
                    if let Err(e) = mcp_server::run_mcp_server(bridge_for_mcp).await {
                        tracing::error!("MCP server error: {}", e);
                    }
                    // The MCP client went away: so does the window.
                    std::process::exit(0);
                });
            });

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
            // The window alone. With --mcp-relay an MCP client outside drives
            // it; `--live` and `--replay` have returned above.
            let initial_scene = cli
                .scene
                .as_ref()
                .and_then(|path| gen3d::plugin::resolve_gltf_path(path, &workspace));
            let initial_world = startup_world(cli.world.as_deref(), &workspace)?;

            let (bridge, channels) = gen3d::create_gen_channels();
            let enable_relay = cli.mcp_relay;

            #[cfg(feature = "multiplayer")]
            let host_options = {
                let mut opts = crate::net::host::NetHostOptions {
                    session_name: cli
                        .session_name
                        .clone()
                        .unwrap_or_else(crate::net::default_session_name),
                    port: cli.port,
                    open: cli.open,
                    autostart: cli.host,
                    web: cli.web,
                    web_edit: cli.web_edit,
                    resume: cli.resume.clone(),
                    relay: cli.relay.clone(),
                };
                if !cli.host {
                    opts = crate::net::host::NetHostOptions {
                        session_name: opts.session_name,
                        ..Default::default()
                    };
                }
                opts
            };

            if enable_relay {
                // The relay on a tokio thread (Bevy must own the main thread
                // for windowing and the GPU on macOS).
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to build tokio runtime for the MCP relay");
                    rt.block_on(async move {
                        match gen3d::mcp_relay::start_mcp_relay(bridge).await {
                            Ok(port) => eprintln!(
                                "MCP relay active on port {port} (external MCP clients can \
                                 connect to this window)"
                            ),
                            Err(e) => tracing::warn!(
                                "MCP relay failed to start: {e} (external MCP clients won't \
                                 be able to connect)"
                            ),
                        }
                        // Keep the runtime, and the relay's tasks, alive.
                        std::future::pending::<()>().await;
                    });
                });
            } else {
                eprintln!(
                    "{prog}: nothing drives this window — run it with --mcp-relay for \
                     an MCP client, `mcp-server` as an agent's own process, or --live <world> \
                     for a package's API"
                );
            }

            // Run Bevy on the main thread (blocks until the window closes)
            #[cfg(feature = "multiplayer")]
            let result = run_host_bevy_app(
                channels,
                workspace,
                initial_scene,
                initial_world,
                host_options,
            );
            #[cfg(not(feature = "multiplayer"))]
            let result = run_bevy_app(
                channels,
                workspace,
                initial_scene,
                initial_world,
                None,
                None,
            );

            // Clean up the relay port file so stale ports aren't discovered
            if enable_relay {
                gen3d::mcp_relay::cleanup_relay_port();
            }

            result
        }
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
    replay: Option<(
        std::collections::VecDeque<localgpt_world_sync::OpLogEntry>,
        f32,
    )>,
    live: Option<(PathBuf, bool)>,
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
    if let Some((dir, review)) = live {
        gen3d::live::setup_live(&mut app, &dir, review)?;
    }

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
    options: crate::net::host::NetHostOptions,
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
    app.add_plugins(crate::net::host::NetHostPlugin {
        options: std::sync::Mutex::new(Some(options)),
    });

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
    app.add_plugins(crate::net::ops_client::OpsClientPlugin {
        options: std::sync::Mutex::new(Some(crate::net::ops_client::OpsClientOptions {
            server_addr,
            pin,
            name: guest_display_name(),
            prompt_rx,
        })),
    });
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

/// The session PIN from --pin, the env hand-off, or a line typed at the
/// terminal (three attempts isn't meaningful against a bearer secret — the
/// host refuses and closes on a wrong one).
#[cfg(feature = "multiplayer")]
fn resolve_session_pin(pin_arg: Option<&str>) -> Result<String> {
    if let Some(pin) = pin_arg {
        return Ok(crate::net::normalize_pin(pin));
    }
    if let Ok(pin) = std::env::var(crate::net::JOIN_PIN_ENV) {
        // SAFETY: called before any threads are spawned.
        unsafe { std::env::remove_var(crate::net::JOIN_PIN_ENV) };
        let pin = crate::net::normalize_pin(&pin);
        if !pin.is_empty() {
            return Ok(pin);
        }
    }
    eprint!("Session PIN (shown on the host's console): ");
    let mut pin = String::new();
    std::io::stdin().read_line(&mut pin)?;
    Ok(crate::net::normalize_pin(&pin))
}

/// `--join` mode: resolve the host address (mDNS browse when unspecified),
/// fetch session info, and run the ops client app.
#[cfg(feature = "multiplayer")]
fn run_join_mode(cli: &Cli, workspace: &Path) -> Result<()> {
    use std::io::BufRead as _;
    use std::net::SocketAddr;

    let addr: SocketAddr = match cli.join.clone().flatten() {
        Some(spec) => crate::net::parse_peer_addr(&spec)?,
        None => {
            eprintln!("Browsing the LAN for collaborative sessions…");
            let sessions = crate::net::mdns::browse_sessions(
                std::time::Duration::from_secs(3),
                crate::net::PROTOCOL_ID,
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
        Some(resolve_session_pin(cli.pin.as_deref())?)
    } else {
        eprintln!("Joining '{}' (open session).", info.name);
        None
    };

    // Lines typed at the terminal are chat to the room (and `/stats`,
    // `/quit`); they flow to the ops client on a background thread.
    let (prompt_tx, prompt_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else {
                break;
            };
            let text = line.trim().to_string();
            if !text.is_empty() && prompt_tx.send(text).is_err() {
                break;
            }
        }
        // Ctrl+D: the Bevy window keeps running; exiting the process is the
        // only way out of the terminal.
        std::process::exit(0);
    });

    run_ops_client_app(addr, pin, prompt_rx, workspace.to_path_buf())
}

/// Set up and run headless Bevy (no window) on the main thread.
///
/// Headless mode uses DefaultPlugins without a primary window, and adds
/// the completion detector system that exits when the flag is set.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mcp_server_with_app_forwarded_name() {
        let (prog, cli) = try_parse_from_args([
            "localgpt gen",
            "mcp-server",
            "--headless",
            "--mcp-http",
            "8765",
        ])
        .unwrap();
        assert_eq!(prog, "localgpt gen");
        match cli.command {
            Some(GenSubcommand::McpServer {
                headless,
                connect,
                mcp_http,
            }) => {
                assert!(headless);
                assert!(connect.is_none());
                assert_eq!(mcp_http, Some(8765));
            }
            _ => panic!("expected mcp-server subcommand"),
        }
    }

    #[test]
    fn parses_connect_with_optional_port_and_live_flags() {
        let (_, cli) = try_parse_from_args(["localgpt-gen", "mcp-server", "--connect"]).unwrap();
        match cli.command {
            Some(GenSubcommand::McpServer { connect, .. }) => assert_eq!(connect, Some(None)),
            _ => panic!("expected mcp-server --connect"),
        }

        let (_, cli) =
            try_parse_from_args(["localgpt-app", "--live", "/tmp/w", "--review"]).unwrap();
        assert_eq!(cli.live.as_deref(), Some(Path::new("/tmp/w")));
        assert!(cli.review);
    }

    #[test]
    fn help_reflects_forwarded_bin_name() {
        let err = try_parse_from_args(["localgpt gen", "--help"])
            .err()
            .unwrap();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
        let rendered = err.to_string();
        assert!(
            rendered.contains("Usage: localgpt gen [OPTIONS] [COMMAND]"),
            "{rendered}"
        );
    }
}
