//! Selection highlight — an emissive glow on every selected entity.
//!
//! The glow is written into the entity's material, so the material's own
//! emissive is kept aside to come back on deselect. The material can be
//! replaced under the glow — a commit that recolors the entity hands it a
//! new material — so the kept value names the material it came from: a
//! stale one is never written back over a newer material, and the newer
//! material gets its own glow, keeping its own emissive aside.

use bevy::prelude::*;

use super::InspectorSelection;

/// The material's own emissive, kept while the highlight is on it.
#[derive(Component)]
pub struct OriginalEmissive {
    pub color: LinearRgba,
    /// The material the color was read from; the glow is only taken off
    /// that one.
    pub material: AssetId<StandardMaterial>,
}

/// Selection highlight color (blue glow).
const HIGHLIGHT_EMISSIVE: LinearRgba = LinearRgba {
    red: 0.2,
    green: 0.4,
    blue: 1.0,
    alpha: 1.0,
};

/// System that manages emissive highlight on the selected entities.
pub fn highlight_selected(
    mut commands: Commands,
    selection: Res<InspectorSelection>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    material_handles: Query<&MeshMaterial3d<StandardMaterial>>,
    mut original_emissives: Query<(Entity, &mut OriginalEmissive)>,
) {
    // Take the glow off whatever left the selection.
    for (entity, original) in original_emissives.iter() {
        if !selection.contains(entity) {
            restore_emissive(
                &mut commands,
                entity,
                original,
                &mut materials,
                &material_handles,
            );
        }
    }

    // Put it on whatever is selected and doesn't glow yet — including an
    // entity whose material was replaced while it was selected.
    for selected in selection.all() {
        let Ok(mat_handle) = material_handles.get(selected) else {
            continue;
        };
        let Some(mut material) = materials.get_mut(&mat_handle.0) else {
            continue;
        };
        match original_emissives.get_mut(selected) {
            Ok((_, mut original)) => {
                if original.material != mat_handle.0.id() {
                    // A new material under the glow: keep its emissive
                    // aside and glow it, as if just selected.
                    original.color = material.emissive;
                    original.material = mat_handle.0.id();
                    material.emissive = HIGHLIGHT_EMISSIVE;
                }
            }
            Err(_) => {
                commands.entity(selected).insert(OriginalEmissive {
                    color: material.emissive,
                    material: mat_handle.0.id(),
                });
                material.emissive = HIGHLIGHT_EMISSIVE;
            }
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
        && mat_handle.0.id() == original.material
        && let Some(mut material) = materials.get_mut(&mat_handle.0)
    {
        material.emissive = original.color;
    }
    commands.entity(entity).remove::<OriginalEmissive>();
}
