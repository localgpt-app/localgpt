//! Entity outliner tree — hierarchical view of all named entities.

use bevy::prelude::*;
use bevy_egui::egui;
use std::collections::HashSet;

use crate::gen3d::registry::{GenEntity, GenEntityType, NameRegistry};

use super::InspectorSelection;

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

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

/// Build a flat DFS-ordered tree from the registry.
fn build_tree(
    registry: &NameRegistry,
    gen_entities: &Query<&GenEntity>,
    visibility_q: &Query<&Visibility>,
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
        visibility_q: &Query<&Visibility>,
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
pub fn draw_outliner(
    ctx: &egui::Context,
    registry: &NameRegistry,
    selection: &mut InspectorSelection,
    cache: &mut OutlinerCache,
    gen_entities: &Query<&GenEntity>,
    visibility_q: &Query<&Visibility>,
    children_q: &Query<&Children>,
    parent_q: &Query<&ChildOf>,
) {
    // Rebuild tree when entity count changes
    let current_count = registry.len();
    if current_count != cache.last_entity_count {
        cache.nodes = build_tree(registry, gen_entities, visibility_q, children_q, parent_q);
        cache.last_entity_count = current_count;
    }

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

                        let is_selected = selection.entity == Some(node.entity);

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

                            // Name (clickable for selection)
                            let label = egui::RichText::new(&node.name);
                            let label = if !node.visible {
                                label.strikethrough().weak()
                            } else if is_selected {
                                label.strong()
                            } else {
                                label
                            };

                            let response = ui.selectable_label(is_selected, label);
                            if response.double_clicked() {
                                // Double-click: select + focus camera
                                selection.entity = Some(node.entity);
                                cache.pending_focus = Some(node.entity);
                            } else if response.clicked() {
                                selection.entity = Some(node.entity);
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
                });

            ui.separator();
            ui.label(format!("{} entities", current_count));
        });
}
