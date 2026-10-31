import { FitAddon } from "@xterm/addon-fit";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal as XTerm } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { useEffect, useRef } from "react";
import { decode, listen, send } from "./bridge";

export const THEME = {
  background: "#0f1218",
  foreground: "#e6e9ef",
  cursor: "#7aa2ff",
  selectionBackground: "#2c3a5a",
  black: "#1b1f2a",
  red: "#f7768e",
  green: "#9ece6a",
  yellow: "#e0af68",
  blue: "#7aa2ff",
  magenta: "#bb9af7",
  cyan: "#7dcfff",
  white: "#c0c6d4",
  brightBlack: "#596173",
  brightRed: "#ff8fa3",
  brightGreen: "#b5e48c",
  brightYellow: "#f2c37c",
  brightBlue: "#9cb8ff",
  brightMagenta: "#cdb2ff",
  brightCyan: "#a3e2ff",
  brightWhite: "#eef1f6",
};

/** One tab's terminal: xterm.js here, a PTY in the app. The tab keeps its
 *  terminal while hidden, so switching tabs never restarts an agent. A tab
 *  another page opened — a paired phone's, or this page's before a reload —
 *  is attached rather than opened, and the app sends its screen. */
export function Terminal({ id, kind, visible, attach = false }: { id: string; kind: string; visible: boolean; attach?: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const term = useRef<XTerm | null>(null);
  const fit = useRef<FitAddon | null>(null);

  useEffect(() => {
    const xterm = new XTerm({
      fontFamily: '"SF Mono", Menlo, monospace',
      fontSize: 12,
      lineHeight: 1.15,
      cursorBlink: true,
      allowProposedApi: true,
      scrollback: 10000,
      theme: THEME,
    });
    const fitter = new FitAddon();
    xterm.loadAddon(fitter);
    xterm.loadAddon(new Unicode11Addon());
    xterm.unicode.activeVersion = "11";
    xterm.loadAddon(new WebLinksAddon((_, url) => send({ t: "open", url })));
    xterm.open(ref.current!);
    try {
      const gl = new WebglAddon();
      gl.onContextLoss(() => gl.dispose());
      xterm.loadAddon(gl);
    } catch {
      // The DOM renderer stays.
    }
    fitter.fit();

    // Shift-Enter as Claude Code's terminal setup sends it: a new line in the
    // prompt rather than a send.
    xterm.attachCustomKeyEventHandler((e) => {
      if (e.type === "keydown" && e.key === "Enter" && e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey) {
        send({ t: "term.input", id, data: "\x1b\r" });
        return false;
      }
      return true;
    });
    const stop = listen((m) => {
      if (m.t === "term.data" && m.id === id) xterm.write(decode(m.b64));
      if (m.t === "term.exit" && m.id === id) {
        const why = m.error ?? (m.code === null ? "ended" : `exited with ${m.code}`);
        xterm.write(`\r\n\x1b[2m[${why}]\x1b[0m\r\n`);
      }
    });
    const input = xterm.onData((data) => send({ t: "term.input", id, data }));
    const resized = xterm.onResize(({ cols, rows }) => send({ t: "term.resize", id, cols, rows }));
    if (attach) send({ t: "term.attach", id, cols: xterm.cols, rows: xterm.rows });
    else send({ t: "term.open", id, kind, cols: xterm.cols, rows: xterm.rows });
    // Clicked into: the PTY takes this page's size back from a phone.
    const focused = () => send({ t: "term.focus", id });
    xterm.textarea?.addEventListener("focus", focused);

    const observer = new ResizeObserver(() => fitter.fit());
    observer.observe(ref.current!);
    term.current = xterm;
    fit.current = fitter;
    return () => {
      observer.disconnect();
      stop();
      input.dispose();
      resized.dispose();
      xterm.textarea?.removeEventListener("focus", focused);
      send({ t: "term.close", id });
      xterm.dispose();
    };
  }, [id, kind, attach]);

  useEffect(() => {
    if (visible) {
      fit.current?.fit();
      term.current?.focus();
    }
  }, [visible]);

  return <div ref={ref} className={`absolute inset-0 ${visible ? "" : "invisible"}`} />;
}
