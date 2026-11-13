//! LocalGPT Document — open a Markdown file and walk through it as a 3D world.
//!
//! Every `##` section becomes a place: a rule-based draft lays it out
//! instantly (PLAN.md M0), and what an agent authored for a section — a
//! build or a recipe in the sidecar next to the document (M2), keyed by the
//! section's hash — replaces the draft for as long as the text stands.
//! ←/→ walks between sections; saving the file rebuilds. No model runs here.
//!
//! Pipeline: [`doc`] (Markdown → sections) → [`draft`] (sections →
//! `WorldManifest`, LocalGPT's shared world format, styled by [`recipe`] +
//! [`sidecar`] entries) → [`scene`] (manifest → Bevy), with [`tour`] for
//! navigation and [`watch`] for hot reload.

use localgpt_doc::{assets, doc, draft, sidecar};

mod scene;
mod tour;
mod watch;

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::{RenderTarget, ShadowLodOrigin};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::ui::IsDefaultUiCamera;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;

use localgpt_world_types as wt;

use crate::scene::{CurrentWorld, TourCamera};

const USAGE: &str = "\
Usage: localgpt-doc [FILE.md] [--print-ron] [--export OUT]

Opens FILE.md (default: samples/hello.md) as a walkable 3D world. Each ##
section becomes a place, and the world rebuilds whenever the file is saved.
What an agent authored for a section (the .world.json sidecar beside the
file) is applied; no model runs here.

  --print-ron   Print the compiled world as RON (LocalGPT Gen's world.ron
                format) and exit without opening a window. The sidecar's
                builds and recipes are applied.
  --export OUT  Write the compiled world to OUT and exit — no window. The
                extension picks the format: .json (the LocalGPT world format
                the web viewer reads), .ron (Gen's world.ron), or .html (a
                self-contained page with the shared web viewer).

Environment:
  LOCALGPT_DOC_SCREENSHOT=out.png  Render the first stop offscreen (no window),
                                   save it as a PNG, and exit.";

const DEFAULT_DOC: &str = "samples/hello.md";

/// Set to a `.png` path, the first stop renders there offscreen and the app
/// exits ([`Smoke`]).
const SCREENSHOT_ENV: &str = "LOCALGPT_DOC_SCREENSHOT";

/// What [`SCREENSHOT_ENV`] was called while the crate was `localgpt-md`:
/// still read, after it.
const SCREENSHOT_ENV_OLD: &str = "LOCALGPT_MD_SCREENSHOT";

/// The sample opened when no path is given. Relative to the working directory
/// first, then to this crate — because since Document joined the localgpt workspace
/// `cargo run -p localgpt-doc` runs from the workspace root, where
/// `samples/hello.md` is not. Falls back to the bare relative path so the
/// "file not found" message still names what the user would expect.
fn default_doc() -> PathBuf {
    let cwd = PathBuf::from(DEFAULT_DOC);
    if cwd.is_file() {
        return cwd;
    }
    let in_crate = Path::new(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_DOC);
    if in_crate.is_file() {
        return in_crate;
    }
    cwd
}

fn main() -> AppExit {
    let mut path = None;
    let mut print_ron = false;
    let mut export = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--print-ron" => print_ron = true,
            "--export" => match args.next() {
                Some(out) => export = Some(PathBuf::from(out)),
                None => {
                    eprintln!("localgpt-doc: --export needs a file name\n\n{USAGE}");
                    return AppExit::error();
                }
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return AppExit::Success;
            }
            flag if flag.starts_with('-') => {
                eprintln!("localgpt-doc: unknown option {flag}\n\n{USAGE}");
                return AppExit::error();
            }
            _ => path = Some(PathBuf::from(&arg)),
        }
    }
    let path = path.unwrap_or_else(default_doc);
    let sidecar_path = path.with_extension("world.json");
    let store = sidecar::RecipeStore::load(&sidecar_path);

    let world = match load(&path, &store) {
        Ok(world) => world,
        Err(err) => {
            eprintln!("localgpt-doc: {}: {err}", path.display());
            return AppExit::error();
        }
    };
    for issue in draft::validate(&world.manifest) {
        eprintln!("localgpt-doc: {:?}: {}", issue.severity, issue.message);
    }

    if print_ron {
        return match to_ron(&world.manifest) {
            Ok(text) => {
                println!("{text}");
                AppExit::Success
            }
            Err(err) => {
                eprintln!("localgpt-doc: {err}");
                AppExit::error()
            }
        };
    }

    if let Some(out) = export {
        return export_world(&world.manifest, &out);
    }

    let smoke = std::env::var(SCREENSHOT_ENV)
        .or_else(|_| std::env::var(SCREENSHOT_ENV_OLD))
        .ok();
    let mut app = App::new();
    // Pin the asset server to the assets root the manifest's mesh paths are
    // relative to (`assets::assets_dir()` — the same root the export copier
    // reads), so a placed GLB resolves the same in dev and packaged builds
    // (Verse's AssetPlugin lesson).
    let plugins = DefaultPlugins.set(AssetPlugin {
        file_path: assets::assets_dir().to_string_lossy().into_owned(),
        ..default()
    });
    if smoke.is_some() {
        // Windowless: reading back a window's frame returns black on a locked
        // or headless Mac, so the smoke shot renders to an offscreen image.
        app.add_plugins((
            plugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
            ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0)),
        ));
    } else {
        let window = Window {
            title: format!("{} — LocalGPT Document", world.doc.title),
            resolution: (1280, 800).into(),
            ..default()
        };
        app.add_plugins(plugins.set(WindowPlugin {
            primary_window: Some(window),
            ..default()
        }));
    }
    app.insert_resource(watch::DocSource::new(path))
        .insert_resource(world)
        .insert_resource(store)
        .add_plugins((scene::ScenePlugin, tour::TourPlugin, watch::WatchPlugin));

    if let Some(path) = smoke {
        app.insert_resource(Smoke::new(path))
            .add_systems(PostStartup, smoke_target)
            .add_systems(Update, smoke_screenshot);
    }
    app.run()
}

/// The manifest as Gen's world.ron.
fn to_ron(manifest: &wt::WorldManifest) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(manifest, ron::ser::PrettyConfig::default())
}

/// `--export OUT`: the compiled world as `.json` (the web viewer's format),
/// `.ron` (Gen's) or `.html` (a page with the shared web viewer, the same
/// bytes Gen's `gen_export_html` writes). An HTML export also copies the
/// placed GLB assets next to the page — the viewer fetches them from
/// `assets/` relative to the file.
fn export_world(manifest: &wt::WorldManifest, out: &Path) -> AppExit {
    let html = matches!(
        out.extension().and_then(|e| e.to_str()),
        Some("html") | Some("htm")
    );
    let text = match out.extension().and_then(|e| e.to_str()) {
        Some("json") => localgpt_world_export::to_json_pretty(manifest),
        Some("ron") => match to_ron(manifest) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("localgpt-doc: {err}");
                return AppExit::error();
            }
        },
        Some("html") | Some("htm") => localgpt_world_export::generate_html(manifest),
        _ => {
            eprintln!(
                "localgpt-doc: --export {}: use a .json, .ron or .html file name",
                out.display()
            );
            return AppExit::error();
        }
    };
    if let Err(err) = std::fs::write(out, text) {
        eprintln!("localgpt-doc: {}: {err}", out.display());
        return AppExit::error();
    }
    if html {
        copy_export_assets(manifest, out);
    }
    println!(
        "{} — {} entities -> {}",
        manifest.meta.name,
        manifest.entities.len(),
        out.display()
    );
    AppExit::Success
}

/// Copy every mesh asset the manifest references into `<out dir>/assets/`,
/// best effort — a missing pack just means the viewer shows its placeholder
/// boxes, which is worth a warning but not a failed export.
fn copy_export_assets(manifest: &wt::WorldManifest, out: &Path) {
    let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return;
    };
    let mut paths: Vec<&str> = manifest
        .entities
        .iter()
        .filter_map(|e| e.mesh_asset.as_ref().map(|m| m.path.as_str()))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    let mut copied = 0usize;
    for path in paths {
        let src = assets::assets_dir().join(path);
        let dst = dir.join("assets").join(path);
        if !src.is_file() {
            eprintln!(
                "localgpt-doc: export: asset {path} not in the pack — the viewer shows a placeholder"
            );
            continue;
        }
        if dst.is_file() {
            copied += 1;
            continue;
        }
        if let Some(parent) = dst.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::copy(&src, &dst).is_ok() {
            copied += 1;
        }
    }
    if copied > 0 {
        println!(
            "copied {copied} asset(s) -> {}",
            dir.join("assets").display()
        );
    }
}

/// Read, parse, and compile a document, applying whatever recipes the
/// sidecar already holds for its sections.
fn load(path: &Path, recipes: &sidecar::RecipeStore) -> std::io::Result<CurrentWorld> {
    let src = std::fs::read_to_string(path)?;
    let fallback_title = path.file_stem().map_or_else(
        || "Untitled".into(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let doc = doc::Doc::parse(&src, &fallback_title);
    let manifest = draft::compile_with(&doc, recipes);
    Ok(CurrentWorld { doc, manifest })
}

/// `LOCALGPT_DOC_SCREENSHOT=out.png`: render the first stop offscreen once
/// shaders have warmed up, save it, and exit — a visual check that needs no
/// human and no display.
#[derive(Resource)]
struct Smoke {
    path: String,
    target: Option<Handle<Image>>,
    frame: u32,
    saved: bool,
}

impl Smoke {
    /// Frames to wait before capturing, so pipelines have compiled.
    const WARM_UP: u32 = 240;
    /// Give up after this many frames if no capture arrives.
    const TIMEOUT: u32 = 1200;

    fn new(path: String) -> Self {
        Self {
            path,
            target: None,
            frame: 0,
            saved: false,
        }
    }
}

/// Point the camera at an offscreen image the size of the normal window.
fn smoke_target(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    camera: Query<Entity, With<TourCamera>>,
    mut smoke: ResMut<Smoke>,
) {
    let image = Image::new_target_texture(1280, 800, TextureFormat::Rgba8UnormSrgb, None);
    let target = images.add(image);
    // With no window, Bevy won't pick this camera for UI or shadow LOD on its own.
    if let Ok(camera) = camera.single() {
        commands.entity(camera).insert((
            RenderTarget::Image(target.clone().into()),
            IsDefaultUiCamera,
            ShadowLodOrigin,
        ));
    }
    smoke.target = Some(target);
}

fn smoke_screenshot(
    mut commands: Commands,
    mut smoke: ResMut<Smoke>,
    mut exit: MessageWriter<AppExit>,
) {
    smoke.frame += 1;
    if smoke.frame == Smoke::WARM_UP
        && let Some(target) = smoke.target.clone()
    {
        commands
            .spawn(Screenshot::image(target))
            .observe(save_to_disk(smoke.path.clone()))
            .observe(|_: On<ScreenshotCaptured>, mut smoke: ResMut<Smoke>| smoke.saved = true);
    }
    // `save_to_disk` writes synchronously, so the file is complete by now.
    if smoke.saved {
        exit.write(AppExit::Success);
    } else if smoke.frame > Smoke::TIMEOUT {
        error!("no screenshot captured after {} frames", Smoke::TIMEOUT);
        exit.write(AppExit::error());
    }
}
