# Four agent desktops: what MiniMax Design, DeepSeek Harness, Codex and Orca are built on

**Status:** research, 2026-11-01. A reading of four desktop applications that
run coding or creative agents, from their installed bundles on a Mac (the
`app.asar` headers and their JavaScript read as text; nothing was run) and,
where it is published, their source: `external/ade/deepseek-harness`
(0.2.0-rc.1, 2026-09-28), `external/ade/orca` (2026-10-30),
`external/cli/codex` (2026-05-02). MiniMax Design's desktop source is not
published; its CLI (`external/cli/minimax-code`) says so. The companion
reading of two Rust applications is [cad-lessons.md](cad-lessons.md), and
what both readings mean for who owns LocalGPT's window is in
[the-world-in-the-page.md](the-world-in-the-page.md) and
[all-rust.md](all-rust.md). Nothing here is used as code; Orca and DeepSeek
Harness are MIT, the other two are not open.

## 1. The stacks

| | MiniMax Design 4.0.6 | DeepSeek Harness 0.2.1-rc.3 | Codex 26.915.31945 | Orca 1.4.245 |
|---|---|---|---|---|
| Bundle | `com.minimax.hub.global` (internal name "Hilo"), 704 MB | `com.deepseek.dsh`, 990 MB | installed as `ChatGPT.app`, `com.openai.codex`, 1.3 GB | `com.stablyai.orca`, 715 MB |
| Shell | Electron 43.8.3 (Chromium 150), electron-vite; Velopack updates | Electron 44.7.0 (Chromium 152), electron-builder, electron-updater | Electron app code (`openai-codex-electron`, Forge + Vite) on OWL, OpenAI's own Chromium: `Codex Framework.framework` 153.0.8010.48, `runtimeName: "owl"`; Sparkle updates on the Mac | Electron 43.8.1 (source pins 43.8.4), electron-vite, electron-builder, electron-updater |
| Interface | React 19, TanStack Router and Query, Base UI, Tailwind classes; a copy of React Flow for the node canvas; Tiptap, Mermaid, a PPTX renderer, wavesurfer, Excalidraw and Monaco viewers, three.js | React, Vite, zustand, its own primitives and docking layout; xterm.js, Shiki, KaTeX | React, Radix, Tailwind, jotai, TanStack Query, Motion; xterm.js, CodeMirror 6, Shiki, KaTeX, Mermaid, three.js | React 19.2, Tailwind 4, Radix, zustand, shadcn-style components; Monaco, xterm.js 6.1 (WebGL addon), Tiptap 3, Shiki 4 |
| The agent | the `opencode` binary (`opencode serve --hostname --port`) with their own plugins on `@opencode-ai/plugin` 1.18.18 and MCP tool servers | the "host": a Node process started with `ELECTRON_RUN_AS_NODE=1`, port 19387, over Node IPC; built on Cordis | `codex -c features.code_mode_host=true app-server`, JSON-RPC over stdin and stdout | the app's own, in the main process: node-pty, ssh2 for remote machines, Playwright and a bundled `agent-browser` |
| Bundled runtimes | ffmpeg, Python, a headless entry, Telegram and Lark bots; a "gateway" Node service (better-sqlite3, sharp, napi-rs) run as Node | Node 24.21.0, pnpm 11.28.5, Python with office libraries; node-pty, koffi; C for a Landlock launcher and a `flock` binding | `codex-code-mode-host` (deno_core and rusty_v8), `cua_node` (Node 24.21, for computer use), ripgrep, plugins; node-pty, better-sqlite3, yjs, vscode-jsonrpc, objc-js | `agent-browser`, "Orca Computer Use.app", `orcad-template`, a relay, ripgrep, plugins, skills |
| Native code | — | the two C pieces above | — | Swift (computer use on macOS, 35 files; keyboard layout, notifications, memory use), Objective-C (update scheduling), C and Python (Wayland and X11 clipboard, computer use on Linux), Rust (`windows-agent-hook`, 2 files; `windows-cli-launcher`, 5), C++ Node add-ons (Windows registry and security) |
| Elsewhere | plugins in sandboxed iframes with agent-method manifests; the Director Stage and ComfyUI's front end bundled (§3) | | | `mobile/`: an Expo and React Native app with a web view; `cloud/`: a separate workspace |

Orca is TypeScript on Electron, not Rust: its Rust is the two Windows
helpers.

## 2. The pattern

The same shape four times:

- **A page in its own Chromium.** All four ship a browser engine; three on
  Electron and Codex on OpenAI's build of Chromium with Electron's app code
  on top. None uses the system web view, as LocalGPT's page does through wry
  and WKWebView. That is what the 700 MB to 1.3 GB are.
- **The shell loads the page from a custom scheme** (`dsh-app://app/`) and
  reaches a local process for the work: the agent or a host, started as Node
  (`ELECTRON_RUN_AS_NODE=1`) or as a binary, over localhost HTTP and
  WebSocket, stdin and stdout JSON-RPC, or Node IPC.
- **The terminal is xterm.js over node-pty** wherever there is one (DeepSeek
  Harness, Codex, Orca): the agents' own TUIs, whole. MiniMax Design has no
  terminal; its chat is `opencode serve` and plugins.
- **Embedded web content is a `WebContentsView`**, and **runtimes are
  bundled** rather than found: Node, Python, ffmpeg, Deno, the agent
  binaries.

## 3. The Director Stage

MiniMax Design's previs plugin, Director Stage 0.24.10, is the nearest thing
to [world-strategy.md](../../world-strategy.md) §15's plan, and is read
against it in §15.2's table.

- A three.js stage: mannequins, GLB and glTF props, Gaussian splats (SPZ and
  PLY) as sets; clips recorded with `MediaRecorder`.
- The agent's methods: `scene.get`, `scene.edit`, `scene.history`,
  `scene.snapshot`, `scene.diagnostics`; `model.generate`, `model.capture`,
  `model.compare`; `campath.read`, `motion.read`. Camera paths and motion
  are two text DSLs.
- `model.generate` takes JavaScript that returns a `THREE.Group`: the agent
  writes code for a prop. The format takes ops.
- Cameras are set by field of view, not lens and sensor; units are metres
  and the ground is `y = 0`.
- A phone is a handheld camera: its gyroscope over WebRTC, with a Python
  signalling daemon.

**For LocalGPT:** *adopt* `scene.diagnostics` — a scene's warnings as one
call an agent reads, which `GET /proposals` carries today only for a
proposal. *Adapt* `model.compare`: two renders of the same shot from the
same camera, which is the parity check the-world-in-the-page.md's status
asks for between renderers. *Avoid* code as the agent's way to a prop; the
format's ops and the pack are the vocabulary, and a prop an agent generates
arrives as a mesh asset. *Avoid* fov-only cameras: `ext-cinematography`
already carries sensor and lens.

## 4. What transfers

- **The agent as a local server with a typed seam** (Codex's `app-server`,
  JSON-RPC on stdio; DeepSeek's host over IPC) is the shape
  [agents-outside-the-window.md](agents-outside-the-window.md) chose for ACP,
  and the page chose the other door, the agents' TUIs in PTYs. Both are in
  use at this scale; the terminal is the one every one of them that runs a
  coding agent ships.
- **One emulator and a PTY per tab, with the agents whole**, is the
  industry's answer, not only the page's (the-page.md).
- **The bundle's size is the browser's.** LocalGPT keeps its own small by
  using the system's web view, and keeps it smaller still with no web view
  at all ([all-rust.md](all-rust.md)).

## 5. What does not

- **Bundling runtimes.** The agents come from their makers
  ([the-harness-goes.md](the-harness-goes.md)); the app runs what is
  installed and says what is missing (`localgpt doctor`).
- **Plugins in iframes** and a node canvas: no plugin system is planned.
- **Computer use.** Orca's Swift and Python drivers and Codex's `cua_node`
  are their products' reach into the desktop; LocalGPT's agents reach the
  world through its API.
