//! Entity outliner tree — hierarchical view of all named entities.
//!
//! In a live world the tree edits too: a shift-click adds to the
//! selection, a row dragged onto another row hangs it under that one (and
//! onto the zone at the bottom, at the root), and a right-click offers
//! duplicate, delete, unparent, group and focus. Each becomes an intent
//! the canvas commits through the authority, like a gesture.

use bevy::prelude::*;
use bevy_egui::egui;
use std::collections::HashSet;

use crate::gen3d::canvas_edit::{Named, PersonIntent};
use crate::gen3d::registry::{GenEntity, GenEntityType, NameRegistry};

use super::{InspectorQueries, InspectorSelection, detail};

// ---------------------------------------------------------------------------
// Cached tree data
// ---------------------------------------------------------------------------

/// A single node in the flattened tree (DFS order).
pub struct TreeNode {
    pub entity: Entity,
    pub name: String,
    pub entity_type: GenEntityType,
    pub depth: u32,
    pub has_children: bool,
    pub visible: bool,
}

/// Cached outliner state, rebuilt when entity count changes.
#[derive(Resource)]
pub struct OutlinerCache {
    pub nodes: Vec<TreeNode>,
    pub search_text: String,
    pub collapsed: HashSet<Entity>,
    /// Entities whose visibility should be toggled (processed by inspector_ui).
    pub pending_visibility_toggles: Vec<Entity>,
    /// Entity to focus the camera on (set by double-click, processed by inspector_ui).
    pub pending_focus: Option<Entity>,
    pub(super) last_entity_count: usize,
}

impl Default for OutlinerCache {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            search_text: String::new(),
            collapsed: HashSet::new(),
            pending_visibility_toggles: Vec::new(),
            pending_focus: None,
            last_entity_count: usize::MAX, // Force rebuild on first frame
        }
    }
}

/// Where the outliner's edits go, in a live world.
pub(crate) struct OutlinerEdit<'a, 'w, 's> {
    pub intents: &'a mut Vec<PersonIntent>,
    pub doc: &'a openworldformat::WorldDoc,
    pub q: &'a InspectorQueries<'w, 's>,
}

/// What a row's menu asked for, acted on once the rows are drawn.
enum Action {
    Duplicate(Vec<Entity>),
    Delete(Vec<Entity>),
    Unparent(Entity),
    Group(Vec<Entity>),
    Focus(Entity),
}

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

/// Build a flat DFS-ordered tree from the registry.
fn build_tree(
    registry: &NameRegistry,
    gen_entities: &Query<&GenEntity>,
    visibility_q: &Query<&mut Visibility>,
    children_q: &Query<&Children>,
    parent_q: &Query<&ChildOf>,
) -> Vec<TreeNode> {
    // Collect all gen entities
    let mut all: Vec<(Entity, String, GenEntityType)> = registry
        .all_names()
        .filter_map(|(name, entity)| {
            let gen_e = gen_entities.get(entity).ok()?;
            Some((entity, name.to_string(), gen_e.entity_type))
        })
        .collect();

    // Sort by name for stable ordering
    all.sort_by_key(|a| a.1.to_lowercase());

    // Find root entities (no parent in registry, or parent has no GenEntity)
    let roots: Vec<(Entity, String, GenEntityType)> = all
        .iter()
        .filter(|(e, _, _)| {
            parent_q.get(*e).map_or(true, |p| {
                // Parent exists but is it a Gen entity in the registry?
                gen_entities.get(p.parent()).is_err()
            })
        })
        .cloned()
        .collect();

    let mut nodes = Vec::with_capacity(all.len());

    // Recursive DFS
    #[allow(clippy::too_many_arguments)]
    fn visit(
        entity: Entity,
        name: &str,
        etype: GenEntityType,
        depth: u32,
        nodes: &mut Vec<TreeNode>,
        registry: &NameRegistry,
        gen_entities: &Query<&GenEntity>,
        visibility_q: &Query<&mut Visibility>,
        children_q: &Query<&Children>,
    ) {
        let mut child_entries: Vec<(Entity, String, GenEntityType)> = children_q
            .get(entity)
            .map(|ch| {
                ch.iter()
                    .filter_map(|child| {
                        let gen_e = gen_entities.get(child).ok()?;
                        let child_name = registry.get_name(child)?;
                        Some((child, child_name.to_string(), gen_e.entity_type))
                    })
                    .collect()
            })
            .unwrap_or_default();

        child_entries.sort_by_key(|a| a.1.to_lowercase());

        let visible = visibility_q
            .get(entity)
            .map_or(true, |v| *v != Visibility::Hidden);

        nodes.push(TreeNode {
            entity,
            name: name.to_string(),
            entity_type: etype,
            depth,
            has_children: !child_entries.is_empty(),
            visible,
        });

        for (child_entity, child_name, child_type) in &child_entries {
            visit(
                *child_entity,
                child_name,
                *child_type,
                depth + 1,
                nodes,
                registry,
                gen_entities,
                visibility_q,
                children_q,
            );
        }
    }

    for (entity, name, etype) in &roots {
        visit(
            *entity,
            name,
            *etype,
            0,
            &mut nodes,
            registry,
            gen_entities,
            visibility_q,
            children_q,
        );
    }

    nodes
}

// ---------------------------------------------------------------------------
// Icon helpers
// ---------------------------------------------------------------------------

fn entity_type_icon(etype: GenEntityType) -> &'static str {
    match etype {
        GenEntityType::Primitive => "\u{25A0}",    // ■
        GenEntityType::Light => "\u{2600}",        // ☀
        GenEntityType::Camera => "\u{229E}",       // ⊞
        GenEntityType::Mesh => "\u{25B3}",         // △
        GenEntityType::Group => "\u{25B6}",        // ▶
        GenEntityType::AudioEmitter => "\u{266A}", // ♪
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
// egui 0.34 deprecates `Panel::show(ctx)` in favour of `show_inside(ui)`, but
// bevy_egui only exposes a `Context` (not a root `Ui`), so the context-level
// `show` is still the correct entry point here.
#[allow(deprecated)]
pub(crate) fn draw_outliner(
    ctx: &egui::Context,
    registry: &NameRegistry,
    selection: &mut InspectorSelection,
    cache: &mut OutlinerCache,
    gen_entities: &Query<&GenEntity>,
    visibility_q: &Query<&mut Visibility>,
    children_q: &Query<&Children>,
    parent_q: &Query<&ChildOf>,
    mut edit: Option<OutlinerEdit<'_, '_, '_>>,
) {
    // Rebuild tree when entity count changes
    let current_count = registry.len();
    if current_count != cache.last_entity_count {
        cache.nodes = build_tree(registry, gen_entities, visibility_q, children_q, parent_q);
        cache.last_entity_count = current_count;
    }

    let mut actions: Vec<Action> = Vec::new();
    // Rows dropped onto rows: (dragged, new parent — None for the root).
    let mut drops: Vec<(Entity, Option<Entity>)> = Vec::new();

    egui::Panel::left("inspector_outliner")
        .default_size(250.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("Outliner");
            ui.separator();

            // Search bar
            ui.horizontal(|ui| {
                ui.label("\u{1F50D}"); // 🔍
                ui.text_edit_singleline(&mut cache.search_text);
            });
            ui.separator();

            // Entity tree
            let search = cache.search_text.to_lowercase();
            let has_search = !search.is_empty();
            let live = edit.is_some();
            let extend = ui.input(|i| i.modifiers.shift || i.modifiers.command);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut skip_depth: Option<u32> = None;

                    for node in &cache.nodes {
                        // Skip children of collapsed nodes
                        if let Some(sd) = skip_depth {
                            if node.depth > sd {
                                continue;
                            } else {
                                skip_depth = None;
                            }
                        }

                        // Search filter: show node if name matches
                        if has_search && !node.name.to_lowercase().contains(&search) {
                            continue;
                        }

                        // Check if collapsed
                        let is_collapsed = cache.collapsed.contains(&node.entity);
                        if is_collapsed && node.has_children {
                            skip_depth = Some(node.depth);
                        }

                        let is_selected = selection.contains(node.entity);
                        let is_primary = selection.entity == Some(node.entity);

                        // Indent
                        let indent = node.depth as f32 * 16.0;

                        ui.horizontal(|ui| {
                            ui.add_space(indent);

                            // Expand/collapse toggle for nodes with children
                            if node.has_children {
                                let arrow = if is_collapsed { "\u{25B6}" } else { "\u{25BC}" }; // ▶ / ▼
                                if ui
                                    .selectable_label(false, arrow)
                                    .on_hover_text("Expand/collapse")
                                    .clicked()
                                {
                                    if is_collapsed {
                                        cache.collapsed.remove(&node.entity);
                                    } else {
                                        cache.collapsed.insert(node.entity);
                                    }
                                }
                            } else {
                                ui.add_space(16.0); // Align with expand buttons
                            }

                            // Icon
                            ui.label(entity_type_icon(node.entity_type));

                            // Name (clickable for selection; draggable to reparent)
                            let label = egui::RichText::new(&node.name);
                            let label = if !node.visible {
                                label.strikethrough().weak()
                            } else if is_primary {
                                label.strong()
                            } else {
                                label
                            };

                            let response = ui.selectable_label(is_selected, label);
                            let response = if live {
                                response.interact(egui::Sense::drag())
                            } else {
                                response
                            };
                            if response.double_clicked() {
                                // Double-click: select + focus camera
                                selection.set(Some(node.entity));
                                cache.pending_focus = Some(node.entity);
                            } else if response.clicked() {
                                if extend {
                                    selection.toggle(node.entity);
                                } else {
                                    selection.set(Some(node.entity));
                                }
                            }
                            if live {
                                response.dnd_set_drag_payload(node.entity);
                                if let Some(dropped) = response.dnd_release_payload::<Entity>()
                                    && *dropped != node.entity
                                {
                                    drops.push((*dropped, Some(node.entity)));
                                }
                                response.context_menu(|ui| {
                                    let targets: Vec<Entity> = if selection.contains(node.entity) {
                                        selection.all().collect()
                                    } else {
                                        vec![node.entity]
                                    };
                                    let many = targets.len() > 1;
                                    let label = |verb: &str| {
                                        if many {
                                            format!("{verb} {} selected", targets.len())
                                        } else {
                                            verb.to_string()
                                        }
                                    };
                                    if ui.button(label("Duplicate")).clicked() {
                                        actions.push(Action::Duplicate(targets.clone()));
                                    }
                                    if ui.button(label("Delete")).clicked() {
                                        actions.push(Action::Delete(targets.clone()));
                                    }
                                    if node.depth > 0 && ui.button("Unparent").clicked() {
                                        actions.push(Action::Unparent(node.entity));
                                    }
                                    if many && ui.button(label("Group")).clicked() {
                                        actions.push(Action::Group(targets.clone()));
                                    }
                                    if ui.button("Focus camera").clicked() {
                                        actions.push(Action::Focus(node.entity));
                                    }
                                });
                            }

                            // Visibility toggle (right-aligned)
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let eye = if node.visible {
                                        "\u{1F441}"
                                    } else {
                                        "\u{2014}"
                                    }; // 👁 / —
                                    if ui
                                        .small_button(eye)
                                        .on_hover_text("Toggle visibility")
                                        .clicked()
                                    {
                                        cache.pending_visibility_toggles.push(node.entity);
                                    }
                                },
                            );
                        });
                    }

                    // While a row is being dragged, a place to drop it at
                    // the root.
                    if live && egui::DragAndDrop::has_any_payload(ui.ctx()) {
                        ui.add_space(6.0);
                        let frame = egui::Frame::default()
                            .inner_margin(8.0)
                            .stroke(ui.visuals().widgets.inactive.bg_stroke);
                        let (_, dropped) = ui.dnd_drop_zone::<Entity, ()>(frame, |ui| {
                            ui.weak("drop here to unparent");
                        });
                        if let Some(dropped) = dropped {
                            drops.push((*dropped, None));
                        }
                    }
                });

            ui.separator();
            ui.label(format!("{} entities", current_count));
            if live {
                ui.small("shift-click adds · drag a row onto another to parent it · right-click for more");
            }
        });

    let Some(edit) = edit.as_mut() else {
        return;
    };
    let named = |entity: Entity| -> Option<Named> {
        let id = gen_entities.get(entity).ok()?.world_id.0;
        let name = registry.get_name(entity)?.to_string();
        Some(Named { id, name })
    };

    for (dragged, parent) in drops {
        let Some(child) = named(dragged) else {
            continue;
        };
        // Not under itself: a row can't hang below its own subtree.
        if let Some(parent) = parent
            && let Some(parent_named) = named(parent)
            && edit
                .doc
                .subtree_entities_parent_first(child.id)
                .iter()
                .any(|e| e.id.0 == parent_named.id)
        {
            continue;
        }
        let parent = parent.and_then(|p| {
            let n = named(p)?;
            Some((p, n.id, n.name))
        });
        if let Some(intent) = detail::reparent_intent(
            edit.q,
            dragged,
            child.id,
            &child.name,
            parent
                .as_ref()
                .map(|(e, id, name)| (*e, *id, name.as_str())),
        ) {
            edit.intents.push(intent);
        }
    }

    for action in actions {
        match action {
            Action::Duplicate(targets) => {
                let ids: Vec<u64> = targets.iter().filter_map(|e| Some(named(*e)?.id)).collect();
                if !ids.is_empty() {
                    edit.intents.push(PersonIntent::Duplicate(ids));
                }
            }
            Action::Delete(targets) => {
                let named: Vec<Named> = targets.iter().filter_map(|e| named(*e)).collect();
                if !named.is_empty() {
                    edit.intents.push(PersonIntent::Delete(named));
                }
            }
            Action::Unparent(entity) => {
                if let Some(child) = named(entity)
                    && let Some(intent) =
                        detail::reparent_intent(edit.q, entity, child.id, &child.name, None)
                {
                    edit.intents.push(intent);
                }
            }
            Action::Group(targets) => {
                let named: Vec<Named> = targets.iter().filter_map(|e| named(*e)).collect();
                if named.len() > 1 {
                    edit.intents.push(PersonIntent::Group(named));
                }
            }
            Action::Focus(entity) => cache.pending_focus = Some(entity),
        }
    }
}
