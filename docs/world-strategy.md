# localgpt.world strategy

**Status:** decisions taken 2026-09-24. As of 2026-09-25 the format, the
shared renderers and the conformance suite are built (§5.3, §11.1), and the
static showcase — step 2 of §11 — is live on the site (§12).
[§13](#13-the-consumer-pivot) (2026-09-27) revises §1 and §12 for a
consumer-first product; where it and an earlier section disagree, §13 is later.
**Scope:** turning the placeholder at localgpt.world into a site backed by a
SpacetimeDB world that LocalGPT Gen hosts write into, with the content shown
on the web to attract users.

Where this document cites code it names the repository and path; line
numbers are as of the commits listed under [Sources](#sources).

## 1. Decisions

| Question | Decision |
|---|---|
| Who pays for LLM inference | The creator, from their own Gen app with their own model. No public inference worker in v1. |
| Hosting | SpacetimeDB Maincloud (not self-hosted). |
| Chat | Not in v1. Presence is a visitor count plus optional display names. |
| MD worlds | In v1, as a static publish (decks first), with the guardrails in [§3.2](#32-localgpt-md-in-v1-static-publish). |
| Verse worlds | In v1 for the CC0 starter pack, as enterable worlds with their audio ([§3.3](#33-localgpt-verse-showcase-in-v1)). Verse exports its world in the shared format (soundtrack curves and `ModulationDef`s, never the audio file or the embedding); personal songs stay a local-first sidecar exchange, never a server. |
| Format | `localgpt-world-types` is the source format for Gen, MD and Verse; glTF is the distribution export ([§5](#5-one-format-for-gen-md-and-verse)). |

## 2. Shape

```
visitor ─► localgpt.world (Cloudflare Workers static assets)
            ├─ curated world pages: poster or video loop, 3D viewer loaded on click
            └─ world snapshots on R2, named by content hash
        ─► "Enter live world" ─► SpacetimeDB Maincloud ◄── creator's Gen (headless or windowed)
                                   anonymous = read-only        commits each build as one batch
        ─► sign in ─► claim a plot, edit it from your own Gen

creator ─► local Gen / MD ─► explicit Publish ─► R2 (+ SpacetimeDB for live plots)
```

- Browsers can't join Gen's `--host` sessions (lightyear over UDP). `--host`
  stays the LAN and private-session path; SpacetimeDB is the only cloud path.
  Both already speak `localgpt-world-types`, and the multiplayer doc kept the
  seams for this ("replication events translate to reducer calls one-for-one";
  `localgpt/docs/gen/multiplayer.md`).
- Under creator-pays there is no worker fleet, so the cloud job queue
  (`localgpt/crates/spacetime/src/jobs.rs`) is out of v1 scope. A good v1.5:
  visitors queue prompt requests that the plot owner's Gen fulfils when it is
  online. The owner pays, and the owner can turn it off. That is the
  listen-server model over SpacetimeDB.

## 3. Content sources

| Source | v1 | How |
|---|---|---|
| Gen plots | Live | Creator's Gen commits batches to SpacetimeDB with the creator's model |
| Gen snapshots | Static | RON/GLB on R2, rendered by the same web viewer |
| MD decks and docs | Static | Manifest plus optional captions, explicit publish with preview |
| Verse | Curated | The CC0 starter pack's worlds, with their audio, as enterable worlds on the site |
| Verse user songs | Deferred | Local-first sidecar exchange, no server |

### 3.1 LocalGPT Gen

The main source. Everything a creator builds locally can be published as a
snapshot; a claimed plot is edited live from the creator's own Gen. The
scene-only remote tool set (`localgpt/crates/gen/src/net/remote_scope.rs`)
is the right tool surface for anything that runs on behalf of another user.

### 3.2 LocalGPT MD: in v1, static publish

Why it is cheap and safe to include:

- **Already on world-types.** `draft::compile` emits a `WorldManifest`,
  `--print-ron` prints it, and a test enforces Gen's `validate`. The web viewer
  built for Gen renders MD worlds as-is; the only new code is an upload path.
- **Zero inference on the server.** The sidecar (`notes.world.json`) holds
  clamped recipe numbers keyed by section hash, and "a `.md` plus its sidecar
  renders identically on any machine, even in the default (no-`llm`) build"
  (`localgpt-md/README.md`). Publishing costs kilobytes on R2.
- **What is actually uploaded.** The manifest carries the document title, the
  first line of the intro, and the section headings as tour waypoints
  (`localgpt-md/src/draft.rs:60–63`, `:120–121`). Body text is not in it;
  captions are excerpted from the `.md` at runtime (`localgpt-md/src/tour.rs:187`).
  So a publish is the manifest, plus captions as an explicit checkbox.

Guardrails:

- Publishing is a separate one-shot action. It is never wired to the file
  watcher that rebuilds the world on save.
- The publish preview shows exactly the title, intro line, headings and any
  caption text that will be uploaded.
- Lead with the `deck` genre: talks are made for audiences, notes are not.
  Allow `world` too, with captions off by default.

The "local model for private use" positioning is orthogonal: the model runs
locally before and after, and publishing is the only thing that leaves the
machine.

### 3.3 LocalGPT Verse: showcase in v1

- **Now on world-types.** `localgpt-verse/Cargo.toml:43` depends on it, and
  `localgpt-verse/src/world_manifest.rs` exports the whole world: the mood
  palette, the analysis sidecar as a `SoundtrackDef` (BPM, beat grid,
  sections, the energy and stem curves), and `ModulationDef`s that bind
  entities to it. A test (`manifest_validates_and_performs`) proves the CLAP
  embedding never leaves. The web viewer already plays a world's soundtrack
  (`website/viewer/world-viewer.js:649`).
- **The world is a performance.** The live audio tap and stem curves drive it.
  Without the song it is a diorama, and a visitor's copy of a copyrighted song
  cannot be streamed to other visitors. That is the hard stop.
- **The sidecar is mostly fine, with two exceptions.** `TrackAnalysis`
  (`localgpt-verse/src/analysis.rs:61`) holds BPM, beat offset, section
  boundaries, a per-second energy curve, spectral centroid, loudness and
  Demucs per-second stem envelopes. Those are analysis features, not the
  recording: low risk. But the CLAP embedding is computed with CC-BY-NC weights
  (`localgpt-verse/README.md`), so strip it from anything public. And a public
  gallery of "worlds for *\<hit song\>*" is exactly what a rights holder
  notices, even when nothing infringing is served.
- **What is hosted with audio:** the four original CC0 tracks in
  `localgpt-verse-assets/music/` (and the 171 CC0 Poly Haven models). Only a
  track whose folder's `music.json` licenses it ships its audio; personal
  libraries export with no audio path, and the world performs silently from
  its curves anywhere.
- **Later, local-first:** an export/import of the sidecar (recipe + build +
  analysis, minus the embedding) so a friend with the same track gets the same
  world locally. No audio, no server. Do not build a content-hash registry: it
  becomes a database of who owns which songs.

## 4. Where the Verse showcase lives

The CC0 Verse worlds live **on localgpt.world** as proper worlds — enterable,
with their audio — because the format now carries the performance. That was
the original condition for moving them here ("once Verse is ported to
world-types"), and it is met. verse.localgpt.app keeps the *listen and watch*
pitch and the app's own page; localgpt.world is *walk and build*.

The earlier plan put a video-only showcase on verse.localgpt.app instead; the
port removed the reason for it. Keeping the music's license story explicit
(the CC0 pack) is what keeps localgpt.world's legal surface small, so the
audio on each Verse world page names its license.

## 5. One format for Gen, MD and Verse

**Recommendation: standardize on `localgpt-world-types` as the source format
for all three apps, and on glTF 2.0 as the distribution export.** Do not invent
a second internal format, and do not try to make a public standard the
authoring format.

### 5.1 Why world-types as the source

- It exists, is published, is serde-only (native, WASM, iOS, Android), carries
  a schema version, and has validation (`validate_entities`, `WorldLimits`).
- It is the LLM-facing layer: parametric shapes that "never degrade to raw
  triangles", human-readable entity names, declarative behaviors, procedural
  audio, tours, avatar. No public standard covers that layer.
- Two producers already emit it (Gen, MD) and the SpacetimeDB rows are shaped
  by it. Verse is the third producer, which is what makes localgpt.world's
  "portable, open formats" claim true.

Verse's one addition is in: signal-driven modulation. `ModulationDef {
target, signal, range, smoothing }` where `signal` is one of `energy`,
`bass`, `highs`, `beat`, `stem(drums|bass|vocals|other)` (world-types
`modulation.rs`). That keeps Verse's reactivity declarative and renderable on
the web (the web viewer derives the same signals from the CC0 tracks; for a
user's own songs the signals come from local analysis and never leave the
machine). Keep world-types small; add
things when a second app needs them.

### 5.2 Existing standards, and where each fits

| Standard | What it covers | Fit |
|---|---|---|
| **glTF 2.0** (Khronos, ISO/IEC 12113) | Meshes, PBR materials, hierarchy, animation, cameras; lights via `KHR_lights_punctual` | The distribution format. Renders in Bevy (`bevy_gltf`) and the web (three.js `GLTFLoader`), and opens in Blender, Unity, Godot. Gen's `gen_export_gltf` already targets it. Parametric shapes bake to meshes. |
| `KHR_interactivity` | Behavior graphs inside glTF | Submitted for ratification July 2026; runtime support is still arriving. Map world-types behaviors to it when runtimes catch up; bake to animations until then. |
| `KHR_audio_emitter` | Spatial/ambient audio in glTF | Draft. Sample-based, so procedural (FunDSP) ambience has no standard home; ship a rendered sample in the export. |
| OMI extensions (`OMI_physics_body`, `OMI_physics_shape`, `OMI_spawn_point`, `OMI_link`, …) | Physics, spawn points, portals for virtual worlds | Stage proposals from the Open Metaverse Interoperability Group; useful targets for the export, not a source format. |
| VRM | Avatars on glTF | Relevant if user avatars arrive. |
| glTF External Reference Format (glXF) | Composing several glTF assets into a scene | Not a specification; the published documents are marked obsolete, with a rewrite anticipated. Watch, don't adopt. |
| **OpenUSD** | Authoring and composition (film, Omniverse, Apple's USDZ) | No mature Rust or web runtime. Already planned as an export (`localgpt/docs/gen/usd-export.md`), correctly as a converter step after glTF. |
| Bevy scenes (`DynamicScene` RON, BSN) | Bevy's own scene format | Bevy-only. Not web-renderable, so not a candidate. |

The pattern is the one Substance uses (`.sbs` source, `.sbsar` compiled) and
the repository's own three-tier RFC describes: the parametric spec is the
source; instances and exports are derived and never edited. A third tier is
proposed: publishing world-types plus the session package as a public
`.world` specification others can implement
(`docs/rfcs/world/world-format-spec.md`).

### 5.3 Consistent rendering on Bevy and the web

**Status (2026-09-24): built.** `localgpt-world-bevy` is the one Bevy mapping
(Gen and MD call it; Verse exports through world-types), `localgpt-world-export`
holds the one web viewer and the HTML export, the conformance worlds live in
`localgpt/crates/world-types/conformance/` and are served here from
`website/worlds/`, and `world.schema.json` is generated from the types. Left: the
SpacetimeDB web client (`apps/web`) still has its own renderer, and the
screenshot comparison between Bevy and the web viewer is not automated yet.

Consistency is a testing problem, not a format problem. Before this work
there were four renderers of world-types-shaped data: Gen's Bevy plugin
(`localgpt/crates/gen/src/gen3d/plugin.rs`), MD's `scene.rs` (kept in sync by
hand, per its CLAUDE.md), Gen's HTML export (three.js,
`localgpt/crates/gen/src/gen3d/html_export.rs`) and the web client
(`localgpt/apps/web/client/src/components/World3D.tsx`). Reduce that to two:

1. **One Bevy renderer crate** (the shared runtime MD's PLAN M5 calls for),
   used by Gen, MD and Verse.
2. **One web viewer package**, extracted from `html_export.rs`, used by the
   site for both static manifests and live SpacetimeDB rows. The live path
   has to render world-types directly anyway, so rendering glTF on the site
   would only add a second code path.
3. **A conformance suite**: a set of reference manifests (one per shape,
   material property, light type, behavior, tour) rendered by both renderers
   from fixed camera positions and compared as images. MD already has
   offscreen screenshots (`LOCALGPT_MD_SCREENSHOT`), Gen has headless
   screenshots, and Playwright covers the web viewer.
4. **A JSON Schema** generated from world-types, used for validation on
   publish and in LLM prompts.

glTF stays the export for third parties: it is what makes a world usable
outside LocalGPT and what keeps world-types from ever trapping content.

## 6. Cost

| Item | Cost | Notes |
|---|---|---|
| Site and snapshots | ~$0 | Requests to Workers static assets are free and unlimited. R2 Standard is $0.015/GB-month with free egress; the free tier covers 10 GB-month. |
| SpacetimeDB | Free tier, then Pro at $25/month | Free: 2,500 TeV/month (about 3M reducer calls, 12.5 GB egress or 1 GB storage). Pro: 100,000 TeV/month, overage at 2,592 TeV per dollar. Derived: about $0.32 per 1M reducer calls, $0.077/GB egress, $0.96/GB-month storage. |
| LLM inference | Paid by creators | See below for what a creator should expect. |

Run rate for v1 is therefore roughly Maincloud Pro plus R2 pennies. Take Pro
before launch for the SLA; the free tier pauses idle databases and resumes in
under a second, which is fine while building.

### 6.1 What a creator pays per Gen prompt

Gen's 108 tool definitions are about 70K characters, roughly 20K tokens, and
are resent on every turn. Assumptions: 15 turns per prompt, 1.5K tokens of
history added per turn, 600 output tokens per turn, prompt caching at 0.1×
input for reads and 1.25× for writes. These are estimates; measure real usage
before quoting them to creators.

| Model | With caching | Without caching |
|---|---|---|
| Haiku 4.5 ($1 / $5 per MTok) | ~$0.16 | ~$0.60 |
| Sonnet 5 ($2 / $10) | ~$0.32 | ~$1.20 |
| Opus 5.5 ($4 / $20) | ~$0.52 | ~$2.40 |

Trimming the tool set to the scene-only subset takes Sonnet 5 from about $0.32
to $0.23 per prompt in the same model. Local models via Ollama cost nothing
but must be tested on Gen's full tool loop first: MD and Verse only ask their
8B model for a single JSON recipe, which is a far easier task.

Do not run anything public on `claude-cli/opus` (the default): it uses a
personal Claude login. A service should use API keys on commercial terms.

### 6.2 Presence is the SpacetimeDB cost trap

The current web client subscribes every visitor to the whole `player` table
(`localgpt/apps/web/client/src/hooks/useSpacetime.ts:105–108`), so each move
goes to everyone and cost grows with the square of the player count.
Assuming ~150 bytes per update:

| Design | 100 concurrent | 1,000 concurrent |
|---|---|---|
| Everyone moves at 10 Hz, everyone sees everyone | ~$5/h | ~$430/h |
| 20% walk at 4 Hz, each update reaches ≤20 nearby players | ~$0.16/h | ~$1.60/h |

Visitors who only watch should make no reducer calls at all.

## 7. Performance

- **First load:** no WebGL and no WebSocket until the visitor asks. Show a
  poster or video loop; load the viewer on click.
- **Loading worlds:** per-chunk snapshots from the CDN, then subscribe only to
  changes since the snapshot. Gen's content-addressed asset store was designed
  so that "the host's server can be replaced by a real CDN without protocol
  changes".
- **Nearby-only subscriptions** need a btree index on `(chunk_x, chunk_y)`,
  which the module does not have yet. Keep frequently updated player positions
  in a separate table from world objects.
- **One transaction per build:** a `commit_batch(plot, ops)` reducer instead of
  many `spawn_entity` calls, so viewers never see half a castle, the batch is
  validated as a whole, and moderators can roll back one build as a unit.
- **Same look everywhere:** the conformance suite in [§5.3](#53-consistent-rendering-on-bevy-and-the-web).

## 8. Privacy

- **Positioning first.** localgpt.app promises "All data stays on your machine
  — no cloud storage, no telemetry", and the placeholder says "your machine
  generates the meshes". Present localgpt.world as the opt-in public layer:
  nothing leaves a machine until the creator presses Publish, and the Publish
  dialog previews exactly what will be uploaded.
- **Public tables are readable by any client.** Today that includes every
  player's position, online flag and last-seen time (`player`), all chat
  forever (`chat_message`), and every prompt's text tied to a stable identity
  (`prompt_job`). Make these private and expose only needed fields through
  2.x `#[view]`s. Do not rely on row-level security: in `spacetimedb` 2.10.1,
  `client_visibility_filter` is behind the `unstable` feature and documented as
  "currently unimplemented, and are not enforced".
- **Retention:** delete old rows with scheduled reducers. Today
  `prune_terminal` only runs when someone submits a prompt
  (`jobs.rs:202`).
- **Identity:** the anonymous identity token persists in the browser and acts
  as a pseudonymous ID. Connect only when a visitor enters the live world.
  Require sign-in (SpacetimeAuth or OIDC) only for writes.
- **Third parties:** Maincloud sees IP addresses and all rows, and its hosting
  region is not published; name it in the privacy page. The creator's LLM
  provider sees the creator's prompts, not other users' data.
- **No third-party requests on the site.** The site self-hosts three.js
  (`website/vendor/three/`). Gen's standalone HTML export still loads it from unpkg
  by default (a single file has to work from disk); `ExportOptions::three_base_url`
  points it elsewhere. Count usage in the database rather than adding analytics.

## 9. Security, abuse, moderation, legal

- Every reducer that changes the world checks the caller (plot owner, admin,
  or a registered worker). Validate content with world-types'
  `validate_entities` / `WorldLimits`; the module already depends on
  world-types. Cap string sizes: storage is billed and held in memory.
- Anonymous identities are free to create, so writes need sign-in.
- Anything that runs on behalf of another user uses the scene-only tool set,
  never `--remote-tools full`, in a locked-down container (reuse the
  `read_only` / `cap_drop` pattern from `localgpt/docker-compose.yml`).
- Enforce plot boundaries in reducers, not in prompts: other users' entity
  names end up in a worker's context and could steer it.
- Moderation: report, hide, and roll back by build. Terms of service, a
  content policy, and a DMCA/DSA takedown process before launch.
- Licensing: self-hosted SpacetimeDB is BSL 1.1 (one production instance, no
  database-as-a-service use, converts to AGPL-3.0 with a linking exception on
  2031-09-15). Moot on Maincloud; relevant if that decision changes.

## 10. Code changes required before anything is public

The module type-checks against `spacetimedb` 2.10.1, so these are logic
problems, not build errors.

| Where | Problem | Impact |
|---|---|---|
| `localgpt/crates/spacetime/src/lib.rs:116`, `:157` | `__identity_connected` / `__identity_disconnected` are plain reducers. 2.x runs lifecycle hooks only when declared `#[reducer(client_connected)]` / `#[reducer(client_disconnected)]`. | Player rows are never created, so moves and renames do nothing; the world is never generated on first connect; `jobs::on_disconnect` never runs. |
| `lib.rs:349`, `:391`, `:396`, `:414` | `spawn_entity`, `remove_entity`, `regenerate_world`, `clear_world` have no caller check and no limits. | Any visitor can wipe or flood the world. |
| `lib.rs:25`, `:65`, `jobs.rs:64` | `player`, `chat_message`, `prompt_job` are public tables. | See §8. Drop `chat_message` entirely (no chat in v1). |
| `lib.rs:40` | The entity row has a single uniform `scale` and no `parent`, `visible` or `mesh_asset`; no environment table; no `modify_entity`. | Gen worlds don't survive a round trip through the database. |
| `localgpt/apps/web/client/package.json:14` | Depends on `@clockworklabs/spacetimedb-sdk ^2.0.2`, which doesn't exist; the package is deprecated at 2.0.0 in favour of `spacetimedb` (2.10.1). The client also uses the pre-1.0 API and subscribes to whole tables. | `npm install` fails; the client needs a rewrite on the extracted web viewer. |
| `localgpt/crates/gen/src` | No SpacetimeDB client code. | The bridge that lets a creator's Gen commit to a plot doesn't exist yet. |

## 11. Order of work

1. **Unify the format and its distribution first (done 2026-09-24).**
   world-types carries modulation and soundtrack, one Bevy mapping crate, one
   web viewer crate, conformance worlds, a JSON Schema; Gen, MD and Verse all
   produce the format and MD renders through the shared crate. Remaining:
   automate the Bevy-vs-web screenshot comparison, port `apps/web` to the
   shared viewer, publish the crates to crates.io and drop the git pins in
   MD and Verse.
2. **Static showcase, no backend (live 2026-09-25).** Seven curated worlds —
   three Gen scenes, two MD walks and the two CC0 Verse tracks — each on its
   own page with a poster, its title and the one command to keep building on
   it in Gen (`localgpt-gen --world <url>`). This tells us whether the content
   draws people before we pay for live infrastructure.
3. **Live world.** Fix the module (§10), rebuild the web client on the shared
   viewer, build the Gen → SpacetimeDB bridge. Launch one public plaza where
   anonymous visitors watch and signed-in creators build from their own Gen.
4. **Creators.** Plots, MD publish flow with preview, moderation tools.
   v1.5: visitor prompt requests fulfilled by the plot owner's Gen.
5. **Verse, fully.** All four CC0 tracks, the modulation extension's remaining
   performance, and the local-first sidecar exchange for personal songs.
6. **Scale.** Split across instances only when one is actually full.

## 12. Sites and domains

Six public sites exist today, built three ways and hosted two ways:

| Domain | Lives in | Stack | Holds today |
|--------|----------|-------|-------------|
| localgpt.app | `localgpt/website` | Docusaurus, GitHub Pages | core docs, Gen docs, blog, templates, Apps menu |
| gen.localgpt.app | `localgpt/website-gen` | static HTML, Cloudflare | landing; docs link to the hub |
| verse.localgpt.app | `localgpt-verse/website` | static HTML, Cloudflare | landing; its docs moved to the hub |
| localgpt.md | `localgpt-md/website` | static HTML, Cloudflare | vanity redirect to `md.localgpt.app` |
| localgpt.world | `localgpt-world` | static HTML + viewer, Cloudflare | the world gallery and viewer |
| localgpt.rs | `localgpt-rs` | Zola, Cloudflare | engineering devlog |

The two old inconsistencies are fixed: the MD landing's canonical URL is
`md.localgpt.app` (and `localgpt.md` 301s there), and the hub's tagline now
describes the assistant. Verse's four docs pages and MD's README docs live in
the hub at `/docs/verse` and `/docs/md`, next to `/docs/gen`.

### 12.1 Target assignment

- **localgpt.app is the hub and the only docs site.** Brand, family overview,
  downloads, the assistant's docs, and `/docs/gen`, `/docs/verse`, `/docs/md`
  side by side with one search and one sidebar. Gen already does this; Verse's
  four pages and MD's README move here. The blog stays announcements only.
- **Product subdomains are landing pages.** gen., verse., md.: hero, video,
  features, download, one "Read the docs" link into the hub. They live next to
  the code they describe and change with releases. `localgpt.md` stays as a
  vanity front door that redirects (301) to `md.localgpt.app`, so the family
  reads gen / verse / md and search authority stays under one apex.
- **localgpt.world is the destination, not a docs site.** Everything on it is
  a world you can enter; creator spaces follow the plan above. It links to the
  hub for everything else.
- **localgpt.rs stays separate.** Its audience is Rust developers, its voice is
  first person, and its posts would dilute the product blog. The hub blog
  links to it under "engineering notes".
- **One family strip on every site**, same order and one-liners
  (LocalGPT, Gen, Verse, MD, World, devlog); today each footer lists a
  different subset.

### 12.2 Repositories and stacks

Keep landing pages with their code (they are release material), keep the
docs in the hub (one build, one search, cross-links between apps), and keep
`localgpt-world` and `localgpt-rs` as their own repositories (own toolchains,
own cadence). A sites monorepo would add a step for every product change
without removing one. The only stack change worth making: the five
Cloudflare sites deploy by hand (`deploy.sh`) while the hub deploys from CI;
a `wrangler deploy` workflow on push to main, with a Cloudflare API token as
a repository secret, brings them level.

## 13. The consumer pivot

**Status:** decisions taken 2026-09-27, revising §1 and §12. Where this
section and an earlier one disagree, this one is later.

§1–§12 describe a family of apps whose user installs a binary, brings a model
and pays for their own inference. Every decision in them is sound for that
user, who is a developer. A consumer will not download 5 GB before seeing
anything, will not run a host, and cannot choose between three apps that all
promise "AI makes you a 3D world". This section states what changes.

What does not change, because it is what the rest of this section leans on:
`localgpt-world-types` stays the source format (§5), localgpt.app stays the
only docs site (§12.1), localgpt.rs stays separate, and the conformance suite
stays the guarantee that a world renders the same everywhere (§5.3).

### 13.1 Decisions

| Question | Decision |
|---|---|
| How many apps ship | **Revised (2026-09-28): one desktop app.** All four use cases — assistant, Gen, MD, Verse — are modes of a single LocalGPT desktop app, not two products or a launcher. Supersedes the Worlds/Gen split below it in priority; §13.2's naming note follows. |
| Where generation happens | Deterministic first, on-device second, local GGUF last ([§13.3](#133-generation-tiers)). Link-first, not local-first. |
| What the product hands a user | A link: `localgpt.world/w/<hash>`. The world is the unit; the apps are editors of it. |
| localgpt.md | No longer a 301 to `md.localgpt.app`. It becomes a zero-install, no-model utility ([§13.4](#134-localgptmd-the-developer-wedge)), reversing §12.1. |
| Multiplayer in v1 | Out. A shared link carries most of the social value of a shared room at a fraction of the cost; `world-sync` and `localgpt-relay` ship after. |
| Live SpacetimeDB plots in v1 | Out. Static R2 snapshots plus posters prove the loop first (§3). |
| The assistant inside the desktop app | **Revised (2026-09-28): a mode.** The one app is `localgpt-app` (`LocalGPT.app`); there is no separate Worlds binary, so "absent from Worlds" no longer describes anything. Its assistant mode is the assistant itself: chat over the assistant's own `config.toml`, memory workspace, POLICY and MCP servers, one turn at a time under the workspace lock like `localgpt chat`. The autonomous half — heartbeat, cron, dreaming, bridges, the HTTP server — stays in `localgpt daemon`; the app does not run it. The world modes keep Gen's own settings and workspace, so nothing from the assistant's memory reaches a world or a share link unless the user puts it there. |
| Verse's `ml` tier in a paid build | Out. LAION CLAP is CC-BY-NC-4.0, so the tier it powers — semantic moods and asset-selection ranking — ships only while the app is free. HT-Demucs (MIT) is **not** a substitute: it separates stems, it cannot tell that a track sounds aggressive. Without `ml` the rule-derived mood stands, which §13.3 already calls a finished world for a song. |
| Android renderer | The web viewer in a WebView. One renderer per tier: Bevy on desktop, RealityKit on iOS, three.js on Android and web. |
| A desktop "hub" app that downloads and launches the others | **Rejected** (2026-09-28). The one app's Open… is the hub; downloads live on localgpt.app. See below. |

**Why no hub app.** Four use cases (assistant, Gen, MD, Verse) producing four
desktop things is a real problem, and the launcher is the one answer that
makes it worse: one more native app — three platforms, signing, notarization,
an update protocol and a catalog, on top of the per-app release pipeline of
§13.7 that does not exist yet — whose first-run experience is a screen of apps
the user does not have yet (the launcher cold-start, with no catalog). It
would also be scaffolding around a demolition: MD and Verse are scheduled to
merge into Worlds, so it manages a state this section is trying to end, and it
was floated under the `localgpt.world` name, which is the destination — the
gallery and every share link — not a store (the TLD-is-the-noun rule, §13.5).

Each underlying need already has a cheaper home: discovery is localgpt.md
(zero install, live) and localgpt.app's download page; using MD *and* Verse is
Worlds, one binary with two doors; prompt→world is Worlds' prompt door at the
core tool profile; updates are one release pipeline. And the audio work
removed the last *technical* reason Gen was a separate binary — if Worlds ever
grows a creator mode, there is nothing left to launch. If a hub-shaped itch
survives all that, the honest form is an "Apps" panel inside the assistant's
existing desktop GUI, after Worlds ships, if users ask — a link, not an app.

**One desktop app (2026-09-28).** The consolidation decision goes further than
the two-app split above: assistant, Gen, MD and Verse become modes of **one**
desktop app. This is not the launcher — no second binary, no catalog, no update
protocol; it is the opposite, the door picker inside a single window — and it
is newly cheap because the unifications landed: one workspace, one world
format, one renderer (`world-bevy`), one audio engine (`world-audio`, whose
existence removed the last technical reason Gen was a separate process). The
merge vehicle is gen's existing desktop shell (Bevy + an egui panel + a world
viewport + an agent loop), extended with a mode switcher — not a new shell.

The naming note in §13.2 ("Worlds" as the consumer app's own name) softens
with this: one app named **LocalGPT** with Gen/MD/Verse as its modes is a
*stronger* trademark posture (one consistent mark on one app, the Adobe/Raycast
shape) and retires the descriptive-name problem §13.2 flagged. localgpt.world
is unaffected: the destination, not an app.

MD's mode also carries a raised ambition, decided the same day: **live
authoring** — the user types Markdown, the LLM analyses each section as it
settles and rebuilds that section's place in the world, with a bidirectional
map between text and places (click a heading, fly to its region; click a
region, scroll to its section) and a hierarchy view of the document's
structure. MD's existing architecture is already most of this: sections hash
independently, a changed section regenerates alone, results upgrade in place
under stable entity ids, and generation runs off the main thread. What is
genuinely new is the in-app editor pane (egui `TextEdit` — gen already embeds
egui), a typing debounce feeding the existing worker, and the
navigation/hierarchy UI. That is product work, not an engine rewrite.

**Status (2026-09-28): the app exists; documents and songs open in it.**
`crates/app` (`localgpt-app`) is Gen's shell — Bevy window, egui prompt panel,
agent loop — assembled from the gen lib, with documents and songs as inputs:

- **Documents** (`--md`). An editor pane beside the viewport; the file saves
  and the world rebuilds 0.7 s after typing stops — in place, changing only
  the sections whose text changed (verified: an edit to one of three
  sections replaced 11 entities and kept 54, assets loaded). The app's own model
  authors each section through `world-agent`'s protocol — one request per
  section with a JSON plan, so it works on the zero-config default, a
  signed-in CLI backend, which ignores tool schemas. Builds land in MD's
  sidecar keyed by section hash. Verified with `claude-cli/opus` on a
  three-section document: three authored places in about 45 s; editing one
  section re-authored only that one (about 25 s) and kept the other two;
  a restart reopened all three from the cache with no model call; and the
  chat's stored CLI session was untouched (`make_ephemeral`). The text↔world
  map: the outline labels each section fence / model / authoring… / draft;
  the cursor moving into a section flies the camera there; a place card
  shows the model's description and what the place is made of, and picking
  a thing selects it in Gen's inspector. The UI interactions are
  unit-tested, not yet driven by hand.
- **Songs** (`--song`). Verse is now a lib plus a thin bin; `song_world`
  gives a song's world — analysis, mood, props, soundtrack curves and
  modulations — and Gen's viewport plays it: `world-bevy` gained the
  modulation runtime (the web viewer's semantics, applied only for
  rendering, so saves and shared sessions see authored values) and
  `world-audio` streams the soundtrack on the one mixer and reports its
  position as the modulation clock. Verified: a starter-pack song opens as
  its 178-entity world and plays; the modulation clock follows the song,
  and the heroes' rendered scale breathes with its energy. A small
  transport at the bottom of the window shows the song and pauses it.
- Two Gen bugs this surfaced, both fixed: reloading a world kept the old
  lights and skipped the new ones, and every mesh asset of a loaded world
  landed at the origin (gallery and saved worlds included).
- **The panel runs Gen's own agent loop** (`localgpt_gen::agent_loop`, moved
  out of `localgpt-gen`'s main): the model menu and `/model` remembering
  through Gen's settings, streamed turns with tool calls shown, the MCP relay
  a CLI backend reaches the window through, and hosting and joining
  collaborative sessions from the Collaborate section — all in the app, none
  of it duplicated. `--prompt "…"` starts a turn at launch.

Still to do: the assistant mode and the release pipeline of §13.7. Modes
switch in the window: an "Open…" button takes a document, a song or a world,
as the launch arguments do.

**Settled the same day, so the app has one shape to build toward:**

- *The one app is `localgpt-app`, and it includes the assistant.* The
  consumer app §13.2 called Worlds and the creator app it called Gen are
  both this binary; the table above records what the assistant mode reads.
  Only that mode reads the assistant's config and workspace — the world
  modes keep Gen's own settings (`gen-settings.json`) and `gen-workspace`,
  and each agent keeps its own CLI conversation (a Gen turn used to resume
  and overwrite the assistant's).
- *`LocalGPT.app` supersedes `LocalGPT Gen.app`.* `apps/gen-desktop/` is
  retired; `apps/app-desktop/` is the one bundler, with the Linux launcher
  beside it. `localgpt-gen` stays as a binary — the MCP server, headless
  generation, `--host`/`--join` — and `cargo install localgpt-gen` keeps
  working; there is no second desktop bundle. Nothing was lost to users:
  Gen's bundle was only ever ad-hoc signed. Gen's agent loop is
  `localgpt_gen::agent_loop` now, so the app's panel has everything Gen's
  had — slash commands, the model menu, the MCP relay a CLI backend needs,
  and hosting and joining from the window.
- *The app's toolbelt default is `core`* (§13.2), unless `--tools` or
  Gen's settings pick another; Gen alone keeps `full`.

### 13.2 The two names

**Superseded in part (2026-09-28): one app, one name.** There is one desktop
app, **LocalGPT** — the house mark on one product, which the trademark
reasoning below favours anyway. Gen, MD and Verse are its modes and remain
the names of their crates and binaries (`localgpt-gen` is still the MCP
server and headless generator). *Worlds* no longer names an app;
`localgpt.world` stays the gallery and share-link domain. What carries over
from the table is the consumer default: the app opens with the core tool
profile and a deterministic first world. The table is the two-app plan it
replaces:

| App | Name | Is | Lives at |
|---|---|---|---|
| Consumer | **Worlds** (`LocalGPT Worlds` where a publisher is needed) | Worlds from what you already have — a sentence, a document, a song. Core tool profile, deterministic default. | localgpt.world |
| Creator | **Gen** (`LocalGPT Gen`, unchanged) | Build a world tool by tool: the full 77-tool belt, the WorldGen pipeline, session hosting. | gen.localgpt.app |

**Worlds**, because the app's name, its domain and the noun it produces are
one word: nothing has to be explained and no brand has to be built. The
consumer says "Worlds", sees `localgpt.world`, and never parses "GPT".
**Gen** stays as it is — established, short, correctly signals what it does,
and renaming it would rewrite a domain, a landing page and `/docs/gen` for no
gain. The pair reads *Worlds makes a world out of something you have; Gen
builds one deliberately*.

**The trademark position decides this, and it points the same way.** A
`LOCALGPT` application is in prosecution, with the Supplemental Register a
likely outcome — the register for a mark held to be descriptive rather than
inherently distinctive. That placement still gives the ®, a USPTO record
examiners cite against later confusingly similar applications, and standing in
federal court; it does not give a presumption of validity or of exclusive
rights, constructive nationwide notice, incontestability, or Customs
recordation. After five years of continuous use it can be re-applied for on
the Principal Register under §2(f) as an acquired-distinctiveness claim, so it
is a waystation rather than a ceiling.

Three consequences for naming, all of which favour keeping LocalGPT as the
house mark rather than retiring it:

- **Concentrate use on one form.** An acquired-distinctiveness claim is built
  from consistent use, so the mark is `LocalGPT` everywhere — never
  "Local GPT", never lowercase in prose — and the products are
  `LocalGPT Worlds` and `LocalGPT Gen`. Keep dated specimens across the whole
  five-year runway; localgpt.rs is already that evidence trail.
- **Check the goods and services cover this pivot** while the application is
  still pending. A description written around assistant software with
  persistent memory does not obviously reach generating 3D environments or
  hosting an online world gallery, and amending or adding a class later costs
  more than getting it right now. One for the trademark attorney, not this
  document.
- **® goes on the registered mark only.** `Worlds` and `Gen` are descriptive
  sub-names carrying no registration of their own, which is normal under a
  house mark (Adobe Lightroom, Apple Notes) and is why neither needs to be
  distinctive. The practical protection for them is the domain set, not the
  register.

The residual cost is real and worth naming: `LocalGPT` says *local*, an
implementation detail §13.3 demotes, and *GPT*, which is someone else's, and
neither it nor `Worlds` is a strong mark. A defensible consumer brand would
need a distinctive word of its own, its own domain, its own search authority
and a Principal Register filing — an expense to take deliberately later, if
the consumer bet proves out, not by drift now while the `LOCALGPT` filing is
the asset being built.

### 13.3 Generation tiers

The 5 GB local model is the current road to a world and it is a launch
blocker: 4.9 GB sits in `~/.local/share/localgpt/models/llm`
(`localgpt/crates/core/src/paths.rs:268`) before a consumer has seen
anything. Three tiers, and the first is the default:

| Tier | Needs | Gives | When the user meets it |
|---|---|---|---|
| Rule-derived | nothing | a **finished** world for a song; an instant **draft** for a document | the first frame, always |
| On-device | Apple Intelligence | authored regions with real assets | first run on iOS |
| Local GGUF | ~5 GB download | authored regions with real assets | opt-in, framed as private and offline |

Both of the first two are already built, not work to schedule. MD's recipe is
"pure (no Bevy, no model), and always compiled"
(`localgpt-md/src/recipe.rs:5`); Verse's `world_recipe`/`world_mood`
(`localgpt-verse/src/analysis.rs:226-241`) are deterministic by design so a
kept world survives replay; and
`localgpt/apps/apple/LocalGPT/Services/AppleFoundationModelsService.swift`
already drives `SystemLanguageModel` with a world-tool loop.

The two rule-derived paths are **not** equal, and the difference sets the
cold start. Verse derives its world from the music itself — analysis, mood,
modulation — so a song's model-free world is a finished world. MD's draft is a
placeholder the model is meant to author over ([§13.4](#134-localgptmd-the-developer-wedge)).
So:

- **Desktop, no key, no download:** lead with **a song** — the one input whose
  model-free output is a world rather than a draft.
- **iOS:** lead with **a few spoken words** — the model is already on the
  device, free, with nothing to fetch.
- **A document is the mode that most wants a model**, so it is the wrong thing
  to put in front of someone who has not got one yet.

### 13.4 localgpt.md, the developer wedge

One page at `localgpt.md`: walk worlds MD authored from documents you know,
and drop or paste a Markdown file to see yours take shape in the same browser
tab. `md.localgpt.app` 301s to it —
the reverse of §12.1 — and that one page carries the hero and the download
call to action, so there is a single page to maintain instead of a landing
plus a redirect.

**What MD generates is LLM-authored 3D content.** Four tiers run per section,
first match winning (`localgpt-md/src/draft.rs` `compile_with`): a
```` ```world ```` fence's exact entities, then the **agent build**, then a
cached **recipe** restyle, then the rule-derived draft. The agent tier is the
product. `localgpt-world-agent`'s interpreter hands the model `place_asset`,
`scatter_field`, `spawn_primitive`, `set_light` and `environment` over a CC0
Poly Haven pack of 171 glTF models in 19 kinds — rock, tree, ruin, statue,
lamp, furniture, instrument, creature, machine and the rest — graded into
hero, medium and decorative tiers, so a section becomes scenery chosen for
what it says. `generation.rs` runs that off the main thread one section at a
time, and each region **upgrades in place** when its result lands, keyed to
stable entity ids so the tour stop you are standing at does not move.

**The rule-derived draft is the first frame, not the product.** It exists so
the document is walkable before the model has authored anything, and so a
cached sidecar replays without one. It is a pure function of structure and
text statistics, which is why it never fails on an unfamiliar file: sections
are `#`/`##` headings or `---` slides, and a document with no headings becomes
one section (`doc.rs`); each section takes a stable 64-bit seed from a hash of
its own content (`doc.rs:68`); regions land along a winding path 22 units
apart, each on a platform, with a ground strip, a sun and one tour stop per
section in order; and the geometry comes off the seed and the word count —
landmark kind `(seed >> 16) % 6`, height `2.5 + (words / 20).min(7)`, accent
colour through `hsl()`, prop count `3 + ((seed >> 24) % 5) + (words / 30).min(4)`,
primitives only (`draft.rs:318-448`). Nothing there reads the prose, so it
renders the *shape* of a document, never its meaning. It is a placeholder of
known quality, and the recipe tier — optional fields over that same draft
(`recipe.rs:22-33`) — is the degraded path when the agent chain fails, not the
model's real job.

**So the page must not lead with the draft.** It cannot run the agent tier:
no model in a browser, and no inference budget for anonymous visitors. It
therefore leads with worlds the agent tier **already authored** — a handful of
recognizable public documents (a well-known README, an RFC, a spec) generated
once in the app at full quality, each with a poster and a `/w/<hash>` link,
walkable at the ceiling of what MD does. The visitor's own dropped file gets
the rule-derived draft, labelled as the draft, with "generate the full world"
as the call to action into Worlds. Both ends are visible on one page, the
ceiling is the honest one, and per-visit cost stays zero.

Why this is the cheapest distribution the project has:

- **Nothing to install, no key, no model, no inference bill.** The draft tier
  is deterministic, so the page can be free forever and cannot be farmed for
  inference: no rate limit, no abuse surface, no per-visit cost. That
  combination is rare, and it is what makes a permanent public demo
  affordable.
- **The visitor already has the input.** Every developer has Markdown open
  right now — a README, an ADR, a docs page, an Obsidian vault. There is no
  use case to imagine and nothing to type; the distance from curious to
  having seen it work is one drag and drop.
- **The domain is the pitch.** `localgpt.md` *is* the file extension. It needs
  no tagline, and it ends the waste of 301'ing the best-named domain in the
  set into a subdomain nobody types.
- **The demo and the funnel are one artifact.** A link in a README, in the
  hub docs, in a post — the channel developers already use, and the honest
  answer to "show me" in any conversation about the format.
- **It proves §5.3 in public.** Same viewer (`website/viewer/world-viewer.js`),
  same format, same conformance scenes as the desktop apps, so the page is
  marketing and a conformance witness at once.
- **It measures the central unknown cheaply.** Whether "content becomes a
  place" appeals to anyone is the bet this section rests on, and the
  pre-generated worlds — not the draft — are what put the real thing in front
  of a free audience before Worlds ships.

**Status (2026-09-27, evening): built and verified.** `crates/md` is split
into a lib (doc/draft/recipe/sidecar/assets — pure, `--no-default-features`
compiles no Bevy) and the Bevy bin behind an `app` feature; `crates/md-web`
is the WASM wrapper, one function, depending on the lib with default features
off so nothing here can grow a renderer or an inference engine. The page
(`website-md/`) assembles like website-world — viewer from world-export,
three.js vendored, samples from the crate — and its check drops the app's own
`hello.md` through the real file input in headless Chromium and requires a
rendered world (4 sections, 40 entities, draft labelled, zero page errors).
It runs in the `viewer` CI job. Deploy: `website-md/deploy.sh`; still to do in
the dashboard — localgpt.md canonical, md.localgpt.app 301.

How to build the drop-a-file half: compile the section parser and the draft
(the fence and rule tiers — no agent, no model) to **WASM** —
both are pure, with no Bevy, no model and no tokio, so it is a small
wasm-bindgen target — and render with the viewer already on this site. A
Worker returning JSON is less code and would also work, but WASM wins for a
reason beyond cost: **the Markdown never leaves the browser**, which the page
should say plainly. Every competing "paste your document into our AI" tool
uploads it.

One deliberate limit: **no inference on this page.** That holds the cost at
zero, the abuse surface at nothing, and keeps the drop-a-file path working
offline once loaded. The LLM's output is still on the page — pre-generated,
in the worlds the visitor walks first — so the upgrade is a capability they
have already seen rather than a paywall over the same thing. If a hosted tier
exists later and inference is being paid for anyway, generating a dropped
file on the server becomes a small change to this page; it is not worth a
budget of its own before then.

One more thing the page has to get right: **`localgpt.md` reads as a
filename**, so many chat clients and Markdown linters will not autolink it.
Every README, post and docs page must write it as `https://localgpt.md`.

Where it leads: "keep building" → Worlds; "publish this" →
`localgpt.world/w/<hash>`; "how it works" → `/docs/md` in the hub.

### 13.5 What §12 becomes

| Domain | Job | Audience |
|---|---|---|
| localgpt.app | Software and the only docs site: hub, downloads, `/docs/*` | developers, evaluators |
| localgpt.world | Worlds' home, the gallery, and every share link: `/w/<hash>`, later `/r/<code>` | consumers |
| localgpt.md | The zero-install utility of §13.4 | developers, writers |
| localgpt.rs | Engineering devlog, unchanged | Rust developers |
| gen.localgpt.app | Gen's landing page, unchanged | creators |
| verse.localgpt.app, md.localgpt.app | 301s — to Worlds and to localgpt.md | — |

The TLD carries the noun: `.app` is what you install, `.world` is what you
make, `.md` is the file you already have, `.rs` is how it is built. Nobody
has to remember a mapping.

Two rules follow. **Share links live on a hostname the project owns**:
Worlds' share button must produce `localgpt.world/w/…`, never a `workers.dev`
host or a LAN address, and `localgpt-relay`'s rooms belong at
`localgpt.world/r/<code>` when they ship, so a world and a room are the same
kind of link to whoever receives one. **One canonical per surface**, with
everything else a 301 — the inconsistency §12 fixed once for MD, pointed the
other way.

The family strip (`website/index.html:869`) then lists five doors, not six —
LocalGPT, Worlds, Gen, the `.md` utility, devlog — and Verse and MD leave it,
because they are modes.

### 13.6 Surface reduction

The three apps are three clones with three Cargo workspaces pinned to
`localgpt` by git rev, and those seams now cost more than the code does:

- The revs have **already drifted**: `localgpt-md` on `db8eb57`,
  `localgpt-verse` on `04aa514` — the same four world crates at two
  versions. A shared format only holds if the apps agree on its version.
- The 522 MB CC0 pack is duplicated per repo (`localgpt-md/assets/models`,
  `localgpt-verse/assets/models`).
- Three mistral.rs loaders and loops, 321 + 547 + 585 lines
  (`localgpt-md/src/llm.rs`, `localgpt-verse/src/llm.rs`,
  `localgpt/crates/gen/src/local_llm.rs`), for one model.
- `shared_llm_dir` is hand-copied into `localgpt-verse/src/llm.rs:242` with a
  keep-in-sync comment.
- iOS carries a parallel world format — `WorldState`, `WorldEntity` and a
  bespoke seven-tool vocabulary in `WorldChatViewModel.swift`. §5 forbids
  that in the Rust crates and should forbid it in Swift too.

One workspace, with MD and Verse folded in, removes the first four. The
asset pack then moves to the shared directory beside the model, with the path
helper living in `world-types` — serde-only, already a dependency of all
three — instead of being copied. Mobile gets the format over UniFFI, which
`localgpt/crates/mobile-ffi` does not expose today, and the Swift lookalike
goes.

**`localgpt-world` merges too, and this repository is the proof.** Its viewer
is a hand-synced copy of `localgpt/crates/world-export/js/world-viewer.js`, and
on 2026-09-27 the copy was found 185 lines behind the original — 845 against
1030 — so the public site had been rendering with an older renderer than the
apps, quietly falsifying §5.3 on the one surface strangers see. The sync also
copied only the conformance JSONs and not their assets, so the texture scene
404'd. Both are fixed and `scripts/sync-viewer.sh` now carries
`conformance/assets/`, but the class of bug is the same as the git-rev pins:
a copy a human has to remember to refresh. Inside one repository the viewer is
referenced, not copied, and the script goes away. This revises §12.2, which
kept this repository separate for "own toolchain, own cadence" — the toolchain
is `python3 -m http.server` and wrangler, and the cadence argument cost 185
lines.

**The rule worth adopting: a separate repository is justified by binary weight
or by editorial cadence, never by "it is a different app."** That sorts the
family cleanly.

| Repository | Commits | Verdict |
|---|---|---|
| `localgpt` | 675 | the center |
| `localgpt-md` | 18 | merge — 3.3k lines and 18 commits is a crate, not a project |
| `localgpt-verse` | 85 | merge — becomes Worlds with MD folded in |
| `localgpt-world` | 13 | merge — the drift above is the argument |
| `localgpt-rs` | 4 | separate: editorial cadence, and Zola is its own toolchain |
| `localgpt-verse-assets` | 9 | separate (1.4 GB of LFS models), but **rename `localgpt-world-assets`** |
| `localgpt-assets` | 6 | separate as the brand archive; last touched 2026-03-19 and 52 MB of it is `logo-draft`. Move only the icons a build consumes into `localgpt` |

The rename matters beyond tidiness: three apps read that pack, MD falls back to
`../localgpt-verse-assets` by path (`localgpt-md/src/assets.rs`), and after the
fold "Verse" is not a product name. Consolidating also collapses three Cargo
workspaces, three `ci.yml`, three `deny.toml` and three hand-rolled release
paths (§13.7) into one each.

**Status (2026-09-27): the fold is done on `localgpt`'s
`refactor/one-workspace` branch.** MD and Verse are now `crates/md` and
`crates/verse` in the localgpt workspace — 17 members, no git-rev pins, their
landing pages at `website-md/` and `website-verse/` beside `website-gen/`, and
their CLAUDE.md files carried in as crate-scoped guides. The 522 MB pack was
not copied: `scripts/fetch-model.sh` and `scripts/fetch-assets.sh` now fetch
both shared downloads once per machine into
`~/.local/share/localgpt/models/{llm,pack}`, and a new
`localgpt-world-agent::paths` module owns the resolution rules that used to be
hand-copied: `shared_llm_dir` (three copies), the asset-pack probe (two), and
GGUF/tokenizer discovery (two weak copies replaced by Gen's sorted,
per-model-tokenizer version). It also absorbs the asset-repo rename by
accepting either name.

CI gained a **feature-tier job**, and it earned its place immediately. MD and
Verse each had one; the workspace did not, and Verse's own comment had said why
that matters — "without these gates the gated code could rot broken while
default-feature CI stays green." Pointing Gen at world-agent referenced a crate
Gen did not depend on, and no check caught it, because `local-llm` is not a
default feature and nothing built it. The job now checks MD's and Verse's
`llm`, Verse's `ml`, and Gen's `local-llm` — coverage Gen never had. The
lesson generalises past this branch: **every optional feature in this workspace
needs a CI job or it is not really compiled**, and the release pipeline of
§13.7 does not exist yet either.

Verified clean: `cargo test --workspace` (~1,440 tests), `cargo clippy
--workspace --all-targets`, `cargo fmt --check`, `cargo deny check`, each
feature tier, and `cargo run -p localgpt-md -- --print-ron` from the workspace
root. What remains for a later branch: `localgpt-world-audio`, the
MD-into-Verse merge that actually produces Worlds, and prompt mode.

**Audio is the one part of the format with no shared mapping, and Worlds
forces the issue.** `world-types` carries audio throughout — `AudioDef` per
entity (`entity.rs:92`), `AmbienceLayerDef` and `AudioEmitterSpec`
(`library.rs:28,37`), `SoundtrackDef` (`world.rs:75`), `ModulationDef` — but
`localgpt-world-bevy` contains none of it. Gen renders ambience and emitters
with its own FunDSP/cpal engine (`gen/src/gen3d/audio.rs`, 868 lines of
AudioEngine, AudioEmitter and a three-thread model); Verse renders soundtrack
and modulation with kira (`localgpt-verse/src/audio.rs`, 721 lines). Neither
can play the other's half, and Worlds has to play both: a document world has
ambience and emitters, a song world has a soundtrack.

They unify, and not by picking a winner — kira is a playback engine (mixer,
clocks, tweens, streaming decode, spatial tracks) and FunDSP is a synthesis
DSL, so the shape is **one kira `AudioManager` with the FunDSP graphs wrapped
as a source**. What goes is Gen's hand-rolled cpal engine and its distance
attenuation, not its graphs (`audio_graphs.rs`, 453 lines, survives). That
this works is already demonstrated here: Verse implements a custom
`kira::effect::Effect` (`TapEffect`, `audio.rs:601`) whose
`process(&mut [Frame], dt, _info)` is the same shape a FunDSP `Net` bridge
needs.

So a new crate, **`localgpt-world-audio`**, beside `world-bevy`: the one
mapping from the format's audio types to sound, depended on by Worlds and by
Gen. It retires the earlier reason for keeping Gen in its own process — with
one engine there is no device contention — but not the decision: Gen stays a
separate binary for its toolbelt, its user and its onboarding, which is the
seam that was always doing the work.

**Status (2026-09-27, later): built, and both apps are on it.** The crate is
`graphs::build` (the twelve FunDSP builders, keyed on `wt::AudioSource`, pure
and testable without a device), `FundspSound` (a graph as a kira voice:
retunes on a sample-rate mismatch, clamps non-finite samples, finishes when
its handle drops) and `Engine` (the `AudioManager`, ambience layers, spatial
emitters as distance-attenuating sub-tracks, master volume, `manager()` for a
caller's own playback layers). Gen's `gen3d/audio.rs` was rewritten onto it:
same public surface, but the cpal transport, the in-place Net rebuilds, the
per-frame attenuation and panning math are gone — `spatial_audio_update`
forwards transforms and kira does the rest — and fundsp and cpal left Gen's
Cargo.toml. Verse's `AudioInner` holds the Engine and adds its music sub-track
and tap effect through `manager()`, so soundtrack and ambience provably share
one mixer. Still open: mapping `AmbienceLayerDef`/`AudioEmitterSpec` from a
`WorldManifest` directly (both apps currently drive the Engine from their own
command paths), and `SoundtrackDef` playback as more than Verse's internal
layer.

**Status (2026-09-28, later): the mistral.rs loaders are one.** The two real
copies — MD's and Verse's load-a-GGUF-then-complete-with-a-timeout plumbing —
are `localgpt_world_agent::LocalGguf` behind the crate's existing `llm`
feature, with each app keeping its directory candidates, prompt and parse (the
bullet above counted three; Gen's `local_llm.rs` is not a copy but a full
LLMProvider with tool calls and the model menu, already resolved through
world-agent's paths). What §13.6 still owes and nothing has started: **the
iOS parallel format** — `apps/apple/LocalGPT/Models/WorldEntity.swift` (204
lines) and the two view models around it (1,360) still parse their own
Codable mirror instead of receiving the format. The path, when it is taken:
feature-gate `uniffi::Record`/`Enum` derives on the `world-types` structs
(the crate is serde-only and stays so without the feature), expose
parse/save of a `WorldManifest` through `crates/mobile-ffi`, then adopt the
generated Swift types in the view models — RealityKit rendering stays
Swift's; only the data model crosses. That is a branch of its own, and a
prerequisite for the iOS/RealityKit tier of §13.7's table.

### 13.7 Artifacts and delivery channels

Nothing below ships today. The repository carries `ci.yml` and
`deploy-website.yml` — there is **no release workflow**, and every artifact
that exists is hand-built. `apps/app-desktop/macos/build-app.sh` builds
`LocalGPT.app` (2026-09-28): ad-hoc signed by default, and with a Developer
ID signature, hardened runtime and notarization when given the credentials
(`APPLE_SIGNING_IDENTITY`, `APPLE_NOTARY_PROFILE`) — only its ad-hoc path
has been run, since no Developer ID is set up here. It builds with
`local-llm-metal` on Apple Silicon, so the bundle runs a GGUF from the
shared model folder (§13.3's third tier). It replaced
`apps/gen-desktop/`, whose `LocalGPT Gen.app` it supersedes, and
`crates/verse/scripts/bundle.sh` still assembles a portable `dist/`. One
workspace (§13.6) also means one release pipeline instead of three.

**LocalGPT — the one desktop app** (the Worlds and Gen apps of §13.2, merged)

| Platform | Artifact | Channel |
|---|---|---|
| macOS (Apple Silicon) | notarized `.app` in a `.dmg` | direct download from localgpt.app and localgpt.world; Homebrew cask for developers |
| Windows | signed `.msi` | direct download; winget |
| Linux | AppImage | direct download; Flatpak later (`bundle.sh` already anticipates it via `VERSE_ASSET_ROOT`) |
| macOS / Windows | the same build as a Steam app | Steam |
| iOS, iPadOS | `.ipa` | App Store |
| visionOS | `.ipa` | App Store — the Swift views already branch on `os(visionOS)` |
| Android | `.aab` | Play Store |

The local-GGUF tier on the desktop: `local-llm-metal` is macOS-only, so the
Windows and Linux artifacts need `local-llm` (CPU) or a GPU backend of
mistral.rs chosen per platform — undecided, and nothing builds them yet.

**Gen as a binary**

| Platform | Artifact | Channel |
|---|---|---|
| any | `localgpt-gen` — MCP server, headless generation, session host | `cargo install localgpt-gen`, already true; the MCP container image (`Dockerfile.gen`) for registries |

**LocalGPT — the assistant**

| Platform | Artifact | Channel |
|---|---|---|
| macOS, Linux, Windows | CLI/daemon binaries | GitHub Releases; `cargo install localgpt`; Homebrew formula |
| server | container image for the daemon and the relay | a registry, once the relay ships (§13.1 defers it) |

**Libraries — how the format spreads**

| Artifact | Channel |
|---|---|
| `world-types`, `world-bevy`, `world-export`, `world-agent`, `world-audio`, `world-sync` | crates.io — which also retires the git-rev pins of §13.6; the Cargo.toml comments already plan for it |
| the three.js viewer as a package | npm, so a world embeds in anyone's page |
| `LocalGPTWrapper` XCFramework, Android AAR | only if third parties need them; internal until then |

**Data — the things a store should not carry**

| Artifact | Size | Channel |
|---|---|---|
| Bonsai-8B Q4_K_M + tokenizer | ~5 GB | R2, resumable, on opt-in (§13.3); a Steam depot handles it natively |
| CC0 Poly Haven pack | 522 MB | R2 with its manifest, on first generate (§14). Git LFS in `localgpt-verse-assets` is the source of truth, not a delivery channel |
| HT-Demucs | — | fetched and cached by `stem-splitter-core` itself; `fetch-demucs.sh` only pre-warms |
| CLAP | — | opt-in only, and never in a paid build (§14) |
| a world | small | `.world.ron`/`.json`, a `/w/<hash>` link, a self-contained exported `.html`, or glTF for other tools |

Three judgements behind the table. **Steam deserves a serious look for a 3D
consumer app**: it is the one consumer channel where "a place you walk around
in" needs no explanation, and its depots solve the 5 GB model and 522 MB pack
delivery that an App Store makes painful — at 30%, against a notarized
download that costs nothing but reaches nobody who is not already looking.
**Direct download stays primary on macOS** rather than the Mac App Store,
because App Sandbox and review sit badly with an app that fetches a
multi-gigabyte model on demand. **iOS cannot take the GGUF at all**, which is
why the on-device tier of §13.3 is not a nicety there but the only path.

## 14. Open questions

- Plot size and how many a creator may claim.
- Whether snapshots publish as world-types RON only, or RON plus a baked GLB
  for third-party tools from day one.
- Sign-in provider: SpacetimeAuth alone, or an OIDC provider people already
  have accounts with.
- **How far the 171-model CC0 pack stretches.** Now that LLM-authored scenery
  is the product (§13.4) and not a restyle, the pack is a quality ceiling as
  much as a 522 MB download: 19 kinds cover rock, tree, ruin, statue, lamp,
  furniture and the like, and a section about database migrations resolves to
  none of them. Open: how gracefully `resolve_kind` should fall back to
  primitives, and whether the pack grows (more CC0 kinds, or generated meshes)
  before Worlds ships.
- **When the pack is fetched.** §13.3 defers the model to opt-in, but scenery
  needs the pack, so it cannot ride the same "later" as the GGUF — most
  likely on first generate rather than first run.
- **`localgpt-world-audio` scope and timing** (§13.6): whether the crate owns
  only ambience, emitters and soundtrack, or the modulation evaluation too;
  and whether kira 0.12's spatial-track and listener API covers what Gen's
  hand-rolled distance attenuation does today.
- Whether a permissively licensed audio-text embedding model can replace CLAP
  well enough to restore semantic moods to a paid build (§13.1 ships without
  it for now). Not a drop-in: `ml.rs`'s mel frontend is validated against
  Hugging Face's `ClapFeatureExtractor`, and the four mood embeddings would
  need regenerating.
- Whether `crates/verse/assets/ml/mood_text_embeddings.json` — 28 KB of CLAP
  *output*, committed in an Apache-2.0 repository and naming its own
  provenance — may be redistributed there. It is compiled only under `ml`
  (`mod ml` is cfg-gated), so it is in no default binary, but it is in the
  source. Whether model outputs inherit a model's licence is unsettled; one
  for counsel, with the trademark questions.

## Sources

Repositories, at the commits this document was written against (updated
2026-09-25, when the showcase shipped):
`localgpt` 45c170f, `localgpt-md` a529850, `localgpt-verse` 07350d6,
`localgpt-verse-assets` aeb1b09, `localgpt-world` 6b2f4ea.

- [SpacetimeDB pricing](https://spacetimedb.com/pricing) and the
  [pricing announcement](https://spacetimedb.com/blog/all-new-spacetimedb-pricing)
- [Maincloud docs](https://spacetimedb.com/docs/how-to/deploy/maincloud/)
- [SpacetimeDB license (BSL 1.1)](https://raw.githubusercontent.com/clockworklabs/SpacetimeDB/master/LICENSE.txt)
- [Cloudflare R2 pricing](https://developers.cloudflare.com/r2/pricing/) and
  [Workers static assets billing](https://developers.cloudflare.com/workers/static-assets/billing-and-limitations/)
- [glTF interactivity extension submitted for ratification](https://www.khronos.org/news/press/gltf-interactivity-extension-submitted-for-ratification)
- [KHR_audio_emitter pull request](https://github.com/KhronosGroup/glTF/pull/2137)
- [OMI glTF extensions](https://github.com/omigroup/gltf-extensions)
- [glTF External Reference Format](https://github.com/KhronosGroup/glTF-External-Reference)
- Anthropic model pricing as listed in the Claude API reference at the time of
  writing; cache multipliers 0.1× read, 1.25× write, Batch API 50% off.
