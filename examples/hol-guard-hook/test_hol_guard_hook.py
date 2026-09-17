import importlib.util
import json
import pathlib
import subprocess
import unittest
from unittest.mock import patch


MODULE_PATH = pathlib.Path(__file__).with_name("hol_guard_hook.py")
SPEC = importlib.util.spec_from_file_location("hol_guard_hook", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(MODULE)


def event(tool_name="bash", arguments=None):
    return {
        "event": "BeforeToolCall",
        "data": {
            "tool_name": tool_name,
            "arguments": arguments if arguments is not None else {"command": "ls"},
            "session_id": "test-session",
        },
    }


class HolGuardHookTests(unittest.TestCase):
    def test_unmapped_tool_is_unchanged(self):
        self.assertEqual(MODULE.evaluate_event(event("read_file", {"path": "README.md"})), 0)

    def test_mapped_tool_missing_command_blocks(self):
        self.assertEqual(MODULE.evaluate_event(event(arguments={})), 2)

    @patch.object(MODULE.subprocess, "run")
    def test_explicit_benign_allow_proceeds(self, run):
        run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps(
                {
                    "classification": {"explicitly_benign": True},
                    "minimum_action": "allow",
                }
            ),
            stderr="",
        )
        self.assertEqual(MODULE.evaluate_event(event()), 0)

    @patch.object(MODULE.subprocess, "run")
    def test_review_blocks(self, run):
        run.return_value = subprocess.CompletedProcess(
            args=[],
            returncode=0,
            stdout=json.dumps(
                {
                    "classification": {"explicitly_benign": False},
                    "minimum_action": "review",
                }
            ),
            stderr="",
        )
        self.assertEqual(MODULE.evaluate_event(event()), 2)

    @patch.object(MODULE.subprocess, "run")
    def test_hol_guard_error_blocks(self, run):
        run.return_value = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="guard error"
        )
        self.assertEqual(MODULE.evaluate_event(event()), 2)

    @patch.object(MODULE.subprocess, "run", side_effect=FileNotFoundError("hol-guard"))
    def test_missing_hol_guard_executable_blocks(self, _run):
        self.assertEqual(MODULE.evaluate_event(event()), 2)

    @patch.object(MODULE.subprocess, "run")
    def test_malformed_output_blocks(self, run):
        run.return_value = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="not-json", stderr=""
        )
        self.assertEqual(MODULE.evaluate_event(event()), 2)

    @patch.object(MODULE.subprocess, "run", side_effect=subprocess.TimeoutExpired("hol-guard", 8))
    def test_timeout_blocks(self, _run):
        self.assertEqual(MODULE.evaluate_event(event()), 2)


if __name__ == "__main__":
    unittest.main()
