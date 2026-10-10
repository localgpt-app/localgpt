//! The deterministic draft: a parsed scene becomes a `WorldManifest`
//! with a set, a cast and coverage.
//!
//! Everything below is a function of the screenplay text — same text,
//! same bytes:
//!
//! - **Set.** `INT.` builds a room: an 8 × 6 m floor, three walls 3 m
//!   high (the open side faces +Z, toward the coverage), a ceiling
//!   light. `EXT.` (and `EST.`/`I/E`, staged as exteriors) builds a
//!   40 × 40 m ground with a sun and sky from the heading's time of
//!   day (`DAY`, `NIGHT`, `DUSK`/`EVENING`, `DAWN`/`MORNING`; anything
//!   else, including none, is day).
//! - **Cast.** Every character with dialogue, in first-appearance
//!   order. One actor stands at the centre; two face each other 1.4 m
//!   apart; three or more form an arc of radius 1.8 m facing the open
//!   side. Each actor is a stand-in figure: a 1.7 m capsule body and a
//!   head sphere, named after the character.
//! - **Coverage.** Shot `<scene>A` is the master: a wide on 24 mm
//!   (Super 35, cropped to 2.39), at 1.6 m, aimed at the cast's
//!   centroid from far enough down the open side that every actor
//!   projects inside the frame. Then one single per speaking
//!   character, in first-speaking order (`<scene>B`, `<scene>C`, …):
//!   50 mm at eye height from just beside the scene partner's
//!   shoulder, aimed at the speaker's head.
//! - **Timing.** A page-time estimate: dialogue at 2.5 words a second,
//!   action at 1 second a line, rounded to 0.1 s. The master covers
//!   the whole scene; a single covers its speaker's first-to-last
//!   line.

use serde_json::json;

use localgpt_world_types as wt;

use crate::cinema::{self, CameraComponent};
use crate::fountain::{Element, Script};

/// Eye (and camera) height for the coverage, m.
pub const EYE_HEIGHT: f64 = 1.6;
/// The actor stand-in's total height, m.
pub const ACTOR_HEIGHT: f32 = 1.7;
/// The head sphere's centre height (the singles' aim height), m.
pub const HEAD_HEIGHT: f64 = 1.6;
/// The master's lens, mm.
pub const MASTER_FOCAL_MM: f64 = 24.0;
/// The singles' lens, mm.
pub const SINGLE_FOCAL_MM: f64 = 50.0;
/// The show's frame aspect (the crop the whole coverage shares).
pub const SHOW_ASPECT: f64 = 2.39;
/// Dialogue pace for the page-time estimate, words a second.
pub const WORDS_PER_SECOND: f64 = 2.5;

/// Interior or exterior, from the heading's prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneKind {
    /// `INT.` (and the default for `.`-forced headings).
    Interior,
    /// `EXT.` / `EST.`
    Exterior,
    /// `INT./EXT.` / `INT/EXT.` / `I/E` — staged as an exterior.
    Mixed,
}

/// The heading's declared time of day (drives the exterior sun and sky).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeOfDay {
    /// `DAY` (also the default when the heading says nothing).
    Day,
    /// `NIGHT`
    Night,
    /// `DUSK` / `EVENING`
    Dusk,
    /// `DAWN` / `MORNING`
    Dawn,
}

/// One scene of the screenplay: its heading and the elements under it.
#[derive(Debug, Clone)]
pub struct Scene {
    /// 1-based scene number (the shot names' prefix: scene 1 → 1A, 1B…).
    pub number: usize,
    /// The heading text as written (`INT. KITCHEN - DAY`).
    pub heading: String,
    /// Interior / exterior.
    pub kind: SceneKind,
    /// The declared time of day, when the heading names one.
    pub time_of_day: Option<TimeOfDay>,
    /// The elements between this heading and the next.
    pub elements: Vec<Element>,
}

/// A character with dialogue in a scene, with the page-time span their
/// lines cover.
#[derive(Debug, Clone)]
pub struct Speaker {
    /// The character's name (cue text).
    pub name: String,
    /// Their dialogue blocks, in order.
    pub lines: Vec<String>,
    /// Seconds into the scene of their first line, rounded to 0.1.
    pub in_s: f64,
    /// Seconds into the scene at the end of their last line, rounded.
    pub out_s: f64,
}

/// Split a parsed screenplay into scenes (the elements before the first
/// heading belong to no scene and are dropped).
pub fn scenes(script: &Script) -> Vec<Scene> {
    let mut out: Vec<Scene> = Vec::new();
    for element in &script.elements {
        if let Element::SceneHeading { text, forced } = element {
            out.push(Scene {
                number: out.len() + 1,
                heading: text.clone(),
                kind: scene_kind(text, *forced),
                time_of_day: time_of_day(text),
                elements: Vec::new(),
            });
        } else if let Some(scene) = out.last_mut() {
            scene.elements.push(element.clone());
        }
    }
    out
}

/// The heading's kind: the `INT.`/`EXT.`/`EST.`/`I/E` prefixes; a
/// `.`-forced heading takes the same prefixes or defaults interior.
fn scene_kind(text: &str, forced: bool) -> SceneKind {
    let upper = text.to_uppercase();
    for (prefix, kind) in [
        ("INT./EXT.", SceneKind::Mixed),
        ("INT/EXT.", SceneKind::Mixed),
        ("I/E", SceneKind::Mixed),
        ("INT.", SceneKind::Interior),
        ("EXT.", SceneKind::Exterior),
        ("EST.", SceneKind::Exterior),
    ] {
        if upper.starts_with(prefix) {
            return kind;
        }
    }
    if forced {
        return SceneKind::Interior;
    }
    SceneKind::Interior
}

/// The heading's time of day: the ` - DAY` tail, when it names one.
fn time_of_day(text: &str) -> Option<TimeOfDay> {
    let tail = text.rsplit(" - ").next()?.to_uppercase();
    let word = tail.split_whitespace().next()?;
    match word {
        "DAY" => Some(TimeOfDay::Day),
        "NIGHT" => Some(TimeOfDay::Night),
        "DUSK" | "EVENING" => Some(TimeOfDay::Dusk),
        "DAWN" | "MORNING" => Some(TimeOfDay::Dawn),
        _ => None,
    }
}

/// Round to the page-time estimate's 0.1 s.
fn round_tenth(t: f64) -> f64 {
    (t * 10.0).round() / 10.0
}

/// Round a coordinate to 0.1 mm — the draft's grid. Keeps the
/// canonical manifest text tidy (`0.7`, not the f32 neighbourhood of
/// it) without changing the staging.
fn round_mm(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

fn round3(v: [f64; 3]) -> [f64; 3] {
    [round_mm(v[0]), round_mm(v[1]), round_mm(v[2])]
}

/// The scene's speakers in first-appearance order, with the page-time
/// spans. The scene's total duration (master `out_s`) comes back too.
pub fn speakers(scene: &Scene) -> (Vec<Speaker>, f64) {
    let mut speakers: Vec<Speaker> = Vec::new();
    let mut order: Vec<usize> = Vec::new(); // speaker index per dialogue event
    let mut t = 0.0_f64;
    let mut current: Option<usize> = None;
    for element in &scene.elements {
        match element {
            Element::Action { lines } => {
                t += lines.len() as f64 * 1.0;
                current = None;
            }
            Element::Character { name, .. } => {
                let index = match speakers.iter().position(|s| &s.name == name) {
                    Some(i) => i,
                    None => {
                        speakers.push(Speaker {
                            name: name.clone(),
                            lines: Vec::new(),
                            in_s: 0.0,
                            out_s: 0.0,
                        });
                        speakers.len() - 1
                    }
                };
                if speakers[index].lines.is_empty() {
                    speakers[index].in_s = t;
                }
                current = Some(index);
            }
            Element::Dialogue { text } => {
                let Some(index) = current else { continue };
                let words = text.split_whitespace().count() as f64;
                let duration = words / WORDS_PER_SECOND;
                speakers[index].lines.push(text.clone());
                t += duration;
                speakers[index].out_s = t;
                order.push(index);
            }
            // Parentheticals time with the dialogue around them.
            _ => {}
        }
    }
    for speaker in &mut speakers {
        speaker.in_s = round_tenth(speaker.in_s);
        speaker.out_s = round_tenth(speaker.out_s);
    }
    (speakers, round_tenth(t))
}

/// The cast's ground positions (and facing yaw), in speaker order:
/// one at the centre, two facing each other 1.4 m apart, three or
/// more on an arc of radius 1.8 m facing the open side (+Z).
fn cast_positions(count: usize) -> Vec<([f64; 3], f64)> {
    match count {
        0 => Vec::new(),
        1 => vec![([0.0, 0.0, 0.0], 180.0)],
        2 => vec![
            ([-0.7, 0.0, 0.0], -90.0),
            ([0.7, 0.0, 0.0], 90.0),
        ],
        n => (0..n)
            .map(|i| {
                let phi = (-45.0 + 90.0 * i as f64 / (n - 1) as f64).to_radians();
                ([1.8 * phi.sin(), 0.0, -1.8 * phi.cos()], 180.0)
            })
            .collect(),
    }
}

/// A speaker's scene partner for the over-the-shoulder single: the
/// other character who spoke most recently before their first line,
/// else the next other character to speak, else none (a solo scene
/// gets its single from the open side).
fn partner_of(scene: &Scene, speaker: &str) -> Option<String> {
    let mut events: Vec<&str> = Vec::new();
    for element in &scene.elements {
        if let Element::Character { name, .. } = element {
            events.push(name);
        }
    }
    let first = events.iter().position(|n| *n == speaker)?;
    events[..first]
        .iter()
        .rev()
        .find(|n| **n != speaker)
        .or_else(|| events[first + 1..].iter().find(|n| **n != speaker))
        .map(|n| n.to_string())
}

/// The shot suffix: A, B, …, Z, AA, AB, …
fn shot_suffix(index: usize) -> String {
    let mut index = index;
    let mut s = String::new();
    loop {
        s.insert(0, (b'A' + (index % 26) as u8) as char);
        index /= 26;
        if index == 0 {
            return s;
        }
        index -= 1;
    }
}

fn cuboid(
    id: u64,
    name: &str,
    dims: [f32; 3],
    position: [f32; 3],
    color: [f32; 4],
) -> wt::WorldEntity {
    let mut e = wt::WorldEntity::new(id, name);
    e.transform.position = position;
    e.shape = Some(wt::Shape::Cuboid {
        x: dims[0],
        y: dims[1],
        z: dims[2],
    });
    e.material = Some(wt::MaterialDef {
        color,
        roughness: 0.95,
        ..Default::default()
    });
    e
}

/// The deterministic draft: stage one scene as a `WorldManifest`.
pub fn stage(scene: &Scene) -> wt::WorldManifest {
    let (speakers, total_s) = speakers(scene);
    let mut manifest = wt::WorldManifest::new(format!("scene-{}", scene.number));
    manifest.meta.description = Some(scene.heading.clone());
    manifest.meta.time_of_day = Some(match scene.time_of_day.unwrap_or(TimeOfDay::Day) {
        TimeOfDay::Day => 12.0,
        TimeOfDay::Night => 0.0,
        TimeOfDay::Dusk => 19.5,
        TimeOfDay::Dawn => 6.0,
    });

    let mut next_id = 1_u64;
    let mut alloc = || {
        let id = next_id;
        next_id += 1;
        id
    };

    // ---- Set ----
    match scene.kind {
        SceneKind::Interior => {
            // An 8 × 6 m room, three walls 3 m high, open side +Z.
            manifest.environment = Some(wt::EnvironmentDef {
                background_color: Some([0.05, 0.05, 0.07, 1.0]),
                ambient_intensity: Some(240.0),
                ambient_color: Some([1.0, 0.95, 0.88, 1.0]),
                ..Default::default()
            });
            let wall = [0.72, 0.68, 0.62, 1.0];
            manifest.entities.push(cuboid(
                alloc(),
                "floor",
                [8.0, 0.1, 6.0],
                [0.0, -0.05, 0.0],
                [0.4, 0.36, 0.3, 1.0],
            ));
            manifest.entities.push(cuboid(
                alloc(),
                "wall-back",
                [8.0, 3.0, 0.1],
                [0.0, 1.5, -3.05],
                wall,
            ));
            manifest.entities.push(cuboid(
                alloc(),
                "wall-left",
                [0.1, 3.0, 6.0],
                [-4.05, 1.5, 0.0],
                wall,
            ));
            manifest.entities.push(cuboid(
                alloc(),
                "wall-right",
                [0.1, 3.0, 6.0],
                [4.05, 1.5, 0.0],
                wall,
            ));
            let mut light = wt::WorldEntity::new(alloc(), "ceiling-light");
            light.transform.position = [0.0, 2.9, 0.0];
            light.light = Some(wt::LightDef {
                light_type: wt::LightType::Point,
                color: [1.0, 0.9, 0.8, 1.0],
                // Room-scale on the format's lumen scale (the conformance
                // suite's points run 60k–400k).
                intensity: 120_000.0,
                range: Some(12.0),
                shadows: true,
                ..Default::default()
            });
            manifest.entities.push(light);
        }
        SceneKind::Exterior | SceneKind::Mixed => {
            let time = scene.time_of_day.unwrap_or(TimeOfDay::Day);
            // Sky, ambient and sun per the heading's time of day.
            let (sky, ambient, sun_color, sun_lux, sun_pos): (
                [f32; 4],
                f32,
                [f32; 4],
                f32,
                [f32; 3],
            ) = match time {
                TimeOfDay::Day => (
                    [0.55, 0.7, 0.9, 1.0],
                    300.0,
                    [1.0, 0.96, 0.9, 1.0],
                    12_000.0,
                    [12.0, 20.0, 8.0],
                ),
                TimeOfDay::Night => (
                    [0.02, 0.03, 0.08, 1.0],
                    25.0,
                    [0.35, 0.45, 0.7, 1.0],
                    150.0,
                    [-8.0, 18.0, -6.0],
                ),
                TimeOfDay::Dusk => (
                    [0.4, 0.25, 0.3, 1.0],
                    160.0,
                    [1.0, 0.6, 0.35, 1.0],
                    5_000.0,
                    [20.0, 5.0, 4.0],
                ),
                TimeOfDay::Dawn => (
                    [0.6, 0.55, 0.62, 1.0],
                    200.0,
                    [1.0, 0.8, 0.65, 1.0],
                    6_000.0,
                    [-20.0, 6.0, 4.0],
                ),
            };
            manifest.environment = Some(wt::EnvironmentDef {
                background_color: Some(sky),
                ambient_intensity: Some(ambient),
                ambient_color: Some([1.0, 1.0, 1.0, 1.0]),
                ..Default::default()
            });
            let mut ground = wt::WorldEntity::new(alloc(), "ground");
            ground.shape = Some(wt::Shape::Plane { x: 40.0, z: 40.0 });
            ground.material = Some(wt::MaterialDef {
                color: [0.35, 0.45, 0.3, 1.0],
                roughness: 0.95,
                ..Default::default()
            });
            manifest.entities.push(ground);
            let l = (sun_pos[0] * sun_pos[0] + sun_pos[1] * sun_pos[1] + sun_pos[2] * sun_pos[2])
                .sqrt();
            let mut sun = wt::WorldEntity::new(alloc(), "sun");
            sun.transform.position = sun_pos;
            sun.light = Some(wt::LightDef {
                light_type: wt::LightType::Directional,
                color: sun_color,
                intensity: sun_lux,
                direction: Some([
                    -sun_pos[0] / l,
                    -sun_pos[1] / l,
                    -sun_pos[2] / l,
                ]),
                shadows: true,
                ..Default::default()
            });
            manifest.entities.push(sun);
        }
    }

    // ---- Cast ----
    const PALETTE: [[f32; 4]; 8] = [
        [0.75, 0.3, 0.25, 1.0],
        [0.25, 0.45, 0.75, 1.0],
        [0.3, 0.65, 0.4, 1.0],
        [0.8, 0.65, 0.25, 1.0],
        [0.6, 0.35, 0.7, 1.0],
        [0.85, 0.5, 0.3, 1.0],
        [0.3, 0.7, 0.7, 1.0],
        [0.7, 0.4, 0.5, 1.0],
    ];
    let positions = cast_positions(speakers.len());
    let mut cast: Vec<(String, [f64; 3])> = Vec::new(); // (name, ground position)
    for (i, speaker) in speakers.iter().enumerate() {
        let ([x, _, z], yaw) = positions[i];
        let color = PALETTE[i % PALETTE.len()];
        // The capsule body: 1.7 m of stand-in, feet on the ground.
        let mut body = wt::WorldEntity::new(alloc(), speaker.name.clone());
        body.transform.position = [x as f32, ACTOR_HEIGHT / 2.0, z as f32];
        body.transform.rotation_degrees = [0.0, yaw as f32, 0.0];
        body.shape = Some(wt::Shape::Capsule {
            radius: 0.18,
            half_length: (ACTOR_HEIGHT - 0.36) / 2.0,
        });
        body.material = Some(wt::MaterialDef {
            color,
            roughness: 0.8,
            ..Default::default()
        });
        manifest.entities.push(body);
        let mut head = wt::WorldEntity::new(alloc(), format!("{}.head", speaker.name));
        head.transform.position = [x as f32, HEAD_HEIGHT as f32, z as f32];
        head.shape = Some(wt::Shape::Sphere { radius: 0.12 });
        head.material = Some(wt::MaterialDef {
            color: [color[0] * 0.6 + 0.4, color[1] * 0.6 + 0.4, color[2] * 0.6 + 0.4, 1.0],
            roughness: 0.7,
            ..Default::default()
        });
        manifest.entities.push(head);
        cast.push((speaker.name.clone(), [x, 0.0, z]));
    }

    // ---- Coverage ----
    let scene_number = scene.number.to_string();
    let mut camera_entity = |manifest: &mut wt::WorldManifest,
                             suffix: &str,
                             order: u32,
                             component: CameraComponent,
                             position: [f64; 3],
                             size: &str,
                             description: String,
                             in_s: f64,
                             out_s: f64| {
        let position = round3(position);
        let mut component = component;
        component.aim = component.aim.map(round3);
        let mut e = wt::WorldEntity::new(alloc(), format!("{}{}", scene.number, suffix));
        e.transform.position = [position[0] as f32, position[1] as f32, position[2] as f32];
        e.extra.insert(
            cinema::EXTENSION_NAME.to_string(),
            json!({
                "camera": component,
                "shot": {
                    "scene": scene_number,
                    "order": order,
                    "in_s": in_s,
                    "out_s": out_s,
                    "size": size,
                    "description": description,
                },
            }),
        );
        manifest.entities.push(e);
        position
    };

    // The master, `<scene>A`: a wide on 24 mm from down the open side,
    // far enough that every actor (feet and head) projects inside the
    // frame with a little air.
    if !cast.is_empty() {
        let n = cast.len() as f64;
        let centroid = [
            cast.iter().map(|c| c.1[0]).sum::<f64>() / n,
            0.0,
            cast.iter().map(|c| c.1[2]).sum::<f64>() / n,
        ];
        let aim = [centroid[0], 1.2, centroid[2]];
        let master = CameraComponent {
            focal_length_mm: MASTER_FOCAL_MM,
            aspect_ratio: Some(SHOW_ASPECT),
            aim: Some(aim),
            ..Default::default()
        };
        let frame = cinema::frame_of(&master);
        let mut distance = 2.0_f64;
        let master_pos = loop {
            let position = [centroid[0], EYE_HEIGHT, centroid[2] + distance];
            let view = cinema::view_of(position, &master);
            let framed = cast.iter().all(|(_, p)| {
                let feet = cinema::project(&view, &frame, &master, *p);
                let head = cinema::project(&view, &frame, &master, [p[0], ACTOR_HEIGHT as f64, p[2]]);
                [feet, head].iter().all(|q| {
                    q.z > 0.0 && q.x.abs() <= 0.95 && q.y.abs() <= 0.95
                })
            });
            if framed || distance > 60.0 {
                break position;
            }
            distance += 0.25;
        };
        let description = match scene.time_of_day {
            Some(_) => format!("Master wide — {}", scene.heading),
            None => format!("Master wide — {}, day", scene.heading.to_lowercase()),
        };
        let placed = camera_entity(
            &mut manifest,
            "A",
            1,
            master,
            master_pos,
            "WS",
            description,
            0.0,
            total_s,
        );

        // The singles, `<scene>B`, `<scene>C`, … in first-speaking
        // order: 50 mm at eye height from just beside the scene
        // partner's shoulder, aimed at the speaker's head.
        for (i, speaker) in speakers.iter().enumerate() {
            let (_, p) = &cast[i];
            let speaker_head = [p[0], HEAD_HEIGHT, p[2]];
            let position = match partner_of(scene, &speaker.name)
                .and_then(|partner| cast.iter().find(|(n, _)| n == &partner))
            {
                Some((_, q)) => {
                    // 0.45 m behind the partner (away from the speaker),
                    // 0.28 m to the side — over their shoulder.
                    let dir = {
                        let d = [p[0] - q[0], 0.0, p[2] - q[2]];
                        let l = (d[0] * d[0] + d[2] * d[2]).sqrt();
                        if l < 1e-9 { [0.0, 0.0, 1.0] } else { [d[0] / l, 0.0, d[2] / l] }
                    };
                    [
                        q[0] - dir[0] * 0.45 - dir[2] * 0.28,
                        EYE_HEIGHT,
                        q[2] - dir[2] * 0.45 + dir[0] * 0.28,
                    ]
                }
                None => [p[0] + 0.3, EYE_HEIGHT, p[2] + 2.2],
            };
            let single = CameraComponent {
                focal_length_mm: SINGLE_FOCAL_MM,
                aspect_ratio: Some(SHOW_ASPECT),
                aim: Some(speaker_head),
                ..Default::default()
            };
            camera_entity(
                &mut manifest,
                &shot_suffix(i + 1),
                (i + 2) as u32,
                single,
                position,
                "MCU",
                format!("Single on {}", speaker.name),
                speaker.in_s,
                speaker.out_s,
            );
        }

        // Visitors open the world looking through the master.
        let master_frame = cinema::frame_of(&CameraComponent {
            focal_length_mm: MASTER_FOCAL_MM,
            aspect_ratio: Some(SHOW_ASPECT),
            ..Default::default()
        });
        manifest.camera = Some(wt::CameraDef {
            position: [placed[0] as f32, placed[1] as f32, placed[2] as f32],
            look_at: [aim[0] as f32, aim[1] as f32, aim[2] as f32],
            fov_degrees: master_frame.vfov_degrees as f32,
        });
    }

    manifest.next_entity_id = next_id;
    manifest
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fountain;

    const SCRIPT: &str = "\
Title: Framing Test

INT. KITCHEN - DAY

Maya fries an egg at the counter. Kai stumbles in, half asleep.

MAYA
You're late. The eggs are cold.

KAI
(yawning)
The eggs are always cold.

MAYA
Then you're always late.

EXT. ROOFTOP - NIGHT

The three of them stare at the skyline.

MAYA
We could just leave.

KAI
And go where?

JUNE
Anywhere the radio can't reach.
";

    fn staged_scenes() -> Vec<wt::WorldManifest> {
        scenes(&fountain::parse(SCRIPT))
            .iter()
            .map(stage)
            .collect()
    }

    fn camera(manifest: &wt::WorldManifest, name: &str) -> (wt::WorldEntity, CameraComponent) {
        let entity = manifest
            .entities
            .iter()
            .find(|e| e.name.as_str() == name)
            .unwrap_or_else(|| panic!("no entity {name}"));
        let component = CameraComponent::of(entity).unwrap_or_else(|| panic!("{name} is no camera"));
        (entity.clone(), component)
    }

    #[test]
    fn the_draft_is_deterministic_same_text_same_bytes() {
        let a = staged_scenes();
        let b = staged_scenes();
        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(
                openworldformat::manifest_text_of(x),
                openworldformat::manifest_text_of(y)
            );
        }
    }

    #[test]
    fn staged_worlds_pass_the_formats_own_validation() {
        for manifest in staged_scenes() {
            let issues = openworldformat::validation::validate_manifest(
                &manifest,
                &openworldformat::validation::WorldLimits::default(),
            );
            let errors: Vec<_> = issues
                .iter()
                .filter(|i| matches!(i.severity, openworldformat::validation::Severity::Error))
                .collect();
            assert!(errors.is_empty(), "{}: {errors:?}", manifest.meta.name);
        }
    }

    #[test]
    fn every_actor_is_inside_the_masters_frame() {
        for manifest in staged_scenes() {
            let scene_number = manifest.meta.name.trim_start_matches("scene-");
            let (entity, master) = camera(&manifest, &format!("{scene_number}A"));
            let frame = cinema::frame_of(&master);
            let view = cinema::view_of(
                entity.transform.position.map(|p| p as f64),
                &master,
            );
            for entity in &manifest.entities {
                let name = entity.name.as_str();
                if !name.ends_with(".head") {
                    continue;
                }
                let feet = [
                    entity.transform.position[0] as f64,
                    0.0,
                    entity.transform.position[2] as f64,
                ];
                let p = cinema::project(&view, &frame, &master, feet);
                assert!(
                    p.in_frame(),
                    "{}: actor {name} projects at ({:.3}, {:.3}), z {:.3}",
                    manifest.meta.name,
                    p.x,
                    p.y,
                    p.z
                );
            }
        }
    }

    #[test]
    fn each_single_frames_its_speakers_head_centred() {
        for manifest in staged_scenes() {
            for entity in &manifest.entities {
                let Some(component) = CameraComponent::of(entity) else { continue };
                let shot = &entity.extra[cinema::EXTENSION_NAME]["shot"];
                if shot["size"] != "MCU" {
                    continue;
                }
                let speaker = shot["description"].as_str().unwrap().replace("Single on ", "");
                let head = manifest
                    .entities
                    .iter()
                    .find(|e| e.name.as_str() == format!("{speaker}.head"))
                    .unwrap_or_else(|| panic!("no head for {speaker}"));
                let frame = cinema::frame_of(&component);
                let view = cinema::view_of(entity.transform.position.map(|p| p as f64), &component);
                let p = cinema::project(
                    &view,
                    &frame,
                    &component,
                    head.transform.position.map(|x| x as f64),
                );
                assert!(
                    p.in_frame() && p.x.abs() < 0.05 && p.y.abs() < 0.05,
                    "{}: {} frames {} at ({:.3}, {:.3})",
                    manifest.meta.name,
                    entity.name.as_str(),
                    speaker,
                    p.x,
                    p.y
                );
            }
        }
    }

    #[test]
    fn cameras_stand_at_sane_heights() {
        for manifest in staged_scenes() {
            for entity in &manifest.entities {
                if CameraComponent::of(entity).is_some() {
                    let h = entity.transform.position[1];
                    assert!(
                        (1.0..=2.5).contains(&h),
                        "{} at {h} m",
                        entity.name.as_str()
                    );
                }
            }
        }
    }

    #[test]
    fn the_page_time_estimate() {
        let script = fountain::parse(SCRIPT);
        let scene_list = scenes(&script);
        let (speakers, total) = speakers(&scene_list[0]);
        // Scene 1: 1 action line (1 s), MAYA 6 words (2.4 s), KAI 5
        // words (2 s), MAYA 4 words (1.6 s) → 7.0 s total.
        assert_eq!(total, 7.0);
        assert_eq!(speakers.len(), 2);
        assert_eq!(speakers[0].name, "MAYA");
        assert_eq!((speakers[0].in_s, speakers[0].out_s), (1.0, 7.0));
        assert_eq!((speakers[1].in_s, speakers[1].out_s), (3.4, 5.4));
        // Scene 2: MAYA, KAI, JUNE in first-speaking order.
        let (speakers2, _) = super::speakers(&scene_list[1]);
        let names: Vec<&str> = speakers2.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["MAYA", "KAI", "JUNE"]);
    }

    #[test]
    fn cast_blocking_follows_the_rules() {
        assert_eq!(cast_positions(1)[0].0, [0.0, 0.0, 0.0]);
        let two = cast_positions(2);
        let d = (two[1].0[0] - two[0].0[0]).abs();
        assert!((d - 1.4).abs() < 1e-9);
        let four = cast_positions(4);
        assert_eq!(four.len(), 4);
        for (p, _) in &four {
            let r = (p[0] * p[0] + p[2] * p[2]).sqrt();
            assert!((r - 1.8).abs() < 1e-9, "arc radius: {r}");
            assert!(p[2] <= 0.0, "the arc bows away from the open side");
        }
    }

    #[test]
    fn shot_suffixes() {
        assert_eq!(shot_suffix(0), "A");
        assert_eq!(shot_suffix(25), "Z");
        assert_eq!(shot_suffix(26), "AA");
        assert_eq!(shot_suffix(27), "AB");
    }
}
