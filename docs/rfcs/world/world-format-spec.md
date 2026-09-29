# The `.world` format as a public specification

**Status:** proposed. Nothing here changes the formats — world-types (schema
3) and the session package are the substance; this RFC is about *freezing
and publishing* them as a specification other software can implement, and
about what that commitment costs. See
[Implementation status](#implementation-status) for what already exists.

Related: `docs/rfcs/multiplayer/session-package-format.md` (the artifact),
`docs/world-strategy.md` §5 (source format versus distribution export — the
decision this builds on), `docs/rfcs/worldgen/world-package-format.md`
(the survey).

## The question

Gen, MD and Verse already share one world format, one session package, two
renderers, a generated schema and a conformance suite. Could that be a
*public* specification — a `.world` folder other people's software reads
and writes, covering interactive worlds, AI authoring and gaming?

Yes, with one discipline: **cover use cases through a small mandatory core
plus profiles, never through maximalism.** Formats that try to hold
everything die of their own weight (the survey's "single shareable
archive" idea, superseded). Formats that survive — glTF, Lottie, the web —
are a minimal core, a must-ignore extension rule, and hard versioning.

## Positioning: the layer above glTF

`.world` does not compete with glTF, and the spec says so in its first
paragraph. glTF owns mesh transport; `.world` owns what glTF deliberately
doesn't: composition, parametric authoring, behavior, audio, game state,
sessions, history. Mesh assets stay glTF leaves, referenced by content
hash. Every glTF tool keeps working; every glTF viewer is a leaf renderer.
The niche is empty: USD owns film composition, glTF owns transport, engine
formats are engine-locked — nobody owns *the save format for interactive,
AI-authorable worlds*.

## The package

```
thing.world/                     a directory (a zip is the transport form)
  manifest.json            L0    the world document
  state.json               L0    the world's typed state document
  ops.jsonl                L1    the session log: edits, tool calls,
                                  visitor input, state deltas, clocks
  snapshots/rev-<N>.json   L1    derived keyframes, never authoritative
  assets/<sha256>…         L2    content leaves (glTF meshes, textures,
                                  audio), content-addressed
  package.json             L3    format version, profiles, integrity
                                  hashes, license, provenance
```

Invariant (the session package's, restated as the spec's): state at any
revision is a pure fold of the log over the base.

The normative serialization is **JSON** — `world.schema.json` is generated
from the types and *is* the definition, with prose as explanation. RON
remains a LocalGPT authoring convenience, not part of the spec. This is
the anti-`extras` lesson: no "implementations may ignore" side channels
for core semantics.

## Profiles

| Profile | Adds | For |
|---|---|---|
| **Viewer** (core, required) | manifest, entities, parametric shapes + mesh refs, PBR materials, punctual lights, environment, hierarchy, ids/names, versioning | any renderer, converter, gallery |
| **Player** | behaviors, triggers, ambient/emitter audio, soundtrack + modulation, avatar, tours, the state document | games, walkable documents, song worlds |
| **Session** | the ops log, revisions, authorship, snapshots, undo semantics; the wire protocol as an appendix | multiplayer, recordings, editors, save games |
| **Extensions** | namespaced, registry-governed (`ext-physics`, `ext-avatars`, …) | what a second implementer needs |

Rules: an implementation ignores what it doesn't understand — unknown
profile, unknown extension, unknown field — and still renders the core.
The extension registry accepts a namespaced extension only with a
reference implementation and conformance cases (glTF's process, adopted
deliberately).

## Gaming, almost for free

- **Save games** are base + the player's session log — a save *is* a
  package.
- **Replays** are the input log (chess notation, Doom demos).
- **Multiplayer** is the ops protocol plus one authority.
- **Mods** are `EntityPatch` batches against a pinned base revision;
  the patch type is the mod format, and versioned bases make mods
  addressable ("for base rev ≥ 42").
- **Quests, inventory, score** are the state document.
- **NPCs** — `npc.rs` already models brains and memories declaratively.

Two honest gaps gaming exposes:

1. **The state document.** Today `StateRecord` is a bag of JSON values.
   The spec version declares a world's state schema — namespaced, typed
   (`score: int`, `inventory: map<item, count>`, `quest.*: bool`) — with
   the log carrying deltas. This is the one real format work item.
2. **The determinism contract.** Bit-exact cross-engine replay is not
   achievable (floats, physics ordering) and the spec must not promise
   it. It defines *semantic replay*: same fold, same trigger outcomes,
   approximately the same frames, with the seed field reserved for
   engines that want more.

## What an implementer gets, and what makes them adopt it

1. The JSON Schema, normative, already generated.
2. The conformance suite — the reference worlds that two engines (Bevy,
   three.js) already render; compliance means "renders the suite".
3. A reference viewer in a few hundred lines (`world-viewer.js`,
   permissive license) — "I rendered a `.world` in an afternoon."
4. Three producers on day one (Gen, MD, Verse) and a public gallery
   rendering the format live in the browser.

## Versioning policy

Written as policy from lessons already paid for:

- `schema_version` gates parsing; minor versions add optional fields;
  major versions may restructure but must not silently drop data (the
  v2→v3 lesson: a v2 reader silently losing instanced parts and triggers
  was the bug that forced schema 3).
- Log formats evolve additively: new entry kinds fold to nothing for old
  readers (the untagged-`SessionOp` rule), and old files parse under new
  readers forever.
- Conformance worlds are versioned with the schema; a renderer claiming
  version N passes version N's suite.

## Home, naming, governance

The spec must not live under the LocalGPT brand, or it reads as a vendor
format — the exact accusation localgpt.world's "portable, open formats"
line exists to pre-empt.

- **A neutral repo and org** (name undecided; a working candidate is the
  *Open World Format*, which earns its pun: an open format for open
  worlds). The repo holds spec prose, schema, conformance worlds, the
  reference viewer and an RFC process — the workflow this repository
  already uses, lifted out. The name's namespace is verified free as of
  2026-09-29 (RDAP): `openworldformat.org`, `.dev`, `.world` and `.io`,
  the `openworldformat` GitHub org and npm name all unregistered — with
  `worldfile` / `worldpack` as fallbacks. Before committing the name,
  check it for prior use and trademark (not verifiable from here).
- **A small spec site** on its own domain, carrying documentation and an
  in-browser viewer demo — and deliberately *not* the LocalGPT family
  strip, which every LocalGPT site carries and a neutral site must not.
- **localgpt.world is the showroom, not the home**: the gallery is the
  strongest marketing asset the spec has — every world page is the
  format rendering live — and the spec site links to it as the flagship
  demonstration, the way a spec site links to a reference deployment.
  Cross-link both ways; brand one direction only.

## Non-goals

Not a game engine, not a physics solver, not a renderer, not glTF's job,
not a content-management system. The spec describes what a world *is*;
what an engine *does* with it stays the engine's.

## Adoption path

1. Extract: spec prose + schema + conformance worlds + reference viewer
   move (or mirror) to the neutral repo, describing what exists today as
   1.0. Zero format changes required.
2. Close the two gaps: the typed state document; profiles and the
   extension mechanism.
3. Publish: spec site with the browser demo; the gallery as showcase;
   the SpacetimeDB module as a second consumer.
4. Then chase gaming explicitly: the physics extension aligned with
   OMI/KHR drafts, a save-game example, a mod example.

## Risks, written on the wall

- **Covering everything** kills credibility — profiles and non-goals are
  the defense.
- **Vendor branding** kills trust — the neutral home is the defense.
- **Bit-exactness promises** kill implementers — semantic replay is the
  defense.
- A fourth, quieter one: **spec maintenance is a job**. Two renderers
  must pass the suite on every change forever; if that cost isn't
  accepted, don't publish the spec — the gallery's "portable, open
  formats" claim survives on world-types being *renderable elsewhere*,
  not on a governance process we abandon.

## Implementation status

Exists today: world-types (schema 3, JSON Schema generation, conformance
worlds, validation, two renderers), the session package RFC, the ops
protocol and authority, content-hashed assets, the in-browser gallery.
Not started: the typed state document, profiles/extensions mechanism,
the neutral repo and site.
