# localgpt-world-sync

Collaborative sessions over the LocalGPT world format: the room around the
fold. The authority that orders ops, the wire protocol, scene diffing and
undo live here. The document fold, the op log and the session op
vocabulary come from the [`openworldformat`](https://crates.io/crates/openworldformat)
crate.

No Bevy, no async runtime, no sockets: a transport (a session server, a
relay, a headless server) feeds messages in and delivers what comes out.

Part of [LocalGPT](https://github.com/localgpt-app/localgpt). License: Apache-2.0.
