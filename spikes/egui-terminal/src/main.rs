//! Test 1 of docs/rfcs/authoring/all-rust.md: a terminal widget on
//! alacritty_terminal in an egui panel, measured under the loads of
//! scripts/world-in-the-page/load.py (gone with the page's host; tag
//! `desktop-the-page` has it). A crate of its own: nothing here reaches
//! the apps.
//!
//! Environment:
//!   SPIKE_CMD       a command for `zsh -lc`; unset, the login shell
//!   SPIKE_CWD       the shell's folder (default: the current one)
//!   SPIKE_EXIT      seconds after which report.json is written and the app closes
//!   SPIKE_SHOT      seconds at which shot.png is saved
//!   SPIKE_RESIZE    seconds at which the window is resized, to test the PTY resize
//!   SPIKE_IME       seconds at which a preedit is drawn, then committed through
//!                   the widget's own path (the OS's input method needs a hand)
//!   SPIKE_IME_TEXT  the text committed then (default 你好，世界)
//!   SPIKE_OUT       the folder for report.json and shot.png (default .)
//!   SPIKE_FONT      font size in points (default 13)
//!   SPIKE_LABEL     a name for the run, written into the report
//!   SPIKE_HZ        the display's rate, for the late-frame count (default 120)
//!   SPIKE_ANIMATE   1: redraw every frame, as the window does while a world plays

use std::{
    borrow::Cow,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use alacritty_terminal::{
    event::{Event as TermEvent, EventListener, WindowSize},
    event_loop::{EventLoop, EventLoopSender, Msg},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point, Side},
    selection::{Selection, SelectionType},
    sync::FairMutex,
    term::{Config as TermConfig, RenderableContent, Term, TermMode, cell::Flags, color::Colors},
    tty,
    vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Rgb},
};
use eframe::egui::{self, Color32, FontId, Rect, Sense, Stroke, Vec2, pos2, vec2};

fn env_f64(name: &str) -> Option<f64> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

/// What the terminal tells the window: new content (repaint), a title, the
/// child gone. Cloned into the `Term` and the PTY loop.
#[derive(Clone)]
struct Proxy {
    ctx: egui::Context,
    wakeups: Arc<AtomicU64>,
    title: Arc<Mutex<String>>,
    exited: Arc<AtomicBool>,
    sender: Arc<Mutex<Option<Arc<EventLoopSender>>>>,
}

impl EventListener for Proxy {
    fn send_event(&self, event: TermEvent) {
        match event {
            TermEvent::Wakeup => {
                self.wakeups.fetch_add(1, Ordering::Relaxed);
                self.ctx.request_repaint();
            }
            TermEvent::Title(t) => *self.title.lock().unwrap() = t,
            TermEvent::ResetTitle => self.title.lock().unwrap().clear(),
            TermEvent::PtyWrite(s) => {
                if let Some(tx) = self.sender.lock().unwrap().as_ref() {
                    let _ = tx.send(Msg::Input(s.into_bytes().into()));
                }
            }
            TermEvent::Exit | TermEvent::ChildExit(_) => {
                self.exited.store(true, Ordering::Relaxed);
                self.ctx.request_repaint();
            }
            _ => {}
        }
    }
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
                let v = |x: u8| if x == 0 { 0 } else { 55 + 40 * x };
                rgb(v(r), v(g), v(b))
            }
            _ => {
                let v = 8 + 10 * (i - 232);
                rgb(v, v, v)
            }
        }
    }

    fn resolve(&self, c: AnsiColor, colors: &Colors, bold: bool) -> Color32 {
        let of = |c: Rgb| rgb(c.r, c.g, c.b);
        match c {
            AnsiColor::Spec(c) => of(c),
            AnsiColor::Indexed(i) => colors[i as usize]
                .map(of)
                .unwrap_or_else(|| self.indexed(i)),
            AnsiColor::Named(n) => {
                if let Some(c) = colors[n] {
                    return of(c);
                }
                let idx = n as usize;
                match n {
                    NamedColor::Foreground | NamedColor::BrightForeground => self.fg,
                    NamedColor::Background => self.bg,
                    NamedColor::Cursor => self.fg,
                    NamedColor::DimForeground => self.fg.gamma_multiply(0.66),
                    _ if idx < 16 => {
                        let idx = if bold && idx < 8 { idx + 8 } else { idx };
                        self.ansi[idx]
                    }
                    _ if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize)
                        .contains(&idx) =>
                    {
                        self.ansi[idx - NamedColor::DimBlack as usize].gamma_multiply(0.66)
                    }
                    _ => self.fg,
                }
            }
        }
    }
}

const STYLE: Flags = Flags::BOLD
    .union(Flags::ITALIC)
    .union(Flags::ALL_UNDERLINES)
    .union(Flags::STRIKEOUT);

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

struct Terminal {
    id: egui::Id,
    term: Arc<FairMutex<Term<Proxy>>>,
    sender: Arc<EventLoopSender>,
    proxy: Proxy,
    size: Size,
    cell: Vec2,
    font: FontId,
    font_bold: FontId,
    font_italic: FontId,
    palette: Palette,
    preedit: Option<String>,
    scroll_accum: f32,
    selecting: bool,
}

impl Terminal {
    // egui 0.37 deprecates `Id::new`; the widget's id is made before any `Ui` exists.
    #[allow(deprecated)]
    fn spawn(
        ctx: &egui::Context,
        cmd: Option<String>,
        cwd: Option<PathBuf>,
        font: FontId,
        cell: Vec2,
        size: Size,
        menlo: bool,
    ) -> std::io::Result<Self> {
        let proxy = Proxy {
            ctx: ctx.clone(),
            wakeups: Default::default(),
            title: Default::default(),
            exited: Default::default(),
            sender: Default::default(),
        };
        let term = Term::new(TermConfig::default(), &size, proxy.clone());
        let term = Arc::new(FairMutex::new(term));
        let shell = cmd.map(|c| tty::Shell::new("/bin/zsh".into(), vec!["-lc".into(), c]));
        let mut env = HashMap::new();
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("COLORTERM".to_string(), "truecolor".to_string());
        let options = tty::Options {
            shell,
            working_directory: cwd,
            drain_on_exit: false,
            env,
        };
        let window_size = WindowSize {
            num_lines: size.rows as u16,
            num_cols: size.cols as u16,
            cell_width: cell.x as u16,
            cell_height: cell.y as u16,
        };
        let pty = tty::new(&options, window_size, 0)?;
        let event_loop = EventLoop::new(term.clone(), proxy.clone(), pty, false, false)?;
        let sender = Arc::new(event_loop.channel());
        *proxy.sender.lock().unwrap() = Some(sender.clone());
        let _ = event_loop.spawn();
        let face = |name: &str| {
            if menlo {
                FontId::new(font.size, egui::FontFamily::Name(name.into()))
            } else {
                font.clone()
            }
        };
        Ok(Self {
            id: egui::Id::new("spike-terminal"),
            term,
            sender,
            proxy,
            size,
            cell,
            font_bold: face("menlo-bold"),
            font_italic: face("menlo-italic"),
            font,
            palette: Palette::dark(),
            preedit: None,
            scroll_accum: 0.0,
            selecting: false,
        })
    }

    fn write<B: Into<Cow<'static, [u8]>>>(&self, bytes: B) {
        let _ = self.sender.send(Msg::Input(bytes.into()));
    }

    fn resize(&mut self, size: Size) {
        self.size = size;
        self.term.lock().resize(size);
        let _ = self.sender.send(Msg::Resize(WindowSize {
            num_lines: size.rows as u16,
            num_cols: size.cols as u16,
            cell_width: self.cell.x as u16,
            cell_height: self.cell.y as u16,
        }));
    }

    fn point_at(&self, p: egui::Pos2, rect: Rect, display_offset: usize) -> Point {
        let col = ((p.x - rect.left()) / self.cell.x).floor().max(0.0) as usize;
        let row = ((p.y - rect.top()) / self.cell.y).floor().max(0.0) as usize;
        Point::new(
            Line(row.min(self.size.rows - 1) as i32 - display_offset as i32),
            Column(col.min(self.size.cols - 1)),
        )
    }

    fn input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: Rect, display_offset: usize) {
        let mode = *self.term.lock().mode();
        let app_cursor = mode.contains(TermMode::APP_CURSOR);
        for ev in ui.input(|i| i.events.clone()) {
            match ev {
                egui::Event::Text(t) => self.write(t.into_bytes()),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(bytes) = key_bytes(key, modifiers, app_cursor) {
                        self.write(bytes);
                    }
                }
                egui::Event::Paste(t) => {
                    if mode.contains(TermMode::BRACKETED_PASTE) {
                        self.write(format!("\x1b[200~{t}\x1b[201~").into_bytes());
                    } else {
                        self.write(t.into_bytes());
                    }
                }
                egui::Event::Copy => {
                    if let Some(s) = self.term.lock().selection_to_string() {
                        ui.ctx().copy_text(s);
                    }
                }
                egui::Event::Ime(ime) => match ime {
                    egui::ImeEvent::Preedit { text, .. } => {
                        self.preedit = if text.is_empty() { None } else { Some(text) };
                    }
                    egui::ImeEvent::Commit(t) => {
                        self.preedit = None;
                        self.write(t.into_bytes());
                    }
                    _ => self.preedit = None,
                },
                _ => {}
            }
        }
        // The wheel: history, or arrows on the alternate screen.
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            self.scroll_accum += scroll / self.cell.y;
            let n = self.scroll_accum.trunc() as i32;
            self.scroll_accum -= n as f32;
            if n != 0 {
                if mode.contains(TermMode::ALT_SCREEN) && mode.contains(TermMode::ALTERNATE_SCROLL)
                {
                    let seq: &[u8] = if n > 0 { b"\x1b[A" } else { b"\x1b[B" };
                    for _ in 0..n.abs() {
                        self.write(seq.to_vec());
                    }
                } else {
                    self.term.lock().scroll_display(Scroll::Delta(n));
                }
            }
        }
        // Selection by drag.
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                let pt = self.point_at(p, rect, display_offset);
                self.term.lock().selection =
                    Some(Selection::new(SelectionType::Simple, pt, Side::Left));
                self.selecting = true;
            }
        } else if resp.dragged()
            && self.selecting
            && let Some(p) = resp.interact_pointer_pos()
        {
            let pt = self.point_at(p, rect, display_offset);
            if let Some(sel) = self.term.lock().selection.as_mut() {
                sel.update(pt, Side::Right);
            }
        }
        if resp.drag_stopped() {
            self.selecting = false;
        }
        if resp.double_clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                let pt = self.point_at(p, rect, display_offset);
                self.term.lock().selection =
                    Some(Selection::new(SelectionType::Semantic, pt, Side::Left));
            }
        } else if resp.clicked() {
            self.term.lock().selection = None;
        }
    }

    /// Draws the grid; returns (galleys laid out, time spent).
    fn show(&mut self, ui: &mut egui::Ui, blink_on: bool) -> (usize, Duration) {
        let rect = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(rect, Sense::click_and_drag());
        if resp.clicked() || resp.drag_started() {
            ui.memory_mut(|m| m.request_focus(self.id));
        }
        let focused = ui.memory(|m| m.has_focus(self.id));
        if focused {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    self.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        let cols = ((rect.width() / self.cell.x).floor() as usize).max(2);
        let rows = ((rect.height() / self.cell.y).floor() as usize).max(1);
        if (Size { cols, rows }) != self.size {
            self.resize(Size { cols, rows });
        }
        let display_offset = self.term.lock().grid().display_offset();
        if focused {
            self.input(ui, &resp, rect, display_offset);
        }

        let t0 = Instant::now();
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, self.palette.bg);
        let (cw, ch) = (self.cell.x, self.cell.y);
        let origin = rect.min;
        let term = self.term.lock();
        let RenderableContent {
            display_iter,
            selection,
            cursor,
            display_offset,
            colors,
            mode: _,
        } = term.renderable_content();

        let mut runs: Vec<Run> = Vec::with_capacity(rows * 4);
        let mut cur: Option<Run> = None;
        for cell in display_iter {
            let row = cell.point.line.0 + display_offset as i32;
            if row < 0 || row as usize >= rows {
                continue;
            }
            let row = row as usize;
            let col = cell.point.column.0;
            let c = cell.cell;
            let flags = c.flags;
            if flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let bold = flags.contains(Flags::BOLD);
            let mut fg = self.palette.resolve(c.fg, colors, bold);
            let mut bg = self.palette.resolve(c.bg, colors, false);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if flags.contains(Flags::DIM) {
                fg = fg.gamma_multiply(0.66);
            }
            if selection.as_ref().is_some_and(|s| s.contains(cell.point)) {
                bg = self.palette.selection;
            }
            let ch_ = if flags.contains(Flags::HIDDEN) {
                ' '
            } else {
                c.c
            };
            let wide = flags.contains(Flags::WIDE_CHAR);
            let width = if wide { 2 } else { 1 };
            let style = flags & STYLE;
            let ascii = !wide && ch_.is_ascii();
            match &mut cur {
                Some(r)
                    if r.row == row
                        && r.col + r.width == col
                        && r.fg == fg
                        && r.bg == bg
                        && r.flags == style
                        && ascii
                        && r.ascii =>
                {
                    r.text.push(ch_);
                    r.width += 1;
                }
                _ => {
                    if let Some(r) = cur.take() {
                        runs.push(r);
                    }
                    cur = Some(Run {
                        row,
                        col,
                        width,
                        text: ch_.to_string(),
                        fg,
                        bg,
                        flags: style,
                        ascii,
                    });
                }
            }
        }
        if let Some(r) = cur.take() {
            runs.push(r);
        }

        let mut galleys = 0usize;
        for r in &runs {
            let x = origin.x + r.col as f32 * cw;
            let y = origin.y + r.row as f32 * ch;
            let rrect = Rect::from_min_size(pos2(x, y), vec2(r.width as f32 * cw, ch));
            if r.bg != self.palette.bg {
                painter.rect_filled(rrect, 0.0, r.bg);
            }
            if r.text.trim().is_empty() {
                continue;
            }
            if !r.ascii {
                let mut chars = r.text.chars();
                if let (Some(c), None) = (chars.next(), chars.next())
                    && draw_special(&painter, rrect, c, r.fg)
                {
                    continue;
                }
            }
            let font = if r.flags.contains(Flags::BOLD) {
                &self.font_bold
            } else if r.flags.contains(Flags::ITALIC) {
                &self.font_italic
            } else {
                &self.font
            };
            let galley = ui
                .ctx()
                .fonts_mut(|f| f.layout_no_wrap(r.text.clone(), font.clone(), r.fg));
            galleys += 1;
            let pos = if r.ascii {
                pos2(x, y)
            } else {
                pos2(x + (rrect.width() - galley.size().x) / 2.0, y)
            };
            painter.galley(pos, galley, r.fg);
            if r.flags.intersects(Flags::ALL_UNDERLINES) {
                let yy = y + ch - 1.5;
                painter.line_segment(
                    [pos2(x, yy), pos2(rrect.right(), yy)],
                    Stroke::new(1.0, r.fg),
                );
            }
            if r.flags.contains(Flags::STRIKEOUT) {
                let yy = y + ch * 0.55;
                painter.line_segment(
                    [pos2(x, yy), pos2(rrect.right(), yy)],
                    Stroke::new(1.0, r.fg),
                );
            }
        }

        // The cursor, and where the input method's window goes.
        let crow = cursor.point.line.0 + display_offset as i32;
        if crow >= 0 && (crow as usize) < rows {
            let x = origin.x + cursor.point.column.0 as f32 * cw;
            let y = origin.y + crow as f32 * ch;
            let cell_rect = Rect::from_min_size(pos2(x, y), vec2(cw, ch));
            if focused && cursor.shape != CursorShape::Hidden {
                match cursor.shape {
                    CursorShape::Block | CursorShape::HollowBlock => {
                        if blink_on || !focused {
                            painter.rect_filled(cell_rect, 0.0, self.palette.cursor);
                        } else {
                            let _ = painter.rect_stroke(
                                cell_rect,
                                0.0,
                                Stroke::new(1.0, self.palette.cursor),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                    CursorShape::Underline => {
                        painter.rect_filled(
                            Rect::from_min_size(pos2(x, y + ch - 2.0), vec2(cw, 2.0)),
                            0.0,
                            self.palette.cursor,
                        );
                    }
                    CursorShape::Beam => {
                        painter.rect_filled(
                            Rect::from_min_size(pos2(x, y), vec2(2.0, ch)),
                            0.0,
                            self.palette.cursor,
                        );
                    }
                    CursorShape::Hidden => {}
                }
            }
            if focused {
                ui.ctx().output_mut(|o| {
                    o.ime = Some(egui::output::IMEOutput {
                        purpose: egui::IMEPurpose::Terminal,
                        rect,
                        cursor_rect: cell_rect,
                        should_interrupt_composition: false,
                    })
                });
            }
            if let Some(p) = &self.preedit {
                let galley = ui
                    .ctx()
                    .fonts_mut(|f| f.layout_no_wrap(p.clone(), self.font.clone(), self.palette.fg));
                let prect = Rect::from_min_size(pos2(x, y), vec2(galley.size().x.max(cw), ch));
                painter.rect_filled(prect, 0.0, rgb(50, 50, 70));
                painter.galley(pos2(x, y), galley, self.palette.fg);
                painter.line_segment(
                    [pos2(x, y + ch - 1.0), pos2(prect.right(), y + ch - 1.0)],
                    Stroke::new(1.5, rgb(120, 170, 255)),
                );
            }
        }
        (galleys, t0.elapsed())
    }
}

/// Box drawing, block elements and braille are drawn, not typeset: a font's
/// glyphs for them rarely fill the cell, so lines break and dots drift.
/// Every terminal does this. Returns false for a character it does not know.
fn draw_special(p: &egui::Painter, r: Rect, c: char, color: Color32) -> bool {
    let (w, h) = (r.width(), r.height());
    let (cx, cy) = (r.center().x, r.center().y);
    // bits: up 8, down 4, left 2, right 1; style: 0 light, 1 heavy, 2 double
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
    if let Some((bits, style)) = arms {
        let (up, down, left, right) = (bits & 8 != 0, bits & 4 != 0, bits & 2 != 0, bits & 1 != 0);
        let stroke = Stroke::new(if style == 1 { 2.5 } else { 1.0 }, color);
        let offsets: &[f32] = if style == 2 { &[-1.5, 1.5] } else { &[0.0] };
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
    let fill = |rr: Rect| {
        p.rect_filled(rr, 0.0, color);
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
            let a = match c {
                '░' => 0.3,
                '▒' => 0.55,
                _ => 0.8,
            };
            p.rect_filled(r, 0.0, color.gamma_multiply(a));
        }
        '▁'..='▇' => {
            let frac = (c as u32 - 0x2580) as f32 / 8.0;
            fill(Rect::from_min_max(pos2(l, b - h * frac), r.max));
        }
        '▉'..='▏' => {
            let frac = (0x2590 - c as u32) as f32 / 8.0;
            fill(Rect::from_min_max(r.min, pos2(l + w * frac, b)));
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
            let map = [
                (0, 0),
                (0, 1),
                (0, 2),
                (1, 0),
                (1, 1),
                (1, 2),
                (0, 3),
                (1, 3),
            ];
            for (bit, (col, row)) in map.iter().enumerate() {
                if bits & (1 << bit) != 0 {
                    p.circle_filled(pos2(xs[*col], ys[*row]), dot, color);
                }
            }
        }
        _ => return false,
    }
    true
}

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
        Enter => b"\r".to_vec(),
        Backspace => {
            if m.alt {
                b"\x1b\x7f".to_vec()
            } else {
                b"\x7f".to_vec()
            }
        }
        Tab => {
            if m.shift {
                b"\x1b[Z".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        Escape => b"\x1b".to_vec(),
        ArrowUp => arrow('A'),
        ArrowDown => arrow('B'),
        ArrowRight => arrow('C'),
        ArrowLeft => arrow('D'),
        Home => {
            if app_cursor {
                b"\x1bOH".to_vec()
            } else {
                b"\x1b[H".to_vec()
            }
        }
        End => {
            if app_cursor {
                b"\x1bOF".to_vec()
            } else {
                b"\x1b[F".to_vec()
            }
        }
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
        _ => {
            if m.ctrl && !m.alt && !m.command {
                let name = key.name();
                if name.len() == 1 {
                    let c = name.chars().next()?;
                    if c.is_ascii_alphabetic() {
                        return Some(vec![(c.to_ascii_lowercase() as u8) & 0x1f]);
                    }
                }
                return match name {
                    "Space" => Some(vec![0]),
                    "[" => Some(vec![0x1b]),
                    "]" => Some(vec![0x1d]),
                    "\\" => Some(vec![0x1c]),
                    _ => None,
                };
            }
            return None;
        }
    })
}

fn save_png(path: &Path, img: &egui::ColorImage) {
    let [w, h] = img.size;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    match image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
        Some(buf) => match buf.save(path) {
            Ok(()) => eprintln!("spike: wrote {}", path.display()),
            Err(e) => eprintln!("spike: {}: {e}", path.display()),
        },
        None => eprintln!("spike: screenshot of {w}x{h} had the wrong length"),
    }
}

struct Spike {
    term: Terminal,
    t0: Instant,
    last: Instant,
    frames: Vec<[f32; 3]>,
    cpu_ms: Vec<f32>,
    galleys: Vec<usize>,
    exit_at: Option<f64>,
    shot_at: Option<f64>,
    resize_at: Option<f64>,
    ime_at: Option<f64>,
    ime_text: String,
    ime_stage: u8,
    shot_done: bool,
    resize_done: bool,
    done: bool,
    out: PathBuf,
    label: String,
    hz: f64,
    cell_measured: bool,
    animate: bool,
}

impl Spike {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.add_font_provider(Arc::new(
            egui_system_font_provider::SystemFontProvider::new(),
        ));
        // The Mac's own monospace, with its bold and italic faces, as the
        // page's xterm.js has; everything else falls back to the system's fonts.
        let menlo = match std::fs::read("/System/Library/Fonts/Menlo.ttc") {
            Ok(data) => {
                let mut fonts = egui::FontDefinitions::default();
                for (name, index) in [("menlo", 0u32), ("menlo-bold", 1), ("menlo-italic", 2)] {
                    let mut fd = egui::FontData::from_owned(data.clone());
                    fd.index = index;
                    fonts.font_data.insert(name.to_string(), Arc::new(fd));
                    fonts
                        .families
                        .insert(egui::FontFamily::Name(name.into()), vec![name.to_string()]);
                }
                if let Some(mono) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                    mono.insert(0, "menlo".to_string());
                }
                cc.egui_ctx.set_fonts(fonts);
                true
            }
            Err(_) => false,
        };
        let font = FontId::monospace(env_f64("SPIKE_FONT").unwrap_or(13.0) as f32);
        // Fonts exist only once the first frame runs: the first `ui` measures
        // the cell and the grid takes its real size then.
        let cell = vec2(8.0, 17.0);
        let cmd = std::env::var("SPIKE_CMD").ok();
        let cwd = std::env::var("SPIKE_CWD").ok().map(PathBuf::from);
        let term = Terminal::spawn(
            &cc.egui_ctx,
            cmd,
            cwd,
            font,
            cell,
            Size { cols: 80, rows: 24 },
            menlo,
        )
        .expect("a PTY");
        cc.egui_ctx.memory_mut(|m| m.request_focus(term.id));
        let out = std::env::var("SPIKE_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| ".".into());
        let _ = std::fs::create_dir_all(&out);
        Self {
            term,
            t0: Instant::now(),
            last: Instant::now(),
            frames: Vec::new(),
            cpu_ms: Vec::new(),
            galleys: Vec::new(),
            exit_at: env_f64("SPIKE_EXIT"),
            shot_at: env_f64("SPIKE_SHOT"),
            resize_at: env_f64("SPIKE_RESIZE"),
            ime_at: env_f64("SPIKE_IME"),
            ime_text: std::env::var("SPIKE_IME_TEXT").unwrap_or_else(|_| "你好，世界".into()),
            ime_stage: 0,
            shot_done: false,
            resize_done: false,
            done: false,
            out,
            label: std::env::var("SPIKE_LABEL").unwrap_or_default(),
            hz: env_f64("SPIKE_HZ").unwrap_or(120.0),
            cell_measured: false,
            animate: std::env::var("SPIKE_ANIMATE")
                .map(|v| v != "0")
                .unwrap_or(false),
        }
    }

    fn write_report(&self) {
        let mut dts: Vec<f32> = self.frames.iter().map(|f| f[1]).collect();
        // The first frame's dt is the startup, not a frame.
        if !dts.is_empty() {
            dts.remove(0);
        }
        dts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let pct = |p: f64| -> f32 {
            if dts.is_empty() {
                0.0
            } else {
                dts[((dts.len() - 1) as f64 * p).round() as usize]
            }
        };
        let mean = |v: &[f32]| {
            if v.is_empty() {
                0.0
            } else {
                v.iter().sum::<f32>() / v.len() as f32
            }
        };
        let interval = 1000.0 / self.hz as f32;
        let late = dts.iter().filter(|d| **d > 1.5 * interval).count();
        let hitches = dts.iter().filter(|d| **d > 50.0).count();
        let paint: Vec<f32> = self.frames.iter().map(|f| f[2]).collect();
        let galleys: Vec<f32> = self.galleys.iter().map(|g| *g as f32).collect();
        let secs = self.t0.elapsed().as_secs_f64();
        let json = format!(
            "{{\n  \"label\": {:?},\n  \"seconds\": {secs:.2},\n  \"frames\": {},\n  \"frames_per_second\": {:.1},\n  \"frame_ms\": {{\"mean\": {:.2}, \"p50\": {:.2}, \"p95\": {:.2}, \"p99\": {:.2}, \"max\": {:.2}}},\n  \"late_frames\": {late},\n  \"hitches\": {hitches},\n  \"display_hz\": {},\n  \"update_cpu_ms_mean\": {:.3},\n  \"grid_paint_ms_mean\": {:.3},\n  \"grid_paint_ms_max\": {:.3},\n  \"galleys_per_frame_mean\": {:.1},\n  \"wakeups\": {},\n  \"cols\": {},\n  \"rows\": {},\n  \"cell\": [{:.2}, {:.2}],\n  \"title\": {:?}\n}}\n",
            self.label,
            dts.len(),
            dts.len() as f64 / secs,
            mean(&dts),
            pct(0.5),
            pct(0.95),
            pct(0.99),
            dts.last().copied().unwrap_or(0.0),
            self.hz,
            mean(&self.cpu_ms),
            mean(&paint),
            paint.iter().cloned().fold(0.0, f32::max),
            mean(&galleys),
            self.term.proxy.wakeups.load(Ordering::Relaxed),
            self.term.size.cols,
            self.term.size.rows,
            self.term.cell.x,
            self.term.cell.y,
            self.term.proxy.title.lock().unwrap().clone(),
        );
        let path = self.out.join("report.json");
        match std::fs::write(&path, &json) {
            Ok(()) => eprintln!("spike: wrote {}\n{json}", path.display()),
            Err(e) => eprintln!("spike: {}: {e}", path.display()),
        }
    }
}

impl eframe::App for Spike {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        if !self.cell_measured {
            self.cell_measured = true;
            let font = self.term.font.clone();
            self.term.cell =
                ctx.fonts_mut(|f| vec2(f.glyph_width(&font, 'M'), f.row_height(&font)));
            eprintln!("spike: cell {:?} for {:?}", self.term.cell, font);
        }
        let now = Instant::now();
        let t = (now - self.t0).as_secs_f64();
        let dt_ms = (now - self.last).as_secs_f32() * 1000.0;
        self.last = now;
        if let Some(c) = frame.info().cpu_usage {
            self.cpu_ms.push(c * 1000.0);
        }

        if let Some(at) = self.resize_at
            && t >= at
            && !self.resize_done
        {
            self.resize_done = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(1000.0, 640.0)));
        }
        if let Some(at) = self.ime_at {
            match self.ime_stage {
                0 if t >= at => {
                    self.term.preedit = Some("ni hao shi jie".into());
                    self.ime_stage = 1;
                }
                1 if t >= at + 1.5 => {
                    self.term.preedit = None;
                    self.term.write(self.ime_text.clone().into_bytes());
                    self.ime_stage = 2;
                }
                _ => {}
            }
        }
        if let Some(at) = self.shot_at
            && t >= at
            && !self.shot_done
        {
            self.shot_done = true;
            let path = self.out.join("shot.png");
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |img| save_png(&path, &img)),
            ));
        }
        if let Some(at) = self.exit_at
            && t >= at
            && !self.done
        {
            self.done = true;
            self.write_report();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.term.proxy.exited.load(Ordering::Relaxed) && !self.done {
            self.done = true;
            self.write_report();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        let wakeups = self.term.proxy.wakeups.load(Ordering::Relaxed);
        let title = self.term.proxy.title.lock().unwrap().clone();
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{dt_ms:5.1} ms since the last frame · {} frames · {wakeups} wakeups · {}×{} · {title}",
                    self.frames.len(),
                    self.term.size.cols,
                    self.term.size.rows
                ));
            });
        });
        egui::Panel::left("history").show(ui, |ui| {
            ui.heading("history");
            for i in 0..28 {
                ui.label(format!("● entry {}: a batch of ops", 28 - i));
            }
        });
        egui::Panel::bottom("timeline").show(ui, |ui| {
            ui.label("timeline");
            let (r, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
            let p = ui.painter();
            for i in 0..6 {
                let y = r.top() + 8.0 + i as f32 * 12.0;
                p.line_segment(
                    [pos2(r.left(), y), pos2(r.right(), y)],
                    Stroke::new(1.0, Color32::from_gray(60)),
                );
            }
            let x = r.left() + (t as f32 * 20.0) % r.width().max(1.0);
            p.line_segment(
                [pos2(x, r.top()), pos2(x, r.bottom())],
                Stroke::new(1.0, Color32::LIGHT_RED),
            );
        });
        let blink_on = ((t * 2.0) as u64).is_multiple_of(2);
        let (galleys, paint) = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.term.show(ui, blink_on))
            .inner;
        self.frames
            .push([t as f32, dt_ms, paint.as_secs_f32() * 1000.0]);
        self.galleys.push(galleys);
        if self.animate {
            // The window redraws at the display's rate, as it does while a
            // world plays: a frame that is late is the terminal's fault. Paced
            // here too, so a locked Mac (no vsync) measures the same cadence.
            ctx.request_repaint_after(Duration::from_secs_f64(1.0 / self.hz));
        } else {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }
}

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_title("spike: a terminal in egui"),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "spike-egui-terminal",
        options,
        Box::new(|cc| Ok(Box::new(Spike::new(cc)))),
    )
}
