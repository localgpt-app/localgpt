//! LocalGPT Previs — script to board.
//!
//! Reads a Fountain screenplay and writes, per scene, a head-first
//! `.world` package with the deterministic draft (set, cast, coverage),
//! plus a `shotlist.csv` and a printable `board.html`. No model, no
//! network — the output is a function of the text.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use localgpt_previs::board::{self, SceneBoard};
use localgpt_previs::{fountain, package, shots, stage};

const USAGE: &str = "\
Usage: localgpt-previs <script.fountain> --out <dir>

Stages a Fountain screenplay as previs, deterministically (no model, no
network):

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
                out = Some(PathBuf::from(
                    args.next().ok_or("--out needs a directory")?,
                ));
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
    let script = fountain::parse(&source);
    let scenes = stage::scenes(&script);
    if scenes.is_empty() {
        eprintln!("{}: no scene headings found", args.script.display());
        return Err(std::io::Error::other("no scenes"));
    }

    std::fs::create_dir_all(&args.out)?;

    let title = script
        .title_page
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("title"))
        .and_then(|(_, v)| v.first())
        .cloned()
        .unwrap_or_else(|| {
            args.script
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Untitled".to_string())
        });

    let mut boards: Vec<SceneBoard> = Vec::new();
    let mut rows = Vec::new();
    for scene in &scenes {
        let manifest = stage::stage(scene);
        let scene_rows = shots::shot_list(&manifest);
        let (speakers, total_s) = stage::speakers(scene);

        // The lines each shot covers: the master's is the scene
        // summary; a single covers its speaker's lines.
        let mut covers = Vec::with_capacity(scene_rows.len());
        if !scene_rows.is_empty() {
            covers.push(vec![format!(
                "Whole scene — {} speaker{}, {:.1} s",
                speakers.len(),
                if speakers.len() == 1 { "" } else { "s" },
                total_s
            )]);
            for speaker in &speakers {
                covers.push(speaker.lines.clone());
            }
        }

        let package_dir = format!("scene-{}.world", scene.number);
        package::write_package(&args.out.join(&package_dir), &manifest)?;
        println!(
            "  {:<16} {} entities, {} shots ({})",
            format!("{package_dir}/"),
            manifest.entities.len(),
            scene_rows.len(),
            scene.heading
        );

        rows.extend(scene_rows.iter().cloned());
        boards.push(SceneBoard {
            scene: scene.number.to_string(),
            heading: scene.heading.clone(),
            package_dir,
            manifest,
            shots: scene_rows,
            covers,
        });
    }

    let csv_path = args.out.join("shotlist.csv");
    std::fs::write(&csv_path, shots::csv(&rows))?;

    let board_path = args.out.join("board.html");
    std::fs::write(&board_path, board::board_html(&title, &boards))?;

    println!(
        "{}: {} scene{}, {} shots → {}, {}, {}",
        title,
        scenes.len(),
        if scenes.len() == 1 { "" } else { "s" },
        rows.len(),
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
