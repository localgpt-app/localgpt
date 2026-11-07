//! `cargo xtask sync-viewer [--check]`: vendor the web renderer from the
//! `openworldformat` npm package (it was `scripts/sync-viewer.sh`).
//!
//! `crates/world-export/js/world-viewer.js` and `cinematography.js` are
//! verbatim snapshots of `openworldformat@$OWF_VERSION` (its `src/render.js`
//! and `src/cinematography.js`), never edited by hand. They are the one
//! copy the apps use: `html.rs` embeds them into exported HTML, the session
//! servers serve them, and both websites assemble from them. The renderer
//! imports `./cinematography.js`, so the two travel together and every
//! import map keys that relative specifier. The renderer's upstream is the
//! openworldformat repository: changes ship there, are released on npm,
//! and flow back through this command.
//!
//! The source is always the registry's tarball, never a sibling checkout:
//! what the apps embed is exactly what npm shipped. Bumping is deliberate:
//! raise `OWF_VERSION` below (or set `OWF_VERSION=x.y.z`), run this, then
//! `cargo test -p localgpt-world-export` and the websites' checks. CI runs
//! `--check` on every push, so a hand edit or a forgotten sync fails the
//! build instead of drifting.

use std::path::Path;
use std::process::Command;

pub const USAGE: &str = "  sync-viewer [--check]
                   vendor the web renderer from openworldformat on npm
                   (--check: fail if the vendored copy has drifted)";

const OWF_VERSION: &str = "0.3.6";
/// Upstream file, and its vendored name.
const FILES: [(&str, &str); 2] = [
    ("render.js", "world-viewer.js"),
    ("cinematography.js", "cinematography.js"),
];

/// What the embed needs of upstream, refused before anything is written.
fn refuse(render: &str, sources: &[(&str, String)]) -> Result<(), String> {
    if !render.contains("export function createWorldViewer") {
        return Err("refusing: upstream render.js no longer exports createWorldViewer".into());
    }
    if !render.contains("from './cinematography.js'") {
        return Err(
            "refusing: upstream render.js no longer imports './cinematography.js': the import maps key that specifier"
                .into(),
        );
    }
    for (name, text) in sources {
        if text.contains("</script") {
            return Err(format!(
                "refusing: upstream {name} contains '</script': it would break the inline embed"
            ));
        }
    }
    Ok(())
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let check = args.iter().any(|a| a == "--check");
    let version = std::env::var("OWF_VERSION").unwrap_or_else(|_| OWF_VERSION.into());
    let work = std::env::temp_dir().join(format!("xtask-viewer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let result = sync(root, &work, &version, check);
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn sync(root: &Path, work: &Path, version: &str, check: bool) -> Result<(), String> {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let package = format!("openworldformat@{version}");
    let packed = Command::new(npm)
        .args(["pack", &package, "--silent", "--pack-destination"])
        .arg(work)
        .output()
        .map_err(|e| format!("npm would not start (install node 22): {e}"))?;
    if !packed.status.success() {
        return Err(format!(
            "npm pack {package}: {}",
            String::from_utf8_lossy(&packed.stderr).trim()
        ));
    }
    let tarball = std::fs::read_dir(work)
        .map_err(|e| format!("{}: {e}", work.display()))?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "tgz"))
        .ok_or_else(|| format!("npm pack {package} left no tarball"))?;
    let mut tar = Command::new("tar");
    tar.arg("-xzf").arg(&tarball).arg("-C").arg(work);
    for (upstream, _) in FILES {
        tar.arg(format!("package/src/{upstream}"));
    }
    let untarred = tar
        .status()
        .map_err(|e| format!("tar would not start: {e}"))?;
    if !untarred.success() {
        return Err(format!("tar could not unpack {}", tarball.display()));
    }
    let mut sources = Vec::new();
    for (upstream, _) in FILES {
        let path = work.join("package/src").join(upstream);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        sources.push((upstream, text));
    }
    refuse(&sources[0].1, &sources)?;

    let dest = root.join("crates/world-export/js");
    if check {
        let mut drifted = false;
        for ((upstream, vendored), (_, text)) in FILES.iter().zip(&sources) {
            let path = dest.join(vendored);
            if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
                drifted = true;
                let _ = Command::new("diff")
                    .arg("-u")
                    .arg(&path)
                    .arg(work.join("package/src").join(upstream))
                    .status();
            }
        }
        if drifted {
            return Err(format!(
                "drifted from {package}: run cargo xtask sync-viewer"
            ));
        }
        println!("in sync with {package}");
        return Ok(());
    }
    for ((upstream, vendored), (_, text)) in FILES.iter().zip(&sources) {
        std::fs::write(dest.join(vendored), text).map_err(|e| format!("{vendored}: {e}"))?;
        println!("vendored {package} src/{upstream} -> crates/world-export/js/{vendored}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_upstream_that_breaks_the_embed_is_refused() {
        let good =
            "import { a } from './cinematography.js';\nexport function createWorldViewer() {}"
                .to_string();
        let ok = [
            ("render.js", good.clone()),
            ("cinematography.js", "export const b = 1;".into()),
        ];
        assert!(refuse(&good, &ok).is_ok());
        let script = [
            ("render.js", good.clone()),
            ("cinematography.js", "'</script>'".into()),
        ];
        assert!(refuse(&good, &script).is_err());
        let moved =
            "import { a } from './shot.js';\nexport function createWorldViewer() {}".to_string();
        assert!(refuse(&moved, &[("render.js", moved.clone())]).is_err());
    }
}
