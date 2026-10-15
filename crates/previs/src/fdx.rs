//! Final Draft as script pages: a `.fdx` file's typed paragraphs →
//! Fountain, so the file a director actually has drafts through the
//! same parser as a `.fountain` one and nothing downstream knows the
//! difference. Deterministic — same bytes in, same Fountain out.
//!
//! FDX is XML: `<FinalDraft><Content>` holds `<Paragraph Type="…">`
//! elements in script order, each a few `<Text>` runs, and
//! `<TitlePage><Content>` holds the title page's lines. The mapping:
//!
//! - **Scene Heading** → the heading, `.`-forced when it doesn't open
//!   `INT.`/`EXT.`; the paragraph's `Number` rides along as the shooting
//!   script's `#12A#`.
//! - **Character** → the cue, uppercased (Final Draft stores it as
//!   typed), `@`-forced when Fountain's rule wouldn't read it (`MAYA &
//!   KAI`); the second cue of a `<DualDialogue>` carries the `^`. A cue
//!   with no dialogue after it is dropped — Final Draft leaves those
//!   behind, and Fountain would read one as action.
//! - **Parenthetical**, **Dialogue**, **Lyrics** → the lines of the open
//!   dialogue block; outside one (no cue before them) they are action.
//! - **Transition** → `> …`, or as written when it already ends in `TO:`.
//! - **Action**, **General**, **Shot**, anything unknown → action,
//!   `!`-forced when a line would otherwise read as something else; a
//!   centered General paragraph is `>…<`. Final Draft wraps action by
//!   its page layout, not with newlines, so action is re-wrapped at
//!   [`ACTION_COLUMNS`] — a screenplay page's action line — and the
//!   page-time estimate (a second a line) reads it the way it reads a
//!   hard-wrapped Fountain file.
//! - **New Act**, **End of Act** → sections (`# …`); **Cast List** is
//!   dropped; a `<ScriptNote>` is a note (`[[ … ]]`) after its paragraph
//!   — after the dialogue block, when it sits inside one.
//! - **The title page**: the first line is the title; a line reading
//!   `by` / `written by` / `screenplay by` … is the credit and the line
//!   after it the author. The rest (contact, draft date) is left.
//!
//! Styling (`Style="Bold"`), revision marks, page breaks and the scene
//! properties Final Draft keeps per heading don't reach the draft.

use std::fmt;

use crate::fountain;

/// The action column of a screenplay page, in characters: six inches
/// at ten characters an inch.
pub const ACTION_COLUMNS: usize = 61;

/// Why a file isn't Final Draft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FdxError {
    /// The text isn't well-formed XML (the parser's reason).
    NotXml(String),
    /// Well-formed XML whose document element isn't `<FinalDraft>` (the
    /// element it is).
    NotFinalDraft(String),
}

impl fmt::Display for FdxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FdxError::NotXml(reason) => write!(f, "the XML doesn't parse ({reason})"),
            FdxError::NotFinalDraft(root) => {
                write!(f, "the XML is a <{root}> document, not <FinalDraft>")
            }
        }
    }
}

impl std::error::Error for FdxError {}

/// Translate a Final Draft file to Fountain.
pub fn to_fountain(fdx: &str) -> Result<String, FdxError> {
    let source = fdx.strip_prefix('\u{FEFF}').unwrap_or(fdx);
    let doc = roxmltree::Document::parse(source).map_err(|e| FdxError::NotXml(e.to_string()))?;
    let root = doc.root_element();
    if !root.has_tag_name("FinalDraft") {
        return Err(FdxError::NotFinalDraft(root.tag_name().name().to_string()));
    }

    let title_page = root
        .children()
        .find(|n| n.has_tag_name("TitlePage"))
        .map(title_page)
        .unwrap_or_default();
    let mut body: Vec<String> = Vec::new();
    if let Some(content) = root.children().find(|n| n.has_tag_name("Content")) {
        emit(&paragraphs(content), &mut body);
    }

    while body.last().is_some_and(|l| l.is_empty()) {
        body.pop();
    }
    let mut out = String::new();
    for (key, value) in &title_page {
        out.push_str(&format!("{key}: {value}\n"));
    }
    if !title_page.is_empty() && !body.is_empty() {
        out.push('\n');
    }
    for line in &body {
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// What a paragraph is to the draft.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    SceneHeading,
    /// Action, General, Shot, and any type this draft doesn't know.
    Action,
    Character,
    Parenthetical,
    /// Dialogue, and Lyrics.
    Dialogue,
    Transition,
    /// New Act, End of Act.
    Section,
    /// Cast List.
    Skip,
}

/// One `<Paragraph>`, read.
#[derive(Debug)]
struct Paragraph {
    kind: Kind,
    /// The `<Text>` runs joined, whitespace collapsed, trimmed.
    text: String,
    /// The `Number` attribute, when a scene heading carries one.
    number: Option<String>,
    /// `Alignment="Center"`.
    centered: bool,
    /// The second (and any later) cue of a `<DualDialogue>`.
    dual: bool,
    /// The `<ScriptNote>`s attached, each flattened to a line.
    notes: Vec<String>,
}

/// The `Type` attribute's kind. Final Draft's names, case-insensitive;
/// absent (the title page's paragraphs) is General.
fn kind_of(type_attr: Option<&str>) -> Kind {
    match type_attr
        .unwrap_or("General")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "scene heading" => Kind::SceneHeading,
        "character" => Kind::Character,
        "parenthetical" => Kind::Parenthetical,
        "dialogue" | "lyrics" => Kind::Dialogue,
        "transition" => Kind::Transition,
        "new act" | "end of act" | "act break" => Kind::Section,
        "cast list" => Kind::Skip,
        _ => Kind::Action,
    }
}

/// `<Content>`'s paragraphs in order, a `<DualDialogue>`'s flattened
/// into the sequence with its second cue marked.
fn paragraphs(content: roxmltree::Node) -> Vec<Paragraph> {
    let mut out = Vec::new();
    for node in content.children().filter(|n| n.is_element()) {
        if node.has_tag_name("Paragraph") {
            out.push(paragraph(node));
        } else if node.has_tag_name("DualDialogue") {
            let mut cues = 0;
            for p in node.children().filter(|n| n.has_tag_name("Paragraph")) {
                let mut para = paragraph(p);
                if para.kind == Kind::Character {
                    cues += 1;
                    para.dual = cues >= 2;
                }
                out.push(para);
            }
        }
    }
    out
}

fn paragraph(node: roxmltree::Node) -> Paragraph {
    let notes = node
        .children()
        .filter(|n| n.has_tag_name("ScriptNote"))
        .map(|note| {
            note.descendants()
                .filter(|n| n.has_tag_name("Paragraph"))
                .map(text_of)
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|t| !t.is_empty())
        .collect();
    Paragraph {
        kind: kind_of(node.attribute("Type")),
        text: text_of(node),
        number: node
            .attribute("Number")
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string),
        centered: node
            .attribute("Alignment")
            .is_some_and(|a| a.eq_ignore_ascii_case("Center")),
        dual: false,
        notes,
    }
}

/// A paragraph's text: its `<Text>` runs joined (entities resolved by
/// the parser), every run of whitespace — a line break inside a run
/// included; Final Draft wraps by layout — one space, the ends trimmed.
fn text_of(node: roxmltree::Node) -> String {
    let mut raw = String::new();
    for run in node.children().filter(|n| n.has_tag_name("Text")) {
        for piece in run.children().filter(|n| n.is_text()) {
            raw.push_str(piece.text().unwrap_or(""));
        }
    }
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The title page's entries: the first line is the title; a credit line
/// (`by`, `written by`, …) and the line after it are the credit and the
/// author.
fn title_page(node: roxmltree::Node) -> Vec<(String, String)> {
    let lines: Vec<String> = node
        .descendants()
        .filter(|n| n.has_tag_name("Paragraph"))
        .map(text_of)
        .filter(|t| !t.is_empty())
        .collect();
    let mut out = Vec::new();
    let Some(title) = lines.first() else {
        return out;
    };
    out.push(("Title".to_string(), title.clone()));
    if let Some(at) = lines.iter().skip(1).position(|l| is_credit(l)) {
        let at = at + 1;
        out.push(("Credit".to_string(), lines[at].clone()));
        if let Some(author) = lines.get(at + 1) {
            out.push(("Author".to_string(), author.clone()));
        }
    }
    out
}

/// `by`, `written by`, `screenplay by`, `story by` … — short, and ending
/// in the word.
fn is_credit(line: &str) -> bool {
    let lower = line.trim().trim_end_matches(':').to_ascii_lowercase();
    lower == "by" || (lower.ends_with(" by") && lower.len() <= 40)
}

/// Fountain's scene number: alphanumerics, dashes and periods.
fn is_scene_number(number: &str) -> bool {
    !number.is_empty()
        && number
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
}

/// Write the paragraphs out as Fountain lines.
fn emit(paragraphs: &[Paragraph], body: &mut Vec<String>) {
    // A cue opened a dialogue block that nothing has closed yet.
    let mut in_block = false;
    // Notes met inside the block, held until it closes so they don't
    // split it (a blank line ends a dialogue block).
    let mut held: Vec<String> = Vec::new();
    for (index, p) in paragraphs.iter().enumerate() {
        let text = p.text.as_str();
        if in_block && matches!(p.kind, Kind::Parenthetical | Kind::Dialogue) {
            if !text.is_empty() {
                let line = if p.kind == Kind::Parenthetical
                    && !(text.starts_with('(') && text.ends_with(')'))
                {
                    format!("({text})")
                } else {
                    text.to_string()
                };
                body.push(line);
            }
            held.extend(p.notes.iter().cloned());
            continue;
        }
        // Anything else closes the block, and the notes it held come out.
        in_block = false;
        for note in held.drain(..) {
            block(body, format!("[[ {note} ]]"));
        }

        match p.kind {
            Kind::Skip => continue,
            Kind::SceneHeading if !text.is_empty() => {
                let mut line = if fountain::is_scene_heading(text) {
                    text.to_string()
                } else {
                    format!(".{text}")
                };
                if let Some(number) = p.number.as_deref().filter(|n| is_scene_number(n)) {
                    line.push_str(&format!(" #{number}#"));
                }
                block(body, line);
            }
            Kind::Character if !text.is_empty() => {
                // The cue needs its lines: Fountain reads a lone one as
                // action, and Final Draft leaves empty cues behind.
                let follows = paragraphs.get(index + 1).is_some_and(|next| {
                    matches!(next.kind, Kind::Parenthetical | Kind::Dialogue)
                        && !next.text.is_empty()
                });
                if !follows {
                    for note in &p.notes {
                        block(body, format!("[[ {note} ]]"));
                    }
                    continue;
                }
                let mut cue = text.to_uppercase();
                if p.dual {
                    cue.push_str(" ^");
                }
                if fountain::character_cue(&cue).is_none() {
                    cue.insert(0, '@');
                }
                blank(body);
                body.push(cue);
                in_block = true;
                held.extend(p.notes.iter().cloned());
                continue;
            }
            Kind::Transition if !text.is_empty() => {
                let line = if fountain::is_transition(text) {
                    text.to_string()
                } else {
                    format!("> {text}")
                };
                block(body, line);
            }
            Kind::Section if !text.is_empty() => block(body, format!("# {text}")),
            Kind::Action | Kind::Parenthetical | Kind::Dialogue if !text.is_empty() => {
                // (A parenthetical or dialogue with no cue before it is
                // action: Final Draft lets a writer type them anywhere.)
                if p.centered && p.kind == Kind::Action {
                    block(body, format!("> {text} <"));
                } else {
                    blank(body);
                    for line in wrap(text, ACTION_COLUMNS) {
                        body.push(force_action(line));
                    }
                    blank(body);
                }
            }
            _ => {}
        }
        for note in &p.notes {
            block(body, format!("[[ {note} ]]"));
        }
    }
    for note in held {
        block(body, format!("[[ {note} ]]"));
    }
}

/// Push a blank line unless the body is empty or already ends in one.
fn blank(body: &mut Vec<String>) {
    if body.last().is_some_and(|l| !l.is_empty()) {
        body.push(String::new());
    }
}

/// A line on its own: blank, the line, blank.
fn block(body: &mut Vec<String>, line: String) {
    blank(body);
    body.push(line);
    blank(body);
}

/// Greedy word wrap to `width` characters; a word longer than the
/// width stands alone.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// An action line, `!`-forced when Fountain would read it as a cue, a
/// heading, a transition, a section, a note, or any other element.
fn force_action(line: String) -> String {
    let reads_otherwise = fountain::forced_heading(&line).is_some()
        || line.starts_with(['#', '=', '>', '~', '!', '@'])
        || line.starts_with("[[")
        || line.starts_with("/*")
        || fountain::is_scene_heading(&line)
        || fountain::is_transition(&line)
        || fountain::character_cue(&line).is_some();
    if reads_otherwise {
        format!("!{line}")
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../samples/cold-eggs.fdx");

    fn fdx(content: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<FinalDraft DocumentType=\"Script\" Template=\"No\" Version=\"5\">\n<Content>\n{content}\n</Content>\n</FinalDraft>\n"
        )
    }

    fn para(kind: &str, text: &str) -> String {
        format!("<Paragraph Type=\"{kind}\"><Text>{text}</Text></Paragraph>")
    }

    #[test]
    fn the_sample_is_the_fountain_sample_with_the_page_wrapped() {
        assert_eq!(
            to_fountain(SAMPLE).unwrap(),
            "Title: Cold Eggs\n\
             Credit: written by\n\
             Author: A. Previs\n\
             \n\
             # Act One\n\
             \n\
             INT. KITCHEN - DAY #1#\n\
             \n\
             Maya fries an egg at the counter. The radio mutters the\n\
             shipping forecast to nobody. Kai stumbles in, half asleep,\n\
             and drops onto a stool.\n\
             \n\
             MAYA\n\
             You're late. The eggs are cold.\n\
             \n\
             KAI\n\
             (yawning)\n\
             The eggs are always cold.\n\
             \n\
             MAYA\n\
             Then maybe don't be late.\n\
             \n\
             KAI\n\
             It's a system. It works.\n\
             \n\
             [[ the radio should name the storm from scene two ]]\n\
             \n\
             EXT. CLIFF PATH - DUSK #2#\n\
             \n\
             Wind. The lighthouse blinks out at sea. June waits by the\n\
             gate, collar up against the spray.\n\
             \n\
             JUNE\n\
             You came.\n\
             \n\
             MAYA\n\
             We argued about it.\n\
             \n\
             KAI\n\
             I lost the argument.\n\
             \n\
             JUNE\n\
             (pointing at the sea)\n\
             There. Where the gulls won't go.\n\
             \n\
             > FADE OUT.\n"
        );
    }

    #[test]
    fn a_scene_number_rides_the_heading_and_an_odd_heading_is_forced() {
        let f = to_fountain(&fdx(&format!(
            "<Paragraph Type=\"Scene Heading\" Number=\"12A\"><Text>INT. KITCHEN - DAY</Text></Paragraph>\
             <Paragraph Type=\"Scene Heading\" Number=\"1 2\"><Text>Somewhere, later</Text></Paragraph>\
             {}",
            para("Scene Heading", "")
        )))
        .unwrap();
        assert_eq!(f, "INT. KITCHEN - DAY #12A#\n\n.Somewhere, later\n");
    }

    #[test]
    fn cues_are_uppercased_forced_when_odd_and_dropped_when_empty() {
        let f = to_fountain(&fdx(&[
            para("Character", "Maya (v.o.)"),
            para("Dialogue", "Hi."),
            para("Character", "MAYA &amp; KAI"),
            para("Parenthetical", "together"),
            para("Dialogue", "Hi!"),
            para("Character", "GHOST"),
            para("Action", "Nobody answers."),
            para("Dialogue", "An orphan line is action."),
        ]
        .concat()))
        .unwrap();
        assert_eq!(
            f,
            "MAYA (V.O.)\nHi.\n\n@MAYA & KAI\n(together)\nHi!\n\nNobody answers.\n\nAn orphan line is action.\n"
        );
    }

    #[test]
    fn dual_dialogue_marks_the_second_cue() {
        let f = to_fountain(&fdx(&format!(
            "<DualDialogue>{}{}{}{}</DualDialogue>",
            para("Character", "MAYA"),
            para("Dialogue", "Now."),
            para("Character", "KAI"),
            para("Dialogue", "Later."),
        )))
        .unwrap();
        assert_eq!(f, "MAYA\nNow.\n\nKAI ^\nLater.\n");
    }

    #[test]
    fn action_that_reads_as_another_element_is_forced() {
        let f = to_fountain(&fdx(&[
            para("Action", "INT. NOT A HEADING, says the writer."),
            para("Shot", "CLOSE ON MAYA"),
            para("General", "CUT TO:"),
            para("Action", "# not a section"),
            para("Transition", "SMASH CUT TO:"),
            para("Transition", "FADE OUT."),
            "<Paragraph Type=\"General\" Alignment=\"Center\"><Text>THE END</Text></Paragraph>"
                .to_string(),
        ]
        .concat()))
        .unwrap();
        assert_eq!(
            f,
            "!INT. NOT A HEADING, says the writer.\n\n!CLOSE ON MAYA\n\n!CUT TO:\n\n!# not a section\n\nSMASH CUT TO:\n\n> FADE OUT.\n\n> THE END <\n"
        );
    }

    #[test]
    fn long_action_wraps_at_the_page_column() {
        let words: Vec<String> = (0..40).map(|i| format!("word{i}")).collect();
        let f = to_fountain(&fdx(&para("Action", &words.join(" ")))).unwrap();
        let lines: Vec<&str> = f.lines().collect();
        assert!(lines.len() > 1, "{f}");
        assert!(lines.iter().all(|l| l.len() <= ACTION_COLUMNS), "{f}");
        assert_eq!(f.split_whitespace().count(), 40);
        // An all-caps line that wrapping leaves above another line
        // would read as a cue: forced.
        let shout = to_fountain(&fdx(&para(
            "Action",
            "EVERYTHING IS FINE AND NOTHING HURTS AND THE SEA IS CALM AND THE GULLS ARE QUIET",
        )))
        .unwrap();
        assert!(shout.starts_with("!EVERYTHING"), "{shout}");
    }

    #[test]
    fn text_runs_join_and_entities_resolve() {
        let f = to_fountain(&fdx(
            "<Paragraph Type=\"Action\"><Text Style=\"Bold\">Maya </Text><Text>eats\n   &amp; \n drinks &#8212; slowly.</Text></Paragraph>",
        ))
        .unwrap();
        assert_eq!(f, "Maya eats & drinks — slowly.\n");
    }

    #[test]
    fn notes_wait_for_the_block_to_close() {
        let f = to_fountain(&fdx(&format!(
            "{}<Paragraph Type=\"Dialogue\"><Text>One.</Text><ScriptNote><Paragraph><Text>louder</Text></Paragraph></ScriptNote></Paragraph>{}<Paragraph Type=\"Action\"><Text>Later.</Text><ScriptNote><Paragraph><Text>a</Text></Paragraph><Paragraph><Text>b</Text></Paragraph></ScriptNote></Paragraph>",
            para("Character", "MAYA"),
            para("Dialogue", "Two."),
        )))
        .unwrap();
        assert_eq!(
            f,
            "MAYA\nOne.\nTwo.\n\n[[ louder ]]\n\nLater.\n\n[[ a b ]]\n"
        );
    }

    #[test]
    fn the_title_page_reads_title_credit_and_author() {
        let f = to_fountain(
            "<FinalDraft><TitlePage><Content>\
             <Paragraph><Text></Text></Paragraph>\
             <Paragraph Alignment=\"Center\"><Text>COLD EGGS</Text></Paragraph>\
             <Paragraph><Text>an original screenplay by</Text></Paragraph>\
             <Paragraph><Text>A. Previs</Text></Paragraph>\
             <Paragraph><Text>a.previs@example.com</Text></Paragraph>\
             </Content></TitlePage><Content></Content></FinalDraft>",
        )
        .unwrap();
        assert_eq!(
            f,
            "Title: COLD EGGS\nCredit: an original screenplay by\nAuthor: A. Previs\n"
        );
        // No title page, no entries; a cast list is dropped.
        assert_eq!(
            to_fountain(&fdx(&[
                para("Cast List", "MAYA, KAI"),
                para("Action", "Hi.")
            ]
            .concat()))
            .unwrap(),
            "Hi.\n"
        );
    }

    #[test]
    fn not_xml_and_not_final_draft_are_their_own_errors() {
        assert!(matches!(
            to_fountain("INT. A - DAY\n"),
            Err(FdxError::NotXml(_))
        ));
        assert_eq!(
            to_fountain("<?xml version=\"1.0\"?><html><body/></html>"),
            Err(FdxError::NotFinalDraft("html".to_string()))
        );
        assert_eq!(
            to_fountain("\u{FEFF}<FinalDraft/>").unwrap(),
            "",
            "an empty script is empty Fountain, and a BOM is fine"
        );
    }

    #[test]
    fn the_translation_is_deterministic() {
        assert_eq!(to_fountain(SAMPLE).unwrap(), to_fountain(SAMPLE).unwrap());
    }
}
