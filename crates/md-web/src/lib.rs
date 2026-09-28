//! The localgpt.md page's WASM module.
//!
//! One function: [`compile_world`] takes Markdown text and returns the same
//! `WorldManifest` JSON the desktop app's `--export` writes, which the site's
//! viewer (`crates/world-export/js/world-viewer.js`) renders. It is the
//! rule-derived draft — sections become places along a winding path, one tour
//! stop each — plus any ```` ```world ```` fences, which are exact and need no
//! model. The LLM tiers are deliberately absent: no inference on this page,
//! ever, so it is free to serve, works offline once loaded, and the dropped
//! Markdown never leaves the browser tab. That is the page's whole pitch.
//!
//! Build (into `website-md/wasm/`, which is what the page loads):
//!
//! ```sh
//! wasm-pack build crates/md-web --target web --out-dir ../../website-md/wasm
//! ```

use wasm_bindgen::prelude::*;

/// Compile Markdown into a world manifest JSON string.
///
/// `fallback_title` names the world when the document has no `#` heading
/// (the file name, on the page).
#[wasm_bindgen]
pub fn compile_world(markdown: String, fallback_title: String) -> String {
    let doc = localgpt_md::doc::Doc::parse(&markdown, &fallback_title);
    let world = localgpt_md::draft::compile_with(&doc, &localgpt_md::RecipeStore::in_memory());
    serde_json::to_string(&world).expect("a WorldManifest serializes")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tour the draft builds: one waypoint per section, named "sections".
    fn stops(world: &serde_json::Value) -> &serde_json::Value {
        &world["tours"][0]["waypoints"]
    }

    #[test]
    fn a_document_becomes_a_world() {
        let json = compile_world(
            "# Trip\n\n## Sea\n\nsalt wind\n\n## Peak\n\nthin air".into(),
            "fallback".into(),
        );
        let world: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(world["meta"]["name"], "Trip");
        // Two sections: two regions on top of the draft's own ground and sun.
        let entities = world["entities"].as_array().unwrap();
        assert!(entities.len() > 2, "draft adds regions: {}", entities.len());
        // One stop per section, in order.
        assert_eq!(stops(&world).as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_document_without_headings_is_one_section() {
        let json = compile_world("just prose, no structure".into(), "name.md".into());
        let world: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(world["meta"]["name"], "name.md");
        assert_eq!(stops(&world).as_array().unwrap().len(), 1);
    }

    #[test]
    fn a_world_fence_overrides_its_section() {
        // Fences are model-free authoring, so they belong on the page. The
        // entity form matches the format's JSON: serde enum shape, transform.
        let md = "# Doc\n\n## S\n\n```world\n[{\"id\": 9001, \"name\": \"authored\", \"transform\": {\"position\": [0.0, 0.5, 0.0]}, \"shape\": {\"Cuboid\": {\"size\": [1.0, 1.0, 1.0]}}}]\n```\n";
        let json = compile_world(md.into(), "t".into());
        assert!(json.contains("authored"), "fence entity present");
    }
}
