//! What the world format carries that Gen's scene components don't: which
//! entities are instances of a reusable creation (and which are the parts
//! expanded under them), the triggers as the format writes them, and an
//! imported mesh's hash and node overrides. Loading puts a [`WorldExtras`]
//! on the entity; saving reads it back, so none of these are lost in a
//! load–save round trip.

use bevy::prelude::*;
use localgpt_world_types as wt;
use std::collections::HashMap;

/// Format data kept on a scene entity between load and save.
#[derive(Component, Debug, Clone, Default)]
pub struct WorldExtras {
    /// The entity places a copy of a reusable creation.
    pub instance_of: Option<wt::InstanceOf>,
    /// The entity is a part expanded under an instance; saving folds it
    /// back into the instance's overrides instead of writing it.
    pub part_of: Option<wt::PartLink>,
    /// Triggers in the format's form (the runtime components are separate).
    pub triggers: Vec<wt::TriggerDef>,
    /// `MeshAssetRef::sha256` of the entity's imported mesh.
    pub mesh_sha256: Option<String>,
    /// `MeshAssetRef::node_overrides` of the entity's imported mesh.
    pub node_overrides: Vec<wt::NodeOverride>,
}

impl WorldExtras {
    /// The extras a loaded entity carries (`None` when it carries none).
    pub fn of(we: &wt::WorldEntity, links: &HashMap<wt::EntityId, wt::PartLink>) -> Option<Self> {
        let extras = Self {
            instance_of: we.instance_of.clone(),
            part_of: links.get(&we.id).cloned(),
            triggers: we.triggers.clone(),
            mesh_sha256: we.mesh_asset.as_ref().and_then(|m| m.sha256.clone()),
            node_overrides: we
                .mesh_asset
                .as_ref()
                .map(|m| m.node_overrides.clone())
                .unwrap_or_default(),
        };
        (!extras.is_empty()).then_some(extras)
    }

    pub fn is_empty(&self) -> bool {
        self.instance_of.is_none()
            && self.part_of.is_none()
            && self.triggers.is_empty()
            && self.mesh_sha256.is_none()
            && self.node_overrides.is_empty()
    }

    /// Record a trigger. The runtime keeps one trigger per event kind on an
    /// entity (a second `click` replaces the first), so this does too.
    pub fn add_trigger(&mut self, trigger: wt::TriggerDef) {
        let kind = std::mem::discriminant(&trigger.on);
        self.triggers
            .retain(|t| std::mem::discriminant(&t.on) != kind);
        self.triggers.push(trigger);
    }

    /// Write the extras into a saved entity.
    pub fn write_into(&self, we: &mut wt::WorldEntity) {
        we.instance_of = self.instance_of.clone();
        we.triggers = self.triggers.clone();
        if let Some(mesh) = we.mesh_asset.as_mut() {
            mesh.sha256 = mesh.sha256.take().or_else(|| self.mesh_sha256.clone());
            mesh.node_overrides = self.node_overrides.clone();
        }
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
    /// patch from the definition's part to what the part is now. `entities`
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
            let Some(saved) = parts.get(&entity.id) else {
                continue;
            };
            for part in &def.parts {
                let Some((_, now)) = saved.iter().find(|(name, _)| *name == part.name) else {
                    continue;
                };
                let patch = wt::EntityPatch::between(part, now);
                if !patch.is_empty() {
                    instance.overrides.push(wt::PartOverride {
                        part: part.name.clone(),
                        patch,
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
    }

    #[test]
    fn a_second_trigger_of_a_kind_replaces_the_first() {
        let mut extras = WorldExtras::default();
        let click = |text: &str| wt::TriggerDef {
            on: wt::TriggerEvent::Click {
                max_distance: 5.0,
                prompt: None,
            },
            action: wt::TriggerActionDef::ShowText { text: text.into() },
            once: false,
            cooldown: None,
            requires_item: None,
        };
        extras.add_trigger(click("a"));
        extras.add_trigger(click("b"));
        assert_eq!(extras.triggers.len(), 1);
        assert_eq!(
            extras.triggers[0].action,
            wt::TriggerActionDef::ShowText { text: "b".into() }
        );
    }
}
