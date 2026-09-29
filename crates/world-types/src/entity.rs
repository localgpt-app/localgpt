//! WorldEntity — the composable entity definition.
//!
//! Instead of separate disconnected systems for geometry, audio, lights,
//! and behaviors, one entity can have **any combination** of component slots.

use serde::{Deserialize, Serialize};

use crate::asset::MeshAssetRef;
use crate::audio::AudioDef;
use crate::behavior::BehaviorDef;
use crate::identity::{CreationId, EntityId, EntityName};
use crate::instance::InstanceOf;
use crate::light::LightDef;
use crate::material::MaterialDef;
use crate::modulation::ModulationDef;
use crate::shape::Shape;
use crate::spatial::ChunkCoord;
use crate::trigger::TriggerDef;

/// Transform in world space (or parent-relative if parented).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorldTransform {
    /// Position [x, y, z].
    #[serde(default)]
    pub position: [f32; 3],
    /// Euler rotation in degrees [pitch, yaw, roll].
    #[serde(default)]
    pub rotation_degrees: [f32; 3],
    /// Scale [x, y, z].
    #[serde(default = "default_scale")]
    pub scale: [f32; 3],
    /// Visibility flag.
    #[serde(default = "default_true")]
    pub visible: bool,
}

impl Default for WorldTransform {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation_degrees: [0.0, 0.0, 0.0],
            scale: default_scale(),
            visible: true,
        }
    }
}

fn default_scale() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

fn default_true() -> bool {
    true
}

/// A single entity in the world.  Component slots are all optional —
/// any combination is valid (e.g., a glowing orb has shape + light + audio).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct WorldEntity {
    /// Stable numeric identifier.
    pub id: EntityId,
    /// Human-readable name.
    pub name: EntityName,
    /// Spatial transform.
    #[serde(default)]
    pub transform: WorldTransform,
    /// Parent entity (for hierarchy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<EntityId>,
    /// Spatial chunk assignment (for large worlds).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk: Option<ChunkCoord>,
    /// If this entity belongs to a compound creation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_id: Option<CreationId>,

    // ---- Component slots (all optional) ----
    /// Parametric shape — never loses dimension info.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
    /// PBR material properties.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<MaterialDef>,
    /// Light source — can coexist with shape (e.g., glowing orb).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<LightDef>,
    /// Behaviors stack — multiple can be active simultaneously.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub behaviors: Vec<BehaviorDef>,
    /// Audio source — spatial or ambient.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioDef>,
    /// Reference to an imported mesh asset (alternative to Shape).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_asset: Option<MeshAssetRef>,
    /// Signal-driven modulations (soundtrack energy, beat, oscillators)
    /// stacked on top of the authored values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modulations: Vec<ModulationDef>,
    /// Places a copy of a reusable creation here, with per-part overrides
    /// (see [`crate::instance`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_of: Option<InstanceOf>,
    /// Events and the actions they run (see [`crate::trigger`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggers: Vec<TriggerDef>,
}

impl WorldEntity {
    /// Create a minimal entity with just an ID and name.
    pub fn new(id: u64, name: impl Into<String>) -> Self {
        Self {
            id: EntityId(id),
            name: EntityName::new(name),
            transform: WorldTransform::default(),
            parent: None,
            chunk: None,
            creation_id: None,
            shape: None,
            material: None,
            light: None,
            behaviors: Vec::new(),
            audio: None,
            mesh_asset: None,
            modulations: Vec::new(),
            instance_of: None,
            triggers: Vec::new(),
        }
    }

    /// Builder: set shape.
    pub fn with_shape(mut self, shape: Shape) -> Self {
        self.shape = Some(shape);
        self
    }

    /// Builder: set material.
    pub fn with_material(mut self, material: MaterialDef) -> Self {
        self.material = Some(material);
        self
    }

    /// Builder: set light.
    pub fn with_light(mut self, light: LightDef) -> Self {
        self.light = Some(light);
        self
    }

    /// Builder: add a signal-driven modulation.
    pub fn with_modulation(mut self, modulation: ModulationDef) -> Self {
        self.modulations.push(modulation);
        self
    }

    /// Builder: add a behavior.
    pub fn with_behavior(mut self, behavior: BehaviorDef) -> Self {
        self.behaviors.push(behavior);
        self
    }

    /// Builder: set audio.
    pub fn with_audio(mut self, audio: AudioDef) -> Self {
        self.audio = Some(audio);
        self
    }

    /// Builder: set position.
    pub fn at(mut self, position: [f32; 3]) -> Self {
        self.transform.position = position;
        self
    }
}

/// Patch for modifying an existing entity.
///
/// Uses `Option<Option<T>>` semantics:
/// - `None` — field not changed
/// - `Some(None)` — field removed/cleared
/// - `Some(Some(v))` — field set to `v`
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EntityPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<EntityName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transform: Option<WorldTransform>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Option<EntityId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<Option<Shape>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<Option<MaterialDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<Option<LightDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub behaviors: Option<Vec<BehaviorDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<Option<AudioDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_asset: Option<Option<MeshAssetRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modulations: Option<Vec<ModulationDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_of: Option<Option<InstanceOf>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggers: Option<Vec<TriggerDef>>,
}

impl EntityPatch {
    /// The patch that turns `base` into `target`, field by field, leaving
    /// out `name` and `parent` (a part's place comes from its definition).
    /// Numbers within `1e-4` count as equal, so values that went through
    /// an engine's float conversions (quaternions and back, sRGB and back)
    /// don't show up as changes.
    pub fn between(base: &WorldEntity, target: &WorldEntity) -> EntityPatch {
        fn differs<T: Serialize>(a: &T, b: &T) -> bool {
            match (serde_json::to_value(a), serde_json::to_value(b)) {
                (Ok(a), Ok(b)) => !json_close(&a, &b),
                _ => true,
            }
        }
        fn changed<T: Serialize + Clone>(a: &T, b: &T) -> Option<T> {
            differs(a, b).then(|| b.clone())
        }
        EntityPatch {
            name: None,
            parent: None,
            transform: changed(&base.transform, &target.transform),
            shape: changed(&base.shape, &target.shape),
            material: changed(&base.material, &target.material),
            light: changed(&base.light, &target.light),
            behaviors: changed(&base.behaviors, &target.behaviors),
            audio: changed(&base.audio, &target.audio),
            mesh_asset: changed(&base.mesh_asset, &target.mesh_asset),
            modulations: changed(&base.modulations, &target.modulations),
            instance_of: changed(&base.instance_of, &target.instance_of),
            triggers: changed(&base.triggers, &target.triggers),
        }
    }

    /// Whether the patch changes nothing.
    pub fn is_empty(&self) -> bool {
        *self == EntityPatch::default()
    }

    /// Apply this patch to a WorldEntity, modifying it in place.
    pub fn apply(&self, entity: &mut WorldEntity) {
        if let Some(ref name) = self.name {
            entity.name = name.clone();
        }
        if let Some(ref transform) = self.transform {
            entity.transform = transform.clone();
        }
        if let Some(ref parent) = self.parent {
            entity.parent = *parent;
        }
        if let Some(ref shape) = self.shape {
            entity.shape = shape.clone();
        }
        if let Some(ref material) = self.material {
            entity.material = material.clone();
        }
        if let Some(ref light) = self.light {
            entity.light = light.clone();
        }
        if let Some(ref behaviors) = self.behaviors {
            entity.behaviors = behaviors.clone();
        }
        if let Some(ref audio) = self.audio {
            entity.audio = audio.clone();
        }
        if let Some(ref mesh_asset) = self.mesh_asset {
            entity.mesh_asset = mesh_asset.clone();
        }
        if let Some(ref modulations) = self.modulations {
            entity.modulations = modulations.clone();
        }
        if let Some(ref instance_of) = self.instance_of {
            entity.instance_of = instance_of.clone();
        }
        if let Some(ref triggers) = self.triggers {
            entity.triggers = triggers.clone();
        }
    }
}

/// JSON equality with numbers compared to within `1e-4` (relative for large
/// values).
fn json_close(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() <= 1e-4 * x.abs().max(y.abs()).max(1.0),
            _ => x == y,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| json_close(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_close(v, w)))
        }
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{AudioDef, AudioKind, AudioSource, Rolloff};

    #[test]
    fn patch_between_ignores_float_noise() {
        let base = WorldEntity::new(1, "trunk")
            .with_shape(Shape::Cylinder {
                radius: 0.3,
                height: 3.0,
            })
            .at([0.0, 1.5, 0.0]);
        let mut target = base.clone();
        target.name = EntityName::new("oak_1/trunk");
        target.parent = Some(EntityId(9));
        target.transform.position[1] = 1.500_001;
        target.transform.rotation_degrees[1] = -0.000_02;
        assert!(EntityPatch::between(&base, &target).is_empty());

        target.shape = Some(Shape::Cylinder {
            radius: 0.3,
            height: 4.5,
        });
        let patch = EntityPatch::between(&base, &target);
        assert_eq!(patch.shape, Some(target.shape.clone()));
        assert!(patch.transform.is_none() && patch.name.is_none());
        let mut rebuilt = base.clone();
        patch.apply(&mut rebuilt);
        assert_eq!(rebuilt.shape, target.shape);
    }

    #[test]
    fn entity_builder() {
        let e = WorldEntity::new(1, "campfire")
            .at([5.0, 0.0, 3.0])
            .with_shape(Shape::Cone {
                radius: 0.5,
                height: 1.0,
            })
            .with_light(LightDef {
                light_type: crate::light::LightType::Point,
                color: [1.0, 0.8, 0.3, 1.0],
                intensity: 500.0,
                direction: None,
                shadows: true,
                range: None,
                outer_angle: None,
                inner_angle: None,
            })
            .with_audio(AudioDef {
                kind: AudioKind::Sfx,
                source: AudioSource::Fire {
                    intensity: 0.8,
                    crackle: 0.5,
                },
                volume: 0.7,
                radius: Some(15.0),
                rolloff: Rolloff::InverseSquare,
            });

        assert_eq!(e.name.as_str(), "campfire");
        assert!(e.shape.is_some());
        assert!(e.light.is_some());
        assert!(e.audio.is_some());
        assert_eq!(e.transform.position, [5.0, 0.0, 3.0]);
    }

    #[test]
    fn entity_roundtrip() {
        let e = WorldEntity::new(42, "test_cube").with_shape(Shape::Cuboid {
            x: 2.0,
            y: 3.0,
            z: 4.0,
        });
        let json = serde_json::to_string_pretty(&e).unwrap();
        let back: WorldEntity = serde_json::from_str(&json).unwrap();
        assert_eq!(e.id, back.id);
        assert_eq!(e.name, back.name);
        assert_eq!(e.shape, back.shape);
    }

    #[test]
    fn entity_patch_apply() {
        let mut e = WorldEntity::new(1, "box");
        let patch = EntityPatch {
            name: Some(EntityName::new("renamed_box")),
            shape: Some(Some(Shape::Sphere { radius: 2.0 })),
            ..Default::default()
        };
        patch.apply(&mut e);
        assert_eq!(e.name.as_str(), "renamed_box");
        assert!(matches!(e.shape, Some(Shape::Sphere { radius }) if radius == 2.0));
    }

    #[test]
    fn entity_patch_clear_field() {
        let mut e = WorldEntity::new(1, "lit_box").with_light(LightDef::default());
        assert!(e.light.is_some());

        let patch = EntityPatch {
            light: Some(None), // Clear the light
            ..Default::default()
        };
        patch.apply(&mut e);
        assert!(e.light.is_none());
    }
}
