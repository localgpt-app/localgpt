//! The manifest as the JSON the web viewer consumes.

use localgpt_world_types::WorldManifest;

/// Compact JSON.
pub fn to_json(manifest: &WorldManifest) -> String {
    serde_json::to_string(manifest).expect("WorldManifest serializes")
}

/// Pretty-printed JSON, for files people read and diff.
pub fn to_json_pretty(manifest: &WorldManifest) -> String {
    serde_json::to_string_pretty(manifest).expect("WorldManifest serializes")
}

/// JSON that is safe to place inside a `<script>` element: `</` is written
/// as `<\/`, which JSON reads back as `</`, so a name like `</script>` in the
/// manifest cannot end the script early.
pub fn to_script_safe_json(manifest: &WorldManifest) -> String {
    to_json(manifest).replace("</", "<\\/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_safe_json_round_trips() {
        let mut m = WorldManifest::new("</script><b>x</b>");
        m.entities
            .push(localgpt_world_types::WorldEntity::new(1, "a</b>"));
        let safe = to_script_safe_json(&m);
        assert!(!safe.contains("</"));
        let back: WorldManifest = serde_json::from_str(&safe).unwrap();
        assert_eq!(back, m);
    }
}
