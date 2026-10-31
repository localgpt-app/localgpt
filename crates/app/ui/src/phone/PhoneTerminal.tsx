import { Unicode11Addon } from "@xterm/addon-unicode11";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef, useState } from "react";
import type { TermTab } from "../bridge";
import { decode, listen, send } from "../bridge";
import { Icon } from "../Icon";
import { THEME } from "../Terminal";

/** The keys a TUI wants and a phone's keyboard lacks: Esc interrupts
 *  Claude Code, Shift-Tab cycles its modes, the arrows pick, ^C stops. The
 *  arrows follow the terminal's cursor-key mode, as a keyboard's would. */
const KEYS: { label: string; aria: string; seq: (app: boolean) => string }[] = [
  { label: "esc", aria: "Escape", seq: () => "\x1b" },
  { label: "⇥", aria: "Tab", seq: () => "\t" },
  { label: "⇧⇥", aria: "Shift-Tab", seq: () => "\x1b[Z" },
  { label: "↑", aria: "Up", seq: (app) => (app ? "\x1bOA" : "\x1b[A") },
  { label: "↓", aria: "Down", seq: (app) => (app ? "\x1bOB" : "\x1b[B") },
  { label: "←", aria: "Left", seq: (app) => (app ? "\x1bOD" : "\x1b[D") },
  { label: "→", aria: "Right", seq: (app) => (app ? "\x1bOC" : "\x1b[C") },
  { label: "^C", aria: "Control-C", seq: () => "\x03" },
  { label: "⏎", aria: "Enter", seq: () => "\r" },
];

/** A tab on a phone. While another page drives the tab, its screen is drawn
 *  at the PTY's size and scaled to fit; once this device types (or the
 *  terminal is tapped), the PTY takes the size that fits here and the text
 *  is full size. It attaches each time the line comes up — the app sends
 *  the screen — and never closes the tab. */
export function PhoneTerminal({ tab, visible, live }: { tab: TermTab; visible: boolean; live: boolean }) {
  const box = useRef<HTMLDivElement>(null);
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<XTerm | null>(null);
  const asked = useRef<{ cols: number; rows: number } | null>(null);
  const [scale, setScale] = useState(1);
  const [compose, setCompose] = useState(false);
  const [text, setText] = useState("");

  const screen = () => host.current?.querySelector<HTMLElement>(".xterm-screen") ?? null;
  /** What fits the box at this font size: the size this device asks for. */
  const fits = () => {
    const xterm = term.current;
    const el = box.current;
    const drawn = screen();
    if (!xterm || !el || !drawn?.clientWidth) return null;
    const cell = { w: drawn.clientWidth / xterm.cols, h: drawn.clientHeight / xterm.rows };
    return {
      cols: Math.max(20, Math.floor((el.clientWidth - 8) / cell.w)),
      rows: Math.max(6, Math.floor((el.clientHeight - 8) / cell.h)),
    };
  };
  const rescale = () => {
    const el = box.current;
    const drawn = screen();
    if (!el || !drawn?.clientWidth) return;
    setScale(Math.min(1, (el.clientWidth - 8) / drawn.clientWidth, (el.clientHeight - 8) / drawn.clientHeight));
  };

  useEffect(() => {
    const id = tab.id;
    const xterm = new XTerm({
      cols: Math.max(tab.cols, 2),
      rows: Math.max(tab.rows, 2),
      fontFamily: '"SF Mono", Menlo, monospace',
      fontSize: 12,
      lineHeight: 1.15,
      scrollback: 2000,
      allowProposedApi: true,
      theme: THEME,
    });
    xterm.loadAddon(new Unicode11Addon());
    xterm.unicode.activeVersion = "11";
    xterm.open(host.current!);
    const textarea = xterm.textarea;
    // A phone's keyboard corrects and capitalises; a terminal wants neither.
    textarea?.setAttribute("autocorrect", "off");
    textarea?.setAttribute("autocapitalize", "off");
    textarea?.setAttribute("spellcheck", "false");
    const stop = listen((m) => {
      if (m.t === "term.data" && m.id === id) xterm.write(decode(m.b64));
      if (m.t === "term.size" && m.id === id) {
        xterm.resize(m.cols, m.rows);
        requestAnimationFrame(rescale);
      }
      if (m.t === "term.exit" && m.id === id) xterm.write(`\r\n\x1b[2m[${m.error ?? "ended"}]\x1b[0m\r\n`);
    });
    const input = xterm.onData((data) => send({ t: "term.input", id, data }));
    const focused = () => send({ t: "term.focus", id });
    textarea?.addEventListener("focus", focused);
    term.current = xterm;
    // The keyboard opening, a turn of the phone: what fits changes.
    const observer = new ResizeObserver(() => {
      rescale();
      const fit = fits();
      if (fit && (fit.cols !== asked.current?.cols || fit.rows !== asked.current?.rows)) {
        asked.current = fit;
        send({ t: "term.resize", id, ...fit });
      }
    });
    observer.observe(box.current!);
    return () => {
      observer.disconnect();
      stop();
      input.dispose();
      textarea?.removeEventListener("focus", focused);
      xterm.dispose();
      term.current = null;
    };
    // The tab's first size only seeds the terminal; term.size keeps it after.
  }, [tab.id]);

  // Attached each time the line comes up: the screen comes fresh.
  useEffect(() => {
    if (!live) return;
    const fit = fits() ?? { cols: tab.cols, rows: tab.rows };
    asked.current = fit;
    send({ t: "term.attach", id: tab.id, ...fit });
  }, [live, tab.id]);

  useEffect(() => {
    if (visible) requestAnimationFrame(rescale);
  }, [visible]);

  const key = (data: string) => send({ t: "term.input", id: tab.id, data });
  /** A prompt from the box: pasted, so its new lines stay new lines, then
   *  Enter on its own. */
  const submit = () => {
    const xterm = term.current;
    if (text) {
      const pasted = xterm?.modes.bracketedPasteMode ? `\x1b[200~${text}\x1b[201~` : text.replace(/\n/g, "\r");
      key(pasted);
      setTimeout(() => key("\r"), 60);
    } else {
      key("\r");
    }
    setText("");
  };

  return (
    <div className={`absolute inset-0 flex flex-col ${visible ? "" : "invisible"}`}>
      <div ref={box} className="relative min-h-0 flex-1 overflow-hidden">
        <div ref={host} className="phone-term absolute top-1 left-1 origin-top-left" style={{ transform: `scale(${scale})` }} />
      </div>
      <KeyRow live={live} compose={compose} onKey={(k) => key(k.seq(term.current?.modes.applicationCursorKeysMode ?? false))} onCompose={() => setCompose(!compose)} />
      {compose && <Compose text={text} live={live} onText={setText} onSend={submit} />}
    </div>
  );
}

/** The keys under a tab's terminal, and the prompt box's switch. */
export function KeyRow({ live, compose, onKey, onCompose }: { live: boolean; compose: boolean; onKey?: (key: (typeof KEYS)[number]) => void; onCompose?: () => void }) {
  return (
    <div className="flex shrink-0 items-center gap-1 border-t border-line bg-raised px-1.5 py-1.5">
      {KEYS.map((k) => (
        <button
          key={k.label}
          aria-label={k.aria}
          disabled={!live}
          // Keeps the terminal's keyboard up.
          onPointerDown={(e) => e.preventDefault()}
          onClick={() => onKey?.(k)}
          className="h-10 min-w-0 flex-1 rounded-md border border-line bg-panel font-mono text-[13px] text-ink active:bg-line disabled:opacity-40"
        >
          {k.label}
        </button>
      ))}
      <button
        aria-label={compose ? "Hide the prompt box" : "Write a prompt"}
        onClick={onCompose}
        className={`grid h-10 min-w-0 flex-1 place-items-center rounded-md border border-line ${compose ? "bg-accent text-panel" : "bg-panel text-ink"}`}
      >
        <Icon name="keyboard" />
      </button>
    </div>
  );
}

/** The prompt box: written or dictated, sent as one paste and an Enter. */
export function Compose({ text, live, onText, onSend }: { text: string; live: boolean; onText?: (text: string) => void; onSend?: () => void }) {
  return (
    <div className="flex shrink-0 items-end gap-1.5 border-t border-line bg-raised p-1.5">
      <textarea
        value={text}
        onChange={(e) => onText?.(e.target.value)}
        rows={3}
        placeholder="Write or dictate a prompt"
        className="min-w-0 flex-1 resize-none rounded-md border border-line bg-panel px-2 py-1.5 text-[16px] text-ink select-text placeholder:text-faint"
      />
      <button aria-label="Send" disabled={!live} onClick={onSend} className="grid h-10 w-12 shrink-0 place-items-center rounded-md bg-accent text-panel disabled:opacity-40">
        <Icon name="send" />
      </button>
    </div>
  );
}
