//! A Fountain screenplay parser (`spec`-less but faithful: the elements
//! the staging draft reads, at <https://fountain.io> syntax).
//!
//! Line-oriented and deterministic: the same bytes always parse to the
//! same [`Script`]. Supported: title pages (`Key: value` blocks at the
//! head of the file), scene headings (`INT.`/`EXT.`/`EST.`/`INT./EXT.`/
//! `INT/EXT.`/`I/E`, plus the `.`-forced form), action, character cues
//! with extensions (`(V.O.)`, `(O.S.)`, `(CONT'D)`), parentheticals,
//! dialogue, dual dialogue (`^`), transitions (`TO:` and the `>`-forced
//! form), centered text (`>…<`), notes (`[[ … ]]`), the boneyard
//! (`/* … */`), sections (`#`), synopses (`=`) and page breaks (`===`).
//!
//! Notes and boneyard blocks are recognized when they open at the start
//! of a line (both may span lines); a `[[ … ]]` that opens mid-line is
//! left in the text, which is where screenplays put them least.

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
        /// The heading text without the marker (`INT.` prefix / forcing
        /// dot), trimmed.
        text: String,
        /// True when the heading was forced with a leading `.`.
        forced: bool,
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
const TITLE_KEYS: &[&str] = &[
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
        // Scene heading: forced (leading dot) or a standard prefix.
        if let Some(rest) = trimmed.strip_prefix('.') {
            script.elements.push(Element::SceneHeading {
                text: rest.trim().to_string(),
                forced: true,
            });
            i += 1;
            continue;
        }
        if is_scene_heading(trimmed) {
            script.elements.push(Element::SceneHeading {
                text: trimmed.to_string(),
                forced: false,
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
        // Character cue: uppercase, next line non-blank. Its dialogue
        // block (parentheticals and dialogue lines) follows until a
        // blank line.
        if let Some((name, extension, dual)) = character_cue(trimmed)
            && next_non_blank(&lines, i + 1).is_some_and(|n| n == i + 1) {
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
        // no other element. (A cue-like line mid-paragraph still starts
        // a dialogue block per Fountain's blank-line rule: it can't,
        // since the cue check above requires a non-blank follower —
        // inside a paragraph every line qualifies, so test each line.)
        let mut paragraph: Vec<String> = Vec::new();
        while i < lines.len() {
            let a = lines[i].trim();
            if a.is_empty() {
                break;
            }
            if !paragraph.is_empty()
                && (a.starts_with('.')
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
                && character_cue(a).is_some()
                && next_non_blank(&lines, i + 1).is_some_and(|n| n == i + 1)
            {
                break;
            }
            paragraph.push(a.to_string());
            i += 1;
        }
        if paragraph.is_empty() {
            // A line nothing claimed (e.g. an uppercase line at the end
            // of the file) — action of one line.
            paragraph.push(trimmed.to_string());
            i += 1;
        }
        script.elements.push(Element::Action { lines: paragraph });
    }
    script
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

/// True for the unforced scene-heading forms: `INT.`, `EXT.`, `EST.`,
/// `INT./EXT.`, `INT/EXT.`, `I/E.` / `I/E `.
fn is_scene_heading(line: &str) -> bool {
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
fn is_transition(line: &str) -> bool {
    line.len() > 3 && line.ends_with("TO:") && is_upper(line) && !line.starts_with('.')
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
fn character_cue(line: &str) -> Option<(String, Option<String>, bool)> {
    let mut body = line;
    let mut dual = false;
    if let Some(rest) = body.strip_suffix('^') {
        dual = true;
        body = rest.trim_end();
    }
    let mut extension: Option<String> = None;
    // Extensions trail the cue: `MAYA (V.O.) (CONT'D)` — take all
    // trailing parenthesized groups, rightmost last.
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
                [Element::SceneHeading { text, forced: f }] => {
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
