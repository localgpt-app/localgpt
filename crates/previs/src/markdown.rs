//! Markdown as script pages: a deterministic convention → Fountain
//! mapping, so a `.md` file drafts through the same parser as a
//! `.fountain` one and nothing downstream knows the difference. Same
//! bytes in, same Fountain out — the draft stays a function of the text.
//!
//! The convention is Markdown's own habits, read as a screenplay:
//!
//! - **Front matter** (`---` … `---` at the top) with Fountain's title
//!   keys — `title:`, `author:`, `credit:`, `draft date:` … — is the
//!   title page; failing that, the first `#` heading before any scene is
//!   the title.
//! - **A heading** at any depth that reads as a scene heading — `INT.`,
//!   `EXT.`, `EST.`, `I/E` — is that scene. Any other heading is a
//!   Fountain section of the same depth (`# Act One`).
//! - **A bold name with a colon** opens dialogue: `**MAYA:** You're
//!   late.` (also `**MAYA**: …`; `**Maya:**` is uppercased; `**MAYA
//!   (V.O.):**` keeps its extension). A plain `MAYA: You're late.` needs
//!   the uppercase, as Fountain's cues do. A `*(beat)*` or `(beat)` at
//!   the head of the line is the parenthetical; a line that is only one
//!   is a parenthetical too.
//! - **An HTML comment** `<!-- … -->` is a note (`[[ … ]]`).
//! - **Code fences** (and what they hold), **horizontal rules** and
//!   **list markers** are dropped; a link keeps its text; a line that is
//!   wholly bold or italic loses the marks (`**INT. KITCHEN - DAY**` is a
//!   scene heading).
//! - **Everything else passes through** as Fountain: a bare `INT.` line,
//!   an uppercase cue over its lines, `>` transitions and `>centered<`
//!   text, `[[notes]]`, `/* boneyard */`, and emphasis — Fountain's `*`
//!   and `_` are Markdown's.

use crate::fountain;

/// Translate Markdown script pages to Fountain.
pub fn to_fountain(markdown: &str) -> String {
    let source = markdown.strip_prefix('\u{FEFF}').unwrap_or(markdown);
    let lines: Vec<&str> = source.lines().collect();

    let mut title_page: Vec<(String, String)> = Vec::new();
    let mut i = front_matter(&lines, &mut title_page);
    let mut titled = title_page
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("title"));

    let mut body: Vec<String> = Vec::new();
    let mut in_fence = false;
    let mut seen_scene = false;

    while i < lines.len() {
        let raw = lines[i];
        i += 1;
        let trimmed = raw.trim();

        // A code fence and what it holds: not script.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.is_empty() {
            blank(&mut body);
            continue;
        }
        // An HTML comment, possibly over lines → a note.
        if let Some(rest) = trimmed.strip_prefix("<!--") {
            let (text, next) = until(&lines, i, rest, "-->");
            i = next;
            blank(&mut body);
            body.push(format!("[[ {text} ]]"));
            blank(&mut body);
            continue;
        }
        if is_rule(trimmed) {
            blank(&mut body);
            continue;
        }
        // A heading: the scene, the title, or a section.
        if let Some((level, text)) = heading(trimmed) {
            let text = unwrap_marks(&inline(text));
            if is_scene_line(&text) {
                blank(&mut body);
                body.push(text);
                blank(&mut body);
                seen_scene = true;
            } else if level == 1 && !titled && !seen_scene {
                title_page.push(("Title".to_string(), text));
                titled = true;
            } else {
                blank(&mut body);
                body.push(format!("{} {}", "#".repeat(level), text));
                blank(&mut body);
            }
            continue;
        }

        let line = inline(strip_list_marker(trimmed));
        let line = unwrap_marks(&line);
        if is_scene_line(&line) {
            blank(&mut body);
            body.push(line);
            blank(&mut body);
            seen_scene = true;
            continue;
        }
        if fountain::is_transition(&line) {
            blank(&mut body);
            body.push(line);
            blank(&mut body);
            continue;
        }
        // A cue with its first line: `**MAYA:** text`, `MAYA: text`.
        if let Some((cue, rest)) = cue(&line) {
            blank(&mut body);
            body.push(cue);
            let (parenthetical, dialogue) = leading_parenthetical(&rest);
            if let Some(p) = parenthetical {
                body.push(format!("({p})"));
            }
            if !dialogue.is_empty() {
                body.push(dialogue);
            }
            continue;
        }
        // `*(beat)*` on its own line.
        if let (Some(p), rest) = leading_parenthetical(&line)
            && rest.is_empty()
        {
            body.push(format!("({p})"));
            continue;
        }
        body.push(line);
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
    out
}

/// Push a blank line unless the body is empty or already ends in one.
fn blank(body: &mut Vec<String>) {
    if body.last().is_some_and(|l| !l.is_empty()) {
        body.push(String::new());
    }
}

/// YAML front matter at the head of the file: `---`, `key: value` lines,
/// `---`. Fountain's title keys become title-page entries, the key's
/// first letter uppercased (`Title: …`). Returns the index of the first
/// body line — 0 when there is no (closed) front matter.
fn front_matter(lines: &[&str], title_page: &mut Vec<(String, String)>) -> usize {
    if lines.first().map(|l| l.trim()) != Some("---") {
        return 0;
    }
    let Some(close) = lines[1..]
        .iter()
        .position(|l| matches!(l.trim(), "---" | "..."))
    else {
        return 0;
    };
    for line in &lines[1..=close] {
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let lower = key.to_lowercase();
            if fountain::TITLE_KEYS.contains(&lower.as_str()) {
                let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
                if !value.is_empty() {
                    let mut chars = lower.chars();
                    let key = match chars.next() {
                        Some(first) => first.to_uppercase().chain(chars).collect(),
                        None => lower.clone(),
                    };
                    title_page.push((key, value.to_string()));
                }
            }
        }
    }
    close + 2
}

/// Collect text from `rest` (what followed the opener on the current
/// line) until `closer`, over lines; returns the trimmed text and the
/// index of the line after the one holding the closer.
fn until(lines: &[&str], start: usize, rest: &str, closer: &str) -> (String, usize) {
    let mut parts: Vec<String> = Vec::new();
    let mut current = rest.to_string();
    let mut i = start;
    loop {
        if let Some(pos) = current.find(closer) {
            parts.push(current[..pos].trim().to_string());
            return (parts.join("\n").trim().to_string(), i);
        }
        parts.push(current.trim().to_string());
        match lines.get(i) {
            Some(line) => {
                current = line.trim().to_string();
                i += 1;
            }
            None => return (parts.join("\n").trim().to_string(), i),
        }
    }
}

/// A horizontal rule: three or more `-`, `*` or `_` (one kind), spaces
/// allowed between.
fn is_rule(line: &str) -> bool {
    let mut marker: Option<char> = None;
    let mut count = 0;
    for c in line.chars() {
        if c == ' ' {
            continue;
        }
        if !matches!(c, '-' | '*' | '_') || marker.is_some_and(|m| m != c) {
            return false;
        }
        marker = Some(c);
        count += 1;
    }
    count >= 3
}

/// An ATX heading: one to six `#`, then a space (or nothing), then the
/// text, trailing `#`s dropped.
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &line[level..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    let text = rest.trim().trim_end_matches('#').trim_end();
    Some((level, text))
}

/// A scene heading as Fountain reads it: the standard prefixes or the
/// `.`-forced form.
fn is_scene_line(line: &str) -> bool {
    fountain::is_scene_heading(line) || fountain::forced_heading(line).is_some()
}

/// Strip a list marker: `- `, `* `, `+ `, `1. `, `1) `.
fn strip_list_marker(line: &str) -> &str {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return rest.trim_start();
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && digits <= 9 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return rest.trim_start();
        }
    }
    line
}

/// Links and images keep their text: `[text](url)` → `text`,
/// `![alt](src)` → `alt`.
fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](") else {
            break;
        };
        let close = open + close;
        let Some(end) = rest[close + 2..].find(')') else {
            break;
        };
        let end = close + 2 + end;
        let before = if open > 0 && rest.as_bytes()[open - 1] == b'!' {
            &rest[..open - 1]
        } else {
            &rest[..open]
        };
        out.push_str(before);
        out.push_str(&rest[open + 1..close]);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A line that is wholly `**…**`, `__…__`, `*…*` or `_…_` loses the
/// marks; any other line is returned as is.
fn unwrap_marks(line: &str) -> String {
    for marker in ["**", "__", "*", "_"] {
        if line.len() > 2 * marker.len()
            && let Some(inner) = line
                .strip_prefix(marker)
                .and_then(|l| l.strip_suffix(marker))
            && !inner.starts_with(marker.chars().next().unwrap_or('*'))
            && !inner.ends_with(marker.chars().next().unwrap_or('*'))
            && !inner.contains(marker)
        {
            return inner.trim().to_string();
        }
    }
    line.to_string()
}

/// A cue and the rest of its line: `**MAYA:** text`, `**MAYA**: text`
/// (any case — the name is uppercased), or `MAYA: text` (uppercase, as
/// Fountain's cues). A bare transition (`CUT TO:`) is not a cue. The
/// rest may be empty: `**MAYA:**` alone opens the block below it.
fn cue(line: &str) -> Option<(String, String)> {
    for marker in ["**", "__"] {
        if let Some(after) = line.strip_prefix(marker)
            && let Some(close) = after.find(marker)
        {
            let mut name = after[..close].trim();
            let mut rest = after[close + marker.len()..].trim_start();
            let mut colon = false;
            if let Some(n) = name.strip_suffix(':') {
                name = n.trim_end();
                colon = true;
            }
            if let Some(r) = rest.strip_prefix(':') {
                rest = r.trim_start();
                colon = true;
            }
            if !colon {
                return None;
            }
            let upper = name.to_uppercase();
            fountain::character_cue(&upper)?;
            return Some((upper, rest.trim().to_string()));
        }
    }
    let (name, rest) = line.split_once(':')?;
    let name = name.trim();
    let rest = rest.trim();
    if name.contains('*') || name.contains('_') || fountain::is_transition(line) {
        return None;
    }
    fountain::character_cue(name)?;
    Some((name.to_string(), rest.to_string()))
}

/// A parenthetical at the head of the text — `(beat) …`, `*(beat)* …`,
/// `_(beat)_ …` — and what follows it, trimmed.
fn leading_parenthetical(text: &str) -> (Option<String>, String) {
    let mut body = text;
    let mut mark: Option<&str> = None;
    for m in ["*", "_"] {
        if let Some(rest) = body.strip_prefix(m) {
            body = rest;
            mark = Some(m);
            break;
        }
    }
    if let Some(inner) = body.strip_prefix('(')
        && let Some(close) = inner.find(')')
    {
        let parenthetical = inner[..close].trim();
        let mut rest = &inner[close + 1..];
        if let Some(m) = mark {
            // `*(beat)*`: the closing mark must follow, or it wasn't one.
            match rest.strip_prefix(m) {
                Some(r) => rest = r,
                None => return (None, text.to_string()),
            }
        }
        if !parenthetical.is_empty() {
            return (Some(parenthetical.to_string()), rest.trim().to_string());
        }
    }
    (None, text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_is_the_title_page() {
        let f = to_fountain(
            "---\ntitle: \"Cold Eggs\"\nauthor: A. Previs\ngenre: drama\n---\n\n## INT. KITCHEN - DAY\n",
        );
        assert_eq!(
            f,
            "Title: Cold Eggs\nAuthor: A. Previs\n\nINT. KITCHEN - DAY\n"
        );
    }

    #[test]
    fn the_first_h1_before_a_scene_is_the_title_and_later_ones_are_sections() {
        let f = to_fountain("# Cold Eggs\n\n# Act One\n\n## INT. KITCHEN - DAY\n\n# Act Two\n");
        assert_eq!(
            f,
            "Title: Cold Eggs\n\n# Act One\n\nINT. KITCHEN - DAY\n\n# Act Two\n"
        );
        // With a front-matter title, the H1 is a section.
        let f = to_fountain("---\ntitle: T\n---\n# Act One\n");
        assert_eq!(f, "Title: T\n\n# Act One\n");
    }

    #[test]
    fn headings_of_any_depth_can_be_scenes() {
        let f =
            to_fountain("### EXT. ROOF - NIGHT ###\n\n#### .Somewhere\n\n**INT. HALL - DAY**\n");
        assert_eq!(f, "EXT. ROOF - NIGHT\n\n.Somewhere\n\nINT. HALL - DAY\n");
    }

    #[test]
    fn bold_cues_open_dialogue_in_any_case() {
        let f = to_fountain(
            "**MAYA:** You're late.\n**Kai**: *(yawning)* Always.\n__JUNE (V.O.):__\nThere.\n",
        );
        assert_eq!(
            f,
            "MAYA\nYou're late.\n\nKAI\n(yawning)\nAlways.\n\nJUNE (V.O.)\nThere.\n"
        );
    }

    #[test]
    fn plain_cues_need_the_uppercase() {
        let f = to_fountain("MAYA: Hi.\n\nNote: not a cue.\n\nCUT TO:\n\nhttps://example.com/x\n");
        assert_eq!(
            f,
            "MAYA\nHi.\n\nNote: not a cue.\n\nCUT TO:\n\nhttps://example.com/x\n"
        );
    }

    #[test]
    fn a_parenthetical_line_and_a_note() {
        let f =
            to_fountain("**MAYA:**\n*(beat)*\nNot again.\n\n<!-- the radio\nnames the storm -->\n");
        assert_eq!(
            f,
            "MAYA\n(beat)\nNot again.\n\n[[ the radio\nnames the storm ]]\n"
        );
    }

    #[test]
    fn fences_rules_lists_and_links_go() {
        let f = to_fountain(
            "INT. A - DAY\n\n```world\n{\"not\": \"script\"}\n```\n\n---\n\n- She reads [the letter](http://x) twice.\n1. Then ![the photo](p.png).\n\n***\n",
        );
        assert_eq!(
            f,
            "INT. A - DAY\n\nShe reads the letter twice.\nThen the photo.\n"
        );
    }

    #[test]
    fn fountain_inside_markdown_passes_through() {
        let script = "INT. A - DAY\n\nMAYA\n(beat)\nHi.\n\n>FADE OUT.\n\n>THE END<\n\n/* gone */\n\n[[ note ]]\n";
        assert_eq!(to_fountain(script), script);
    }

    #[test]
    fn an_unclosed_front_matter_is_body() {
        let f = to_fountain("---\ntitle: T\n\nINT. A - DAY\n");
        assert_eq!(f, "title: T\n\nINT. A - DAY\n");
    }

    #[test]
    fn the_translation_is_deterministic() {
        let md = "# T\n\n## INT. A - DAY\n\n**MAYA:** Hi.\n";
        assert_eq!(to_fountain(md), to_fountain(md));
    }
}
