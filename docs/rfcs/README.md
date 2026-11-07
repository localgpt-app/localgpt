# RFCs and technical design notes

Only active designs and references used by current work live here. Completed proposals, superseded interfaces and old research are available through Git history rather than a second archive of documentation.

Every design document opens with a `**Status:**` line that says what is true now. Implementation status belongs in that line or in a dedicated status section, not inferred from the rest of the proposal.

## Native application and editing

| Document | Current role |
|---|---|
| [authoring/all-rust.md](authoring/all-rust.md) | **Decided; core migration implemented.** eframe owns the desktop windows, egui draws the panels, Bevy renders into textures, and the terminal widget runs over `alacritty_terminal`. The remaining sections cover browser/mobile convergence, media editing and the all-Rust tooling rule. |
| [authoring/world-editor-stack.md](authoring/world-editor-stack.md) | **Partly implemented.** The rule that the scene is a projection of a fold, the `world-editor` core, and the remaining reconciliation work. Its native chrome is now the all-Rust shell. |
| [authoring/mobile-remote.md](authoring/mobile-remote.md) | **Step 1 implemented.** A paired phone or tablet uses the desktop app's page to view the world and drive its terminals; no native mobile app or model runs on the device. |
| [authoring/rooms-and-the-channel.md](authoring/rooms-and-the-channel.md) | **Proposal; step 0 implemented.** Rooms over an open `.world`, per-person keys and the encrypted channel that would let rooms and the phone remote use a relay safely. |

## Technical lessons still in use

| Document | What it informs |
|---|---|
| [authoring/blender-lessons.md](authoring/blender-lessons.md) | Editor commands, undo, selection, transforms, the outliner and the separation between authored and evaluated state. |
| [authoring/cad-lessons.md](authoring/cad-lessons.md) | eframe/egui rendering, a command-driven agent surface, headless parity and Rust tooling. |

## Collaborative sessions

| Document | Current role |
|---|---|
| [multiplayer/collaborative-world-engine-architecture.md](multiplayer/collaborative-world-engine-architecture.md) | **Implemented in part.** The current op-based room authority, wire protocol, native and browser guests, persistence and relay. |
| [multiplayer/session-package-format.md](multiplayer/session-package-format.md) | **Implemented.** One package for a session's base world, committed ops, snapshots and replay. |

The old SpacetimeDB and planetary-scale designs are out of scope. A hosted room uses the same `world-sync` authority through a headless host or relay.

## The format

The `.world` format is a separate project. Its specification, conformance worlds and RFCs live in [`openworldformat/openworldformat`](https://github.com/openworldformat/openworldformat). Proposals that change what the format means belong there.

## Writing or retiring a design

1. State the current problem before proposing a design, and name the trade-off.
2. Keep a status line and update it when implementation lands.
3. Link only to living documents; do not build a chain of superseded proposals.
4. When the design is no longer needed for current work, delete it and repair its inbound links. Git preserves the decision history.
