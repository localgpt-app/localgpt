#!/usr/bin/env python3
"""Smoke-test the live canvas with no one at the keyboard.

Launches `localgpt-gen --live <world>` with the inspector open
(`LOCALGPT_INSPECTOR=full`), drives it over the live API in
`<world>/.live/endpoint.json`, the inspector's WebSocket (port 9877) and
a synthesized hand (`LOCALGPT_SYNTHETIC_INPUT=1`, `POST /input`), and
checks what an editor is judged by: selection, proposals rejected and
accepted by key and by the bar's buttons, the busy replies while one is
pending and the log untouched, the stale-revision refusal, a commit with
the right expected revision, a handle drag, snapping, key turns, a
shift-click multi-selection dragged together, grouping, deleting, and
the receipt's warning when a delete leaves a behavior without its
target. Screenshots land in `<world>/.live/` for a person to look at.

    cargo run -p localgpt-world-agent --example live -- init demo.world
    cargo build -p localgpt-gen
    scripts/live-smoke.py demo.world            # --offscreen on a locked screen

The world must be a live package (the `init` above) with the example's
entities: a cuboid (10), a capsule (14) and a lantern (100).
"""

import argparse
import base64
import json
import os
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WS_PORT = 9877
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
        return call(self.ep, "POST", "/input", {"events": events})

    def select(self, entity_id):
        ws_send(WS_PORT, json.dumps({"type": "select_entity", "entity_id": entity_id}))
        time.sleep(0.3)
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
        time.sleep(0.2)
        hot = (call(self.ep, "GET", "/selection")[1].get("handles") or {}).get("hot")
        events = [{"button": "left", "pressed": True}]
        events += [{"cursor": [start[0] + ux * step * i, start[1] + uy * step * i]} for i in range(1, px // step + 1)]
        events += [{"button": "left", "pressed": False}]
        self.input(events)
        return hot


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
    parser.add_argument("--select", type=int, default=100, help="entity id to select over the inspector socket")
    parser.add_argument("--modify", default="cuboid", help="entity the proposed batch moves and recolors")
    parser.add_argument("--other", default="capsule", help="a solid entity to shift-click into the selection")
    parser.add_argument("--offscreen", action="store_true", help="render into an image for the screenshots (a locked or sleeping screen leaves the window black)")
    args = parser.parse_args()
    Canvas.offscreen = args.offscreen
    world = os.path.abspath(args.world)
    binary = os.path.join(ROOT, "target", "debug", "localgpt-gen")
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
            buttons = call(ep, "GET", "/proposals")[1].get("buttons") or {}
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
            buttons = call(ep, "GET", "/proposals")[1].get("buttons") or {}
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

            near_len, near_origin, _ = arrow_px()
            check(hot_off_arrow(8) == "x" and hot_off_arrow(60) != "x", "hover forgives 8 px off the arrow, not 60")
            canvas.input([{"key": "KeyW", "pressed": True}])
            time.sleep(0.45)
            canvas.input([{"key": "KeyW", "pressed": False}])
            time.sleep(0.4)
            far_len, far_origin, _ = arrow_px()
            moved_px = ((far_origin[0] - near_origin[0]) ** 2 + (far_origin[1] - near_origin[1]) ** 2) ** 0.5
            check(moved_px > 5 and abs(far_len - near_len) / near_len < 0.3,
                  f"the handles keep their screen size as the camera moves ({near_len:.0f} px -> {far_len:.0f} px, the origin shifted {moved_px:.0f} px)")
            check(hot_off_arrow(8) == "x" and hot_off_arrow(60) != "x", "hover tolerance holds from the new distance")
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
            warned = any("targets" in w and "gone" in w for w in body.get("warnings", []))
            check(status == 200 and warned, f"deleting its target lands, and the receipt warns ({body.get('warnings')})")
        finally:
            canvas.stop()
    with open(os.path.join(world, ".live", "smoke-commit.log")) as f:
        check("panicked" not in f.read(), "no panic with review off")

    print("screenshots: " + " ".join(s for s in shots if s))
    if check.failures:
        sys.exit("%d check(s) failed" % len(check.failures))
    print("all checks passed")


if __name__ == "__main__":
    main()
