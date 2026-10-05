//! The history rail: a live canvas's log, drawn for a person.
//!
//! A canvas holds a `.world` package open and agents change it from outside,
//! so until now the only way to move through its history was the API's
//! `POST /goto`. The rail puts the same thing in the window: the log as rows,
//! newest at the top, the entry the canvas shows highlighted (`>`), the head
//! — main's tip, what `manifest.json` holds — marked `@` as git writes HEAD,
//! and other branch ends `*`. Click a row to show that point; `[` and `]`
//! step back and forward through entries in the order they were written.
//! The base — the world before any entry — is the bottom row.
//!
//! The rail owns no history. It draws `live::LiveHistory`, a snapshot the
//! canvas publishes when something changes, and asks for seeks with
//! `live::SeekTo`, which the canvas serves through the same `seek` as the
//! API — so a person and an agent moving through history cannot behave
//! differently. A seek is a view move: nothing is written. An agent that
//! builds on the entry on screen (`"at"` the canvas's `current`) starts a
//! branch there, and the canvas follows it.
//!
//! Plain `bevy_ui`, deliberately: the rail is the first surface of the
//! editor-stack RFC that a person touches, and it should not drag egui into
//! a canvas that has none. Text is ASCII because the default font has no
//! glyphs for fancier markers.

use std::ops::Range;

use bevy::prelude::*;

use super::live::{HistoryRow, LiveHistory, SeekTo};

/// Most rows on screen at once; the rail windows around the current entry.
const MAX_ROWS: usize = 16;

const PANEL: Color = Color::srgba(0.04, 0.05, 0.08, 0.84);
const ROW: Color = Color::srgba(0.0, 0.0, 0.0, 0.0);
const ROW_HOVER: Color = Color::srgba(1.0, 1.0, 1.0, 0.08);
const ROW_CURRENT: Color = Color::srgba(0.22, 0.45, 0.85, 0.55);
const INK: Color = Color::srgb(0.92, 0.94, 0.97);
const INK_MUTED: Color = Color::srgb(0.58, 0.64, 0.72);
const REFUSED: Color = Color::srgb(0.95, 0.6, 0.35);

pub struct HistoryRailPlugin;

impl Plugin for HistoryRailPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_rail)
            .add_systems(Update, (redraw_rail, click_rows, step_keys));
    }
}

/// The rail's panel.
#[derive(Component)]
struct Rail;

/// A row, and the point in history it shows (`None`: the base).
#[derive(Component)]
struct RailRow {
    tip: Option<String>,
    current: bool,
}

fn spawn_rail(mut commands: Commands) {
    commands.spawn((
        Rail,
        Name::new("History rail"),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(12.0),
            right: Val::Px(12.0),
            // Wide enough for a full row at 13px; anything longer is
            // clipped on one line rather than wrapped onto two, because a
            // wrapped row reads as two entries.
            width: Val::Px(470.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(10.0)),
            row_gap: Val::Px(2.0),
            ..default()
        },
        BackgroundColor(PANEL),
        ZIndex(40),
    ));
}

fn redraw_rail(history: Res<LiveHistory>, rail: Query<Entity, With<Rail>>, mut commands: Commands) {
    if !history.is_changed() {
        return;
    }
    let Ok(rail) = rail.single() else {
        return;
    };
    let current = current_index(&history);
    let window = window(history.rows.len(), current, MAX_ROWS);
    let hidden_newer = history.rows.len() - window.end;
    let hidden_older = window.start;

    commands.entity(rail).despawn_children();
    commands.entity(rail).with_children(|panel| {
        panel.spawn(text(&title(&history), 14.0, INK));
        if let Some(status) = &history.status {
            let color = if status.starts_with("refused") {
                REFUSED
            } else {
                INK_MUTED
            };
            panel.spawn(text(&clip(status, 64), 12.0, color));
        }
        if hidden_newer > 0 {
            panel.spawn(text(&format!("  … {hidden_newer} newer"), 12.0, INK_MUTED));
        }
        // Newest at the top, the way a log reads.
        for i in window.clone().rev() {
            let row = &history.rows[i];
            let is_current = Some(i) == current;
            spawn_row(
                panel,
                Some(row.id.clone()),
                &row_label(row, is_current),
                is_current,
            );
        }
        if hidden_older > 0 {
            panel.spawn(text(&format!("  … {hidden_older} older"), 12.0, INK_MUTED));
        }
        // The base is a real place to stand: the world before any entry.
        let at_base = history.current.is_none();
        spawn_row(panel, None, &base_label(at_base), at_base);
        panel.spawn(text("[ ] step   click a row to show it", 11.0, INK_MUTED));
        panel.spawn(text(
            "click select  drag move  , . turn  - = scale  Del  Cmd+D copy  Cmd+Z undo",
            11.0,
            INK_MUTED,
        ));
    });
}

fn spawn_row(panel: &mut ChildSpawnerCommands, tip: Option<String>, label: &str, current: bool) {
    panel
        .spawn((
            Button,
            RailRow { tip, current },
            Node {
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                overflow: Overflow::clip_x(),
                ..default()
            },
            BackgroundColor(if current { ROW_CURRENT } else { ROW }),
        ))
        .with_children(|row| {
            row.spawn(text(label, 13.0, INK));
        });
}

fn text(s: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s.to_string()),
        TextLayout::default().with_no_wrap(),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

fn click_rows(
    mut rows: Query<(&Interaction, &RailRow, &mut BackgroundColor), Changed<Interaction>>,
    mut seek: MessageWriter<SeekTo>,
) {
    for (interaction, row, mut background) in &mut rows {
        match interaction {
            Interaction::Pressed => {
                seek.write(SeekTo(row.tip.clone()));
            }
            Interaction::Hovered if !row.current => background.0 = ROW_HOVER,
            _ => background.0 = if row.current { ROW_CURRENT } else { ROW },
        }
    }
}

fn step_keys(
    keys: Res<ButtonInput<KeyCode>>,
    history: Res<LiveHistory>,
    mut seek: MessageWriter<SeekTo>,
) {
    let by = if keys.just_pressed(KeyCode::BracketRight) {
        1
    } else if keys.just_pressed(KeyCode::BracketLeft) {
        -1
    } else {
        return;
    };
    if let Some(target) = step(&history, by) {
        seek.write(SeekTo(target));
    }
}

/// Where the current entry sits in file order; `None` at the base, or when
/// the current tip is somehow not a row.
fn current_index(history: &LiveHistory) -> Option<usize> {
    let current = history.current.as_deref()?;
    history.rows.iter().position(|r| r.id == current)
}

/// The rows to show: at most `max`, keeping `current` in view. With no
/// current entry (the base), the window sits at the oldest end, next to it.
fn window(len: usize, current: Option<usize>, max: usize) -> Range<usize> {
    if len <= max {
        return 0..len;
    }
    let anchor = current.unwrap_or(0);
    // Center on the current entry, then clamp into the log.
    let start = anchor.saturating_sub(max / 2).min(len - max);
    start..start + max
}

/// The seek `[` (-1) or `]` (+1) asks for, walking entries in file order with
/// the base before the first. `None` when there is nowhere to go; otherwise
/// the target, itself `None` for the base.
fn step(history: &LiveHistory, by: isize) -> Option<Option<String>> {
    // The base is position -1, so stepping back from the first entry lands
    // on it and stepping forward from it lands on the first entry.
    let here = current_index(history).map_or(-1, |i| i as isize);
    let there = here + by;
    if there < -1 || there >= history.rows.len() as isize {
        return None;
    }
    Some(if there == -1 {
        None
    } else {
        Some(history.rows[there as usize].id.clone())
    })
}

/// A short, recognisable form of an entry id: the first seven hex digits of
/// a content hash, as git shows commits; anything else (`line-3`) as is.
fn short_id(id: &str) -> String {
    match id.strip_prefix("sha256:") {
        Some(hex) => hex.chars().take(7).collect(),
        None => id.to_string(),
    }
}

/// Two marks, so being here never hides what the entry is: `>` where the
/// canvas is, then `@` for the head or `*` for another branch's end.
fn row_label(row: &HistoryRow, current: bool) -> String {
    let here = if current { '>' } else { ' ' };
    let kind = match (row.head, row.tip) {
        (true, _) => '@',
        (false, true) => '*',
        (false, false) => ' ',
    };
    format!(
        "{here}{kind} r{:<3} {:<8} {:<9} {}",
        row.revision,
        short_id(&row.id),
        clip(&row.author, 9),
        clip(&row.summary, 33)
    )
}

fn base_label(current: bool) -> String {
    format!("{}  -- the base --", if current { '>' } else { ' ' })
}

fn title(history: &LiveHistory) -> String {
    let tips = history.rows.iter().filter(|r| r.tip).count();
    let current = current_index(history);
    let head = history.rows.iter().position(|r| r.head);
    let at = match (current, head) {
        (Some(i), Some(h)) if i == h => format!("r{} (head)", history.rows[i].revision),
        (Some(i), Some(h)) => format!(
            "r{} · head r{}",
            history.rows[i].revision, history.rows[h].revision
        ),
        (Some(i), None) => format!("r{}", history.rows[i].revision),
        (None, Some(h)) => format!("the base · head r{}", history.rows[h].revision),
        (None, None) => "the base".to_string(),
    };
    match tips {
        0 | 1 => format!("History · {} entries · at {at}", history.rows.len()),
        n => format!(
            "History · {} entries · {n} branches · at {at}",
            history.rows.len()
        ),
    }
}

/// At most `n` characters, with a marker when cut. By `char`, so a name in
/// any script is cut on a character boundary.
fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
        out.push('~');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, revision: u64, tip: bool) -> HistoryRow {
        HistoryRow {
            id: id.into(),
            revision,
            author: "claude".into(),
            summary: format!("spawn #{revision}"),
            tip,
            head: false,
        }
    }

    fn head(mut row: HistoryRow) -> HistoryRow {
        row.head = true;
        row
    }

    /// A linear log: its one tip is the head.
    fn linear(n: u64, current: Option<&str>) -> LiveHistory {
        LiveHistory {
            rows: (1..=n)
                .map(|i| {
                    let r = row(&format!("e{i}"), i, i == n);
                    if i == n { head(r) } else { r }
                })
                .collect(),
            current: current.map(str::to_string),
            status: None,
        }
    }

    #[test]
    fn stepping_walks_file_order_with_the_base_before_the_first_entry() {
        let h = linear(3, Some("e2"));
        assert_eq!(step(&h, 1), Some(Some("e3".into())));
        assert_eq!(step(&h, -1), Some(Some("e1".into())));

        let at_first = linear(3, Some("e1"));
        assert_eq!(
            step(&at_first, -1),
            Some(None),
            "back from the first is the base"
        );

        let at_base = linear(3, None);
        assert_eq!(
            step(&at_base, 1),
            Some(Some("e1".into())),
            "forward from the base"
        );
        assert_eq!(step(&at_base, -1), None, "nothing before the base");

        let at_head = linear(3, Some("e3"));
        assert_eq!(step(&at_head, 1), None, "nothing after the newest");
    }

    #[test]
    fn an_empty_history_has_nowhere_to_step() {
        let empty = LiveHistory::default();
        assert_eq!(step(&empty, 1), None);
        assert_eq!(step(&empty, -1), None);
    }

    #[test]
    fn the_window_keeps_the_current_entry_in_view() {
        assert_eq!(window(5, Some(4), 16), 0..5, "a short log shows whole");
        // A long log centers on the current entry...
        let w = window(100, Some(50), 16);
        assert!(w.contains(&50), "{w:?}");
        assert_eq!(w.len(), 16);
        // ...and clamps at both ends.
        assert_eq!(window(100, Some(99), 16), 84..100);
        assert_eq!(window(100, Some(0), 16), 0..16);
        // At the base the window sits beside it, at the oldest end.
        assert_eq!(window(100, None, 16), 0..16);
    }

    #[test]
    fn a_content_hash_shortens_like_a_git_commit() {
        assert_eq!(short_id("sha256:9cc44aa944b674cd8a5c0cba"), "9cc44aa");
        assert_eq!(short_id("line-3"), "line-3");
        assert_eq!(short_id("e2"), "e2");
    }

    #[test]
    fn a_row_marks_where_the_canvas_is_and_where_branches_end() {
        assert!(row_label(&row("e1", 1, false), true).starts_with(">  "));
        assert!(row_label(&row("e2", 2, true), false).starts_with(" * "));
        assert!(row_label(&row("e3", 3, false), false).starts_with("   "));
        assert!(row_label(&head(row("e4", 4, true)), false).starts_with(" @ "));
        // Being here never hides what the entry is.
        assert!(row_label(&head(row("e4", 4, true)), true).starts_with(">@ "));
        assert!(row_label(&row("e5", 5, true), true).starts_with(">* "));
    }

    #[test]
    fn the_title_counts_branches_and_says_where_the_head_is() {
        assert_eq!(
            title(&linear(3, Some("e3"))),
            "History · 3 entries · at r3 (head)"
        );
        assert_eq!(
            title(&linear(3, Some("e1"))),
            "History · 3 entries · at r1 · head r3"
        );
        let forked = LiveHistory {
            rows: vec![
                row("e1", 1, false),
                row("e2", 2, true),
                head(row("e3", 3, true)),
            ],
            current: Some("e2".into()),
            status: None,
        };
        assert_eq!(
            title(&forked),
            "History · 3 entries · 2 branches · at r2 · head r3"
        );
        assert_eq!(
            title(&linear(2, None)),
            "History · 2 entries · at the base · head r2"
        );
    }

    #[test]
    fn clipping_cuts_on_characters_and_says_so() {
        assert_eq!(clip("claude", 9), "claude");
        assert_eq!(clip("a-very-long-author", 9), "a-very-l~");
        // A multi-byte name is cut on a character boundary, not a byte one.
        assert_eq!(clip("灯塔看守人和他的狗", 4), "灯塔看~");
    }
}
