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
//! rail, `POST /goto`), and a batch that names an earlier entry (`"at"`)
//! starts a branch there while the head — `manifest.json`, main's tip —
//! stays where it was. The canvas follows a commit when it continues what is
//! on screen and otherwise only grows the rail, so an agent working on main
//! never yanks a person off the point they chose.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use localgpt_world_agent::live::{self, LiveWorld};
use localgpt_world_types as wt;
use serde_json::{Value, json};

use super::canvas_edit::PersonIntent;
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
    /// A file the answer names; the API thread waits for it to be written.
    wait_for: Option<PathBuf>,
}

impl Reply {
    fn ok(body: Value) -> Self {
        Self {
            status: 200,
            body,
            wait_for: None,
        }
    }

    fn status(status: u16, body: Value) -> Self {
        Self {
            status,
            body,
            wait_for: None,
        }
    }
}

/// The open live world and the canvas's own bookkeeping.
#[derive(Resource)]
struct Live {
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
    /// The newest thing that happened, for the rail: a commit, or a refusal.
    status: Option<String>,
}

/// What the history rail draws: the log as rows, and where the canvas is.
///
/// Published by `live_world` whenever the history or the canvas's place in it
/// changes, so the rail draws from a snapshot and never reaches into the
/// authority or the view. Rows are in file order.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct LiveHistory {
    pub rows: Vec<HistoryRow>,
    /// The entry the canvas shows, or `None` for the base.
    pub current: Option<String>,
    /// The newest commit or refusal, in one line.
    pub status: Option<String>,
}

/// One entry of the log, as the rail shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRow {
    pub id: String,
    pub revision: u64,
    pub author: String,
    pub summary: String,
    /// Nothing builds on this entry: the end of a branch.
    pub tip: bool,
    /// Main's tip — the entry whose world `manifest.json` holds.
    pub head: bool,
}

/// Ask the canvas to show a point in its history (`None`: the base).
///
/// The rail sends these, and `POST /goto` takes the same path — [`seek`] —
/// so a person and an agent moving through history cannot behave differently.
#[derive(Message, Debug, Clone)]
pub struct SeekTo(pub Option<String>);

/// A replay of the git history: keyframes shown one after another.
struct Replay {
    frames: Vec<live::Snapshot>,
    next: usize,
    timer: Timer,
    shown: wt::WorldManifest,
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
pub fn setup_live(app: &mut App, dir: &Path) -> anyhow::Result<()> {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let world = LiveWorld::open(&dir).map_err(|e| anyhow::anyhow!("{e}"))?;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    let token = localgpt_world_agent::session::sha256_hex(
        format!(
            "{}-{:?}-{}",
            std::process::id(),
            std::time::SystemTime::now(),
            dir.display()
        )
        .as_bytes(),
    )[..32]
        .to_string();
    let (tx, rx) = channel();
    {
        let token = token.clone();
        std::thread::Builder::new()
            .name("live-api".into())
            .spawn(move || serve(listener, token, tx))?;
    }
    let endpoint = dir.join(live::LIVE).join("endpoint.json");
    std::fs::create_dir_all(dir.join(live::LIVE))?;
    std::fs::write(
        &endpoint,
        serde_json::to_string_pretty(&json!({
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
        status: None,
    })
    .insert_resource(Endpoint(endpoint))
    .init_resource::<LiveHistory>()
    .add_message::<SeekTo>()
    .add_plugins((
        super::history_rail::HistoryRailPlugin,
        super::canvas_edit::CanvasEditPlugin,
    ))
    .add_systems(
        Update,
        (live_world, live_view)
            .chain()
            .after(super::canvas_edit::PersonEdits),
    );
    Ok(())
}

/// Move the canvas to `tip` (`None`: the base) — the one seek path, used by
/// `POST /goto` and the rail alike. A view move: nothing is written, and a
/// batch still lands on the head unless it names where it builds (`"at"`);
/// the canvas shows history, it does not rewrite it. Returns what changed,
/// for whoever asked.
fn seek(
    live: &mut Live,
    applier: &mut OpsApplier<'_, '_>,
    tip: Option<&str>,
) -> Result<Vec<String>, String> {
    let delta = live.view.goto(tip).map_err(|e| e.to_string())?;
    applier.apply_ops(&for_scene(&delta.ops, live.world.dir()));
    live.preview = Some(Timer::from_seconds(0.8, TimerMode::Once));
    live.history_dirty = true;
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
    applier.apply_ops(&for_scene(&ops, live.world.dir()));
    live.preview = Some(Timer::from_seconds(0.8, TimerMode::Once));
    true
}

/// The person at the keyboard, as the history names them.
fn person() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "you".into())
}

/// One line for the rail about a commit.
fn status_line(done: &live::Committed, what: &str) -> String {
    format!(
        "r{} · {} · {}{}",
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
    let restore = match &intent {
        PersonIntent::Transform { id, .. } => Some(*id),
        _ => None,
    };
    let refuse = |live: &mut Live, applier: &mut OpsApplier<'_, '_>, why: String| {
        if let Some(entity) = restore.and_then(|id| live.view.doc().get(id)) {
            applier.apply_ops(&[wt::EditOp::ModifyEntity {
                id: entity.id,
                patch: wt::EntityPatch {
                    transform: Some(entity.transform.clone()),
                    ..Default::default()
                },
            }]);
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
    let mut select_name = None;
    let (outcome, what) = match intent {
        PersonIntent::Transform {
            id,
            name,
            position,
            yaw_degrees,
            scale_by,
        } => {
            let Some(current) = live.view.doc().get(id).map(|e| e.transform.clone()) else {
                return refuse(
                    live,
                    applier,
                    format!("{name} is not in the world on screen"),
                );
            };
            let patch = gesture_patch(&current, position, yaw_degrees, scale_by);
            if patch.is_empty() {
                // A drag that ended where it began.
                return;
            }
            let what = format!("{} {name}", gesture_verbs(&patch));
            let ops = json!([{"ModifyEntity": {"id": id, "patch": {"transform": patch}}}]);
            (submit(live, ops, &what), what)
        }
        PersonIntent::Delete { id, name } => {
            let what = format!("delete {name}");
            (
                submit(live, json!([{"DeleteEntity": {"id": id}}]), &what),
                what,
            )
        }
        PersonIntent::Duplicate { id } => {
            let Some(original) = live.view.doc().get(id).cloned() else {
                return refuse(
                    live,
                    applier,
                    "that entity is not in the world on screen".into(),
                );
            };
            let doc = live.view.doc();
            let name = copy_name(&original.name.0, |n| doc.get_by_name(n).is_some());
            let mut entity = serde_json::to_value(&original).unwrap_or_default();
            if let Some(object) = entity.as_object_mut() {
                object.remove("id");
                object.insert("name".into(), json!(name));
            }
            let mut position = original.transform.position;
            position[0] += 1.0;
            entity["transform"]["position"] = json!(position);
            let what = format!("duplicate {}", original.name.0);
            select_name = Some(name);
            (
                submit(live, json!([{"SpawnEntity": {"entity": entity}}]), &what),
                what,
            )
        }
        PersonIntent::Undo => (
            live.world
                .undo_at(at.as_deref(), &author)
                .map_err(|r| r.errors.join("; ")),
            "undo".to_string(),
        ),
    };
    match outcome {
        Ok(done) => {
            follow(live, applier, &done);
            let what = match done.warnings.first() {
                Some(w) if what == "undo" => w.clone(),
                _ => what,
            };
            live.status = Some(status_line(&done, &what));
            eprintln!("[live] {}", live.status.as_deref().unwrap_or_default());
            if let (Some(name), Some(selection)) = (select_name, selection) {
                selection.entity = done
                    .spawned
                    .get(&name)
                    .and_then(|id| applier.entity_of(*id));
            }
        }
        Err(why) => refuse(live, applier, why),
    }
}

/// The transform patch a gesture makes, from the document's current value:
/// the position the canvas left it at, a turn about the vertical, a uniform
/// scale. Only what changed, rounded so the log reads (mm, 0.01°).
fn gesture_patch(
    current: &wt::WorldTransform,
    position: Option<[f32; 3]>,
    yaw_degrees: f32,
    scale_by: f32,
) -> serde_json::Map<String, Value> {
    // Rounded, then read as the f32's shortest decimal — widening an f32
    // to JSON's f64 directly writes 2.122999906539917 for 2.123.
    let round = |v: f32, by: f32| -> f64 {
        let r = (v * by).round() / by;
        r.to_string().parse().unwrap_or(f64::from(r))
    };
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
    if yaw_degrees.rem_euclid(360.0).abs() > 1e-3
        && (yaw_degrees.rem_euclid(360.0) - 360.0).abs() > 1e-3
    {
        patch.insert(
            "rotation_degrees".into(),
            json!(turned(current.rotation_degrees, yaw_degrees).map(|v| round(v, 100.0))),
        );
    }
    if (scale_by - 1.0).abs() > 1e-4 {
        patch.insert(
            "scale".into(),
            json!(current.scale.map(|s| round(s * scale_by, 1000.0))),
        );
    }
    patch
}

/// Turn XYZ angles about the vertical. Without a tilt that is the yaw plus
/// the turn, kept in (-180, 180]; with one, the rotations are composed and
/// read back as the format's XYZ angles — what the canvas previewed.
fn turned(rotation: [f32; 3], yaw: f32) -> [f32; 3] {
    let wrap = |d: f32| {
        let w = (d + 180.0).rem_euclid(360.0) - 180.0;
        if w == -180.0 { 180.0 } else { w }
    };
    if rotation[0].abs() < 1e-3 && rotation[2].abs() < 1e-3 {
        return [rotation[0], wrap(rotation[1] + yaw), rotation[2]];
    }
    let [x, y, z] = rotation.map(f32::to_radians);
    let q = Quat::from_rotation_y(yaw.to_radians()) * Quat::from_euler(EulerRot::XYZ, x, y, z);
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
                revision: e.entry.revision,
                author: e.entry.author.name.clone(),
                summary: live::describe_ops(&e.entry.edit_ops()).join(" · "),
                tip: tips.contains(e.id),
                head: head.as_deref() == Some(e.id),
            })
            .collect(),
        current: live.view.tip().map(str::to_string),
        status: live.status.clone(),
    }
}

const ENDPOINTS: &[(&str, &str)] = &[
    ("GET /world", "the world now (manifest.json)"),
    (
        "POST /ops",
        "a batch of edit ops: [op, ...] or {\"ops\": [...], \"author\": ..., \"message\": ..., \"at\": ...} — \"at\" names the entry to build on (default: the head); an earlier one starts a branch",
    ),
    (
        "POST /undo",
        "undo the head's newest change (appends its inverse); {\"at\": id} undoes on that entry's path instead",
    ),
    (
        "GET /log",
        "the history: id, parent, revision, author, message, changes — and which entry is the head",
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
fn live_world(
    time: Res<Time>,
    mut live: ResMut<Live>,
    mut applier: OpsApplier,
    mut snapshot: ResMut<LiveHistory>,
    mut seeks: MessageReader<SeekTo>,
    mut intents: MessageReader<PersonIntent>,
    mut selection: Option<ResMut<InspectorSelection>>,
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
            ("GET", "/world") => {
                Reply::ok(serde_json::to_value(live.world.head_manifest()).unwrap_or_default())
            }
            ("GET", "/log") => Reply::ok(history(&live.world, live.view.history())),
            ("GET", "/verify") => {
                let problems = live.world.verify();
                Reply::ok(json!({"ok": problems.is_empty(), "problems": problems}))
            }
            ("POST", "/ops") => match serde_json::from_slice::<Value>(&call.body) {
                Err(e) => Reply::status(
                    400,
                    json!({"result": "rejected", "errors": [format!("not JSON: {e}")]}),
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
            ("POST", "/undo") => match live.world.undo_at(
                param(&call.query, "at")
                    .or_else(|| {
                        serde_json::from_slice::<Value>(&call.body)
                            .ok()?
                            .get("at")?
                            .as_str()
                            .map(str::to_string)
                    })
                    .as_deref(),
                &param(&call.query, "author")
                    .or_else(|| {
                        // The MCP shim sends {"author": ...}; a query wins.
                        serde_json::from_slice::<Value>(&call.body)
                            .ok()?
                            .get("author")?
                            .as_str()
                            .map(str::to_string)
                    })
                    .unwrap_or("you".into()),
            ) {
                Ok(done) => {
                    let on_screen = follow(live, &mut applier, &done);
                    Reply::ok(json!({
                        "result": "committed",
                        "revision": done.revision,
                        "id": done.entry.id,
                        "on_screen": on_screen,
                        "changes": live::describe_ops(&done.ops),
                        "warnings": done.warnings,
                    }))
                }
                Err(refused) => {
                    Reply::status(409, json!({"result": "rejected", "errors": refused.errors}))
                }
            },
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
                            live.replay = Some(Replay {
                                frames,
                                next: 0,
                                timer: Timer::from_seconds(seconds, TimerMode::Repeating),
                                shown: live.world.head_manifest().clone(),
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
            _ => Reply::status(
                404,
                json!({"error": "no such endpoint", "endpoints": endpoints()}),
            ),
        };
        let _ = call.reply.send(reply);
    }

    // A person's edits, committed at the entry on screen.
    let asked: Vec<PersonIntent> = intents.read().cloned().collect();
    for intent in asked {
        person_edit(live, &mut applier, selection.as_deref_mut(), intent);
    }

    // The rail's requests, through the same seek the API uses.
    let asked: Vec<Option<String>> = seeks.read().map(|s| s.0.clone()).collect();
    for tip in asked {
        if let Err(e) = seek(live, &mut applier, tip.as_deref()) {
            eprintln!("[live] can't seek: {e}");
        }
    }
    if live.history_dirty {
        live.history_dirty = false;
        snapshot.set_if_neq(publish(live));
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
        applier.apply_ops(&for_scene(&ops, live.world.dir()));
        replay.shown = target;
        replay.next += 1;
        if replay.next > replay.frames.len() {
            eprintln!(
                "[live] replay done — back at revision {}",
                live.world.revision()
            );
            live.replay = None;
        }
    }

    if live.guard.tick(time.delta()).just_finished()
        && let Some(said) = live.world.guard()
    {
        eprintln!("[live] {said}");
    }

    if let Some(timer) = live.preview.as_mut()
        && timer.tick(time.delta()).just_finished()
    {
        live.preview = None;
        let path = live.world.dir().join(live::LIVE).join("preview.png");
        live.shots.push(path);
    }
}

/// The view side: the camera, screenshots, the selection.
fn live_view(
    mut live: ResMut<Live>,
    mut commands: Commands,
    mut camera: Query<&mut Transform, With<FlyCam>>,
    selection: Option<Res<InspectorSelection>>,
    gen_entities: Query<&GenEntity>,
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
            Reply::ok(json!({ "selected": picked }))
        };
        let _ = call.reply.send(reply);
    }

    for path in std::mem::take(&mut live.shots) {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
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

    let authorized =
        auth == format!("Bearer {token}") || param(&query, "token").as_deref() == Some(token);
    let reply = if !authorized {
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
    let text = serde_json::to_string_pretty(&reply.body).unwrap_or_default() + "\n";
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
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
        reply.status,
        text.len()
    )
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
    fn a_gesture_patches_only_what_it_changed() {
        let t = transform([0.0, 170.0, 0.0]);
        // A drag that came back to where it began changes nothing.
        assert!(gesture_patch(&t, Some([1.00001, 0.5, -2.0]), 0.0, 1.0).is_empty());
        let moved = gesture_patch(&t, Some([2.12345, 0.5, -2.0]), 0.0, 1.0);
        assert_eq!(moved.get("position"), Some(&json!([2.123, 0.5, -2.0])));
        assert_eq!(moved.len(), 1);
        // Turning past 180 wraps, keeping the angles' form.
        let turn = gesture_patch(&t, None, 15.0, 1.0);
        assert_eq!(
            turn.get("rotation_degrees"),
            Some(&json!([0.0, -175.0, 0.0]))
        );
        // A full circle is no turn.
        assert!(gesture_patch(&t, None, 360.0, 1.0).is_empty());
        let scaled = gesture_patch(&t, None, 0.0, 1.21);
        assert_eq!(scaled.get("scale"), Some(&json!([1.21, 1.21, 1.21])));
        assert_eq!(gesture_verbs(&moved), "move");
        let mut both = moved.clone();
        both.extend(turn);
        assert_eq!(gesture_verbs(&both), "move and turn");
    }

    #[test]
    fn a_tilted_turn_is_the_rotation_the_canvas_previewed() {
        let tilted = [20.0, 30.0, -10.0];
        let angles = turned(tilted, 45.0);
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
