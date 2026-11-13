//! `localgpt world link` — a world as a link.
//!
//! A world of shapes is a few kilobytes of JSON; a previs scene deflates to
//! about a kilobyte. So the whole world can ride in a URL's fragment:
//! `https://localgpt.world/world.html#world=z:<payload>`, the manifest as
//! compact JSON, raw DEFLATE (RFC 1951), unpadded base64url. A browser never
//! sends a fragment to a server, so opening the link uploads nothing and
//! stores nothing: the page inflates it and draws it. Which is the point
//! for a scene from a script nobody has read yet.
//!
//! What cannot ride is a file. A world whose entities reference meshes,
//! textures or a recording is refused, naming them: its link would open a
//! different world. Publish that one as a package.

use std::io::Write as _;

use anyhow::{Result, bail};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;

/// The viewer a link opens in.
pub const VIEWER: &str = "https://localgpt.world/world.html";

/// What a fragment's payload starts with: the parameter, and the encoding
/// (`z`: raw DEFLATE), so another encoding can sit beside it later.
pub const PREFIX: &str = "world=z:";

/// The longest link this writes. Browsers take far more; the things a link
/// is pasted into do not, and the viewer refuses more than it should have
/// to inflate.
pub const MAX_LINK_CHARS: usize = 64 * 1024;

/// The files a world's manifest references: every `path` and every
/// `*_texture`, wherever it sits.
pub fn assets_of(world: &Value) -> Vec<String> {
    fn walk(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, value) in map {
                    match value {
                        Value::String(path) if key == "path" || key.ends_with("_texture") => {
                            out.push(path.clone())
                        }
                        other => walk(other, out),
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(world, &mut out);
    out.sort();
    out.dedup();
    out
}

/// The link that opens `world` (a manifest, as JSON) in the viewer at `base`.
pub fn link(world: &Value, base: &str) -> Result<String> {
    if !world["entities"].is_array() {
        bail!("that is not a world: a manifest has `entities`");
    }
    let assets = assets_of(world);
    if !assets.is_empty() {
        let (shown, more) = (assets.iter().take(3), assets.len().saturating_sub(3));
        bail!(
            "this world uses {} file{} ({}{}), and a link carries only the manifest: publish the package instead",
            assets.len(),
            if assets.len() == 1 { "" } else { "s" },
            shown.cloned().collect::<Vec<_>>().join(", "),
            if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            },
        );
    }
    let json = serde_json::to_vec(world)?;
    let mut deflate = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    deflate.write_all(&json)?;
    let payload = URL_SAFE_NO_PAD.encode(deflate.finish()?);
    let base = base.split('#').next().unwrap_or(base);
    let link = format!("{base}#{PREFIX}{payload}");
    if link.len() > MAX_LINK_CHARS {
        bail!(
            "the link would be {} characters, over the {MAX_LINK_CHARS} a link holds: publish the package instead",
            link.len()
        );
    }
    Ok(link)
}

/// The world a link carries: what the viewer's page does, here so the
/// tests can open what [`link`] wrote.
#[cfg(test)]
pub(crate) fn world_of(link: &str) -> Result<Value> {
    use std::io::Read as _;

    use anyhow::anyhow;

    let payload = link
        .split_once('#')
        .and_then(|(_, fragment)| fragment.strip_prefix(PREFIX))
        .ok_or_else(|| anyhow!("no `#{PREFIX}…` in that link"))?;
    let deflated = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|e| anyhow!("the link's payload isn't base64url: {e}"))?;
    let mut json = Vec::new();
    flate2::read::DeflateDecoder::new(deflated.as_slice())
        // A link is small; what it inflates to must be too.
        .take(16 << 20)
        .read_to_end(&mut json)
        .map_err(|e| anyhow!("the link's payload doesn't inflate: {e}"))?;
    Ok(serde_json::from_slice(&json)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scene() -> Value {
        json!({
            "version": 3,
            "meta": {"name": "scene-1", "description": "INT. KITCHEN — DAY"},
            "entities": (1..=14).map(|id| json!({
                "id": id, "name": format!("thing-{id}"),
                "transform": {"position": [id as f32 * 0.5, 0.85, 0.0], "rotation_degrees": [0, 0, 0], "scale": [1, 1, 1]},
                "shape": {"Capsule": {"radius": 0.18, "half_length": 0.67}},
                "material": {"color": [0.75, 0.3, 0.25, 1.0], "roughness": 0.8},
            })).collect::<Vec<_>>(),
            "next_entity_id": 15,
        })
    }

    #[test]
    fn a_world_goes_into_a_link_and_comes_back_the_same() {
        let world = scene();
        let link = link(&world, VIEWER).unwrap();
        assert!(
            link.starts_with("https://localgpt.world/world.html#world=z:"),
            "{link}"
        );
        let payload = link.split_once("z:").unwrap().1;
        assert!(
            payload
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "base64url, nothing a URL would escape: {payload}"
        );
        assert!(
            link.len() < 1200,
            "fourteen entities in {} characters",
            link.len()
        );
        assert_eq!(world_of(&link).unwrap(), world);
        // A base that already has a fragment loses it, not the world.
        let again = super::link(&world, "http://localhost:8000/world.html#old").unwrap();
        assert!(again.starts_with("http://localhost:8000/world.html#world=z:"));
    }

    #[test]
    fn a_world_that_needs_files_is_refused_naming_them() {
        let mut world = scene();
        world["entities"][0]["mesh_asset"] = json!({"path": "tree.glb", "sha256": "ab"});
        world["entities"][1]["material"]["base_color_texture"] = json!("brick.png");
        world["soundtrack"] = json!({"path": "song.mp3", "duration": 64});
        assert_eq!(assets_of(&world), ["brick.png", "song.mp3", "tree.glb"]);
        let refused = link(&world, VIEWER).unwrap_err().to_string();
        assert!(
            refused.contains("3 files (brick.png, song.mp3, tree.glb)")
                && refused.contains("publish the package"),
            "{refused}"
        );
        // A soundtrack that is curves only, with no recording, rides.
        let mut silent = scene();
        silent["soundtrack"] = json!({"duration": 64, "bpm": 120});
        assert!(link(&silent, VIEWER).is_ok());
    }

    #[test]
    fn what_is_not_a_world_or_not_a_link_says_so() {
        assert!(
            link(&json!({"ops": []}), VIEWER)
                .unwrap_err()
                .to_string()
                .contains("not a world")
        );
        assert!(world_of("https://localgpt.world/world.html?src=x").is_err());
        assert!(world_of("https://localgpt.world/world.html#world=z:!!").is_err());
        let mut huge = scene();
        // Noise does not deflate (xorshift, so the test is the same each run).
        let mut state = 0x9E37_79B9_u32;
        let noise: String = (0..90_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                char::from(b'!' + (state % 90) as u8)
            })
            .collect();
        huge["meta"]["description"] = json!(noise);
        let refused = link(&huge, VIEWER).unwrap_err().to_string();
        assert!(refused.contains("over the 65536"), "{refused}");
    }
}
