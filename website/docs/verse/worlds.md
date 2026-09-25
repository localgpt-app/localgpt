---
description: "The eight worlds of LocalGPT Verse — Ember Flats, Velvet Circuit, Tide Gardens, Glass Expanse, and their quadrant variants — and how a song finds, furnishes, and performs its world."
---

# Worlds & moods

*Eight worlds along an energy–valence continuum. The song decides which one you get — and how it performs.*

## How a song finds its world {#how}

The analysis pass measures each track's mean energy and valence and lands it in a quadrant — calm/bright, driving/bright, driving/dark, calm/dark. Each quadrant is a base world, and splitting the quadrants by energy yields eight: every base has a *variant* that borrows its assets — same neighbourhood, different hour.

- **Timbre** (the spectral centroid stored in the sidecar) nudges hues warmer or cooler inside the world.
- **MoodBlend** mixes in the nearest boundary world's palette when a track sits between quadrants.
- With the optional [`ml` feature](./index.md#ml), a CLAP model votes zero-shot on the mood instead.

## The eight worlds {#moods}

Each world contributes exactly one value to the chrome — its accent — while its sky, fog, and ground paint the backdrop. Arrangement is part of what a world *is*: how it lays out its ground props.

### <span aria-hidden="true" style={{display: 'inline-block', width: '0.8em', height: '0.8em', borderRadius: '50%', background: '#ffb38a', marginRight: '0.4em'}}></span>Ember Flats {#ember-flats}

The dawn chorus — warm ambers over a violet plain. Calm and bright. Props gather in **clumps separated by open ground**, outcrops on a plain.

### <span aria-hidden="true" style={{display: 'inline-block', width: '0.8em', height: '0.8em', borderRadius: '50%', background: '#62f5ff', marginRight: '0.4em'}}></span>Velvet Circuit {#velvet-circuit}

The neon surge — cyan over deep indigo and magenta. Driving and dark. Props arrange as a **jittered city grid**: blocks and streets.

### <span aria-hidden="true" style={{display: 'inline-block', width: '0.8em', height: '0.8em', borderRadius: '50%', background: '#a8d8de', marginRight: '0.4em'}}></span>Tide Gardens {#tide-gardens}

The night bloom — cool aqua over near-black water. Calm and dark. Props follow **meandering tidal terraces**, rows riding a slow wave.

### <span aria-hidden="true" style={{display: 'inline-block', width: '0.8em', height: '0.8em', borderRadius: '50%', background: '#8ef4ff', marginRight: '0.4em'}}></span>Glass Expanse {#glass-expanse}

The glass runner — pale ice over violet. Props form **concentric rings** with crystal symmetry.

### <span aria-hidden="true" style={{display: 'inline-block', width: '0.8em', height: '0.8em', borderRadius: '50%', background: '#e86a4a', marginRight: '0.4em'}}></span>Cinder Reach {#cinder-reach}

Ember Flats, smoldering — driving, dark, *restrained*. Same clumped outcrops, burnt down to ember orange and ash.

### <span aria-hidden="true" style={{display: 'inline-block', width: '0.8em', height: '0.8em', borderRadius: '50%', background: '#b7f5ff', marginRight: '0.4em'}}></span>Mirage Circuit {#mirage-circuit}

Velvet Circuit bleached by daylight haze — driving, bright, restrained. The city grid under a washed, pale sky.

### <span aria-hidden="true" style={{display: 'inline-block', width: '0.8em', height: '0.8em', borderRadius: '50%', background: '#6fa8b8', marginRight: '0.4em'}}></span>Abyss Terraces {#abyss-terraces}

Tide Gardens at their stillest — calm, dark, quiet. The terraces sink toward the abyssal plain.

### <span aria-hidden="true" style={{display: 'inline-block', width: '0.8em', height: '0.8em', borderRadius: '50%', background: '#ffd9b8', marginRight: '0.4em'}}></span>Dawn Expanse {#dawn-expanse}

Glass Expanse at first light — calm, bright, hushed. Crystal rings under a pale rose sky.

## How a world is furnished {#assets}

Worlds draw from the manifest-driven pack of 171 CC0 Poly Haven models (variants borrow their base quadrant's set), placed in three tiers:

| Tier | Target span | Role |
|---|---|---|
| Hero | 7 m | The landmarks — a handful per world |
| Prop | 2.5 m | The middle distance |
| Ground cover | 1 m | Merged into a single vertex-tinted mesh for density |

- Placement holds a per-tier **entity budget** (15 heroes / 25 props / 27 cover seeds) and spends it across the mood's pool by weighted round-robin — a bigger pack means more variety, not more entities.
- Every model is **span-normalized** from its native dimensions in the manifest (Poly Haven scans span 0.1–92 m) so nothing swallows the camera.
- **VisibilityRange** culls distant props — LOD by distance.
- With `ml` enabled, placement counts and the hero pick are weighted by the cosine between the track's audio embedding and each asset's text embedding — the world literally plants what sounds like the song.

## Keep this world {#pins}

**Keep this world** pins a `{mood, seed}` pair into the track's sidecar: that song always returns to that world with that exact layout. Layouts are splitmix64-seeded, and **Build a different world** re-rolls the seed — a new arrangement of the same mood.

## The transition engine {#transitions}

The product's signature feel is temporal:

- **Materialize** — a 0.8 s palette wash, then terrain and props rise over 1.3–2.4 s, staggered so the world **settles on the first downbeat**.
- **Song → song** — a dual-stream equal-power crossfade (final `min(6 s, 25%)` window) scheduled to land on the incoming track's nearest **section boundary**.
- **Close moods** — when consecutive tracks share a mood, the world morphs in place instead of rebuilding.

## The performed world {#performed}

A world isn't a static diorama — it's staged against the song's structure:

- **Whole-song materialize** — scatter in the intro, mediums in the verses, heroes rising on the chorus.
- **Waveform skyline** — rim stelae trace the track's energy curve around the horizon.
- **Bass-displaced ground** and per-prop bob and spin driven by the live signal and stem levels.
- **Section-aware Drift camera** and a horizon world title, with the recipe tier's choreography shifting energy, motion, and palette washes per section.
