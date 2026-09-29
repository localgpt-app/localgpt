//! The authored record of every scene entity.
//!
//! Gen's scene is built from world-format records ([`wt::WorldEntity`]), and
//! [`Authored`] keeps that record on the entity. It is the source of truth:
//! saving, undo snapshots and exports read it rather than rebuilding the
//! entity from Bevy components, which can't hold everything the format does
//! (a material's extra fields, an emitter's kind, triggers, instances, a
//! mesh's hash and node overrides) and round-trip numbers through floats.
//!
//! Tools that change an entity update its record as they change the scene.
//! A tool that changes the scene without updating it is caught by
//! [`reconcile`]: where the live scene disagrees with the record, the live
//! value is taken and logged, so nothing a tool did is lost.

use bevy::prelude::*;
use localgpt_world_types as wt;
use std::collections::HashMap;

/// An entity's world-format record.
#[derive(Component, Debug, Clone)]
pub struct Authored {
    /// The record, as authored. Its `id`, `name` and `parent` follow the
    /// scene (the registry and hierarchy own those).
    pub entity: wt::WorldEntity,
    /// The entity is a part expanded under an instance: saving folds it
    /// back into the instance's overrides instead of writing it.
    pub part_of: Option<wt::PartLink>,
}

impl Authored {
    pub fn new(entity: wt::WorldEntity) -> Self {
        Self {
            entity,
            part_of: None,
        }
    }

    /// Record a trigger. A second trigger for the same event replaces the
    /// first, as `gen_add_trigger` always has.
    pub fn add_trigger(&mut self, trigger: wt::TriggerDef) {
        let kind = std::mem::discriminant(&trigger.on);
        self.entity
            .triggers
            .retain(|t| std::mem::discriminant(&t.on) != kind);
        self.entity.triggers.push(trigger);
    }
}

/// The record to save for an entity: its authored record, except where the
/// live scene (`live`, rebuilt from Bevy components) disagrees with it —
/// there a tool changed the scene without updating the record, and the live
/// value wins. `animated` entities (behaviors, modulation, triggers) move at
/// run time, so their live transform is not an edit and the authored one is
/// kept. Identity and hierarchy always come from the scene.
///
/// Returns the record and the fields taken from the scene.
pub fn reconcile(
    authored: &wt::WorldEntity,
    live: &wt::WorldEntity,
    animated: bool,
) -> (wt::WorldEntity, Vec<&'static str>) {
    let mut out = authored.clone();
    let mut drifted = Vec::new();
    out.id = live.id;
    out.name = live.name.clone();
    out.parent = live.parent;

    if !animated && !wt::values_close(&authored.transform, &live.transform) {
        out.transform = live.transform.clone();
        drifted.push("transform");
    }
    if !wt::values_close(&authored.shape, &live.shape) {
        out.shape = live.shape.clone();
        drifted.push("shape");
    }
    if material_core(authored.material.as_ref()) != material_core(live.material.as_ref()) {
        out.material = live.material.clone();
        drifted.push("material");
    }
    if !light_matches(authored.light.as_ref(), live.light.as_ref()) {
        out.light = live.light.clone();
        drifted.push("light");
    }
    if !wt::values_close(&authored.behaviors, &live.behaviors) {
        out.behaviors = live.behaviors.clone();
        drifted.push("behaviors");
    }
    if !audio_matches(authored.audio.as_ref(), live.audio.as_ref()) {
        out.audio = live.audio.clone();
        drifted.push("audio");
    }
    if authored.mesh_asset.as_ref().map(|m| &m.path) != live.mesh_asset.as_ref().map(|m| &m.path) {
        out.mesh_asset = live.mesh_asset.clone();
        drifted.push("mesh_asset");
    }
    if !wt::values_close(&authored.modulations, &live.modulations) {
        out.modulations = live.modulations.clone();
        drifted.push("modulations");
    }
    (out, drifted)
}

/// The material fields a Bevy `StandardMaterial` holds, normalized, so an
/// authored material and one rebuilt from the scene compare equal when
/// nothing changed (texture paths and the rest stay authored).
fn material_core(m: Option<&wt::MaterialDef>) -> Option<serde_json::Value> {
    let m = m?;
    let alpha = match m.alpha_mode.unwrap_or(wt::AlphaModeDef::Opaque) {
        wt::AlphaModeDef::Opaque => serde_json::json!("opaque"),
        other => serde_json::to_value(other).ok()?,
    };
    let round = |v: f32| (v * 1e4).round() / 1e4;
    let arr = |a: [f32; 4]| a.map(round);
    Some(serde_json::json!({
        "color": arr(m.color),
        "metallic": round(m.metallic),
        "roughness": round(m.roughness),
        "emissive": arr(m.emissive),
        "alpha": alpha,
        "unlit": m.unlit.unwrap_or(false),
        "double_sided": m.double_sided.unwrap_or(false),
        "reflectance": round(m.reflectance.unwrap_or(0.5)),
    }))
}

/// Lights match when what the scene holds matches; optional fields the
/// author left to the renderer's default (direction, range, cone angles)
/// are only compared when authored.
fn light_matches(a: Option<&wt::LightDef>, b: Option<&wt::LightDef>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            let opt = |x: Option<f32>, y: Option<f32>| x.is_none() || wt::values_close(&x, &y);
            a.light_type == b.light_type
                && wt::values_close(&a.color, &b.color)
                && wt::values_close(&a.intensity, &b.intensity)
                && a.shadows == b.shadows
                && opt(a.range, b.range)
                && opt(a.outer_angle, b.outer_angle)
                && opt(a.inner_angle, b.inner_angle)
                && (a.direction.is_none() || wt::values_close(&a.direction, &b.direction))
        }
        _ => false,
    }
}

/// Audio matches on what the scene holds (source and volume); kind, radius
/// and rolloff stay authored.
fn audio_matches(a: Option<&wt::AudioDef>, b: Option<&wt::AudioDef>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            wt::values_close(&a.source, &b.source) && wt::values_close(&a.volume, &b.volume)
        }
        _ => false,
    }
}

/// SHA-256 of a file's bytes, lowercase hex (`MeshAssetRef::sha256`).
pub fn file_sha256(path: &std::path::Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).ok()?;
    Some(hex::encode(Sha256::digest(&bytes)))
}

/// The reusable creations of the world in the scene (`WorldManifest::creations`).
#[derive(Resource, Debug, Clone, Default)]
pub struct CreationLibrary {
    pub creations: Vec<wt::CreationDef>,
}

impl CreationLibrary {
    pub fn get(&self, id: wt::CreationId) -> Option<&wt::CreationDef> {
        self.creations.iter().find(|c| c.id == id)
    }

    pub fn by_name(&self, name: &str) -> Option<&wt::CreationDef> {
        self.creations.iter().find(|c| c.name == name)
    }

    /// An id no creation uses yet.
    pub fn next_id(&self) -> wt::CreationId {
        wt::CreationId(self.creations.iter().map(|c| c.id.0).max().unwrap_or(0) + 1)
    }

    /// Add a world's creations. A cleared scene takes them as they are; a
    /// world merged into the scene keeps the current ones and adds any
    /// whose id is new (a clash keeps the current definition).
    pub fn load(&mut self, creations: &[wt::CreationDef], clear: bool) {
        if clear {
            self.creations = creations.to_vec();
            return;
        }
        for c in creations {
            if self.get(c.id).is_none() {
                self.creations.push(c.clone());
            }
        }
    }

    /// Fold the saved parts of each instance back into its overrides: the
    /// patch from the definition's part to what the part is now, and a
    /// removal for a part no longer in the scene. `entities`
    /// are the saved instances (and everything else); `parts` maps an
    /// instance id to its saved parts by part name.
    pub fn fold_parts(
        &self,
        entities: &mut [wt::WorldEntity],
        parts: &HashMap<wt::EntityId, Vec<(wt::EntityName, wt::WorldEntity)>>,
    ) {
        for entity in entities.iter_mut() {
            let Some(instance) = entity.instance_of.as_mut() else {
                continue;
            };
            let Some(def) = self.get(instance.creation) else {
                continue;
            };
            instance.overrides.clear();
            let saved = parts.get(&entity.id).map(Vec::as_slice).unwrap_or_default();
            let present = |name: &wt::EntityName| saved.iter().any(|(n, _)| n == name);
            for part in &def.parts {
                let Some((_, now)) = saved.iter().find(|(name, _)| *name == part.name) else {
                    // Deleted from this instance: removed, unless its parent
                    // is gone too (removing the parent removes it).
                    let parent_gone = part.parent.is_some_and(|p| {
                        def.parts
                            .iter()
                            .find(|q| q.id == p)
                            .is_some_and(|q| !present(&q.name))
                    });
                    if !parent_gone {
                        instance.overrides.push(wt::PartOverride {
                            part: part.name.clone(),
                            patch: wt::EntityPatch::default(),
                            removed: true,
                        });
                    }
                    continue;
                };
                let patch = wt::EntityPatch::between(part, now);
                if !patch.is_empty() {
                    instance.overrides.push(wt::PartOverride {
                        part: part.name.clone(),
                        patch,
                        removed: false,
                    });
                }
            }
        }
    }
}

/// A reusable creation made from snapshots of the entities it consists of:
/// the named ones first, then everything under them. Parts get ids `1..`
/// in that order and keep their parents among themselves; top-level parts
/// are placed relative to the first one's position, which is returned as
/// where the first instance goes.
pub fn creation_from(
    name: &str,
    id: wt::CreationId,
    category: Option<&str>,
    members: &[wt::WorldEntity],
) -> (wt::CreationDef, [f32; 3]) {
    let origin = members
        .first()
        .map(|m| m.transform.position)
        .unwrap_or_default();
    let ids: HashMap<wt::EntityId, wt::EntityId> = members
        .iter()
        .enumerate()
        .map(|(i, m)| (m.id, wt::EntityId(i as u64 + 1)))
        .collect();
    let mut half = [0.0f32; 3];
    let parts = members
        .iter()
        .map(|m| {
            let mut part = m.clone();
            part.id = ids[&m.id];
            part.parent = m.parent.and_then(|p| ids.get(&p).copied());
            if part.parent.is_none() {
                for (k, o) in origin.iter().enumerate() {
                    part.transform.position[k] -= o;
                }
                if let Some(shape) = &part.shape {
                    let h = shape.local_aabb_half();
                    for (k, extent) in half.iter_mut().enumerate() {
                        *extent = extent.max(
                            part.transform.position[k].abs() + h[k] * part.transform.scale[k].abs(),
                        );
                    }
                }
            }
            part.instance_of = None;
            part.creation_id = None;
            part.chunk = None;
            part
        })
        .collect();
    let semantic_category = category.map(|c| {
        serde_json::from_value(serde_json::Value::String(c.to_string()))
            .unwrap_or_else(|_| wt::SemanticCategory::Custom(c.to_string()))
    });
    (
        wt::CreationDef {
            id,
            name: name.to_string(),
            semantic_category,
            bbox_half: half,
            entities: Vec::new(),
            parts,
        },
        origin,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> wt::WorldEntity {
        let mut e = wt::WorldEntity::new(4, "lamp")
            .with_shape(wt::Shape::Sphere { radius: 0.5 })
            .at([1.0, 2.0, 3.0]);
        e.transform.rotation_degrees = [0.0, 90.0, 0.0];
        e.material = Some(wt::MaterialDef {
            color: [0.9, 0.8, 0.2, 1.0],
            base_color_texture: Some("textures/brass.png".into()),
            ..Default::default()
        });
        e.audio = Some(wt::AudioDef {
            kind: wt::AudioKind::Ambient,
            source: wt::AudioSource::Hum {
                frequency: 60.0,
                warmth: 0.3,
            },
            volume: 0.5,
            radius: Some(8.0),
            rolloff: wt::Rolloff::Linear,
        });
        e
    }

    /// What Gen rebuilds from the scene: float noise, no texture paths, and
    /// an emitter that forgets its kind and rolloff.
    fn rebuilt(r: &wt::WorldEntity) -> wt::WorldEntity {
        let mut live = r.clone();
        live.transform.rotation_degrees = [0.0, 89.999_99, 0.0];
        live.transform.position[0] += 1e-6;
        let m = live.material.as_mut().unwrap();
        m.base_color_texture = None;
        m.color[0] = 0.900_000_1;
        let a = live.audio.as_mut().unwrap();
        a.kind = wt::AudioKind::Sfx;
        a.rolloff = wt::Rolloff::InverseSquare;
        live
    }

    #[test]
    fn reconcile_keeps_the_record_where_the_scene_agrees() {
        let record = record();
        let (out, drifted) = reconcile(&record, &rebuilt(&record), false);
        assert!(drifted.is_empty(), "{drifted:?}");
        assert_eq!(
            out, record,
            "exact numbers, texture path, emitter kind all kept"
        );
    }

    #[test]
    fn reconcile_takes_what_a_tool_changed_behind_the_record() {
        let record = record();
        let mut live = rebuilt(&record);
        live.transform.position = [5.0, 0.0, 0.0];
        live.material.as_mut().unwrap().roughness = 0.1;
        live.parent = Some(wt::EntityId(9));
        let (out, drifted) = reconcile(&record, &live, false);
        assert_eq!(drifted, ["transform", "material"]);
        assert_eq!(out.transform.position, [5.0, 0.0, 0.0]);
        assert_eq!(out.material.as_ref().unwrap().roughness, 0.1);
        assert_eq!(
            out.parent,
            Some(wt::EntityId(9)),
            "hierarchy follows the scene"
        );
        assert_eq!(out.audio, record.audio);

        // A spinning entity's pose is not an edit.
        let (out, drifted) = reconcile(&record, &live, true);
        assert_eq!(drifted, ["material"]);
        assert_eq!(out.transform, record.transform);
    }

    #[test]
    fn creation_from_members_is_relative_to_the_first() {
        let trunk = wt::WorldEntity::new(40, "trunk")
            .with_shape(wt::Shape::Cylinder {
                radius: 0.3,
                height: 3.0,
            })
            .at([10.0, 1.5, -2.0]);
        let mut leaves = wt::WorldEntity::new(41, "leaves")
            .with_shape(wt::Shape::Sphere { radius: 1.5 })
            .at([0.0, 2.0, 0.0]);
        leaves.parent = Some(wt::EntityId(40));
        let rock = wt::WorldEntity::new(7, "rock")
            .with_shape(wt::Shape::Sphere { radius: 0.5 })
            .at([11.0, 0.0, -2.0]);
        let (def, origin) = creation_from(
            "oak_tree",
            wt::CreationId(1),
            Some("vegetation"),
            &[trunk, leaves, rock],
        );
        assert_eq!(origin, [10.0, 1.5, -2.0]);
        assert_eq!(
            def.semantic_category,
            Some(wt::SemanticCategory::Vegetation)
        );
        let ids: Vec<u64> = def.parts.iter().map(|p| p.id.0).collect();
        assert_eq!(ids, [1, 2, 3]);
        assert_eq!(def.parts[0].transform.position, [0.0, 0.0, 0.0]);
        // A child keeps its local transform and its parent, renumbered.
        assert_eq!(def.parts[1].parent, Some(wt::EntityId(1)));
        assert_eq!(def.parts[1].transform.position, [0.0, 2.0, 0.0]);
        assert_eq!(def.parts[2].transform.position, [1.0, -1.5, 0.0]);
        assert_eq!(def.bbox_half, [1.5, 2.0, 0.5]);

        let (custom, _) = creation_from("x", wt::CreationId(2), Some("lamp_post"), &[]);
        assert_eq!(
            custom.semantic_category,
            Some(wt::SemanticCategory::Custom("lamp_post".into()))
        );
    }

    fn def() -> wt::CreationDef {
        wt::CreationDef {
            id: wt::CreationId(3),
            name: "lamp".into(),
            semantic_category: None,
            bbox_half: [0.5, 1.0, 0.5],
            entities: Vec::new(),
            parts: vec![
                wt::WorldEntity::new(1, "post").with_shape(wt::Shape::Cylinder {
                    radius: 0.1,
                    height: 2.0,
                }),
                wt::WorldEntity::new(2, "bulb").with_shape(wt::Shape::Sphere { radius: 0.2 }),
            ],
        }
    }

    #[test]
    fn save_folds_changed_parts_into_overrides() {
        let library = CreationLibrary {
            creations: vec![def()],
        };
        let mut root = wt::WorldEntity::new(10, "lamp_1");
        root.instance_of = Some(wt::InstanceOf {
            creation: wt::CreationId(3),
            overrides: Vec::new(),
        });
        let expanded = wt::expand_instances(&[root.clone()], &library.creations, 20);
        // The post is unchanged; the bulb grew.
        let post = expanded[1].clone();
        let mut bulb = expanded[2].clone();
        bulb.shape = Some(wt::Shape::Sphere { radius: 0.4 });
        let mut parts = HashMap::new();
        parts.insert(
            wt::EntityId(10),
            vec![
                (wt::EntityName::new("post"), post),
                (wt::EntityName::new("bulb"), bulb),
            ],
        );
        let mut saved = vec![root];
        library.fold_parts(&mut saved, &parts);
        let overrides = &saved[0].instance_of.as_ref().unwrap().overrides;
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].part.as_str(), "bulb");
        assert_eq!(
            overrides[0].patch.shape,
            Some(Some(wt::Shape::Sphere { radius: 0.4 }))
        );

        // Expanding the saved instance gives the edited bulb back.
        let again = wt::expand_instances(&saved, &library.creations, 20);
        assert_eq!(again[2].shape, Some(wt::Shape::Sphere { radius: 0.4 }));

        // Deleting the post from the scene saves it as removed.
        parts.get_mut(&wt::EntityId(10)).unwrap().remove(0);
        library.fold_parts(&mut saved, &parts);
        let overrides = &saved[0].instance_of.as_ref().unwrap().overrides;
        assert!(
            overrides
                .iter()
                .any(|o| o.part.as_str() == "post" && o.removed)
        );
        let again = wt::expand_instances(&saved, &library.creations, 20);
        let names: Vec<&str> = again.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["lamp_1", "lamp_1/bulb"]);
    }

    #[test]
    fn a_second_trigger_of_a_kind_replaces_the_first() {
        let mut extras = Authored::new(wt::WorldEntity::new(1, "sign"));
        let click = |text: &str| wt::TriggerDef {
            on: wt::TriggerEvent::Click {
                max_distance: 5.0,
                prompt: None,
            },
            action: wt::TriggerActionDef::ShowText {
                text: text.into(),
                seconds: 4.0,
            },
            once: false,
            cooldown: None,
            requires_item: None,
        };
        extras.add_trigger(click("a"));
        extras.add_trigger(click("b"));
        assert_eq!(extras.entity.triggers.len(), 1);
        assert_eq!(
            extras.entity.triggers[0].action,
            wt::TriggerActionDef::ShowText {
                text: "b".into(),
                seconds: 4.0
            }
        );
    }
}
