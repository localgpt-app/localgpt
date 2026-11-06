//! The panels as tiles (`egui_tiles`, `docs/rfcs/authoring/all-rust.md`).
//! They begin where the page had them — the left column's tabs (the
//! document, a live canvas's history, the world's things), the world and
//! the terminals side by side, the timeline across the foot — and every
//! one has a tab, so a hand drags any of them anywhere: beside another,
//! over it as a tab, or split off on its own, and drags the gaps to resize.
//!
//! A tab's × hides its panel, and the top bar's Panels menu shows it again
//! or puts every panel back where it began. The button at the right of a
//! tab bar, or a double click on a tab, folds the panels under that bar: in
//! a column to the bar alone, in a row to a strip of their names, a click
//! on which unfolds them. Dragging a folded panel's edge opens it too.
//!
//! A pane shows while the app has sent what it shows and no hand has hidden
//! it, and a container while anything in it shows, so a mode's panes go
//! with the mode wherever they were dragged.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Stroke, Vec2};
use egui_tiles::{
    Container, LinearDir, SimplificationOptions, TabState, Tabs, Tile, TileId, Tiles, Tree,
    UiResponse,
};
use serde::{Deserialize, Serialize};

use super::document::Document;
use super::history::History;
use super::scene::Scene;
use super::state::State;
use super::terminals::Terminals;
use super::theme;
use super::timeline::{self, Timeline};
use super::world;
use crate::webui::link::ShellEnd;

/// What a tile shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pane {
    Document,
    History,
    Outliner,
    World,
    Terminals,
    Timeline,
}

impl Pane {
    /// Every pane, in the order the panels' menu lists them.
    const ALL: [Pane; 6] = [
        Pane::Document,
        Pane::History,
        Pane::Outliner,
        Pane::World,
        Pane::Terminals,
        Pane::Timeline,
    ];

    fn title(self) -> &'static str {
        match self {
            Pane::Document => "Document",
            Pane::History => "History",
            Pane::Outliner => "Outliner",
            Pane::World => "World",
            Pane::Terminals => "Terminals",
            Pane::Timeline => "Timeline",
        }
    }

    /// Whether the app has sent what it shows.
    fn there(self, state: &State) -> bool {
        match self {
            Pane::Document => state.document.is_some(),
            Pane::History => state.history.is_some(),
            Pane::Outliner => state.scene.is_some(),
            Pane::World | Pane::Terminals => true,
            Pane::Timeline => state.timeline.is_some(),
        }
    }

    /// When it is there, for the menu while it is not.
    fn when(self) -> &'static str {
        match self {
            Pane::Document => "While a document is open",
            Pane::History => "While a live world is open",
            Pane::Outliner | Pane::Timeline => "While a world is open",
            Pane::World | Pane::Terminals => "Always",
        }
    }
}

/// The page's widths: the left column, a document's wider, and the
/// terminals'.
const LEFT: f32 = 340.0;
const LEFT_DOCUMENT: f32 = 420.0;
const TERMINALS: f32 = 480.0;
/// A tab bar's height, which a folded column keeps, and a folded row's
/// strip's width.
const BAR: f32 = 24.0;
const STRIP: f32 = 24.0;
/// Between two tiles.
const GAP: f32 = 1.0;
/// What a row or a column keeps, at least, for the panels open in it when
/// others are given a size.
const ROOM: f32 = 120.0;

pub struct Layout {
    tree: Tree<Pane>,
    /// The containers the panels began in, for a mode's widths: the left
    /// column's tabs, the world's and the terminals'.
    left: TileId,
    world: TileId,
    terminals: TileId,
    /// Every pane's tile, in [`Pane::ALL`]'s order.
    panes: [TileId; 6],
    /// Which of them a hand has hidden, in the same order.
    hidden: [bool; 6],
    /// Which of the left column's panes showed last frame.
    shown: [bool; 3],
    /// What the row was last sized for: a document or not, and whether the
    /// left column had anything.
    sized_for: Option<(bool, bool)>,
    /// The height last given the timeline for what it holds: given again
    /// only when that changes, so a hand's resize stays.
    timeline_height: Option<f32>,
    folds: Vec<Fold>,
    /// `LOCALGPT_APP_UI_OPEN=outliner`: the Outliner first, once it is there.
    outliner_first: bool,
    /// Where the layout is kept between launches (`<state>/app-layout.json`);
    /// none in tests.
    path: Option<PathBuf>,
    /// When a hand last changed the panels, until that is kept.
    changed: Option<Instant>,
    /// As a hand left it at the last launch: the first mode that shows the
    /// left column keeps its widths and its tab in front, rather than the
    /// page's.
    restored: bool,
}

/// The panels as a hand left them, kept between launches.
#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    tree: Tree<Pane>,
    hidden: [bool; 6],
    /// Each folded container, and its size open.
    folds: Vec<(TileId, f32)>,
    timeline_height: Option<f32>,
}

/// The kept layout's version: a file of another is set aside.
const SAVED: u32 = 1;
/// How long after a hand's change the layout is kept: a drag of a gap
/// changes it every frame.
const KEEP_AFTER: Duration = Duration::from_secs(1);

/// A container folded to its bar, or in a row to a strip.
struct Fold {
    container: TileId,
    /// Its size along its row or column when it folded, to unfold to.
    open: f32,
    /// The share last given it: a hand that drags its edge changes that,
    /// and so unfolds it.
    share: Option<f32>,
}

/// What a hand asked in the tiles, done once they are drawn.
enum Ask {
    Hide(TileId),
    /// A container, and the pane to bring to the front in it.
    Unfold(TileId, Option<TileId>),
    Toggle(TileId),
}

impl Layout {
    pub fn new(outliner_first: bool) -> Self {
        let mut tiles = Tiles::default();
        let panes = Pane::ALL.map(|pane| tiles.insert_pane(pane));
        let [
            document,
            history,
            outliner,
            world_pane,
            terminals_pane,
            timeline_pane,
        ] = panes;
        // Every pane in tabs, so whatever is dragged keeps a tab to be
        // dragged by (`all_panes_must_have_tabs`).
        let left = tiles.insert_tab_tile(vec![document, history, outliner]);
        let world = tiles.insert_tab_tile(vec![world_pane]);
        let terminals = tiles.insert_tab_tile(vec![terminals_pane]);
        let timeline = tiles.insert_tab_tile(vec![timeline_pane]);
        let row = tiles.insert_horizontal_tile(vec![left, world, terminals]);
        let root = tiles.insert_vertical_tile(vec![row, timeline]);
        // Nothing on the left and no timeline until the app says what is
        // open.
        for id in [document, history, outliner, left, timeline_pane, timeline] {
            tiles.set_visible(id, false);
        }
        Self {
            tree: Tree::new(egui::Id::unique("the shell's panels"), root, tiles),
            left,
            world,
            terminals,
            panes,
            hidden: [false; 6],
            shown: [false; 3],
            sized_for: None,
            timeline_height: None,
            folds: Vec::new(),
            outliner_first,
            path: None,
            changed: None,
            restored: false,
        }
    }

    /// The layout a hand left at the last launch, when it is whole, else the
    /// page's; kept at `path` from here on.
    pub fn open(outliner_first: bool, path: Option<PathBuf>) -> Self {
        let saved = path.as_deref().and_then(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            serde_json::from_str::<Saved>(&text)
                .inspect_err(|e| {
                    tracing::warn!("{}: {e}; the panels start where they began", path.display())
                })
                .ok()
        });
        let mut layout = saved
            .and_then(|saved| Self::restore(saved, outliner_first))
            .unwrap_or_else(|| Self::new(outliner_first));
        layout.path = path;
        layout
    }

    /// A kept layout, when it is this version's and holds every pane once.
    fn restore(saved: Saved, outliner_first: bool) -> Option<Self> {
        if saved.version != SAVED {
            return None;
        }
        let tiles = saved.tree.tiles;
        let mut found = [None; 6];
        for (id, tile) in tiles.iter() {
            if let Tile::Pane(pane) = tile {
                let i = Pane::ALL.iter().position(|p| p == pane)?;
                if found[i].replace(*id).is_some() {
                    return None;
                }
            }
        }
        let mut panes = [TileId::from_u64(0); 6];
        for (pane, found) in panes.iter_mut().zip(found) {
            *pane = found?;
        }
        let root = saved.tree.root.filter(|root| tiles.get(*root).is_some())?;
        let home = |pane: TileId| tiles.parent_of(pane).unwrap_or(pane);
        let (left, world, terminals) = (home(panes[0]), home(panes[3]), home(panes[4]));
        let folds = saved
            .folds
            .into_iter()
            .filter(|(container, _)| {
                matches!(
                    tiles.get(*container),
                    Some(Tile::Container(Container::Tabs(_)))
                )
            })
            .map(|(container, open)| Fold {
                container,
                open,
                share: None,
            })
            .collect();
        Some(Self {
            tree: Tree::new(egui::Id::unique("the shell's panels"), root, tiles),
            left,
            world,
            terminals,
            panes,
            hidden: saved.hidden,
            shown: [false; 3],
            sized_for: None,
            timeline_height: saved.timeline_height,
            folds,
            outliner_first,
            path: None,
            changed: None,
            restored: true,
        })
    }

    /// The layout as a hand left it, for the next launch.
    fn keep(&mut self) {
        self.changed = None;
        let Some(path) = &self.path else {
            return;
        };
        let saved = Saved {
            version: SAVED,
            tree: self.tree.clone(),
            hidden: self.hidden,
            folds: self
                .folds
                .iter()
                .map(|fold| (fold.container, fold.open))
                .collect(),
            timeline_height: self.timeline_height,
        };
        let kept = serde_json::to_string(&saved)
            .map_err(anyhow::Error::from)
            .and_then(|json| {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                // Whole or not at all: a launch never reads half a file.
                let part = path.with_extension("json.part");
                std::fs::write(&part, json)?;
                std::fs::rename(&part, path)?;
                Ok(())
            });
        if let Err(e) = kept {
            tracing::warn!("{}: {e:#}", path.display());
        }
    }

    /// The window closes: what a hand changed since it was last kept is kept.
    pub fn on_exit(&mut self) {
        if self.changed.is_some() {
            self.keep();
        }
    }

    /// What is open and what a hand hid decide what shows: the document's
    /// pane while there is a document, the history's for a live canvas,
    /// the outliner whenever the world has things, the timeline while
    /// there is one. When the left column's panes change, the first of
    /// them is the tab in front; when the mode changes, the row takes the
    /// page's widths for it; the timeline is as tall as what it holds; and
    /// what is folded keeps to its bar. `area` is the panels' room.
    pub fn follow(&mut self, state: &State, area: Vec2) {
        let mut visible = [false; 6];
        for (i, pane) in Pane::ALL.into_iter().enumerate() {
            visible[i] = pane.there(state) && !self.hidden[i];
            self.tree.tiles.set_visible(self.panes[i], visible[i]);
        }
        if let Some(root) = self.tree.root {
            shows(&mut self.tree.tiles, root);
        }
        let left = [visible[0], visible[1], visible[2]];
        if left != self.shown {
            // A kept layout's left column comes back with the tab a hand
            // left in front.
            let keep_front = self.restored && !self.shown.iter().any(|shown| *shown);
            self.shown = left;
            let first = if keep_front {
                None
            } else if self.outliner_first && left[2] {
                Some(self.panes[2])
            } else {
                self.panes
                    .iter()
                    .zip(left)
                    .find_map(|(id, shows)| shows.then_some(*id))
            };
            if let Some(first) = first {
                self.tree.make_active(|id, _| id == first);
            }
        }
        // By what is there, not by what shows: a hand hiding a panel does
        // not resize the others.
        let mode = (
            state.mode_document,
            Pane::ALL[..3].iter().any(|pane| pane.there(state)),
        );
        if self.sized_for != Some(mode) {
            if self.restored {
                // The widths a hand left, until the mode changes after the
                // first that shows the left column.
                if mode.1 {
                    self.sized_for = Some(mode);
                    self.restored = false;
                }
            } else {
                let width = if state.mode_document {
                    LEFT_DOCUMENT
                } else {
                    LEFT
                };
                if self.size(width.min(area.x * 0.3).max(240.0), area.x) {
                    self.sized_for = Some(mode);
                }
            }
        }
        self.fit_timeline(state, area.y);
        self.keep_folds();
        if self
            .changed
            .is_some_and(|changed| changed.elapsed() >= KEEP_AFTER)
        {
            self.keep();
        }
    }

    /// The left column at `left` points and the terminals at the page's
    /// width, the world the rest, where they still stand in its row. False
    /// when the row has not been drawn yet.
    fn size(&mut self, left: f32, width: f32) -> bool {
        let terminals = TERMINALS.min(width * 0.4).max(280.0);
        let Some(row) = self.tree.tiles.parent_of(self.world) else {
            // The world has left the row its tabs began in: nothing to size.
            return true;
        };
        let sets: Vec<(TileId, f32)> = [(self.left, left), (self.terminals, terminals)]
            .into_iter()
            .filter(|(id, _)| {
                self.tree.tiles.parent_of(*id) == Some(row) && self.tree.tiles.is_visible(*id)
            })
            .collect();
        self.arrange(row, &sets)
    }

    /// The timeline as tall as what it holds (the page's rule), when that
    /// changes, while it stands alone in a column.
    fn fit_timeline(&mut self, state: &State, tall: f32) {
        let pane = self.panes[5];
        if !self.tree.tiles.is_visible(pane) {
            return;
        }
        let wanted = timeline::wanted_height(state.timeline.as_deref(), tall) + BAR;
        if self
            .timeline_height
            .is_some_and(|height| (height - wanted).abs() < 0.5)
        {
            return;
        }
        let Some(container) = self.tree.tiles.parent_of(pane) else {
            return;
        };
        let alone = matches!(
            self.tree.tiles.get(container),
            Some(Tile::Container(Container::Tabs(tabs))) if tabs.children == [pane]
        );
        let Some((column, LinearDir::Vertical)) = linear_parent(&self.tree.tiles, container) else {
            return;
        };
        if alone && !self.folded(container) && self.arrange(column, &[(container, wanted)]) {
            self.timeline_height = Some(wanted);
        }
    }

    /// Each folded container at its bar's height in a column, a strip's
    /// width in a row. One that is gone, no longer in a row or a column, or
    /// whose edge a hand dragged, unfolds; so does the first of a row's or
    /// a column's when nothing open is left beside them.
    fn keep_folds(&mut self) {
        let tiles = &self.tree.tiles;
        self.folds.retain(|fold| {
            let Some((parent, _)) = linear_parent(tiles, fold.container) else {
                return false;
            };
            let Some(Tile::Container(Container::Linear(linear))) = tiles.get(parent) else {
                return false;
            };
            fold.share
                .is_none_or(|share| same(linear.shares[fold.container], share))
        });
        let mut parents: Vec<TileId> = Vec::new();
        for fold in &self.folds {
            if let Some(parent) = self.tree.tiles.parent_of(fold.container)
                && !parents.contains(&parent)
            {
                parents.push(parent);
            }
        }
        for parent in parents {
            let shown = shown_children(&self.tree.tiles, parent);
            if !shown.is_empty() && shown.iter().all(|child| self.folded(*child)) {
                let first = shown[0];
                self.folds.retain(|fold| fold.container != first);
            }
            self.arrange(parent, &[]);
        }
    }

    /// Sizes the children of a row or a column by their shares: each
    /// folded one to its bar or strip, each of `sets` to the points it
    /// asks (all of them less, together, where there is not room), and the
    /// others share the rest as they did. Returns false when the row or
    /// column has not been drawn yet, or nothing in it is left to share.
    fn arrange(&mut self, parent: TileId, sets: &[(TileId, f32)]) -> bool {
        let Some(rect) = self.tree.tiles.rect(parent) else {
            return false;
        };
        let Some(Tile::Container(Container::Linear(linear))) = self.tree.tiles.get(parent) else {
            return false;
        };
        let dir = linear.dir;
        let shown = shown_children(&self.tree.tiles, parent);
        let (length, thin) = match dir {
            LinearDir::Horizontal => (rect.width(), STRIP),
            LinearDir::Vertical => (rect.height(), BAR),
        };
        let available = length - GAP * shown.len().saturating_sub(1) as f32;
        let folded: Vec<TileId> = shown
            .iter()
            .copied()
            .filter(|child| self.folded(*child))
            .collect();
        let asked: Vec<(TileId, f32)> = sets
            .iter()
            .copied()
            .filter(|(id, _)| shown.contains(id) && !folded.contains(id))
            .collect();
        let open: f32 = shown
            .iter()
            .filter(|child| !folded.contains(child) && !asked.iter().any(|(id, _)| id == *child))
            .map(|child| linear.shares[*child])
            .sum();
        if open <= 0.0 {
            return false;
        }
        let thin_total = thin * folded.len() as f32;
        let asked_total: f32 = asked.iter().map(|(_, size)| size).sum();
        let room = available - thin_total - ROOM;
        if room <= 0.0 {
            return false;
        }
        let scale = if asked_total > room {
            room / asked_total
        } else {
            1.0
        };
        let rest = available - thin_total - asked_total * scale;
        // Shares per point: the open children keep theirs and take the rest.
        let per_point = open / rest;
        let thin_share = thin * per_point;
        if let Some(Tile::Container(Container::Linear(linear))) = self.tree.tiles.get_mut(parent) {
            for child in &folded {
                linear.shares.set_share(*child, thin_share);
            }
            for (id, size) in &asked {
                linear.shares.set_share(*id, size * scale * per_point);
            }
        }
        for fold in &mut self.folds {
            if folded.contains(&fold.container) {
                fold.share = Some(thin_share);
            }
        }
        true
    }

    fn folded(&self, container: TileId) -> bool {
        self.folds.iter().any(|fold| fold.container == container)
    }

    fn fold(&mut self, container: TileId) {
        if self.folded(container) {
            return;
        }
        let Some((_, dir)) = linear_parent(&self.tree.tiles, container) else {
            return;
        };
        let Some(rect) = self.tree.tiles.rect(container) else {
            return;
        };
        let open = match dir {
            LinearDir::Horizontal => rect.width(),
            LinearDir::Vertical => rect.height(),
        };
        // Its share is given when the folds are kept, before the next draw.
        self.folds.push(Fold {
            container,
            open,
            share: None,
        });
    }

    /// Back to the size it folded from, or where there is no room for
    /// that, as wide as the others around it.
    fn unfold(&mut self, container: TileId, front: Option<TileId>) {
        if let Some(i) = self
            .folds
            .iter()
            .position(|fold| fold.container == container)
        {
            let fold = self.folds.remove(i);
            if let Some((parent, _)) = linear_parent(&self.tree.tiles, container)
                && !self.arrange(parent, &[(container, fold.open)])
                && let Some(Tile::Container(Container::Linear(linear))) =
                    self.tree.tiles.get_mut(parent)
            {
                let others: Vec<f32> = linear
                    .children
                    .iter()
                    .filter(|child| **child != container)
                    .map(|child| linear.shares[*child])
                    .collect();
                if !others.is_empty() {
                    let mean = others.iter().sum::<f32>() / others.len() as f32;
                    linear.shares.set_share(container, mean);
                }
            }
        }
        if let Some(front) = front {
            self.tree.make_active(|id, _| id == front);
        }
    }

    fn answer(&mut self, ask: Ask) {
        self.changed = Some(Instant::now());
        match ask {
            Ask::Hide(tile) => {
                if let Some(i) = self.panes.iter().position(|pane| *pane == tile) {
                    self.hidden[i] = true;
                }
            }
            Ask::Unfold(container, front) => self.unfold(container, front),
            Ask::Toggle(container) => {
                if self.folded(container) {
                    self.unfold(container, None);
                } else {
                    self.fold(container);
                }
            }
        }
    }

    /// A panel shown again from the menu comes unfolded and in front.
    fn show(&mut self, i: usize, on: bool) {
        self.changed = Some(Instant::now());
        self.hidden[i] = !on;
        if on {
            let pane = self.panes[i];
            if let Some(container) = self.tree.tiles.parent_of(pane)
                && self.folded(container)
            {
                self.unfold(container, None);
            }
            self.tree.make_active(|id, _| id == pane);
        }
    }

    /// Every panel where it began, shown and unfolded, and kept so.
    fn reset(&mut self) {
        let path = self.path.take();
        *self = Self::new(self.outliner_first);
        self.path = path;
        self.changed = Some(Instant::now());
    }

    /// The panels' menu: each panel shown or hidden, and all of them put
    /// back where they began.
    pub fn menu(&mut self, ui: &mut egui::Ui, state: &State) {
        use egui::containers::menu::{MenuButton, MenuConfig};
        let button = egui::Button::new(egui::RichText::new("Panels").color(theme::MUTED))
            .frame_when_inactive(false);
        let (response, _) = MenuButton::from_button(button)
            .config(MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside))
            .ui(ui, |ui| {
                for (i, pane) in Pane::ALL.into_iter().enumerate() {
                    let mut on = !self.hidden[i];
                    let response = ui
                        .add_enabled(
                            pane.there(state),
                            egui::Checkbox::new(&mut on, pane.title()),
                        )
                        .on_disabled_hover_text(pane.when());
                    if response.changed() {
                        self.show(i, on);
                    }
                }
                ui.separator();
                if ui
                    .button("Put them back")
                    .on_hover_text("Every panel where it began, shown and unfolded")
                    .clicked()
                {
                    self.reset();
                    ui.close();
                }
            });
        response.on_hover_text("Show, hide or put back the panels");
    }

    /// Draws the tiles; false when the world was not among what they drew
    /// (folded, hidden, or behind another tab).
    pub fn ui(&mut self, ui: &mut egui::Ui, panels: Panels<'_>) -> bool {
        let tiles = &self.tree.tiles;
        let folded = self
            .folds
            .iter()
            .filter_map(|fold| {
                let (_, dir) = linear_parent(tiles, fold.container)?;
                let Some(Tile::Container(Container::Tabs(tabs))) = tiles.get(fold.container) else {
                    return None;
                };
                let panes = tabs
                    .children
                    .iter()
                    .filter(|child| tiles.is_visible(**child))
                    .filter_map(|child| match tiles.get(*child) {
                        Some(Tile::Pane(pane)) => Some((*child, *pane)),
                        _ => None,
                    })
                    .collect();
                Some(Folded {
                    container: fold.container,
                    across: dir == LinearDir::Horizontal,
                    panes,
                })
            })
            .collect();
        let mut foldables = Vec::new();
        let mut homes = Vec::new();
        for (id, tile) in tiles.iter() {
            if let Tile::Container(Container::Tabs(tabs)) = tile {
                if foldable(tiles, *id, tabs) {
                    foldables.push(*id);
                }
                homes.extend(tabs.children.iter().map(|child| (*child, *id)));
            }
        }
        let mut panes = Panes {
            panels,
            folded,
            foldable: foldables,
            homes,
            asks: Vec::new(),
            edited: false,
            world_drawn: false,
        };
        self.tree.ui(&mut panes, ui);
        if panes.edited {
            self.changed = Some(Instant::now());
        }
        for ask in std::mem::take(&mut panes.asks) {
            self.answer(ask);
        }
        panes.world_drawn
    }
}

/// The row or column a container stands in, and which it is.
fn linear_parent(tiles: &Tiles<Pane>, id: TileId) -> Option<(TileId, LinearDir)> {
    let parent = tiles.parent_of(id)?;
    match tiles.get(parent)? {
        Tile::Container(Container::Linear(linear)) => Some((parent, linear.dir)),
        _ => None,
    }
}

/// The children of a row or a column that show.
fn shown_children(tiles: &Tiles<Pane>, parent: TileId) -> Vec<TileId> {
    match tiles.get(parent) {
        Some(Tile::Container(container)) => container
            .children()
            .copied()
            .filter(|child| tiles.is_visible(*child))
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether a tab bar's panes may fold: they stand in a row or a column
/// beside something else that shows, and are all panes.
fn foldable(tiles: &Tiles<Pane>, container: TileId, tabs: &Tabs) -> bool {
    let Some((parent, _)) = linear_parent(tiles, container) else {
        return false;
    };
    shown_children(tiles, parent).len() > 1
        && tabs
            .children
            .iter()
            .all(|child| matches!(tiles.get(*child), Some(Tile::Pane(_))))
}

/// Two shares the same but for rounding.
fn same(a: f32, b: f32) -> bool {
    (a - b).abs() <= b.abs() * 1e-3
}

/// Whether anything under `id` shows: a pane by its own flag, a container
/// by its children's, which it then takes. Every child is visited.
fn shows(tiles: &mut Tiles<Pane>, id: TileId) -> bool {
    let children = match tiles.get(id) {
        Some(Tile::Pane(_)) => return tiles.is_visible(id),
        Some(Tile::Container(container)) => container.children_vec(),
        None => return false,
    };
    let mut any = false;
    for child in children {
        any |= shows(tiles, child);
    }
    tiles.set_visible(id, any);
    any
}

/// The fold button's mark: a bar to fold, a box to unfold.
fn paint_fold(painter: &egui::Painter, rect: egui::Rect, folded: bool, hovered: bool) {
    let stroke = Stroke::new(1.5, if hovered { theme::INK } else { theme::MUTED });
    let center = rect.center();
    if folded {
        painter.rect_stroke(
            egui::Rect::from_center_size(center, egui::vec2(8.0, 8.0)),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
    } else {
        painter.hline((center.x - 4.0)..=(center.x + 4.0), center.y + 3.0, stroke);
    }
}

/// The panels a tile draws, lent for a frame.
pub struct Panels<'a> {
    pub state: &'a mut State,
    pub end: &'a ShellEnd,
    pub frame: &'a eframe::Frame,
    pub world: &'a mut world::Panel,
    pub world_rect: &'a mut egui::Rect,
    pub terminals: &'a mut Terminals,
    pub scene: &'a mut Scene,
    pub document: &'a mut Document,
    pub history: &'a mut History,
    pub timeline: &'a mut Timeline,
}

/// A folded container, as the tiles draw it.
struct Folded {
    container: TileId,
    /// In a row: a strip of its panes' names, with no tab bar.
    across: bool,
    /// Its panes that show, and what they are.
    panes: Vec<(TileId, Pane)>,
}

/// The tiles' behaviour for a frame: the panels, what is folded, and what a
/// hand asks.
struct Panes<'a> {
    panels: Panels<'a>,
    folded: Vec<Folded>,
    /// The tab bars whose panes may fold, and the bar each tab is in:
    /// worked out before the tiles are drawn, since a container is out of
    /// the tree while it draws (its children cannot find it).
    foldable: Vec<TileId>,
    homes: Vec<(TileId, TileId)>,
    asks: Vec<Ask>,
    /// A hand resized a tile or dropped one somewhere new.
    edited: bool,
    world_drawn: bool,
}

impl Panes<'_> {
    /// A folded row's column: the button that unfolds it, and its panes'
    /// names down the strip, each of which unfolds it to that pane.
    fn strip(&mut self, ui: &mut egui::Ui, index: usize) {
        let rect = ui.max_rect();
        let painter = ui.painter().clone();
        painter.rect_filled(rect, 0.0, theme::PANEL);
        let folded = &self.folded[index];
        let top = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), BAR));
        let unfold = ui
            .interact(top, ui.scope_id().with("unfold"), egui::Sense::click())
            .on_hover_text("Unfold");
        paint_fold(&painter, top, true, unfold.hovered());
        let mut asks = Vec::new();
        if unfold.clicked() {
            asks.push(Ask::Unfold(folded.container, None));
        }
        let font = egui::TextStyle::Button.resolve(ui.style());
        let mut y = top.bottom() + 4.0;
        for (tile, pane) in &folded.panes {
            let galley =
                painter.layout_no_wrap(pane.title().to_owned(), font.clone(), theme::MUTED);
            let size = galley.size();
            let area = egui::Rect::from_min_size(
                egui::pos2(rect.min.x, y),
                egui::vec2(rect.width(), size.x + 12.0),
            );
            let response = ui
                .interact(
                    area,
                    ui.scope_id().with(("strip", *tile)),
                    egui::Sense::click(),
                )
                .on_hover_text(format!("Unfold to {}", pane.title()));
            let color = if response.hovered() {
                theme::INK
            } else {
                theme::MUTED
            };
            // A quarter turn clockwise about its top left: it reads down
            // the strip, centred across it.
            let at = egui::pos2(area.center().x + size.y / 2.0, area.min.y + 6.0);
            painter.add(
                egui::epaint::TextShape::new(at, galley, color)
                    .with_override_text_color(color)
                    .with_angle(std::f32::consts::FRAC_PI_2),
            );
            if response.clicked() {
                asks.push(Ask::Unfold(folded.container, Some(*tile)));
            }
            y = area.bottom() + 2.0;
        }
        self.asks.extend(asks);
    }
}

impl egui_tiles::Behavior<Pane> for Panes<'_> {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        pane.title().into()
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, tile: TileId, pane: &mut Pane) -> UiResponse {
        if let Some(index) = self
            .folded
            .iter()
            .position(|folded| folded.panes.iter().any(|(id, _)| *id == tile))
        {
            // Folded in a column, only its bar shows.
            if self.folded[index].across {
                self.strip(ui, index);
            }
            return UiResponse::None;
        }
        let p = &mut self.panels;
        if *pane == Pane::World {
            *p.world_rect = ui.available_rect_before_wrap();
            p.world.ui(ui, p.frame);
            self.world_drawn = true;
            return UiResponse::None;
        }
        let margin = if *pane == Pane::Timeline {
            egui::Margin::symmetric(8, 2)
        } else {
            egui::Margin::same(8)
        };
        egui::Frame::new()
            .fill(theme::PANEL)
            .inner_margin(margin)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                match pane {
                    Pane::Document => {
                        // Kept until the document shows: the latest place
                        // picked is where its text opens.
                        let reveal = p.state.reveal.take();
                        p.document.ui(ui, p.state, reveal, p.end);
                    }
                    Pane::History => p.history.ui(ui, p.state, p.end),
                    Pane::Outliner => p.scene.ui(ui, p.state, p.end),
                    Pane::Terminals => p.terminals.ui(ui, p.end),
                    Pane::Timeline => p.timeline.ui(ui, p.state, p.end),
                    Pane::World => unreachable!("drawn above"),
                }
            });
        UiResponse::None
    }

    fn simplification_options(&self) -> SimplificationOptions {
        SimplificationOptions {
            // A pane dragged anywhere keeps a tab, to be dragged again.
            all_panes_must_have_tabs: true,
            ..SimplificationOptions::default()
        }
    }

    /// Every tab bar shows, but a folded row's: its strip stands for it.
    fn is_tab_bar_visible(&self, tile: TileId, _tabs: &Tabs) -> bool {
        !self
            .folded
            .iter()
            .any(|folded| folded.container == tile && folded.across)
    }

    fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
        BAR
    }

    /// The fold button, at the right of every bar whose panes may fold.
    fn top_bar_right_ui(
        &mut self,
        _tiles: &Tiles<Pane>,
        ui: &mut egui::Ui,
        tile: TileId,
        _tabs: &Tabs,
        _scroll_offset: &mut f32,
    ) {
        if !self.foldable.contains(&tile) {
            return;
        }
        let folded = self.folded.iter().any(|folded| folded.container == tile);
        let (rect, response) = ui.allocate_exact_size(egui::vec2(BAR, BAR), egui::Sense::click());
        let response = response.on_hover_text(if folded { "Unfold" } else { "Fold" });
        paint_fold(ui.painter(), rect, folded, response.hovered());
        if response.clicked() {
            self.asks.push(Ask::Toggle(tile));
        }
    }

    /// A double click on a tab folds its bar, or unfolds it.
    fn on_tab_button(
        &mut self,
        _tiles: &mut Tiles<Pane>,
        tile: TileId,
        response: egui::Response,
    ) -> egui::Response {
        if response.double_clicked()
            && let Some((_, container)) = self.homes.iter().find(|(pane, _)| *pane == tile)
            && self.foldable.contains(container)
        {
            self.asks.push(Ask::Toggle(*container));
        }
        response
    }

    fn is_tab_closable(&self, tiles: &Tiles<Pane>, tile: TileId) -> bool {
        matches!(tiles.get(tile), Some(Tile::Pane(_)))
    }

    /// A tab's × hides its panel; the tile stays, for the menu to show
    /// again where it was.
    fn on_tab_close(&mut self, _tiles: &mut Tiles<Pane>, tile: TileId) -> bool {
        self.asks.push(Ask::Hide(tile));
        false
    }

    /// A resize or a drop is a hand's; a tab selected may be egui's own
    /// pick, and is kept with the next change a hand makes.
    fn on_edit(&mut self, action: egui_tiles::EditAction) {
        if matches!(
            action,
            egui_tiles::EditAction::TileResized | egui_tiles::EditAction::TileDropped
        ) {
            self.edited = true;
        }
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        GAP
    }

    fn min_size(&self) -> f32 {
        160.0
    }

    fn resize_stroke(&self, _style: &egui::Style, state: egui_tiles::ResizeState) -> Stroke {
        match state {
            egui_tiles::ResizeState::Idle => Stroke::new(1.0, theme::LINE),
            egui_tiles::ResizeState::Hovering => Stroke::new(1.0, theme::MUTED),
            egui_tiles::ResizeState::Dragging => Stroke::new(1.0, theme::ACCENT),
        }
    }

    fn tab_bar_color(&self, _visuals: &egui::Visuals) -> Color32 {
        theme::PANEL
    }

    fn tab_bg_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile: TileId,
        state: &TabState,
    ) -> Color32 {
        if state.active {
            theme::alpha(theme::ACCENT, 0.25)
        } else {
            theme::PANEL
        }
    }

    fn tab_text_color(
        &self,
        _visuals: &egui::Visuals,
        _tiles: &Tiles<Pane>,
        _tile: TileId,
        state: &TabState,
    ) -> Color32 {
        if state.active {
            theme::INK
        } else {
            theme::MUTED
        }
    }

    fn tab_bar_hline_stroke(&self, _visuals: &egui::Visuals) -> Stroke {
        Stroke::new(1.0, theme::LINE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tiles with nothing in their panes, for their layout alone.
    struct Blank;

    impl egui_tiles::Behavior<Pane> for Blank {
        fn pane_ui(&mut self, _ui: &mut egui::Ui, _tile: TileId, _pane: &mut Pane) -> UiResponse {
            UiResponse::None
        }

        fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
            pane.title().into()
        }

        fn simplification_options(&self) -> SimplificationOptions {
            SimplificationOptions {
                all_panes_must_have_tabs: true,
                ..SimplificationOptions::default()
            }
        }

        fn gap_width(&self, _style: &egui::Style) -> f32 {
            GAP
        }

        fn tab_bar_height(&self, _style: &egui::Style) -> f32 {
            BAR
        }
    }

    const AREA: Vec2 = Vec2::new(1440.0, 800.0);

    /// A frame of the panels in a window `AREA` big, the layout following
    /// `state` first, as the shell does.
    fn frame(ctx: &egui::Context, layout: &mut Layout, state: &State) {
        layout.follow(state, AREA);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, AREA)),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| layout.tree.ui(&mut Blank, ui));
        // No painter takes the font atlas here.
        output.textures_delta.clear();
    }

    fn frames(ctx: &egui::Context, layout: &mut Layout, state: &State) {
        for _ in 0..3 {
            frame(ctx, layout, state);
        }
    }

    fn width(layout: &Layout, id: TileId) -> f32 {
        layout.tree.tiles.rect(id).map_or(0.0, |rect| rect.width())
    }

    fn height(layout: &Layout, id: TileId) -> f32 {
        layout.tree.tiles.rect(id).map_or(0.0, |rect| rect.height())
    }

    /// The share of `id` in the row or column it stands in, times `by`: a
    /// hand dragging its edge.
    fn drag(layout: &mut Layout, id: TileId, by: f32) {
        let parent = layout.tree.tiles.parent_of(id).expect("in a row or column");
        let Some(Tile::Container(Container::Linear(linear))) = layout.tree.tiles.get_mut(parent)
        else {
            panic!("not in a row or column");
        };
        let share = linear.shares[id];
        linear.shares.set_share(id, share * by);
    }

    #[test]
    fn a_column_folds_to_a_strip_and_unfolds_to_its_width() {
        let (ctx, mut layout, state) = (
            egui::Context::default(),
            Layout::new(false),
            State::default(),
        );
        frames(&ctx, &mut layout, &state);
        let terminals = layout.terminals;
        let open = width(&layout, terminals);
        assert!((open - TERMINALS).abs() < 0.5, "the page's width: {open}");

        layout.fold(terminals);
        frame(&ctx, &mut layout, &state);
        assert!((width(&layout, terminals) - STRIP).abs() < 0.5);
        // The world, open beside it, takes the rest.
        let world = width(&layout, layout.world);
        assert!((world - (AREA.x - STRIP - GAP)).abs() < 0.5, "{world}");
        // And it stays a strip.
        frame(&ctx, &mut layout, &state);
        assert!((width(&layout, terminals) - STRIP).abs() < 0.5);

        layout.unfold(terminals, None);
        frame(&ctx, &mut layout, &state);
        assert!((width(&layout, terminals) - open).abs() < 0.5);
    }

    #[test]
    fn dragging_a_folded_panels_edge_opens_it() {
        let (ctx, mut layout, state) = (
            egui::Context::default(),
            Layout::new(false),
            State::default(),
        );
        frames(&ctx, &mut layout, &state);
        let terminals = layout.terminals;
        layout.fold(terminals);
        frames(&ctx, &mut layout, &state);
        drag(&mut layout, terminals, 8.0);
        frame(&ctx, &mut layout, &state);
        assert!(!layout.folded(terminals));
        frame(&ctx, &mut layout, &state);
        assert!(width(&layout, terminals) > STRIP * 4.0);
    }

    #[test]
    fn something_in_a_row_stays_open() {
        let (ctx, mut layout, state) = (
            egui::Context::default(),
            Layout::new(false),
            State::default(),
        );
        frames(&ctx, &mut layout, &state);
        let (world, terminals) = (layout.world, layout.terminals);
        layout.fold(world);
        layout.fold(terminals);
        frames(&ctx, &mut layout, &state);
        assert!(!layout.folded(world), "the first of them unfolds");
        assert!(layout.folded(terminals));
        assert!((width(&layout, world) - (AREA.x - STRIP - GAP)).abs() < 0.5);
    }

    #[test]
    fn a_hidden_panel_comes_back_from_the_menu_and_all_go_back_at_once() {
        let (ctx, mut layout, state) = (
            egui::Context::default(),
            Layout::new(false),
            State::default(),
        );
        frames(&ctx, &mut layout, &state);
        let world_pane = layout.panes[3];
        layout.answer(Ask::Hide(world_pane));
        frame(&ctx, &mut layout, &state);
        assert!(!layout.tree.tiles.is_visible(world_pane));
        assert!(
            !layout.tree.tiles.is_visible(layout.world),
            "its tabs went too"
        );
        assert!((width(&layout, layout.terminals) - AREA.x).abs() < 0.5);

        layout.show(3, true);
        frame(&ctx, &mut layout, &state);
        assert!(layout.tree.tiles.is_visible(world_pane));

        // Folded beside nothing open, the terminals unfold; then everything
        // goes back where it began.
        layout.fold(layout.terminals);
        layout.answer(Ask::Hide(world_pane));
        frames(&ctx, &mut layout, &state);
        assert!(layout.folds.is_empty());
        layout.reset();
        frames(&ctx, &mut layout, &state);
        assert!(layout.tree.tiles.is_visible(layout.panes[3]));
        assert!((width(&layout, layout.terminals) - TERMINALS).abs() < 0.5);
    }

    #[test]
    fn the_timeline_is_as_tall_as_what_it_holds_until_a_hand_resizes_it() {
        let (ctx, mut layout) = (egui::Context::default(), Layout::new(false));
        let state = State {
            // No clock: one quiet line.
            timeline: Some(Box::default()),
            ..State::default()
        };
        frames(&ctx, &mut layout, &state);
        let timeline = layout.tree.tiles.parent_of(layout.panes[5]).unwrap();
        let fitted = height(&layout, timeline);
        assert!((fitted - (40.0 + BAR)).abs() < 0.5, "{fitted}");

        drag(&mut layout, timeline, 3.0);
        frames(&ctx, &mut layout, &state);
        let resized = height(&layout, timeline);
        assert!(resized > fitted * 2.0, "{resized}");

        layout.fold(timeline);
        frame(&ctx, &mut layout, &state);
        assert!((height(&layout, timeline) - BAR).abs() < 0.5);
        layout.unfold(timeline, None);
        frame(&ctx, &mut layout, &state);
        assert!((height(&layout, timeline) - resized).abs() < 0.5);
    }

    #[test]
    fn the_panels_come_back_as_a_hand_left_them() {
        let dir = std::env::temp_dir().join(format!("localgpt-app-layout-{}", std::process::id()));
        let path = dir.join("app-layout.json");
        let _ = std::fs::remove_dir_all(&dir);
        let state = State::default();

        let ctx = egui::Context::default();
        let mut layout = Layout::open(false, Some(path.clone()));
        frames(&ctx, &mut layout, &state);
        // A hand widens the world and hides the timeline, then folds the
        // terminals; nothing is kept until the hand has let things be.
        let world_tabs = layout.world;
        drag(&mut layout, world_tabs, 1.5);
        layout.answer(Ask::Hide(layout.panes[5]));
        frames(&ctx, &mut layout, &state);
        let world = width(&layout, layout.world);
        assert!(world > 1000.0, "{world}");
        layout.answer(Ask::Toggle(layout.terminals));
        frames(&ctx, &mut layout, &state);
        assert!(!path.exists());
        layout.on_exit();
        assert!(path.exists());

        let ctx = egui::Context::default();
        let mut again = Layout::open(false, Some(path.clone()));
        frames(&ctx, &mut again, &state);
        assert!(again.hidden[5], "the timeline stays hidden");
        assert!(again.folded(again.terminals));
        assert!((width(&again, again.terminals) - STRIP).abs() < 0.5);
        // The world kept what the hand gave it, not the page's width.
        let unfolded = {
            again.unfold(again.terminals, None);
            frame(&ctx, &mut again, &state);
            width(&again, again.world)
        };
        assert!((unfolded - world).abs() < 1.0, "{unfolded} against {world}");

        // A file that is not whole is set aside: the panels start where they
        // began.
        std::fs::write(&path, "{\"version\": 1}").unwrap();
        let fresh = Layout::open(false, Some(path.clone()));
        assert!(fresh.folds.is_empty() && fresh.hidden == [false; 6] && !fresh.restored);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
