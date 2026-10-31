// Opened in a browser tab with `?preview=document`, `world` or `song`, the
// page gets a stand-in for the app: the messages the app would send for that
// mode, from fixtures. It is how the page is looked at — and checked
// headlessly — with nothing behind it. Asks are answered only as far as
// showing them needs.

import type { DocumentView, FromPage, HistoryRow, ToPage } from "./bridge";
import { setStandIn } from "./bridge";

const recv = (m: ToPage) => window.__lg!.recv(m);

const TEXT = `# Harbour at dusk

A small harbour as the light goes, for the music video's first verse.

## The tower

A lighthouse on the breakwater, its beam turning once every six seconds.

### Lamp room

Brass and glass; the lamp is the brightest thing in the place.

## The jetty

Wooden, long, with a lantern at the end that pulses on the beat.

## The boat

A fishing boat moored off the jetty, rocking on the swell.
`;

const outline: DocumentView["outline"] = [
  { level: 1, text: "Harbour at dusk", section: null, line: null, tier: null, failed: false, reauthor: false },
  { level: 2, text: "The tower", section: 0, line: 4, tier: "model", failed: false, reauthor: true },
  { level: 3, text: "Lamp room", section: 0, line: 8, tier: null, failed: false, reauthor: false },
  { level: 2, text: "The jetty", section: 1, line: 12, tier: "authoring…", failed: false, reauthor: false },
  { level: 2, text: "The boat", section: 2, line: 16, tier: "draft", failed: false, reauthor: true },
];

const doc: DocumentView = {
  name: "harbour.md",
  path: "~/notes/harbour.md",
  status: "3 sections",
  conflict: false,
  authoring: "1 of 3 places by claude-cli/opus · 1 to go",
  author_enabled: true,
  author_error: null,
  follow_cursor: true,
  outline,
  selected: 0,
  card: {
    section: 0,
    heading: "The tower",
    tier: "model",
    description: "A white lighthouse on a rough stone breakwater, its lamp lit against the dusk.",
    entities: 9,
    things: [
      { label: "lighthouse", first: "s01-lighthouse", count: 1 },
      { label: "breakwater stone", first: "s01-breakwater_stone_1", count: 5 },
      { label: "lamp", first: "s01-lamp", count: 1 },
      { label: "gull", first: "s01-gull_1", count: 2 },
    ],
    more: 0,
  },
};

const ago = (m: number) => Date.now() - m * 60_000;
const entry = (id: string, parent: string | null, author: string, summary: string, minutes: number, head = false): HistoryRow => ({
  id: `sha256:${id}`,
  parent: parent && `sha256:${parent}`,
  revision: 0,
  author,
  summary,
  message: null,
  timestamp_ms: ago(minutes),
  tip: head,
  head,
});
const rows = [
  entry("14894e0a1c77d2f3", null, "claude", "ground and sun", 52),
  entry("2b7c91d03e4a5b60", "14894e0a1c77d2f3", "claude", "tower", 47),
  entry("3f02aa7e1d9c8b45", "2b7c91d03e4a5b60", "claude", "lamp", 41),
  entry("47d9c0b55a1e3f72", "2b7c91d03e4a5b60", "gemini", "dome", 33),
  entry("5a61e2f08c4d7b19", "3f02aa7e1d9c8b45", "claude", "jetty", 21),
  entry("6c3b7d419e2f0a58", "47d9c0b55a1e3f72", "gemini", "flag", 14),
  entry("7e85f1c2a4b6d390", "5a61e2f08c4d7b19", "claude", "boat", 6, true),
];

const song = { title: "Tidewater", artist: "Starter pack", duration: 192, position: 64.2, playing: true, playable: true };

const MODES: Record<string, ToPage[]> = {
  document: [
    { t: "mode", history: false, document: true },
    { t: "title", text: "Document · harbour.md" },
    { t: "doc.text", text: TEXT, rev: 1 },
    { t: "document", doc },
    { t: "song", song: null },
  ],
  world: [
    { t: "mode", history: true, document: false },
    { t: "title", text: "World · harbour.world" },
    { t: "history", rows, current: rows[6].id, status: "boat · revision 7", turns: [{ n: 1, agent: "claude", rows: 3, revision: 3 }] },
    { t: "playback", playing: false },
  ],
  song: [
    { t: "mode", history: false, document: false },
    { t: "title", text: "Song · Tidewater" },
    { t: "song", song },
  ],
};

export function preview(kind: string) {
  const messages = MODES[kind];
  if (!messages) return;
  setStandIn((m: FromPage) => {
    if (m.t === "ready") messages.forEach(recv);
    if (m.t === "doc.go") recv({ t: "document", doc: { ...doc, selected: m.section } });
  });
}
