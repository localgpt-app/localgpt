//! Which entities a phrase means: "the red chair on the left", "both
//! lamps", "every wall except the back one".
//!
//! A person looking at a world says "the left one"; the agent beside them
//! has a document of ids and coordinates. `selection` hands the agent what
//! the person clicked, and this hands it what they said — resolved by rule,
//! with no model: names (whole, or a word of one, singular or plural),
//! shape and colour words, sides and distance as a [`View`] has them, height
//! and size, a count, an ordinal, lists and exceptions.
//!
//! It answers with entities only when the phrase picks them out. When it
//! could mean several ("the chair", of two), the answer is the candidates
//! and a question, and no guess; a word outside the rules ("next to",
//! "behind") comes back under `unknown`, for the agent to settle with what
//! it knows. A resolver that guessed would be an edit to the wrong thing.

use std::collections::BTreeSet;

use localgpt_world_types as wt;
use serde_json::{Value, json};
use wt::cinematography as cinema;

use crate::lint::{Placed, V3, cross, dot, length, place, sub, unit};

/// Where "left", "right", "front" and "back" are seen from.
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub position: V3,
    pub forward: V3,
    pub right: V3,
    /// What the view is, for the answer to say: "the app's view", "shot 1A".
    pub label: String,
}

impl View {
    fn looking(position: V3, forward: V3, label: impl Into<String>) -> Option<Self> {
        let forward = unit(forward)?;
        // Level: left and right are read on the ground, even looking down.
        let right = unit(cross(forward, [0.0, 1.0, 0.0]))?;
        Some(Self {
            position,
            forward,
            right,
            label: label.into(),
        })
    }

    /// The view a visitor opens on.
    pub fn opening(camera: &wt::CameraDef) -> Option<Self> {
        let position = camera.position.map(f64::from);
        Self::looking(
            position,
            sub(camera.look_at.map(f64::from), position),
            "the world's opening view",
        )
    }

    /// The view through a camera entity: a shot, or any entity carrying
    /// `ext-cinematography.camera`.
    pub fn through(entity: &wt::WorldEntity, manifest: &wt::WorldManifest) -> Option<Self> {
        let camera = cinema::CameraComponent::of(entity)?;
        let view = cinema::view_in(entity, &camera, manifest);
        Self::looking(
            view.position,
            view.forward,
            format!("shot {}", entity.name.as_str()),
        )
    }

    /// The view an open app reports beside the selection: `{"position":
    /// [x, y, z], "forward": [x, y, z]}`.
    pub fn from_json(view: &Value) -> Option<Self> {
        let vec3 = |key: &str| -> Option<V3> {
            let v: Vec<f64> = view
                .get(key)?
                .as_array()?
                .iter()
                .filter_map(Value::as_f64)
                .collect();
            (v.len() == 3).then(|| [v[0], v[1], v[2]])
        };
        Self::looking(vec3("position")?, vec3("forward")?, "the app's view")
    }

    /// A world point as the view has it: to the right, and ahead.
    fn sees(&self, p: V3) -> (f64, f64) {
        let d = sub(p, self.position);
        (dot(d, self.right), dot(d, self.forward))
    }
}

/// One entity an answer names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub id: u64,
    pub name: String,
}

/// What a phrase resolved to.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Found {
    /// The entities the phrase means; empty when it means none, or when it
    /// could mean several and `question` asks which.
    pub matches: Vec<Match>,
    /// The entities to choose between, when the phrase did not decide.
    pub candidates: Vec<Match>,
    /// What to ask the person, when the phrase did not decide.
    pub question: Option<String>,
    /// How each part of the phrase was read.
    pub read: Vec<String>,
    /// Words outside the rules: relations ("behind"), pronouns ("this"),
    /// anything else. With any of these the answer is at best partial.
    pub unknown: Vec<String>,
    /// What sides and distance were taken from, when the phrase used them.
    pub view: Option<String>,
}

impl Found {
    pub fn to_json(&self) -> Value {
        let list = |matches: &[Match]| -> Vec<Value> {
            matches
                .iter()
                .map(|m| json!({"id": m.id, "name": m.name}))
                .collect()
        };
        json!({
            "matches": list(&self.matches),
            "candidates": list(&self.candidates),
            "question": self.question,
            "read": self.read,
            "unknown": self.unknown,
            "view": self.view,
        })
    }

    /// The answer as short lines: a line per entity, or the question.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .matches
            .iter()
            .map(|m| format!("#{} {}", m.id, m.name))
            .collect();
        if let Some(question) = &self.question {
            lines.push(question.clone());
        } else if self.matches.is_empty() && self.unknown.is_empty() {
            // With a word not understood, the line below is the answer:
            // nothing was looked for.
            lines.push("nothing in the world matches".into());
        }
        if !self.unknown.is_empty() {
            lines.push(format!(
                "not understood: {} — settle {} yourself (`localgpt world show` has every entity's place)",
                self.unknown.join(", "),
                if self.unknown.len() == 1 { "it" } else { "them" }
            ));
        }
        if let Some(view) = &self.view {
            lines.push(format!("sides and distance as seen from {view}"));
        }
        lines
    }
}

/// A way to order candidates and take from one end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Front,
    Back,
    Top,
    Bottom,
    Big,
    Small,
    Tall,
    Short,
}

impl Side {
    fn of(word: &str) -> Option<Self> {
        Some(match word {
            "left" | "leftmost" | "lefthand" => Self::Left,
            "right" | "rightmost" | "righthand" => Self::Right,
            "front" | "nearest" | "closest" | "frontmost" | "foreground" => Self::Front,
            "back" | "farthest" | "furthest" | "rear" | "backmost" | "background" => Self::Back,
            "top" | "highest" | "upper" | "topmost" | "uppermost" => Self::Top,
            "bottom" | "lowest" | "lower" | "bottommost" | "lowermost" => Self::Bottom,
            "big" | "bigger" | "biggest" | "large" | "larger" | "largest" => Self::Big,
            "small" | "smaller" | "smallest" | "little" | "tiny" => Self::Small,
            "tall" | "taller" | "tallest" => Self::Tall,
            "short" | "shorter" | "shortest" => Self::Short,
            _ => return None,
        })
    }

    fn needs_view(self) -> bool {
        matches!(self, Self::Left | Self::Right | Self::Front | Self::Back)
    }

    fn said(self) -> &'static str {
        match self {
            Self::Left => "leftmost",
            Self::Right => "rightmost",
            Self::Front => "nearest",
            Self::Back => "farthest",
            Self::Top => "highest",
            Self::Bottom => "lowest",
            Self::Big => "largest",
            Self::Small => "smallest",
            Self::Tall => "tallest",
            Self::Short => "shortest",
        }
    }
}

const STOP: &[&str] = &[
    "the", "a", "an", "of", "in", "on", "at", "to", "that", "which", "is", "are", "one", "ones",
    "thing", "things", "object", "objects", "entity", "entities", "item", "items", "please",
    "select", "find", "pick", "choose", "show", "me", "from", "side", "hand", "most", "with", "as",
    "seen", "by", "coloured", "colored", "color", "colour", "shaped", "called", "named",
];

/// Words that tie one thing to another, or point: outside the rules here.
const RELATION: &[&str] = &[
    "near",
    "next",
    "beside",
    "behind",
    "under",
    "underneath",
    "below",
    "above",
    "over",
    "between",
    "inside",
    "outside",
    "around",
    "facing",
    "opposite",
    "this",
    "that's",
    "these",
    "those",
    "it",
    "them",
    "they",
    "selected",
    "selection",
    "here",
    "there",
];

fn number(word: &str) -> Option<usize> {
    const WORDS: [&str; 11] = [
        "zero", "single", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    match word {
        "couple" | "pair" | "both" => Some(2),
        word => WORDS
            .iter()
            .position(|w| *w == word)
            .filter(|n| *n > 0)
            .or_else(|| word.parse().ok().filter(|n| (1..=1000).contains(n))),
    }
}

fn ordinal(word: &str) -> Option<usize> {
    const WORDS: [&str; 10] = [
        "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
        "tenth",
    ];
    WORDS
        .iter()
        .position(|w| *w == word)
        .map(|n| n + 1)
        .or_else(|| {
            let digits = word
                .strip_suffix("st")
                .or_else(|| word.strip_suffix("nd"))
                .or_else(|| word.strip_suffix("rd"))
                .or_else(|| word.strip_suffix("th"))?;
            digits.parse().ok().filter(|n| (1..=1000).contains(n))
        })
}

/// A shape, or a kind of entity, by its everyday word.
fn kind_of(word: &str) -> Option<&'static str> {
    Some(match word {
        "cube" | "box" | "block" | "cuboid" | "crate" => "cuboid",
        "sphere" | "ball" | "orb" => "sphere",
        "cylinder" | "pillar" | "column" | "pole" => "cylinder",
        "cone" => "cone",
        // A stand-in figure is a capsule: what a draft's cast is made of.
        "capsule" | "pill" | "actor" | "character" | "figure" | "person" => "capsule",
        "torus" | "ring" | "donut" | "doughnut" => "torus",
        "plane" => "plane",
        "pyramid" => "pyramid",
        "tetrahedron" => "tetrahedron",
        "icosahedron" => "icosahedron",
        "wedge" | "ramp" => "wedge",
        "prism" => "prism",
        "light" | "lamp" => "light",
        "camera" | "shot" => "camera",
        "mesh" | "model" => "mesh",
        _ => return None,
    })
}

const COLOURS: &[&str] = &[
    "red", "orange", "yellow", "green", "cyan", "blue", "purple", "pink", "brown", "white", "grey",
    "black",
];

fn colour_word(word: &str) -> Option<&'static str> {
    let word = match word {
        "gray" | "silver" => "grey",
        "violet" => "purple",
        "teal" | "turquoise" | "aqua" => "cyan",
        "magenta" => "pink",
        "gold" | "golden" => "yellow",
        word => word,
    };
    COLOURS.iter().copied().find(|c| *c == word)
}

/// The everyday name of an sRGB colour.
pub fn colour_name(color: [f32; 4]) -> &'static str {
    let [r, g, b] = [color[0], color[1], color[2]].map(|c| f64::from(c).clamp(0.0, 1.0));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let (value, chroma) = (max, max - min);
    let saturation = if max > 0.0 { chroma / max } else { 0.0 };
    if value < 0.15 {
        return "black";
    }
    if saturation < 0.15 {
        return if value > 0.85 { "white" } else { "grey" };
    }
    let hue = 60.0
        * if max == r {
            ((g - b) / chroma).rem_euclid(6.0)
        } else if max == g {
            (b - r) / chroma + 2.0
        } else {
            (r - g) / chroma + 4.0
        };
    match hue {
        h if !(15.0..345.0).contains(&h) => {
            if saturation < 0.5 && value > 0.8 {
                "pink"
            } else {
                "red"
            }
        }
        h if h < 45.0 => {
            if value < 0.65 {
                "brown"
            } else {
                "orange"
            }
        }
        h if h < 70.0 => "yellow",
        h if h < 165.0 => "green",
        h if h < 200.0 => "cyan",
        h if h < 260.0 => "blue",
        h if h < 290.0 => "purple",
        _ => "pink",
    }
}

/// The words of a name: `wall-back` is "wall" and "back", `ChairLeft2` is
/// "chair", "left" and "2".
fn name_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut last: Option<char> = None;
    for c in name.chars() {
        let breaks = !c.is_alphanumeric()
            || last.is_some_and(|l| {
                (l.is_lowercase() && c.is_uppercase()) || (l.is_alphabetic() != c.is_alphabetic())
            });
        if breaks && !word.is_empty() {
            words.push(std::mem::take(&mut word).to_lowercase());
        }
        if c.is_alphanumeric() {
            word.push(c);
        }
        last = Some(c);
    }
    if !word.is_empty() {
        words.push(word.to_lowercase());
    }
    words
}

/// `chairs` → `chair`, `boxes` → `box`, `glass` stays.
fn singular(word: &str) -> Option<&str> {
    if word.len() > 3 && !word.ends_with("ss") {
        if let Some(stem) = word.strip_suffix("es")
            && (stem.ends_with('x') || stem.ends_with("ch") || stem.ends_with("sh"))
        {
            return Some(stem);
        }
        return word.strip_suffix('s');
    }
    None
}

/// One entity, as a phrase can pick it out.
struct Thing<'a> {
    placed: &'a Placed<'a>,
    words: Vec<String>,
    kinds: Vec<&'static str>,
    colour: Option<&'static str>,
}

impl Thing<'_> {
    fn id(&self) -> u64 {
        self.placed.entity.id.0
    }

    fn matched(&self) -> Match {
        Match {
            id: self.id(),
            name: self.placed.name().to_string(),
        }
    }

    /// Top and bottom of its box, or its origin's height when it has none.
    fn heights(&self) -> (f64, f64) {
        match self.placed.bounds {
            Some([min, max]) => (min[1], max[1]),
            None => {
                let y = self.placed.origin()[1];
                (y, y)
            }
        }
    }

    fn volume(&self) -> f64 {
        self.placed.bounds.map_or(0.0, |[min, max]| {
            (0..3).map(|i| (max[i] - min[i]).max(1e-3)).product()
        })
    }
}

/// One item of a list: "the two red chairs on the left".
#[derive(Debug, Default)]
struct Item {
    /// Entities named outright, by whole name or `#id`.
    exact: BTreeSet<u64>,
    names: Vec<String>,
    kinds: Vec<&'static str>,
    colours: Vec<&'static str>,
    sides: Vec<Side>,
    count: Option<usize>,
    ordinal: Option<usize>,
    /// "all", "every", a plural: every candidate, when nothing narrows it.
    all: bool,
    plural: bool,
    unknown: Vec<String>,
    /// A word that ties this to something else ("behind", "next to") or
    /// points ("this"): the item means more than its other words say.
    relation: bool,
}

impl Item {
    fn has_noun(&self) -> bool {
        !self.exact.is_empty() || !self.names.is_empty() || !self.kinds.is_empty()
    }

    fn is_empty(&self) -> bool {
        !self.has_noun() && self.colours.is_empty() && self.sides.is_empty() && !self.all
    }

    /// Whether what was understood is the whole of what was said. A
    /// relation changes what the other words pick ("the thing behind KAI"
    /// is not KAI), and a word outside the rules where the noun should be
    /// ("the gizmo on the left") leaves the rest picking from everything.
    /// Neither is answered in part: a part of an answer reads as the answer.
    fn understood(&self) -> bool {
        !self.relation && (self.unknown.is_empty() || self.has_noun())
    }
}

fn clean(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_alphanumeric() && c != '#')
}

/// Read one item's words against the world's names.
fn read_item(words: &[&str], things: &[Thing<'_>]) -> Item {
    let mut item = Item::default();
    let is_name_word = |word: &str| things.iter().any(|t| t.words.iter().any(|w| w == word));
    for raw in words {
        let raw = clean(raw);
        if raw.is_empty() {
            continue;
        }
        let word = raw.to_lowercase();
        // A whole name, or an id, is that entity whatever else the word
        // could mean: a shot named "1A", a prop named "left".
        if let Some(id) = raw.strip_prefix('#').and_then(|id| id.parse::<u64>().ok()) {
            item.exact.insert(id);
            continue;
        }
        let whole: Vec<u64> = things
            .iter()
            .filter(|t| t.placed.name().eq_ignore_ascii_case(raw))
            .map(Thing::id)
            .collect();
        if !whole.is_empty() {
            item.exact.extend(whole);
            continue;
        }
        if matches!(
            word.as_str(),
            "all" | "every" | "each" | "everything" | "any"
        ) {
            item.all = true;
            continue;
        }
        if STOP.contains(&word.as_str()) {
            continue;
        }
        if RELATION.contains(&word.as_str()) {
            item.unknown.push(word);
            item.relation = true;
            continue;
        }
        if let Some(n) = ordinal(&word) {
            item.ordinal = Some(n);
            continue;
        }
        if word == "last" {
            item.ordinal = Some(usize::MAX);
            continue;
        }
        // "chair 2" of `chair-2` is its name's word before it is a count.
        let digits = word.chars().all(|c| c.is_ascii_digit());
        if let Some(n) = number(&word).filter(|_| !(digits && is_name_word(&word))) {
            item.count = Some(n);
            item.plural = n > 1;
            if word == "both" {
                item.all = true;
            }
            continue;
        }
        if let Some(side) = Side::of(&word) {
            item.sides.push(side);
            continue;
        }
        if let Some(colour) = colour_word(&word) {
            item.colours.push(colour);
            continue;
        }
        // A word of a name, then a shape's word; in the plural too.
        let forms = [Some(word.as_str()), singular(&word)];
        let named = forms.iter().flatten().find(|form| is_name_word(form));
        if let Some(form) = named {
            item.plural |= *form != word;
            item.names.push(form.to_string());
            continue;
        }
        let kind = forms
            .iter()
            .flatten()
            .find_map(|form| kind_of(form).map(|kind| (kind, *form != word)));
        if let Some((kind, plural)) = kind {
            item.plural |= plural;
            item.kinds.push(kind);
            continue;
        }
        item.unknown.push(word);
    }
    item
}

/// What one item resolved to.
enum Picked {
    These(Vec<u64>),
    /// Several fit and nothing chose: the candidates, and how many were
    /// asked for.
    Unsure(Vec<u64>, usize),
    /// A side was named and there is no view to read it from.
    NoView(Vec<u64>),
}

fn pick(item: &Item, things: &[Thing<'_>], view: Option<&View>, read: &mut Vec<String>) -> Picked {
    let mut pool: Vec<&Thing<'_>> = things
        .iter()
        .filter(|t| item.exact.is_empty() || item.exact.contains(&t.id()))
        .filter(|t| item.names.iter().all(|n| t.words.iter().any(|w| w == n)))
        .filter(|t| item.kinds.iter().all(|k| t.kinds.contains(k)))
        .filter(|t| item.colours.iter().all(|c| t.colour == Some(*c)))
        // With no noun, "the one on the left" is something you can see.
        .filter(|t| item.has_noun() || t.placed.bounds.is_some())
        .collect();
    let mut said: Vec<String> = Vec::new();
    said.extend(item.colours.iter().map(|c| c.to_string()));
    said.extend(item.names.iter().map(|n| format!("named \"{n}\"")));
    said.extend(item.kinds.iter().map(|k| format!("a {k}")));
    if !item.exact.is_empty() {
        said.push("by name".into());
    }
    let what = if said.is_empty() {
        "anything with a shape".to_string()
    } else {
        said.join(", ")
    };
    read.push(format!("{what}: {} in the world", pool.len()));
    if pool.is_empty() {
        return Picked::These(Vec::new());
    }

    // A side with no view to read it from: a word of a name instead, when
    // one of the candidates carries it ("chair-left"), else it waits.
    let mut sides = item.sides.clone();
    if view.is_none() && sides.iter().any(|s| s.needs_view()) {
        let word = |side: Side| match side {
            Side::Left => "left",
            Side::Right => "right",
            Side::Front => "front",
            _ => "back",
        };
        let by_name: Vec<&Thing<'_>> = pool
            .iter()
            .copied()
            .filter(|t| {
                sides
                    .iter()
                    .filter(|s| s.needs_view())
                    .all(|s| t.words.iter().any(|w| w == word(*s)))
            })
            .collect();
        if by_name.is_empty() {
            return Picked::NoView(pool.iter().map(|t| t.id()).collect());
        }
        read.push("no view: the side is read from the names".into());
        pool = by_name;
        sides.retain(|s| !s.needs_view());
    }

    // What you see is what "left" is said of: when anything is ahead of the
    // view, the things behind it are not candidates for a side.
    if let Some(view) = view
        && sides.iter().any(|s| s.needs_view())
    {
        let ahead: Vec<&Thing<'_>> = pool
            .iter()
            .copied()
            .filter(|t| view.sees(t.placed.origin()).1 > 0.0)
            .collect();
        if !ahead.is_empty() {
            pool = ahead;
        }
    }

    let key = |side: Side, t: &Thing<'_>| -> f64 {
        let seen = |t: &Thing<'_>| view.map_or((0.0, 0.0), |v| v.sees(t.placed.origin()));
        match side {
            // Where it lands across the frame: its bearing, not its metres.
            Side::Left => {
                let (x, z) = seen(t);
                if z > 0.0 { x / z } else { x }
            }
            Side::Right => {
                let (x, z) = seen(t);
                -(if z > 0.0 { x / z } else { x })
            }
            Side::Front => view.map_or(0.0, |v| length(sub(t.placed.origin(), v.position))),
            Side::Back => -view.map_or(0.0, |v| length(sub(t.placed.origin(), v.position))),
            Side::Top => -t.heights().1,
            Side::Bottom => t.heights().0,
            Side::Big => -t.volume(),
            Side::Small => t.volume(),
            Side::Tall => -(t.heights().1 - t.heights().0),
            Side::Short => t.heights().1 - t.heights().0,
        }
    };
    let sort_by = |pool: &mut Vec<&Thing<'_>>, side: Side| {
        pool.sort_by(|a, b| {
            key(side, a)
                .partial_cmp(&key(side, b))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id().cmp(&b.id()))
        });
    };

    // Two things as far left as each other, or as tall: an order that does
    // not separate them does not choose between them. The entries of a
    // sorted pool that tie with the one at `index`, when any other does.
    let tied = |pool: &[&Thing<'_>], side: Side, index: usize| -> Vec<u64> {
        let at = key(side, pool[index]);
        let group: Vec<u64> = pool
            .iter()
            .filter(|t| (key(side, t) - at).abs() <= 1e-9 * (1.0 + at.abs()))
            .map(|t| t.id())
            .collect();
        if group.len() > 1 { group } else { Vec::new() }
    };

    // "the second from the left", "the third chair": one, by its place in
    // an order — across the view when there is one, else as made.
    if let Some(nth) = item.ordinal {
        let side = sides.first().copied().or_else(|| view.map(|_| Side::Left));
        let order = match side {
            Some(side) => {
                sort_by(&mut pool, side);
                format!("from the {}", side.said())
            }
            None => {
                pool.sort_by_key(|t| t.id());
                "in the order made".to_string()
            }
        };
        let index = if nth == usize::MAX {
            pool.len() - 1
        } else {
            nth - 1
        };
        let Some(thing) = pool.get(index) else {
            read.push(format!("there are only {}", pool.len()));
            return Picked::These(Vec::new());
        };
        if let Some(side) = side {
            let tie = tied(&pool, side, index);
            if !tie.is_empty() {
                read.push(format!("number {} {order} is a tie", index + 1));
                return Picked::Unsure(tie, 1);
            }
        }
        read.push(format!("number {} of {} {order}", index + 1, pool.len()));
        return Picked::These(vec![thing.id()]);
    }

    let wanted = item.count.unwrap_or(1);
    for side in &sides {
        sort_by(&mut pool, *side);
        // A count takes that many from the end; "the chairs on the left"
        // takes the half that is there; otherwise the one at the end.
        let take = match (item.count, item.plural) {
            (Some(n), _) => n,
            (None, true) => pool.len().div_ceil(2),
            (None, false) => 1,
        };
        // The cut falls between two that tie: which of them is a question.
        if take < pool.len() {
            let tie = tied(&pool, *side, take - 1);
            if tie.contains(&pool[take].id()) {
                let settled = pool.iter().position(|t| tie.contains(&t.id())).unwrap_or(0);
                read.push(format!("the {} is a tie", side.said()));
                return Picked::Unsure(tie, take - settled);
            }
        }
        read.push(format!(
            "the {} {} of {}",
            side.said(),
            take.min(pool.len()),
            pool.len()
        ));
        pool.truncate(take);
    }
    let ids: Vec<u64> = pool.iter().map(|t| t.id()).collect();
    if !sides.is_empty() {
        return Picked::These(ids);
    }
    // Nothing chose among them: every one of them, when the phrase asked
    // for all or for exactly as many as there are; else it is a question.
    let whole_set = item.all && item.count.is_none_or(|n| n == ids.len());
    let named_outright = !item.exact.is_empty();
    let plural_all = item.plural && item.count.is_none();
    if whole_set || named_outright || plural_all || ids.len() == wanted {
        Picked::These(ids)
    } else {
        Picked::Unsure(ids, wanted)
    }
}

/// Resolve `phrase` against a world. `view` is where sides and distance are
/// seen from: the person's, when an app has the world open.
pub fn find(manifest: &wt::WorldManifest, phrase: &str, view: Option<&View>) -> Found {
    let mut expanded;
    let manifest = if manifest.has_instances() {
        expanded = manifest.clone();
        expanded.expand_instances();
        &expanded
    } else {
        manifest
    };
    let world = place(manifest);
    let things: Vec<Thing<'_>> = world
        .iter()
        .map(|placed| {
            let entity = placed.entity;
            let mut kinds: Vec<&'static str> = Vec::new();
            if let Some(shape) = &entity.shape {
                kinds.push(shape.kind());
            }
            if entity.light.is_some() {
                kinds.push("light");
            }
            if cinema::CameraComponent::of(entity).is_some() {
                kinds.push("camera");
            }
            if entity.mesh_asset.is_some() {
                kinds.push("mesh");
            }
            Thing {
                placed,
                words: name_words(placed.name()),
                kinds,
                colour: entity
                    .material
                    .as_ref()
                    .filter(|_| entity.shape.is_some() || entity.mesh_asset.is_some())
                    .map(|m| colour_name(m.color)),
            }
        })
        .collect();

    let mut found = Found::default();
    // A comma ends an item whatever it is attached to.
    let spaced = phrase.replace(',', " , ");
    let words: Vec<&str> = spaced.split_whitespace().collect();
    // "… except …": what follows is taken out of what came before.
    let cut = (0..words.len()).find_map(|i| {
        let word = |i: usize| words.get(i).map(|w| w.to_lowercase()).unwrap_or_default();
        match (word(i).as_str(), word(i + 1).as_str()) {
            ("except" | "excluding", _) => Some((i, i + 1)),
            ("but", "not") | ("other", "than") | ("apart", "from") => Some((i, i + 2)),
            _ => None,
        }
    });
    let (keep, drop) = match cut {
        Some((at, after)) => (&words[..at], Some(&words[after..])),
        None => (&words[..], None),
    };

    let items_of = |words: &[&str]| -> Vec<Item> {
        // A list: items between commas and "and".
        let joins = |word: &str| {
            word == ","
                || ["and", "&", "plus"]
                    .iter()
                    .any(|j| word.eq_ignore_ascii_case(j))
        };
        let mut items: Vec<Item> = words
            .split(|word| joins(word))
            .filter(|item| !item.is_empty())
            .map(|item| read_item(item, &things))
            .filter(|item| !item.is_empty() || !item.unknown.is_empty())
            .collect();
        // "the red and blue chairs": a part with no noun takes the next
        // part's.
        for i in (0..items.len().saturating_sub(1)).rev() {
            if !items[i].has_noun() && !items[i].colours.is_empty() && items[i].sides.is_empty() {
                let (names, kinds, plural) = (
                    items[i + 1].names.clone(),
                    items[i + 1].kinds.clone(),
                    items[i + 1].plural,
                );
                items[i].names = names;
                items[i].kinds = kinds;
                items[i].plural |= plural;
            }
        }
        items
    };

    let mut chosen: Vec<u64> = Vec::new();
    for item in items_of(keep) {
        found.unknown.extend(item.unknown.iter().cloned());
        if item.is_empty() || !item.understood() {
            continue;
        }
        if item.sides.iter().any(|s| s.needs_view()) {
            found.view = view.map(|v| v.label.clone());
        }
        match pick(&item, &things, view, &mut found.read) {
            Picked::These(ids) => chosen.extend(ids),
            Picked::Unsure(ids, wanted) => {
                found.candidates = matches_of(&things, &ids);
                found.question = Some(question(&found.candidates, wanted));
            }
            Picked::NoView(ids) => {
                found.candidates = matches_of(&things, &ids);
                found.question = Some(
                    "left, right, front and back need a view: open the world in the app, or name a camera (`--camera <entity>`)"
                        .into(),
                );
            }
        }
    }
    if let Some(drop) = drop {
        for mut item in items_of(drop) {
            found.unknown.extend(item.unknown.iter().cloned());
            if item.is_empty() || !item.understood() {
                // What was to be left out is not known, so neither is
                // what is left.
                if !item.is_empty() || !item.unknown.is_empty() {
                    chosen.clear();
                }
                continue;
            }
            // "except the red one" is every red one among those chosen.
            item.all = true;
            if let Picked::These(ids) | Picked::Unsure(ids, _) =
                pick(&item, &things, view, &mut found.read)
            {
                chosen.retain(|id| !ids.contains(id));
                found.read.push(format!("less {}", ids.len()));
            }
        }
    }
    if found.question.is_none() {
        let mut seen = BTreeSet::new();
        chosen.retain(|id| seen.insert(*id));
        found.matches = matches_of(&things, &chosen);
    }
    found.unknown.dedup();
    found
}

/// Where a caller wants sides and distance read from.
#[derive(Debug, Clone, Copy)]
pub enum Viewpoint<'a> {
    /// Through this camera entity, by name or `#id`: "left in shot 1A".
    Camera(&'a str),
    /// As an open app reports the person's view ([`View::from_json`]),
    /// when it does; else the world's opening view.
    App(Option<&'a Value>),
}

/// [`find`] for a world held as JSON — a manifest as `manifest.json` and
/// the open app's API give it — with the view chosen by `viewpoint`.
pub fn find_json(world: &Value, phrase: &str, viewpoint: Viewpoint<'_>) -> Result<Found, String> {
    let manifest: wt::WorldManifest = serde_json::from_value(world.clone())
        .map_err(|e| format!("that is not a world manifest: {e}"))?;
    let view = match viewpoint {
        Viewpoint::Camera(name) => {
            let id = name.strip_prefix('#').and_then(|id| id.parse::<u64>().ok());
            let entity = manifest
                .entities
                .iter()
                .find(|e| Some(e.id.0) == id || e.name.as_str().eq_ignore_ascii_case(name))
                .ok_or_else(|| format!("no entity is named \"{name}\""))?;
            Some(View::through(entity, &manifest).ok_or_else(|| {
                format!(
                    "{} is not a camera: it carries no `{}.camera`",
                    entity.name.as_str(),
                    cinema::EXTENSION_NAME
                )
            })?)
        }
        Viewpoint::App(view) => view
            .and_then(View::from_json)
            .or_else(|| manifest.camera.as_ref().and_then(View::opening)),
    };
    Ok(find(&manifest, phrase, view.as_ref()))
}

fn matches_of(things: &[Thing<'_>], ids: &[u64]) -> Vec<Match> {
    ids.iter()
        .filter_map(|id| things.iter().find(|t| t.id() == *id))
        .map(Thing::matched)
        .collect()
}

fn question(candidates: &[Match], wanted: usize) -> String {
    const NAMED: usize = 6;
    let mut names: Vec<&str> = candidates
        .iter()
        .take(NAMED)
        .map(|m| m.name.as_str())
        .collect();
    let more = candidates.len().saturating_sub(NAMED);
    let list = match (names.pop(), more) {
        (Some(last), 0) if !names.is_empty() => format!("{} or {last}", names.join(", ")),
        (Some(last), 0) => last.to_string(),
        (Some(last), more) => format!("{}, {last} or one of {more} more", names.join(", ")),
        (None, _) => String::new(),
    };
    match wanted {
        1 => format!("Which one: {list}?"),
        n => format!("Which {n} of {}: {list}?", candidates.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thing(
        id: u64,
        name: &str,
        shape: wt::Shape,
        at: [f32; 3],
        colour: [f32; 3],
    ) -> wt::WorldEntity {
        let mut e = wt::WorldEntity::new(id, name).with_shape(shape);
        e.transform.position = at;
        e.material = Some(wt::MaterialDef {
            color: [colour[0], colour[1], colour[2], 1.0],
            ..Default::default()
        });
        e
    }

    const RED: [f32; 3] = [0.8, 0.1, 0.1];
    const BLUE: [f32; 3] = [0.1, 0.2, 0.8];
    const GREY: [f32; 3] = [0.5, 0.5, 0.5];

    /// A room seen from +Z, looking at −Z: −X is the left of the view.
    fn room() -> wt::WorldManifest {
        let chair = |id, name: &str, x: f32, z: f32, colour| {
            thing(
                id,
                name,
                wt::Shape::Cuboid {
                    x: 0.5,
                    y: 0.9,
                    z: 0.5,
                },
                [x, 0.45, z],
                colour,
            )
        };
        let mut manifest = wt::WorldManifest::new("room");
        manifest.entities = vec![
            chair(1, "chair-a", -2.0, 0.0, RED),
            chair(2, "chair-b", 0.0, -1.0, RED),
            chair(3, "chair-c", 2.0, 0.0, BLUE),
            thing(
                4,
                "wall-back",
                wt::Shape::Cuboid {
                    x: 8.0,
                    y: 3.0,
                    z: 0.1,
                },
                [0.0, 1.5, -3.0],
                GREY,
            ),
            thing(
                5,
                "wall-left",
                wt::Shape::Cuboid {
                    x: 0.1,
                    y: 3.0,
                    z: 6.0,
                },
                [-4.0, 1.5, 0.0],
                GREY,
            ),
            thing(
                6,
                "table",
                wt::Shape::Cylinder {
                    radius: 0.6,
                    height: 0.75,
                },
                [0.0, 0.375, 1.0],
                [0.45, 0.3, 0.15],
            ),
            thing(
                7,
                "ball",
                wt::Shape::Sphere { radius: 0.2 },
                [1.0, 0.95, 1.0],
                BLUE,
            ),
        ];
        let mut lamp = wt::WorldEntity::new(8, "ceiling-lamp");
        lamp.transform.position = [0.0, 2.9, 0.0];
        lamp.light = Some(wt::LightDef::default());
        manifest.entities.push(lamp);
        manifest.next_entity_id = 9;
        manifest.camera = Some(wt::CameraDef {
            position: [0.0, 1.6, 6.0],
            look_at: [0.0, 1.0, 0.0],
            fov_degrees: 45.0,
        });
        manifest
    }

    fn names(found: &Found) -> Vec<&str> {
        found.matches.iter().map(|m| m.name.as_str()).collect()
    }

    fn seen(phrase: &str) -> Found {
        let manifest = room();
        let view = View::opening(manifest.camera.as_ref().unwrap());
        find(&manifest, phrase, view.as_ref())
    }

    #[test]
    fn a_name_a_word_of_a_name_a_shape_and_a_colour_each_pick() {
        assert_eq!(names(&seen("wall-back")), ["wall-back"]);
        assert_eq!(names(&seen("the table")), ["table"]);
        assert_eq!(names(&seen("#7")), ["ball"]);
        assert_eq!(
            names(&seen("the chairs")),
            ["chair-a", "chair-b", "chair-c"]
        );
        assert_eq!(names(&seen("all the walls")), ["wall-back", "wall-left"]);
        assert_eq!(names(&seen("the sphere")), ["ball"]);
        assert_eq!(names(&seen("the blue chair")), ["chair-c"]);
        assert_eq!(names(&seen("the red chairs")), ["chair-a", "chair-b"]);
        assert_eq!(names(&seen("the brown cylinder")), ["table"]);
        assert_eq!(names(&seen("the lamp")), ["ceiling-lamp"]);
        assert_eq!(names(&seen("everything blue")), ["chair-c", "ball"]);
    }

    #[test]
    fn sides_and_distance_are_the_views() {
        assert_eq!(names(&seen("the chair on the left")), ["chair-a"]);
        assert_eq!(names(&seen("the rightmost chair")), ["chair-c"]);
        assert_eq!(
            names(&seen("the two chairs on the left")),
            ["chair-a", "chair-b"]
        );
        assert_eq!(names(&seen("the nearest wall")), ["wall-left"]);
        // Two chairs as near as each other: the order does not choose
        // between them, so neither does the answer.
        let tie = seen("the nearest chair");
        assert!(tie.matches.is_empty(), "{tie:?}");
        assert_eq!(
            tie.question.as_deref(),
            Some("Which one: chair-a or chair-c?")
        );
        assert_eq!(names(&seen("the chair at the back")), ["chair-b"]);
        assert_eq!(names(&seen("the back wall")), ["wall-back"]);
        assert_eq!(names(&seen("the left wall")), ["wall-left"]);
        assert_eq!(names(&seen("the second chair from the left")), ["chair-b"]);
        assert_eq!(names(&seen("the last chair")), ["chair-c"]);
        assert_eq!(
            seen("the chair on the left").view.as_deref(),
            Some("the world's opening view")
        );

        // The same words from behind the back wall, looking the other way:
        // the other chair is on the left.
        let manifest = room();
        let behind =
            View::looking([0.0, 1.6, -6.0], [0.0, 0.0, 1.0], "a view from behind").unwrap();
        assert_eq!(
            names(&find(&manifest, "the chair on the left", Some(&behind))),
            ["chair-c"]
        );
    }

    #[test]
    fn height_and_size_need_no_view() {
        let manifest = room();
        let found = |phrase: &str| find(&manifest, phrase, None);
        assert_eq!(names(&found("the biggest thing")), ["wall-back"]);
        assert_eq!(names(&found("the smallest one")), ["ball"]);
        assert_eq!(
            names(&found("the two biggest things")),
            ["wall-back", "wall-left"]
        );
        // Three chairs of one height, two walls as high as each other: a
        // superlative that does not separate them is a question.
        assert_eq!(
            found("the tallest chair").question.as_deref(),
            Some("Which one: chair-a, chair-b or chair-c?")
        );
        assert_eq!(
            found("the highest thing").question.as_deref(),
            Some("Which one: wall-back or wall-left?")
        );
        assert_eq!(
            names(&found("the second chair")),
            ["chair-b"],
            "in the order made"
        );
        assert_eq!(found("the biggest thing").view, None);
    }

    #[test]
    fn what_could_mean_several_is_a_question_and_no_guess() {
        let found = seen("the chair");
        assert_eq!(names(&found), Vec::<&str>::new());
        assert_eq!(
            found.question.as_deref(),
            Some("Which one: chair-a, chair-b or chair-c?")
        );
        assert_eq!(found.candidates.len(), 3);

        let found = seen("two chairs");
        assert_eq!(
            found.question.as_deref(),
            Some("Which 2 of 3: chair-a, chair-b or chair-c?")
        );
        // Exactly as many as there are is not a question.
        assert_eq!(names(&seen("both red chairs")), ["chair-a", "chair-b"]);
        assert_eq!(names(&seen("the two walls")), ["wall-back", "wall-left"]);
        // And nothing is nothing.
        let found = seen("the green chair");
        assert!(
            found.matches.is_empty() && found.question.is_none(),
            "{found:?}"
        );
        assert_eq!(found.lines()[0], "nothing in the world matches");
    }

    #[test]
    fn a_side_without_a_view_waits_unless_a_name_carries_it() {
        let manifest = room();
        let found = find(&manifest, "the chair on the left", None);
        assert!(found.matches.is_empty());
        assert!(
            found.question.as_deref().unwrap().contains("need a view"),
            "{found:?}"
        );
        assert_eq!(found.candidates.len(), 3);
        // wall-left carries the word.
        assert_eq!(
            names(&find(&manifest, "the left wall", None)),
            ["wall-left"]
        );
    }

    #[test]
    fn lists_and_exceptions_add_and_take_away() {
        assert_eq!(names(&seen("the table and the ball")), ["table", "ball"]);
        assert_eq!(
            names(&seen("the table, the ball and the lamp")),
            ["table", "ball", "ceiling-lamp"]
        );
        assert_eq!(
            names(&seen("all the chairs except the blue one")),
            ["chair-a", "chair-b"]
        );
        assert_eq!(
            names(&seen("the chairs but not chair-b")),
            ["chair-a", "chair-c"]
        );
        assert_eq!(
            names(&seen("the red and blue chairs")),
            ["chair-a", "chair-b", "chair-c"]
        );
        assert_eq!(names(&seen("the table and the table")), ["table"], "once");
    }

    #[test]
    fn a_relation_or_a_pronoun_comes_back_as_not_understood() {
        // "behind the table" is not the table, and not every chair.
        let found = seen("the chair behind the table");
        assert_eq!(found.unknown, ["behind"]);
        assert!(
            found.matches.is_empty() && found.question.is_none(),
            "{found:?}"
        );
        let found = seen("the thing behind chair-a");
        assert!(found.matches.is_empty(), "not chair-a: {found:?}");
        let found = seen("this");
        assert_eq!(found.unknown, ["this"]);
        assert!(found.matches.is_empty());
        // A word where the noun should be leaves nothing to pick from:
        // "the leftmost anything" is not what was asked.
        let found = seen("the gizmo on the left");
        assert_eq!(found.unknown, ["gizmo"]);
        assert!(found.matches.is_empty(), "not the left wall: {found:?}");
        assert!(
            found.lines()[0].starts_with("not understood: gizmo"),
            "nothing was looked for, so nothing is said to be missing: {:?}",
            found.lines()
        );
        // With the noun understood, a word beside it is said and the rest
        // still answers.
        let found = seen("the wobbly chair on the left");
        assert_eq!(found.unknown, ["wobbly"]);
        assert_eq!(names(&found), ["chair-a"]);
        assert!(
            found
                .lines()
                .iter()
                .any(|l| l.starts_with("not understood: wobbly"))
        );
        // The rest of a list still answers, and an exception that was not
        // understood takes the whole answer with it.
        assert_eq!(names(&seen("the table and the thing behind it")), ["table"]);
        assert!(
            seen("the chairs except the one behind the table")
                .matches
                .is_empty()
        );
    }

    #[test]
    fn a_stand_in_is_an_actor() {
        let mut manifest = room();
        for (id, name, x) in [(20, "MAYA", -0.7_f32), (21, "KAI", 0.7)] {
            manifest.entities.push(thing(
                id,
                name,
                wt::Shape::Capsule {
                    radius: 0.18,
                    half_length: 0.67,
                },
                [x, 0.85, 2.0],
                RED,
            ));
        }
        let view = View::opening(manifest.camera.as_ref().unwrap());
        let found = |phrase: &str| find(&manifest, phrase, view.as_ref());
        assert_eq!(names(&found("the actor on the left")), ["MAYA"]);
        assert_eq!(names(&found("both actors")), ["MAYA", "KAI"]);
        assert_eq!(names(&found("the character on the right")), ["KAI"]);
    }

    #[test]
    fn a_shot_is_a_view() {
        let mut manifest = room();
        let mut shot = wt::WorldEntity::new(9, "1A");
        shot.transform.position = [-6.0, 1.6, 0.0];
        shot.extra.insert(
            cinema::EXTENSION_NAME.into(),
            json!({"camera": {"aim": [0, 1, 0]}}),
        );
        manifest.entities.push(shot);
        let view = View::through(manifest.entities.last().unwrap(), &manifest).unwrap();
        assert_eq!(view.label, "shot 1A");
        // From the left wall looking +X: −Z is on the left.
        assert_eq!(
            names(&find(&manifest, "the leftmost chair", Some(&view))),
            ["chair-b"]
        );
        assert_eq!(
            names(&find(&manifest, "the nearest chair", Some(&view))),
            ["chair-a"]
        );
        // The app's view arrives as JSON.
        let app =
            View::from_json(&json!({"position": [0, 1.6, 6], "forward": [0, 0, -1]})).unwrap();
        assert_eq!(
            names(&find(&manifest, "the chair on the right", Some(&app))),
            ["chair-c"]
        );
        assert_eq!(app.label, "the app's view");
    }

    #[test]
    fn colours_are_named_as_a_person_would() {
        let named = |r, g, b| colour_name([r, g, b, 1.0]);
        assert_eq!(named(0.8, 0.1, 0.1), "red");
        assert_eq!(named(0.1, 0.2, 0.8), "blue");
        assert_eq!(named(0.2, 0.7, 0.3), "green");
        assert_eq!(named(0.95, 0.85, 0.1), "yellow");
        assert_eq!(named(1.0, 0.55, 0.1), "orange");
        assert_eq!(named(0.45, 0.3, 0.15), "brown");
        assert_eq!(named(0.6, 0.2, 0.8), "purple");
        assert_eq!(named(1.0, 0.7, 0.8), "pink");
        assert_eq!(named(0.2, 0.8, 0.8), "cyan");
        assert_eq!(named(0.95, 0.95, 0.95), "white");
        assert_eq!(named(0.5, 0.5, 0.5), "grey");
        assert_eq!(named(0.02, 0.02, 0.02), "black");
    }

    #[test]
    fn names_break_into_words() {
        assert_eq!(name_words("wall-back"), ["wall", "back"]);
        assert_eq!(name_words("ChairLeft2"), ["chair", "left", "2"]);
        assert_eq!(name_words("MAYA.head"), ["maya", "head"]);
        assert_eq!(name_words("1A"), ["1", "a"]);
        assert_eq!(singular("chairs"), Some("chair"));
        assert_eq!(singular("boxes"), Some("box"));
        assert_eq!(singular("glass"), None);
    }
}
