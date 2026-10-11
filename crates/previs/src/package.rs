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

/// The files a head-first package of `manifest` is made of, path within
/// the package → text, in write order: `manifest.json` (the canonical
/// text), an empty `ops.jsonl`, `snapshots/base.json` (base revision 0,
/// head revision 0: the base is the manifest) and `package.json` naming
/// the bytes. Pure — the previs page builds its download from it in the
/// browser; [`write_package`] writes it.
pub fn package_files(manifest: &wt::WorldManifest) -> Vec<(String, String)> {
    let text = openworldformat::manifest_text_of(manifest);
    let mut meta = SessionMeta::new(manifest.meta.name.clone());
    meta.app = Some("localgpt-previs".to_string());
    meta.world_sha256 = Some(openworldformat::sha256_hex(text.as_bytes()));
    let mut package_json = serde_json::to_string_pretty(&meta).expect("a session meta serializes");
    package_json.push('\n');
    vec![
        (openworldformat::package::MANIFEST.to_string(), text.clone()),
        ("ops.jsonl".to_string(), String::new()),
        (openworldformat::package::BASE_SNAPSHOT.to_string(), text),
        ("package.json".to_string(), package_json),
    ]
}

/// Write `manifest` as a head-first `.world` package under `dir`
/// (created, with `snapshots/`). Returns the files written.
pub fn write_package(dir: &Path, manifest: &wt::WorldManifest) -> io::Result<PackageFiles> {
    std::fs::create_dir_all(dir.join("snapshots"))?;
    for (path, text) in package_files(manifest) {
        std::fs::write(dir.join(path), text)?;
    }
    Ok(PackageFiles {
        manifest: dir.join(openworldformat::package::MANIFEST),
        package: dir.join("package.json"),
        ops: dir.join("ops.jsonl"),
        base: dir.join(openworldformat::package::BASE_SNAPSHOT),
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
        let package: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&files.package).unwrap()).unwrap();
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
