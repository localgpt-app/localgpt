#!/usr/bin/env python3
"""Which workspace crates still need publishing at their current version.

The release workflow (.github/workflows/release.yml) runs this before
`cargo publish --workspace`, so a rerun after a partial publish (a rate
limit, a network blip) picks up where the last run stopped instead of
failing on the first crate that is already up: cargo refuses to upload a
version crates.io has.

It reads the publishable members from `cargo metadata` (`publish = false`
ones are skipped, as cargo skips them) and asks crates.io's sparse index
whether each one's version is there. Prints a summary to stdout and, under
GitHub Actions, writes `excludes` (the `--exclude` flags for crates already
published) and `remaining` (how many are left) to $GITHUB_OUTPUT.

    python3 scripts/release-plan.py
"""

import json
import os
import subprocess
import sys
import urllib.error
import urllib.request


def index_url(name: str) -> str:
    n = name.lower()
    if len(n) <= 2:
        return f"https://index.crates.io/{len(n)}/{n}"
    if len(n) == 3:
        return f"https://index.crates.io/3/{n[0]}/{n}"
    return f"https://index.crates.io/{n[:2]}/{n[2:4]}/{n}"


def published_versions(name: str) -> set[str]:
    request = urllib.request.Request(index_url(name), headers={"User-Agent": "localgpt-release-plan"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            lines = response.read().decode().splitlines()
    except urllib.error.HTTPError as e:
        if e.code == 404:  # never published
            return set()
        raise
    return {json.loads(line)["vers"] for line in lines if line.strip()}


def main() -> None:
    meta = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
    members = sorted((p for p in meta["packages"] if p.get("publish") != []), key=lambda p: p["name"])
    done, left = [], []
    for p in members:
        (done if p["version"] in published_versions(p["name"]) else left).append(p)
    for p in done:
        print(f"up already  {p['name']} {p['version']}")
    for p in left:
        print(f"to publish  {p['name']} {p['version']}")
    excludes = " ".join(f"--exclude {p['name']}" for p in done)
    if out := os.environ.get("GITHUB_OUTPUT"):
        with open(out, "a") as f:
            f.write(f"excludes={excludes}\nremaining={len(left)}\n")
    if summary := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary, "a") as f:
            f.write(f"### crates.io\n\n{len(left)} to publish, {len(done)} up already.\n\n")
            f.write("".join(f"- `{p['name']}` {p['version']}\n" for p in left))
    if not members:
        sys.exit("no publishable members")


if __name__ == "__main__":
    main()
