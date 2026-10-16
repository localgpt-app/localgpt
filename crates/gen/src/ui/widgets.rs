//! Named `bevy_ui` nodes, for a driver with no eyes.
//!
//! The smoke harness drives the panels by name through `GET /widgets`,
//! which until now knew only egui's response rectangles
//! (`inspector::PanelRects`). A `bevy_ui` node that carries a [`Widget`] is
//! reported the same way — `[x, y, w, h]` in window points — so a surface
//! can move from egui to `bevy_ui` without a check losing its handle on it.
//! The history rail's rows and the feathers world bar are the first.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::ui::{ComputedNode, UiGlobalTransform, UiSystems};

/// The name a node answers to in `GET /widgets` — dotted, as the egui
/// widgets are named (`panel.world_info`, `world_info.snap`, `rail.row.r3`).
#[derive(Component, Debug, Clone)]
pub struct Widget(pub String);

impl Widget {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }
}

/// Every visible named node's rectangle this frame, in window points.
#[derive(Resource, Default, Debug)]
pub struct UiWidgetRects(pub HashMap<String, [f32; 4]>);

pub struct WidgetSeamPlugin;

impl Plugin for WidgetSeamPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiWidgetRects>()
            // After layout, so the rectangles are this frame's.
            .add_systems(PostUpdate, note_widgets.after(UiSystems::Layout));
    }
}

fn note_widgets(
    mut rects: ResMut<UiWidgetRects>,
    nodes: Query<(
        &Widget,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
) {
    rects.0.clear();
    for (widget, node, transform, visibility) in &nodes {
        if !visibility.get() {
            continue;
        }
        if let Some(rect) = rect_of(node, transform) {
            rects.0.insert(widget.0.clone(), rect);
        }
    }
}

/// A node's rectangle in window points. The layout works in physical
/// pixels and keeps the node's centre as its transform; a node laid out to
/// nothing (`Display::None`, an empty row) has no rectangle.
pub fn rect_of(node: &ComputedNode, transform: &UiGlobalTransform) -> Option<[f32; 4]> {
    let scale = node.inverse_scale_factor();
    let size = node.size() * scale;
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let center = transform.translation * scale;
    Some([
        center.x - size.x / 2.0,
        center.y - size.y / 2.0,
        size.x,
        size.y,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_rectangle_is_in_window_points_from_its_centre() {
        // A 200×60 physical node centred at (300, 100) on a 2× display.
        let node = ComputedNode {
            size: Vec2::new(200.0, 60.0),
            inverse_scale_factor: 0.5,
            ..Default::default()
        };
        let transform = UiGlobalTransform::from_translation(Vec2::new(300.0, 100.0));
        assert_eq!(rect_of(&node, &transform), Some([100.0, 35.0, 100.0, 30.0]));
    }

    #[test]
    fn a_node_laid_out_to_nothing_has_no_rectangle() {
        let node = ComputedNode {
            size: Vec2::ZERO,
            ..Default::default()
        };
        assert_eq!(rect_of(&node, &UiGlobalTransform::default()), None);
    }
}
