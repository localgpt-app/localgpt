import type { ReactNode } from "react";
import { createRoot } from "react-dom/client";
import type { TermTab } from "../src/bridge";
import { HistoryPanel } from "../src/HistoryPanel";
import { PlaybackBar } from "../src/PlaybackBar";
import { Body, type Layout } from "../src/phone/layout";
import { PhoneHeader, PhoneNav, ProposalBar, TabChips, Unpaired, type View, WorldFrame } from "../src/phone/Phone";
import { Compose, KeyRow } from "../src/phone/PhoneTerminal";
import { WorldPane } from "../src/phone/WorldPane";
import { Frame } from "../src/timeline/Frame";
import * as w from "./fixtures";
import { Figure, Row, Section, Sheet } from "./Sheet";
import "./design.css";

// A paired phone's page, for review: the terminals, the world and a
// proposal, the history — then on a tablet, and pairing. Drawn with the
// components the app wires (`src/phone/`), so the sheet stays their
// reference; the terminal is a transcript. The window's Phone button is the
// shell's (`crates/app/src/shell/bar.rs`).

const PHONE = { w: 390, h: 844 };
/** An 11-inch iPad held upright, in CSS pixels; turned, the other way. */
const TABLET = { w: 820, h: 1180 };
/** What an iPad's keyboard takes of an upright screen, its suggestions included. */
const KEYBOARD = 398;
const live = { state: "live" as const, why: null, device: "iPhone" };
const onIpad = { ...live, device: "iPad" };

const TRANSCRIPT = `> cut 3B two bars later, so it
  lands on the bridge's second
  phrase

  Reading the shot list through
  the world's MCP. 3B starts with
  the bridge at 1:55.20; two bars
  at 92 bpm is 5.22 s.

  world.submit — 1 op:
  3B in_s 115.20 → 120.42

  Proposed. It waits in the
  timeline for review.

────────────────────────────────
> 
────────────────────────────────
  ? for shortcuts`;

const ASKING = `  world.submit — 1 op:
  3B in_s 115.20 → 120.42

────────────────────────────────
 Do you want to make this edit
 to the world?

 ❯ 1. Yes
   2. Yes, and don't ask again
      this session
   3. No, and tell Claude what
      to do differently
────────────────────────────────`;

/** The same agent at a tablet's width. */
const WIDE = `~/worlds/harbour.world $ claude

> cut 3B two bars later, so it lands on the bridge's second phrase

  Reading the shot list through the world's MCP. 3B starts with the
  bridge at 1:55.20; two bars at 92 bpm is 5.22 s.

  world.submit — 1 op: 3B in_s 115.20 → 120.42

  Proposed. It waits in the timeline for review.

──────────────────────────────────────────────────────────────────────
>
──────────────────────────────────────────────────────────────────────
  ? for shortcuts`;

const WIDE_ASKING = `> hold the last stop of the dusk tour a beat longer

  The dusk tour ends at the lantern, held 2.0 s; a beat at 92 bpm is
  0.65 s.

  world.submit — 1 op: tour "dusk" stop 4 hold 2.00 → 2.65

──────────────────────────────────────────────────────────────────────
 Do you want to make this edit to the world?

 ❯ 1. Yes
   2. Yes, and don't ask again this session
   3. No, and tell Claude what to do differently
──────────────────────────────────────────────────────────────────────`;

const tabs: TermTab[] = [
  { id: "t1", cols: 46, rows: 30, label: "Cut 3B on the bridge" },
  { id: "t2", cols: 46, rows: 30, label: null },
];

/** The device, at an iPhone's size. */
function Phone({ view = "terminals", children }: { view?: View; children: ReactNode }) {
  return (
    <div className="flex flex-col overflow-hidden bg-panel text-[13px] text-ink" style={{ width: PHONE.w, height: PHONE.h }}>
      {children}
      <PhoneNav view={view} />
    </div>
  );
}

function Terminal({ text }: { text: string }) {
  return <pre className="m-0 min-h-0 flex-1 overflow-hidden px-2.5 py-2 font-mono text-[12px] leading-[17px] text-ink/90">{text}</pre>;
}

/** A tablet's page at its size in CSS pixels, drawn at `scale`: the
 *  header, then the two panes as the app lays them out. */
function Tablet({ layout, w, h, scale, proposal, terminal, world }: { layout: Layout; w: number; h: number; scale: number; proposal?: boolean; terminal: ReactNode; world: ReactNode }) {
  return (
    <div style={{ width: w * scale, height: h * scale }}>
      <div className="flex flex-col overflow-hidden bg-panel text-[13px] text-ink" style={{ width: w, height: h, transform: `scale(${scale})`, transformOrigin: "0 0" }}>
        <PhoneHeader title="World · harbour.world" line={onIpad} />
        {proposal && <ProposalBar proposal={{ author: "claude", what: "3B two bars later, on the bridge's second phrase", shots: [] }} live />}
        <Body layout={layout} view="terminals" terminals={terminal} world={world} />
      </div>
    </div>
  );
}

function App() {
  return (
    <Sheet
      title="A phone or a tablet"
      lede={
        <>
          The window's page on a phone or a tablet, served by the app on the local network: the agents' own terminals to read and answer, the world as
          the window shows it, the history. Paired once by a QR code; plain HTTP, for a network the person trusts. Nothing runs on the phone — the agents
          stay on the computer, whole (<code>docs/rfcs/authoring/mobile-remote.md</code>).
        </>
      }
    >
      <Section
        title="The terminals, at the thumb"
        lede="Each tab as a chip, labelled with what its program calls itself in its title. The terminal is the program's own — a coding agent, whatever the person started — with the keys a phone lacks under it and a box to write or dictate a prompt."
        notes={
          <>
            <p>
              <b>The screen, not the history:</b> the app keeps each tab's screen as a terminal would, so a phone that arrives late is sent what the
              terminal shows.
            </p>
            <p>
              <b>One size:</b> a PTY has one. While the window drives it, the phone draws the screen at the window's size, scaled to fit; once the phone
              types or is tapped into, the tab takes the phone's size, and the window takes it back when it is clicked into.
            </p>
            <p>
              <b>No agent is known to the app:</b> a tab is a login shell in the world's folder, and what runs in it is the person's. The app reads
              nothing from it but its title.
            </p>
            <p>
              <b>The prompt box</b> sends one paste and an Enter, so its new lines stay in the prompt; the phone's own dictation writes into it.
            </p>
          </>
        }
      >
        <Row cols={3}>
          <Figure title="An agent at work" caption="The chip's label is what the program calls the task in its title.">
            <Phone>
              <PhoneHeader title="World · harbour.world" line={live} />
              <TabChips tabs={tabs} active="t1" />
              <Terminal text={TRANSCRIPT} />
              <KeyRow live compose={false} />
            </Phone>
          </Figure>
          <Figure title="A second terminal" caption="Opened from the +, in the same folder; a shell until something is started in it.">
            <Phone>
              <PhoneHeader title="World · harbour.world" line={live} />
              <TabChips tabs={tabs} active="t2" />
              <Terminal text={"~/worlds/harbour.world $ "} />
              <KeyRow live compose={false} />
            </Phone>
          </Figure>
          <Figure title="It asks" caption="The arrows and Enter answer it, or the number on the phone's keyboard.">
            <Phone>
              <PhoneHeader title="World · harbour.world" line={live} />
              <TabChips tabs={tabs} active="t1" />
              <Terminal text={ASKING} />
              <KeyRow live compose />
              <Compose text="and after that, hold the last stop of the dusk tour a beat longer" live />
            </Phone>
          </Figure>
        </Row>
      </Section>

      <Section
        title="The world, and its history"
        lede="The world as the window shows it — a frame read back from the window, asked again as each arrives while the view is open — with the shots to look through and the song's play button. A proposal waits over every view, as Enter and Esc take it at the desk."
        notes={
          <>
            <p>
              <b>No video:</b> a JPEG of the world's part of the window, no wider than the phone, about 70 KB, two or three a second while the view is
              open and none when it is not.
            </p>
            <p>
              <b>The history</b> is the window's own panel and transport, at the phone's width.
            </p>
            <p>
              <b>Not on a phone:</b> Open…, the timeline, the outliner and the document — the window's work, at its width.
            </p>
          </>
        }
      >
        <Row cols={3}>
          <Figure title="The world" caption="Tap a shot to look through its camera; Free camera goes back.">
            <Phone view="world">
              <PhoneHeader title="Song · Tidewater" line={live} />
              <WorldFrame frame={null} shots={w.shots.slice(0, 6)} thumbs={{}} clock still={<Frame look={w.shots[0].look!} className="size-full" />} />
            </Phone>
          </Figure>
          <Figure title="A proposal waiting" caption="Claude's change, the person's to take or leave.">
            <Phone view="world">
              <PhoneHeader title="World · harbour.world" line={live} />
              <ProposalBar proposal={{ author: "claude", what: "3B two bars later, on the bridge's second phrase", shots: [] }} live />
              <WorldFrame frame={null} shots={w.shots.slice(3, 7)} thumbs={{}} clock={false} still={<Frame look={w.shots[5].look!} className="size-full" />} />
            </Phone>
          </Figure>
          <Figure title="The history" caption="Each entry, by whom; a tap shows it in the world on the computer too.">
            <Phone view="history">
              <PhoneHeader title="World · harbour.world" line={live} />
              <div className="flex min-h-0 flex-1 flex-col [&>aside]:min-h-0 [&>aside]:flex-1 [&>aside]:border-r-0">
                <HistoryPanel history={w.history} bare />
                <PlaybackBar history={w.history} playing={false} />
              </div>
            </Phone>
          </Figure>
        </Row>
      </Section>

      <Section
        title="A tablet"
        lede="The world and the terminals at once: side by side when the tablet is turned, as the window has them, and the world over the terminals when it is upright. The world's pane keeps the history and the shots in a tab each under its frame, so a tap on an entry shows it above."
        notes={
          <>
            <p>
              <b>By the page's width</b>, on a screen whose short side is 600 px or more: two panes from 600 px, side by side from 1100, or from 900
              when the tablet is turned. Narrower — an iPad in Split View — gets the phone's three views, and a phone turned sideways keeps them.
            </p>
            <p>
              <b>Never by the height:</b> the keyboard takes it, and a layout that changed with it would move the terminal from under the fingers
              typing. Upright, the world keeps a third of what is above the keyboard, between 180 and 440 px.
            </p>
            <p>
              <b>A turn keeps the terminals:</b> they sit first in every layout, so each keeps its screen; the PTY takes the new size when the tablet
              next types.
            </p>
            <p>
              <b>The history first</b>, when the world keeps one: what the agents did, entry by entry. A world with neither history nor shots — a
              document's — is the frame alone.
            </p>
            <p>
              <b>Frames while the page is open</b>, since the world is always in view: two or three a second, about 70 KB each, none while the page
              is hidden.
            </p>
          </>
        }
      >
        <Row cols={1}>
          <Figure title="Turned: 1180 × 820" caption="The world beside the terminals, as in the window, the history under its frame. The proposal waits over both.">
            <Tablet
              layout="side"
              w={TABLET.h}
              h={TABLET.w}
              scale={0.75}
              proposal
              terminal={
                <>
                  <TabChips tabs={tabs} active="t1" />
                  <Terminal text={WIDE} />
                  <KeyRow live compose={false} />
                </>
              }
              world={
                <WorldPane
                  dir="column"
                  frame={null}
                  still={<Frame look={w.shots[5].look!} className="size-full" />}
                  shots={w.shots}
                  thumbs={{}}
                  clock={false}
                  history={w.history}
                  playing={false}
                  tab="History"
                />
              }
            />
          </Figure>
        </Row>
        <Row cols={2}>
          <Figure title="Upright: 820 × 1180" caption="The world over the terminals, its frame beside the shots; the terminal gets the rest, more than a hundred columns wide.">
            <Tablet
              layout="stacked"
              w={TABLET.w}
              h={TABLET.h}
              scale={0.55}
              terminal={
                <>
                  <TabChips tabs={tabs} active="t1" />
                  <Terminal text={WIDE} />
                  <KeyRow live compose={false} />
                </>
              }
              world={
                <WorldPane
                  dir="row"
                  frame={null}
                  still={<Frame look={w.shots[0].look!} className="size-full" />}
                  shots={w.shots}
                  thumbs={{}}
                  clock
                  history={w.history}
                  playing={false}
                  tab="Shots"
                />
              }
            />
          </Figure>
          <Figure title="Upright, typing" caption="The keyboard up: the page is what is above it, and the world keeps a third of that.">
            <Tablet
              layout="stacked"
              w={TABLET.w}
              h={TABLET.h - KEYBOARD}
              scale={0.55}
              terminal={
                <>
                  <TabChips tabs={tabs} active="t1" />
                  <Terminal text={WIDE_ASKING} />
                  <KeyRow live compose />
                  <Compose text="and after that, start the boat's rocking on the chorus" live />
                </>
              }
              world={
                <WorldPane
                  dir="row"
                  frame={null}
                  still={<Frame look={w.shots[4].look!} className="size-full" />}
                  shots={w.shots}
                  thumbs={{}}
                  clock={false}
                  history={w.history}
                  playing={false}
                  tab="History"
                />
              }
            />
          </Figure>
        </Row>
      </Section>

      <Section
        title="Pairing, and a lost line"
        lede="A device with no key — never paired, or forgotten on the computer — is told how to pair. A line that drops is tried again, at once when the phone wakes, and the page asks for everything afresh when it is back."
        notes={
          <>
            <p>
              <b>Nothing typed while the line is down is kept:</b> keys sent minutes late into an agent's terminal would do harm.
            </p>
            <p>
              <b>Forget this computer</b> (the ⋯ menu) drops the key on the device; the computer forgets a device from its own panel.
            </p>
          </>
        }
      >
        <Row cols={3}>
          <Figure title="Not paired" caption="The steps, from the computer's side.">
            <div style={{ width: PHONE.w, height: PHONE.h }}>
              <Unpaired line={{ state: "unpaired", why: null, device: null }} />
            </div>
          </Figure>
          <Figure title="A code used twice" caption="Or expired, or forgotten on the computer: the reason, under the steps.">
            <div style={{ width: PHONE.w, height: PHONE.h }}>
              <Unpaired line={{ state: "unpaired", why: "That code is used or expired: show a new one on the computer.", device: null }} />
            </div>
          </Figure>
          <Figure title="The line lost" caption="The dot goes red; the terminals are still there when it is back.">
            <Phone>
              <PhoneHeader title="World · harbour.world" line={{ state: "offline", why: null, device: "iPhone" }} />
              <TabChips tabs={tabs} active="t1" />
              <Terminal text={TRANSCRIPT} />
              <KeyRow live={false} compose={false} />
            </Phone>
          </Figure>
        </Row>
      </Section>
    </Sheet>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
