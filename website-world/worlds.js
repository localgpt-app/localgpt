// The gallery's worlds, in show order. The landing page and the viewer read
// this; the manifest itself supplies the title, description and camera.
// `poster` and `audio` mark what the card shows; `remix` is the one command
// to keep building on it in Gen.
const WORLDS = [
  {
    file: 'gen-basalt-lighthouse.json',
    title: 'Basalt Lighthouse',
    credit: 'LocalGPT Gen',
    note: 'A lighthouse on a basalt headland at dusk — the beam turns on the rocks below, gulls orbit the lamp.',
  },
  {
    file: 'gen-lantern-night-market.json',
    title: 'Lantern Night Market',
    credit: 'LocalGPT Gen',
    note: 'A night market under strings of paper lanterns, steam rising from the noodle stalls.',
  },
  {
    file: 'gen-misty-temple-pool.json',
    title: 'Misty Temple Pool',
    credit: 'LocalGPT Gen',
    note: 'An overgrown shrine at first light, mist over a temple pool while koi circle its stone lantern.',
  },
  {
    file: 'md-deck.json',
    title: 'Markdown, as a place',
    credit: 'LocalGPT MD',
    note: 'A Markdown talk you can walk through, six slides as places. Press the tour.',
  },
  {
    file: 'md-hello.json',
    title: 'A Walk Through LocalGPT MD',
    credit: 'LocalGPT MD',
    note: 'Any Markdown file becomes a place: every section a region, rebuilt on save.',
  },
  {
    file: 'verse-amber-drift.json',
    title: 'Amber Drift',
    credit: 'LocalGPT Verse',
    note: 'A world for "Amber Drift" — it plays to the track. Turn the sound on.',
    audio: true,
  },
  {
    file: 'verse-nightglass.json',
    title: 'Nightglass',
    credit: 'LocalGPT Verse',
    note: 'A world for "Nightglass" — stone and still water performing the song.',
    audio: true,
  },
];
export default WORLDS;
