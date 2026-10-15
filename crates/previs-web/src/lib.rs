//! The localgpt.md page's WASM module.
//!
//! Two functions: [`draft_previs_bytes`] takes a file's bytes — a
//! Fountain screenplay, a Final Draft `.fdx` file by
//! `localgpt_previs::fdx`'s mapping, a PDF by `localgpt_previs::pdf`'s
//! reading of its page layout, or Markdown by
//! `localgpt_previs::markdown`'s convention, told apart by the file name
//! and the bytes' head — and returns the whole draft as JSON: every
//! scene's `.world` package files, the shot list and the printable
//! board, which the page shows as the board and offers as downloads.
//! [`draft_previs`] is the same for text already decoded. Both are
//! `localgpt_previs::draft`, the function the CLI writes to disk:
//! deterministic, model-free, no network, so the page is free to serve,
//! works offline once loaded, and the script never leaves the browser
//! tab.
//!
//! Build (into `website-md/wasm/`, which is what the page loads):
//!
//! ```sh
//! wasm-pack build crates/previs-web --target web --out-dir ../../website-md/wasm
//! ```

use localgpt_previs::draft::{self, Draft, DraftError, Format};
use serde_json::json;
use wasm_bindgen::prelude::*;

/// Draft a file's bytes into previs, as a JSON string:
///
/// ```json
/// {"title": "...",
///  "scenes": [{"scene": "1", "heading": "INT. …", "package_dir": "scene-1.world",
///              "entities": 14, "shots": [<shot rows>],
///              "files": [["manifest.json", "…"], ["ops.jsonl", ""], …]}],
///  "shotlist_csv": "…", "board_html": "…"}
/// ```
///
/// or `{"error": "…"}` when the file isn't a screenplay, has no scene
/// headings, or has more scenes than `max_scenes` or more pages than
/// `max_pages` — the page's ceilings, checked before any staging
/// ([`localgpt_previs::draft::draft_bytes_limited`]). `file_name` picks
/// the format by its extension (`.fdx` is Final Draft, `.pdf` a PDF,
/// `.md` Markdown, anything else Fountain — and XML or `%PDF-` at the
/// head wins over the name) and names the draft by its stem when the
/// title page has no title.
#[wasm_bindgen]
pub fn draft_previs_bytes(
    bytes: &[u8],
    file_name: String,
    max_scenes: usize,
    max_pages: usize,
) -> String {
    let fallback_title = Format::stem_of(&file_name);
    as_json(draft::draft_bytes_limited(
        bytes,
        &file_name,
        fallback_title,
        max_scenes,
        max_pages,
    ))
}

/// [`draft_previs_bytes`] for text already decoded (no PDFs this way).
#[wasm_bindgen]
pub fn draft_previs(source: String, file_name: String, max_scenes: usize) -> String {
    let format = Format::of(&file_name, &source);
    let fallback_title = Format::stem_of(&file_name);
    as_json(draft::draft_limited_as(
        &source,
        format,
        fallback_title,
        max_scenes,
    ))
}

fn as_json(result: Result<Draft, DraftError>) -> String {
    match result {
        Err(err) => json!({ "error": err.to_string() }).to_string(),
        Ok(drafted) => json!({
            "title": drafted.title,
            "scenes": drafted
                .scenes
                .iter()
                .map(|scene| {
                    json!({
                        "scene": scene.scene,
                        "heading": scene.heading,
                        "package_dir": scene.package_dir,
                        "entities": scene.manifest.entities.len(),
                        "shots": scene.shots,
                        "files": scene.files,
                    })
                })
                .collect::<Vec<_>>(),
            "shotlist_csv": drafted.shotlist_csv,
            "board_html": drafted.board_html,
        })
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../previs/samples/cold-eggs.fountain");
    const SAMPLE_MD: &str = include_str!("../../previs/samples/cold-eggs.md");
    const SAMPLE_FDX: &str = include_str!("../../previs/samples/cold-eggs.fdx");
    const SAMPLE_PDF: &[u8] = include_bytes!("../../previs/samples/cold-eggs.pdf");
    const SCAN_PDF: &[u8] = include_bytes!("../../previs/samples/pdf/cold-eggs-scan.pdf");

    #[test]
    fn the_pdf_sample_drafts_from_its_bytes_to_the_same_board() {
        let fountain: serde_json::Value = serde_json::from_str(&draft_previs_bytes(
            SAMPLE.as_bytes(),
            "cold-eggs.fountain".into(),
            300,
            300,
        ))
        .unwrap();
        let pdf: serde_json::Value = serde_json::from_str(&draft_previs_bytes(
            SAMPLE_PDF,
            "cold-eggs.pdf".into(),
            300,
            300,
        ))
        .unwrap();
        assert_eq!(pdf["title"], "Cold Eggs");
        assert_eq!(pdf["board_html"], fountain["board_html"]);
        assert_eq!(pdf["shotlist_csv"], fountain["shotlist_csv"]);
        // The same bytes, however the file is named.
        let renamed: serde_json::Value = serde_json::from_str(&draft_previs_bytes(
            SAMPLE_PDF,
            "cold-eggs.txt".into(),
            300,
            300,
        ))
        .unwrap();
        assert_eq!(renamed["board_html"], fountain["board_html"]);
        // The bytes path drafts text too, and the text path is unchanged.
        let text: serde_json::Value = serde_json::from_str(&draft_previs(
            SAMPLE.into(),
            "cold-eggs.fountain".into(),
            300,
        ))
        .unwrap();
        assert_eq!(text["board_html"], fountain["board_html"]);
        // A scan, the page ceiling and a non-PDF named .pdf each say why.
        let error = |bytes: &[u8], name: &str, pages: usize| -> String {
            let out: serde_json::Value =
                serde_json::from_str(&draft_previs_bytes(bytes, name.into(), 300, pages)).unwrap();
            out["error"].as_str().unwrap_or("").to_string()
        };
        assert!(error(SCAN_PDF, "scan.pdf", 300).contains("picture of a script"));
        assert!(
            error(SAMPLE_PDF, "cold-eggs.pdf", 1).contains("2 pages — this page reads up to 1")
        );
        assert!(
            error(b"INT. A - DAY\n", "script.pdf", 300).contains("isn't a PDF this draft can read")
        );
    }

    #[test]
    fn the_sample_drafts_to_the_board_the_cli_writes() {
        let out: serde_json::Value = serde_json::from_str(&draft_previs(
            SAMPLE.into(),
            "cold-eggs.fountain".into(),
            300,
        ))
        .unwrap();
        assert_eq!(out["title"], "Cold Eggs");
        let scenes = out["scenes"].as_array().unwrap();
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0]["package_dir"], "scene-1.world");
        assert_eq!(scenes[0]["files"].as_array().unwrap().len(), 4);
        assert_eq!(scenes[0]["files"][0][0], "manifest.json");
        assert_eq!(scenes[0]["shots"].as_array().unwrap().len(), 3);
        assert_eq!(scenes[1]["shots"].as_array().unwrap().len(), 4);
        assert!(
            out["shotlist_csv"]
                .as_str()
                .unwrap()
                .starts_with("scene,shot,")
        );
        // The same board the CLI writes.
        let cli = localgpt_previs::draft::draft(SAMPLE, "cold-eggs").unwrap();
        assert_eq!(out["board_html"], cli.board_html);
    }

    #[test]
    fn the_markdown_sample_drafts_to_the_same_board_by_its_file_name() {
        let fountain: serde_json::Value = serde_json::from_str(&draft_previs(
            SAMPLE.into(),
            "cold-eggs.fountain".into(),
            300,
        ))
        .unwrap();
        let md: serde_json::Value =
            serde_json::from_str(&draft_previs(SAMPLE_MD.into(), "cold-eggs.md".into(), 300))
                .unwrap();
        assert_eq!(md["board_html"], fountain["board_html"]);
        assert_eq!(md["shotlist_csv"], fountain["shotlist_csv"]);
        // Named `.fountain`, the Markdown is a script with no scenes.
        let misnamed: serde_json::Value = serde_json::from_str(&draft_previs(
            SAMPLE_MD.into(),
            "cold-eggs.fountain".into(),
            300,
        ))
        .unwrap();
        assert!(
            misnamed["error"]
                .as_str()
                .unwrap()
                .contains("no scene headings")
        );
    }

    #[test]
    fn the_final_draft_sample_drafts_to_the_same_board_by_its_name_or_its_head() {
        let fountain: serde_json::Value = serde_json::from_str(&draft_previs(
            SAMPLE.into(),
            "cold-eggs.fountain".into(),
            300,
        ))
        .unwrap();
        let fdx: serde_json::Value = serde_json::from_str(&draft_previs(
            SAMPLE_FDX.into(),
            "cold-eggs.fdx".into(),
            300,
        ))
        .unwrap();
        assert_eq!(fdx["title"], "Cold Eggs");
        assert_eq!(fdx["board_html"], fountain["board_html"]);
        assert_eq!(fdx["shotlist_csv"], fountain["shotlist_csv"]);
        // Renamed, the XML still says what it is.
        let renamed: serde_json::Value = serde_json::from_str(&draft_previs(
            SAMPLE_FDX.into(),
            "cold-eggs.txt".into(),
            300,
        ))
        .unwrap();
        assert_eq!(renamed["board_html"], fountain["board_html"]);
        // And XML that isn't Final Draft is refused in its own words.
        let html: serde_json::Value = serde_json::from_str(&draft_previs(
            "<?xml version=\"1.0\"?><html/>".into(),
            "page.fdx".into(),
            300,
        ))
        .unwrap();
        assert!(
            html["error"]
                .as_str()
                .unwrap()
                .contains("isn't a Final Draft file")
        );
    }

    #[test]
    fn an_untitled_draft_is_named_by_the_file_stem() {
        let out: serde_json::Value = serde_json::from_str(&draft_previs(
            "## INT. ROOM - DAY\n\nA chair.\n".into(),
            "Pages/scene 4.md".into(),
            300,
        ))
        .unwrap();
        assert_eq!(out["title"], "scene 4");
    }

    #[test]
    fn a_script_without_scenes_is_an_error_not_a_panic() {
        let out: serde_json::Value = serde_json::from_str(&draft_previs(
            "no scenes here".into(),
            "notes.txt".into(),
            300,
        ))
        .unwrap();
        assert!(out["error"].as_str().unwrap().contains("no scene headings"));
    }

    #[test]
    fn the_ceiling_and_binary_are_errors_with_their_own_words() {
        let error = |source: String, limit: usize| -> String {
            let out: serde_json::Value =
                serde_json::from_str(&draft_previs(source, "x.fountain".into(), limit)).unwrap();
            out["error"].as_str().unwrap().to_string()
        };
        assert!(error("INT. A - DAY\n\nINT. B - DAY\n".into(), 1).contains("boards up to 1"),);
        let noise: String = std::iter::repeat_n('\u{FFFD}', 100).collect();
        assert!(error(noise, 300).contains("isn't a text screenplay"));
    }
}
