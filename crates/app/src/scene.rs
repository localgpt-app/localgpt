//! The outliner's and the inspector's half in the app: the world's things
//! as its document has them — the entities as `manifest.json` writes them,
//! from the world the timeline reads — and the one selection, which the
//! page draws in the left panel's Outliner tab (`ui/src/scene`, drawn first
//! in `ui/design/scene.html`).
//!
//! **An edit** is the person's, through the authority the agents use: a
//! field let go of is one [`PersonIntent::Patch`] — a merge patch for each
//! selected thing, one entry in the history — and while a drag on a field
//! is held, a [`PersonPreview`] shows it on the canvas ahead of the
//! document. Only a live canvas edits; a song's world or a document's
//! reads, since its own source is what changes it.
//!
//! **The eye** in the outliner hides a thing in the window alone, as the
//! timeline's does: the document keeps it as it was.

use std::collections::HashSet;

use bevy::camera::visibility::VisibilitySystems;
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use localgpt_gen::gen3d::canvas_edit::{Patched, PersonIntent, PersonPreview};
use localgpt_gen::gen3d::registry::NameRegistry;
use localgpt_gen::inspector::{InspectorSelection, PendingFocusEntity};
use localgpt_world_types as wt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::timeline::{Timeline, WorldRead};

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Scene>()
            .add_message::<SceneAsk>()
            .add_systems(
                Update,
                (build_view, answer_asks, report_selection)
                    .chain()
                    .after(WorldRead),
            )
            // After the canvas has applied what it applies this frame, and
            // before visibility is worked out from it.
            .add_systems(
                PostUpdate,
                hide_here.before(VisibilitySystems::VisibilityPropagate),
            );
    }
}

/// What the page draws — the shapes `ui/src/scene/model.ts` names.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SceneView {
    /// The format's own JSON for each entity, in the document's order.
    pub entities: Vec<Value>,
    /// Fields edit: a live canvas.
    pub editable: bool,
    /// The world in a few lines, for when nothing is selected.
    pub world: WorldLines,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct WorldLines {
    pub name: String,
    pub sky: Option<[f32; 4]>,
    pub song: Option<String>,
    pub tours: usize,
}

pub fn view_of(world: &wt::WorldManifest, editable: bool) -> SceneView {
    SceneView {
        entities: world
            .entities
            .iter()
            .map(|entity| serde_json::to_value(entity).unwrap_or(Value::Null))
            .collect(),
        editable,
        world: WorldLines {
            name: world.meta.name.clone(),
            sky: world
                .environment
                .as_ref()
                .and_then(|env| env.background_color),
            song: world
                .soundtrack
                .as_ref()
                .map(|song| match (&song.title, &song.artist) {
                    (Some(title), Some(artist)) => format!("{title} — {artist}"),
                    (Some(title), None) => title.clone(),
                    _ => song.path.clone().unwrap_or_else(|| "a song".into()),
                }),
            tours: world.tours.len(),
        },
    }
}

/// The scene panel's state in the app.
#[derive(Resource, Default)]
pub struct Scene {
    /// `None` until a world is open.
    pub view: Option<SceneView>,
    /// Counts the view's changes, so the page gets it only when it changed.
    pub rev: u64,
    /// The selection by world id, its primary first.
    pub selected: Vec<u64>,
    /// The timeline's rebuild the view was made at.
    built: Option<u64>,
    /// What the eye hid, by world id.
    hidden: HashSet<u64>,
    /// What it let go of, to be shown as the document has it.
    unhidden: Vec<u64>,
}

/// The page's asks of the outliner and the inspector.
#[derive(Message, Debug, Clone, PartialEq)]
pub enum SceneAsk {
    /// Select these, the primary first; none, nothing.
    Select(Vec<u64>),
    /// The camera flies to it.
    Focus(u64),
    /// A field let go of: one edit, for each thing it changes.
    Patch { edits: Vec<SceneEdit>, what: String },
    /// A field held: shown on the canvas, not committed.
    Preview(Vec<SceneEdit>),
    /// What the eye hides in the window, by id; the rest show.
    Hide(Vec<u64>),
}

/// One thing a field edit changes: its merge patch, as `ModifyEntity`
/// takes it, and its name for the history.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SceneEdit {
    pub id: u64,
    pub name: String,
    pub patch: Value,
}

fn patched(edits: &[SceneEdit]) -> Vec<Patched> {
    edits
        .iter()
        .map(|edit| Patched {
            id: edit.id,
            name: edit.name.clone(),
            patch: edit.patch.clone(),
        })
        .collect()
}

fn build_view(timeline: Res<Timeline>, mut scene: ResMut<Scene>) {
    if scene.built == Some(timeline.rev) {
        return;
    }
    scene.built = Some(timeline.rev);
    let view = timeline
        .world()
        .map(|world| view_of(world, timeline.editable()));
    if view != scene.view {
        scene.view = view;
        scene.rev += 1;
    }
}

#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn answer_asks(
    mut asks: MessageReader<SceneAsk>,
    mut scene: ResMut<Scene>,
    timeline: Res<Timeline>,
    registry: Option<Res<NameRegistry>>,
    mut selection: Option<ResMut<InspectorSelection>>,
    mut focus: Option<ResMut<PendingFocusEntity>>,
    mut intents: Option<ResMut<Messages<PersonIntent>>>,
    mut previews: Option<ResMut<Messages<PersonPreview>>>,
) {
    let entity_of = |id: &u64| {
        registry
            .as_deref()
            .and_then(|registry| registry.get_entity_by_id(&wt::EntityId(*id)))
    };
    for ask in asks.read() {
        match ask {
            SceneAsk::Select(ids) => {
                let Some(selection) = selection.as_deref_mut() else {
                    continue;
                };
                let mut picked = ids.iter().filter_map(entity_of);
                selection.set(picked.next());
                let others: Vec<Entity> = picked.collect();
                if selection.others != others {
                    selection.others = others;
                }
            }
            SceneAsk::Focus(id) => {
                if let Some(focus) = focus.as_deref_mut() {
                    focus.target = entity_of(id);
                }
            }
            // A world that isn't a canvas reads: its own source changes it.
            SceneAsk::Patch { .. } | SceneAsk::Preview(_) if !timeline.editable() => {}
            SceneAsk::Patch { edits, what } => {
                if let Some(intents) = intents.as_deref_mut() {
                    intents.write(PersonIntent::Patch {
                        edits: patched(edits),
                        what: what.clone(),
                    });
                }
            }
            SceneAsk::Preview(edits) => {
                if let Some(previews) = previews.as_deref_mut() {
                    previews.write(PersonPreview(patched(edits)));
                }
            }
            SceneAsk::Hide(ids) => {
                let now: HashSet<u64> = ids.iter().copied().collect();
                let unhidden: Vec<u64> = scene.hidden.difference(&now).copied().collect();
                scene.unhidden.extend(unhidden);
                scene.hidden = now;
            }
        }
    }
}

/// The selection by world id, for the page: a click in the world selects
/// in the outliner too.
fn report_selection(
    selection: Option<Res<InspectorSelection>>,
    registry: Option<Res<NameRegistry>>,
    mut scene: ResMut<Scene>,
) {
    let (Some(selection), Some(registry)) = (selection, registry) else {
        return;
    };
    let ids: Vec<u64> = selection
        .all()
        .filter_map(|entity| registry.get_id(entity))
        .map(|id| id.0)
        .collect();
    if scene.selected != ids {
        scene.selected = ids;
    }
}

/// Holds what the eye hid hidden, whatever the canvas applied this frame,
/// and shows what it let go of as the document has it.
fn hide_here(
    mut scene: ResMut<Scene>,
    timeline: Res<Timeline>,
    registry: Option<Res<NameRegistry>>,
    mut visibility: Query<&mut Visibility>,
) {
    let Some(registry) = registry else {
        return;
    };
    let entity_of = |id: u64| registry.get_entity_by_id(&wt::EntityId(id));
    if !scene.unhidden.is_empty() {
        for id in std::mem::take(&mut scene.unhidden) {
            let written = timeline
                .world()
                .and_then(|world| world.entities.iter().find(|e| e.id.0 == id))
                .map_or(Visibility::Inherited, |e| {
                    localgpt_world_bevy::visibility(&e.transform)
                });
            if let Some(mut shown) = entity_of(id).and_then(|e| visibility.get_mut(e).ok()) {
                *shown = written;
            }
        }
    }
    for id in &scene.hidden {
        if let Some(mut shown) = entity_of(*id).and_then(|e| visibility.get_mut(e).ok())
            && *shown != Visibility::Hidden
        {
            *shown = Visibility::Hidden;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use localgpt_gen::gen3d::live::ReviewDecision;
    use localgpt_gen::gen3d::shots::{LiveShots, LookThrough, ShotOps};
    use localgpt_world_bevy::modulation::Soundtrack;
    use serde_json::json;
    use std::path::PathBuf;

    use crate::timeline::TimelinePlugin;

    fn harbour() -> wt::WorldManifest {
        serde_json::from_value(json!({
            "version": 3,
            "meta": {"name": "harbour"},
            "environment": {"background_color": [0.66, 0.47, 0.52, 1.0]},
            "tours": [{"name": "walk", "waypoints": []}],
            "entities": [
                {"id": 30, "name": "jetty", "transform": {"position": [4.0, 0.0, -3.0]}},
                {"id": 37, "name": "lantern", "parent": 30,
                 "transform": {"position": [0.7, 1.35, -8.4]},
                 "shape": {"Cylinder": {"radius": 0.1, "height": 0.3}},
                 "material": {"color": [0.2, 0.2, 0.2, 1.0], "roughness": 0.6}},
                {"id": 2, "name": "sea", "transform": {"position": [0.0, 0.02, -18.0], "visible": false},
                 "shape": {"Plane": {"x": 80.0, "z": 30.0}}}
            ],
            "next_entity_id": 38
        }))
        .unwrap()
    }

    /// The plugin over a canvas showing the harbour, with its three things
    /// in the scene under their world ids.
    fn canvas(editable: bool) -> (App, [Entity; 3]) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Soundtrack>()
            .init_resource::<NameRegistry>()
            .init_resource::<InspectorSelection>()
            .init_resource::<PendingFocusEntity>()
            .add_message::<ShotOps>()
            .add_message::<LookThrough>()
            .add_message::<ReviewDecision>()
            .add_message::<PersonIntent>()
            .add_message::<PersonPreview>()
            .add_plugins((
                TimelinePlugin {
                    canvas: editable.then(|| PathBuf::from("harbour.world")),
                },
                ScenePlugin,
            ))
            .insert_resource(LiveShots {
                shots: Vec::new(),
                manifest: harbour(),
            });
        let things = [(30, "jetty"), (37, "lantern"), (2, "sea")].map(|(id, name)| {
            let entity = app.world_mut().spawn(Visibility::Inherited).id();
            app.world_mut()
                .resource_mut::<NameRegistry>()
                .insert_with_id(name.into(), entity, wt::EntityId(id));
            entity
        });
        app.update();
        (app, things)
    }

    fn drained<T: Message>(app: &mut App) -> Vec<T> {
        app.world_mut()
            .resource_mut::<Messages<T>>()
            .drain()
            .collect()
    }

    #[test]
    fn the_page_gets_the_documents_entities_and_the_world_in_a_few_lines() {
        let world = harbour();
        let view = view_of(&world, true);
        assert_eq!(view.entities.len(), 3);
        assert_eq!(view.entities[1]["name"], "lantern");
        assert_eq!(view.entities[1]["parent"], 30);
        assert_eq!(
            view.entities[1]["shape"]["Cylinder"]["radius"].as_f64(),
            Some(f64::from(0.1f32)),
            "the format's own JSON"
        );
        assert_eq!(
            view.world,
            WorldLines {
                name: "harbour".into(),
                sky: Some([0.66, 0.47, 0.52, 1.0]),
                song: None,
                tours: 1,
            }
        );

        let (app, _) = canvas(true);
        let scene = app.world().resource::<Scene>();
        assert_eq!(scene.view.as_ref().map(|v| v.editable), Some(true));
        assert_eq!(scene.rev, 1);
    }

    #[test]
    fn selecting_on_the_page_is_selecting_in_the_world_and_back() {
        let (mut app, [jetty, lantern, _]) = canvas(true);
        app.world_mut()
            .write_message(SceneAsk::Select(vec![37, 30, 999]));
        app.update();
        let selection = app.world().resource::<InspectorSelection>();
        assert_eq!(selection.entity, Some(lantern), "the primary first");
        assert_eq!(
            selection.others,
            [jetty],
            "what isn't in the world is left out"
        );
        assert_eq!(app.world().resource::<Scene>().selected, [37, 30]);

        // A click in the world, as the canvas has it.
        app.world_mut()
            .resource_mut::<InspectorSelection>()
            .set(Some(jetty));
        app.update();
        assert_eq!(app.world().resource::<Scene>().selected, [30]);

        app.world_mut().write_message(SceneAsk::Focus(37));
        app.world_mut().write_message(SceneAsk::Select(Vec::new()));
        app.update();
        assert_eq!(
            app.world().resource::<PendingFocusEntity>().target,
            Some(lantern)
        );
        assert!(app.world().resource::<Scene>().selected.is_empty());
    }

    #[test]
    fn on_a_canvas_a_field_held_is_shown_and_let_go_is_one_edit() {
        let (mut app, _) = canvas(true);
        let edit = |y: f64| SceneEdit {
            id: 37,
            name: "lantern".into(),
            patch: json!({"transform": {"position": [0.7, y, -8.4]}}),
        };
        app.world_mut()
            .write_message(SceneAsk::Preview(vec![edit(1.55)]));
        app.world_mut().write_message(SceneAsk::Patch {
            edits: vec![edit(1.75)],
            what: "move lantern".into(),
        });
        app.update();
        let previews: Vec<PersonPreview> = drained(&mut app);
        assert_eq!(previews.len(), 1);
        assert_eq!(previews[0].0[0].patch["transform"]["position"][1], 1.55);
        let intents: Vec<PersonIntent> = drained(&mut app);
        assert_eq!(
            intents,
            [PersonIntent::Patch {
                edits: vec![Patched {
                    id: 37,
                    name: "lantern".into(),
                    patch: json!({"transform": {"position": [0.7, 1.75, -8.4]}}),
                }],
                what: "move lantern".into(),
            }]
        );
    }

    #[test]
    fn a_world_that_isnt_a_canvas_reads() {
        let (mut app, _) = canvas(false);
        // The timeline reads a loaded world from its folder; here, none is
        // open, so nothing is drawn and nothing edits.
        assert!(app.world().resource::<Scene>().view.is_none());
        app.world_mut().write_message(SceneAsk::Patch {
            edits: vec![SceneEdit {
                id: 37,
                name: "lantern".into(),
                patch: json!({"name": "lamp"}),
            }],
            what: "rename lantern to lamp".into(),
        });
        app.update();
        assert!(drained::<PersonIntent>(&mut app).is_empty());
    }

    #[test]
    fn the_eye_hides_in_the_window_and_lets_the_document_show_it_again() {
        let (mut app, [jetty, lantern, sea]) = canvas(true);
        app.world_mut().write_message(SceneAsk::Hide(vec![37, 2]));
        app.update();
        let shown = |app: &App, entity: Entity| *app.world().get::<Visibility>(entity).unwrap();
        assert_eq!(shown(&app, lantern), Visibility::Hidden);
        assert_eq!(shown(&app, jetty), Visibility::Inherited);

        // The canvas applies the document under it: the eye holds.
        *app.world_mut().get_mut::<Visibility>(lantern).unwrap() = Visibility::Inherited;
        app.update();
        assert_eq!(shown(&app, lantern), Visibility::Hidden);

        // Let go, each is as the document has it: the sea was hidden there.
        app.world_mut().write_message(SceneAsk::Hide(Vec::new()));
        app.update();
        assert_eq!(shown(&app, lantern), Visibility::Inherited);
        assert_eq!(shown(&app, sea), Visibility::Hidden);
    }
}
