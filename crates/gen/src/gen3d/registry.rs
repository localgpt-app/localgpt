//! Name → Entity bidirectional registry with stable world IDs.
//!
//! All Gen entities are referenced by human-readable names rather than
//! opaque Bevy Entity IDs. This registry maintains the mapping and also
//! tracks stable `EntityId`s that survive save/load cycles.

use bevy::prelude::*;
use std::collections::HashMap;

use localgpt_world_types as wt;

/// Marker component attached to every Gen-managed entity.
#[derive(Component)]
pub struct GenEntity {
    /// What kind of entity this is (for scene_info reporting).
    pub entity_type: GenEntityType,
    /// Stable entity ID from the world data model (survives renames).
    pub world_id: wt::EntityId,
}

/// Bevy component storing the parametric shape alongside the mesh.
///
/// When a primitive is spawned, Bevy gets the mesh (triangles) and we also
/// store the parametric `Shape` here so it survives save/load cycles.
/// Without this, the shape info is lost when exporting to glTF.
#[derive(Component, Clone, Debug)]
pub struct ParametricShape {
    pub shape: wt::Shape,
}

/// Associates an entity with a named region for dirty tracking and bulk operations.
#[derive(Component, Debug, Clone)]
pub struct RegionMember {
    pub region_id: String,
}

/// Bevy component tracking the source file of an imported glTF/GLB.
///
/// Attached to the root entity spawned by `LoadGltf` so that
/// `snapshot_entity` / save can record the path in `WorldEntity.mesh_asset`.
#[derive(Component, Clone, Debug)]
pub struct GltfSource {
    /// Absolute or workspace-relative path to the original .glb/.gltf file.
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum GenEntityType {
    Primitive,
    Light,
    Camera,
    Mesh,
    Group,
    AudioEmitter,
}

impl GenEntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Primitive => "primitive",
            Self::Light => "light",
            Self::Camera => "camera",
            Self::Mesh => "mesh",
            Self::Group => "group",
            Self::AudioEmitter => "audio_emitter",
        }
    }
}

/// Monotonic allocator for stable entity IDs.
#[derive(Resource)]
pub struct NextEntityId {
    next: u64,
}

impl Default for NextEntityId {
    fn default() -> Self {
        // Start at 1 — 0 is reserved for "no entity".
        Self { next: 1 }
    }
}

impl NextEntityId {
    /// Allocate the next entity ID.
    pub fn alloc(&mut self) -> wt::EntityId {
        let id = wt::EntityId(self.next);
        self.next += 1;
        id
    }

    /// Ensure the counter is at least `min_next` (used after loading a world
    /// that already has assigned IDs).
    pub fn ensure_at_least(&mut self, min_next: u64) {
        if min_next > self.next {
            self.next = min_next;
        }
    }
}

/// Bevy resource that maps names ↔ entities ↔ world IDs.
#[derive(Resource, Default)]
pub struct NameRegistry {
    name_to_entity: HashMap<String, Entity>,
    entity_to_name: HashMap<Entity, String>,
    id_to_entity: HashMap<u64, Entity>,
    entity_to_id: HashMap<Entity, u64>,
}

#[allow(dead_code)]
impl NameRegistry {
    /// Insert a name ↔ entity mapping (without a world ID).
    pub fn insert(&mut self, name: String, entity: Entity) {
        self.name_to_entity.insert(name.clone(), entity);
        self.entity_to_name.insert(entity, name);
    }

    /// Insert a name ↔ entity mapping with a stable world ID.
    pub fn insert_with_id(&mut self, name: String, entity: Entity, id: wt::EntityId) {
        self.name_to_entity.insert(name.clone(), entity);
        self.entity_to_name.insert(entity, name);
        self.id_to_entity.insert(id.0, entity);
        self.entity_to_id.insert(entity, id.0);
    }

    pub fn get_entity(&self, name: &str) -> Option<Entity> {
        self.name_to_entity.get(name).copied()
    }

    pub fn get_name(&self, entity: Entity) -> Option<&str> {
        self.entity_to_name.get(&entity).map(|s| s.as_str())
    }

    /// Look up a Bevy entity by its stable world ID.
    pub fn get_entity_by_id(&self, id: &wt::EntityId) -> Option<Entity> {
        self.id_to_entity.get(&id.0).copied()
    }

    /// Look up a stable world ID by Bevy entity.
    pub fn get_id(&self, entity: Entity) -> Option<wt::EntityId> {
        self.entity_to_id.get(&entity).map(|id| wt::EntityId(*id))
    }

    pub fn remove_by_name(&mut self, name: &str) -> Option<Entity> {
        if let Some(entity) = self.name_to_entity.remove(name) {
            self.entity_to_name.remove(&entity);
            // Also clean up ID mappings
            if let Some(id) = self.entity_to_id.remove(&entity) {
                self.id_to_entity.remove(&id);
            }
            Some(entity)
        } else {
            None
        }
    }

    pub fn remove_by_entity(&mut self, entity: Entity) -> Option<String> {
        if let Some(name) = self.entity_to_name.remove(&entity) {
            self.name_to_entity.remove(&name);
            if let Some(id) = self.entity_to_id.remove(&entity) {
                self.id_to_entity.remove(&id);
            }
            Some(name)
        } else {
            None
        }
    }

    pub fn contains_name(&self, name: &str) -> bool {
        self.name_to_entity.contains_key(name)
    }

    pub fn all_names(&self) -> impl Iterator<Item = (&str, Entity)> {
        self.name_to_entity.iter().map(|(k, v)| (k.as_str(), *v))
    }

    pub fn len(&self) -> usize {
        self.name_to_entity.len()
    }

    pub fn is_empty(&self) -> bool {
        self.name_to_entity.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_entity(bits: u64) -> Entity {
        Entity::from_bits(bits)
    }

    #[test]
    fn test_next_entity_id_starts_at_1() {
        let mut alloc = NextEntityId::default();
        let id = alloc.alloc();
        assert_eq!(id.0, 1);
        let id2 = alloc.alloc();
        assert_eq!(id2.0, 2);
    }

    #[test]
    fn test_next_entity_id_ensure_at_least() {
        let mut alloc = NextEntityId::default();
        alloc.ensure_at_least(100);
        let id = alloc.alloc();
        assert_eq!(id.0, 100);

        // Should not go backwards
        alloc.ensure_at_least(50);
        let id2 = alloc.alloc();
        assert_eq!(id2.0, 101);
    }

    #[test]
    fn test_registry_insert_and_lookup() {
        let mut reg = NameRegistry::default();
        let entity = dummy_entity(1);
        reg.insert("box".to_string(), entity);

        assert_eq!(reg.get_entity("box"), Some(entity));
        assert_eq!(reg.get_name(entity), Some("box"));
        assert!(reg.contains_name("box"));
        assert!(!reg.contains_name("sphere"));
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn test_registry_insert_with_id() {
        let mut reg = NameRegistry::default();
        let entity = dummy_entity(1);
        let wid = wt::EntityId(42);
        reg.insert_with_id("sphere".to_string(), entity, wid);

        assert_eq!(reg.get_entity("sphere"), Some(entity));
        assert_eq!(reg.get_entity_by_id(&wid), Some(entity));
        assert_eq!(reg.get_id(entity), Some(wid));
    }

    #[test]
    fn test_registry_remove_by_name() {
        let mut reg = NameRegistry::default();
        let entity = dummy_entity(1);
        let wid = wt::EntityId(1);
        reg.insert_with_id("cube".to_string(), entity, wid);

        let removed = reg.remove_by_name("cube");
        assert_eq!(removed, Some(entity));
        assert!(reg.get_entity("cube").is_none());
        assert!(reg.get_entity_by_id(&wid).is_none());
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_remove_by_entity() {
        let mut reg = NameRegistry::default();
        let entity = dummy_entity(2);
        let wid = wt::EntityId(5);
        reg.insert_with_id("cone".to_string(), entity, wid);

        let removed_name = reg.remove_by_entity(entity);
        assert_eq!(removed_name, Some("cone".to_string()));
        assert!(reg.get_entity("cone").is_none());
        assert!(reg.get_entity_by_id(&wid).is_none());
    }

    #[test]
    fn test_gen_entity_type_as_str() {
        assert_eq!(GenEntityType::Primitive.as_str(), "primitive");
        assert_eq!(GenEntityType::Light.as_str(), "light");
        assert_eq!(GenEntityType::AudioEmitter.as_str(), "audio_emitter");
    }

    #[test]
    fn test_dirty_tracker() {
        let mut tracker = DirtyTracker::default();
        assert!(!tracker.has_changes());
        assert_eq!(tracker.dirty_count(), 0);

        let id = wt::EntityId(1);
        tracker.mark_dirty(id);
        assert!(tracker.has_changes());
        assert!(tracker.is_dirty(&id));
        assert_eq!(tracker.dirty_count(), 1);

        tracker.clear();
        assert!(!tracker.has_changes());
        assert!(!tracker.is_dirty(&id));
    }

    #[test]
    fn test_dirty_tracker_world_meta() {
        let mut tracker = DirtyTracker {
            world_meta_dirty: true,
            ..Default::default()
        };
        assert!(tracker.has_changes());

        tracker.clear();
        assert!(!tracker.has_changes());
        assert!(!tracker.world_meta_dirty);
    }
}

/// Tracks which entities have been modified since the last save.
///
/// Enables incremental saves for large worlds — only dirty entities
/// need to be re-serialized. Call `mark_dirty` on spawn, modify,
/// behavior add/remove, and audio changes. Call `clear` after a
/// successful save.
#[derive(Resource, Default)]
pub struct DirtyTracker {
    dirty: std::collections::HashSet<u64>,
    /// Set when any world-level metadata changed (environment, camera, avatar, tours).
    pub world_meta_dirty: bool,
}

#[allow(dead_code)]
impl DirtyTracker {
    /// Mark an entity as modified.
    pub fn mark_dirty(&mut self, id: wt::EntityId) {
        self.dirty.insert(id.0);
    }

    /// Check if an entity is dirty.
    pub fn is_dirty(&self, id: &wt::EntityId) -> bool {
        self.dirty.contains(&id.0)
    }

    /// Check if anything has changed since the last save.
    pub fn has_changes(&self) -> bool {
        !self.dirty.is_empty() || self.world_meta_dirty
    }

    /// Get all dirty entity IDs.
    pub fn dirty_ids(&self) -> impl Iterator<Item = wt::EntityId> + '_ {
        self.dirty.iter().map(|id| wt::EntityId(*id))
    }

    /// Number of dirty entities.
    pub fn dirty_count(&self) -> usize {
        self.dirty.len()
    }

    /// Clear all dirty flags (call after a successful save).
    pub fn clear(&mut self) {
        self.dirty.clear();
        self.world_meta_dirty = false;
    }
}
