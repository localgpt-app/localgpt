//! The localgpt previs page's WASM module.
//!
//! One function: [`draft_previs`] takes a Fountain screenplay and returns
//! the whole draft as JSON — every scene's `.world` package files, the
//! shot list and the printable board — which the page shows as the board
//! and offers as downloads. It is `localgpt_previs::draft`, the function
//! the CLI writes to disk: deterministic, model-free, no network, so the
//! page is free to serve, works offline once loaded, and the script never
//! leaves the browser tab.
//!
//! Build (into `website-previs/wasm/`, which is what the page loads):
//!
//! ```sh
//! wasm-pack build crates/previs-web --target web --out-dir ../../website-previs/wasm
//! ```

use serde_json::json;
use wasm_bindgen::prelude::*;

/// Draft a screenplay into previs, as a JSON string:
///
/// ```json
/// {"title": "...",
///  "scenes": [{"scene": "1", "heading": "INT. …", "package_dir": "scene-1.world",
///              "entities": 14, "shots": [<shot rows>],
///              "files": [["manifest.json", "…"], ["ops.jsonl", ""], …]}],
///  "shotlist_csv": "…", "board_html": "…"}
/// ```
///
/// or `{"error": "…"}` when the script has no scene headings.
/// `fallback_title` names the draft when the title page has no title
/// (the file name, on the page).
#[wasm_bindgen]
pub fn draft_previs(fountain: String, fallback_title: String) -> String {
    match localgpt_previs::draft::draft(&fountain, &fallback_title) {
        None => json!({
            "error": "no scene headings found — a scene starts with INT. or EXT.",
        })
        .to_string(),
        Some(drafted) => json!({
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

    #[test]
    fn the_sample_drafts_to_the_board_the_cli_writes() {
        let out: serde_json::Value =
            serde_json::from_str(&draft_previs(SAMPLE.into(), "cold-eggs".into())).unwrap();
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
    fn a_script_without_scenes_is_an_error_not_a_panic() {
        let out: serde_json::Value =
            serde_json::from_str(&draft_previs("no scenes here".into(), "notes".into())).unwrap();
        assert!(out["error"].as_str().unwrap().contains("no scene headings"));
    }
}
