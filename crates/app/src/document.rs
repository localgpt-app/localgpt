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
//!
//! **A model authors each place** ([`crate::authoring`]). The draft is on
//! screen the moment a section settles; the section is then queued for the
//! app's model, and its build replaces the draft when it lands, stored in MD's
//! sidecar so it survives restarts and every edit elsewhere in the document.
//! Editing a section changes its hash, which queues it again — ahead of the
//! rest, since it is the one being looked at. The outline labels each section
//! with where its place came from: the writer's own ```world fence, the
//! model, or the draft.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use localgpt_gen::gen3d::avatar::CameraMode;
use localgpt_gen::gen3d::plugin::GenInitialWorld;
use localgpt_gen::gen3d::registry::NameRegistry;
use localgpt_gen::inspector::{InspectorMode, InspectorSelection, InspectorState};
use localgpt_md::doc::Doc;
use localgpt_md::sidecar::BuildEntry;

use crate::authoring::Authored;
use localgpt_world_types as wt;

/// How long typing has to pause before the document is saved and rebuilt.
/// Long enough not to rebuild mid-word, short enough to feel live.
const SETTLE: Duration = Duration::from_millis(700);
/// How often the file is checked for changes made elsewhere.
const POLL: Duration = Duration::from_secs(1);
/// Camera flight time to a section's place, in seconds.
const FLIGHT_SECS: f32 = 0.7;
/// How far from a thing the camera stops when one is picked on the card.
const THING_VIEW_DISTANCE: f32 = 7.0;
/// The most kinds of thing the place card lists before "+N more".
const MAX_THINGS: usize = 18;

// ---------------------------------------------------------------------------
// Compiling a document into a world
// ---------------------------------------------------------------------------

/// Parse and compile Markdown text with MD's pipeline. The sidecar beside the
/// file (`doc.world.json`) supplies the places a model already authored — the
/// app's worker, or MD's own `--generate`; they share the file.
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
/// Open `md` as a live document: compile it with MD's pipeline, write its
/// first world (camera included — it places the view once) to its folder,
/// and return the document with that folder. With a `config`, the app's
/// model authors each section in the background; the draft is on screen
/// meanwhile.
pub fn open(
    md: &Path,
    workspace: &Path,
    config: Option<&localgpt_core::config::Config>,
) -> anyhow::Result<(Document, PathBuf)> {
    let text = std::fs::read_to_string(md)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", md.display()))?;
    let (doc, world) = compile(&text, md);
    let dir = world_dir(workspace, md);
    write_world(&world, &dir)?;
    tracing::info!(
        "{} -> {} ({} entities, {} sections)",
        md.display(),
        dir.display(),
        world.entities.len(),
        doc.sections.len(),
    );
    let mut live = Document::new(md.to_path_buf(), dir.clone(), text, &doc, &world);
    if let Some(config) = config {
        let manifest = localgpt_md::assets::read_manifest_from_disk();
        live = live.with_authoring(crate::authoring::Worker::spawn(config, manifest), &doc);
    }
    Ok((live, dir))
}

pub fn world_dir(workspace: &Path, md: &Path) -> PathBuf {
    workspace.join("documents").join(document_key(md))
}

/// The folder name for a document: its stem, plus a hash of where it lives,
/// so two files both called README.md never share a world. FNV-1a rather
/// than `DefaultHasher`, whose output is not promised across Rust releases.
/// A stable folder name for a source file: its slugged stem plus a hash of
/// its full path, so two `README.md`s in different places never share one.
pub(crate) fn document_key(md: &Path) -> String {
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
    /// The heading's line in the text, from 0. `None` for the title and for a
    /// section the heading scan could not place (a deck slide with no
    /// heading), which the editor's cursor then cannot land in.
    pub line: Option<usize>,
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
        line: None,
    }];
    let mut cursor = 0;
    for (index, section) in doc.sections.iter().enumerate() {
        let want = normalize(&section.heading);
        let found = raw[cursor..]
            .iter()
            .position(|heading| normalize(&heading.text) == want)
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
            line: found.map(|at| raw[at].line),
        });
    }
    if !doc.sections.is_empty() {
        push_subheadings(&mut items, &raw[cursor..], doc.sections.len() - 1);
    }
    items
}

fn push_subheadings(items: &mut Vec<OutlineItem>, raw: &[RawHeading], section: usize) {
    for heading in raw {
        // A title-level heading between sections is the document title
        // already shown at the top, not structure within a section.
        if heading.level <= 1 {
            continue;
        }
        items.push(OutlineItem {
            level: heading.level.max(3),
            text: heading.text.clone(),
            section: Some(section),
            line: Some(heading.line),
        });
    }
}

/// An ATX heading found by scanning the text.
struct RawHeading {
    line: usize,
    level: u8,
    text: String,
}

/// ATX headings outside fenced code, in order.
fn raw_headings(text: &str) -> Vec<RawHeading> {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    for (number, line) in text.lines().enumerate() {
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
                out.push(RawHeading {
                    line: number,
                    level: level as u8,
                    text: heading.to_string(),
                });
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

/// The section a line of the text belongs to: that of the last outline
/// heading at or above it. `None` above the first section (the title).
fn section_at_line(outline: &[OutlineItem], line: usize) -> Option<usize> {
    outline
        .iter()
        .rev()
        .filter(|item| item.line.is_some_and(|at| at <= line))
        .find_map(|item| item.section)
}

/// The line a character offset falls on — egui's cursor counts characters,
/// not bytes.
fn line_of_char(text: &str, char_index: usize) -> usize {
    text.chars().take(char_index).filter(|c| *c == '\n').count()
}

/// The character offset where `line` starts, or the end of the text.
fn char_of_line(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut newlines = 0;
    for (index, c) in text.chars().enumerate() {
        if c == '\n' {
            newlines += 1;
            if newlines == line {
                return index + 1;
            }
        }
    }
    text.chars().count()
}

/// One kind of thing in a place, for the place card.
#[derive(Debug, Clone, PartialEq)]
struct Thing {
    /// What it is, from its name: `s01-fishing_boat_2` is "fishing boat".
    label: String,
    /// The first entity of the kind, the one a click selects.
    first: String,
    count: usize,
}

/// What a place is made of: its entities grouped by what they are, in the
/// order the place defines them.
fn things<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<Thing> {
    let mut out: Vec<Thing> = Vec::new();
    for name in names {
        let label = thing_label(name);
        match out.iter_mut().find(|thing| thing.label == label) {
            Some(thing) => thing.count += 1,
            None => out.push(Thing {
                label,
                first: name.to_string(),
                count: 1,
            }),
        }
    }
    out
}

fn thing_label(name: &str) -> String {
    let base = match section_of(name) {
        Some(_) => name.split_once('-').map_or(name, |(_, rest)| rest),
        None => name,
    };
    // A trailing counter is how a place numbers its copies: `boat_2`, `prop-3`.
    let trimmed = base
        .trim_end_matches(|c: char| c.is_ascii_digit())
        .trim_end_matches(['_', '-', ' ']);
    let label = if trimmed.is_empty() { base } else { trimmed };
    label.replace(['_', '-'], " ")
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

/// Where a section's place came from — MD's tier chain, as the outline
/// shows it. A fence outranks the model, the model the recipe, the recipe
/// the draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    /// The writer's own ```world fence.
    Fence,
    /// Queued for, or with, the model.
    Authoring,
    /// A build the model authored, from the sidecar.
    Model,
    /// A recipe from MD's lighter model tier.
    Recipe,
    /// The model could not build it; the draft stands.
    Failed,
    /// The rule-derived draft.
    Draft,
}

impl Tier {
    fn label(self) -> &'static str {
        match self {
            Tier::Fence => "fence",
            Tier::Authoring => "authoring…",
            Tier::Model => "model",
            Tier::Recipe => "recipe",
            Tier::Failed => "draft · failed",
            Tier::Draft => "draft",
        }
    }
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
    /// The model authoring each section's place, when there is one.
    authoring: Option<crate::authoring::Worker>,
    /// Author sections as they settle — the pane's toggle.
    author_enabled: bool,
    /// Sections queued for the model or with it, by hash.
    pending: HashSet<blake3::Hash>,
    /// Sections that failed this run. Not retried until their text changes
    /// (a new hash), so one bad section cannot loop on requests.
    failed: HashSet<blake3::Hash>,
    /// The last authoring problem, shown in the pane.
    author_error: Option<String>,
    /// Rebuild although the text is unchanged: a build landed in the sidecar.
    force_rebuild: bool,
    /// Each built section's tier, by section index.
    tiers: Vec<Tier>,
    /// Each built section's entity names, by section index.
    places: Vec<Vec<String>>,
    /// The model's one-line description of each section's place, if any.
    descriptions: Vec<Option<String>>,
    /// The editor's cursor, as a character offset, when last seen.
    caret: Option<usize>,
    /// The section the cursor was last in.
    caret_section: Option<usize>,
    /// Moving the cursor into another section flies the camera there.
    follow_cursor: bool,
    /// Scroll the editor to this line on the next frame (an outline click).
    scroll_to_line: Option<usize>,
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
            authoring: None,
            author_enabled: true,
            pending: HashSet::new(),
            failed: HashSet::new(),
            author_error: None,
            force_rebuild: false,
            tiers: Vec::new(),
            places: Vec::new(),
            descriptions: Vec::new(),
            caret: None,
            caret_section: None,
            follow_cursor: true,
            scroll_to_line: None,
        };
        document.refresh(doc, world);
        document.sync_authoring(doc, false);
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
        self.places = vec![Vec::new(); doc.sections.len()];
        for entity in &world.entities {
            let name = entity.name.as_str();
            if let Some(place) = section_of(name).and_then(|s| self.places.get_mut(s)) {
                place.push(name.to_string());
            }
        }
    }

    fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Author places with `worker`, starting with every section of `doc`
    /// that has no build yet, in document order.
    pub fn with_authoring(mut self, worker: crate::authoring::Worker, doc: &Doc) -> Self {
        self.authoring = Some(worker);
        self.sync_authoring(doc, false);
        self
    }

    /// MD's cache, beside the document: where authored builds live.
    fn sidecar_path(&self) -> PathBuf {
        self.path.with_extension("world.json")
    }

    /// Bring authoring in line with `doc` as just built. Requests still
    /// waiting for sections that changed or went away are dropped (a request
    /// already running finishes, and its result is pruned on save); every
    /// section with nothing better than the draft is queued — not one with a
    /// ```world fence, which is the writer's own place and outranks any model;
    /// and each section is labelled with its tier. `first` sends the new
    /// requests ahead of the queue: after an edit, they are the sections the
    /// writer just touched.
    fn sync_authoring(&mut self, doc: &Doc, first: bool) {
        let store = localgpt_md::RecipeStore::load(&self.sidecar_path());
        let live: HashSet<blake3::Hash> = doc.sections.iter().map(|s| s.hash).collect();
        self.failed.retain(|hash| live.contains(hash));
        if let Some(worker) = &self.authoring {
            for hash in worker.retain(&live) {
                self.pending.remove(&hash);
            }
        }
        self.descriptions = doc
            .sections
            .iter()
            .map(|section| {
                store
                    .get_build(&section.hash)
                    .and_then(|build| build.description.clone())
            })
            .collect();
        let worker = self.authoring.as_ref().filter(|_| self.author_enabled);
        self.tiers.clear();
        for section in &doc.sections {
            let hash = section.hash;
            let tier = if section.world.is_some() {
                Tier::Fence
            } else if self.pending.contains(&hash) {
                Tier::Authoring
            } else if store.get_build(&hash).is_some() {
                Tier::Model
            } else if self.failed.contains(&hash) {
                Tier::Failed
            } else if let Some(worker) = worker {
                worker.submit(job_for(doc, section), first);
                self.pending.insert(hash);
                Tier::Authoring
            } else if store.get(&hash).is_some() {
                Tier::Recipe
            } else {
                Tier::Draft
            };
            self.tiers.push(tier);
        }
    }

    /// Ask the model for section `index` again — a place the writer did not
    /// like, or one that failed. The current place stays until the new one
    /// lands and replaces it.
    fn reauthor(&mut self, index: usize) {
        let doc = Doc::parse(&self.built, &self.file_name());
        let (Some(worker), Some(section)) = (&self.authoring, doc.sections.get(index)) else {
            return;
        };
        if section.world.is_some() || self.pending.contains(&section.hash) {
            return;
        }
        worker.submit(job_for(&doc, section), true);
        self.failed.remove(&section.hash);
        self.pending.insert(section.hash);
        if let Some(tier) = self.tiers.get_mut(index) {
            *tier = Tier::Authoring;
        }
    }

    /// The pane's toggle. Off drops every waiting request (one already
    /// running still lands); on queues whatever is missing.
    fn set_authoring(&mut self, enabled: bool) {
        self.author_enabled = enabled;
        if !enabled && let Some(worker) = &self.authoring {
            for hash in worker.retain(&HashSet::new()) {
                self.pending.remove(&hash);
            }
        }
        let doc = Doc::parse(&self.built, &self.file_name());
        self.sync_authoring(&doc, false);
    }

    /// One line on what the model is doing, for the pane.
    fn authoring_summary(&self) -> Option<String> {
        let worker = self.authoring.as_ref()?;
        let count = |tier: Tier| self.tiers.iter().filter(|t| **t == tier).count();
        let (built, busy, failed) = (
            count(Tier::Model),
            count(Tier::Authoring),
            count(Tier::Failed),
        );
        let authorable = self.tiers.len() - count(Tier::Fence);
        let mut line = format!("{built} of {authorable} places by {}", worker.model);
        if busy > 0 {
            line.push_str(&format!(" · {busy} to go"));
        }
        if failed > 0 {
            line.push_str(&format!(" · {failed} failed"));
        }
        Some(line)
    }

    /// A section's heading, as the outline shows it.
    fn heading(&self, section: usize) -> Option<&str> {
        self.outline
            .iter()
            .find(|item| item.level == 2 && item.section == Some(section))
            .map(|item| item.text.as_str())
    }

    /// Select a section and fly to its place.
    fn go_to(&mut self, section: usize, flight: &mut Flight) {
        self.selected = Some(section);
        if let Some(stop) = self.stops.get(section) {
            *flight = Flight {
                target: Some(*stop),
                ..Default::default()
            };
        }
    }
}

fn job_for(doc: &Doc, section: &localgpt_md::Section) -> crate::authoring::Job {
    crate::authoring::Job {
        hash: section.hash,
        heading: section.heading.clone(),
        body: section.body.clone(),
        genre: doc.genre().to_string(),
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
                    receive_builds,
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

    let forced = std::mem::take(&mut doc.force_rebuild);
    let text_changed = doc.text != doc.built;
    if !text_changed && !forced {
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
    doc.sync_authoring(&parsed, true);
    if text_changed {
        doc.status = format!(
            "Saved · {} section{}",
            parsed.sections.len(),
            if parsed.sections.len() == 1 { "" } else { "s" }
        );
    }
}

/// Put finished builds into MD's sidecar, then rebuild so they show.
fn receive_builds(mut doc: ResMut<Document>) {
    let doc = &mut *doc;
    let Some(worker) = &doc.authoring else {
        return;
    };
    let outcomes = worker.drain();
    if outcomes.is_empty() {
        return;
    }
    let model = worker.model.clone();
    let mut store = localgpt_md::RecipeStore::load(&doc.sidecar_path());
    let mut landed = Vec::new();
    for outcome in outcomes {
        doc.pending.remove(&outcome.hash);
        match outcome.result {
            Authored::Built(build) => {
                store.insert_build(
                    &outcome.hash,
                    BuildEntry {
                        model: model.clone(),
                        description: build.description,
                        entities: build.entities,
                    },
                );
                landed.push(outcome.heading);
            }
            Authored::Empty => {
                doc.author_error = Some(format!(
                    "the model's plan for “{}” built nothing — the draft stands",
                    outcome.heading
                ));
                doc.failed.insert(outcome.hash);
            }
            Authored::Failed(e) => {
                warn!("authoring “{}”: {e}", outcome.heading);
                doc.author_error = Some(e);
                doc.failed.insert(outcome.hash);
            }
        }
    }
    if landed.is_empty() {
        // Nothing to rebuild; only the outline's labels change.
        let built = Doc::parse(&doc.built, &doc.file_name());
        doc.sync_authoring(&built, false);
        return;
    }
    // Saving prunes sections the document no longer has, so it needs the
    // document as it stands now, not as it was when the request went out: a
    // build for a section edited since is dropped here, and its new text is
    // already queued.
    let current = Doc::parse(&doc.text, &doc.file_name());
    if let Err(e) = store.save(&current) {
        doc.author_error = Some(format!(
            "could not save {}: {e}",
            doc.sidecar_path().display()
        ));
        return;
    }
    doc.author_error = None;
    doc.status = match landed.as_slice() {
        [one] => format!("Placed “{one}”"),
        many => format!("Placed {} sections", many.len()),
    };
    // Rebuild on the next settle check — at once, unless the writer is
    // mid-edit, in which case the edit's own rebuild picks the build up.
    doc.force_rebuild = true;
    if doc.edited_at.is_none() {
        doc.edited_at = Some(Instant::now() - SETTLE);
    }
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

#[allow(clippy::too_many_arguments)]
fn document_panel(
    mut contexts: EguiContexts,
    mut doc: ResMut<Document>,
    mut flight: ResMut<Flight>,
    registry: Res<NameRegistry>,
    selection: Option<ResMut<InspectorSelection>>,
    inspector: Option<ResMut<InspectorState>>,
    transforms: Query<&GlobalTransform>,
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
    // What the pane asked for this frame, applied once it is drawn.
    let mut go_to: Option<(usize, Option<usize>)> = None;
    let mut again = None;
    let mut picked: Option<String> = None;
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
            if let Some(summary) = doc.authoring_summary() {
                let mut enabled = doc.author_enabled;
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut enabled, "Author")
                        .on_hover_text(
                            "Have the model build each section's place. Off, sections keep \
                             what they have and new text gets the draft.",
                        )
                        .changed()
                    {
                        doc.set_authoring(enabled);
                    }
                    ui.label(
                        egui::RichText::new(summary)
                            .small()
                            .color(ui.visuals().weak_text_color()),
                    );
                });
                if let Some(error) = &doc.author_error {
                    ui.label(
                        egui::RichText::new(error)
                            .small()
                            .color(egui::Color32::from_rgb(214, 120, 60)),
                    );
                }
            }
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Outline").small().strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(
                        &mut doc.follow_cursor,
                        egui::RichText::new("Follow cursor").small(),
                    )
                    .on_hover_text("Fly to a section's place when the cursor moves into it");
                });
            });
            egui::ScrollArea::vertical()
                .id_salt("document_outline")
                .max_height(ui.available_height() * 0.25)
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
                        // The tier sits on the section's own row, not its
                        // sub-headings': it is one place either way.
                        let tier = item
                            .section
                            .filter(|_| item.level == 2)
                            .and_then(|section| doc.tiers.get(section).copied());
                        ui.horizontal(|ui| {
                            ui.add_space(indent);
                            let response = ui.selectable_label(selected, text);
                            if response.clicked()
                                && let Some(section) = item.section
                            {
                                go_to = Some((section, item.line));
                            }
                            let Some(tier) = tier else {
                                return;
                            };
                            // A fence is the writer's own place, and one
                            // already authoring has a request out.
                            if doc.authoring.is_some()
                                && !matches!(tier, Tier::Fence | Tier::Authoring)
                            {
                                response.context_menu(|ui| {
                                    if ui.button("Author this place again").clicked() {
                                        again = item.section;
                                    }
                                });
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let color = match tier {
                                        Tier::Failed => egui::Color32::from_rgb(214, 120, 60),
                                        _ => ui.visuals().weak_text_color(),
                                    };
                                    ui.label(
                                        egui::RichText::new(tier.label()).small().color(color),
                                    );
                                },
                            );
                        });
                    }
                });
            ui.separator();

            if let Some(section) = doc.selected {
                place_card(ui, doc, section, &mut picked);
                ui.separator();
            }

            egui::ScrollArea::vertical()
                .id_salt("document_editor")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let output = egui::TextEdit::multiline(&mut doc.text)
                        .id(egui::Id::new("document_text"))
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(24)
                        .lock_focus(true)
                        .show(ui);
                    if output.response.changed() {
                        doc.edited_at = Some(Instant::now());
                    }
                    // An outline click brings its heading into view. The
                    // editor does not take focus: keys would type into the
                    // document instead of moving through the world.
                    if let Some(line) = doc.scroll_to_line.take() {
                        let cursor = egui::text::CCursor::new(char_of_line(&doc.text, line));
                        let offset = output.galley_pos.to_vec2()
                            - egui::vec2(output.galley.rect.left(), 0.0);
                        let rect = output.galley.pos_from_cursor(cursor).translate(offset);
                        ui.scroll_to_rect(rect, Some(egui::Align::TOP));
                    }
                    // Text → world: the cursor moving into another section
                    // selects it, and flies there unless following is off.
                    if output.response.has_focus()
                        && let Some(range) = output.cursor_range
                        && doc.caret != Some(range.primary.index)
                    {
                        doc.caret = Some(range.primary.index);
                        let line = line_of_char(&doc.text, range.primary.index);
                        let section = section_at_line(&doc.outline, line);
                        if section != doc.caret_section {
                            doc.caret_section = section;
                            if let Some(section) = section
                                && doc.follow_cursor
                                && doc.selected != Some(section)
                            {
                                go_to = Some((section, None));
                            }
                        }
                    }
                });
        });

    if let Some((section, line)) = go_to {
        doc.go_to(section, &mut flight);
        doc.scroll_to_line = line;
    }
    if let Some(section) = again {
        doc.reauthor(section);
    }
    // World ← card: select the thing in Gen's inspector, which shows what it
    // is in the world format, and fly to it.
    if let Some(name) = picked
        && let Some(entity) = registry.get_entity(&name)
    {
        if let Some(mut selection) = selection {
            selection.entity = Some(entity);
        }
        if let Some(mut inspector) = inspector
            && inspector.mode == InspectorMode::Hidden
        {
            inspector.mode = InspectorMode::Full;
        }
        if let Ok(global) = transforms.get(entity) {
            let target = global.translation();
            // From the side the section's tour stop looks from, closer in.
            let away = section_of(&name)
                .and_then(|section| doc.stops.get(section))
                .and_then(|stop| (stop.position - stop.look_at).try_normalize())
                .unwrap_or(Vec3::new(0.0, 0.45, 1.0).normalize());
            *flight = Flight {
                target: Some(Stop {
                    position: target + away * THING_VIEW_DISTANCE,
                    look_at: target,
                }),
                ..Default::default()
            };
        }
    }
}

/// The selected section's place: where it came from, the model's line about
/// it, and what it is made of. Each thing selects its entity in the world.
fn place_card(ui: &mut egui::Ui, doc: &Document, section: usize, picked: &mut Option<String>) {
    let weak = ui.visuals().weak_text_color();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(doc.heading(section).unwrap_or("Section")).strong());
        if let Some(tier) = doc.tiers.get(section) {
            ui.label(egui::RichText::new(tier.label()).small().color(weak));
        }
    });
    if let Some(Some(description)) = doc.descriptions.get(section) {
        ui.label(egui::RichText::new(description).small().italics());
    }
    let Some(place) = doc.places.get(section).filter(|place| !place.is_empty()) else {
        return;
    };
    let things = things(place.iter().map(String::as_str));
    ui.label(
        egui::RichText::new(format!(
            "{} entit{} — click one to select it (F1 hides the inspector)",
            place.len(),
            if place.len() == 1 { "y" } else { "ies" }
        ))
        .small()
        .color(weak),
    );
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        for thing in things.iter().take(MAX_THINGS) {
            let text = match thing.count {
                1 => thing.label.clone(),
                n => format!("{} ×{n}", thing.label),
            };
            if ui.small_button(text).on_hover_text(&thing.first).clicked() {
                *picked = Some(thing.first.clone());
            }
        }
        if things.len() > MAX_THINGS {
            ui.label(
                egui::RichText::new(format!("+{} more", things.len() - MAX_THINGS))
                    .small()
                    .color(weak),
            );
        }
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
    fn the_cursor_line_finds_its_section() {
        // line 0 "# Trip", 2 "## Sea", 6 "### Tides", 10 "## Peak"
        let text = "# Trip\n\n## Sea\n\nsalt\n\n### Tides\n\nin, out\n\n## Peak\n\nair\n";
        let items = outline(text, &parse(text));
        let lines: Vec<Option<usize>> = items.iter().map(|i| i.line).collect();
        assert_eq!(lines, [None, Some(2), Some(6), Some(10)]);

        assert_eq!(section_at_line(&items, 0), None, "the title is no place");
        assert_eq!(section_at_line(&items, 2), Some(0));
        assert_eq!(section_at_line(&items, 8), Some(0), "under a sub-heading");
        assert_eq!(section_at_line(&items, 10), Some(1));
        assert_eq!(section_at_line(&items, 99), Some(1));
    }

    #[test]
    fn cursor_offsets_count_characters_not_bytes() {
        let text = "# 你好\n\n## 海\n\n盐\n";
        // egui's cursor after "盐" is character 13; its byte offset is 21.
        assert_eq!(line_of_char(text, 13), 4);
        assert_eq!(line_of_char(text, 11), 3, "the blank line before it");
        assert_eq!(line_of_char(text, 0), 0);
        assert_eq!(
            char_of_line(text, 2),
            6,
            "“## 海” starts after 6 characters"
        );
        assert_eq!(char_of_line(text, 0), 0);
        assert_eq!(char_of_line(text, 99), text.chars().count());
        for line in 0..5 {
            assert_eq!(line_of_char(text, char_of_line(text, line)), line);
        }
    }

    #[test]
    fn a_place_is_listed_by_what_its_things_are() {
        let names = [
            "s01-fishing_boat_1",
            "s01-lighthouse_tower",
            "s01-fishing_boat_2",
            "s01-prop-3",
            "s01-prop-4",
            "s01-landmark",
            "s01-7",
        ];
        let listed: Vec<(String, &str, usize)> = things(names)
            .into_iter()
            .map(|t| {
                let first = names.iter().find(|n| **n == t.first).unwrap();
                (t.label, *first, t.count)
            })
            .collect();
        assert_eq!(
            listed,
            [
                ("fishing boat".to_string(), "s01-fishing_boat_1", 2),
                ("lighthouse tower".to_string(), "s01-lighthouse_tower", 1),
                ("prop".to_string(), "s01-prop-3", 2),
                ("landmark".to_string(), "s01-landmark", 1),
                ("7".to_string(), "s01-7", 1),
            ]
        );
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
