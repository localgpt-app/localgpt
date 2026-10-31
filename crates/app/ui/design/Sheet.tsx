import type { ReactNode } from "react";

// A design sheet is a document about the interface: paper, with the
// interface drawn on it in its own dark theme. Commentary lives in the
// margin, never inside a mock-up, where it would read as real UI. Pages are
// static — no state, no effects, no handlers — so every state a component
// can be in is drawn side by side instead of reached by clicking.

/** The sheets, in the order they were drawn. */
const SHEETS = [
  { href: "./index.html", title: "Timeline" },
  { href: "./scene.html", title: "Outliner and inspector" },
  { href: "./phone.html", title: "A phone" },
];

export function Sheet({ title, lede, children }: { title: string; lede: ReactNode; children: ReactNode }) {
  return (
    <div className="min-h-full bg-paper px-12 pt-10 pb-24 text-paper-ink">
      <header className="mb-10 max-w-[1700px]">
        <p className="flex gap-4 text-[11px] font-semibold tracking-[0.08em] text-paper-muted uppercase">
          <span>LocalGPT · interface design</span>
          {SHEETS.map((sheet) =>
            sheet.title === title ? (
              <span key={sheet.href} className="text-paper-ink">
                {sheet.title}
              </span>
            ) : (
              <a key={sheet.href} href={sheet.href} className="underline decoration-paper-line underline-offset-4 hover:text-paper-ink">
                {sheet.title}
              </a>
            ),
          )}
        </p>
        <h1 className="mt-1 text-[30px] font-semibold tracking-tight">{title}</h1>
        <p className="mt-2 max-w-[820px] text-[14px] leading-relaxed text-paper-muted">{lede}</p>
      </header>
      {children}
    </div>
  );
}

/** A logical unit: a heading, one line about it, its views, and the notes
 *  that go with them in the margin. */
export function Section({ title, lede, notes, children }: { title: string; lede: ReactNode; notes?: ReactNode; children: ReactNode }) {
  return (
    <section className="mb-14 grid max-w-[1700px] grid-cols-[minmax(0,1fr)_320px] gap-x-12 border-t border-paper-line pt-6">
      <div className="min-w-0">
        <h2 className="text-[19px] font-semibold tracking-tight">{title}</h2>
        <p className="mt-1 mb-6 max-w-[820px] text-[13px] leading-relaxed text-paper-muted">{lede}</p>
        {children}
      </div>
      <aside className="space-y-3 pt-9 text-[12.5px] leading-relaxed text-paper-muted [&_b]:font-semibold [&_b]:text-paper-ink [&_code]:rounded [&_code]:bg-paper-line/60 [&_code]:px-1 [&_code]:text-[11.5px]">
        {notes}
      </aside>
    </section>
  );
}

/** Views to compare: titles on one line, mock-ups starting level, captions
 *  on one line, however long any of them is. */
export function Row({ children, cols = 2 }: { children: ReactNode; cols?: 1 | 2 | 3 }) {
  const grid = { 1: "grid-cols-1", 2: "grid-cols-2", 3: "grid-cols-3" }[cols];
  return <div className={`mb-8 grid ${grid} grid-rows-[auto_auto_auto] gap-x-8 gap-y-2`}>{children}</div>;
}

export function Figure({ title, caption, children }: { title: string; caption?: ReactNode; children: ReactNode }) {
  return (
    <figure className="row-span-3 m-0 grid min-w-0 grid-rows-subgrid">
      <figcaption className="text-[12.5px] font-semibold">{title}</figcaption>
      <div className="w-fit max-w-full self-start overflow-hidden rounded-md shadow-[0_1px_4px_rgba(20,24,32,0.28)] ring-1 ring-black/20">{children}</div>
      <p className="max-w-[680px] text-[12px] leading-relaxed text-paper-muted">{caption}</p>
    </figure>
  );
}
