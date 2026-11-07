//! `cargo xtask live-smoke <world>`: the live canvas smoke-tested with no
//! one at the keyboard (it was `scripts/live-smoke.py`).
//!
//! Launches `localgpt-gen --live <world>` and drives it over the live API in
//! `<world>/.live/endpoint.json`, the inspector's WebSocket (port 9877, or
//! `LOCALGPT_INSPECTOR_PORT` for a second canvas on the same desk) and a
//! synthesized hand (`LOCALGPT_SYNTHETIC_INPUT=1`, `POST /input`), and
//! checks what an editor is judged by: selection, proposals rejected and
//! accepted by key, the busy replies while one is pending and the log
//! untouched, the stale-revision refusal, a commit with the right expected
//! revision, handle drags, snapping, key turns, a shift-click
//! multi-selection dragged together, grouping, deleting, the receipt's
//! warnings, branches, undo and redo, the manifest's guard, a modulation,
//! and in a git world a checkout and a replay. Screenshots land in
//! `<world>/.live/` for a person to look at.
//!
//! ```text
//! mkdir demo.world && cp ../openworldformat/examples/hello-world/manifest.json demo.world/
//! cargo run -p localgpt-world-agent --example live -- init demo.world --git
//! cargo xtask live-smoke demo.world --build      # --offscreen on a locked screen
//! ```
//!
//! The world must be a live package with the spec's hello-world entities:
//! a cuboid (10), a capsule (14) and a lantern (100). The canvas is Gen's
//! window, whose every pixel is the world's: nothing here drives a widget.

use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

pub const USAGE: &str = "  live-smoke <world> [--build] [--offscreen] [--binary <gen>]
                   Gen's live canvas driven over its API by a synthesized
                   hand: ~90 checks (see xtask/src/smoke.rs)";

const GRID: f64 = 0.25;

fn wait(seconds: f64) {
    sleep(Duration::from_secs_f64(seconds));
}

/// The live API's address and token, from `.live/endpoint.json`.
struct Endpoint {
    host: String,
    token: String,
}

/// One call to the live API: the status and the JSON body (null when it
/// has none, or the call failed, status 0).
fn call(ep: &Endpoint, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
    let attempt = || -> std::io::Result<(u16, Value)> {
        let mut stream = TcpStream::connect(&ep.host)?;
        stream.set_read_timeout(Some(Duration::from_secs(40)))?;
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n",
            ep.host, ep.token
        );
        if body.is_some() {
            request.push_str(&format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                payload.len()
            ));
        }
        request.push_str("\r\n");
        request.push_str(&payload);
        stream.write_all(request.as_bytes())?;
        let mut reply = Vec::new();
        stream.read_to_end(&mut reply)?;
        let split = reply
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap_or(reply.len());
        let head = String::from_utf8_lossy(&reply[..split]).to_string();
        let mut rest = reply.get(split + 4..).unwrap_or_default().to_vec();
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        if head
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            rest = dechunk(&rest);
        }
        let value = if rest.iter().all(u8::is_ascii_whitespace) {
            Value::Null
        } else {
            serde_json::from_slice(&rest).unwrap_or(Value::Null)
        };
        Ok((status, value))
    };
    attempt().unwrap_or((0, Value::Null))
}

fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(line_end) = body.windows(2).position(|w| w == b"\r\n") {
        let size = usize::from_str_radix(
            String::from_utf8_lossy(&body[..line_end])
                .split(';')
                .next()
                .unwrap_or("0")
                .trim(),
            16,
        )
        .unwrap_or(0);
        body = &body[line_end + 2..];
        if size == 0 || size > body.len() {
            break;
        }
        out.extend_from_slice(&body[..size]);
        body = body.get(size + 2..).unwrap_or_default();
    }
    out
}

/// Bytes no one can guess beforehand: the WebSocket key and mask.
fn random_bytes<const N: usize>() -> [u8; N] {
    use std::hash::{BuildHasher, Hasher};
    let mut out = [0u8; N];
    for (i, chunk) in out.chunks_mut(8).enumerate() {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_usize(i);
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        let bytes = hasher.finish().to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
    out
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| (n << 8) | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// One masked text frame to the inspector's WebSocket, by hand.
fn ws_send(port: u16, message: &str) -> Result<(), String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))
        .map_err(|e| format!("the inspector socket: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let key = base64(&random_bytes::<16>());
    let handshake = format!(
        "GET /ws HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream
        .write_all(handshake.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut reply = [0u8; 4096];
    let n = stream.read(&mut reply).map_err(|e| e.to_string())?;
    let first = String::from_utf8_lossy(&reply[..n])
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    if !format!("{first} ").contains(" 101 ") {
        return Err(format!("no websocket upgrade: {first}"));
    }
    let payload = message.as_bytes();
    let mask = random_bytes::<4>();
    let mut frame = vec![0x81u8];
    match payload.len() {
        n if n < 126 => frame.push(0x80 | n as u8),
        n if n <= 0xffff => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    stream.write_all(&frame).map_err(|e| e.to_string())?;
    wait(0.8);
    Ok(())
}

fn press(key: &str) -> Vec<Value> {
    vec![
        json!({"key": key, "pressed": true}),
        json!({"key": key, "pressed": false}),
    ]
}

fn click(at: (f64, f64)) -> Vec<Value> {
    vec![
        json!({"cursor": [at.0, at.1]}),
        json!({"button": "left", "pressed": true}),
        json!({"button": "left", "pressed": false}),
    ]
}

fn point(v: &Value) -> Option<(f64, f64)> {
    Some((v.get(0)?.as_f64()?, v.get(1)?.as_f64()?))
}

fn vec3(v: &Value, default: [f64; 3]) -> [f64; 3] {
    match v.as_array() {
        Some(a) if a.len() >= 3 => [
            a[0].as_f64().unwrap_or(default[0]),
            a[1].as_f64().unwrap_or(default[1]),
            a[2].as_f64().unwrap_or(default[2]),
        ],
        _ => default,
    }
}

fn position(entity: &Value) -> [f64; 3] {
    vec3(&entity["transform"]["position"], [0.0; 3])
}

/// The app as a subprocess: launched, driven, stopped.
struct Canvas {
    world: PathBuf,
    child: Child,
    ep: Endpoint,
    ws_port: u16,
}

impl Canvas {
    fn launch(
        root: &Path,
        binary: &Path,
        world: &Path,
        review: bool,
        log: File,
        offscreen: bool,
        ws_port: u16,
    ) -> Result<Self, String> {
        let endpoint = world.join(".live/endpoint.json");
        let _ = std::fs::remove_file(&endpoint);
        let mut command = Command::new(binary);
        command
            .arg("--live")
            .arg(world)
            .env("LOCALGPT_SYNTHETIC_INPUT", "1")
            .current_dir(root)
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log);
        if review {
            command.arg("--review");
        }
        if offscreen {
            // A locked or sleeping screen leaves a window's surface black:
            // the canvas renders into an image instead, and screenshots
            // read that.
            command.env("LOCALGPT_OFFSCREEN_SHOTS", "1");
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("{}: {e}", binary.display()))?;
        for _ in 0..240 {
            if let Ok(text) = std::fs::read_to_string(&endpoint)
                && let Ok(found) = serde_json::from_str::<Value>(&text)
            {
                wait(3.0); // the first frames: the scene built, the first pass
                let url = found["url"].as_str().unwrap_or_default();
                return Ok(Self {
                    world: world.to_path_buf(),
                    child,
                    ep: Endpoint {
                        host: url
                            .trim_start_matches("http://")
                            .trim_end_matches('/')
                            .to_string(),
                        token: found["token"].as_str().unwrap_or_default().to_string(),
                    },
                    ws_port,
                });
            }
            if let Ok(Some(status)) = child.try_wait() {
                return Err(format!("the app exited early with {status}"));
            }
            wait(0.5);
        }
        let _ = child.kill();
        Err("no endpoint after 120 s".into())
    }

    /// As a person closes it: asked to stop, killed when it does not.
    fn stop(&mut self) {
        let _ = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            wait(0.1);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
        call(&self.ep, method, path, body)
    }

    fn log_lines(&self) -> usize {
        std::fs::read_to_string(self.world.join("ops.jsonl"))
            .map(|text| text.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0)
    }

    /// A hand's events, one a frame; returns once the canvas has emitted
    /// them all, plus a frame for the last to land. The waits after a call
    /// are settle time for what the events set off, not for the events.
    fn input(&self, events: Vec<Value>) {
        self.call("POST", "/input", Some(&json!({ "events": events })));
        let deadline = Instant::now() + Duration::from_secs(60);
        while Instant::now() < deadline {
            let (status, body) = self.call("GET", "/input", None);
            if status != 200 || body["queued"].as_u64().unwrap_or(0) == 0 {
                break;
            }
            wait(0.05);
        }
        wait(0.05);
    }

    fn selection(&self) -> Value {
        self.call("GET", "/selection", None).1
    }

    fn handles(&self) -> Value {
        self.selection()["handles"].clone()
    }

    /// Selects over the inspector socket; the selection as it then stands.
    fn select(&self, id: i64) -> Value {
        let _ = ws_send(
            self.ws_port,
            &json!({"type": "select_entity", "entity_id": id}).to_string(),
        );
        wait(0.3);
        self.selection()
    }

    fn entity(&self, name: &str) -> Value {
        let (_, world) = self.call("GET", "/world", None);
        world["entities"]
            .as_array()
            .and_then(|all| all.iter().find(|e| e["name"] == name).cloned())
            .unwrap_or(Value::Null)
    }

    /// The viewport, which is the whole window: x across it, y down to its
    /// bottom edge.
    fn free_band(&self) -> (f64, f64, f64) {
        (8.0, 1272.0, 712.0)
    }

    /// The pointer rested in the viewport, away from the handles.
    fn park(&self) {
        let (x0, _, _) = self.free_band();
        self.input(vec![json!({"cursor": [x0 + 24.0, 100.0]})]);
    }

    /// The handle under the pointer once the canvas has drawn a frame with
    /// the pointer there, polled.
    fn hot_after_a_moment(&self) -> Option<String> {
        for _ in 0..15 {
            wait(0.1);
            if let Some(hot) = self.handles()["hot"].as_str() {
                return Some(hot.to_string());
            }
        }
        None
    }

    /// Hover an arrow, press, pull along it, release; the handle under the
    /// pointer at the press.
    fn drag_handle(&self, px: i32, axis: &str) -> Option<String> {
        let step = 10;
        let handles = self.handles();
        let (o, x) = (point(&handles["origin"])?, point(&handles[axis])?);
        let (dx, dy) = (x.0 - o.0, x.1 - o.1);
        let n = dx.hypot(dy).max(1.0);
        let (ux, uy) = (dx / n, dy / n);
        let start = (o.0 + dx * 0.8, o.1 + dy * 0.8);
        self.input(vec![json!({"cursor": [start.0, start.1]})]);
        let hot = self.hot_after_a_moment();
        let mut events = vec![json!({"button": "left", "pressed": true})];
        for i in 1..=px / step {
            let d = f64::from(step * i);
            events.push(json!({"cursor": [start.0 + ux * d, start.1 + uy * d]}));
        }
        events.push(json!({"button": "left", "pressed": false}));
        self.input(events);
        hot
    }

    /// Hover a ring where the API puts it, press, pull along it (across
    /// its spoke), release; the handle under the pointer at the press.
    fn drag_ring(&self, name: &str, px: i32) -> Option<String> {
        let step = 8;
        let handles = self.handles();
        let (o, r) = (point(&handles["origin"])?, point(&handles[name])?);
        let (dx, dy) = (r.0 - o.0, r.1 - o.1);
        let n = dx.hypot(dy).max(1.0);
        let (tx, ty) = (-dy / n, dx / n);
        self.input(vec![json!({"cursor": [r.0, r.1]})]);
        let hot = self.hot_after_a_moment();
        let mut events = vec![json!({"button": "left", "pressed": true})];
        for i in 1..=px / step {
            let d = f64::from(step * i);
            events.push(json!({"cursor": [r.0 + tx * d, r.1 + ty * d]}));
        }
        events.push(json!({"button": "left", "pressed": false}));
        self.input(events);
        hot
    }

    /// Press at `start`, pull to `end`, release, and report the box the
    /// canvas showed mid-drag.
    fn drag_box(&self, start: (f64, f64), end: (f64, f64)) -> Value {
        let steps = 10;
        self.input(vec![
            json!({"cursor": [start.0, start.1]}),
            json!({"button": "left", "pressed": true}),
        ]);
        wait(0.2);
        let mut mid = Value::Null;
        for i in 1..=steps {
            let t = f64::from(i) / f64::from(steps);
            self.input(vec![json!({"cursor": [start.0 + (end.0 - start.0) * t, start.1 + (end.1 - start.1) * t]})]);
            if i == steps / 2 {
                wait(0.2);
                mid = self.selection()["box"].clone();
            }
        }
        wait(0.2);
        self.input(vec![json!({"button": "left", "pressed": false})]);
        mid
    }

    fn screenshot(&self, shots: &mut Vec<String>) -> Option<String> {
        let path = self.call("GET", "/screenshot", None).1["path"]
            .as_str()
            .map(str::to_string);
        shots.extend(path.clone());
        path
    }
}

struct Check {
    failures: Vec<String>,
}

impl Check {
    fn check(&mut self, ok: bool, what: impl Into<String>) {
        let what = what.into();
        println!("{}{what}", if ok { "ok   " } else { "FAIL " });
        if !ok {
            self.failures.push(what);
        }
    }
}

struct Options {
    world: PathBuf,
    build: bool,
    binary: Option<PathBuf>,
    select: i64,
    modify: String,
    other: String,
    offscreen: bool,
}

fn options(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        world: PathBuf::new(),
        build: false,
        binary: None,
        select: 100,
        modify: "cuboid".into(),
        other: "capsule".into(),
        offscreen: false,
    };
    let mut world = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = || iter.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--build" => options.build = true,
            "--offscreen" => options.offscreen = true,
            "--binary" => options.binary = Some(PathBuf::from(value()?)),
            "--select" => options.select = value()?.parse().map_err(|_| "--select takes an id")?,
            "--modify" => options.modify = value()?,
            "--other" => options.other = value()?,
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option {flag:?}\n{USAGE}"));
            }
            path => world = Some(PathBuf::from(path)),
        }
    }
    let world = world.ok_or(format!("usage: cargo xtask live-smoke <world>\n{USAGE}"))?;
    options.world =
        std::fs::canonicalize(&world).map_err(|e| format!("{}: {e}", world.display()))?;
    Ok(options)
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let o = options(args)?;
    let binary = o
        .binary
        .clone()
        .unwrap_or_else(|| root.join("target/debug/localgpt-gen"));
    if o.build || !binary.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let built = Command::new(cargo)
            .args(["build", "-p", "localgpt-gen"])
            .current_dir(root)
            .status()
            .map_err(|e| format!("cargo: {e}"))?;
        if !built.success() {
            return Err("cargo build -p localgpt-gen FAILED".into());
        }
    }
    let ws_port = std::env::var("LOCALGPT_INSPECTOR_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(9877);
    let mut c = Check {
        failures: Vec::new(),
    };
    let mut shots = Vec::new();
    review_on(root, &binary, &o, ws_port, &mut c, &mut shots)?;
    review_off(root, &binary, &o, ws_port, &mut c, &mut shots)?;
    println!(
        "screenshots: {}",
        shots
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    );
    if c.failures.is_empty() {
        println!("all checks passed");
        Ok(())
    } else {
        Err(format!("{} check(s) failed", c.failures.len()))
    }
}

fn batch(modify: &str) -> Value {
    json!({
        "author": "smoke",
        "message": format!("lift and redden {modify}"),
        "ops": [{"ModifyEntity": {"id": modify, "patch": {
            "transform": {"position": [0.0, 3.0, 0.0]},
            "material": {"color": [1.0, 0.1, 0.1, 1.0]}}}}],
    })
}

fn red(entity: &Value) -> bool {
    let want = [1.0, 0.1, 0.1, 1.0];
    entity["material"]["color"].as_array().is_some_and(|color| {
        color.len() == 4
            && color
                .iter()
                .zip(want)
                .all(|(a, b)| (a.as_f64().unwrap_or(f64::NAN) - b).abs() < 1e-3)
    })
}

fn statuses(canvas: &Canvas) -> Vec<String> {
    canvas.call("GET", "/proposals", None).1["proposals"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| p["status"].as_str().unwrap_or_default().to_string())
        .collect()
}

fn panicked(world: &Path, log: &str) -> bool {
    std::fs::read_to_string(world.join(".live").join(log))
        .is_ok_and(|text| text.contains("panicked"))
}

/// Review on, with a synthesized hand: proposals rejected and accepted by
/// key, then one left pending; nothing is written while one waits.
fn review_on(
    root: &Path,
    binary: &Path,
    o: &Options,
    ws_port: u16,
    c: &mut Check,
    shots: &mut Vec<String>,
) -> Result<(), String> {
    let log = File::create(o.world.join(".live/smoke-review.log")).map_err(|e| e.to_string())?;
    let mut canvas = Canvas::launch(root, binary, &o.world, true, log, o.offscreen, ws_port)?;
    let batch = batch(&o.modify);
    {
        let canvas = &canvas;
        let (status, body) = canvas.call("GET", "/", None);
        c.check(
            status == 200 && body["endpoints"].get("GET /proposals").is_some(),
            "the API lists /proposals",
        );
        let selected = canvas.select(o.select)["selected"]["id"].as_i64();
        c.check(
            selected == Some(o.select),
            format!(
                "the inspector socket selects entity {} (got {selected:?})",
                o.select
            ),
        );
        canvas.screenshot(shots);
        let lines = canvas.log_lines();
        let (status, body) = canvas.call("POST", "/ops", Some(&batch));
        c.check(
            status == 202 && body["result"] == "proposed",
            format!(
                "a batch is proposed, not committed ({status} {})",
                body["result"]
            ),
        );
        canvas.input(press("Escape"));
        wait(0.6);
        c.check(
            statuses(canvas) == ["rejected"],
            format!("Esc rejects the proposal ({:?})", statuses(canvas)),
        );
        c.check(
            canvas.log_lines() == lines,
            "a rejected proposal wrote nothing",
        );
        c.check(
            !red(&canvas.entity(&o.modify)),
            "the world keeps its color after a rejection",
        );
        canvas.call("POST", "/ops", Some(&batch));
        canvas.input(press("Enter"));
        wait(0.6);
        c.check(
            statuses(canvas) == ["rejected", "accepted"],
            format!("Enter accepts the proposal ({:?})", statuses(canvas)),
        );
        c.check(
            canvas.log_lines() == lines + 1,
            "an accepted proposal is one entry",
        );
        c.check(
            red(&canvas.entity(&o.modify)),
            "the accepted change is in the world",
        );
        let before = canvas.log_lines();
        let (status, _) = canvas.call("POST", "/ops", Some(&batch));
        c.check(
            status == 202,
            format!("a third batch is proposed ({status})"),
        );
        let (status, _) = canvas.call("POST", "/ops", Some(&batch));
        c.check(
            status == 409,
            format!("a second batch waits for the decision ({status})"),
        );
        let (status, _) = canvas.call("POST", "/undo", Some(&json!({})));
        c.check(
            status == 409,
            format!("an undo waits for the decision ({status})"),
        );
        let (status, _) = canvas.call("POST", "/redo", Some(&json!({})));
        c.check(
            status == 409,
            format!("a redo waits for the decision too ({status})"),
        );
        wait(1.0);
        canvas.screenshot(shots);
        c.check(
            canvas.log_lines() == before,
            "the log is untouched while the proposal is pending",
        );
    }
    canvas.stop();
    c.check(
        !panicked(&o.world, "smoke-review.log"),
        "no panic with review on",
    );
    Ok(())
}

/// Review off: expected revisions, the gestures, the selection, the
/// receipts, branches, undo and redo, the guard, a modulation, and in a
/// git world a checkout and a replay.
fn review_off(
    root: &Path,
    binary: &Path,
    o: &Options,
    ws_port: u16,
    c: &mut Check,
    shots: &mut Vec<String>,
) -> Result<(), String> {
    let log = File::create(o.world.join(".live/smoke-commit.log")).map_err(|e| e.to_string())?;
    let mut canvas = Canvas::launch(root, binary, &o.world, false, log, o.offscreen, ws_port)?;
    gestures(&canvas, o, c, shots);
    canvas.stop();
    c.check(
        !panicked(&o.world, "smoke-commit.log"),
        "no panic with review off",
    );
    Ok(())
}

fn gestures(canvas: &Canvas, o: &Options, c: &mut Check, shots: &mut Vec<String>) {
    let batch = batch(&o.modify);
    let (_, body) = canvas.call("GET", "/log", None);
    let revision = body["revision"].as_i64().unwrap_or(0);
    let mut stale = batch.clone();
    stale["expected_revision"] = json!(revision + 7);
    let (status, body) = canvas.call("POST", "/ops", Some(&stale));
    let names_revision = body["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|e| e.as_str().unwrap_or_default().contains("revision"));
    c.check(
        status == 422 && names_revision,
        "a stale expected_revision is refused, naming the revisions",
    );
    let mut right = batch.clone();
    right["expected_revision"] = json!(revision);
    let (status, body) = canvas.call("POST", "/ops", Some(&right));
    c.check(
        status == 200 && body["result"] == "committed",
        format!("the right expected_revision commits ({status})"),
    );
    let (_, body) = canvas.call("GET", "/log", None);
    c.check(
        body["revision"].as_i64() == Some(revision + 1),
        "the log's revision moved by one",
    );

    // The batch put the entity in front of the camera: select it, drag its
    // X handle 100 px, then turn it twice with the comma key.
    let modified = canvas.entity(&o.modify);
    let modified_id = modified["id"].as_i64().unwrap_or(10);
    let other_id = canvas.entity(&o.other)["id"].as_i64().unwrap_or(14);
    let handles = canvas.select(modified_id)["handles"].clone();
    c.check(
        !handles["x"].is_null(),
        format!("GET /selection reports the handles ({handles})"),
    );
    let before_x = position(&modified)[0];
    let lines = canvas.log_lines();
    let hot = canvas.drag_handle(100, "x");
    c.check(
        hot.as_deref() == Some("x"),
        format!("the pointer over the X arrow makes it hot ({hot:?})"),
    );
    wait(0.15);
    canvas.screenshot(shots); // mid-drag
    wait(1.5);
    let after_x = position(&canvas.entity(&o.modify))[0];
    c.check(
        canvas.log_lines() == lines + 1,
        "the drag committed one entry",
    );
    c.check(
        after_x > before_x + 0.05,
        format!(
            "the drag moved {} along +x ({before_x} -> {after_x})",
            o.modify
        ),
    );
    let lines = canvas.log_lines();
    let mut turns = press("Comma");
    turns.extend(press("Comma"));
    canvas.input(turns);
    wait(1.8);
    let yaw = vec3(
        &canvas.entity(&o.modify)["transform"]["rotation_degrees"],
        [0.0; 3],
    )[1];
    c.check(
        canvas.log_lines() == lines + 1,
        "the key turns committed as one entry",
    );
    c.check(
        (yaw - 30.0).abs() < 0.01,
        format!("two turns are 30 degrees ({yaw})"),
    );
    canvas.screenshot(shots);

    // A ring: the most face-on of the three (its spoke longest on screen),
    // hovered where the API puts it, pulled along the ring: a turn about
    // that axis, committed as one entry.
    let h = canvas.handles();
    let spoke = |name: &str| match (point(&h["origin"]), point(&h[name])) {
        (Some(o), Some(p)) => (p.0 - o.0).hypot(p.1 - o.1),
        _ => 0.0,
    };
    let ring = ["ring_x", "ring_y", "ring_z"]
        .into_iter()
        .max_by(|a, b| spoke(a).total_cmp(&spoke(b)))
        .unwrap_or("ring_x");
    let rotation = |e: &Value| vec3(&e["transform"]["rotation_degrees"], [0.0; 3]);
    let before = rotation(&canvas.entity(&o.modify));
    let lines = canvas.log_lines();
    let hot = canvas.drag_ring(ring, 48);
    c.check(
        hot.as_deref() == Some(ring),
        format!("the pointer over the {ring} ring makes it hot ({hot:?})"),
    );
    wait(1.5);
    let after = rotation(&canvas.entity(&o.modify));
    c.check(
        canvas.log_lines() == lines + 1,
        "the ring drag committed one entry",
    );
    let turned = after
        .iter()
        .zip(before)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    c.check(
        turned > 1.0,
        format!(
            "the ring turned {} about its axis ({before:?} -> {after:?})",
            o.modify
        ),
    );

    // L: the handles follow the entity's own axes. The cuboid is turned
    // now, so its X arrow swings on screen with local axes on, and back
    // with them off.
    let arrow_angle = || {
        let h = canvas.handles();
        let angle = match (point(&h["origin"]), point(&h["x"])) {
            (Some(o), Some(x)) => (x.1 - o.1).atan2(x.0 - o.0).to_degrees(),
            _ => f64::NAN,
        };
        (angle, h["local"].as_bool())
    };
    let (a0, local0) = arrow_angle();
    canvas.input(press("KeyL"));
    wait(0.4);
    let (a1, local1) = arrow_angle();
    canvas.input(press("KeyL"));
    wait(0.4);
    let (a2, local2) = arrow_angle();
    c.check(
        (local0, local1, local2) == (Some(false), Some(true), Some(false)),
        format!("L toggles local axes ({local0:?}, {local1:?}, {local2:?})"),
    );
    c.check(
        (a1 - a0).abs() > 3.0 && (a2 - a0).abs() < 0.5,
        format!("local axes swing the X arrow with the turned cuboid ({a0:.1} -> {a1:.1} -> {a2:.1} deg)"),
    );

    // Alt-drag an arrow scales along the entity's own matching axis.
    let scale = |e: &Value| vec3(&e["transform"]["scale"], [1.0; 3]);
    let scale0 = scale(&canvas.entity(&o.modify));
    let lines = canvas.log_lines();
    canvas.input(vec![json!({"key": "AltLeft", "pressed": true})]);
    let hot = canvas.drag_handle(60, "x");
    canvas.input(vec![json!({"key": "AltLeft", "pressed": false})]);
    wait(1.5);
    let scale1 = scale(&canvas.entity(&o.modify));
    c.check(
        hot.as_deref() == Some("x") && canvas.log_lines() == lines + 1,
        format!("an Alt-drag of the X arrow commits one entry ({hot:?})"),
    );
    c.check(
        scale1[0] > scale0[0] + 0.05
            && (scale1[1] - scale0[1]).abs() < 1e-3
            && (scale1[2] - scale0[2]).abs() < 1e-3,
        format!(
            "it scaled {} along its own x only ({scale0:?} -> {scale1:?})",
            o.modify
        ),
    );

    // Alt + right-drag orbits the camera around the selection: the
    // selected origin keeps its place on screen while the X arrow swings.
    // Then the same drag back, so the view is as it was.
    let orbit = |dx: f64| {
        let mut events = vec![
            json!({"key": "AltLeft", "pressed": true}),
            json!({"cursor": [640.0, 330.0]}),
            json!({"button": "right", "pressed": true}),
        ];
        for i in 1..=8 {
            events.push(json!({"cursor": [640.0 + dx * f64::from(i), 330.0]}));
        }
        events.push(json!({"button": "right", "pressed": false}));
        events.push(json!({"key": "AltLeft", "pressed": false}));
        canvas.input(events);
        wait(0.5);
    };
    let origin = || point(&canvas.handles()["origin"]).unwrap_or((f64::NAN, f64::NAN));
    let (a_before, _) = arrow_angle();
    let o_before = origin();
    orbit(20.0);
    let (a_after, _) = arrow_angle();
    let o_after = origin();
    let drift = (o_after.0 - o_before.0).hypot(o_after.1 - o_before.1);
    c.check(
        drift < 4.0 && (a_after - a_before).abs() > 2.0,
        format!(
            "Alt + right-drag orbits around the selection (origin drifted {drift:.1} px, the X arrow swung {:.1} deg)",
            (a_after - a_before).abs()
        ),
    );
    orbit(-20.0);

    // What a hand would feel, measured: the handles keep about the same
    // size on screen from near and far, and the hover tolerance stays
    // usable: a few pixels off the arrow still finds it, far off does not.
    let arrow_px = || {
        let h = canvas.handles();
        let o = point(&h["origin"]).unwrap_or((f64::NAN, f64::NAN));
        let x = point(&h["x"]).unwrap_or((f64::NAN, f64::NAN));
        ((x.0 - o.0).hypot(x.1 - o.1), o, x)
    };
    let hot_off_arrow = |offset: f64| {
        let (_, o, x) = arrow_px();
        let along = (o.0 + (x.0 - o.0) * 0.6, o.1 + (x.1 - o.1) * 0.6);
        canvas.input(vec![json!({"cursor": [along.0, along.1 + offset]})]);
        wait(0.2);
        canvas.handles()["hot"].as_str().map(str::to_string)
    };
    let (near_len, near_origin, _) = arrow_px();
    c.check(
        hot_off_arrow(8.0).as_deref() == Some("x") && hot_off_arrow(60.0).as_deref() != Some("x"),
        "hover forgives 8 px off the arrow, not 60",
    );
    canvas.park();
    canvas.input(vec![json!({"key": "KeyW", "pressed": true})]);
    wait(0.45);
    canvas.input(vec![json!({"key": "KeyW", "pressed": false})]);
    wait(0.4);
    let (far_len, far_origin, _) = arrow_px();
    let moved = (far_origin.0 - near_origin.0).hypot(far_origin.1 - near_origin.1);
    c.check(
        moved > 5.0 && (far_len - near_len).abs() / near_len < 0.3,
        format!(
            "the handles keep their screen size as the camera moves ({near_len:.0} px -> {far_len:.0} px, the origin shifted {moved:.0} px)"
        ),
    );
    c.check(
        hot_off_arrow(8.0).as_deref() == Some("x") && hot_off_arrow(60.0).as_deref() != Some("x"),
        "hover tolerance holds from the new distance",
    );
    // The same tolerance on an arrow laid along the entity's own, turned
    // axis (L). Off the arrow means perpendicular to it on screen.
    let hot_off_perp = |offset: f64| {
        let (_, o, x) = arrow_px();
        let (dx, dy) = (x.0 - o.0, x.1 - o.1);
        let n = dx.hypot(dy).max(1.0);
        let (px, py) = (-dy / n, dx / n);
        let along = (o.0 + dx * 0.6, o.1 + dy * 0.6);
        canvas.input(vec![
            json!({"cursor": [along.0 + px * offset, along.1 + py * offset]}),
        ]);
        wait(0.2);
        canvas.handles()["hot"].as_str().map(str::to_string)
    };
    canvas.input(press("KeyL"));
    wait(0.4);
    let local_now = canvas.handles()["local"].as_bool();
    let (near_local, far_local) = (hot_off_perp(8.0), hot_off_perp(60.0));
    canvas.input(press("KeyL"));
    wait(0.4);
    c.check(
        local_now == Some(true) && near_local.as_deref() == Some("x") && far_local.as_deref() != Some("x"),
        format!(
            "on local axes the turned X arrow forgives 8 px off it, not 60 (local {local_now:?}, {near_local:?}, {far_local:?})"
        ),
    );
    canvas.park();
    canvas.input(vec![json!({"key": "KeyS", "pressed": true})]);
    wait(0.45);
    canvas.input(vec![json!({"key": "KeyS", "pressed": false})]);
    wait(0.4);

    // Snapping: Y on, a drag up the Y arrow lands on the grid, Y off.
    canvas.input(press("KeyY"));
    wait(0.3);
    let lines = canvas.log_lines();
    let before_y = position(&canvas.entity(&o.modify))[1];
    let hot = canvas.drag_handle(57, "y");
    wait(1.5);
    let snapped_y = position(&canvas.entity(&o.modify))[1];
    let on_grid = (snapped_y / GRID - (snapped_y / GRID).round()).abs() < 1e-3;
    c.check(
        hot.as_deref() == Some("y"),
        format!("the pointer over the Y arrow makes it hot ({hot:?})"),
    );
    c.check(
        canvas.log_lines() == lines + 1 && snapped_y > before_y && on_grid,
        format!(
            "with snapping on, the drag lands on the {GRID} m grid ({before_y} -> {snapped_y})"
        ),
    );
    canvas.input(press("KeyY"));
    wait(0.3);
    canvas.screenshot(shots);

    selection_and_groups(canvas, o, c, shots, modified_id, other_id);
    receipts(canvas, o, c);
    history(canvas, o, c);
    guard_and_modulation(canvas, o, c, shots);
}

/// A shift-click adds a second entity; a drag on it moves both; a Shift box
/// selects; Cmd+G groups; Delete takes the group.
fn selection_and_groups(
    canvas: &Canvas,
    o: &Options,
    c: &mut Check,
    shots: &mut Vec<String>,
    modified_id: i64,
    other_id: i64,
) {
    let shift_click = |at: (f64, f64)| {
        let mut events = vec![json!({"key": "ShiftLeft", "pressed": true})];
        events.extend(click(at));
        events.push(json!({"key": "ShiftLeft", "pressed": false}));
        canvas.input(events);
        wait(0.6);
    };
    let other_at = point(&canvas.select(other_id)["handles"]["origin"]);
    canvas.select(modified_id);
    c.check(
        other_at.is_some(),
        format!("{}'s place on screen is known ({other_at:?})", o.other),
    );
    if let Some(at) = other_at {
        shift_click(at);
    }
    let selection = canvas.selection();
    c.check(
        selection["selected"]["id"].as_i64() == Some(modified_id)
            && selection["others"] == json!([other_id]),
        format!(
            "shift-click adds {} to the selection ({}, {})",
            o.other, selection["selected"]["id"], selection["others"]
        ),
    );
    canvas.screenshot(shots); // two selection boxes
    let x = |name: &str| position(&canvas.entity(name))[0];
    let (x1, x2) = (x(&o.modify), x(&o.other));
    let lines = canvas.log_lines();
    if let Some(at) = other_at {
        let mut events = vec![
            json!({"cursor": [at.0, at.1]}),
            json!({"button": "left", "pressed": true}),
        ];
        for i in 1..=6 {
            events.push(json!({"cursor": [at.0 + 10.0 * f64::from(i), at.1]}));
        }
        events.push(json!({"button": "left", "pressed": false}));
        canvas.input(events);
        wait(1.5);
    }
    let (x1b, x2b) = (x(&o.modify), x(&o.other));
    c.check(
        canvas.log_lines() == lines + 1,
        "the drag of the selection is one entry",
    );
    c.check(
        x2b > x2 + 0.05 && ((x1b - x1) - (x2b - x2)).abs() < 0.05,
        format!("both moved together ({x1}->{x1b}, {x2}->{x2b})"),
    );

    // A box drawn with Shift held around the cuboid and the capsule selects
    // both, whatever else it encloses with them, and the release that ends
    // it is not a click on nothing.
    canvas.input(press("Escape"));
    wait(0.3);
    let a = point(&canvas.select(modified_id)["handles"]["origin"]);
    let pad = match a {
        Some(_) => {
            let h = canvas.handles();
            match (point(&h["origin"]), point(&h["x"])) {
                (Some(o), Some(x)) => 0.9 * (x.0 - o.0).hypot(x.1 - o.1),
                _ => 150.0,
            }
        }
        None => 150.0,
    };
    let b = point(&canvas.select(other_id)["handles"]["origin"]);
    canvas.input(press("Escape"));
    wait(0.3);
    let (mut lo, mut hi, mut mid_box) = ((0.0, 0.0), (0.0, 0.0), Value::Null);
    if let (Some(a), Some(b)) = (a, b) {
        let (x0, x1, y1) = canvas.free_band();
        lo = ((a.0.min(b.0) - pad).max(x0), (a.1.min(b.1) - pad).max(80.0));
        hi = ((a.0.max(b.0) + pad).min(x1), (a.1.max(b.1) + pad).min(y1));
        canvas.input(vec![json!({"key": "ShiftLeft", "pressed": true})]);
        mid_box = canvas.drag_box(lo, hi);
        canvas.input(vec![json!({"key": "ShiftLeft", "pressed": false})]);
        wait(0.8);
    }
    let selection = canvas.selection();
    let mut picked: Vec<i64> = selection["others"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .collect();
    picked.extend(selection["selected"]["id"].as_i64());
    picked.sort();
    c.check(
        picked.contains(&modified_id) && picked.contains(&other_id),
        format!(
            "a Shift-box around {} and {} selects both ({picked:?}; box {lo:?} -> {hi:?}, mid-drag {mid_box}, a {a:?}, b {b:?})",
            o.modify, o.other
        ),
    );
    // Back to just the pair, for the grouping below.
    canvas.select(modified_id);
    wait(0.3);
    let other_at = point(&canvas.select(other_id)["handles"]["origin"]);
    canvas.select(modified_id);
    wait(0.3);
    if let Some(at) = other_at {
        shift_click(at);
    }
    let pair = canvas.selection();
    c.check(
        pair["selected"]["id"].as_i64() == Some(modified_id) && pair["others"] == json!([other_id]),
        format!(
            "the pair is selected again ({}, {})",
            pair["selected"]["id"], pair["others"]
        ),
    );
    // Cmd+G groups them under a new empty; Delete takes the group.
    let lines = canvas.log_lines();
    let mut events = vec![json!({"key": "SuperLeft", "pressed": true})];
    events.extend(press("KeyG"));
    events.push(json!({"key": "SuperLeft", "pressed": false}));
    canvas.input(events);
    wait(1.0);
    let group = canvas.entity("group");
    let parents = [
        canvas.entity(&o.modify)["parent"].clone(),
        canvas.entity(&o.other)["parent"].clone(),
    ];
    c.check(
        canvas.log_lines() == lines + 1
            && !group["id"].is_null()
            && parents.iter().all(|p| *p == group["id"]),
        format!(
            "Cmd+G groups the selection under 'group' ({}, parents {parents:?})",
            group["id"]
        ),
    );
    let selected = canvas.selection()["selected"]["name"].clone();
    c.check(
        selected == "group",
        format!("the new group is selected ({selected})"),
    );
    canvas.screenshot(shots);
    let lines = canvas.log_lines();
    canvas.input(press("Delete"));
    wait(1.0);
    let gone = canvas.entity("group").is_null()
        && canvas.entity(&o.modify).is_null()
        && canvas.entity(&o.other).is_null();
    c.check(
        canvas.log_lines() == lines + 1 && gone,
        "Delete takes the group and what hangs under it, as one entry",
    );
    canvas.screenshot(shots);
}

fn errors_name(body: &Value, word: &str) -> bool {
    body["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|e| e.as_str().unwrap_or_default().contains(word))
}

/// The receipts: a delete's warning, targets by name, the package's
/// verification, the numbers /world prints, an orbit's center.
fn receipts(canvas: &Canvas, o: &Options, c: &mut Check) {
    let (status, body) = canvas.call(
        "POST",
        "/ops",
        Some(&json!({"author": "smoke", "ops": [
        {"SpawnEntity": {"entity": {"name": "smoke-watcher", "shape": {"Sphere": {"radius": 0.3}},
                                    "transform": {"position": [2.0, 1.0, 2.0]},
                                    "behaviors": [{"LookAt": {"target": o.select}}]}}}]})),
    );
    c.check(
        status == 200,
        format!(
            "a watcher that looks at entity {} spawns ({status} {})",
            o.select, body["errors"]
        ),
    );
    let (status, body) = canvas.call(
        "POST",
        "/ops",
        Some(&json!({"author": "smoke", "ops": [{"DeleteEntity": {"id": o.select}}]})),
    );
    let warned = body["warnings"].as_array().into_iter().flatten().any(|w| {
        let w = w.as_str().unwrap_or_default();
        w.contains("targeting") && w.contains("not in the world")
    });
    c.check(
        status == 200 && warned,
        format!(
            "deleting its target lands, and the receipt warns ({})",
            body["warnings"]
        ),
    );
    // A target sent by name commits as the id; a name nothing has refuses
    // the batch, naming it.
    let (status, _) = canvas.call(
        "POST",
        "/ops",
        Some(&json!({"author": "smoke", "ops": [
        {"SpawnEntity": {"entity": {"name": "smoke-gazer", "shape": {"Sphere": {"radius": 0.2}},
                                    "transform": {"position": [-6.0, 1.0, 2.0]},
                                    "behaviors": [{"LookAt": {"target": "sphere"}}]}}}]})),
    );
    let target = canvas.entity("smoke-gazer")["behaviors"][0]["LookAt"]["target"].clone();
    c.check(
        status == 200 && target == 11,
        format!("a LookAt target sent by name commits as the id ({target})"),
    );
    let (status, body) = canvas.call("GET", "/verify", None);
    c.check(
        status == 200 && body["ok"] == true,
        format!(
            "the package verifies against its history ({})",
            body["problems"]
        ),
    );
    let (status, body) = canvas.call("POST", "/ops", Some(&json!({"author": "smoke", "ops": [
        {"ModifyEntity": {"id": "smoke-gazer", "patch": {"behaviors": [{"LookAt": {"target": "nobody"}}]}}}]})));
    c.check(
        status == 422 && errors_name(&body, "nobody"),
        format!(
            "a target nothing is named refuses the batch, naming it ({status} {})",
            body["errors"]
        ),
    );
    let (status, body) = canvas.call("GET", "/world", None);
    let noisy: Vec<f64> = body["entities"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|e| {
            e["transform"]["position"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|v| v.as_f64())
        .filter(|v| v.fract() != 0.0 && format!("{v:?}").len() > 12)
        .collect();
    c.check(
        status == 200 && noisy.is_empty(),
        format!(
            "GET /world prints the numbers manifest.json prints ({:?})",
            &noisy[..noisy.len().min(3)]
        ),
    );
    // An orbit around an entity resolves by the id the op committed with:
    // the orbiter's place on screen is by the sphere's, far from the torus
    // at the origin, and it moves.
    let (status, body) = canvas.call("POST", "/ops", Some(&json!({"author": "smoke", "ops": [
        {"SpawnEntity": {"entity": {"name": "smoke-moon", "shape": {"Sphere": {"radius": 0.15}},
                                    "transform": {"position": [-12.0, 2.0, 0.0]},
                                    "behaviors": [{"Orbit": {"center": "sphere", "radius": 1.0, "speed": 90}}]}}}]})));
    let moon_id = body["spawned"]["smoke-moon"].as_i64();
    c.check(
        status == 200 && moon_id.is_some(),
        format!(
            "an orbiter around the sphere spawns ({status} {})",
            body["errors"]
        ),
    );
    let sphere_at = point(&canvas.select(11)["handles"]["origin"]);
    let torus_at = point(&canvas.select(15)["handles"]["origin"]);
    let moon_at = moon_id.and_then(|id| point(&canvas.select(id)["handles"]["origin"]));
    wait(0.5);
    let moon_later = point(&canvas.handles()["origin"]);
    let px = |a: Option<(f64, f64)>, b: Option<(f64, f64)>| match (a, b) {
        (Some(a), Some(b)) => (a.0 - b.0).hypot(a.1 - b.1),
        _ => f64::INFINITY,
    };
    c.check(
        px(moon_at, sphere_at) < 160.0 && px(moon_at, torus_at) > 250.0,
        format!(
            "the orbiter circles the sphere, not the origin ({:.0} px from the sphere, {:.0} px from the torus)",
            px(moon_at, sphere_at),
            px(moon_at, torus_at)
        ),
    );
    c.check(
        px(moon_at, moon_later) > 2.0,
        format!(
            "and it moves ({:.1} px in half a second)",
            px(moon_at, moon_later)
        ),
    );
}

/// The log's reads, the undo receipt, `at: null`, branches, and undo and
/// redo by person and by API.
fn history(canvas: &Canvas, o: &Options, c: &mut Check) {
    let log = canvas.call("GET", "/log", None).1;
    let first = log["entries"][0].clone();
    c.check(
        first.get("message").is_some() && first["message"] == first["intent"]["message"],
        format!(
            "a log entry carries its message at the top level ({})",
            first["message"]
        ),
    );
    let names = |body: &Value| -> Vec<String> {
        body["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| e["name"].as_str().map(str::to_string))
            .collect()
    };
    let first_id = first["id"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| first["id"].to_string());
    let (status, body) = canvas.call("GET", &format!("/world?tip={first_id}"), None);
    let then = names(&body);
    c.check(
        status == 200
            && !then.iter().any(|n| n == "smoke-moon")
            && then.iter().any(|n| n == "cuboid"),
        format!(
            "GET /world?tip=<id> folds the world at that entry ({status}, {} entities)",
            then.len()
        ),
    );
    let (status, _) = canvas.call("GET", "/world?tip=no-such-entry", None);
    c.check(
        status == 404,
        format!("an unknown tip is a 404, not the head ({status})"),
    );
    let (status, body) = canvas.call("POST", "/undo", Some(&json!({"author": "smoke"})));
    c.check(
        status == 200
            && body["head"] == body["id"]
            && !body["parent"].is_null()
            && body["branch"] == false,
        format!("an undo receipt names parent, head and branch like a batch's ({body})"),
    );
    let (status, body) = canvas.call(
        "POST",
        "/ops",
        Some(&json!({"author": "smoke", "at": null,
        "ops": [{"SpawnEntity": {"entity": {"name": "smoke-stray"}}}]})),
    );
    c.check(
        status == 422 && errors_name(&body, "base") && canvas.entity("smoke-stray")["id"].is_null(),
        format!(
            "at null is refused once there is history, not landed on main ({status} {})",
            body["errors"]
        ),
    );
    // Undo and redo are per path: a branch's undo is the branch's child,
    // its redo too, and main's own redo is untouched by them.
    let (status, body) = canvas.call("POST", "/ops", Some(&json!({"author": "smoke", "at": first["id"], "ops": [
        {"SpawnEntity": {"entity": {"name": "smoke-branch-post", "shape": {"Cylinder": {"radius": 0.05, "height": 2.0}},
                                    "transform": {"position": [4.0, 1.0, 4.0]}}}}]})));
    let (b1, b1_rev) = (body["id"].clone(), body["revision"].clone());
    c.check(
        status == 200 && body["branch"] == true && !b1.is_null(),
        format!(
            "a batch at the first entry starts a branch ({status} branch={})",
            body["branch"]
        ),
    );
    let (status, body) = canvas.call("POST", "/undo", Some(&json!({"author": "smoke", "at": b1})));
    let u1 = body["id"].clone();
    c.check(
        status == 200
            && body["branch"] == true
            && body["parent"] == b1
            && body["warnings"] == json!([format!("undid revision {b1_rev}")]),
        format!(
            "an undo at the branch's tip is the branch's child ({status} {})",
            body["warnings"]
        ),
    );
    let tip = |id: &Value| {
        id.as_str()
            .map(str::to_string)
            .unwrap_or_else(|| id.to_string())
    };
    let (status, body) = canvas.call("GET", &format!("/world?tip={}", tip(&u1)), None);
    c.check(
        status == 200 && !names(&body).iter().any(|n| n == "smoke-branch-post"),
        "the branch after its undo has no post",
    );
    let (status, body) = canvas.call("POST", "/redo", Some(&json!({"author": "smoke", "at": u1})));
    let r1 = body["id"].clone();
    c.check(
        status == 200
            && body["branch"] == true
            && body["parent"] == u1
            && body["warnings"] == json!([format!("redid revision {b1_rev}")]),
        format!(
            "a redo at the undo's tip brings the post back on the branch ({status} {})",
            body["warnings"]
        ),
    );
    let (status, body) = canvas.call("GET", &format!("/world?tip={}", tip(&r1)), None);
    c.check(
        status == 200 && names(&body).iter().any(|n| n == "smoke-branch-post"),
        "the branch after its redo has the post again",
    );
    let (status, body) = canvas.call("POST", "/redo", Some(&json!({"author": "smoke"})));
    c.check(
        status == 200
            && body["branch"] == false
            && body["warnings"] != json!([format!("redid revision {b1_rev}")])
            && !canvas.entity("smoke-moon")["id"].is_null(),
        format!(
            "main's own redo is untouched by the branch's: it brings the moon back ({status} {})",
            body["warnings"]
        ),
    );
    // The person's undo and redo: a nudge of the sphere commits as theirs;
    // Cmd+Z takes back their own newest (not the agent's delete above),
    // Cmd+Shift+Z brings it back. Anyone's: /undo and /redo.
    let sphere = || position(&canvas.entity("sphere"));
    canvas.select(11);
    wait(0.3);
    let p0 = sphere();
    let lines = canvas.log_lines();
    canvas.input(press("ArrowRight"));
    wait(1.5);
    let p1 = sphere();
    c.check(
        canvas.log_lines() == lines + 1 && p1 != p0,
        format!("a nudge of the sphere commits ({p0:?} -> {p1:?})"),
    );
    let mut undo = vec![json!({"key": "SuperLeft", "pressed": true})];
    undo.extend(press("KeyZ"));
    undo.push(json!({"key": "SuperLeft", "pressed": false}));
    canvas.input(undo);
    wait(1.2);
    let p2 = sphere();
    c.check(
        canvas.log_lines() == lines + 2 && p2 == p0,
        format!("Cmd+Z undoes the person's own nudge, not the agent's delete ({p2:?})"),
    );
    c.check(
        canvas.entity_by_id(o.select).is_null(),
        "the agent's delete above stands",
    );
    let mut redo = vec![
        json!({"key": "SuperLeft", "pressed": true}),
        json!({"key": "ShiftLeft", "pressed": true}),
    ];
    redo.extend(press("KeyZ"));
    redo.push(json!({"key": "ShiftLeft", "pressed": false}));
    redo.push(json!({"key": "SuperLeft", "pressed": false}));
    canvas.input(redo);
    wait(1.2);
    let p3 = sphere();
    c.check(
        canvas.log_lines() == lines + 3 && p3 == p1,
        format!("Cmd+Shift+Z redoes it ({p3:?})"),
    );
    let (status, _) = canvas.call("POST", "/ops", Some(&json!({"author": "smoke", "ops": [
        {"SpawnEntity": {"entity": {"name": "smoke-post", "shape": {"Cylinder": {"radius": 0.1, "height": 1.5}},
                                    "transform": {"position": [-2.0, 0.75, 2.0]}}}}]})));
    c.check(status == 200, format!("an agent spawns a post ({status})"));
    let (status, body) = canvas.call("POST", "/undo", Some(&json!({"author": "smoke"})));
    c.check(
        status == 200 && canvas.entity("smoke-post")["id"].is_null(),
        format!(
            "POST /undo takes the newest change back ({status} {})",
            body["warnings"]
        ),
    );
    let (status, body) = canvas.call("POST", "/redo", Some(&json!({"author": "smoke"})));
    c.check(
        status == 200 && !canvas.entity("smoke-post")["id"].is_null(),
        format!("POST /redo brings it back ({status} {})", body["warnings"]),
    );
    let (status, body) = canvas.call(
        "POST",
        "/undo",
        Some(&json!({"author": "nobody", "own": true})),
    );
    c.check(
        status == 409 && errors_name(&body, "nothing to undo"),
        format!("an own undo with nothing of one's own is refused ({status})"),
    );
}

impl Canvas {
    /// The entity with an id, from the world as it stands.
    fn entity_by_id(&self, id: i64) -> Value {
        let (_, world) = self.call("GET", "/world", None);
        world["entities"]
            .as_array()
            .and_then(|all| all.iter().find(|e| e["id"] == id).cloned())
            .unwrap_or(Value::Null)
    }

    /// How far the torus travels vertically while sampled, in metres, as
    /// the scene draws it (`scene.rendered`: its global transform after
    /// modulation), and what the last reply said, for a failure.
    fn torus_bob(&self) -> (f64, Value) {
        self.select(15);
        wait(0.2);
        let mut ys = Vec::new();
        let mut saw = Value::Null;
        for _ in 0..8 {
            let s = self.selection();
            saw = json!({"selected": s["selected"]["id"], "origin": s["handles"]["origin"],
                         "scene": s["scene"], "clock": s["clock"]});
            if let Some(y) = s["scene"]["rendered"][1].as_f64() {
                ys.push(y);
            }
            wait(0.4);
        }
        let travel = match (
            ys.iter().copied().reduce(f64::max),
            ys.iter().copied().reduce(f64::min),
        ) {
            (Some(max), Some(min)) => max - min,
            _ => 0.0,
        };
        (travel, saw)
    }
}

/// The manifest's guard, a modulation on the torus, and in a git world a
/// checkout and a replay.
fn guard_and_modulation(canvas: &Canvas, o: &Options, c: &mut Check, shots: &mut Vec<String>) {
    // A direct write to manifest.json is put back within the guard's half
    // second, and noted where an agent can read why its change vanished.
    let manifest = o.world.join("manifest.json");
    let before = std::fs::read(&manifest).unwrap_or_default();
    if let Ok(mut file) = std::fs::OpenOptions::new().append(true).open(&manifest) {
        let _ = file.write_all(b"\n");
    }
    wait(1.5);
    let after = std::fs::read(&manifest).unwrap_or_default();
    let (status, body) = canvas.call("GET", "/refusals", None);
    let notes = body["refusals"].as_array().cloned().unwrap_or_default();
    c.check(
        after == before,
        "a direct write to manifest.json is put back",
    );
    c.check(
        status == 200
            && notes.iter().any(|n| {
                n["what"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("direct write")
            }),
        format!("GET /refusals names it ({} note(s))", notes.len()),
    );
    // A modulation committed to the open scene: an offset_y driven by a
    // free-running oscillator bobs the torus. Last before the git phase, so
    // that the checkout of HEAD~1 there is exactly this commit's removal.
    let (still, _) = canvas.torus_bob();
    let (status, body) = canvas.call("POST", "/ops", Some(&json!({"author": "smoke", "ops": [
        {"ModifyEntity": {"id": "torus", "patch": {"modulations": [
            {"target": "offset_y", "signal": {"oscillator": {"frequency": 0.7}}, "range": [0.0, 1.5]}]}}}]})));
    c.check(
        status == 200,
        format!(
            "a modulation batch on the torus commits ({status} {})",
            body["errors"]
        ),
    );
    let (bobbing, saw) = canvas.torus_bob();
    c.check(
        still < 0.01 && bobbing > 0.5,
        format!(
            "the committed modulation bobs the torus as drawn ({still:.3} m of travel still, {bobbing:.3} m with it; on_screen {}, saw {saw})",
            body["on_screen"]
        ),
    );
    if o.world.join(".git").is_dir() {
        git_phase(canvas, o, c, shots);
    } else {
        println!("skip the checkout and replay checks: not a git world (init with --git)");
    }
}

/// In a git world: a checkout under the open window is followed, and a
/// replay walks the commits and comes back to the head, drawing each
/// commit's own asset bytes.
fn git_phase(canvas: &Canvas, o: &Options, c: &mut Check, shots: &mut Vec<String>) {
    let world = &o.world;
    let git = |args: &[&str], c: &mut Check| {
        let out = Command::new("git")
            .arg("-C")
            .arg(world)
            .args(args)
            .stdin(Stdio::null())
            .output();
        let (ok, stdout, stderr) = match out {
            Ok(out) => (
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).trim().to_string(),
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ),
            Err(e) => (false, String::new(), e.to_string()),
        };
        c.check(
            ok,
            format!(
                "git {} works under the open window ({stderr})",
                args.join(" ")
            ),
        );
        stdout
    };
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"], c);
    let dirty = git(&["status", "--porcelain"], c);
    c.check(
        dirty.is_empty(),
        format!(
            "the committer leaves the git tree clean, a restored manifest included ({dirty:?})"
        ),
    );
    let revision = || {
        canvas.call("GET", "/log", None).1["revision"]
            .as_i64()
            .unwrap_or(-1)
    };
    let rev_before = revision();
    let (bob_head, _) = canvas.torus_bob();
    git(&["checkout", "--quiet", "HEAD~1"], c);
    wait(1.5);
    let rev_back = revision();
    c.check(
        rev_back == rev_before - 1,
        format!(
            "a checkout under the open window is followed (revision {rev_before} -> {rev_back})"
        ),
    );
    let (bob_back, saw) = canvas.torus_bob();
    c.check(
        bob_head > 0.5 && bob_back < 0.01,
        format!(
            "the scene shows the checked-out state: the torus's modulation, committed last, is gone ({bob_head:.3} m -> {bob_back:.3} m; saw {saw})"
        ),
    );
    git(&["checkout", "--quiet", &branch], c);
    wait(1.5);
    c.check(
        revision() == rev_before && canvas.torus_bob().0 > 0.5,
        "and back on the branch, the torus bobs again",
    );
    // A replay shows each commit's own asset bytes. A swatch texture goes
    // in through a batch (stored by hash); its stored copy is then
    // overwritten with other bytes by hand and committed, so the working
    // tree lies about history, and the replayed frame of the spawn still
    // draws the committed red.
    let (red, blue) = (
        png_bytes(8, 8, [220, 30, 30]),
        png_bytes(8, 8, [30, 30, 220]),
    );
    let _ = std::fs::write(world.join("assets/swatch.png"), &red);
    let (status, body) = canvas.call("POST", "/ops", Some(&json!({"author": "smoke", "message": "a swatch to replay", "ops": [
        {"SpawnEntity": {"entity": {"name": "smoke-swatch", "shape": {"Cuboid": {"x": 3.0, "y": 3.0, "z": 0.2}},
                                    "transform": {"position": [0.0, 2.5, 8.0]},
                                    "material": {"color": [1.0, 1.0, 1.0, 1.0], "base_color_texture": "swatch.png"}}}}]})));
    let swatch_id = body["spawned"]["smoke-swatch"].as_i64();
    let stored = body["stored"]["swatch.png"].as_str().map(str::to_string);
    c.check(
        status == 200 && swatch_id.is_some() && stored.is_some(),
        format!("a textured swatch spawns, its texture stored by hash ({status} {stored:?})"),
    );
    let spawn_commit = git(&["log", "-n1", "--format=%H", "--", "manifest.json"], c);
    if let Some(stored) = &stored {
        let _ = std::fs::write(world.join("assets").join(stored), &blue);
        git(&["add", "-A"], c);
        git(
            &[
                "commit",
                "--quiet",
                "-m",
                "the swatch's stored copy overwritten by hand",
            ],
            c,
        );
        wait(1.5);
    }
    let rev_head = revision();
    let (status, body) = canvas.call("POST", "/replay?seconds=0.3", Some(&json!({})));
    c.check(
        status == 200 && body["result"] == "replaying",
        format!("a replay starts ({status} {body})"),
    );
    let mut done = false;
    for _ in 0..400 {
        wait(0.25);
        if std::fs::read_to_string(world.join(".live/smoke-commit.log"))
            .is_ok_and(|t| t.contains("replay done"))
        {
            done = true;
            break;
        }
    }
    c.check(
        done,
        "the replay walks every commit and returns to the head",
    );
    c.check(
        revision() == rev_head,
        "the world is at the head after the replay",
    );
    let cached = world
        .join(".live/replay")
        .join(&spawn_commit)
        .join("assets")
        .join(stored.as_deref().unwrap_or_default());
    c.check(
        stored.is_some() && std::fs::read(&cached).is_ok_and(|bytes| bytes == red),
        "the spawn's commit was materialized with its own bytes (red), the working tree's being blue",
    );
    wait(1.5);
    let at = swatch_id.and_then(|id| point(&canvas.select(id)["handles"]["origin"]));
    // Sample the swatch where the window shows it, a little in from its
    // edges (the swatch is some 230 px wide).
    let (x0, x1, _) = canvas.free_band();
    let sample = at.map(|at| (at.0.max(x0 + 40.0).min(x1 - 40.0), at.1));
    let shot = canvas.screenshot(shots);
    let mean = match (sample, shot) {
        (Some(s), Some(path)) => png_mean(Path::new(&path), s.0 as i64, s.1 as i64, 10),
        _ => None,
    };
    c.check(
        mean.is_some_and(|m| m[0] > m[2] + 40.0 && m[0] > 100.0),
        format!(
            "the replayed frame draws the swatch with the committed red, not today's blue (mean RGB {mean:?} sampled at {sample:?}; origin {at:?}, free band {x0:.0}..{x1:.0})"
        ),
    );
}

/// A solid w×h RGB PNG.
fn png_bytes(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        if let Ok(mut writer) = encoder.write_header() {
            let pixels: Vec<u8> = (0..w * h).flat_map(|_| rgb).collect();
            let _ = writer.write_image_data(&pixels);
        }
    }
    out
}

/// The mean RGB of a patch of a screenshot, `half` pixels around a point.
fn png_mean(path: &Path, cx: i64, cy: i64, half: i64) -> Option<[f64; 3]> {
    let decoder = png::Decoder::new(std::io::BufReader::new(File::open(path).ok()?));
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    if info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let bpp = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => return None,
    };
    let (w, h) = (i64::from(info.width), i64::from(info.height));
    let (mut sums, mut n) = ([0.0f64; 3], 0.0);
    for y in (cy - half).max(0)..(cy + half).min(h) {
        for x in (cx - half).max(0)..(cx + half).min(w) {
            let at = ((y * w + x) * bpp) as usize;
            for (sum, value) in sums.iter_mut().zip(&buffer[at..at + 3]) {
                *sum += f64::from(*value);
            }
            n += 1.0;
        }
    }
    (n > 0.0).then(|| sums.map(|s| s / n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_and_chunks_as_the_wire_has_them() {
        assert_eq!(base64(b"the sample nonce"), "dGhlIHNhbXBsZSBub25jZQ==");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(
            dechunk(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n"),
            b"Wikipedia"
        );
    }

    #[test]
    fn a_swatch_written_reads_back_its_colour() {
        let path = std::env::temp_dir().join(format!("xtask-swatch-{}.png", std::process::id()));
        std::fs::write(&path, png_bytes(8, 8, [220, 30, 30])).unwrap();
        assert_eq!(png_mean(&path, 4, 4, 2), Some([220.0, 30.0, 30.0]));
        let _ = std::fs::remove_file(&path);
    }
}
