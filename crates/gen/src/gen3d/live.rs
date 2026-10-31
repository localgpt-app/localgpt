//! `--live <dir>`: Gen as the canvas of a live `.world` folder — a proof of
//! concept (branch `poc/live-editing`).
//!
//! The window shows the world and nothing in it talks to a model. Agents
//! work from their own terminal: they write asset files into the folder and
//! send batches of ops to a small HTTP API served here on localhost and
//! advertised in `.live/endpoint.json`. `LiveWorld` (world-agent) checks and
//! commits each batch — whole or not at all — and the canvas applies what
//! was committed. The same API takes screenshots, reports what the person
//! has selected, undoes, and replays the world's git history commit by
//! commit. The folder's `AGENTS.md` tells an agent all of this.
//!
//! The history is non-linear. The canvas can show any point in it (the
//! app's history panel, `POST /goto`), and a batch that names an earlier
//! entry (`"at"`) starts a branch there while the head — `manifest.json`,
//! main's tip — stays where it was. The canvas follows a commit when it
//! continues what is on screen and otherwise only grows the history, so an
//! agent working on main never yanks a person off the point they chose.

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use bevy::camera::{ImageRenderTarget, NormalizedRenderTarget};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::MouseButtonInput;
use bevy::picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{CursorMoved, PrimaryWindow, WindowEvent};
use localgpt_world_agent::live::{self, LiveWorld};
use localgpt_world_types as wt;
use serde_json::{Value, json};

use super::canvas_edit::{Named, Patched, PersonIntent, PersonPreview};
use super::ops_apply::OpsApplier;
use super::plugin::FlyCam;
use super::registry::GenEntity;
use crate::inspector::InspectorSelection;

/// A request the API thread hands to the app, answered over `reply`.
struct Call {
    method: String,
    path: String,
    query: String,
    body: Vec<u8>,
    reply: Sender<Reply>,
}

struct Reply {
    status: u16,
    body: Value,
    /// The board pages are HTML; everything else is JSON.
    content_type: &'static str,
    /// A file the answer names; the API thread waits for it to be written.
    wait_for: Option<PathBuf>,
}

impl Reply {
    fn ok(body: Value) -> Self {
        Self {
            status: 200,
            body,
            content_type: "application/json",
            wait_for: None,
        }
    }

    fn status(status: u16, body: Value) -> Self {
        Self {
            status,
            body,
            content_type: "application/json",
            wait_for: None,
        }
    }

    /// A rendered page (`GET /board`): sent as it is, not JSON-quoted.
    fn html(body: String) -> Self {
        Self {
            status: 200,
            body: Value::String(body),
            content_type: "text/html; charset=utf-8",
            wait_for: None,
        }
    }
}

/// The API's own address, so a panel can open board pages in the
/// person's browser (the token rides the query, which the API accepts for
/// `GET /board` alone — it already sits in `.live/endpoint.json`).
#[derive(Resource, Clone)]
pub struct LiveEndpoint {
    pub url: String,
    pub token: String,
}

/// The open live world and the canvas's own bookkeeping.
#[derive(Resource)]
pub(crate) struct Live {
    world: LiveWorld,
    /// The non-linear view over the same log: seeks, tips, and the delta
    /// between what the canvas shows and where it is going. The authority
    /// commits; this follows — one document, two halves of the reconciler.
    view: localgpt_world_editor::Editor,
    calls: Mutex<Receiver<Call>>,
    started: bool,
    guard: Timer,
    /// Counts down to a fresh `.live/preview.png` after a change.
    preview: Option<Timer>,
    /// Screenshots to take this frame.
    shots: Vec<PathBuf>,
    shot_count: u32,
    /// Calls the second system answers (camera, selection, screenshots).
    deferred: Vec<Call>,
    camera: Option<wt::CameraDef>,
    replay: Option<Replay>,
    /// Set whenever the history or the canvas's place in it changes, so the
    /// rail's snapshot is rebuilt once rather than every frame.
    history_dirty: bool,
    /// Set whenever the scene applied ops, so the shot thumbnails re-render
    /// even when the shot list itself is unchanged (a prop moved inside a
    /// frame). Flushed to [`super::shots::ThumbsDirty`] each frame.
    thumbs_dirty: bool,
    /// The newest thing that happened, for the history: a commit, or a refusal.
    status: Option<String>,
    /// Agents' batches show on the canvas before they commit, and the
    /// person accepts or rejects each one (`LOCALGPT_REVIEW=1` starts on).
    pub(crate) review: bool,
    /// The batch on the canvas awaiting the person's decision.
    proposal: Option<Proposal>,
    /// Every proposal so far, for `GET /proposals`.
    proposals: Vec<ProposalRecord>,
    /// A person's input, synthesized — the queue `POST /input` fills, one
    /// event emitted per frame. `None` unless the app started with
    /// `LOCALGPT_SYNTHETIC_INPUT=1`: it is a test's seam, and with it an
    /// agent could press Enter on its own proposal.
    synthetic: Option<VecDeque<SyntheticEvent>>,
    /// The turns that ended ([`TurnEnded`]), for the history.
    turns: Vec<TurnMark>,
}

/// A person's input, synthesized for a test that drives the canvas with
/// no one at the keyboard. One event lands per frame, so a press and a
/// release fall on different frames as a hand's do.
#[derive(Debug, Clone)]
enum SyntheticEvent {
    /// The pointer, in window points.
    Cursor(Vec2),
    Button {
        button: MouseButton,
        pressed: bool,
    },
    Key {
        key: KeyCode,
        pressed: bool,
    },
}

fn parse_synthetic(value: &Value) -> Result<SyntheticEvent, String> {
    if let Some(at) = value.get("cursor") {
        let xy: [f32; 2] = serde_json::from_value(at.clone())
            .map_err(|_| "\"cursor\" is [x, y] in window points".to_string())?;
        return Ok(SyntheticEvent::Cursor(Vec2::new(xy[0], xy[1])));
    }
    let pressed = value
        .get("pressed")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if let Some(button) = value.get("button").and_then(Value::as_str) {
        let button = match button {
            "left" => MouseButton::Left,
            "right" => MouseButton::Right,
            "middle" => MouseButton::Middle,
            other => return Err(format!("no mouse button '{other}'")),
        };
        return Ok(SyntheticEvent::Button { button, pressed });
    }
    if let Some(key) = value.get("key").and_then(Value::as_str) {
        let key = key_code(key).ok_or_else(|| format!("no key '{key}'"))?;
        return Ok(SyntheticEvent::Key { key, pressed });
    }
    Err("an event is {\"cursor\": [x, y]}, {\"button\": \"left\", \"pressed\": true} or {\"key\": \"Enter\", \"pressed\": true}".into())
}

/// The keys the canvas answers to, by their `KeyCode` names.
fn key_code(name: &str) -> Option<KeyCode> {
    Some(match name {
        "Enter" => KeyCode::Enter,
        "Escape" => KeyCode::Escape,
        "Delete" => KeyCode::Delete,
        "Backspace" => KeyCode::Backspace,
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "Comma" => KeyCode::Comma,
        "Period" => KeyCode::Period,
        "Minus" => KeyCode::Minus,
        "Equal" => KeyCode::Equal,
        "KeyD" => KeyCode::KeyD,
        "KeyG" => KeyCode::KeyG,
        "KeyH" => KeyCode::KeyH,
        "KeyY" => KeyCode::KeyY,
        "KeyZ" => KeyCode::KeyZ,
        "KeyL" => KeyCode::KeyL,
        // The camera's keys, to look from somewhere else.
        "KeyW" => KeyCode::KeyW,
        "KeyA" => KeyCode::KeyA,
        "KeyS" => KeyCode::KeyS,
        "KeyQ" => KeyCode::KeyQ,
        "Space" => KeyCode::Space,
        "ShiftLeft" => KeyCode::ShiftLeft,
        "ControlLeft" => KeyCode::ControlLeft,
        "SuperLeft" => KeyCode::SuperLeft,
        "AltLeft" => KeyCode::AltLeft,
        "F1" => KeyCode::F1,
        _ => return None,
    })
}

/// Emit the next synthesized event as the input it stands for: the same
/// messages the window would send, so picking, the handles and the keys
/// all see a hand.
#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn inject_synthetic_input(
    mut live: ResMut<Live>,
    windows: Query<Entity, With<PrimaryWindow>>,
    offscreen: Option<Res<super::offscreen::OffscreenRenderTarget>>,
    mut keys: MessageWriter<KeyboardInput>,
    mut buttons: MessageWriter<MouseButtonInput>,
    mut cursor: MessageWriter<CursorMoved>,
    mut motions: MessageWriter<bevy::input::mouse::MouseMotion>,
    mut window_events: MessageWriter<WindowEvent>,
    mut pointer_inputs: MessageWriter<PointerInput>,
    mut cursor_last: Local<Vec2>,
) {
    let Some(event) = live.synthetic.as_mut().and_then(VecDeque::pop_front) else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let state = |pressed: bool| {
        if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        }
    };
    // The window backend sends each input twice: as its own message, and
    // wrapped in `WindowEvent` — picking reads the wrapper, the button maps
    // read the message. Both, then, as a real hand gets.
    //
    // With the canvas rendering into an image (`LOCALGPT_OFFSCREEN_SHOTS`),
    // picking would pair a window pointer with no camera: the pointer is
    // fed to picking directly, aimed at the image, instead of the wrapper.
    let image_target = offscreen
        .as_ref()
        .and_then(|o| o.image_handle.clone())
        .map(|handle| {
            NormalizedRenderTarget::Image(ImageRenderTarget {
                handle,
                scale_factor: 1.0,
            })
        });
    match event {
        SyntheticEvent::Cursor(position) => {
            let delta = position - *cursor_last;
            let moved = CursorMoved {
                window,
                position,
                delta: None,
            };
            cursor.write(moved.clone());
            // The raw motion the camera's look reads, as the device sends it.
            motions.write(bevy::input::mouse::MouseMotion { delta });
            match &image_target {
                Some(target) => {
                    pointer_inputs.write(PointerInput {
                        pointer_id: PointerId::Mouse,
                        location: Location {
                            target: target.clone(),
                            position,
                        },
                        action: PointerAction::Move { delta },
                    });
                }
                None => {
                    window_events.write(WindowEvent::CursorMoved(moved));
                }
            }
            *cursor_last = position;
        }
        SyntheticEvent::Button { button, pressed } => {
            let input = MouseButtonInput {
                button,
                state: state(pressed),
                window,
            };
            buttons.write(input);
            match &image_target {
                Some(target) => {
                    let pointer_button = match button {
                        MouseButton::Right => PointerButton::Secondary,
                        MouseButton::Middle => PointerButton::Middle,
                        _ => PointerButton::Primary,
                    };
                    pointer_inputs.write(PointerInput {
                        pointer_id: PointerId::Mouse,
                        location: Location {
                            target: target.clone(),
                            position: *cursor_last,
                        },
                        action: if pressed {
                            PointerAction::Press(pointer_button)
                        } else {
                            PointerAction::Release(pointer_button)
                        },
                    });
                }
                None => {
                    window_events.write(WindowEvent::MouseButtonInput(input));
                }
            }
        }
        SyntheticEvent::Key { key, pressed } => {
            let input = KeyboardInput {
                key_code: key,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state: state(pressed),
                text: None,
                repeat: false,
                window,
            };
            keys.write(input.clone());
            window_events.write(WindowEvent::KeyboardInput(input));
        }
    }
}

/// `LOCALGPT_OFFSCREEN_SHOTS=1`: the canvas renders into an image rather
/// than the window, and `GET /screenshot` reads that image. For a test on
/// a locked or sleeping screen, where a window's surface stays black
/// whatever the scene does; the window itself goes dark meanwhile.
fn offscreen_canvas(
    mut commands: Commands,
    mut offscreen: ResMut<super::offscreen::OffscreenRenderTarget>,
    mut cameras: Query<(Entity, &mut bevy::camera::RenderTarget), With<FlyCam>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (width, height) = (offscreen.width, offscreen.height);
    for (camera, mut target) in &mut cameras {
        if matches!(*target, bevy::camera::RenderTarget::Window(_)) {
            let handle = offscreen
                .image_handle
                .get_or_insert_with(|| {
                    super::offscreen::create_offscreen_image(&mut images, width, height)
                })
                .clone();
            *target = bevy::camera::RenderTarget::Image(handle.into());
            // `bevy_ui` draws through the camera that targets the primary
            // window; with none, the world's HUD and the selection box would
            // be laid out against nothing and drawn nowhere. This camera is
            // it now.
            commands.entity(camera).insert(bevy::ui::IsDefaultUiCamera);
        }
    }
}

/// The handles' places in the window, for a driver with no eyes: the
/// origin, the three arrow ends, a point on each ring (near where it faces
/// the camera, a turn along it away from the arrows it crosses) and the
/// scale cube.
fn handle_points(
    handles: Option<&super::canvas_edit::Handles>,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCam>>,
) -> Value {
    let (Some(frame), Ok((camera, global))) = (handles.and_then(|h| h.frame()), cameras.single())
    else {
        return Value::Null;
    };
    let at = |world: Vec3| {
        camera
            .world_to_viewport(global, world)
            .ok()
            .map(|v| json!([v.x, v.y]))
            .unwrap_or(Value::Null)
    };
    let to_camera = global.translation() - frame.origin;
    // A point of each ring near the camera: in the ring's plane, toward the
    // camera, then an eighth of a turn along the ring so it is not where an
    // arrow crosses it (the arrows win a tie there).
    let axes = frame.axes;
    let on_ring = |i: usize| {
        let axis = axes[i];
        let facing = (to_camera - axis * to_camera.dot(axis)).normalize_or(axes[(i + 1) % 3]);
        let spoke = Quat::from_axis_angle(axis, std::f32::consts::FRAC_PI_4) * facing;
        at(frame.origin + spoke * frame.ring_radius)
    };
    json!({
        "origin": at(frame.origin),
        "x": at(frame.origin + axes[0] * frame.length),
        "y": at(frame.origin + axes[1] * frame.length),
        "z": at(frame.origin + axes[2] * frame.length),
        "ring_x": on_ring(0),
        "ring_y": on_ring(1),
        "ring_z": on_ring(2),
        // The ring about the vertical, under the name it had alone.
        "ring": on_ring(1),
        "scale": at(frame.scale_at),
        // Along the entity's own axes (`L`), or the world's.
        "local": frame.local,
        // The one under the pointer now, if any.
        "hot": handles.and_then(|h| h.hot_name()),
    })
}

impl Live {
    /// How many entries the log holds.
    pub(crate) fn history_len(&self) -> usize {
        self.view.history().len()
    }

    /// The head's revision.
    pub(crate) fn revision(&self) -> u64 {
        self.world.revision()
    }
}

/// An agent's batch shown on the canvas before it commits — the person
/// accepts it (it goes through the authority then, as it would have) or
/// rejects it (the canvas is put back from the document). What DeepMonkey
/// calls a previewable, cancelable AI edit; here it is a batch read
/// against the world on screen without being written.
pub(crate) struct Proposal {
    pub(crate) n: u64,
    pub(crate) author: String,
    /// The entry it builds on: what the canvas showed when it arrived.
    at: Option<String>,
    batch: Value,
    /// The world the canvas shows while it is pending.
    shown: wt::WorldManifest,
    pub(crate) changes: Vec<String>,
}

struct ProposalRecord {
    n: u64,
    author: String,
    changes: Vec<String>,
    /// What the authority warned about the batch (a delete that leaves a
    /// behavior without its target), for whoever shows the proposal.
    warnings: Vec<String>,
    outcome: Outcome,
}

enum Outcome {
    Pending,
    Accepted { revision: u64 },
    Rejected { why: String },
}

impl ProposalRecord {
    fn to_json(&self) -> Value {
        let (status, revision, why) = match &self.outcome {
            Outcome::Pending => ("pending", None, None),
            Outcome::Accepted { revision } => ("accepted", Some(*revision), None),
            Outcome::Rejected { why } => ("rejected", None, Some(why.clone())),
        };
        json!({
            "proposal": self.n,
            "author": self.author,
            "changes": self.changes,
            "warnings": self.warnings,
            "status": status,
            "revision": revision,
            "why": why,
        })
    }
}

/// The person's decision on the proposal on the canvas.
#[derive(Message, Debug, Clone)]
pub struct ReviewDecision(pub bool);

/// What a panel draws of the proposal on the canvas, published when one
/// arrives and when it is decided: who proposes, what changes, and the
/// shot list as it would be — what the timeline draws dashed.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct LiveReview {
    pub pending: Option<PendingReview>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingReview {
    pub n: u64,
    pub author: String,
    pub changes: Vec<String>,
    /// The entities on screen it would change or delete, by id.
    pub entities: Vec<u64>,
    /// The proposal's world's shots, in `shot.order`.
    pub shots: Vec<localgpt_previs::ShotRow>,
}

/// What a history panel draws: the log as rows, and where the canvas is.
///
/// Published by `live_world` whenever the history or the canvas's place in it
/// changes, so a panel draws from a snapshot and never reaches into the
/// authority or the view. Rows are in file order.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct LiveHistory {
    pub rows: Vec<HistoryRow>,
    /// The entry the canvas shows, or `None` for the base.
    pub current: Option<String>,
    /// The newest commit or refusal, in one line.
    pub status: Option<String>,
    /// The turns that ended, in order ([`TurnEnded`]).
    pub turns: Vec<TurnMark>,
}

/// An agent's turn ended: whoever hosts its terminal says so, and the
/// history marks the entries since the last turn as this one's.
#[derive(Message, Debug, Clone)]
pub struct TurnEnded {
    /// The agent, by its command's name (`claude`, `codex`, …).
    pub agent: String,
}

/// A turn that ended, as the history marks it: the agent stopped working
/// when the log held `rows` entries, so the entries after the previous mark
/// and up to this one are that turn's.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnMark {
    pub n: u32,
    pub agent: String,
    /// The log's length when the turn ended: the mark sits above row `rows - 1`.
    pub rows: usize,
    /// The head's revision then.
    pub revision: u64,
}

/// One entry of the log, as the history panel shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    pub id: String,
    /// The entry it builds on; `None` for one built on the base.
    pub parent: Option<String>,
    pub revision: u64,
    pub author: String,
    pub summary: String,
    /// The entry's own message, when the author gave one.
    pub message: Option<String>,
    /// When it was written (milliseconds since the epoch).
    pub timestamp_ms: u64,
    /// Nothing builds on this entry: the end of a branch.
    pub tip: bool,
    /// Main's tip — the entry whose world `manifest.json` holds.
    pub head: bool,
}

/// Ask the canvas to show a point in its history (`None`: the base).
///
/// The history panel sends these, and `POST /goto` takes the same path — [`seek`] —
/// so a person and an agent moving through history cannot behave differently.
#[derive(Message, Debug, Clone)]
pub struct SeekTo(pub Option<String>);

/// A replay of the git history: keyframes shown one after another.
struct Replay {
    frames: Vec<live::Snapshot>,
    next: usize,
    timer: Timer,
    shown: wt::WorldManifest,
    /// Each frame's assets as its commit held them, materialized on a
    /// thread ahead of playback: commit → the root to resolve against, or
    /// why there is none.
    roots: std::collections::HashMap<String, Result<PathBuf, String>>,
    assets: Mutex<std::sync::mpsc::Receiver<(String, Result<PathBuf, String>)>>,
}

/// Writes `.live/endpoint.json` for its owner's eyes alone: `.live/` is
/// the owner's (0700) and the file too (0600), made afresh so a file left
/// by a crash keeps no wider mode.
fn write_endpoint(path: &Path, text: &str) -> std::io::Result<()> {
    let folder = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(folder)?;
    let _ = std::fs::remove_file(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))?;
        options.mode(0o600);
    }
    options.open(path)?.write_all(text.as_bytes())
}

/// Removes `.live/endpoint.json` when the app goes away.
#[derive(Resource)]
struct Endpoint(PathBuf);

impl Drop for Endpoint {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Open the folder, start the API, and add the canvas systems.
pub fn setup_live(app: &mut App, dir: &Path, review: bool) -> anyhow::Result<()> {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let world = LiveWorld::open(&dir).map_err(|e| anyhow::anyhow!("{e}"))?;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    // 128 random bits: the token is all that stands between the world and
    // any process on this computer, so nothing about it can be guessed.
    let token: String = rand::random::<[u8; 16]>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let (tx, rx) = channel();
    {
        let token = token.clone();
        std::thread::Builder::new()
            .name("live-api".into())
            .spawn(move || serve(listener, token, tx))?;
    }
    let endpoint = dir.join(live::LIVE).join("endpoint.json");
    write_endpoint(
        &endpoint,
        &serde_json::to_string_pretty(&json!({
            "url": url,
            "token": token,
            "pid": std::process::id(),
            "world": dir,
            "endpoints": endpoints(),
        }))?,
    )?;
    eprintln!(
        "[live] {} · revision {} · API {url} (see {}/AGENTS.md)",
        dir.display(),
        world.revision(),
        dir.display()
    );

    // At main's tip, which is what the scene is built from — once a branch
    // has been committed, the last line is a branch's.
    let view = localgpt_world_editor::Editor::open_at(
        world.base().clone(),
        world.entries().to_vec(),
        world.main_tip().as_deref(),
    )
    .map_err(|e| anyhow::anyhow!("the history doesn't open as a tree: {e}"))?;
    app.insert_resource(Live {
        camera: world.head_manifest().camera.clone(),
        world,
        view,
        calls: Mutex::new(rx),
        started: false,
        guard: Timer::from_seconds(0.5, TimerMode::Repeating),
        preview: None,
        shots: Vec::new(),
        shot_count: 0,
        deferred: Vec::new(),
        replay: None,
        history_dirty: true,
        thumbs_dirty: true,
        status: None,
        review: review || std::env::var("LOCALGPT_REVIEW").is_ok_and(|v| v == "1"),
        proposal: None,
        proposals: Vec::new(),
        synthetic: std::env::var("LOCALGPT_SYNTHETIC_INPUT")
            .is_ok_and(|v| v == "1")
            .then(VecDeque::new),
        turns: Vec::new(),
    })
    .insert_resource(Endpoint(endpoint))
    .insert_resource(LiveEndpoint { url, token })
    .init_resource::<LiveHistory>()
    .init_resource::<LiveReview>()
    .init_resource::<RenderedPrimary>()
    .add_message::<SeekTo>()
    .add_message::<ReviewDecision>()
    .add_message::<TurnEnded>()
    .add_plugins((
        super::canvas_edit::CanvasEditPlugin,
        super::shots::ShotsPlugin,
    ));
    app.add_systems(Update, review_keys.before(live_world))
        // After the frame's last propagation: the pose the renderer gets.
        .add_systems(
            PostUpdate,
            note_rendered.after(bevy::transform::TransformSystems::Propagate),
        )
        .add_systems(
            Update,
            inject_synthetic_input.before(super::canvas_edit::PersonEdits),
        )
        .add_systems(
            Update,
            (live_world, live_view, perform_song)
                .chain()
                .after(super::canvas_edit::PersonEdits),
        );
    if std::env::var("LOCALGPT_OFFSCREEN_SHOTS").is_ok_and(|v| v == "1") {
        app.add_systems(Update, offscreen_canvas);
    }
    Ok(())
}

/// A pending proposal is decided from the keyboard: Enter accepts, Esc
/// rejects. A panel asks the same with [`ReviewDecision`].
fn review_keys(
    live: Res<Live>,
    keys: Res<ButtonInput<KeyCode>>,
    mut decisions: MessageWriter<ReviewDecision>,
) {
    if live.proposal.is_none() {
        return;
    }
    if keys.just_pressed(KeyCode::Enter) {
        decisions.write(ReviewDecision(true));
    } else if keys.just_pressed(KeyCode::Escape) {
        decisions.write(ReviewDecision(false));
    }
}

/// Show an agent's batch on the canvas without committing it: read as the
/// authority would, against the world on screen, applied to the scene, and
/// held for the person's decision.
fn propose(
    live: &mut Live,
    applier: &mut OpsApplier<'_, '_>,
    batch: Value,
    author: String,
) -> Reply {
    if let Some(pending) = &live.proposal {
        return Reply::status(
            409,
            json!({"result": "busy", "pending": pending.n,
                   "why": "an agent's proposal is on the canvas; it decides first"}),
        );
    }
    if live.replay.is_some() {
        return Reply::status(
            409,
            json!({"result": "busy", "why": "a replay is running; batches wait for it"}),
        );
    }
    let at = live.view.tip().map(str::to_string);
    if at.is_none() && !live.world.entries().is_empty() {
        return Reply::status(
            422,
            json!({"result": "rejected",
                   "errors": ["the canvas shows the base, which can't take an edit once there is history"]}),
        );
    }
    // A proposal builds on what the person is looking at — that is what
    // they review. A batch that names another entry is told so.
    if let Some(named) = batch.get("at").and_then(Value::as_str)
        && Some(named) != at.as_deref()
    {
        return Reply::status(
            422,
            json!({"result": "rejected",
                   "errors": [format!("review shows batches on the entry on screen ({}); send without \"at\", or ask the person to seek to {named}",
                                      at.as_deref().unwrap_or("the base"))]}),
        );
    }
    if let Some(expected) = batch.get("expected_revision").and_then(Value::as_u64)
        && expected != live.view.revision()
    {
        return Reply::status(
            422,
            json!({"result": "rejected",
                   "errors": [format!("the world is at revision {}, not {expected}: read it again, then decide again", live.view.revision())]}),
        );
    }
    let author = batch
        .get("author")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or(author);
    let ingested = match openworldformat::ingest(live.view.doc(), &batch) {
        Ok(ingested) => ingested,
        Err(refused) => {
            eprintln!("[live] refused: {}", refused.errors.join(" · "));
            return Reply::status(422, json!({"result": "rejected", "errors": refused.errors}));
        }
    };
    let changes = live::describe_ops(&ingested.ops);
    applier.apply_ops(&for_scene(&ingested.ops, live.world.dir()));
    let n = live.proposals.len() as u64 + 1;
    live.proposals.push(ProposalRecord {
        n,
        author: author.clone(),
        changes: changes.clone(),
        warnings: ingested.warnings.clone(),
        outcome: Outcome::Pending,
    });
    live.status = Some(format!(
        "{author} proposes: {} — Enter accepts, Esc rejects",
        changes.join(" · ")
    ));
    eprintln!("[live] {}", live.status.as_deref().unwrap_or_default());
    live.history_dirty = true;
    live.thumbs_dirty = true;
    live.preview = Some(Timer::from_seconds(0.8, TimerMode::Once));
    let warnings = ingested.warnings.clone();
    live.proposal = Some(Proposal {
        n,
        author,
        at,
        batch,
        shown: ingested.doc.to_manifest(),
        changes: changes.clone(),
    });
    Reply::status(
        202,
        json!({
            "result": "proposed",
            "proposal": n,
            "changes": changes,
            "warnings": warnings,
            "review": "the person sees this on the canvas; poll GET /proposals until it is accepted or rejected",
        }),
    )
}

/// The person's answer to the proposal on the canvas: put the scene back
/// to the document, then commit the batch for real — or not.
fn decide(live: &mut Live, applier: &mut OpsApplier<'_, '_>, accept: bool) {
    let Some(proposal) = live.proposal.take() else {
        return;
    };
    // Back to the document first: a commit moves the canvas by its own
    // delta, and the proposal's ops are already on it.
    let back = live::diff_manifests(&proposal.shown, &live.view.doc().to_manifest());
    applier.apply_ops(&for_scene(&back, live.world.dir()));
    live.thumbs_dirty = true;
    live.history_dirty = true;
    let what = proposal.changes.join(" · ");
    let outcome = if accept {
        match live.world.submit_at(
            proposal.at.as_deref(),
            &proposal.batch,
            &proposal.author,
            None,
        ) {
            Ok(done) => {
                follow(live, applier, &done);
                live.status = Some(status_line(&done, &what));
                eprintln!(
                    "[live] accepted · {}",
                    live.status.as_deref().unwrap_or_default()
                );
                Outcome::Accepted {
                    revision: done.revision,
                }
            }
            Err(refused) => {
                let why = refused.errors.join("; ");
                eprintln!("[live] refused: {why}");
                live.status = Some(format!("refused: {why}"));
                Outcome::Rejected { why }
            }
        }
    } else {
        live.status = Some(format!("rejected {}'s proposal: {what}", proposal.author));
        eprintln!("[live] {}", live.status.as_deref().unwrap_or_default());
        Outcome::Rejected {
            why: "rejected by the person".into(),
        }
    };
    if let Some(record) = live.proposals.iter_mut().find(|r| r.n == proposal.n) {
        record.outcome = outcome;
    }
}

/// Whether `id` hangs under any of `ids` in the document — a delete of the
/// ancestor takes it, so naming it too would no longer apply.
fn has_ancestor_in(doc: &openworldformat::WorldDoc, id: u64, ids: &HashSet<u64>) -> bool {
    let mut current = doc.get(id).and_then(|e| e.parent);
    let mut budget = 4096;
    while let Some(parent) = current {
        if ids.contains(&parent.0) {
            return true;
        }
        budget -= 1;
        if budget == 0 {
            return false;
        }
        current = doc.get(parent.0).and_then(|e| e.parent);
    }
    false
}

/// A name nobody holds: `base`, else `base-2`, `base-3`, …
fn free_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| base.to_string())
}

/// The batch that gathers siblings under a new empty at their centroid:
/// the spawn, then each member re-hung with the position that keeps it
/// where it stands. Members under different parents are refused — a
/// group is a thing in one frame.
fn group_ops(
    doc: &openworldformat::WorldDoc,
    members: &[Named],
) -> Result<(Value, String), String> {
    let mut parent: Option<Option<u64>> = None;
    let mut entities = Vec::with_capacity(members.len());
    for member in members {
        let Some(entity) = doc.get(member.id) else {
            return Err(format!("{} is not in the world on screen", member.name));
        };
        let under = entity.parent.map(|p| p.0);
        match parent {
            None => parent = Some(under),
            Some(seen) if seen != under => {
                return Err("group siblings: the selection hangs under different parents".into());
            }
            _ => {}
        }
        entities.push(entity);
    }
    let parent = parent.flatten();
    let count = entities.len() as f32;
    let mut centroid = [0.0f32; 3];
    for entity in &entities {
        for (axis, value) in centroid.iter_mut().zip(entity.transform.position) {
            *axis += value / count;
        }
    }
    let round = |v: [f32; 3]| v.map(|x| (x * 1000.0).round() / 1000.0);
    let centroid = round(centroid);
    let name = free_name("group", |n| doc.get_by_name(n).is_some());
    let mut ops = vec![json!({"SpawnEntity": {"entity": {
        "name": name, "parent": parent, "transform": {"position": centroid}}}})];
    for entity in &entities {
        let mut position = entity.transform.position;
        for (axis, origin) in position.iter_mut().zip(centroid) {
            *axis -= origin;
        }
        ops.push(json!({"ModifyEntity": {"id": entity.id.0, "patch": {
            "parent": name, "transform": {"position": round(position)}}}}));
    }
    Ok((Value::Array(ops), name))
}

/// Move the canvas to `tip` (`None`: the base) — the one seek path, used by
/// `POST /goto` and the history panel alike. A view move: nothing is written, and a
/// batch still lands on the head unless it names where it builds (`"at"`);
/// the canvas shows history, it does not rewrite it. Returns what changed,
/// for whoever asked.
fn seek(
    live: &mut Live,
    applier: &mut OpsApplier<'_, '_>,
    tip: Option<&str>,
) -> Result<Vec<String>, String> {
    if live.proposal.is_some() {
        return Err("an agent's proposal is on the canvas: accept or reject it first".into());
    }
    let delta = live.view.goto(tip).map_err(|e| e.to_string())?;
    note_camera(live, &delta.ops);
    applier.apply_ops(&for_scene(&delta.ops, live.world.dir()));
    live.preview = Some(Timer::from_seconds(0.8, TimerMode::Once));
    live.history_dirty = true;
    live.thumbs_dirty = true;
    let changes = live::describe_ops(&delta.ops);
    eprintln!(
        "[live] sought {} · {}",
        live.view.tip().unwrap_or("the base"),
        changes.join(" · ")
    );
    Ok(changes)
}

/// Take a commit into the view, and into the scene if the canvas follows
/// it — when it builds on the entry on screen. Returns whether it did.
///
/// The delta comes from the view, not from the batch's ops: the canvas may
/// be scrubbed back, and what moves it from what it shows to the new entry
/// is not what the batch carried. If the view cannot take the entry at all
/// (a log it could not open as a tree), a commit on main falls back to the
/// batch's own ops — right while the canvas sits at the head, the only place
/// it can be without the view — and a branch is not drawn.
fn follow(live: &mut Live, applier: &mut OpsApplier<'_, '_>, done: &live::Committed) -> bool {
    live.history_dirty = true;
    live.thumbs_dirty = true;
    let ops = match live.view.observe(done.entry.clone()) {
        Ok(Some(delta)) => delta.ops,
        Ok(None) => return false,
        Err(e) => {
            eprintln!("[live] the view could not follow: {e}");
            if done.forked {
                return false;
            }
            done.ops.clone()
        }
    };
    note_camera(live, &ops);
    applier.apply_ops(&for_scene(&ops, live.world.dir()));
    live.preview = Some(Timer::from_seconds(0.8, TimerMode::Once));
    true
}

/// A checkout made under the open window: git's `HEAD` is not the commit
/// the committer last made, so the files on disk are another point of the
/// history. Reopen the package there and move the scene to its head — the
/// guard must not put the old head back over it. Returns whether `HEAD` had
/// moved: followed now, or left for the next tick while a drag, a proposal
/// or a replay holds the canvas.
fn follow_git(live: &mut Live, applier: &mut OpsApplier<'_, '_>, busy: bool) -> bool {
    if !live.world.git_moved() {
        return false;
    }
    if busy || live.proposal.is_some() || live.replay.is_some() {
        return true;
    }
    let before = live.view.doc().clone();
    let branch = live.world.git_branch().unwrap_or_else(|| "HEAD".into());
    let short: String = live
        .world
        .git_head()
        .unwrap_or_default()
        .chars()
        .take(7)
        .collect();
    live.history_dirty = true;
    if let Err(e) = live.world.reopen() {
        live.world.acknowledge_git();
        live.status = Some(format!(
            "refused: git moved to {branch} ({short}), which doesn't open as a live package: {e}"
        ));
        eprintln!("[live] {}", live.status.as_deref().unwrap_or_default());
        return true;
    }
    match localgpt_world_editor::Editor::open_at(
        live.world.base().clone(),
        live.world.entries().to_vec(),
        live.world.main_tip().as_deref(),
    ) {
        Ok(view) => live.view = view,
        Err(e) => {
            live.status = Some(format!(
                "refused: the history at {branch} ({short}) doesn't open as a tree: {e}"
            ));
            eprintln!("[live] {}", live.status.as_deref().unwrap_or_default());
            return true;
        }
    }
    let delta = localgpt_world_editor::delta_between(&before, live.view.doc());
    note_camera(live, &delta.ops);
    applier.apply_ops(&for_scene(&delta.ops, live.world.dir()));
    live.thumbs_dirty = true;
    live.preview = Some(Timer::from_seconds(0.8, TimerMode::Once));
    live.status = Some(format!(
        "git moved to {branch} ({short}); the canvas followed to revision {}",
        live.world.revision()
    ));
    eprintln!("[live] {}", live.status.as_deref().unwrap_or_default());
    true
}

/// `POST /undo` and `POST /redo`: `at` (an entry id; the head when absent),
/// `author`, and `own` (only that author's own entries count) — each from
/// the query or the body, a query winning, since the MCP shim sends a body.
fn take_back(live: &mut Live, applier: &mut OpsApplier<'_, '_>, call: &Call, redo: bool) -> Reply {
    let body: Value = serde_json::from_slice(&call.body).unwrap_or(Value::Null);
    let text =
        |key: &str| param(&call.query, key).or_else(|| body.get(key)?.as_str().map(str::to_string));
    let at = text("at");
    let author = text("author").unwrap_or_else(|| "you".into());
    let own = param(&call.query, "own")
        .map(|v| v == "1" || v == "true")
        .or_else(|| body.get("own")?.as_bool())
        .unwrap_or(false);
    let outcome = if redo {
        live.world.redo_at(at.as_deref(), &author, scope_of(own))
    } else {
        live.world
            .undo_scoped(at.as_deref(), &author, scope_of(own))
    };
    match outcome {
        Ok(done) => {
            let on_screen = follow(live, applier, &done);
            // The same receipt a batch gets, so a reader parses one shape.
            Reply::ok(json!({
                "result": "committed",
                "revision": done.revision,
                "id": done.entry.id,
                "parent": done.entry.parent,
                "branch": done.forked,
                "head": live.world.main_tip(),
                "on_screen": on_screen,
                "changes": live::describe_ops(&done.ops),
                "spawned": done.spawned,
                "stored": done.stored,
                "warnings": done.warnings,
            }))
        }
        Err(refused) => Reply::status(409, json!({"result": "rejected", "errors": refused.errors})),
    }
}

fn scope_of(own: bool) -> live::UndoScope {
    if own {
        live::UndoScope::Own
    } else {
        live::UndoScope::Anyone
    }
}

/// The camera the ops set, if any — the last word among them.
fn camera_in(ops: &[wt::EditOp]) -> Option<wt::CameraDef> {
    let mut found = None;
    for op in ops {
        match op {
            wt::EditOp::SetCamera { camera } => found = Some(camera.clone()),
            wt::EditOp::ModifyWorld { patch } => {
                if let Some(Some(camera)) = &patch.camera {
                    found = Some(camera.clone());
                }
            }
            wt::EditOp::Batch { ops } => {
                if let Some(camera) = camera_in(ops) {
                    found = Some(camera);
                }
            }
            _ => {}
        }
    }
    found
}

/// A committed camera op moves the canvas's camera on the next frame — the
/// scene applier leaves cameras alone, since a guest's op is the document's
/// default view, not the host's eyes; here the document is what the person
/// is looking at.
fn note_camera(live: &mut Live, ops: &[wt::EditOp]) {
    if let Some(camera) = camera_in(ops) {
        live.camera = Some(camera);
    }
}

/// A value for a reply, through text: `serde_json::to_value` widens an f32
/// to its f64 (0.55 arrives as 0.550000011920929), while the text form is
/// the f32's shortest decimal — what `manifest.json` holds, so a reader
/// diffing the two sees the same numbers.
fn clean_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_string(value)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The person at the keyboard, as the history names them.
/// The person at the keyboard, as the history names them.
pub fn person() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "you".into())
}

/// One line for the history about a commit.
fn status_line(done: &live::Committed, what: &str) -> String {
    format!(
        "r{} - {} - {}{}",
        done.revision,
        done.entry.author.name,
        what,
        if done.forked { " (a branch)" } else { "" }
    )
}

/// Commit a person's intent at the entry on screen — so an edit while the
/// canvas shows an earlier point starts a branch there — and follow it.
/// A refused gesture is put back from the document.
fn person_edit(
    live: &mut Live,
    applier: &mut OpsApplier<'_, '_>,
    selection: Option<&mut InspectorSelection>,
    intent: PersonIntent,
) {
    live.history_dirty = true;
    live.thumbs_dirty = true;
    // What the gesture or the panel moved ahead of the document, to put
    // back from it on a refusal.
    let restore: Vec<u64> = match &intent {
        PersonIntent::Transform(moves) => moves.iter().map(|m| m.id).collect(),
        PersonIntent::Patch { edits, .. } => edits.iter().map(|e| e.id).collect(),
        PersonIntent::Reparent { moved, .. } => moved.iter().map(|m| m.id).collect(),
        _ => Vec::new(),
    };
    let refuse = |live: &mut Live, applier: &mut OpsApplier<'_, '_>, why: String| {
        let ops: Vec<wt::EditOp> = restore
            .iter()
            .filter_map(|id| live.view.doc().get(*id))
            .map(|entity| wt::EditOp::ModifyEntity {
                id: entity.id,
                patch: wt::EntityPatch {
                    name: Some(entity.name.clone()),
                    transform: Some(entity.transform.clone()),
                    parent: Some(entity.parent),
                    // A shape only where there is one: clearing it would
                    // take a mesh's away.
                    shape: entity.shape.clone().map(Some),
                    material: Some(entity.material.clone()),
                    light: Some(entity.light.clone()),
                    ..Default::default()
                },
            })
            .collect();
        if !ops.is_empty() {
            applier.apply_ops(&for_scene(&ops, live.world.dir()));
        }
        eprintln!("[live] refused: {why}");
        live.status = Some(format!("refused: {why}"));
    };
    if live.replay.is_some() {
        return refuse(
            live,
            applier,
            "a replay is running; edits wait for it".into(),
        );
    }
    if live.proposal.is_some() {
        return refuse(
            live,
            applier,
            "an agent's proposal is on the canvas: accept or reject it first (Enter / Esc)".into(),
        );
    }
    let at = live.view.tip().map(str::to_string);
    if at.is_none() && !live.world.entries().is_empty() {
        // An entry always builds on an entry once there is history; the
        // format has no way to say "on the base" after the first line.
        return refuse(
            live,
            applier,
            "the base can't take an edit once there is history; step to an entry first".into(),
        );
    }
    let author = person();
    let submit = |live: &mut Live, ops: Value, what: &str| {
        let batch = json!({"ops": ops, "message": what});
        live.world
            .submit_at(at.as_deref(), &batch, &author, None)
            .map_err(|r| r.errors.join("; "))
    };
    // After a spawn, select what was made: this one first, the rest after.
    let mut select_name = None;
    let (outcome, what) = match intent {
        PersonIntent::Transform(moves) => {
            let doc = live.view.doc();
            let mut ops = Vec::with_capacity(moves.len());
            let mut verbs: Vec<String> = Vec::new();
            let mut names = Vec::new();
            for moved in &moves {
                let Some(current) = doc.get(moved.id).map(|e| e.transform.clone()) else {
                    return refuse(
                        live,
                        applier,
                        format!("{} is not in the world on screen", moved.name),
                    );
                };
                let patch = gesture_patch(&current, moved.position, moved.turn, moved.scale_by);
                if patch.is_empty() {
                    continue;
                }
                let verb = gesture_verbs(&patch);
                if !verbs.contains(&verb) {
                    verbs.push(verb);
                }
                names.push(moved.name.clone());
                ops.push(json!({"ModifyEntity": {"id": moved.id, "patch": {"transform": patch}}}));
            }
            if ops.is_empty() {
                // A drag that ended where it began.
                return;
            }
            let what = format!("{} {}", verbs.join(", "), names.join(", "));
            (submit(live, Value::Array(ops), &what), what)
        }
        PersonIntent::Delete(targets) => {
            // Deleting a parent takes its children: one named too would no
            // longer apply by the time its turn came.
            let doc = live.view.doc();
            let ids: HashSet<u64> = targets.iter().map(|t| t.id).collect();
            let roots: Vec<&Named> = targets
                .iter()
                .filter(|t| !has_ancestor_in(doc, t.id, &ids))
                .collect();
            let what = format!(
                "delete {}",
                roots
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            let ops: Vec<Value> = roots
                .iter()
                .map(|t| json!({"DeleteEntity": {"id": t.id}}))
                .collect();
            (submit(live, Value::Array(ops), &what), what)
        }
        PersonIntent::Duplicate(ids) => {
            let doc = live.view.doc();
            let mut ops = Vec::with_capacity(ids.len());
            let mut names = Vec::new();
            let mut minted: Vec<String> = Vec::new();
            for id in ids {
                let Some(original) = doc.get(id).cloned() else {
                    return refuse(
                        live,
                        applier,
                        "that entity is not in the world on screen".into(),
                    );
                };
                let name = copy_name(&original.name.0, |n| {
                    doc.get_by_name(n).is_some() || minted.iter().any(|m| m == n)
                });
                let mut entity = serde_json::to_value(&original).unwrap_or_default();
                if let Some(object) = entity.as_object_mut() {
                    object.remove("id");
                    object.insert("name".into(), json!(name));
                }
                let mut position = original.transform.position;
                position[0] += 1.0;
                entity["transform"]["position"] = json!(position);
                names.push(original.name.0.clone());
                if select_name.is_none() {
                    select_name = Some(name.clone());
                }
                minted.push(name);
                ops.push(json!({"SpawnEntity": {"entity": entity}}));
            }
            let what = format!("duplicate {}", names.join(", "));
            (submit(live, Value::Array(ops), &what), what)
        }
        PersonIntent::Group(members) => match group_ops(live.view.doc(), &members) {
            Ok((ops, group)) => {
                let what = format!(
                    "group {} as {group}",
                    members
                        .iter()
                        .map(|m| m.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                select_name = Some(group);
                (submit(live, ops, &what), what)
            }
            Err(why) => return refuse(live, applier, why),
        },
        PersonIntent::Patch { edits, what } => {
            let doc = live.view.doc();
            if let Some(gone) = edits.iter().find(|e| doc.get(e.id).is_none()) {
                return refuse(
                    live,
                    applier,
                    format!("{} is not in the world on screen", gone.name),
                );
            }
            let ops: Vec<Value> = edits
                .iter()
                .map(|e| json!({"ModifyEntity": {"id": e.id, "patch": e.patch}}))
                .collect();
            (submit(live, Value::Array(ops), &what), what)
        }
        PersonIntent::World { patch, what } => (
            submit(live, json!([{"ModifyWorld": {"patch": patch}}]), &what),
            what,
        ),
        PersonIntent::Reparent {
            moved,
            parent,
            keep_offset,
        } => match reparent_ops(live.view.doc(), &moved, parent.as_ref(), keep_offset) {
            // Everything already hangs there.
            Ok(None) => return,
            Ok(Some(ops)) => {
                let names = moved
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let what = match &parent {
                    Some(parent) => format!("put {names} in {}", parent.name),
                    None => format!("unparent {names}"),
                };
                (submit(live, ops, &what), what)
            }
            Err(why) => return refuse(live, applier, why),
        },
        PersonIntent::Undo { own } => (
            live.world
                .undo_scoped(at.as_deref(), &author, scope_of(own))
                .map_err(|r| r.errors.join("; ")),
            "undo".to_string(),
        ),
        PersonIntent::Redo => (
            live.world
                .redo_at(at.as_deref(), &author, live::UndoScope::Own)
                .map_err(|r| r.errors.join("; ")),
            "redo".to_string(),
        ),
    };
    match outcome {
        Ok(done) => {
            follow(live, applier, &done);
            let what = match done.warnings.first() {
                Some(w) if what == "undo" || what == "redo" => w.clone(),
                _ => what,
            };
            live.status = Some(status_line(&done, &what));
            eprintln!("[live] {}", live.status.as_deref().unwrap_or_default());
            if let (Some(name), Some(selection)) = (select_name, selection) {
                let primary = done
                    .spawned
                    .get(&name)
                    .and_then(|id| applier.entity_of(*id));
                let others: Vec<Entity> = done
                    .spawned
                    .iter()
                    .filter(|(n, _)| **n != name)
                    .filter_map(|(_, id)| applier.entity_of(*id))
                    .collect();
                selection.set(primary);
                selection.others = others;
            }
        }
        Err(why) => refuse(live, applier, why),
    }
}

/// A panel's held edit on the canvas: each entity as its patch would
/// leave it, folded as the authority folds a batch, applied to the scene
/// alone. Nothing is committed, and the next thing the canvas applies —
/// the commit the drag lets go as, a seek — shows the document again.
/// While a replay or a proposal holds the canvas, a preview waits for it.
fn person_preview(live: &Live, applier: &mut OpsApplier<'_, '_>, edits: &[Patched]) {
    if live.replay.is_some() || live.proposal.is_some() {
        return;
    }
    let ops: Vec<wt::EditOp> = edits
        .iter()
        .filter_map(|edit| {
            let entity = live.view.doc().get(edit.id)?;
            let shown = live::patched_entity(entity, &edit.patch).ok()?;
            let touched = |field: &str| edit.patch.get(field).is_some();
            Some(wt::EditOp::ModifyEntity {
                id: entity.id,
                patch: wt::EntityPatch {
                    name: touched("name").then(|| shown.name.clone()),
                    transform: touched("transform").then(|| shown.transform.clone()),
                    shape: touched("shape").then(|| shown.shape.clone()),
                    material: touched("material").then(|| shown.material.clone()),
                    light: touched("light").then(|| shown.light.clone()),
                    ..Default::default()
                },
            })
        })
        .collect();
    if !ops.is_empty() {
        applier.apply_ops(&for_scene(&ops, live.world.dir()));
    }
}

/// The batch hanging `moved` under `parent` (`None`: at the top), each
/// standing where it stood: its new transform is its place in the world
/// seen from the new parent — the document's transforms composed, not the
/// scene's, which the song and behaviors move. With `keep_offset` the
/// transforms stay as they are, and the new parent carries them. `None`
/// when everything already hangs there. A parent inside what it would
/// hold is the authority's to refuse, in its own words.
fn reparent_ops(
    doc: &openworldformat::WorldDoc,
    moved: &[Named],
    parent: Option<&Named>,
    keep_offset: bool,
) -> Result<Option<Value>, String> {
    if let Some(parent) = parent
        && doc.get(parent.id).is_none()
    {
        return Err(format!("{} is not in the world on screen", parent.name));
    }
    let under = parent.map(|p| p.id);
    let frame = under.map_or(GlobalTransform::IDENTITY, |id| placed(doc, id));
    let mut ops = Vec::with_capacity(moved.len());
    for m in moved {
        let Some(entity) = doc.get(m.id) else {
            return Err(format!("{} is not in the world on screen", m.name));
        };
        if entity.parent.map(|p| p.0) == under {
            continue;
        }
        let mut patch = json!({ "parent": under });
        if !keep_offset {
            let local = placed(doc, m.id).reparented_to(&frame);
            let t = localgpt_world_bevy::world_transform(&local, entity.transform.visible);
            patch["transform"] = json!({
                "position": t.position.map(|v| rounded(v, 1000.0)),
                "rotation_degrees": t.rotation_degrees.map(|v| rounded(v, 100.0)),
                "scale": t.scale.map(|v| rounded(v, 1000.0)),
            });
        }
        ops.push(json!({"ModifyEntity": {"id": m.id, "patch": patch}}));
    }
    Ok((!ops.is_empty()).then_some(Value::Array(ops)))
}

/// Where an entity stands in the world by its document: its transform and
/// every parent's, composed.
fn placed(doc: &openworldformat::WorldDoc, id: u64) -> GlobalTransform {
    let mut chain = Vec::new();
    let mut at = doc.get(id);
    while let Some(entity) = at {
        chain.push(localgpt_world_bevy::transform(&entity.transform));
        // A folded document has no cycle; this only stops a broken one.
        if chain.len() > doc.len() {
            break;
        }
        at = entity.parent.and_then(|p| doc.get(p.0));
    }
    chain
        .into_iter()
        .rev()
        .fold(GlobalTransform::IDENTITY, |placed, t| {
            placed.mul_transform(t)
        })
}

/// Rounded, then read as the f32's shortest decimal — widening an f32 to
/// JSON's f64 directly writes 2.122999906539917 for 2.123. The `+ 0.0`
/// turns a -0.0 into 0.0.
fn rounded(v: f32, by: f32) -> f64 {
    let r = (v * by).round() / by + 0.0;
    r.to_string().parse().unwrap_or(f64::from(r))
}

/// The transform patch a gesture makes, from the document's current value:
/// the position the canvas left it at, a turn (in the parent's frame), a
/// scale along the entity's own axes. Only what changed, rounded so the
/// log reads (mm, 0.01°).
fn gesture_patch(
    current: &wt::WorldTransform,
    position: Option<[f32; 3]>,
    turn: Quat,
    scale_by: [f32; 3],
) -> serde_json::Map<String, Value> {
    let round = rounded;
    let mut patch = serde_json::Map::new();
    if let Some(p) = position {
        let p = p.map(|v| round(v, 1000.0));
        if p.iter()
            .zip(current.position)
            .any(|(a, b)| (a - f64::from(b)).abs() > 1e-4)
        {
            patch.insert("position".into(), json!(p));
        }
    }
    // A full circle is no turn: the angle to the identity is the short way.
    if turn.angle_between(Quat::IDENTITY).to_degrees() > 1e-3 {
        patch.insert(
            "rotation_degrees".into(),
            json!(turned(current.rotation_degrees, turn).map(|v| round(v, 100.0))),
        );
    }
    if scale_by.iter().any(|by| (by - 1.0).abs() > 1e-4) {
        let scaled: Vec<f64> = current
            .scale
            .iter()
            .zip(scale_by)
            .map(|(s, by)| round(s * by, 1000.0))
            .collect();
        patch.insert("scale".into(), json!(scaled));
    }
    patch
}

/// Apply a turn to XYZ angles. A turn about the vertical on an untilted
/// body is the yaw plus the turn, kept in (-180, 180], so the angles keep
/// their form; otherwise the rotations are composed and read back as the
/// format's XYZ angles — what the canvas previewed either way.
fn turned(rotation: [f32; 3], turn: Quat) -> [f32; 3] {
    let wrap = |d: f32| {
        let w = (d + 180.0).rem_euclid(360.0) - 180.0;
        if w == -180.0 { 180.0 } else { w }
    };
    let (axis, angle) = turn.to_axis_angle();
    let about_vertical = axis.x.abs() < 1e-4 && axis.z.abs() < 1e-4;
    if about_vertical && rotation[0].abs() < 1e-3 && rotation[2].abs() < 1e-3 {
        let yaw = angle.to_degrees() * axis.y.signum();
        return [rotation[0], wrap(rotation[1] + yaw), rotation[2]];
    }
    let [x, y, z] = rotation.map(f32::to_radians);
    let q = turn * Quat::from_euler(EulerRot::XYZ, x, y, z);
    let (x, y, z) = q.to_euler(EulerRot::XYZ);
    [x, y, z].map(f32::to_degrees)
}

fn gesture_verbs(patch: &serde_json::Map<String, Value>) -> String {
    let verbs: Vec<&str> = [
        ("position", "move"),
        ("rotation_degrees", "turn"),
        ("scale", "scale"),
    ]
    .iter()
    .filter(|(key, _)| patch.contains_key(*key))
    .map(|(_, verb)| *verb)
    .collect();
    verbs.join(" and ")
}

/// A free name for a copy: `lighthouse` → `lighthouse 2`, `lighthouse 2` →
/// `lighthouse 3`, skipping any taken.
fn copy_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let (base, first) = match name.rsplit_once(' ') {
        Some((base, n)) => match n.parse::<u32>() {
            Ok(n) => (base, n + 1),
            Err(_) => (name, 2),
        },
        None => (name, 2),
    };
    (first..)
        .map(|n| format!("{base} {n}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| format!("{name} copy"))
}

/// The rail's snapshot of the history, from the view.
fn publish(live: &Live) -> LiveHistory {
    let tips: std::collections::HashSet<&str> = live.view.tips().into_iter().collect();
    let head = live.world.main_tip();
    LiveHistory {
        rows: live
            .view
            .history()
            .iter()
            .map(|e| HistoryRow {
                id: e.id.to_string(),
                parent: e.entry.parent.clone(),
                revision: e.entry.revision,
                author: e.entry.author.name.clone(),
                summary: live::describe_ops(&e.entry.edit_ops()).join(", "),
                message: e.entry.message.clone(),
                timestamp_ms: e.entry.timestamp_ms,
                tip: tips.contains(e.id),
                head: head.as_deref() == Some(e.id),
            })
            .collect(),
        current: live.view.tip().map(str::to_string),
        status: live.status.clone(),
        turns: live.turns.clone(),
    }
}

/// The strip's snapshot of the shot list, from the document on screen:
/// the cameras carrying a `shot`, in `shot.order`, each with its
/// derived row. Same cadence as [`publish`] — a seek shows that
/// point's shots, not the head's.
fn publish_shots(live: &Live) -> super::shots::LiveShots {
    let manifest = live.view.doc().to_manifest();
    let shots = localgpt_previs::shots::shot_list(&manifest)
        .into_iter()
        .filter_map(|row| {
            let entity = live.view.doc().get(row.entity_id)?.clone();
            Some(super::shots::ShotEntry { entity, row })
        })
        .collect();
    super::shots::LiveShots { shots, manifest }
}

/// The review's snapshot: the pending proposal, with the shot list of the
/// world it would make.
fn publish_review(live: &Live) -> LiveReview {
    LiveReview {
        pending: live.proposal.as_ref().map(|proposal| PendingReview {
            n: proposal.n,
            author: proposal.author.clone(),
            changes: proposal.changes.clone(),
            entities: live::diff_manifests(&live.view.doc().to_manifest(), &proposal.shown)
                .iter()
                .filter_map(|op| match op {
                    wt::EditOp::ModifyEntity { id, .. } | wt::EditOp::DeleteEntity { id } => {
                        Some(id.0)
                    }
                    _ => None,
                })
                .collect(),
            shots: localgpt_previs::shots::shot_list(&proposal.shown),
        }),
    }
}

/// The canvas performs its document's song, as a loaded world does: the
/// curves drive the modulations, and the file in the package's `assets/`
/// plays. Seeking to an entry with another song, or none, changes it; an
/// entry that only revises the curves leaves the song playing where it is.
fn perform_song(
    live: Res<Live>,
    shots: Res<super::shots::LiveShots>,
    mut soundtrack: ResMut<localgpt_world_bevy::modulation::Soundtrack>,
    audio: Option<ResMut<super::audio::AudioEngine>>,
) {
    if !shots.is_changed() || soundtrack.def == shots.manifest.soundtrack {
        return;
    }
    let file = |def: &Option<wt::SoundtrackDef>| def.as_ref().and_then(|song| song.path.clone());
    let same_file = file(&soundtrack.def) == file(&shots.manifest.soundtrack);
    soundtrack.def = shots.manifest.soundtrack.clone();
    if same_file {
        return;
    }
    if let Some(mut audio) = audio {
        match file(&soundtrack.def) {
            Some(path) => audio.play_soundtrack(live.world.dir().join("assets").join(path)),
            None => audio.stop_soundtrack(),
        }
    }
}

const ENDPOINTS: &[(&str, &str)] = &[
    (
        "GET /world",
        "the head as a manifest (manifest.json); ?tip=current for the entry the window shows, ?tip=<id> for any entry, ?tip=null for the base",
    ),
    (
        "POST /input",
        "tests only, with LOCALGPT_SYNTHETIC_INPUT=1: {\"events\": [{\"cursor\": [x, y]}, {\"button\": \"left\", \"pressed\": true}, {\"key\": \"Enter\", \"pressed\": false}]} — a person's input synthesized, one event per frame; GET /selection reports the handles' window positions",
    ),
    (
        "POST /ops",
        "a batch of edit ops: [op, ...] or {\"ops\": [...], \"author\": ..., \"message\": ..., \"at\": ..., \"expected_revision\": ...} — \"at\" names the entry to build on (default: the head); an earlier one starts a branch; \"expected_revision\" is the revision you read, and a world that moved since refuses the batch",
    ),
    (
        "POST /undo",
        "undo the head's newest change (appends its inverse): {\"author\": ..., \"own\": true (your own newest), \"at\": id (on that entry's path, as a branch)}",
    ),
    (
        "POST /redo",
        "take the newest undo back (appends the undo's inverse); {\"author\": ..., \"own\": true, \"at\": id} as /undo",
    ),
    (
        "GET /refusals",
        "what was tried outside the API and did not take: direct writes the committer put back (.live/refusals.jsonl, created on the first)",
    ),
    (
        "GET /log",
        "the history: id, parent, revision, author, message, changes — and which entry is the head",
    ),
    (
        "GET /proposals",
        "with review on, a batch waits on the canvas for the person (POST /ops replies 202 \"proposed\"): each proposal's status — pending, accepted with its revision, or rejected with why",
    ),
    (
        "GET /verify",
        "check that the head's fold equals manifest.json and every asset its hash",
    ),
    (
        "GET /screenshot",
        "render the view now; replies with the PNG's path",
    ),
    ("GET /selection", "what the person has selected"),
    (
        "POST /replay",
        "replay the git history commit by commit (?rev=branch&seconds=0.8)",
    ),
    (
        "GET /tips",
        "every branch end in the history, which one is the head, and where the canvas is",
    ),
    (
        "POST /goto",
        "move the canvas to an entry ({\"tip\": \"id\"}, or null for the base) — a view move; nothing is written",
    ),
    (
        "GET /board",
        "the previs board as a printable page: ?tip=<id> (default the entry on screen, \"head\" for main's tip); &vs=<id> puts the two side by side, changed shots marked",
    ),
];

fn endpoints() -> Value {
    Value::Object(
        ENDPOINTS
            .iter()
            .map(|(route, what)| (route.to_string(), json!(what)))
            .collect(),
    )
}

/// The world side: first build, ops, undo, replays, guarding manifest.json.
#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
pub(super) fn live_world(
    time: Res<Time>,
    mut live: ResMut<Live>,
    mut applier: OpsApplier,
    mut snapshot: ResMut<LiveHistory>,
    mut shots_snapshot: ResMut<super::shots::LiveShots>,
    mut review: ResMut<LiveReview>,
    mut thumbs_dirty: ResMut<super::shots::ThumbsDirty>,
    mut seeks: MessageReader<SeekTo>,
    mut intents: MessageReader<PersonIntent>,
    mut previews: MessageReader<PersonPreview>,
    mut shot_ops: MessageReader<super::shots::ShotOps>,
    mut decisions: MessageReader<ReviewDecision>,
    mut selection: Option<ResMut<InspectorSelection>>,
    gesture: Option<Res<super::canvas_edit::CanvasGesture>>,
    mut turns: MessageReader<TurnEnded>,
) {
    let live = &mut *live;
    if !live.started {
        live.started = true;
        // Start empty, as a replay does: the startup scene's defaults would
        // collide with the world's ids.
        applier.rebuild_scene(&[]);
        let manifest = live.world.head_manifest().clone();
        let entities: Vec<wt::WorldEntity> = manifest
            .entities
            .iter()
            .map(|e| for_scene_entity(e, live.world.dir()))
            .collect();
        applier.spawn_all(&entities);
        if let Some(env) = manifest.environment {
            applier.apply_ops(&[wt::EditOp::SetEnvironment { env }]);
        }
        live.preview = Some(Timer::from_seconds(1.5, TimerMode::Once));
        return;
    }

    let calls: Vec<Call> = live
        .calls
        .lock()
        .map(|rx| rx.try_iter().collect())
        .unwrap_or_default();
    for call in calls {
        let (method, path) = (call.method.clone(), call.path.clone());
        let reply = match (method.as_str(), path.as_str()) {
            ("GET", "/" | "") => Reply::ok(json!({ "endpoints": endpoints() })),
            // The head — or `?tip=current`, the entry the window shows,
            // which `manifest.json` is not while the person looks back.
            ("GET", "/world") => match param(&call.query, "tip").as_deref() {
                None | Some("head") => Reply::ok(clean_json(live.world.head_manifest())),
                Some("current") => Reply::ok(clean_json(&live.view.doc().to_manifest())),
                Some("null") | Some("base") => {
                    Reply::ok(clean_json(&live.world.base().to_manifest()))
                }
                // Any entry, folded along its own path — without moving the
                // window. An unknown tip is a 404, never the head by mistake.
                Some(tip) => match openworldformat::fold_path(
                    live.world.base(),
                    live.world.entries(),
                    Some(tip),
                ) {
                    Ok((doc, _)) => Reply::ok(clean_json(&doc.to_manifest())),
                    Err(e) => Reply::status(404, json!({"error": e.to_string()})),
                },
            },
            ("GET", "/board") => match super::board::page(
                &live.world,
                &live.view,
                param(&call.query, "tip").as_deref(),
                param(&call.query, "vs").as_deref(),
            ) {
                Ok(html) => Reply::html(html),
                Err(e) => Reply::status(404, json!({"error": e})),
            },
            // The log, and where the canvas stands in it (`current`: an
            // entry id, or null at the base) — what the history panel's
            // scrubber and playback move.
            ("GET", "/log") => {
                let mut log = history(&live.world, live.view.history());
                if let Value::Object(map) = &mut log {
                    map.insert("current".into(), json!(live.view.tip()));
                }
                Reply::ok(log)
            }
            // How many synthesized events still wait to be emitted, one per
            // frame — a driver waits for zero before it reads, so a slow
            // stretch of frames never makes a check read too early.
            ("GET", "/input") => match &live.synthetic {
                None => Reply::status(
                    404,
                    json!({"error": "no synthetic input (LOCALGPT_SYNTHETIC_INPUT=1)"}),
                ),
                Some(queue) => Reply::ok(json!({"queued": queue.len()})),
            },
            ("GET", "/proposals") => Reply::ok(json!({
                "review": live.review,
                "pending": live.proposal.as_ref().map(|p| p.n),
                "proposals": live.proposals.iter().map(ProposalRecord::to_json).collect::<Vec<_>>(),
            })),
            // A replay holds the canvas: an undo or a redo would commit under
            // it, and the view would apply the delta onto a replayed frame.
            ("POST", "/undo" | "/redo") if live.replay.is_some() => Reply::status(
                409,
                json!({"result": "busy", "why": "a replay is running; edits wait for it"}),
            ),
            ("POST", "/undo" | "/redo" | "/replay") if live.proposal.is_some() => Reply::status(
                409,
                json!({"result": "busy", "pending": live.proposal.as_ref().map(|p| p.n),
                       "why": "an agent's proposal is on the canvas; it decides first"}),
            ),
            ("GET", "/verify") => {
                let problems = live.world.verify();
                Reply::ok(json!({"ok": problems.is_empty(), "problems": problems}))
            }
            ("POST", "/ops") => match serde_json::from_slice::<Value>(&call.body) {
                Err(e) => Reply::status(
                    400,
                    json!({"result": "rejected", "errors": [format!("not JSON: {e}")]}),
                ),
                // With review on, the batch shows on the canvas first and
                // the person decides; the authority sees it on accept.
                Ok(batch) if live.review || live.proposal.is_some() => propose(
                    live,
                    &mut applier,
                    batch,
                    param(&call.query, "author").unwrap_or("agent".into()),
                ),
                Ok(batch) => match live.world.submit_at(
                    param(&call.query, "at").as_deref(),
                    &batch,
                    &param(&call.query, "author").unwrap_or("agent".into()),
                    None,
                ) {
                    Ok(done) => {
                        let on_screen = follow(live, &mut applier, &done);
                        live.status = Some(status_line(
                            &done,
                            &live::describe_ops(&done.ops).join(" · "),
                        ));
                        eprintln!(
                            "[live] revision {}{} · {}",
                            done.revision,
                            if done.forked { " (a branch)" } else { "" },
                            live::describe_ops(&done.ops).join(" · ")
                        );
                        Reply::ok(json!({
                            "result": "committed",
                            "revision": done.revision,
                            "id": done.entry.id,
                            "parent": done.entry.parent,
                            // A branch leaves the head where it was.
                            "branch": done.forked,
                            "head": live.world.main_tip(),
                            // Whether the canvas shows it: not when the person
                            // is looking elsewhere in the history, which is
                            // also what a screenshot would show.
                            "on_screen": on_screen,
                            "changes": live::describe_ops(&done.ops),
                            "spawned": done.spawned,
                            "stored": done.stored,
                            "warnings": done.warnings,
                        }))
                    }
                    Err(refused) => {
                        eprintln!("[live] refused: {}", refused.errors.join(" · "));
                        Reply::status(422, json!({"result": "rejected", "errors": refused.errors}))
                    }
                },
            },
            ("POST", "/undo") => take_back(live, &mut applier, &call, false),
            ("POST", "/redo") => take_back(live, &mut applier, &call, true),
            // What was tried outside the API and did not take — a direct
            // write to the package's files, put back by the committer.
            ("GET", "/refusals") => Reply::ok(json!({
                "refusals": live::refusals(live.world.dir()),
            })),
            ("POST", "/replay") => {
                if !live.world.is_git() {
                    Reply::status(
                        409,
                        json!({"error": "the world isn't a git repository; replay walks its commits"}),
                    )
                } else {
                    let rev = param(&call.query, "rev");
                    let seconds = param(&call.query, "seconds")
                        .and_then(|s| s.parse::<f32>().ok())
                        .unwrap_or(0.8)
                        .clamp(0.05, 10.0);
                    match live::git_history(live.world.dir(), rev.as_deref()) {
                        Ok(frames) if !frames.is_empty() => {
                            let commits: Vec<String> = frames
                                .iter()
                                .map(|f| format!("{} {} — {}", &f.commit[..7], f.author, f.subject))
                                .collect();
                            // The commits' assets, fetched ahead of the frames
                            // on a thread: git is not asked on the frame.
                            let (tx, rx) = std::sync::mpsc::channel();
                            let dir = live.world.dir().to_path_buf();
                            let commits_to_fetch: Vec<String> =
                                frames.iter().map(|f| f.commit.clone()).collect();
                            std::thread::spawn(move || {
                                let into = dir.join(live::LIVE).join("replay");
                                for commit in commits_to_fetch {
                                    let root = live::assets_at(&dir, &commit, &into)
                                        .map_err(|e| e.to_string());
                                    if tx.send((commit, root)).is_err() {
                                        break;
                                    }
                                }
                            });
                            live.replay = Some(Replay {
                                frames,
                                next: 0,
                                timer: Timer::from_seconds(seconds, TimerMode::Repeating),
                                shown: live.world.head_manifest().clone(),
                                roots: std::collections::HashMap::new(),
                                assets: Mutex::new(rx),
                            });
                            Reply::ok(json!({"result": "replaying", "commits": commits}))
                        }
                        Ok(_) => {
                            Reply::status(404, json!({"error": "no commits change manifest.json"}))
                        }
                        Err(e) => Reply::status(400, json!({"error": e.to_string()})),
                    }
                }
            }
            ("GET", "/tips") => Reply::ok(json!({
                "tips": live.view.tips(),
                // Main's tip: what manifest.json holds, and where a batch
                // without "at" lands.
                "head": live.world.main_tip(),
                // What the canvas shows — "at" this to build on it.
                "current": live.view.tip(),
                "revision": live.view.revision(),
            })),
            ("POST", "/goto") => {
                let body: Value = serde_json::from_slice(&call.body).unwrap_or_else(|_| json!({}));
                let asked = if let Some(t) = param(&call.query, "tip") {
                    // `?tip=null` is how a query string says the base.
                    (t != "null").then_some(t)
                } else if let Some(t) = body.get("tip") {
                    if t.is_null() {
                        None
                    } else {
                        match t.as_str() {
                            Some(t) => Some(t.to_string()),
                            None => {
                                let _ = call.reply.send(Reply::status(
                                    400,
                                    json!({"result": "rejected",
                                           "errors": ["\"tip\" is an entry id, or null for the base"]}),
                                ));
                                continue;
                            }
                        }
                    }
                } else {
                    let _ = call.reply.send(Reply::status(
                        400,
                        json!({"result": "rejected",
                               "errors": ["name a tip, or send null for the base"]}),
                    ));
                    continue;
                };
                // `"head"` is main's tip, whatever its id is today.
                let asked = match asked.as_deref() {
                    Some("head") => live.world.main_tip(),
                    _ => asked,
                };
                match seek(live, &mut applier, asked.as_deref()) {
                    Ok(changes) => Reply::ok(json!({
                        "result": "sought",
                        "tip": live.view.tip(),
                        "revision": live.view.revision(),
                        "changes": changes,
                    })),
                    Err(e) => Reply::status(404, json!({"result": "rejected", "errors": [e]})),
                }
            }
            ("GET", "/screenshot" | "/selection") => {
                live.deferred.push(call);
                continue;
            }
            ("POST", "/input") => match live.synthetic.as_mut() {
                None => Reply::status(
                    404,
                    json!({"error": "synthetic input is off: start the canvas with LOCALGPT_SYNTHETIC_INPUT=1 (a test's seam, not an agent's)"}),
                ),
                Some(queue) => match serde_json::from_slice::<Value>(&call.body) {
                    Err(e) => Reply::status(400, json!({"error": format!("not JSON: {e}")})),
                    Ok(body) => {
                        let events: Vec<Result<SyntheticEvent, String>> = body
                            .get("events")
                            .and_then(Value::as_array)
                            .map(|events| events.iter().map(parse_synthetic).collect())
                            .unwrap_or_default();
                        match events.into_iter().collect::<Result<Vec<_>, _>>() {
                            Ok(events) if events.is_empty() => Reply::status(
                                400,
                                json!({"error": "send {\"events\": [...]}, one input each"}),
                            ),
                            Ok(events) => {
                                let queued = events.len();
                                queue.extend(events);
                                Reply::ok(json!({"queued": queued, "pending": queue.len()}))
                            }
                            Err(why) => Reply::status(400, json!({"error": why})),
                        }
                    }
                },
            },
            _ => Reply::status(
                404,
                json!({"error": "no such endpoint", "endpoints": endpoints()}),
            ),
        };
        let _ = call.reply.send(reply);
    }

    // Turns that ended, marked at the log's length now.
    for ended in turns.read() {
        let mark = TurnMark {
            n: live.turns.len() as u32 + 1,
            agent: ended.agent.clone(),
            rows: live.history_len(),
            revision: live.revision(),
        };
        live.turns.push(mark);
        live.history_dirty = true;
    }

    // The person's word on a proposal, before their own edits.
    let decided: Vec<bool> = decisions.read().map(|d| d.0).collect();
    for accept in decided {
        decide(live, &mut applier, accept);
    }

    // A panel's held edits, shown ahead of the document — before the
    // commits, so a drag let go in the same frame lands as committed.
    for preview in previews.read() {
        person_preview(live, &mut applier, &preview.0);
    }

    // A person's edits, committed at the entry on screen.
    let asked: Vec<PersonIntent> = intents.read().cloned().collect();
    for intent in asked {
        person_edit(live, &mut applier, selection.as_deref_mut(), intent);
    }

    // A panel's shot-list edits (a drag to reorder): the same person at
    // the keyboard, committed at the entry on screen like a gesture.
    let asked: Vec<super::shots::ShotOps> = shot_ops.read().cloned().collect();
    for edit in asked {
        live.history_dirty = true;
        let refuse = |live: &mut Live, why: &str| {
            eprintln!("[live] refused: {why}");
            live.status = Some(format!("refused: {why}"));
        };
        if live.replay.is_some() {
            refuse(live, "a replay is running; edits wait for it");
            continue;
        }
        if live.proposal.is_some() {
            refuse(
                live,
                "an agent's proposal is on the canvas: accept or reject it first (Enter / Esc)",
            );
            continue;
        }
        let at = live.view.tip().map(str::to_string);
        if at.is_none() && !live.world.entries().is_empty() {
            refuse(
                live,
                "the base can't take an edit once there is history; step to an entry first",
            );
            continue;
        }
        let batch = json!({"ops": edit.ops, "message": edit.what});
        match live.world.submit_at(at.as_deref(), &batch, &person(), None) {
            Ok(done) => {
                follow(live, &mut applier, &done);
                live.status = Some(status_line(&done, &edit.what));
                eprintln!("[live] {}", live.status.as_deref().unwrap_or_default());
            }
            Err(refused) => refuse(live, &refused.errors.join("; ")),
        }
    }

    // The rail's requests, through the same seek the API uses. A drag in
    // progress holds an entity where the pointer has it, not where the
    // document says: it is released before the canvas moves through
    // history, so the gesture can't land on the wrong entry.
    let asked: Vec<Option<String>> = seeks.read().map(|s| s.0.clone()).collect();
    for tip in asked {
        if gesture.as_ref().is_some_and(|g| g.dragging()) {
            live.status = Some("refused: release the drag before moving through history".into());
            live.history_dirty = true;
            eprintln!("[live] can't seek: a drag is in progress");
            continue;
        }
        if let Err(e) = seek(live, &mut applier, tip.as_deref()) {
            live.status = Some(format!("refused: {e}"));
            live.history_dirty = true;
            eprintln!("[live] can't seek: {e}");
        }
    }
    if live.thumbs_dirty {
        live.thumbs_dirty = false;
        thumbs_dirty.0 = true;
    }
    if live.history_dirty {
        live.history_dirty = false;
        snapshot.set_if_neq(publish(live));
        shots_snapshot.set_if_neq(publish_shots(live));
        review.set_if_neq(publish_review(live));
    }

    // A replay steps keyframe to keyframe, then returns to the head.
    if let Some(replay) = live.replay.as_mut()
        && replay.timer.tick(time.delta()).just_finished()
    {
        let target = match replay.frames.get(replay.next) {
            Some(frame) => {
                eprintln!(
                    "[live] replay {}/{} · {} · {}",
                    replay.next + 1,
                    replay.frames.len(),
                    frame.author,
                    frame.subject
                );
                frame.manifest.clone()
            }
            None => live.world.head_manifest().clone(),
        };
        let ops = live::diff_manifests(&replay.shown, &target);
        // The assets as that commit held them — a replay of an older frame
        // must not show today's bytes for a file that changed since. They
        // were fetched ahead; only when playback outruns git does a frame
        // wait for its own.
        let dir = live.world.dir();
        let root = {
            // The guard lives only here: the replay is cleared further down.
            let fetched = replay.assets.lock();
            if let Ok(fetched) = fetched.as_ref() {
                for (commit, root) in fetched.try_iter() {
                    replay.roots.insert(commit, root);
                }
            }
            match replay.frames.get(replay.next) {
                Some(frame) => {
                    while !replay.roots.contains_key(&frame.commit) {
                        match fetched.as_ref().ok().and_then(|rx| rx.recv().ok()) {
                            Some((commit, root)) => {
                                replay.roots.insert(commit, root);
                            }
                            None => break,
                        }
                    }
                    match replay.roots.get(&frame.commit) {
                        Some(Ok(root)) => root.clone(),
                        Some(Err(e)) => {
                            eprintln!(
                                "[live] replay: that commit's assets couldn't be read ({e}); \
                                 showing the working tree's"
                            );
                            dir.to_path_buf()
                        }
                        None => dir.to_path_buf(),
                    }
                }
                None => dir.to_path_buf(),
            }
        };
        if let Some(camera) = camera_in(&ops) {
            live.camera = Some(camera);
        }
        applier.apply_ops(&for_scene(&ops, &root));
        replay.shown = target;
        replay.next += 1;
        live.thumbs_dirty = true;
        if replay.next > replay.frames.len() {
            eprintln!(
                "[live] replay done — back at revision {}",
                live.world.revision()
            );
            live.replay = None;
        }
    }

    if live.guard.tick(time.delta()).just_finished() {
        // A checkout under the window first: the guard would put the old
        // head back over it. Then a direct write, put back and said so.
        let busy = gesture.as_ref().is_some_and(|g| g.dragging());
        if !follow_git(live, &mut applier, busy)
            && let Some(said) = live.world.guard()
        {
            eprintln!("[live] {said}");
            live.status = Some(format!("refused: {said}"));
            live.history_dirty = true;
        }
    }

    if let Some(timer) = live.preview.as_mut()
        && timer.tick(time.delta()).just_finished()
    {
        live.preview = None;
        let path = live.world.dir().join(live::LIVE).join("preview.png");
        live.shots.push(path);
    }
}

/// Where the scene drew the primary selection last frame: its global
/// transform after modulation and the frame's final propagation. A
/// `GlobalTransform` read in `Update` is not that — the physics schedule
/// propagates transforms itself each fixed step, from the pose the
/// modulation restore left, so in `Update` it is the authored pose on any
/// frame with a step. This is the pose on screen, for a reader without eyes.
#[derive(Resource, Default)]
pub(crate) struct RenderedPrimary(Option<(Entity, Vec3)>);

fn note_rendered(
    selection: Option<Res<InspectorSelection>>,
    globals: Query<&GlobalTransform>,
    mut rendered: ResMut<RenderedPrimary>,
) {
    rendered.0 = selection
        .as_ref()
        .and_then(|s| s.entity)
        .and_then(|e| globals.get(e).ok().map(|g| (e, g.translation())));
}

/// An entity as the scene draws it, for the selection reply: its local and
/// global transform, and whether a modulation or behaviors hold it.
type Drawn = (
    &'static Transform,
    &'static GlobalTransform,
    Option<&'static localgpt_world_bevy::modulation::Modulated>,
    Option<&'static super::behaviors::EntityBehaviors>,
    Option<&'static ChildOf>,
);

/// The view side: the camera, screenshots, the selection.
#[allow(clippy::too_many_arguments)] // a Bevy system; the repo's convention
fn live_view(
    mut live: ResMut<Live>,
    mut commands: Commands,
    mut camera: Query<&mut Transform, With<FlyCam>>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCam>>,
    selection: Option<Res<InspectorSelection>>,
    gen_entities: Query<&GenEntity>,
    handles: Option<Res<super::canvas_edit::Handles>>,
    marquee: Option<Res<super::canvas_edit::CanvasMarquee>>,
    offscreen: Option<Res<super::offscreen::OffscreenRenderTarget>>,
    // The scene's own state of the selection — what is drawn, which the
    // document is not while a modulation or a drag holds it — and the
    // modulation clock, for a test driver without eyes.
    drawn: Query<Drawn, Without<FlyCam>>,
    rendered: Res<RenderedPrimary>,
    time: Res<Time>,
    soundtrack: Option<Res<localgpt_world_bevy::modulation::Soundtrack>>,
) {
    let live = &mut *live;
    if let Some(def) = live.camera.take()
        && let Ok(mut transform) = camera.single_mut()
    {
        *transform = Transform::from_translation(Vec3::from_array(def.position))
            .looking_at(Vec3::from_array(def.look_at), Vec3::Y);
    }

    for call in std::mem::take(&mut live.deferred) {
        let reply = if call.path == "/screenshot" {
            live.shot_count += 1;
            let path = live
                .world
                .dir()
                .join(live::LIVE)
                .join(format!("shot-{}.png", live.shot_count));
            let _ = std::fs::remove_file(&path);
            live.shots.push(path.clone());
            // What the picture shows, which is the view's entry — not the
            // head's, when the person is looking elsewhere in the history.
            Reply {
                status: 200,
                body: json!({
                    "path": path,
                    "revision": live.view.revision(),
                    "tip": live.view.tip(),
                    "at_head": live.view.tip().map(str::to_string) == live.world.main_tip(),
                }),
                content_type: "application/json",
                wait_for: Some(path),
            }
        } else {
            // The entity as the person sees it: on a branch, the head may
            // not have it at all.
            let picked = selection
                .as_ref()
                .and_then(|s| s.entity)
                .and_then(|e| gen_entities.get(e).ok())
                .and_then(|g| live.view.doc().get(g.world_id.0).cloned());
            // The rest of a multi-selection, by id, and where the handles
            // are in the window.
            let others: Vec<u64> = selection
                .as_ref()
                .map(|s| {
                    s.others
                        .iter()
                        .filter_map(|e| gen_entities.get(*e).ok().map(|g| g.world_id.0))
                        .collect()
                })
                .unwrap_or_default();
            Reply::ok(json!({
                "selected": picked,
                "others": others,
                "handles": handle_points(handles.as_deref(), &cameras),
                // A selection box being drawn: [x0, y0, x1, y1] in window points.
                "box": marquee.as_ref().and_then(|m| m.rect()),
                // The primary as the scene draws it.
                "scene": selection
                    .as_ref()
                    .and_then(|s| s.entity)
                    .and_then(|e| drawn.get(e).ok().map(|d| (e, d)))
                    .map(|(e, (transform, global, modulated, behaviors, parent))| json!({
                        "translation": transform.translation.to_array(),
                        // As of this system: on a frame with a physics step,
                        // the authored pose — see `RenderedPrimary`.
                        "global": global.translation().to_array(),
                        // As drawn last frame: modulation included.
                        "rendered": rendered.0.filter(|(re, _)| *re == e).map(|(_, v)| v.to_array()),
                        "modulations": modulated.map(|m| m.defs().count()),
                        "behaviors": behaviors.map(|b| b.behaviors.len()),
                        "parented": parent.is_some(),
                    })),
                "clock": json!({
                    "elapsed": time.elapsed_secs(),
                    "soundtrack_position": soundtrack.as_ref().and_then(|s| s.position),
                    "soundtrack_def": soundtrack.as_ref().is_some_and(|s| s.def.is_some()),
                }),
            }))
        };
        let _ = call.reply.send(reply);
    }

    for path in std::mem::take(&mut live.shots) {
        // The window's surface — or the image the canvas renders into
        // when it has one (headless, or `LOCALGPT_OFFSCREEN_SHOTS=1`).
        let shot = match offscreen.as_ref().and_then(|o| o.image_handle.clone()) {
            Some(image) => Screenshot::image(image),
            None => Screenshot::primary_window(),
        };
        commands.spawn(shot).observe(save_to_disk(path));
    }
}

/// The history as the API reports it.
fn history(world: &LiveWorld, ids: &localgpt_world_editor::History) -> Value {
    let entries: Vec<Value> = world
        .entries()
        .iter()
        .enumerate()
        .map(|(n, entry)| {
            let intent = entry.ops.iter().find_map(|op| match op {
                localgpt_world_sync::SessionOp::Tool(t) if t.tool == "live" => Some(t.args.clone()),
                _ => None,
            });
            let id = ids.id_at(n);
            json!({
                // The id a tip names — the log's own when it carries one, the
                // synthesized `line-<n>` when file order is the chain.
                "id": id,
                // What it builds on; null for the base. Branches are entries
                // whose parent already has a child.
                "parent": id.and_then(|id| ids.parent_of(id)),
                "revision": entry.revision,
                "author": entry.author.name,
                // The message at the top level, as the guide says; the
                // intent keeps it too, with `via`.
                "message": intent.as_ref().and_then(|i| i.get("message").cloned()).unwrap_or(Value::Null),
                "intent": intent,
                "changes": live::describe_ops(&entry.edit_ops()),
            })
        })
        .collect();
    json!({
        "revision": world.revision(),
        "head": world.main_tip(),
        "entries": entries,
    })
}

/// Ops as the scene needs them: asset paths made absolute, since the
/// scene loads files and the world names them relative to `assets/`.
fn for_scene(ops: &[wt::EditOp], dir: &Path) -> Vec<wt::EditOp> {
    ops.iter()
        .map(|op| match op {
            wt::EditOp::SpawnEntity { entity } => wt::EditOp::SpawnEntity {
                entity: for_scene_entity(entity, dir),
            },
            wt::EditOp::ModifyEntity { id, patch } => {
                let mut patch = patch.clone();
                if let Some(Some(material)) = patch.material.as_mut() {
                    absolute_textures(material, dir);
                }
                if let Some(Some(mesh)) = patch.mesh_asset.as_mut() {
                    mesh.path = absolute(&mesh.path, dir);
                }
                wt::EditOp::ModifyEntity { id: *id, patch }
            }
            wt::EditOp::Batch { ops } => wt::EditOp::Batch {
                ops: for_scene(ops, dir),
            },
            other => other.clone(),
        })
        .collect()
}

fn for_scene_entity(entity: &wt::WorldEntity, dir: &Path) -> wt::WorldEntity {
    let mut entity = entity.clone();
    if let Some(material) = entity.material.as_mut() {
        absolute_textures(material, dir);
    }
    if let Some(mesh) = entity.mesh_asset.as_mut() {
        mesh.path = absolute(&mesh.path, dir);
    }
    entity
}

fn absolute_textures(material: &mut wt::MaterialDef, dir: &Path) {
    for slot in [
        &mut material.base_color_texture,
        &mut material.metallic_roughness_texture,
        &mut material.normal_map_texture,
        &mut material.emissive_texture,
    ] {
        if let Some(path) = slot.as_mut() {
            *path = absolute(path, dir);
        }
    }
}

fn absolute(path: &str, dir: &Path) -> String {
    if Path::new(path).is_absolute() {
        path.to_string()
    } else {
        dir.join(live::ASSETS)
            .join(path)
            .to_string_lossy()
            .into_owned()
    }
}

/// A query parameter's value (no percent-decoding beyond `+`; enough here).
fn param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_decode(&v.replace('+', " ")))
    })
}

/// `%3A` → `:` and the like, so an entry id survives a client that encodes
/// its query string. A malformed escape is kept as written.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = s
                .get(i + 1..i + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The API thread: accept, check the token, hand the call to the app,
/// write its answer.
fn serve(listener: TcpListener, token: String, calls: Sender<Call>) {
    for stream in listener.incoming().flatten() {
        let (token, calls) = (token.clone(), calls.clone());
        std::thread::spawn(move || {
            let _ = answer(stream, &token, &calls);
        });
    }
}

fn answer(mut stream: TcpStream, token: &str, calls: &Sender<Call>) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    let mut parts = request.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = target
        .split_once('?')
        .map(|(p, q)| (p.to_string(), q.to_string()))
        .unwrap_or((target.clone(), String::new()));
    let (mut length, mut auth) = (0usize, String::new());
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            match key.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = value.trim().parse().unwrap_or(0),
                "authorization" => auth = value.trim().to_string(),
                _ => {}
            }
        }
    }
    let mut body = vec![0; length.min(16 << 20)];
    reader.read_exact(&mut body)?;

    let reply = if !authorized(&method, &path, &query, &auth, token) {
        Reply::status(
            401,
            json!({"error": "send the token in .live/endpoint.json as `Authorization: Bearer <token>`"}),
        )
    } else {
        let (tx, rx) = channel();
        let _ = calls.send(Call {
            method,
            path,
            query,
            body,
            reply: tx,
        });
        rx.recv_timeout(Duration::from_secs(30))
            .unwrap_or(Reply::status(
                504,
                json!({"error": "the app didn't answer"}),
            ))
    };
    if let Some(file) = &reply.wait_for {
        wait_for_file(file, Duration::from_secs(10));
    }
    let (content_type, text) = if reply.content_type == "application/json" {
        (
            reply.content_type,
            serde_json::to_string_pretty(&reply.body).unwrap_or_default() + "\n",
        )
    } else {
        (
            reply.content_type,
            reply.body.as_str().unwrap_or_default().to_string(),
        )
    };
    let reason = match reply.status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        _ => "Gateway Timeout",
    };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        reply.status,
        text.len()
    )
}

/// The token as a bearer header — or, for a browser opening a board page,
/// which can send no header, in the query: there and nowhere else, since a
/// query is logged and kept in a browser's history.
fn authorized(method: &str, path: &str, query: &str, auth: &str, token: &str) -> bool {
    let board = method == "GET" && path == "/board";
    auth == format!("Bearer {token}") || (board && param(query, "token").as_deref() == Some(token))
}

/// Wait until a file exists and has stopped growing.
fn wait_for_file(path: &Path, limit: Duration) {
    let start = std::time::Instant::now();
    let mut last = None;
    while start.elapsed() < limit {
        let size = std::fs::metadata(path).map(|m| m.len()).ok();
        if size.is_some_and(|s| s > 0) && size == last {
            return;
        }
        last = size;
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform(rotation: [f32; 3]) -> wt::WorldTransform {
        wt::WorldTransform {
            position: [1.0, 0.5, -2.0],
            rotation_degrees: rotation,
            ..Default::default()
        }
    }

    #[test]
    fn reparenting_keeps_a_thing_where_it_stands_unless_it_keeps_its_offset() {
        let manifest: wt::WorldManifest = serde_json::from_value(json!({
            "version": 3,
            "meta": {"name": "harbour"},
            "entities": [
                {"id": 1, "name": "jetty", "transform": {"position": [4.0, 0.0, -3.0],
                 "rotation_degrees": [0.0, 90.0, 0.0], "scale": [2.0, 2.0, 2.0]}},
                {"id": 2, "name": "lantern", "parent": 1, "transform": {"position": [1.0, 0.5, 0.0]}},
                {"id": 3, "name": "posts", "transform": {"position": [0.0, 0.0, 0.0]}}
            ],
            "next_entity_id": 4
        }))
        .unwrap();
        let doc = openworldformat::WorldDoc::from_manifest(&manifest).unwrap();
        let named = |id: u64, name: &str| Named {
            id,
            name: name.into(),
        };
        let lantern = [named(2, "lantern")];

        // Out from under the turned, doubled jetty: where it stood, with
        // the jetty's turn and size now its own.
        let ops = reparent_ops(&doc, &lantern, None, false).unwrap().unwrap();
        let patch = &ops[0]["ModifyEntity"]["patch"];
        assert_eq!(patch["parent"], Value::Null);
        assert_eq!(patch["transform"]["position"], json!([4.0, 1.0, -5.0]));
        assert_eq!(
            patch["transform"]["rotation_degrees"],
            json!([0.0, 90.0, 0.0])
        );
        assert_eq!(patch["transform"]["scale"], json!([2.0, 2.0, 2.0]));

        // Keeping its offset, only the parent changes.
        let ops = reparent_ops(&doc, &lantern, Some(&named(3, "posts")), true)
            .unwrap()
            .unwrap();
        assert_eq!(
            ops,
            json!([{"ModifyEntity": {"id": 2, "patch": {"parent": 3}}}])
        );

        // Already there, nothing to do; a parent that isn't, refused.
        assert!(
            reparent_ops(&doc, &lantern, Some(&named(1, "jetty")), false)
                .unwrap()
                .is_none()
        );
        assert!(reparent_ops(&doc, &lantern, Some(&named(9, "pier")), false).is_err());
    }

    #[test]
    fn a_gesture_patches_only_what_it_changed() {
        let t = transform([0.0, 170.0, 0.0]);
        let yaw = |degrees: f32| Quat::from_rotation_y(degrees.to_radians());
        let same = [1.0; 3];
        // A drag that came back to where it began changes nothing.
        assert!(gesture_patch(&t, Some([1.00001, 0.5, -2.0]), Quat::IDENTITY, same).is_empty());
        let moved = gesture_patch(&t, Some([2.12345, 0.5, -2.0]), Quat::IDENTITY, same);
        assert_eq!(moved.get("position"), Some(&json!([2.123, 0.5, -2.0])));
        assert_eq!(moved.len(), 1);
        // Turning past 180 wraps, keeping the angles' form.
        let turn = gesture_patch(&t, None, yaw(15.0), same);
        assert_eq!(
            turn.get("rotation_degrees"),
            Some(&json!([0.0, -175.0, 0.0]))
        );
        // A full circle is no turn.
        assert!(gesture_patch(&t, None, yaw(360.0), same).is_empty());
        // A turn about X on an untilted body tilts it, and the yaw stays.
        let tilt = gesture_patch(
            &transform([0.0, 30.0, 0.0]),
            None,
            Quat::from_rotation_x(20f32.to_radians()),
            same,
        );
        assert_eq!(
            tilt.get("rotation_degrees"),
            Some(&json!([20.0, 30.0, 0.0]))
        );
        let scaled = gesture_patch(&t, None, Quat::IDENTITY, [1.21; 3]);
        assert_eq!(scaled.get("scale"), Some(&json!([1.21, 1.21, 1.21])));
        // A stretch along one of the entity's own axes leaves the others.
        let stretched = gesture_patch(&t, None, Quat::IDENTITY, [2.0, 1.0, 1.0]);
        assert_eq!(stretched.get("scale"), Some(&json!([2.0, 1.0, 1.0])));
        assert_eq!(gesture_verbs(&moved), "move");
        let mut both = moved.clone();
        both.extend(turn);
        assert_eq!(gesture_verbs(&both), "move and turn");
    }

    #[test]
    fn a_tilted_turn_is_the_rotation_the_canvas_previewed() {
        let tilted = [20.0, 30.0, -10.0];
        let angles = turned(tilted, Quat::from_rotation_y(45f32.to_radians()));
        let rad = |a: [f32; 3]| a.map(f32::to_radians);
        let [x, y, z] = rad(angles);
        let [a, b, c] = rad(tilted);
        let got = Quat::from_euler(EulerRot::XYZ, x, y, z);
        let want =
            Quat::from_rotation_y(45f32.to_radians()) * Quat::from_euler(EulerRot::XYZ, a, b, c);
        assert!(got.angle_between(want) < 1e-3, "{angles:?}");
    }

    #[test]
    fn a_copy_takes_the_next_free_name() {
        let taken = ["lighthouse", "lighthouse 2"];
        let free = |n: &str| taken.contains(&n);
        assert_eq!(copy_name("lighthouse", free), "lighthouse 3");
        assert_eq!(copy_name("lighthouse 2", free), "lighthouse 3");
        assert_eq!(copy_name("jetty", |_| false), "jetty 2");
        assert_eq!(copy_name("pier 9b", |_| false), "pier 9b 2");
    }

    #[test]
    fn the_token_rides_a_query_only_to_a_board_page() {
        let token = "0123456789abcdef0123456789abcdef";
        let bearer = format!("Bearer {token}");
        assert!(authorized("POST", "/ops", "", &bearer, token));
        assert!(authorized(
            "GET",
            "/board",
            &format!("token={token}&tip=head"),
            "",
            token
        ));
        assert!(!authorized(
            "POST",
            "/ops",
            &format!("token={token}"),
            "",
            token
        ));
        assert!(!authorized(
            "GET",
            "/world",
            &format!("token={token}"),
            "",
            token
        ));
        assert!(!authorized("GET", "/board", "token=nope", "", token));
    }

    #[test]
    fn the_endpoint_is_its_owners_alone() {
        let dir = std::env::temp_dir().join(format!("lg-endpoint-{}", std::process::id()));
        let path = dir.join(".live").join("endpoint.json");
        // A file left by a crash, wider than it should be.
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "old").unwrap();
        write_endpoint(&path, r#"{"token": "t"}"#).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"token": "t"}"#);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_query_parameter_reads_back_as_the_client_meant_it() {
        // An entry id through a client that encodes its query string.
        assert_eq!(
            param("at=sha256%3Aabc&author=claude", "at").as_deref(),
            Some("sha256:abc")
        );
        assert_eq!(param("author=a+b", "author").as_deref(), Some("a b"));
        assert_eq!(param("author=a%2Bb", "author").as_deref(), Some("a+b"));
        // Malformed escapes are kept, not dropped.
        assert_eq!(param("x=100%", "x").as_deref(), Some("100%"));
        assert_eq!(param("x=%zz", "x").as_deref(), Some("%zz"));
        assert_eq!(param("y=1", "x"), None);
    }
}
