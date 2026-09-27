//! Real 3D world props — PLAN.md M6.
//!
//! Loads a manifest of CC0 glTF models (bundled from the `verse-assets`
//! repo into `assets/models/`) and places them on the ground per world mood,
//! in three tiers (hero landmarks / medium props / ground scatter). These are
//! grounded *features*; the procedural drifters in `world.rs` stay as floating
//! ambient. When no manifest is present (fresh clone without the asset pack)
//! or a mood has no matching assets, the world simply keeps its primitives —
//! so the app always runs.

use bevy::camera::visibility::VisibilityRange;
use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;

use crate::theme::{Arrangement, Theme};

/// Placement tier — governs instance count, size, and spread.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Hero,
    Medium,
    Scatter,
}

impl Tier {
    /// Entity budget for the tier across a whole world (not per asset): 15
    /// heroes, 25 mediums, 27 scatter seeds. The original pack sized worlds by
    /// per-asset counts (5 heroes × 3, …); with the pool grown well past that,
    /// per-asset counts would push the scene over the measured ~90-prop
    /// comfort ceiling. The budget holds perf constant while
    /// [`populate_world_props`]'s weighted round-robin spends it across every
    /// variant the pool offers — more variety, same entity count.
    fn budget(self) -> usize {
        match self {
            Tier::Hero => 15,
            Tier::Medium => 25,
            Tier::Scatter => 27,
        }
    }
    /// Base scale multiplier (glTF are real-world metres; a touch larger so
    /// props read as landmarks without dwarfing the scene). Fallback when the
    /// manifest lacks native `dims` for a model.
    fn base_scale(self) -> f32 {
        match self {
            Tier::Hero => 1.5,
            Tier::Medium => 1.15,
            Tier::Scatter => 0.9,
        }
    }
    /// Target real-world span (metres) — models with known native size are
    /// rescaled to it, since Poly Haven scans range from 0.1 m (shell) to
    /// 90 m (cliff). Keeps every tier's footprint consistent across the pack.
    fn target_span(self) -> f32 {
        match self {
            Tier::Hero => 7.0,
            Tier::Medium => 2.5,
            Tier::Scatter => 1.0,
        }
    }
    /// Distance culling (ARCHITECTURE R7): ground cover drops out well inside
    /// the fog band (18–95), medium props just before the fog wall; hero
    /// landmarks stay visible — they define the skyline.
    pub(crate) fn visibility_range(self) -> Option<VisibilityRange> {
        match self {
            Tier::Hero => None,
            Tier::Medium => Some(VisibilityRange::abrupt(0.0, 80.0)),
            Tier::Scatter => Some(VisibilityRange::abrupt(0.0, 45.0)),
        }
    }
}

/// One manifest entry. Provenance fields (author/license/source) double as the
/// Credits screen's data + the CC0/CC-BY audit trail.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetEntry {
    /// Stable id / source URL — provenance kept in the manifest (audit trail)
    /// even though the UI currently renders only name/author/license.
    #[allow(dead_code)]
    pub id: String,
    pub name: String,
    /// glTF path relative to `assets/models/`.
    pub file: String,
    /// Semantic kind (`rock`, `tree`, `lamp`, …) — the stable small vocabulary
    /// the agent's `place_asset` enum exposes; variants behind it rotate
    /// ([`resolve_kind`]). Defaults for manifests predating kinds.
    #[serde(default)]
    pub kind: String,
    pub tier: Tier,
    /// Index into [`crate::theme::moods()`].
    ///
    /// Positional, and written by `fetch_polyhaven.py` in the separate
    /// `verse-assets` repo — so reordering [`crate::theme::moods()`] silently
    /// repoints all 52 assets, and nothing in this repo would catch it. Read
    /// through [`AssetEntry::mood_index`], which prefers `mood_id`.
    pub mood: usize,
    /// Stable mood id ([`crate::theme::WorldMood::id`]), when the manifest
    /// carries one.
    ///
    /// Not yet emitted by the generator: adding it there is a change in another
    /// repo, and the numeric field keeps working until it lands. Accepting it
    /// now means a regenerated manifest is understood without a code change
    /// here, and that a mood added or reordered in the meantime is a
    /// recoverable mistake rather than a silent one.
    #[serde(default)]
    pub mood_id: Option<String>,
    #[serde(default = "one")]
    pub scale: f32,
    /// Native dimensions in metres `[x, y, z]` (from the source catalog) —
    /// placement rescales to the tier's target span when present.
    #[serde(default)]
    pub dims: Option<[f32; 3]>,
    pub license: String,
    pub author: String,
    #[allow(dead_code)]
    pub source: String,
}

fn one() -> f32 {
    1.0
}

impl AssetEntry {
    /// The live mood index, preferring the stable id over the stored position.
    pub fn mood_index(&self) -> usize {
        crate::theme::resolve_mood(crate::theme::moods(), self.mood_id.as_deref(), self.mood)
    }

    /// Human tier name for the Credits row.
    pub fn tier_label(&self) -> &'static str {
        match self.tier {
            Tier::Hero => "hero landmark",
            Tier::Medium => "prop",
            Tier::Scatter => "ground cover",
        }
    }

    /// Placement scale: normalized to the tier's target span from the
    /// model's native size when known, else the tier's legacy multiplier.
    pub fn placement_scale(&self) -> f32 {
        match self.dims {
            Some(d) => {
                let span = d[0].max(d[1]).max(d[2]).max(0.01);
                self.scale * self.tier.target_span() / span
            }
            None => self.scale * self.tier.base_scale(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssetManifest {
    #[allow(dead_code)]
    pub version: u32,
    pub assets: Vec<AssetEntry>,
}

impl AssetManifest {
    /// The distinct kinds present, in first-appearance order — the agent's
    /// `place_asset` enum ([`agent`]'s tool schema) and a stable, small
    /// vocabulary however large the variant pool grows.
    #[cfg_attr(not(feature = "llm"), allow(dead_code))] // agent-only surface
    pub fn kinds(&self) -> Vec<&str> {
        let mut kinds: Vec<&str> = Vec::new();
        for a in &self.assets {
            if !a.kind.is_empty() && !kinds.contains(&a.kind.as_str()) {
                kinds.push(a.kind.as_str());
            }
        }
        kinds
    }
}

/// Resolve a semantic kind to a concrete manifest variant — the host half of
/// the two-level vocabulary. The search pool is the *neighbourhood*: the given
/// mood's own entries plus, for an extended mood, its base quadrant's (same
/// neighbourhood, different hour). Rotation runs across the whole pool — every
/// variant is used once before any repeats, so a world asking for six `rock`s
/// gets six different rocks — and restarts from the top when the neighbourhood
/// is exhausted rather than reaching for a wrong-mood variant. Only when the
/// neighbourhood lacks the kind entirely does the pool widen to any mood.
///
/// `used` accumulates files across calls (one list per agent session) and is
/// updated in place. Deterministic: ties break by manifest order, never by
/// hash or time, so the same (manifest, mood, call sequence) always resolves
/// the same way.
#[cfg_attr(not(feature = "llm"), allow(dead_code))] // agent-only surface
pub fn resolve_kind<'a>(
    manifest: &'a AssetManifest,
    kind: &str,
    mood: Option<usize>,
    used: &mut Vec<String>,
) -> Option<&'a AssetEntry> {
    let of_kind = |m: Option<usize>| {
        manifest
            .assets
            .iter()
            .filter(|a| a.kind == kind && m.is_none_or(|m| a.mood_index() == m))
            .collect::<Vec<_>>()
    };
    // Neighbourhood pool: own mood (+ base quadrant for extended moods),
    // widening to any mood only when the neighbourhood is empty for the kind.
    let mut pool: Vec<&AssetEntry> = vec![];
    if let Some(mood) = mood.map(|m| m % crate::theme::moods().len()) {
        pool.extend(of_kind(Some(mood)));
        if mood >= crate::theme::ASSET_BASE_MOODS {
            pool.extend(of_kind(Some(mood % crate::theme::ASSET_BASE_MOODS)));
        }
    }
    if pool.is_empty() {
        pool = of_kind(None);
    }
    // Rotate: first unused variant, else restart deterministically from the
    // top. An empty pool means the pack carries no such kind at all.
    let first = pool.first()?;
    let entry = *pool
        .iter()
        .find(|e| !used.contains(&e.file))
        .unwrap_or(first);
    used.push(entry.file.clone());
    Some(entry)
}

/// Deterministic scatter offsets for one `scatter_field` command: `count`
/// points on a uniform disk of `radius` (sqrt-distributed so the field doesn't
/// clump at the centre), y always 0 — the executor adds the authored base
/// position. Seeded by the session track + field name, so a cached build
/// replays to the identical field.
#[cfg_attr(not(feature = "llm"), allow(dead_code))] // agent-only surface
pub(crate) fn scatter_offsets(seed: u64, count: usize, radius: f32) -> Vec<Vec3> {
    let mut rng = seed | 1; // never zero (splitmix handles it, but stay odd)
    (0..count)
        .map(|_| {
            let ang = rand01(&mut rng) * std::f32::consts::TAU;
            let dist = rand01(&mut rng).sqrt() * radius.max(0.0);
            Vec3::new(ang.cos() * dist, 0.0, ang.sin() * dist)
        })
        .collect()
}

/// Deterministic 64-bit fold of a string — the seed source for
/// [`scatter_offsets`] (track id + field name).
#[cfg_attr(not(feature = "llm"), allow(dead_code))] // agent-only surface
pub(crate) fn fold_seed(s: &str) -> u64 {
    s.bytes().fold(0xC0FFEE_u64, |acc, b| {
        acc.wrapping_mul(31).wrapping_add(b as u64)
    })
}

/// The loaded manifest (None when no asset pack is bundled). Read by both the
/// world populator and the Credits screen.
#[derive(Resource, Default)]
pub struct WorldAssets {
    pub manifest: Option<AssetManifest>,
}

/// Per-asset CLAP text embeddings — the M5→M6 asset-selection hook (PLAN.md).
/// Filled in the background by `sync_asset_embeddings` (ml feature): each
/// asset's text (`"{name}, {tier_label}"`) embeds into the same 512-d space
/// as the track embeddings, so a song whose audio sits near "coral" favours
/// coral assets. Placement re-runs once when the first embeddings land, so
/// the opening world upgrades instead of staying neutral until track two.
///
/// Ungated (a plain map) so placement code compiles in every feature config:
/// an empty map means neutral weights — the no-model path is exactly today's
/// behaviour. Deliberately track-*audio*-driven: CLAP's trained alignment is
/// audio↔text; text↔text similarity on this model is off-manifold (probe:
/// `ml::tests::text_embedding_probe`), so recipe `intent` strings stay on the
/// keyword path and only the track embedding ranks here.
#[derive(Resource, Default)]
pub struct AssetEmbeddings {
    pub map: HashMap<String, Vec<f32>>,
}

/// The lazy text-tower state behind [`AssetEmbeddings`] (ml feature only).
/// The CLAP text tower is far too slow for the main thread (a forward pass
/// per asset would stall frames), so embedding runs on a dedicated std
/// thread: the system spawns it once, then drains finished results into
/// [`AssetEmbeddings`] a few per frame. A missing model logs once and the
/// keyword path stands.
#[cfg(feature = "ml")]
#[derive(Resource, Default)]
pub struct TextModelState {
    /// Written by the background thread, drained into [`AssetEmbeddings`].
    shared: std::sync::Arc<std::sync::Mutex<HashMap<String, Vec<f32>>>>,
    /// Whether the fill thread has been spawned (once per manifest).
    started: bool,
}

/// Spawn the background fill once (first call with a manifest), then drain
/// finished asset embeddings into [`AssetEmbeddings`]. Never blocks a frame.
#[cfg(feature = "ml")]
pub fn sync_asset_embeddings(
    assets: Res<WorldAssets>,
    mut state: ResMut<TextModelState>,
    mut embeddings: ResMut<AssetEmbeddings>,
) {
    if !state.started {
        let Some(manifest) = &assets.manifest else {
            return;
        };
        state.started = true;
        let shared = state.shared.clone();
        let texts: Vec<(String, String)> = manifest
            .assets
            .iter()
            .map(|a| (a.file.clone(), format!("{}, {}", a.name, a.tier_label())))
            .collect();
        std::thread::spawn(move || {
            let Some(mut model) = crate::ml::TextEmbedder::try_load() else {
                return; // keyword matching stands; nothing more to do
            };
            let mut done = 0usize;
            for (file, text) in texts {
                match model.embed(&text) {
                    Ok(emb) => {
                        shared.lock().unwrap().insert(file, emb);
                        done += 1;
                    }
                    Err(e) => warn!("ml: asset embed failed for {file}: {e}"),
                }
            }
            info!("ml: embedded {done} assets for track-driven placement");
        });
    }
    // Drain what the thread has finished so far.
    let drained: Vec<(String, Vec<f32>)> = {
        let mut shared = state.shared.lock().unwrap();
        let keys: Vec<String> = shared
            .keys()
            .filter(|k| !embeddings.map.contains_key(*k))
            .cloned()
            .collect();
        keys.into_iter()
            .filter_map(|k| shared.remove(&k).map(|v| (k, v)))
            .collect()
    };
    for (k, v) in drained {
        embeddings.map.insert(k, v);
    }
}

/// The current world layout seed. Same (mood, seed) → same placement; "Build
/// a different world" re-rolls it and "Keep this world" pins it per track
/// (ARCHITECTURE R6).
#[derive(Resource)]
pub struct WorldLayout {
    pub seed: u64,
}

impl Default for WorldLayout {
    fn default() -> Self {
        Self { seed: 0x5EED }
    }
}

/// splitmix64 — tiny deterministic PRNG (no `rand` dependency).
pub(crate) fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

/// Uniform f32 in [0, 1).
pub(crate) fn rand01(state: &mut u64) -> f32 {
    (splitmix(state) >> 40) as f32 / (1u64 << 24) as f32
}

/// Deterministic per-track default seed (from the path, like `path_mood`).
pub(crate) fn path_seed(path: &std::path::Path) -> u64 {
    path.as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xC0FFEE_u64, |acc, &b| {
            acc.wrapping_mul(31).wrapping_add(b as u64)
        })
}

/// Place prop `i` under a world's arrangement ("structured layouts" as
/// deterministic rules — the M7-lite stand-in for full WFC, ARCHITECTURE §3
/// layer 4). Same (arrangement, seed) → same layout.
///
/// Which arrangement a world uses is [`crate::theme::WorldMood::arrangement`],
/// not this function's business: keying it on the mood index meant inserting a
/// mood handed its layout to whatever took that slot.
fn layout_position(arrangement: Arrangement, i: usize, rng: &mut u64) -> Vec3 {
    let fi = i as f32;
    match arrangement {
        Arrangement::Grid => {
            // City grid: 7.5m pitch, 10 columns, jittered ±1.8m.
            const PITCH: f32 = 7.5;
            const COLS: usize = 10;
            let x = (i % COLS) as f32 * PITCH - (COLS as f32 - 1.0) * PITCH / 2.0;
            let z = (i / COLS) as f32 * PITCH - 30.0;
            Vec3::new(
                x + (rand01(rng) - 0.5) * 3.6,
                -0.5,
                z + (rand01(rng) - 0.5) * 3.6,
            )
        }
        Arrangement::Rings => {
            // Rings: 8m inner radius growing 6m per ring, 6+3k seats per ring.
            let mut ring = 0usize;
            let mut first = 0usize; // first index in this ring
            let mut seats = 6usize;
            while i >= first + seats {
                first += seats;
                ring += 1;
                seats = 6 + 3 * ring;
            }
            let radius = 8.0 + 6.0 * ring as f32 + (rand01(rng) - 0.5) * 2.0;
            let ang = (i - first) as f32 / seats as f32 * std::f32::consts::TAU
                + (rand01(rng) - 0.5) * 0.35;
            Vec3::new(ang.cos() * radius, -0.5, ang.sin() * radius)
        }
        Arrangement::Clusters => {
            // Outcrops on open ground: clumps of CLUMP_SIZE around scattered
            // anchors, so the eye reads groups with empty flats between them
            // rather than an even field.
            const CLUMP_SIZE: usize = 4;
            const CLUMP_SPREAD: f32 = 3.4;
            // The anchor's own jitter must not depend on how many props landed
            // before it, so derive it from the clump index alone.
            let clump = i / CLUMP_SIZE;
            let mut anchor_rng = (clump as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let ang = rand01(&mut anchor_rng) * std::f32::consts::TAU;
            let radius = 11.0 + rand01(&mut anchor_rng) * 26.0;
            let anchor = Vec3::new(ang.cos() * radius, -0.5, ang.sin() * radius);

            let off = rand01(rng) * std::f32::consts::TAU;
            let dist = rand01(rng).sqrt() * CLUMP_SPREAD;
            anchor + Vec3::new(off.cos() * dist, 0.0, off.sin() * dist)
        }
        Arrangement::Terraces => {
            // Tidal terraces: long rows on a slow sine, each row offset so the
            // bands read as water lines rather than a grid.
            const ROW_LEN: usize = 7;
            const ROW_PITCH: f32 = 8.5;
            const SPAN: f32 = 46.0;
            let row = i / ROW_LEN;
            let seat = i % ROW_LEN;
            let z = row as f32 * ROW_PITCH - 26.0;
            let t = seat as f32 / (ROW_LEN - 1) as f32;
            // Stagger alternate rows by half a seat so terraces interlock.
            let stagger = if row.is_multiple_of(2) {
                0.0
            } else {
                0.5 / ROW_LEN as f32
            };
            let x = (t + stagger - 0.5) * SPAN;
            // The wave gives each row its curve; amplitude grows with distance.
            let wave = (z * 0.07).sin() * (4.0 + row as f32 * 0.6);
            Vec3::new(
                x + (rand01(rng) - 0.5) * 2.2,
                -0.5,
                z + wave + (rand01(rng) - 0.5) * 1.4,
            )
        }
        Arrangement::Spiral => {
            // Organic golden-angle spiral (the original arrangement); the
            // centre stays clear — it's the camera's focus and orbit path.
            let ang = fi * 2.399_963 + (rand01(rng) - 0.5) * 0.9;
            let radius = (9.0 + (fi + 2.0).sqrt() * 4.2 + (rand01(rng) - 0.5) * 3.0).max(8.0);
            Vec3::new(ang.cos() * radius, -0.5, ang.sin() * radius)
        }
    }
}

/// Marker for spawned prop entities, so a mood change can clear them.
#[derive(Component)]
pub struct WorldProp;

/// Marker for scatter-tier props: the only tier that follows the section
/// verbs' `scatter` multiplier after settling (the outro sinks the ground
/// cover back into the ground it rose from).
#[derive(Component)]
pub struct ScatterProp;

/// Ambient behaviour for a placed prop — a gentle per-prop sway/spin so the
/// grounded world breathes with the track instead of freezing after the
/// materialize. Heroes are monuments and stay still by design.
#[derive(Component)]
pub struct PropMotion {
    /// Home Y (bob returns here, never fights the placement).
    pub base_y: f32,
    /// Deterministic per-prop phase.
    pub seed: f32,
    /// Bob amplitude (metres) — mediums 0.15, scatter 0.06.
    pub bob: f32,
    /// Yaw speed (rad/s) — a slow drift, not a carousel.
    pub spin: f32,
}

/// Animate placed props' ambient behaviour after they settle: a slow bob and
/// spin scaled by the recipe's motion, the section feel, and the bass level
/// (the ground cover visibly rides the low end). Comfort › gentler world
/// motion damps the bob; the spin is slow enough to keep.
pub fn animate_props(
    time: Res<Time>,
    clock: Res<crate::WorldClock>,
    comfort: Res<crate::Comfort>,
    beat: Res<crate::playback::Beat>,
    stems: Res<crate::playback::StemLevels>,
    feel: Res<crate::world::SectionFeel>,
    mut props: Query<(&PropRise, &PropMotion, &mut Transform), Without<crate::world::Drifter>>,
) {
    let gentle = if comfort.gentler_motion { 0.4 } else { 1.0 };
    let bass = stems.0[1].max(beat.bass);
    let t = time.elapsed_secs() * clock.speed;
    let dt = time.delta_secs() * clock.speed;
    for (rise, motion, mut tf) in &mut props {
        if rise.t < rise.delay + rise.dur {
            continue; // still materializing — the rise owns the transform
        }
        let p = t + motion.seed * std::f32::consts::TAU;
        let bob = motion.bob * (1.0 + bass * 1.5) * gentle * feel.motion;
        tf.translation.y = motion.base_y + p.sin() * bob;
        tf.rotate_y(dt * motion.spin * feel.motion);
    }
}

/// A landmark's emissive beacon, carrying its full-intensity emissive so
/// [`pulse_beacons`] can breathe it with the drums/bass without recomputing
/// the colour.
#[derive(Component)]
pub struct Beacon {
    pub base: LinearRgba,
}

/// Breathe the landmark beacons with the rhythm: the drums stem (Demucs
/// curve) when present, else the live tap's bass envelope. Reduce-flashing
/// holds them steady — this is the one stem layer that modulates
/// *brightness*, so it takes the strictest gate; everything else reacts via
/// motion.
pub fn pulse_beacons(
    comfort: Res<crate::Comfort>,
    beat: Res<crate::playback::Beat>,
    stems: Res<crate::playback::StemLevels>,
    feel: Res<crate::world::SectionFeel>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&Beacon, &MeshMaterial3d<StandardMaterial>)>,
) {
    let drums = stems.0[0].max(beat.bass);
    let intensity = if comfort.reduce_flashing {
        0.8 * feel.beacons.clamp(0.0, 1.5)
    } else {
        // Motion-dominant: a slow swell with a small beat tickle on top. The
        // section's verb lights the chorus up and dims the bridge.
        (0.55 + drums * 0.45 + beat.pulse * 0.12) * feel.beacons.clamp(0.0, 1.5)
    };
    for (beacon, mat) in &mut q {
        if let Some(mut m) = materials.get_mut(&mat.0) {
            m.emissive = LinearRgba::rgb(
                beacon.base.red * intensity,
                beacon.base.green * intensity,
                beacon.base.blue * intensity,
            );
        }
    }
}

/// Materialize rise animation (spec 1g): each prop grows in from the ground,
/// staggered so the last one settles on the incoming track's first downbeat.
#[derive(Component)]
pub struct PropRise {
    delay: f32,
    dur: f32,
    /// Target scale, per-axis (non-uniform for the skyline stelae, which grow
    /// upward only).
    target: Vec3,
    t: f32,
}

/// Rise duration per prop.
const RISE_SECS: f32 = 0.9;
/// First prop starts shortly after the palette wash begins.
const FIRST_DELAY: f32 = 0.35;

/// Start delay for prop `i` of `n`, spread so the final prop finishes its
/// rise exactly at `settle` (the first downbeat), decelerating overall.
pub(crate) fn stagger_delay(i: usize, n: usize, settle: f32) -> f32 {
    let last = (settle - RISE_SECS).max(FIRST_DELAY);
    if n <= 1 {
        return last;
    }
    FIRST_DELAY + (last - FIRST_DELAY) * (i as f32 / (n as f32 - 1.0))
}

/// Grow rising props in with an ease-out; obeys the world clock so a paused
/// world holds its breath mid-materialize. After settling, scatter props keep
/// following their section verb's scale (the outro sinks them).
pub fn rise_props(
    time: Res<Time>,
    clock: Res<crate::WorldClock>,
    feel: Res<crate::world::SectionFeel>,
    mut props: Query<(&mut PropRise, &mut Transform, Option<&ScatterProp>)>,
) {
    let dt = time.delta_secs() * clock.speed;
    for (mut rise, mut tf, scatter) in &mut props {
        if rise.t >= rise.delay + rise.dur {
            if scatter.is_some() {
                tf.scale = rise.target * feel.scatter.max(0.0);
            }
            continue;
        }
        rise.t += dt;
        let f = ((rise.t - rise.delay) / rise.dur).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - f).powi(3); // out-cubic, no overshoot
        tf.scale = rise.target * eased.max(0.01);
    }
}

/// The directory LocalGPT Verse loads bundled assets from.
///
/// Everything resolves through here: the Bevy asset server (pinned to this
/// path via `AssetPlugin::file_path` in `main`) and the direct-filesystem
/// readers that can't go through it — the model manifest (also read off the
/// analysis worker thread), the fonts, the ML/LLM models, the starter music.
/// One root means those two halves cannot disagree about where `assets/` is.
///
/// Resolved once, in this order:
/// 1. `VERSE_ASSET_ROOT` — explicit override.
/// 2. `<exe dir>/assets` — the shipped layout (`scripts/bundle.sh`, the
///    Windows zip, the Linux tarball).
/// 3. `<exe dir>/../Resources/assets` — inside a macOS `.app`, where the
///    binary sits in `Contents/MacOS/` and its data in `Contents/Resources/`.
/// 4. `CARGO_MANIFEST_DIR/assets` — the dev tree, baked at compile time, so
///    `cargo run` and a bare `target/release/localgpt-verse` work from any cwd.
/// 5. `assets` — relative last resort.
///
/// Exe-relative comes before the dev tree so a packaged build never prefers a
/// stale source checkout that happens to exist on the machine that built it.
/// Nothing else is cwd-relative: a bundle launched from Finder or a Start Menu
/// shortcut gets a working directory unrelated to where it was installed.
pub(crate) fn asset_root() -> std::path::PathBuf {
    static ROOT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(resolve_asset_root).clone()
}

fn resolve_asset_root() -> std::path::PathBuf {
    if let Some(over) = std::env::var_os("VERSE_ASSET_ROOT") {
        return std::path::PathBuf::from(over);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let beside_exe = dir.join("assets");
        if beside_exe.is_dir() {
            return beside_exe;
        }
        // `Contents/MacOS/localgpt-verse` → `Contents/Resources/assets`. Harmless to
        // probe elsewhere; no other platform lays a bundle out this way.
        if let Some(contents) = dir.parent() {
            let in_bundle = contents.join("Resources").join("assets");
            if in_bundle.is_dir() {
                return in_bundle;
            }
        }
    }
    let dev_tree = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    if dev_tree.is_dir() {
        return dev_tree;
    }
    // Nothing bundled: fall through to the directories every LocalGPT app
    // shares — the pack download and a sibling checkout of the asset
    // repository, under its current and former names. Resolving the *root*
    // here (rather than the manifest alone) keeps meshes resolving from the
    // same directory the manifest came from.
    if let Some(shared) = localgpt_world_agent::world_pack_dir(&[]) {
        return shared;
    }
    std::path::PathBuf::from("assets")
}

/// Read `assets/models/manifest.json` from disk if present. Used both by the
/// Bevy-side loader ([`load_asset_manifest`]) and by the analysis worker
/// thread, which needs the asset vocabulary to build the agent's `place_asset`
/// tool schema but cannot reach the `World` resource.
pub(crate) fn read_manifest_from_disk() -> Option<AssetManifest> {
    let path = asset_root().join("models/manifest.json");
    std::fs::read_to_string(&path).ok().and_then(|text| {
        match serde_json::from_str::<AssetManifest>(&text) {
            Ok(m) => Some(m),
            Err(e) => {
                warn!("Ignoring asset manifest ({e})");
                None
            }
        }
    })
}

/// Load `assets/models/manifest.json` if present. Absent → procedural worlds.
pub fn load_asset_manifest(mut commands: Commands) {
    let manifest = read_manifest_from_disk();
    match &manifest {
        Some(m) => info!("Asset pack: {} models", m.assets.len()),
        None => info!("No asset pack bundled — procedural worlds"),
    }
    commands.insert_resource(WorldAssets { manifest });
}

// ---------------------------------------------------------------------------
// Merged scatter field — the prop-ceiling unlock
// ---------------------------------------------------------------------------

/// How much denser the merged scatter field is than the per-entity scatter it
/// replaces: one entity and one draw call, so the per-entity ceiling (measured
/// ~90 props at 60 fps; 1k scene-roots at 14) no longer applies to ground
/// cover. The world reads *denser* before it reads *cheaper*.
const SCATTER_FIELD_BOOST: f32 = 4.0;

/// Merged scatter geometry under construction — pebbles/shards/chips baked
/// into one mesh with per-vertex tint, so hundreds of ground-cover instances
/// cost one entity.
#[derive(Default)]
struct MergedScatter {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MergedScatter {
    /// Append one transformed copy of `base` (a unit primitive's mesh).
    fn append(&mut self, base: &Mesh, pos: Vec3, rot: Quat, scale: f32, color: [f32; 4]) {
        let offset = self.positions.len() as u32;
        let verts = base
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|v| v.as_float3())
            .map(<[[f32; 3]]>::to_vec)
            .unwrap_or_default();
        let norms = base
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .and_then(|v| v.as_float3())
            .map(<[[f32; 3]]>::to_vec)
            .unwrap_or_default();
        for (i, v) in verts.iter().enumerate() {
            self.positions
                .push((rot * Vec3::from(*v) * scale + pos).to_array());
            let n = norms.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
            self.normals
                .push((rot * Vec3::from(n)).normalize_or_zero().to_array());
            self.colors.push(color);
        }
        if let Some(indices) = base.indices() {
            for i in indices.iter() {
                self.indices.push(i as u32 + offset);
            }
        }
    }

    fn build(self) -> Mesh {
        let mut mesh = Mesh::new(
            bevy::render::render_resource::PrimitiveTopology::TriangleList,
            default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(bevy::render::mesh::Indices::U32(self.indices));
        mesh
    }
}

/// The ground-cover pebble shape for a world's arrangement family: chips for
/// city grids, shards for crystal rings and terraces, pebbles elsewhere.
fn scatter_base_handle(arrangement: Arrangement, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
    match arrangement {
        Arrangement::Grid => meshes.add(Cuboid::new(1.0, 0.5, 1.0)),
        Arrangement::Rings | Arrangement::Terraces => meshes.add(Tetrahedron::default().mesh()),
        _ => meshes.add(Sphere::new(1.0).mesh().ico(1).unwrap()),
    }
}

/// Everything a world's rule-based props depend on: the same inputs place the
/// same world, in the app ([`populate_world_props`]) and in an export
/// ([`crate::world_manifest`]).
pub(crate) struct PropInputs<'a> {
    pub mood: usize,
    /// The track's layout seed ([`crate::analysis::layout_seed`]).
    pub seed: u64,
    pub recipe: Option<&'a crate::recipe::WorldRecipe>,
    pub manifest: &'a AssetManifest,
    /// The track's CLAP embedding, when the `ml` tier computed one.
    pub track_embedding: Option<&'a [f32]>,
    pub embeddings: &'a AssetEmbeddings,
    /// The track's section count (heroes rise on the middle section).
    pub section_count: usize,
}

/// A world's rule-based props, planned before anything spawns.
pub(crate) struct PropPlan {
    pub arrangement: Arrangement,
    pub props: Vec<PlannedProp>,
    /// The merged ground-cover field, pebble by pebble.
    pub scatter: Vec<ScatterPebble>,
}

/// One pebble of the merged ground-cover field.
pub(crate) struct ScatterPebble {
    pub pos: Vec3,
    pub rot_y: f32,
    pub scale: f32,
    /// Tint from the ground colour (0) toward the accent.
    pub mix: f32,
}

/// Plan a world's rule-based props — the mood's props in three tiers, the
/// recipe's secondary-biome accents and landmarks, each prop's motion phase
/// and the ground-cover field — from one seeded sequence, so the same inputs
/// always give the same world.
pub(crate) fn plan_world_props(input: &PropInputs<'_>) -> PropPlan {
    let mood = input.mood % crate::theme::moods().len();
    let recipe = input.recipe;
    let manifest = input.manifest;
    let embeddings = input.embeddings;
    // M7: the recipe's primary biome overrides the mood's default
    // arrangement.
    let arrangement = recipe
        .and_then(|r| r.biomes.first())
        .map(|b| b.layout.arrangement())
        .unwrap_or(crate::theme::moods()[mood].arrangement);
    let chorus_seg = input.section_count / 2;

    // M7: a recipe may scale prop density within [0.3, 2.0] (already clamped).
    // Absent recipe → 1.0 (today's per-tier counts).
    let density = recipe.map(|r| r.density).unwrap_or(1.0);

    // Seeded per-mood arrangement: same (mood, seed, recipe) → identical world.
    let mut rng = input.seed ^ (mood as u64).wrapping_mul(0x9E37_79B9);
    let mut plan: Vec<PlannedProp> = Vec::new();
    // Scatter positions accumulate here instead of entities — the tier becomes
    // one merged mesh (see the field spawn below the plan).
    let mut scatter_pts: Vec<Vec3> = Vec::new();
    let mut placed = 0usize; // layout index across every class of placement

    // Primary mood props, three tiers, each assigned the section it rises in.
    // The extended moods (Cinder Reach, Mirage Circuit, …) keep their base
    // quadrant's set — same neighbourhood, different hour — and layer their
    // own entries on top when the pack carries any (previously all-or-nothing
    // borrowing: three own entries would have *shrunk* the world).
    let mut entries: Vec<_> = manifest
        .assets
        .iter()
        .filter(|a| a.mood_index() == mood)
        .collect();
    if mood >= crate::theme::ASSET_BASE_MOODS {
        let base = mood % crate::theme::ASSET_BASE_MOODS;
        entries.extend(manifest.assets.iter().filter(|a| a.mood_index() == base));
    }
    // Track-embedding asset weights (M5→M6): mediums get weighted counts, so
    // a track that sounds oceanic favours coral over concrete. Neutral when
    // the track has no embedding or the manifest hasn't been embedded.
    let track_emb = input.track_embedding;
    let weights = embedding_weights(&entries, track_emb, embeddings);
    // Tier budgets spent across the pool by weighted round-robin
    // (`Tier::budget`): the original per-asset counts sized worlds at ~5
    // variants per tier; with the pool grown well past that they would blow
    // past the measured prop ceiling. The budget holds the entity count while
    // every variant still gets its turn.
    for tier in [Tier::Hero, Tier::Medium, Tier::Scatter] {
        let pool: Vec<(&AssetEntry, f32)> = entries
            .iter()
            .zip(&weights)
            .filter(|(a, _)| a.tier == tier)
            .map(|(a, w)| (*a, if tier == Tier::Medium { *w } else { 1.0 }))
            .collect();
        if pool.is_empty() {
            continue;
        }
        let pool_w: Vec<f32> = pool.iter().map(|(_, w)| *w).collect();
        let total = (tier.budget() as f32 * density).round().max(1.0) as usize;
        let mut used = vec![0u32; pool.len()];
        for _ in 0..total {
            let j = pick_weighted_round_robin(&pool_w, &mut used);
            let entry = pool[j].0;
            if tier == Tier::Scatter {
                scatter_pts.push(layout_position(arrangement, placed, &mut rng));
                placed += 1;
                continue;
            }
            let i = placed;
            placed += 1;
            plan.push(PlannedProp {
                file: entry.file.clone(),
                pos: layout_position(arrangement, i, &mut rng),
                rot_y: rand01(&mut rng) * std::f32::consts::TAU,
                scale: entry.placement_scale() * (0.85 + rand01(&mut rng) * 0.3),
                range: entry.tier.visibility_range(),
                beacon: None,
                motion_seed: 0.0,
                tier: entry.tier,
                seg: match entry.tier {
                    // Mediums spread over the verses; heroes land on the
                    // chorus.
                    Tier::Medium => 1 + (i % chorus_seg.max(1)),
                    _ => chorus_seg,
                },
            });
        }
    }

    // M7 secondary biomes: contrasting accents from each secondary mood's own
    // set (non-hero tiers), claiming a share of the primary budget scaled by
    // that biome's density. At most two secondaries, so accents accent rather
    // than take over. Accents arrive *with* the chorus.
    if let Some(recipe) = recipe {
        let primary = plan.len().max(1);
        for biome in recipe.biomes.iter().skip(1).take(2) {
            let bmood = biome.mood % crate::theme::moods().len();
            let accent_count = (biome.density * primary as f32 * 0.25)
                .round()
                .clamp(1.0, 24.0) as usize;
            let accents: Vec<_> = manifest
                .assets
                .iter()
                .filter(|a| a.mood_index() == bmood && a.tier != Tier::Hero)
                .collect();
            if accents.is_empty() {
                continue;
            }
            for k in 0..accent_count {
                let entry = &accents[k % accents.len()];
                let i = plan.len();
                plan.push(PlannedProp {
                    file: entry.file.clone(),
                    pos: layout_position(biome.layout.arrangement(), i, &mut rng),
                    rot_y: rand01(&mut rng) * std::f32::consts::TAU,
                    scale: entry.placement_scale() * (0.8 + rand01(&mut rng) * 0.3),
                    range: entry.tier.visibility_range(),
                    beacon: None,
                    motion_seed: 0.0,
                    tier: entry.tier,
                    seg: chorus_seg,
                });
            }
        }
    }

    // M7 landmarks: hero assets matched to the recipe's `kind` by name
    // keywords (see `pick_hero_for_kind`), placed at their anchors. Heroes
    // stay visible through the fog — they define the skyline — and
    // `emissive > 0.1` raises a beacon above them (baked glTF materials can't
    // take a runtime emissive). They rise on the chorus.
    if let Some(recipe) = recipe {
        let heroes: Vec<_> = manifest
            .assets
            .iter()
            .filter(|a| a.mood_index() == mood && a.tier == Tier::Hero)
            .collect();
        let mut used: Vec<String> = Vec::new();
        for (i, landmark) in recipe.landmarks.iter().enumerate() {
            let score = |e: &AssetEntry| {
                track_emb
                    .and_then(|q| {
                        embeddings
                            .map
                            .get(&e.file)
                            .map(|a| crate::analysis::dot_normed(q, a))
                    })
                    .unwrap_or(0.0)
            };
            let Some(entry) = pick_hero_for_kind(&heroes, landmark.kind, &mut used, Some(&score))
            else {
                break;
            };
            plan.push(PlannedProp {
                file: entry.file.clone(),
                pos: landmark_anchor(landmark.at, i),
                rot_y: rand01(&mut rng) * std::f32::consts::TAU,
                scale: landmark.scale * entry.placement_scale(),
                range: None,
                beacon: (landmark.emissive > 0.1).then_some(landmark.emissive),
                motion_seed: 0.0,
                tier: Tier::Hero,
                seg: chorus_seg,
            });
        }
    }

    // Ambient motion phases, drawn in plan order: mediums and scatter bob and
    // spin; heroes stay monuments.
    for p in &mut plan {
        if p.tier != Tier::Hero {
            p.motion_seed = rand01(&mut rng);
        }
    }

    // The merged scatter field: SCATTER_FIELD_BOOST× the per-entity density
    // the old scatter tier could afford (see the field spawn).
    let mut scatter = Vec::new();
    if !scatter_pts.is_empty() {
        let target = (scatter_pts.len() as f32 * SCATTER_FIELD_BOOST).round() as usize;
        while scatter_pts.len() < target {
            scatter_pts.push(layout_position(arrangement, placed, &mut rng));
            placed += 1;
        }
        for pos in scatter_pts {
            let scale = 0.18 + rand01(&mut rng) * 0.5;
            let rot_y = rand01(&mut rng) * std::f32::consts::TAU;
            let mix = rand01(&mut rng) * 0.55;
            scatter.push(ScatterPebble {
                pos,
                rot_y,
                scale,
                mix,
            });
        }
    }
    PropPlan {
        arrangement,
        props: plan,
        scatter,
    }
}

/// (Re)place ground props for the current mood on a mood change, rising in a
/// materialize sequence that settles on the current track's first downbeat
/// (when its analysis is already in — the next track is prefetched, so the
/// crossfade case normally has it).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn populate_world_props(
    theme: Res<Theme>,
    layout: Res<WorldLayout>,
    assets: Res<WorldAssets>,
    embeddings: Res<AssetEmbeddings>,
    active_recipe: Res<crate::recipe::ActiveRecipe>,
    asset_server: Res<AssetServer>,
    analysis: Res<crate::analysis::AnalysisStore>,
    playback: Res<crate::playback::Playback>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    existing: Query<Entity, With<WorldProp>>,
    mut last: Local<Option<(usize, u64, Option<String>, bool)>>,
) {
    let mood = theme.mood % crate::theme::moods().len();
    let recipe = active_recipe.get();
    // The recipe's world name keys layout identity so two recipes over the
    // same (mood, seed) can still ask for different worlds.
    let recipe_key = recipe.map(|r| r.world_name.clone());
    // Re-run once when the first asset embeddings land, so the opening world
    // upgrades from neutral weights instead of waiting for the next track.
    let embedded = !embeddings.map.is_empty();
    if *last == Some((mood, layout.seed, recipe_key.clone(), embedded)) {
        return;
    }
    *last = Some((mood, layout.seed, recipe_key, embedded));

    for e in &existing {
        commands.entity(e).despawn();
    }
    let Some(manifest) = &assets.manifest else {
        return;
    };

    // "Geometry settles on the first downbeat" (spec 1g): the stagger window
    // ends at the incoming track's beat offset when known, else mid-window.
    let current_id = playback
        .queue
        .get(playback.current % playback.queue.len().max(1))
        .and_then(|t| t.id.clone());
    let settle = current_id
        .as_deref()
        .and_then(|id| analysis.beat_offset_for(id))
        .map(|offset| offset.clamp(1.3, 3.5))
        .unwrap_or(2.4);

    // Whole-song materialize: with measured sections, props rise *across the
    // track* — scatter in the intro, mediums through the verses, heroes
    // landing exactly on the chorus — instead of everything in the first
    // 2.4 s. The first section still settles on the downbeat (spec 1g).
    let track_duration = playback
        .queue
        .get(playback.current % playback.queue.len().max(1))
        .map(|t| t.duration)
        .unwrap_or(0.0);
    let sections = &playback.sections;
    let sectioned = sections.len() >= 3 && track_duration > 0.0;
    let section_start =
        |seg: usize| -> f32 { sections.get(seg).copied().unwrap_or(0.0) * track_duration };

    let density = recipe.map(|r| r.density).unwrap_or(1.0);
    let PropPlan {
        arrangement,
        props: plan,
        scatter,
    } = plan_world_props(&PropInputs {
        mood,
        seed: layout.seed,
        recipe,
        manifest,
        track_embedding: current_id
            .as_deref()
            .and_then(|id| analysis.get(id))
            .and_then(|a| a.embedding.as_deref()),
        embeddings: &embeddings,
        section_count: sections.len(),
    });

    // One materialize over the whole plan: seg-0 props keep the
    // settle-on-the-downbeat stagger; later sections rise as they arrive.
    let total = plan.len();
    let beacon_mesh = meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap());
    let accent = theme.current().accent;
    for (i, p) in plan.into_iter().enumerate() {
        let handle: Handle<_> =
            asset_server.load(GltfAssetLabel::Scene(0).from_asset(format!("models/{}", p.file)));
        let delay = if !sectioned || p.seg == 0 {
            stagger_delay(i, total, settle)
        } else {
            // Rise a moment into the section (deterministic jitter from the
            // plan index), never before the intro has settled.
            (section_start(p.seg) + (i as f32 * 0.37).fract() * 0.7).max(settle + 0.2)
        };
        let mut e = commands.spawn((
            WorldProp,
            WorldAssetRoot(handle),
            Transform::from_translation(p.pos)
                .with_scale(Vec3::splat(p.scale * 0.01))
                .with_rotation(Quat::from_rotation_y(p.rot_y)),
            PropRise {
                delay,
                dur: RISE_SECS,
                target: Vec3::splat(p.scale),
                t: 0.0,
            },
        ));
        if let Some(range) = p.range {
            e.insert(range);
        }
        if p.tier == Tier::Scatter {
            e.insert(ScatterProp);
        }
        // Ambient behaviour: mediums bob visibly, scatter shimmers — both
        // riding the bass. Heroes stay monuments.
        if p.tier != Tier::Hero {
            e.insert(PropMotion {
                base_y: p.pos.y,
                seed: p.motion_seed,
                bob: if p.tier == Tier::Medium { 0.15 } else { 0.06 },
                spin: if p.tier == Tier::Medium { 0.08 } else { 0.2 },
            });
        }
        if let Some(emissive) = p.beacon {
            // An unlit sphere — glow without a per-landmark light cost. Its
            // intensity breathes with the drums/bass in `pulse_beacons`
            // (Comfort-gated to steady under reduce-flashing).
            let radius = (0.22 * p.scale).clamp(0.1, 0.6);
            let height = (2.6 * p.scale).clamp(2.0, 14.0);
            let base = LinearRgba::new(
                accent.to_linear().red * 1.4 * emissive,
                accent.to_linear().green * 1.4 * emissive,
                accent.to_linear().blue * 1.4 * emissive,
                1.0,
            );
            commands.spawn((
                WorldProp,
                Beacon { base },
                Mesh3d(beacon_mesh.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: accent.with_alpha(0.9),
                    emissive: base,
                    unlit: true,
                    ..default()
                })),
                Transform::from_translation(p.pos + Vec3::new(0.0, height, 0.0))
                    .with_scale(Vec3::splat(radius)),
            ));
        }
    }

    // The waveform skyline ([`skyline`]), each stela rising as its section
    // arrives. Analysis pending → no skyline this pass, and the (mood, seed)
    // re-populate when it lands draws it.
    if let Some(curve) = current_id
        .as_deref()
        .and_then(|id| analysis.get(id))
        .map(|a| &a.energy)
        && !curve.is_empty()
    {
        let stela_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        let stela_mat = materials.add(StandardMaterial {
            base_color: theme.current().ground,
            emissive: LinearRgba::new(
                accent.to_linear().red * 0.22,
                accent.to_linear().green * 0.22,
                accent.to_linear().blue * 0.22,
                1.0,
            ),
            unlit: true,
            ..default()
        });
        let stelae = skyline(curve);
        let count = stelae.len();
        for (k, stela) in stelae.into_iter().enumerate() {
            let (pos, h) = (stela.pos, stela.height);
            // Which section does stela k live in? It rises with it.
            let seg = sections
                .iter()
                .filter(|&&s| s <= stela.along)
                .count()
                .saturating_sub(1);
            let delay = if sectioned {
                section_start(seg).max(1.3)
            } else {
                stagger_delay(k, count, settle)
            };
            commands.spawn((
                WorldProp,
                Mesh3d(stela_mesh.clone()),
                MeshMaterial3d(stela_mat.clone()),
                Transform::from_translation(pos)
                    .looking_at(Vec3::new(0.0, pos.y, 0.0), Vec3::Y)
                    .with_scale(Vec3::new(0.8, h, 1.4)),
                PropRise {
                    delay,
                    dur: RISE_SECS,
                    target: Vec3::new(0.8, h, 1.4),
                    t: 0.0,
                },
            ));
        }
    }

    // The merged scatter field: one entity, one draw, SCATTER_FIELD_BOOST×
    // the per-entity density the old tier could afford — the ground cover
    // now reads as a *field* rather than a scatter of samples. Vertex tints
    // blend ground → accent; it rises with the intro and sinks on the outro
    // like any scatter prop.
    if !scatter.is_empty() {
        let base = scatter_base_handle(arrangement, &mut meshes);
        let base_mesh = meshes
            .get(&base)
            .cloned()
            .expect("the base shape was just added to the asset store");
        let ground_lin = theme.current().ground.to_linear();
        let accent_lin = accent.to_linear();
        let mut merged = MergedScatter::default();
        for pebble in &scatter {
            let mix = pebble.mix;
            let color = [
                ground_lin.red + (accent_lin.red - ground_lin.red) * mix,
                ground_lin.green + (accent_lin.green - ground_lin.green) * mix,
                ground_lin.blue + (accent_lin.blue - ground_lin.blue) * mix,
                1.0,
            ];
            let rot = Quat::from_rotation_y(pebble.rot_y);
            merged.append(&base_mesh, pebble.pos, rot, pebble.scale, color);
        }
        let handle = meshes.add(merged.build());
        let material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.95,
            ..default()
        });
        commands.spawn((
            WorldProp,
            ScatterProp,
            Mesh3d(handle),
            MeshMaterial3d(material),
            Transform::from_scale(Vec3::splat(0.01)),
            PropRise {
                delay: 0.35,
                dur: 1.6,
                target: Vec3::splat(1.0),
                t: 0.0,
            },
        ));
    }

    if total > 0 {
        info!(
            "Placed {total} props + {} merged scatter for {} \
             (settle {settle:.2}s, density {density:.2})",
            scatter.len(),
            crate::theme::moods()[mood].world_name
        );
    }
}

/// One stela of the waveform skyline.
pub(crate) struct Stela {
    pub pos: Vec3,
    pub height: f32,
    /// Where along the song (0..=1) it samples the energy curve; it rises
    /// with that section.
    pub along: f32,
}

/// The skyline is the waveform: thin stelae around the rim whose heights
/// sample the track's energy curve — walk the rim and you read the song's
/// shape (drops are literal peaks). Same song → same skyline. Each stela is a
/// unit cube scaled to `(0.8, height, 1.4)`, facing the centre.
pub(crate) fn skyline(curve: &[f32]) -> Vec<Stela> {
    const STELAE: usize = 28;
    if curve.is_empty() {
        return Vec::new();
    }
    (0..STELAE)
        .map(|k| {
            let along = k as f32 / (STELAE - 1) as f32;
            let idx = (along * (curve.len() - 1) as f32).round() as usize;
            let ang = along * std::f32::consts::TAU;
            Stela {
                pos: Vec3::new(ang.cos() * 46.0, -0.5, ang.sin() * 46.0),
                height: 1.5 + curve[idx].clamp(0.0, 1.0) * 10.0,
                along,
            }
        })
        .collect()
}

/// One planned prop placement — collected first so every class of placement
/// (mood props, secondary-biome accents, recipe landmarks) shares one
/// staggered materialize pass and one denominator for the stagger math.
pub(crate) struct PlannedProp {
    /// glTF file relative to `assets/models/`.
    pub file: String,
    pub pos: Vec3,
    pub rot_y: f32,
    /// Target scale (metres-normalized); the entity starts at 1% and rises.
    pub scale: f32,
    pub range: Option<VisibilityRange>,
    /// M7 landmark emissive (0..1): beacon strength floated above the prop.
    /// `None` = no beacon.
    pub beacon: Option<f32>,
    /// Phase of the ambient bob (0..1); unused for heroes, which stay still.
    pub motion_seed: f32,
    /// Placement tier — drives the section-verb markers (`ScatterProp`) and
    /// the section the prop rises in.
    pub tier: Tier,
    /// The section (segment index) this prop rises in — the whole-song
    /// materialize. 0 keeps the settle-on-the-downbeat stagger.
    pub seg: usize,
}

/// Keyword vocabulary mapping a recipe [`crate::recipe::LandmarkKind`] onto
/// hero assets by name and (manifest v2) semantic kind — the M7-lite stand-in
/// for embedding-based selection (which wants the CLAP text space). Ties break
/// by manifest order, and every hero is used once before any repeats
/// (round-robin fallback), so two "Spire" landmarks don't clone the same model
/// when alternatives exist.
fn pick_hero_for_kind<'a>(
    heroes: &[&'a AssetEntry],
    kind: crate::recipe::LandmarkKind,
    used: &mut Vec<String>,
    score: Option<&dyn Fn(&AssetEntry) -> f32>,
) -> Option<&'a AssetEntry> {
    use crate::recipe::LandmarkKind;
    let keywords: &[&str] = match kind {
        LandmarkKind::Spire => &[
            "tower",
            "spire",
            "monolith",
            "antenna",
            "lighthouse",
            "pillar",
            "column",
        ],
        LandmarkKind::Gateway => &["arch", "gate", "portal", "bridge", "torii", "ruin"],
        LandmarkKind::Mass => &["rock", "boulder", "cliff", "mesa", "mountain", "reef"],
        LandmarkKind::Monument => &["crystal", "sculpture", "monument", "statue", "obelisk"],
    };
    // Kind affinities — a manifest hit scores like a name-keyword hit, so the
    // vocabulary does the matching even when display names don't.
    let affinities: &[&str] = match kind {
        LandmarkKind::Spire => &["tree", "dead_tree", "machine", "lamp"],
        LandmarkKind::Gateway => &["ruin", "nautical"],
        LandmarkKind::Mass => &["rock"],
        LandmarkKind::Monument => &["statue", "vase", "decor"],
    };
    heroes
        .iter()
        .enumerate()
        .filter(|(_, e)| !used.contains(&e.file))
        .map(|(i, e)| {
            let name_hits = e
                .name
                .to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| keywords.contains(w))
                .count();
            let kind_hits = affinities.contains(&e.kind.as_str()) as usize;
            let score = name_hits + kind_hits;
            (score, i, *e)
        })
        .filter(|(score, _, _)| *score > 0)
        .max_by_key(|(score, i, _)| score * 1000 - *i)
        .map(|(_, _, e)| {
            used.push(e.file.clone());
            e
        })
        // Fallback: the unused hero the *track* best matches (CLAP embedding,
        // when the manifest has been embedded), else manifest order.
        .or_else(|| {
            let fallback = match score {
                Some(s) => heroes
                    .iter()
                    .filter(|e| !used.contains(&e.file))
                    .max_by(|a, b| s(a).partial_cmp(&s(b)).unwrap_or(std::cmp::Ordering::Equal))
                    .copied(),
                None => heroes.iter().find(|e| !used.contains(&e.file)).copied(),
            }?;
            used.push(fallback.file.clone());
            Some(fallback)
        })
}

/// Per-entry placement weights from the track's audio embedding against the
/// assets' text embeddings: min-max normalized within the pool, scaled to
/// [0.4, 1.6] so totals roughly hold while *which* assets dominate shifts
/// with the song. Neutral (all 1.0) without a track embedding or any asset
/// coverage — the no-model path is today's behaviour.
///
/// Deliberately driven by the *track's audio* embedding only: CLAP's trained
/// alignment is audio↔text (text↔text similarity on this model is
/// off-manifold — `ml::tests::text_embedding_probe`), so recipe prose stays
/// on the keyword path and the song itself picks the flavour.
/// Weighted round-robin slot pick: the entry maximizing `weight / (1 + used)`,
/// ties to the lowest index. Each pick raises that entry's `used`, so the next
/// slot goes elsewhere — the rotation spreads a tier's budget over every
/// variant in the pool while the weights tilt which ones appear more.
/// Deterministic by construction (manifest order + weights only).
fn pick_weighted_round_robin(weights: &[f32], used: &mut [u32]) -> usize {
    debug_assert_eq!(weights.len(), used.len());
    let mut best = 0usize;
    let mut best_score = f32::NEG_INFINITY;
    for (i, (u, w)) in used.iter().zip(weights).enumerate() {
        let score = *w / (1.0 + *u as f32);
        if score > best_score {
            best_score = score;
            best = i;
        }
    }
    used[best] += 1;
    best
}

fn embedding_weights(
    entries: &[&AssetEntry],
    track_emb: Option<&[f32]>,
    embeddings: &AssetEmbeddings,
) -> Vec<f32> {
    let Some(query) = track_emb else {
        return vec![1.0; entries.len()];
    };
    let raw: Vec<f32> = entries
        .iter()
        .map(|e| {
            embeddings
                .map
                .get(&e.file)
                .map(|a| crate::analysis::dot_normed(query, a))
                .unwrap_or(f32::NAN) // unembedded entries are neutral
        })
        .collect();
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut covered = false;
    for c in raw.iter().filter(|c| c.is_finite()) {
        lo = lo.min(*c);
        hi = hi.max(*c);
        covered = true;
    }
    if !covered {
        return vec![1.0; entries.len()];
    }
    let span = (hi - lo).max(1e-6);
    raw.iter()
        .map(|c| {
            if c.is_finite() {
                0.4 + 1.2 * (*c - lo) / span
            } else {
                1.0
            }
        })
        .collect()
}

/// World position for a landmark anchor. Cardinal rotates around the rim by
/// index so multiple Cardnals spread out; Rim sits on the far horizon.
fn landmark_anchor(anchor: crate::recipe::Anchor, i: usize) -> Vec3 {
    use crate::recipe::Anchor;
    match anchor {
        Anchor::Center => Vec3::new(0.0, -0.5, -6.0),
        Anchor::Cardinal => {
            let ang = i as f32 * std::f32::consts::FRAC_PI_2;
            Vec3::new(ang.cos() * 16.0, -0.5, ang.sin() * 16.0 - 4.0)
        }
        Anchor::Rim => {
            let ang = (i as f32 + 0.5) * std::f32::consts::FRAC_PI_2;
            Vec3::new(ang.cos() * 40.0, -0.5, ang.sin() * 40.0)
        }
    }
}

// --- perf stress test (VERSE_STRESS) ---------------------------------------

/// Dev perf validation: `VERSE_STRESS=5000 cargo run` spawns that many prop
/// instances (cycling the current mood's manifest set) and logs frame rates —
/// the idea.md Stage-1 budget is 60 fps @ ~5k instances on a mid GPU, which
/// ARCHITECTURE §3 flags as an unvalidated claim until measured.
#[derive(Resource)]
pub struct StressTest {
    pub target: usize,
    spawned: bool,
    frames: u32,
    total_frames: u32,
    window: f32,
    total: f32,
}

impl StressTest {
    pub fn from_env() -> Option<Self> {
        let target = std::env::var("VERSE_STRESS").ok()?.parse().ok()?;
        // N=0 is the control run: no props, just the frame-rate report.
        Some(Self {
            target,
            spawned: false,
            frames: 0,
            total_frames: 0,
            window: 0.0,
            total: 0.0,
        })
    }
}

/// Spawn the stress field once, cycling the current mood's manifest entries on
/// the same golden-angle spiral as the real world. No `PropRise` — we measure
/// the steady-state frame cost, not the materialize hit.
pub fn stress_spawn(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    assets: Res<WorldAssets>,
    theme: Res<Theme>,
    mut stress: ResMut<StressTest>,
) {
    if stress.spawned {
        return;
    }
    // The manifest loads at Startup; wait until it (or its absence) is known.
    let Some(manifest) = &assets.manifest else {
        return;
    };
    stress.spawned = true;
    let mood = theme.mood % crate::theme::moods().len();
    let arrangement = crate::theme::moods()[mood].arrangement;
    let mood_entries: Vec<_> = manifest
        .assets
        .iter()
        .filter(|a| a.mood_index() == mood)
        .collect();
    let entries: Vec<_> = if mood_entries.is_empty() {
        manifest.assets.iter().collect()
    } else {
        mood_entries
    };
    if entries.is_empty() {
        warn!("stress: manifest has no assets");
        return;
    }

    let mut rng = 0xDEAD_5EED_u64;
    for i in 0..stress.target {
        let entry = entries[i % entries.len()];
        let handle: Handle<_> = asset_server
            .load(GltfAssetLabel::Scene(0).from_asset(format!("models/{}", entry.file)));
        let pos = layout_position(arrangement, i, &mut rng);
        let scale = entry.placement_scale() * (0.85 + rand01(&mut rng) * 0.3);
        let mut e = commands.spawn((
            WorldProp,
            WorldAssetRoot(handle),
            Transform::from_translation(pos)
                .with_scale(Vec3::splat(scale))
                .with_rotation(Quat::from_rotation_y(
                    rand01(&mut rng) * std::f32::consts::TAU,
                )),
        ));
        if let Some(range) = entry.tier.visibility_range() {
            e.insert(range);
        }
    }
    info!("stress: spawning {} prop roots", stress.target);
}

/// Log windowed FPS every 5 s; exit with an overall summary after 30 s.
pub fn stress_report(
    time: Res<Time<bevy::time::Real>>,
    mut stress: ResMut<StressTest>,
    mut exit: MessageWriter<AppExit>,
) {
    let dt = time.delta_secs();
    stress.frames += 1;
    stress.total_frames += 1;
    stress.window += dt;
    stress.total += dt;
    if stress.window >= 5.0 {
        info!(
            "stress: {:.1} fps ({} frames / {:.1}s)",
            stress.frames as f32 / stress.window,
            stress.frames,
            stress.window
        );
        stress.frames = 0;
        stress.window = 0.0;
    }
    if stress.total >= 30.0 {
        info!(
            "stress: done — overall {:.1} fps over {:.0}s",
            stress.total_frames as f32 / stress.total,
            stress.total
        );
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A relative root is the packaging bug: it happens to work while the
    /// working directory is the app's own (`cargo run`, `./localgpt-verse` inside
    /// `dist/`) and silently resolves to nothing the moment the app is
    /// launched from Finder or a shortcut — no models, no fonts, no starter
    /// music, no ML tiers, and a procedural world instead of an error.
    #[test]
    fn asset_root_is_an_absolute_directory() {
        let root = asset_root();
        assert!(
            root.is_absolute(),
            "asset root fell through to a relative path: {}",
            root.display()
        );
        assert!(
            root.is_dir(),
            "asset root is not a directory: {}",
            root.display()
        );
    }

    fn entry(file: &str, name: &str, tier: Tier) -> AssetEntry {
        AssetEntry {
            id: file.into(),
            name: name.into(),
            file: file.into(),
            kind: String::new(),
            tier,
            mood: 0,
            mood_id: None,
            scale: 1.0,
            dims: None,
            license: "CC0".into(),
            author: "test".into(),
            source: "test".into(),
        }
    }

    fn kind_entry(file: &str, kind: &str, tier: Tier, mood: usize) -> AssetEntry {
        AssetEntry {
            kind: kind.into(),
            mood,
            ..entry(file, file, tier)
        }
    }

    /// The two-level vocabulary's host half: mood preference, anti-repeat
    /// rotation, cross-mood fallback, and determinism.
    #[test]
    fn resolve_kind_prefers_mood_then_rotates() {
        let mut m = AssetManifest {
            version: 2,
            assets: vec![
                kind_entry("tide_rock_a.glb", "rock", Tier::Hero, 2),
                kind_entry("tide_rock_b.glb", "rock", Tier::Hero, 2),
                kind_entry("ember_rock.glb", "rock", Tier::Hero, 0),
                kind_entry("tide_tree.glb", "tree", Tier::Hero, 2),
            ],
        };
        let mut used = Vec::new();
        // Tide mood: both tide rocks rotate before anything else.
        assert_eq!(
            resolve_kind(&m, "rock", Some(2), &mut used).unwrap().file,
            "tide_rock_a.glb"
        );
        assert_eq!(
            resolve_kind(&m, "rock", Some(2), &mut used).unwrap().file,
            "tide_rock_b.glb"
        );
        // Pool exhausted → restart deterministically from the top, not the
        // ember fallback (mood pool still preferred over cross-mood).
        assert_eq!(
            resolve_kind(&m, "rock", Some(2), &mut used).unwrap().file,
            "tide_rock_a.glb"
        );
        // A mood with no rocks of its own falls back across moods.
        let mut used = Vec::new();
        assert_eq!(
            resolve_kind(&m, "rock", Some(3), &mut used).unwrap().file,
            "tide_rock_a.glb"
        );
        // Unknown kind resolves to None (the session reports an error).
        let mut used = Vec::new();
        assert!(resolve_kind(&m, "crystal", Some(2), &mut used).is_none());
        // Determinism: identical calls, identical answers.
        let mut a = Vec::new();
        let mut b = Vec::new();
        for _ in 0..5 {
            resolve_kind(&m, "rock", Some(2), &mut a);
            resolve_kind(&m, "rock", Some(2), &mut b);
        }
        assert_eq!(a, b);
        // Extended mood (6 = abyss, base 2 = tide) consults its own pool
        // first, then the base quadrant's.
        m.assets
            .push(kind_entry("abyss_rock.glb", "rock", Tier::Hero, 6));
        let mut used = Vec::new();
        assert_eq!(
            resolve_kind(&m, "rock", Some(6), &mut used).unwrap().file,
            "abyss_rock.glb"
        );
        assert_eq!(
            resolve_kind(&m, "rock", Some(6), &mut used).unwrap().file,
            "tide_rock_a.glb"
        );
    }

    #[test]
    fn manifest_kinds_are_distinct_in_order() {
        let m = AssetManifest {
            version: 2,
            assets: vec![
                kind_entry("a.glb", "rock", Tier::Hero, 0),
                kind_entry("b.glb", "rock", Tier::Medium, 0),
                kind_entry("c.glb", "lamp", Tier::Medium, 1),
                entry("d.glb", "D", Tier::Scatter), // no kind — excluded
            ],
        };
        assert_eq!(m.kinds(), vec!["rock", "lamp"]);
    }

    /// Scatter fields replay identically and stay inside their disk.
    #[test]
    fn scatter_offsets_are_deterministic_and_bounded() {
        let a = scatter_offsets(fold_seed("track1|field"), 24, 10.0);
        let b = scatter_offsets(fold_seed("track1|field"), 24, 10.0);
        assert_eq!(a, b, "same seed → identical field");
        assert_ne!(
            scatter_offsets(fold_seed("track2|field"), 24, 10.0),
            a,
            "different track → different field"
        );
        assert_eq!(a.len(), 24);
        for p in &a {
            assert_eq!(p.y, 0.0);
            assert!(p.x * p.x + p.z * p.z <= 10.0 * 10.0 + 1e-4, "inside disk");
        }
        // Seeds spread: not every offset collapsed onto the first draw.
        let distinct = a
            .iter()
            .map(|p| (p.x * 100.0).round() as i32 * 997 + (p.z * 100.0).round() as i32)
            .collect::<std::collections::HashSet<_>>();
        assert!(distinct.len() > 20, "offsets are spread, not stacked");
    }

    /// The tier-budget spender: variety-spreading, weight-tilting, budget-cap.
    #[test]
    fn weighted_round_robin_spreads_and_respects_weights() {
        // Equal weights over 4 entries, 8 slots → exactly 2 each, manifest
        // order on ties.
        let mut used = vec![0u32; 4];
        for _ in 0..8 {
            pick_weighted_round_robin(&[1.0; 4], &mut used);
        }
        assert_eq!(used, vec![2, 2, 2, 2]);
        // A 2× weight earns ~2× the slots without starving the others.
        let mut used = vec![0u32; 3];
        for _ in 0..12 {
            pick_weighted_round_robin(&[2.0, 1.0, 1.0], &mut used);
        }
        assert_eq!(used, vec![6, 3, 3]);
        // Budget cap: 3 entries, 2 slots → two distinct entries.
        let mut used = vec![0u32; 3];
        pick_weighted_round_robin(&[1.0; 3], &mut used);
        pick_weighted_round_robin(&[1.0; 3], &mut used);
        assert_eq!(used.iter().sum::<u32>(), 2);
        assert_eq!(used, vec![1, 1, 0]);
    }

    #[test]
    fn hero_fallback_prefers_the_track_scored_hero() {
        // Names deliberately free of Gateway keywords, so the keyword path
        // finds nothing and the fallback decides.
        let heroes: Vec<AssetEntry> = vec![
            entry("a.glb", "Granite Slab", Tier::Hero),
            entry("b.glb", "Basalt Shelf", Tier::Hero),
        ];
        let refs: Vec<&AssetEntry> = heroes.iter().collect();
        let mut used = Vec::new();
        // The score closure says the track favours the basalt shelf.
        let score = |e: &AssetEntry| if e.file == "b.glb" { 1.0 } else { 0.0 };
        let picked = pick_hero_for_kind(
            &refs,
            crate::recipe::LandmarkKind::Gateway,
            &mut used,
            Some(&score),
        )
        .expect("a hero is picked");
        assert_eq!(picked.file, "b.glb");
        // …and without scores the same call takes manifest order (determinism).
        let mut used = Vec::new();
        let picked =
            pick_hero_for_kind(&refs, crate::recipe::LandmarkKind::Gateway, &mut used, None)
                .unwrap();
        assert_eq!(picked.file, "a.glb");
    }

    #[test]
    fn keyword_match_still_beats_embedding_order() {
        // A monolith query should not rerank a real "tower" keyword hit.
        let heroes: Vec<AssetEntry> = vec![
            entry("a.glb", "Coral Arch", Tier::Hero),
            entry("b.glb", "Clock Tower", Tier::Hero),
        ];
        let refs: Vec<&AssetEntry> = heroes.iter().collect();
        let mut used = Vec::new();
        let score = |e: &AssetEntry| if e.file == "a.glb" { 1.0 } else { 0.0 };
        let picked = pick_hero_for_kind(
            &refs,
            crate::recipe::LandmarkKind::Spire,
            &mut used,
            Some(&score),
        )
        .unwrap();
        assert_eq!(picked.file, "b.glb", "keyword match wins over embeddings");
    }

    #[test]
    fn kind_affinity_matches_where_names_do_not() {
        // Manifest v2 kinds score like keyword hits: a Monument query finds
        // the statue even with a keyword-free display name.
        let mut statue = entry("a.glb", "Strange Object", Tier::Hero);
        statue.kind = "statue".into();
        let mut crate_entry = entry("b.glb", "Another Thing", Tier::Hero);
        crate_entry.kind = "container".into();
        let heroes = [statue, crate_entry];
        let refs: Vec<&AssetEntry> = heroes.iter().collect();
        let mut used = Vec::new();
        let picked = pick_hero_for_kind(
            &refs,
            crate::recipe::LandmarkKind::Monument,
            &mut used,
            None,
        )
        .unwrap();
        assert_eq!(picked.file, "a.glb");
    }

    #[test]
    fn embedding_weights_are_neutral_without_signals() {
        let entries: Vec<AssetEntry> = vec![
            entry("a.glb", "A", Tier::Medium),
            entry("b.glb", "B", Tier::Medium),
        ];
        let refs: Vec<&AssetEntry> = entries.iter().collect();
        let empty = AssetEmbeddings::default();
        assert_eq!(embedding_weights(&refs, None, &empty), vec![1.0, 1.0]);
        // A track embedding with no asset coverage is also neutral.
        let query = vec![1.0f32; 4];
        assert_eq!(
            embedding_weights(&refs, Some(&query), &empty),
            vec![1.0, 1.0]
        );
    }

    #[test]
    fn embedding_weights_rank_and_bound() {
        let entries: Vec<AssetEntry> = vec![
            entry("a.glb", "A", Tier::Medium),
            entry("b.glb", "B", Tier::Medium),
            entry("c.glb", "C", Tier::Medium), // unembedded → neutral
        ];
        let refs: Vec<&AssetEntry> = entries.iter().collect();
        let mut embeddings = AssetEmbeddings::default();
        embeddings.map.insert("a.glb".into(), vec![1.0, 0.0]);
        embeddings.map.insert("b.glb".into(), vec![0.0, 1.0]);
        let query = vec![1.0f32, 0.0];
        let w = embedding_weights(&refs, Some(&query), &embeddings);
        assert!((w[0] - 1.6).abs() < 1e-6, "best match gets the top weight");
        assert!((w[1] - 0.4).abs() < 1e-6, "worst match the floor");
        assert_eq!(w[2], 1.0, "uncovered assets stay neutral");
        // Deterministic: same inputs, same weights.
        assert_eq!(w, embedding_weights(&refs, Some(&query), &embeddings));
    }

    #[test]
    fn stagger_ends_on_settle_and_orders() {
        let settle = 2.4;
        let n = 16;
        let first = stagger_delay(0, n, settle);
        let last = stagger_delay(n - 1, n, settle);
        assert_eq!(first, FIRST_DELAY);
        // Last prop finishes its rise exactly at the settle time.
        assert!((last + RISE_SECS - settle).abs() < 1e-5);
        // Monotonic stagger.
        for i in 1..n {
            assert!(stagger_delay(i, n, settle) >= stagger_delay(i - 1, n, settle));
        }
    }

    #[test]
    fn stagger_clamps_tight_settles() {
        // A settle earlier than one rise can finish still yields a sane delay.
        let d = stagger_delay(3, 4, 0.5);
        assert!(d >= FIRST_DELAY);
    }

    #[test]
    fn seeded_layout_is_deterministic() {
        let (mut a, mut b) = (42u64, 42u64);
        for _ in 0..8 {
            assert_eq!(splitmix(&mut a), splitmix(&mut b));
        }
        let mut s = 7u64;
        for _ in 0..100 {
            let v = rand01(&mut s);
            assert!((0.0..1.0).contains(&v));
        }
        let p = std::path::Path::new("/music/a.flac");
        assert_eq!(path_seed(p), path_seed(p));
        assert_ne!(
            path_seed(p),
            path_seed(std::path::Path::new("/music/b.flac"))
        );
    }

    #[test]
    fn layout_styles_are_deterministic_and_distinct() {
        for arrangement in ALL_ARRANGEMENTS {
            let (mut a, mut b) = (99u64, 99u64);
            for i in 0..40 {
                assert_eq!(
                    layout_position(arrangement, i, &mut a),
                    layout_position(arrangement, i, &mut b)
                );
            }
        }
        // Grid: positions snap to the 7.5m pitch lattice ±jitter.
        let mut rng = 5u64;
        let p = layout_position(Arrangement::Grid, 23, &mut rng);
        let lattice = |v: f32, off: f32| ((v - off) / 7.5).fract().abs();
        assert!(lattice(p.x, -33.75) < 0.25 || lattice(p.x, -33.75) > 0.75);
        // Rings: radius stays within the ring band.
        let mut rng = 5u64;
        let p = layout_position(Arrangement::Rings, 40, &mut rng);
        let r = (p.x * p.x + p.z * p.z).sqrt();
        assert!((7.0..60.0).contains(&r), "ring radius {r}");
    }

    const ALL_ARRANGEMENTS: [Arrangement; 5] = [
        Arrangement::Spiral,
        Arrangement::Grid,
        Arrangement::Rings,
        Arrangement::Clusters,
        Arrangement::Terraces,
    ];

    /// Average nearest-neighbour distance — a cheap proxy for "does this read
    /// as clumped or as evenly spread".
    fn mean_nearest_neighbour(arrangement: Arrangement, n: usize) -> f32 {
        let mut rng = 7u64;
        let points: Vec<Vec3> = (0..n)
            .map(|i| layout_position(arrangement, i, &mut rng))
            .collect();
        let total: f32 = points
            .iter()
            .enumerate()
            .map(|(i, p)| {
                points
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| *j != i)
                    .map(|(_, q)| p.distance(*q))
                    .fold(f32::MAX, f32::min)
            })
            .sum();
        total / n as f32
    }

    #[test]
    fn the_base_moods_have_distinct_arrangements_and_variants_match_their_base() {
        let moods = crate::theme::moods();
        let base = &moods[..crate::theme::ASSET_BASE_MOODS.min(moods.len())];
        // The base four each own a distinct arrangement (world identity).
        for (i, a) in base.iter().map(|m| m.arrangement).enumerate() {
            for (j, b) in base.iter().map(|m| m.arrangement).enumerate() {
                assert!(
                    i == j || a != b,
                    "`{}` and `{}` share the {a:?} arrangement",
                    base[i].id,
                    base[j].id
                );
            }
        }
        // The extended variants borrow their base quadrant's asset set — and
        // must lay it out the same way, or the "same neighbourhood, different
        // hour" promise breaks.
        for (i, m) in moods.iter().enumerate().skip(base.len()) {
            let b = &base[i % base.len()];
            assert_eq!(
                m.arrangement, b.arrangement,
                "`{}` must arrange like its base `{}`",
                m.id, b.id
            );
        }
    }

    #[test]
    fn clusters_clump_more_tightly_than_the_spiral() {
        // The point of Clusters is groups with open ground between them, so
        // neighbours sit closer than in an evenly-spread layout.
        let clustered = mean_nearest_neighbour(Arrangement::Clusters, 40);
        let spread = mean_nearest_neighbour(Arrangement::Spiral, 40);
        assert!(
            clustered < spread,
            "clusters ({clustered}) should be tighter than spiral ({spread})"
        );
    }

    #[test]
    fn terraces_form_distinct_rows() {
        // Seven props per row, so the first seven share a row band and the
        // eighth starts the next one.
        let mut rng = 3u64;
        let points: Vec<Vec3> = (0..14)
            .map(|i| layout_position(Arrangement::Terraces, i, &mut rng))
            .collect();
        let row0_spread = points[..7].iter().map(|p| p.z).fold(f32::MIN, f32::max)
            - points[..7].iter().map(|p| p.z).fold(f32::MAX, f32::min);
        let row_gap = (points[7].z - points[0].z).abs();
        assert!(
            row_gap > row0_spread / 2.0,
            "rows ({row_gap}) are not separated from within-row spread ({row0_spread})"
        );
    }

    #[test]
    fn open_centre_arrangements_keep_the_camera_clear() {
        // Spiral, Rings, and Clusters treat the centre as the camera's focus
        // and orbit path. Grid deliberately does not — a city block reads as a
        // city because you stand inside it — and Terraces runs rows across the
        // whole span, so neither promises a clear middle.
        for arrangement in [
            Arrangement::Spiral,
            Arrangement::Rings,
            Arrangement::Clusters,
        ] {
            let mut rng = 11u64;
            let close = (0..60)
                .map(|i| layout_position(arrangement, i, &mut rng))
                .filter(|p| (p.x * p.x + p.z * p.z).sqrt() < 3.0)
                .count();
            assert_eq!(close, 0, "{arrangement:?} placed props on the camera");
        }
    }
}
