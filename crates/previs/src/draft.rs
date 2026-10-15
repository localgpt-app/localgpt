//! The whole draft, in memory: script pages in (Fountain, a Final Draft
//! file by [`fdx`]'s mapping, or Markdown by [`markdown`]'s convention);
//! every scene's `.world` package files, the shot list and the printable
//! board out. The CLI writes what this returns to disk and the
//! localgpt.md page hands it to the browser, from one function — so the
//! two can never drift, and the page's draft is the CLI's draft, byte
//! for byte.

use localgpt_world_types as wt;

use crate::board::{self, SceneBoard};
#[cfg(feature = "pdf")]
use crate::pdf;
use crate::shots::{self, ShotRow};
use crate::{fdx, fountain, markdown, package, stage};

/// What the pages are written in. Picked by the file's extension
/// ([`Format::of_name`]), since the text alone can't tell Fountain from
/// Markdown: Fountain is valid inside Markdown and the two differ only
/// in what a `#` heading or a bold name means. A Final Draft file is
/// XML and a PDF is a PDF, which the bytes do say ([`Format::of`],
/// [`Format::of_bytes`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    /// Fountain — `.fountain`, `.txt`, `.spmd`, and anything unnamed.
    #[default]
    Fountain,
    /// Final Draft — `.fdx`: [`fdx`]'s mapping of the typed paragraphs,
    /// translated to Fountain before the parse.
    Fdx,
    /// Markdown — `.md`, `.markdown`, `.mdown`, `.mkd`: [`markdown`]'s
    /// convention, translated to Fountain before the parse.
    Markdown,
    /// A PDF — `.pdf`: the text layer read back into elements by where
    /// it sits on the page (the `pdf` module, behind the `pdf` feature),
    /// translated to Fountain before the parse. Read from the bytes
    /// ([`draft_bytes`]), never from text.
    Pdf,
}

impl Format {
    /// The format a file name implies (the extension, case-insensitive;
    /// a path is fine — only the last segment is read).
    pub fn of_name(name: &str) -> Format {
        let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
        match file.rsplit_once('.') {
            Some((stem, ext)) if !stem.is_empty() => match ext.to_ascii_lowercase().as_str() {
                "md" | "markdown" | "mdown" | "mkd" => Format::Markdown,
                "fdx" => Format::Fdx,
                "pdf" => Format::Pdf,
                _ => Format::Fountain,
            },
            _ => Format::Fountain,
        }
    }

    /// The format of a file by its name, unless the text says otherwise:
    /// XML at the head (`<?xml`, `<FinalDraft`) is a Final Draft file
    /// whatever it was renamed to — and an XML file that isn't one gets
    /// told so, instead of "no scene headings". (A PDF's bytes aren't
    /// text; [`Format::of_bytes`] knows those.)
    pub fn of(name: &str, source: &str) -> Format {
        let head = source
            .strip_prefix('\u{FEFF}')
            .unwrap_or(source)
            .trim_start();
        if head.starts_with("<?xml") || head.starts_with("<FinalDraft") {
            return Format::Fdx;
        }
        Format::of_name(name)
    }

    /// The format of a file's bytes: `%PDF-` at the head (or a `.pdf`
    /// name) is a PDF; anything else is text, read as [`Format::of`]
    /// reads it once decoded ([`decode_text`]).
    pub fn of_bytes(name: &str, bytes: &[u8]) -> Format {
        if bytes.starts_with(b"%PDF-") || Format::of_name(name) == Format::Pdf {
            return Format::Pdf;
        }
        Format::of(name, &decode_text(bytes))
    }

    /// The file name's stem — what names an untitled draft.
    pub fn stem_of(name: &str) -> &str {
        let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
        match file.rsplit_once('.') {
            Some((stem, _)) if !stem.is_empty() => stem,
            _ => file,
        }
    }
}

/// One scene of the draft: its staged world, its shots, and the files
/// its `.world` package is made of.
#[derive(Debug, Clone)]
pub struct SceneDraft {
    /// The scene's number as text (the shot prefix).
    pub scene: String,
    /// The heading as written (`INT. KITCHEN - DAY`).
    pub heading: String,
    /// The package directory name (`scene-1.world`).
    pub package_dir: String,
    /// The staged world.
    pub manifest: wt::WorldManifest,
    /// The scene's shots, in order.
    pub shots: Vec<ShotRow>,
    /// The package's files, path within the package → text
    /// ([`package::package_files`]).
    pub files: Vec<(String, String)>,
}

/// A screenplay's draft: the scenes, the shot list and the board.
#[derive(Debug, Clone)]
pub struct Draft {
    /// The title page's title, else the fallback the caller gave.
    pub title: String,
    /// The scenes, in script order.
    pub scenes: Vec<SceneDraft>,
    /// Every shot, in script order.
    pub rows: Vec<ShotRow>,
    /// `shotlist.csv`.
    pub shotlist_csv: String,
    /// `board.html`: one self-contained page.
    pub board_html: String,
}

/// Why a screenplay didn't draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {
    /// The text isn't a screenplay in any encoding this draft reads:
    /// replacement characters or control bytes where a script has prose
    /// (binary dropped on the page, a PDF, UTF-16 read as UTF-8).
    NotText,
    /// Read as Final Draft (named `.fdx`, or XML at the head), the text
    /// isn't: not well-formed, or not a `<FinalDraft>` document.
    NotFinalDraft {
        /// What the reader found ([`fdx::FdxError`]'s words).
        reason: String,
    },
    /// Read as a PDF (named `.pdf`, or `%PDF-` at the head), the bytes
    /// don't open as one, or the file is encrypted — or it was handed
    /// over as text, which a PDF never is.
    NotPdf {
        /// What the reader found (`pdf::PdfError`'s words).
        reason: String,
    },
    /// A PDF with pages and no text on them: a scan.
    NoTextLayer,
    /// More pages than the caller reads ([`draft_bytes_limited`]).
    TooManyPages {
        /// The pages the file has.
        pages: usize,
        /// The caller's ceiling.
        limit: usize,
    },
    /// A PDF, but this build has no PDF reader (the `pdf` feature is
    /// off).
    NoPdfSupport,
    /// No scene headings — nothing to stage.
    NoScenes,
    /// More scenes than the caller boards ([`draft_limited`]).
    TooManyScenes {
        /// The scenes the script has.
        scenes: usize,
        /// The caller's ceiling.
        limit: usize,
    },
}

impl std::fmt::Display for DraftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DraftError::NotText => write!(
                f,
                "that isn't a text screenplay — binary, or an encoding this draft can't read; export the script as Fountain or plain text"
            ),
            DraftError::NotFinalDraft { reason } => write!(
                f,
                "that isn't a Final Draft file — {reason}; save the script from Final Draft as .fdx, or export it as Fountain"
            ),
            DraftError::NotPdf { reason } => write!(
                f,
                "that isn't a PDF this draft can read — {reason}; export the script again, or as Fountain"
            ),
            DraftError::NoTextLayer => write!(
                f,
                "this PDF is a picture of a script — no text layer to read; export the script's text from the app that wrote it, or OCR it first"
            ),
            DraftError::TooManyPages { pages, limit } => write!(
                f,
                "{pages} pages — this page reads up to {limit}; split the script, or run localgpt-previs on the whole thing"
            ),
            DraftError::NoPdfSupport => write!(
                f,
                "a PDF, and this build reads none (localgpt-previs was built without its pdf feature); export the script as Fountain or .fdx"
            ),
            DraftError::NoScenes => {
                write!(
                    f,
                    "no scene headings found — a scene starts with INT. or EXT. (a line, or a Markdown heading)"
                )
            }
            DraftError::TooManyScenes { scenes, limit } => write!(
                f,
                "{scenes} scenes — this page boards up to {limit}; split the script, or run localgpt-previs on the whole thing"
            ),
        }
    }
}

impl std::error::Error for DraftError {}

/// A file's bytes as text: UTF-16 by its byte-order mark (what some
/// Windows exports write), else UTF-8 with the usual replacement
/// characters — which the text gate then judges.
pub fn decode_text(bytes: &[u8]) -> String {
    let utf16 = |pairs: &mut dyn Iterator<Item = u16>| -> String {
        char::decode_utf16(pairs)
            .map(|c| c.unwrap_or('\u{FFFD}'))
            .collect()
    };
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        return utf16(
            &mut rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_le_bytes(*p)),
        );
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        return utf16(
            &mut rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_be_bytes(*p)),
        );
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Whether the text could be a screenplay at all. Binary dropped on the
/// page, a PDF, or UTF-16 read as UTF-8 arrives full of replacement
/// characters and control bytes, which a script never has; a stray one
/// or two (an old file's odd byte) is just a typo. The gate is a share:
/// a hundredth of the characters, and at least eight of them.
fn looks_like_text(source: &str) -> bool {
    let mut total = 0usize;
    let mut suspect = 0usize;
    for c in source.chars() {
        total += 1;
        if c == '\u{FFFD}' || (c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\u{0C}')) {
            suspect += 1;
        }
    }
    suspect < 8 || suspect * 100 < total
}

/// Draft a Fountain screenplay: the set, cast and coverage of every
/// scene, the shot list and the board — deterministic, a function of the
/// text.
pub fn draft(source: &str, fallback_title: &str) -> Result<Draft, DraftError> {
    draft_inner(source, Format::Fountain, fallback_title, None)
}

/// [`draft`] with a ceiling on the scenes it will stage — the previs
/// page's guard. The scene count is known after the parse (cheap) and
/// checked before staging (the cost), so an outsized script costs a
/// parse, not a board.
pub fn draft_limited(
    source: &str,
    fallback_title: &str,
    max_scenes: usize,
) -> Result<Draft, DraftError> {
    draft_inner(source, Format::Fountain, fallback_title, Some(max_scenes))
}

/// [`draft`] for pages in any format: a Final Draft file or Markdown is
/// translated to Fountain first ([`fdx::to_fountain`],
/// [`markdown::to_fountain`]), then drafted the same way.
pub fn draft_as(source: &str, format: Format, fallback_title: &str) -> Result<Draft, DraftError> {
    draft_inner(source, format, fallback_title, None)
}

/// [`draft_limited`] for pages in any format.
pub fn draft_limited_as(
    source: &str,
    format: Format,
    fallback_title: &str,
    max_scenes: usize,
) -> Result<Draft, DraftError> {
    draft_inner(source, format, fallback_title, Some(max_scenes))
}

/// [`draft`] from a file's bytes, whatever it is: a PDF is read by its
/// page layout, anything else is decoded ([`decode_text`]) and read by
/// its name and head ([`Format::of_bytes`]). What the command line and
/// the localgpt.md page both run.
pub fn draft_bytes(
    bytes: &[u8],
    file_name: &str,
    fallback_title: &str,
) -> Result<Draft, DraftError> {
    draft_bytes_inner(bytes, file_name, fallback_title, None, None)
}

/// [`draft_bytes`] with the page's ceilings: scenes, checked after the
/// parse and before staging, and PDF pages, checked before any page is
/// read.
pub fn draft_bytes_limited(
    bytes: &[u8],
    file_name: &str,
    fallback_title: &str,
    max_scenes: usize,
    max_pages: usize,
) -> Result<Draft, DraftError> {
    draft_bytes_inner(
        bytes,
        file_name,
        fallback_title,
        Some(max_scenes),
        Some(max_pages),
    )
}

fn draft_bytes_inner(
    bytes: &[u8],
    file_name: &str,
    fallback_title: &str,
    max_scenes: Option<usize>,
    max_pages: Option<usize>,
) -> Result<Draft, DraftError> {
    match Format::of_bytes(file_name, bytes) {
        Format::Pdf => {
            let translated = pdf_to_fountain(bytes, max_pages)?;
            draft_script(fountain::parse(&translated), fallback_title, max_scenes)
        }
        format => draft_inner(&decode_text(bytes), format, fallback_title, max_scenes),
    }
}

#[cfg(feature = "pdf")]
fn pdf_to_fountain(bytes: &[u8], max_pages: Option<usize>) -> Result<String, DraftError> {
    pdf::to_fountain(bytes, max_pages).map_err(|e| match e {
        pdf::PdfError::NotPdf(reason) => DraftError::NotPdf { reason },
        pdf::PdfError::NoTextLayer => DraftError::NoTextLayer,
        pdf::PdfError::TooManyPages { pages, limit } => DraftError::TooManyPages { pages, limit },
    })
}

#[cfg(not(feature = "pdf"))]
fn pdf_to_fountain(_bytes: &[u8], _max_pages: Option<usize>) -> Result<String, DraftError> {
    Err(DraftError::NoPdfSupport)
}

fn draft_inner(
    source: &str,
    format: Format,
    fallback_title: &str,
    max_scenes: Option<usize>,
) -> Result<Draft, DraftError> {
    if !looks_like_text(source) {
        return Err(DraftError::NotText);
    }
    let script = match format {
        Format::Fountain => fountain::parse(source),
        Format::Fdx => {
            let translated = fdx::to_fountain(source).map_err(|e| DraftError::NotFinalDraft {
                reason: e.to_string(),
            })?;
            fountain::parse(&translated)
        }
        Format::Markdown => fountain::parse(&markdown::to_fountain(source)),
        Format::Pdf => {
            return Err(DraftError::NotPdf {
                reason: "a PDF is read from its bytes, not its text".to_string(),
            });
        }
    };
    draft_script(script, fallback_title, max_scenes)
}

/// The draft of a parsed script: the scenes staged, the shots listed,
/// the board built.
fn draft_script(
    script: fountain::Script,
    fallback_title: &str,
    max_scenes: Option<usize>,
) -> Result<Draft, DraftError> {
    let scenes = stage::scenes(&script);
    if scenes.is_empty() {
        return Err(DraftError::NoScenes);
    }
    if let Some(limit) = max_scenes
        && scenes.len() > limit
    {
        return Err(DraftError::TooManyScenes {
            scenes: scenes.len(),
            limit,
        });
    }
    let title = script
        .title_page
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("title"))
        .and_then(|(_, v)| v.first())
        .cloned()
        .unwrap_or_else(|| fallback_title.to_string());

    let mut drafts = Vec::with_capacity(scenes.len());
    let mut boards = Vec::with_capacity(scenes.len());
    let mut rows = Vec::new();
    for scene in &scenes {
        let manifest = stage::stage(scene);
        let scene_rows = shots::shot_list(&manifest);
        let (speakers, total_s) = stage::speakers(scene);

        // The lines each shot covers: the master's is the scene
        // summary; a single covers its speaker's lines.
        let mut covers = Vec::with_capacity(scene_rows.len());
        if !scene_rows.is_empty() {
            covers.push(vec![format!(
                "Whole scene — {} speaker{}, {:.1} s",
                speakers.len(),
                if speakers.len() == 1 { "" } else { "s" },
                total_s
            )]);
            for speaker in &speakers {
                covers.push(speaker.lines.clone());
            }
        }

        let package_dir = format!("scene-{}.world", scene.number);
        rows.extend(scene_rows.iter().cloned());
        drafts.push(SceneDraft {
            scene: scene.number.clone(),
            heading: scene.heading.clone(),
            package_dir: package_dir.clone(),
            files: package::package_files(&manifest),
            manifest: manifest.clone(),
            shots: scene_rows.clone(),
        });
        boards.push(SceneBoard {
            scene: scene.number.clone(),
            heading: scene.heading.clone(),
            package_dir,
            manifest,
            shots: scene_rows,
            covers,
        });
    }

    let shotlist_csv = shots::csv(&rows);
    let board_html = board::board_html(&title, &boards);
    Ok(Draft {
        title,
        scenes: drafts,
        rows,
        shotlist_csv,
        board_html,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../samples/cold-eggs.fountain");

    #[test]
    fn the_sample_drafts_to_two_scenes_seven_shots_and_a_board() {
        let d = draft(SAMPLE, "cold-eggs").expect("the sample has scenes");
        assert_eq!(d.title, "Cold Eggs", "the title page's title");
        assert_eq!(d.scenes.len(), 2);
        assert_eq!(d.rows.len(), 7);
        assert_eq!(d.scenes[0].shots.len(), 3);
        assert_eq!(d.scenes[1].shots.len(), 4);
        assert_eq!(d.scenes[0].package_dir, "scene-1.world");
        // Four files make a head-first package.
        let paths: Vec<&str> = d.scenes[0].files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            [
                "manifest.json",
                "ops.jsonl",
                "snapshots/base.json",
                "package.json"
            ]
        );
        assert!(d.shotlist_csv.starts_with("scene,shot,size,"));
        assert!(d.board_html.contains("Cold Eggs — previs board"));
    }

    #[test]
    fn the_markdown_sample_drafts_to_the_fountain_samples_board_byte_for_byte() {
        const SAMPLE_MD: &str = include_str!("../samples/cold-eggs.md");
        let fountain = draft(SAMPLE, "cold-eggs").unwrap();
        let md = draft_as(SAMPLE_MD, Format::Markdown, "cold-eggs").unwrap();
        assert_eq!(md.title, "Cold Eggs");
        assert_eq!(md.board_html, fountain.board_html);
        assert_eq!(md.shotlist_csv, fountain.shotlist_csv);
        assert_eq!(md.scenes.len(), fountain.scenes.len());
        for (a, b) in md.scenes.iter().zip(&fountain.scenes) {
            assert_eq!(a.files, b.files, "{}", a.package_dir);
        }
        // Read as Fountain, the same Markdown has no scenes: `## INT.` is
        // a section there, which is why the format rides the file name.
        assert!(matches!(
            draft(SAMPLE_MD, "cold-eggs"),
            Err(DraftError::NoScenes)
        ));
    }

    #[test]
    fn the_final_draft_sample_drafts_to_the_fountain_samples_board_byte_for_byte() {
        const SAMPLE_FDX: &str = include_str!("../samples/cold-eggs.fdx");
        let fountain = draft(SAMPLE, "cold-eggs").unwrap();
        let fdx = draft_as(SAMPLE_FDX, Format::Fdx, "cold-eggs").unwrap();
        assert_eq!(fdx.title, "Cold Eggs");
        assert_eq!(fdx.board_html, fountain.board_html);
        assert_eq!(fdx.shotlist_csv, fountain.shotlist_csv);
        assert_eq!(fdx.scenes.len(), fountain.scenes.len());
        for (a, b) in fdx.scenes.iter().zip(&fountain.scenes) {
            assert_eq!(a.files, b.files, "{}", a.package_dir);
        }
        // Read as Fountain, the XML has no scenes; read as Final Draft,
        // Fountain isn't XML, and XML that isn't Final Draft says which.
        assert!(matches!(
            draft(SAMPLE_FDX, "cold-eggs"),
            Err(DraftError::NoScenes)
        ));
        let not_xml = draft_as(SAMPLE, Format::Fdx, "x").unwrap_err();
        assert!(matches!(not_xml, DraftError::NotFinalDraft { .. }));
        assert!(
            not_xml
                .to_string()
                .starts_with("that isn't a Final Draft file — the XML doesn't parse"),
            "{not_xml}"
        );
        let html = draft_as("<?xml version=\"1.0\"?><html/>", Format::Fdx, "x").unwrap_err();
        assert_eq!(
            html.to_string(),
            "that isn't a Final Draft file — the XML is a <html> document, not <FinalDraft>; save the script from Final Draft as .fdx, or export it as Fountain"
        );
    }

    #[test]
    fn bytes_are_read_by_their_head_then_their_name() {
        assert_eq!(Format::of_bytes("x.txt", b"%PDF-1.4\n%junk"), Format::Pdf);
        assert_eq!(Format::of_bytes("x.pdf", b"INT. A - DAY"), Format::Pdf);
        assert_eq!(Format::of_bytes("x.md", b"# T"), Format::Markdown);
        assert_eq!(
            Format::of_bytes("x.txt", b"<?xml?><FinalDraft/>"),
            Format::Fdx
        );
        // UTF-16 by its byte-order mark, either way round.
        let le: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(
                "INT. A - DAY\n\nMAYA\nHi.\n"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes),
            )
            .collect();
        assert_eq!(decode_text(&le), "INT. A - DAY\n\nMAYA\nHi.\n");
        let be: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain("Title: T\n".encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(decode_text(&be), "Title: T\n");
        let d = draft_bytes(&le, "script.txt", "x").unwrap();
        assert_eq!(d.scenes.len(), 1);
        assert_eq!(d.scenes[0].shots.len(), 2);
        // Read as text, UTF-16 is replacement characters: the gate holds.
        assert!(matches!(
            draft(&String::from_utf8_lossy(&le), "x"),
            Err(DraftError::NotText)
        ));
        // A PDF handed over as text is refused, not misread.
        assert!(matches!(
            draft_as("%PDF-1.4", Format::Pdf, "x"),
            Err(DraftError::NotPdf { .. })
        ));
    }

    /// A draft's shape: per scene, the heading and the shot names —
    /// what every reading of the same script must agree on, whatever
    /// its line breaks did to the timing.
    #[cfg(feature = "pdf")]
    fn shape(d: &Draft) -> Vec<(String, Vec<String>)> {
        d.scenes
            .iter()
            .map(|s| {
                (
                    s.heading.clone(),
                    s.shots.iter().map(|r| r.shot.clone()).collect(),
                )
            })
            .collect()
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn the_pdf_sample_drafts_to_the_fountain_samples_board_byte_for_byte() {
        const SAMPLE_PDF: &[u8] = include_bytes!("../samples/cold-eggs.pdf");
        let fountain = draft(SAMPLE, "cold-eggs").unwrap();
        let pdf = draft_bytes(SAMPLE_PDF, "cold-eggs.pdf", "cold-eggs").unwrap();
        assert_eq!(pdf.title, "Cold Eggs");
        assert_eq!(pdf.board_html, fountain.board_html);
        assert_eq!(pdf.shotlist_csv, fountain.shotlist_csv);
        for (a, b) in pdf.scenes.iter().zip(&fountain.scenes) {
            assert_eq!(a.files, b.files, "{}", a.package_dir);
        }
        // The same bytes, named anything.
        let renamed = draft_bytes(SAMPLE_PDF, "cold-eggs.txt", "cold-eggs").unwrap();
        assert_eq!(renamed.board_html, fountain.board_html);
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn the_corpus_drafts_the_same_scenes_whatever_wrote_the_pdf() {
        let fountain = draft(SAMPLE, "cold-eggs").unwrap();
        for (name, bytes, title) in [
            (
                "afterwriting",
                &include_bytes!("../samples/pdf/cold-eggs-afterwriting.pdf")[..],
                "COLD EGGS",
            ),
            (
                "chromium",
                &include_bytes!("../samples/pdf/cold-eggs-chromium.pdf")[..],
                "cold-eggs",
            ),
            (
                "cups",
                &include_bytes!("../samples/pdf/cold-eggs-cups.pdf")[..],
                "Cold Eggs",
            ),
            (
                "soffice",
                &include_bytes!("../samples/pdf/cold-eggs-soffice.pdf")[..],
                "Cold Eggs",
            ),
        ] {
            let d = draft_bytes(bytes, "cold-eggs.pdf", "cold-eggs")
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(d.title, title, "{name}");
            assert_eq!(shape(&d), shape(&fountain), "{name}");
        }
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn a_ten_scene_script_keeps_its_numbers_and_its_shots_through_two_generators() {
        const SOURCE: &str = include_str!("../samples/pdf/gull-point.fountain");
        let fountain = draft(SOURCE, "gull-point").unwrap();
        assert_eq!(fountain.scenes.len(), 10);
        for (name, bytes) in [
            (
                "screenplain",
                &include_bytes!("../samples/pdf/gull-point-screenplain.pdf")[..],
            ),
            (
                "afterwriting",
                &include_bytes!("../samples/pdf/gull-point-afterwriting.pdf")[..],
            ),
        ] {
            let d = draft_bytes(bytes, "gull-point.pdf", "gull-point")
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                d.title.eq_ignore_ascii_case("The Lamp at Gull Point"),
                "{name}: {}",
                d.title
            );
            let theirs = shape(&d);
            let ours = shape(&fountain);
            assert_eq!(theirs.len(), 10, "{name}");
            for (i, (a, b)) in theirs.iter().zip(&ours).enumerate() {
                assert_eq!(a.0, b.0, "{name}: scene {}", i + 1);
                // Scene 4 has dual dialogue, which a page sets side by
                // side and this reading takes as one cue (documented).
                if i == 3 {
                    assert!(
                        a.1.len() >= b.1.len() - 1,
                        "{name}: scene 4 {:?} vs {:?}",
                        a.1,
                        b.1
                    );
                } else {
                    assert_eq!(a.1, b.1, "{name}: scene {}", i + 1);
                }
            }
            // The margin numbers and the ordinals agree, so both label
            // the scenes 1–10.
            let dirs: Vec<&str> = d.scenes.iter().map(|s| s.package_dir.as_str()).collect();
            assert_eq!(dirs[0], "scene-1.world", "{name}");
            assert_eq!(dirs[9], "scene-10.world", "{name}");
        }
    }

    #[cfg(feature = "pdf")]
    #[test]
    fn a_scan_a_fake_and_a_torn_pdf_are_refused_not_panics() {
        const SCAN: &[u8] = include_bytes!("../samples/pdf/cold-eggs-scan.pdf");
        const SAMPLE_PDF: &[u8] = include_bytes!("../samples/cold-eggs.pdf");
        assert_eq!(
            draft_bytes(SCAN, "scan.pdf", "x").unwrap_err(),
            DraftError::NoTextLayer
        );
        assert!(matches!(
            draft_bytes(b"INT. A - DAY\n", "script.pdf", "x").unwrap_err(),
            DraftError::NotPdf { .. }
        ));
        assert!(matches!(
            draft_bytes(b"%PDF-1.7\ngarbage", "script.txt", "x").unwrap_err(),
            DraftError::NotPdf { .. } | DraftError::NoTextLayer | DraftError::NoScenes
        ));
        // Torn at several points: whatever comes back, it comes back.
        for cut in [200, 2000, 8000, 20000] {
            let _ = draft_bytes(&SAMPLE_PDF[..cut.min(SAMPLE_PDF.len())], "torn.pdf", "x");
        }
        assert_eq!(
            draft_bytes_limited(SAMPLE_PDF, "cold-eggs.pdf", "x", 300, 1).unwrap_err(),
            DraftError::TooManyPages { pages: 2, limit: 1 }
        );
    }

    #[test]
    fn the_format_and_the_stem_come_off_the_file_name() {
        for (name, format, stem) in [
            ("cold-eggs.fountain", Format::Fountain, "cold-eggs"),
            ("cold-eggs.md", Format::Markdown, "cold-eggs"),
            ("cold-eggs.fdx", Format::Fdx, "cold-eggs"),
            ("script.pdf", Format::Pdf, "script"),
            ("Drafts/Script v3.PDF", Format::Pdf, "Script v3"),
            ("Drafts/COLD EGGS.FDX", Format::Fdx, "COLD EGGS"),
            ("Pages/Scene 4.MD", Format::Markdown, "Scene 4"),
            ("notes.markdown", Format::Markdown, "notes"),
            ("script.txt", Format::Fountain, "script"),
            ("script", Format::Fountain, "script"),
            (".md", Format::Fountain, ".md"),
            ("a.b/script", Format::Fountain, "script"),
        ] {
            assert_eq!(Format::of_name(name), format, "{name}");
            assert_eq!(Format::stem_of(name), stem, "{name}");
        }
    }

    #[test]
    fn xml_at_the_head_is_final_draft_whatever_the_name() {
        assert_eq!(
            Format::of(
                "script.txt",
                "\u{FEFF}\n<?xml version=\"1.0\"?><FinalDraft/>"
            ),
            Format::Fdx
        );
        assert_eq!(Format::of("script.md", "<FinalDraft>"), Format::Fdx);
        assert_eq!(Format::of("script.md", "# Title"), Format::Markdown);
        assert_eq!(Format::of("script.fdx", "INT. A - DAY"), Format::Fdx);
        assert_eq!(Format::of("script", "<b>not xml</b>"), Format::Fountain);
    }

    #[test]
    fn a_script_without_scene_headings_is_nothing_to_stage() {
        assert!(matches!(
            draft("Just a line of prose.\n", "notes"),
            Err(DraftError::NoScenes)
        ));
    }

    #[test]
    fn a_numbered_script_keeps_its_numbers() {
        let d = draft(
            "INT. A - DAY #12#\n\nMAYA\nHi.\n\nINT. B - DAY #12A#\n\nKAI\nHi.\n",
            "x",
        )
        .unwrap();
        let dirs: Vec<&str> = d.scenes.iter().map(|s| s.package_dir.as_str()).collect();
        assert_eq!(dirs, ["scene-12.world", "scene-12A.world"]);
        let shots: Vec<&str> = d.rows.iter().map(|r| r.shot.as_str()).collect();
        assert_eq!(shots, ["12A", "12B", "12A-A", "12A-B"]);
        assert!(
            d.shotlist_csv.contains("\n12A,12A-A,WS,"),
            "{}",
            d.shotlist_csv
        );
        assert!(d.board_html.contains("Scene 12A &middot; INT. B - DAY"));
    }

    #[test]
    fn binary_is_refused_as_not_text() {
        let noise: String = (0..200)
            .map(|i| if i % 3 == 0 { '\u{FFFD}' } else { 'a' })
            .collect();
        assert!(matches!(draft(&noise, "x"), Err(DraftError::NotText)));
        // A stray odd byte is a script with a typo, not binary.
        let odd = "INT. A - DAY\n\nA line with one \u{FFFD} in it.\n";
        assert!(draft(odd, "x").is_ok());
    }

    #[test]
    fn the_scene_ceiling_refuses_before_staging() {
        let script = "INT. A - DAY\n\nINT. B - DAY\n\nINT. C - DAY\n";
        assert_eq!(
            draft_limited(script, "x", 2).err(),
            Some(DraftError::TooManyScenes {
                scenes: 3,
                limit: 2
            })
        );
        assert!(draft_limited(script, "x", 3).is_ok());
        assert_eq!(
            DraftError::TooManyScenes {
                scenes: 3,
                limit: 2
            }
            .to_string(),
            "3 scenes — this page boards up to 2; split the script, or run localgpt-previs on the whole thing"
        );
    }

    #[test]
    fn the_fallback_title_names_an_untitled_script() {
        let d = draft("INT. ROOM - DAY\n\nA chair.\n", "untitled-pages").unwrap();
        assert_eq!(d.title, "untitled-pages");
    }

    #[test]
    fn the_draft_is_deterministic() {
        let a = draft(SAMPLE, "x").unwrap();
        let b = draft(SAMPLE, "x").unwrap();
        assert_eq!(a.board_html, b.board_html);
        assert_eq!(a.shotlist_csv, b.shotlist_csv);
        for (x, y) in a.scenes.iter().zip(&b.scenes) {
            assert_eq!(x.files, y.files);
        }
    }
}
