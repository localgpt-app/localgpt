#!/usr/bin/env python3
"""Make every published workspace crate trust the release workflow.

crates.io's Trusted Publishing is configured per crate. This gives each
publishable member that exists on crates.io the same GitHub configuration:
owner `localgpt-app`, repository `localgpt`, workflow `release.yml`,
environment `crates-io` (what .github/workflows/release.yml runs as).
Crates that already have it are left alone. Crates not on crates.io yet
are listed: crates.io can trust a workflow only for a crate that exists, so
publish it once, then run this again.

    CRATES_IO_TOKEN=... python3 scripts/trust-release-workflow.py [--dry-run]

The token needs the `trusted-publishing` scope (crates.io > Account
Settings > API Tokens), best limited to the crates `localgpt*`. Delete it
when this has run: the workflow itself needs no token.
"""

import json
import os
import subprocess
import sys
import urllib.error
import urllib.request

API = "https://crates.io/api/v1"
WANT = {
    "repository_owner": "localgpt-app",
    "repository_name": "localgpt",
    "workflow_filename": "release.yml",
    "environment": "crates-io",
}


def call(method: str, path: str, token: str | None, body: dict | None = None):
    request = urllib.request.Request(
        API + path,
        method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"User-Agent": "localgpt-trust-release-workflow", "Accept": "application/json"},
    )
    if token:
        request.add_header("Authorization", token)
    if body is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.status, json.loads(response.read() or b"null")
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read() or b"null")


def main() -> None:
    dry_run = "--dry-run" in sys.argv[1:]
    token = os.environ.get("CRATES_IO_TOKEN")
    if not token and not dry_run:
        sys.exit(__doc__)
    meta = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1"]))
    names = sorted(p["name"] for p in meta["packages"] if p.get("publish") != [])
    failed = False
    for name in names:
        status, _ = call("GET", f"/crates/{name}", None)
        if status == 404:
            print(f"not yet on crates.io  {name}: publish it once, then run this again")
            continue
        if not token:  # a dry run without a token cannot list configurations
            print(f"would trust           {name}")
            continue
        status, listing = call("GET", f"/trusted_publishing/github_configs?crate={name}", token)
        if status != 200:
            print(f"cannot list           {name}: {status} {listing}")
            failed = True
            continue
        have = listing.get("github_configs", [])
        if any(all(c.get(k) == v for k, v in WANT.items()) for c in have):
            print(f"trusts already        {name}")
            continue
        if dry_run:
            print(f"would trust           {name} ({len(have)} other configuration(s) kept)")
            continue
        status, created = call("POST", "/trusted_publishing/github_configs", token, {"github_config": {"crate": name, **WANT}})
        if status == 200:
            print(f"trusts now            {name}")
        else:
            print(f"refused               {name}: {status} {created}")
            failed = True
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
