#!/usr/bin/env python3
"""Smoke-test the live canvas with no one at the keyboard.

Launches `localgpt-gen --live <world>` with the inspector open
(`LOCALGPT_INSPECTOR=full`), drives it over the live API in
`<world>/.live/endpoint.json` and the inspector's WebSocket (port 9877),
and checks what an editor is judged by: selection, a reviewed proposal
that leaves the log untouched, the busy replies while it is pending, the
stale-revision refusal, a commit with the right expected revision, and
the receipt's warning when a delete leaves a behavior without its
target. Screenshots land in `<world>/.live/` for a person to look at.

    cargo run -p localgpt-world-agent --example live -- init demo.world
    cargo build -p localgpt-gen
    scripts/live-smoke.py demo.world

The world must be a live package (the `init` above). Accepting or
rejecting a proposal is the person's key press and is not driven here.
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


class Canvas:
    """The app as a subprocess: launched, driven, stopped."""

    def __init__(self, binary, world, review, log):
        self.world = world
        ep_file = os.path.join(world, ".live", "endpoint.json")
        if os.path.exists(ep_file):
            os.remove(ep_file)
        env = dict(os.environ, LOCALGPT_INSPECTOR="full")
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
    args = parser.parse_args()
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

    # Review on: the batch shows and waits; nothing is written.
    with open(os.path.join(world, ".live", "smoke-review.log"), "w") as log:
        canvas = Canvas(binary, world, True, log)
        try:
            ep = canvas.ep
            status, body = call(ep, "GET", "/")
            check(status == 200 and "GET /proposals" in body.get("endpoints", {}), "the API lists /proposals")
            ws_send(WS_PORT, json.dumps({"type": "select_entity", "entity_id": args.select}))
            status, body = call(ep, "GET", "/selection")
            selected = (body.get("selected") or {}).get("id")
            check(selected == args.select, f"the inspector socket selects entity {args.select} (got {selected})")
            status, body = call(ep, "GET", "/screenshot")
            shots.append(body.get("path"))
            before = canvas.log_lines()
            status, body = call(ep, "POST", "/ops", batch)
            check(status == 202 and body.get("result") == "proposed", f"a batch is proposed, not committed ({status} {body.get('result')})")
            status, body = call(ep, "GET", "/proposals")
            check(body.get("pending") == 1, "GET /proposals shows it pending")
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

    # Review off: expected revisions, receipts, a dangling target.
    with open(os.path.join(world, ".live", "smoke-commit.log"), "w") as log:
        canvas = Canvas(binary, world, False, log)
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
            ws_send(WS_PORT, json.dumps({"type": "select_entity", "entity_id": 10}))
            time.sleep(0.5)
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
