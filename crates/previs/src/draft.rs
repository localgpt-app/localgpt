//! The whole draft, in memory: a screenplay in; every scene's `.world`
//! package files, the shot list and the printable board out. The CLI
//! writes what this returns to disk and the previs page hands it to the
//! browser, from one function — so the two can never drift, and the
//! page's draft is the CLI's draft, byte for byte.

use localgpt_world_types as wt;

use crate::board::{self, SceneBoard};
use crate::shots::{self, ShotRow};
use crate::{fountain, package, stage};

/// One scene of the draft: its staged world, its shots, and the files
/// its `.world` package is made of.
#[derive(Debug, Clone)]
pub struct SceneDraft {
    /// The scene's number as text (the shot prefix).
    pub scene: String,
    /// The heading as written (`INT. KITCHEN - DAY`).
    pub heading: String,
    /// The package directory name (`scene-1.world`).
    pub package_dir: String,
    /// The staged world.
    pub manifest: wt::WorldManifest,
    /// The scene's shots, in order.
    pub shots: Vec<ShotRow>,
    /// The package's files, path within the package → text
    /// ([`package::package_files`]).
    pub files: Vec<(String, String)>,
}

/// A screenplay's draft: the scenes, the shot list and the board.
#[derive(Debug, Clone)]
pub struct Draft {
    /// The title page's title, else the fallback the caller gave.
    pub title: String,
    /// The scenes, in script order.
    pub scenes: Vec<SceneDraft>,
    /// Every shot, in script order.
    pub rows: Vec<ShotRow>,
    /// `shotlist.csv`.
    pub shotlist_csv: String,
    /// `board.html`: one self-contained page.
    pub board_html: String,
}

/// Draft a screenplay: the set, cast and coverage of every scene, the
/// shot list and the board — deterministic, a function of the text.
/// `None` when the script has no scene headings (nothing to stage).
pub fn draft(source: &str, fallback_title: &str) -> Option<Draft> {
    let script = fountain::parse(source);
    let scenes = stage::scenes(&script);
    if scenes.is_empty() {
        return None;
    }
    let title = script
        .title_page
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("title"))
        .and_then(|(_, v)| v.first())
        .cloned()
        .unwrap_or_else(|| fallback_title.to_string());

    let mut drafts = Vec::with_capacity(scenes.len());
    let mut boards = Vec::with_capacity(scenes.len());
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
        rows.extend(scene_rows.iter().cloned());
        drafts.push(SceneDraft {
            scene: scene.number.to_string(),
            heading: scene.heading.clone(),
            package_dir: package_dir.clone(),
            files: package::package_files(&manifest),
            manifest: manifest.clone(),
            shots: scene_rows.clone(),
        });
        boards.push(SceneBoard {
            scene: scene.number.to_string(),
            heading: scene.heading.clone(),
            package_dir,
            manifest,
            shots: scene_rows,
            covers,
        });
    }

    let shotlist_csv = shots::csv(&rows);
    let board_html = board::board_html(&title, &boards);
    Some(Draft {
        title,
        scenes: drafts,
        rows,
        shotlist_csv,
        board_html,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../samples/cold-eggs.fountain");

    #[test]
    fn the_sample_drafts_to_two_scenes_seven_shots_and_a_board() {
        let d = draft(SAMPLE, "cold-eggs").expect("the sample has scenes");
        assert_eq!(d.title, "Cold Eggs", "the title page's title");
        assert_eq!(d.scenes.len(), 2);
        assert_eq!(d.rows.len(), 7);
        assert_eq!(d.scenes[0].shots.len(), 3);
        assert_eq!(d.scenes[1].shots.len(), 4);
        assert_eq!(d.scenes[0].package_dir, "scene-1.world");
        // Four files make a head-first package.
        let paths: Vec<&str> = d.scenes[0].files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            [
                "manifest.json",
                "ops.jsonl",
                "snapshots/base.json",
                "package.json"
            ]
        );
        assert!(d.shotlist_csv.starts_with("scene,shot,size,"));
        assert!(d.board_html.contains("Cold Eggs — previs board"));
    }

    #[test]
    fn a_script_without_scene_headings_is_nothing_to_stage() {
        assert!(draft("Just a line of prose.\n", "notes").is_none());
    }

    #[test]
    fn the_fallback_title_names_an_untitled_script() {
        let d = draft("INT. ROOM - DAY\n\nA chair.\n", "untitled-pages").unwrap();
        assert_eq!(d.title, "untitled-pages");
    }

    #[test]
    fn the_draft_is_deterministic() {
        let a = draft(SAMPLE, "x").unwrap();
        let b = draft(SAMPLE, "x").unwrap();
        assert_eq!(a.board_html, b.board_html);
        assert_eq!(a.shotlist_csv, b.shotlist_csv);
        for (x, y) in a.scenes.iter().zip(&b.scenes) {
            assert_eq!(x.files, y.files);
        }
    }
}
