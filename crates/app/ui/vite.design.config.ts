import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// The design sheets in design/: documents about the interface, drawn with
// its own components and fixture data, reviewed before behaviour is written
// (`npm run design`). Nothing here reaches the app's bundle.
export default defineConfig({
  root: "design",
  plugins: [react(), tailwindcss()],
  server: { port: 5175, strictPort: true },
  build: {
    outDir: "../design-dist",
    emptyOutDir: true,
    target: "safari18",
    chunkSizeWarningLimit: 1500,
    // One page per sheet.
    rollupOptions: { input: { timeline: "design/index.html", scene: "design/scene.html" } },
  },
});
