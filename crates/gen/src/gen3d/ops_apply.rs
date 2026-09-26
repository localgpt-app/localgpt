//! Apply committed world ops to the live scene — the reverse direction of
//! the projection. Used for undo, guest edits (`submit`), replay, and later
//! relay/cloud-driven changes.
//!
//! The projection turns scene state into ops; this turns ops into scene
//! state. A host must apply authority-originated ops or its next projection
//! would "heal" the document back to the pre-undo scene.
//!
//! Coverage: spawn, delete, and every `EntityPatch` field (transform, name,
//! parent, shape, material, light, behaviors), plus `SetEnvironment`.
//! Audio ops, ambience, camera, modulations and mesh-asset replacement are
//! no-ops here (audio follows the scene's own state; the host keeps its
//! camera) — the document still tracks them for other clients.

use bevy::prelude::*;
use localgpt_world_types as wt;

use super::behaviors::{BehaviorInstance, BehaviorState, EntityBehaviors};
use super::commands::BehaviorDef as GenBehaviorDef;
use super::plugin::{
    PendingGltfLoads, PendingWorldSetup, handle_clear_scene, insert_light_component,
    shape_mesh_for, spawn_world_entities, textured_material,
};
use super::registry::{GenEntity, NameRegistry, NextEntityId, ParametricShape};

/// Everything the applier needs from the ECS, in one SystemParam.
#[derive(bevy::ecs::system::SystemParam)]
pub struct OpsApplier<'w, 's> {
    commands: Commands<'w, 's>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    registry: ResMut<'w, NameRegistry>,
    next_entity_id: ResMut<'w, NextEntityId>,
    behavior_state: ResMut<'w, BehaviorState>,
    asset_server: Res<'w, AssetServer>,
    pending_gltf: ResMut<'w, PendingGltfLoads>,
    transforms: Query<'w, 's, &'static mut Transform>,
    behaviors_query: Query<'w, 's, &'static mut EntityBehaviors>,
    parametric_shapes: Query<'w, 's, &'static mut ParametricShape>,
    visibility_query: Query<'w, 's, &'static mut Visibility>,
    clear_color: Option<ResMut<'w, ClearColor>>,
    ambient_light: Option<ResMut<'w, GlobalAmbientLight>>,
    gen_entities: Query<'w, 's, &'static GenEntity>,
    audio_engine: ResMut<'w, super::audio::AudioEngine>,
    pending_world: ResMut<'w, PendingWorldSetup>,
}

impl OpsApplier<'_, '_> {
    /// Replace the whole scene with a document's entities (used after
    /// replaying an op log). Clears first — merging into the startup scene
    /// would collide world ids and names with the replayed entities.
    pub fn rebuild_scene(&mut self, entities: &[wt::WorldEntity]) {
        handle_clear_scene(
            true,
            false,
            &mut self.commands,
            &mut self.registry,
            &self.gen_entities,
            &mut self.audio_engine,
            &mut self.behavior_state,
            &mut self.pending_world,
        );
        self.spawn_all(entities);
    }
    /// Spawn every entity in a document (used after replaying an op log).
    pub fn spawn_all(&mut self, entities: &[wt::WorldEntity]) {
        spawn_world_entities(
            entities,
            &mut self.commands,
            &mut self.meshes,
            &mut self.materials,
            &mut self.registry,
            &mut self.next_entity_id,
            &mut self.behavior_state,
            &self.asset_server,
            &mut self.pending_gltf,
            None,
        );
    }

    /// Apply committed ops to the scene.
    pub fn apply_ops(&mut self, ops: &[wt::EditOp]) {
        for op in ops {
            self.apply_op(op);
        }
    }

    fn apply_op(&mut self, op: &wt::EditOp) {
        match op {
            wt::EditOp::SpawnEntity { entity } => {
                // Already in the scene (e.g. the projection re-announcing a
                // resumed entity): spawn_world_entities skips duplicates.
                self.spawn_all(std::slice::from_ref(entity));
            }
            wt::EditOp::DeleteEntity { id } => {
                let Some(bevy_entity) = self.registry.get_entity_by_id(id) else {
                    return;
                };
                self.registry.remove_by_entity(bevy_entity);
                self.commands.entity(bevy_entity).despawn();
            }
            wt::EditOp::ModifyEntity { id, patch } => self.modify(*id, patch),
            wt::EditOp::SetEnvironment { env } => {
                if let (Some(bg), Some(clear)) =
                    (env.background_color, self.clear_color.as_deref_mut())
                {
                    clear.0 = Color::srgba(bg[0], bg[1], bg[2], bg[3]);
                }
                if let Some(ambient) = self.ambient_light.as_deref_mut() {
                    if let Some(intensity) = env.ambient_intensity {
                        ambient.brightness = intensity;
                    }
                    if let Some(color) = env.ambient_color {
                        ambient.color = Color::srgba(color[0], color[1], color[2], color[3]);
                    }
                }
            }
            // The host keeps its own camera; audio follows the scene's own
            // emitter components (audio ops matter for other clients).
            wt::EditOp::SetCamera { .. }
            | wt::EditOp::SetAmbience { .. }
            | wt::EditOp::SpawnAudioEmitter { .. }
            | wt::EditOp::RemoveAudioEmitter { .. } => {}
            wt::EditOp::Batch { ops } => self.apply_ops(ops),
        }
    }

    fn modify(&mut self, id: wt::EntityId, patch: &wt::EntityPatch) {
        let Some(bevy_entity) = self.registry.get_entity_by_id(&id) else {
            return;
        };

        if let Some(name) = &patch.name {
            self.registry.remove_by_entity(bevy_entity);
            self.registry
                .insert_with_id(name.0.clone(), bevy_entity, id);
            self.commands
                .entity(bevy_entity)
                .insert(Name::new(name.0.clone()));
        }

        if let Some(t) = &patch.transform {
            let rotation = Quat::from_euler(
                EulerRot::XYZ,
                t.rotation_degrees[0].to_radians(),
                t.rotation_degrees[1].to_radians(),
                t.rotation_degrees[2].to_radians(),
            );
            if let Ok(mut transform) = self.transforms.get_mut(bevy_entity) {
                transform.translation = Vec3::from_array(t.position);
                transform.rotation = rotation;
                transform.scale = Vec3::from_array(t.scale);
            }
            if let Ok(mut vis) = self.visibility_query.get_mut(bevy_entity) {
                *vis = if t.visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            // Keep behavior bases in step so additive behaviors (bob, pulse)
            // don't stomp the edit on the next frame.
            if let Ok(mut ebs) = self.behaviors_query.get_mut(bevy_entity) {
                for b in &mut ebs.behaviors {
                    b.base_position = Vec3::from_array(t.position);
                    b.base_scale = Vec3::from_array(t.scale);
                }
            }
        }

        if let Some(parent) = &patch.parent {
            match parent {
                Some(pid) => {
                    if let Some(parent_entity) = self.registry.get_entity_by_id(pid) {
                        self.commands
                            .entity(bevy_entity)
                            .insert(ChildOf(parent_entity));
                    }
                }
                None => {
                    self.commands.entity(bevy_entity).remove::<ChildOf>();
                }
            }
        }

        if let Some(Some(shape)) = &patch.shape {
            let mesh_handle = self
                .meshes
                .add(shape_mesh_for(shape, &wt::MaterialDef::default()));
            self.commands
                .entity(bevy_entity)
                .insert(Mesh3d(mesh_handle));
            if let Ok(mut ps) = self.parametric_shapes.get_mut(bevy_entity) {
                ps.shape = shape.clone();
            } else {
                self.commands.entity(bevy_entity).insert(ParametricShape {
                    shape: shape.clone(),
                });
            }
        }

        if let Some(Some(def)) = &patch.material {
            let (material, textures) = textured_material(def, None, &self.asset_server);
            let handle = self.materials.add(material);
            let mut entity_cmd = self.commands.entity(bevy_entity);
            entity_cmd.insert(MeshMaterial3d(handle));
            if !textures.maps.is_empty() {
                entity_cmd.insert(textures);
            }
        }

        if let Some(light) = &patch.light {
            match light {
                Some(def) => {
                    let mut entity_cmd = self.commands.entity(bevy_entity);
                    insert_light_component(&mut entity_cmd, def);
                }
                None => {
                    self.commands
                        .entity(bevy_entity)
                        .remove::<DirectionalLight>()
                        .remove::<PointLight>()
                        .remove::<SpotLight>();
                }
            }
        }

        if let Some(defs) = &patch.behaviors {
            let base = self
                .transforms
                .get(bevy_entity)
                .copied()
                .unwrap_or_default();
            let mut instances = Vec::with_capacity(defs.len());
            for def in defs {
                instances.push(BehaviorInstance {
                    id: self.behavior_state.next_id(),
                    def: GenBehaviorDef::from(def),
                    base_position: base.translation,
                    base_scale: base.scale,
                });
            }
            if let Ok(mut ebs) = self.behaviors_query.get_mut(bevy_entity) {
                ebs.behaviors = instances;
            } else if !instances.is_empty() {
                self.commands.entity(bevy_entity).insert(EntityBehaviors {
                    behaviors: instances,
                });
            }
        }

        // audio / mesh_asset / modulations: intentionally not applied (see
        // the module doc). The document tracks them for other clients.
    }
}
