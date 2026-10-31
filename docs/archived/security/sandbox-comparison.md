# AI Agent Sandbox Comparison

**macOS Seatbelt (SBPL) Policies Across AI Coding Agents**

Last updated: February 2026

---

## Summary

| Tool | Read Policy | Home Dir Reads | Write Policy | Credential Protection | Network Default |
|------|-------------|----------------|-------------|----------------------|----------------|
| **Codex CLI** | `(allow file-read*)` | Full | Workspace only (carves out `.git/`, `.codex/`) | None | Denied |
| **Claude Code** | `(allow file-read*)` + deny list | Full (except denied dirs) | CWD only | Mandatory deny list | Denied |
| **Gemini CLI** | `(allow file-read*)` | Full | TARGET_DIR + tmp + cache + `~/.gemini` + `~/.npm` | None | Open by default |
| **Cursor** | `(allow file-read*)` (inferred) | Full (including `~/.ssh`) | CWD only | None (known vuln) | Restricted |
| **LocalGPT** | `(allow file-read*)` + home deny | Denied (except workspace) | Workspace + `/tmp` only | Home deny + credential deny list | Denied |

**Key finding:** Every major AI coding agent uses `(allow file-read*)` as their default macOS sandbox read posture. None restrict reads to workspace + system directories. LocalGPT is currently the strictest by denying the entire home directory and re-allowing only the workspace.

---

## 1. OpenAI Codex CLI

**Source:** Open source Rust implementation at [github.com/openai/codex](https://github.com/openai/codex/tree/main/codex-rs/core/src)

### Architecture

The SBPL profile is assembled dynamically in `seatbelt.rs` by concatenating a static base policy (`seatbelt_base_policy.sbpl`) with dynamically generated file-read, file-write, and network policies.

### Base Policy

Starts with `(deny default)`, allows process-exec, process-fork, signal, user-preference-read, process-info. Allows writing to `/dev/null`. Enumerates specific sysctl reads (hw.activecpu, hw.memsize, kern.osversion, etc.) and specific mach-lookup services.

### File Read Policy

Generated conditionally:

```rust
let file_read_policy = if sandbox_policy.has_full_disk_read_access() {
    "(allow file-read*)"
} else {
    ""  // no read access at all
};
```

In the default `workspace-write` mode, **full disk read access is granted**. The agent can read `~/.ssh`, `~/.aws`, and everything else. There is no selective-read mode — it's either all reads or no reads.

### File Write Policy

Uses parameterized `writable_roots` with `(require-not ...)` carve-outs:

```scheme
(allow file-write*
  (require-all
    (subpath (param "WRITABLE_ROOT_0"))
    (require-not (subpath (param "WRITABLE_ROOT_0_RO_0")))  ;; .git
    (require-not (subpath (param "WRITABLE_ROOT_0_RO_1")))  ;; .codex
  )
)
```

This uniquely prevents the agent from modifying git hooks or its own configuration within writable directories.

### Sandbox Modes

- `read-only`: Full disk reads, no writes, no network
- `workspace-write` (default): Full disk reads, writes to workspace (minus .git/.codex), no network
- `danger-full-access`: No sandbox constraints

### Sources

- [seatbelt_base_policy.sbpl](https://github.com/openai/codex/blob/main/codex-rs/core/src/seatbelt_base_policy.sbpl)
- [seatbelt.rs](https://github.com/openai/codex/blob/main/codex-rs/core/src/seatbelt.rs)
- [seatbelt_network_policy.sbpl](https://github.com/openai/codex/blob/main/codex-rs/core/src/seatbelt_network_policy.sbpl)
- [Codex Security Documentation](https://developers.openai.com/codex/security/)

---

## 2. Claude Code (Anthropic)

**Source:** Open source npm package at [github.com/anthropic-experimental/sandbox-runtime](https://github.com/anthropic-experimental/sandbox-runtime)

### Architecture

The profile is generated dynamically in `src/sandbox/macos-sandbox-utils.ts` via `generateSandboxProfile()`. Builds SBPL as a string with configurable deny/allow paths.

### File Read Policy

**Default: `(allow file-read*)` with configurable deny list.**

Denials are specified via `denyRead` array using either:
- Glob patterns converted to `(deny file-read* (regex "pattern"))`
- Literal paths using `(deny file-read* (subpath "path"))`

With an empty deny list, the agent has full read access to the entire filesystem.

### File Write Policy

Default: all writes denied. Writes must be explicitly allowed via `allowWrite` paths (typically CWD + subdirectories). TMPDIR parent is automatically detected and allowed.

**Mandatory denied paths** (always write-protected regardless of configuration):
- `.bashrc`, `.zshrc`, shell configuration files
- `.git/hooks/` directories
- `.ssh` directory
- `.aws` directory
- `.kube` directory
- `.env` files
- `secrets.json` and similar credential files
- Git config (unless `allowGitConfig` is true)

Claude Code is the only tool with a hardcoded mandatory deny list for sensitive write targets.

### Network Policy

When restricted, all network is denied except connections through a localhost proxy (HTTP and SOCKS ports). Domain filtering happens at the proxy layer, not in the SBPL profile.

### Sources

- [sandbox-runtime — macos-sandbox-utils.ts](https://github.com/anthropic-experimental/sandbox-runtime/blob/main/src/sandbox/macos-sandbox-utils.ts)
- [Claude Code Sandboxing Documentation](https://code.claude.com/docs/en/sandboxing)
- [Anthropic Engineering — Claude Code Sandboxing](https://www.anthropic.com/engineering/claude-code-sandboxing)
- [DeepWiki — sandbox-runtime macOS Sandboxing](https://deepwiki.com/anthropic-experimental/sandbox-runtime/6.2-macos-sandboxing)

---

## 3. Google Gemini CLI

**Source:** Open source TypeScript implementation at [github.com/google-gemini/gemini-cli](https://github.com/google-gemini/gemini-cli)

### Architecture

Uses six static `.sb` profile files covering a matrix of `{permissive, restrictive}` x `{open, closed, proxied}` network modes. Paths are injected via `-D` parameters.

### Permissive Mode (Default: `permissive-open`)

```scheme
(version 1)
(allow default)           ;; EVERYTHING allowed by default

(deny file-write*)
(allow file-write*
    (subpath (param "TARGET_DIR"))
    (subpath (param "TMP_DIR"))
    (subpath (param "CACHE_DIR"))
    (subpath (string-append (param "HOME_DIR") "/.gemini"))
    (subpath (string-append (param "HOME_DIR") "/.npm"))
    (subpath (string-append (param "HOME_DIR") "/.cache"))
    ...
)
```

Starts with `(allow default)` — the broadest possible posture. Only restricts writes.

### Restrictive Mode

```scheme
(version 1)
(deny default)
(allow file-read*)        ;; still broad reads even in "restrictive" mode
(allow process-exec)
(allow process-fork)
...
```

Even the "restrictive" mode allows `(allow file-read*)`. The restrictive mode only means that non-file operations (mach-lookup, iokit) are denied by default.

### Write Policy (Same Across All Modes)

Writes restricted to: TARGET_DIR, TMP_DIR, CACHE_DIR, `~/.gemini`, `~/.npm`, `~/.cache`, `~/.gitconfig`, plus up to 5 user-specified include directories.

No mandatory deny list for `.ssh`, `.aws`, `.git/hooks`, or other sensitive paths.

### Network Variations

- `*-open`: Unrestricted outbound (default)
- `*-closed`: No outbound
- `*-proxied`: Localhost proxy only

### Sources

- [sandbox-macos-permissive-open.sb](https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/utils/sandbox-macos-permissive-open.sb)
- [sandbox-macos-restrictive-open.sb](https://github.com/google-gemini/gemini-cli/blob/main/packages/cli/src/utils/sandbox-macos-restrictive-open.sb)
- [Gemini CLI Sandboxing Documentation](https://google-gemini.github.io/gemini-cli/docs/cli/sandbox.html)

---

## 4. Cursor

**Source:** Closed source. Information from security research and documentation.

### What We Know

- Cursor 2.0 (late 2025) replaced their previous command allowlist with macOS Seatbelt sandboxing
- **Read policy**: Full filesystem read access (`(allow file-read*)` inferred)
- **Write policy**: Writes restricted to CWD and presumably temp dirs
- **Network**: Restricted
- **No credential protection**: `~/.ssh/id_rsa` is fully readable

### Known Vulnerability

Luca Becker's security analysis (November 2025) identified that while `.cursorignore` prevents Cursor's `file_read` tool from accessing files, the agent trivially bypasses this via `cat` commands through the sandboxed terminal, since the Seatbelt profile grants `(allow file-read*)`.

### Sources

- [Luca Becker — Cursor Sandboxing Leaks Your Secrets](https://luca-becker.me/blog/cursor-sandboxing-leaks-secrets/)
- [Pierce Freeman — A Deep Dive on Agent Sandboxes](https://pierce.dev/notes/a-deep-dive-on-agent-sandboxes)

---

## 5. LocalGPT (This Project)

### Current Approach

LocalGPT takes a stricter approach than all competitors:

```scheme
(version 1)
(deny default)
(allow process*) (allow signal) (allow mach*) (allow ipc*)
(allow sysctl*) (allow pseudo-tty)
(allow file-read*)                                         ;; broad reads for bash compatibility
(allow file-write* (subpath "/dev"))
(deny file-read* file-write* (subpath "/Users/yi"))        ;; deny entire home directory
(allow file-read* file-write* (subpath "~/.localgpt/workspace"))  ;; re-allow workspace
(allow file-read* file-write* (subpath "/tmp"))             ;; re-allow tmp
(deny file-read* file-write* (subpath "~/.ssh"))            ;; credential deny (belt-and-suspenders)
(deny file-read* file-write* (subpath "~/.aws"))
(deny file-read* file-write* (subpath "~/.gnupg"))
...
(deny network*)
```

### How This Differs

| Dimension | Industry Standard | LocalGPT |
|-----------|------------------|----------|
| Home dir reads | Fully readable | Denied (except workspace) |
| `~/repos` readable | Yes | No |
| `~/Documents` readable | Yes | No |
| `~/.ssh` readable | Yes (except Claude Code) | No |
| Non-home paths readable | Yes | Yes |
| Write scope | Workspace/CWD only | Workspace + /tmp only |
| Network | Denied | Denied |
| Credential write deny | Only Claude Code | Yes (via home deny) |

### Trade-offs

**Advantages of LocalGPT's stricter approach:**
- Agent cannot exfiltrate data from user's repos, documents, downloads
- Agent cannot read SSH keys, AWS credentials, GPG keys
- Matches the spec's intent of "read access to system binaries and workspace only"

**Disadvantages:**
- Agent cannot read files outside workspace that legitimate commands may need
- Users who want the agent to work on projects outside workspace need to configure `allow_paths.read`
- Non-home paths (e.g., `/var/log/`) remain readable (same as competitors)

---

## Analysis: Why Everyone Uses Broad Reads

The `(allow file-read*)` approach is universal because:

1. **macOS bash complexity**: bash and its dependencies need access to dyld cache, system frameworks, `/private/etc` (symlinked from `/etc`), and dozens of other paths that are impractical to enumerate
2. **Developer toolchains**: Commands like `git`, `node`, `python`, `cargo` read from many locations (package caches, global configs, etc.)
3. **Seatbelt limitations**: SBPL path matching doesn't handle macOS symlinks well (`/etc` -> `/private/etc`, `/tmp` -> `/private/tmp`), making allowlists fragile
4. **Write restriction is the bigger lever**: Most damage from rogue commands comes from writes (deleting files, overwriting configs) and network access (exfiltration), not reads

The community [neko-kai/claude-code-sandbox](https://github.com/neko-kai/claude-code-sandbox) project is the only known attempt at restrictive reads, denying reads except for the working directory, git config, and system directories.

---

## Competitive Comparison

| Product | Sandbox Approach | Default | Dependency | Limitation |
|---------|-----------------|---------|------------|------------|
| **LocalGPT** | Landlock + seccomp + Seatbelt | On | None (single binary) | Degrades gracefully |
| Claude Code | Bubblewrap + seccomp + Seatbelt | On | bwrap binary | External binary dependency |
| Codex CLI | Landlock + seccomp + Seatbelt | On | None (Rust) | Panics if Landlock missing |
| OpenClaw | Docker containers | Off | Docker daemon + images | Heavy setup, off by default |
| Cursor | Seatbelt (macOS only) | On | None | macOS only |
| Aider | None | N/A | N/A | No sandboxing |

---

## Recommendations

1. **Current approach is defensible**: LocalGPT's home-dir deny is stricter than all competitors. This is a product differentiator for security-conscious users.
2. **Consider making read scope configurable**: Allow users to choose between "strict" (home deny, current default) and "standard" (broad reads, industry standard) via `sandbox.level`.
3. **Add mandatory write deny list**: Follow Claude Code's pattern of always denying writes to `.ssh`, `.aws`, `.git/hooks`, shell configs, regardless of configured write paths.
4. **Monitor for breakage**: The stricter read policy may break legitimate workflows. Collect user feedback and adjust the default deny paths as needed.
