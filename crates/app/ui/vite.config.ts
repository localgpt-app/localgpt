import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// The app loads dist/ through its lgui:// scheme, or this dev server when
// LOCALGPT_APP_UI_URL=http://localhost:5174 (the page reloads as it is edited).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: { port: 5174, strictPort: true },
  // One bundle, read from disk: xterm.js and its WebGL renderer are most of it.
  build: { outDir: "dist", target: "safari18", chunkSizeWarningLimit: 1500 },
});
