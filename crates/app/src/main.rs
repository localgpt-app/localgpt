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
//! cargo run -p localgpt-app -- --world place.json    # any world manifest
//! ```
//!
//! Verse (playback + worlds) and the assistant (chat + memory) are the next
//! modes; nothing here forecloses them. What this crate deliberately does NOT
//! do is duplicate Gen's entry path — everything heavy comes from the gen
//! lib, and this is the thin assembly.

use std::path::{Path, PathBuf};
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
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--md" => args.md = iter.next().map(PathBuf::from),
            "--world" => args.world = iter.next(),
            "--tools" => args.tools = iter.next(),
            other => {
                eprintln!("localgpt-app: unknown argument {other:?} (--md, --world, --tools)");
                std::process::exit(2);
            }
        }
    }
    args
}

/// Compile a Markdown document into a world folder the app owns, and return
/// the folder. Called on every launch, and the folder is overwritten each
/// time, because the document is the source of truth — routing it through
/// `world_import` instead would reopen a stale world after an edit (same slug,
/// "already imported") and would file the document under `skills/`, where
/// Gen's gallery lists *saved* worlds. A live view of a document is neither.
///
/// It is MD's own pipeline, not a second one: `Doc::parse`, then
/// `draft::compile_with` over MD's sidecar (`doc.world.json` beside the file),
/// so scenery a local model already authored in `localgpt-md` shows up here
/// too. The sidecar is read, never written — authoring is MD's job until its
/// live-authoring mode lands in this app.
fn compile_md_world(md: &Path, workspace: &Path) -> anyhow::Result<PathBuf> {
    let text = std::fs::read_to_string(md)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", md.display()))?;
    let title = md
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".into());
    let doc = localgpt_md::doc::Doc::parse(&text, &title);
    let store = localgpt_md::RecipeStore::load(&md.with_extension("world.json"));
    let world = localgpt_md::draft::compile_with(&doc, &store);
    for issue in localgpt_md::draft::validate(&world) {
        eprintln!("localgpt-app: {:?}: {}", issue.severity, issue.message);
    }

    let dir = workspace.join("documents").join(document_key(md));
    std::fs::create_dir_all(&dir)?;
    let ron = ron::ser::to_string_pretty(&world, ron::ser::PrettyConfig::default())?;
    std::fs::write(dir.join("world.ron"), ron)?;
    link_asset_pack(&world, &dir);
    eprintln!(
        "localgpt-app: {} -> {} ({} entities, {} section{})",
        md.display(),
        dir.display(),
        world.entities.len(),
        doc.sections.len(),
        if doc.sections.len() == 1 { "" } else { "s" },
    );
    Ok(dir)
}

/// The folder name for a document: its stem, plus a hash of where it lives,
/// so two files both called README.md never share a world. FNV-1a rather
/// than `DefaultHasher`, whose output is not promised across Rust releases.
fn document_key(md: &Path) -> String {
    let full = md.canonicalize().unwrap_or_else(|_| md.to_path_buf());
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in full.to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let stem = md
        .file_stem()
        .map(|s| slug(&s.to_string_lossy()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "document".into());
    format!("{stem}-{:08x}", hash as u32)
}

/// Authored scenery references pack models as `models/<file>`; Gen resolves a
/// world's assets under `<world>/assets/`. So the world's `assets` is the
/// shared pack — a symlink on Unix, one copy on disk however many documents
/// use it, the same trick `scripts/fetch-assets.sh` plays for MD and Verse.
/// A draft with no meshes needs nothing, and a missing pack only costs the
/// placeholder boxes Gen already shows for a missing model.
fn link_asset_pack(world: &localgpt_world_types::WorldManifest, dir: &Path) {
    if !world.entities.iter().any(|e| e.mesh_asset.is_some()) {
        return;
    }
    let pack = localgpt_md::assets::assets_dir();
    if !pack.join("models/manifest.json").is_file() {
        eprintln!(
            "localgpt-app: this document uses pack models but no pack is installed — run \
             scripts/fetch-assets.sh"
        );
        return;
    }
    let link = dir.join("assets");
    #[cfg(unix)]
    {
        let pack = pack.canonicalize().unwrap_or(pack);
        if std::fs::read_link(&link).ok().as_deref() == Some(pack.as_path()) {
            return;
        }
        let _ = std::fs::remove_file(&link);
        if let Err(e) = std::os::unix::fs::symlink(&pack, &link) {
            eprintln!("localgpt-app: could not link the asset pack: {e}");
        }
    }
    #[cfg(not(unix))]
    for entity in &world.entities {
        let Some(mesh) = &entity.mesh_asset else {
            continue;
        };
        let (from, to) = (pack.join(&mesh.path), link.join(&mesh.path));
        if to.is_file() || !from.is_file() {
            continue;
        }
        if let Some(parent) = to.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::copy(&from, &to);
    }
}

fn slug(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    while s.contains("--") {
        s = s.replace("--", "-");
    }
    s.trim_matches('-').to_lowercase()
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
    let initial_world = if let Some(md) = &args.md {
        Some(
            compile_md_world(md, &workspace)?
                .to_string_lossy()
                .into_owned(),
        )
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
