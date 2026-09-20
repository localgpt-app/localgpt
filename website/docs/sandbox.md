---
sidebar_position: 15
---

# Shell Sandbox

LocalGPT applies **kernel-level isolation** on every shell command the AI executes. No Docker, no containers, no external dependencies — sandboxing is built into the single binary and enabled by default. These are best-effort defenses that significantly reduce risk, not guarantees of absolute security.

## Why Sandboxing Matters

AI agents with shell access can execute arbitrary commands with your full user permissions. Without isolation, a hallucinated or prompt-injected command could delete files, exfiltrate secrets, or escalate privileges. LocalGPT mitigates this at the OS level — not with regex blocklists, but with kernel-enforced restrictions that are significantly harder to bypass than application-level controls. No sandbox is perfect, but kernel-level enforcement raises the bar substantially.

## How It Works

LocalGPT uses the **argv[0] re-exec pattern**: when the agent runs a shell command, the binary re-executes itself as a sandbox helper, applies kernel restrictions in a clean child process, then execs `bash`. The parent process (your LocalGPT session) is never restricted.

```
localgpt agent runtime
       |
       | fork + exec(self, argv[0]="localgpt-sandbox")
       |     + policy JSON as argument
       |     + "bash -c <command>" as remaining args
       v
localgpt-sandbox (clean child process)
       |
       | 1. Deserialize SandboxPolicy
       | 2. Apply resource limits (rlimits)
       | 3. Apply filesystem rules (Landlock / Seatbelt)
       | 4. Apply network deny (seccomp / Seatbelt)
       | 5. exec("bash", "-c", command)
       v
bash -c <command>  (fully sandboxed)
```

## Sandbox Modes

Users never write sandbox policies. LocalGPT auto-derives the policy from a single high-level **mode** setting and the current workspace path.

| Mode | Filesystem | Network | Use Case |
|------|-----------|---------|----------|
| `workspace-write` | R/W workspace + `/tmp`; R/O system dirs; deny credentials | Denied | **Default.** Normal agent work — editing code, running tests, building. |
| `read-only` | R/O everywhere; no writes | Denied | Exploratory analysis, code review, auditing. |
| `full-access` | Unrestricted | Unrestricted | Requires explicit opt-in via config. |

### Default: `workspace-write`

The default mode allows the agent to read and write files within your project workspace and `/tmp`, read system binaries and libraries, but blocks access to credential directories and all network activity.

**Allowed paths (read + write):**
- Your workspace directory (e.g., `~/projects/my-app`)
- `/tmp/localgpt-sandbox-*` (ephemeral scratch)

**Allowed paths (read-only):**
- `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin` — system binaries and libraries
- `/etc` — system configuration (DNS, locales)
- `/dev/null`, `/dev/urandom`, `/dev/zero` — standard devices
- `/proc/self` — process introspection

**Denied paths:**
- `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config`, `~/.docker` — credential directories

**Denied syscalls:**
- `socket`, `connect`, `bind`, `sendto`, `recvfrom`, `ptrace`, and all other network/tracing syscalls

## Platform Support

| Platform | Filesystem Isolation | Network Isolation | Status |
|----------|---------------------|-------------------|--------|
| Linux | Landlock LSM (V1–V5) | seccomp-bpf syscall deny | Full support |
| macOS | Seatbelt SBPL profiles | Seatbelt `(deny network*)` | Experimental |
| Windows | AppContainer ACLs | Restricted tokens | Planned |

### Linux

Uses two complementary kernel mechanisms:

- **Landlock** — A Linux Security Module (kernel 5.13+) that enforces deny-by-default filesystem rules. Only explicitly allowed paths are accessible.
- **seccomp-bpf** — A BPF filter that returns `EPERM` for all network-related syscalls (`socket`, `connect`, `bind`, etc.).

Order of operations: Landlock first (filesystem), then seccomp (network). Seccomp must be last because it blocks syscalls that Landlock setup requires.

### macOS

Uses Apple's **Seatbelt** framework via `sandbox-exec`. LocalGPT generates SBPL (Sandbox Profile Language) profiles dynamically from the sandbox policy. Despite being officially deprecated since macOS 10.12, Seatbelt remains functional through macOS 15+ and is used by Bazel, OpenAI Codex, and Google Gemini CLI.

## Graceful Degradation

Not all kernels support all features. LocalGPT detects available capabilities at startup and operates at the highest available level. When full kernel support isn't available, LocalGPT **always warns you** and explains what protections are active.

| Level | Requirements | Protections | User Experience |
|-------|-------------|-------------|-----------------|
| Full | Landlock V4+ + seccomp + userns | Filesystem + network + PID + mount | Silent — no prompt |
| Standard | Landlock V1+ + seccomp | Filesystem + network | Silent — no prompt |
| Minimal | seccomp only | Network blocking only | Warning banner |
| None | No kernel support | rlimits + timeout only | Explicit acknowledgment required |

Unlike Codex (which panics on missing Landlock), LocalGPT warns and degrades. Unlike OpenClaw (which defaults to no sandbox), LocalGPT defaults to the highest available level.

## Claude CLI Backend

:::note
If using the Claude CLI as your LLM backend (`agent.default_model = "claude-cli/*"`), the sandbox **does not apply** to Claude CLI subprocess calls — only to tool-executed shell commands. The Claude CLI subprocess itself runs outside the sandbox with access to your system.
:::

## Limitations

Sandboxing significantly reduces the attack surface, but it is not a silver bullet:

- **LLMs are probabilistic** — an AI agent may find unexpected ways to accomplish tasks that weren't anticipated by sandbox rules. Prompt injection and jailbreaks are active areas of research with no complete solutions yet.
- **Kernel features vary** — older kernels may lack Landlock or seccomp support, reducing the effective protection level. LocalGPT warns you, but degraded mode provides weaker isolation.
- **Allowed paths are still writable** — the sandbox restricts *where* the agent can write, but within the workspace it has full access. A compromised agent could still modify your project files.
- **No sandbox is escape-proof** — kernel vulnerabilities, while rare, can exist. Defense in depth (sandboxing + protected files + human review) is the strategy, not reliance on any single layer.

## Resource Limits

Every sandboxed command has resource limits enforced via rlimits and process management:

| Resource | Default | Config Key |
|----------|---------|------------|
| Execution timeout | 120 seconds | `sandbox.timeout_secs` |
| Max output | 1 MB (stdout + stderr) | `sandbox.max_output_bytes` |
| Max file size | 50 MB (`RLIMIT_FSIZE`) | `sandbox.max_file_size_bytes` |
| Max processes | 64 (`RLIMIT_NPROC`) | — |

## Configuration

The sandbox works with zero configuration. These settings are escape hatches for power users:

```toml
[sandbox]
enabled = true              # default: true
level = "auto"              # auto | full | standard | minimal | none
timeout_secs = 120          # kill command after N seconds
max_output_bytes = 1048576  # 1MB output limit
max_file_size_bytes = 52428800  # 50MB file size limit

[sandbox.allow_paths]
read = ["/data/datasets"]   # additional read-only paths
write = ["/tmp/builds"]     # additional writable paths

[sandbox.network]
policy = "deny"             # deny | proxy (future)
```

## CLI Commands

### `localgpt sandbox status`

Inspect the sandbox capabilities detected on your system:

```
$ localgpt sandbox status

Sandbox Capabilities:
  Landlock:  v5 (kernel 6.10+)     ✓
  Seccomp:   available              ✓
  Userns:    available              ✓
  Level:     Full
```

### `localgpt sandbox test`

Run smoke tests to verify enforcement:

```
$ localgpt sandbox test

Running sandbox smoke tests...
  ✓ Write to workspace:     allowed
  ✓ Write outside workspace: denied (EACCES)
  ✓ Read ~/.ssh/id_rsa:     denied (EACCES)
  ✓ Network (curl):         denied (EPERM)
  ✓ Timeout enforcement:    killed after 5s
  ✓ Child process inherits: confirmed
All 6 tests passed.
```

## Tool Integration

The sandbox integrates into LocalGPT's tool execution pipeline. All entry points — CLI, HTTP API, desktop GUI, heartbeat — route through the same sandboxed execution path.

| Tool | Sandboxed? | Details |
|------|-----------|---------|
| `bash` | Yes — always | Arbitrary command execution, full sandbox |
| `write_file` | Yes — path-validated | Writes restricted to workspace |
| `read_file` | Yes — path-validated | Reads restricted, credentials blocked |
| `edit_file` | Yes — path-validated | Same restrictions as `write_file` |
| `web_fetch` | No | Separate SSRF protection layer |
| `memory_search` | No | Internal SQLite query, no shell |
