# Orca: Architecture Analysis and Rust Feasibility Assessment

**Can we build Orca-class agent orchestration on LocalGPT's existing Rust foundation?**

| Field | Value |
|-------|-------|
| Version | 1.0 |
| Date | September 20, 2026 |
| Author | Yi / LocalGPT |
| Status | Research — no commitment |
| Subject | `stablyai/orca` @ `b5b727bd` (2026-09-19), MIT |
| Related | [rust-ecosystem-integration-spec.md](./rust-ecosystem-integration-spec.md), [../architecture/bridge-development.md](../architecture/bridge-development.md) |

---

## 1. Executive Summary

Orca is an Electron desktop app that orchestrates coding-agent CLIs — Claude Code, Codex,
Cursor, Copilot, Grok, OpenCode, Droid, Pi and ~25 others — each running in its own git
worktree, with an embedded terminal, editor, browser, git/PR review, and a phone companion.
It does not reimplement agents; it hosts whatever runs in a terminal and derives agent state
by parsing each CLI's PTY transcript and hook output.

Two findings matter for LocalGPT:

1. **Orca's own architecture already pushes the heavy lifting out of Electron.** The runtime
   (`orcad`) is plain Node, the terminal daemon is a separate detached process, and the SSH
   relay is a standalone esbuild bundle. Electron is only the UI shell. That seam is exactly
   where a Rust implementation would substitute.

2. **The backend half is genuinely open and the identity half is not.** The entire mobile
   relay service, push gateway, pairing protocol and E2EE implementation ship in-repo under
   MIT, including real production Terraform. Auth (`login.onorca.dev`), artifact/skill sharing
   (`share.onorca.dev`) and feedback intake exist only as clients.

On feasibility: **yes for the orchestration core, no for a 1:1 port.** Everything that makes
Orca an *orchestrator* — daemon, PTY ownership, RPC, relay, pairing, E2EE, git worktrees, SSH,
process supervision — is work Rust does better than Node, and LocalGPT already has four of the
eight foundations in its workspace today. Everything that makes it an *IDE* — a Monaco-class
editor, TipTap, mermaid, PDF preview, a shadcn design system — is where a pure-Rust rewrite
loses badly and should not be attempted. Section 6 recommends a hybrid split along that line.

---

## 2. Scale and Shape

| Metric | Value |
|--------|-------|
| Repo-wide source | ~4.5M lines |
| `src/` TypeScript | ~3.83M lines (`main` 1.72M, `renderer` 1.70M, `shared` 268K, `relay` 76K, `cli` 50K, `preload` 13K) |
| Test files under `src/` | 8,910 colocated `*.test.ts(x)` |
| Commits / last commit | 11,398 / 2026-09-19 |
| CI workflows | 66 (25 of them `cloud-*`) |
| Subsystem dirs in `src/main` | ~135 |
| RPC method modules | 134 |
| Locales | 7 (en, zh, ja, ko, es, fr, + pt via plugin) |

For calibration: LocalGPT's entire Rust workspace is roughly two orders of magnitude smaller.
A 1:1 port is not a project; it is a company.

---

## 3. Tech Stack Inventory

### 3.1 Core

| Layer | Tech |
|-------|------|
| Language / PM | TypeScript 7 (`typescript-api` pinned to 6.0.3), Node 24, pnpm 12 |
| Desktop shell | Electron 43, electron-vite 5, **rolldown-vite 7.3** (aliased over `vite`), electron-builder 26 → dmg/zip, NSIS+Squirrel, AppImage/deb/rpm |
| Renderer | React 19, **Zustand 5**, Tailwind v4, shadcn/ui (`new-york-v4`) + radix-ui, lucide, cmdk, sonner, dnd-kit, tanstack/react-virtual |
| Editors / viewers | Monaco, TipTap 3 (ProseMirror), react-markdown + remark/rehype (gfm, math/KaTeX, frontmatter, cjk-friendly), mermaid 11, pdfjs 6, textmate/oniguruma, dompurify |
| Terminal | **xterm.js v6-beta with 7 addons, all locally patched** (WebGL, ligatures, image, search, serialize, unicode11, web-links); `@xterm/headless` in main; **node-pty 1.1 (patched)** |
| Host / OS | ssh2, @parcel/watcher, @vscode/windows-process-tree, a `@orca/windows-registry` native workspace package, WSL support |
| Agents | `@anthropic-ai/claude-agent-sdk` 0.3.251 — with **all bundled CLI binaries excluded from install** so it always launches the user's own resolved CLI |
| Other runtime | zod 4, i18next, **sherpa-onnx** on-device ASR + OpenAI transcription, `agent-browser` (external Chromium + CDP), tweetnacl X25519 pairing, posthog-node |

### 3.2 Process Architecture

Not a two-process Electron app. It is **main + preload + renderer, plus four detached
processes**:

| Process | Role |
|---------|------|
| **Terminal daemon** | Owns every local PTY. Forked by `orcad` and *deliberately outliving it* so app updates and restarts do not kill running agents. |
| **`orcad`** | The runtime served from plain Node (`orca serve`). Binds `127.0.0.1` by default; owns RPC, git, worktrees, persistence. |
| **Relay** (`src/relay`) | esbuild-bundled CommonJS daemon pushed to remote hosts over SSH for SSH worktrees. |
| **ai-vault service / parcel-watcher / managed hook runtime** | Crash-isolated children. |

Everything communicates over one **WebSocket RPC with 134 method modules**
(`src/main/runtime/rpc/methods/`), a generated params catalog, and explicit mixed-version wire
compatibility rules (new optional field = safe; new stream opcode = must be capability
negotiated, because decoders drop unknown opcodes silently).

**Three clients share that RPC**: the Electron renderer, a web build of the same renderer
(`out/web`), and the mobile app.

Local / WSL / SSH are unified behind an **execution host** abstraction, and loss of contact is
never treated as process death — the verdict vocabulary is `live` / `unverifiable` / `exited`,
with no synonyms permitted.

### 3.3 The Other Three Codebases In-Repo

- **`cloud/`** — separate pnpm workspace. Hono 4 on Node, Postgres (`pg`) in prod / SQLite in
  dev, jose, tweetnacl. Director/cell topology on GCP Cloud Run + GCE, Terraform IaC, plus an
  APNs+FCM push gateway. Phones and desktops never connect directly.
- **`mobile/`** — Expo 55 / React Native 0.83, expo-router, Reanimated 4 + worklets, zustand,
  tweetnacl. xterm runs inside a `react-native-webview` engine bundled at postinstall.
- **`native/`** — Swift (SwiftPM) for macOS computer-use / keyboard-layout / notification-status,
  PowerShell for Windows computer-use, Python for Linux, node-gyp for the Windows registry and
  process-tree addons.
- **`docs/site/`** — Next.js 16 + fumadocs (MDX) on Vercel. Serves `/docs` only: two routes,
  docs search and OG images.

---

## 4. The Open / Closed Boundary

### 4.1 Fully Open (MIT, in-repo)

The **entire mobile relay service**, not just a client SDK:

- `cloud/apps/relay` — 52 source files. One image runs as **director** (assigns hosts to cells,
  coordinates migrations) or **cell** (carries user connections) per `ORCA_RELAY_ROLE`.
  Splice forwarder, admission selector, assignment store, regional rehome worker, Postgres
  pooling, close-code semantics.
- `cloud/packages/relay-contract` — frame shapes, close codes, admission budgets, splice state
  machine, host-proof transcript.
- `cloud/apps/push` (APNs + FCM) + `push-contract`; `relay-fence-broker`; `relay-ops`.
- Dockerfiles for all three services and 25 `cloud-*.yml` workflows that deploy them.
- `cloud/infra/terraform` — the **real production Terraform**, not a redacted template: project
  `onorca-cloud`, `us-central1`, real Cloud Run/GCE service names, real DNS.

**Pairing is complete on every side**, and genuinely end-to-end encrypted — the relay splices
ciphertext and cannot read it:

- Desktop: `src/shared/pairing.ts` (the `orca://pair?code=` base64url offer),
  `src/main/runtime/relay/`, `src/main/runtime/push/`, `src/main/runtime/e2ee-keypair.ts`
- Mobile: `mobile/app/pair{,-scan,-confirm}.tsx`,
  `mobile/src/transport/mobile-e2ee-v2-{key-schedule,framing,client-session,physical-channel}.ts`
- Web: `src/renderer/src/web/{web-pairing,web-e2ee}.ts`
- Both v2 and legacy framings, shared golden fixtures, and a `simulated-mobile-e2ee-v2-peer.ts`
  so the desktop can test against a fake phone.

You could stand up your own relay and push fleet from this repo alone.

### 4.2 Absent — Client Only, No Server, No Other Public Repo

| Service | Client in repo | Server |
|---------|----------------|--------|
| `login.onorca.dev` | `src/main/orca-profiles/profile-cloud-auth-config.ts` | absent |
| `share.onorca.dev` | `src/main/artifacts/artifact-cloud-config.ts`, `src/shared/skill-share-link.ts` | absent |
| `www.onorca.dev/v1/feedback` | `src/main/ipc/feedback.ts` | absent |

`login.onorca.dev` is the substantial gap: first-party auth (authorize / session / refresh /
capabilities / profile / org / logout) that also **mints the relay token**. A comment in
`production.tfvars` explains it — `auth.onorca.dev` is PropelAuth's prod domain, so their own
service sits at `login.onorca.dev` in front of it.

**Net: the data path is open, the identity and sharing planes are not.** Self-hosting the relay
works; authenticating against their cloud or sharing artifacts/skills does not.

### 4.3 Closed npm Dependencies

Four public npm tarballs with no source repo linked from the project: `agent-browser@0.27`
(the Chromium automation behind Design Mode), `serve-sim@0.1.40` (simulator streaming for the
emulator pane), `@stablyai/playwright-test`, `react-grab`.

---

## 5. Practices Worth Stealing Regardless of Language

These are free lessons; none require adopting any of their stack.

1. **Socket-ownership protocol** (`src/main/daemon/AGENTS.md`). `net.Server.close()` unlinks its
   pathname with no ownership check, so a departing daemon deletes a live replacement's socket.
   Their protocol: bind a private `.p<hex>` name → attempt an exclusive `link` → on `EEXIST`
   prove the incumbent dead by connecting → re-check the entry → probe once more → `rename` in
   one syscall → verify. Rules: never collapse "can't tell" into "dead" (a timeout or `EPERM`
   proves nothing); `link` first, never an unconditional `rename`; `rename`, never
   `unlink`-then-`link` (measured: gapped on essentially every observation vs. none in ~14,500
   probes); never identify an entry by `birthtimeMs`; never add a sweeper.
   **Directly applicable to `crates/bridge`.**

2. **Detached daemon survives the app.** PTYs outlive app restarts and auto-updates because the
   daemon is not the app's child in lifetime terms. Their doc is candid that process detachment
   is *not* service isolation — under systemd, `KillMode=mixed` and `control-group` both kill
   the cgroup anyway.

3. **Ratchet baselines** checked into `config/`: `max-lines-baseline.txt`,
   `ts-nocheck-baseline.txt`, `runtime-electron-baseline.txt`. The count may only go down. This
   is a cheap, high-yield pattern for `cargo clippy` allowances.

4. **Invariant docs as gates.** 50 files in `docs/reference/` written as "this already cost us
   N defects" — Windows EDR posture, MSYS job breakaway, WSL argv expansion, glibc floor, git
   2.25 baseline, SSH execution boundary. AGENTS.md names the file to read *before* touching
   each area.

5. **Transcript-driven terminal rules.** Any rule that reads what an agent CLI paints on a
   terminal must be written against a captured transcript, never a remembered screen. They
   ship a capture tool that preserves escapes and wrapping and scrubs account identifiers.

6. **Supply-chain gate in the package manager.** `minimumReleaseAge: 4320` (3 days) with an
   explicit per-package exclusion list. `cargo-deny` can approximate this.

---

## 6. Rust Feasibility

### 6.1 What LocalGPT Already Has

Verified present in the workspace today:

| Capability | Where | Crate / version |
|------------|-------|-----------------|
| HTTP + WebSocket RPC server | `crates/server` | axum 0.8.9 |
| Secure local IPC + peer identity | `crates/bridge` | interprocess 2.4.2, tarpc 0.37, tokio-serde |
| Kernel-enforced shell isolation | `crates/sandbox` | landlock 0.4, seccompiler 0.5, nix 0.31 (Seatbelt on macOS) |
| **Browser control via CDP** | `crates/cli-tools/browser.rs` | launches Chrome with `--remote-debugging-port`, speaks CDP over tokio-tungstenite 0.29 — **no external CDP crate** |
| File watching | workspace | notify 8.2.0 |
| Bridge credential encryption | `crates/server/security/bridge.rs` | chacha20poly1305 0.10.1 (per-bridge keys derived from device key) |
| Desktop GUI | `crates/cli` (feature `desktop`) | eframe/egui 0.34.3 |
| Terminal UI | `crates/cli` | ratatui 0.30, crossterm 0.29 |
| Mobile bindings | `crates/mobile-ffi` | UniFFI → Swift/Kotlin |
| Agent subprocess supervision | `crates/core/agent/providers.rs` | claude-cli / gemini-cli / codex-cli |

Two of these are strategically notable. **The CDP browser tool already uses Orca's own
approach** — an external Chromium driven over the DevTools Protocol, which is precisely how
`src/main/orcad/external-chromium-browser-process.ts` works. Design Mode is therefore not
blocked by the lack of an embedded Chromium. And **`crates/sandbox` has no counterpart in
Orca at all** — Landlock/seccomp/Seatbelt isolation of agent shells is a capability LocalGPT
leads on.

### 6.2 Tier A — Rust Is the Better Language

Everything here is systems work that Node does adequately and Rust does better, with a single
static binary instead of a bundled runtime.

| Orca feature | Rust path | Notes |
|--------------|-----------|-------|
| PTY spawn / resize / ConPTY | `portable-pty` (from WezTerm) | Replaces node-pty, including Windows ConPTY. No patches needed; Orca patches node-pty twice. |
| Terminal daemon + socket ownership | `crates/bridge` + §5.1 protocol | interprocess already gives Unix sockets and named pipes. |
| WebSocket RPC + 3 clients | `crates/server` (axum) | Already serving HTTP/WS and an OpenAI-compatible surface. |
| Relay service (director/cell splice) | axum + tokio-tungstenite + `sqlx`/`tokio-postgres` | A frame-splicing proxy holding many mostly-idle connections is the canonical Rust strength — far lower per-connection memory than Node. |
| Pairing + E2EE | `x25519-dalek`, `chacha20poly1305` (have), `ed25519-dalek` (have); or `snow` for a Noise handshake | Noise is a better fit than a hand-rolled transcript protocol. |
| SSH remote execution | `russh` (pure Rust, async) or `ssh2` (libssh2) | Replaces the esbuild relay bundle with a cross-compiled static binary — *simpler* than Orca's approach, which must ship Node artifacts per platform. |
| Git worktree orchestration | shell out to the user's `git`, as Orca does; `gix` or `git2` for reads | Copy the decision, not the library. Orca treats git 2.25 as the baseline and caps capabilities per execution host. `gix` worktree support is still thinner than libgit2. |
| File watching | notify 8.2 (have) | — |
| Agent process supervision + status store | `crates/core` | One store owned by the execution host; readers hold presentation policy only. |
| Cron / scheduling | `croner` 3 (have) | — |
| Mobile companion | `crates/mobile-ffi` UniFFI + native Swift/Kotlin UI | LocalGPT already ships this shape. |

**Assessment: no blockers.** This is roughly the whole of `orcad` + the terminal daemon +
`cloud/apps/relay`, and LocalGPT has the foundations for most of it.

### 6.3 Tier B — Feasible, Real Work

| Feature | Rust path | Cost |
|---------|-----------|------|
| Terminal emulation | `alacritty_terminal` (the VTE state machine published from Alacritty; also what Zed uses) or `vte` for parsing only | Medium. Proven by Alacritty, WezTerm and Zed. |
| Terminal rendering | wgpu 27 (already in-tree via Bevy) + `glyphon`/`cosmic-text` | Medium. Ligatures and emoji width are the long tail — the same tail Orca patched xterm.js seven times for. |
| Native UI shell | egui 0.34 (have), `iced` (used by Local Native), or `dioxus` | Medium. egui is fine for tools and weak for dense IDE chrome. |
| On-device ASR | `whisper-rs`, or bind the same sherpa-onnx C API Orca uses | Low–medium. |
| Markdown preview | `pulldown-cmark` | Low. |
| Computer-use natives | Already Swift/PowerShell/Python in Orca — language-neutral | Low. |

### 6.4 Tier C — Where Pure Rust Loses

Be blunt about these rather than discovering them at month six.

| Feature | Why Rust loses |
|---------|----------------|
| **Monaco-class code editor** | There is no Rust equivalent. Building one means `ropey` + `tree-sitter` + LSP client + your own rendering — Zed proves it is possible and took years with a funded team. |
| **Rich text / TipTap** | No Rust ProseMirror. |
| **Mermaid diagrams** | No Rust renderer. Requires a webview or an embedded JS engine. |
| **PDF preview** | `pdfium-render` or mupdf bindings; both pull large C++ dependencies and neither matches pdf.js's viewer UX. |
| **Design-system velocity** | Tailwind + shadcn + radix is a genuine productivity multiplier. Orca has a 100+ line style guide and four lint configs enforcing it, all of which assume CSS. |
| **7-locale i18n with lazy catalogs** | Possible (`fluent`), but the tooling gap vs. i18next is wide. |

### 6.5 Recommended Architecture — Hybrid, Split at Orca's Own Seam

Orca has already drawn the line for us. `orcad` is plain Node precisely because the runtime
does not need Electron. Substitute Rust there and keep a web UI for Tier C:

```
┌─────────────────────────────────────────────────────────┐
│  UI shell  —  keep the web platform                     │
│  Tauri 2 (or the existing axum-served web client)       │
│  React + Monaco + xterm.js + mermaid + Tailwind         │
└──────────────────────────┬──────────────────────────────┘
                           │  WebSocket RPC (existing)
┌──────────────────────────▼──────────────────────────────┐
│  localgpt-orchestrator   —  Rust, single binary         │
│  worktrees · agent supervision · status store · git     │
│  crates/server (axum)  ·  crates/sandbox  ·  cron       │
└─────┬───────────────────────────────┬───────────────────┘
      │ crates/bridge (interprocess)  │ russh
┌─────▼──────────────┐       ┌────────▼────────────┐
│ terminal daemon    │       │ remote host binary  │
│ portable-pty       │       │ (cross-compiled,    │
│ survives restarts  │       │  no Node needed)    │
└────────────────────┘       └─────────────────────┘
                     ┌───────────────────────────┐
                     │ relay (axum) + pairing    │
                     │ E2EE · push · Postgres    │
                     └───────────────────────────┘
```

What this buys: one static binary instead of ~200MB of Electron, `crates/sandbox` isolation
that Orca has no answer to, a remote agent that cross-compiles rather than shipping per-platform
Node bundles, and no Tier C rewrite.

**Caveats to state up front.** Tauri uses the *system* webview (WebView2 / WKWebView), not
Chromium — xterm.js WebGL behaviour varies by platform, and `wry` has no clean equivalent of
Electron's `BrowserView` overlay. Design Mode needs an external Chrome over CDP either way,
which LocalGPT's `browser.rs` already does. If those webview differences bite, the fallback is
to keep serving the existing web client from axum and skip the native shell entirely.

### 6.6 Rough Effort

Ranges, not estimates — anchored to a small team.

| Scope | Effort |
|-------|--------|
| Rust terminal daemon + PTY multiplexing over `crates/bridge` | weeks |
| Git worktree orchestration + agent status store | weeks |
| SSH remote execution host (`russh`) | weeks–months |
| Relay + pairing + E2EE, self-hosted | months |
| Tauri shell wrapping the existing web client | weeks |
| Native Rust terminal widget (alacritty_terminal + wgpu) | months |
| Monaco-class editor in Rust | **do not** |
| Full Orca parity | not achievable at this team size |

---

## 7. Recommendation

1. **Do not port Orca.** 3.8M lines of `src/` TypeScript with 8,910 test files is not a target.
2. **Do take the orchestration core in Rust.** Tier A is aligned with LocalGPT's existing
   crates, keeps the single-binary story, and is where Rust is genuinely the better tool.
3. **Start with the two highest-leverage, lowest-risk pieces**, both of which stand alone and
   neither of which needs a UI decision:
   - Harden `crates/bridge` with the socket-ownership protocol in §5.1.
   - Add `portable-pty` and give the daemon PTY ownership that survives restarts.
4. **Keep the web platform for anything a user reads or edits.** Editor, diagrams, previews,
   design system. Revisit only if a native shell becomes a product requirement.
5. **Treat `login.onorca.dev` as the real moat.** The relay being open is not the same as the
   product being self-hostable. If LocalGPT ever ships a companion app, the auth plane is the
   part to design first, because Orca demonstrates it is the part everyone keeps private.

---

## 8. References

Paths are relative to the Orca checkout (`external/orca`, read-only reference — do not modify).

| Topic | Path |
|-------|------|
| Contributor rules and platform invariants | `AGENTS.md` |
| Socket ownership protocol | `src/main/daemon/AGENTS.md` |
| Runtime supervision contract | `docs/reference/orcad-operations.md` |
| SSH execution boundary | `docs/reference/ssh-execution-boundary.md` |
| Agent status store | `docs/reference/agent-status-store.md` |
| Mixed-version wire compatibility | `docs/reference/remote-wire-compatibility.md` |
| Git version baseline | `docs/reference/git-compatibility.md` |
| PTY transcript capture | `docs/reference/agent-pty-transcript-capture.md` |
| Relay architecture | `cloud/README.md` |
| Design system | `docs/STYLEGUIDE.md` |
