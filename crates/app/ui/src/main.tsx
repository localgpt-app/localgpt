import { createRoot } from "react-dom/client";
import { App } from "./App";
import { inApp } from "./bridge";
import { Phone } from "./phone/Phone";
import { preview } from "./preview";
import { isRemote, start } from "./remote";
import { startSpike } from "./spike";
import "./styles.css";

// Served to a paired phone or tablet by the app (`remote.rs`): its own
// layout, over a WebSocket, with nothing behind the page. In a browser tab,
// `?preview=document|world|song` stands in for the app.
const remote = !inApp() && isRemote();
const kind = new URLSearchParams(location.search).get("preview");
if (remote) {
  document.body.classList.add("phone");
  document.querySelector('meta[name="viewport"]')?.setAttribute("content", "width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content");
  void start();
} else if (!inApp() && kind) preview(kind);

// No StrictMode: its doubled effects would open every terminal's PTY twice.
createRoot(document.getElementById("root")!).render(remote ? <Phone /> : <App />);

// The world-in-the-page measurement (spike.ts), when `?spike=` asks for one.
if (!remote) startSpike();
