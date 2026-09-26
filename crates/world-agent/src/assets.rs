//! The CC0 asset pack — manifest types, kind resolution, and scatter math,
//! ported from LocalGPT Verse's `world_assets.rs` (Apache-2.0), minus the
//! parts that are Verse's alone (moods, Bevy-side LOD components, the
//! prop-population system).
//!
//! The pack lives in a models-style directory containing `manifest.json`
//! and the `.glb` files it references; [`read_manifest_at`] parses the
//! manifest from any such root. GLB paths are `models/<file>` relative to
//! that root — the app resolves where the pack lives (env var, local
//! `assets/`, a sibling checkout) and passes the root in.
//!
//! Absent pack → no `place_asset`/`scatter_field` tools, primitives only;
//! the app always runs (Verse's rule, kept).

use std::path::Path;

use serde::Deserialize;

/// Placement tier — governs target size. Verse also budgets instance counts
/// per tier and distance-culls with `VisibilityRange`; MD builds one small
/// region per section, so only span normalization ports.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Hero,
    Medium,
    Scatter,
}

impl Tier {
    /// Target real-world span (metres) — models with known native size are
    /// rescaled to it, since Poly Haven scans range from 0.1 m (shell) to
    /// 90 m (cliff). Keeps every tier's footprint consistent (Verse R7: an
    /// unnormalized cliff reached ~138 m and swallowed the camera).
    fn target_span(self) -> f32 {
        match self {
            Tier::Hero => 7.0,
            Tier::Medium => 2.5,
            Tier::Scatter => 1.0,
        }
    }

    /// Legacy multiplier when the manifest lacks native `dims`.
    fn base_scale(self) -> f32 {
        match self {
            Tier::Hero => 1.5,
            Tier::Medium => 1.15,
            Tier::Scatter => 0.9,
        }
    }
}

/// One manifest entry. Provenance fields double as the audit trail (all
/// entries are CC0, enforced by the pack's CI).
#[derive(Debug, Clone, Deserialize)]
pub struct AssetEntry {
    /// Read for the asset tools' example lists (`llm` builds) and tests.
    #[allow(dead_code)]
    pub name: String,
    /// glTF path relative to the models directory.
    pub file: String,
    /// Semantic kind (`rock`, `tree`, `lamp`, …) — the small stable
    /// vocabulary the agent's `place_asset` enum exposes; variants behind it
    /// rotate ([`resolve_kind`]).
    #[serde(default)]
    pub kind: String,
    pub tier: Tier,
    #[serde(default = "one")]
    pub scale: f32,
    /// Native dimensions in metres `[x, y, z]` — placement rescales to the
    /// tier's target span when present.
    #[serde(default)]
    pub dims: Option<[f32; 3]>,
    #[allow(dead_code)]
    pub license: String,
    #[allow(dead_code)]
    pub author: String,
}

fn one() -> f32 {
    1.0
}

impl AssetEntry {
    /// Placement scale: normalized to the tier's target span from the
    /// model's native size when known, else the tier's legacy multiplier.
    /// Baked into the entity transform at build time, so cached builds (and
    /// the default no-model build) need no manifest to render correctly.
    pub fn placement_scale(&self) -> f32 {
        match self.dims {
            Some(d) => {
                let span = d[0].max(d[1]).max(d[2]).max(0.01);
                self.scale * self.tier.target_span() / span
            }
            None => self.scale * self.tier.base_scale(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetManifest {
    #[allow(dead_code)]
    pub version: u32,
    pub assets: Vec<AssetEntry>,
}

impl AssetManifest {
    /// The distinct kinds present, in first-appearance order — the agent's
    /// `place_asset` enum and a stable, small vocabulary however large the
    /// variant pool grows.
    // Read by the `llm` tier's tool schemas; kept in the default build with
    // the type (Verse's `tier.rs` precedent).
    #[allow(dead_code)]
    pub fn kinds(&self) -> Vec<&str> {
        let mut kinds: Vec<&str> = Vec::new();
        for a in &self.assets {
            if !a.kind.is_empty() && !kinds.contains(&a.kind.as_str()) {
                kinds.push(a.kind.as_str());
            }
        }
        kinds
    }
}

/// Resolve a semantic kind to a concrete manifest variant — the host half of
/// the two-level vocabulary. MD has no moods, so the pool is simply every
/// entry of the kind. Rotation runs across the whole pool — every variant is
/// used once before any repeats — and restarts from the top when exhausted.
///
/// `used` accumulates files across calls (one list per session) and is
/// updated in place. Deterministic: ties break by manifest order, never by
/// hash or time.
pub fn resolve_kind<'a>(
    manifest: &'a AssetManifest,
    kind: &str,
    used: &mut Vec<String>,
) -> Option<&'a AssetEntry> {
    let pool: Vec<&AssetEntry> = manifest.assets.iter().filter(|a| a.kind == kind).collect();
    let first = pool.first()?;
    let entry = *pool
        .iter()
        .find(|e| !used.contains(&e.file))
        .unwrap_or(first);
    used.push(entry.file.clone());
    Some(entry)
}

/// Read `models/manifest.json` under `root` if present. Used by the
/// generation worker (no Bevy access) and the asset tools.
// Test-covered; the non-test callers live behind `feature = "llm"`.
#[cfg_attr(not(test), allow(dead_code))]
pub fn read_manifest_at(root: &Path) -> Option<AssetManifest> {
    let path = root.join("models/manifest.json");
    std::fs::read_to_string(&path).ok().and_then(|text| {
        match serde_json::from_str::<AssetManifest>(&text) {
            Ok(m) => Some(m),
            Err(e) => {
                tracing::warn!("Ignoring asset manifest ({e})");
                None
            }
        }
    })
}

/// The `MeshAssetRef` path for a manifest file — `models/<file>`, relative
/// to the assets root (what Bevy's asset server, the HTML viewer, and the
/// export copier all consume).
pub fn mesh_path(file: &str) -> String {
    format!("models/{file}")
}

// --- deterministic scatter math (ported from Verse) ------------------------

/// Deterministic scatter offsets for one `scatter_field` command: `count`
/// points on a uniform disk of `radius` (sqrt-distributed so the field
/// doesn't clump at the centre), y always 0. Seeded by the section key +
/// field name, so a cached build replays to the identical field.
pub fn scatter_offsets(seed: u64, count: usize, radius: f32) -> Vec<[f32; 3]> {
    let mut rng = seed | 1; // never zero (splitmix handles it, but stay odd)
    (0..count)
        .map(|_| {
            let ang = rand01(&mut rng) * std::f32::consts::TAU;
            let dist = rand01(&mut rng).sqrt() * radius.max(0.0);
            [ang.cos() * dist, 0.0, ang.sin() * dist]
        })
        .collect()
}

/// Deterministic 64-bit fold of a string — the seed source for
/// [`scatter_offsets`].
pub fn fold_seed(s: &str) -> u64 {
    s.bytes().fold(0xC0FFEE_u64, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as u64)
    })
}

pub(crate) fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Uniform f32 in [0, 1).
pub(crate) fn rand01(state: &mut u64) -> f32 {
    (splitmix(state) >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> AssetManifest {
        serde_json::from_str::<AssetManifest>(
            r#"{"version":2,"assets":[
                {"id":"a1","name":"Boulder","file":"boulder.glb","kind":"rock","tier":"hero",
                 "mood":0,"scale":1.0,"dims":[2.0,1.5,2.0],"license":"CC0","author":"x","source":"y"},
                {"id":"a2","name":"Pebble","file":"pebble.glb","kind":"rock","tier":"scatter",
                 "mood":1,"scale":1.0,"dims":[0.4,0.2,0.4],"license":"CC0","author":"x","source":"y"},
                {"id":"a3","name":"Palm","file":"palm.glb","kind":"tree","tier":"medium",
                 "mood":2,"scale":1.0,"dims":[4.0,8.0,4.0],"license":"CC0","author":"x","source":"y"}
            ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn kinds_in_first_appearance_order() {
        assert_eq!(manifest().kinds(), vec!["rock", "tree"]);
    }

    #[test]
    fn resolve_kind_rotates_then_restarts_deterministically() {
        let m = manifest();
        let mut used = Vec::new();
        let first = resolve_kind(&m, "rock", &mut used).unwrap().file.clone();
        let second = resolve_kind(&m, "rock", &mut used).unwrap().file.clone();
        let third = resolve_kind(&m, "rock", &mut used).unwrap().file.clone();
        assert_ne!(first, second);
        assert_eq!(third, first); // pool exhausted → restart from the top
        let mut again = Vec::new();
        assert_eq!(
            resolve_kind(&m, "rock", &mut again).unwrap().file,
            first,
            "same call sequence resolves identically"
        );
        assert!(resolve_kind(&m, "boat", &mut Vec::new()).is_none());
    }

    #[test]
    fn placement_scale_normalizes_to_target_span() {
        let m = manifest();
        let boulder = m.assets.iter().find(|a| a.file == "boulder.glb").unwrap();
        // span 2.0, hero target 7.0 → ×3.5
        assert!((boulder.placement_scale() - 3.5).abs() < 1e-5);
        let legacy = serde_json::from_str::<AssetEntry>(
            r#"{"name":"N","file":"n.glb","kind":"rock","tier":"medium",
                "mood":0,"scale":2.0,"license":"CC0","author":"x"}"#,
        )
        .unwrap();
        assert!((legacy.placement_scale() - 2.0 * 1.15).abs() < 1e-5); // no dims → legacy
    }

    #[test]
    fn scatter_offsets_are_deterministic_and_bounded() {
        let seed = fold_seed("abc|field");
        let a = scatter_offsets(seed, 8, 5.0);
        let b = scatter_offsets(seed, 8, 5.0);
        assert_eq!(a, b);
        assert!(a.iter().all(|p| p[1] == 0.0));
        assert!(a.iter().all(|p| (p[0].hypot(p[2])) <= 5.0));
        assert_ne!(a[0], a[1]);
    }

    #[test]
    fn read_manifest_at_parses_and_missing_is_none() {
        assert!(read_manifest_at(Path::new("/nonexistent/localgpt-md-assets")).is_none());
        let dir = std::env::temp_dir().join(format!(
            "localgpt-md-assets-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(
            dir.join("models/manifest.json"),
            r#"{"version":2,"assets":[{"name":"N","file":"n.glb","kind":"rock","tier":"hero","mood":0,"license":"CC0","author":"x"}]}"#,
        )
        .unwrap();
        let m = read_manifest_at(&dir).unwrap();
        assert_eq!(m.assets.len(), 1);
        assert_eq!(m.assets[0].placement_scale(), 1.5); // no dims → hero legacy
        std::fs::remove_dir_all(&dir).ok();
    }
}
