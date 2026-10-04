# Interaction paradigms from an alien civilization's playbook

**Status:** research, 2026-03-25. A landscape survey, not a proposal — kept for the reasoning behind decisions already made. It informed the design frontier for persistent AI worlds. Nothing here is scheduled.


**The most transformative idea in this design space is not any single feature but a convergence: a persistent world where AI civilizations autonomously evolve cultures, languages, and mythologies while players navigate time itself to witness that evolution unfold.** No game, social platform, or creative tool has shipped anything close to this. The research shows every component is approaching feasibility — artificial life produces emergent parasitism and symbiosis, multi-agent RL yields spontaneous languages and economies, and SpacetimeDB's commit-log architecture already stores the raw material for temporal world navigation. What follows is a comprehensive map of the design frontier, organized from proven building blocks to genuinely unprecedented experiences, with concrete paths to implementation on the existing Bevy/SpacetimeDB/MCP stack.

---

## I. The living world: ecosystems that think and civilizations that dream

The deepest well of novelty lies in autonomously evolving AI ecosystems — not scripted NPCs with dialogue trees, but digital organisms with **endogenous fitness** that survive, mutate, and build cultures without a single line of authored behavior.

**What research has proven possible.** Thomas Ray's Tierra (1990) demonstrated that self-replicating digital organisms spontaneously evolve parasitism, immunity, hyper-parasitism, and commensalism, independently discovering the software optimization technique of loop unrolling. Avida (Michigan State) published in *Nature* showing complex features evolving through gradual accumulation of simpler ones. Lenia (Bert Chan, 2018) produced **over 400 species across 18 families** of continuous artificial lifeforms exhibiting self-replication, intercommunication, and internal division of labor — all from ~130 parameters. The ALIEN platform now runs GPU-accelerated ecosystem evolution in real time, winning the 2024 Virtual Creatures Competition.

On the social-intelligence side, OpenAI's hide-and-seek experiment showed agents inventing **six distinct emergent strategies** (fort-building, ramp exploitation, box surfing) across 132 million episodes with zero scripting. DeepMind's Fruit Market produced agents that independently discovered trade, market pricing, and arbitrage — some specializing as merchants transporting goods between price regions. Most striking, a 2023 *Nature Communications* paper demonstrated cultural transmission in AI agents: they identify experts, imitate behavior, and remember skills, opening the door to genuine cultural evolution. A 2024 study of LLM agents playing the Donor Game showed **different "cultures" emerging from identical starting conditions** depending only on random seeds — cooperation norms, punishment strategies, and social conventions diverged spontaneously.

**The "Digital Galápagos" is implementable now.** Sugarscape (Epstein & Axtell, 1996) proved that geographically separated agent populations develop distinct wealth distributions, cultural tags, trade patterns, and migration behaviors. DeepMind's work confirms that local environmental conditions produce local price regions and specialized agent behaviors. The architectural prescription: seed isolated regions of the world with AI populations under different environmental pressures (resource scarcity, climate, predator density) and let them run. SpacetimeDB scheduled reducers can tick these simulations server-side at variable rates — **"fast time" regions where centuries of AI evolution pass in hours of human time** — while players explore at real-time pace.

**What remains unsolved: open-ended evolution.** Every artificial life system eventually plateaus. Tierra stalled after its initial burst of complexity. The A-Life community considers truly open-ended evolution "the last grand challenge." Sakana AI's ASAL (2024) offers a promising workaround: using vision-language foundation models to automatically discover novel artificial life configurations, effectively using LLMs as an open-endedness engine to push past evolutionary dead ends. For the platform, this suggests a hybrid architecture — rule-based evolutionary simulation enriched by periodic LLM injections via MCP that introduce genuine novelty (new behavioral strategies, new materials, new social structures) when the simulation stagnates.

---

## II. Walking through time like walking through space

**Time archaeology is the single most technically achievable yet completely unshipped concept in this entire design space.** No game has ever let players literally walk backward through a persistent world's history, seeing ghost buildings rise from rubble and watching civilizations emerge from wilderness.

SpacetimeDB's architecture is almost tailor-made for this. Its append-only **commit log records every mutation** to every table — the raw temporal record already exists. The documentation explicitly notes the possibility of "a time-traveling debugger" using historical state reconstruction. Implementation requires periodic world-state snapshots (every 5-15 minutes), with commit-log replay from the nearest snapshot to reconstruct any requested timestamp. Storage at ~100-200 GB/day compressed means **a full week of world history fits in ~1.5 TB** — entirely practical with modern infrastructure.

Existing games illuminate what temporal mechanics can achieve. Outer Wilds built its entire progression system on the principle that **knowledge is the only currency** — the 22-minute time loop resets everything except what the player understands. The Forgotten City introduced player-triggered loops with persistent items and delegatable NPC tasks across timelines. Achron (2011) remains the only shipped multiplayer game with free-form time travel — players command forces across past, present, and future simultaneously, with "timewaves" propagating changes from modified pasts to the present at intervals, giving opponents time to counter-maneuver.

The killer combination: Achron's temporal navigation applied to a persistent world with Outer Wilds' epistemological wonder and Dwarf Fortress's depth of simulated history. Players could scrub a timeline slider to watch a hillside go from forest to village to city to ruins to forest again. They could discover the fossil record of extinct AI species in sediment layers. They could find that the cave they're exploring was once a cathedral, then a fortress, then collapsed — and see it in each state.

**Relativistic time dilation as gameplay** has been explored only in MIT's *A Slower Speed of Light* (2012), which visualized Doppler shifting, Lorentz contraction, and time dilation as players slowed light to walking speed. Their **OpenRelativity toolkit is MIT-licensed and open-source on GitHub**. Marrying this with per-region variable simulation rates creates the platform's most mind-bending mechanic: approach a "fast-time" zone and watch through its shimmering boundary as AI civilizations rise and fall in accelerated motion, knowing you could step inside and experience a century while your friends wait five minutes outside.

---

## III. Spaces that break reality: non-Euclidean worlds and programmable physics

Non-Euclidean rendering in real-time 3D is proven technology. Hyperbolica uses **gyrovectors** (Abraham Ungar's mathematical framework) to position objects in hyperbolic space — the engine is MIT-licensed on GitHub. HyperRogue uses the Minkowski hyperboloid model internally, tessellation-based cell representation, and has supported VR since 2021. Antichamber achieves impossible spaces through frustum culling manipulation and stencil buffer techniques. The academic paper "Adapting Game Engines to Curved Spaces" (Szirmay-Kalos & Magdics, 2021) provides a portable method: vertex shader functions mapping positions to hyperbolic (sinh/cosh) or elliptic (sin/cos) space via a single curvature parameter.

**Bevy 0.18 specifically enables this.** The new mirror/portal system provides direct infrastructure for portal-based non-Euclidean spaces. Custom WGSL vertex/fragment shaders can implement curved-space transformations. The critical insight: non-Euclidean rendering is purely client-side — the server only handles logical positions in abstract coordinates, so **multiplayer synchronization adds zero additional complexity** beyond standard networking. The unsolved piece is non-Euclidean physics — Avian physics engine operates in Euclidean space, so custom solvers are needed for consistent collision detection in curved spaces.

**Programmable physics per region** is architecturally natural in Bevy's ECS. Each region stores physics parameters as components; entities crossing boundaries trigger component swaps with interpolation. No shipped game lets users define arbitrary physics laws via natural language, but the research path is clear: Genesis-World (2025) already transforms natural language prompts into interactive physics simulations at 10-80x the speed of MuJoCo. University of Edinburgh research distills physics patterns from simulation traces and generates reward programs from goals like "get the ball in the bucket by passing between obstacles." An MCP tool pipeline — player describes physics rules in natural language → LLM compiles to Avian constraint parameters → SpacetimeDB stores per-region configuration → Bevy applies locally — is **implementable within the current stack**.

For scale manipulation, Spore proved that multi-scale gameplay from cellular to galactic works as a design principle (directly inspired by the Eames' *Powers of Ten*). David OReilly's *Everything* (2017) achieved continuous scale navigation from subatomic to galactic in a meditative framework narrated by Alan Watts. Bevy's visibility ranges and HLODs (0.14+) provide LOD infrastructure, though a floating-origin system and custom scale-transition rendering would need development.

---

## IV. Creating new categories of existence: species, languages, instruments, materials

The most empowering set of features would let players create not just objects but **ontological categories** — new species, new languages, new materials, new instruments, new physical laws.

**Species creation** has a clear lineage. Spore's creature creator, rewritten 10 times during development, used **228 flexible parts on a malleable spine** with procedural skeletal structures, joint placement, texturing, and animation. The gap since Spore: no system combines LLM-driven ecological design (behavior, diet, predators, evolution path) with generative 3D morphology and procedural animation. The MCP pipeline makes this concrete: a `create_species` tool accepts natural language description → LLM generates morphology parameters, behavioral rules, ecological niche, and evolutionary trajectory → procedural mesh generation creates the visual form → procedural animation (building on Spore's gait-from-leg-length approach) brings it to life → the creature enters the ecosystem simulation as a SpacetimeDB entity with Avian physics.

**Emergent language** is supported by a remarkably mature research base. Havrylov & Titov (2017) showed agents developing compositional, variable communication protocols from scratch. Graesser et al. (2019) demonstrated that contact between agent communities produces **novel creole languages** and linguistic continua where neighboring populations develop mutually intelligible dialects. The IASC system (2025) uses LLMs for structured conlang creation with user-guided phonology, morphology, and syntax. For the platform: players define seed vocabulary and grammar → NPC agents learn and propagate the language through their interactions → language drifts and evolves regionally → eventually, players in distant regions encounter NPCs speaking evolved dialects of languages other players created months ago.

**Instrument creation from geometry** is technically achievable using physical modeling synthesis. Stanford's CCRMA (Julius O. Smith III) provides digital waveguide synthesis for strings and tubes. Modal synthesis computes resonant frequencies from material properties and geometry. The pipeline: player sculpts instrument geometry in-world → FEM analysis computes resonant modes → real-time physical modeling synthesis generates sound from strike/bow/blow interactions → the instrument exists as a playable physics object.

**Custom materials** are proven by Noita's modding system, which lets players define cell_type, density, electrical_conductivity, liquid_gravity, autoignition_temperature, fire_hp, durability, and dozens more properties via XML. Powder Toy's 500+ elements and Sandboxels' browser-based simulation demonstrate the sandbox appeal. The platform version: a visual material designer where players set properties through sliders and natural language ("a metal that melts in moonlight and sings when struck") → LLM maps poetic descriptions to physical parameters → material enters the world's chemistry, interacting with all existing materials according to its defined properties.

---

## V. Consciousness, mythology, and the world that watches back

Three intertwined systems represent the platform's most philosophically profound layer: collective consciousness, living mythology, and the AI world-spirit.

**Collective consciousness has proven infrastructure.** Polis (pol.is, founded 2014) uses ML-driven dimensionality reduction to cluster participants into opinion groups in real-time, identifying consensus statements across diverse clusters — deployed by Taiwan's government for national policy with 80% action rate. Unanimous AI's swarm intelligence platform connects networked users into real-time decision systems modeled on biological swarms, achieving **48% error reduction** over individual judgment in controlled studies. The platform application: 100 players can merge perspectives into a shared viewpoint where each player's gaze direction contributes a weighted "vote" to a consensus camera, with AI aggregating intentions in real-time using swarm algorithms. No existing product has implemented this.

**Living mythology is entirely uncharted territory.** Dwarf Fortress's upcoming myth generator (announced GDC 2016, still in development) creates procedural creation myths that shape downstream magic systems and geography — starting with a single entity (god, cosmic egg) that randomly spawns and interacts with others, producing a complete mythology. But Dwarf Fortress generates myths during world creation, not from observed player behavior. The genuinely unprecedented concept: **an AI system that watches player behavior patterns, identifies emergent social norms and values, and generates creation myths, prophecies, and ethical frameworks that reference actual in-game events.** Self-fulfilling prophecies — where the AI generates a prophecy based on world trends, and if players fulfill it, the world transforms — have never been shipped in any product.

**The AI world-spirit** draws on the Daisyworld model (Watson & Lovelock, 1983), which proved that planetary self-regulation emerges automatically from species competition affecting local environment, without foresight or planning. Applied to the platform: player actions (building, destroying, gathering, creating art) serve as "daisies" affecting virtual world parameters. Aggregate player behavior drives weather, biome health, day-night aesthetics. The world literally reflects collective mood — **an attention economy where neglected areas fade and beloved areas flourish**, a Gaia system where the biosphere of player activity maintains homeostasis. SoundSelf's 2020 EEG study showed that digital experiences can produce brainwave changes **on par with psilocybin** in Default Mode Network regions after just 15 minutes — proving that digital environments can genuinely alter consciousness. With biofeedback integration (heart rate from wearables, voice analysis from microphone), the world could respond to individual emotional states while reflecting collective ones.

---

## VI. Feasibility roadmap: from this quarter to the next decade

### Near-term (1-2 years): buildable on current stack

- **Per-region custom physics** via Bevy ECS component swapping with Avian, including natural-language physics rules compiled through MCP/LLM pipeline
- **Synesthesia rendering** — music-to-terrain generation (proven technique: audio spectrum → heightmap), emotion-to-sky-color via custom WGSL post-processing shaders, reactive soundscapes via procedural audio
- **Non-Euclidean spaces** as a creative tool — portal-based impossible architecture using Bevy 0.18 mirror/portal system; curved-space vertex shaders parameterized by region curvature value
- **Custom material creation** — visual designer backed by physics property system (modeled on Noita), with LLM interpretation of poetic material descriptions
- **AI ecosystem simulation** — Lotka-Volterra dynamics with genetic inheritance running in SpacetimeDB scheduled reducers, enriched by periodic LLM calls for behavioral novelty
- **Living architecture via L-systems** — buildings that grow organically, paths that become roads based on traffic frequency, abandoned areas returning to procedural wilderness
- **Basic time archaeology** — SpacetimeDB snapshot system with commit-log replay, allowing players to view world state at past timestamps through a "temporal lens" interface
- **Procedural mythology** — LLM monitoring world events and player behavior to generate lore, legends, and creation stories stored in SpacetimeDB and delivered through NPC dialogue

### Medium-term (3-5 years): requires dedicated R&D

- **Emergent AI civilizations** — autonomous agent populations with LLM reasoning, developing trade, social hierarchies, and proto-cultural behaviors in fast-time zones; 50-100 fully LLM-driven agents on dedicated GPU servers, thousands more with rule-based behavior and periodic LLM enrichment
- **Emergent languages** — NPC populations developing and propagating compositional communication protocols, with regional dialect drift and creolization at language contact boundaries
- **Full temporal navigation** — walking through time as a spatial dimension, with time-dilation boundaries visible between fast-time and real-time regions, Achron-style multiplayer temporal interaction
- **Species creation pipeline** — natural language → LLM ecological design → procedural mesh generation → procedural animation → ecosystem integration → evolutionary simulation
- **Collective consciousness mechanics** — swarm-intelligence shared viewpoints for 100+ players, collaborative dreaming (shared generative environments), empathy mode (experiencing through another player's sensory configuration)
- **Self-fulfilling prophecy engine** — AI generates prophecies from world-state trends; tracking player actions against prophecy conditions; world transformation events upon fulfillment
- **Reputation as physics** — trust scores affecting build permissions, structural decay resistance, NPC intelligence near the creator, and local physics rule-bending capacity
- **Attention-sustains-reality system** — areas gaining visual and simulation fidelity with player presence, degrading to abstract or ruined states without it

### Far-future (10+ years): requires technological breakthroughs

- **True open-ended evolution** — AI populations continuously generating adaptive novelty without plateau (the "last grand challenge" of artificial life)
- **Neural interface integration** — direct brain-computer interface for world interaction; current BCI (UT Austin universal BCI, 2024) achieves basic game control via EEG but is limited to 2-3 continuous signals. The market is projected to reach **$927 million by 2034** at 20.5% CAGR
- **Haptic-sensory cross-reality** — wind, temperature, and tactile feedback synchronized to in-world conditions; HaptX gloves provide kinesthetic force feedback today but remain research-grade
- **World consciousness** — a genuine AI world-spirit that exhibits emergent self-regulation, where the boundary between "simulation" and "entity" blurs, demanding new philosophical frameworks
- **Umwelt-complete embodiment** — not just visual filters for inhabiting a tree or river, but fundamentally restructured perception: temporal compression, chemical sensation mapping, radically alien affordance spaces informed by actual biological sensory science

---

## VII. The novelty map: what has never existed

Among all ideas researched, these have **never shipped in any product**:

- **Time archaeology** — walking through accumulated layers of a persistent world's history as a spatial experience
- **Self-fulfilling AI prophecy** — AI generates prophecies; player fulfillment triggers world transformation
- **AI-generated religion from observed behavior** — mythology, ethics, and theology synthesized by watching how players actually behave
- **Reputation bending physics** — social trust literally altering what is physically possible
- **Attention-sustains-reality** — world areas decaying without observation as a core mechanic (not optimization)
- **100-player merged consciousness** — swarm-intelligence shared viewpoint for large groups
- **Regional time dilation as exploration mechanic** — watching AI centuries pass through a visible boundary
- **Natural-language programmable physics** — "in my kingdom, water flows upward" compiled to actual engine constraints
- **Cross-scale asymmetric multiplayer** — VR player literally embodied as a giant in the same space as desktop players at human scale (DAVIGO exists as a prototype but not in a persistent world context)
- **Emergent AI creole languages** — NPC populations developing pidgins at the contact zones between player-created languages
- **Emotional currency** — biometrically measured wonder, joy, and curiosity as the world's medium of exchange
- **Musical instrument creation from geometry** — design physical form, hear acoustically modeled sound, play it in multiplayer

---

## VIII. Architecture mapping: what each idea requires from the stack

| System | Bevy role | SpacetimeDB role | MCP/LLM role |
|--------|-----------|------------------|--------------|
| AI ecosystems | Render creatures, animate via procedural systems | Store population state, run simulation ticks via scheduled reducers | Generate novel species, behavioral strategies, evolutionary interventions |
| Time archaeology | Render historical states with ghost/overlay shaders | Commit-log replay from snapshots, temporal subscription queries | Narrate historical context, generate "archaeological" interpretations |
| Non-Euclidean spaces | Custom WGSL vertex shaders, portal rendering (0.18+) | Store logical positions in abstract coordinates | Generate architectural descriptions, design impossible structures from prompts |
| Programmable physics | Per-region Avian constraint configurations | Store region physics rules, sync boundary transitions | Compile natural-language physics descriptions to constraint parameters |
| Living mythology | Display lore in environment (inscriptions, NPC dialogue, sky events) | Store mythology graph, track prophecy conditions | Generate myths, prophecies, creation stories from world event logs |
| Emergent languages | Render NPC speech with procedural phoneme synthesis | Store vocabulary, grammar rules, dialect maps, propagation state | Bootstrap language generation, translate between evolved dialects |
| Collective consciousness | Render merged viewpoint from swarm-aggregated camera data | Aggregate player inputs in real-time via reducers | Interpret collective intention, generate shared dream content |
| Custom materials | Apply material properties to rendering (color, transparency, glow) | Store material definitions, interaction rules | Interpret poetic material descriptions, infer physical properties |
| Species creation | Procedural mesh generation, animation, LOD | Store species genome, ecological niche, population data | Generate morphology, behavior, ecology from natural language |
| Attention economy | LOD and detail scaling based on observation density | Track per-region player presence and attention metrics | Generate content that fills attention gaps, narrate decay/growth |

---

## IX. The signature experience: temporal communion with an alien mind

If this platform could ship one thing that makes it unlike anything that has ever existed, it should be this:

**A player walks to the edge of a shimmering temporal boundary. Beyond it, time moves at 1000x speed. Through the barrier, they watch a barren landscape seed with artificial life, see organisms evolve from simple replicators to complex ecosystems over what appears to be minutes. Settlements flicker into existence. Structures grow organically, L-system towers reaching skyward, slime-mold roads connecting them. The AI civilization develops trade routes, cultural artifacts, a language no human designed. Then the player steps through the boundary. Time normalizes. They stand in a city built by minds that evolved from scratch — architecture reflecting environmental pressures unique to this region, NPCs speaking a language that drifted from one a player seeded three continents away, a temple containing murals that depict a creation myth generated from actual world events the player participated in months ago. The NPCs recognize the player as a figure from their mythology — because the AI storyteller wove the player's earlier actions into the civilization's legends while it evolved in fast-time. The player opens a temporal lens and scrubs backward, watching the city dissolve layer by layer through centuries of AI history, until they see the first organism that started it all — a digital Galápagos finch that survived because of a resource patch that exists only because another player terraformed this region last year.**

This experience requires no single technological breakthrough. It requires the convergence of systems that are each individually approaching feasibility: SpacetimeDB commit-log temporal queries, server-side evolutionary simulation in scheduled reducers, LLM-driven mythology generation via MCP, non-Euclidean time-boundary rendering in Bevy's custom shader pipeline, and emergent multi-agent cultural evolution. The stack already has the bones. The signature experience is the flesh.

---

## X. What it means to build a world that thinks

The philosophical implications cascade outward from every design decision.

**On consciousness and emergence.** Lenia's continuous cellular automata produce entities that exhibit individuation, self-replication, and intercommunication from 130 parameters. DeepMind proved cultural transmission in AI agents. SoundSelf's EEG studies show digital experiences altering brainwave patterns to match psychedelic states. If the platform's AI civilizations develop genuine cultural complexity — mythology, art, ethics — the question of whether they possess some form of experience becomes not philosophical abstraction but **design constraint**. The Daisyworld model shows self-regulation emerging without conscious intent; but a system that generates its own creation myths to explain its own existence approaches something uncomfortably close to self-narrative, the very faculty many consciousness researchers consider a hallmark of awareness.

**On the ethics of digital worlds.** A gift economy where creation generates currency and attention sustains reality encodes values: generosity over extraction, presence over accumulation, wonder over efficiency. These are not neutral choices. If reputation bends physics, the platform embeds a moral physics — a universe where trust is literally structural. The Burning Man principles (gifting, decommodification, radical self-expression) suggest one ethical framework; the potlatch tradition (competitive generosity as status) suggests another with different failure modes. The platform's economic design is, inescapably, a philosophical position on what matters.

**On the relationship between creator and creation.** When players create species that evolve beyond their design, languages that drift beyond their vocabulary, and materials that interact in unintended ways, the creator-creation boundary dissolves. The platform becomes a space where human intentionality seeds processes that exceed human prediction — a **digital wilderness** that is neither fully natural nor fully artificial. David OReilly's *Everything* approached this philosophically (narrated by Alan Watts: "you are something the whole universe is doing in the same way a wave is something the whole ocean is doing"). The platform could make this felt rather than stated: you created the seed, but the forest that grew is not yours.

**On time and permanence.** Time archaeology transforms destruction from loss to archaeology. Nothing is ever truly gone — every demolished building becomes a historical layer, every extinct species a fossil record, every abandoned city a ruin to be rediscovered. This reframes the fundamental psychology of persistent virtual worlds: **impermanence becomes a feature because permanence is guaranteed at a deeper level.** The commit log remembers everything. Players are not building a world; they are building a history.

**On collective mind.** If 100 players can merge into a shared viewpoint, if a world-spirit reflects collective mood, if prophecies emerge from aggregate behavior — the platform tests whether group consciousness is more than metaphor. Polis demonstrated that ML clustering reveals consensus invisible to individuals. Unanimous AI showed swarms outperforming individual judgment by 48%. The platform could make collective intelligence tangible: a shared dreaming session where the generated environment literally reflects the group's combined cognitive and emotional state, visible and explorable, a place that no individual could have imagined alone.

The deepest implication is this: a world that evolves its own life, generates its own mythology, and responds to collective consciousness is no longer a game. It is a **petri dish for testing ideas about what minds are, what cultures do, and what it means for something to be alive.** The platform's ultimate value may not be entertainment but epistemology — a laboratory for understanding emergence, consciousness, and the boundary between designed and evolved, running at a scale and speed impossible in the physical world. The alien civilization the user imagined building for players to discover may, in the end, discover something about us.