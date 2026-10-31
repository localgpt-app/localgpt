import type { ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { Inspector } from "../src/scene/Inspector";
import { Outliner } from "../src/scene/Outliner";
import { Frame } from "../src/timeline/Frame";
import { HEADER, Timeline } from "../src/timeline/Timeline";
import * as w from "./fixtures";
import { byId, counts, harbour } from "./scene-fixtures";
import { Figure, Row, Section, Sheet } from "./Sheet";
import { Agents, TopBar } from "./window";
import "./design.css";

// The outliner and the inspector, for review before any of it is wired:
// where they sit in the window, the tree and what it shows, the selected
// thing's fields, the edits as states, and the worlds with less in them.

const W = 1440;
const H = 860;
const PANEL = 300;

/** The left panel's tabs: the history and the outliner share it. */
function Tabs({ active }: { active: "History" | "Outliner" }) {
  return (
    <div className="flex h-9 shrink-0 items-center gap-1 border-b border-line px-2">
      {(["History", "Outliner"] as const).map((tab) => (
        <span key={tab} className={`rounded px-2 py-1 ${tab === active ? "bg-raised text-ink" : "text-muted"}`}>
          {tab}
        </span>
      ))}
      <span className="ml-auto pr-1 text-muted tabular-nums">{active === "Outliner" ? `${harbour.nodes.size} things` : "7 entries"}</span>
    </div>
  );
}

const lanternOpen = new Set([30, 37]);

/** The outliner over the inspector, as the left panel holds them. */
function Scene({ height, selected = [37], expanded = lanternOpen, inspector }: { height: number; selected?: number[]; expanded?: Set<number>; inspector?: ReactNode }) {
  return (
    <div className="flex flex-col bg-panel text-[12px] text-ink" style={{ width: PANEL, height }}>
      <Tabs active="Outliner" />
      <div className="flex min-h-0 flex-[11] flex-col [&>aside]:flex-1">
        <Outliner nodes={harbour.nodes} roots={harbour.roots} expanded={expanded} selected={selected} />
      </div>
      <div className="h-px shrink-0 bg-line" />
      <div className="flex min-h-0 flex-[9] flex-col [&>aside]:flex-1">
        {inspector ?? <Inspector entity={byId(37)} parent="jetty" children={1} editable />}
      </div>
    </div>
  );
}

/** A panel drawn alone, at its size in the window. */
function Alone({ height, children }: { height: number; children: ReactNode }) {
  return (
    <div className="flex flex-col bg-panel text-[12px] text-ink [&>aside]:min-h-0 [&>aside]:flex-1" style={{ width: PANEL, height }}>
      {children}
    </div>
  );
}

/** The window at full size, drawn at `scale`, the left panel on its
 *  Outliner tab. */
function Window({ scale }: { scale: number }) {
  return (
    <div style={{ width: W * scale, height: H * scale }}>
      <div
        className="grid bg-panel text-[12px] text-ink"
        style={{
          width: W,
          height: H,
          gridTemplateColumns: `${PANEL}px minmax(0,1fr) 480px`,
          gridTemplateRows: "36px minmax(0,1fr) auto",
          transform: `scale(${scale})`,
          transformOrigin: "0 0",
        }}
      >
        <TopBar />
        <div className="flex min-h-0 flex-col border-r border-line">
          <Scene height={H - 36 - 144} />
        </div>
        <Frame look={{ horizon: 0.58, sun: [0.64, 0.34], subject: "lamp", at: 0.6 }} className="size-full" />
        <Agents />
        <div className="border-t border-line" style={{ gridColumn: "1 / -1" }}>
          <Timeline clock={w.clock} tracks={[w.shotTrack(w.shotsIn({})), w.soundtrack(false)]} view={[0, w.clock.duration]} width={W - HEADER} playhead={64.2} snap="beats" />
        </div>
      </div>
    </div>
  );
}

const lantern = byId(37);
const lifted = { ...lantern, transform: { ...lantern.transform, position: [0.7, 1.75, -8.4] as [number, number, number] } };
const moved = { ...lantern, transform: { ...lantern.transform, position: [1.1, 1.35, -7.9] as [number, number, number] } };
const boulder = {
  id: 7,
  name: "hero_01_boulder_01",
  transform: { position: [3.2, 0, -5.5] as [number, number, number], scale: [1.4, 1.1, 1.3] as [number, number, number] },
  mesh_asset: { path: "models/rocks/boulder_01.glb" },
  modulations: [{ target: "scale", signal: "energy", range: [0.95, 1.08] as [number, number] }],
};

function Page() {
  return (
    <Sheet
      title="Outliner and inspector"
      lede="The world's things as a tree, and the selected thing's fields — both read from the world's document, so they show what was written, not a frame of the song caught mid-beat. For review before any of it is wired."
    >
      <Section
        title="Where it lives"
        lede="In the left panel, in a tab beside the history: the outliner above, the inspector under it, the timeline across the window as before."
        notes={
          <>
            <p>
              <b>One selection.</b> A click in the world, in the outliner or on the timeline selects the same thing; the inspector shows it, and the agents read it as{" "}
              <code>GET /selection</code>.
            </p>
            <p>
              <b>A tab, not a column.</b> The history and the outliner are both the world's structure — one in time, one in space — and only one is read at a time, so
              they share the panel. The world and the agents keep their width, and the history is a click away.
            </p>
            <p>
              <b>Not chosen</b>: a column of their own beside the world, the history kept in view. The world lost 300 px to it and was left at 360 — too little to frame
              a shot.
            </p>
          </>
        }
      >
        <Row cols={1}>
          <Figure title="The left panel's Outliner tab" caption="The tree above, the selected lantern's fields below; the world, the agents and the timeline as they were.">
            <Window scale={0.7} />
          </Figure>
        </Row>
      </Section>

      <Section
        title="The outliner"
        lede={`The world's ${harbour.nodes.size} things, what holds what, and what moves each one.`}
        notes={
          <>
            <p>
              <b>A row</b> is a thing: what kind it is (shape, mesh, light, camera, sound, or a group that only holds others), its name, and after it what moves it — a
              behavior, the song — and a camera's shot.
            </p>
            <p>
              <b>The tree is the document's</b>, rebuilt whenever it changes; which groups are open is the page's own, kept by id, so an agent's edit never folds what you
              opened.
            </p>
            <p>
              <b>Finding</b> keeps what matches and the way down to it, open. The kinds filter the same way. An eye shut hides a thing in the window only — like the
              timeline's, it changes nothing in the world.
            </p>
            <p>
              <b>Others at work</b>: a row an agent just changed says who, for a while; one its waiting proposal touches is dashed, under its name.
            </p>
          </>
        }
      >
        <Row cols={3}>
          <Figure title="The harbour" caption="The lantern selected, the boat with it (shift-click); the sea hidden while the jetty is worked on.">
            <Alone height={430}>
              <Outliner
                nodes={new Map([...harbour.nodes].map(([id, node]) => [id, id === 2 ? { ...node, visible: false } : node]))}
                roots={harbour.roots}
                expanded={new Set([30, 37])}
                selected={[37, 40]}
                hover={31}
              />
            </Alone>
          </Figure>
          <Figure title="Found by name" caption="“lan” finds the lantern, its light and the planks, and opens the jetty to them.">
            <Alone height={430}>
              <Outliner nodes={harbour.nodes} roots={harbour.roots} expanded={new Set()} query="lan" selected={[37]} />
            </Alone>
          </Figure>
          <Figure title="Others at work" caption="claude moved the boat 20 s ago; gemini's proposal for a gull waits for you.">
            <Alone height={430}>
              <Outliner
                nodes={harbour.nodes}
                roots={harbour.roots}
                expanded={new Set([30, 50])}
                changed={new Map([[40, "claude · 20 s"]])}
                proposed={new Map([[51, "gemini"]])}
              />
            </Alone>
          </Figure>
        </Row>
        <Row cols={3}>
          <Figure title="Lights only" caption="The kinds narrow the tree as a name does.">
            <Alone height={300}>
              <Outliner nodes={harbour.nodes} roots={harbour.roots} expanded={new Set()} kinds={["light"]} />
            </Alone>
          </Figure>
          <Figure title="Into a group" caption="The lantern held over the posts: let go, and it is theirs, standing where it stood.">
            <Alone height={300}>
              <Outliner
                nodes={harbour.nodes}
                roots={harbour.roots}
                expanded={new Set([30, 32])}
                drop={{ held: 37, id: 32, zone: "into", label: "into posts · stays where it is" }}
              />
            </Alone>
          </Figure>
          <Figure title="Beside a row" caption="Its light held after the posts: it leaves the lantern for the jetty. ⌥ keeps its offset instead of its place.">
            <Alone height={300}>
              <Outliner
                nodes={harbour.nodes}
                roots={harbour.roots}
                expanded={new Set([30, 37])}
                drop={{ held: 38, id: 32, zone: "after", label: "into the jetty, after posts" }}
              />
            </Alone>
          </Figure>
        </Row>
      </Section>

      <Section
        title="The inspector"
        lede="The selected thing, part by part, as its document writes it."
        notes={
          <>
            <p>
              <b>Parts</b> appear for what a thing has: every thing has a transform; a shape, a material, a light, a camera and its shot, a mesh, behaviors, what the
              song moves, a sound, only when it carries them.
            </p>
            <p>
              <b>What the song moves</b> lists each modulation with a way to its track on the timeline. A camera's shot gives <b>Look through</b>, as a double-click on its
              clip does.
            </p>
            <p>
              Values are the document's: a lantern pulsing on the beat shows the glow it was given, and the timeline shows what the song does to it.
            </p>
          </>
        }
      >
        <Row cols={3}>
          <Figure title="The lantern" caption="A shape with a glowing material, moved by the beat.">
            <Alone height={560}>
              <Inspector entity={lantern} parent="jetty" children={1} editable />
            </Alone>
          </Figure>
          <Figure title="The beam" caption="A spot light that turns: a light and a behavior.">
            <Alone height={560}>
              <Inspector entity={byId(24)} parent="lighthouse" editable />
            </Alone>
          </Figure>
          <Figure title="2B" caption="A camera in the shot list: its lens and aim, and the shot's place on the clock.">
            <Alone height={560}>
              <Inspector entity={byId(64)} parent="shots" editable />
            </Alone>
          </Figure>
        </Row>
      </Section>

      <Section
        title="Editing"
        lede="A person's edits and an agent's, as the timeline's are: one entry each, through the authority the agents use."
        notes={
          <>
            <p>
              <b>A drag on a label</b> scrubs the value, the world following; let go, and it is one entry in the history. A click types a value instead; Enter commits it,
              Esc lets it go.
            </p>
            <p>
              <b>A refusal</b> is the authority's own words under the field, and the field goes back to what the world holds.
            </p>
            <p>
              <b>A selection of several</b> edits them together: a field they hold alike shows its value, one they differ in shows a dash until it is set.
            </p>
          </>
        }
      >
        <Row cols={3}>
          <Figure title="Scrubbing a value" caption="The lantern lifted 0.40 m by dragging Position's Y.">
            <Alone height={470}>
              <Inspector entity={lifted} parent="jetty" children={1} editable marks={{ "transform.position.1": { scrub: "+0.40 m · one entry when let go" } }} />
            </Alone>
          </Figure>
          <Figure title="Refused" caption="A radius of 0 isn't a cylinder; the world keeps the one it had.">
            <Alone height={470}>
              <Inspector
                entity={lantern}
                parent="jetty"
                children={1}
                editable
                marks={{ "shape.radius": { refused: "refused: a cylinder's radius is above 0 — the world is as it was" } }}
              />
            </Alone>
          </Figure>
          <Figure title="Changed by an agent" caption="claude moved the lantern along the jetty; the fields say so for a while.">
            <Alone height={470}>
              <Inspector
                entity={moved}
                parent="jetty"
                children={1}
                editable
                marks={{ "transform.position.0": { changed: "claude · 20 s ago" }, "transform.position.2": { changed: "claude · 20 s ago" } }}
              />
            </Alone>
          </Figure>
        </Row>
        <Row cols={3}>
          <Figure title="Several selected" caption="Three posts: they share a height and a size, not a place.">
            <Alone height={300}>
              <Inspector entity={byId(33)} others={["post 2", "post 3"]} parent="posts" editable mixed={["transform.position.0", "transform.position.2"]} />
            </Alone>
          </Figure>
        </Row>
      </Section>

      <Section
        title="Worlds with less"
        lede="Nothing selected, and a world that isn't a package."
        notes={
          <>
            <p>
              <b>Nothing selected</b>: the inspector says what the world holds, and how to select.
            </p>
            <p>
              <b>Not a package</b> — a song's world, a document's — reads the same and edits nothing: the song's or the document's own source is what changes it, and
              the panel says where.
            </p>
          </>
        }
      >
        <Row cols={3}>
          <Figure title="Nothing selected" caption="The harbour in a few lines.">
            <Alone height={300}>
              <Inspector entity={null} editable world={{ name: "harbour.world", counts, sky: [0.66, 0.47, 0.52, 1], song: "Tidewater — Starter pack", tours: 1, shots: 5 }} />
            </Alone>
          </Figure>
          <Figure title="A song's world" caption="A boulder that breathes with the song: there to read, not to change.">
            <Alone height={460}>
              <Inspector entity={boulder} editable={false} />
            </Alone>
          </Figure>
          <Figure title="In the panel" caption="Both at once, as the left panel holds them.">
            <Scene height={460} selected={[64]} expanded={new Set([60])} inspector={<Inspector entity={byId(64)} parent="shots" editable />} />
          </Figure>
        </Row>
      </Section>
    </Sheet>
  );
}

createRoot(document.getElementById("root")!).render(<Page />);
