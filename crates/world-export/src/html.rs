//! Self-contained HTML export: the web viewer plus the manifest, one file.
//!
//! The page loads `three` from a CDN by default (an export is a single file
//! that has to work from disk); [`ExportOptions::three_base_url`] points it
//! at a self-hosted copy instead, which is what localgpt.world does.

use localgpt_world_types as wt;

use crate::json::to_script_safe_json;

/// The web viewer module, embedded from `js/world-viewer.js`.
/// The one web renderer for the LocalGPT world format.
pub const WORLD_VIEWER_JS: &str = include_str!("../js/world-viewer.js");

/// The browser client for collaborative sessions (joins over WebSocket,
/// applies ops live). Served by a hosting Gen at `/session-client.js`.
pub const SESSION_CLIENT_JS: &str = include_str!("../js/session-client.js");

/// Vendored three.js (MIT, r170) and the two addons the viewer imports, so
/// a hosted session's join page works with no internet access. Served by a
/// hosting Gen under `/vendor/`; the join page's import map points there.
pub const THREE_MODULE_JS: &str = include_str!("../js/vendor/three/three.module.js");
pub const ORBIT_CONTROLS_JS: &str =
    include_str!("../js/vendor/three/addons/controls/OrbitControls.js");
pub const GLTF_LOADER_JS: &str = include_str!("../js/vendor/three/addons/loaders/GLTFLoader.js");
pub const BUFFER_GEOMETRY_UTILS_JS: &str =
    include_str!("../js/vendor/three/addons/utils/BufferGeometryUtils.js");

/// The three.js release the viewer is written against.
pub const THREE_VERSION: &str = "0.170.0";

/// Knobs for [`generate_html`].
#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// Base URL of a three.js package: `<base>/build/three.module.js` and
    /// `<base>/examples/jsm/` must exist under it. `None` uses unpkg.
    pub three_base_url: Option<String>,
    /// URL prefix for mesh assets and the soundtrack file, relative to the
    /// page. `None` renders placeholders and performs the soundtrack silently
    /// from its analysis curves.
    pub asset_base: Option<String>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            three_base_url: None,
            asset_base: Some("assets".to_string()),
        }
    }
}

/// Generate a complete, self-contained HTML page for the manifest.
pub fn generate_html(manifest: &wt::WorldManifest) -> String {
    generate_html_with(manifest, &ExportOptions::default())
}

/// [`generate_html`] with options.
pub fn generate_html_with(manifest: &wt::WorldManifest, options: &ExportOptions) -> String {
    let title = html_escape(&manifest.meta.name);
    let description = html_escape(
        manifest
            .meta
            .description
            .as_deref()
            .unwrap_or("A 3D world in the LocalGPT world format"),
    );
    let entity_count = manifest.entities.len();
    let has_tours = manifest.tours.iter().any(|t| !t.waypoints.is_empty());
    let has_audio =
        manifest.entities.iter().any(|e| e.audio.is_some()) || manifest.soundtrack.is_some();
    let default_compliance = wt::ComplianceMeta::default();
    let c = manifest
        .meta
        .compliance
        .as_ref()
        .unwrap_or(&default_compliance);
    let three_base = options
        .three_base_url
        .as_deref()
        .map(|b| b.trim_end_matches('/').to_string())
        .unwrap_or_else(|| format!("https://unpkg.com/three@{THREE_VERSION}"));
    let asset_base = options
        .asset_base
        .as_deref()
        .map(|a| serde_json::to_string(a).expect("string"))
        .unwrap_or_else(|| "\"\"".to_string());
    let manifest_json = to_script_safe_json(manifest);
    let viewer = WORLD_VIEWER_JS;

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{description}">
<meta property="og:title" content="{title}">
<meta property="og:description" content="{description}">
<meta property="og:type" content="website">
<meta name="twitter:card" content="summary">
<meta name="twitter:title" content="{title}">
<meta name="twitter:description" content="{description}">
<meta name="generator" content="{generation_tool}">
<meta name="localgpt:generation-method" content="{generation_method}">
<meta name="localgpt:human-modifiable" content="{human_modifiable}">
<meta name="localgpt:steam-code-tool-exempt" content="{steam_code_tool_exempt}">
<meta name="localgpt:eu-ai-act-risk-level" content="{eu_ai_act_risk_level}">
<meta name="localgpt:no-gen-ai-compatible" content="{no_gen_ai_compatible}">
<script type="application/ld+json">
{{
  "@context": "https://schema.org",
  "@type": "3DModel",
  "name": "{title}",
  "description": "{description}",
  "encodingFormat": "text/html",
  "creator": {{ "@type": "SoftwareApplication", "name": "{generation_tool}" }}
}}
</script>
<style>
* {{ margin: 0; padding: 0; box-sizing: border-box; }}
html, body {{ width: 100%; height: 100%; overflow: hidden; background: #000; }}
#scene {{ width: 100%; height: 100%; }}
#info {{
  position: absolute; top: 10px; left: 10px;
  color: #fff; font: 14px/1.4 system-ui, sans-serif;
  background: rgba(0,0,0,0.5); padding: 8px 12px; border-radius: 6px;
  pointer-events: none; user-select: none;
}}
#audio-btn, #tour-btn {{
  position: absolute; bottom: 20px;
  background: rgba(0,0,0,0.6); color: #fff; border: 1px solid rgba(255,255,255,0.3);
  padding: 8px 16px; border-radius: 6px; cursor: pointer;
  font: 14px system-ui, sans-serif;
}}
#audio-btn {{ right: 20px; }}
#tour-btn {{ right: 140px; }}
#audio-btn:hover, #tour-btn:hover {{ background: rgba(0,0,0,0.8); }}
#tour-desc {{
  position: absolute; bottom: 60px; left: 50%; transform: translateX(-50%);
  background: rgba(0,0,0,0.7); color: #fff; padding: 10px 20px; border-radius: 8px;
  font: 14px/1.5 system-ui, sans-serif; max-width: 500px; text-align: center;
  display: none; pointer-events: none;
}}
</style>
</head>
<body>
<div id="scene"></div>
<div id="info">{title}<br><small>Drag to orbit &middot; Scroll to zoom &middot; WASD to move</small><br><small>{entity_count} entities{tours_label}{audio_label}</small></div>
<button id="audio-btn" style="display:none">Sound On</button>
<button id="tour-btn" style="display:none">Start Tour</button>
<div id="tour-desc"></div>
<script type="importmap">
{{
  "imports": {{
    "three": "{three_base}/build/three.module.js",
    "three/addons/": "{three_base}/examples/jsm/"
  }}
}}
</script>
<script type="module">
{viewer}
const MANIFEST = {manifest_json};
window.localgptViewer = createWorldViewer(document.getElementById('scene'), MANIFEST, {{
  assetBase: {asset_base},
  audioButton: document.getElementById('audio-btn'),
  tourButton: document.getElementById('tour-btn'),
  tourCaption: document.getElementById('tour-desc'),
  embedApi: true,
}});
</script>
</body>
</html>
"#,
        title = title,
        description = description,
        entity_count = entity_count,
        tours_label = if has_tours { " &middot; Tours" } else { "" },
        audio_label = if has_audio { " &middot; Audio" } else { "" },
        generation_tool = html_escape(&c.generation_tool),
        generation_method = html_escape(&c.generation_method),
        human_modifiable = c.human_modifiable,
        steam_code_tool_exempt = c.steam_code_tool_exempt,
        eu_ai_act_risk_level = html_escape(&c.eu_ai_act_risk_level),
        no_gen_ai_compatible = c.no_gen_ai_compatible,
        three_base = three_base,
        viewer = viewer,
        manifest_json = manifest_json,
        asset_base = asset_base,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The session join page, served by a hosting Gen at `/` and by
/// `localgpt-relay` at `/r/<code>/`. Logic lives in
/// [`SESSION_CLIENT_JS`]; the WebSocket URL derives from the page path.
pub const SESSION_JOIN_PAGE_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Join a LocalGPT world</title>
<style>
* { margin: 0; padding: 0; box-sizing: border-box; }
html, body { width: 100%; height: 100%; overflow: hidden; background: #0b0e14; color: #e6e9ef;
  font: 14px/1.5 system-ui, sans-serif; }
#scene { width: 100%; height: 100%; }
#join-overlay { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center;
  background: radial-gradient(ellipse at center, #141a26 0%, #0b0e14 100%); z-index: 10; }
.card { background: #161c28; border: 1px solid #2a3347; border-radius: 12px; padding: 28px;
  width: 320px; box-shadow: 0 12px 40px rgba(0,0,0,0.5); }
.card h1 { font-size: 18px; margin-bottom: 4px; }
.card p { color: #8b95a9; margin-bottom: 16px; }
.card input { width: 100%; padding: 10px 12px; border-radius: 8px; border: 1px solid #2a3347;
  background: #0e1219; color: #e6e9ef; font-size: 15px; margin-bottom: 12px; }
.card button { width: 100%; padding: 10px; border-radius: 8px; border: none; background: #4f7cff;
  color: white; font-size: 15px; font-weight: 600; cursor: pointer; }
.card button:disabled { opacity: 0.5; }
#join-error { color: #ff7a7a; min-height: 18px; margin-top: 8px; }
#hud { position: absolute; top: 10px; left: 10px; background: rgba(0,0,0,0.5); padding: 8px 12px;
  border-radius: 8px; pointer-events: none; }
#hud-session { font-weight: 600; }
#hud-peers { color: #aab4c8; font-size: 12px; }
#status { position: absolute; top: 12px; left: 50%; transform: translateX(-50%);
  background: rgba(0,0,0,0.6); padding: 6px 14px; border-radius: 999px; display: none; }
#chat { position: absolute; left: 10px; bottom: 10px; width: 280px; display: flex;
  flex-direction: column; gap: 6px; }
#chat-log { max-height: 160px; overflow-y: auto; background: rgba(0,0,0,0.45); border-radius: 8px;
  padding: 8px; font-size: 13px; }
#chat-log:empty { display: none; }
.chat-line b { color: #8fb3ff; margin-right: 4px; }
.chat-agent b { color: #7be0a3; }
.chat-system b { color: #d9b45f; }
#chat input, #prompt-bar input { width: 100%; padding: 8px 12px; border-radius: 8px;
  border: 1px solid #2a3347; background: rgba(14,18,25,0.9); color: #e6e9ef; }
#prompt-bar { position: absolute; bottom: 10px; left: 50%; transform: translateX(-50%);
  width: min(520px, 60vw); }
</style>
<script type="importmap">
{
  "imports": {
    "three": "/vendor/three.module.js",
    "three/addons/": "/vendor/three/addons/"
  }
}
</script>
</head>
<body>
<div id="scene"></div>
<div id="hud"><div id="hud-session"></div><div id="hud-peers"></div></div>
<div id="status"></div>
<div id="chat"><div id="chat-log"></div><input id="chat-input" placeholder="Chat… (/undo undoes your last build)" autocomplete="off"></div>
<div id="prompt-bar"><input id="prompt-input" placeholder="Ask the AI to build something…" autocomplete="off"></div>
<div id="join-overlay">
  <div class="card">
    <h1>Join this world</h1>
    <p>A friend is hosting a LocalGPT session. Pick a name and step in.</p>
    <input id="name-input" placeholder="Your name" maxlength="32" autocomplete="off">
    <button id="join-btn">Join</button>
    <div id="join-error"></div>
  </div>
</div>
<script type="module">
import { startSessionClient } from '/session-client.js';
startSessionClient();
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn conformance_worlds() -> Vec<(String, wt::WorldManifest)> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../world-types/conformance");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json") {
                let text = std::fs::read_to_string(&path).unwrap();
                out.push((
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    serde_json::from_str(&text).unwrap(),
                ));
            }
        }
        assert!(!out.is_empty());
        out
    }

    #[test]
    fn viewer_module_is_embedded_and_script_safe() {
        assert!(WORLD_VIEWER_JS.contains("export function createWorldViewer"));
        assert!(
            !WORLD_VIEWER_JS.contains("</script"),
            "the viewer would end the inline script early"
        );
    }

    #[test]
    fn every_conformance_world_exports() {
        for (name, manifest) in conformance_worlds() {
            let html = generate_html(&manifest);
            assert!(
                html.contains(&format!("<title>{}</title>", manifest.meta.name)),
                "{name}"
            );
            assert!(
                html.contains("createWorldViewer(document.getElementById('scene')"),
                "{name}"
            );
            assert!(
                html.contains("unpkg.com/three@0.170.0/build/three.module.js"),
                "{name}"
            );
            // Exactly one closing script tag after the module opens, so the
            // manifest never terminates it early.
            let module = html.split("<script type=\"module\">").nth(1).unwrap();
            assert_eq!(module.matches("</script>").count(), 1, "{name}");
            let json_start = module.find("const MANIFEST = ").unwrap() + "const MANIFEST = ".len();
            let json_end = module[json_start..]
                .find(";\nwindow.localgptViewer")
                .unwrap();
            let back: wt::WorldManifest =
                serde_json::from_str(&module[json_start..json_start + json_end]).unwrap();
            assert_eq!(back, manifest, "{name}: manifest survives embedding");
        }
    }

    #[test]
    fn options_select_three_and_assets() {
        let manifest = wt::WorldManifest::new("opts");
        let html = generate_html_with(
            &manifest,
            &ExportOptions {
                three_base_url: Some("vendor/three/".into()),
                asset_base: None,
            },
        );
        assert!(html.contains("\"three\": \"vendor/three/build/three.module.js\""));
        assert!(html.contains("assetBase: \"\","));
    }

    #[test]
    fn titles_are_escaped() {
        let manifest = wt::WorldManifest::new("<b>\"x\" & y</b>");
        let html = generate_html(&manifest);
        assert!(html.contains("<title>&lt;b&gt;&quot;x&quot; &amp; y&lt;/b&gt;</title>"));
    }
}
