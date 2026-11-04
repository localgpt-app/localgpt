# Rooms: other people in an open world, and the channel they come through

**Status:** proposal, 2026-10-31. Nothing built against it. Moves rooms up,
revising [world-strategy.md](../../world-strategy.md) §13.1 ("Multiplayer in
v1: Out") and §15.3 ("Moved down"). Puts rooms in the desktop app, over the
`.world` it has open, under the one authority of
[world-editor-stack.md](world-editor-stack.md) and
[the-cli-and-the-world.md](the-cli-and-the-world.md). Keeps the wire protocol
of [collaborative-world-engine-architecture.md](../multiplayer/collaborative-world-engine-architecture.md),
and designs the end-to-end layer that [mobile-remote.md](mobile-remote.md)'s
step 2 asks for, so that the phone and rooms share it.

## The problem

An open world has one person in it, with their agents. The person edits in
the window, the agents edit through the live API or `localgpt world`, and
every entry in the history names who made it. Nobody on another computer can
join. What exists for that is Gen's, and it falls short in five ways:

- **The app has no rooms.** Its Collaborate section went with its half of the
  harness. `localgpt-gen --host` is the only host, and it hosts Gen's scene,
  kept as a session package, not the `.world` the app has open.
- **A room is a second authority.** world-sync's `Authority` keeps its own
  document, revision and per-author undo stacks
  (`crates/world-sync/src/authority.rs:101–113`), beside `LiveWorld`, which
  the live API, the CLI and the person's own edits commit through.
  world-editor-stack.md settled the same case for `world-editor`: "Two
  implementations of submit and undo is one too many."
- **Who is in a room is a claim.** A hello carries a name the guest picks and
  one secret that every guest shares, the invite token or the 6-digit PIN
  (`crates/world-sync/src/protocol.rs:139–147`). Everyone it lets in gets the
  same role (`crates/gen/src/net/web.rs:846–863`), and a guest may take any
  name not in use, an absent person's included. On a live canvas every entry
  has an author, and `undo --own`, the outliner's marks and proposals depend
  on it. With one shared secret, removing one person for good means a new
  secret for everyone.
- **Nothing is encrypted.** The room's WebSocket is plain, on a LAN and
  through the relay alike. `localgpt-relay` doesn't parse what it forwards,
  but all of it passes through in the clear: the hello and its secret, every
  op, chat line and prompt. It also serves the join page
  (`crates/relay/src/main.rs:363`), so whoever runs a relay can read its
  rooms and could hand guests a page of its own. TLS in front of the relay
  protects the hops to it, not the room from it.
- **A hole that shouldn't wait for this design.** Through the relay, the PIN
  is reachable by anyone with a room's code, and nothing limits the guesses.
  A relayed guest may present the PIN in place of the token (`web.rs:846–857`).
  A failed hello closes a direct socket (`web.rs:705`), but not a relayed
  one: the relay client forwards the error and drops its close flag
  (`crates/gen/src/net/relay_client.rs:147–149`), and the relay acts on
  nothing from the host but frames (`crates/relay/src/main.rs:255`). Step 0
  closes it.

## What others do

- **Excalidraw** encrypts its rooms end to end in the browser. The key rides
  in the link's fragment, which a browser never sends to a server. The page
  comes from excalidraw.com, and its collaboration server forwards
  ciphertext. Anyone with the link is in: there is a link, not a person.
- **Tailscale** runs WireGuard, whose handshake is Noise IK, between devices
  whose public keys its coordination server hands out. When two devices can't
  reach each other, its DERP relays forward packets they cannot read.
- **Orca** pairs a phone with a desktop by QR code. Away from the network, it
  splices end-to-end encrypted streams through relay cells keyed by the
  host's X25519 key (mobile-remote.md, *What others do*).
- **Magic Wormhole** makes short typed codes safe with a PAKE (SPAKE2): a
  wrong guess costs one online try and fails where both sides see it.
- **Figma and Google Docs** keep accounts and serve over TLS to servers that
  read everything. LocalGPT has no accounts and runs no service that a world
  must pass through, so this is the model to avoid.

## The design

### One authority: a guest's edit is a batch through `LiveWorld`

The app hosts a room for the world it has open. The room keeps world-sync's
wire protocol (hello, welcome, presence, chat, the ops fan-out) and its
limits. It gives up its own document, revision and undo stacks.

A guest's edit becomes a batch handed to `live_world`
(`crates/gen/src/gen3d/live.rs:1801`). That is the one system that owns
`LiveWorld`, and it already drains the live API's calls and takes the
person's edits and review decisions. The batch's author is the guest's name.
Both sides speak `wt::EditOp`, so nothing is translated.

So a guest's edit gets everything an agent's has:

- it is ingested as strictly: whole or not at all, with no field the format
  would drop;
- it is in the world's history under the guest's name, and `undo --own`
  scopes to it;
- with review on, it waits as a proposal, as an agent's does;
- `at` makes it a branch.

Every commit goes to the room as ops, whoever made it (the person, an agent
or a guest). It leaves from where `live_world` publishes `LiveHistory`
(`live.rs:1639`). A guest who arrives late gets the head's world first, then
everything after it.

**Roles.** Each person has one, given with the invite and changed in the
people list:

| Role | May |
|---|---|
| watch | walk, look, chat |
| propose | submit batches that wait for review |
| edit | submit batches that commit |

**Presence.** Each person's camera and selection are drawn in the canvas and
marked in the outliner, as an agent's work is.

**Prompts stay out.** A guest's text never reaches the host's agents. They
run with a shell on the host's computer, and text from another person is a
prompt injection with that shell behind it. A guest who wants an agent
brings their own (step 4).

Gen's `--host` stays until the app's room does what it does, then goes, as
Gen's desktop mode went.

### Who is in the room: a key per person

An **invite** is a link or a QR code. It holds the room's address and a
one-time code (128 bits, used once, gone in minutes), as the phone's pairing
link does (`crates/app/src/webui/remote.rs:388`). It names the role it grants
and, from step 2, carries the host's public key.

At the first join, the one-time code is traded for the guest's key and binds
it to the invite's role and to the name the host sees. In step 1 that key is
a bearer key the app makes and keeps the hash of, as the phone's pairing
does (`remote.rs:642`). From step 2 it is a key pair the device makes, and
the host keeps only the public half, so nothing on its disk opens the room.
From then on **the key is the person**:

- The people list shows who is in and who is attached. Forgetting someone
  closes their connections.
- A batch's author is the name the host gave the key, never a string in the
  batch. That is agents-outside-the-window.md's "who sent this batch" as "a
  property of the connection rather than a claim in the body", for people.
- People are a list of their own, apart from the phone's devices. A guest's
  key never opens the remote, whose devices drive the terminals.

**The PIN goes.** A link or a QR code carries 128 bits. Six typed digits
were the one case that needed a PIN, and that is the case a guess can win.
If typed codes come back, they become a PAKE (SPAKE2 or CPace), where each
guess is one online try.

### The channel: end to end, inside the same WebSocket

Every connection opens with a Noise handshake inside the existing WebSocket,
on a LAN and through the relay alike. IK fits:

- The guest knows the host's static key from the invite, and sends its own,
  encrypted, in the first message.
- After the second message, each side has proven its key to the other.
- Everything after is encrypted with keys the relay never sees.

The rest of the channel:

- **The relay keeps its envelope.** `data` carries ciphertext. The relay
  still sees metadata: room codes, addresses, when each connection opened
  and closed, and how big each message is.
- **In Rust,** `snow` (Apache-2.0 OR MIT, which `deny.toml` allows).
- **In the browser,** `Noise_IK_25519_AESGCM_SHA256`, which WebCrypto's
  X25519, AES-GCM and SHA-256 cover. The key is a WebCrypto key the page can
  use but not read, kept in IndexedDB. WebCrypto exists only in a secure
  context, so this needs an HTTPS page (below).
- **The phone takes the same channel.** Its pairing already gives each
  device a key of its own. The remote's step 2 swaps that bearer key for a
  key pair and wraps the socket the same way, so the channel is built once
  for both. Where the phone's page comes from stays mobile-remote.md's
  question (its wrapper app).
- **Why not TLS.** A browser won't trust a certificate for a LAN address,
  and through the relay TLS ends at the relay.

### The page: from where nothing on the path can change it

Whoever serves a browser guest's JavaScript can read its keys. So the page
must come neither from the relay nor over plain HTTP, and a plain-HTTP page
has no WebCrypto besides.

- **Browser guests** open a fixed page at `localgpt.world/r/<code>`, where
  world-strategy.md §13.5 already put rooms, with the host's key and the
  one-time code in the fragment. The relay serves no page.
- **They reach the host through the relay,** even from the next room: an
  HTTPS page can't open `ws://` to a LAN address (mixed content).
- **The host's own plain-HTTP page** stays for a network the person trusts,
  as in step 1, unencrypted.
- **Another app joining as a guest** (step 4) has no page to tamper with, and
  can use the channel directly on a LAN.

The page is a static build of the repository's own client: `crates/world-export`'s
session client and viewer, versioned and served from localgpt.world.
Trusting it is trusting the project's site, as downloading the app is. If
[the-world-in-the-page.md](the-world-in-the-page.md) moves the window's world
into the page, the guest's page and the app's converge.

## What each step protects

| | Step 1 | Step 2 |
|---|---|---|
| A guest | commits as themselves, by role | the same |
| Someone watching the LAN | reads the room and the guests' keys | reads nothing, except a guest on the host's plain page |
| Someone changing LAN traffic | joins with a stolen key, changes the page | can't join; can still change the host's plain page |
| Whoever runs the relay | (no relay) | sees metadata only |
| A lost device | forget that person | the same |

## Steps

0. **Close the relay's PIN hole now**, before any of this:
   - a relayed hello takes the token only;
   - a failed hello ends a relayed connection as it ends a direct one, so the
     relay learns a close from the host;
   - a room stops taking the PIN after a few wrong ones.

   It touches `crates/gen/src/net` and `crates/relay`.
1. **The room in the app, on a network the person trusts.**
   - The app hosts a room for its open world on a listener of its own: 9879,
     Gen's session port, or any free port when that is taken. It is never
     the remote's 9880 or the live API's port.
   - Guests commit through `LiveWorld` by role.
   - Each person gets a key by the phone's pairing as it is: a bearer key,
     whose hash the app keeps.
   - Presence shows in the canvas and the outliner.
   - The browser guest uses the host's own page.
   - The invite and the people list are drawn on a design sheet first.
   - The WebSocket is plain, labelled for a network the person trusts, as
     the phone's step 1 is.
2. **The channel**, for rooms and the phone together:
   - key pairs at pairing;
   - Noise on every socket;
   - the guests' page at `localgpt.world/r/<code>`;
   - the relay forwarding ciphertext.

   This comes before any room goes over the internet, and before we run a
   relay.
3. **A relay we run.** One Durable Object per room, as the collaborative RFC
   sized it; then the phone's push (mobile-remote.md step 2).
4. **Later.**
   - Another app joining as a native guest.
   - A guest's own agent editing under the guest's name, through the
     guest's connection. The guest's app could give `localgpt world` an
     endpoint that forwards to the room.
   - Cloud rooms (the SpacetimeDB module, the collaborative RFC's phase 8),
     for a room that must outlive its host.

## Who it is for

Anyone who wants a second person in a world they are making with agents.
For previs, that is the crew: a director, a DP and an AD in one set from
three places, with a guest's change to a shot waiting as a proposal. Whether
previs crews want that is a question for the previs user test
([previs-user-test.md](../../previs-user-test.md)).

## What this gives up

- **A room without its host.** It lives in the host's app until cloud rooms.
- **Anonymous drop-ins.** Every guest is invited and has a key. A reusable,
  anyone-with-the-link invite is *Not decided*.
- **A direct LAN socket for an encrypted browser guest.** It goes through
  the relay.
- **Anything a server must read.** Moderation, search or history on the
  relay would need the room in the clear, so there is none.

## Not decided

- Whether an invite may be reusable, as Excalidraw's links are, and how
  that shows in the people list.
- What a guest sees when the host's window is on a branch or an earlier
  entry: main's head, or the window's view.
- Whether entries carry a signature by the author's key, so that the
  history proves authorship to someone who doesn't trust the host. That is
  the format's question, for the spec repository.
- Where the host's own key lives: the state directory (`0600`, as
  `app-remote.json` is) or the system keychain.
- Whether a room is also found by mDNS (Gen announces
  `_localgpt-world._udp.local.`).
- Whether a guest's first batch always waits for review.
- What a guest's own agent may do (step 4).

## Status

- [x] the relay's claims corrected in `docs/gen/multiplayer.md`, the
      collaborative RFC, the website's multiplayer page and the code's
      comments (2026-10-31)
- [ ] step 0: the relay's PIN hole
- [ ] step 1: the room in the app, by the phone's pairing, on a network the
      person trusts
- [ ] step 2: the channel, for rooms and the phone
- [ ] step 3: a relay we run, then push
