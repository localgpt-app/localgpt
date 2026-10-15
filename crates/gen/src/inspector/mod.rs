//! World Inspector Panel — egui overlay for inspecting 100% of world state.
//!
//! F1 cycles: Hidden → OutlinerOnly → Full → Hidden.
//! Outliner (left) shows the entity tree. Detail (right) shows all components.
//! World Info (bottom) shows global state.

pub mod detail;
pub mod outliner;
pub mod protocol;
pub mod selection;
pub mod world_info;
pub mod ws_server;

use bevy::ecs::system::SystemParam;
use bevy::picking::prelude::*;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass};

use localgpt_world_types as wt;

use crate::gen3d::audio::AudioEngine;
use crate::gen3d::behaviors::{BehaviorState, EntityBehaviors};
use crate::gen3d::plugin::CurrentWorld;
use crate::gen3d::registry::*;

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

/// Inspector visibility mode.
#[derive(Default, PartialEq, Eq, Clone, Copy, Debug)]
pub enum InspectorMode {
    #[default]
    Hidden,
    OutlinerOnly,
    Full,
}

/// Current inspector state.
#[derive(Resource, Default)]
pub struct InspectorState {
    pub mode: InspectorMode,
}

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

/// Edits the panels ask for — field patches, reparenting, bookmarks —
/// drained into the live canvas's intent stream by `canvas_edit`. Without
/// a live world nothing drains them, and the panels only write when one is
/// open.
#[derive(Resource, Default)]
pub struct PanelIntents(pub Vec<crate::gen3d::canvas_edit::PersonIntent>);

/// When set, the camera flies to this pose: a bookmark, or a shot.
#[derive(Resource, Default)]
pub struct PendingCameraPose {
    pub pose: Option<(Vec3, Vec3)>,
}

/// Marker resource: present when the mouse is over an inspector panel.
/// Camera systems should check `not(resource_exists::<UiHovered>)`.
#[derive(Resource)]
pub struct UiHovered;

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

/// The scene as the panels read it — and, while a field is being dragged,
/// write it, so the person sees the value before the document takes it.
#[derive(SystemParam)]
pub(crate) struct InspectorQueries<'w, 's> {
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
    pub fly_cam: Query<'w, 's, Entity, With<crate::gen3d::plugin::FlyCam>>,
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
        app.add_plugins(EguiPlugin::default())
            // NOTE: do NOT enable enable_absorb_bevy_input_system — it clears
            // ButtonInput<KeyCode> when egui has focus, killing WASD player controls.
            // bevy_egui's default `picking` feature handles pointer blocking via PointerHits.
            .init_resource::<InspectorState>()
            .init_resource::<InspectorSelection>()
            .init_resource::<outliner::OutlinerCache>()
            .init_resource::<WsBridgeState>()
            .init_resource::<PendingFocusEntity>()
            .init_resource::<PendingCameraPose>()
            .init_resource::<PanelIntents>()
            .init_resource::<detail::PanelEdit>()
            .add_systems(Startup, inspector_from_env)
            .add_systems(
                Update,
                (
                    inspector_toggle,
                    selection::highlight_selected,
                    ws_bridge_system,
                    focus_camera_system,
                )
                    .chain(),
            )
            // The panels draw inside egui's pass, which bevy_egui runs in
            // its own schedule (multi-pass mode, its default): a context
            // panel drawn from `Update` finds no pass begun and panics.
            .add_systems(EguiPrimaryContextPass, inspector_ui)
            // 3D viewport click-to-select via Bevy's built-in mesh picking
            .add_observer(on_mesh_clicked);
    }
}

/// Run condition: true when mouse is NOT over an inspector panel.
pub fn not_ui_hovered(hovered: Option<Res<UiHovered>>) -> bool {
    hovered.is_none()
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// `LOCALGPT_INSPECTOR=outliner|full` opens the inspector at startup, for a
/// canvas driven from outside — a smoke test over the live API, a demo.
fn inspector_from_env(mut state: ResMut<InspectorState>) {
    let mode = match std::env::var("LOCALGPT_INSPECTOR").as_deref() {
        Ok("outliner") => InspectorMode::OutlinerOnly,
        Ok("full") => InspectorMode::Full,
        _ => return,
    };
    state.mode = mode;
}

/// F1 cycles through inspector modes. Escape deselects the current entity.
fn inspector_toggle(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<InspectorState>,
    mut selection: ResMut<InspectorSelection>,
) {
    if keys.just_pressed(KeyCode::F1) {
        state.mode = match state.mode {
            InspectorMode::Hidden => InspectorMode::OutlinerOnly,
            InspectorMode::OutlinerOnly => InspectorMode::Full,
            InspectorMode::Full => InspectorMode::Hidden,
        };
    }

    // Escape clears selection (only when inspector is visible)
    if keys.just_pressed(KeyCode::Escape)
        && state.mode != InspectorMode::Hidden
        && !selection.is_empty()
    {
        selection.clear();
    }
}

/// Main inspector UI system — draws all egui panels and updates UiHovered.
///
/// With a live world open (`--live`), the panels edit: the detail panel's
/// fields, the outliner's drag-to-reparent and context menu, and the
/// world bar's snap, review and bookmark controls all write intents that
/// the canvas commits through the authority.
#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn inspector_ui(
    mut commands: Commands,
    mut contexts: EguiContexts,
    state: Res<InspectorState>,
    mut selection: ResMut<InspectorSelection>,
    registry: Res<NameRegistry>,
    mut outliner_cache: ResMut<outliner::OutlinerCache>,
    mut params: InspectorQueries,
    mut live: Option<ResMut<crate::gen3d::live::Live>>,
    mut settings: Option<ResMut<crate::gen3d::canvas_edit::CanvasSettings>>,
    mut panel_intents: ResMut<PanelIntents>,
    mut panel_edit: ResMut<detail::PanelEdit>,
    mut pose: ResMut<PendingCameraPose>,
) {
    if state.mode == InspectorMode::Hidden {
        commands.remove_resource::<UiHovered>();
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    // --- Outliner (left panel) ---
    {
        let edit = live.as_deref().map(|live| outliner::OutlinerEdit {
            intents: &mut panel_intents.0,
            doc: live.doc(),
            q: &params,
        });
        outliner::draw_outliner(
            ctx,
            &registry,
            &mut selection,
            &mut outliner_cache,
            &params.gen_entities,
            &params.visibility_q,
            &params.children_q,
            &params.parent_q,
            edit,
        );
    }

    if state.mode == InspectorMode::Full {
        // --- Detail panel (right) ---
        {
            let edit = live.as_deref().map(|live| detail::EditSurface {
                doc: live.doc(),
                intents: &mut panel_intents.0,
                edit: &mut panel_edit,
                status: live.status(),
            });
            detail::draw_detail(ctx, &mut selection, &registry, &mut params, edit);
        }

        // --- World info bar (bottom) ---
        let camera = params
            .fly_cam
            .single()
            .ok()
            .and_then(|cam| params.transforms.get(cam).ok().copied());
        let controls = live.as_deref_mut().map(|live| world_info::LiveControls {
            live,
            settings: settings.as_deref_mut(),
            intents: &mut panel_intents.0,
            camera,
            pose: &mut pose,
        });
        world_info::draw_world_info(
            ctx,
            &registry,
            &params.behavior_state,
            params.audio_engine.as_deref(),
            &params.current_world,
            controls,
        );
    }

    // --- Process pending visibility toggles from outliner ---
    let pending: Vec<Entity> = std::mem::take(&mut outliner_cache.pending_visibility_toggles);
    for entity in &pending {
        if let Ok(vis) = params.visibility_q.get(*entity) {
            let new_vis = if *vis == Visibility::Hidden {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            commands.entity(*entity).insert(new_vis);
        }
    }
    if !pending.is_empty() {
        // Force tree rebuild to update visibility state
        outliner_cache.last_entity_count = usize::MAX;
    }

    // --- Process pending focus request from outliner double-click ---
    if let Some(focus_entity) = outliner_cache.pending_focus.take() {
        commands.insert_resource(PendingFocusEntity {
            target: Some(focus_entity),
        });
    }

    // --- Update UiHovered resource ---
    if ctx.is_pointer_over_egui() {
        commands.insert_resource(UiHovered);
    } else {
        commands.remove_resource::<UiHovered>();
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
    params: InspectorQueries,
    mut outliner_cache: ResMut<outliner::OutlinerCache>,
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
                    if let Some(entity) = registry.get_entity_by_id(&wt::EntityId(entity_id)) {
                        outliner_cache.pending_visibility_toggles.push(entity);
                    }
                }
                protocol::ClientMessage::FocusEntity { entity_id } => {
                    if let Some(entity) = registry.get_entity_by_id(&wt::EntityId(entity_id)) {
                        outliner_cache.pending_focus = Some(entity);
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

/// Build the scene tree for the WS protocol.
fn build_scene_tree(
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

/// Build entity detail data for the WS protocol.
fn build_entity_detail(
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
fn build_world_info(registry: &NameRegistry, params: &InspectorQueries) -> protocol::WorldInfoData {
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
/// (`canvas_edit`), so this stands down when it is there.
#[allow(clippy::too_many_arguments)] // a Bevy observer; the repo's convention
fn on_mesh_clicked(
    on: On<Pointer<Click>>,
    gen_entities: Query<&GenEntity>,
    parent_q: Query<&ChildOf>,
    state: Res<InspectorState>,
    keys: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<InspectorSelection>,
    hovered: Option<Res<UiHovered>>,
    canvas: Option<Res<crate::gen3d::canvas_edit::CanvasGesture>>,
) {
    // Only when inspector is visible and pointer is not over UI
    if state.mode == InspectorMode::Hidden || hovered.is_some() || canvas.is_some() {
        return;
    }

    // Only left click
    if on.event.button != PointerButton::Primary {
        return;
    }
    let extend = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);

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
