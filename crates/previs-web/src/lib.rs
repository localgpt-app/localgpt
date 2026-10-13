//! The localgpt.md page's WASM module.
//!
//! One function: [`draft_previs`] takes script pages — a Fountain
//! screenplay, or Markdown by `localgpt_previs::markdown`'s convention,
//! told apart by the file name — and returns the whole draft as JSON:
//! every scene's `.world` package files, the shot list and the printable
//! board, which the page shows as the board and offers as downloads. It
//! is `localgpt_previs::draft::draft_as`, the function the CLI writes to
//! disk: deterministic, model-free, no network, so the page is free to
//! serve, works offline once loaded, and the script never leaves the
//! browser tab.
//!
//! Build (into `website-md/wasm/`, which is what the page loads):
//!
//! ```sh
//! wasm-pack build crates/previs-web --target web --out-dir ../../website-md/wasm
//! ```

use localgpt_previs::draft::{self, Format};
use serde_json::json;
use wasm_bindgen::prelude::*;

/// Draft script pages into previs, as a JSON string:
///
/// ```json
/// {"title": "...",
///  "scenes": [{"scene": "1", "heading": "INT. …", "package_dir": "scene-1.world",
///              "entities": 14, "shots": [<shot rows>],
///              "files": [["manifest.json", "…"], ["ops.jsonl", ""], …]}],
///  "shotlist_csv": "…", "board_html": "…"}
/// ```
///
/// or `{"error": "…"}` when the text isn't a screenplay, has no scene
/// headings, or has more scenes than `max_scenes` — the page's ceiling,
/// checked after the parse and before any staging
/// ([`localgpt_previs::draft::draft_limited_as`]). `file_name` picks the
/// format by its extension (`.md` is Markdown, anything else Fountain)
/// and names the draft by its stem when the title page has no title.
#[wasm_bindgen]
pub fn draft_previs(source: String, file_name: String, max_scenes: usize) -> String {
    let format = Format::of_name(&file_name);
    let fallback_title = Format::stem_of(&file_name);
    match draft::draft_limited_as(&source, format, fallback_title, max_scenes) {
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
