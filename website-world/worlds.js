// The gallery's worlds, in show order: each a `.world` package under
// worlds/, whose manifest supplies the title, description and camera. The
// landing page and the per-world page read this; `audio` marks a world
// that performs to its track.
const WORLDS = [
  {
    package: 'gen-basalt-lighthouse.world',
    title: 'Basalt Lighthouse',
    credit: 'LocalGPT Gen',
    note: 'A lighthouse on a basalt headland at dusk — the beam turns on the rocks below, gulls orbit the lamp.',
  },
  {
    package: 'gen-lantern-night-market.world',
    title: 'Lantern Night Market',
    credit: 'LocalGPT Gen',
    note: 'A night market under strings of paper lanterns, steam rising from the noodle stalls.',
  },
  {
    package: 'gen-misty-temple-pool.world',
    title: 'Misty Temple Pool',
    credit: 'LocalGPT Gen',
    note: 'An overgrown shrine at first light, mist over a temple pool while koi circle its stone lantern.',
  },
  {
    package: 'verse-amber-drift.world',
    title: 'Amber Drift',
    credit: 'LocalGPT Verse',
    note: 'A world for "Amber Drift" — it plays to the track. Turn the sound on.',
    audio: true,
  },
  {
    package: 'verse-nightglass.world',
    title: 'Nightglass',
    credit: 'LocalGPT Verse',
    note: 'A world for "Nightglass" — stone and still water performing the song.',
    audio: true,
  },
];
export default WORLDS;
