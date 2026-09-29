//! The trigger runtime (world-types `trigger`): one implementation of the
//! format's trigger semantics for every Bevy app, mirrored by the web
//! viewer.
//!
//! Put [`Triggers`] on an entity (from its record, [`Triggers::of`]) and
//! [`Visitor`] on whoever walks the world. The app turns its own input into
//! [`Activate`] messages (a click, a use key) and handles the messages the
//! runtime writes: [`TriggerText`] to show, [`HostAction`] for actions the
//! format leaves to the app, and [`TriggerFired`] for anything that follows
//! a trigger. The runtime itself shows, hides, toggles, removes, animates
//! and teleports.
//!
//! What a trigger changes is run-time state: an app that saves the world
//! should keep saving the authored values, not what the triggers did.

use std::collections::HashSet;

use bevy::prelude::*;
use localgpt_world_types as wt;

/// Whoever walks the world: the player when there is one, else the camera.
/// Distances and areas are measured to its position.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Visitor;

/// An entity's triggers and where each one stands.
#[derive(Component, Debug, Clone)]
pub struct Triggers {
    defs: Vec<wt::TriggerDef>,
    areas: Vec<Option<wt::TriggerVolume>>,
    state: Vec<TriggerState>,
}

#[derive(Debug, Clone, Default)]
struct TriggerState {
    done: bool,
    last: Option<f32>,
    inside: bool,
    elapsed: f32,
}

impl Triggers {
    /// The triggers of an entity's record, with its areas resolved
    /// (`None` when it has none).
    pub fn of(entity: &wt::WorldEntity) -> Option<Self> {
        if entity.triggers.is_empty() {
            return None;
        }
        Some(Self {
            areas: entity.triggers.iter().map(|t| t.area(entity)).collect(),
            state: vec![TriggerState::default(); entity.triggers.len()],
            defs: entity.triggers.clone(),
        })
    }

    pub fn defs(&self) -> &[wt::TriggerDef] {
        &self.defs
    }

    /// The largest `max_distance` of the entity's click triggers, if any.
    pub fn click_reach(&self) -> Option<f32> {
        self.defs
            .iter()
            .filter_map(|t| match &t.on {
                wt::TriggerEvent::Click { max_distance, .. } => Some(*max_distance),
                _ => None,
            })
            .reduce(f32::max)
    }
}

/// Items the visitor holds, for `requires_item`. Apps with an inventory
/// keep it current; without one it stays empty and such triggers never fire.
#[derive(Resource, Debug, Clone, Default)]
pub struct HeldItems(pub HashSet<String>);

/// The visitor clicked (or used) `entity`.
#[derive(Message, Debug, Clone, Copy)]
pub struct Activate {
    pub entity: Entity,
}

/// Trigger `index` of `entity` fired.
#[derive(Message, Debug, Clone, Copy)]
pub struct TriggerFired {
    pub entity: Entity,
    pub index: usize,
}

/// Show `text` to the visitor for `seconds` (a `show_text` action).
#[derive(Message, Debug, Clone)]
pub struct TriggerText {
    pub entity: Entity,
    pub text: String,
    pub seconds: f32,
}

/// A `host` action for the app to run.
#[derive(Message, Debug, Clone)]
pub struct HostAction {
    pub entity: Entity,
    pub name: String,
    pub args: serde_json::Map<String, serde_json::Value>,
}

/// A transform animation started by an `animate` action.
#[derive(Component, Debug, Clone)]
pub struct TriggerAnimation {
    property: AnimatedProperty,
    duration: f32,
    elapsed: f32,
}

#[derive(Debug, Clone)]
enum AnimatedProperty {
    Position(Vec3, Vec3),
    Rotation(Quat, Quat),
    Scale(Vec3, Vec3),
}

/// Registers the runtime: messages, [`HeldItems`], and the systems.
pub struct TriggerPlugin;

impl Plugin for TriggerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HeldItems>()
            .add_message::<Activate>()
            .add_message::<TriggerFired>()
            .add_message::<TriggerText>()
            .add_message::<HostAction>()
            .add_systems(Update, (run_triggers, animate).chain());
    }
}

/// An entity with triggers, as the runtime reads it.
type Triggered<'a> = (
    Entity,
    &'a mut Triggers,
    &'a GlobalTransform,
    &'a Transform,
    Option<&'a Visibility>,
);

/// Evaluate every trigger once per frame and run what fires.
#[allow(clippy::too_many_arguments)]
pub fn run_triggers(
    time: Res<Time>,
    held: Res<HeldItems>,
    mut activations: MessageReader<Activate>,
    mut visitor: Query<(&GlobalTransform, &mut Transform), With<Visitor>>,
    mut triggered: Query<Triggered, Without<Visitor>>,
    mut commands: Commands,
    mut fired: MessageWriter<TriggerFired>,
    mut texts: MessageWriter<TriggerText>,
    mut host: MessageWriter<HostAction>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let clicked: HashSet<Entity> = activations.read().map(|a| a.entity).collect();
    let visitor_pos = visitor.single().ok().map(|(g, _)| g.translation());
    let mut teleport_to = None;

    for (entity, mut triggers, global, transform, visibility) in &mut triggered {
        let world_pos = global.translation();
        let to_local = global.affine().inverse();
        let mut visible = visibility != Some(&Visibility::Hidden);
        let Triggers {
            defs, areas, state, ..
        } = &mut *triggers;
        let mut remove_all = false;
        for (index, (def, area)) in defs.iter().zip(areas.iter()).enumerate() {
            let st = &mut state[index];
            let near = |radius: f32| visitor_pos.is_some_and(|v| v.distance(world_pos) <= radius);
            let fires = match &def.on {
                wt::TriggerEvent::Start => {
                    let first = st.last.is_none() && !st.inside;
                    st.inside = true;
                    first
                }
                wt::TriggerEvent::Click { max_distance, .. } => {
                    clicked.contains(&entity)
                        && visitor_pos.is_none_or(|v| v.distance(world_pos) <= *max_distance)
                }
                wt::TriggerEvent::Proximity { radius } => near(*radius),
                wt::TriggerEvent::AreaEnter { .. } | wt::TriggerEvent::AreaExit { .. } => {
                    let inside = match (area, visitor_pos) {
                        (Some(volume), Some(v)) => {
                            volume.contains_local(to_local.transform_point3(v).to_array())
                        }
                        _ => false,
                    };
                    let entering = matches!(def.on, wt::TriggerEvent::AreaEnter { .. });
                    let edge = inside != st.inside && inside == entering;
                    st.inside = inside;
                    edge
                }
                wt::TriggerEvent::Collision { radius } => {
                    let inside = near(*radius);
                    let edge = inside && !st.inside;
                    st.inside = inside;
                    edge
                }
                wt::TriggerEvent::Timer { interval } => {
                    st.elapsed += dt;
                    if *interval > 0.0 && st.elapsed >= *interval {
                        st.elapsed -= *interval;
                        true
                    } else {
                        false
                    }
                }
            };
            if !fires || st.done {
                continue;
            }
            if def
                .requires_item
                .as_ref()
                .is_some_and(|item| !held.0.contains(item))
            {
                continue;
            }
            if st.last.is_some_and(|t| now - t < def.cooldown_secs()) {
                continue;
            }
            st.last = Some(now);
            st.done = def.once;
            fired.write(TriggerFired { entity, index });

            match &def.action {
                wt::TriggerActionDef::ShowText { text, seconds } => {
                    texts.write(TriggerText {
                        entity,
                        text: text.clone(),
                        seconds: *seconds,
                    });
                }
                wt::TriggerActionDef::Show => visible = set_visible(&mut commands, entity, true),
                wt::TriggerActionDef::Hide => visible = set_visible(&mut commands, entity, false),
                wt::TriggerActionDef::Toggle => {
                    visible = set_visible(&mut commands, entity, !visible);
                }
                wt::TriggerActionDef::Remove => {
                    set_visible(&mut commands, entity, false);
                    remove_all = true;
                }
                wt::TriggerActionDef::Animate {
                    property,
                    to,
                    duration,
                } => {
                    if let Some(property) = animated(property, to, transform) {
                        commands.entity(entity).insert(TriggerAnimation {
                            property,
                            duration: duration.max(0.0),
                            elapsed: 0.0,
                        });
                    }
                }
                wt::TriggerActionDef::Teleport { destination } => {
                    teleport_to = Some(Vec3::from_array(*destination));
                }
                wt::TriggerActionDef::Host { name, args } => {
                    host.write(HostAction {
                        entity,
                        name: name.clone(),
                        args: args.clone(),
                    });
                }
            }
        }
        if remove_all {
            // A removed entity is out of the running world: nothing of it fires.
            for st in state.iter_mut() {
                st.done = true;
            }
        }
    }

    if let (Some(destination), Ok((_, mut transform))) = (teleport_to, visitor.single_mut()) {
        transform.translation = destination;
    }
}

fn set_visible(commands: &mut Commands, entity: Entity, visible: bool) -> bool {
    commands.entity(entity).insert(if visible {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    visible
}

/// The animation an `animate` action asks for, from the current transform.
fn animated(property: &str, to: &[f32], t: &Transform) -> Option<AnimatedProperty> {
    let vec3 = || (to.len() >= 3).then(|| Vec3::new(to[0], to[1], to[2]));
    match property {
        "position" | "translation" => Some(AnimatedProperty::Position(t.translation, vec3()?)),
        "rotation" => {
            let [x, y, z] = [vec3()?.x, vec3()?.y, vec3()?.z].map(f32::to_radians);
            Some(AnimatedProperty::Rotation(
                t.rotation,
                Quat::from_euler(EulerRot::XYZ, x, y, z),
            ))
        }
        "scale" => {
            let target = match to.len() {
                1 => Vec3::splat(to[0]),
                n if n >= 3 => Vec3::new(to[0], to[1], to[2]),
                _ => return None,
            };
            Some(AnimatedProperty::Scale(t.scale, target))
        }
        _ => None,
    }
}

/// Advance `animate` actions linearly; drop each when it arrives.
pub fn animate(
    time: Res<Time>,
    mut commands: Commands,
    mut query: Query<(Entity, &mut TriggerAnimation, &mut Transform)>,
) {
    for (entity, mut anim, mut transform) in &mut query {
        anim.elapsed += time.delta_secs();
        let f = if anim.duration > 0.0 {
            (anim.elapsed / anim.duration).min(1.0)
        } else {
            1.0
        };
        match anim.property {
            AnimatedProperty::Position(a, b) => transform.translation = a.lerp(b, f),
            AnimatedProperty::Rotation(a, b) => transform.rotation = a.slerp(b, f),
            AnimatedProperty::Scale(a, b) => transform.scale = a.lerp(b, f),
        }
        if f >= 1.0 {
            commands.entity(entity).remove::<TriggerAnimation>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn def(on: wt::TriggerEvent, action: wt::TriggerActionDef) -> wt::TriggerDef {
        wt::TriggerDef {
            on,
            action,
            once: false,
            cooldown: None,
            requires_item: None,
        }
    }

    fn app() -> App {
        let mut app = App::new();
        app.init_resource::<Time>().add_plugins(TriggerPlugin);
        app
    }

    fn step(app: &mut App, secs: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(secs));
        app.update();
    }

    fn spawn(app: &mut App, entity: wt::WorldEntity) -> Entity {
        let t = Transform::from_translation(Vec3::from_array(entity.transform.position));
        app.world_mut()
            .spawn((Triggers::of(&entity).unwrap(), t, GlobalTransform::from(t)))
            .id()
    }

    fn visitor_at(app: &mut App, p: Vec3) -> Entity {
        let t = Transform::from_translation(p);
        app.world_mut()
            .spawn((Visitor, t, GlobalTransform::from(t)))
            .id()
    }

    fn move_visitor(app: &mut App, v: Entity, p: Vec3) {
        let t = Transform::from_translation(p);
        app.world_mut()
            .entity_mut(v)
            .insert((t, GlobalTransform::from(t)));
    }

    fn visibility(app: &App, e: Entity) -> Option<Visibility> {
        app.world().get::<Visibility>(e).copied()
    }

    #[test]
    fn area_fires_on_the_edge_only() {
        let mut app = app();
        let mut gate = wt::WorldEntity::new(1, "gate").at([0.0, 0.0, 0.0]);
        gate.triggers.push(def(
            wt::TriggerEvent::AreaEnter {
                volume: Some(wt::TriggerVolume::Box {
                    half_extents: [1.0, 1.0, 1.0],
                }),
            },
            wt::TriggerActionDef::Toggle,
        ));
        let gate = spawn(&mut app, gate);
        let v = visitor_at(&mut app, Vec3::new(5.0, 0.0, 0.0));
        step(&mut app, 0.1);
        assert_eq!(visibility(&app, gate), None);
        move_visitor(&mut app, v, Vec3::new(0.5, 0.0, 0.0));
        step(&mut app, 0.1);
        assert_eq!(visibility(&app, gate), Some(Visibility::Hidden));
        // Staying inside doesn't fire again.
        step(&mut app, 0.1);
        assert_eq!(visibility(&app, gate), Some(Visibility::Hidden));
        move_visitor(&mut app, v, Vec3::new(5.0, 0.0, 0.0));
        step(&mut app, 0.1);
        move_visitor(&mut app, v, Vec3::new(0.0, 0.5, 0.0));
        step(&mut app, 0.1);
        assert_eq!(visibility(&app, gate), Some(Visibility::Inherited));
    }

    #[test]
    fn proximity_repeats_after_its_cooldown_and_once_stops_it() {
        let mut app = app();
        let mut lamp = wt::WorldEntity::new(1, "lamp");
        lamp.triggers.push(def(
            wt::TriggerEvent::Proximity { radius: 2.0 },
            wt::TriggerActionDef::ShowText {
                text: "hi".into(),
                seconds: 1.0,
            },
        ));
        let mut once = def(
            wt::TriggerEvent::Proximity { radius: 2.0 },
            wt::TriggerActionDef::Hide,
        );
        once.once = true;
        lamp.triggers.push(once);
        spawn(&mut app, lamp);
        visitor_at(&mut app, Vec3::new(1.0, 0.0, 0.0));
        let count = |app: &mut App| {
            let msgs = app.world().resource::<Messages<TriggerFired>>();
            msgs.iter_current_update_messages().count()
        };
        step(&mut app, 0.1);
        assert_eq!(count(&mut app), 2);
        step(&mut app, 0.5);
        assert_eq!(count(&mut app), 0);
        step(&mut app, 0.6);
        assert_eq!(count(&mut app), 1, "only the repeating one");
    }

    #[test]
    fn click_needs_reach_and_items_gate_triggers() {
        let mut app = app();
        let mut door = wt::WorldEntity::new(1, "door");
        door.triggers.push(def(
            wt::TriggerEvent::Click {
                max_distance: 3.0,
                prompt: None,
            },
            wt::TriggerActionDef::Animate {
                property: "position".into(),
                to: vec![0.0, 2.0, 0.0],
                duration: 1.0,
            },
        ));
        let mut locked = def(
            wt::TriggerEvent::Click {
                max_distance: 3.0,
                prompt: None,
            },
            wt::TriggerActionDef::Host {
                name: "add_score".into(),
                args: Default::default(),
            },
        );
        locked.requires_item = Some("key".into());
        door.triggers.push(locked);
        let door = spawn(&mut app, door);
        let v = visitor_at(&mut app, Vec3::new(10.0, 0.0, 0.0));

        app.world_mut().write_message(Activate { entity: door });
        step(&mut app, 0.1);
        assert!(
            app.world().get::<TriggerAnimation>(door).is_none(),
            "too far"
        );

        move_visitor(&mut app, v, Vec3::new(2.0, 0.0, 0.0));
        app.world_mut().write_message(Activate { entity: door });
        step(&mut app, 0.1);
        let hosts = app.world().resource::<Messages<HostAction>>();
        assert_eq!(hosts.iter_current_update_messages().count(), 0, "no key");
        step(&mut app, 0.5);
        step(&mut app, 0.6);
        assert!(app.world().get::<TriggerAnimation>(door).is_none());
        let y = app.world().get::<Transform>(door).unwrap().translation.y;
        assert!((y - 2.0).abs() < 1e-4, "{y}");

        app.world_mut()
            .resource_mut::<HeldItems>()
            .0
            .insert("key".into());
        app.world_mut().write_message(Activate { entity: door });
        step(&mut app, 0.1);
        let hosts = app.world().resource::<Messages<HostAction>>();
        assert_eq!(hosts.iter_current_update_messages().count(), 1);
    }

    #[test]
    fn start_and_timer_and_teleport() {
        let mut app = app();
        let mut beacon = wt::WorldEntity::new(1, "beacon");
        beacon.triggers.push(def(
            wt::TriggerEvent::Start,
            wt::TriggerActionDef::Teleport {
                destination: [0.0, 5.0, 0.0],
            },
        ));
        beacon.triggers.push(def(
            wt::TriggerEvent::Timer { interval: 1.0 },
            wt::TriggerActionDef::Toggle,
        ));
        let beacon = spawn(&mut app, beacon);
        let v = visitor_at(&mut app, Vec3::ZERO);
        step(&mut app, 0.1);
        assert_eq!(
            app.world().get::<Transform>(v).unwrap().translation,
            Vec3::new(0.0, 5.0, 0.0)
        );
        assert_eq!(visibility(&app, beacon), None);
        step(&mut app, 1.0);
        assert_eq!(visibility(&app, beacon), Some(Visibility::Hidden));
        // Start fires once only.
        move_visitor(&mut app, v, Vec3::ZERO);
        step(&mut app, 0.1);
        assert_eq!(
            app.world().get::<Transform>(v).unwrap().translation,
            Vec3::ZERO
        );
    }
}
