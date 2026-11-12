# Mobile: a paired remote for the desktop app

**Status:** step 1 implemented. The desktop app can pair a phone or tablet on the local network and serve it a remote page with terminals, the world and history, laid out for either. Protocol tests (`crates/app/src/webui`) and headless browser checks (`scripts/remote-smoke.mjs`; `--phone` drives the page at an iPhone's and an iPad's size) pass; real devices in a hand remain the acceptance test. There is no separate native mobile app and no model on the device.

## The product boundary

The world and the agents stay on the desktop. A phone or tablet is a remote for the app already running there:

- see the world and the current history;
- read and type into the same terminals;
- answer a waiting proposal;
- look through shots and play or pause the song.

This avoids pretending that a phone can reproduce a coding agent's desktop environment. iOS in particular cannot provide the unrestricted processes, shell, builds and child MCP servers those agents expect. It also avoids putting a second world authority on the device: the desktop remains the owner.

## Step 1: the local remote

The remote is off until enabled from the **Phone** button in the desktop's top bar or with `LOCALGPT_APP_REMOTE=on`, and the choice is remembered (`LOCALGPT_APP_REMOTE=off` keeps it off). When enabled, the app listens on every interface at port 9880 (`LOCALGPT_APP_REMOTE_PORT` overrides it; any free port when that one is taken), serves the device page over HTTP and accepts its WebSocket.

The desktop and device exchange the app's own messages: terminal input and screens, history, the clock, proposals and world frames. A device that attaches late receives the latest snapshots before live updates.

### Pairing and device identity

The desktop displays a QR code containing a one-time pairing code. The code works once and expires after ten minutes. A successful pairing gives the device its own random bearer key; the desktop stores only that key's hash in `~/.local/state/localgpt/app-remote.json`, readable by the owner alone.

The panel lists paired devices, shows which are attached and can forget any of them. Forgetting a device removes its access. Keys are scoped to the remote and do not grant access to a world's live API.

### Phone layout

A narrow page has three views:

- **Terminals** — the selected terminal, a compose box and controls for Escape, Tab, Shift-Tab, arrows, Ctrl-C and Enter. Compose text is sent as a bracketed paste followed by Enter, so multiline prompts survive.
- **World** — a JPEG of the desktop's world, the shots to look through, and song playback controls. Frames are requested only while this view is visible; one desktop readback is shared by attached devices.
- **History** — the current history and its playback controls.

A waiting proposal's Accept and Reject controls are available over every view. A terminal opened on the phone appears in the desktop window too.

### Tablet layout

A tablet shows the world and terminals together:

- side by side on wide layouts;
- world above terminals on narrower portrait layouts;
- history and shots in tabs under the world's frame, or beside it when the world is above the terminals.

Layout follows page width, never height, because opening the keyboard changes height and must not rearrange the terminal under the person's fingers. Below 600 px, including an iPad in Split View, the phone layout is used.

### Terminal sizing

A terminal has one logical size. The window or device that most recently typed or clicked into it sets that size; other views scale the result to fit. This keeps the agent's own interface consistent instead of reformatting it independently for every screen.

## What step 1 deliberately does not do

- It does not run an agent or a language model on the device.
- It does not interpret one agent's terminal title to infer turns or display agent-specific notifications. A tab is only a terminal.
- It does not expose the live package API to the device.
- It does not stream an interactive 3D viewer yet; the World view uses desktop-rendered frames. A format viewer can be added where direct orbiting justifies its cost.
- It does not provide push notifications or a secure internet path.

The longer-term interface direction is the shared panel model in [all-rust.md](all-rust.md). When those panels run well in a browser, they replace the current device-specific page rather than becoming a third interface.

## Security

A paired device can type into a terminal. That is effectively a shell on the desktop, with the person's permissions.

Step 1 is plain HTTP and WebSocket on a network the person trusts, or over a VPN they run. Anyone able to read the traffic can read terminal content, and anyone with a paired key can type until the device is forgotten. Do not expose port 9880 to the public internet.

The important boundaries are:

- pairing codes are one-time and short-lived;
- device keys are random and stored only as hashes on the desktop;
- devices can be revoked individually;
- the listener is off by default;
- a page from another origin can neither pair nor connect (the listener checks `Origin` against `Host`);
- remote keys and live-world API tokens are separate credentials.

## Step 2: away from the trusted network

Step 2 is conditional on people actually using step 1 away from their desk. It requires an end-to-end channel before any relay carries terminals or world content. The channel is shared with the rooms design in [rooms-and-the-channel.md](rooms-and-the-channel.md):

- pairing establishes a per-device key pair rather than a bearer secret alone;
- a Noise handshake runs inside the existing WebSocket;
- the relay forwards ciphertext and connection metadata only.

A relay must not serve the browser code that handles those keys. A thin wrapper with a pinned page bundle, or another trusted HTTPS origin, is required. Push notifications also need a small service holding the platform credentials; notifications may name the app or world but never terminal content.

No relay or push service is part of step 1.

## Previs and standalone viewing

The remote can help someone reviewing a board or walking a set away from the desktop. Whether previs users need that is a question for [`previs-user-test.md`](../../previs-user-test.md), not a reason to build another app in advance.

If users need to open worlds with no desktop nearby, build that as a standalone Open World Format viewer with local packages and sharing. Do not turn the remote into a second editor with its own authority.

## Validation still required

- iOS Safari and Android Chrome on physical phones: keyboard, dictation, terminal controls and scrolling.
- iPad and Android tablet in both orientations, with the keyboard open and in split-screen layouts.
- The first macOS firewall prompt and pairing on a secondary address such as a VPN address.
- Frame rate and latency on a real local network, not only a headless browser.
- Whether a format viewer is preferable to frames for live-canvas inspection.
- Whether mDNS discovery adds anything over the QR pairing flow.

## Status

- [x] Step 1: the listener, pairing and revocation, shared terminals, world frames, history and proposal controls, phone and tablet layouts.
- [ ] Physical phone and tablet validation.
- [ ] Previs mobile question tested with users.
- [ ] End-to-end channel and push, if usage justifies step 2.
- [ ] Browser version of the shared native panels, replacing the device-specific page.
