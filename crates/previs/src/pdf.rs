//! A screenplay PDF as script pages: the text layer read back into
//! elements by where it sits on the page, then Fountain → the one
//! parser. Deterministic — same bytes in, same Fountain out.
//!
//! A screenplay page is a fixed layout — Courier 12, ten characters an
//! inch, each element at its own left margin — which is what makes a
//! PDF readable without a model: action and scene headings start at the
//! page's left margin, dialogue an inch further in, parentheticals half
//! an inch past that, character cues further still, transitions against
//! the right margin. The columns are learned per document (the two most
//! common left edges are action and dialogue; the cue column is the
//! uppercase lines' own), since every tool sets them a few points apart.
//!
//! The reading, line by line in page order:
//!
//! - **Text** comes from `pdf-extract` as positioned glyphs; glyphs on
//!   one baseline make a line, a gap of a few characters inside a line
//!   makes a new run (a scene number beside its heading), and a run
//!   drawn twice (some generators do) counts once.
//! - **The title page** is a first page with no scene heading: its first
//!   line is the title, a `… by` line and the one after it the credit and
//!   the author; the rest (contact, date) is left.
//! - **Furniture** goes: page numbers, `(CONTINUED)` / `CONTINUED:`,
//!   `(MORE)`. A scene number in either margin rides its heading as the
//!   shooting script's `#12A#`.
//! - **At the action margin**: a line that reads `INT.`/`EXT.` is the
//!   scene; anything else is action, `!`-forced when it would read as
//!   something else. An uppercase line in the right third is a
//!   transition. A line at the cue column that is uppercase opens a
//!   dialogue block; the block's lines follow at the dialogue and
//!   parenthetical columns until a blank line or another element. A line
//!   centered on the page, at no column, is `>centered<`.
//! - **Blank lines** are the page's own: a gap of more than a line and a
//!   half between baselines, or a page break.
//! - **No columns** (a Fountain or plain-text file printed as it is, all
//!   at one margin): the lines are handed to the Fountain parser as they
//!   are, blank lines by the same gaps.
//!
//! The page's lines are the script's lines, so the page-time estimate (a
//! second a line of action) reads a PDF the way a screenplay page is
//! read. What it doesn't do: a scanned script (no text layer) is refused
//! with its own words — no OCR; dual dialogue set side by side comes out
//! as the two columns' words interleaved; styling, revision marks and
//! page headers don't reach the draft.

use std::fmt;

use crate::fdx::{force_action, is_credit, is_scene_number};
use crate::fountain;

/// A point is a seventy-second of an inch.
const INCH: f64 = 72.0;

/// Why a PDF didn't read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfError {
    /// Not a PDF, or one too damaged to open (the parser's reason).
    NotPdf(String),
    /// Pages, but no text on any of them: a scan.
    NoTextLayer,
    /// More pages than the caller reads.
    TooManyPages {
        /// The pages the file has.
        pages: usize,
        /// The caller's ceiling.
        limit: usize,
    },
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PdfError::NotPdf(reason) => write!(f, "the PDF doesn't open ({reason})"),
            PdfError::NoTextLayer => write!(
                f,
                "this PDF is a picture of a script — no text layer to read; export the script's text from the app that wrote it, or OCR it first"
            ),
            PdfError::TooManyPages { pages, limit } => {
                write!(f, "{pages} pages — this reads up to {limit}")
            }
        }
    }
}

impl std::error::Error for PdfError {}

/// A run of text on one baseline: where it starts and ends, in points
/// from the page's left edge, and its text.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Run {
    pub x: f64,
    pub x_end: f64,
    pub text: String,
}

/// One baseline's text: its distance from the top of the page in
/// points, its font size, and its runs left to right.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Line {
    pub y: f64,
    pub size: f64,
    pub runs: Vec<Run>,
}

impl Line {
    /// The runs' text, joined by single spaces.
    fn text(&self) -> String {
        self.runs
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// One page: its size in points and its lines, top to bottom.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Page {
    pub width: f64,
    pub height: f64,
    pub lines: Vec<Line>,
}

/// Translate a screenplay PDF to Fountain. `max_pages` is the caller's
/// ceiling (the localgpt.md page's guard; the command line has none),
/// checked before any page is read.
pub fn to_fountain(bytes: &[u8], max_pages: Option<usize>) -> Result<String, PdfError> {
    let pages = extract(bytes, max_pages)?;
    if pages.iter().all(|p| p.lines.is_empty()) {
        return Err(PdfError::NoTextLayer);
    }
    Ok(layout(&pages))
}

// ---------------------------------------------------------------- extract

/// A glyph as `pdf-extract` reports it, in points: left edge, right edge,
/// baseline from the top, the font size on the page, the text.
struct Glyph {
    x: f64,
    x_end: f64,
    y: f64,
    size: f64,
    text: String,
}

/// The `OutputDev` that collects one page's glyphs.
#[derive(Default)]
struct Glyphs {
    llx: f64,
    lly: f64,
    width: f64,
    height: f64,
    glyphs: Vec<Glyph>,
}

impl pdf_extract::OutputDev for Glyphs {
    fn begin_page(
        &mut self,
        _page_num: u32,
        media_box: &pdf_extract::MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), pdf_extract::OutputError> {
        self.llx = media_box.llx;
        self.lly = media_box.lly;
        self.width = media_box.urx - media_box.llx;
        self.height = media_box.ury - media_box.lly;
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &pdf_extract::Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        text: &str,
    ) -> Result<(), pdf_extract::OutputError> {
        // A space glyph is kept: usually the gap says the same, but a
        // font whose widths the reader doesn't know places every glyph
        // at one point, and then the glyph is all there is.
        // The text rendering matrix scales the font: its area scale is
        // the size on the page.
        let scale = (trm.m11 * trm.m22 - trm.m12 * trm.m21).abs().sqrt();
        let mut size = font_size * scale;
        if !size.is_finite() || size <= 0.0 {
            size = 12.0;
        }
        let x = trm.m31 - self.llx;
        let y = self.lly + self.height - trm.m32;
        if !x.is_finite() || !y.is_finite() {
            return Ok(());
        }
        self.glyphs.push(Glyph {
            x,
            x_end: x + width * size,
            y,
            size,
            text: text.to_string(),
        });
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), pdf_extract::OutputError> {
        Ok(())
    }
}

impl Glyphs {
    /// The page: glyphs sorted top to bottom and left to right, drawn-
    /// twice glyphs dropped, then grouped into lines and runs.
    fn into_page(mut self) -> Page {
        self.glyphs.sort_by(|a, b| {
            a.y.partial_cmp(&b.y)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
        });
        let mut lines: Vec<Line> = Vec::new();
        let mut last: Option<(f64, f64, String)> = None;
        for g in self.glyphs {
            let space = g.text.trim().is_empty();
            // The same glyph at the same place twice: some generators
            // draw a line over itself. Once is enough — when the glyph
            // has a width, that is; a font whose widths the reader
            // doesn't know (some Type 3 fonts) reports every glyph of a
            // string at one place, and `gg` is two of them.
            if !space
                && g.x_end > g.x
                && let Some((lx, ly, ref lt)) = last
                && (lx - g.x).abs() < 0.5
                && (ly - g.y).abs() < 0.5
                && *lt == g.text
            {
                continue;
            }
            if !space {
                last = Some((g.x, g.y, g.text.clone()));
            }
            let new_line = match lines.last() {
                Some(line) => (line.y - g.y).abs() > line.size * 0.5,
                None => true,
            };
            if new_line {
                if space {
                    continue;
                }
                lines.push(Line {
                    y: g.y,
                    size: g.size,
                    runs: Vec::new(),
                });
            }
            let line = lines.last_mut().expect("a line was just pushed");
            match line.runs.last_mut() {
                // A gap of a few characters is a new run (a scene number
                // beside its heading); a smaller one is a space — as is
                // a space glyph, for the fonts that place no glyph.
                Some(run) if g.x - run.x_end <= g.size * 2.5 => {
                    if (space || g.x - run.x_end > g.size * 0.25) && !run.text.ends_with(' ') {
                        run.text.push(' ');
                    }
                    if !space {
                        run.text.push_str(&g.text);
                    }
                    run.x_end = g.x_end.max(run.x_end);
                }
                _ if space => {}
                _ => line.runs.push(Run {
                    x: g.x,
                    x_end: g.x_end,
                    text: g.text,
                }),
            }
        }
        for line in &mut lines {
            for run in &mut line.runs {
                run.text = run.text.trim().to_string();
            }
            line.runs.retain(|r| !r.text.is_empty());
        }
        lines.retain(|l| !l.runs.is_empty());
        Page {
            width: self.width,
            height: self.height,
            lines,
        }
    }
}

/// Open the PDF and read every page's text with its positions.
fn extract(bytes: &[u8], max_pages: Option<usize>) -> Result<Vec<Page>, PdfError> {
    let doc =
        pdf_extract::Document::load_mem(bytes).map_err(|e| PdfError::NotPdf(e.to_string()))?;
    if doc.is_encrypted() {
        return Err(PdfError::NotPdf("it is encrypted".to_string()));
    }
    let page_ids = doc.get_pages();
    if let Some(limit) = max_pages
        && page_ids.len() > limit
    {
        return Err(PdfError::TooManyPages {
            pages: page_ids.len(),
            limit,
        });
    }
    let mut pages = Vec::with_capacity(page_ids.len());
    for (number, id) in page_ids {
        // pdf-extract takes a page's dictionary, its MediaBox and its
        // content stream on trust; a page without them is skipped here
        // rather than falling with it, and whatever else a damaged page
        // throws is caught where panics unwind (natively — on the web the
        // page reloads the module).
        if !page_is_sound(&doc, id) {
            continue;
        }
        let mut glyphs = Glyphs::default();
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pdf_extract::output_doc_page(&doc, &mut glyphs, number)
        }));
        if !matches!(read, Ok(Ok(()))) {
            continue;
        }
        pages.push(glyphs.into_page());
    }
    Ok(pages)
}

/// Whether a page has what `pdf-extract` reads without checking: a
/// dictionary, a MediaBox of four numbers (its own or inherited), and
/// content that decodes.
fn page_is_sound(doc: &pdf_extract::Document, id: pdf_extract::ObjectId) -> bool {
    let Ok(dict) = doc.get_object(id).and_then(|o| o.as_dict()) else {
        return false;
    };
    let mut media_box = None;
    let mut node = Some(dict);
    let mut hops = 0;
    while let Some(d) = node
        && media_box.is_none()
        && hops < 64
    {
        hops += 1;
        if let Ok(array) = d.get(b"MediaBox").and_then(|o| o.as_array()) {
            media_box = Some(array.len() == 4 && array.iter().all(|n| n.as_float().is_ok()));
            break;
        }
        node = d
            .get(b"Parent")
            .and_then(|o| o.as_reference())
            .and_then(|r| doc.get_object(r))
            .and_then(|o| o.as_dict())
            .ok();
    }
    if media_box != Some(true) {
        return false;
    }
    let Ok(content) = doc.get_page_content(id) else {
        return false;
    };
    pdf_extract::content::Content::decode(&content).is_ok()
}

// ----------------------------------------------------------------- layout

/// The page's columns, in points from the left edge.
#[derive(Debug, Clone, PartialEq)]
struct Columns {
    /// Action and scene headings.
    action: f64,
    /// Dialogue.
    dialogue: f64,
    /// Character cues.
    cue: f64,
    /// The page's width.
    width: f64,
}

impl Columns {
    /// Learn the columns from the body's lines: the scene headings'
    /// left edge is action (failing any, the leftmost edge a few lines
    /// share — in a talky script the most common edge is dialogue's);
    /// the most common edge half an inch to two inches right of it is
    /// dialogue; the cue column is where the uppercase lines right of
    /// dialogue most often start. No dialogue column, no columns.
    fn learn(pages: &[Page]) -> Option<Columns> {
        let width = pages.first().map(|p| p.width).unwrap_or(8.5 * INCH);
        let mut edges: Vec<(f64, bool)> = Vec::new();
        let mut headings: Vec<f64> = Vec::new();
        for page in pages {
            for line in &page.lines {
                // The line's text starts after a margin scene number.
                let run = line
                    .runs
                    .iter()
                    .find(|r| line.runs.len() < 2 || !is_scene_number(&r.text))
                    .unwrap_or(&line.runs[0]);
                edges.push((run.x, fountain::character_cue(&run.text).is_some()));
                if fountain::is_scene_heading(&run.text) {
                    headings.push(run.x);
                }
            }
        }
        let action = mode(headings.iter().copied()).or_else(|| {
            let floor = (edges.len() / 20).max(3);
            let mut bins: Vec<(i64, usize)> = Vec::new();
            for (x, _) in &edges {
                let bin = (x / (0.05 * INCH)).round() as i64;
                match bins.iter_mut().find(|(b, _)| *b == bin) {
                    Some((_, n)) => *n += 1,
                    None => bins.push((bin, 1)),
                }
            }
            bins.into_iter()
                .filter(|(_, n)| *n >= floor)
                .map(|(bin, _)| bin)
                .min()
                .map(|bin| bin as f64 * 0.05 * INCH)
        })?;
        let dialogue = mode(
            edges
                .iter()
                .map(|(x, _)| *x)
                .filter(|x| *x >= action + 0.5 * INCH && *x <= action + 2.0 * INCH),
        )?;
        let dialogue_count = edges
            .iter()
            .filter(|(x, _)| (x - dialogue).abs() < 0.15 * INCH)
            .count();
        if dialogue_count < 3 || dialogue_count * 20 < edges.len() {
            return None;
        }
        let cue = mode(
            edges
                .iter()
                .filter(|(x, upper)| {
                    *upper && *x >= dialogue + 0.5 * INCH && *x <= dialogue + 2.0 * INCH
                })
                .map(|(x, _)| *x),
        )
        .unwrap_or(dialogue + INCH);
        Some(Columns {
            action,
            dialogue,
            cue,
            width,
        })
    }
}

/// The most common value, to a twentieth of an inch.
fn mode(xs: impl Iterator<Item = f64>) -> Option<f64> {
    let mut bins: Vec<(i64, usize)> = Vec::new();
    for x in xs {
        let bin = (x / (0.05 * INCH)).round() as i64;
        match bins.iter_mut().find(|(b, _)| *b == bin) {
            Some((_, n)) => *n += 1,
            None => bins.push((bin, 1)),
        }
    }
    bins.into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(bin, _)| bin as f64 * 0.05 * INCH)
}

/// Page numbers, `(CONTINUED)`, `CONTINUED:` (with or without its scene
/// number), `(MORE)`.
fn is_furniture(text: &str) -> bool {
    let t = text.trim();
    let digits = t.trim_matches(|c: char| matches!(c, '.' | '-' | ' ' | '(' | ')'));
    if !digits.is_empty() && digits.len() <= 4 && digits.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    let upper = t.to_ascii_uppercase();
    if matches!(
        upper.as_str(),
        "(CONTINUED)" | "CONTINUED:" | "CONTINUED" | "(MORE)" | "MORE:"
    ) {
        return true;
    }
    let mut words = upper.split_whitespace();
    match (words.next(), words.next()) {
        (Some(first), Some("CONTINUED:" | "CONTINUED" | "(CONTINUED)")) => is_scene_number(first),
        (Some("CONTINUED:"), Some(rest)) => rest.starts_with('('),
        _ => false,
    }
}

/// The first page is a title page when it has no scene heading and the
/// script has more pages: its first line is the title, a `… by` line and
/// the next the credit and the author. Returns the entries and the index
/// of the first body page.
fn title_page(pages: &[Page]) -> (Vec<(String, String)>, usize) {
    let Some(first) = pages.first() else {
        return (Vec::new(), 0);
    };
    let has_heading = first.lines.iter().any(|l| {
        l.runs
            .iter()
            .any(|r| fountain::is_scene_heading(&r.text) || is_furniture(&r.text) && false)
    });
    if pages.len() < 2 || has_heading || first.lines.is_empty() {
        return (Vec::new(), 0);
    }
    let texts: Vec<String> = first.lines.iter().map(Line::text).collect();
    let mut out = vec![("Title".to_string(), texts[0].clone())];
    if let Some(at) = texts.iter().skip(1).position(|l| is_credit(l)) {
        let at = at + 1;
        out.push(("Credit".to_string(), texts[at].clone()));
        if let Some(author) = texts.get(at + 1) {
            out.push(("Author".to_string(), author.clone()));
        }
    }
    (out, 1)
}

/// What the last emitted line was, for the blank lines between elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Last {
    Nothing,
    Action,
    Block,
    Other,
}

/// Write the pages out as Fountain.
fn layout(pages: &[Page]) -> String {
    let (title, first_body) = title_page(pages);
    let body_pages = &pages[first_body.min(pages.len())..];
    let mut body: Vec<String> = Vec::new();
    match Columns::learn(body_pages) {
        Some(columns) => classify(body_pages, &columns, &mut body),
        None => as_written(body_pages, &mut body),
    }
    while body.last().is_some_and(|l| l.is_empty()) {
        body.pop();
    }
    let mut out = String::new();
    for (key, value) in &title {
        out.push_str(&format!("{key}: {value}\n"));
    }
    if !title.is_empty() && !body.is_empty() {
        out.push('\n');
    }
    for line in &body {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Push a blank line unless the body is empty or already ends in one.
fn blank(body: &mut Vec<String>) {
    if body.last().is_some_and(|l| !l.is_empty()) {
        body.push(String::new());
    }
}

/// Whether `line` sits a blank line or more below the previous one (or
/// on a new page).
fn gap_above(prev: Option<(usize, f64, f64)>, page: usize, line: &Line) -> bool {
    match prev {
        None => false,
        Some((p, y, size)) => p != page || line.y - y > size.max(line.size) * 1.6,
    }
}

/// No columns: the lines as written, for the Fountain parser.
fn as_written(pages: &[Page], body: &mut Vec<String>) {
    let mut prev: Option<(usize, f64, f64)> = None;
    for (p, page) in pages.iter().enumerate() {
        for line in &page.lines {
            let text = line.text();
            if is_furniture(&text) {
                continue;
            }
            if gap_above(prev, p, line) {
                blank(body);
            }
            body.push(text);
            prev = Some((p, line.y, line.size));
        }
    }
}

/// With columns: every line classified by its left edge and its text.
fn classify(pages: &[Page], c: &Columns, body: &mut Vec<String>) {
    let near = |x: f64, column: f64| (x - column).abs() <= 0.3 * INCH;
    let mut prev: Option<(usize, f64, f64)> = None;
    let mut last = Last::Nothing;
    for (p, page) in pages.iter().enumerate() {
        for (i, line) in page.lines.iter().enumerate() {
            // A cue needs its lines: the next line at the dialogue or
            // parenthetical column, or opening a parenthesis. That is
            // what tells a cue from a line centered on the page.
            let follows_block = page.lines.get(i + 1).is_some_and(|next| {
                let nx = next.runs[0].x;
                (nx >= c.dialogue - 0.3 * INCH && nx < c.cue - 0.3 * INCH)
                    || next.runs[0].text.starts_with('(')
            });
            let mut runs: Vec<&Run> = line.runs.iter().collect();
            runs.retain(|r| !is_furniture(&r.text));
            // A scene number in either margin: off the line, onto the
            // heading if that is what the line is.
            let mut number: Option<String> = None;
            if runs.len() >= 2 {
                if runs[0].x < c.action - 0.3 * INCH && is_scene_number(&runs[0].text) {
                    number = Some(runs[0].text.clone());
                    runs.remove(0);
                }
                if let Some(end) = runs.last()
                    && runs.len() >= 2
                    && end.x >= c.width - 1.5 * INCH
                    && is_scene_number(&end.text)
                {
                    number.get_or_insert_with(|| end.text.clone());
                    runs.pop();
                }
            }
            let Some(first) = runs.first() else { continue };
            let x = first.x;
            let text = runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let x_end = runs.last().map(|r| r.x_end).unwrap_or(x);
            let gap = gap_above(prev, p, line);
            prev = Some((p, line.y, line.size));
            if gap && last == Last::Block {
                last = Last::Other;
            }

            let upper = fountain::character_cue(&text).is_some()
                || (text.chars().any(|ch| ch.is_alphabetic())
                    && !text.chars().any(|ch| ch.is_lowercase()));
            if near(x, c.action) || x < c.action {
                if fountain::is_scene_heading(&text) {
                    let mut heading = text.clone();
                    if let Some(n) = number {
                        heading.push_str(&format!(" #{n}#"));
                    }
                    blank(body);
                    body.push(heading);
                    blank(body);
                    last = Last::Other;
                } else if fountain::is_transition(&text) {
                    blank(body);
                    body.push(text);
                    blank(body);
                    last = Last::Other;
                } else {
                    if gap || last != Last::Action {
                        blank(body);
                    }
                    body.push(force_action(text));
                    last = Last::Action;
                }
            } else if x >= c.action + 3.5 * INCH && upper {
                let line = if fountain::is_transition(&text) {
                    text
                } else {
                    format!("> {text}")
                };
                blank(body);
                body.push(line);
                blank(body);
                last = Last::Other;
            } else if x >= c.dialogue - 0.3 * INCH {
                if upper && x >= c.cue - 0.5 * INCH && !text.starts_with('(') && follows_block {
                    let mut cue = text;
                    if fountain::character_cue(&cue).is_none() {
                        cue.insert(0, '@');
                    }
                    blank(body);
                    body.push(cue);
                    last = Last::Block;
                } else if last == Last::Block {
                    body.push(text);
                } else if ((x + x_end) / 2.0 - c.width / 2.0).abs() <= 0.35 * INCH
                    && !near(x, c.dialogue)
                {
                    blank(body);
                    body.push(format!("> {text} <"));
                    blank(body);
                    last = Last::Other;
                } else {
                    if gap || last != Last::Action {
                        blank(body);
                    }
                    body.push(force_action(text));
                    last = Last::Action;
                }
            } else {
                if gap || last != Last::Action {
                    blank(body);
                }
                body.push(force_action(text));
                last = Last::Action;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page of Letter size from `(x inches, y points, text)` lines.
    fn page(lines: &[(f64, f64, &str)]) -> Page {
        Page {
            width: 8.5 * INCH,
            height: 11.0 * INCH,
            lines: lines
                .iter()
                .map(|(x, y, text)| Line {
                    y: *y,
                    size: 12.0,
                    runs: vec![Run {
                        x: x * INCH,
                        x_end: x * INCH + text.len() as f64 * 7.2,
                        text: text.to_string(),
                    }],
                })
                .collect(),
        }
    }

    /// The columns a drafted page has: action at 1.5", dialogue at 2.5",
    /// parentheticals at 3.0", cues at 3.5".
    fn scene_page() -> Page {
        page(&[
            (1.5, 72.0, "INT. KITCHEN - DAY"),
            (
                1.5,
                96.0,
                "Maya fries an egg. The radio mutters the shipping",
            ),
            (1.5, 108.0, "forecast to nobody."),
            (3.5, 132.0, "MAYA"),
            (2.5, 144.0, "You're late. The eggs are cold."),
            (3.5, 168.0, "KAI"),
            (3.0, 180.0, "(yawning)"),
            (2.5, 192.0, "The eggs are always cold."),
            (2.5, 204.0, "Always."),
            (1.5, 228.0, "He sits."),
            (6.5, 252.0, "CUT TO:"),
        ])
    }

    #[test]
    fn a_page_with_columns_reads_as_its_elements() {
        let f = layout(&[scene_page()]);
        assert_eq!(
            f,
            "INT. KITCHEN - DAY\n\nMaya fries an egg. The radio mutters the shipping\nforecast to nobody.\n\nMAYA\nYou're late. The eggs are cold.\n\nKAI\n(yawning)\nThe eggs are always cold.\nAlways.\n\nHe sits.\n\nCUT TO:\n"
        );
        // And the parser agrees: one scene, two speakers, the action's
        // lines counted as the page has them.
        let script = fountain::parse(&f);
        let scenes = crate::stage::scenes(&script);
        assert_eq!(scenes.len(), 1);
        let (speakers, total) = crate::stage::speakers(&scenes[0]);
        let names: Vec<&str> = speakers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["MAYA", "KAI"]);
        assert!(total > 0.0);
    }

    #[test]
    fn margin_numbers_furniture_and_the_title_page() {
        let title = page(&[
            (3.3, 300.0, "COLD EGGS"),
            (3.7, 336.0, "written by"),
            (3.6, 348.0, "A. Previs"),
            (1.5, 700.0, "a.previs@example.com"),
        ]);
        let mut numbered = page(&[
            (6.9, 36.0, "2."),
            (1.5, 60.0, "CONTINUED:"),
            (1.5, 84.0, "INT. HALL - NIGHT"),
            (1.5, 108.0, "Dark."),
            (3.5, 132.0, "MAYA"),
            (2.5, 144.0, "Who's there?"),
            (2.5, 156.0, "Anyone home?"),
            (3.0, 168.0, "(MORE)"),
            (6.5, 700.0, "(CONTINUED)"),
            (3.5, 192.0, "MAYA (CONT'D)"),
            (2.5, 204.0, "Anyone?"),
        ]);
        // The heading's number in both margins, as separate runs.
        let heading = &mut numbered.lines[2];
        let x_end = heading.runs[0].x_end;
        heading.runs.insert(
            0,
            Run {
                x: 1.0 * INCH,
                x_end: 1.1 * INCH,
                text: "12A".to_string(),
            },
        );
        heading.runs.push(Run {
            x: 7.4 * INCH,
            x_end: 7.6 * INCH,
            text: "12A".to_string(),
        });
        let _ = x_end;
        let f = layout(&[title, numbered]);
        assert_eq!(
            f,
            "Title: COLD EGGS\nCredit: written by\nAuthor: A. Previs\n\nINT. HALL - NIGHT #12A#\n\nDark.\n\nMAYA\nWho's there?\nAnyone home?\n\nMAYA (CONT'D)\nAnyone?\n"
        );
    }

    #[test]
    fn transitions_centered_text_and_odd_action_lines() {
        let f = layout(&[page(&[
            (1.5, 72.0, "EXT. SEA - DAY"),
            // (A line at the action margin that opens `INT.` is a
            // heading — a page can't say otherwise, so neither does this.)
            (1.5, 96.0, "= not a synopsis, she says."),
            (1.5, 120.0, "ALL CAPS ACTION"),
            (1.5, 132.0, "continues here."),
            (3.6, 156.0, "THE SAME SEA"),
            (6.4, 180.0, "FADE OUT."),
            (3.5, 204.0, "MAYA & KAI"),
            (2.5, 216.0, "Together."),
            (2.5, 228.0, "Always."),
            (2.5, 240.0, "Forever."),
        ])]);
        assert_eq!(
            f,
            "EXT. SEA - DAY\n\n!= not a synopsis, she says.\n\n!ALL CAPS ACTION\ncontinues here.\n\n> THE SAME SEA <\n\n> FADE OUT.\n\n@MAYA & KAI\nTogether.\nAlways.\nForever.\n"
        );
    }

    #[test]
    fn a_page_break_inside_a_block_keeps_the_lines() {
        let one = page(&[
            (1.5, 72.0, "INT. A - DAY"),
            (3.5, 96.0, "MAYA"),
            (2.5, 108.0, "First page."),
        ]);
        let two = page(&[(2.5, 72.0, "Second page."), (1.5, 96.0, "Action.")]);
        // A new page is a gap, so the block closes: the orphan line is
        // action, which keeps the parser honest about what it saw.
        assert_eq!(
            layout(&[one, two]),
            "INT. A - DAY\n\nMAYA\nFirst page.\n\nSecond page.\n\nAction.\n"
        );
    }

    #[test]
    fn one_column_is_handed_over_as_written() {
        let f = layout(&[page(&[
            (0.8, 60.0, "Title: Cold Eggs"),
            (0.8, 72.0, "Author: A. Previs"),
            (0.8, 96.0, "INT. KITCHEN - DAY"),
            (0.8, 120.0, "Maya fries an egg."),
            (0.8, 144.0, "MAYA"),
            (0.8, 156.0, "You're late."),
            (0.8, 180.0, "2."),
        ])]);
        assert_eq!(
            f,
            "Title: Cold Eggs\nAuthor: A. Previs\n\nINT. KITCHEN - DAY\n\nMaya fries an egg.\n\nMAYA\nYou're late.\n"
        );
    }

    #[test]
    fn no_pages_no_text() {
        assert_eq!(layout(&[]), "");
        assert_eq!(
            to_fountain(b"not a pdf at all", None).unwrap_err(),
            PdfError::NotPdf(match pdf_extract::Document::load_mem(b"not a pdf at all") {
                Err(e) => e.to_string(),
                Ok(_) => unreachable!(),
            })
        );
    }

    #[test]
    fn furniture_table() {
        for line in [
            "2.",
            "12",
            "- 3 -",
            "(CONTINUED)",
            "CONTINUED:",
            "12 CONTINUED:",
            "12A CONTINUED: (2)",
            "(MORE)",
        ] {
            assert!(is_furniture(line), "{line}");
        }
        for line in ["12 men.", "CONTINUED, she said.", "MORE EGGS", "1917"] {
            assert_eq!(is_furniture(line), line == "1917", "{line}");
        }
    }
}
