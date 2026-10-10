//! `GET /board`: a tip's previs board, served by the live authority —
//! the same self-contained page `localgpt-previs` writes for a draft
//! (the vendored Open World Format viewer steps through each shot's
//! camera and captures the frame into its card), rendered from the
//! tip's folded manifest instead of a staged scene. With `vs`, two
//! tips side by side, the shots [`shots::diff_shots`] marks badged —
//! treatments as branches, compared.
//!
//! The page is built from the authority's own parts (`base()` and
//! `entries()`, the one fold), and the canvas never moves: a tip that
//! isn't the one on screen is folded fresh for the request.

use localgpt_previs::board::{self, SceneBoard};
use localgpt_previs::shots;
use localgpt_world_agent::live::LiveWorld;
use localgpt_world_editor::Editor;
use localgpt_world_types as wt;

/// The board page for `tip` (`None`: the entry on screen; `"head"`:
/// main's tip), or the two compared when `vs` names a second entry.
/// Errors read like the API's other rejections.
pub fn page(
    world: &LiveWorld,
    view: &Editor,
    tip: Option<&str>,
    vs: Option<&str>,
) -> Result<String, String> {
    let tip = resolve(world, view, tip)?;
    let manifest = fold_at(world, view, tip.as_deref())?;
    let board = scene_board(world, &manifest);
    match vs {
        None => Ok(board::board_html(
            &format!("{} · {}", world_name(&manifest), label(tip.as_deref())),
            &[board],
        )),
        Some(vs) => {
            let other = resolve(world, view, Some(vs))?;
            let other_manifest = fold_at(world, view, other.as_deref())?;
            let other_board = scene_board(world, &other_manifest);
            let diff = shots::diff_shots(&manifest, &other_manifest);
            Ok(board::board_compare_html(
                &world_name(&manifest),
                &board,
                &other_board,
                &diff,
                &label(tip.as_deref()),
                &label(other.as_deref()),
            ))
        }
    }
}

/// A `tip`/`vs` value as the entry id it names: `None` the entry on
/// screen, `"head"` main's tip, anything else itself — checked against
/// the log, so a mistyped id is a clean 404, not a fold error.
fn resolve(
    world: &LiveWorld,
    view: &Editor,
    asked: Option<&str>,
) -> Result<Option<String>, String> {
    match asked {
        None => Ok(view.tip().map(str::to_string)),
        Some("head") => Ok(world.main_tip()),
        Some(id) if view.history().get(id).is_some() => Ok(Some(id.to_string())),
        Some(id) => Err(format!("no such entry: {id}")),
    }
}

/// The manifest a tip folds to: the document on screen when that is
/// the tip asked, a fresh fold over the same log otherwise.
fn fold_at(
    world: &LiveWorld,
    view: &Editor,
    tip: Option<&str>,
) -> Result<wt::WorldManifest, String> {
    if tip == view.tip() {
        return Ok(view.doc().to_manifest());
    }
    Editor::open_at(world.base().clone(), world.entries().to_vec(), tip)
        .map(|folded| folded.doc().to_manifest())
        .map_err(|e| e.to_string())
}

/// A live world as the board's one "scene": its shots in order, no
/// covered lines (that's the draft's business, not the canvas's).
fn scene_board(world: &LiveWorld, manifest: &wt::WorldManifest) -> SceneBoard {
    SceneBoard {
        scene: world_name(manifest),
        heading: manifest.meta.description.clone().unwrap_or_default(),
        package_dir: world
            .dir()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        manifest: manifest.clone(),
        shots: shots::shot_list(manifest),
        covers: Vec::new(),
    }
}

fn world_name(manifest: &wt::WorldManifest) -> String {
    manifest.meta.name.clone()
}

/// A tip as a page title says it: the base by name, a content hash
/// shortened like a git commit, anything else as written.
fn label(tip: Option<&str>) -> String {
    match tip {
        None => "the base".to_string(),
        Some(id) => match id.strip_prefix("sha256:") {
            Some(hex) => hex.chars().take(7).collect(),
            None => id.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A live package in a temp dir: two shot cameras and a cube.
    fn world(name: &str) -> (std::path::PathBuf, LiveWorld) {
        let dir =
            std::env::temp_dir().join(format!("localgpt-gen-board-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = json!({
            "version": 3,
            "meta": {"name": "kitchen", "description": "a test set"},
            "entities": [
                {"id": 1, "name": "counter", "transform": {"position": [0.0, 0.5, 0.0]},
                 "shape": {"Cuboid": {"x": 2.0, "y": 1.0, "z": 0.6}}},
                {"id": 2, "name": "1A", "transform": {"position": [0.0, 1.6, 6.0]},
                 "ext-cinematography": {
                     "camera": {"focal_length_mm": 24, "aspect_ratio": 2.39, "aim": [0.0, 1.0, 0.0]},
                     "shot": {"scene": "1", "order": 1, "in_s": 0, "out_s": 3.4,
                              "size": "WS", "description": "the room"}}},
                {"id": 3, "name": "1B", "transform": {"position": [0.7, 1.6, 2.0]},
                 "ext-cinematography": {
                     "camera": {"focal_length_mm": 50, "aspect_ratio": 2.39, "aim": [0.7, 1.6, 0.0]},
                     "shot": {"scene": "1", "order": 2, "in_s": 0.5, "out_s": 1.5,
                              "size": "MCU", "description": "her"}}}
            ],
            "next_entity_id": 4
        });
        std::fs::write(
            dir.join(localgpt_world_agent::live::MANIFEST),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        let world = LiveWorld::open(&dir).unwrap();
        (dir, world)
    }

    fn view_of(world: &LiveWorld) -> Editor {
        Editor::open_at(
            world.base().clone(),
            world.entries().to_vec(),
            world.main_tip().as_deref(),
        )
        .unwrap()
    }

    #[test]
    fn the_board_page_carries_the_tips_shots() {
        let (dir, world) = world("single");
        let view = view_of(&world);
        // No history yet: the base, which is also the head.
        let html = page(&world, &view, None, None).unwrap();
        assert!(html.contains("kitchen · the base — previs board"));
        assert!(html.contains("1A") && html.contains("1B"));
        assert!(html.contains("24 mm on Super"));
        assert!(html.contains("\"frame_aspect\":"));

        // A tip the log doesn't have is a clean error, not a panic.
        let missing = page(&world, &view, Some("nope"), None).unwrap_err();
        assert!(missing.contains("nope"), "{missing}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_tips_compare_side_by_side_with_changed_shots_marked() {
        let (dir, mut world) = world("compare");
        // A commit on main: 1A re-lensed.
        let main = world
            .submit(
                &json!({"ops": [{"ModifyEntity": {"id": "1A", "patch": {
                    "ext-cinematography": {
                        "camera": {"focal_length_mm": 35, "aspect_ratio": 2.39, "aim": [0.0, 1.0, 0.0]},
                        "shot": {"scene": "1", "order": 1, "in_s": 0, "out_s": 3.4,
                                 "size": "WS", "description": "the room"}}}}}]}),
                "test",
                None,
            )
            .unwrap();
        let mut view = view_of(&world);
        assert_eq!(world.main_tip().as_deref(), main.entry.id.as_deref());
        // Scrubbed to the base: the board on the left is the base's,
        // compared against the head — 1A changed, badged on both sides.
        view.goto(None).unwrap();
        let html = page(&world, &view, None, Some("head")).unwrap();
        assert!(html.contains("previs boards compared"));
        assert!(html.contains("the base (left) vs"));
        assert_eq!(html.matches("badge changed").count(), 2);
        assert!(html.contains("1 of 4 shots marked"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
