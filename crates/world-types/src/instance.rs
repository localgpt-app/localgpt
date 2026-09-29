//! Instances of reusable creations — one definition, many placements.
//!
//! A [`CreationDef`] with `parts` is a reusable definition: a small tree of
//! entities in the definition's own coordinates. An entity whose
//! [`WorldEntity::instance_of`] names that creation places a copy of it: the
//! instance entity's transform places the whole tree, and each
//! [`PartOverride`] changes one part of this copy only (a taller trunk, red
//! leaves) by an [`EntityPatch`]. Editing the definition changes every
//! instance that doesn't override the edited field.
//!
//! This is the composition model of OpenUSD (a reference plus sparse
//! overrides), of Godot's instanced scenes (the instance stores only the
//! properties it changes), and of component instances in design tools.
//!
//! Renderers draw instances by expanding them first, with
//! [`expand_instances`] (Rust) or `expandInstances` (the web viewer); both
//! produce the same entities with the same ids, so the conformance world
//! `instances.json` draws the same everywhere. Saved worlds keep the
//! unexpanded form.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::creation::CreationDef;
use crate::entity::{EntityPatch, WorldEntity};
use crate::identity::{CreationId, EntityId, EntityName};
use crate::validation::{Severity, ValidationIssue};
use crate::world::WorldManifest;

/// What an instance entity places: a reusable creation, with per-part changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InstanceOf {
    /// The creation whose `parts` this entity places.
    pub creation: CreationId,
    /// Changes to single parts of this copy, by part name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<PartOverride>,
}

/// A change to one part of one instance.
///
/// `name`, `parent` and `instance_of` in the patch are ignored: a part's
/// place in the tree comes from the definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PartOverride {
    /// The part's name in the definition (`CreationDef::parts`).
    pub part: EntityName,
    /// The fields this instance changes.
    pub patch: EntityPatch,
}

/// The name an expanded part gets: `"<instance>/<part>"`.
pub fn part_name(instance: &str, part: &str) -> String {
    format!("{instance}/{part}")
}

/// Expand every instance in `entities` into its parts.
///
/// The result is `entities` in order, each instance followed by its parts
/// (depth-first in definition order). Parts get fresh ids counting up from
/// `first_id`, the name `"<instance>/<part>"`, the instance (or their parent
/// part) as parent, and `creation_id` set to the definition's id. Instance
/// entities keep their `instance_of`, which renderers ignore. An instance of
/// a missing creation, or of one without parts, expands to nothing
/// ([`validate_instances`] reports it). Parts never expand further, and an
/// instance that already has its parts (a child with the creation's
/// `creation_id`) is left alone, so expanding twice changes nothing.
pub fn expand_instances(
    entities: &[WorldEntity],
    creations: &[CreationDef],
    first_id: u64,
) -> Vec<WorldEntity> {
    let by_id: HashMap<CreationId, &CreationDef> = creations.iter().map(|c| (c.id, c)).collect();
    let expanded: HashSet<(EntityId, CreationId)> = entities
        .iter()
        .filter_map(|e| Some((e.parent?, e.creation_id?)))
        .collect();
    let mut next_id = first_id;
    let mut out = Vec::with_capacity(entities.len());
    for entity in entities {
        out.push(entity.clone());
        let Some(instance) = &entity.instance_of else {
            continue;
        };
        let Some(def) = by_id.get(&instance.creation) else {
            continue;
        };
        if expanded.contains(&(entity.id, def.id)) {
            continue;
        }
        let mut ids: HashMap<EntityId, EntityId> = HashMap::new();
        for part in &def.parts {
            ids.insert(part.id, EntityId(next_id));
            next_id += 1;
        }
        for part in &def.parts {
            let mut expanded = part.clone();
            for o in instance.overrides.iter().filter(|o| o.part == part.name) {
                let mut patch = o.patch.clone();
                patch.name = None;
                patch.parent = None;
                patch.instance_of = None;
                patch.apply(&mut expanded);
            }
            expanded.id = ids[&part.id];
            expanded.name = EntityName::new(part_name(entity.name.as_str(), part.name.as_str()));
            expanded.parent = Some(
                part.parent
                    .and_then(|p| ids.get(&p).copied())
                    .unwrap_or(entity.id),
            );
            expanded.chunk = entity.chunk;
            expanded.creation_id = Some(def.id);
            expanded.instance_of = None;
            out.push(expanded);
        }
    }
    out
}

/// Which instance an expanded part belongs to, and which part it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartLink {
    /// The instance entity the part was expanded under.
    pub instance: EntityId,
    /// The creation the instance places.
    pub creation: CreationId,
    /// The part's name in the definition.
    pub part: EntityName,
}

/// For every expanded part in `entities`, the instance it belongs to: a part
/// is an entity with a `creation_id` whose parent chain reaches an entity
/// that is an instance of that creation, named `"<instance>/<part>"`. Lets a
/// loader tag parts so a saver can fold them back into the instance.
pub fn part_links(entities: &[WorldEntity]) -> HashMap<EntityId, PartLink> {
    let by_id: HashMap<EntityId, &WorldEntity> = entities.iter().map(|e| (e.id, e)).collect();
    let mut links = HashMap::new();
    for e in entities {
        let Some(creation) = e.creation_id else {
            continue;
        };
        let mut cursor = e.parent;
        let mut hops = 0;
        while let Some(id) = cursor
            && hops < 64
        {
            let Some(p) = by_id.get(&id) else { break };
            if p.instance_of
                .as_ref()
                .is_some_and(|i| i.creation == creation)
            {
                let prefix = format!("{}/", p.name);
                if let Some(part) = e.name.as_str().strip_prefix(&prefix) {
                    links.insert(
                        e.id,
                        PartLink {
                            instance: p.id,
                            creation,
                            part: EntityName::new(part),
                        },
                    );
                }
                break;
            }
            if p.creation_id != Some(creation) {
                break;
            }
            cursor = p.parent;
            hops += 1;
        }
    }
    links
}

impl WorldManifest {
    /// Every entity the manifest defines: the placed entities, then the
    /// parts of every reusable creation (for asset lists and the like).
    pub fn all_entities(&self) -> impl Iterator<Item = &WorldEntity> {
        self.entities
            .iter()
            .chain(self.creations.iter().flat_map(|c| c.parts.iter()))
    }

    /// Whether any entity is an instance of a creation.
    pub fn has_instances(&self) -> bool {
        self.entities.iter().any(|e| e.instance_of.is_some())
    }

    /// The first id free for expanded parts: past `next_entity_id` and past
    /// every entity's id.
    pub fn first_expansion_id(&self) -> u64 {
        let max = self.entities.iter().map(|e| e.id.0).max().unwrap_or(0);
        self.next_entity_id.max(max + 1)
    }

    /// Replace the entities with their expansion ([`expand_instances`]) and
    /// move `next_entity_id` past the parts' ids. Returns how many parts
    /// were added. For renderers and loaders; a world saved after this has
    /// lost its instancing.
    pub fn expand_instances(&mut self) -> usize {
        if !self.has_instances() {
            return 0;
        }
        let first = self.first_expansion_id();
        let before = self.entities.len();
        self.entities = expand_instances(&self.entities, &self.creations, first);
        let added = self.entities.len() - before;
        self.next_entity_id = first + added as u64;
        added
    }
}

/// Check the definitions and the instances of a manifest.
pub fn validate_instances(manifest: &WorldManifest) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let error = |message: String| ValidationIssue {
        severity: Severity::Error,
        message,
    };

    let mut creation_ids = HashSet::new();
    for def in &manifest.creations {
        if !creation_ids.insert(def.id) {
            issues.push(error(format!("Creation id {} is used twice", def.id.0)));
        }
        let mut part_ids = HashSet::new();
        let mut part_names = HashSet::new();
        for part in &def.parts {
            if !part_ids.insert(part.id) {
                issues.push(error(format!(
                    "Creation '{}' has two parts with id {}",
                    def.name, part.id
                )));
            }
            if !part_names.insert(part.name.as_str()) {
                issues.push(error(format!(
                    "Creation '{}' has two parts named '{}'",
                    def.name, part.name
                )));
            }
            if part.instance_of.is_some() {
                issues.push(error(format!(
                    "Part '{}' of creation '{}' is itself an instance; parts don't nest",
                    part.name, def.name
                )));
            }
        }
        for part in &def.parts {
            if let Some(parent) = part.parent
                && !part_ids.contains(&parent)
            {
                issues.push(error(format!(
                    "Part '{}' of creation '{}' has parent {} outside the creation",
                    part.name, def.name, parent
                )));
            }
        }
    }

    let creations: HashMap<CreationId, &CreationDef> =
        manifest.creations.iter().map(|c| (c.id, c)).collect();
    for entity in &manifest.entities {
        let Some(instance) = &entity.instance_of else {
            continue;
        };
        let Some(def) = creations.get(&instance.creation) else {
            issues.push(error(format!(
                "Entity '{}' is an instance of creation {}, which doesn't exist",
                entity.name, instance.creation.0
            )));
            continue;
        };
        if def.parts.is_empty() {
            issues.push(error(format!(
                "Entity '{}' is an instance of creation '{}', which has no parts",
                entity.name, def.name
            )));
        }
        for o in &instance.overrides {
            if !def.parts.iter().any(|p| p.name == o.part) {
                issues.push(ValidationIssue {
                    severity: Severity::Warning,
                    message: format!(
                        "Entity '{}' overrides part '{}', which creation '{}' doesn't have",
                        entity.name, o.part, def.name
                    ),
                });
            }
        }
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::MaterialDef;
    use crate::shape::Shape;

    fn oak() -> CreationDef {
        let trunk = WorldEntity::new(1, "trunk").with_shape(Shape::Cylinder {
            radius: 0.3,
            height: 3.0,
        });
        let mut leaves = WorldEntity::new(2, "leaves")
            .with_shape(Shape::Sphere { radius: 1.5 })
            .at([0.0, 2.0, 0.0]);
        leaves.parent = Some(EntityId(1));
        CreationDef {
            id: CreationId(7),
            name: "oak_tree".into(),
            semantic_category: None,
            bbox_half: [1.5, 3.0, 1.5],
            entities: Vec::new(),
            parts: vec![trunk, leaves],
        }
    }

    fn instance(id: u64, name: &str, overrides: Vec<PartOverride>) -> WorldEntity {
        let mut e = WorldEntity::new(id, name).at([id as f32 * 5.0, 0.0, 0.0]);
        e.instance_of = Some(InstanceOf {
            creation: CreationId(7),
            overrides,
        });
        e
    }

    #[test]
    fn expands_parts_under_the_instance() {
        let red = PartOverride {
            part: EntityName::new("leaves"),
            patch: EntityPatch {
                material: Some(Some(MaterialDef {
                    color: [0.8, 0.1, 0.1, 1.0],
                    ..Default::default()
                })),
                // Ignored: a part's place comes from the definition.
                name: Some(EntityName::new("renamed")),
                parent: Some(None),
                ..Default::default()
            },
        };
        let entities = vec![
            instance(1, "oak_1", Vec::new()),
            instance(2, "oak_2", vec![red]),
        ];
        let out = expand_instances(&entities, &[oak()], 100);

        let names: Vec<&str> = out.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "oak_1",
                "oak_1/trunk",
                "oak_1/leaves",
                "oak_2",
                "oak_2/trunk",
                "oak_2/leaves"
            ]
        );
        let ids: Vec<u64> = out.iter().map(|e| e.id.0).collect();
        assert_eq!(ids, [1, 100, 101, 2, 102, 103]);
        // Trunk hangs off the instance, leaves off the trunk copy.
        assert_eq!(out[1].parent, Some(EntityId(1)));
        assert_eq!(out[2].parent, Some(EntityId(100)));
        assert_eq!(out[5].parent, Some(EntityId(102)));
        assert_eq!(out[1].creation_id, Some(CreationId(7)));
        // Only oak_2's leaves are red.
        assert!(out[2].material.is_none());
        assert_eq!(
            out[5].material.as_ref().unwrap().color,
            [0.8, 0.1, 0.1, 1.0]
        );
        assert_eq!(out[5].transform.position, [0.0, 2.0, 0.0]);
    }

    #[test]
    fn part_links_find_the_instance() {
        let entities = vec![
            instance(1, "oak_1", Vec::new()),
            instance(2, "oak_2", Vec::new()),
        ];
        let out = expand_instances(&entities, &[oak()], 100);
        let links = part_links(&out);
        assert_eq!(links.len(), 4);
        assert_eq!(
            links[&EntityId(101)],
            PartLink {
                instance: EntityId(1),
                creation: CreationId(7),
                part: EntityName::new("leaves"),
            }
        );
        assert_eq!(links[&EntityId(102)].instance, EntityId(2));
        assert!(!links.contains_key(&EntityId(1)));
    }

    #[test]
    fn manifest_expansion_moves_next_id() {
        let mut m = WorldManifest::new("forest");
        m.creations.push(oak());
        m.entities.push(instance(1, "oak_1", Vec::new()));
        m.entities.push(instance(9, "oak_2", Vec::new()));
        m.next_entity_id = 5;
        assert_eq!(m.first_expansion_id(), 10);
        assert_eq!(m.expand_instances(), 4);
        assert_eq!(m.next_entity_id, 14);
        assert_eq!(m.entities.len(), 6);
        // Expanding again changes nothing.
        let again = expand_instances(&m.entities, &m.creations, 100);
        assert_eq!(again, m.entities);
        assert_eq!(m.expand_instances(), 0);
    }

    #[test]
    fn validation_catches_bad_references() {
        let mut m = WorldManifest::new("broken");
        let mut def = oak();
        def.parts[0].instance_of = Some(InstanceOf {
            creation: CreationId(7),
            overrides: Vec::new(),
        });
        def.parts[1].parent = Some(EntityId(42));
        m.creations.push(def);
        let mut missing = WorldEntity::new(1, "ghost");
        missing.instance_of = Some(InstanceOf {
            creation: CreationId(99),
            overrides: Vec::new(),
        });
        m.entities.push(missing);
        m.entities.push(instance(
            2,
            "oak",
            vec![PartOverride {
                part: EntityName::new("branch"),
                patch: EntityPatch::default(),
            }],
        ));

        let issues = validate_instances(&m);
        let errors: Vec<&str> = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .map(|i| i.message.as_str())
            .collect();
        assert!(errors.iter().any(|m| m.contains("don't nest")));
        assert!(errors.iter().any(|m| m.contains("outside the creation")));
        assert!(errors.iter().any(|m| m.contains("doesn't exist")));
        assert!(
            issues
                .iter()
                .any(|i| i.severity == Severity::Warning && i.message.contains("'branch'"))
        );
    }
}
