//! The sidecar (PLAN.md M2, v2 in M3): LLM output cached per section hash in
//! a JSON file next to the document — `<doc>.world.json`. Renderer-free (the
//! only Bevy touch is the [`Resource`] derive that lets systems share one
//! live copy).
//!
//! Lockfile semantics: the key is the BLAKE3 hash of the section's heading +
//! body (+ any ```` ```world ```` fence), so an unchanged section is never
//! regenerated, editing one section invalidates only its own entry, and a
//! shared `.md` plus its sidecar renders identically on any machine (with or
//! without the `llm` feature — the default build *applies* cached output, it
//! just can't author it). Entries for sections no longer in the document are
//! pruned on save.
//!
//! v2 (M3) adds `builds`: per-section agent output as plain
//! `localgpt-world-types` entities in platform-local coordinates, with the
//! model that authored them. v1 files (recipes only) are rejected with a
//! warn — regenerate with `--generate`.

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use localgpt_world_types as wt;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::doc::Doc;
use crate::recipe::RegionRecipe;

/// The sidecar schema version. Bump on incompatible changes; loaders reject
/// other versions rather than guessing.
const VERSION: u32 = 2;

/// One cached agent build: the section's entities in platform-local
/// coordinates (ids are local placeholders — `draft::compile_with`
/// renumbers them into the section's id band) plus provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildEntry {
    /// The GGUF file name that authored this build (PLAN.md M2's open note).
    pub model: String,
    /// The model's closing description of the place (logged, and there for
    /// future UI surfacing).
    #[serde(default)]
    pub description: Option<String>,
    pub entities: Vec<wt::WorldEntity>,
}

/// Cached LLM output for a document, keyed by section-hash hex, backed by a
/// file. A Bevy [`Resource`] so the world, watcher, and generation worker
/// share one live copy; the module itself stays renderer-free.
///
/// Most methods are called by the `llm` feature's worker or `--generate`;
/// they stay compiled in the default build (Verse's `tier.rs` precedent) so
/// the tests proving the cache contract run without the model, and so the
/// default build still *applies* a cached sidecar.
#[derive(Debug, Default)]
#[cfg_attr(feature = "app", derive(bevy::prelude::Resource))]
pub struct RecipeStore {
    /// The sidecar file; empty for an in-memory store (tests, `--print-ron`
    /// without a sidecar).
    #[allow(dead_code)]
    path: PathBuf,
    recipes: BTreeMap<String, RegionRecipe>,
    builds: BTreeMap<String, BuildEntry>,
}

#[derive(Serialize, Deserialize)]
struct SidecarFile {
    version: u32,
    #[serde(default)]
    generator: String,
    #[serde(default)]
    recipes: BTreeMap<String, RegionRecipe>,
    #[serde(default)]
    builds: BTreeMap<String, BuildEntry>,
}

#[allow(dead_code)] // see the type docs: the writers live behind `llm`
impl RecipeStore {
    /// An empty store with no file behind it.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Load the sidecar next to `doc`. A missing file is an empty store; a
    /// corrupt, future-version, or v1 file warns and stays empty (the next
    /// save replaces it). Recipes are clamped on load, so a hand-edited
    /// sidecar can't push out-of-range values into the world either; build
    /// entities are trusted to the interpreter's clamps plus the placement
    /// clamp `draft` applies.
    pub fn load(path: &Path) -> Self {
        let mut store = Self {
            path: path.to_path_buf(),
            recipes: BTreeMap::new(),
            builds: BTreeMap::new(),
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return store;
        };
        match serde_json::from_str::<SidecarFile>(&text) {
            Ok(file) if file.version == VERSION => {
                store.recipes = file
                    .recipes
                    .into_iter()
                    .map(|(hash, recipe)| (hash, recipe.clamped()))
                    .collect();
                store.builds = file.builds;
            }
            Ok(file) if file.version == 1 => {
                warn!(
                    "sidecar {}: version 1 (recipes only) is superseded by v2 \
                     (agent builds) — ignoring it; regenerate with --generate",
                    path.display()
                );
            }
            Ok(file) => {
                warn!(
                    "sidecar {}: version {} != {VERSION} — ignoring it",
                    path.display(),
                    file.version
                );
            }
            Err(err) => {
                warn!(
                    "sidecar {}: can't parse ({err}) — ignoring it",
                    path.display()
                );
            }
        }
        store
    }

    /// The recipe for a section, if authored (and still valid for its text).
    pub fn get(&self, hash: &blake3::Hash) -> Option<&RegionRecipe> {
        self.recipes.get(&key(hash))
    }

    /// Cache a recipe (already clamped by the generator; clamped again here —
    /// cheap, and makes the store safe regardless of caller).
    pub fn insert(&mut self, hash: &blake3::Hash, recipe: RegionRecipe) {
        self.recipes.insert(key(hash), recipe.clamped());
    }

    /// The agent build for a section, if one was authored.
    pub fn get_build(&self, hash: &blake3::Hash) -> Option<&BuildEntry> {
        self.builds.get(&key(hash))
    }

    /// Cache an agent build.
    pub fn insert_build(&mut self, hash: &blake3::Hash, entry: BuildEntry) {
        self.builds.insert(key(hash), entry);
    }

    /// How many cached outputs (recipes + builds) the store holds.
    pub fn len(&self) -> usize {
        self.recipes.len() + self.builds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Write the sidecar, pruned to the document's current sections, atomically
    /// (temp file + rename) so a half-written sidecar can't replace a good one.
    pub fn save(&self, doc: &Doc) -> io::Result<()> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        let live: HashSet<String> = doc.sections.iter().map(|s| key(&s.hash)).collect();
        let file = SidecarFile {
            version: VERSION,
            generator: "localgpt-md".into(),
            recipes: pruned(&self.recipes, &live),
            builds: pruned(&self.builds, &live),
        };
        let text = serde_json::to_string_pretty(&file)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)
    }
}

fn key(hash: &blake3::Hash) -> String {
    hash.to_hex().to_string()
}

/// The entries whose keys are still live sections.
fn pruned<V: Clone>(map: &BTreeMap<String, V>, live: &HashSet<String>) -> BTreeMap<String, V> {
    map.iter()
        .filter(|(hash, _)| live.contains(*hash))
        .map(|(hash, value)| (hash.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Doc;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "localgpt-md-sidecar-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn hello_doc() -> Doc {
        Doc::parse(include_str!("../samples/hello.md"), "hello")
    }

    fn build_entry(entities: usize) -> BuildEntry {
        BuildEntry {
            model: "some-model.gguf".into(),
            description: Some("a quiet clearing".into()),
            entities: (1..=entities)
                .map(|i| wt::WorldEntity::new(i as u64, format!("e{i}")))
                .collect(),
        }
    }

    #[test]
    fn round_trip_recipes_and_builds_through_the_file() {
        let path = temp_path("roundtrip").with_extension("world.json");
        let doc = hello_doc();
        let mut store = RecipeStore::load(&path);
        store.insert(
            &doc.sections[0].hash,
            RegionRecipe {
                accent: Some([0.1, 0.2, 0.3]),
                ..Default::default()
            },
        );
        store.insert_build(&doc.sections[1].hash, build_entry(3));
        store.save(&doc).unwrap();

        let reloaded = RecipeStore::load(&path);
        assert_eq!(
            reloaded.get(&doc.sections[0].hash).unwrap().accent,
            Some([0.1, 0.2, 0.3])
        );
        let build = reloaded.get_build(&doc.sections[1].hash).unwrap();
        assert_eq!(build.model, "some-model.gguf");
        assert_eq!(build.entities.len(), 3);
        assert_eq!(build.description.as_deref(), Some("a quiet clearing"));
        assert!(reloaded.get_build(&doc.sections[0].hash).is_none());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn save_prunes_sections_no_longer_in_the_doc() {
        let path = temp_path("prune").with_extension("world.json");
        let doc = hello_doc();
        let mut store = RecipeStore::load(&path);
        for section in &doc.sections {
            store.insert(&section.hash, RegionRecipe::default());
            store.insert_build(&section.hash, build_entry(1));
        }
        // Entries for sections that no longer exist.
        let gone = blake3::hash(b"gone");
        store.insert(&gone, RegionRecipe::default());
        store.insert_build(&gone, build_entry(1));
        store.save(&doc).unwrap();
        let reloaded = RecipeStore::load(&path);
        assert!(reloaded.get(&gone).is_none());
        assert!(reloaded.get_build(&gone).is_none());
        assert_eq!(reloaded.len(), doc.sections.len() * 2);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn missing_and_corrupt_files_load_empty() {
        assert!(RecipeStore::load(Path::new("/nonexistent/x.world.json")).is_empty());
        let path = temp_path("corrupt").with_extension("world.json");
        std::fs::write(&path, "{\"version\":2,\"recipes\":").unwrap();
        assert!(RecipeStore::load(&path).is_empty());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn v1_and_future_versions_are_rejected_not_guessed() {
        let path = temp_path("v1").with_extension("world.json");
        std::fs::write(
            &path,
            r#"{"version":1,"recipes":{"00":{"accent":[1,1,1]}}}"#,
        )
        .unwrap();
        assert!(RecipeStore::load(&path).is_empty());
        std::fs::write(
            &path,
            r#"{"version":99,"recipes":{"00":{"accent":[1,1,1]}}}"#,
        )
        .unwrap();
        assert!(RecipeStore::load(&path).is_empty());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_clamps_hand_edited_values() {
        let path = temp_path("hand").with_extension("world.json");
        let doc = hello_doc();
        let k = key(&doc.sections[0].hash);
        std::fs::write(
            &path,
            format!(r#"{{"version":2,"recipes":{{"{k}":{{"props":{{"count":9999}}}}}}}}"#),
        )
        .unwrap();
        assert_eq!(
            RecipeStore::load(&path)
                .get(&doc.sections[0].hash)
                .unwrap()
                .props
                .map(|p| p.count),
            Some(12)
        );
        std::fs::remove_file(&path).ok();
    }
}
