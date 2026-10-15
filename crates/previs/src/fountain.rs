//! A Fountain screenplay parser (`spec`-less but faithful: the elements
//! the staging draft reads, at <https://fountain.io> syntax).
//!
//! Line-oriented and deterministic: the same bytes always parse to the
//! same [`Script`]. Supported: title pages (`Key: value` blocks at the
//! head of the file), scene headings (`INT.`/`EXT.`/`EST.`/`INT./EXT.`/
//! `INT/EXT.`/`I/E`, plus the `.`-forced form — a single period then an
//! alphanumeric, so an ellipsis stays action — with the shooting
//! script's trailing `#12A#` scene number lifted off the text), action
//! (and the `!`-forced form, for a line that would otherwise read as a
//! cue, a heading or a transition), character cues with extensions
//! (`(V.O.)`, `(O.S.)`, `(CONT'D)`) and the `@`-forced form (any case,
//! any marks: `@McCLANE`), parentheticals, dialogue, dual dialogue (`^`),
//! transitions (`TO:` and the `>`-forced form), centered text (`>…<`),
//! notes (`[[ … ]]`), the boneyard (`/* … */`), sections (`#`), synopses
//! (`=`) and page breaks (`===`).
//!
//! Notes and boneyard blocks are recognized when they open at the start
//! of a line (both may span lines); a `[[ … ]]` that opens mid-line is
//! left in the text, which is where screenplays put them least.
//!
//! A leading byte-order mark (what Windows editors write) is skipped.

/// One parsed screenplay: the title page entries in order, then the body
/// elements in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Script {
    /// Title page entries: `(key, value lines)` in file order. Keys keep
    /// their source casing ("Title", "Draft date").
    pub title_page: Vec<(String, Vec<String>)>,
    /// The body, element by element.
    pub elements: Vec<Element>,
}

/// One body element.
#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    /// `INT. KITCHEN - DAY`, or the forced `.Kitchen` form.
    SceneHeading {
        /// The heading text, trimmed: as written for the standard forms,
        /// without the forcing dot for the forced one, and without the
        /// scene number.
        text: String,
        /// True when the heading was forced with a leading `.`.
        forced: bool,
        /// The shooting script's scene number when the heading ends
        /// with one — `INT. KITCHEN - DAY #12A#` carries `12A`
        /// (alphanumerics, dashes and periods between `#`s).
        number: Option<String>,
    },
    /// A paragraph of action; the source's line breaks are kept (the
    /// page-time estimate bills a second a line).
    Action {
        /// The paragraph's lines.
        lines: Vec<String>,
    },
    /// A character cue: `MAYA (V.O.)`, `MAYA ^` for dual dialogue.
    Character {
        /// The character's name (extension and `^` stripped).
        name: String,
        /// The parenthesized extension, e.g. `V.O.`, when present.
        extension: Option<String>,
        /// True for the right-hand half of a dual-dialogue pair.
        dual: bool,
    },
    /// A parenthetical inside a dialogue block: `(beat)`.
    Parenthetical {
        /// The text without the parentheses.
        text: String,
    },
    /// A line (or wrapped lines) of dialogue.
    Dialogue {
        /// The spoken text.
        text: String,
    },
    /// `CUT TO:`, or the forced `>DISSOLVE TO:` form.
    Transition {
        /// The transition text (`>` stripped for the forced form).
        text: String,
        /// True when forced with a leading `>`.
        forced: bool,
    },
    /// Centered text: `>THE END<`.
    Centered {
        /// The text between `>` and `<`, trimmed.
        text: String,
    },
    /// A note: `[[ remember the cat ]]` (may span lines).
    Note {
        /// The note text between the brackets, trimmed.
        text: String,
    },
    /// A boneyard block: `/* … */` (may span lines).
    Boneyard {
        /// The elided text between the markers, trimmed.
        text: String,
    },
    /// A section marker: `# Act One` (level = number of `#`).
    Section {
        /// The section text.
        text: String,
        /// The heading depth (1–6).
        level: usize,
    },
    /// A synopsis: `= the crew regroups`.
    Synopsis {
        /// The synopsis text.
        text: String,
    },
    /// A page break: a line of three or more `=`.
    PageBreak,
}

/// The title page keys Fountain recognizes, lowercased for comparison.
pub(crate) const TITLE_KEYS: &[&str] = &[
    "title",
    "credit",
    "author",
    "authors",
    "source",
    "draft date",
    "contact",
    "copyright",
    "notes",
    "date",
    "revision",
];

/// Parse a Fountain screenplay.
pub fn parse(source: &str) -> Script {
    // A byte-order mark would hide the title page's first key.
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);
    let lines: Vec<&str> = source.lines().collect();
    let mut script = Script::default();
    let mut i = skip_blanks(&lines, 0);

    i = parse_title_page(&lines, i, &mut script);

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // Boneyard: `/*` opens, `*/` closes (possibly lines later).
        if let Some(rest) = trimmed.strip_prefix("/*") {
            let (text, next) = take_until(&lines, i, rest, "*/");
            script.elements.push(Element::Boneyard { text });
            i = next;
            continue;
        }
        // Note: `[[` opens, `]]` closes.
        if let Some(rest) = trimmed.strip_prefix("[[") {
            let (text, next) = take_until(&lines, i, rest, "]]");
            script.elements.push(Element::Note { text });
            i = next;
            continue;
        }
        // Forced action: a leading `!` — Fountain's escape for a line
        // that would otherwise read as a cue, a heading or a
        // transition. The paragraph runs on from it as any action does.
        if let Some(rest) = trimmed.strip_prefix('!') {
            let (lines, next) = action_paragraph(&lines, i, Some(rest.trim_start()));
            script.elements.push(Element::Action { lines });
            i = next;
            continue;
        }
        // Page break: three or more `=` and nothing else.
        if trimmed.chars().all(|c| c == '=') && trimmed.len() >= 3 {
            script.elements.push(Element::PageBreak);
            i += 1;
            continue;
        }
        // Section: one to six `#`.
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|&c| c == '#').count().min(6);
            let text = trimmed[level..].trim().to_string();
            script.elements.push(Element::Section { text, level });
            i += 1;
            continue;
        }
        // Synopsis: a single leading `=` (not a page break).
        if let Some(rest) = trimmed.strip_prefix('=') {
            script.elements.push(Element::Synopsis {
                text: rest.trim().to_string(),
            });
            i += 1;
            continue;
        }
        // Scene heading: forced (a single dot, then an alphanumeric) or
        // a standard prefix; a trailing `#12A#` is the scene number.
        if let Some(rest) = forced_heading(trimmed) {
            let (text, number) = split_scene_number(rest);
            script.elements.push(Element::SceneHeading {
                text,
                forced: true,
                number,
            });
            i += 1;
            continue;
        }
        if is_scene_heading(trimmed) {
            let (text, number) = split_scene_number(trimmed);
            script.elements.push(Element::SceneHeading {
                text,
                forced: false,
                number,
            });
            i += 1;
            continue;
        }
        // Centered text: `>…<` (check before the forced transition).
        if trimmed.starts_with('>') && trimmed.ends_with('<') && trimmed.len() > 2 {
            script.elements.push(Element::Centered {
                text: trimmed[1..trimmed.len() - 1].trim().to_string(),
            });
            i += 1;
            continue;
        }
        // Transition: forced `>…` or an uppercase line ending in `TO:`.
        if let Some(rest) = trimmed.strip_prefix('>') {
            script.elements.push(Element::Transition {
                text: rest.trim().to_string(),
                forced: true,
            });
            i += 1;
            continue;
        }
        if is_transition(trimmed) {
            script.elements.push(Element::Transition {
                text: trimmed.to_string(),
                forced: false,
            });
            i += 1;
            continue;
        }
        // Character cue: uppercase (or `@`-forced, in any case), next
        // line non-blank. Its dialogue block (parentheticals and
        // dialogue lines) follows until a blank line.
        let cue = match trimmed.strip_prefix('@') {
            Some(rest) => forced_character_cue(rest),
            None => character_cue(trimmed),
        };
        if let Some((name, extension, dual)) = cue
            && next_non_blank(&lines, i + 1).is_some_and(|n| n == i + 1)
        {
            script.elements.push(Element::Character {
                name,
                extension,
                dual,
            });
            i += 1;
            while i < lines.len() && !lines[i].trim().is_empty() {
                let d = lines[i].trim();
                if d.starts_with('(') && d.ends_with(')') && d.len() > 2 {
                    script.elements.push(Element::Parenthetical {
                        text: d[1..d.len() - 1].trim().to_string(),
                    });
                } else {
                    script.elements.push(Element::Dialogue {
                        text: d.to_string(),
                    });
                }
                i += 1;
            }
            continue;
        }
        // Action: a paragraph of consecutive non-blank lines that open
        // no other element.
        let (paragraph, next) = action_paragraph(&lines, i, None);
        script.elements.push(Element::Action { lines: paragraph });
        i = next;
    }
    script
}

/// An action paragraph from line `start`: that line (as `first` when
/// the caller already stripped a forcing `!`, else as it is), then the
/// consecutive non-blank lines that open no other element. A `!` at the
/// head of a later line is dropped too. Returns the lines and the index
/// after the paragraph. (A cue-like line mid-paragraph still starts a
/// dialogue block per Fountain's blank-line rule: it can't, since the
/// cue check requires a non-blank follower — inside a paragraph every
/// line qualifies, so each line is tested.)
fn action_paragraph(lines: &[&str], start: usize, first: Option<&str>) -> (Vec<String>, usize) {
    let mut paragraph: Vec<String> = Vec::new();
    let mut i = start;
    if let Some(first) = first {
        paragraph.push(first.to_string());
        i += 1;
    }
    while i < lines.len() {
        let a = lines[i].trim();
        if a.is_empty() {
            break;
        }
        // A `!` line stays in the paragraph whatever it reads as.
        let forced = a.starts_with('!');
        if !paragraph.is_empty()
            && !forced
            && (forced_heading(a).is_some()
                || a.starts_with('#')
                || a.starts_with('=')
                || a.starts_with("/*")
                || a.starts_with("[[")
                || is_scene_heading(a)
                || is_transition(a)
                || a.starts_with('>'))
        {
            break;
        }
        if !paragraph.is_empty()
            && !forced
            && (a.starts_with('@') || character_cue(a).is_some())
            && next_non_blank(lines, i + 1).is_some_and(|n| n == i + 1)
        {
            break;
        }
        paragraph.push(a.strip_prefix('!').unwrap_or(a).trim_start().to_string());
        i += 1;
    }
    if paragraph.is_empty() {
        // A line nothing claimed (e.g. an uppercase line at the end
        // of the file) — action of one line.
        paragraph.push(lines[start].trim().to_string());
        i = start + 1;
    }
    (paragraph, i)
}

fn skip_blanks(lines: &[&str], mut i: usize) -> usize {
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    i
}

/// The index of the next non-blank line at or after `from`.
fn next_non_blank(lines: &[&str], mut from: usize) -> Option<usize> {
    while from < lines.len() {
        if !lines[from].trim().is_empty() {
            return Some(from);
        }
        from += 1;
    }
    None
}

/// Title page: `Key: value` entries from the head of the file, values
/// continuing on indented lines. Stops at the first body line.
fn parse_title_page(lines: &[&str], mut i: usize, script: &mut Script) -> usize {
    // The title page only exists if the file *opens* with a known key;
    // otherwise a line like "Note: …" in the body would be eaten.
    let Some(first) = lines.get(i) else { return i };
    if title_key(first).is_none() {
        return i;
    }
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            // Blank: the title page continues only if a known key or an
            // indented continuation follows.
            match next_non_blank(lines, i + 1) {
                Some(n)
                    if title_key(lines[n]).is_some()
                        || lines[n].starts_with(' ')
                        || lines[n].starts_with('\t') =>
                {
                    i = n;
                    continue;
                }
                _ => return i + 1,
            }
        }
        if let Some(key) = title_key(line) {
            let (k, v) = line.split_once(':').expect("a key line has a colon");
            debug_assert_eq!(k.trim().to_lowercase(), key);
            let mut values: Vec<String> = Vec::new();
            let first_value = v.trim();
            if !first_value.is_empty() {
                values.push(first_value.to_string());
            }
            script.title_page.push((k.trim().to_string(), values));
            i += 1;
            continue;
        }
        if (line.starts_with(' ') || line.starts_with('\t'))
            && let Some(last) = script.title_page.last_mut()
        {
            last.1.push(line.trim().to_string());
            i += 1;
            continue;
        }
        return i;
    }
    i
}

/// The recognized title key of a `Key:` line, lowercased.
fn title_key(line: &str) -> Option<String> {
    let (key, _) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.len() > 20 {
        return None;
    }
    let lower = key.to_lowercase();
    TITLE_KEYS.contains(&lower.as_str()).then_some(lower)
}

/// Consume a `/* … */` or `[[ … ]]` block that opened mid-line `rest`
/// (the text after the opener on line `start`). Returns the joined text
/// and the index of the line after the closer.
fn take_until(lines: &[&str], start: usize, rest: &str, closer: &str) -> (String, usize) {
    let mut parts: Vec<String> = Vec::new();
    let mut current = rest.to_string();
    let mut i = start;
    loop {
        if let Some(pos) = current.find(closer) {
            parts.push(current[..pos].trim().to_string());
            return (parts.join("\n").trim().to_string(), i + 1);
        }
        parts.push(current.trim().to_string());
        i += 1;
        match lines.get(i) {
            Some(line) => current = line.trim().to_string(),
            None => return (parts.join("\n").trim().to_string(), i),
        }
    }
}

/// The forced scene heading: a single leading `.` followed by an
/// alphanumeric — Fountain's rule, so `...` and `..` stay action.
/// Returns the heading text after the dot.
pub(crate) fn forced_heading(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('.')?;
    rest.starts_with(|c: char| c.is_alphanumeric())
        .then_some(rest)
}

/// Split a heading's trailing scene number off its text:
/// `INT. KITCHEN - DAY #12A#` → (`INT. KITCHEN - DAY`, `12A`). A scene
/// number is alphanumerics, dashes and periods between two `#`s at the
/// end of the line; anything else leaves the text whole.
fn split_scene_number(heading: &str) -> (String, Option<String>) {
    let heading = heading.trim();
    if let Some(body) = heading.strip_suffix('#')
        && let Some(open) = body.rfind('#')
    {
        let number = &body[open + 1..];
        if !number.is_empty()
            && number
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
        {
            return (
                body[..open].trim_end().to_string(),
                Some(number.to_string()),
            );
        }
    }
    (heading.to_string(), None)
}

/// True for the unforced scene-heading forms: `INT.`, `EXT.`, `EST.`,
/// `INT./EXT.`, `INT/EXT.`, `I/E.` / `I/E `.
pub(crate) fn is_scene_heading(line: &str) -> bool {
    const PREFIXES: &[&str] = &["INT./EXT.", "INT/EXT.", "INT.", "EXT.", "EST.", "I/E."];
    if PREFIXES.iter().any(|p| line.starts_with(p)) {
        return true;
    }
    // `I/E ` (a space, no dot) and `INT ` / `EXT ` bare forms.
    for p in ["I/E", "INT", "EXT", "EST"] {
        if let Some(rest) = line.strip_prefix(p)
            && rest.starts_with(' ')
        {
            return true;
        }
    }
    false
}

/// True for an uppercase line ending in `TO:` (the unforced transition).
pub(crate) fn is_transition(line: &str) -> bool {
    line.len() > 3 && line.ends_with("TO:") && is_upper(line) && !line.starts_with(['.', '!'])
}

/// True when every cased character is uppercase (digits and punctuation
/// ignored), with at least one cased character.
fn is_upper(line: &str) -> bool {
    let mut any = false;
    for c in line.chars() {
        if c.is_lowercase() {
            return false;
        }
        any |= c.is_alphabetic();
    }
    any
}

/// Parse a character cue: an uppercase line, optionally carrying an
/// extension (`(V.O.)`, `(O.S.)`, `(CONT'D)`) and the dual-dialogue
/// caret. Returns `(name, extension, dual)`.
pub(crate) fn character_cue(line: &str) -> Option<(String, Option<String>, bool)> {
    let (body, extension, dual) = split_cue(line);
    if body.is_empty() || body.ends_with('.') || !is_upper(body) {
        return None;
    }
    // A name is letters, digits, spaces and the usual marks — a line
    // ending in `!` or `?` is shouted action, not a cue.
    if !body
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '-' | '\'' | '_'))
    {
        return None;
    }
    Some((body.to_string(), extension, dual))
}

/// The `@`-forced cue (`line` is what follows the `@`): any case, any
/// marks — `@McCLANE`, `@MAYA & KAI` — with the same extension and
/// caret parse as an uppercase cue. Only an empty name is refused.
pub(crate) fn forced_character_cue(line: &str) -> Option<(String, Option<String>, bool)> {
    let (body, extension, dual) = split_cue(line.trim());
    (!body.is_empty()).then(|| (body.to_string(), extension, dual))
}

/// A cue's parts: the name, the trailing parenthesized extensions
/// (`MAYA (V.O.) (CONT'D)` — all of them, rightmost last) and whether
/// a `^` marks it dual. No judgement on the name.
fn split_cue(line: &str) -> (&str, Option<String>, bool) {
    let mut body = line;
    let mut dual = false;
    if let Some(rest) = body.strip_suffix('^') {
        dual = true;
        body = rest.trim_end();
    }
    let mut extension: Option<String> = None;
    while body.ends_with(')') {
        let Some(open) = body.rfind('(') else { break };
        let ext = body[open + 1..body.len() - 1].trim();
        if ext.is_empty() || ext.len() > 12 {
            break;
        }
        extension = match extension {
            None => Some(ext.to_string()),
            Some(prev) => Some(format!("{ext} {prev}")),
        };
        body = body[..open].trim_end();
    }
    (body, extension, dual)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(script: &Script) -> Vec<&'static str> {
        script
            .elements
            .iter()
            .map(|e| match e {
                Element::SceneHeading { .. } => "scene",
                Element::Action { .. } => "action",
                Element::Character { .. } => "character",
                Element::Parenthetical { .. } => "parenthetical",
                Element::Dialogue { .. } => "dialogue",
                Element::Transition { .. } => "transition",
                Element::Centered { .. } => "centered",
                Element::Note { .. } => "note",
                Element::Boneyard { .. } => "boneyard",
                Element::Section { .. } => "section",
                Element::Synopsis { .. } => "synopsis",
                Element::PageBreak => "pagebreak",
            })
            .collect()
    }

    #[test]
    fn title_page_entries_in_order_with_continuations() {
        let script = parse(
            "Title: The Sample\n\
             Credit: written by\n\
             Author: A. Writer\n\
             Draft date: 2026-01-01\n\
             Contact:\n\
             \tA. Writer\n\
             \t1 Main St\n\
             \n\
             INT. KITCHEN - DAY\n",
        );
        assert_eq!(
            script.title_page,
            vec![
                ("Title".to_string(), vec!["The Sample".to_string()]),
                ("Credit".to_string(), vec!["written by".to_string()]),
                ("Author".to_string(), vec!["A. Writer".to_string()]),
                ("Draft date".to_string(), vec!["2026-01-01".to_string()]),
                (
                    "Contact".to_string(),
                    vec!["A. Writer".to_string(), "1 Main St".to_string()]
                ),
            ]
        );
        assert_eq!(kinds(&script), vec!["scene"]);
    }

    #[test]
    fn no_title_page_when_the_file_opens_with_a_scene() {
        let script = parse("INT. KITCHEN - DAY\n\nAction.\n");
        assert!(script.title_page.is_empty());
        assert_eq!(kinds(&script), vec!["scene", "action"]);
    }

    #[test]
    fn scene_heading_table() {
        for (line, forced) in [
            ("INT. KITCHEN - DAY", false),
            ("EXT. ROOFTOP - NIGHT", false),
            ("EST. DINER - DAWN", false),
            ("INT./EXT. CAR - CONTINUOUS", false),
            ("INT/EXT. CAR - CONTINUOUS", false),
            ("I/E. PORCH - DUSK", false),
            ("I/E PORCH - DUSK", false),
            ("INT KITCHEN - DAY", false),
            ("EXT ROOFTOP - NIGHT", false),
            (".A hallway, impossible to place", true),
        ] {
            let script = parse(line);
            match &script.elements[..] {
                [
                    Element::SceneHeading {
                        text,
                        forced: f,
                        number: None,
                    },
                ] => {
                    assert_eq!(*f, forced, "{line}");
                    if forced {
                        assert_eq!(text, "A hallway, impossible to place");
                    } else {
                        assert_eq!(text, line);
                    }
                }
                other => panic!("{line}: {other:?}"),
            }
        }
    }

    #[test]
    fn lowercase_int_is_action() {
        let script = parse("int. not a heading\n");
        assert_eq!(kinds(&script), vec!["action"]);
    }

    #[test]
    fn scene_numbers_come_off_the_heading() {
        for (line, text, number) in [
            ("INT. KITCHEN - DAY #12#", "INT. KITCHEN - DAY", Some("12")),
            (
                "INT. KITCHEN - DAY #12A#",
                "INT. KITCHEN - DAY",
                Some("12A"),
            ),
            (
                "EXT. YARD - NIGHT #I-1-A#",
                "EXT. YARD - NIGHT",
                Some("I-1-A"),
            ),
            ("EXT. YARD - NIGHT #1.#", "EXT. YARD - NIGHT", Some("1.")),
            (".Kitchen #3#", "Kitchen", Some("3")),
            // No closing hash, an empty number, a space inside: no number.
            ("INT. CELL #4 - DAY", "INT. CELL #4 - DAY", None),
            ("INT. CELL - DAY ##", "INT. CELL - DAY ##", None),
            ("INT. CELL - DAY #1 2#", "INT. CELL - DAY #1 2#", None),
        ] {
            let script = parse(line);
            match &script.elements[..] {
                [
                    Element::SceneHeading {
                        text: t, number: n, ..
                    },
                ] => {
                    assert_eq!(t, text, "{line}");
                    assert_eq!(n.as_deref(), number, "{line}");
                }
                other => panic!("{line}: {other:?}"),
            }
        }
    }

    #[test]
    fn an_ellipsis_is_action_not_a_forced_heading() {
        // Fountain: a single period followed by an alphanumeric forces a
        // heading; `...` and `..` are action, as is a dot alone.
        for line in ["...and then nothing.", "..", "."] {
            assert_eq!(kinds(&parse(line)), vec!["action"], "{line}");
        }
        assert_eq!(kinds(&parse(".5 seconds pass")), vec!["scene"]);
        // Mid-paragraph the same rule holds: an ellipsis line continues
        // the action instead of opening a scene.
        let script = parse("She waits.\n...and waits.\n");
        assert_eq!(kinds(&script), vec!["action"]);
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_title_page() {
        let script = parse("\u{FEFF}Title: Marked\n\nINT. KITCHEN - DAY\n");
        assert_eq!(
            script.title_page,
            vec![("Title".to_string(), vec!["Marked".to_string()])]
        );
        assert_eq!(kinds(&script), vec!["scene"]);
    }

    #[test]
    fn every_element_kind() {
        let script = parse(
            "# Act One\n\
             = the crew wakes up\n\
             \n\
             INT. KITCHEN - DAY\n\
             \n\
             Maya fries an egg. The radio mutters.\n\
             A second line of action.\n\
             \n\
             MAYA (V.O.)\n\
             (beat)\n\
             Not again.\n\
             \n\
             KAI ^\n\
             Breakfast!\n\
             \n\
             [[ remember the cat ]]\n\
             \n\
             /* this whole beat\n\
             is elided */\n\
             \n\
             CUT TO:\n\
             \n\
             >DISSOLVE TO:\n\
             \n\
             >THE END<\n\
             \n\
             ===\n",
        );
        assert_eq!(
            kinds(&script),
            vec![
                "section",
                "synopsis",
                "scene",
                "action",
                "character",
                "parenthetical",
                "dialogue",
                "character",
                "dialogue",
                "note",
                "boneyard",
                "transition",
                "transition",
                "centered",
                "pagebreak",
            ]
        );
        match &script.elements[3] {
            Element::Action { lines } => assert_eq!(lines.len(), 2),
            other => panic!("{other:?}"),
        }
        match &script.elements[4] {
            Element::Character {
                name,
                extension,
                dual,
            } => {
                assert_eq!(name, "MAYA");
                assert_eq!(extension.as_deref(), Some("V.O."));
                assert!(!dual);
            }
            other => panic!("{other:?}"),
        }
        match &script.elements[7] {
            Element::Character {
                name,
                extension,
                dual,
            } => {
                assert_eq!(name, "KAI");
                assert_eq!(*extension, None);
                assert!(*dual);
            }
            other => panic!("{other:?}"),
        }
        match &script.elements[5] {
            Element::Parenthetical { text } => assert_eq!(text, "beat"),
            other => panic!("{other:?}"),
        }
        match &script.elements[9] {
            Element::Note { text } => assert_eq!(text, "remember the cat"),
            other => panic!("{other:?}"),
        }
        match &script.elements[10] {
            Element::Boneyard { text } => assert_eq!(text, "this whole beat\nis elided"),
            other => panic!("{other:?}"),
        }
        match &script.elements[11] {
            Element::Transition { text, forced } => {
                assert_eq!(text, "CUT TO:");
                assert!(!forced);
            }
            other => panic!("{other:?}"),
        }
        match &script.elements[12] {
            Element::Transition { text, forced } => {
                assert_eq!(text, "DISSOLVE TO:");
                assert!(forced);
            }
            other => panic!("{other:?}"),
        }
        match &script.elements[13] {
            Element::Centered { text } => assert_eq!(text, "THE END"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_forced_action_line_is_action_whatever_it_reads_as() {
        // `!` escapes a cue, a heading and a transition; the paragraph
        // runs on, and a later `!` is dropped too.
        let script = parse(
            "!MAYA\nstands in the door.\n\n!INT. NOT A SCENE\n\n!CUT TO:\n!and still action\n\n!\n",
        );
        assert_eq!(kinds(&script), vec!["action", "action", "action", "action"]);
        match &script.elements[0] {
            Element::Action { lines } => {
                assert_eq!(lines, &["MAYA", "stands in the door."]);
            }
            other => panic!("{other:?}"),
        }
        match &script.elements[2] {
            Element::Action { lines } => {
                assert_eq!(lines, &["CUT TO:", "and still action"]);
            }
            other => panic!("{other:?}"),
        }
        // A bare `!` is an empty action line, not a crash.
        match &script.elements[3] {
            Element::Action { lines } => assert_eq!(lines, &[""]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_forced_cue_takes_any_case_and_any_marks() {
        let script =
            parse("@McCLANE (V.O.)\nYippee.\n\n@Maya & Kai ^\nTogether.\n\n@\nNot a cue.\n");
        assert_eq!(
            kinds(&script),
            vec!["character", "dialogue", "character", "dialogue", "action"]
        );
        match &script.elements[0] {
            Element::Character {
                name,
                extension,
                dual,
            } => {
                assert_eq!(name, "McCLANE");
                assert_eq!(extension.as_deref(), Some("V.O."));
                assert!(!dual);
            }
            other => panic!("{other:?}"),
        }
        match &script.elements[2] {
            Element::Character { name, dual, .. } => {
                assert_eq!(name, "Maya & Kai");
                assert!(*dual);
            }
            other => panic!("{other:?}"),
        }
        // A forced cue needs its lines like any other: alone, it's action.
        assert_eq!(kinds(&parse("@MAYA\n")), vec!["action"]);
        // And it breaks an action paragraph the way an uppercase cue does.
        assert_eq!(
            kinds(&parse("She turns.\n@McClane\nHi.\n")),
            vec!["action", "character", "dialogue"]
        );
    }

    #[test]
    fn shouted_action_is_not_a_character_cue() {
        let script = parse("The door slams.\n\nGET DOWN!\n\nThe room goes quiet.\n");
        assert_eq!(kinds(&script), vec!["action", "action", "action"]);
    }

    #[test]
    fn cue_at_end_of_file_is_action() {
        let script = parse("Walking home alone.\n\nMAYA\n");
        assert_eq!(kinds(&script), vec!["action", "action"]);
    }

    #[test]
    fn multi_line_notes_and_unclosed_blocks_parse() {
        let script = parse("[[ first\nsecond ]]\n\n/* unclosed\n");
        assert_eq!(
            script.elements,
            vec![
                Element::Note {
                    text: "first\nsecond".to_string()
                },
                Element::Boneyard {
                    text: "unclosed".to_string()
                },
            ]
        );
    }
}
