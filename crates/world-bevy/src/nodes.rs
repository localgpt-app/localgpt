//! Node overrides on imported meshes (`MeshAssetRef::node_overrides`).
//!
//! Put [`NodeOverrides`] on the entity a glTF scene is spawned under; once
//! the scene's named nodes exist, each override hides or shows its node and
//! recolours the materials under it, then drops out. Materials are cloned,
//! so other placements of the same file keep theirs. The web viewer's
//! `applyNodeOverrides` does the same.

use bevy::prelude::*;
use localgpt_world_types as wt;

/// Overrides still waiting for their nodes to appear under this entity.
#[derive(Component, Debug, Clone, Default)]
pub struct NodeOverrides(pub Vec<wt::NodeOverride>);

/// Applies [`NodeOverrides`] as glTF scenes finish spawning.
pub struct NodeOverridesPlugin;

impl Plugin for NodeOverridesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, apply_node_overrides);
    }
}

/// Apply every override whose node has spawned; remove the component once
/// all have.
pub fn apply_node_overrides(
    mut commands: Commands,
    mut roots: Query<(Entity, &mut NodeOverrides)>,
    children: Query<&Children>,
    names: Query<&Name>,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (root, mut pending) in &mut roots {
        pending.0.retain(|o| {
            let Some(node) = children
                .iter_descendants(root)
                .find(|e| names.get(*e).is_ok_and(|n| n.as_str() == o.node))
            else {
                return true;
            };
            if let Some(visible) = o.visible {
                commands.entity(node).insert(if visible {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
            if let Some(color) = o.color {
                for e in std::iter::once(node).chain(children.iter_descendants(node)) {
                    let Ok(handle) = mesh_materials.get(e) else {
                        continue;
                    };
                    let Some(mut material) = materials.get(&handle.0).cloned() else {
                        continue;
                    };
                    material.base_color = crate::srgba(color);
                    commands
                        .entity(e)
                        .insert(MeshMaterial3d(materials.add(material)));
                }
            }
            false
        });
        if pending.0.is_empty() {
            commands.entity(root).remove::<NodeOverrides>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_apply_once_their_node_exists() {
        let mut app = App::new();
        app.init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, apply_node_overrides);
        let red = [0.8, 0.1, 0.1, 1.0];
        let root = app
            .world_mut()
            .spawn(NodeOverrides(vec![
                wt::NodeOverride {
                    node: "leaves".into(),
                    visible: None,
                    color: Some(red),
                },
                wt::NodeOverride {
                    node: "branch".into(),
                    visible: Some(false),
                    color: None,
                },
            ]))
            .id();

        // Nothing spawned yet: both wait.
        app.update();
        assert_eq!(app.world().get::<NodeOverrides>(root).unwrap().0.len(), 2);

        let base = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let leaves = app.world_mut().spawn(Name::new("leaves")).id();
        let mesh = app.world_mut().spawn(MeshMaterial3d(base.clone())).id();
        app.world_mut().entity_mut(leaves).add_child(mesh);
        app.world_mut().entity_mut(root).add_child(leaves);
        app.update();
        assert_eq!(app.world().get::<NodeOverrides>(root).unwrap().0.len(), 1);
        let handle = &app
            .world()
            .get::<MeshMaterial3d<StandardMaterial>>(mesh)
            .unwrap()
            .0;
        assert_ne!(handle, &base);
        let materials = app.world().resource::<Assets<StandardMaterial>>();
        assert_eq!(materials.get(handle).unwrap().base_color, crate::srgba(red));

        let branch = app.world_mut().spawn(Name::new("branch")).id();
        app.world_mut().entity_mut(root).add_child(branch);
        app.update();
        assert!(app.world().get::<NodeOverrides>(root).is_none());
        assert_eq!(
            app.world().get::<Visibility>(branch),
            Some(&Visibility::Hidden)
        );
    }
}
