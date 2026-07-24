//! Selection highlight — emissive override on selected entity.

use bevy::prelude::*;

use super::{InspectorMode, InspectorSelection, InspectorState};

/// Stores the original emissive color so it can be restored on deselect.
#[derive(Component)]
pub struct OriginalEmissive {
    pub color: LinearRgba,
}

/// Selection highlight color (blue glow).
const HIGHLIGHT_EMISSIVE: LinearRgba = LinearRgba {
    red: 0.2,
    green: 0.4,
    blue: 1.0,
    alpha: 1.0,
};

/// System that manages emissive highlight on the selected entity.
pub fn highlight_selected(
    mut commands: Commands,
    state: Res<InspectorState>,
    selection: Res<InspectorSelection>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    material_handles: Query<&MeshMaterial3d<StandardMaterial>>,
    original_emissives: Query<(Entity, &OriginalEmissive)>,
) {
    // If inspector is hidden, clear all highlights
    if state.mode == InspectorMode::Hidden {
        for (entity, original) in original_emissives.iter() {
            restore_emissive(
                &mut commands,
                entity,
                original,
                &mut materials,
                &material_handles,
            );
        }
        return;
    }

    // Only update when selection changes
    if !selection.is_changed() {
        return;
    }

    // Restore previous selection's emissive
    for (entity, original) in original_emissives.iter() {
        if selection.entity != Some(entity) {
            restore_emissive(
                &mut commands,
                entity,
                original,
                &mut materials,
                &material_handles,
            );
        }
    }

    // Apply highlight to new selection
    if let Some(selected) = selection.entity {
        // Skip if already highlighted
        if original_emissives.get(selected).is_ok() {
            return;
        }

        if let Ok(mat_handle) = material_handles.get(selected)
            && let Some(mut material) = materials.get_mut(&mat_handle.0)
        {
            // Store original emissive
            let original_color = material.emissive;
            commands.entity(selected).insert(OriginalEmissive {
                color: original_color,
            });

            // Apply highlight
            material.emissive = HIGHLIGHT_EMISSIVE;
        }
    }
}

fn restore_emissive(
    commands: &mut Commands,
    entity: Entity,
    original: &OriginalEmissive,
    materials: &mut Assets<StandardMaterial>,
    material_handles: &Query<&MeshMaterial3d<StandardMaterial>>,
) {
    if let Ok(mat_handle) = material_handles.get(entity)
        && let Some(mut material) = materials.get_mut(&mat_handle.0)
    {
        material.emissive = original.color;
    }
    commands.entity(entity).remove::<OriginalEmissive>();
}
