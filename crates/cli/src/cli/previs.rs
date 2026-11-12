//! CLI subcommand: `localgpt previs` — script to board.
//!
//! Reads a screenplay — Fountain, a Final Draft `.fdx` file by the `fdx`
//! module's mapping, a PDF by the `pdf` module's reading of its page
//! layout, or Markdown script pages by the `markdown` module's
//! convention, picked by the file's extension (XML is Final Draft and
//! `%PDF-` a PDF whatever the name) — and writes, per scene, a head-first
//! `.world` package with the deterministic draft (set, cast, coverage),
//! plus a `shotlist.csv` and a printable `board.html`. No model, no
//! network — the output is a function of the file, and the same function
//! the localgpt.md page runs in the browser (`draft::draft_bytes`).

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Args;
use localgpt_previs::draft::{self, Format};

#[derive(Args, Debug)]
pub struct PrevisArgs {
    /// Screenplay file (.fountain, .fdx, .pdf, or .md script pages)
    #[arg(value_name = "SCRIPT")]
    pub script: PathBuf,

    /// Output directory for <scene>.world/, shotlist.csv, and board.html
    #[arg(long, value_name = "DIR")]
    pub out: PathBuf,
}

pub fn run(args: PrevisArgs) -> Result<()> {
    match draft_to_dir(&args.script, &args.out) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
            Ok(())
        }
        Err(err) => {
            eprintln!("localgpt previs: {err}");
            std::process::exit(1);
        }
    }
}

fn draft_to_dir(script: &Path, out: &Path) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(script).map_err(|e| format!("{}: {e}", script.display()))?;
    let file_name = script
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string());
    let fallback_title = Format::stem_of(&file_name).to_string();
    let drafted = draft::draft_bytes(&bytes, &file_name, &fallback_title)
        .map_err(|err| format!("{}: {err}", script.display()))?;

    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut lines = Vec::with_capacity(drafted.scenes.len() + 1);
    for scene in &drafted.scenes {
        let dir = out.join(&scene.package_dir);
        std::fs::create_dir_all(dir.join("snapshots"))
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        for (path, text) in &scene.files {
            std::fs::write(dir.join(path), text)
                .map_err(|e| format!("{}: {e}", dir.join(path).display()))?;
        }
        lines.push(format!(
            "  {:<16} {} entities, {} shots ({})",
            format!("{}/", scene.package_dir),
            scene.manifest.entities.len(),
            scene.shots.len(),
            scene.heading
        ));
    }
    std::fs::write(out.join("shotlist.csv"), &drafted.shotlist_csv)
        .map_err(|e| format!("{}: {e}", out.join("shotlist.csv").display()))?;
    std::fs::write(out.join("board.html"), &drafted.board_html)
        .map_err(|e| format!("{}: {e}", out.join("board.html").display()))?;

    lines.push(format!(
        "{}: {} scene{}, {} shots → {}, {}, {}",
        drafted.title,
        drafted.scenes.len(),
        if drafted.scenes.len() == 1 { "" } else { "s" },
        drafted.rows.len(),
        display_path(out, "*.world/"),
        display_path(out, "shotlist.csv"),
        display_path(out, "board.html"),
    ));
    Ok(lines)
}

fn display_path(base: &Path, name: &str) -> String {
    format!("{}/{}", base.display(), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_a_screenplay_to_world_packages_shotlist_and_board() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("cold-eggs.fountain");
        std::fs::write(
            &script,
            "Title: Cold Eggs\n\nINT. KITCHEN - DAY\n\nMAYA\nCoffee's cold.\n",
        )
        .unwrap();
        let out = tmp.path().join("previs");
        let lines = draft_to_dir(&script, &out).unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("scene-1.world/"));
        assert!(lines[1].starts_with("Cold Eggs: 1 scene, 2 shots → "));
        assert!(out.join("scene-1.world/manifest.json").is_file());
        assert!(out.join("scene-1.world/ops.jsonl").is_file());
        assert!(out.join("scene-1.world/snapshots/base.json").is_file());
        assert!(out.join("scene-1.world/package.json").is_file());
        assert!(out.join("shotlist.csv").is_file());
        assert!(out.join("board.html").is_file());
    }

    #[test]
    fn refuses_a_file_with_no_scenes_naming_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("notes.fountain");
        std::fs::write(&script, "Just prose, no scene headings.\n").unwrap();
        let out = tmp.path().join("previs");
        let err = draft_to_dir(&script, &out).unwrap_err();
        assert!(err.contains("notes.fountain"), "{err}");
        assert!(err.contains("no scene headings found"), "{err}");
    }
}
