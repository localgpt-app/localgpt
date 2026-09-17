#!/usr/bin/env python3
"""Fail-closed LocalGPT before_tool_call adapter for HOL Guard."""

import json
import subprocess
import sys


def evaluate_event(event: object) -> int:
    if not isinstance(event, dict):
        return 2
    if event.get("event") != "BeforeToolCall":
        return 0

    data = event.get("data")
    if not isinstance(data, dict):
        return 2
    if data.get("tool_name") != "bash":
        return 0

    arguments = data.get("arguments")
    if not isinstance(arguments, dict):
        return 2
    command = arguments.get("command")
    if not isinstance(command, str) or not command.strip():
        return 2

    try:
        result = subprocess.run(
            ["hol-guard", "command", "test", command, "--json"],
            capture_output=True,
            text=True,
            timeout=8,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return 2

    if result.returncode != 0:
        return 2

    try:
        report = json.loads(result.stdout)
    except (TypeError, json.JSONDecodeError):
        return 2

    classification = report.get("classification")
    explicitly_benign = (
        isinstance(classification, dict)
        and classification.get("explicitly_benign") is True
    )
    return 0 if explicitly_benign and report.get("minimum_action") == "allow" else 2


def main() -> int:
    try:
        event = json.load(sys.stdin)
    except (TypeError, json.JSONDecodeError):
        return 2
    return evaluate_event(event)


if __name__ == "__main__":
    raise SystemExit(main())
