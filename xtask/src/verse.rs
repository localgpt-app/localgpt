//! Verse's own tooling: the CLAP models its `ml` tier reads, and a portable
//! bundle of the app with its assets beside it (they were
//! `crates/verse/scripts/fetch-clap.sh` and `bundle.sh`). The Demucs stem
//! model needs no fetch: `stem-splitter-core` downloads and caches it on
//! first use.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const FETCH_CLAP_USAGE: &str =
    "  fetch-clap       the CLAP models Verse's ml tier reads (~207 MB, Xenova's ONNX
                   export of LAION's Apache-2.0 weights)";
pub const BUNDLE_USAGE: &str =
    "  verse-bundle     a portable Verse: the release binary with assets/ beside it,
                   in crates/verse/dist";

/// LAION CLAP (larger_clap_music_and_speech), ONNX export by Xenova. LAION's
/// model card gives the weights Apache-2.0; the export states no licence of
/// its own (Verse's PLAN.md §4).
const CLAP: &str = "https://huggingface.co/Xenova/larger_clap_music_and_speech/resolve/main";
/// Remote path, and the file under `crates/verse/assets/ml/`: the audio
/// model (moods, track embeddings), and the text model and tokenizer the
/// asset ranking embeds the manifest with.
const CLAP_FILES: [(&str, &str); 3] = [
    (
        "onnx/audio_model_quantized.onnx",
        "audio_model_quantized.onnx",
    ),
    (
        "onnx/text_model_quantized.onnx",
        "text_model_quantized.onnx",
    ),
    ("tokenizer.json", "tokenizer.json"),
];

/// `cargo xtask fetch-clap`. The app runs without them (rule-based moods).
pub fn fetch_clap(root: &Path) -> Result<(), String> {
    let out = root.join("crates/verse/assets/ml");
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    for (remote, file) in CLAP_FILES {
        let dest = out.join(file);
        if std::fs::metadata(&dest).is_ok_and(|m| m.len() > 0) {
            println!("  have {}", dest.display());
            continue;
        }
        println!("  fetching {remote}");
        let status = Command::new("curl")
            .args(["-sL", "--fail", "-o"])
            .arg(&dest)
            .arg(format!("{CLAP}/{remote}"))
            .status()
            .map_err(|e| format!("curl would not start: {e}"))?;
        if !status.success() {
            let _ = std::fs::remove_file(&dest);
            return Err(format!("{remote}: the download failed ({status})"));
        }
    }
    println!("done -> {}", out.display());
    Ok(())
}

/// `cargo xtask verse-bundle`: `crates/verse/dist/` with the release binary
/// and `assets/` beside it (fonts, the models the pack's manifest names,
/// the starter tracks), portable and launchable from anywhere, since the
/// binary finds its assets from its own folder. A macOS `.app` puts
/// `assets/` in `Contents/Resources/`; a packager wanting a third layout
/// sets `VERSE_ASSET_ROOT`. The assets come from `VERSE_ASSETS`, else a
/// localgpt-world-assets checkout.
pub fn bundle(root: &Path) -> Result<(), String> {
    let assets = std::env::var_os("VERSE_ASSETS")
        .map(PathBuf::from)
        .or_else(|| {
            crate::assets::checkouts(root)
                .into_iter()
                .find(|dir| dir.is_dir())
        });
    println!("==> release build");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let built = Command::new(cargo)
        .args(["build", "--release", "-p", "localgpt-verse"])
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo would not start: {e}"))?;
    if !built.success() {
        return Err("the release build FAILED".into());
    }
    println!("==> assembling dist/");
    let krate = root.join("crates/verse");
    let dist = krate.join("dist");
    let _ = std::fs::remove_dir_all(&dist);
    let into = dist.join("assets");
    std::fs::create_dir_all(&into).map_err(|e| format!("{}: {e}", into.display()))?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let binary = target.join("release/localgpt-verse");
    std::fs::copy(&binary, dist.join("localgpt-verse"))
        .map_err(|e| format!("{}: {e}", binary.display()))?;
    copy_tree(&krate.join("assets/fonts"), &into.join("fonts"))?;
    match assets.filter(|dir| dir.join("models/manifest.json").is_file()) {
        Some(source) => {
            // Only the models the manifest names (the packed .glb files),
            // not the originals' trees; and the CC0 starter tracks.
            let report = crate::assets::fetch_into(&source, &into)?;
            println!("    bundled the 3D pack from {}", source.display());
            match report {
                Some(tracks) => println!("    bundled {tracks} starter files"),
                None => println!("    warn: no music/ there: shipping without starter music"),
            }
        }
        None => println!(
            "    warn: no assets checkout with models/: bundling without the 3D pack (procedural worlds)"
        ),
    }
    let _ = Command::new("du").arg("-sh").arg(&dist).status();
    println!(
        "done: {} (run dist/localgpt-verse from anywhere)",
        dist.display()
    );
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .flatten()
    {
        let (path, target) = (entry.path(), to.join(entry.file_name()));
        if path.is_dir() {
            copy_tree(&path, &target)?;
        } else {
            std::fs::copy(&path, &target).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(())
}
