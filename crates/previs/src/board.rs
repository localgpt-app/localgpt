//! `board.html`: a printable grid of shot cards, one card per shot —
//! the frame, then shot name, size, lens and sensor, camera height,
//! distance to subject, and the lines it covers.
//!
//! The frames render with the Open World Format viewer (the workspace's
//! vendored `world-viewer.js`, inlined into the page the way
//! `localgpt-world-export::html::generate_html` inlines it): the page
//! keeps one offscreen viewer per scene and steps it through the
//! scene's shots — camera position and `look_at` from the shot, `fov`
//! the derived vertical FOV, the canvas sized to the frame aspect —
//! capturing each frame into its card as a still (a static image
//! prints to PDF from the browser; a fleet of live WebGL canvases
//! wouldn't, and browsers cap their number).
//!
//! The page is one file and works offline from disk: browsers refuse
//! ES-module imports over `file://`, so three.js and the viewer's two
//! addons ride the import map as `data:` URLs instead of relative
//! paths. Without WebGL the cards still carry the shot data — the
//! page prints regardless.

use localgpt_world_export as world_export;
use localgpt_world_types as wt;
use serde_json::json;

use crate::shots::ShotRow;

/// One scene's row of cards: its staged world and shot list.
#[derive(Debug, Clone)]
pub struct SceneBoard {
    /// The scene's number (the shot prefix).
    pub scene: String,
    /// The heading as written (`INT. KITCHEN - DAY`).
    pub heading: String,
    /// The scene's `.world` package directory name (`scene-1.world`),
    /// relative to the board.
    pub package_dir: String,
    /// The staged world.
    pub manifest: wt::WorldManifest,
    /// The shots, in order.
    pub shots: Vec<ShotRow>,
    /// Per shot, the lines it covers (the master's is the scene
    /// summary).
    pub covers: Vec<Vec<String>>,
}

/// A JS module as a `data:` URL: browsers refuse ES-module imports
/// over `file://`, but an import map may point at `data:` URLs, which
/// is how the board stays one offline file.
fn module_data_url(source: &str) -> String {
    use base64::Engine;
    format!(
        "data:text/javascript;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(source)
    )
}

/// The import map: `three` and the viewer's two addons as `data:`
/// URLs. `GLTFLoader`'s relative import of `BufferGeometryUtils` can't
/// resolve against a `data:` URL, so it's rewritten to the mapped
/// specifier (the only edit the vendored bytes get).
fn import_map() -> String {
    let gltf_loader = world_export::html::GLTF_LOADER_JS.replace(
        "from '../utils/BufferGeometryUtils.js'",
        "from 'three/addons/utils/BufferGeometryUtils.js'",
    );
    format!(
        "{{\n  \"imports\": {{\n    \"three\": \"{}\",\n    \"three/addons/controls/OrbitControls.js\": \"{}\",\n    \"three/addons/loaders/GLTFLoader.js\": \"{}\",\n    \"three/addons/utils/BufferGeometryUtils.js\": \"{}\"\n  }}\n}}",
        module_data_url(world_export::html::THREE_MODULE_JS),
        module_data_url(world_export::html::ORBIT_CONTROLS_JS),
        module_data_url(&gltf_loader),
        module_data_url(world_export::html::BUFFER_GEOMETRY_UTILS_JS),
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// JSON safe to place inside a `<script>` element (the world-export
/// rule: `</` becomes `<\/`, so no name can end the script early).
fn script_safe(value: &serde_json::Value) -> String {
    serde_json::to_string(value)
        .expect("the board payload serializes")
        .replace("</", "<\\/")
}

/// One shot's card, and the payload the viewer steps it from. `badge`
/// marks a card on a compare board (`("changed", "changed")`).
fn card_html(
    si: usize,
    i: usize,
    shot: &ShotRow,
    covers: &[String],
    badge: Option<&str>,
) -> (String, serde_json::Value) {
    let aspect = {
        let s = format!("{:.4}", shot.frame_aspect);
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    };
    let lens = format!(
        "{} mm on Super&nbsp;35 ({:.2}&times;{:.2})",
        crate::shots::focal(shot.focal_length_mm),
        shot.sensor_mm[0],
        shot.sensor_mm[1]
    );
    let lines_html = covers
        .iter()
        .map(|l| format!("<li>{}</li>", html_escape(l)))
        .collect::<String>();
    let badge_html = badge
        .map(|b| {
            format!(
                " <span class=\"badge {}\">{}</span>",
                html_escape(b),
                html_escape(b)
            )
        })
        .unwrap_or_default();
    let html = format!(
        "<div class=\"card{}\">\n\
         <div class=\"frame\" style=\"aspect-ratio:{a}\"><img id=\"f-{si}-{i}\" alt=\"{shot} frame\"></div>\n\
         <div class=\"meta\">\n\
         <h3>{shot} <span class=\"size\">{size}</span>{badge_html}</h3>\n\
         <p>{lens} &middot; {a}:1<br>\n\
         height {h:.2} m &middot; subject {d:.2} m<br>\n\
         {t0}&ndash;{t1} s</p>\n\
         <p class=\"desc\">{desc}</p>\n\
         <ul class=\"lines\">{lines_html}</ul>\n\
         </div>\n</div>\n",
        if badge.is_some() { " marked" } else { "" },
        a = aspect,
        si = si,
        i = i,
        shot = html_escape(&shot.shot),
        size = html_escape(&shot.size),
        badge_html = badge_html,
        lens = lens,
        h = shot.height_m,
        d = shot.distance_m,
        t0 = shot.in_s,
        t1 = shot.out_s,
        desc = html_escape(&shot.description),
        lines_html = lines_html,
    );
    let payload = json!({
        "shot": shot.shot,
        "position": shot.position,
        "aim": shot.aim,
        "vfov": shot.vfov_degrees,
        "frame_aspect": shot.frame_aspect,
    });
    (html, payload)
}

/// One scene's section — heading and grid of cards — and its viewer
/// payload. `marks` badges cards by shot name (the compare board).
fn section_html(
    si: usize,
    scene: &SceneBoard,
    marks: &std::collections::BTreeMap<String, String>,
) -> (String, serde_json::Value) {
    let mut cards = String::new();
    let mut payload_shots = Vec::new();
    cards.push_str(&format!(
        "<section class=\"scene\">\n<h2>Scene {} &middot; {} <a href=\"{}/\">{}.world/</a></h2>\n<div class=\"grid\">\n",
        html_escape(&scene.scene),
        html_escape(&scene.heading),
        html_escape(&scene.package_dir),
        html_escape(&scene.scene),
    ));
    for (i, shot) in scene.shots.iter().enumerate() {
        let covers = scene.covers.get(i).cloned().unwrap_or_default();
        let (card, payload) = card_html(
            si,
            i,
            shot,
            &covers,
            marks.get(&shot.shot).map(String::as_str),
        );
        cards.push_str(&card);
        payload_shots.push(payload);
    }
    cards.push_str("</div>\n</section>\n");
    let payload = json!({
        "index": si,
        "manifest": serde_json::to_value(&scene.manifest)
            .expect("a manifest serializes"),
        "shots": payload_shots,
    });
    (cards, payload)
}

/// The page's head: styles and the import map, shared by the board and
/// the compare board.
fn page_head(title: &str, extra_css: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} — previs board</title>
<meta name="generator" content="localgpt-previs">
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
body {{ font: 14px/1.45 system-ui, sans-serif; background: #111; color: #eee; padding: 24px; }}
header h1 {{ font-size: 22px; }}
header p {{ color: #999; margin-bottom: 16px; }}
h2 {{ font-size: 16px; margin: 24px 0 10px; font-weight: 600; }}
h2 a {{ color: #7ab; font-weight: 400; font-size: 13px; text-decoration: none; margin-left: 8px; }}
.grid {{ display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: 14px; }}
.card {{ background: #1c1c1e; border: 1px solid #333; border-radius: 8px; overflow: hidden; break-inside: avoid; }}
.frame {{ width: 100%; background: #000; display: flex; }}
.frame img {{ width: 100%; height: 100%; object-fit: cover; display: block; }}
.meta {{ padding: 10px 12px; }}
.meta h3 {{ font-size: 15px; }}
.meta .size {{ color: #9cf; font-size: 12px; margin-left: 6px; }}
.meta p {{ color: #bbb; font-size: 12.5px; margin-top: 4px; }}
.meta .desc {{ color: #eee; }}
ul.lines {{ margin: 8px 0 2px 18px; color: #999; font-size: 12.5px; }}
{extra_css}
@media print {{
  body {{ background: #fff; color: #000; padding: 0; }}
  .card {{ border-color: #999; background: #fff; }}
  .meta p, ul.lines {{ color: #333; }}
  h2 {{ break-after: avoid; }}
}}
</style>
</head>
<body>
"##,
        title = html_escape(title),
        extra_css = extra_css,
    )
}

/// The page's tail: the inlined viewer and the script stepping it
/// through every scene's shots, capturing each frame into its card.
fn page_tail(payload: &str) -> String {
    format!(
        r##"<script type="importmap">
{import_map}
</script>
<script type="module">
{viewer}

const SCENES = {payload};

// One offscreen viewer per scene, stepped through the shots: position
// and look_at from the shot, fov the derived vertical FOV, the canvas
// sized to the frame aspect. Each rendered frame is captured into its
// card as a still image (static images print; live canvases don't).
const W = 960;
for (const scene of SCENES) {{
  const stage = document.createElement('div');
  stage.style.cssText = `position:fixed;left:${{-2 * W}}px;top:0;width:${{W}}px;height:${{Math.round(W / 2.39)}}px;visibility:hidden;`;
  document.body.appendChild(stage);
  let viewer = null;
  try {{
    viewer = createWorldViewer(stage, scene.manifest, {{ keyboard: false, embedApi: false, preserveDrawingBuffer: true }});
  }} catch (err) {{
    stage.remove();
    continue; // no WebGL — the cards still carry the shot data
  }}
  viewer.controls.enabled = false;
  viewer.controls.enableDamping = false;
  scene.shots.forEach((shot, i) => {{
    viewer.renderer.setSize(W, Math.round(W / shot.frame_aspect), false);
    viewer.camera.aspect = shot.frame_aspect;
    viewer.camera.fov = shot.vfov;
    viewer.camera.updateProjectionMatrix();
    viewer.camera.position.set(...shot.position);
    viewer.controls.target.set(...shot.aim);
    viewer.controls.update();
    viewer.renderer.render(viewer.scene, viewer.camera);
    const img = document.getElementById(`f-${{scene.index}}-${{i}}`);
    if (img) img.src = viewer.renderer.domElement.toDataURL('image/png');
  }});
  viewer.dispose();
  stage.remove();
}}
</script>
</body>
</html>
"##,
        import_map = import_map(),
        viewer = world_export::html::WORLD_VIEWER_JS,
        payload = payload,
    )
}

/// The board page: a card per shot, frames rendered by the vendored
/// viewer stepping through each scene's cameras.
pub fn board_html(title: &str, scenes: &[SceneBoard]) -> String {
    let mut cards = String::new();
    let mut payload_scenes = Vec::new();
    let total_shots: usize = scenes.iter().map(|s| s.shots.len()).sum();

    for (si, scene) in scenes.iter().enumerate() {
        let (section, payload) = section_html(si, scene, &std::collections::BTreeMap::new());
        cards.push_str(&section);
        payload_scenes.push(payload);
    }

    let payload = script_safe(&json!(payload_scenes));

    format!(
        r##"{head}
<header>
<h1>{title} — previs board</h1>
<p>{scene_count} scenes &middot; {total_shots} shots &middot; staged by localgpt-previs (deterministic, no model) &middot; print to PDF from the browser</p>
</header>
{cards}
{tail}"##,
        head = page_head(title, ""),
        title = html_escape(title),
        scene_count = scenes.len(),
        total_shots = total_shots,
        cards = cards,
        tail = page_tail(&payload),
    )
}

/// The compare board: two tips' shot lists side by side, the shots
/// [`crate::shots::diff_shots`] marks badged on their cards —
/// *changed* on both sides, *removed* on the left, *added* on the
/// right. `left_label` / `right_label` name the two tips as the caller
/// knows them (entry ids, "head", …).
pub fn board_compare_html(
    title: &str,
    left: &SceneBoard,
    right: &SceneBoard,
    diff: &crate::shots::ShotDiff,
    left_label: &str,
    right_label: &str,
) -> String {
    let mut left_marks = std::collections::BTreeMap::new();
    let mut right_marks = std::collections::BTreeMap::new();
    for name in &diff.changed {
        left_marks.insert(name.clone(), "changed".to_string());
        right_marks.insert(name.clone(), "changed".to_string());
    }
    for name in &diff.removed {
        left_marks.insert(name.clone(), "removed".to_string());
    }
    for name in &diff.added {
        right_marks.insert(name.clone(), "added".to_string());
    }

    let (left_cards, left_payload) = section_html(0, left, &left_marks);
    let (right_cards, right_payload) = section_html(1, right, &right_marks);
    let payload = script_safe(&json!([left_payload, right_payload]));

    let css = ".columns { display: grid; grid-template-columns: 1fr 1fr; gap: 18px; align-items: start; }\n\
               .columns .grid { grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); }\n\
               .badge { font-size: 10px; text-transform: uppercase; letter-spacing: 0.06em; \
               padding: 1px 6px; border-radius: 4px; margin-left: 6px; vertical-align: middle; }\n\
               .badge.changed { background: #7a5; color: #111; }\n\
               .badge.added { background: #9cf; color: #111; }\n\
               .badge.removed { background: #e97; color: #111; }\n\
               .card.marked { border-color: #7a5; }\n\
               @media (max-width: 900px) { .columns { grid-template-columns: 1fr; } }";

    format!(
        r##"{head}
<header>
<h1>{title} — previs boards compared</h1>
<p>{left_label} (left) vs {right_label} (right) &middot; {marked} of {total} shots marked &middot; print to PDF from the browser</p>
</header>
<div class="columns">
{left_cards}
{right_cards}
</div>
{tail}"##,
        head = page_head(title, css),
        title = html_escape(title),
        left_label = html_escape(left_label),
        right_label = html_escape(right_label),
        marked = diff.len(),
        total = left.shots.len() + right.shots.len(),
        left_cards = left_cards,
        right_cards = right_cards,
        tail = page_tail(&payload),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{fountain, shots, stage};

    fn a_scene_board() -> SceneBoard {
        let script = fountain::parse(
            "INT. KITCHEN - DAY\n\nMaya cooks.\n\nMAYA\nSit down.\n\nKAI\nI will.\n",
        );
        let scene = &stage::scenes(&script)[0];
        let manifest = stage::stage(scene);
        let shots = shots::shot_list(&manifest);
        let covers = vec![
            vec!["Whole scene".to_string()],
            vec!["Sit down.".to_string()],
            vec!["I will.".to_string()],
        ];
        SceneBoard {
            scene: "1".to_string(),
            heading: scene.heading.clone(),
            package_dir: "scene-1.world".to_string(),
            manifest,
            shots,
            covers,
        }
    }

    #[test]
    fn the_board_carries_every_shot_card_and_the_viewer_payload() {
        let html = board_html("Test", &[a_scene_board()]);
        assert!(html.contains("<title>Test — previs board</title>"));
        // One card per shot (1A, 1B, 1C), each with an image slot.
        for (id, name) in [("f-0-0", "1A"), ("f-0-1", "1B"), ("f-0-2", "1C")] {
            assert!(html.contains(&format!("id=\"{id}\"")), "{id}");
            assert!(html.contains(name), "{name}");
        }
        // One self-contained offline file: the viewer inlined, three.js
        // and the addons as import-map data URLs (no file:// module
        // imports — browsers refuse those).
        assert!(html.contains("export function createWorldViewer"));
        assert!(html.contains("\"three\": \"data:text/javascript;base64,"));
        assert!(!html.contains("./vendor/"));
        // The payload is script-safe and carries the cameras.
        let payload_line = html
            .lines()
            .find(|l| l.contains("const SCENES"))
            .expect("the payload line");
        assert!(!payload_line.contains("</"));
        assert!(payload_line.contains("\"frame_aspect\":"));
        // The card data the spec asks for.
        assert!(html.contains("24 mm on Super"));
        assert!(html.contains("height 1.60 m"));
        assert!(html.contains("Sit down."));
    }

    #[test]
    fn the_compare_board_marks_changed_shots_on_both_sides() {
        let left = a_scene_board();
        // The treatment: 1A re-lensed, everything else as staged.
        let mut right_manifest = left.manifest.clone();
        let camera = right_manifest
            .entities
            .iter_mut()
            .find(|e| e.name.as_str() == "1A")
            .unwrap()
            .extra
            .get_mut("ext-cinematography")
            .unwrap();
        camera
            .get_mut("camera")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("focal_length_mm".into(), serde_json::json!(35));
        let right_shots = shots::shot_list(&right_manifest);
        let right = SceneBoard {
            manifest: right_manifest,
            shots: right_shots,
            ..left.clone()
        };

        let diff = shots::diff_shots(&left.manifest, &right.manifest);
        let html = board_compare_html("Test", &left, &right, &diff, "main", "treatment");
        assert!(html.contains("<title>Test — previs board</title>"));
        assert!(html.contains("main (left) vs treatment (right)"));
        assert!(html.contains("1 of 6 shots marked"));
        // 1A is badged on both boards, and only there.
        assert_eq!(html.matches("badge changed").count(), 2);
        // Two columns of cards: frame slots unique per side, one
        // payload scene each, so the viewer renders both boards.
        for id in ["f-0-0", "f-1-0"] {
            assert!(html.contains(&format!("id=\"{id}\"")), "{id}");
        }
        let payload_line = html
            .lines()
            .find(|l| l.contains("const SCENES"))
            .expect("the payload line");
        assert!(!payload_line.contains("</"));
        assert_eq!(payload_line.matches("\"manifest\":").count(), 2);
        assert!(payload_line.contains("\"focal_length_mm\":35"));
    }

    #[test]
    fn the_import_maps_gltf_loader_import_is_data_url_safe() {
        let map = import_map();
        assert!(map.contains("data:text/javascript;base64,"));
        // The rewrite happened before encoding (the relative import
        // can't resolve against a data: URL).
        let marker = "three/addons/utils/BufferGeometryUtils.js";
        let data_url = map
            .split("\"three/addons/loaders/GLTFLoader.js\": \"data:text/javascript;base64,")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("the loader's data URL");
        use base64::Engine;
        let decoded = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(data_url)
                .unwrap(),
        )
        .unwrap();
        assert!(decoded.contains(&format!("from '{marker}'")));
        assert!(!decoded.contains("'../utils/"));
    }
}
