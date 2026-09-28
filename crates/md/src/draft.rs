//! The rule-based draft: compile a [`Doc`] into a `WorldManifest` instantly
//! and deterministically — one region per section along a winding path, and
//! a tour with one stop per section.
//!
//! This is the world you see before (or without) a local LLM. Authored
//! content layers on top per section (PLAN.md M3): a ```` ```world ```` fence's
//! exact entities, then the sidecar's cached agent build, then the cached
//! recipe — the draft is always the fallback, the same split as Verse's
//! rule-derived world. Authored entities replace the landmark/props/lamp;
//! ground, platform, and tour stops are always the draft's own.
//!
//! Renderer-free (no Bevy but `log`). Entity ids are stable per section
//! (`(index + 1) * 1000 + n`), so editing one section leaves every other
//! region identical.

use localgpt_world_types as wt;

use crate::doc::{Doc, Section};
use crate::recipe::{LandmarkKind, LandmarkSpec, PropKind, PropSpec, RegionRecipe};
use crate::sidecar::RecipeStore;

/// Distance between consecutive regions along the path.
const SPACING: f32 = 22.0;
/// Slide spacing on a deck's straight presentation path — a little tighter
/// than the world's winding stroll.
const DECK_SPACING: f32 = 16.0;
/// Radius of each region's platform.
const PLATFORM_RADIUS: f32 = 6.0;
/// Height of a platform's top surface (the ground is at y = 0).
const PLATFORM_TOP: f32 = 0.5;
/// Width of the ground strip under the path — wide enough for fog to hide
/// its edges, well inside Gen's 200-unit half-extent limit.
const GROUND_WIDTH: f32 = 300.0;
/// Where the camera stands relative to a region's centre.
const VIEW_OFFSET: [f32; 3] = [0.0, 4.0, 14.0];
/// Evening-blue sky, reused as the fog colour so the horizon dissolves.
const SKY: [f32; 4] = [0.46, 0.56, 0.72, 1.0];
const GROUND: [f32; 4] = [0.26, 0.31, 0.29, 1.0];

/// Compile a document into a world manifest LocalGPT Gen can load, with no
/// recipes — the pure rule-based draft.
///
/// The bin always compiles with the sidecar store (even when it's empty);
/// this no-recipe entry point is for tests and future callers.
#[allow(dead_code)]
pub fn compile(doc: &Doc) -> wt::WorldManifest {
    compile_with(doc, &RecipeStore::in_memory())
}

/// Compile with authored content applied on top of the draft. Per section,
/// the first of these wins (PLAN.md M3): the ```` ```world ```` fence's
/// exact entities, the sidecar's cached agent build, the cached recipe's
/// restyling — else the pure rule-derived region. Authored entities replace
/// the landmark/props/lamp; the ground strip, platform, and tour stop are
/// always the draft's own.
pub fn compile_with(doc: &Doc, store: &RecipeStore) -> wt::WorldManifest {
    let mut entities = vec![lead_in_ground(), sun()];
    let mut waypoints = Vec::with_capacity(doc.sections.len());
    // The highest tier that authored anything, for `meta.source`.
    let mut authored: Option<&'static str> = None;
    let deck = doc.is_deck();
    for (index, section) in doc.sections.iter().enumerate() {
        let center = region_center(index, deck);
        // Priority: exact fence override → cached agent build → recipe.
        let override_entities = section.world.as_deref().and_then(parse_world_fence);
        let has_override = override_entities.is_some();
        let build = store.get_build(&section.hash);
        let content = override_entities.or_else(|| build.map(|b| b.entities.clone()));
        let mut ids = RegionIds::new(index);
        if let Some(locals) = content {
            entities.extend(region_base(&mut ids, center, section, None));
            entities.extend(placed_entities(&mut ids, center, &locals));
            authored = Some(if has_override {
                "world override"
            } else {
                "llm build"
            });
        } else {
            let recipe = store.get(&section.hash);
            if recipe.is_some() {
                authored = Some("llm recipe");
            }
            entities.extend(region(section, center, recipe, &mut ids));
        }
        waypoints.push(wt::TourWaypoint {
            position: add(center, VIEW_OFFSET),
            look_at: [center[0], 2.0, center[2]],
            description: Some(section.heading.clone()),
            pause_duration: 0.0,
        });
    }

    let next_entity_id = entities.iter().map(|e| e.id.0).max().unwrap_or(0) + 1;
    let camera = waypoints.first().map(|stop| wt::CameraDef {
        position: stop.position,
        look_at: stop.look_at,
        fov_degrees: 50.0,
    });
    let tours = if waypoints.is_empty() {
        Vec::new()
    } else {
        vec![wt::TourDef {
            name: "sections".into(),
            description: Some("One stop per section, in document order".into()),
            waypoints,
            speed: 6.0,
            mode: wt::TourMode::Fly,
            autostart: false,
            loop_tour: false,
            pov: None,
        }]
    };

    wt::WorldManifest {
        version: wt::world::WORLD_SCHEMA_VERSION,
        meta: meta(doc, authored),
        environment: Some(wt::EnvironmentDef {
            background_color: Some(SKY),
            ambient_intensity: Some(350.0),
            ambient_color: Some([0.85, 0.88, 1.0, 1.0]),
            fog_density: Some(0.012),
            fog_color: Some(SKY),
        }),
        camera,
        avatar: None,
        tours,
        soundtrack: None,
        layout_file: None,
        region_files: None,
        behavior_files: None,
        audio_files: None,
        avatar_file: None,
        entities,
        creations: Vec::new(),
        next_entity_id,
    }
}

/// Gen's own save-time checks, so a draft is always loadable by Gen.
pub fn validate(manifest: &wt::WorldManifest) -> Vec<wt::ValidationIssue> {
    wt::validation::validate_entities(&manifest.entities, &wt::WorldLimits::default())
}

fn meta(doc: &Doc, authored: Option<&'static str>) -> wt::WorldMeta {
    wt::WorldMeta {
        name: doc.title.clone(),
        description: doc.intro.lines().next().map(str::to_string),
        biome: None,
        time_of_day: None,
        tags: Some(vec!["localgpt-md".into(), doc.genre().to_string()]),
        source: Some(match authored {
            Some(kind) => format!("localgpt-md draft + {kind}"),
            None => "localgpt-md draft".into(),
        }),
        variation_group: None,
        variation: None,
        prompt: None,
        model: None,
        generation_duration_ms: None,
        style_ref: None,
        bevy_version: Some("0.19".into()),
        compliance: None,
    }
}

/// Where a section's region sits. The `world` genre winds gently left and
/// right as the path heads away (towards −z); a `deck` lays its slides out
/// along one straight presentation path.
fn region_center(index: usize, deck: bool) -> [f32; 3] {
    let t = index as f32;
    if deck {
        [0.0, 0.0, -t * DECK_SPACING]
    } else {
        [(t * 0.9).sin() * 8.0, 0.0, -t * SPACING]
    }
}

/// Ground under the first viewpoint, before region 1's own strip begins.
fn lead_in_ground() -> wt::WorldEntity {
    let mut ground = wt::WorldEntity::new(1, "ground-lead-in");
    ground.transform.position = [0.0, 0.0, SPACING];
    ground.shape = Some(wt::Shape::Plane {
        x: GROUND_WIDTH,
        z: SPACING,
    });
    ground.material = Some(material(GROUND, 0.95, [0.0; 4]));
    ground
}

fn sun() -> wt::WorldEntity {
    let mut sun = wt::WorldEntity::new(2, "sun");
    sun.transform.position = [0.0, 30.0, 0.0];
    sun.light = Some(wt::LightDef {
        light_type: wt::LightType::Directional,
        color: [1.0, 0.94, 0.86, 1.0],
        intensity: 9000.0,
        direction: Some([-0.4, -1.0, -0.35]),
        shadows: true,
        ..Default::default()
    });
    sun
}

/// Id/name allocation for one section's region band: ids stay inside the
/// section's `1000·n` band (n ≥ 1; ids 1–999 are global), so unchanged
/// sections stay identical across edits — what the per-section cache and
/// the editing tests rely on.
struct RegionIds {
    base_id: u64,
    prefix: String,
    next: u64,
}

impl RegionIds {
    fn new(index: usize) -> Self {
        Self {
            base_id: (index as u64 + 1) * 1000,
            prefix: format!("s{:02}", index + 1),
            next: 0,
        }
    }

    fn entity(&mut self, name: &str, position: [f32; 3]) -> wt::WorldEntity {
        self.next += 1;
        let mut e =
            wt::WorldEntity::new(self.base_id + self.next, format!("{}-{name}", self.prefix));
        e.transform.position = position;
        e
    }
}

/// Parse a ```` ```world ```` fence: a JSON array of world entities in
/// platform-local coordinates (see `doc::Section::world`). Entities may omit
/// `id` — it's a placeholder anyway, renumbered into the section's band on
/// placement — so hand-written fences stay ergonomic. A malformed fence
/// returns `None` (the section falls back to the next tier) with a warning —
/// the document still opens.
fn parse_world_fence(text: &str) -> Option<Vec<wt::WorldEntity>> {
    let parse =
        |err: serde_json::Error| tracing::warn!("world fence ignored — can't parse ({err})");
    let mut items = serde_json::from_str::<Vec<serde_json::Value>>(text.trim())
        .map_err(parse)
        .ok()?;
    for (index, item) in items.iter_mut().enumerate() {
        if let Some(object) = item.as_object_mut() {
            object
                .entry("id")
                .or_insert_with(|| serde_json::json!((index + 1) as u64));
        }
    }
    serde_json::from_value::<Vec<wt::WorldEntity>>(serde_json::Value::Array(items))
        .map_err(parse)
        .ok()
}

/// Place authored entities (an agent build or a fence override) onto the
/// section's platform: local coordinates → region-centred (local y = 0 is
/// the platform surface), ids renumbered into the band, names prefixed.
/// The same position clamp the agent interpreter applies holds here, so a
/// hand-edited sidecar or fence can't escape the platform neighbourhood.
fn placed_entities(
    ids: &mut RegionIds,
    center: [f32; 3],
    locals: &[wt::WorldEntity],
) -> Vec<wt::WorldEntity> {
    let [cx, _, cz] = center;
    locals
        .iter()
        .map(|local| {
            let mut e = local.clone();
            e.id = wt::EntityId(ids.base_id + ids.next + 1);
            ids.next += 1;
            e.name = wt::EntityName(format!("{}-{}", ids.prefix, local.name.0));
            // Clamp in LOCAL space first, then translate — clamping after the
            // offset would drag distant sections' entities back toward the
            // origin.
            let l = crate::agent::clamp_local(local.transform.position);
            e.transform.position = [cx + l[0], PLATFORM_TOP + l[1], cz + l[2]];
            e
        })
        .collect()
}

/// The structural part of every region: the ground strip and the platform.
/// `ground_color` (a recipe tint) applies when given.
fn region_base(
    ids: &mut RegionIds,
    center: [f32; 3],
    section: &Section,
    ground_color: Option<[f32; 4]>,
) -> Vec<wt::WorldEntity> {
    let [cx, _, cz] = center;
    let hue = (section.seed() % 360) as f32;
    let mut out = Vec::new();

    let mut ground = ids.entity("ground", [0.0, 0.0, cz]);
    ground.shape = Some(wt::Shape::Plane {
        x: GROUND_WIDTH,
        z: SPACING,
    });
    ground.material = Some(material(ground_color.unwrap_or(GROUND), 0.95, [0.0; 4]));
    out.push(ground);

    let mut platform = ids.entity("platform", [cx, PLATFORM_TOP / 2.0, cz]);
    platform.shape = Some(wt::Shape::Cylinder {
        radius: PLATFORM_RADIUS,
        height: PLATFORM_TOP,
    });
    platform.material = Some(material(hsl(hue, 0.12, 0.72), 0.9, [0.0; 4]));
    out.push(platform);
    out
}

/// A section's rule-derived region: base (ground + platform) plus a
/// landmark, a ring of props, and a lamp. Size comes from the amount of
/// prose; colour, shape, and arrangement from the section hash — unless a
/// recipe (PLAN.md M1) overrides the palette, landmark, or props.
fn region(
    section: &Section,
    center: [f32; 3],
    recipe: Option<&RegionRecipe>,
    ids: &mut RegionIds,
) -> Vec<wt::WorldEntity> {
    let seed = section.seed();
    let words = section.body.split_whitespace().count();
    let hue = (seed % 360) as f32;
    let accent = recipe
        .and_then(|r| r.accent)
        .map(|[r, g, b]| [r, g, b, 1.0])
        .unwrap_or_else(|| hsl(hue, 0.55, 0.58));
    let ground_color = recipe
        .and_then(|r| r.ground)
        .map(|[r, g, b]| [r, g, b, 1.0]);
    let [cx, _, cz] = center;

    let mut out = region_base(ids, center, section, ground_color);

    // Longer sections get taller landmarks. A recipe can override the kind,
    // the scale, and the glow; the height's base stays rule-derived.
    let base_height = 2.5 + (words as f32 / 20.0).min(7.0);
    let LandmarkSpec {
        kind: landmark_kind,
        scale,
        emissive,
    } = recipe.and_then(|r| r.landmark).unwrap_or(LandmarkSpec {
        kind: match (seed >> 16) % 6 {
            0 => LandmarkKind::Pyramid,
            1 => LandmarkKind::Cone,
            2 => LandmarkKind::Column,
            3 => LandmarkKind::Cube,
            4 => LandmarkKind::Orb,
            _ => LandmarkKind::Ring,
        },
        scale: 1.0,
        emissive: 0.6,
    });
    let height = base_height * scale;
    let (shape, y, rotation_degrees) = match landmark_kind {
        LandmarkKind::Pyramid => (
            wt::Shape::Pyramid {
                base_x: 3.2,
                base_z: 3.2,
                height,
            },
            PLATFORM_TOP + height / 2.0,
            [0.0; 3],
        ),
        LandmarkKind::Cone => (
            wt::Shape::Cone {
                radius: 1.6,
                height,
            },
            PLATFORM_TOP + height / 2.0,
            [0.0; 3],
        ),
        LandmarkKind::Column => (
            wt::Shape::Cylinder {
                radius: 0.8,
                height,
            },
            PLATFORM_TOP + height / 2.0,
            [0.0; 3],
        ),
        LandmarkKind::Cube => (
            wt::Shape::Cuboid {
                x: 1.8,
                y: height,
                z: 1.8,
            },
            PLATFORM_TOP + height / 2.0,
            [0.0, 45.0, 0.0],
        ),
        // A floating orb.
        LandmarkKind::Orb => (
            wt::Shape::Sphere {
                radius: height * 0.3,
            },
            PLATFORM_TOP + height * 0.3 + 1.0,
            [0.0; 3],
        ),
        // An upright ring, facing the viewpoint.
        LandmarkKind::Ring => (
            wt::Shape::Torus {
                major_radius: height * 0.35,
                minor_radius: 0.22,
            },
            PLATFORM_TOP + height * 0.35 + 0.22,
            [90.0, 0.0, 0.0],
        ),
    };
    let mut landmark = ids.entity("landmark", [cx, y, cz]);
    landmark.transform.rotation_degrees = rotation_degrees;
    landmark.shape = Some(shape);
    landmark.material = Some(material(accent, 0.35, glow(accent, emissive)));
    out.push(landmark);

    let prop_spec: Option<PropSpec> = recipe.and_then(|r| r.props);
    let props = prop_spec.map_or(3 + ((seed >> 24) % 5) as usize + (words / 30).min(4), |p| {
        p.count as usize
    });
    let offset = ((seed >> 32) % 360) as f32;
    let prop_color = hsl(hue + 180.0, 0.35, 0.7);
    for k in 0..props {
        let angle = (offset + k as f32 * 360.0 / props as f32).to_radians();
        let r = PLATFORM_RADIUS - 1.1;
        let size = 0.35 + ((seed >> (k % 8 * 4)) & 0xF) as f32 / 40.0;
        let (shape, half_height) = match prop_spec.map(|p| p.kind) {
            Some(PropKind::Spheres) => (wt::Shape::Sphere { radius: size * 0.8 }, size * 0.8),
            Some(PropKind::Crystals) => {
                let crystal = size * 1.8;
                (
                    wt::Shape::Cone {
                        radius: size * 0.45,
                        height: crystal,
                    },
                    crystal / 2.0,
                )
            }
            // No recipe for the kind: the rule path mixes blocks and spheres
            // by parity, as it always did.
            _ if prop_spec.is_none() && (seed >> k) & 1 != 0 => {
                (wt::Shape::Sphere { radius: size * 0.8 }, size * 0.8)
            }
            _ => {
                let side = size * 1.4;
                (
                    wt::Shape::Cuboid {
                        x: side,
                        y: side,
                        z: side,
                    },
                    side / 2.0,
                )
            }
        };
        let position = [
            cx + r * angle.cos(),
            PLATFORM_TOP + half_height,
            cz + r * angle.sin(),
        ];
        let mut prop = ids.entity(&format!("prop-{:02}", k + 1), position);
        prop.transform.rotation_degrees = [0.0, (k as f32 * 37.0) % 90.0, 0.0];
        prop.shape = Some(shape);
        prop.material = Some(material(prop_color, 0.8, [0.0; 4]));
        out.push(prop);
    }

    let mut lamp = ids.entity("lamp", [cx, PLATFORM_TOP + height + 2.0, cz + 2.0]);
    lamp.light = Some(wt::LightDef {
        light_type: wt::LightType::Point,
        color: accent,
        intensity: 250_000.0,
        range: Some(20.0),
        shadows: false,
        ..Default::default()
    });
    out.push(lamp);

    out
}

fn material(color: [f32; 4], roughness: f32, emissive: [f32; 4]) -> wt::MaterialDef {
    wt::MaterialDef {
        color,
        roughness,
        emissive,
        metallic: 0.1,
        ..Default::default()
    }
}

/// Emissive is linear RGB in Gen's mapping; bloom picks up values near 1.
fn glow(color: [f32; 4], strength: f32) -> [f32; 4] {
    [
        color[0] * strength,
        color[1] * strength,
        color[2] * strength,
        1.0,
    ]
}

/// HSL (hue in degrees, saturation and lightness 0–1) → sRGB RGBA.
fn hsl(hue: f32, saturation: f32, lightness: f32) -> [f32; 4] {
    let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let h = hue.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = lightness - c / 2.0;
    [r + m, g + m, b + m, 1.0]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = include_str!("../samples/hello.md");

    fn region_of(world: &wt::WorldManifest, prefix: &str) -> Vec<wt::WorldEntity> {
        world
            .entities
            .iter()
            .filter(|e| e.name.0.starts_with(prefix))
            .cloned()
            .collect()
    }

    #[test]
    fn one_tour_stop_per_section_in_order() {
        let doc = Doc::parse(HELLO, "hello");
        let world = compile(&doc);
        let stops: Vec<_> = world.tours[0]
            .waypoints
            .iter()
            .map(|w| w.description.clone().unwrap())
            .collect();
        let headings: Vec<_> = doc.sections.iter().map(|s| s.heading.clone()).collect();
        assert_eq!(stops, headings);
        assert_eq!(
            world.camera.unwrap().position,
            world.tours[0].waypoints[0].position
        );
    }

    #[test]
    fn deterministic() {
        let doc = Doc::parse(HELLO, "hello");
        assert_eq!(compile(&doc), compile(&doc));
    }

    #[test]
    fn editing_one_section_leaves_other_regions_identical() {
        let before = compile(&Doc::parse(HELLO, "hello"));
        let edited = HELLO.replace("dusty", "dusty, cobwebbed");
        assert_ne!(edited, HELLO, "sample must mention 'dusty' in section 2");
        let after = compile(&Doc::parse(&edited, "hello"));
        assert_ne!(region_of(&before, "s02-"), region_of(&after, "s02-"));
        for prefix in ["s01-", "s03-", "s04-"] {
            assert_eq!(region_of(&before, prefix), region_of(&after, prefix));
        }
    }

    #[test]
    fn passes_gen_validation() {
        let issues = validate(&compile(&Doc::parse(HELLO, "hello")));
        let messages: Vec<_> = issues.iter().map(|i| &i.message).collect();
        assert!(messages.is_empty(), "{messages:?}");
    }

    #[test]
    fn ron_round_trip() {
        let world = compile(&Doc::parse(HELLO, "hello"));
        let text = ron::ser::to_string_pretty(&world, ron::ser::PrettyConfig::default()).unwrap();
        let back: wt::WorldManifest = ron::from_str(&text).unwrap();
        assert_eq!(back, world);
    }

    fn override_doc() -> Doc {
        // A fence carrying one cuboid, serialized with world-types' own serde
        // config — exactly what a hand-written fence must round-trip.
        let entity = wt::WorldEntity::new(0, "monolith")
            .with_shape(wt::Shape::Cuboid {
                x: 2.0,
                y: 9.0,
                z: 2.0,
            })
            .at([0.0, 4.5, 0.0]);
        let json = serde_json::to_string(&vec![entity]).unwrap();
        Doc::parse(&format!("## Over\n\nprose\n\n```world\n{json}\n```\n"), "t")
    }

    #[test]
    fn fence_override_replaces_the_region_contents() {
        let world = compile(&override_doc());
        let names: Vec<&str> = world.entities.iter().map(|e| e.name.0.as_str()).collect();
        assert!(names.contains(&"s01-ground") && names.contains(&"s01-platform"));
        assert!(names.contains(&"s01-monolith"));
        assert!(
            !names
                .iter()
                .any(|n| n.contains("landmark") || n.contains("prop-"))
        );
        // Local y = 4.5 lands on the platform surface.
        let monolith = world
            .entities
            .iter()
            .find(|e| e.name.0 == "s01-monolith")
            .unwrap();
        assert!((monolith.transform.position[1] - (PLATFORM_TOP + 4.5)).abs() < 1e-6);
        assert!(validate(&world).is_empty());
    }

    #[test]
    fn malformed_fence_falls_back_to_the_rules() {
        let doc = Doc::parse("## Bad\n\n```world\nnot json\n```\n", "t");
        let world = compile(&doc);
        assert!(world.entities.iter().any(|e| e.name.0 == "s01-landmark"));
    }

    #[test]
    fn cached_build_replaces_the_region_and_is_local() {
        let doc = Doc::parse(HELLO, "hello");
        let mut store = RecipeStore::in_memory();
        let locals: Vec<wt::WorldEntity> = vec![
            wt::WorldEntity::new(0, "gate").at([0.0, 0.0, -1.0]),
            wt::WorldEntity::new(0, "seat").at([2.0, 0.5, 1.0]),
        ];
        store.insert_build(
            &doc.sections[1].hash,
            crate::sidecar::BuildEntry {
                model: "m.gguf".into(),
                description: None,
                entities: locals,
            },
        );
        let with = compile_with(&doc, &store);
        let without = compile(&doc);

        // The built section has the prefixed entities in its id band…
        assert!(with.entities.iter().any(|e| e.name.0 == "s02-gate"));
        assert!(with.entities.iter().any(|e| e.name.0 == "s02-seat"));
        let gate = with
            .entities
            .iter()
            .find(|e| e.name.0 == "s02-gate")
            .unwrap();
        assert_eq!(gate.id.0, 2003); // base takes 2001-2002; first placed is 2003
        // …centered on the platform…
        let center = region_center(1, false);
        assert!((gate.transform.position[0] - center[0]).abs() < 1e-6);
        assert!((gate.transform.position[1] - (PLATFORM_TOP + 0.0)).abs() < 1e-6);
        // …every other region identical, and deterministic + valid.
        for prefix in ["s01-", "s03-", "s04-"] {
            assert_eq!(region_of(&with, prefix), region_of(&without, prefix));
        }
        assert_eq!(compile_with(&doc, &store), with);
        assert!(validate(&with).is_empty());
        assert!(
            with.meta
                .source
                .as_deref()
                .is_some_and(|s| s.contains("llm build"))
        );
    }

    #[test]
    fn empty_document_still_compiles() {
        let world = compile(&Doc::parse("", "empty"));
        assert!(world.tours.is_empty());
        assert!(world.camera.is_none());
        assert_eq!(world.entities.len(), 2); // lead-in ground + sun
    }

    #[test]
    fn deck_lays_out_a_straight_presentation_path() {
        let doc = Doc::parse(include_str!("../samples/deck.md"), "deck");
        assert!(doc.is_deck());
        let world = compile(&doc);
        assert!(validate(&world).is_empty());

        // One stop per slide, in deck order.
        let stops: Vec<_> = world.tours[0]
            .waypoints
            .iter()
            .map(|w| w.description.clone().unwrap())
            .collect();
        let titles: Vec<_> = doc.sections.iter().map(|s| s.heading.clone()).collect();
        assert_eq!(stops, titles);

        // Collinear on x = 0, evenly spaced, walking away from the viewer.
        for (index, stop) in world.tours[0].waypoints.iter().enumerate() {
            assert_eq!(stop.position[0], 0.0);
            assert!((stop.position[2] - (14.0 - index as f32 * DECK_SPACING)).abs() < 1e-6);
        }
    }

    #[test]
    fn recipe_restyling_is_deterministic_and_local() {
        let doc = Doc::parse(HELLO, "hello");
        let mut store = RecipeStore::in_memory();
        store.insert(
            &doc.sections[0].hash,
            RegionRecipe {
                accent: Some([0.9, 0.3, 0.1]),
                ground: Some([0.5, 0.4, 0.3]),
                landmark: Some(LandmarkSpec {
                    kind: LandmarkKind::Orb,
                    scale: 1.5,
                    emissive: 0.9,
                }),
                props: Some(PropSpec {
                    kind: PropKind::Crystals,
                    count: 10,
                }),
            },
        );
        let with = compile_with(&doc, &store);
        let without = compile(&doc);

        // The restyled region changed, and only it.
        assert_ne!(region_of(&with, "s01-"), region_of(&without, "s01-"));
        for prefix in ["s02-", "s03-", "s04-"] {
            assert_eq!(region_of(&with, prefix), region_of(&without, prefix));
        }

        // The recipe's prop count and landmark kind actually arrived.
        let props = with
            .entities
            .iter()
            .filter(|e| e.name.0.starts_with("s01-prop-"))
            .count();
        assert_eq!(props, 10);
        let is_sphere = matches!(
            with.entities
                .iter()
                .find(|e| e.name.0 == "s01-landmark")
                .unwrap()
                .shape,
            Some(wt::Shape::Sphere { .. })
        );
        assert!(is_sphere);

        // Deterministic with the store, and still valid for Gen.
        assert_eq!(compile_with(&doc, &store), with);
        assert!(validate(&with).is_empty());
    }
}
