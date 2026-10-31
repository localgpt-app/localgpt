import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// The format's web viewer, which `?spike=web` loads (Viewport.tsx), imports
// `three` as localgpt.world vendors it. (A file URL made a path by hand: the
// page's tsconfig has no Node types for `node:url`.)
const three = decodeURIComponent(new URL("../../../website-world/vendor/three/", import.meta.url).pathname).replace(/^\/([A-Za-z]:)/, "$1");

// The app loads dist/ through its lgui:// scheme, or this dev server when
// LOCALGPT_APP_UI_URL=http://localhost:5174 (the page reloads as it is edited).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: [
      { find: /^three$/, replacement: `${three}build/three.module.min.js` },
      { find: /^three\/addons\/(.*)$/, replacement: `${three}examples/jsm/$1` },
    ],
  },
  server: { port: 5174, strictPort: true },
  // One bundle, read from disk: xterm.js and its WebGL renderer are most of
  // it. The web viewer is a second chunk, fetched only under `?spike=web`.
  build: { outDir: "dist", target: "safari18", chunkSizeWarningLimit: 1500 },
});
