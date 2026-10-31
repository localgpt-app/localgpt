import type { ReactNode } from "react";
import { createRoot } from "react-dom/client";
import type { Activity, RemoteView, TermTab } from "../src/bridge";
import { HistoryPanel } from "../src/HistoryPanel";
import { Icon } from "../src/Icon";
import { PlaybackBar } from "../src/PlaybackBar";
import { PhoneHeader, PhoneNav, ProposalBar, TabChips, TurnNote, Unpaired, type View, WorldFrame } from "../src/phone/Phone";
import { Compose, KeyRow } from "../src/phone/PhoneTerminal";
import { RemotePanel } from "../src/RemotePanel";
import { Frame } from "../src/timeline/Frame";
import * as w from "./fixtures";
import { Figure, Row, Section, Sheet } from "./Sheet";
import "./design.css";

// A paired phone's page, for review: the window's Phone button and what it
// shows, then the page on the phone — the agents, a turn that ended, the
// world and a proposal, the history — and pairing. Drawn with the
// components the app wires (`src/phone/`, `src/RemotePanel.tsx`), so the
// sheet stays their reference; the terminal is a transcript, as on the
// other sheets.

const PHONE = { w: 390, h: 844 };
const live = { state: "live" as const, why: null, device: "iPhone" };

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

const tabs = (claude: Activity | null): TermTab[] => [
  { id: "t1", kind: "claude", cols: 46, rows: 30, label: "Cut 3B on the bridge", activity: claude },
  { id: "t2", kind: "codex", cols: 46, rows: 30, label: null, activity: null },
];

/** A pairing code as the app draws one: a stand-in pattern, not a code. */
function standInQr(): string {
  const n = 25;
  let cells = "";
  for (let y = 0; y < n; y++)
    for (let x = 0; x < n; x++) {
      const finder = (cx: number, cy: number) => x >= cx && x < cx + 7 && y >= cy && y < cy + 7 && (x === cx || x === cx + 6 || y === cy || y === cy + 6 || (x >= cx + 2 && x <= cx + 4 && y >= cy + 2 && y <= cy + 4));
      const zone = (cx: number, cy: number) => x >= cx - 1 && x <= cx + 7 && y >= cy - 1 && y <= cy + 7;
      const on = finder(0, 0) || finder(18, 0) || finder(0, 18) || (!zone(0, 0) && !zone(18, 0) && !zone(0, 18) && (x * 7 + y * 13 + x * y) % 3 === 0);
      if (on) cells += `<rect x="${x + 2}" y="${y + 2}" width="1" height="1"/>`;
    }
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 29 29"><rect width="29" height="29" fill="#fff"/><g fill="#0f1218">${cells}</g></svg>`;
}

const url = "http://192.168.1.20:9880/";
const views: Record<string, RemoteView> = {
  off: { on: false, url: null, pairing: null, devices: [], error: null },
  pairing: { on: true, url, pairing: { url: `${url}#pair=4f0c9a1d…`, qr: standInQr(), left_s: 581 }, devices: [], error: null },
  devices: {
    on: true,
    url,
    pairing: null,
    devices: [
      { id: "a1", name: "iPhone", paired_ms: 0, seen_ms: Date.now(), attached: true },
      { id: "b2", name: "iPad", paired_ms: 0, seen_ms: Date.now() - 3 * 3600_000, attached: false },
    ],
    error: null,
  },
};

/** The top bar's right end, with the Phone button as it shows each state. */
function Bar({ label, attached }: { label: string; attached?: boolean }) {
  return (
    <div className="flex h-9 w-80 items-center justify-end gap-3 border-b border-line bg-panel px-3 text-[12px]">
      <span className="truncate text-faint">3 entries · main at lantern</span>
      <span className={`flex shrink-0 items-center gap-1.5 rounded px-1.5 py-0.5 ${attached ? "text-ink" : "text-muted"}`}>
        <Icon name="phone" className="size-3.5" />
        {label}
        {attached && <span className="size-1.5 rounded-full bg-[#9ece6a]" />}
      </span>
    </div>
  );
}

function Panel({ view, label, attached }: { view: RemoteView; label: string; attached?: boolean }) {
  return (
    <div className="bg-[#0b0e13] pb-3">
      <Bar label={label} attached={attached} />
      <div className="relative h-[460px] w-80 [&>div]:top-2 [&>div]:right-0">
        <RemotePanel view={view} left={view.pairing?.left_s ?? 0} />
      </div>
    </div>
  );
}

/** The device, at an iPhone's size; `claude`, what its tab's agent is
 *  doing, for the dot on Agents. */
function Phone({ view = "agents", claude = null, children }: { view?: View; claude?: Activity | null; children: ReactNode }) {
  return (
    <div className="flex flex-col overflow-hidden bg-panel text-[13px] text-ink" style={{ width: PHONE.w, height: PHONE.h }}>
      {children}
      <PhoneNav view={view} tabs={tabs(claude)} />
    </div>
  );
}

function Terminal({ text }: { text: string }) {
  return <pre className="m-0 min-h-0 flex-1 overflow-hidden px-2.5 py-2 font-mono text-[12px] leading-[17px] text-ink/90">{text}</pre>;
}

function App() {
  return (
    <Sheet
      title="A phone"
      lede={
        <>
          The window's page on a phone or a tablet, served by the app on the local network: the agents' own terminals to read and answer, the world as
          the window shows it, the history. Paired once by a QR code; plain HTTP, for a network the person trusts. Nothing runs on the phone — the agents
          stay on the computer, whole (<code>docs/rfcs/authoring/mobile-remote.md</code>).
        </>
      }
    >
      <Section
        title="The Phone button"
        lede="At the right of the window's top bar. Off until it is turned on; on, it shows a code to pair with, and the devices, each of which can be forgotten. It says when one is attached."
        notes={
          <>
            <p>
              <b>Off by default</b>, and remembered once on. The app listens on port 9880 of every interface; the live API stays on 127.0.0.1.
            </p>
            <p>
              <b>The code works once</b>, for ten minutes, and is traded for a key the device keeps; the app keeps the key's hash, in a file only its
              owner can read. Turned on with nothing paired, the code shows at once.
            </p>
            <p>
              <b>Seen:</b> the button names the device attached, with a dot. A paired device that can type into the terminals is a shell on this
              computer, and the panel says so.
            </p>
          </>
        }
      >
        <Row cols={3}>
          <Figure title="Off" caption="What it is for, and the one button.">
            <Panel view={views.off} label="Phone" />
          </Figure>
          <Figure title="On, pairing" caption="Scan with the device's camera: the address and the code, in one link.">
            <Panel view={views.pairing} label="Phone: on" />
          </Figure>
          <Figure title="On, a device attached" caption="The iPhone is here now; the iPad was, three hours ago.">
            <Panel view={views.devices} label="iPhone" attached />
          </Figure>
        </Row>
      </Section>

      <Section
        title="The agents, at the thumb"
        lede="Each tab as a chip, its dot saying what the agent's title says: blue while it works, amber when it asks, green at rest. The terminal is the agent's own, with the keys a phone lacks under it and a box to write or dictate a prompt."
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
              <b>A turn that ended</b> — the spinner in Claude Code's title giving way to ✳ — shows over every view, buzzes where the phone can, and
              marks the history on a live canvas (<code>TurnEnded</code>).
            </p>
            <p>
              <b>The prompt box</b> sends one paste and an Enter, so its new lines stay in the prompt; the phone's own dictation writes into it.
            </p>
          </>
        }
      >
        <Row cols={3}>
          <Figure title="Claude at work" caption="The chip pulses; the task is what Claude calls it in its title.">
            <Phone claude="working">
              <PhoneHeader title="World · harbour.world" line={live} />
              <TabChips tabs={tabs("working")} active="t1" />
              <Terminal text={TRANSCRIPT} />
              <KeyRow live compose={false} />
            </Phone>
          </Figure>
          <Figure title="Its turn ended" caption="Over whatever is shown, until it is looked at; Show opens the tab.">
            <Phone view="world">
              <PhoneHeader title="World · harbour.world" line={live} />
              <TurnNote turn={{ id: "t1", kind: "claude", activity: "idle", task: "Cut 3B on the bridge" }} />
              <WorldFrame frame={null} shots={[]} thumbs={{}} clock={false} still={<Frame look={w.shots[5].look!} className="size-full" />} />
            </Phone>
          </Figure>
          <Figure title="It asks" caption="Amber. The arrows and Enter answer it, or the number on the phone's keyboard.">
            <Phone claude="asking">
              <PhoneHeader title="World · harbour.world" line={live} />
              <TurnNote turn={{ id: "t1", kind: "claude", activity: "asking", task: "Cut 3B on the bridge" }} />
              <TabChips tabs={tabs("asking")} active="t1" />
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
          <Figure title="The line lost" caption="The dot goes red; the agents are still there when it is back.">
            <Phone claude="working">
              <PhoneHeader title="World · harbour.world" line={{ state: "offline", why: null, device: "iPhone" }} />
              <TabChips tabs={tabs("working")} active="t1" />
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
