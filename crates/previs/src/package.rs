//! Write a head-first `.world` package per scene (spec/package.md): the
//! world as it is now in `manifest.json`, an empty `ops.jsonl` (no
//! history yet), `snapshots/base.json` (the base is the manifest — a
//! package with no history), and a `package.json` naming the bytes.
//!
//! `manifest.json` is the crate's canonical text
//! ([`openworldformat::manifest_text_of`]), so one world is always the
//! same bytes and the `world_sha256` means something. Everything here
//! is deterministic — `updated_ms` stays 0 so the same screenplay
//! writes the same package, byte for byte.

use std::io;
use std::path::{Path, PathBuf};

use localgpt_world_types as wt;
use openworldformat::session::SessionMeta;

/// The files a package write produced.
#[derive(Debug, Clone)]
pub struct PackageFiles {
    /// `manifest.json`
    pub manifest: PathBuf,
    /// `package.json`
    pub package: PathBuf,
    /// `ops.jsonl`
    pub ops: PathBuf,
    /// `snapshots/base.json`
    pub base: PathBuf,
}

/// Write `manifest` as a head-first `.world` package under `dir`
/// (created, with `snapshots/`). Returns the files written.
pub fn write_package(dir: &Path, manifest: &wt::WorldManifest) -> io::Result<PackageFiles> {
    std::fs::create_dir_all(dir.join("snapshots"))?;

    let text = openworldformat::manifest_text_of(manifest);
    let manifest_path = dir.join(openworldformat::package::MANIFEST);
    std::fs::write(&manifest_path, &text)?;

    let ops_path = dir.join("ops.jsonl");
    std::fs::write(&ops_path, "")?;

    // Base revision 0, head revision 0: the base is the manifest.
    let base_path = dir.join(openworldformat::package::BASE_SNAPSHOT);
    std::fs::write(&base_path, &text)?;

    let mut meta = SessionMeta::new(manifest.meta.name.clone());
    meta.app = Some("localgpt-previs".to_string());
    meta.world_sha256 = Some(openworldformat::sha256_hex(text.as_bytes()));
    let package_path = dir.join("package.json");
    let mut package_json = serde_json::to_string_pretty(&meta)
        .map_err(io::Error::other)?;
    package_json.push('\n');
    std::fs::write(&package_path, package_json)?;

    Ok(PackageFiles {
        manifest: manifest_path,
        package: package_path,
        ops: ops_path,
        base: base_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "localgpt-previs-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn a_world() -> wt::WorldManifest {
        let script = crate::fountain::parse(
            "INT. KITCHEN - DAY\n\nMaya cooks.\n\nMAYA\nSit down and eat.\n",
        );
        crate::stage::stage(&crate::stage::scenes(&script)[0])
    }

    #[test]
    fn the_written_manifest_folds_back_to_itself() {
        let dir = temp_dir("fold");
        let manifest = a_world();
        let files = write_package(&dir, &manifest).unwrap();

        // package.json: format_version 2, name, base 0, head 0, and the
        // sha256 of the manifest's canonical bytes.
        let package: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&files.package).unwrap(),
        )
        .unwrap();
        assert_eq!(package["format_version"], 2);
        assert_eq!(package["name"], "scene-1");
        assert_eq!(package["base_revision"], 0);
        assert_eq!(package["head_revision"], 0);
        let manifest_bytes = std::fs::read(&files.manifest).unwrap();
        assert_eq!(
            package["world_sha256"].as_str().unwrap(),
            openworldformat::sha256_hex(&manifest_bytes)
        );
        assert_eq!(std::fs::read_to_string(&files.ops).unwrap(), "");

        // fold(m, []) == m: the openworldformat fold of an empty log is
        // the base unchanged. Compared as canonical text (the format's
        // own equality): the canonical form writes integral floats as
        // `1`, so a typed `==` on the untyped extra map would split
        // hairs the format itself doesn't.
        let read_back: wt::WorldManifest =
            serde_json::from_str(&String::from_utf8(manifest_bytes).unwrap()).unwrap();
        assert_eq!(
            openworldformat::manifest_text_of(&read_back),
            openworldformat::manifest_text_of(&manifest)
        );
        let doc = openworldformat::WorldDoc::from_manifest(&read_back).unwrap();
        let folded = openworldformat::fold_log(&doc, &[]).unwrap();
        let folded_manifest = folded.to_manifest();
        assert_eq!(
            openworldformat::manifest_text_of(&folded_manifest),
            openworldformat::manifest_text_of(&read_back)
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn writing_twice_is_byte_identical() {
        let dir_a = temp_dir("a");
        let dir_b = temp_dir("b");
        let manifest = a_world();
        let a = write_package(&dir_a, &manifest).unwrap();
        let b = write_package(&dir_b, &manifest).unwrap();
        for (x, y) in [
            (a.manifest, b.manifest),
            (a.package, b.package),
            (a.ops, b.ops),
            (a.base, b.base),
        ] {
            assert_eq!(std::fs::read(&x).unwrap(), std::fs::read(&y).unwrap());
        }
        let _ = std::fs::remove_dir_all(&dir_a);
        let _ = std::fs::remove_dir_all(&dir_b);
    }
}
