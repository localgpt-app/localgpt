//! Test 3 of `docs/rfcs/authoring/all-rust.md`: egui in a phone's browser,
//! before the phone's page is rebuilt from the shell's panels (step F).
//!
//! What it asks of a phone in a hand, each a thing the page does today:
//! the keyboard comes up for a text field and types into it; an input
//! method composes (pinyin to 中文) and its candidates commit; dictation
//! writes into the field; a terminal's grid draws and answers the key bar;
//! a long list scrolls under a finger; the page turns with the phone; and
//! Chinese draws at all, which in a browser needs a font egui is handed —
//! the system's are out of its reach. The input events egui sees are listed
//! in a view of their own, for the report.
//!
//! The terminal is vt100's grid, fed by a pretend shell: the app's own
//! emulator, `alacritty_terminal`, does not build for the browser (its
//! terminal device and `polling` have no wasm32), so step F streams the
//! app's grids or parses the bytes here; this draws a grid either way.
//!
//! `src/bin/serve.rs` serves it on the network, with the font.

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::collections::VecDeque;
use std::num::NonZeroU16;
use std::rc::Rc;
use std::sync::Arc;

use eframe::egui::{self, Color32, FontId, Rect, RichText, Sense, Vec2, vec2};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

/// The page's start: eframe on the canvas the server's page holds.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id("phone"))
            .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("the page's canvas");
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(Phone::new(&cc.egui_ctx)))),
            )
            .await;
        if let Err(e) = started {
            web_sys::console::error_1(&e);
        }
    });
}

const BACKGROUND: Color32 = Color32::from_rgb(0x0f, 0x12, 0x18);
const INK: Color32 = Color32::from_rgb(0xdd, 0xe1, 0xe8);
const ACCENT: Color32 = Color32::from_rgb(0x6b, 0x9c, 0xff);
/// The pretend shell's prompt, coloured as a shell's is.
const PROMPT: &str = "\x1b[32mphone\x1b[0m:\x1b[34m~\x1b[0m$ ";
/// The key bar the page has under its terminal: a label, and what it sends.
const KEYS: [(&str, &[u8]); 9] = [
    ("esc", b"\x1b"),
    ("⇥", b"\t"),
    ("↑", b"\x1b[A"),
    ("↓", b"\x1b[B"),
    ("←", b"\x1b[D"),
    ("→", b"\x1b[C"),
    ("^C", b"\x03"),
    ("⏎", b"\r"),
    ("clear", b"\x0c"),
];

#[derive(Clone, Copy, PartialEq)]
enum View {
    Terminal,
    Scroll,
    Events,
}

enum Font {
    Loading,
    Arrived(Vec<u8>),
    Installed(usize),
    Failed(String),
}

struct Phone {
    term: vt100::Parser,
    prompt: String,
    view: View,
    /// The input events egui saw, newest first.
    seen: VecDeque<String>,
    font: Rc<RefCell<Font>>,
    lines: Vec<String>,
}

impl Phone {
    fn new(ctx: &egui::Context) -> Self {
        let font = Rc::new(RefCell::new(Font::Loading));
        {
            let (font, ctx) = (font.clone(), ctx.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let got = fetch("cjk.ttc").await;
                *font.borrow_mut() = match got {
                    Ok(bytes) => Font::Arrived(bytes),
                    Err(e) => Font::Failed(format!("{e:?}")),
                };
                ctx.request_repaint();
            });
        }
        let size = |n| NonZeroU16::new(n).expect("not zero");
        let mut term = vt100::Parser::new(size(24), size(60), 0);
        term.process(b"\x1b[1mTest 3\x1b[0m: a terminal's grid in egui, from vt100.\r\n");
        term.process(b"Write or dictate below and Send; the keys go here too.\r\n\r\n");
        term.process(PROMPT.as_bytes());
        let lines = (1..=3000)
            .map(|n| {
                format!(
                    "{n:>4} · a line to scroll past · 第{n}行 · {}",
                    "▮".repeat(n % 9)
                )
            })
            .collect();
        Self {
            term,
            prompt: String::new(),
            view: View::Terminal,
            seen: VecDeque::new(),
            font,
            lines,
        }
    }

    /// The font, once it has come: egui's own faces, then it for what they
    /// lack.
    fn take_font(&mut self, ctx: &egui::Context) {
        let mut font = self.font.borrow_mut();
        if let Font::Arrived(bytes) = &mut *font {
            let bytes = std::mem::take(bytes);
            let size = bytes.len();
            let mut fonts = egui::FontDefinitions::default();
            let mut data = egui::FontData::from_owned(bytes);
            data.index = 0;
            fonts.font_data.insert("cjk".into(), Arc::new(data));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".into());
            }
            ctx.set_fonts(fonts);
            *font = Font::Installed(size);
        }
    }

    fn font_status(&self) -> String {
        match &*self.font.borrow() {
            Font::Loading | Font::Arrived(_) => "CJK font: loading…".into(),
            Font::Installed(size) => format!("CJK font: {:.1} MB", *size as f64 / 1e6),
            Font::Failed(e) => format!("CJK font: failed, {e}"),
        }
    }

    /// What egui was given this frame, for the events view.
    fn note_events(&mut self, ui: &egui::Ui) {
        let events = ui.input(|i| i.events.clone());
        for event in events {
            let line = match event {
                egui::Event::Text(text) => format!("text {text:?}"),
                egui::Event::Ime(ime) => format!("ime {ime:?}"),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => format!("key {key:?} {modifiers:?}"),
                egui::Event::Paste(text) => format!("paste {text:?}"),
                egui::Event::Touch { phase, .. } if phase != egui::TouchPhase::Move => {
                    format!("touch {phase:?}")
                }
                _ => continue,
            };
            self.seen.push_front(line);
            self.seen.truncate(200);
        }
    }

    fn send(&mut self) {
        let text = std::mem::take(&mut self.prompt);
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let reply = format!(
            "{text}\r\n\x1b[36myou said\x1b[0m {} characters: \x1b[33m{text}\x1b[0m\r\n{PROMPT}",
            text.chars().count()
        );
        self.term.process(reply.as_bytes());
    }

    fn terminal(&mut self, ui: &mut egui::Ui) {
        let font = FontId::monospace(13.0);
        let cell = ui
            .ctx()
            .fonts_mut(|f| vec2(f.glyph_width(&font, 'M'), f.row_height(&font)));
        // The key bar and the prompt at the foot, the grid above them.
        let foot = 44.0 + 72.0 + 2.0 * ui.spacing().item_spacing.y;
        let room = ui.available_size() - vec2(0.0, foot);
        let cols = ((room.x / cell.x).floor() as u16).clamp(10, 300);
        let rows = ((room.y / cell.y).floor() as u16).clamp(4, 200);
        let (now_rows, now_cols) = self.term.screen().size();
        if (now_rows.get(), now_cols.get()) != (rows, cols) {
            let size = |n| NonZeroU16::new(n).expect("clamped above zero");
            self.term.set_size(size(rows), size(cols));
        }
        let (rect, _) = ui.allocate_exact_size(room.max(Vec2::ZERO), Sense::click());
        self.grid(ui, rect, &font, cell, rows, cols);

        ui.horizontal_wrapped(|ui| {
            for (label, bytes) in KEYS {
                if ui
                    .add_sized(
                        [44.0, 40.0],
                        egui::Button::new(RichText::new(label).size(15.0)),
                    )
                    .clicked()
                {
                    self.term.process(bytes);
                }
            }
        });
        ui.horizontal(|ui| {
            let send = 64.0;
            ui.add(
                egui::TextEdit::multiline(&mut self.prompt)
                    .desired_rows(3)
                    .desired_width(ui.available_width() - send - ui.spacing().item_spacing.x)
                    .hint_text("Write or dictate a prompt (中文 too)"),
            );
            if ui
                .add_sized(
                    [send, 64.0],
                    egui::Button::new(RichText::new("Send").size(15.0)),
                )
                .clicked()
            {
                self.send();
            }
        });
    }

    /// The screen, a run of cells at a time: the same colours together,
    /// and each wide character (中) on its own at its column, so a fallback
    /// font's widths cannot push the cells after it out of their columns.
    fn grid(&self, ui: &egui::Ui, rect: Rect, font: &FontId, cell: Vec2, rows: u16, cols: u16) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, BACKGROUND);
        let screen = self.term.screen();
        for row in 0..rows {
            let y = rect.min.y + row as f32 * cell.y;
            let mut col = 0;
            while col < cols {
                let Some(first) = screen.cell(row, col) else {
                    break;
                };
                let ink = color(first.fgcolor(), first.bold());
                let start = col;
                let mut text = String::new();
                if first.is_wide() {
                    text.push_str(first.contents());
                    col += 2;
                } else {
                    while col < cols {
                        let Some(next) = screen.cell(row, col) else {
                            break;
                        };
                        if next.is_wide() || color(next.fgcolor(), next.bold()) != ink {
                            break;
                        }
                        if !next.is_wide_continuation() {
                            text.push_str(if next.has_contents() {
                                next.contents()
                            } else {
                                " "
                            });
                        }
                        col += 1;
                    }
                }
                if !text.trim().is_empty() {
                    let galley = ui
                        .ctx()
                        .fonts_mut(|f| f.layout_no_wrap(text, font.clone(), ink));
                    painter.galley(
                        egui::pos2(rect.min.x + start as f32 * cell.x, y),
                        galley,
                        ink,
                    );
                }
            }
        }
        let (row, col) = screen.cursor_position();
        painter.rect_filled(
            Rect::from_min_size(
                rect.min + vec2(col as f32 * cell.x, row as f32 * cell.y),
                cell,
            ),
            0.0,
            ACCENT.gamma_multiply(0.6),
        );
    }

    fn scroll(&self, ui: &mut egui::Ui) {
        let height = ui.text_style_height(&egui::TextStyle::Body);
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            height,
            self.lines.len(),
            |ui, range| {
                for line in &self.lines[range] {
                    ui.label(line);
                }
            },
        );
    }

    fn events(&self, ui: &mut egui::Ui) {
        ui.weak("What egui is given, newest first: text, an input method's steps, keys, touches.");
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for line in &self.seen {
                    ui.monospace(line);
                }
            });
    }
}

/// A cell's colour: the default ink, the sixteen named colours, the 256
/// palette's cube and greys, or its own.
fn color(color: vt100::Color, bold: bool) -> Color32 {
    const NAMED: [Color32; 16] = [
        Color32::from_rgb(0x1d, 0x1f, 0x21),
        Color32::from_rgb(0xcc, 0x66, 0x66),
        Color32::from_rgb(0xb5, 0xbd, 0x68),
        Color32::from_rgb(0xf0, 0xc6, 0x74),
        Color32::from_rgb(0x81, 0xa2, 0xbe),
        Color32::from_rgb(0xb2, 0x94, 0xbb),
        Color32::from_rgb(0x8a, 0xbe, 0xb7),
        Color32::from_rgb(0xc5, 0xc8, 0xc6),
        Color32::from_rgb(0x66, 0x66, 0x66),
        Color32::from_rgb(0xd5, 0x4e, 0x53),
        Color32::from_rgb(0xb9, 0xca, 0x4a),
        Color32::from_rgb(0xe7, 0xc5, 0x47),
        Color32::from_rgb(0x7a, 0xa6, 0xda),
        Color32::from_rgb(0xc3, 0x97, 0xd8),
        Color32::from_rgb(0x70, 0xc0, 0xb1),
        Color32::from_rgb(0xea, 0xea, 0xea),
    ];
    match color {
        vt100::Color::Default if bold => Color32::WHITE,
        vt100::Color::Default => INK,
        vt100::Color::Idx(i) if i < 8 && bold => NAMED[i as usize + 8],
        vt100::Color::Idx(i) if i < 16 => NAMED[i as usize],
        vt100::Color::Idx(i) if i < 232 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(level(i / 36), level((i / 6) % 6), level(i % 6))
        }
        vt100::Color::Idx(i) => {
            let grey = 8 + (i - 232) * 10;
            Color32::from_gray(grey)
        }
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

async fn fetch(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response =
        wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url))
            .await?
            .dyn_into()?;
    if !response.ok() {
        return Err(JsValue::from_str(&format!("HTTP {}", response.status())));
    }
    let buffer = wasm_bindgen_futures::JsFuture::from(response.array_buffer()?).await?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}

impl eframe::App for Phone {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.take_font(&ctx);
        self.note_events(ui);
        let size = ui.max_rect().size();
        // As the page decides: by the width, never the height, which the
        // keyboard takes.
        let narrow = size.x < 700.0;
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("Test 3 · egui on this phone");
                ui.weak(format!(
                    "{:.0}×{:.0} pt at {:.2}×",
                    size.x,
                    size.y,
                    ctx.pixels_per_point()
                ));
                ui.weak(self.font_status());
            });
        });
        if narrow {
            egui::Panel::bottom("views").show(ui, |ui| {
                ui.horizontal(|ui| {
                    let width = (ui.available_width() - 2.0 * ui.spacing().item_spacing.x) / 3.0;
                    for (view, name) in [
                        (View::Terminal, "Terminal"),
                        (View::Scroll, "Scroll"),
                        (View::Events, "Events"),
                    ] {
                        let button = egui::Button::selectable(
                            self.view == view,
                            RichText::new(name).size(15.0),
                        );
                        if ui.add_sized([width, 44.0], button).clicked() {
                            self.view = view;
                        }
                    }
                });
            });
        }
        egui::CentralPanel::default().show(ui, |ui| {
            if narrow {
                match self.view {
                    View::Terminal => self.terminal(ui),
                    View::Scroll => self.scroll(ui),
                    View::Events => self.events(ui),
                }
            } else {
                // A tablet: the terminal beside the list and the events.
                ui.columns(2, |columns| {
                    self.terminal(&mut columns[0]);
                    let half = columns[1].available_height() / 2.0;
                    columns[1].allocate_ui(vec2(columns[1].available_width(), half), |ui| {
                        self.scroll(ui)
                    });
                    self.events(&mut columns[1]);
                });
            }
        });
    }
}
