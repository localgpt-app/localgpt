//! The panels as tiles (`egui_tiles`, `docs/rfcs/authoring/all-rust.md`):
//! the left column's tabs (the document, a live canvas's history, the
//! world's things), the world and the terminals side by side, as the page
//! lays them out; from there a hand drags a tab wherever it wants it,
//! beside the world, over the terminals, or split off on its own, and
//! drags the gaps to resize. The top bar and the timeline stay across the
//! window, as the page has them.
//!
//! A pane shows while the app has sent what it shows, and a container
//! while anything in it shows, so a mode's panes go with the mode wherever
//! they were dragged.

use eframe::egui::{self, Color32, Stroke};
use egui_tiles::{
    Container, SimplificationOptions, TabState, Tile, TileId, Tiles, Tree, UiResponse,
};

use super::document::Document;
use super::history::History;
use super::scene::Scene;
use super::state::State;
use super::terminals::Terminals;
use super::theme;
use super::world;
use crate::webui::link::ShellEnd;

/// What a tile shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Document,
    History,
    Outliner,
    World,
    Terminals,
}

impl Pane {
    fn title(self) -> &'static str {
        match self {
            Pane::Document => "Document",
            Pane::History => "History",
            Pane::Outliner => "Outliner",
            Pane::World => "World",
            Pane::Terminals => "Terminals",
        }
    }
}

/// The page's widths: the left column, a document's wider, and the
/// terminals'.
const LEFT: f32 = 340.0;
const LEFT_DOCUMENT: f32 = 420.0;
const TERMINALS: f32 = 480.0;

pub struct Layout {
    tree: Tree<Pane>,
    /// The row's columns as they began: the left tabs, the world's tabs and
    /// the terminals'.
    left: TileId,
    world: TileId,
    terminals: TileId,
    /// The world's pane and the terminals', which show no tab bar alone.
    alone: [TileId; 2],
    /// The left column's panes, in the page's order.
    panes: [(Pane, TileId); 3],
    /// Which of them were there last frame.
    shown: [bool; 3],
    /// The mode the left column was last sized for (a document or not).
    sized_for: Option<bool>,
    /// `LOCALGPT_APP_UI_OPEN=outliner`: the Outliner first, once it is there.
    outliner_first: bool,
}

impl Layout {
    pub fn new(outliner_first: bool) -> Self {
        let mut tiles = Tiles::default();
        let document = tiles.insert_pane(Pane::Document);
        let history = tiles.insert_pane(Pane::History);
        let outliner = tiles.insert_pane(Pane::Outliner);
        let left = tiles.insert_tab_tile(vec![document, history, outliner]);
        // Every pane in tabs, so whatever is dragged keeps a tab to be
        // dragged by; the world's and the terminals' show no bar while they
        // stand alone (`Panes::is_tab_bar_visible`).
        let world_pane = tiles.insert_pane(Pane::World);
        let world = tiles.insert_tab_tile(vec![world_pane]);
        let terminals_pane = tiles.insert_pane(Pane::Terminals);
        let terminals = tiles.insert_tab_tile(vec![terminals_pane]);
        let root = tiles.insert_horizontal_tile(vec![left, world, terminals]);
        // Nothing on the left until the app says what is open.
        for id in [document, history, outliner, left] {
            tiles.set_visible(id, false);
        }
        Self {
            tree: Tree::new(egui::Id::unique("the shell's panels"), root, tiles),
            left,
            world,
            terminals,
            alone: [world_pane, terminals_pane],
            panes: [
                (Pane::Document, document),
                (Pane::History, history),
                (Pane::Outliner, outliner),
            ],
            shown: [false; 3],
            sized_for: None,
            outliner_first,
        }
    }

    /// What is open decides what shows: the document's pane while there is
    /// a document, the history's for a live canvas, the outliner whenever
    /// the world has things. When that changes, the first of them is the
    /// tab in front, as the page shows it; when the mode changes, the left
    /// column takes the page's width for it.
    pub fn follow(&mut self, state: &State, width: f32) {
        let there = [
            state.document.is_some(),
            state.history.is_some(),
            state.scene.is_some(),
        ];
        for ((_, id), there) in self.panes.iter().zip(there) {
            self.tree.tiles.set_visible(*id, there);
        }
        if let Some(root) = self.tree.root {
            shows(&mut self.tree.tiles, root);
        }
        if there != self.shown {
            self.shown = there;
            let outliner = self.panes[2].1;
            let first = if self.outliner_first && there[2] {
                Some(outliner)
            } else {
                self.panes
                    .iter()
                    .zip(there)
                    .find_map(|((_, id), there)| there.then_some(*id))
            };
            if let Some(first) = first {
                self.tree.make_active(|id, _| id == first);
            }
        }
        if self.sized_for != Some(state.mode_document) && there.iter().any(|t| *t) {
            self.sized_for = Some(state.mode_document);
            let left = if state.mode_document {
                LEFT_DOCUMENT
            } else {
                LEFT
            };
            self.size(left.min(width * 0.3).max(240.0), width);
        }
    }

    /// The left column at `left` points and the terminals at the page's
    /// width, the world the rest: shares of the row they are in, where they
    /// are still in it.
    fn size(&mut self, left: f32, width: f32) {
        let terminals = TERMINALS.min(width * 0.4).max(280.0);
        let Some(row) = self.tree.tiles.parent_of(self.world) else {
            return;
        };
        let Some(Tile::Container(Container::Linear(linear))) = self.tree.tiles.get_mut(row) else {
            return;
        };
        for (id, share) in [
            (self.left, left),
            (self.world, (width - left - terminals).max(240.0)),
            (self.terminals, terminals),
        ] {
            if linear.children.contains(&id) {
                linear.shares.set_share(id, share);
            }
        }
    }

    /// The world's pane and the terminals', for [`Panes::alone`].
    pub fn alone(&self) -> [TileId; 2] {
        self.alone
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, panes: &mut Panes<'_>) {
        self.tree.ui(panes, ui);
    }
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

/// The panels a tile draws, lent for a frame.
pub struct Panes<'a> {
    pub state: &'a mut State,
    pub end: &'a ShellEnd,
    pub frame: &'a eframe::Frame,
    pub world: &'a mut world::Panel,
    pub world_rect: &'a mut egui::Rect,
    pub terminals: &'a mut Terminals,
    pub scene: &'a mut Scene,
    pub document: &'a mut Document,
    pub history: &'a mut History,
    /// The panes that show no tab bar alone ([`Layout::alone`]).
    pub alone: [TileId; 2],
}

impl egui_tiles::Behavior<Pane> for Panes<'_> {
    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        pane.title().into()
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile: TileId, pane: &mut Pane) -> UiResponse {
        if *pane == Pane::World {
            *self.world_rect = ui.available_rect_before_wrap();
            self.world.ui(ui, self.frame);
            return UiResponse::None;
        }
        egui::Frame::new()
            .fill(theme::PANEL)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                match pane {
                    Pane::Document => {
                        // Kept until the document shows: the latest place
                        // picked is where its text opens.
                        let reveal = self.state.reveal.take();
                        self.document.ui(ui, self.state, reveal, self.end);
                    }
                    Pane::History => self.history.ui(ui, self.state, self.end),
                    Pane::Outliner => self.scene.ui(ui, self.state, self.end),
                    Pane::Terminals => self.terminals.ui(ui, self.end),
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

    /// The world and the terminals stand as the page's columns do, with no
    /// tab over them, until something is dropped beside them as a tab.
    fn is_tab_bar_visible(&self, _tile: TileId, tabs: &egui_tiles::Tabs) -> bool {
        !matches!(tabs.children.as_slice(), [only] if self.alone.contains(only))
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        1.0
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
