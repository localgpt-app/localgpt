#!/usr/bin/env python3
"""The world-in-the-page measurement's page server: the built page, the
song's world and its assets, and the reports the page posts back. Listens
on 127.0.0.1 only.

usage: serve.py <ui-dist> <songs-dir> <out-dir> [port]
"""
import http.server
import base64
import json
import os
import sys
import urllib.parse

DIST, SONGS, OUT = (os.path.abspath(p) for p in sys.argv[1:4])
PORT = int(sys.argv[4]) if len(sys.argv) > 4 else 8765


def world_dir():
    """The one song the spike's workspace holds."""
    try:
        names = sorted(n for n in os.listdir(SONGS) if os.path.isdir(os.path.join(SONGS, n)))
    except FileNotFoundError:
        names = []
    return os.path.join(SONGS, names[0]) if names else SONGS


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".js": "text/javascript",
        ".css": "text/css",
        ".json": "application/json",
        ".glb": "model/gltf-binary",
        ".svg": "image/svg+xml",
    }

    def translate_path(self, path):
        p = urllib.parse.unquote(urllib.parse.urlsplit(path).path)
        if p.startswith("/world/"):
            root = world_dir()
            full = os.path.normpath(os.path.join(root, p[len("/world/") :]))
            return full if full.startswith(root) else os.path.join(root, "missing")
        full = os.path.normpath(os.path.join(DIST, p.lstrip("/") or "index.html"))
        if not full.startswith(DIST) or not os.path.isfile(full):
            full = os.path.join(DIST, "index.html")
        return full

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def do_POST(self):
        if self.path.startswith("/__spike/log"):
            body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
            with open(os.path.join(OUT, "page.log"), "ab") as f:
                f.write(body + b"\n")
            self.send_response(204)
            self.end_headers()
            return
        if not self.path.startswith("/__spike/report"):
            self.send_error(404)
            return
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        report = json.loads(body)
        run = report.get("run", "run")
        snapshot = report.pop("snapshot", None)
        if snapshot:
            with open(os.path.join(OUT, f"{run}-world.jpg"), "wb") as f:
                f.write(base64.b64decode(snapshot.split(",", 1)[1]))
        with open(os.path.join(OUT, f"{run}-page.json"), "w") as f:
            json.dump(report, f)
        self.send_response(204)
        self.end_headers()

    def log_message(self, fmt, *args):
        with open(os.path.join(OUT, "server.log"), "a") as f:
            f.write("%s %s\n" % (self.log_date_time_string(), fmt % args))


os.makedirs(OUT, exist_ok=True)
http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
