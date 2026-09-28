//! Document mode: a Markdown file, edited in the app, rebuilt as a world while
//! you type.
//!
//! The left pane holds the document's outline above an editor. Typing marks
//! the document edited; once the text has settled for [`SETTLE`], the file is
//! saved and the world rebuilt from it — MD's own pipeline, `Doc::parse` then
//! `draft::compile_with` over MD's sidecar, so scenery a model authored is
//! kept for every section whose text did not change. The rebuild goes through
//! Gen's `GenInitialWorld`, the same hook the gallery uses, with the camera
//! stripped from the manifest so the view does not snap back on every edit.
//!
//! The mapping between text and world runs both ways, which is the point of
//! editing in the app rather than beside it:
//!
//! * **text → world**: clicking an outline item flies the camera to that
//!   section's place (the draft's tour holds one stop per section, in order).
//! * **world → text**: selecting an entity in the inspector highlights the
//!   section it belongs to. MD names every entity `sNN-…` after its section,
//!   and names survive a world load where ids may not.
//!
//! The file stays the source of truth. It is polled for changes made in
//! another editor: a clean buffer reloads, an edited one is flagged as a
//! conflict and not saved, so an external edit is never silently overwritten.
//! That also makes any editor a way to drive the world, which matters while
//! the in-app one is young.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use localgpt_gen::gen3d::avatar::CameraMode;
use localgpt_gen::gen3d::plugin::GenInitialWorld;
use localgpt_gen::gen3d::registry::NameRegistry;
use localgpt_gen::inspector::InspectorSelection;
use localgpt_md::doc::Doc;
use localgpt_world_types as wt;

/// How long typing has to pause before the document is saved and rebuilt.
/// Long enough not to rebuild mid-word, short enough to feel live.
const SETTLE: Duration = Duration::from_millis(700);
/// How often the file is checked for changes made elsewhere.
const POLL: Duration = Duration::from_secs(1);
/// Camera flight time to a section's place, in seconds.
const FLIGHT_SECS: f32 = 0.7;

// ---------------------------------------------------------------------------
// Compiling a document into a world
// ---------------------------------------------------------------------------

/// Parse and compile Markdown text with MD's pipeline. The sidecar beside the
/// file (`doc.world.json`) supplies scenery a model already authored; it is
/// read, never written — authoring stays MD's until its worker lands here.
pub fn compile(text: &str, md: &Path) -> (Doc, wt::WorldManifest) {
    let title = md
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".into());
    let doc = Doc::parse(text, &title);
    let store = localgpt_md::RecipeStore::load(&md.with_extension("world.json"));
    let world = localgpt_md::draft::compile_with(&doc, &store);
    for issue in localgpt_md::draft::validate(&world) {
        warn!("document: {:?}: {}", issue.severity, issue.message);
    }
    (doc, world)
}

/// Write a compiled world into the app-owned folder for its document, and
/// link the shared asset pack if it uses pack models.
pub fn write_world(world: &wt::WorldManifest, dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    let ron = ron::ser::to_string_pretty(world, ron::ser::PrettyConfig::default())?;
    std::fs::write(dir.join("world.ron"), ron)?;
    link_asset_pack(world, dir);
    Ok(())
}

/// The folder a document's world lives in: `<workspace>/documents/<key>`.
///
/// App-owned and overwritten on every build, because the document is the
/// source of truth. Routing it through `world_import` instead would reopen a
/// stale world after an edit (same slug, "already imported") and would file
/// it under `skills/`, where Gen's gallery lists *saved* worlds.
pub fn world_dir(workspace: &Path, md: &Path) -> PathBuf {
    workspace.join("documents").join(document_key(md))
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
/// use it, the trick `scripts/fetch-assets.sh` plays for MD and Verse. A
/// draft with no meshes needs nothing, and a missing pack only costs the
/// placeholder boxes Gen already shows for a missing model.
fn link_asset_pack(world: &wt::WorldManifest, dir: &Path) {
    if !world.entities.iter().any(|e| e.mesh_asset.is_some()) {
        return;
    }
    let pack = localgpt_md::assets::assets_dir();
    if !pack.join("models/manifest.json").is_file() {
        warn!("document uses pack models but no pack is installed — run scripts/fetch-assets.sh");
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
            warn!("could not link the asset pack: {e}");
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

// ---------------------------------------------------------------------------
// The outline
// ---------------------------------------------------------------------------

/// One row of the outline: a heading, and the section (place) it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineItem {
    /// 1 for the title, 2 for a section, 3+ for a sub-heading within one.
    pub level: u8,
    pub text: String,
    /// The section whose place this heading lives in; `None` for the title.
    pub section: Option<usize>,
}

/// The outline of a document: the title, then each section — exactly the
/// sections `Doc::parse` made places of, so the outline cannot disagree with
/// the world — each followed by the sub-headings inside it.
///
/// MD's parser is the ground truth for what a section *is* (its rules differ
/// by genre: `##` in a world, `---` slides in a deck). Only the sub-headings
/// come from a scan of the raw text, matched in order against the section
/// headings; a heading the scan cannot match is shown as a sub-heading of the
/// section before it, which is harmless.
pub fn outline(text: &str, doc: &Doc) -> Vec<OutlineItem> {
    let raw = raw_headings(text);
    let mut items = vec![OutlineItem {
        level: 1,
        text: doc.title.clone(),
        section: None,
    }];
    let mut cursor = 0;
    for (index, section) in doc.sections.iter().enumerate() {
        let want = normalize(&section.heading);
        let found = raw[cursor..]
            .iter()
            .position(|(_, h)| normalize(h) == want)
            .map(|offset| cursor + offset);
        // Headings between the previous section's and this one belong to the
        // previous section (or, before the first, are the title — skipped).
        if let Some(at) = found {
            if index > 0 {
                push_subheadings(&mut items, &raw[cursor..at], index - 1);
            }
            cursor = at + 1;
        }
        items.push(OutlineItem {
            level: 2,
            text: section.heading.clone(),
            section: Some(index),
        });
    }
    if !doc.sections.is_empty() {
        push_subheadings(&mut items, &raw[cursor..], doc.sections.len() - 1);
    }
    items
}

fn push_subheadings(items: &mut Vec<OutlineItem>, raw: &[(u8, String)], section: usize) {
    for (level, text) in raw {
        // A title-level heading between sections is the document title
        // already shown at the top, not structure within a section.
        if *level <= 1 {
            continue;
        }
        items.push(OutlineItem {
            level: (*level).max(3),
            text: text.clone(),
            section: Some(section),
        });
    }
}

/// ATX headings outside fenced code, as `(level, text)`.
fn raw_headings(text: &str) -> Vec<(u8, String)> {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        for marker in ["```", "~~~"] {
            if trimmed.starts_with(marker) {
                fence = match fence {
                    Some(open) if open == marker => None,
                    None => Some(marker),
                    other => other,
                };
            }
        }
        if fence.is_some() || trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            continue;
        }
        let level = trimmed.bytes().take_while(|b| *b == b'#').count();
        if (1..=6).contains(&level) && trimmed[level..].starts_with([' ', '\t']) {
            let heading = trimmed[level..].trim().trim_end_matches('#').trim();
            if !heading.is_empty() {
                out.push((level as u8, heading.to_string()));
            }
        }
    }
    out
}

/// Heading text for matching: pulldown-cmark renders `## The **Big** Idea` as
/// "The Big Idea", so inline markup and case are dropped before comparing.
fn normalize(heading: &str) -> String {
    heading
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The section an entity belongs to, from MD's `sNN-…` naming: `s01` is the
/// first section. `None` for the ground, the sun, and anything not MD's.
pub fn section_of(name: &str) -> Option<usize> {
    let rest = name.strip_prefix('s')?;
    let (digits, tail) = rest.split_at(rest.find(|c: char| !c.is_ascii_digit())?);
    if digits.is_empty() || !tail.starts_with('-') {
        return None;
    }
    digits.parse::<usize>().ok()?.checked_sub(1)
}

// ---------------------------------------------------------------------------
// The live document
// ---------------------------------------------------------------------------

/// A camera stop for one section, from the draft's tour.
#[derive(Debug, Clone, Copy)]
struct Stop {
    position: Vec3,
    look_at: Vec3,
}

#[derive(Resource)]
pub struct Document {
    path: PathBuf,
    world_dir: PathBuf,
    /// The editor buffer.
    text: String,
    /// The last text read from or written to the file.
    on_disk: String,
    /// The text the current world was built from.
    built: String,
    disk_mtime: Option<SystemTime>,
    edited_at: Option<Instant>,
    polled_at: Instant,
    outline: Vec<OutlineItem>,
    stops: Vec<Stop>,
    selected: Option<usize>,
    status: String,
    conflict: bool,
    open: bool,
}

impl Document {
    /// Open a document whose world was just built from `text` at `world_dir`.
    pub fn new(
        path: PathBuf,
        world_dir: PathBuf,
        text: String,
        doc: &Doc,
        world: &wt::WorldManifest,
    ) -> Self {
        let disk_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let mut document = Self {
            path,
            world_dir,
            on_disk: text.clone(),
            built: text.clone(),
            text,
            disk_mtime,
            edited_at: None,
            polled_at: Instant::now(),
            outline: Vec::new(),
            stops: Vec::new(),
            selected: None,
            status: String::new(),
            conflict: false,
            open: true,
        };
        document.refresh(doc, world);
        document.status = format!("{} sections", doc.sections.len());
        document
    }

    fn refresh(&mut self, doc: &Doc, world: &wt::WorldManifest) {
        self.outline = outline(&self.text, doc);
        self.stops = world
            .tours
            .first()
            .map(|tour| {
                tour.waypoints
                    .iter()
                    .map(|w| Stop {
                        position: Vec3::from_array(w.position),
                        look_at: Vec3::from_array(w.look_at),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if self.selected.is_some_and(|s| s >= self.stops.len()) {
            self.selected = None;
        }
    }

    fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// An in-progress camera flight to a section's place.
#[derive(Resource, Default)]
struct Flight {
    target: Option<Stop>,
    from: Option<Transform>,
    t: f32,
}

pub struct DocumentPlugin;

impl Plugin for DocumentPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Flight>()
            .add_systems(
                Update,
                (
                    watch_file,
                    rebuild_when_settled,
                    follow_selection,
                    fly_camera,
                )
                    .run_if(resource_exists::<Document>),
            )
            .add_systems(
                EguiPrimaryContextPass,
                document_panel.run_if(resource_exists::<Document>),
            );
    }
}

/// Save and rebuild once typing has settled.
fn rebuild_when_settled(mut doc: ResMut<Document>, mut initial_world: ResMut<GenInitialWorld>) {
    let Some(edited_at) = doc.edited_at else {
        return;
    };
    if edited_at.elapsed() < SETTLE {
        return;
    }
    doc.edited_at = None;

    if doc.text != doc.on_disk && !doc.conflict {
        match save(&doc.path, &doc.text, &doc.on_disk) {
            Ok(Saved::Written(mtime)) => {
                doc.on_disk = doc.text.clone();
                doc.disk_mtime = mtime;
            }
            Ok(Saved::ChangedOnDisk) => {
                doc.conflict = true;
                doc.status = "Changed on disk by another editor — not saved".into();
                return;
            }
            Err(e) => {
                doc.status = format!("Could not save: {e}");
                return;
            }
        }
    }

    if doc.text == doc.built {
        return;
    }
    let (parsed, mut world) = compile(&doc.text, &doc.path);
    // Leave the camera where the writer put it; only the first build places it.
    world.camera = None;
    if let Err(e) = write_world(&world, &doc.world_dir) {
        doc.status = format!("Could not build the world: {e}");
        return;
    }
    initial_world.path = Some(doc.world_dir.to_string_lossy().into_owned());
    doc.built = doc.text.clone();
    doc.refresh(&parsed, &world);
    doc.status = format!(
        "Saved · {} section{}",
        parsed.sections.len(),
        if parsed.sections.len() == 1 { "" } else { "s" }
    );
}

enum Saved {
    Written(Option<SystemTime>),
    ChangedOnDisk,
}

/// Write `text` over the file, but only if the file still holds `expected` —
/// what this editor last read or wrote. Atomic: a temp file in the same
/// directory, then a rename, so a crash mid-save cannot truncate the document.
fn save(path: &Path, text: &str, expected: &str) -> std::io::Result<Saved> {
    match std::fs::read_to_string(path) {
        Ok(current) if current != expected => return Ok(Saved::ChangedOnDisk),
        Ok(_) => {}
        // A deleted file is re-created rather than treated as a conflict.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document.md".into());
    let tmp = path.with_file_name(format!(".{name}.localgpt-save"));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(Saved::Written(
        std::fs::metadata(path).and_then(|m| m.modified()).ok(),
    ))
}

/// Pick up edits made in another editor.
fn watch_file(mut doc: ResMut<Document>) {
    if doc.polled_at.elapsed() < POLL {
        return;
    }
    doc.polled_at = Instant::now();
    let Ok(mtime) = std::fs::metadata(&doc.path).and_then(|m| m.modified()) else {
        return;
    };
    if Some(mtime) == doc.disk_mtime {
        return;
    }
    doc.disk_mtime = Some(mtime);
    let Ok(current) = std::fs::read_to_string(&doc.path) else {
        return;
    };
    if current == doc.on_disk {
        return;
    }
    if doc.text == doc.on_disk {
        // Clean buffer: follow the file, and rebuild on the next tick.
        doc.text = current.clone();
        doc.on_disk = current;
        doc.conflict = false;
        doc.edited_at = Some(Instant::now() - SETTLE);
        doc.status = "Reloaded — changed in another editor".into();
    } else {
        doc.conflict = true;
        doc.status = "Changed on disk by another editor — not saved".into();
    }
}

/// World → text: selecting an entity highlights the section it belongs to.
fn follow_selection(
    selection: Option<Res<InspectorSelection>>,
    names: Query<&Name>,
    mut doc: ResMut<Document>,
) {
    let Some(selection) = selection else {
        return;
    };
    if !selection.is_changed() {
        return;
    }
    let section = selection
        .entity
        .and_then(|entity| names.get(entity).ok())
        .and_then(|name| section_of(name.as_str()));
    if section.is_some() {
        doc.selected = section;
    }
}

/// Text → world: ease the camera to the selected section's place.
fn fly_camera(
    time: Res<Time>,
    mode: Option<Res<CameraMode>>,
    registry: Res<NameRegistry>,
    mut flight: ResMut<Flight>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(target) = flight.target else {
        return;
    };
    // In Player mode the camera follows a character; flying it would fight.
    if mode.is_some_and(|m| *m != CameraMode::FreeFly) {
        flight.target = None;
        return;
    }
    let Some(camera) = registry.get_entity("main_camera") else {
        return;
    };
    let Ok(mut transform) = transforms.get_mut(camera) else {
        return;
    };
    let from = *flight.from.get_or_insert(*transform);
    let to = Transform::from_translation(target.position).looking_at(target.look_at, Vec3::Y);
    flight.t = (flight.t + time.delta_secs() / FLIGHT_SECS).min(1.0);
    let s = flight.t * flight.t * (3.0 - 2.0 * flight.t); // smoothstep
    transform.translation = from.translation.lerp(to.translation, s);
    transform.rotation = from.rotation.slerp(to.rotation, s);
    if flight.t >= 1.0 {
        *flight = Flight::default();
    }
}

fn document_panel(
    mut contexts: EguiContexts,
    mut doc: ResMut<Document>,
    mut flight: ResMut<Flight>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if ctx.input(|input| input.key_pressed(egui::Key::F3)) {
        doc.open = !doc.open;
    }
    if !doc.open {
        egui::Area::new(egui::Id::new("document_reopen"))
            .anchor(egui::Align2::LEFT_BOTTOM, [14.0, -14.0])
            .show(ctx, |ui| {
                if ui.button("Document (F3)").clicked() {
                    doc.open = true;
                }
            });
        return;
    }

    let doc = &mut *doc;
    // bevy_egui hands systems a Context, and egui 0.34 deprecates top-level
    // panels in favour of show_inside(ui); Gen's panels do the same.
    #[allow(deprecated)]
    egui::Panel::left("localgpt_document")
        .default_size(420.0)
        .min_size(280.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(doc.file_name()).strong().size(15.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("Hide")
                        .on_hover_text("Hide this pane (F3)")
                        .clicked()
                    {
                        doc.open = false;
                    }
                });
            });
            let status = if doc.edited_at.is_some() {
                "Editing…".to_string()
            } else {
                doc.status.clone()
            };
            let color = if doc.conflict {
                egui::Color32::from_rgb(214, 120, 60)
            } else {
                ui.visuals().weak_text_color()
            };
            ui.label(egui::RichText::new(status).small().color(color));
            ui.separator();

            ui.label(egui::RichText::new("Outline").small().strong());
            egui::ScrollArea::vertical()
                .id_salt("document_outline")
                .max_height(ui.available_height() * 0.3)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for item in &doc.outline {
                        let selected = item.section.is_some() && item.section == doc.selected;
                        let indent = 12.0 * f32::from(item.level.saturating_sub(1));
                        let mut text = egui::RichText::new(&item.text);
                        text = match item.level {
                            1 => text.strong(),
                            2 => text,
                            _ => text.small(),
                        };
                        ui.horizontal(|ui| {
                            ui.add_space(indent);
                            let clicked = ui.selectable_label(selected, text).clicked();
                            if clicked && let Some(section) = item.section {
                                doc.selected = Some(section);
                                if let Some(stop) = doc.stops.get(section) {
                                    *flight = Flight {
                                        target: Some(*stop),
                                        ..Default::default()
                                    };
                                }
                            }
                        });
                    }
                });
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("document_editor")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut doc.text)
                            .id(egui::Id::new("document_text"))
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(24)
                            .lock_focus(true),
                    );
                    if response.changed() {
                        doc.edited_at = Some(Instant::now());
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Doc {
        Doc::parse(text, "fallback.md")
    }

    #[test]
    fn outline_follows_the_sections_mds_parser_made() {
        let text = "# Trip\n\n## Sea\n\nsalt\n\n### Tides\n\nin, out\n\n## Peak\n\nair\n";
        let items = outline(text, &parse(text));
        let shape: Vec<(u8, &str, Option<usize>)> = items
            .iter()
            .map(|i| (i.level, i.text.as_str(), i.section))
            .collect();
        assert_eq!(
            shape,
            [
                (1, "Trip", None),
                (2, "Sea", Some(0)),
                (3, "Tides", Some(0)),
                (2, "Peak", Some(1)),
            ]
        );
    }

    #[test]
    fn inline_markup_in_a_heading_still_matches() {
        let text = "# T\n\n## The **Big** `Idea`\n\nbody\n\n### Sub\n";
        let items = outline(text, &parse(text));
        assert_eq!(items[1].section, Some(0));
        assert_eq!(items.last().unwrap().text, "Sub");
        assert_eq!(items.last().unwrap().section, Some(0));
    }

    #[test]
    fn headings_inside_code_fences_are_not_structure() {
        let text = "# T\n\n## Real\n\n```md\n## Not a heading\n```\n";
        let items = outline(text, &parse(text));
        assert!(items.iter().all(|i| i.text != "Not a heading"), "{items:?}");
    }

    #[test]
    fn entity_names_map_back_to_sections() {
        assert_eq!(section_of("s01-landmark"), Some(0));
        assert_eq!(section_of("s12-prop-3"), Some(11));
        assert_eq!(section_of("sun"), None);
        assert_eq!(section_of("ground"), None);
        assert_eq!(section_of("s00-x"), None, "sections count from s01");
        assert_eq!(section_of("sx-1"), None);
    }

    #[test]
    fn saving_refuses_to_overwrite_an_external_edit() {
        let dir = std::env::temp_dir().join(format!("localgpt-app-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.md");
        std::fs::write(&path, "original").unwrap();

        // Someone else edits the file after we read "original".
        std::fs::write(&path, "theirs").unwrap();
        assert!(matches!(
            save(&path, "mine", "original").unwrap(),
            Saved::ChangedOnDisk
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs");

        // With the file as we left it, the save goes through, atomically.
        assert!(matches!(
            save(&path, "mine", "theirs").unwrap(),
            Saved::Written(_)
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine");
        assert!(
            !dir.join(".doc.md.localgpt-save").exists(),
            "temp file left behind"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn document_keys_separate_same_named_files() {
        let a = document_key(Path::new("/one/README.md"));
        let b = document_key(Path::new("/two/README.md"));
        assert!(a.starts_with("readme-") && b.starts_with("readme-"));
        assert_ne!(a, b);
    }
}
