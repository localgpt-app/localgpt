import { createRoot } from "react-dom/client";
import { Phone } from "./phone/Phone";
import { start } from "./remote";
import "./styles.css";

// The page a paired phone or tablet gets from the app (`remote.rs`): its own
// layout, over a WebSocket, with nothing behind the page. The window's
// interface is the shell's, in Rust (`crates/app/src/shell`); this page goes
// when the shell's panels are built for the browser
// (`docs/rfcs/authoring/all-rust.md`, step F).
document.body.classList.add("phone");
document.querySelector('meta[name="viewport"]')?.setAttribute("content", "width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content");
void start();

// No StrictMode: its doubled effects would open every terminal's PTY twice.
createRoot(document.getElementById("root")!).render(<Phone />);
