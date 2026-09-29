//! Opening things from inside the window — a document, a song or a world —
//! so the app is usable without a terminal. What `--md`, `--song` and
//! `--world` do at launch, from an "Open…" button at the top of the window.
//!
//! The native picker runs the way Verse's folder picker does: the
//! synchronous dialog cannot be called from a Bevy system on macOS (the panel
//! needs the main-thread run loop that the system is blocking), so the async
//! dialog is awaited on the task pool and its answer comes back over a
//! channel. A song's analysis and a world's import take seconds, so they run
//! on a thread too, with a status line meanwhile; a document opens at once.
//!
//! `LOCALGPT_APP_OPEN=<file>` opens a file a few seconds after launch
//! (`LOCALGPT_APP_OPEN_AFTER`, default 3 s) exactly as if it had been picked
//! — a dev hook for checking a switch between modes without a person at the
//! window.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use localgpt_core::config::Config;
use localgpt_gen::gen3d::plugin::GenInitialWorld;

use crate::document::Document;

/// What opening needs from the launch: the app's config (for the authoring
/// model) and workspace, and whether documents are authored.
#[derive(Resource)]
pub struct Launch {
    pub config: Config,
    pub workspace: PathBuf,
    pub author: bool,
}

/// What is open, for the bar.
#[derive(Resource, Default)]
pub struct Showing(pub Option<String>);

#[derive(Resource)]
struct Opener {
    tx: Sender<Opened>,
    rx: Mutex<Receiver<Opened>>,
    /// Work in progress ("Listening to …"), or the last failure.
    status: Option<String>,
    busy: bool,
}

impl Default for Opener {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx: Mutex::new(rx),
            status: None,
            busy: false,
        }
    }
}

enum Opened {
    Picked(PathBuf),
    Ready { dir: PathBuf, showing: String },
    Failed(String),
}

/// What a file is, by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Document,
    Song,
    World,
}

const DOCUMENTS: &[&str] = &["md", "markdown"];
const SONGS: &[&str] = &["mp3", "flac", "ogg", "wav", "m4a", "aac", "aif", "aiff"];
const WORLDS: &[&str] = &["ron", "json", "glb", "gltf"];

fn kind(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    if DOCUMENTS.contains(&ext.as_str()) {
        Some(Kind::Document)
    } else if SONGS.contains(&ext.as_str()) {
        Some(Kind::Song)
    } else if WORLDS.contains(&ext.as_str()) {
        Some(Kind::World)
    } else {
        None
    }
}

pub struct OpenPlugin;

impl Plugin for OpenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Opener>()
            .init_resource::<Showing>()
            .add_systems(Update, receive)
            .add_systems(EguiPrimaryContextPass, open_bar);
        if let Some(path) = std::env::var_os("LOCALGPT_APP_OPEN") {
            let after = std::env::var("LOCALGPT_APP_OPEN_AFTER")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3.0);
            app.insert_resource(OpenLater {
                path: Some(PathBuf::from(path)),
                after,
            })
            .add_systems(Update, open_later);
        }
    }
}

#[derive(Resource)]
struct OpenLater {
    path: Option<PathBuf>,
    after: f32,
}

fn open_later(time: Res<Time>, mut later: ResMut<OpenLater>, opener: Res<Opener>) {
    if time.elapsed_secs() >= later.after
        && let Some(path) = later.path.take()
    {
        let _ = opener.tx.send(Opened::Picked(path));
    }
}

/// Remember what was opened, so a bare launch reopens it. A settings write
/// is not why anyone opens a file, so failures are a log line, nothing more.
fn remember(opened: localgpt_gen::settings::LastOpened) {
    if let Err(e) = localgpt_gen::settings::update(|saved| saved.last_opened = Some(opened)) {
        warn!("couldn't remember what was opened: {e}");
    }
}

fn open_bar(mut contexts: EguiContexts, opener: Res<Opener>, showing: Res<Showing>) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    egui::Area::new(egui::Id::new("open_bar"))
        .anchor(egui::Align2::CENTER_TOP, [0.0, 10.0])
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let open = ui
                        .add_enabled(!opener.busy, egui::Button::new("Open…"))
                        .on_hover_text("A Markdown document, a song, or a world");
                    if open.clicked() {
                        pick(opener.tx.clone());
                    }
                    let weak = ui.visuals().weak_text_color();
                    match (&opener.status, &showing.0) {
                        (Some(status), _) => {
                            ui.label(egui::RichText::new(status).small().color(weak));
                        }
                        (None, Some(showing)) => {
                            ui.label(egui::RichText::new(showing).small().color(weak));
                        }
                        (None, None) => {}
                    }
                });
            });
        });
}

/// The native picker, awaited on the task pool.
fn pick(tx: Sender<Opened>) {
    let task = async move {
        let picked = rfd::AsyncFileDialog::new()
            .set_title("Open a document, a song or a world")
            .add_filter(
                "Everything LocalGPT opens",
                &[DOCUMENTS, SONGS, WORLDS].concat(),
            )
            .add_filter("Markdown", DOCUMENTS)
            .add_filter("Songs", SONGS)
            .add_filter("Worlds", WORLDS)
            .pick_file()
            .await;
        if let Some(file) = picked {
            let _ = tx.send(Opened::Picked(file.path().to_path_buf()));
        }
    };
    AsyncComputeTaskPool::get().spawn(task).detach();
}

fn receive(
    mut commands: Commands,
    mut opener: ResMut<Opener>,
    mut showing: ResMut<Showing>,
    mut initial_world: ResMut<GenInitialWorld>,
    launch: Res<Launch>,
) {
    let received: Vec<Opened> = match opener.rx.lock() {
        Ok(rx) => rx.try_iter().collect(),
        Err(_) => return,
    };
    for opened in received {
        match opened {
            Opened::Picked(path) => open(
                &path,
                &launch,
                &mut opener,
                &mut showing,
                &mut initial_world,
                &mut commands,
            ),
            Opened::Ready { dir, showing: what } => {
                info!("opened {what} ({})", dir.display());
                opener.busy = false;
                opener.status = None;
                // Leaving document mode, if it was on: the pane and its model
                // requests go with the resource.
                commands.remove_resource::<Document>();
                let dir = dir.to_string_lossy().into_owned();
                remember(localgpt_gen::settings::LastOpened::World { dir: dir.clone() });
                initial_world.path = Some(dir);
                showing.0 = Some(what);
            }
            Opened::Failed(why) => {
                opener.busy = false;
                opener.status = Some(why);
            }
        }
    }
}

fn open(
    path: &Path,
    launch: &Launch,
    opener: &mut Opener,
    showing: &mut Showing,
    initial_world: &mut GenInitialWorld,
    commands: &mut Commands,
) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match kind(path) {
        Some(Kind::Document) => {
            let config = launch.author.then_some(&launch.config);
            match crate::document::open(path, &launch.workspace, config) {
                Ok((document, dir)) => {
                    info!("opened document {name} ({})", dir.display());
                    commands.insert_resource(document);
                    initial_world.path = Some(dir.to_string_lossy().into_owned());
                    showing.0 = Some(format!("Document · {name}"));
                    remember(localgpt_gen::settings::LastOpened::Document {
                        path: path.to_string_lossy().into_owned(),
                    });
                    opener.status = None;
                }
                Err(e) => opener.status = Some(format!("Could not open {name}: {e}")),
            }
        }
        Some(Kind::Song) => {
            opener.busy = true;
            opener.status = Some(format!("Listening to {name} …"));
            let (tx, song, workspace) = (
                opener.tx.clone(),
                path.to_path_buf(),
                launch.workspace.clone(),
            );
            std::thread::spawn(move || {
                let opened = match crate::song::prepare(&song, &workspace) {
                    Ok((dir, world)) => {
                        let title = world
                            .soundtrack
                            .as_ref()
                            .and_then(|s| s.title.clone())
                            .unwrap_or(name);
                        Opened::Ready {
                            dir,
                            showing: format!("Song · {title}"),
                        }
                    }
                    Err(e) => Opened::Failed(format!("Could not open {name}: {e}")),
                };
                let _ = tx.send(opened);
            });
        }
        Some(Kind::World) => {
            opener.busy = true;
            opener.status = Some(format!("Opening {name} …"));
            let (tx, world, workspace) = (
                opener.tx.clone(),
                path.to_string_lossy().into_owned(),
                launch.workspace.clone(),
            );
            std::thread::spawn(move || {
                let imported = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())
                    .and_then(|rt| {
                        rt.block_on(localgpt_gen::gen3d::world_import::prepare(
                            &world, &workspace,
                        ))
                        .map_err(|e| e.to_string())
                    });
                let _ = tx.send(match imported {
                    Ok(imported) => Opened::Ready {
                        dir: imported.dir,
                        showing: format!("World · {name}"),
                    },
                    Err(e) => Opened::Failed(format!("Could not open {name}: {e}")),
                });
            });
        }
        None => opener.status = Some(format!("{name}: not a document, a song or a world")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_open_as_what_their_extension_says() {
        assert_eq!(kind(Path::new("notes.md")), Some(Kind::Document));
        assert_eq!(kind(Path::new("README.MARKDOWN")), Some(Kind::Document));
        assert_eq!(kind(Path::new("track.MP3")), Some(Kind::Song));
        assert_eq!(kind(Path::new("a/b/song.flac")), Some(Kind::Song));
        assert_eq!(kind(Path::new("place.world.ron")), Some(Kind::World));
        assert_eq!(kind(Path::new("scene.glb")), Some(Kind::World));
        assert_eq!(kind(Path::new("photo.png")), None);
        assert_eq!(kind(Path::new("no-extension")), None);
    }
}
