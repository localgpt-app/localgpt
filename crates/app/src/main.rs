//! The LocalGPT desktop app — one window, every mode.
//!
//! Gen's world, and around it the app's interface: the history and its
//! playback, the document and its outline, the timeline of the world's clock
//! across the window, Open…, and terminals, each a login shell started in the
//! folder of what is open, where the person runs whatever agent they have.
//! The app runs no agent and no language model, and holds no agent's or
//! model's configuration.
//!
//! The interface is the shell (`docs/rfcs/authoring/all-rust.md`): eframe
//! owns the window, egui draws the panels, and Bevy draws the world into a
//! texture the shell shows (`shell`). The panels and the app speak one
//! protocol (`webui`), which a paired phone's page speaks too.
//!
//! ```sh
//! cargo run -p localgpt-app                          # reopens what was opened last
//! cargo run -p localgpt-app -- --doc notes.md         # a document as a world
//! cargo run -p localgpt-app -- --song track.mp3      # a song as a world that performs it
//! cargo run -p localgpt-app -- --world harbour.world # a package, live: agents edit it
//! cargo run -p localgpt-app -- gen mcp-server        # Gen's MCP server / CLI mode
//! ```
//!
//! A document (`--doc`) compiles with Document's pure lib — the rule-derived draft
//! plus any ```world fences — and opens as a world that is rebuilt while it
//! is edited. A song (`--song`) opens as the world Verse's library builds
//! for it, which Gen's viewport plays. A `.world` package opens as a live
//! canvas: this window holds its authority and serves the API agents change
//! it through. Everything heavy comes from the gen lib; this crate is the
//! thin assembly.

mod document;
mod open;
mod peaks;
mod scene;
mod share;
mod shell;
mod song;
mod starter;
mod timeline;
mod webui;

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use localgpt_gen::desktop;
use localgpt_gen::gen3d;
use localgpt_world_sync::OpLogEntry;

#[derive(Debug, Default, PartialEq)]
struct Args {
    md: Option<PathBuf>,
    song: Option<PathBuf>,
    world: Option<String>,
    /// A session played back: a name, a package, an `ops.jsonl` or a
    /// `.world` file.
    replay: Option<String>,
    /// Its pace, in batches a second.
    replay_speed: Option<f32>,
    /// Someone's session, as a guest; `Some(None)` finds one on this
    /// network.
    join: Option<Option<String>>,
    /// The PIN it asks for.
    pin: Option<String>,
}

#[derive(Debug, PartialEq)]
enum LaunchAction {
    Window(Args),
    Gen(Vec<OsString>),
    Help,
    Version,
}

const USAGE: &str = "\
LocalGPT — one window, every mode

Usage: localgpt-app [OPTIONS]
       localgpt-app gen [GEN_ARGS...]
       localgpt-app mcp-server [MCP_ARGS...]

Options:
      --doc <PATH>          Open a Markdown document as a world (alias: --md)
      --song <PATH>         Open a song as a world that performs it
      --world <PATH>        Open a .world package (folder, workspace name, or URL)
      --replay <SESSION>    Play a session back as a time-lapse: its name, its
                            package, an ops.jsonl or a .world file
      --replay-speed <N>    The replay's pace, in batches a second [default: 4]
      --join [HOST[:PORT]]  Join someone's session as a guest (no address: the
                            first found on this network)
      --pin <PIN>           The session's PIN, when it asks for one
  -h, --help                Print help
  -V, --version             Print version";

/// The replay's pace when none is asked for: Gen's.
const REPLAY_SPEED: f32 = 4.0;

/// Parse launch arguments from `raw` (`argv[0]` included). `gen …` and
/// `mcp-server …` hand off directly to [`localgpt_gen::cli::run_from`] before
/// app logging or the egui shell starts.
fn parse_action<I>(raw: I) -> Result<LaunchAction, String>
where
    I: IntoIterator<Item = OsString>,
{
    let mut iter = raw.into_iter();
    let _exe = iter.next();
    let rest: Vec<OsString> = iter.collect();
    match rest.first().and_then(|s| s.to_str()) {
        Some("gen") => {
            let forwarded = std::iter::once(OsString::from("localgpt gen"))
                .chain(rest.into_iter().skip(1))
                .collect();
            return Ok(LaunchAction::Gen(forwarded));
        }
        Some("mcp-server") => {
            let forwarded = std::iter::once(OsString::from("localgpt-app"))
                .chain(rest)
                .collect();
            return Ok(LaunchAction::Gen(forwarded));
        }
        _ => {}
    }

    let mut args = Args::default();
    let mut iter = rest.into_iter().peekable();
    const KNOWN: &str = "--doc, --song, --world, --replay, --join";
    while let Some(arg) = iter.next() {
        let Some(flag) = arg.to_str() else {
            return Err(format!("unknown argument {arg:?} ({KNOWN})"));
        };
        match flag {
            "-h" | "--help" => return Ok(LaunchAction::Help),
            "-V" | "--version" => return Ok(LaunchAction::Version),
            "--doc" | "--md" => args.md = iter.next().map(PathBuf::from),
            "--song" => args.song = iter.next().map(PathBuf::from),
            "--world" => args.world = iter.next().and_then(|s| s.into_string().ok()),
            "--replay" => args.replay = iter.next().and_then(|s| s.into_string().ok()),
            "--replay-speed" => {
                let speed = iter.next().and_then(|s| s.into_string().ok());
                args.replay_speed = Some(
                    speed
                        .as_deref()
                        .and_then(|s| s.parse::<f32>().ok())
                        .filter(|s| *s > 0.0)
                        .ok_or_else(|| {
                            format!("--replay-speed takes batches a second, not {speed:?}")
                        })?,
                );
            }
            // The address is optional: without one, a session is found.
            "--join" => {
                let address = iter.next_if(|next| !next.to_string_lossy().starts_with('-'));
                args.join = Some(address.and_then(|s| s.into_string().ok()));
            }
            "--pin" => args.pin = iter.next().and_then(|s| s.into_string().ok()),
            other => return Err(format!("unknown argument {other:?} ({KNOWN})")),
        }
    }
    // A replay and a guest's window show what a log and a room drive: each
    // opens on its own.
    let opens = args.md.is_some() || args.song.is_some() || args.world.is_some();
    if args.replay.is_some() && (opens || args.join.is_some()) {
        return Err("--replay opens on its own, without --doc, --song, --world or --join".into());
    }
    if args.join.is_some() && opens {
        return Err("--join opens on its own, without --doc, --song or --world".into());
    }
    if args.replay_speed.is_some() && args.replay.is_none() {
        return Err("--replay-speed is the pace of a --replay".into());
    }
    if args.pin.is_some() && args.join.is_none() {
        return Err("--pin is the PIN of a session to --join".into());
    }
    Ok(LaunchAction::Window(args))
}

/// Logging, before Bevy starts: Bevy's own LogPlugin is disabled (as in Gen),
/// so without this every warning and error from Gen's internals — a world that
/// failed to load, a missing model — would vanish. Launched from a terminal it
/// goes to stderr; from Finder there is no terminal, so it goes to
/// `<logs>/localgpt-app.log`. `RUST_LOG` overrides the default of `warn`.
fn init_logging() {
    use std::io::IsTerminal;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let log_file = (!std::io::stderr().is_terminal())
        .then(|| {
            let dir = localgpt_world_agent::paths::Dirs::resolve()?.logs();
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
    let args = match parse_action(std::env::args_os()) {
        Ok(LaunchAction::Gen(forwarded)) => return localgpt_gen::cli::run_from(forwarded),
        Ok(LaunchAction::Help) => {
            println!("{USAGE}");
            return Ok(());
        }
        Ok(LaunchAction::Version) => {
            println!("localgpt-app {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Ok(LaunchAction::Window(args)) => args,
        Err(msg) => {
            eprintln!("localgpt-app: {msg}");
            std::process::exit(2);
        }
    };
    // A guest's PIN, handed over by the window that opened this one: out of
    // the environment before anything this app starts (its terminals) can
    // inherit it. SAFETY: program start, before threads.
    let handed_pin = unsafe { localgpt_gen::net::join::take_env_pin() };
    init_logging();

    // Launched from Finder, the app has launchd's minimal PATH: take the
    // login shell's, so what the app runs by name is found, and keep the
    // bundle's own folder (`Contents/MacOS`, where `localgpt` sits beside
    // `localgpt-app`) on it so the terminals find `localgpt` even when the
    // app was dragged straight from a `.dmg`.
    #[cfg(unix)]
    {
        let current = std::env::var("PATH").unwrap_or_default();
        let with_exe = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .and_then(|dir| dir.to_str().map(str::to_owned))
            .map(|dir| desktop::shell_env::merge_paths(&current, &dir))
            .unwrap_or(current);
        let merged = match desktop::shell_env::login_shell_path() {
            Some(login_path) => desktop::shell_env::merge_paths(&with_exe, &login_path),
            None => with_exe,
        };
        // SAFETY: program start, before threads.
        unsafe { std::env::set_var("PATH", merged) };
    }

    let opened = open_launch(&args, handed_pin)?;

    // The shell: eframe takes this thread, and the app is built and run on
    // a thread of its own, drawing on the shell's device.
    let gpu = shell::Gpu::new()?;
    shell::run(gpu, move |app| assemble(app, opened))
}

/// What the launch opens, worked out before the app is built: the first
/// world, the document or canvas behind it, and where the terminals start.
struct Opened {
    workspace: PathBuf,
    showing: Option<String>,
    initial_world: Option<String>,
    /// A document: Document's pipeline builds the first world (camera included —
    /// it places the view once), and the live Document drives every rebuild
    /// after.
    live_document: Option<document::Document>,
    /// A package opened as a live canvas: the window holds the authority and
    /// serves the ops API, and the agents change the world through it.
    live_canvas: Option<PathBuf>,
    /// Where the agents' terminals start: beside what is open.
    agents_dir: Option<PathBuf>,
    /// A session played back (`--replay`): its batches, and their pace.
    replay: Option<(VecDeque<OpLogEntry>, f32)>,
    /// Someone's session (`--join`): where, and the PIN it asks for.
    join: Option<(std::net::SocketAddr, Option<String>)>,
}

fn open_launch(args: &Args, handed_pin: Option<String>) -> anyhow::Result<Opened> {
    // Gen's directories and settings: a desktop app opens on a machine with
    // nothing on disk, reads no config file and writes none. Same path as
    // `localgpt-gen`.
    let config = localgpt_gen::config::gen_config();
    let settings = localgpt_gen::settings::load().unwrap_or_default();
    let workspace = config.workspace.clone();

    // A replay and a guest's window open on their own, and are not
    // remembered: neither is a place to come back to.
    if let Some(spec) = &args.replay {
        let path = localgpt_world_agent::session::resolve(&workspace, spec);
        let entries = gen3d::replay::load_op_log(&path.to_string_lossy())?;
        let speed = args.replay_speed.unwrap_or(REPLAY_SPEED);
        eprintln!(
            "localgpt-app: replaying {} batches from {} ({speed}/s)",
            entries.len(),
            path.display()
        );
        return Ok(Opened {
            workspace,
            showing: Some(format!("Replay · {}", file_name(&path))),
            initial_world: None,
            live_document: None,
            live_canvas: None,
            agents_dir: None,
            replay: Some((entries, speed)),
            join: None,
        });
    }
    if let Some(spec) = &args.join {
        let addr = localgpt_gen::net::join::locate(spec.as_deref())?;
        let session = localgpt_gen::net::join::ask(addr)?;
        let pin = match (
            session.needs_pin,
            args.pin.as_deref().or(handed_pin.as_deref()),
        ) {
            (false, _) => None,
            (true, Some(pin)) => Some(localgpt_gen::net::normalize_pin(pin)),
            (true, None) => anyhow::bail!(
                "'{}' asks for its PIN: localgpt-app --join {addr} --pin <PIN>",
                session.name
            ),
        };
        eprintln!("localgpt-app: joining '{}' at {addr}", session.name);
        return Ok(Opened {
            workspace,
            showing: Some(format!("Guest · {}", session.name)),
            initial_world: None,
            live_document: None,
            live_canvas: None,
            agents_dir: None,
            replay: None,
            join: Some((addr, pin)),
        });
    }

    // The initial world: an explicit --world arg, or a Markdown document
    // compiled in-process. Both land in the same place — GenInitialWorld —
    // and `--doc x --world y` prefers the document (the more specific ask).
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
    let mut live_document = None;
    let mut live_canvas: Option<PathBuf> = None;
    let showing;
    let initial_world;
    let mut agents_dir: Option<PathBuf> = None;
    // What to remember for the next bare launch, and what a bare launch
    // fell back to (not remembered again — it already is).
    let mut remember: Option<localgpt_gen::settings::LastOpened> = None;
    if let Some(md) = &args.md {
        let (live, dir) = document::open(md, &workspace)?;
        eprintln!("localgpt-app: {} -> {}", md.display(), dir.display());
        live_document = Some(live);
        showing = Some(format!("Document · {}", file_name(md)));
        agents_dir = md.parent().map(Path::to_path_buf);
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
        agents_dir = Some(dir.clone());
        let dir = dir.to_string_lossy().into_owned();
        remember = Some(localgpt_gen::settings::LastOpened::World { dir: dir.clone() });
        initial_world = Some(dir);
    } else if let Some(world) = &args.world {
        // A package folder is the document now (`docs/rfcs/authoring/
        // world-editor-stack.md`): open it as a live canvas — the app holds
        // the authority, serves the ops API, and agents change the world
        // through it — rather than importing its manifest to `world.ron`.
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
            agents_dir = Some(PathBuf::from(&dir));
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
                let (live, dir) = document::open(&md, &workspace)?;
                eprintln!(
                    "localgpt-app: reopened {} ({})",
                    md.display(),
                    dir.display()
                );
                live_document = Some(live);
                showing = Some(format!("Document · {}", file_name(&md)));
                agents_dir = md.parent().map(Path::to_path_buf);
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
                    agents_dir = Some(PathBuf::from(&dir));
                    initial_world = Some(dir);
                }
            }
            _ => {
                // A true cold start: nothing asked for, nothing remembered.
                // Lead with a song (`docs/world-strategy.md`). Of the
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
                        // look like a choice they made. Its terminals open in
                        // its world's folder, as any song's do.
                        agents_dir = Some(PathBuf::from(&dir));
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
    Ok(Opened {
        workspace,
        showing,
        initial_world,
        live_document,
        live_canvas,
        agents_dir,
        replay: None,
        join: None,
    })
}

/// Everything the app is, on top of Bevy's own plugins: Gen's world, the
/// mode that opened, the timeline and the scene, and the interface's
/// protocol (`webui`), whichever interface draws it.
fn assemble(app: &mut App, opened: Opened) -> anyhow::Result<()> {
    let Opened {
        workspace,
        showing,
        initial_world,
        live_document,
        live_canvas,
        agents_dir,
        replay,
        join,
    } = opened;

    // No agent runs in the window. Gen's command channel has someone on its
    // far end only while the bar's MCP relay is on, for an agent outside
    // (`share.rs`), and never on a live canvas, which changes only through
    // its API; otherwise the scene changes only through the modes below.
    let (bridge, channels) = gen3d::create_gen_channels();
    let mode = if join.is_some() {
        share::Mode::Guest
    } else if replay.is_some() {
        share::Mode::Replay
    } else if live_canvas.is_some() {
        share::Mode::Canvas
    } else {
        share::Mode::Scene
    };

    // **Canvas mode.** A package opened live makes the window the authority's
    // canvas: agents change the world from outside, through the ops API this
    // window serves — from the page's terminals, or anywhere else (`localgpt
    // world`, Claude Code over `localgpt world mcp`). That is the shape gen's
    // `--live` proved, and it is why nothing here may mutate the scene except
    // ops. Opening another file is a launch-time shape, so there is no Open…
    // in it.
    let canvas = live_canvas.is_some();

    // Opening from the window, as the launch arguments do.
    app.insert_resource(open::Launch {
        workspace: workspace.clone(),
    })
    .insert_resource(open::Showing(showing));
    gen3d::plugin::setup_gen_app(app, channels, workspace.clone(), None);
    // A canvas starts from the package's own log — the live plugin builds the
    // scene from it — so there is no initial world to load. Any startup
    // defaults would collide with the ids the log reuses, the same reason a
    // replay clears the scene first.
    app.insert_resource(gen3d::plugin::GenInitialWorld {
        path: if canvas { None } else { initial_world },
    });
    if let Some(dir) = &live_canvas {
        // The app has no `--review` of its own yet: an agent's batch commits
        // as soon as the authority takes it, as it did before Gen grew one.
        localgpt_gen::gen3d::live::setup_live(app, dir, false)?;
    } else if let Some((entries, speed)) = replay {
        // The log rebuilds the world batch by batch; nothing else opens.
        gen3d::replay::setup_replay(app, entries, speed);
    } else if let Some((addr, pin)) = join {
        // The room drives the scene. What the guest would say in it has no
        // box in the window yet, so its sender goes unused.
        let (client, _say) = localgpt_gen::net::join::client(addr, pin);
        app.add_plugins(client);
    } else {
        app.add_plugins((document::DocumentPlugin, open::OpenPlugin));
    }
    // Gen's room, dormant until Share starts it — not in a guest's window,
    // whose world is the host's to share.
    if mode.can_host() {
        app.add_plugins(localgpt_gen::net::host::NetHostPlugin {
            options: std::sync::Mutex::new(Some(localgpt_gen::net::host::NetHostOptions {
                session_name: localgpt_gen::net::default_session_name(),
                ..Default::default()
            })),
        });
    }
    app.add_plugins(share::SharePlugin {
        mode,
        bridge,
        workspace,
    });
    // World time — the song, the shots, the tour — on one clock, in every
    // mode; and the world's things, from the same document.
    app.add_plugins((
        timeline::TimelinePlugin {
            canvas: live_canvas.clone(),
        },
        scene::ScenePlugin,
    ));
    if let Some(live) = live_document {
        app.insert_resource(live);
    }
    // The interface's protocol, its terminals in the folder of what is open:
    // the shell's panels (`shell::run` inserted its link before this) and a
    // paired phone's page.
    app.add_plugins(webui::WebUiPlugin {
        cwd: terminals_dir(
            live_canvas.clone().or(agents_dir),
            std::env::current_dir().ok(),
            std::env::var_os("HOME").map(PathBuf::from),
        ),
    });
    Ok(())
}

/// Where the terminals open: the folder of what is open; with nothing open,
/// the folder the app was started in — but not the root, which is where
/// Finder starts an app — and then the home folder.
fn terminals_dir(
    open: Option<PathBuf>,
    started_in: Option<PathBuf>,
    home: Option<PathBuf>,
) -> PathBuf {
    open.or(started_in.filter(|dir| dir.parent().is_some()))
        .or(home)
        .unwrap_or_default()
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_open_where_something_is_never_in_finders_root() {
        let path = |p: &str| Some(PathBuf::from(p));
        let home = path("/Users/someone");
        // What is open wins.
        assert_eq!(
            terminals_dir(path("/worlds/harbour.world"), path("/"), home.clone()),
            PathBuf::from("/worlds/harbour.world")
        );
        // Nothing open: where a terminal started the app...
        assert_eq!(
            terminals_dir(None, path("/Users/someone/project"), home.clone()),
            PathBuf::from("/Users/someone/project")
        );
        // ...but Finder starts it in the root, and then the home folder.
        assert_eq!(
            terminals_dir(None, path("/"), home.clone()),
            PathBuf::from("/Users/someone")
        );
        assert_eq!(
            terminals_dir(None, None, home),
            PathBuf::from("/Users/someone")
        );
    }

    #[test]
    fn parse_action_routes_gen_and_mcp_server_and_desktop_flags() {
        let os = |items: &[&str]| items.iter().map(OsString::from).collect::<Vec<_>>();

        assert_eq!(
            parse_action(os(&["localgpt-app", "gen", "mcp-server", "--headless"])),
            Ok(LaunchAction::Gen(os(&[
                "localgpt gen",
                "mcp-server",
                "--headless"
            ])))
        );
        assert_eq!(
            parse_action(os(&["localgpt-app", "mcp-server", "--headless"])),
            Ok(LaunchAction::Gen(os(&[
                "localgpt-app",
                "mcp-server",
                "--headless"
            ])))
        );
        assert_eq!(
            parse_action(os(&["localgpt-app", "--doc", "notes.md"])),
            Ok(LaunchAction::Window(Args {
                md: Some(PathBuf::from("notes.md")),
                ..Args::default()
            }))
        );
        assert_eq!(
            parse_action(os(&["localgpt-app", "--help"])),
            Ok(LaunchAction::Help)
        );
        assert!(parse_action(os(&["localgpt-app", "--unknown"])).is_err());
    }

    /// A replay and a guest's window are launch shapes of their own, which
    /// the bar's Share panel opens in a new window.
    #[test]
    fn replay_and_join_open_on_their_own() {
        let os = |items: &[&str]| items.iter().map(OsString::from).collect::<Vec<_>>();
        let window = |items: &[&str]| match parse_action(os(items)) {
            Ok(LaunchAction::Window(args)) => Ok(args),
            Ok(other) => panic!("{other:?}"),
            Err(e) => Err(e),
        };

        assert_eq!(
            window(&["localgpt-app", "--replay", "castle", "--replay-speed", "8"]),
            Ok(Args {
                replay: Some("castle".into()),
                replay_speed: Some(8.0),
                ..Args::default()
            })
        );
        assert_eq!(
            window(&[
                "localgpt-app",
                "--join",
                "192.168.1.5:9879",
                "--pin",
                "482913"
            ]),
            Ok(Args {
                join: Some(Some("192.168.1.5:9879".into())),
                pin: Some("482913".into()),
                ..Args::default()
            })
        );
        // No address: one is found on this network.
        assert_eq!(
            window(&["localgpt-app", "--join", "--pin", "482913"]),
            Ok(Args {
                join: Some(None),
                pin: Some("482913".into()),
                ..Args::default()
            })
        );
        assert_eq!(
            window(&["localgpt-app", "--join"]),
            Ok(Args {
                join: Some(None),
                ..Args::default()
            })
        );

        for refused in [
            &["localgpt-app", "--replay", "castle", "--doc", "notes.md"][..],
            &["localgpt-app", "--replay", "castle", "--join"],
            &["localgpt-app", "--join", "host", "--world", "harbour.world"],
            &["localgpt-app", "--replay-speed", "8"],
            &[
                "localgpt-app",
                "--replay",
                "castle",
                "--replay-speed",
                "fast",
            ],
            &["localgpt-app", "--replay", "castle", "--replay-speed", "0"],
            &["localgpt-app", "--pin", "482913"],
        ] {
            assert!(window(refused).is_err(), "{refused:?}");
        }
    }
}
