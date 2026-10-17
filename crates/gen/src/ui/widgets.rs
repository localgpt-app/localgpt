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
use bevy::ui::{CalculatedClip, ComputedNode, UiGlobalTransform, UiSystems};

/// The name a node answers to in `GET /widgets` — dotted, as the egui
/// widgets are named (`panel.world_info`, `world_info.snap`, `rail.row.r3`).
#[derive(Component, Debug, Clone, Default)]
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
        Option<&CalculatedClip>,
    )>,
) {
    rects.0.clear();
    for (widget, node, transform, visibility, clip) in &nodes {
        if !visibility.get() {
            continue;
        }
        // Inside a scrolling list, only the part a person can see (and
        // click) counts; a row scrolled out of view is not on screen.
        if let Some(mut rect) = rect_of(node, transform) {
            if let Some(clip) = clip {
                let scale = node.inverse_scale_factor();
                let (cx0, cy0) = (clip.clip.min.x * scale, clip.clip.min.y * scale);
                let (cx1, cy1) = (clip.clip.max.x * scale, clip.clip.max.y * scale);
                let (x0, y0) = (rect[0].max(cx0), rect[1].max(cy0));
                let (x1, y1) = ((rect[0] + rect[2]).min(cx1), (rect[1] + rect[3]).min(cy1));
                if x1 <= x0 || y1 <= y0 {
                    continue;
                }
                rect = [x0, y0, x1 - x0, y1 - y0];
            }
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
