# HOL Guard before-tool hook

LocalGPT can run modifying `before_tool_call` hooks before a tool executes. This example uses that boundary to inspect the built-in `bash` tool with HOL Guard before the command can run.

Install HOL Guard so the `hol-guard` command is available, then copy the example into a LocalGPT workspace:

```bash
pip install --pre hol-guard
mkdir -p hooks
cp examples/hol-guard-hook/hol_guard_hook.py hooks/
cp examples/hol-guard-hook/hol-guard.hook.json hooks/
```

The adapter reads LocalGPT's `BeforeToolCall` JSON from stdin and runs:

```bash
hol-guard command test '<command>' --json
```

It exits successfully only when `classification.explicitly_benign` is `true` and `minimum_action` is `allow`. Review, risky or unknown results, malformed output, a missing command, a missing HOL Guard executable, a non-zero HOL Guard exit, and timeout all return a non-zero exit code. LocalGPT therefore blocks the modifying hook before `bash` executes.

Tools other than `bash` are left unchanged by this example. This is command inspection through HOL Guard's `command test` surface, not a claim of full harness-wide HOL Guard policy enforcement.

Run the focused adapter tests with:

```bash
python3 -m unittest examples/hol-guard-hook/test_hol_guard_hook.py
```
