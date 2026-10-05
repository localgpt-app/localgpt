//! The LocalGPT desktop app — one window, every mode.
//!
//! Phase 1 of the one-app consolidation (docs/world-strategy.md §13.1): the
//! shell is Gen's — a Bevy window, an egui prompt panel, an agent loop, a
//! world viewport — with Markdown as the first second-class input: `--md
//! doc.md` compiles the document with MD's pure lib (the rule-derived draft
//! plus any ```world fences) and opens it as a world in the same viewport,
//! tour and all, where Gen's tools still work on it (the `core` profile
//! unless `--tools` or Gen's settings say otherwise).
//!
//! ```sh
//! cargo run -p localgpt-app                          # the shell
//! cargo run -p localgpt-app -- --md notes.md         # a document as a world
//! cargo run -p localgpt-app -- --md notes.md --no-author   # rules only, no model
//! cargo run -p localgpt-app -- --song track.mp3      # a song as a world that performs it
//! cargo run -p localgpt-app -- --world place.json    # any world manifest
//! cargo run -p localgpt-app -- --prompt "a quiet harbor at dusk"   # a first prompt
//! ```
//!
//! A song (`--song`) opens as the world Verse's library builds for it, and
//! Gen's viewport plays it: the soundtrack streams on the shared mixer and the
//! world's modulations follow it. The assistant (chat + memory) is the next
//! mode; nothing here forecloses it. What this crate deliberately does NOT do
//! is duplicate Gen's entry path — everything heavy comes from the gen lib,
//! and this is the thin assembly. The prompt panel runs Gen's own agent loop
//! (`localgpt_gen::agent_loop`): its slash commands and model menu, streamed
//! turns, the MCP relay a CLI backend reaches the window through
//! (`localgpt-app mcp-server --connect`), and guests once a session is hosted.

mod authoring;
mod document;
mod now_playing;
mod open;
mod screenshot;
mod song;
mod starter;

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use localgpt_gen::desktop::{self, ChatEvent, PanelSettings};
use localgpt_gen::gen3d;

/// Gen's agent id. The app is Gen desktop's successor, so it reads and writes
/// the same memory index and session history rather than starting a parallel
/// one; when the gen binary retires, this is the only id left.
const AGENT_ID: &str = "gen";

#[derive(Debug, Default)]
struct Args {
    md: Option<PathBuf>,
    song: Option<PathBuf>,
    world: Option<String>,
    tools: Option<String>,
    /// Build documents from rules and fences only, never asking a model.
    no_author: bool,
    /// A first prompt for the panel's agent, as `localgpt-gen "<prompt>"`.
    prompt: Option<String>,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--md" => args.md = iter.next().map(PathBuf::from),
            "--song" => args.song = iter.next().map(PathBuf::from),
            "--world" => args.world = iter.next(),
            "--tools" => args.tools = iter.next(),
            "--no-author" => args.no_author = true,
            "--prompt" => args.prompt = iter.next(),
            other => {
                eprintln!(
                    "localgpt-app: unknown argument {other:?} \
                     (--md, --song, --world, --tools, --prompt, --no-author)"
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

/// `mcp-server --connect [port]`: what a CLI backend's MCP config runs to
/// reach the open window (see `agent_loop::uses_cli_backend`). `None` for an
/// ordinary launch; `Some(None)` means "the port the window advertised".
fn relay_request(args: &[String]) -> Option<Option<u16>> {
    match args {
        [command, flag, rest @ ..] if command == "mcp-server" && flag == "--connect" => {
            Some(rest.first().and_then(|port| port.parse().ok()))
        }
        _ => None,
    }
}

fn main() -> anyhow::Result<()> {
    // A stdio ↔ TCP relay into the running window, spawned by the CLI
    // backend: no window, and nothing but the protocol on stdout.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(port) = relay_request(&argv) {
        let port = port
            .or_else(gen3d::mcp_relay::read_relay_port)
            .ok_or_else(|| anyhow::anyhow!("no LocalGPT window is running to connect to"))?;
        return tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(localgpt_gen::agent_loop::run_mcp_stdio_relay(port));
    }

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

    // `gguf/<name>` models run in-process (Gen's local_llm): registered before
    // any agent or authoring worker asks for a provider, as Gen's main does.
    // Without the feature the model menu says the build has no in-process LLM.
    #[cfg(feature = "local-llm")]
    localgpt_gen::local_llm::register();

    // Gen's in-memory config, not Config::load(): a desktop app must open on a
    // machine with nothing on disk and must not write the assistant's
    // config.toml or read its workspace. Same path as `localgpt-gen`.
    let (config, settings) = localgpt_gen::config::gen_config_and_settings();
    let workspace = config.workspace_path();

    // The initial world: an explicit --world arg, or a Markdown document
    // compiled in-process. Both land in the same place — GenInitialWorld —
    // and `--md x --world y` prefers the document (the more specific ask).
    // A failure to import falls back to the raw path so the reason shows up
    // in the window's log rather than killing the launch. With none of the
    // three given, the app reopens what it opened last.
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
    // A package opened as a live canvas: the window holds the authority and
    // serves the ops API, and there is no in-window agent.
    let mut live_canvas: Option<PathBuf> = None;
    let showing;
    let initial_world;
    // What to remember for the next bare launch, and what a bare launch
    // fell back to (not remembered again — it already is).
    let mut remember: Option<localgpt_gen::settings::LastOpened> = None;
    if let Some(md) = &args.md {
        // The model authors each section's place in the background, with the
        // same model as the prompt panel.
        let (live, dir) = document::open(md, &workspace, (!args.no_author).then_some(&config))?;
        eprintln!("localgpt-app: {} -> {}", md.display(), dir.display());
        live_document = Some(live);
        showing = Some(format!("Document · {}", file_name(md)));
        initial_world = Some(dir.to_string_lossy().into_owned());
        remember = Some(localgpt_gen::settings::LastOpened::Document {
            path: md.to_string_lossy().into_owned(),
        });
    } else if let Some(track) = &args.song {
        // A song: Verse's analysis (seconds on first open, cached after),
        // then its world, which Gen loads and plays.
        eprintln!("localgpt-app: listening to {} …", track.display());
        let (dir, world) = song::prepare(track, &workspace)?;
        let soundtrack = world.soundtrack.as_ref();
        eprintln!(
            "localgpt-app: {} -> {} ({} entities; {} — {}, {:.0} bpm)",
            track.display(),
            dir.display(),
            world.entities.len(),
            soundtrack.and_then(|s| s.title.as_deref()).unwrap_or("?"),
            soundtrack.and_then(|s| s.artist.as_deref()).unwrap_or("?"),
            soundtrack.map_or(0.0, |s| s.bpm),
        );
        let title = soundtrack.and_then(|s| s.title.clone());
        showing = Some(format!(
            "Song · {}",
            title.unwrap_or_else(|| file_name(track))
        ));
        let dir = dir.to_string_lossy().into_owned();
        remember = Some(localgpt_gen::settings::LastOpened::World { dir: dir.clone() });
        initial_world = Some(dir);
    } else if let Some(world) = &args.world {
        // A package folder is the document now (`docs/rfcs/authoring/
        // world-editor-stack.md`): open it as a live canvas — the app holds
        // the authority, serves the ops API, and agents change the world
        // from outside — rather than importing its manifest to `world.ron`.
        let world_path = Path::new(world);
        if localgpt_world_agent::live::is_package(world_path) {
            eprintln!(
                "localgpt-app: {} is a world package — opening it live",
                world_path.display()
            );
            showing = Some(format!("World · {}", file_name(world_path)));
            live_canvas = Some(world_path.to_path_buf());
            remember = Some(localgpt_gen::settings::LastOpened::World { dir: world.clone() });
            initial_world = None;
        } else {
            showing = Some(format!("World · {world}"));
            let dir = import(world);
            remember = Some(localgpt_gen::settings::LastOpened::World { dir: dir.clone() });
            initial_world = Some(dir);
        }
    } else {
        // Reopen where the user left off: a document with its editor again,
        // anything else as its world (a song's plays on its own). Gone is
        // gone — an empty world, as a first run.
        match settings.last_opened {
            Some(localgpt_gen::settings::LastOpened::Document { path })
                if std::path::Path::new(&path).is_file() =>
            {
                let md = PathBuf::from(&path);
                let (live, dir) =
                    document::open(&md, &workspace, (!args.no_author).then_some(&config))?;
                eprintln!(
                    "localgpt-app: reopened {} ({})",
                    md.display(),
                    dir.display()
                );
                live_document = Some(live);
                showing = Some(format!("Document · {}", file_name(&md)));
                initial_world = Some(dir.to_string_lossy().into_owned());
            }
            Some(localgpt_gen::settings::LastOpened::World { dir })
                if std::path::Path::new(&dir).join("world.ron").is_file()
                    || localgpt_world_agent::live::is_package(std::path::Path::new(&dir)) =>
            {
                eprintln!("localgpt-app: reopened {}", dir);
                showing = Some(format!("World · {}", file_name(std::path::Path::new(&dir))));
                if localgpt_world_agent::live::is_package(std::path::Path::new(&dir)) {
                    live_canvas = Some(PathBuf::from(&dir));
                    initial_world = None;
                } else {
                    initial_world = Some(dir);
                }
            }
            _ => {
                // A true cold start: nothing asked for, nothing remembered.
                // Lead with a song (`docs/world-strategy.md` §13.3). Of the
                // inputs this app takes it is the only one whose model-free
                // world is a *finished* world rather than a draft, so it is
                // what someone with no key, no model and no download sees in
                // the first frame. An empty window was the honest thing to
                // show before there was a starter track; it is not a
                // welcome.
                match starter::open(&workspace) {
                    Ok(Some((title, dir))) => {
                        eprintln!("localgpt-app: starting with {title}");
                        showing = Some(format!("Song · {title}"));
                        // Deliberately not remembered: the starter is where a
                        // cold start begins, not somewhere the user chose to
                        // be, and remembering it would make the second launch
                        // look like a choice they made.
                        initial_world = Some(dir);
                    }
                    Ok(None) => {
                        showing = None;
                        initial_world = None;
                    }
                    Err(e) => {
                        eprintln!("localgpt-app: no starter world ({e})");
                        showing = None;
                        initial_world = None;
                    }
                }
            }
        }
    };
    if let Some(opened) = remember
        && let Err(e) = localgpt_gen::settings::update(|saved| saved.last_opened = Some(opened))
    {
        eprintln!("localgpt-app: couldn't remember what it opened: {e}");
    }

    let (bridge, channels) = gen3d::create_gen_channels();
    let (panel_channels, agent_channels) = desktop::create_chat_channels();

    // **Canvas mode.** A package opened live makes the window the authority's
    // canvas: no in-window agent, no prompt panel, no collaboration host and
    // no open bar — agents change the world from outside, through the ops API
    // this window serves, from a terminal (`localgpt world`, herdr, Claude
    // Code over `localgpt world mcp`). That is the shape gen's `--live`
    // proved, and it is why nothing here may mutate the scene except ops.
    // The reconciler is what later lets a panel return, its tools emitting
    // ops instead of touching the ECS.
    let canvas = live_canvas.is_some();

    // Collaboration, as in Gen's desktop mode: the host plugin is installed
    // dormant, and the panel's Collaborate section starts a session.
    let (mut host_options, net_hooks) = localgpt_gen::net::host::create_host_channels();
    host_options.session_name = localgpt_gen::net::default_session_name();

    // A CLI backend reaches the world's tools through the MCP relay, which
    // the window serves and `localgpt-app mcp-server --connect` joins.
    let mut relay = false;
    if !canvas {
        relay = localgpt_gen::agent_loop::uses_cli_backend(&config);

        let tool_profile = tool_profile(args.tools.as_deref(), settings.tool_profile.as_deref())
            .unwrap_or_else(|e| {
                eprintln!("localgpt-app: {e}");
                std::process::exit(2);
            });

        // Gen's agent loop on a background thread: Bevy owns the main thread
        // (macOS). No REPL: the panel is the only local input.
        let agent_config = config.clone();
        let relay_bridge = bridge.clone();
        let failure_sink = agent_channels.sink.clone();
        let first_prompt = args.prompt.clone();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    failure_sink.send(ChatEvent::Failed(format!("no tokio runtime: {e}")));
                    return;
                }
            };
            let outcome = rt.block_on(async move {
                if relay {
                    match gen3d::mcp_relay::start_mcp_relay(relay_bridge, &agent_config).await {
                        Ok(port) => tracing::info!("MCP relay on port {port} for the CLI backend"),
                        Err(e) => tracing::warn!(
                            "MCP relay failed to start ({e}): the CLI backend can't reach the \
                         world's tools"
                        ),
                    }
                }
                localgpt_gen::agent_loop::run_agent_loop(
                    bridge,
                    AGENT_ID,
                    first_prompt,
                    agent_config,
                    None,
                    Some(net_hooks),
                    agent_channels,
                    true,
                    tool_profile,
                )
                .await
            });
            match outcome {
                // /quit in the panel ends the loop, and the app with it.
                Ok(()) => {
                    if relay {
                        gen3d::mcp_relay::cleanup_relay_port();
                    }
                    std::process::exit(0);
                }
                // Say what went wrong in the panel, and leave the window open to
                // read it.
                Err(e) => {
                    tracing::error!("agent loop stopped: {e:#}");
                    failure_sink.send(ChatEvent::Failed(format!("{e:#}")));
                }
            }
        });
    }

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
    // Opening from the window, as the launch arguments do.
    app.insert_resource(open::Launch {
        config: config.clone(),
        workspace: workspace.clone(),
        author: !args.no_author,
    })
    .insert_resource(open::Showing(showing));
    gen3d::plugin::setup_gen_app(&mut app, channels, workspace, None);
    // A canvas starts from the package's own log — the live plugin builds the
    // scene from it — so there is no initial world to load. Any startup
    // defaults would collide with the ids the log reuses, the same reason a
    // replay clears the scene first.
    app.insert_resource(gen3d::plugin::GenInitialWorld {
        path: if canvas { None } else { initial_world },
    });
    if let Some(dir) = &live_canvas {
        localgpt_gen::gen3d::live::setup_live(&mut app, dir)?;
    } else {
        app.add_plugins(localgpt_gen::net::host::NetHostPlugin {
            options: std::sync::Mutex::new(Some(host_options)),
        });
        app.add_plugins(desktop::PromptPanelPlugin::new(
            panel_channels,
            PanelSettings {
                open: true,
                focus_input: true,
                settings_file: localgpt_gen::settings::settings_path(),
                collab: Some(settings.collab.clone()),
            },
        ));
        app.add_plugins((
            document::DocumentPlugin,
            now_playing::NowPlayingPlugin,
            open::OpenPlugin,
        ));
    }
    if let Some(live) = live_document {
        app.insert_resource(live);
    }
    if let Some(shot) = screenshot::ScreenshotPlugin::from_env() {
        app.add_plugins(shot);
    }
    app.run();
    // So a later window doesn't find this one's stale port.
    if relay {
        gen3d::mcp_relay::cleanup_relay_port();
    }
    Ok(())
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The prompt panel's toolbelt: `--tools`, else the profile picked in Gen's
/// settings, else `core`. Gen alone falls back to `full` — it is the tool for
/// building a world tool by tool — but this app opens for anyone
/// (docs/world-strategy.md §13.2: core, deterministic default), and the full
/// belt's ~14k tokens of schema ride on every request, more than a small
/// local model's whole context.
fn tool_profile(
    flag: Option<&str>,
    saved: Option<&str>,
) -> Result<gen3d::tool_profile::ToolProfile, String> {
    use gen3d::tool_profile::ToolProfile;
    if let Some(flag) = flag {
        return ToolProfile::parse(flag)
            .ok_or_else(|| format!("--tools {flag:?} (core, standard, full)"));
    }
    Ok(saved
        .and_then(ToolProfile::parse)
        .unwrap_or(ToolProfile::Core))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gen3d::tool_profile::ToolProfile;

    #[test]
    fn a_cli_backend_reaches_the_window_through_mcp_server_connect() {
        let args = |line: &str| {
            line.split_whitespace()
                .map(String::from)
                .collect::<Vec<_>>()
        };
        assert_eq!(relay_request(&args("mcp-server --connect")), Some(None));
        assert_eq!(
            relay_request(&args("mcp-server --connect 9878")),
            Some(Some(9878))
        );
        // An ordinary launch is not a relay.
        assert_eq!(relay_request(&args("--md notes.md")), None);
        assert_eq!(relay_request(&args("")), None);
        assert_eq!(relay_request(&args("mcp-server")), None);
    }

    #[test]
    fn the_app_opens_with_the_core_toolbelt_unless_told_otherwise() {
        assert_eq!(tool_profile(None, None), Ok(ToolProfile::Core));
        // What the user picked in Gen's settings carries over.
        assert_eq!(tool_profile(None, Some("full")), Ok(ToolProfile::Full));
        // The flag beats the setting.
        assert_eq!(
            tool_profile(Some("standard"), Some("full")),
            Ok(ToolProfile::Standard)
        );
        // A bad setting must not stop the app opening; a bad flag is an error.
        assert_eq!(tool_profile(None, Some("typo")), Ok(ToolProfile::Core));
        assert!(tool_profile(Some("typo"), None).is_err());
    }
}
