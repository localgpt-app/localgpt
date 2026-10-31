import { createRoot } from "react-dom/client";
import { HistoryPanel } from "../src/HistoryPanel";
import { PlaybackBar } from "../src/PlaybackBar";
import { Frame } from "../src/timeline/Frame";
import type { Clock, FrameLook, Shot, Track } from "../src/timeline/model";
import { HEADER, Timeline } from "../src/timeline/Timeline";
import * as w from "./fixtures";
import { Figure, Row, Section, Sheet } from "./Sheet";
import { Agents, TopBar } from "./window";
import "./design.css";

// The timeline's design: where it sits in the window, the whole of it on one
// world, the edits as states, the worlds with less in them, and the track
// headers' toggles — drawn with the component the app wires
// (`src/timeline/LiveTimeline.tsx`), so the sheet stays its reference.

const W = 1440;
const H = 860;
const FULL = 1144;
const HALF = 476;

const selection2B = { label: "2B", detail: "OTS · 35 mm · 0:51.84 → 1:22.56 · 30.7 s" };
const windowTracks: Track[] = [w.shotTrack(w.shotsIn({ "2b": "selected" })), w.camera, w.soundtrack(false), w.modulations[0], w.behaviors[0], w.ambience[0]];

/** The window at full size, drawn at `scale`: the history (with its own
 *  transport) on the left, the world, the agents on the right, and the
 *  timeline across all three. */
function Window({ scale }: { scale: number }) {
  return (
    <div style={{ width: W * scale, height: H * scale }}>
      <div
        className="grid grid-cols-[300px_minmax(0,1fr)_480px] grid-rows-[36px_minmax(0,1fr)_auto] bg-panel text-[12px] text-ink"
        style={{ width: W, height: H, transform: `scale(${scale})`, transformOrigin: "0 0" }}
      >
        <TopBar />
        <div className="flex min-h-0 flex-col [&>aside]:flex-1">
          <HistoryPanel history={w.history} />
          <PlaybackBar history={w.history} playing={false} />
        </div>
        <Frame look={{ horizon: 0.58, sun: [0.64, 0.34], subject: "tower", at: 0.36 }} className="size-full" />
        <Agents />
        <div className="col-span-3 border-t border-line">
          <Timeline
            clock={w.clock}
            tracks={windowTracks}
            view={[0, w.clock.duration]}
            width={W - HEADER}
            playhead={64.2}
            snap="beats"
            selection={selection2B}
          />
        </div>
      </div>
    </div>
  );
}

const everything: Track[] = [
  w.shotTrack(w.shotsIn({ "2b": "selected", "3b": "proposed" })),
  w.camera,
  w.soundtrack(true),
  ...w.stems,
  ...w.modulations,
  ...w.behaviors,
  ...w.ambience,
];

const editTracks = (shots: Shot[]): Track[] => [w.shotTrack(shots), w.soundtrack(false), w.modulations[0]];
const BAR_31 = w.bar(31);
const THREE_B = w.shots.find((shot) => shot.id === "3b")!;

// A screenplay's previs with no song: the shot list is the clock.
const previsClock: Clock = { duration: 96, bpm: 0, beatOffset: 0, sections: [0] };
const previsShots: Shot[] = [
  ["1A", "WS", 24, 0, 12, "tower", 0.3],
  ["1B", "MS", 40, 12, 27.5, "jetty", 0.5],
  ["1C", "CU", 85, 27.5, 36, "lamp", 0.45],
  ["2A", "WS", 18, 36, 58, "boat", 0.5],
  ["2B", "OTS", 35, 58, 71, "boat", 0.62],
  ["2C", "CU", 75, 71, 96, "lamp", 0.55],
].map(([name, size, lens, from, to, subject, at]) => ({
  id: String(name),
  name: String(name),
  size: String(size),
  lens: Number(lens),
  from: Number(from),
  to: Number(to),
  look: { horizon: 0.6, sun: [0.7, 0.4], subject: subject as FrameLook["subject"], at: Number(at) },
}));

function Page() {
  return (
    <Sheet
      title="Timeline"
      lede="World time — the shots, the song and what the song moves — on one clock, across the window. Nothing here is behaviour: every state is drawn beside the others, with the component the app wires."
    >
      <Section
        title="Where it lives"
        lede="Across the whole window, under the history, the world and the agents. The history keeps its own transport, in the history panel."
        notes={
          <>
            <p>
              <b>Two clocks, never one bar.</b> Stepping through entries is authoring time — who changed what, in the order it was written — and belongs with the
              graph. The timeline's transport is world time: where the song is, which shot is on.
            </p>
            <p>
              <b>The window's width</b> goes to the lanes — 1,264 px, 6.6 px a second when the song fits — and the timeline's height comes from the terminal and the
              history. The terminal keeps 34 rows.
            </p>
            <p>
              <b>Not under the world alone</b>, which was the other way drawn: it keeps the terminal and the history at full height but leaves the lanes 484 px, a
              three-minute song at 2.5 px a second — enough to see the cuts and too little to see a beat. The timeline is the one panel that wants width.
            </p>
          </>
        }
      >
        <Row cols={1}>
          <Figure title="Across the window" caption="The history and the agents end above the timeline. The song bar under the world goes: its play and pause are the timeline's.">
            <Window scale={FULL / W} />
          </Figure>
        </Row>
      </Section>

      <Section
        title="The whole of it"
        lede="A music video's previs at the harbour, cut to a song from the starter pack: every kind of track the format has a clock for, at fit."
        notes={
          <>
            <p>Each track is a view of the world's document, never a copy of it:</p>
            <p>
              <b>Shots</b> — camera entities with <code>ext-cinematography</code>: name, size, lens, and <code>in_s</code>/<code>out_s</code> on the clock (previs's{" "}
              <code>ShotRow</code>).
            </p>
            <p>
              <b>Camera</b> — the world's tour: waypoints, with each <code>pause_duration</code> drawn as a hold.
            </p>
            <p>
              <b>Soundtrack</b> — <code>SoundtrackDef</code>. Its <code>duration</code>, <code>bpm</code> and <code>beat_offset</code> make the ruler and the grid,
              <code>sections</code> the § band; its <code>stems</code> list under it. The waveform is the decoded song's peaks — <code>energy</code> is one value a
              second, too coarse to draw.
            </p>
            <p>
              <b>Modulations</b> — <code>ModulationDef</code>: the entity, target, signal and range; the curve is the signal sampled on the clock.
            </p>
            <p>
              <b>Behaviors</b> loop; their tracks show the period. <b>Ambience</b> is the world's ambient and emitter sound.
            </p>
            <p>The grid behind every lane is bars and sections, so a cut can be seen to land on the music.</p>
          </>
        }
      >
        <Row cols={1}>
          <Figure title="Harbour · Tidewater, 3:12 at 92 bpm" caption="2B is selected; 3B moved two bars later at claude's proposal and waits for review. The loop holds §3 and §4.">
            <Timeline
              clock={w.clock}
              tracks={everything}
              view={[0, w.clock.duration]}
              width={FULL}
              playhead={64.2}
              loop={[w.sectionAt(2), w.sectionAt(4)]}
              snap="beats"
              proposal={{ author: "claude", what: "3B two bars later · in 1:55.20 → 2:00.42" }}
            />
          </Figure>
        </Row>
      </Section>

      <Section
        title="Editing"
        lede="A person's edits and an agent's, zoomed in. Drags draw a ghost and write nothing until they are let go."
        notes={
          <>
            <p>
              <b>One edit, one entry.</b> A drag is the canvas's gesture rule again: the ghost follows the pointer, and on release one batch goes through the authority
              the agents use, so the history gets one entry per edit — and an agent can undo it like any other.
            </p>
            <p>
              <b>Snapping</b> follows the toolbar — beats, sections or cuts — and draws the line it snapped to with its name.
            </p>
            <p>
              <b>An agent's change waits</b> as a dashed clip with its author's name. Enter accepts and Esc rejects, as the canvas's review does; until then the world
              plays as it was.
            </p>
            <p>
              <b>The song's sections</b> are the ruler's § band: a boundary dragged along it moves where one section ends and the next begins, snapped to a bar. A
              double-click on the band starts a section there; a boundary dragged onto its neighbour joins the two. The song's curves stay as they are — only where
              its sections fall changes, one entry, as a trim is.
            </p>
            <p>
              <b>A tour's stops</b> hold: dragging the end of a hold lengthens the camera's wait there, and the stops after it come later by as much. The travel
              between stops is the tour's speed, and stays.
            </p>
          </>
        }
      >
        <Row>
          <Figure title="Select" caption="The selection's numbers in the toolbar, its edges as handles.">
            <Timeline clock={w.clock} tracks={editTracks(w.shotsIn({ "2b": "selected" }))} view={[44, 96]} width={HALF} playhead={64.2} snap="beats" selection={selection2B} />
          </Figure>
          <Figure title="Trim" caption="The cut between 2B and 3A rolls back to bar 31, where the lantern's pulse lands.">
            <Timeline
              clock={w.clock}
              tracks={editTracks(w.shotsIn({ "2b": "selected" }))}
              view={[44, 96]}
              width={HALF}
              playhead={64.2}
              snap="beats"
              selection={{ label: "2B", detail: "out 1:22.56 → 1:18.66 · −3.90 s" }}
              drag={{ track: "shots", from: w.sectionAt(2), to: BAR_31, label: "2B | 3A at bar 31", snapped: { at: BAR_31, label: "bar 31" } }}
            />
          </Figure>
        </Row>
        <Row>
          <Figure title="Move" caption="3B dragged back into the gap snaps to the bridge's first beat.">
            <Timeline
              clock={w.clock}
              tracks={editTracks(w.shotsIn({ "3b": "hover" }))}
              view={[100, 156]}
              width={HALF}
              playhead={118}
              snap="sections"
              selection={{ label: "3B", detail: "in 2:00.42 → 1:55.20 · −5.22 s" }}
              drag={{
                track: "shots",
                from: w.sectionAt(4),
                to: w.sectionAt(4) + (THREE_B.to - THREE_B.from),
                label: "3B at 1:55.20",
                snapped: { at: w.sectionAt(4), label: "§5 · 1:55.20" },
              }}
            />
          </Figure>
          <Figure title="An agent's proposal" caption="claude moved 3B; the toolbar asks, and the clip waits dashed under its name.">
            <Timeline
              clock={w.clock}
              tracks={editTracks(w.shotsIn({ "3b": "proposed" }))}
              view={[100, 156]}
              width={HALF}
              playhead={118}
              snap="sections"
              proposal={{ author: "claude", what: "3B two bars later" }}
            />
          </Figure>
        </Row>
        <Row>
          <Figure title="Recut a section" caption="The bridge, §5, drawn back a bar: the chorus before it ends there, and the shots and the curves stay put.">
            <Timeline
              clock={w.clock}
              tracks={editTracks(w.shotsIn({}))}
              view={[100, 156]}
              width={HALF}
              playhead={118}
              snap="beats"
              recut={{ section: 4, to: w.bar(44), label: "bar 44 · 1:52.57" }}
            />
          </Figure>
          <Figure title="Hold a stop longer" caption="The camera waits 1.5 s more at the third stop; the stops after it come 1.5 s later.">
            <Timeline
              clock={w.clock}
              tracks={[w.camera, w.soundtrack(false), w.modulations[0]]}
              view={[70, 126]}
              width={HALF}
              playhead={84}
              snap="beats"
              drag={{ track: w.camera.id, from: w.sectionAt(3), to: w.sectionAt(3) + 4.5, label: "holds 4.5 s · the stops after +1.5 s" }}
            />
          </Figure>
        </Row>
      </Section>

      <Section
        title="Worlds with less in them"
        lede="The same timeline when a world has no shots, no song, or no clock at all."
        notes={
          <>
            <p>
              <b>A song world</b> — what a cold start opens. No shots and no tour: the soundtrack is the clock, and what it moves is most of what there is.
            </p>
            <p>
              <b>A screenplay's previs</b> has no song: the shot list is the clock, its last out point the duration. There is no beat grid, so the bar readout goes and
              snapping offers cuts only.
            </p>
            <p>
              <b>No clock</b>: behaviors loop without one, and the timeline says so and stays small.
            </p>
          </>
        }
      >
        <Row cols={1}>
          <Figure title="A song world" caption="Tidewater with its stems open, and everything it drives, playing.">
            <Timeline
              clock={w.clock}
              tracks={[w.soundtrack(true), ...w.stems, ...w.modulations, ...w.behaviors, ...w.ambience]}
              view={[0, w.clock.duration]}
              width={FULL}
              playhead={131.5}
              playing
              snap="beats"
            />
          </Figure>
        </Row>
        <Row>
          <Figure title="A screenplay's previs" caption="Six shots over two scenes; 96 s on the clock.">
            <Timeline
              clock={previsClock}
              tracks={[w.shotTrack(previsShots), { ...w.camera, keys: [{ t: 0, hold: 3 }, { t: 12 }, { t: 36, hold: 4 }, { t: 58 }, { t: 96 }] }]}
              view={[0, 96]}
              width={HALF}
              playhead={40.5}
              snap="shots"
            />
          </Figure>
          <Figure title="No clock" caption="An agent's world with behaviors and nothing timed.">
            <Timeline clock={null} tracks={[]} view={[0, 1]} width={HALF} playhead={0} />
          </Figure>
        </Row>
      </Section>

      <Section
        title="Track headers"
        lede="What a header's toggles look like on and off."
        notes={
          <p>
            Seen tracks hide; heard tracks mute and solo; any track locks. A toggle is quiet until it is on. A locked track takes no drags from a person; an agent's
            ops still land on it, and show.
          </p>
        }
      >
        <Row>
          <Figure title="Seen" caption="Shown, hidden, locked.">
            <Timeline
              clock={w.clock}
              tracks={[w.behaviors[0], { ...w.behaviors[1], flags: { hidden: true } }, { ...w.behaviors[2], flags: { locked: true } }, w.shotTrack(w.shotsIn({ "1a": "locked", "1b": "locked", "2a": "locked", "2b": "locked" }), true)]}
              view={[0, 60]}
              width={HALF}
              playhead={22}
              bare
            />
          </Figure>
          <Figure title="Heard" caption="Playing, muted, solo, locked.">
            <Timeline
              clock={w.clock}
              tracks={[w.ambience[0], { ...w.ambience[1], flags: { muted: true } }, { ...w.stems[0], flags: { solo: true } }, { ...w.stems[1], flags: { locked: true } }]}
              view={[0, 60]}
              width={HALF}
              playhead={22}
              bare
            />
          </Figure>
        </Row>
      </Section>
    </Sheet>
  );
}

createRoot(document.getElementById("root")!).render(<Page />);
