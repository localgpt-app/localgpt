//! One terminal tab's screen in the shell: an `alacritty_terminal` emulator
//! fed the bytes of the tab's PTY, which the app holds (`webui/term.rs`), and
//! drawn with egui (`docs/rfcs/authoring/all-rust.md`, "The terminal widget";
//! the first test's widget, `spikes/egui-terminal`).
//!
//! The PTY is the app's, so a paired phone attaches to the same tab; this
//! side keeps a screen, draws it, and sends what is typed. Runs of cells are
//! laid out as galleys in Menlo's three faces, with the system's fonts for
//! what Menlo lacks (eframe's font provider); box drawing, block elements and
//! braille are drawn rather than typeset, as every terminal does.

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config as TermConfig, RenderableContent, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Processor, Rgb};
use eframe::egui::{self, Color32, FontId, Rect, Sense, Stroke, Vec2, pos2, vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Size {
    cols: usize,
    rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// What the emulator says back: replies for the PTY (a cursor position, a
/// colour asked for), and the title.
#[derive(Clone, Default)]
struct Replies(Arc<Mutex<Vec<String>>>);

impl EventListener for Replies {
    fn send_event(&self, event: TermEvent) {
        if let TermEvent::PtyWrite(text) = event
            && let Ok(mut out) = self.0.lock()
        {
            out.push(text);
        }
    }
}

/// The fonts and colours every tab draws with.
pub struct Style {
    font: FontId,
    bold: FontId,
    italic: FontId,
    cell: Vec2,
    palette: Palette,
}

impl Style {
    /// Menlo's faces when the Mac has them (`/System/Library/Fonts`), else
    /// egui's monospace. Call once, before the first frame.
    pub fn install(ctx: &egui::Context, size: f32) -> Self {
        let menlo = std::fs::read("/System/Library/Fonts/Menlo.ttc").ok();
        let mut faces = [
            egui::FontFamily::Monospace,
            egui::FontFamily::Monospace,
            egui::FontFamily::Monospace,
        ];
        if let Some(data) = menlo {
            let mut fonts = egui::FontDefinitions::default();
            for (i, name) in ["menlo", "menlo-bold", "menlo-italic"]
                .into_iter()
                .enumerate()
            {
                let mut face = egui::FontData::from_owned(data.clone());
                face.index = i as u32;
                fonts.font_data.insert(name.to_string(), Arc::new(face));
                let family = egui::FontFamily::Name(name.into());
                fonts
                    .families
                    .insert(family.clone(), vec![name.to_string()]);
                faces[i] = family;
            }
            ctx.set_fonts(fonts);
        }
        let [regular, bold, italic] = faces;
        Self {
            font: FontId::new(size, regular),
            bold: FontId::new(size, bold),
            italic: FontId::new(size, italic),
            // Measured on the first frame, once the fonts exist.
            cell: Vec2::ZERO,
            palette: Palette::dark(),
        }
    }

    fn measure(&mut self, ctx: &egui::Context) {
        if self.cell == Vec2::ZERO {
            let font = self.font.clone();
            self.cell = ctx.fonts_mut(|f| vec2(f.glyph_width(&font, 'M'), f.row_height(&font)));
        }
    }
}

/// One tab's emulator and what its view keeps between frames.
pub struct Screen {
    term: Term<Replies>,
    parser: Processor,
    replies: Replies,
    size: Size,
    preedit: Option<String>,
    scroll: f32,
    selecting: bool,
    /// This window sets the PTY's size: from the start, and again once it
    /// is clicked into after another page (a phone) set it.
    drives: bool,
}

/// What a frame of the view asks of the PTY.
#[derive(Default)]
pub struct Asks {
    /// Typed, pasted and replied, in order.
    pub input: Vec<String>,
    /// The view took another size.
    pub resized: Option<(u16, u16)>,
    /// It was clicked into: this window drives the PTY's size again.
    pub focused: bool,
}

impl Screen {
    pub fn new(cols: u16, rows: u16) -> Self {
        let size = Size {
            cols: cols.max(2) as usize,
            rows: rows.max(1) as usize,
        };
        let replies = Replies::default();
        Self {
            term: Term::new(TermConfig::default(), &size, replies.clone()),
            parser: Processor::new(),
            replies,
            size,
            preedit: None,
            scroll: 0.0,
            selecting: false,
            drives: true,
        }
    }

    /// The PTY's bytes, as they come.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    /// Another page set the PTY's size: draw at it until this one is
    /// clicked into again.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let size = Size {
            cols: cols.max(2) as usize,
            rows: rows.max(1) as usize,
        };
        if size != self.size {
            self.size = size;
            self.term.resize(size);
            self.drives = false;
        }
    }

    fn take_replies(&self, asks: &mut Asks) {
        if let Ok(mut out) = self.replies.0.lock() {
            asks.input.append(&mut out);
        }
    }

    fn point_at(&self, p: egui::Pos2, rect: Rect, cell: Vec2) -> Point {
        let offset = self.term.grid().display_offset() as i32;
        let col = ((p.x - rect.left()) / cell.x).floor().max(0.0) as usize;
        let row = ((p.y - rect.top()) / cell.y).floor().max(0.0) as usize;
        Point::new(
            Line(row.min(self.size.rows - 1) as i32 - offset),
            Column(col.min(self.size.cols - 1)),
        )
    }

    /// The view: the grid in `ui`'s space, the keyboard when it has the
    /// focus. Returns what to send the PTY.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        id: egui::Id,
        style: &mut Style,
        blink: bool,
        // The keys are given to it now, as a click in it gives them.
        focus: bool,
    ) -> Asks {
        style.measure(ui.ctx());
        let cell = style.cell;
        let mut asks = Asks::default();
        // Never past what shows: a grid wider than its panel would put the
        // cursor out of sight.
        let rect = ui.available_rect_before_wrap().intersect(ui.clip_rect());
        let response = ui.interact(rect, id, Sense::click_and_drag());
        // The grid takes the rest of the panel; the panel hears it is used.
        ui.advance_cursor_after_rect(rect);
        if focus || response.clicked() || response.drag_started() {
            response.request_focus();
            asks.focused = true;
            self.drives = true;
        } else if response.has_focus()
            && ui.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
            })
        {
            // A press anywhere else takes the keys with it, as the world's
            // panel gives them up: Space after a click in the timeline
            // plays it rather than typing into the shell.
            response.surrender_focus();
        }
        let focused = response.has_focus();
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        if cell.x > 0.0 && cell.y > 0.0 {
            let size = Size {
                cols: ((rect.width() / cell.x).floor() as usize).max(2),
                rows: ((rect.height() / cell.y).floor() as usize).max(1),
            };
            // The view's size goes to the PTY while this window drives it.
            if size != self.size && self.drives {
                self.size = size;
                self.term.resize(size);
                asks.resized = Some((size.cols as u16, size.rows as u16));
            }
        }
        if focused {
            self.input(ui, &response, rect, cell, &mut asks);
        }
        self.take_replies(&mut asks);
        self.draw(ui, rect, style, focused, blink);
        asks
    }

    /// The size the view would take now, in cells, before it has drawn: what
    /// a tab opens or attaches at.
    pub fn fits(ui: &egui::Ui, style: &mut Style) -> (u16, u16) {
        style.measure(ui.ctx());
        let rect = ui.available_rect_before_wrap().intersect(ui.clip_rect());
        if style.cell.x <= 0.0 || style.cell.y <= 0.0 {
            return (80, 24);
        }
        (
            ((rect.width() / style.cell.x).floor() as u16).max(2),
            ((rect.height() / style.cell.y).floor() as u16).max(1),
        )
    }

    fn input(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: Rect,
        cell: Vec2,
        asks: &mut Asks,
    ) {
        let mode = *self.term.mode();
        let app_cursor = mode.contains(TermMode::APP_CURSOR);
        for event in ui.input(|i| i.events.clone()) {
            match event {
                egui::Event::Text(text) => asks.input.push(text),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(bytes) = key_bytes(key, modifiers, app_cursor) {
                        asks.input
                            .push(String::from_utf8_lossy(&bytes).into_owned());
                    }
                }
                egui::Event::Paste(text) => {
                    if mode.contains(TermMode::BRACKETED_PASTE) {
                        asks.input.push(format!("\x1b[200~{text}\x1b[201~"));
                    } else {
                        asks.input.push(text);
                    }
                }
                egui::Event::Copy => {
                    if let Some(text) = self.term.selection_to_string() {
                        ui.ctx().copy_text(text);
                    }
                }
                egui::Event::Ime(ime) => match ime {
                    egui::ImeEvent::Preedit { text, .. } => {
                        self.preedit = (!text.is_empty()).then_some(text);
                    }
                    egui::ImeEvent::Commit(text) => {
                        self.preedit = None;
                        asks.input.push(text);
                    }
                    _ => self.preedit = None,
                },
                _ => {}
            }
        }
        // The wheel: the history, or arrows on the alternate screen.
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 && cell.y > 0.0 {
            self.scroll += scroll / cell.y;
            let n = self.scroll.trunc() as i32;
            self.scroll -= n as f32;
            if n != 0 {
                if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL)
                {
                    let arrow = if n > 0 { "\x1b[A" } else { "\x1b[B" };
                    for _ in 0..n.abs() {
                        asks.input.push(arrow.into());
                    }
                } else {
                    self.term.scroll_display(Scroll::Delta(n));
                }
            }
        }
        // Selection by drag, a word by double click.
        if response.drag_started() {
            if let Some(p) = response.interact_pointer_pos() {
                let at = self.point_at(p, rect, cell);
                self.term.selection = Some(Selection::new(SelectionType::Simple, at, Side::Left));
                self.selecting = true;
            }
        } else if response.dragged()
            && self.selecting
            && let Some(p) = response.interact_pointer_pos()
        {
            let at = self.point_at(p, rect, cell);
            if let Some(selection) = self.term.selection.as_mut() {
                selection.update(at, Side::Right);
            }
        }
        if response.drag_stopped() {
            self.selecting = false;
        }
        if response.double_clicked() {
            if let Some(p) = response.interact_pointer_pos() {
                let at = self.point_at(p, rect, cell);
                self.term.selection = Some(Selection::new(SelectionType::Semantic, at, Side::Left));
            }
        } else if response.clicked() {
            self.term.selection = None;
        }
    }

    fn draw(&self, ui: &egui::Ui, rect: Rect, style: &Style, focused: bool, blink: bool) {
        let (cw, ch) = (style.cell.x, style.cell.y);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, style.palette.bg);
        if cw <= 0.0 || ch <= 0.0 {
            return;
        }
        let origin = rect.min;
        let rows = self.size.rows;
        let RenderableContent {
            display_iter,
            selection,
            cursor,
            display_offset,
            colors,
            mode: _,
        } = self.term.renderable_content();

        let mut runs: Vec<Run> = Vec::with_capacity(rows * 4);
        let mut current: Option<Run> = None;
        for indexed in display_iter {
            let row = indexed.point.line.0 + display_offset as i32;
            if row < 0 || row as usize >= rows {
                continue;
            }
            let (row, col) = (row as usize, indexed.point.column.0);
            let c = indexed.cell;
            let flags = c.flags;
            if flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let mut fg = style
                .palette
                .resolve(c.fg, colors, flags.contains(Flags::BOLD));
            let mut bg = style.palette.resolve(c.bg, colors, false);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if flags.contains(Flags::DIM) {
                fg = fg.gamma_multiply(0.66);
            }
            if selection
                .as_ref()
                .is_some_and(|s| s.contains(indexed.point))
            {
                bg = style.palette.selection;
            }
            let character = if flags.contains(Flags::HIDDEN) {
                ' '
            } else {
                c.c
            };
            let wide = flags.contains(Flags::WIDE_CHAR);
            let width = if wide { 2 } else { 1 };
            let face = flags & STYLE;
            let ascii = !wide && character.is_ascii();
            match &mut current {
                Some(run)
                    if run.row == row
                        && run.col + run.width == col
                        && run.fg == fg
                        && run.bg == bg
                        && run.flags == face
                        && ascii
                        && run.ascii =>
                {
                    run.text.push(character);
                    run.width += 1;
                }
                _ => {
                    if let Some(run) = current.take() {
                        runs.push(run);
                    }
                    current = Some(Run {
                        row,
                        col,
                        width,
                        text: character.to_string(),
                        fg,
                        bg,
                        flags: face,
                        ascii,
                    });
                }
            }
        }
        if let Some(run) = current.take() {
            runs.push(run);
        }

        for run in &runs {
            let x = origin.x + run.col as f32 * cw;
            let y = origin.y + run.row as f32 * ch;
            let cells = Rect::from_min_size(pos2(x, y), vec2(run.width as f32 * cw, ch));
            if run.bg != style.palette.bg {
                painter.rect_filled(cells, 0.0, run.bg);
            }
            if run.text.trim().is_empty() {
                continue;
            }
            if !run.ascii {
                let mut chars = run.text.chars();
                if let (Some(c), None) = (chars.next(), chars.next())
                    && draw_special(&painter, cells, c, run.fg)
                {
                    continue;
                }
            }
            let font = if run.flags.contains(Flags::BOLD) {
                &style.bold
            } else if run.flags.contains(Flags::ITALIC) {
                &style.italic
            } else {
                &style.font
            };
            let galley = ui
                .ctx()
                .fonts_mut(|f| f.layout_no_wrap(run.text.clone(), font.clone(), run.fg));
            let at = if run.ascii {
                pos2(x, y)
            } else {
                pos2(x + (cells.width() - galley.size().x) / 2.0, y)
            };
            painter.galley(at, galley, run.fg);
            if run.flags.intersects(Flags::ALL_UNDERLINES) {
                let line = y + ch - 1.5;
                painter.line_segment(
                    [pos2(x, line), pos2(cells.right(), line)],
                    Stroke::new(1.0, run.fg),
                );
            }
            if run.flags.contains(Flags::STRIKEOUT) {
                let line = y + ch * 0.55;
                painter.line_segment(
                    [pos2(x, line), pos2(cells.right(), line)],
                    Stroke::new(1.0, run.fg),
                );
            }
        }

        // The cursor, and where the input method's window goes.
        let cursor_row = cursor.point.line.0 + display_offset as i32;
        if cursor_row < 0 || cursor_row as usize >= rows {
            return;
        }
        let x = origin.x + cursor.point.column.0 as f32 * cw;
        let y = origin.y + cursor_row as f32 * ch;
        let at = Rect::from_min_size(pos2(x, y), vec2(cw, ch));
        let color = style.palette.cursor;
        if cursor.shape != CursorShape::Hidden {
            match cursor.shape {
                CursorShape::Block | CursorShape::HollowBlock if focused && blink => {
                    painter.rect_filled(at, 0.0, color);
                }
                CursorShape::Block | CursorShape::HollowBlock => {
                    let _ = painter.rect_stroke(
                        at,
                        0.0,
                        Stroke::new(1.0, color),
                        egui::StrokeKind::Inside,
                    );
                }
                CursorShape::Underline => {
                    painter.rect_filled(
                        Rect::from_min_size(pos2(x, y + ch - 2.0), vec2(cw, 2.0)),
                        0.0,
                        color,
                    );
                }
                CursorShape::Beam => {
                    painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(2.0, ch)), 0.0, color);
                }
                CursorShape::Hidden => {}
            }
        }
        if focused {
            ui.ctx().output_mut(|o| {
                o.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Terminal,
                    rect,
                    cursor_rect: at,
                    should_interrupt_composition: false,
                })
            });
        }
        if let Some(text) = &self.preedit {
            let galley = ui.ctx().fonts_mut(|f| {
                f.layout_no_wrap(text.clone(), style.font.clone(), style.palette.fg)
            });
            let under = Rect::from_min_size(pos2(x, y), vec2(galley.size().x.max(cw), ch));
            painter.rect_filled(under, 0.0, rgb(50, 50, 70));
            painter.galley(pos2(x, y), galley, style.palette.fg);
            painter.line_segment(
                [pos2(x, y + ch - 1.0), pos2(under.right(), y + ch - 1.0)],
                Stroke::new(1.5, rgb(120, 170, 255)),
            );
        }
    }
}

const STYLE: Flags = Flags::BOLD
    .union(Flags::ITALIC)
    .union(Flags::ALL_UNDERLINES)
    .union(Flags::STRIKEOUT);

/// Cells next to each other that draw alike, laid out as one galley.
struct Run {
    row: usize,
    col: usize,
    width: usize,
    text: String,
    fg: Color32,
    bg: Color32,
    flags: Flags,
    ascii: bool,
}

struct Palette {
    fg: Color32,
    bg: Color32,
    cursor: Color32,
    selection: Color32,
    ansi: [Color32; 16],
}

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Palette {
    fn dark() -> Self {
        Self {
            fg: rgb(222, 222, 222),
            bg: rgb(22, 22, 26),
            cursor: Color32::from_rgba_unmultiplied(230, 230, 230, 170),
            selection: rgb(60, 80, 120),
            ansi: [
                rgb(0, 0, 0),
                rgb(205, 49, 49),
                rgb(13, 188, 121),
                rgb(229, 229, 16),
                rgb(36, 114, 200),
                rgb(188, 63, 188),
                rgb(17, 168, 205),
                rgb(229, 229, 229),
                rgb(102, 102, 102),
                rgb(241, 76, 76),
                rgb(35, 209, 139),
                rgb(245, 245, 67),
                rgb(59, 142, 234),
                rgb(214, 112, 214),
                rgb(41, 184, 219),
                rgb(255, 255, 255),
            ],
        }
    }

    fn indexed(&self, i: u8) -> Color32 {
        match i {
            0..=15 => self.ansi[i as usize],
            16..=231 => {
                let i = i - 16;
                let (r, g, b) = (i / 36, (i % 36) / 6, i % 6);
                let level = |x: u8| if x == 0 { 0 } else { 55 + 40 * x };
                rgb(level(r), level(g), level(b))
            }
            _ => {
                let v = 8 + 10 * (i - 232);
                rgb(v, v, v)
            }
        }
    }

    fn resolve(&self, color: AnsiColor, colors: &Colors, bold: bool) -> Color32 {
        let of = |c: Rgb| rgb(c.r, c.g, c.b);
        match color {
            AnsiColor::Spec(c) => of(c),
            AnsiColor::Indexed(i) => colors[i as usize]
                .map(of)
                .unwrap_or_else(|| self.indexed(i)),
            AnsiColor::Named(name) => {
                if let Some(c) = colors[name] {
                    return of(c);
                }
                let index = name as usize;
                match name {
                    NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => {
                        self.fg
                    }
                    NamedColor::Background => self.bg,
                    NamedColor::DimForeground => self.fg.gamma_multiply(0.66),
                    _ if index < 16 => {
                        let index = if bold && index < 8 { index + 8 } else { index };
                        self.ansi[index]
                    }
                    _ if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize)
                        .contains(&index) =>
                    {
                        self.ansi[index - NamedColor::DimBlack as usize].gamma_multiply(0.66)
                    }
                    _ => self.fg,
                }
            }
        }
    }
}

/// Box drawing, block elements and braille, drawn to fill the cell: a
/// font's glyphs for them rarely do, so lines break and dots drift. False for
/// a character it does not draw.
fn draw_special(p: &egui::Painter, r: Rect, c: char, color: Color32) -> bool {
    let (w, h) = (r.width(), r.height());
    let (cx, cy) = (r.center().x, r.center().y);
    // Arms: up 8, down 4, left 2, right 1; weight: light 0, heavy 1, double 2.
    let arms: Option<(u8, u8)> = match c {
        '─' => Some((0b0011, 0)),
        '│' => Some((0b1100, 0)),
        '┌' | '╭' => Some((0b0101, 0)),
        '┐' | '╮' => Some((0b0110, 0)),
        '└' | '╰' => Some((0b1001, 0)),
        '┘' | '╯' => Some((0b1010, 0)),
        '├' => Some((0b1101, 0)),
        '┤' => Some((0b1110, 0)),
        '┬' => Some((0b0111, 0)),
        '┴' => Some((0b1011, 0)),
        '┼' => Some((0b1111, 0)),
        '╴' => Some((0b0010, 0)),
        '╵' => Some((0b1000, 0)),
        '╶' => Some((0b0001, 0)),
        '╷' => Some((0b0100, 0)),
        '━' => Some((0b0011, 1)),
        '┃' => Some((0b1100, 1)),
        '┏' => Some((0b0101, 1)),
        '┓' => Some((0b0110, 1)),
        '┗' => Some((0b1001, 1)),
        '┛' => Some((0b1010, 1)),
        '┣' => Some((0b1101, 1)),
        '┫' => Some((0b1110, 1)),
        '┳' => Some((0b0111, 1)),
        '┻' => Some((0b1011, 1)),
        '╋' => Some((0b1111, 1)),
        '═' => Some((0b0011, 2)),
        '║' => Some((0b1100, 2)),
        '╔' => Some((0b0101, 2)),
        '╗' => Some((0b0110, 2)),
        '╚' => Some((0b1001, 2)),
        '╝' => Some((0b1010, 2)),
        '╠' => Some((0b1101, 2)),
        '╣' => Some((0b1110, 2)),
        '╦' => Some((0b0111, 2)),
        '╩' => Some((0b1011, 2)),
        '╬' => Some((0b1111, 2)),
        _ => None,
    };
    if let Some((bits, weight)) = arms {
        let (up, down, left, right) = (bits & 8 != 0, bits & 4 != 0, bits & 2 != 0, bits & 1 != 0);
        let stroke = Stroke::new(if weight == 1 { 2.5 } else { 1.0 }, color);
        let offsets: &[f32] = if weight == 2 { &[-1.5, 1.5] } else { &[0.0] };
        for &o in offsets {
            if up {
                p.line_segment([pos2(cx + o, r.top()), pos2(cx + o, cy + o.abs())], stroke);
            }
            if down {
                p.line_segment(
                    [pos2(cx + o, cy - o.abs()), pos2(cx + o, r.bottom())],
                    stroke,
                );
            }
            if left {
                p.line_segment([pos2(r.left(), cy + o), pos2(cx + o.abs(), cy + o)], stroke);
            }
            if right {
                p.line_segment(
                    [pos2(cx - o.abs(), cy + o), pos2(r.right(), cy + o)],
                    stroke,
                );
            }
        }
        return true;
    }
    let fill = |within: Rect| {
        p.rect_filled(within, 0.0, color);
    };
    let (l, t, rt, b) = (r.left(), r.top(), r.right(), r.bottom());
    match c {
        '█' => fill(r),
        '▀' => fill(Rect::from_min_max(r.min, pos2(rt, cy))),
        '▄' => fill(Rect::from_min_max(pos2(l, cy), r.max)),
        '▌' => fill(Rect::from_min_max(r.min, pos2(cx, b))),
        '▐' => fill(Rect::from_min_max(pos2(cx, t), r.max)),
        '▔' => fill(Rect::from_min_max(r.min, pos2(rt, t + h / 8.0))),
        '▕' => fill(Rect::from_min_max(pos2(rt - w / 8.0, t), r.max)),
        '░' | '▒' | '▓' => {
            let alpha = match c {
                '░' => 0.3,
                '▒' => 0.55,
                _ => 0.8,
            };
            p.rect_filled(r, 0.0, color.gamma_multiply(alpha));
        }
        '▁'..='▇' => {
            let part = (c as u32 - 0x2580) as f32 / 8.0;
            fill(Rect::from_min_max(pos2(l, b - h * part), r.max));
        }
        '▉'..='▏' => {
            let part = (0x2590 - c as u32) as f32 / 8.0;
            fill(Rect::from_min_max(r.min, pos2(l + w * part, b)));
        }
        '▖' | '▗' | '▘' | '▝' | '▙' | '▚' | '▛' | '▜' | '▞' | '▟' => {
            let (ul, ur, ll, lr) = match c {
                '▖' => (false, false, true, false),
                '▗' => (false, false, false, true),
                '▘' => (true, false, false, false),
                '▝' => (false, true, false, false),
                '▙' => (true, false, true, true),
                '▚' => (true, false, false, true),
                '▛' => (true, true, true, false),
                '▜' => (true, true, false, true),
                '▞' => (false, true, true, false),
                _ => (false, true, true, true),
            };
            if ul {
                fill(Rect::from_min_max(r.min, pos2(cx, cy)));
            }
            if ur {
                fill(Rect::from_min_max(pos2(cx, t), pos2(rt, cy)));
            }
            if ll {
                fill(Rect::from_min_max(pos2(l, cy), pos2(cx, b)));
            }
            if lr {
                fill(Rect::from_min_max(pos2(cx, cy), r.max));
            }
        }
        '\u{2800}'..='\u{28FF}' => {
            let bits = c as u32 - 0x2800;
            let dot = (w / 7.0).max(1.0);
            let xs = [l + w * 0.3, l + w * 0.7];
            let ys: [f32; 4] = std::array::from_fn(|i| t + h * (0.5 + i as f32) / 4.0);
            let dots = [
                (0, 0),
                (0, 1),
                (0, 2),
                (1, 0),
                (1, 1),
                (1, 2),
                (0, 3),
                (1, 3),
            ];
            for (bit, (col, row)) in dots.iter().enumerate() {
                if bits & (1 << bit) != 0 {
                    p.circle_filled(pos2(xs[*col], ys[*row]), dot, color);
                }
            }
        }
        _ => return false,
    }
    true
}

/// What a key sends a terminal: xterm's sequences, with the modifiers in
/// them, the cursor keys as the application asked (`DECCKM`), Ctrl-letter as
/// its control code, and Shift-Enter as ESC CR, as the page sends it (a new
/// line in a TUI's prompt rather than a send). Printable keys arrive as text.
fn key_bytes(key: egui::Key, m: egui::Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    use egui::Key::*;
    let modified = m.shift || m.alt || m.ctrl;
    let code = 1 + (m.shift as u8) + 2 * (m.alt as u8) + 4 * (m.ctrl as u8);
    let arrow = |c: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{code}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{n};{code}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    Some(match key {
        Enter if m.shift && !m.ctrl && !m.alt && !m.mac_cmd => b"\x1b\r".to_vec(),
        Enter => b"\r".to_vec(),
        Backspace if m.alt => b"\x1b\x7f".to_vec(),
        Backspace => b"\x7f".to_vec(),
        Tab if m.shift => b"\x1b[Z".to_vec(),
        Tab => b"\t".to_vec(),
        Escape => b"\x1b".to_vec(),
        ArrowUp => arrow('A'),
        ArrowDown => arrow('B'),
        ArrowRight => arrow('C'),
        ArrowLeft => arrow('D'),
        Home if app_cursor => b"\x1bOH".to_vec(),
        Home => b"\x1b[H".to_vec(),
        End if app_cursor => b"\x1bOF".to_vec(),
        End => b"\x1b[F".to_vec(),
        Insert => tilde(2),
        Delete => tilde(3),
        PageUp => tilde(5),
        PageDown => tilde(6),
        F1 => b"\x1bOP".to_vec(),
        F2 => b"\x1bOQ".to_vec(),
        F3 => b"\x1bOR".to_vec(),
        F4 => b"\x1bOS".to_vec(),
        F5 => tilde(15),
        F6 => tilde(17),
        F7 => tilde(18),
        F8 => tilde(19),
        F9 => tilde(20),
        F10 => tilde(21),
        F11 => tilde(23),
        F12 => tilde(24),
        _ if m.ctrl && !m.alt && !m.mac_cmd => {
            let name = key.name();
            let mut chars = name.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) if c.is_ascii_alphabetic() => {
                    Some(vec![(c.to_ascii_lowercase() as u8) & 0x1f])
                }
                _ => match name {
                    "Space" => Some(vec![0]),
                    "[" => Some(vec![0x1b]),
                    "]" => Some(vec![0x1d]),
                    "\\" => Some(vec![0x1c]),
                    _ => None,
                },
            };
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_send_what_xterm_sends() {
        let none = egui::Modifiers::NONE;
        let ctrl = egui::Modifiers { ctrl: true, ..none };
        let shift = egui::Modifiers {
            shift: true,
            ..none
        };
        assert_eq!(key_bytes(egui::Key::Enter, none, false).unwrap(), b"\r");
        assert_eq!(
            key_bytes(egui::Key::Enter, shift, false).unwrap(),
            b"\x1b\r"
        );
        assert_eq!(key_bytes(egui::Key::C, ctrl, false).unwrap(), [3]);
        assert_eq!(
            key_bytes(egui::Key::ArrowUp, none, false).unwrap(),
            b"\x1b[A"
        );
        assert_eq!(
            key_bytes(egui::Key::ArrowUp, none, true).unwrap(),
            b"\x1bOA"
        );
        assert_eq!(
            key_bytes(egui::Key::ArrowLeft, shift, false).unwrap(),
            b"\x1b[1;2D"
        );
        assert_eq!(
            key_bytes(egui::Key::A, none, false),
            None,
            "printable keys come as text"
        );
    }

    #[test]
    fn a_screen_draws_what_the_pty_writes() {
        let mut screen = Screen::new(20, 4);
        screen.feed("hi \u{4f60}\u{597d}\r\n\x1b[1mbold".as_bytes());
        let grid = screen.term.grid();
        assert_eq!(grid[Line(0)][Column(0)].c, 'h');
        assert_eq!(grid[Line(0)][Column(3)].c, '你');
        assert!(
            grid[Line(0)][Column(3)].flags.contains(Flags::WIDE_CHAR),
            "two cells wide"
        );
        assert_eq!(grid[Line(0)][Column(5)].c, '好');
        assert!(grid[Line(1)][Column(0)].flags.contains(Flags::BOLD));
    }

    #[test]
    fn the_emulators_replies_go_back_to_the_pty() {
        let mut screen = Screen::new(20, 4);
        // Where is the cursor? (DSR 6)
        screen.feed(b"ab\x1b[6n");
        let mut asks = Asks::default();
        screen.take_replies(&mut asks);
        assert_eq!(asks.input, vec!["\x1b[1;3R".to_string()]);
    }
}
