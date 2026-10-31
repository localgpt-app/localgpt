# Cross-platform process isolation for local AI bridge daemons

**The "separate Unix user per bridge" model that works elegantly on Linux does not translate cleanly to macOS or Windows — but each platform offers strong alternatives that, combined with the right IPC and credential architecture, deliver equivalent security.** The most practical cross-platform approach for a personal machine is a tiered model: full multi-user isolation on Linux, platform-native sandboxing on macOS and Windows, and a common authenticated-IPC layer built on the `interprocess` Rust crate. Real-world precedent from 1Password, ssh-agent, and Chromium confirms that process-level isolation with credential mediation through IPC is the industry-standard pattern — separate OS users are a Linux-specific optimization, not a universal requirement.

---

## macOS can mirror the Linux model, but with significant friction

macOS fully supports creating low-privilege "role" accounts analogous to Linux system users. Apple itself creates **~80+ underscore-prefixed service accounts** (`_www`, `_postgres`, `_mysql`, etc.) with UIDs below 500, which are automatically hidden from the login screen. You can create your own via `dscl`:

```bash
sudo dscl . -create /Users/_bridge_cli UserShell /usr/bin/false
sudo dscl . -create /Users/_bridge_cli UniqueID 450
sudo dscl . -create /Users/_bridge_cli NFSHomeDirectory /var/empty
sudo dscl . -create /Users/_bridge_cli IsHidden 1
```

Apple's `sysadminctl -roleAccount` flag is the "official" path, but its `-UID` parameter is **broken in Ventura and later** — it silently assigns UIDs in the 500+ range. Use `dscl` for reliable UID control.

**launchd supports running daemons as specific users** through the `UserName` key in LaunchDaemon plists placed in `/Library/LaunchDaemons/`. This is directly analogous to systemd's `User=` directive — launchd (running as root) drops privileges before exec'ing the process. launchd also provides `SandboxProfile` for Seatbelt sandboxing, `SoftResourceLimits`/`HardResourceLimits`, `Umask`, and `RootDirectory` (chroot). However, it **lacks systemd's advanced hardening** like `ProtectHome`, `PrivateTmp`, `NoNewPrivileges`, `SystemCallFilter`, and `CapabilityBoundingSet`. Seatbelt profiles partially compensate.

Unix domain sockets on macOS work almost identically to Linux for access control purposes. File permissions on socket files are enforced, and directory permissions provide an additional layer. The critical differences: macOS **does not support the abstract socket namespace** (all sockets must be filesystem paths), and the path length limit is **104 bytes** versus Linux's 108. For peer identity verification, macOS provides `getpeereid()` (returning effective UID/GID) and the macOS-specific `LOCAL_PEERPID` socket option for PID — but **not** Linux's `SO_PEERCRED`. The `getpeereid()` function is the portable BSD alternative.

For credential isolation, each macOS user has their own keychain file, and daemon users can create **dedicated custom keychains** in protected directories. The recommended pattern: create `/var/bridge-cli/credentials.keychain` owned by `_bridge_cli` with `0700` directory permissions, and use the `security-framework` Rust crate (146M+ downloads) for programmatic access. Keychain item ACLs can further restrict access to specific code-signed binaries.

The macOS sandbox (`sandbox-exec` with Seatbelt profiles) provides kernel-level restriction of filesystem, network, IPC, and process capabilities. While **officially deprecated**, it remains functional and is used by Apple's own system daemons, Google's Gemini CLI, and Deno. App Sandbox and Hardened Runtime are **not required** for non-App-Store daemons, though Hardened Runtime is needed for notarization.

**The bottom line on macOS**: the multi-user model is feasible but creates installer complexity (`.pkg` with pre/post-install scripts for user creation, directory setup, keychain initialization, and plist installation). On a personal Mac, this is heavier than it needs to be.

---

## Windows Virtual Accounts solve the identity problem elegantly

Windows offers a mechanism that is, in some ways, **cleaner than Linux's approach**: Virtual Accounts. Introduced in Windows 7, these per-service accounts (`NT SERVICE\ServiceName`) are automatically created when a service is configured to use them. Each gets a **deterministic SID** derived from the service name via SHA-1, requires no password management, creates no user profile clutter, and can be used directly in ACLs. A bridge service running as `NT SERVICE\BridgeCli` automatically has a distinct security identity from any other bridge service (e.g., `NT SERVICE\BridgeCustom`).

To install services from Rust, the **`windows-service` crate** (maintained by Mullvad VPN, 2.8M+ downloads) provides the full lifecycle: service registration, event handling, and status reporting. Each service can specify its own account via `ServiceInfo.account_name`. Service installation requires **one-time admin elevation** — there is no equivalent of systemd user units that can be created without privileges.

For IPC, **named pipes are the correct choice on Windows**, not AF_UNIX sockets. While Windows 10+ supports AF_UNIX, it **lacks credential passing and peer identity verification** — it's a compatibility shim, not a security feature. Named pipes offer DACL-based access control (restrict which SIDs can connect), `GetNamedPipeClientProcessId()` for peer identification, and native integration with the Windows security model. The default DACL gives Everyone read access, so custom security descriptors are essential — use the `windows-rs` crate to build DACLs that reference specific service SIDs.

**DPAPI provides automatic per-account encryption**: data encrypted with `CryptProtectData` under `NT SERVICE\BridgeCli` cannot be decrypted by another bridge's Virtual Account because they have different SIDs and therefore different master keys. The `windows-dpapi` Rust crate wraps this cleanly. This means credential files encrypted at rest get per-bridge isolation "for free" when using Virtual Accounts.

For additional containment, **Job Objects** provide cgroup-like resource limits (CPU rate, memory cap, process count, network bandwidth, disk I/O) and can prevent child process creation. **AppContainers** add mandatory access control — Chrome and Adobe Acrobat use AppContainers for sandboxing non-UWP Win32 processes. A service can act as a broker, launching bridge workers inside AppContainers for defense-in-depth.

**The recommended Windows stack**: Virtual Accounts for identity isolation, named pipes with per-service DACLs for IPC, DPAPI for credential encryption, and Job Objects for resource limits. AppContainers are available for maximum hardening but add complexity.

---

## The Rust IPC layer should use `interprocess` with `tarpc` or `tonic`

The **`interprocess` crate** (v2.2, 505 GitHub stars, 5.6M downloads) is the clear winner for cross-platform local IPC in Rust. Its `LocalSocketListener`/`LocalSocketStream` abstraction maps to Unix domain sockets on Linux/macOS and named pipes on Windows, with full async support via a `tokio` feature flag. It handles platform differences like naming conventions and the abstract namespace transparently.

For the RPC layer on top of `interprocess`, two strong options exist:

- **`tarpc`** (maintained by Google): Schema defined in Rust code (no `.proto` files), cascading cancellation, deadline propagation. Accepts any `AsyncRead + AsyncWrite` stream, so wrapping `interprocess` is trivial. Best for an all-Rust codebase.
- **`tonic`** (gRPC, ~10k stars): Built-in Unix domain socket support via `serve_with_incoming()`, custom connectors for named pipes. Provides the full gRPC ecosystem including health checks and streaming. Best if you might have non-Rust clients.

For peer credential verification, no single cross-platform crate exists. Use `#[cfg]` gating:

- **Linux**: `SO_PEERCRED` via the `nix` crate or the unstable `peer_cred()` on `UnixStream`
- **macOS**: `getpeereid()` via `libc` or the `unix-cred` crate (abstracts across Unix variants)
- **Windows**: `GetNamedPipeClientProcessId()` via `windows-rs`

The `unix-cred` crate (v0.1.1) provides a unified `get_peer_ids(socket_fd) → (uid, gid)` API across Linux and macOS, which eliminates one `#[cfg]` boundary. Combining it with `windows-rs` on Windows requires only a two-way platform split.

For cross-platform service management, the **`service-manager` crate** provides a unified install/uninstall/start/stop API across systemd (Linux), launchd (macOS), and the Windows Service Control Manager. This avoids writing three separate service installation codepaths.

---

## 1Password and ssh-agent prove the architecture works without separate users

The most instructive real-world precedent is **1Password's architecture**: a core desktop process holds decrypted vault data and exposes it to the CLI and SSH agent via platform-native IPC — XPC on macOS, Unix domain sockets on Linux, named pipes on Windows. Each platform uses its native process identity verification: code signature verification on macOS, socket file permissions + GID checking on Linux, Authenticode signature verification on Windows. **No separate OS users are involved.**

**ssh-agent** follows the same pattern: a single long-lived process holds unencrypted private keys in memory and performs signing operations on behalf of clients over a Unix domain socket (Linux/macOS) or named pipe (Windows). The socket is protected by file permissions, and the agent never exposes raw keys.

**Mautrix bridges** — the closest architectural analog to LocalGPT's bridge model — explicitly recommend separate OS users on Linux with systemd hardening (`NoNewPrivileges`, `ProtectHome`, `PrivateDevices`). On macOS, they **do not use the multi-user model** in practice. Docker containers are the recommended isolation mechanism for non-Linux deployments.

**Chromium** demonstrates that even the highest-security multi-process architectures don't use separate OS users. Instead, it relies on platform-specific sandboxing: seccomp-BPF + namespaces on Linux, Seatbelt on macOS, restricted tokens + integrity levels + AppContainers on Windows. Its custom IPC framework (Mojo/ipcz) handles cross-process communication with shared memory for performance.

---

## LocalGPT should tier its security model by platform

Based on all the evidence, here is the recommended architecture:

**Tier 1 — Linux (strongest, full multi-user):**
Separate system users per bridge (`_bridge_cli`, etc.), systemd units with `User=` and full hardening (`NoNewPrivileges`, `ProtectHome`, `PrivateTmp`, `SystemCallFilter`), Unix domain sockets with `0660` permissions in per-user directories, `SO_PEERCRED` for identity verification, and credentials in per-user-owned files with `0600` permissions. This is the proven mautrix model.

**Tier 2 — macOS (strong, sandbox-based):**
Run bridge processes as the **current user** (not separate OS users), each in its own Seatbelt sandbox profile restricting filesystem access to its own data directory and network to specific endpoints. Use Unix domain sockets in `~/Library/Application Support/LocalGPT/sockets/` with per-bridge subdirectories. Use the macOS Keychain for credential storage with per-application ACLs (the `security-framework` crate). Verify peer identity via `getpeereid()`. Optionally support the full multi-user model for advanced users, but default to single-user sandboxed.

**Tier 3 — Windows (strong, Virtual Account or single-user):**
For the service deployment path: use **Virtual Accounts** (`NT SERVICE\BridgeCli`) with per-service SIDs, named pipes with custom DACLs, DPAPI `Scope::User` for per-bridge credential encryption, and Job Objects for resource limits. For the personal desktop path (non-service): run as the current user with encrypted per-bridge credential files (DPAPI), named pipes with process identity verification via `GetNamedPipeClientProcessId()`, and optional AppContainer sandboxing for untrusted bridges.

**Cross-platform minimum viable security model:**

All three platforms share the same core pattern — the **credential mediation architecture** proven by 1Password and ssh-agent:

1. A **core process** holds a master key (derived from user password, OS keychain, or biometric unlock)
2. **Per-bridge credential files** encrypted at rest with bridge-specific keys derived from the master key
3. Bridge processes are **separate OS processes** communicating via `interprocess`-based IPC
4. The core process **verifies bridge identity** before dispensing credentials (via `getpeereid`/`SO_PEERCRED`/`GetNamedPipeClientProcessId`)
5. Bridge processes **never persist plaintext credentials** — they receive them from the core at startup and hold them only in memory

This model works across all three platforms without any platform-specific user management, while still providing credential isolation between bridges. The separate-OS-user model on Linux and Virtual Accounts on Windows become **defense-in-depth layers on top**, not requirements.

## Conclusion

The Linux multi-user model is the gold standard for daemon isolation, but it is a **Linux-specific optimization** — not a portable architecture. The portable core of LocalGPT's security should be the credential-mediation pattern: a trusted core process that gates access to per-bridge secrets via authenticated IPC. This pattern works identically on all three platforms, is proven by 1Password and ssh-agent, and can be built cleanly in Rust using `interprocess` + `tarpc` with platform-specific peer verification behind `#[cfg]` gates. Layer OS-level isolation on top where available — systemd hardening on Linux, Seatbelt profiles on macOS, Virtual Accounts + Job Objects on Windows — but design the architecture so it is secure even without them. The `service-manager` crate can handle cross-platform service installation, and the `security-framework` (macOS) and `windows-dpapi` (Windows) crates cover platform-native credential storage. The total crate surface is manageable: `interprocess`, `tarpc`, `service-manager`, `unix-cred`, `security-framework`, `windows-dpapi`, and `windows-service`.