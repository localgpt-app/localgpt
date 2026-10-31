import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Annotation, EditorState } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, keymap } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import { useEffect, useRef } from "react";
import type { DocumentView, OutlineRow } from "./bridge";
import { last, listen, send } from "./bridge";
import { Icon } from "./Icon";

/** A change the app made (the file, reloaded), not the writer: not sent back. */
const fromApp = Annotation.define<boolean>();

const theme = EditorView.theme(
  {
    "&": { height: "100%", backgroundColor: "transparent", color: "var(--color-ink)", fontSize: "12.5px" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.6" },
    ".cm-content": { caretColor: "var(--color-accent)", padding: "10px 0 40vh" },
    ".cm-line": { padding: "0 14px" },
    "&.cm-focused": { outline: "none" },
    ".cm-cursor": { borderLeftColor: "var(--color-accent)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "#2b3a5c" },
    ".cm-activeLine": { backgroundColor: "#ffffff07" },
    ".cm-selectionMatch": { backgroundColor: "#7aa2ff22" },
    ".cm-panels": { backgroundColor: "var(--color-raised)", color: "var(--color-ink)" },
    ".cm-panels input, .cm-panels button": { fontSize: "11px" },
  },
  { dark: true },
);

const highlight = HighlightStyle.define([
  { tag: tags.heading1, fontWeight: "650", fontSize: "1.25em", color: "var(--color-ink)" },
  { tag: [tags.heading2, tags.heading3, tags.heading4, tags.heading5, tags.heading6], fontWeight: "600", color: "var(--color-ink)" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strong, fontWeight: "650" },
  { tag: [tags.link, tags.url], color: "var(--color-accent)" },
  { tag: tags.monospace, color: "var(--color-behavior)" },
  { tag: tags.quote, color: "var(--color-muted)" },
  { tag: tags.list, color: "var(--color-muted)" },
  { tag: [tags.processingInstruction, tags.meta, tags.contentSeparator], color: "var(--color-faint)" },
]);

/** The text, edited here and rebuilt as a world in the app: every edit goes
 *  back whole, and the caret's line says which place to fly to. */
function Editor() {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    let rev = 0;
    let caretLine = -1;
    let queued = false;
    const view = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: "",
        extensions: [
          history(),
          drawSelection(),
          highlightActiveLine(),
          highlightSelectionMatches(),
          search({ top: true }),
          markdown(),
          syntaxHighlighting(highlight),
          EditorView.lineWrapping,
          theme,
          keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
          EditorView.updateListener.of((update) => {
            const ours = update.transactions.some((tr) => tr.annotation(fromApp));
            // One whole text a frame at most, however fast the keys.
            if (update.docChanged && !ours && !queued) {
              queued = true;
              requestAnimationFrame(() => {
                queued = false;
                send({ t: "doc.edit", text: view.state.doc.toString() });
              });
            }
            if (update.selectionSet && update.view.hasFocus) {
              const line = update.state.doc.lineAt(update.state.selection.main.head).number - 1;
              if (line !== caretLine) {
                caretLine = line;
                send({ t: "doc.caret", line });
              }
            }
          }),
        ],
      }),
    });
    const replace = (text: string, next: number) => {
      if (next <= rev) return;
      rev = next;
      const head = Math.min(view.state.selection.main.head, text.length);
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: text },
        selection: { anchor: head },
        annotations: fromApp.of(true),
      });
    };
    const opened = last("doc.text");
    if (opened) replace(opened.text, opened.rev);
    const stop = listen((m) => {
      if (m.t === "doc.text") replace(m.text, m.rev);
      if (m.t === "doc.reveal") {
        const doc = view.state.doc;
        const at = doc.line(Math.min(Math.max(1, m.line + 1), doc.lines)).from;
        // Into view without taking the keys: they may be the world's.
        view.dispatch({ effects: EditorView.scrollIntoView(at, { y: "start", yMargin: 12 }) });
      }
    });
    return () => {
      stop();
      view.destroy();
    };
  }, []);
  return <div ref={host} className="min-h-0 flex-1 overflow-hidden" />;
}

function Toggle({ on, label, title, onClick }: { on: boolean; label: string; title: string; onClick: () => void }) {
  return (
    <button title={title} onClick={onClick} className="flex items-center gap-1.5 text-muted hover:text-ink">
      <span className={`grid size-3 place-items-center rounded-sm border ${on ? "border-accent bg-accent" : "border-faint"}`}>
        {on && <span className="size-1.5 rounded-[1px] bg-panel" />}
      </span>
      {label}
    </button>
  );
}

function OutlineItem({ row, selected }: { row: OutlineRow; selected: boolean }) {
  const indent = 12 * Math.max(0, row.level - 1);
  return (
    <div className={`group flex h-6 items-center pr-2 ${selected ? "bg-raised" : "hover:bg-raised/60"}`}>
      <button
        disabled={row.section === null}
        onClick={() => row.section !== null && send({ t: "doc.go", section: row.section, line: row.line })}
        className={`min-w-0 flex-1 truncate text-left ${row.level === 1 ? "font-semibold text-ink" : row.level === 2 ? "text-ink/90" : "text-[11px] text-muted"}`}
        style={{ paddingLeft: 12 + indent }}
      >
        {row.text}
      </button>
      {row.reauthor && row.section !== null && (
        <button
          title="Author this place again"
          onClick={() => send({ t: "doc.reauthor", section: row.section! })}
          className="mr-1 hidden text-[11px] text-muted group-hover:block hover:text-ink"
        >
          again
        </button>
      )}
      {row.tier && <span className={`shrink-0 text-[10px] ${row.failed ? "text-danger" : "text-faint"}`}>{row.tier}</span>}
    </div>
  );
}

/** Document mode's pane: the file and what the model is doing, the
 *  outline, the selected section's place, and the text. */
export function DocumentPanel({ doc }: { doc: DocumentView | null }) {
  return (
    <aside className="flex min-h-0 flex-col border-r border-line bg-panel">
      <header className="flex h-9 shrink-0 items-center gap-2 border-b border-line px-3">
        <Icon name="document" className="size-3.5 text-muted" />
        <span className="truncate font-medium" title={doc?.path}>
          {doc?.name ?? "Document"}
        </span>
        <span className={`ml-auto truncate text-[11px] ${doc?.conflict ? "text-danger" : "text-faint"}`}>{doc?.status}</span>
      </header>
      {doc?.authoring && (
        <div className="shrink-0 space-y-1 border-b border-line px-3 py-2 text-[11px]">
          <div className="flex items-center gap-3">
            <Toggle
              on={doc.author_enabled}
              label="Author"
              title="Have the model build each section's place. Off, sections keep what they have and new text gets the draft."
              onClick={() => send({ t: "doc.author", on: !doc.author_enabled })}
            />
            <span className="truncate text-faint">{doc.authoring}</span>
          </div>
          {doc.author_error && <p className="text-danger">{doc.author_error}</p>}
        </div>
      )}
      <div className="flex h-8 shrink-0 items-center justify-between px-3 text-[11px]">
        <span className="font-medium text-muted">Outline</span>
        {doc && (
          <Toggle
            on={doc.follow_cursor}
            label="Follow cursor"
            title="Fly to a section's place when the cursor moves into it"
            onClick={() => send({ t: "doc.follow", on: !doc.follow_cursor })}
          />
        )}
      </div>
      <nav className="max-h-[30%] shrink-0 overflow-y-auto border-b border-line pb-1">
        {doc?.outline.map((row, i) => (
          <OutlineItem key={i} row={row} selected={row.section !== null && row.section === doc.selected} />
        ))}
      </nav>
      {doc?.card && (
        <section className="shrink-0 space-y-1.5 border-b border-line px-3 py-2.5">
          <div className="flex items-baseline gap-2">
            <span className="truncate font-medium">{doc.card.heading ?? "Section"}</span>
            {doc.card.tier && <span className="text-[10px] text-faint">{doc.card.tier}</span>}
          </div>
          {doc.card.description && <p className="text-[11px] leading-snug text-muted italic">{doc.card.description}</p>}
          {doc.card.things.length > 0 && (
            <div className="flex flex-wrap gap-1">
              {doc.card.things.map((thing) => (
                <button
                  key={thing.first}
                  title={thing.first}
                  onClick={() => send({ t: "doc.pick", name: thing.first })}
                  className="rounded bg-raised px-1.5 py-0.5 text-[11px] text-ink/90 hover:bg-line"
                >
                  {thing.label}
                  {thing.count > 1 && <span className="text-faint"> ×{thing.count}</span>}
                </button>
              ))}
              {doc.card.more > 0 && <span className="px-1 py-0.5 text-[11px] text-faint">+{doc.card.more} more</span>}
            </div>
          )}
        </section>
      )}
      <Editor />
    </aside>
  );
}
