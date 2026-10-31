//! The inspector's state: what is selected, the edits a panel asks for,
//! where the camera should fly, and the WebSocket protocol remote
//! inspectors speak (SwiftUI on iPad and macOS, Compose on Android).
//!
//! Gen draws no panels. The desktop app's interface is a web page over the
//! viewport (`crates/app/ui`), and it reads and writes the same resources a
//! click in the viewport or a remote inspector does: [`InspectorSelection`],
//! [`PanelIntents`], [`PendingFocusEntity`].

pub mod protocol;
pub mod selection;
pub mod ws_server;

use bevy::ecs::system::SystemParam;
use bevy::picking::prelude::*;
use bevy::prelude::*;

use localgpt_world_types as wt;

use crate::gen3d::audio::AudioEngine;
use crate::gen3d::behaviors::{BehaviorState, EntityBehaviors};
use crate::gen3d::plugin::CurrentWorld;
use crate::gen3d::registry::*;

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

/// What the inspector and the canvas have selected: a primary entity, and
/// any others added to it with shift-click. Gestures and keys act on all
/// of them; the detail panel shows the primary.
#[derive(Resource, Default)]
pub struct InspectorSelection {
    pub entity: Option<Entity>,
    /// The rest of a multi-selection, in the order they were added; never
    /// holds `entity`.
    pub others: Vec<Entity>,
}

impl InspectorSelection {
    /// Select one entity and nothing else (`None`: nothing). Writes only
    /// what changes, so change detection stays meaningful.
    pub fn set(&mut self, entity: Option<Entity>) {
        if self.entity != entity {
            self.entity = entity;
        }
        if !self.others.is_empty() {
            self.others.clear();
        }
    }

    pub fn clear(&mut self) {
        self.set(None);
    }

    /// Add an entity to the selection, or take it out if it is in (a
    /// shift-click). Taking out the primary promotes the next one.
    pub fn toggle(&mut self, entity: Entity) {
        if self.entity == Some(entity) {
            self.entity = if self.others.is_empty() {
                None
            } else {
                Some(self.others.remove(0))
            };
        } else if let Some(n) = self.others.iter().position(|e| *e == entity) {
            self.others.remove(n);
        } else if self.entity.is_none() {
            self.entity = Some(entity);
        } else {
            self.others.push(entity);
        }
    }

    pub fn contains(&self, entity: Entity) -> bool {
        self.entity == Some(entity) || self.others.contains(&entity)
    }

    /// The primary first, then the others.
    pub fn all(&self) -> impl Iterator<Item = Entity> + '_ {
        self.entity.into_iter().chain(self.others.iter().copied())
    }

    pub fn len(&self) -> usize {
        usize::from(self.entity.is_some()) + self.others.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entity.is_none()
    }
}

/// Edits a panel asks for — field patches, scene-wide patches, undo —
/// drained into the live canvas's intent stream by `canvas_edit`. Without
/// a live world nothing drains them, and a panel only writes when one is
/// open.
#[derive(Resource, Default)]
pub struct PanelIntents(pub Vec<crate::gen3d::canvas_edit::PersonIntent>);

/// When set, the camera flies to this pose: a bookmark, or a shot.
#[derive(Resource, Default)]
pub struct PendingCameraPose {
    pub pose: Option<(Vec3, Vec3)>,
}

/// When set, the camera system should fly to focus on this entity's position.
#[derive(Resource, Default)]
pub struct PendingFocusEntity {
    pub target: Option<Entity>,
}

/// Tracks last known registry count and selected entity transform for change detection.
#[derive(Resource)]
pub struct WsBridgeState {
    last_registry_count: usize,
    last_transform_push: f64,
    last_transform_pos: Option<[f32; 3]>,
    last_transform_rot: Option<[f32; 3]>,
}

impl Default for WsBridgeState {
    fn default() -> Self {
        Self {
            last_registry_count: usize::MAX,
            last_transform_push: 0.0,
            last_transform_pos: None,
            last_transform_rot: None,
        }
    }
}

// ---------------------------------------------------------------------------
// SystemParam for component queries
// ---------------------------------------------------------------------------

/// The scene as an inspector reads it: what [`build_scene_tree`],
/// [`build_entity_detail`] and [`build_world_info`] describe.
#[derive(SystemParam)]
pub struct InspectorQueries<'w, 's> {
    pub gen_entities: Query<'w, 's, &'static GenEntity>,
    pub transforms: Query<'w, 's, &'static mut Transform>,
    pub globals: Query<'w, 's, &'static GlobalTransform>,
    pub shapes: Query<'w, 's, &'static ParametricShape>,
    pub material_handles: Query<'w, 's, &'static MeshMaterial3d<StandardMaterial>>,
    pub materials: ResMut<'w, Assets<StandardMaterial>>,
    pub original_emissives: Query<'w, 's, &'static mut selection::OriginalEmissive>,
    pub point_lights: Query<'w, 's, &'static mut PointLight>,
    pub dir_lights: Query<'w, 's, &'static mut DirectionalLight>,
    pub spot_lights: Query<'w, 's, &'static mut SpotLight>,
    pub behaviors_q: Query<'w, 's, &'static EntityBehaviors>,
    pub gltf_sources: Query<'w, 's, &'static GltfSource>,
    pub visibility_q: Query<'w, 's, &'static mut Visibility>,
    pub children_q: Query<'w, 's, &'static Children>,
    pub parent_q: Query<'w, 's, &'static ChildOf>,
    pub behavior_state: Res<'w, BehaviorState>,
    pub audio_engine: Option<Res<'w, AudioEngine>>,
    pub current_world: Res<'w, CurrentWorld>,
    pub mesh_handles: Query<'w, 's, &'static Mesh3d>,
    pub mesh_assets: Res<'w, Assets<Mesh>>,
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        // Start the WebSocket inspector server
        ws_server::start_ws_server(app);

        // Mesh picking is not in DefaultPlugins: without it no mesh is ever
        // hit, and clicking the viewport selects nothing.
        if !app.is_plugin_added::<MeshPickingPlugin>() {
            app.add_plugins(MeshPickingPlugin);
        }
        app.init_resource::<InspectorSelection>()
            .init_resource::<WsBridgeState>()
            .init_resource::<PendingFocusEntity>()
            .init_resource::<PendingCameraPose>()
            .init_resource::<PanelIntents>()
            .add_systems(
                Update,
                (
                    deselect_on_escape,
                    selection::highlight_selected,
                    ws_bridge_system,
                    focus_camera_system,
                )
                    .chain(),
            )
            // 3D viewport click-to-select via Bevy's built-in mesh picking
            .add_observer(on_mesh_clicked);
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Escape clears the selection — unless the live canvas is there, which
/// takes Escape for itself (it ends a gesture, and leaves a shot).
fn deselect_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    canvas: Option<Res<crate::gen3d::canvas_edit::CanvasGesture>>,
    mut selection: ResMut<InspectorSelection>,
) {
    if canvas.is_none() && keys.just_pressed(KeyCode::Escape) && !selection.is_empty() {
        selection.clear();
    }
}

// ---------------------------------------------------------------------------
// WebSocket bridge system
// ---------------------------------------------------------------------------

/// Processes inbound client messages, detects scene changes, and streams transforms.
fn ws_bridge_system(
    bridge: Res<ws_server::InspectorWsBridge>,
    mut selection: ResMut<InspectorSelection>,
    registry: Res<NameRegistry>,
    mut params: InspectorQueries,
    mut focus: ResMut<PendingFocusEntity>,
    mut ws_state: ResMut<WsBridgeState>,
    time: Res<Time>,
) {
    // --- Process inbound messages (non-blocking) ---
    let rx = bridge.rx.clone();
    if let Ok(mut rx_guard) = rx.try_lock() {
        while let Ok(msg) = rx_guard.try_recv() {
            match msg {
                protocol::ClientMessage::RequestSceneTree => {
                    let tree = build_scene_tree(&registry, &params);
                    let _ = bridge
                        .tx
                        .send(protocol::ServerMessage::SceneTree { entities: tree });
                }
                protocol::ClientMessage::RequestEntityDetail { entity_id } => {
                    if let Some(entity) = registry.get_entity_by_id(&wt::EntityId(entity_id))
                        && let Some(data) = build_entity_detail(entity, &registry, &params)
                    {
                        let _ = bridge.tx.send(protocol::ServerMessage::EntityDetail {
                            entity_id,
                            data: Box::new(data),
                        });
                    }
                }
                protocol::ClientMessage::RequestWorldInfo => {
                    let data = build_world_info(&registry, &params);
                    let _ = bridge.tx.send(protocol::ServerMessage::WorldInfo { data });
                }
                protocol::ClientMessage::SelectEntity { entity_id } => {
                    if let Some(entity) = registry.get_entity_by_id(&wt::EntityId(entity_id)) {
                        selection.set(Some(entity));
                        let _ = bridge
                            .tx
                            .send(protocol::ServerMessage::SelectionChanged { entity_id });
                    }
                }
                protocol::ClientMessage::Deselect => {
                    selection.clear();
                    let _ = bridge.tx.send(protocol::ServerMessage::SelectionCleared);
                }
                protocol::ClientMessage::ToggleVisibility { entity_id } => {
                    if let Some(entity) = registry.get_entity_by_id(&wt::EntityId(entity_id))
                        && let Ok(mut visibility) = params.visibility_q.get_mut(entity)
                    {
                        *visibility = if *visibility == Visibility::Hidden {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        };
                    }
                }
                protocol::ClientMessage::FocusEntity { entity_id } => {
                    if let Some(entity) = registry.get_entity_by_id(&wt::EntityId(entity_id)) {
                        focus.target = Some(entity);
                    }
                }
                protocol::ClientMessage::RequestSceneSnapshot => {
                    match crate::gen3d::gltf_export::build_glb_bytes(
                        &registry,
                        &params.transforms.as_readonly(),
                        &params.gen_entities,
                        &params.parent_q,
                        &params.material_handles,
                        params.materials.as_ref(),
                        &params.mesh_handles,
                        params.mesh_assets.as_ref(),
                    ) {
                        Ok(glb_bytes) => {
                            bridge.send_binary(glb_bytes);
                        }
                        Err(e) => {
                            warn!("Scene snapshot export failed: {}", e);
                        }
                    }
                }
                protocol::ClientMessage::Subscribe { .. } => {
                    // Handled in ws_server connection handler
                }
            }
        }
    }

    // --- Detect scene changes (entity added/removed) ---
    let current_count = registry.len();
    if current_count != ws_state.last_registry_count {
        if ws_state.last_registry_count != usize::MAX {
            // Not the first frame — something changed
            let _ = bridge.tx.send(protocol::ServerMessage::SceneChanged);
        }
        ws_state.last_registry_count = current_count;
    }

    // --- Push selection changes to WS clients ---
    if selection.is_changed() {
        match selection.entity {
            Some(entity) => {
                if let Ok(gen_e) = params.gen_entities.get(entity) {
                    let _ = bridge.tx.send(protocol::ServerMessage::SelectionChanged {
                        entity_id: gen_e.world_id.0,
                    });
                }
                // Reset transform tracking for new selection
                ws_state.last_transform_pos = None;
                ws_state.last_transform_rot = None;
            }
            None => {
                let _ = bridge.tx.send(protocol::ServerMessage::SelectionCleared);
                ws_state.last_transform_pos = None;
                ws_state.last_transform_rot = None;
            }
        }
    }

    // --- Stream selected entity transform at ≤10 Hz ---
    let now = time.elapsed_secs_f64();
    if now - ws_state.last_transform_push >= 0.1 {
        if let Some(entity) = selection.entity
            && let Ok(gen_e) = params.gen_entities.get(entity)
            && let Ok(transform) = params.transforms.get(entity)
        {
            let pos: [f32; 3] = transform.translation.into();
            let (axis, angle) = transform.rotation.to_axis_angle();
            let deg = angle.to_degrees();
            let rot = [axis.x * deg, axis.y * deg, axis.z * deg];

            // Only send if transform actually changed
            let pos_changed = ws_state.last_transform_pos.as_ref() != Some(&pos);
            let rot_changed = ws_state.last_transform_rot.as_ref() != Some(&rot);

            if pos_changed || rot_changed {
                let _ = bridge
                    .tx
                    .send(protocol::ServerMessage::EntityTransformUpdated {
                        entity_id: gen_e.world_id.0,
                        position: pos,
                        rotation_degrees: rot,
                    });
                ws_state.last_transform_pos = Some(pos);
                ws_state.last_transform_rot = Some(rot);
            }
        }
        ws_state.last_transform_push = now;
    }
}

/// Build the scene tree for the WS protocol (and the app's outliner).
pub fn build_scene_tree(
    registry: &NameRegistry,
    params: &InspectorQueries,
) -> Vec<protocol::TreeEntity> {
    let mut entities = Vec::new();

    for (name, entity) in registry.all_names() {
        let Ok(gen_e) = params.gen_entities.get(entity) else {
            continue;
        };

        let visible = params
            .visibility_q
            .get(entity)
            .map_or(true, |v| *v != Visibility::Hidden);

        let parent_id = params
            .parent_q
            .get(entity)
            .ok()
            .and_then(|p| params.gen_entities.get(p.parent()).ok())
            .map(|pg| pg.world_id.0);

        let children: Vec<u64> = params
            .children_q
            .get(entity)
            .map(|ch| {
                ch.iter()
                    .filter_map(|c| params.gen_entities.get(c).ok().map(|g| g.world_id.0))
                    .collect()
            })
            .unwrap_or_default();

        entities.push(protocol::TreeEntity {
            id: gen_e.world_id.0,
            name: name.to_string(),
            entity_type: format!("{:?}", gen_e.entity_type),
            parent_id,
            visible,
            children,
        });
    }

    entities.sort_by_key(|a| a.name.to_lowercase());
    entities
}

/// Build entity detail data for the WS protocol (and the app's detail panel).
pub fn build_entity_detail(
    entity: Entity,
    registry: &NameRegistry,
    params: &InspectorQueries,
) -> Option<protocol::EntityDetailData> {
    let gen_e = params.gen_entities.get(entity).ok()?;
    let name = registry.get_name(entity)?;

    let transform = params.transforms.get(entity).ok().map(|t| {
        let (axis, angle) = t.rotation.to_axis_angle();
        let deg = angle.to_degrees();
        protocol::TransformSection {
            position: t.translation.into(),
            rotation_degrees: [axis.x * deg, axis.y * deg, axis.z * deg],
            scale: t.scale.into(),
            visible: params
                .visibility_q
                .get(entity)
                .map_or(true, |v| *v != Visibility::Hidden),
        }
    });

    let shape = params
        .shapes
        .get(entity)
        .ok()
        .map(|s| format!("{:?}", s.shape));

    let material = params
        .material_handles
        .get(entity)
        .ok()
        .and_then(|mh| params.materials.get(&mh.0))
        .map(|mat| protocol::MaterialSection {
            base_color: mat.base_color.to_linear().to_f32_array(),
            metallic: mat.metallic,
            roughness: mat.perceptual_roughness,
            reflectance: mat.reflectance,
            emissive: mat.emissive.to_f32_array(),
            alpha_mode: format!("{:?}", mat.alpha_mode),
            double_sided: mat.double_sided,
            unlit: mat.unlit,
        });

    let light = params
        .point_lights
        .get(entity)
        .ok()
        .map(|l| protocol::LightSection {
            light_type: "point".to_string(),
            color: [
                l.color.to_linear().red,
                l.color.to_linear().green,
                l.color.to_linear().blue,
            ],
            intensity: l.intensity,
            range: Some(l.range),
            shadows_enabled: l.shadow_maps_enabled,
            inner_angle: None,
            outer_angle: None,
        })
        .or_else(|| {
            params
                .dir_lights
                .get(entity)
                .ok()
                .map(|l| protocol::LightSection {
                    light_type: "directional".to_string(),
                    color: [
                        l.color.to_linear().red,
                        l.color.to_linear().green,
                        l.color.to_linear().blue,
                    ],
                    intensity: l.illuminance,
                    range: None,
                    shadows_enabled: l.shadow_maps_enabled,
                    inner_angle: None,
                    outer_angle: None,
                })
        })
        .or_else(|| {
            params
                .spot_lights
                .get(entity)
                .ok()
                .map(|l| protocol::LightSection {
                    light_type: "spot".to_string(),
                    color: [
                        l.color.to_linear().red,
                        l.color.to_linear().green,
                        l.color.to_linear().blue,
                    ],
                    intensity: l.intensity,
                    range: Some(l.range),
                    shadows_enabled: l.shadow_maps_enabled,
                    inner_angle: Some(l.inner_angle),
                    outer_angle: Some(l.outer_angle),
                })
        });

    let behaviors = params
        .behaviors_q
        .get(entity)
        .ok()
        .map(|eb| {
            eb.behaviors
                .iter()
                .map(|bi| protocol::BehaviorSection {
                    id: bi.id.clone(),
                    behavior_type: format!("{:?}", bi.def)
                        .split('{')
                        .next()
                        .unwrap_or("unknown")
                        .trim()
                        .to_string(),
                    base_position: bi.base_position.into(),
                    base_scale: bi.base_scale.into(),
                })
                .collect()
        })
        .unwrap_or_default();

    let mesh_asset = params.gltf_sources.get(entity).ok().map(|g| g.path.clone());

    let parent = params
        .parent_q
        .get(entity)
        .ok()
        .and_then(|p| registry.get_name(p.parent()))
        .map(|n| n.to_string());

    let children: Vec<String> = params
        .children_q
        .get(entity)
        .map(|ch| {
            ch.iter()
                .filter_map(|c| registry.get_name(c).map(|n| n.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Some(protocol::EntityDetailData {
        identity: protocol::IdentitySection {
            name: name.to_string(),
            id: gen_e.world_id.0,
            entity_type: format!("{:?}", gen_e.entity_type),
        },
        transform,
        shape,
        material,
        light,
        behaviors,
        audio: params.audio_engine.as_ref().and_then(|engine| {
            // Find emitter attached to this entity by name
            let name_str = name.to_string();
            engine
                .emitter_meta
                .values()
                .find(|meta| meta.attached_to.as_deref() == Some(&name_str))
                .map(|meta| protocol::AudioSection {
                    sound_type: meta.sound_type.clone(),
                    volume: meta.base_volume,
                    radius: meta.radius,
                    attached_to: meta.attached_to.clone(),
                    position: meta.position,
                })
        }),
        mesh_asset,
        hierarchy: protocol::HierarchySection { parent, children },
    })
}

/// Build world info data for the WS protocol.
pub fn build_world_info(
    registry: &NameRegistry,
    params: &InspectorQueries,
) -> protocol::WorldInfoData {
    protocol::WorldInfoData {
        name: params.current_world.name.clone(),
        entity_count: registry.len(),
        behavior_state: protocol::BehaviorStateInfo {
            paused: params.behavior_state.paused,
            elapsed: params.behavior_state.elapsed,
        },
        audio: params
            .audio_engine
            .as_ref()
            .map(|engine| protocol::AudioStateInfo {
                active: engine.active,
                emitter_count: engine.emitter_meta.len(),
                ambience_layers: engine.ambience_layer_names.clone(),
            }),
    }
}

// ---------------------------------------------------------------------------
// Camera focus system
// ---------------------------------------------------------------------------

/// When a PendingFocusEntity is set, teleport the camera to look at the target entity.
fn focus_camera_system(
    mut pending: ResMut<PendingFocusEntity>,
    mut pose: ResMut<PendingCameraPose>,
    mut camera_q: Query<&mut Transform, With<crate::gen3d::plugin::FlyCam>>,
    target_transforms: Query<&Transform, Without<crate::gen3d::plugin::FlyCam>>,
) {
    // A bookmark: fly to the pose as it was saved.
    if let Some((position, look_at)) = pose.pose.take()
        && let Ok(mut camera_transform) = camera_q.single_mut()
    {
        *camera_transform = Transform::from_translation(position).looking_at(look_at, Vec3::Y);
    }
    let Some(target_entity) = pending.target.take() else {
        return;
    };

    let Ok(target_transform) = target_transforms.get(target_entity) else {
        return;
    };

    let Ok(mut camera_transform) = camera_q.single_mut() else {
        return;
    };

    // Position camera at a reasonable distance from the target, looking at it
    let target_pos = target_transform.translation;
    let offset = Vec3::new(3.0, 2.0, 3.0); // Slightly above and to the side
    let camera_pos = target_pos + offset;

    *camera_transform = Transform::from_translation(camera_pos).looking_at(target_pos, Vec3::Y);
}

// ---------------------------------------------------------------------------
// 3D Viewport picking
// ---------------------------------------------------------------------------

/// Observer: when a mesh is clicked in the 3D viewport, select the GenEntity.
/// Walks up the parent chain to find the nearest GenEntity ancestor. A
/// shift-click adds to the selection. The live canvas selects on its own
/// (`canvas_edit`), so this stands down when it is there. A click on a
/// panel never gets here: the page takes the pointer everywhere but the
/// viewport.
fn on_mesh_clicked(
    on: On<Pointer<Click>>,
    gen_entities: Query<&GenEntity>,
    parent_q: Query<&ChildOf>,
    keys: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<InspectorSelection>,
    canvas: Option<Res<crate::gen3d::canvas_edit::CanvasGesture>>,
) {
    if canvas.is_some() {
        return;
    }

    // Only left click
    if on.event.button != PointerButton::Primary {
        return;
    }
    let extend = keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);

    // Walk up parent chain to find the GenEntity
    let mut entity = on.entity;
    loop {
        if gen_entities.contains(entity) {
            if extend {
                selection.toggle(entity);
            } else {
                selection.set(Some(entity));
            }
            return;
        }
        if let Ok(child_of) = parent_q.get(entity) {
            entity = child_of.parent();
        } else {
            break;
        }
    }
}
