// Opened in a browser tab with `?preview=document`, `world` or `song`, the
// page gets a stand-in for the app: the messages the app would send for that
// mode, from fixtures. It is how the page is looked at — and checked
// headlessly — with nothing behind it. Asks are answered only as far as
// showing them needs: the timeline's clock plays and seeks, a drag moves
// the shot it let go of, and an inspector's edit lands in the harbour's
// entities as the authority would fold it — or is refused as it would be,
// for a name that is taken. A row let go in the outliner changes parents
// standing where it stood, as far as positions alone say: the harbour's
// groups don't turn or scale.

import { entities as harbour } from "../design/scene-fixtures";
import type { DocumentView, FromPage, HistoryRow, SceneView, TimelineView, ToPage, TrackIn } from "./bridge";
import { setStandIn } from "./bridge";
import { patched } from "./scene/model";
import type { Shot, Signal } from "./timeline/model";

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

// A song a little like Tidewater, for the timeline: 92 bpm over 192 s,
// seven sections, curves drawn from a seeded wobble rather than a decoding.
const DURATION = 192;
const SECTIONS = [0, 0.1, 0.27, 0.43, 0.6, 0.78, 0.9];
const wobble = (i: number) => (Math.sin(i * 12.9898) * 43758.5453) % 1;
const swell = (t: number) => 0.35 + 0.5 * Math.sin((t / DURATION) * Math.PI) ** 2;
const curve = (rate: number, shape: (t: number, i: number) => number): Signal => ({
  values: Array.from({ length: DURATION * rate + 1 }, (_, i) => Math.round(Math.min(1, Math.max(0, shape(i / rate, i))) * 1000) / 1000),
  rate,
});
const beat = (t: number) => 1 - (((t - 0.4) * 92) / 60 - Math.floor(((t - 0.4) * 92) / 60));

const SHOTS: [string, string, number, number, number][] = [
  ["1A", "WS", 24, 0, 19.2],
  ["1B", "MS", 40, 19.2, 36.4],
  ["2A", "WS", 18, 36.4, 51.84],
  ["2B", "OTS", 35, 51.84, 82.56],
  ["3A", "WS", 18, 82.56, 115.2],
  ["3B", "MCU", 65, 120.42, 149.72],
  ["4A", "EWS", 14, 149.72, DURATION],
];

function timeline(shots: boolean): TimelineView {
  const tracks: TrackIn[] = [];
  if (shots)
    tracks.push({
      kind: "shots",
      id: "shots",
      name: "Shots",
      toggles: ["lock"],
      shots: SHOTS.map(([name, size, lens, from, to], i): Shot => ({ id: String(i + 1), name, size, lens, from, to })),
    });
  if (shots)
    tracks.push({
      kind: "camera",
      id: "tour:harbour approach",
      name: "Camera",
      tour: "harbour approach",
      toggles: ["lock"],
      keys: [{ t: 0, hold: 4 }, { t: 19.2 }, { t: 36.4, hold: 6 }, { t: 51.84 }, { t: 82.56, hold: 3 }, { t: 115.2 }, { t: 149.72, hold: 8 }, { t: DURATION }],
    });
  tracks.push(
    {
      kind: "soundtrack",
      id: "soundtrack",
      name: "Soundtrack",
      title: "Tidewater",
      artist: "Starter pack",
      licence: "CC0-1.0",
      toggles: ["mute", "solo"],
      peaks: curve(20, (t, i) => swell(t) * (0.55 + 0.45 * Math.abs(wobble(i)))),
    },
    { kind: "stem", id: "stem:drums", name: "Drums", envelope: curve(1, (t, i) => swell(t) * (0.6 + 0.4 * Math.abs(wobble(i + 7)))) },
    { kind: "stem", id: "stem:bass", name: "Bass", envelope: curve(1, (t, i) => swell(t) * (0.5 + 0.5 * Math.abs(wobble(i + 13)))) },
    { kind: "modulation", id: "mod:emissive:beat:0.4:2", name: "lantern", target: "emissive", signal: "beat", source: "beat", range: [0.4, 2], toggles: ["hide"] },
    { kind: "modulation", id: "mod:height:stem:bass:-0.1:0.1", name: "boat", target: "height", signal: "bass stem", source: "stem:bass", range: [-0.1, 0.1], toggles: ["hide"] },
    { kind: "behavior", id: "behavior:spin:6", name: "beacon", what: "spin", period: 6, from: 0, to: DURATION, toggles: ["hide"] },
    { kind: "ambience", id: "ambience:ocean", name: "ocean", from: 0, to: DURATION, fade: [0, 0], toggles: ["mute", "solo"] },
  );
  return {
    clock: { duration: DURATION, bpm: 92, beatOffset: 0.4, sections: SECTIONS },
    tracks,
    signals: { beat: curve(16, beat), "stem:bass": curve(4, (t, i) => swell(t) * (0.5 + 0.5 * Math.abs(wobble(i + 13)))) },
    editable: shots,
    proposal: null,
  };
}

const MODES: Record<string, ToPage[]> = {
  document: [
    { t: "mode", history: false, document: true },
    { t: "title", text: "Document · harbour.md" },
    { t: "doc.text", text: TEXT, rev: 1 },
    { t: "document", doc },
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
  ],
};

/** The app's clock, stood in: it runs while it plays and loops the song. */
function standInClock() {
  let at = 64.2;
  let playing = false;
  let loop: [number, number] | null = null;
  let since = performance.now();
  const now = () => (playing ? (at + (performance.now() - since) / 1000) % DURATION : at);
  const tell = () => recv({ t: "clock", at: now(), playing, loop });
  return {
    tell,
    seek(t: number) {
      at = t;
      since = performance.now();
      tell();
    },
    play(on: boolean) {
      at = now();
      since = performance.now();
      playing = on;
      tell();
    },
    loop(span: [number, number] | null) {
      loop = span;
      tell();
    },
  };
}

export function preview(kind: string) {
  const messages = MODES[kind];
  if (!messages) return;
  let scene: SceneView | null =
    kind === "world"
      ? {
          entities: harbour,
          editable: true,
          world: { name: "harbour", sky: [0.66, 0.47, 0.52, 1], song: "Tidewater — Starter pack", tours: 1 },
          person: "person",
          proposal: null,
          tracks: { "37": ["mod:emissive:beat:0.4:2"], "38": ["mod:light_intensity:beat:0.4:2"], "40": ["mod:height:stem:bass:-0.1:0.1"] },
        }
      : null;
  /** Commit a change to the harbour as the person: a new entry, the head. */
  const commit = (entities: SceneView["entities"], what: string) => {
    if (!scene) return;
    scene = { ...scene, entities };
    const parent = log.at(-1)!;
    log = [
      ...log.map((row) => ({ ...row, tip: false, head: false })),
      { ...entry(`${(log.length + 1).toString(16).padStart(2, "0")}aa${Date.now().toString(16)}`, null, "person", what, 0, true), parent: parent.id, revision: log.length + 1 },
    ];
    recv({ t: "history", rows: log, current: log.at(-1)!.id, status: `r${log.length} - person - ${what}`, turns });
    recv({ t: "scene", view: scene });
  };
  let log = [...rows];
  const turns = [{ n: 1, agent: "claude", rows: 3, revision: 3 }];
  // A document's world keeps no time: the timeline is its one quiet line.
  let view: TimelineView = kind === "document" ? { clock: null, tracks: [], signals: {}, editable: false, proposal: null } : timeline(kind === "world");
  const clock = standInClock();
  setStandIn((m: FromPage) => {
    switch (m.t) {
      case "ready":
        messages.forEach(recv);
        recv({ t: "timeline", view });
        clock.tell();
        if (scene) {
          recv({ t: "scene", view: scene });
          recv({ t: "selection", ids: [] });
        }
        break;
      case "select":
        recv({ t: "selection", ids: m.ids });
        break;
      case "entity.patch": {
        if (!scene) break;
        const shown = scene;
        const taken = m.edits.find((e) => typeof e.patch.name === "string" && shown.entities.some((o) => o.name === e.patch.name && o.id !== e.id));
        if (taken) {
          recv({ t: "history", rows: log, current: log.at(-1)!.id, status: `refused: an entity named '${taken.patch.name}' already exists`, turns });
          break;
        }
        commit(
          shown.entities.map((e) => m.edits.reduce((at, edit) => (edit.id === e.id ? patched(at, edit.patch) : at), e)),
          m.what,
        );
        break;
      }
      case "entity.parent": {
        if (!scene) break;
        const all = scene.entities;
        const byId = new Map(all.map((e) => [e.id, e]));
        // Where a thing stands, by positions alone.
        const at = (id: number | null | undefined): number[] => {
          const e = id == null ? undefined : byId.get(id);
          if (!e) return [0, 0, 0];
          const up = at(e.parent);
          return e.transform.position.map((v, i) => v + up[i]);
        };
        const names = m.ids.map((id) => byId.get(id)?.name ?? "").join(", ");
        commit(
          all.map((e) => {
            if (!m.ids.includes(e.id)) return e;
            const position = m.keep ? e.transform.position : (at(e.id).map((v, i) => Math.round((v - at(m.parent)[i]) * 1000) / 1000) as [number, number, number]);
            return { ...e, parent: m.parent, transform: { ...e.transform, position } };
          }),
          m.parent == null ? `unparent ${names}` : `put ${names} in ${byId.get(m.parent)?.name}`,
        );
        break;
      }
      case "doc.go":
        recv({ t: "document", doc: { ...doc, selected: m.section } });
        break;
      case "clock.seek":
        clock.seek(m.at);
        break;
      case "clock.play":
        clock.play(m.playing);
        break;
      case "clock.loop":
        clock.loop(m.span);
        break;
      case "song.sections":
        if (view.clock) view = { ...view, clock: { ...view.clock, sections: m.sections } };
        recv({ t: "timeline", view });
        break;
      case "tour.hold":
        view = {
          ...view,
          tracks: view.tracks.map((track) => {
            if (track.kind !== "camera" || track.tour !== m.tour) return track;
            const by = m.hold - (track.keys[m.stop]?.hold ?? 0);
            const keys = track.keys.map((key, i) => (i === m.stop ? { ...key, hold: m.hold || undefined } : i > m.stop ? { ...key, t: key.t + by } : key));
            return { ...track, keys };
          }),
        };
        recv({ t: "timeline", view });
        break;
      case "shot.time":
        view = {
          ...view,
          tracks: view.tracks.map((track) =>
            track.kind !== "shots"
              ? track
              : {
                  ...track,
                  shots: track.shots.map((shot) => {
                    const edit = m.edits.find((e) => String(e.id) === shot.id);
                    return edit ? { ...shot, from: edit.in_s, to: edit.out_s } : shot;
                  }),
                },
          ),
        };
        recv({ t: "timeline", view });
        if (scene)
          commit(
            scene.entities.map((e) => {
              const edit = m.edits.find((x) => x.id === e.id);
              const cine = e["ext-cinematography"];
              return edit && cine?.shot ? { ...e, "ext-cinematography": { ...cine, shot: { ...cine.shot, in_s: edit.in_s, out_s: edit.out_s } } } : e;
            }),
            `retime ${m.edits.map((x) => scene!.entities.find((e) => e.id === x.id)?.name).join(", ")}`,
          );
        break;
    }
  });
}
