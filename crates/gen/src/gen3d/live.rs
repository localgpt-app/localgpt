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
}

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

    app.insert_resource(Live {
        camera: world.head_manifest().camera.clone(),
        world,
        calls: Mutex::new(rx),
        started: false,
        guard: Timer::from_seconds(0.5, TimerMode::Repeating),
        preview: None,
        shots: Vec::new(),
        shot_count: 0,
        deferred: Vec::new(),
        replay: None,
    })
    .insert_resource(Endpoint(endpoint))
    .add_systems(Update, (live_world, live_view).chain());
    Ok(())
}

const ENDPOINTS: &[(&str, &str)] = &[
    ("GET /world", "the world now (manifest.json)"),
    (
        "POST /ops",
        "a batch of edit ops: [op, ...] or {\"ops\": [...], \"author\": ..., \"message\": ...}",
    ),
    ("POST /undo", "undo the newest change (appends its inverse)"),
    (
        "GET /log",
        "the history: revision, author, message, changes",
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
fn live_world(time: Res<Time>, mut live: ResMut<Live>, mut applier: OpsApplier) {
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
            ("GET", "/log") => Reply::ok(history(&live.world)),
            ("POST", "/ops") => match serde_json::from_slice::<Value>(&call.body) {
                Err(e) => Reply::status(
                    400,
                    json!({"result": "rejected", "errors": [format!("not JSON: {e}")]}),
                ),
                Ok(batch) => match live.world.submit(
                    &batch,
                    &param(&call.query, "author").unwrap_or("agent".into()),
                    None,
                ) {
                    Ok(done) => {
                        applier.apply_ops(&for_scene(&done.ops, live.world.dir()));
                        live.preview = Some(Timer::from_seconds(0.8, TimerMode::Once));
                        eprintln!(
                            "[live] revision {} · {}",
                            done.revision,
                            live::describe_ops(&done.ops).join(" · ")
                        );
                        Reply::ok(json!({
                            "result": "committed",
                            "revision": done.revision,
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
            ("POST", "/undo") => match live
                .world
                .undo(&param(&call.query, "author").unwrap_or("you".into()))
            {
                Ok(done) => {
                    applier.apply_ops(&for_scene(&done.ops, live.world.dir()));
                    live.preview = Some(Timer::from_seconds(0.8, TimerMode::Once));
                    Reply::ok(json!({
                        "result": "committed",
                        "revision": done.revision,
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
            Reply {
                status: 200,
                body: json!({"path": path, "revision": live.world.revision()}),
                wait_for: Some(path),
            }
        } else {
            let picked = selection
                .as_ref()
                .and_then(|s| s.entity)
                .and_then(|e| gen_entities.get(e).ok())
                .and_then(|g| live.world.head().get(g.world_id.0).cloned());
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
fn history(world: &LiveWorld) -> Value {
    let entries: Vec<Value> = world
        .entries()
        .iter()
        .map(|entry| {
            let intent = entry.ops.iter().find_map(|op| match op {
                localgpt_world_sync::SessionOp::Tool(t) if t.tool == "live" => Some(t.args.clone()),
                _ => None,
            });
            json!({
                "revision": entry.revision,
                "author": entry.author.name,
                "intent": intent,
                "changes": live::describe_ops(&entry.edit_ops()),
            })
        })
        .collect();
    json!({ "revision": world.revision(), "entries": entries })
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
        (k == key).then(|| v.replace('+', " "))
    })
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
