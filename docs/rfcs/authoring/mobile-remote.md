# Mobile: the page as a remote for the desktop app

**Status:** decided, and step 1 built on 2026-10-31, but not yet tried on
a phone in a hand. It was checked by a protocol test and a headless browser
at an iPhone's size; *Step 1, as built* says what that covered. Answers the
*Mobile* question [the-harness-goes.md](the-harness-goes.md) left open:
`apps/apple` and `apps/android` leave with `mobile-ffi` in 0.5.0, and no
native app takes their place. Builds on [the-page.md](the-page.md). The
readings behind it are Orca's mobile notes (`external/ade/orca/mobile/`,
MIT) and [cad-lessons.md](cad-lessons.md) §7.

## The problem

The two app trees are the assistant on phones. `apps/apple` (7,864 Swift
lines) and `apps/android` (2,031 Kotlin lines) hold a chat, the memory
workspace and, on Apple, an on-device model whose persona begins "You are
LocalGPT, a helpful AI assistant with persistent memory"
(`AppleFoundationModelsService.swift`); `LocalGPTClient` wraps `AgentHandle`
through `mobile-ffi`. Nine commits, the last on 2026-09-23. With the harness
gone, nothing in them is the product.

The product is a desktop app whose agents are processes in its page's
terminals. Asked of a phone or an iPad, that gives two wishes:

1. **A remote.** Away from the desk: see what the agents did, answer the one
   that is waiting, type the next prompt, look at the world.
2. **Everything on the device.** A coding agent on an iPad or a phone, and
   the app's features beside it.

## The agent does not run on the device

- **iOS.** An App Store app cannot start processes, and Claude Code and
  Codex are processes that start more: a shell, `git`, builds, MCP servers.
  Node runs on iOS only without a JIT (nodejs-mobile) and without
  `child_process`. The shells iOS allows, iSH (which emulates x86) and
  a-Shell (whose commands are WebAssembly), are far too slow and narrow for
  an agent's builds, and App Review guideline 2.5.2 rules out downloading
  code that changes what an app does.
- **Android.** Termux's Linux userland runs Claude Code, but a Play app that
  targets Android 10 (API 29) or later cannot execute files from its writable
  storage, which is why Termux ships from F-Droid and GitHub. An agent on
  Android is a sideload.
- **Either way it undoes the pivot.** A device that cannot run the agents'
  own processes gets an agent only through a harness of ours (our prompts,
  our tools, our model menu) or through a Linux userland we would ship and
  maintain. The page chose terminals so that the agents come whole, with
  their own TUIs, approvals and sign-ins; a device that cannot run them
  cannot have them whole.

So the agent stays where it runs, on the desktop or (step 3) in a cloud
sandbox, and the device is a window onto it.

## What others do

- **Orca** (`external/ade/orca`) has the remote, as a product. A React Native
  app of about 3,000 source files pairs with the desktop by QR code, talks to
  a WebSocket RPC on port 6768 (`terminal.list`, `subscribe`, `send`,
  `read`, `create`, `show`; `worktree.ps`) and draws each terminal with
  xterm.js in a web view (`mobile/README.md`). Away from the local network it
  goes through relay cells that splice end-to-end encrypted streams keyed by
  the host's X25519 key, with a push gateway for APNs and FCM
  (`cloud/README.md`), a stack with a security review of its own
  (`cloud/docs/relay-security-2026-10-21.md`). That is a company's
  infrastructure.
- **SolidCraft and CADCraft** have no mobile app. Each ships the whole
  application as WebAssembly in a browser tab.
- **Claude Code on the web and Codex's cloud tasks** run the agent in a cloud
  sandbox against a repository, and the phone is a browser. Step 3 is built
  on them.

## The design: the page is the remote

The desktop app serves its own page to the phone over the local network.
Nothing is installed on the phone.

**The page already allowed it.** All its traffic passes through one file,
`crates/app/ui/src/bridge.ts`: in through `window.__lg.recv`, out through
`window.ipc`, and in a plain browser through a stand-in. On a device the
stand-in is a WebSocket (`ui/src/remote.ts`), and the app's listener
(`crates/app/src/webui/remote.rs`) carries the window's own messages,
`FromPage` and `ToPage`. Whatever the window is sent, the devices are sent
too. A device that arrives late gets the newest of each snapshot first, in
the order they last changed. A device's asks are the window's, except for
what belongs to the computer: the hole, Open…, a link for its browser, and
the listener itself.

**The terminals** became every page's (`crates/app/src/webui/term.rs`):

- **The screen, not a ring of bytes.** Each tab keeps its screen as a
  terminal would (`vt100`). A page that attaches late is sent a reset, then
  the cells, the cursor and the input modes, so it draws what the terminal
  shows. A ring of the last bytes would start mid-escape and replay a TUI's
  redraws onto a blank screen. The window's page attaches the same way after
  a reload, and finds its tabs still running.
- **Fanned out.** Every page gets every tab's bytes, and a tab one page
  opens is listed for all; the window adopts a tab the phone started.
- **One size.** A PTY has one size, and a phone is narrower. The page that
  typed or clicked into a tab last sets it, as tmux gives the latest client
  its size, and when a device goes, the window takes the size back. A page
  that is not driving draws the screen at the PTY's size; the phone scales
  it to its width.

Orca's notes add what it learned the hard way, and step 1 follows them. A
page asks again for everything, and attaches again, each time the line
comes back (`terminal-output-streaming-findings.md`). Keys go straight to
the terminal by default, with a compose box to switch to
(`mobile-terminal-direct-input-default.md`). The phone also has a key row
with Esc, Tab, Shift-Tab, the arrows (in the terminal's cursor-key mode),
^C and Enter, which Claude Code's TUI needs and a phone keyboard lacks.
The compose box takes the phone's dictation. It sends its text as one
bracketed paste and then Enter, so new lines stay in the prompt.

**The world: frames first.** The design said the phone would draw the world
with the format's web viewer, from `GET /world`. Step 1 sends frames instead:
the window read back once for everyone who asked, cut to the world's part,
no wider than a phone, and a JPEG of about 70 KB. A frame is asked for again
as each one arrives, while the World view is open, and never otherwise. That
works in every mode: a document's world and a song's have no live API, and
the phone never reaches the live API at all. The viewer stays the step for a
live canvas, where orbiting on the phone is worth its weight, and
[the-world-in-the-page.md](the-world-in-the-page.md) asks whether the window
draws with it too. Beside the frame are the shots, from the timeline's view:
a tap looks through one, and the song plays and pauses.

**A phone layout** (`ui/src/phone/`; drawn for review on
`design/phone.html`), not a second interface:

- three views at the thumb: Agents, World and History;
- a waiting proposal's Accept and Reject over all three, as Enter and Esc
  are at the desk;
- a note when an agent's turn ends.

Open…, the timeline, the outliner and the document stay the window's, at
its width. An iPad gets the phone's layout, roomier.

**Telling the person.** Turn marks come from the marks agents put in their
terminal titles, read as Orca reads them:

- a spinner while one works: Claude Code's braille frames, then its quarter
  circles, and Gemini CLI's ✦;
- ✋ when it asks;
- ✳ or ◇ at rest.

A title without these marks says the state is not known.

The working-to-rest edge is `TurnEnded`. It marks the history on a live
canvas, gives every tab its dot on both pages, and shows "Claude Code is
done" or "is asking you" on the phone, with a buzz where the phone allows.
An agent that puts words in its title rather than these glyphs
(Orca reads Codex's that way) has no turns yet. A page served over plain HTTP
gets no system notifications (the Notification API needs a secure context),
so step 2 brings those.

## Mirror the terminal; don't speak ACP to the phone

A structured conversation suits a phone better than a TUI at phone width:
message bubbles, a card per tool call, Allow and Deny as real buttons.
SolidCraft's chat shows the form (cad-lessons.md §7). But an ACP session is
a conversation of its own, and it cannot attach to the Claude Code already
running in the desktop's terminal. A native chat on the phone would mean
running that agent over ACP on the desktop too and giving up its TUI there,
which is the trade the-page.md decided against. So the phone mirrors the
terminal. If testers find that too cramped, the fallback is an ACP session
the phone starts for a new conversation, beside the terminals, not instead
of them.

## Security

A paired phone that can type into those terminals is a remote shell on the
Mac.

- **Off by default.** It is turned on from the top bar's Phone button (or
  with `LOCALGPT_APP_REMOTE=on`, which also prints a pairing link to the
  terminal the app was started from), and remembered after.
- **Pairing.** Turning it on shows a QR code with the address and a
  one-time code (128 bits, ten minutes). The device trades the code for a key
  of its own (256 bits), which it keeps in its browser. The app keeps only the
  key's hash, in `<state>/app-remote.json`, written `0600`. The panel lists
  the paired devices and forgets any of them; forgetting closes that
  device's connections.
- **Seen.** While a device is attached, the button names it, with a dot.
- **Its own socket.** The listener is on port 9880 of every interface (any
  free port when that is taken), never the live API's, which stays on
  127.0.0.1. A pairing or a socket from another site's page is refused by
  its `Origin`.
- **A trusted network only.** Step 1 is plain HTTP on the local network:
  anyone who can intercept it can read the terminals or change the page. It
  is for a network the person trusts, and the panel says so. A VPN the
  person already runs (Tailscale, WireGuard) carries step 1 away from home,
  with the tunnel as the encryption, at no cost to us.
- **The live API's token** (cad-lessons.md §8), done first:
  - it is now 128 random bits, not a hash of the process id, the time and
    the folder;
  - `.live/` is `0700` and `endpoint.json` `0600`;
  - the token rides a query string only to `GET /board`, the page a person
    opens in a browser.

## Steps

1. **The local remote.** Built 2026-10-31; see *Step 1, as built*.
2. **Away from home without a VPN, with push**, when step 1's users ask for
   it. Through `crates/relay`, which forwards frames without reading them
   (`crates/relay/src/main.rs:1–6`) but encrypts nothing, so whoever runs it
   could read a terminal. First, then, an end-to-end layer keyed at pairing
   (an X25519 key in the QR code, as Orca's relay cells use). The page's code
   must then come from somewhere other than the relay, or the relay could
   serve a page that reads the keys: a thin wrapper app (a web view holding
   the page's bundle, and push registration) does that and gets push too.
   Push needs a small service holding the APNs key, as Orca runs one, and a
   notification names the agent and the world, never terminal content.
   Weeks.
3. **The iPad without the desktop**, if previs testers ask (below). The
   person talks to a cloud agent in that agent's own web app (Claude Code on
   the web, Codex). The agent edits the world, kept in a git repository,
   headless with the `localgpt` CLI from its one-line installer, and pushes a
   branch. The page, in the iPad's browser, opens the world from the
   repository, folds it with the format's npm package, shows the branch in
   the history, and the viewer draws it. This is review, not live
   co-editing: minutes, not seconds. Gestures on the iPad itself would need
   the authority's write half (binding, strict ingest, commit) in the
   browser. It exists in Rust (`world-agent::live`) and in Swift (the
   format's `Authoring.swift`); a WebAssembly build of it is unproven,
   though `previs-web` runs a workspace crate as WebAssembly in a page.
   Weeks.
4. **Native only for what a browser cannot do**, chiefly AR on a location
   scout. That is worldwalk's: it already looks through a shot, has no ARKit
   yet, and keeps its rule of adding no format semantics of its own.

## Step 1, as built

Checked on 2026-10-31 against the app on a live canvas, by
`scripts/remote-smoke.mjs`, which launches the app with its state in a
throwaway folder and plays a device:

- **Its 25 protocol checks passed:**
  - the page served and marked as a device's;
  - pairing, its code working once, the key file `0600` and holding no key;
  - a stranger refused;
  - the catch-up, then a shell opened from the phone, attached, typed into,
    and its output back;
  - a spinner title and ✳ as working and at rest;
  - a second device attaching late and getting the screen;
  - the size following the page that typed, and going back when it left;
  - a frame;
  - a device refused the listener's own asks.
- **The phone's page** (`--phone`) ran in headless Chromium at an iPhone's
  size. It opened a shell, typed into it through the compose box, and showed
  each view with no page errors.
- **The window's own page adopted the device's tab.** When the second device
  left, the size went back to the window's agents panel, 65 by 46, so the
  page in the real web view had attached to the tab.

The frames' pixels came back black, as did the live API's own
`GET /screenshot`, because the Mac's screen was locked; what a frame shows
is unchecked.

Still to do:

- **On a phone in a hand:** iOS Safari's keyboard with xterm.js, its
  dictation in the compose box, Android Chrome, and the first macOS
  firewall prompt for port 9880.
- **The addresses besides the primary one** in the panel, such as a
  Tailscale address. A browser keeps the key per address, so a device opened
  at another address pairs again.
- **The viewer** for a live canvas's World view.
- **A layout for the iPad** that uses its width.

## Who it is for

Steps 1 and 2 serve anyone who runs agents in the app. Steps 3 and 4 serve
previs, whose people work on set and on scouts with iPads: Previs Pro, one
of the two references in [world-strategy.md](../../world-strategy.md) §15.2,
runs on iPhone, iPad and Mac. Whether they want this from LocalGPT is for
the previs user test to say, and
[previs-user-test.md](../../previs-user-test.md) now asks.

## What this gives up

- A native app's polish and a place in the App Store; step 2's wrapper is as
  close as this comes.
- Notifications while the phone's page is closed, until step 2.
- The Apple app's on-device authoring with Apple's Foundation Models
  (`AppleFoundationModelsService.swift`,
  [apple-foundation-models.md](../../mobile/apple-foundation-models.md)). If
  it is wanted, it is worldwalk's question, as the-harness-goes.md says.
- A phone that works with no network path to the desktop.

## Not decided

- Whether the phone also finds the desktop by mDNS (Gen already announces
  `_localgpt-world._udp.local.` for rooms), or by the QR code only.
- Whether the phone can open another world (Open…) or only follows the
  desktop's: it only follows, for now.
- Whether the Windows host (on the-page.md's *Next* list) is still needed
  as soon: where no host opens the page, the app now makes the page all the
  same, and a device's browser can be the interface.

## Status

- [ ] `apps/apple`, `apps/android` and `mobile-ffi` removed (0.5.0,
      the-harness-goes.md)
- [x] turn marks from the page's terminals, from the agents' titles
- [x] the live API's token: random, the owner's alone, in a query only to
      a board page
- [x] step 1: the transport, the listener and pairing; the terminals'
      screens, fan-out and size rule; frames; the phone layout and its
      sheet (`design/phone.html`)
- [ ] step 1 on a phone in a hand, and the sheet reviewed
- [ ] the previs user test's mobile question asked
- [ ] step 2, when step 1's users ask
- [ ] steps 3 and 4, if previs testers ask
