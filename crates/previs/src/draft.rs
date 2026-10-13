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

/// Why a screenplay didn't draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {
    /// The text isn't a screenplay in any encoding this draft reads:
    /// replacement characters or control bytes where a script has prose
    /// (binary dropped on the page, a PDF, UTF-16 read as UTF-8).
    NotText,
    /// No scene headings — nothing to stage.
    NoScenes,
    /// More scenes than the caller boards ([`draft_limited`]).
    TooManyScenes {
        /// The scenes the script has.
        scenes: usize,
        /// The caller's ceiling.
        limit: usize,
    },
}

impl std::fmt::Display for DraftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DraftError::NotText => write!(
                f,
                "that isn't a text screenplay — binary, or an encoding this draft can't read; export the script as Fountain or plain text"
            ),
            DraftError::NoScenes => {
                write!(
                    f,
                    "no scene headings found — a scene starts with INT. or EXT."
                )
            }
            DraftError::TooManyScenes { scenes, limit } => write!(
                f,
                "{scenes} scenes — this page boards up to {limit}; split the script, or run localgpt-previs on the whole thing"
            ),
        }
    }
}

impl std::error::Error for DraftError {}

/// Whether the text could be a screenplay at all. Binary dropped on the
/// page, a PDF, or UTF-16 read as UTF-8 arrives full of replacement
/// characters and control bytes, which a script never has; a stray one
/// or two (an old file's odd byte) is just a typo. The gate is a share:
/// a hundredth of the characters, and at least eight of them.
fn looks_like_text(source: &str) -> bool {
    let mut total = 0usize;
    let mut suspect = 0usize;
    for c in source.chars() {
        total += 1;
        if c == '\u{FFFD}' || (c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\u{0C}')) {
            suspect += 1;
        }
    }
    suspect < 8 || suspect * 100 < total
}

/// Draft a screenplay: the set, cast and coverage of every scene, the
/// shot list and the board — deterministic, a function of the text.
pub fn draft(source: &str, fallback_title: &str) -> Result<Draft, DraftError> {
    draft_inner(source, fallback_title, None)
}

/// [`draft`] with a ceiling on the scenes it will stage — the previs
/// page's guard. The scene count is known after the parse (cheap) and
/// checked before staging (the cost), so an outsized script costs a
/// parse, not a board.
pub fn draft_limited(
    source: &str,
    fallback_title: &str,
    max_scenes: usize,
) -> Result<Draft, DraftError> {
    draft_inner(source, fallback_title, Some(max_scenes))
}

fn draft_inner(
    source: &str,
    fallback_title: &str,
    max_scenes: Option<usize>,
) -> Result<Draft, DraftError> {
    if !looks_like_text(source) {
        return Err(DraftError::NotText);
    }
    let script = fountain::parse(source);
    let scenes = stage::scenes(&script);
    if scenes.is_empty() {
        return Err(DraftError::NoScenes);
    }
    if let Some(limit) = max_scenes
        && scenes.len() > limit
    {
        return Err(DraftError::TooManyScenes {
            scenes: scenes.len(),
            limit,
        });
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
            scene: scene.number.clone(),
            heading: scene.heading.clone(),
            package_dir: package_dir.clone(),
            files: package::package_files(&manifest),
            manifest: manifest.clone(),
            shots: scene_rows.clone(),
        });
        boards.push(SceneBoard {
            scene: scene.number.clone(),
            heading: scene.heading.clone(),
            package_dir,
            manifest,
            shots: scene_rows,
            covers,
        });
    }

    let shotlist_csv = shots::csv(&rows);
    let board_html = board::board_html(&title, &boards);
    Ok(Draft {
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
        assert!(matches!(
            draft("Just a line of prose.\n", "notes"),
            Err(DraftError::NoScenes)
        ));
    }

    #[test]
    fn a_numbered_script_keeps_its_numbers() {
        let d = draft(
            "INT. A - DAY #12#\n\nMAYA\nHi.\n\nINT. B - DAY #12A#\n\nKAI\nHi.\n",
            "x",
        )
        .unwrap();
        let dirs: Vec<&str> = d.scenes.iter().map(|s| s.package_dir.as_str()).collect();
        assert_eq!(dirs, ["scene-12.world", "scene-12A.world"]);
        let shots: Vec<&str> = d.rows.iter().map(|r| r.shot.as_str()).collect();
        assert_eq!(shots, ["12A", "12B", "12A-A", "12A-B"]);
        assert!(
            d.shotlist_csv.contains("\n12A,12A-A,WS,"),
            "{}",
            d.shotlist_csv
        );
        assert!(d.board_html.contains("Scene 12A &middot; INT. B - DAY"));
    }

    #[test]
    fn binary_is_refused_as_not_text() {
        let noise: String = (0..200)
            .map(|i| if i % 3 == 0 { '\u{FFFD}' } else { 'a' })
            .collect();
        assert!(matches!(draft(&noise, "x"), Err(DraftError::NotText)));
        // A stray odd byte is a script with a typo, not binary.
        let odd = "INT. A - DAY\n\nA line with one \u{FFFD} in it.\n";
        assert!(draft(odd, "x").is_ok());
    }

    #[test]
    fn the_scene_ceiling_refuses_before_staging() {
        let script = "INT. A - DAY\n\nINT. B - DAY\n\nINT. C - DAY\n";
        assert_eq!(
            draft_limited(script, "x", 2).err(),
            Some(DraftError::TooManyScenes {
                scenes: 3,
                limit: 2
            })
        );
        assert!(draft_limited(script, "x", 3).is_ok());
        assert_eq!(
            DraftError::TooManyScenes {
                scenes: 3,
                limit: 2
            }
            .to_string(),
            "3 scenes — this page boards up to 2; split the script, or run localgpt-previs on the whole thing"
        );
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
