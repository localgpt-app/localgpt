//! The LocalGPT desktop app — one window, every mode.
//!
//! Gen's world in a Bevy window, and around it the app's interface: one web
//! page over the viewport (`webui`, built from `ui/`) with the history and
//! its playback, the document and its outline, the timeline of the world's
//! clock across the window, Open…, and the agents' own terminals — Claude
//! Code, Codex and the rest, a tab each, started in the world's folder. The app runs no agent of its own and
//! manages no agent's configuration (`docs/rfcs/authoring/the-page.md`).
//!
//! ```sh
//! cargo run -p localgpt-app                          # reopens what was opened last
//! cargo run -p localgpt-app -- --md notes.md         # a document as a world
//! cargo run -p localgpt-app -- --md notes.md --no-author   # rules only, no model
//! cargo run -p localgpt-app -- --song track.mp3      # a song as a world that performs it
//! cargo run -p localgpt-app -- --world harbour.world # a package, live: agents edit it
//! ```
//!
//! A document (`--md`) compiles with MD's pure lib — the rule-derived draft
//! plus any ```world fences — and opens as a world that is rebuilt while it
//! is edited. A song (`--song`) opens as the world Verse's library builds
//! for it, which Gen's viewport plays. A `.world` package opens as a live
//! canvas: this window holds its authority and serves the API agents change
//! it through. Everything heavy comes from the gen lib; this crate is the
//! thin assembly. The page is macOS's for now: elsewhere the window shows
//! the world alone.

mod authoring;
mod document;
mod open;
mod peaks;
mod scene;
mod screenshot;
mod song;
mod starter;
mod timeline;
mod webui;

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use localgpt_gen::desktop;
use localgpt_gen::gen3d;

#[derive(Debug, Default)]
struct Args {
    md: Option<PathBuf>,
    song: Option<PathBuf>,
    world: Option<String>,
    /// Build documents from rules and fences only, never asking a model.
    no_author: bool,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--md" => args.md = iter.next().map(PathBuf::from),
            "--song" => args.song = iter.next().map(PathBuf::from),
            "--world" => args.world = iter.next(),
            "--no-author" => args.no_author = true,
            other => {
                eprintln!(
                    "localgpt-app: unknown argument {other:?} (--md, --song, --world, --no-author)"
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
/// `<logs>/localgpt-app.log`. `RUST_LOG` overrides the default of `warn`.
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

    // Launched from Finder, the app has launchd's minimal PATH: take the
    // login shell's, so the authoring model's CLI backend is found. (The
    // agents' terminals start a login shell of their own.)
    #[cfg(unix)]
    if let Some(login_path) = desktop::shell_env::login_shell_path() {
        let current = std::env::var("PATH").unwrap_or_default();
        let merged = desktop::shell_env::merge_paths(&current, &login_path);
        // SAFETY: program start, before threads.
        unsafe { std::env::set_var("PATH", merged) };
    }

    // `gguf/<name>` models run in-process (Gen's local_llm), for documents'
    // authoring: registered before any worker asks for a provider.
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
    // serves the ops API, and the agents change the world through it.
    let mut live_canvas: Option<PathBuf> = None;
    let showing;
    let initial_world;
    // Where the agents' terminals start: beside what is open.
    let mut agents_dir: Option<PathBuf> = None;
    // What to remember for the next bare launch, and what a bare launch
    // fell back to (not remembered again — it already is).
    let mut remember: Option<localgpt_gen::settings::LastOpened> = None;
    if let Some(md) = &args.md {
        // The model authors each section's place in the background.
        let (live, dir) = document::open(md, &workspace, (!args.no_author).then_some(&config))?;
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
                let (live, dir) =
                    document::open(&md, &workspace, (!args.no_author).then_some(&config))?;
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

    // No agent runs in the window: Gen's command channel has no one on its
    // far end, and the scene changes only through the modes below.
    let (_no_agent, channels) = gen3d::create_gen_channels();

    // **Canvas mode.** A package opened live makes the window the authority's
    // canvas: agents change the world from outside, through the ops API this
    // window serves — from the page's terminals, or anywhere else (`localgpt
    // world`, Claude Code over `localgpt world mcp`). That is the shape gen's
    // `--live` proved, and it is why nothing here may mutate the scene except
    // ops. Opening another file is a launch-time shape, so there is no Open…
    // in it.
    let canvas = live_canvas.is_some();

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
        // The app has no `--review` of its own yet: an agent's batch commits
        // as soon as the authority takes it, as it did before Gen grew one.
        localgpt_gen::gen3d::live::setup_live(&mut app, dir, false)?;
    } else {
        app.add_plugins((document::DocumentPlugin, open::OpenPlugin));
    }
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
    if let Some(shot) = screenshot::ScreenshotPlugin::from_env() {
        app.add_plugins(shot);
    }
    // The interface: a page over the viewport, its terminals in the folder
    // of what is open.
    if let Some(ui) = webui::WebUiPlugin::from_env(
        live_canvas
            .clone()
            .or(agents_dir)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default(),
    ) {
        app.add_plugins(ui);
    }
    app.run();
    Ok(())
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
