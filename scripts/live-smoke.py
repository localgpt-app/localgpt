#!/usr/bin/env python3
"""Smoke-test the live canvas with no one at the keyboard.

Launches `localgpt-gen --live <world>` with the inspector open
(`LOCALGPT_INSPECTOR=full`), drives it over the live API in
`<world>/.live/endpoint.json`, the inspector's WebSocket (port 9877, or
`LOCALGPT_INSPECTOR_PORT` for a second canvas on the same desk) and a
synthesized hand (`LOCALGPT_SYNTHETIC_INPUT=1`, `POST /input`), and
checks what an editor is judged by: selection, proposals rejected and
accepted by key and by the bar's buttons, the busy replies while one is
pending and the log untouched, the stale-revision refusal, a commit with
the right expected revision, a handle drag, snapping, key turns, a
shift-click multi-selection dragged together, grouping, deleting, and
the receipt's warning when a delete leaves a behavior without its
target. Screenshots land in `<world>/.live/` for a person to look at.

    mkdir demo.world && cp ../openworldformat/examples/hello-world/manifest.json demo.world/
    cargo run -p localgpt-world-agent --example live -- init demo.world --git
    cargo build -p localgpt-gen
    scripts/live-smoke.py demo.world            # --offscreen on a locked screen

The world must be a live package with the spec's hello-world entities:
a cuboid (10), a capsule (14) and a lantern (100). `init` on an empty
folder makes an empty world, so copy the manifest in first. With `--git`
the checkout and replay checks run too; without it they are skipped.
`--herdr` starts a headless herdr of the smoke's own (an isolated config
directory under the world's `.live/`, a socket of its own) and checks the
bar's agent menu and presence against it; it needs `herdr` on the path.
Run it against a `--features feathers` build as well (`--binary` names
the executable, so both builds can be kept): the bar, the outliner, the
review bar and the gallery checks drive whichever stack draws them
through the same widget names.
"""

import argparse
import base64
import json
import math
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WS_PORT = int(os.environ.get("LOCALGPT_INSPECTOR_PORT", "9877"))
GRID = 0.25


def call(ep, method, path, body=None, timeout=40):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(ep["url"] + path, data=data, method=method)
    req.add_header("Authorization", "Bearer " + ep["token"])
    if data is not None:
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read().decode() or "null")
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read().decode() or "null")


def ws_send(port, message):
    """One masked text frame to the inspector's WebSocket, by hand — no
    dependency outside the standard library."""
    s = socket.create_connection(("127.0.0.1", port), timeout=5)
    key = base64.b64encode(os.urandom(16)).decode()
    s.sendall(
        (
            f"GET /ws HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\n"
            f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\n\r\n"
        ).encode()
    )
    resp = s.recv(4096)
    if b" 101 " not in resp.split(b"\r\n")[0] + b" ":
        raise RuntimeError("no websocket upgrade: %r" % resp[:120])
    payload = message.encode()
    mask = os.urandom(4)
    n = len(payload)
    header = bytes([0x81]) + (bytes([0x80 | n]) if n < 126 else bytes([0x80 | 126]) + n.to_bytes(2, "big"))
    s.sendall(header + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))
    time.sleep(0.8)
    s.close()


def press(key):
    return [{"key": key, "pressed": True}, {"key": key, "pressed": False}]


def click(at):
    return [{"cursor": list(at)}, {"button": "left", "pressed": True}, {"button": "left", "pressed": False}]


def center(rect):
    x, y, w, h = rect
    return (x + w / 2, y + h / 2)


class Herdr:
    """A headless herdr of the smoke's own, so the canvas's agent presence
    can be checked without a terminal: `herdr server` with an isolated
    config directory and a socket of its own (short: a Unix socket path
    has a hundred-odd bytes), and `pane report-agent` standing in for the
    detected state of an agent. The canvas inherits `HERDR_SOCKET_PATH`."""

    def __init__(self, world):
        self.sock = os.path.join(os.environ.get("TMPDIR") or "/tmp", f"owf-smoke-{os.getpid()}.sock")
        self.env = dict(os.environ, XDG_CONFIG_HOME=os.path.join(world, ".live", "herdr"), HERDR_SOCKET_PATH=self.sock)
        os.makedirs(self.env["XDG_CONFIG_HOME"], exist_ok=True)
        self.log = open(os.path.join(world, ".live", "smoke-herdr.log"), "w")
        self.proc = subprocess.Popen(["herdr", "server"], env=self.env, stdout=self.log, stderr=subprocess.STDOUT)
        for _ in range(40):
            if os.path.exists(self.sock):
                break
            time.sleep(0.25)
        os.environ["HERDR_SOCKET_PATH"] = self.sock

    def cli(self, *args):
        return subprocess.run(["herdr", *args], env=self.env, capture_output=True, text=True, timeout=20)

    def report(self, pane, state):
        """What a detection manifest would conclude, said outright."""
        return self.cli("pane", "report-agent", pane, "--source", "smoke", "--agent", "claude", "--state", state)

    def rpc(self, method, params):
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(10)
        s.connect(self.sock)
        s.sendall((json.dumps({"id": "smoke", "method": method, "params": params}) + "\n").encode())
        buf = b""
        while not buf.endswith(b"\n"):
            chunk = s.recv(65536)
            if not chunk:
                break
            buf += chunk
        s.close()
        return json.loads(buf.decode() or "null") or {}

    def stop(self):
        self.cli("server", "stop")
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
        os.environ.pop("HERDR_SOCKET_PATH", None)
        self.log.close()


class Canvas:
    """The app as a subprocess: launched, driven, stopped."""

    offscreen = False

    def __init__(self, binary, world, review, log, synthetic=False):
        self.world = world
        ep_file = os.path.join(world, ".live", "endpoint.json")
        if os.path.exists(ep_file):
            os.remove(ep_file)
        env = dict(os.environ, LOCALGPT_INSPECTOR="full")
        if synthetic:
            env["LOCALGPT_SYNTHETIC_INPUT"] = "1"
        if Canvas.offscreen:
            # A locked or sleeping screen leaves a window's surface black:
            # the canvas renders into an image instead, and screenshots
            # read that.
            env["LOCALGPT_OFFSCREEN_SHOTS"] = "1"
        args = [binary, "--live", world] + (["--review"] if review else [])
        self.proc = subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT, cwd=ROOT)
        for _ in range(240):
            if os.path.exists(ep_file):
                time.sleep(3)  # the first frames: the scene built, egui's first pass
                with open(ep_file) as f:
                    self.ep = json.load(f)
                return
            if self.proc.poll() is not None:
                raise RuntimeError("the app exited early with %s" % self.proc.returncode)
            time.sleep(0.5)
        self.proc.kill()
        raise RuntimeError("no endpoint after 120 s")

    def stop(self):
        self.proc.terminate()
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()

    def log_lines(self):
        with open(os.path.join(self.world, "ops.jsonl")) as f:
            return sum(1 for line in f if line.strip())

    # -- the hand -----------------------------------------------------

    def input(self, events):
        """Queue a hand's events, one per frame, and return once the canvas
        has emitted them all — plus a frame for the last to land. The fixed
        waits after a call are settle time for what the events set off, not
        for the events themselves, so a slow stretch of frames cannot make a
        check read before its input arrived."""
        reply = call(self.ep, "POST", "/input", {"events": events})
        deadline = time.time() + 60.0
        while time.time() < deadline:
            status, body = call(self.ep, "GET", "/input")
            if status != 200 or (body or {}).get("queued", 0) == 0:
                break
            time.sleep(0.05)
        time.sleep(0.05)
        return reply

    def select(self, entity_id):
        """Select over the inspector socket, and — as a person would before
        taking hold of the handles — move the camera until none of them is
        under a panel. The reply is the selection as it then stands."""
        ws_send(WS_PORT, json.dumps({"type": "select_entity", "entity_id": entity_id}))
        time.sleep(0.3)
        self.clear_of_chrome()
        return call(self.ep, "GET", "/selection")[1]

    def entity(self, name):
        _, world_now = call(self.ep, "GET", "/world")
        return next((e for e in world_now.get("entities", []) if e.get("name") == name), {})

    def widgets(self):
        """The panels' widgets by name, in window points (GET /widgets)."""
        return call(self.ep, "GET", "/widgets")[1] or {}

    def click_widget(self, name):
        """Click a panel widget by name; False when it isn't on screen."""
        rect = self.widgets().get(name)
        if not rect:
            return False
        self.input(click(center(rect)))
        return True

    def panels(self):
        """The chrome's rectangles by name (`panel.*` in GET /widgets)."""
        return {n: r for n, r in self.widgets().items() if n.startswith("panel.")}

    def free_band(self):
        """The viewport the chrome leaves free, from the panels by name: x
        from the outliner's right edge to the detail panel's left, y down to
        the bottom bar — and the history rail's rectangle, which floats
        inside it."""
        p = self.panels()
        x0 = p["panel.outliner"][0] + p["panel.outliner"][2] + 8 if "panel.outliner" in p else 8.0
        x1 = p["panel.detail"][0] - 8 if "panel.detail" in p else 1272.0
        y1 = p["panel.world_info"][1] - 8 if "panel.world_info" in p else 712.0
        return x0, x1, y1, p.get("panel.history_rail")

    @staticmethod
    def inside(point, rect):
        return bool(rect) and rect[0] <= point[0] <= rect[0] + rect[2] and rect[1] <= point[1] <= rect[1] + rect[3]

    def park(self):
        """Rest the pointer in the free viewport, off every panel: over a
        panel the camera keys are the panel's, and a handle under one is
        not hot."""
        x0, x1, y1, rail = self.free_band()
        at = (x0 + 24.0, 100.0)
        if self.inside(at, rail):
            at = (x0 + 24.0, y1 - 24.0)
        self.input([{"cursor": [at[0], at[1]]}])
        return at

    def clear_of_chrome(self, tries=8):
        """Move the camera until no point of the selection's handles — the
        origin, the arrow tips, the ring points, the scale cube — is under
        a panel, as a person moves to see what they grab: down (Q) when the
        panel is below the handles, so the scene rises; sideways otherwise.
        True when they are clear (at once, mostly: a no-op then)."""
        names = ("origin", "x", "y", "z", "ring_x", "ring_y", "ring_z", "scale")
        for _ in range(tries):
            h = call(self.ep, "GET", "/selection")[1].get("handles") or {}
            if not h.get("origin"):
                return False
            rects = list(self.panels().values())
            points = [h[k] for k in names if h.get(k)]
            covered = next((r for pt in points for r in rects if self.inside(pt, r)), None)
            if covered is None:
                return True
            ox, oy = h["origin"]
            # Only while the entity itself is in the free viewport: a move
            # that would push it out is worse than a handle under a panel.
            x0, x1, y1, _ = self.free_band()
            if not (x0 < ox < x1 and 0 < oy < y1):
                return False
            key = "KeyQ" if covered[1] > oy else ("Space" if covered[1] + covered[3] < oy else ("KeyD" if covered[0] > ox else "KeyA"))
            self.park()
            self.input([{"key": key, "pressed": True}])
            time.sleep(0.3)
            self.input([{"key": key, "pressed": False}])
            time.sleep(0.3)
        return False

    def hot_after_a_moment(self):
        """The handle under the pointer once the canvas has drawn a frame
        with the pointer there — polled, since a frame under load (a build
        beside the run) can take longer than a fixed wait."""
        hot = None
        for _ in range(15):
            time.sleep(0.1)
            hot = (call(self.ep, "GET", "/selection")[1].get("handles") or {}).get("hot")
            if hot:
                break
        return hot

    def drag(self, a, b, steps=8):
        """Press at `a`, pull to `b` in steps, release."""
        events = [{"cursor": list(a)}, {"button": "left", "pressed": True}]
        events += [{"cursor": [a[0] + (b[0] - a[0]) * i / steps, a[1] + (b[1] - a[1]) * i / steps]} for i in range(1, steps + 1)]
        events += [{"button": "left", "pressed": False}]
        return self.input(events)

    def drag_x_handle(self, px, step=10, axis="x"):
        """Hover an arrow of the selection, press, pull along it, release.
        Returns the handle under the pointer at the press."""
        handles = call(self.ep, "GET", "/selection")[1].get("handles") or {}
        (ox, oy), (xx, xy) = handles["origin"], handles[axis]
        dx, dy = xx - ox, xy - oy
        n = max((dx * dx + dy * dy) ** 0.5, 1.0)
        ux, uy = dx / n, dy / n
        start = (ox + dx * 0.8, oy + dy * 0.8)
        self.input([{"cursor": list(start)}])
        hot = self.hot_after_a_moment()
        events = [{"button": "left", "pressed": True}]
        events += [{"cursor": [start[0] + ux * step * i, start[1] + uy * step * i]} for i in range(1, px // step + 1)]
        events += [{"button": "left", "pressed": False}]
        self.input(events)
        return hot

    def drag_ring(self, name, px, step=8):
        """Hover a ring of the selection where the API puts it, press, pull
        along the ring (across its spoke), release. Returns the handle under
        the pointer at the press."""
        handles = call(self.ep, "GET", "/selection")[1].get("handles") or {}
        (ox, oy), (rx, ry) = handles["origin"], handles[name]
        dx, dy = rx - ox, ry - oy
        n = max((dx * dx + dy * dy) ** 0.5, 1.0)
        tx, ty = -dy / n, dx / n
        self.input([{"cursor": [rx, ry]}])
        hot = self.hot_after_a_moment()
        events = [{"button": "left", "pressed": True}]
        events += [{"cursor": [rx + tx * step * i, ry + ty * step * i]} for i in range(1, px // step + 1)]
        events += [{"button": "left", "pressed": False}]
        self.input(events)
        return hot

    def drag_box(self, start, end, steps=10):
        """Press at `start`, pull to `end`, release — and report the box the
        canvas showed mid-drag (GET /selection's `box`), for the record."""
        self.input([{"cursor": list(start)}, {"button": "left", "pressed": True}])
        time.sleep(0.2)
        mid = None
        for i in range(1, steps + 1):
            self.input([{"cursor": [start[0] + (end[0] - start[0]) * i / steps, start[1] + (end[1] - start[1]) * i / steps]}])
            if i == steps // 2:
                time.sleep(0.2)
                mid = call(self.ep, "GET", "/selection")[1].get("box")
        time.sleep(0.2)
        self.input([{"button": "left", "pressed": False}])
        return mid


class Check:
    def __init__(self):
        self.failures = []

    def __call__(self, ok, what):
        print(("ok   " if ok else "FAIL ") + what, flush=True)
        if not ok:
            self.failures.append(what)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("world", help="a live .world folder (see `live -- init`)")
    parser.add_argument("--build", action="store_true", help="cargo build -p localgpt-gen first")
    parser.add_argument("--binary", default=None, help="the localgpt-gen executable to run (default: target/debug/localgpt-gen)")
    parser.add_argument("--select", type=int, default=100, help="entity id to select over the inspector socket")
    parser.add_argument("--modify", default="cuboid", help="entity the proposed batch moves and recolors")
    parser.add_argument("--other", default="capsule", help="a solid entity to shift-click into the selection")
    parser.add_argument("--offscreen", action="store_true", help="render into an image for the screenshots (a locked or sleeping screen leaves the window black)")
    parser.add_argument("--herdr", action="store_true", help="start a headless herdr of the smoke's own and check the bar's agent menu and presence against it (needs `herdr` on the path)")
    args = parser.parse_args()
    Canvas.offscreen = args.offscreen
    world = os.path.abspath(args.world)
    binary = args.binary or os.path.join(ROOT, "target", "debug", "localgpt-gen")
    if args.build or not os.path.exists(binary):
        subprocess.run(["cargo", "build", "-p", "localgpt-gen"], cwd=ROOT, check=True)
    check = Check()
    batch = {
        "author": "smoke",
        "message": f"lift and redden {args.modify}",
        "ops": [{"ModifyEntity": {"id": args.modify, "patch": {
            "transform": {"position": [0.0, 3.0, 0.0]},
            "material": {"color": [1.0, 0.1, 0.1, 1.0]}}}}],
    }
    shots = []

    def red(entity):
        color = entity.get("material", {}).get("color") or []
        return len(color) == 4 and all(abs(a - b) < 1e-3 for a, b in zip(color, [1.0, 0.1, 0.1, 1.0]))

    def statuses(ep):
        return [p.get("status") for p in call(ep, "GET", "/proposals")[1].get("proposals", [])]

    def buttons_soon(ep, seconds=2.0):
        """The review bar's buttons, once the bar has drawn them: a widget
        built on Bevy UI reaches the seam a frame after the proposal lands."""
        deadline = time.time() + seconds
        while True:
            buttons = call(ep, "GET", "/proposals")[1].get("buttons") or {}
            if buttons or time.time() >= deadline:
                return buttons
            time.sleep(0.05)

    # Review on, with a synthesized hand: proposals rejected and accepted
    # by key and by the bar's buttons, then one left pending; nothing is
    # written while one waits.
    with open(os.path.join(world, ".live", "smoke-review.log"), "w") as log:
        canvas = Canvas(binary, world, True, log, synthetic=True)
        try:
            ep = canvas.ep
            status, body = call(ep, "GET", "/")
            check(status == 200 and "GET /proposals" in body.get("endpoints", {}), "the API lists /proposals")
            selected = (canvas.select(args.select).get("selected") or {}).get("id")
            check(selected == args.select, f"the inspector socket selects entity {args.select} (got {selected})")
            status, body = call(ep, "GET", "/screenshot")
            shots.append(body.get("path"))

            lines = canvas.log_lines()
            status, body = call(ep, "POST", "/ops", batch)
            check(status == 202 and body.get("result") == "proposed", f"a batch is proposed, not committed ({status} {body.get('result')})")
            canvas.input(press("Escape"))
            time.sleep(0.6)
            check(statuses(ep) == ["rejected"], f"Esc rejects the proposal ({statuses(ep)})")
            check(canvas.log_lines() == lines, "a rejected proposal wrote nothing")
            check(not red(canvas.entity(args.modify)), "the world keeps its color after a rejection")

            status, body = call(ep, "POST", "/ops", batch)
            buttons = buttons_soon(ep)
            check(bool(buttons.get("reject")), f"GET /proposals reports the bar's buttons ({buttons})")
            if buttons.get("reject"):
                canvas.input(click(center(buttons["reject"])))
                time.sleep(0.6)
            check(statuses(ep) == ["rejected", "rejected"], f"the Reject button, clicked, rejects ({statuses(ep)})")
            check(canvas.log_lines() == lines, "still nothing written")

            status, body = call(ep, "POST", "/ops", batch)
            canvas.input(press("Enter"))
            time.sleep(0.6)
            check(statuses(ep) == ["rejected", "rejected", "accepted"], f"Enter accepts the proposal ({statuses(ep)})")
            check(canvas.log_lines() == lines + 1, "an accepted proposal is one entry")
            check(red(canvas.entity(args.modify)), "the accepted change is in the world")

            status, body = call(ep, "POST", "/ops", batch)
            buttons = buttons_soon(ep)
            if buttons.get("accept"):
                canvas.input(click(center(buttons["accept"])))
                time.sleep(0.6)
            check(statuses(ep) == ["rejected", "rejected", "accepted", "accepted"], f"the Accept button, clicked, accepts ({statuses(ep)})")
            check(canvas.log_lines() == lines + 2, "the second accepted proposal is one more entry")

            before = canvas.log_lines()
            status, body = call(ep, "POST", "/ops", batch)
            check(status == 202, f"a fifth batch is proposed ({status})")
            status, _ = call(ep, "POST", "/ops", batch)
            check(status == 409, f"a second batch waits for the decision ({status})")
            status, _ = call(ep, "POST", "/undo", {})
            check(status == 409, f"an undo waits for the decision ({status})")
            status, _ = call(ep, "POST", "/redo", {})
            check(status == 409, f"a redo waits for the decision too ({status})")
            time.sleep(1.0)
            status, body = call(ep, "GET", "/screenshot")
            shots.append(body.get("path"))
            check(canvas.log_lines() == before, "the log is untouched while the proposal is pending")
        finally:
            canvas.stop()
    with open(os.path.join(world, ".live", "smoke-review.log")) as f:
        check("panicked" not in f.read(), "no panic with review on")

    # Review off: expected revisions, the gestures, the selection, the
    # receipt's warning.
    herdr = None
    if args.herdr:
        if shutil.which("herdr"):
            herdr = Herdr(world)
        else:
            print("skip the herdr checks: no `herdr` on the path")
    with open(os.path.join(world, ".live", "smoke-commit.log"), "w") as log:
        canvas = Canvas(binary, world, False, log, synthetic=True)
        try:
            ep = canvas.ep
            status, body = call(ep, "GET", "/log")
            revision = body.get("revision", 0)
            status, body = call(ep, "POST", "/ops", dict(batch, expected_revision=revision + 7))
            check(status == 422 and any("revision" in e for e in body.get("errors", [])), "a stale expected_revision is refused, naming the revisions")
            status, body = call(ep, "POST", "/ops", dict(batch, expected_revision=revision))
            check(status == 200 and body.get("result") == "committed", f"the right expected_revision commits ({status})")
            status, body = call(ep, "GET", "/log")
            check(body.get("revision") == revision + 1, "the log's revision moved by one")

            # The batch put the entity in front of the camera: select it,
            # drag its X handle 100 px, then turn it twice with the comma key.
            modified = canvas.entity(args.modify)
            modified_id = modified.get("id", 10)
            other = canvas.entity(args.other)
            other_id = other.get("id", 14)
            handles = canvas.select(modified_id).get("handles") or {}
            check(bool(handles.get("x")), f"GET /selection reports the handles ({handles})")
            before_x = modified.get("transform", {}).get("position", [0, 0, 0])[0]
            lines = canvas.log_lines()
            hot = canvas.drag_x_handle(100)
            check(hot == "x", f"the pointer over the X arrow makes it hot ({hot})")
            time.sleep(0.15)
            status, body = call(ep, "GET", "/screenshot")  # mid-drag
            shots.append(body.get("path"))
            time.sleep(1.5)
            after_x = canvas.entity(args.modify).get("transform", {}).get("position", [0, 0, 0])[0]
            check(canvas.log_lines() == lines + 1, "the drag committed one entry")
            check(after_x > before_x + 0.05, f"the drag moved {args.modify} along +x ({before_x} -> {after_x})")

            lines = canvas.log_lines()
            canvas.input(press("Comma") + press("Comma"))
            time.sleep(1.8)
            yaw = canvas.entity(args.modify).get("transform", {}).get("rotation_degrees", [0, 0, 0])[1]
            check(canvas.log_lines() == lines + 1, "the key turns committed as one entry")
            check(abs(yaw - 30.0) < 0.01, f"two turns are 30 degrees ({yaw})")
            status, body = call(ep, "GET", "/screenshot")
            shots.append(body.get("path"))

            # A ring: the most face-on of the three (its spoke is longest on
            # screen), hovered where the API puts it, then pulled along the
            # ring — a turn about that axis, committed as one entry.
            h = call(ep, "GET", "/selection")[1].get("handles") or {}

            def spoke(name):
                if not h.get(name) or not h.get("origin"):
                    return 0.0
                (ox, oy), (px_, py_) = h["origin"], h[name]
                return ((px_ - ox) ** 2 + (py_ - oy) ** 2) ** 0.5

            ring = max(("ring_x", "ring_y", "ring_z"), key=spoke)
            before = canvas.entity(args.modify).get("transform", {}).get("rotation_degrees", [0, 0, 0])
            lines = canvas.log_lines()
            hot = canvas.drag_ring(ring, 48)
            check(hot == ring, f"the pointer over the {ring} ring makes it hot ({hot})")
            time.sleep(1.5)
            after = canvas.entity(args.modify).get("transform", {}).get("rotation_degrees", [0, 0, 0])
            check(canvas.log_lines() == lines + 1, "the ring drag committed one entry")
            check(max(abs(a - b) for a, b in zip(after, before)) > 1.0, f"the ring turned {args.modify} about its axis ({before} -> {after})")

            # L: the handles follow the entity's own axes. The cuboid is turned
            # now, so its X arrow swings on screen with local axes on, and
            # swings back with them off.
            def handles_now():
                return call(ep, "GET", "/selection")[1].get("handles") or {}

            def arrow_angle():
                h = handles_now()
                (ox, oy), (xx, xy) = h["origin"], h["x"]
                return math.degrees(math.atan2(xy - oy, xx - ox)), h.get("local")

            a0, local0 = arrow_angle()
            canvas.input(press("KeyL"))
            time.sleep(0.4)
            a1, local1 = arrow_angle()
            canvas.input(press("KeyL"))
            time.sleep(0.4)
            a2, local2 = arrow_angle()
            check((local0, local1, local2) == (False, True, False), f"L toggles local axes ({local0}, {local1}, {local2})")
            check(abs(a1 - a0) > 3.0 and abs(a2 - a0) < 0.5,
                  f"local axes swing the X arrow with the turned cuboid ({a0:.1f} -> {a1:.1f} -> {a2:.1f} deg)")

            # Alt-drag an arrow scales along the entity's own matching axis.
            scale0 = canvas.entity(args.modify).get("transform", {}).get("scale", [1, 1, 1])
            lines = canvas.log_lines()
            canvas.input([{"key": "AltLeft", "pressed": True}])
            hot = canvas.drag_x_handle(60)
            canvas.input([{"key": "AltLeft", "pressed": False}])
            time.sleep(1.5)
            scale1 = canvas.entity(args.modify).get("transform", {}).get("scale", [1, 1, 1])
            check(hot == "x" and canvas.log_lines() == lines + 1, f"an Alt-drag of the X arrow commits one entry ({hot})")
            check(scale1[0] > scale0[0] + 0.05 and abs(scale1[1] - scale0[1]) < 1e-3 and abs(scale1[2] - scale0[2]) < 1e-3,
                  f"it scaled {args.modify} along its own x only ({scale0} -> {scale1})")

            # Alt + right-drag orbits the camera around the selection: the
            # selected origin keeps its place on screen while the X arrow
            # swings. Then the same drag back, so the view is as it was.
            def orbit(dx):
                events = [{"key": "AltLeft", "pressed": True}, {"cursor": [640.0, 330.0]}, {"button": "right", "pressed": True}]
                events += [{"cursor": [640.0 + dx * i, 330.0]} for i in range(1, 9)]
                events += [{"button": "right", "pressed": False}, {"key": "AltLeft", "pressed": False}]
                canvas.input(events)
                time.sleep(0.5)

            a_before, _ = arrow_angle()
            o_before = handles_now()["origin"]
            orbit(20.0)
            a_after, _ = arrow_angle()
            o_after = handles_now()["origin"]
            drift = math.hypot(o_after[0] - o_before[0], o_after[1] - o_before[1])
            check(drift < 4.0 and abs(a_after - a_before) > 2.0,
                  f"Alt + right-drag orbits around the selection (origin drifted {drift:.1f} px, the X arrow swung {abs(a_after - a_before):.1f} deg)")
            orbit(-20.0)

            # What a hand would feel, measured: the handles keep about the
            # same size on screen from near and far, and the hover
            # tolerance stays usable — a few pixels off the arrow still
            # finds it, far off does not.
            def arrow_px():
                h = call(ep, "GET", "/selection")[1].get("handles") or {}
                (ox, oy), (xx, xy) = h["origin"], h["x"]
                return ((xx - ox) ** 2 + (xy - oy) ** 2) ** 0.5, (ox, oy), (xx, xy)

            def hot_off_arrow(offset_px):
                _, (ox, oy), (xx, xy) = arrow_px()
                along = (ox + (xx - ox) * 0.6, oy + (xy - oy) * 0.6)
                canvas.input([{"cursor": [along[0], along[1] + offset_px]}])
                time.sleep(0.2)
                return (call(ep, "GET", "/selection")[1].get("handles") or {}).get("hot")

            canvas.clear_of_chrome()
            near_len, near_origin, _ = arrow_px()
            check(hot_off_arrow(8) == "x" and hot_off_arrow(60) != "x", "hover forgives 8 px off the arrow, not 60")
            canvas.park()
            canvas.input([{"key": "KeyW", "pressed": True}])
            time.sleep(0.45)
            canvas.input([{"key": "KeyW", "pressed": False}])
            time.sleep(0.4)
            far_len, far_origin, _ = arrow_px()
            canvas.clear_of_chrome()
            moved_px = ((far_origin[0] - near_origin[0]) ** 2 + (far_origin[1] - near_origin[1]) ** 2) ** 0.5
            check(moved_px > 5 and abs(far_len - near_len) / near_len < 0.3,
                  f"the handles keep their screen size as the camera moves ({near_len:.0f} px -> {far_len:.0f} px, the origin shifted {moved_px:.0f} px)")
            check(hot_off_arrow(8) == "x" and hot_off_arrow(60) != "x", "hover tolerance holds from the new distance")

            # The same tolerance on an arrow laid along the entity's own,
            # turned axis (L) — what a hand grabbing a local-axes handle
            # feels. Off the arrow means perpendicular to it on screen: a
            # vertical offset from a turned arrow can run along it.
            def hot_off_arrow_perp(offset_px):
                _, (ox, oy), (xx, xy) = arrow_px()
                dx, dy = xx - ox, xy - oy
                n = max(math.hypot(dx, dy), 1.0)
                px_, py_ = -dy / n, dx / n
                along = (ox + dx * 0.6, oy + dy * 0.6)
                canvas.input([{"cursor": [along[0] + px_ * offset_px, along[1] + py_ * offset_px]}])
                time.sleep(0.2)
                return (call(ep, "GET", "/selection")[1].get("handles") or {}).get("hot")

            canvas.input(press("KeyL"))
            time.sleep(0.4)
            canvas.clear_of_chrome()
            local_now = (call(ep, "GET", "/selection")[1].get("handles") or {}).get("local")
            near_local, far_local = hot_off_arrow_perp(8), hot_off_arrow_perp(60)
            canvas.input(press("KeyL"))
            time.sleep(0.4)
            check(local_now is True and near_local == "x" and far_local != "x",
                  f"on local axes the turned X arrow forgives 8 px off it, not 60 (local {local_now}, {near_local}, {far_local})")
            canvas.park()
            canvas.input([{"key": "KeyS", "pressed": True}])
            time.sleep(0.45)
            canvas.input([{"key": "KeyS", "pressed": False}])
            time.sleep(0.4)

            # Snapping: Y on, a drag up the Y arrow (the X arrow now reaches
            # under the detail panel) lands on the grid, Y off.
            canvas.input(press("KeyY"))
            time.sleep(0.3)
            lines = canvas.log_lines()
            before_y = canvas.entity(args.modify).get("transform", {}).get("position", [0, 0, 0])[1]
            hot = canvas.drag_x_handle(57, axis="y")
            time.sleep(1.5)
            snapped_y = canvas.entity(args.modify).get("transform", {}).get("position", [0, 0, 0])[1]
            on_grid = abs(snapped_y / GRID - round(snapped_y / GRID)) < 1e-3
            check(hot == "y", f"the pointer over the Y arrow makes it hot ({hot})")
            check(canvas.log_lines() == lines + 1 and snapped_y > before_y and on_grid, f"with snapping on, the drag lands on the {GRID} m grid ({before_y} -> {snapped_y})")
            canvas.input(press("KeyY"))
            time.sleep(0.3)

            # The outliner by pointer: drag the other's row onto the
            # modified entity's row, then free it from the row's menu.
            w = canvas.widgets()
            rows = (f"outliner.row.{args.other}", f"outliner.row.{args.modify}")
            check(all(r in w for r in rows), f"GET /widgets reports the outliner rows ({len(w)} widgets)")
            lines = canvas.log_lines()
            if all(r in w for r in rows):
                canvas.drag(center(w[rows[0]]), center(w[rows[1]]))
                time.sleep(1.2)
            parent = canvas.entity(args.other).get("parent")
            check(parent == modified_id and canvas.log_lines() == lines + 1, f"dragging the {args.other} row onto the {args.modify} row hangs it under it (parent {parent})")
            w = canvas.widgets()
            lines = canvas.log_lines()
            if rows[0] in w:
                at = center(w[rows[0]])
                canvas.input([{"cursor": list(at)}, {"button": "right", "pressed": True}, {"button": "right", "pressed": False}])
                time.sleep(0.5)
                w = canvas.widgets()
                check("menu.Unparent" in w, f"the row's context menu opens ({sorted(k for k in w if k.startswith('menu.'))})")
                if "menu.Unparent" in w:
                    canvas.input(click(center(w["menu.Unparent"])))
                    time.sleep(1.2)
            parent = canvas.entity(args.other).get("parent")
            check(parent is None and canvas.log_lines() == lines + 1, f"the menu's Unparent frees it (parent {parent})")

            # The world bar by name, whichever stack draws it (egui, or
            # feathers with `--features feathers`): the rail and its rows
            # report through the same seam, and a click on the bar's
            # checkboxes reaches the settings.
            w = canvas.widgets()
            rail_rows = sorted(k for k in w if k.startswith("rail.row."))
            check("panel.history_rail" in w and "rail.row.base" in w and len(rail_rows) > 1, f"GET /widgets reports the history rail and its rows ({len(rail_rows)} rows)")
            check(all(k in w for k in ("world_info.snap", "world_info.local", "world_info.review")), f"GET /widgets reports the bar's controls ({sorted(k for k in w if k.startswith('world_info.'))})")
            canvas.select(modified_id)
            local_before = (call(ep, "GET", "/selection")[1].get("handles") or {}).get("local")
            if canvas.click_widget("world_info.local"):
                time.sleep(0.4)
            local_after = (call(ep, "GET", "/selection")[1].get("handles") or {}).get("local")
            check(local_after is not None and local_after != local_before, f"the bar's local checkbox switches the handles' axes ({local_before} -> {local_after})")
            if canvas.click_widget("world_info.local"):
                time.sleep(0.4)
            review_before = call(ep, "GET", "/proposals")[1].get("review")
            if canvas.click_widget("world_info.review"):
                time.sleep(0.4)
            review_after = call(ep, "GET", "/proposals")[1].get("review")
            check(review_after is not None and review_after != review_before, f"the bar's review checkbox switches review ({review_before} -> {review_after})")
            if canvas.click_widget("world_info.review"):
                time.sleep(0.4)
            check(call(ep, "GET", "/proposals")[1].get("review") == review_before, "and switches it back")

            # The gallery (G), on whichever stack reports it by name: Bevy's
            # own widgets do; egui's window has no named widgets yet, so
            # there the check is only that G opens and closes something
            # without a panic.
            canvas.park()
            canvas.input(press("KeyG"))
            time.sleep(0.8)
            w = canvas.widgets()
            if "panel.gallery" in w:
                x0, x1, _, _ = canvas.free_band()
                g = w["panel.gallery"]
                check(all(k in w for k in ("gallery.refresh", "gallery.close", "gallery.filter")), f"the gallery opens with its controls ({sorted(k for k in w if k.startswith('gallery.'))[:6]}…)")
                check(g[0] >= x0 - 1 and g[0] + g[2] <= x1 + 1, f"the gallery sits in the free viewport ({[round(v) for v in g]} within {x0:.0f}..{x1:.0f})")
                if canvas.click_widget("gallery.close"):
                    time.sleep(0.5)
                check("panel.gallery" not in canvas.widgets(), "its Close button closes it")
            else:
                canvas.input(press("KeyG"))
                time.sleep(0.4)
                print("skip the gallery's named checks: this bar is egui's (G toggled it)")

            if herdr:
                # herdr's pane: the bar opens a shell in the world folder,
                # and what herdr says of the agent in it reaches the canvas
                # — blocked as a light, a turn's end as a mark on the rail.
                def agents():
                    return call(ep, "GET", "/agents")[1] or {}

                def agents_once(pred, seconds):
                    deadline = time.time() + seconds
                    while time.time() < deadline:
                        a = agents()
                        if pred(a):
                            return a
                        time.sleep(0.25)
                    return agents()

                a = agents_once(lambda a: (a.get("herdr") or {}).get("server"), 8)
                check(bool((a.get("herdr") or {}).get("server")), f"the canvas sees herdr's socket answer ({a.get('herdr')})")
                opened = canvas.click_widget("world_info.agent")
                time.sleep(0.5)
                w = canvas.widgets()
                check(opened and "menu.terminal here" in w, f"the bar's agent menu offers a terminal in the world folder ({sorted(k for k in w if k.startswith('menu.'))})")
                if "menu.terminal here" in w:
                    canvas.input(click(center(w["menu.terminal here"])))
                a = agents_once(lambda a: a.get("pane"), 10)
                pane = (a.get("pane") or {}).get("id")
                check(bool(pane), f"a pane opens in the world folder ({a.get('pane')}; error {a.get('error')})")
                if pane:
                    info = (herdr.rpc("pane.get", {"pane_id": pane}).get("result") or {}).get("pane") or {}
                    check(os.path.realpath(info.get("cwd") or "") == os.path.realpath(world), f"the pane's working directory is the world ({info.get('cwd')})")
                    herdr.report(pane, "working")
                    a = agents_once(lambda a: (a.get("pane") or {}).get("status") == "working", 5)
                    check((a.get("pane") or {}).get("status") == "working", f"herdr's working state reaches the canvas ({a.get('pane')})")
                    herdr.report(pane, "blocked")
                    a = agents_once(lambda a: (a.get("pane") or {}).get("status") == "blocked", 5)
                    check((a.get("pane") or {}).get("status") == "blocked" and (a.get("pane") or {}).get("agent") == "claude", f"blocked reaches the canvas with the agent's name ({a.get('pane')})")
                    herdr.report(pane, "working")
                    time.sleep(0.6)
                    rows_then = canvas.log_lines()
                    herdr.report(pane, "idle")
                    a = agents_once(lambda a: a.get("turns"), 5)
                    turns = a.get("turns") or []
                    check(len(turns) == 1 and turns[0].get("rows") == rows_then and turns[0].get("agent") == "claude", f"the turn that ended is marked at the log's length ({turns}; log {rows_then})")
                    time.sleep(0.6)
                    w = canvas.widgets()
                    check("rail.turn.1" in w, f"the rail draws the turn mark ({sorted(k for k in w if k.startswith('rail.turn'))})")

            # The detail panel's Parent combo, both ways.
            canvas.select(other_id)
            lines = canvas.log_lines()
            if canvas.click_widget("detail.parent"):
                time.sleep(0.5)
                if canvas.click_widget(f"combo.parent.{args.modify}"):
                    time.sleep(1.2)
            parent = canvas.entity(args.other).get("parent")
            check(parent == modified_id and canvas.log_lines() == lines + 1, f"the Parent combo hangs {args.other} under {args.modify} (parent {parent})")
            lines = canvas.log_lines()
            if canvas.click_widget("detail.parent"):
                time.sleep(0.5)
                if canvas.click_widget("combo.parent.— none —"):
                    time.sleep(1.2)
            parent = canvas.entity(args.other).get("parent")
            check(parent is None and canvas.log_lines() == lines + 1, f"the Parent combo's none frees it (parent {parent})")

            # The Alpha Mode combo, and a value field dragged by pointer.
            canvas.select(modified_id)
            lines = canvas.log_lines()
            if canvas.click_widget("detail.alpha_mode"):
                time.sleep(0.5)
                if canvas.click_widget("combo.alpha_mode.Blend"):
                    time.sleep(1.2)
            alpha = canvas.entity(args.modify).get("material", {}).get("alpha_mode")
            check(str(alpha).lower() == "blend" and canvas.log_lines() == lines + 1, f"the Alpha Mode combo sets Blend ({alpha})")
            w = canvas.widgets()
            y0 = canvas.entity(args.modify).get("transform", {}).get("position", [0, 0, 0])[1]
            lines = canvas.log_lines()
            if "detail.position.y" in w:
                at = center(w["detail.position.y"])
                canvas.drag(at, (at[0] + 40, at[1]))
                time.sleep(1.2)
            y1 = canvas.entity(args.modify).get("transform", {}).get("position", [0, 0, 0])[1]
            check(y1 > y0 + 0.1 and canvas.log_lines() == lines + 1, f"dragging the position y field moves it ({y0} -> {y1})")
            status, body = call(ep, "GET", "/screenshot")
            shots.append(body.get("path"))

            # A shift-click adds a second entity; a drag on it moves both.
            other_at = (canvas.select(other_id).get("handles") or {}).get("origin")
            canvas.select(modified_id)
            check(bool(other_at), f"{args.other}'s place on screen is known ({other_at})")
            if other_at:
                canvas.input([{"key": "ShiftLeft", "pressed": True}] + click(other_at) + [{"key": "ShiftLeft", "pressed": False}])
                time.sleep(0.6)
            selection = call(ep, "GET", "/selection")[1]
            check((selection.get("selected") or {}).get("id") == modified_id and selection.get("others") == [other_id],
                  f"shift-click adds {args.other} to the selection ({(selection.get('selected') or {}).get('id')}, {selection.get('others')})")
            status, body = call(ep, "GET", "/screenshot")  # two selection boxes
            shots.append(body.get("path"))
            x1 = canvas.entity(args.modify).get("transform", {}).get("position", [0, 0, 0])[0]
            x2 = canvas.entity(args.other).get("transform", {}).get("position", [0, 0, 0])[0]
            lines = canvas.log_lines()
            if other_at:
                events = [{"cursor": list(other_at)}, {"button": "left", "pressed": True}]
                events += [{"cursor": [other_at[0] + 10 * i, other_at[1]]} for i in range(1, 7)]
                events += [{"button": "left", "pressed": False}]
                canvas.input(events)
                time.sleep(1.5)
            x1b = canvas.entity(args.modify).get("transform", {}).get("position", [0, 0, 0])[0]
            x2b = canvas.entity(args.other).get("transform", {}).get("position", [0, 0, 0])[0]
            check(canvas.log_lines() == lines + 1, "the drag of the selection is one entry")
            check(x2b > x2 + 0.05 and abs((x1b - x1) - (x2b - x2)) < 0.05, f"both moved together ({x1}->{x1b}, {x2}->{x2b})")

            # A box drawn with Shift held around the cuboid and the capsule
            # selects both — whatever else it encloses comes along — and the
            # release that ends it is not a click on nothing.
            canvas.input(press("Escape"))
            time.sleep(0.3)
            a = (canvas.select(modified_id).get("handles") or {}).get("origin")
            pad = 0.9 * arrow_px()[0] if a else 150.0
            b = (canvas.select(other_id).get("handles") or {}).get("origin")
            canvas.input(press("Escape"))
            time.sleep(0.3)
            if a and b:
                # Inside the free viewport: a corner on a panel is egui's
                # press, not the canvas's, and a drag across a panel's fields
                # edits whatever they hold.
                free_x0, free_x1, free_y1, rail = canvas.free_band()
                lo = (max(min(a[0], b[0]) - pad, free_x0), max(min(a[1], b[1]) - pad, 80.0))
                hi = (min(max(a[0], b[0]) + pad, free_x1), min(max(a[1], b[1]) + pad, free_y1))
                # The press and the release land off the rail, which floats
                # in the band; the drag may cross it.
                if canvas.inside(lo, rail):
                    lo = (lo[0], min(lo[1], rail[1] - 8))
                if canvas.inside(hi, rail):
                    hi = (hi[0], max(hi[1], rail[1] + rail[3] + 8))
                panels = {n: [round(v) for v in r] for n, r in canvas.panels().items()}
                canvas.input([{"key": "ShiftLeft", "pressed": True}])
                mid_box = canvas.drag_box(lo, hi, steps=10)
                canvas.input([{"key": "ShiftLeft", "pressed": False}])
                time.sleep(0.8)
            else:
                mid_box, panels = None, {}
            selection = call(ep, "GET", "/selection")[1]
            picked = {(selection.get("selected") or {}).get("id")} | set(selection.get("others") or [])
            check({modified_id, other_id} <= picked,
                  f"a Shift-box around {args.modify} and {args.other} selects both ({sorted(i for i in picked if i is not None)}; "
                  f"box {lo} -> {hi}, mid-drag {mid_box}, a {a}, b {b}, panels {panels})")
            # Back to just the pair, for the grouping below.
            canvas.select(modified_id)
            time.sleep(0.3)
            other_at = (canvas.select(other_id).get("handles") or {}).get("origin")
            canvas.select(modified_id)
            time.sleep(0.3)
            if other_at:
                canvas.input([{"key": "ShiftLeft", "pressed": True}] + click(other_at) + [{"key": "ShiftLeft", "pressed": False}])
                time.sleep(0.6)
            pair = call(ep, "GET", "/selection")[1]
            check((pair.get("selected") or {}).get("id") == modified_id and pair.get("others") == [other_id],
                  f"the pair is selected again ({(pair.get('selected') or {}).get('id')}, {pair.get('others')})")

            # Cmd+G groups them under a new empty; Delete takes the group.
            lines = canvas.log_lines()
            canvas.input([{"key": "SuperLeft", "pressed": True}] + press("KeyG") + [{"key": "SuperLeft", "pressed": False}])
            time.sleep(1.0)
            group = canvas.entity("group")
            parents = {canvas.entity(args.modify).get("parent"), canvas.entity(args.other).get("parent")}
            check(canvas.log_lines() == lines + 1 and group.get("id") is not None and parents == {group.get("id")},
                  f"Cmd+G groups the selection under 'group' ({group.get('id')}, parents {parents})")
            selected = (call(ep, "GET", "/selection")[1].get("selected") or {}).get("name")
            check(selected == "group", f"the new group is selected ({selected})")
            status, body = call(ep, "GET", "/screenshot")  # the group, its children in the outliner
            shots.append(body.get("path"))
            lines = canvas.log_lines()
            canvas.input(press("Delete"))
            time.sleep(1.0)
            gone = not canvas.entity("group") and not canvas.entity(args.modify) and not canvas.entity(args.other)
            check(canvas.log_lines() == lines + 1 and gone, "Delete takes the group and what hangs under it, as one entry")
            status, body = call(ep, "GET", "/screenshot")
            shots.append(body.get("path"))

            status, body = call(ep, "POST", "/ops", {"author": "smoke", "ops": [
                {"SpawnEntity": {"entity": {"name": "smoke-watcher", "shape": {"Sphere": {"radius": 0.3}},
                                            "transform": {"position": [2.0, 1.0, 2.0]},
                                            "behaviors": [{"LookAt": {"target": args.select}}]}}}]})
            check(status == 200, f"a watcher that looks at entity {args.select} spawns ({status} {body.get('errors')})")
            status, body = call(ep, "POST", "/ops", {"author": "smoke", "ops": [{"DeleteEntity": {"id": args.select}}]})
            warned = any("targeting" in w and "not in the world" in w for w in body.get("warnings", []))
            check(status == 200 and warned, f"deleting its target lands, and the receipt warns ({body.get('warnings')})")

            # A target sent by name commits as the id (the cold agent test
            # found it committing as the name, and the package then no
            # longer verified against its history); a name nothing has
            # refuses the batch, naming it.
            status, body = call(ep, "POST", "/ops", {"author": "smoke", "ops": [
                {"SpawnEntity": {"entity": {"name": "smoke-gazer", "shape": {"Sphere": {"radius": 0.2}},
                                            "transform": {"position": [-6.0, 1.0, 2.0]},
                                            "behaviors": [{"LookAt": {"target": "sphere"}}]}}}]})
            gazer = canvas.entity("smoke-gazer")
            target = ((gazer.get("behaviors") or [{}])[0].get("LookAt") or {}).get("target")
            check(status == 200 and target == 11, f"a LookAt target sent by name commits as the id ({target})")
            status, body = call(ep, "GET", "/verify")
            check(status == 200 and body.get("ok") is True, f"the package verifies against its history ({body.get('problems')})")
            status, body = call(ep, "POST", "/ops", {"author": "smoke", "ops": [
                {"ModifyEntity": {"id": "smoke-gazer", "patch": {"behaviors": [{"LookAt": {"target": "nobody"}}]}}}]})
            check(status == 422 and any("nobody" in e for e in body.get("errors", [])), f"a target nothing is named refuses the batch, naming it ({status} {body.get('errors')})")
            status, body = call(ep, "GET", "/world")
            noisy = [v for e in body.get("entities", []) for v in (e.get("transform", {}).get("position") or []) if isinstance(v, float) and len(repr(v)) > 12]
            check(status == 200 and not noisy, f"GET /world prints the numbers manifest.json prints ({noisy[:3]})")

            # An orbit around an entity resolves by the id the op committed
            # with: the second cold run watched its moon circle the origin,
            # because the behavior tick knew its targets by name only. The
            # orbiter's place on screen is by the sphere's, far from the
            # torus at the origin, and it moves.
            status, body = call(ep, "POST", "/ops", {"author": "smoke", "ops": [
                {"SpawnEntity": {"entity": {"name": "smoke-moon", "shape": {"Sphere": {"radius": 0.15}},
                                            "transform": {"position": [-12.0, 2.0, 0.0]},
                                            "behaviors": [{"Orbit": {"center": "sphere", "radius": 1.0, "speed": 90}}]}}}]})
            moon_id = (body.get("spawned") or {}).get("smoke-moon")
            check(status == 200 and moon_id is not None, f"an orbiter around the sphere spawns ({status} {body.get('errors')})")
            sphere_at = (canvas.select(11).get("handles") or {}).get("origin")
            torus_at = (canvas.select(15).get("handles") or {}).get("origin")
            moon_at = (canvas.select(moon_id).get("handles") or {}).get("origin") if moon_id else None
            time.sleep(0.5)
            moon_later = (call(ep, "GET", "/selection")[1].get("handles") or {}).get("origin")

            def px(a, b):
                return math.hypot(a[0] - b[0], a[1] - b[1]) if a and b else float("inf")

            check(px(moon_at, sphere_at) < 160 and px(moon_at, torus_at) > 250,
                  f"the orbiter circles the sphere, not the origin ({px(moon_at, sphere_at):.0f} px from the sphere, {px(moon_at, torus_at):.0f} px from the torus)")
            check(px(moon_at, moon_later) > 2, f"and it moves ({px(moon_at, moon_later):.1f} px in half a second)")

            # Reads the second cold run asked for: the world at any entry by
            # id, the log's message at the top level, an undo receipt that
            # names the head like a batch's, and "at": null refused rather
            # than landed on main.
            log = call(ep, "GET", "/log")[1]
            first = (log.get("entries") or [{}])[0]
            check("message" in first and first.get("message") == (first.get("intent") or {}).get("message"),
                  f"a log entry carries its message at the top level ({first.get('message')!r})")
            status, body = call(ep, "GET", "/world?tip=" + str(first.get("id")))
            names_then = {e.get("name") for e in (body.get("entities") or [])} if isinstance(body, dict) else set()
            check(status == 200 and "smoke-moon" not in names_then and "cuboid" in names_then,
                  f"GET /world?tip=<id> folds the world at that entry ({status}, {len(names_then)} entities)")
            status, body = call(ep, "GET", "/world?tip=no-such-entry")
            check(status == 404, f"an unknown tip is a 404, not the head ({status})")
            status, body = call(ep, "POST", "/undo", {"author": "smoke"})
            check(status == 200 and body.get("head") == body.get("id") and body.get("parent") and body.get("branch") is False,
                  f"an undo receipt names parent, head and branch like a batch's ({sorted(body.keys()) if isinstance(body, dict) else body})")
            status, body = call(ep, "POST", "/ops", {"author": "smoke", "at": None, "ops": [{"SpawnEntity": {"entity": {"name": "smoke-stray"}}}]})
            check(status == 422 and any("base" in e for e in body.get("errors", [])) and canvas.entity("smoke-stray").get("id") is None,
                  f"at null is refused once there is history, not landed on main ({status} {body.get('errors')})")

            # Undo and redo are per path: a branch's undo is the branch's
            # child, its redo too, and main's own redo is untouched by them
            # (it brings back the moon, undone above).
            log = call(ep, "GET", "/log")[1]
            first_id = (log.get("entries") or [{}])[0].get("id")
            status, body = call(ep, "POST", "/ops", {"author": "smoke", "at": first_id, "ops": [
                {"SpawnEntity": {"entity": {"name": "smoke-branch-post", "shape": {"Cylinder": {"radius": 0.05, "height": 2.0}},
                                            "transform": {"position": [4.0, 1.0, 4.0]}}}}]})
            b1, b1_rev = body.get("id"), body.get("revision")
            check(status == 200 and body.get("branch") is True and b1 is not None,
                  f"a batch at the first entry starts a branch ({status} branch={body.get('branch')})")
            status, body = call(ep, "POST", "/undo", {"author": "smoke", "at": b1})
            u1 = body.get("id")
            check(status == 200 and body.get("branch") is True and body.get("parent") == b1 and body.get("warnings") == [f"undid revision {b1_rev}"],
                  f"an undo at the branch's tip is the branch's child ({status} {body.get('warnings')})")
            status, body = call(ep, "GET", "/world?tip=" + str(u1))
            check(status == 200 and not any(e.get("name") == "smoke-branch-post" for e in body.get("entities", [])),
                  "the branch after its undo has no post")
            status, body = call(ep, "POST", "/redo", {"author": "smoke", "at": u1})
            r1 = body.get("id")
            check(status == 200 and body.get("branch") is True and body.get("parent") == u1 and body.get("warnings") == [f"redid revision {b1_rev}"],
                  f"a redo at the undo's tip brings the post back on the branch ({status} {body.get('warnings')})")
            status, body = call(ep, "GET", "/world?tip=" + str(r1))
            check(status == 200 and any(e.get("name") == "smoke-branch-post" for e in body.get("entities", [])),
                  "the branch after its redo has the post again")
            status, body = call(ep, "POST", "/redo", {"author": "smoke"})
            check(status == 200 and body.get("branch") is False and body.get("warnings") != [f"redid revision {b1_rev}"]
                  and canvas.entity("smoke-moon").get("id") is not None,
                  f"main's own redo is untouched by the branch's: it brings the moon back ({status} {body.get('warnings')})")


            # Undo and redo. The person's: a nudge of the sphere commits as
            # theirs; Cmd+Z takes back their own newest (not the agent's
            # delete just above), Cmd+Shift+Z brings it back. Anyone's:
            # the API's /undo and /redo on an agent's spawn.
            def sphere_x():
                return canvas.entity("sphere").get("transform", {}).get("position", [0, 0, 0])
            canvas.select(11)
            time.sleep(0.3)
            p0 = sphere_x()
            lines = canvas.log_lines()
            canvas.input(press("ArrowRight"))
            time.sleep(1.5)
            p1 = sphere_x()
            check(canvas.log_lines() == lines + 1 and p1 != p0, f"a nudge of the sphere commits ({p0} -> {p1})")
            canvas.input([{"key": "SuperLeft", "pressed": True}] + press("KeyZ") + [{"key": "SuperLeft", "pressed": False}])
            time.sleep(1.2)
            p2 = sphere_x()
            check(canvas.log_lines() == lines + 2 and p2 == p0, f"Cmd+Z undoes the person's own nudge, not the agent's delete ({p2})")
            check(canvas.entity(args.select).get("id") is None, "the agent's delete above stands")
            canvas.input([{"key": "SuperLeft", "pressed": True}, {"key": "ShiftLeft", "pressed": True}] + press("KeyZ")
                         + [{"key": "ShiftLeft", "pressed": False}, {"key": "SuperLeft", "pressed": False}])
            time.sleep(1.2)
            p3 = sphere_x()
            check(canvas.log_lines() == lines + 3 and p3 == p1, f"Cmd+Shift+Z redoes it ({p3})")
            status, body = call(ep, "POST", "/ops", {"author": "smoke", "ops": [
                {"SpawnEntity": {"entity": {"name": "smoke-post", "shape": {"Cylinder": {"radius": 0.1, "height": 1.5}},
                                            "transform": {"position": [-2.0, 0.75, 2.0]}}}}]})
            check(status == 200, f"an agent spawns a post ({status})")
            status, body = call(ep, "POST", "/undo", {"author": "smoke"})
            check(status == 200 and canvas.entity("smoke-post").get("id") is None, f"POST /undo takes the newest change back ({status} {body.get('warnings')})")
            status, body = call(ep, "POST", "/redo", {"author": "smoke"})
            check(status == 200 and canvas.entity("smoke-post").get("id") is not None, f"POST /redo brings it back ({status} {body.get('warnings')})")
            status, body = call(ep, "POST", "/undo", {"author": "nobody", "own": True})
            check(status == 409 and any("nothing to undo" in e for e in body.get("errors", [])), f"an own undo with nothing of one's own is refused ({status})")

            # A direct write to manifest.json is put back within the guard's
            # half second, and noted where an agent can read why its change
            # vanished.
            manifest_path = os.path.join(world, "manifest.json")
            with open(manifest_path, "rb") as f:
                before_bytes = f.read()
            with open(manifest_path, "ab") as f:
                f.write(b"\n")
            time.sleep(1.5)
            with open(manifest_path, "rb") as f:
                after_bytes = f.read()
            status, body = call(ep, "GET", "/refusals")
            notes = body.get("refusals", []) if isinstance(body, dict) else []
            check(after_bytes == before_bytes, "a direct write to manifest.json is put back")
            check(status == 200 and any("direct write" in n.get("what", "") for n in notes), f"GET /refusals names it ({len(notes)} note(s))")

            # A modulation committed to the open scene: an offset_y driven by
            # a free-running oscillator bobs the torus, which shows as its
            # place on screen moving. Last before the git phase, so that the
            # checkout of HEAD~1 there is exactly this commit's removal.
            def torus_bob(samples=8, gap=0.4):
                """How far the torus travels vertically while sampled, in
                metres, as the scene draws it: `scene.rendered` is the
                primary's global transform after modulation and the frame's
                last propagation. (The handle origin is a `GlobalTransform`
                read in `Update`, which the physics schedule's own
                propagation makes the authored pose on any frame with a
                step — a measurement that read the modulation as absent
                while it showed on screen.) `torus_bob.saw` keeps the last
                raw reply, for a failure to explain itself."""
                canvas.select(15)
                time.sleep(0.2)
                ys = []
                for _ in range(samples):
                    s = call(ep, "GET", "/selection")[1]
                    scene = s.get("scene") or {}
                    torus_bob.saw = {"selected": (s.get("selected") or {}).get("id"), "handles": bool(s.get("handles")),
                                     "origin": (s.get("handles") or {}).get("origin"), "scene": scene, "clock": s.get("clock")}
                    rendered = scene.get("rendered")
                    if rendered:
                        ys.append(rendered[1])
                    time.sleep(gap)
                torus_bob.ys = [round(y, 3) for y in ys]
                return (max(ys) - min(ys)) if ys else 0.0

            still = torus_bob()
            status, body = call(ep, "POST", "/ops", {"author": "smoke", "ops": [
                {"ModifyEntity": {"id": "torus", "patch": {"modulations": [
                    {"target": "offset_y", "signal": {"oscillator": {"frequency": 0.7}}, "range": [0.0, 1.5]}]}}}]})
            check(status == 200, f"a modulation batch on the torus commits ({status} {body.get('errors')})")
            on_screen = body.get("on_screen")
            bobbing = torus_bob()
            check(still < 0.01 and bobbing > 0.5,
                  f"the committed modulation bobs the torus as drawn ({still:.3f} m of travel still, {bobbing:.3f} m with it; "
                  f"on_screen {on_screen}, saw {torus_bob.saw}, y {torus_bob.ys})")

            # In a git world: a checkout under the open window is followed,
            # and a replay walks the commits and comes back to the head.
            if os.path.isdir(os.path.join(world, ".git")):
                def git(*a):
                    r = subprocess.run(["git", "-C", world] + list(a), capture_output=True, text=True)
                    check(r.returncode == 0, "git %s works under the open window (%s)" % (" ".join(a), r.stderr.strip()))
                    return r.stdout.strip()
                branch = git("rev-parse", "--abbrev-ref", "HEAD")
                dirty = git("status", "--porcelain")
                check(dirty == "", f"the committer leaves the git tree clean, a restored manifest included ({dirty!r})")
                rev_before = call(ep, "GET", "/log")[1].get("revision")
                bob_head = torus_bob()
                git("checkout", "--quiet", "HEAD~1")
                time.sleep(1.5)
                rev_back = call(ep, "GET", "/log")[1].get("revision")
                check(rev_back == rev_before - 1, f"a checkout under the open window is followed (revision {rev_before} -> {rev_back})")
                bob_back = torus_bob()
                check(bob_head > 0.5 and bob_back < 0.01,
                      f"the scene shows the checked-out state: the torus's modulation, committed last, is gone ({bob_head:.3f} m -> {bob_back:.3f} m; saw {torus_bob.saw})")
                git("checkout", "--quiet", branch)
                time.sleep(1.5)
                check(call(ep, "GET", "/log")[1].get("revision") == rev_before and torus_bob() > 0.5,
                      "and back on the branch, the torus bobs again")

                # A replay shows each commit's own asset bytes. A swatch
                # texture goes in through a batch (stored by hash); its stored
                # copy is then overwritten with other bytes by hand and
                # committed, so the working tree lies about history — and the
                # replayed frame of the spawn still draws the committed red.
                import struct
                import zlib

                def png_bytes(w, h, rgb):
                    raw = b"".join(b"\x00" + bytes(rgb) * w for _ in range(h))

                    def chunk(tag, data):
                        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xffffffff)

                    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                            + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))

                def png_mean(path, cx, cy, half=10):
                    """Mean RGB of a patch of an 8-bit RGB/RGBA PNG, no interlace."""
                    data = open(path, "rb").read()
                    pos, idat, w, h, bpp = 8, b"", 0, 0, 4
                    while pos + 8 <= len(data):
                        n, tag = struct.unpack(">I4s", data[pos:pos + 8])
                        body = data[pos + 8:pos + 8 + n]
                        pos += 12 + n
                        if tag == b"IHDR":
                            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
                            if depth != 8 or interlace != 0 or ctype not in (2, 6):
                                return None
                            bpp = 4 if ctype == 6 else 3
                        elif tag == b"IDAT":
                            idat += body
                        elif tag == b"IEND":
                            break
                    raw = zlib.decompress(idat)
                    stride = w * bpp
                    prev = bytearray(stride)
                    rows = []
                    for y in range(min(h, cy + half + 1)):
                        f = raw[y * (stride + 1)]
                        line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
                        if f:
                            for i in range(stride):
                                a = line[i - bpp] if i >= bpp else 0
                                b = prev[i]
                                c = prev[i - bpp] if i >= bpp else 0
                                if f == 1:
                                    line[i] = (line[i] + a) & 255
                                elif f == 2:
                                    line[i] = (line[i] + b) & 255
                                elif f == 3:
                                    line[i] = (line[i] + ((a + b) >> 1)) & 255
                                else:
                                    pr = a + b - c
                                    pa, pb, pc = abs(pr - a), abs(pr - b), abs(pr - c)
                                    line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
                        rows.append(line)
                        prev = line
                    sums, n = [0, 0, 0], 0
                    for y in range(max(0, cy - half), min(len(rows), cy + half)):
                        for x in range(max(0, cx - half), min(w, cx + half)):
                            q = rows[y][x * bpp:x * bpp + 3]
                            sums[0] += q[0]
                            sums[1] += q[1]
                            sums[2] += q[2]
                            n += 1
                    return tuple(s / max(n, 1) for s in sums)

                red, blue = png_bytes(8, 8, (220, 30, 30)), png_bytes(8, 8, (30, 30, 220))
                with open(os.path.join(world, "assets", "swatch.png"), "wb") as f:
                    f.write(red)
                status, body = call(ep, "POST", "/ops", {"author": "smoke", "message": "a swatch to replay", "ops": [
                    {"SpawnEntity": {"entity": {"name": "smoke-swatch", "shape": {"Cuboid": {"x": 3.0, "y": 3.0, "z": 0.2}},
                                                "transform": {"position": [0.0, 2.5, 8.0]},
                                                "material": {"color": [1.0, 1.0, 1.0, 1.0], "base_color_texture": "swatch.png"}}}}]})
                swatch_id = (body.get("spawned") or {}).get("smoke-swatch")
                stored = (body.get("stored") or {}).get("swatch.png")
                check(status == 200 and swatch_id is not None and bool(stored),
                      f"a textured swatch spawns, its texture stored by hash ({status} {stored})")
                spawn_commit = git("log", "-n1", "--format=%H", "--", "manifest.json")
                if stored:
                    with open(os.path.join(world, "assets", stored), "wb") as f:
                        f.write(blue)
                    git("add", "-A")
                    git("commit", "--quiet", "-m", "the swatch's stored copy overwritten by hand")
                    time.sleep(1.5)
                rev_head = call(ep, "GET", "/log")[1].get("revision")
                status, body = call(ep, "POST", "/replay?seconds=0.3", {})
                check(status == 200 and body.get("result") == "replaying", f"a replay starts ({status} {body})")
                done = False
                for _ in range(400):
                    time.sleep(0.25)
                    with open(os.path.join(world, ".live", "smoke-commit.log")) as f:
                        if "replay done" in f.read():
                            done = True
                            break
                check(done, "the replay walks every commit and returns to the head")
                check(call(ep, "GET", "/log")[1].get("revision") == rev_head, "the world is at the head after the replay")
                cached = os.path.join(world, ".live", "replay", spawn_commit, "assets", stored or "")
                check(bool(stored) and os.path.isfile(cached) and open(cached, "rb").read() == red,
                      "the spawn's commit was materialized with its own bytes (red), the working tree's being blue")
                time.sleep(1.5)
                at = (canvas.select(swatch_id).get("handles") or {}).get("origin") if swatch_id else None
                # The offscreen image has the panels composited in, and the
                # detail panel grows to fit the texture's hashed name: sample
                # the swatch where the free viewport shows it, a little in
                # from the panel's edge (the swatch is some 230 px wide).
                free_x0, free_x1, _, rail = canvas.free_band()
                sample = (min(max(at[0], free_x0 + 40), free_x1 - 40), at[1]) if at else None
                if sample and canvas.inside(sample, rail):
                    sample = (max(min(sample[0], rail[0] - 40), free_x0 + 40), sample[1])
                status, body = call(ep, "GET", "/screenshot")
                shots.append(body.get("path"))
                mean = png_mean(body.get("path"), int(sample[0]), int(sample[1])) if (sample and body.get("path")) else None
                check(mean is not None and mean[0] > mean[2] + 40 and mean[0] > 100,
                      f"the replayed frame draws the swatch with the committed red, not today's blue (mean RGB {tuple(round(m) for m in mean) if mean else None} "
                      f"sampled at {tuple(round(v) for v in sample) if sample else None}; origin {at}, free band {free_x0:.0f}..{free_x1:.0f})")
            else:
                print("skip the checkout and replay checks: not a git world (init with --git)")
        finally:
            canvas.stop()
            if herdr:
                herdr.stop()
    with open(os.path.join(world, ".live", "smoke-commit.log")) as f:
        check("panicked" not in f.read(), "no panic with review off")

    print("screenshots: " + " ".join(s for s in shots if s))
    if check.failures:
        sys.exit("%d check(s) failed" % len(check.failures))
    print("all checks passed")


if __name__ == "__main__":
    main()
