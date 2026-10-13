//! LocalGPT Previs — script to board.
//!
//! Reads a screenplay — Fountain, or Markdown script pages by the
//! `markdown` module's convention, picked by the file's extension — and
//! writes, per scene, a head-first `.world` package with the
//! deterministic draft (set, cast, coverage), plus a `shotlist.csv` and
//! a printable `board.html`. No model, no network — the output is a
//! function of the text, and the same function the localgpt.md page
//! runs in the browser (`draft::draft_as`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use localgpt_previs::draft::{self, Format};

const USAGE: &str = "\
Usage: localgpt-previs <script.fountain | pages.md> --out <dir>

Stages a screenplay as previs, deterministically (no model, no network).
A .fountain (or .txt) file is read as Fountain; a .md file as Markdown
script pages — a heading that reads INT./EXT. is the scene, **NAME:**
opens dialogue, front matter is the title page, Fountain inside passes
through.

  <dir>/<scene>.world/   one head-first package per scene: the set
                         (room or location), the cast (a stand-in per
                         speaking character) and the coverage (a master
                         plus one single per speaker) as
                         ext-cinematography cameras
  <dir>/shotlist.csv     the shot list: scene, shot, size, focal length,
                         sensor, aspect, height, distance, in, out,
                         description
  <dir>/board.html       a printable grid of shot cards, one file,
                         offline: the vendored Open World Format viewer
                         (inlined, three.js as import-map data URLs)
                         renders each shot's frame into its card
";

struct Args {
    script: PathBuf,
    out: PathBuf,
}

fn parse_args() -> Result<Args, String> {
    let mut script: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--out" => {
                out = Some(PathBuf::from(args.next().ok_or("--out needs a directory")?));
            }
            "--help" | "-h" => return Err(String::new()),
            _ if arg.starts_with("--out=") => {
                out = Some(PathBuf::from(&arg["--out=".len()..]));
            }
            _ if arg.starts_with('-') => return Err(format!("unknown flag {arg}")),
            _ => {
                if script.replace(PathBuf::from(&arg)).is_some() {
                    return Err(format!("one script at a time ({arg})"));
                }
            }
        }
    }
    match (script, out) {
        (Some(script), Some(out)) => Ok(Args { script, out }),
        _ => Err("a script and --out are both required".to_string()),
    }
}

fn run(args: &Args) -> std::io::Result<()> {
    let source = std::fs::read_to_string(&args.script)?;
    let file_name = args
        .script
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string());
    let format = Format::of_name(&file_name);
    let fallback_title = Format::stem_of(&file_name).to_string();
    let drafted = match draft::draft_as(&source, format, &fallback_title) {
        Ok(drafted) => drafted,
        Err(err) => {
            return Err(std::io::Error::other(format!(
                "{}: {err}",
                args.script.display()
            )));
        }
    };

    std::fs::create_dir_all(&args.out)?;
    for scene in &drafted.scenes {
        let dir = args.out.join(&scene.package_dir);
        std::fs::create_dir_all(dir.join("snapshots"))?;
        for (path, text) in &scene.files {
            std::fs::write(dir.join(path), text)?;
        }
        println!(
            "  {:<16} {} entities, {} shots ({})",
            format!("{}/", scene.package_dir),
            scene.manifest.entities.len(),
            scene.shots.len(),
            scene.heading
        );
    }
    std::fs::write(args.out.join("shotlist.csv"), &drafted.shotlist_csv)?;
    std::fs::write(args.out.join("board.html"), &drafted.board_html)?;

    println!(
        "{}: {} scene{}, {} shots → {}, {}, {}",
        drafted.title,
        drafted.scenes.len(),
        if drafted.scenes.len() == 1 { "" } else { "s" },
        drafted.rows.len(),
        display_path(&args.out, "*.world/"),
        display_path(&args.out, "shotlist.csv"),
        display_path(&args.out, "board.html"),
    );
    Ok(())
}

fn display_path(base: &Path, name: &str) -> String {
    format!("{}/{}", base.display(), name)
}

fn main() -> ExitCode {
    match parse_args() {
        Ok(args) => match run(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("localgpt-previs: {err}");
                ExitCode::FAILURE
            }
        },
        Err(err) => {
            if !err.is_empty() {
                eprintln!("localgpt-previs: {err}\n");
            }
            eprint!("{USAGE}");
            ExitCode::from(if err.is_empty() { 0 } else { 2 })
        }
    }
}
