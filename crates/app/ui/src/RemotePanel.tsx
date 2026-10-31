import { useEffect, useState } from "react";
import type { RemoteView } from "./bridge";
import { last, listen, send } from "./bridge";
import { Icon } from "./Icon";

/** "3 min ago", for when a device was last attached. */
function since(ms: number) {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  return `${Math.floor(s / 86400)} d ago`;
}

/** How long a pairing code still lasts, counted down here between the
 *  app's messages. */
function useLeft(view: RemoteView | null) {
  const [left, setLeft] = useState(0);
  useEffect(() => {
    const from = view?.pairing?.left_s ?? 0;
    const sent = performance.now();
    setLeft(from);
    if (!from) return;
    const timer = window.setInterval(() => setLeft(Math.max(0, from - Math.floor((performance.now() - sent) / 1000))), 1000);
    return () => clearInterval(timer);
  }, [view]);
  return left;
}

/** The top bar's Phone button: this page on a phone or a tablet on the
 *  same network, paired once by a QR code. The button shows when a device
 *  is attached; the panel turns the listener on and off, shows the code,
 *  and lists the devices, each of which can be forgotten. */
export function RemoteButton() {
  const [view, setView] = useState<RemoteView | null>(() => last("remote")?.view ?? null);
  const [open, setOpen] = useState(false);
  const left = useLeft(view);

  useEffect(
    () =>
      listen((m) => {
        if (m.t === "remote") setView(m.view);
      }),
    [],
  );

  const attached = view?.devices.filter((d) => d.attached) ?? [];
  const label = attached.length ? attached.map((d) => d.name).join(", ") : view?.on ? "Phone: on" : "Phone";

  return (
    <div className="relative shrink-0">
      <button
        onClick={() => setOpen(!open)}
        title="Use this window from a phone or a tablet on this network"
        className={`flex items-center gap-1.5 rounded px-1.5 py-0.5 hover:bg-raised hover:text-ink ${attached.length ? "text-ink" : "text-muted"}`}
      >
        <Icon name="phone" className="size-3.5" />
        {label}
        {attached.length > 0 && <span className="size-1.5 rounded-full bg-[#9ece6a]" />}
      </button>
      {open && view && <RemotePanel view={view} left={left} close={() => setOpen(false)} />}
    </div>
  );
}

/** The panel under the Phone button. `left`: the code's seconds still to run. */
export function RemotePanel({ view, left, close }: { view: RemoteView; left: number; close?: () => void }) {
  return (
    <div className="absolute top-8 right-0 z-20 w-80 rounded-md border border-line bg-raised p-3 text-[12px] text-ink shadow-xl">
      <div className="mb-2 flex items-center">
        <span className="font-semibold">A phone or a tablet</span>
        <button aria-label="Close" onClick={close} className="ml-auto grid size-6 place-items-center rounded text-faint hover:bg-line hover:text-ink">
          <Icon name="close" className="size-3" />
        </button>
      </div>
      {!view.on && (
        <>
          <p className="mb-3 text-muted">
            The agents' terminals, the history and the world, in the browser of a phone or a tablet on this network. Off until you turn it on.
          </p>
          <button onClick={() => send({ t: "remote.on", on: true })} className="w-full rounded-md bg-accent px-3 py-1.5 font-medium text-panel hover:opacity-90">
            Turn on
          </button>
        </>
      )}
      {view.on && (
        <>
          {view.pairing ? (
            <div className="mb-3 flex flex-col items-center gap-2">
              {view.pairing.qr && <img alt="The pairing code" className="size-44 rounded bg-white p-1" src={`data:image/svg+xml;utf8,${encodeURIComponent(view.pairing.qr)}`} />}
              <p className="text-center text-muted">
                Scan with the device's camera. The code works once, for {Math.floor(left / 60)}:{String(left % 60).padStart(2, "0")}.
              </p>
              <button onClick={() => navigator.clipboard?.writeText(view.pairing!.url)} className="max-w-full truncate font-mono text-[11px] text-faint hover:text-ink" title="Copy the pairing address">
                {view.pairing.url}
              </button>
            </div>
          ) : (
            <button onClick={() => send({ t: "remote.pair" })} className="mb-3 w-full rounded-md border border-line px-3 py-1.5 hover:bg-line">
              Pair a device
            </button>
          )}
          {view.devices.length > 0 && (
            <ul className="mb-3 divide-y divide-line rounded-md border border-line">
              {view.devices.map((d) => (
                <li key={d.id} className="flex items-center gap-2 px-2 py-1.5">
                  <span className={`size-1.5 shrink-0 rounded-full ${d.attached ? "bg-[#9ece6a]" : "bg-faint"}`} />
                  <span className="min-w-0 flex-1 truncate">{d.name}</span>
                  <span className="shrink-0 text-faint">{d.attached ? "attached" : since(d.seen_ms)}</span>
                  <button onClick={() => send({ t: "remote.forget", id: d.id })} className="shrink-0 rounded px-1.5 text-faint hover:bg-line hover:text-danger">
                    Forget
                  </button>
                </li>
              ))}
            </ul>
          )}
          {view.url && <p className="mb-2 font-mono text-[11px] text-faint">{view.url}</p>}
          <p className="mb-3 text-faint">
            Plain HTTP: for a network you trust, or a VPN you run. A paired device can type into the terminals, which is a shell on this computer.
          </p>
          <button onClick={() => send({ t: "remote.on", on: false })} className="w-full rounded-md border border-line px-3 py-1.5 text-muted hover:bg-line hover:text-ink">
            Turn off
          </button>
        </>
      )}
      {view.error && <p className="mt-2 text-danger">{view.error}</p>}
    </div>
  );
}
