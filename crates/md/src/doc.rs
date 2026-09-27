//! Markdown → [`Doc`]: a title, front matter, and one [`Section`] per place
//! — a `##` heading in the `world` genre, or a `---`-separated slide in the
//! `deck` genre (Marp/Slidev style).
//!
//! Pure (no Bevy), like [`crate::draft`], so both can move into a shared
//! crate later (PLAN.md M5).

use std::collections::BTreeMap;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// A parsed Markdown document.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    /// The first `#` heading, or the caller's fallback (the file stem).
    pub title: String,
    /// `key: value` lines from a leading `---` block, e.g. `genre: deck`.
    pub front_matter: BTreeMap<String, String>,
    /// Prose before the first section — in a deck, the title slide's body.
    pub intro: String,
    /// One per `##` heading (world genre) or slide (deck genre), in
    /// document order.
    pub sections: Vec<Section>,
}

/// One section of prose, keyed by a content hash.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub heading: String,
    /// Paragraphs, list items, and `###` sub-headings as plain text, one per
    /// line. Code blocks are left out — they aren't scenery, except the
    /// [`world`](Self::world) fence, which is captured verbatim.
    pub body: String,
    /// The raw content of a ```` ```world ```` fenced block in this section,
    /// if any: an exact entity override (a JSON array of world entities in
    /// platform-local coordinates) that replaces the LLM tiers for the
    /// section — deterministic, no model. Part of the hash.
    pub world: Option<String>,
    /// BLAKE3 of heading + body + world fence: seeds the draft and keys the
    /// LLM cache, so an unchanged section never regenerates — and editing
    /// only the fence still invalidates it.
    pub hash: blake3::Hash,
}

impl Section {
    fn new(heading: String, body: &str, world: Option<&str>) -> Self {
        let body = body.trim().to_string();
        let world = world
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(str::to_string);
        let hash = blake3::hash(
            format!(
                "{heading}\n{body}\n{}",
                world.as_deref().unwrap_or_default()
            )
            .as_bytes(),
        );
        Self {
            heading,
            body,
            world,
            hash,
        }
    }

    /// A stable 64-bit seed taken from [`Self::hash`].
    pub fn seed(&self) -> u64 {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(&self.hash.as_bytes()[..8]);
        u64::from_le_bytes(bytes)
    }
}

impl Doc {
    /// Parse Markdown. A document without sections becomes one section; a
    /// `deck` splits on `---` separators instead of `##` headings.
    pub fn parse(src: &str, fallback_title: &str) -> Self {
        let options = Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
            | Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS;

        let mut title = None;
        let mut front_matter = BTreeMap::new();
        let mut intro = String::new();
        // (heading, body, world fence) per section.
        let mut sections: Vec<(String, String, Option<String>)> = Vec::new();
        // The heading being read, if inside one.
        let mut heading: Option<(HeadingLevel, String)> = None;
        let mut in_metadata = false;
        let mut in_code = false;
        // Inside a ```world fence: its text is captured verbatim (an exact
        // entity override, PLAN.md M3), not discarded like other fences.
        let mut in_world_fence = false;
        let mut world_buf = String::new();
        // A world fence that appeared before any section — attaches to the
        // first section (or the single intro one) at the end of the parse.
        let mut intro_world: Option<String> = None;
        // Set by the first `---` slide separator (deck genre only).
        let mut seen_rule = false;

        for event in Parser::new_ext(src, options) {
            // Front matter arrives before any separator, so the genre is
            // known by the time a Rule can matter — evaluate it lazily.
            let deck = is_deck_genre(&front_matter);
            match event {
                Event::Start(Tag::MetadataBlock(_)) => in_metadata = true,
                Event::End(TagEnd::MetadataBlock(_)) => in_metadata = false,
                Event::Start(Tag::CodeBlock(kind)) => {
                    in_code = true;
                    in_world_fence = match &kind {
                        pulldown_cmark::CodeBlockKind::Fenced(info) => info.trim() == "world",
                        pulldown_cmark::CodeBlockKind::Indented => false,
                    };
                    world_buf.clear();
                }
                Event::End(TagEnd::CodeBlock) => {
                    if in_world_fence {
                        attach_world(&mut sections, &mut intro_world, &world_buf);
                    }
                    in_world_fence = false;
                    in_code = false;
                }
                Event::Text(text) if in_world_fence => world_buf.push_str(&text),
                Event::Text(text) if in_metadata => parse_front_matter(&text, &mut front_matter),
                Event::Rule if deck && !in_code && !in_metadata => {
                    // A `---` separator starts a new slide. Content before
                    // the first one (the title slide) keeps accumulating in
                    // `intro` and becomes section 0 at the end of the parse.
                    seen_rule = true;
                    sections.push((String::new(), String::new(), None));
                }
                Event::Start(Tag::Heading { level, .. }) => heading = Some((level, String::new())),
                Event::End(TagEnd::Heading(_)) => {
                    let Some((level, text)) = heading.take() else {
                        continue;
                    };
                    let text = text.trim().to_string();
                    match level {
                        HeadingLevel::H1 if title.is_none() && sections.is_empty() => {
                            title = Some(text);
                        }
                        HeadingLevel::H1 | HeadingLevel::H2 if !deck => {
                            sections.push((text, String::new(), None));
                        }
                        _ if deck => match sections.last_mut() {
                            // The first heading in a slide names it, at any
                            // level (Marp titles slides with `#` or `##`).
                            Some((heading, _, _)) if heading.is_empty() => *heading = text,
                            // Later headings are body lines, like `###` in
                            // the world genre.
                            Some((_, body, _)) => {
                                body.push_str(&text);
                                body.push('\n');
                            }
                            None => sections.push((text, String::new(), None)),
                        },
                        _ => {
                            let body = current_body(&mut intro, &mut sections);
                            body.push_str(&text);
                            body.push('\n');
                        }
                    }
                }
                Event::Text(text) | Event::Code(text) if !in_code => match heading.as_mut() {
                    Some((_, h)) => h.push_str(&text),
                    None => current_body(&mut intro, &mut sections).push_str(&text),
                },
                Event::SoftBreak | Event::End(TagEnd::TableCell) => match heading.as_mut() {
                    Some((_, h)) => h.push(' '),
                    None => current_body(&mut intro, &mut sections).push(' '),
                },
                Event::HardBreak
                | Event::End(
                    TagEnd::Paragraph | TagEnd::Item | TagEnd::TableRow | TagEnd::TableHead,
                ) if heading.is_none() => end_line(current_body(&mut intro, &mut sections)),
                _ => {}
            }
        }

        let deck = is_deck_genre(&front_matter);
        let title = title.unwrap_or_else(|| fallback_title.to_string());
        let intro = intro.trim().to_string();
        if deck {
            // Slides with neither heading, body, nor a world fence (a
            // trailing or doubled `---`) drop out.
            sections.retain(|(heading, body, world)| {
                !heading.trim().is_empty() || !body.trim().is_empty() || world.is_some()
            });
            // Everything before the first separator was the title slide.
            if seen_rule && (!intro.is_empty() || intro_world.is_some()) {
                sections.insert(0, (title.clone(), intro.clone(), intro_world.take()));
            }
        } else if let Some(world) = intro_world.take() {
            // A fence before the first heading belongs to the first section.
            match sections.first_mut() {
                Some((_, _, slot)) => *slot = Some(world),
                None => intro_world = Some(world), // intro-only doc, below
            }
        }
        let mut sections: Vec<Section> = sections
            .into_iter()
            .map(|(heading, body, world)| Section::new(heading, &body, world.as_deref()))
            .collect();
        if sections.is_empty() && (!intro.is_empty() || intro_world.is_some()) {
            sections.push(Section::new(title.clone(), &intro, intro_world.as_deref()));
        }
        Self {
            title,
            front_matter,
            intro,
            sections,
        }
    }

    /// The `genre` front-matter key, `world` when absent. `world` and `deck`
    /// are implemented (PLAN.md M4).
    pub fn genre(&self) -> &str {
        self.front_matter
            .get("genre")
            .map_or("world", String::as_str)
    }

    /// True when front matter says `genre: deck` — sections are slides split
    /// on `---` separators, laid out along a straight presentation path.
    pub fn is_deck(&self) -> bool {
        self.genre() == "deck"
    }
}

fn is_deck_genre(front_matter: &BTreeMap<String, String>) -> bool {
    front_matter.get("genre").is_some_and(|g| g == "deck")
}

/// Attach a captured ```` ```world ```` fence to the current section — or,
/// before any section exists, stash it for the first one.
fn attach_world(
    sections: &mut [(String, String, Option<String>)],
    intro_world: &mut Option<String>,
    text: &str,
) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    let slot = match sections.last_mut() {
        Some((_, _, world)) => world,
        None => intro_world,
    };
    *slot = Some(match slot.take() {
        Some(prev) => format!("{prev}\n{text}"),
        None => text,
    });
}

/// The text currently being appended to: the last section, or the intro.
fn current_body<'a>(
    intro: &'a mut String,
    sections: &'a mut [(String, String, Option<String>)],
) -> &'a mut String {
    match sections.last_mut() {
        Some((_, body, _)) => body,
        None => intro,
    }
}

fn end_line(body: &mut String) {
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
}

/// `key: value` lines; surrounding quotes are dropped and `#` lines skipped.
fn parse_front_matter(text: &str, out: &mut BTreeMap<String, String>) {
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            out.insert(key.trim().to_string(), value.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "---
genre: world
note: \"quoted\"
---

# The Title

Intro line.

## First

A paragraph
that wraps.

- one
- two

### Detail

More.

```rust
let code = 1;
```

## Second

Text with `inline` code.
";

    const DECK: &str = r#"---
genre: deck
---

# Markdown, as a place

A talk you can walk through.

---

## One file, one world

- every section is a place
- save the file and it rebuilds

---

## Read by a local model

Runs the recipe tier headless:

```bash
cargo run --features llm-metal -- deck.md --generate
```

---

## Thank you

github.com/localgpt-app/localgpt-md
"#;

    #[test]
    fn title_front_matter_and_sections() {
        let doc = Doc::parse(SAMPLE, "fallback");
        assert_eq!(doc.title, "The Title");
        assert_eq!(doc.genre(), "world");
        assert_eq!(doc.front_matter["note"], "quoted");
        assert_eq!(doc.intro, "Intro line.");
        let headings: Vec<_> = doc.sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["First", "Second"]);
        assert_eq!(
            doc.sections[0].body,
            "A paragraph that wraps.\none\ntwo\nDetail\nMore."
        );
        assert_eq!(doc.sections[1].body, "Text with inline code.");
    }

    #[test]
    fn deck_splits_on_rule_separators() {
        let doc = Doc::parse(DECK, "deck");
        assert!(doc.is_deck());
        assert_eq!(doc.title, "Markdown, as a place");
        let headings: Vec<_> = doc.sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(
            headings,
            [
                "Markdown, as a place",
                "One file, one world",
                "Read by a local model",
                "Thank you",
            ]
        );
        // The title slide carries the pre-separator prose as its body.
        assert_eq!(doc.sections[0].body, "A talk you can walk through.");
        assert_eq!(
            doc.sections[1].body,
            "every section is a place\nsave the file and it rebuilds"
        );
        // Fenced code is not scenery.
        assert_eq!(doc.sections[2].body, "Runs the recipe tier headless:");
        assert_eq!(doc.sections[3].body, "github.com/localgpt-app/localgpt-md");
    }

    #[test]
    fn deck_without_separators_is_one_slide() {
        let doc = Doc::parse("---\ngenre: deck\n---\n\n# T\n\none.\n\ntwo.", "d");
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].heading, "T");
        assert_eq!(doc.sections[0].body, "one.\ntwo.");
    }

    #[test]
    fn deck_drops_empty_slides() {
        // A trailing separator (and a doubled one) yields no empty slide.
        // Separators are blank-line padded, as Marp decks are — a bare `---`
        // directly under text is a CommonMark setext H2, not a separator.
        let doc = Doc::parse("---\ngenre: deck\n---\n\n# T\n\ns\n\n---\n\n---\n", "d");
        let headings: Vec<_> = doc.sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["T"]);
        assert_eq!(doc.sections[0].body, "s");
    }

    #[test]
    fn world_genre_ignores_rule_separators() {
        let doc = Doc::parse("# T\n\nabove\n\n---\n\nbelow\n\n## S\n", "d");
        assert!(!doc.is_deck());
        let headings: Vec<_> = doc.sections.iter().map(|s| s.heading.as_str()).collect();
        assert_eq!(headings, ["S"]);
        assert_eq!(doc.intro, "above\nbelow");
    }

    #[test]
    fn no_headings_is_one_section_named_by_fallback() {
        let doc = Doc::parse("Just a note.\n\nTwo paragraphs.", "note");
        assert_eq!(doc.title, "note");
        assert_eq!(doc.genre(), "world");
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].heading, "note");
        assert_eq!(doc.sections[0].body, "Just a note.\nTwo paragraphs.");
    }

    #[test]
    fn hash_tracks_each_section() {
        let before = Doc::parse(SAMPLE, "t");
        let after = Doc::parse(&SAMPLE.replace("More.", "Much more."), "t");
        assert_ne!(before.sections[0].hash, after.sections[0].hash);
        assert_eq!(before.sections[1].hash, after.sections[1].hash);
        assert_eq!(before.sections[1].seed(), after.sections[1].seed());
    }

    #[test]
    fn deck_slide_hash_tracks_edits() {
        let before = Doc::parse(DECK, "deck");
        let after = Doc::parse(&DECK.replace("save the file", "saving the file"), "deck");
        assert_ne!(before.sections[1].hash, after.sections[1].hash);
        assert_eq!(before.sections[0].hash, after.sections[0].hash);
        assert_eq!(before.sections[3].hash, after.sections[3].hash);
    }

    #[test]
    fn cjk_text_survives() {
        let doc = Doc::parse("# 花园\n\n## 第一章\n\n雾。\n", "t");
        assert_eq!(doc.title, "花园");
        assert_eq!(doc.sections[0].heading, "第一章");
        assert_eq!(doc.sections[0].body, "雾。");
    }

    const FENCED: &str =
        "## The Place\n\nProse.\n\n```world\n[{\"name\":\"x\"}]\n```\n\nMore prose.\n";

    #[test]
    fn world_fence_is_captured_and_excluded_from_body() {
        let doc = Doc::parse(FENCED, "t");
        assert_eq!(doc.sections[0].body, "Prose.\nMore prose.");
        assert_eq!(doc.sections[0].world.as_deref(), Some("[{\"name\":\"x\"}]"));
    }

    #[test]
    fn editing_only_the_fence_rekeys_the_section() {
        let before = Doc::parse(FENCED, "t");
        let after = Doc::parse(&FENCED.replace("\"x\"", "\"y\""), "t");
        assert_eq!(before.sections[0].body, after.sections[0].body);
        assert_ne!(before.sections[0].hash, after.sections[0].hash);
    }

    #[test]
    fn world_fence_before_any_section_attaches_to_the_first() {
        let doc = Doc::parse("# T\n\n```world\n[1]\n```\n\n## One\n\nprose\n", "t");
        assert_eq!(doc.sections[0].heading, "One");
        assert_eq!(doc.sections[0].world.as_deref(), Some("[1]"));
        assert!(doc.sections.get(1).is_none());
    }

    #[test]
    fn other_fenced_languages_are_still_ignored() {
        let doc = Doc::parse("## S\n\n```rust\nlet x = 1;\n```\n", "t");
        assert_eq!(doc.sections[0].body, "");
        assert!(doc.sections[0].world.is_none());
    }
}
