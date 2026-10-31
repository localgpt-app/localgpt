import { createRoot } from "react-dom/client";
import { App } from "./App";
import { inApp } from "./bridge";
import { preview } from "./preview";
import "./styles.css";

// In a browser tab, `?preview=document|world|song` stands in for the app.
const kind = new URLSearchParams(location.search).get("preview");
if (!inApp() && kind) preview(kind);

// No StrictMode: its doubled effects would open every terminal's PTY twice.
createRoot(document.getElementById("root")!).render(<App />);
