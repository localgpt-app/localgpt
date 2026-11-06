import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// The page a paired phone or tablet gets: the app serves dist/ (read from
// disk in a debug build, so `npm run build` and a reload show a change).
// `npm run dev` serves it here too, with nothing behind it.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: { port: 5174, strictPort: true },
  // One bundle: xterm.js and its WebGL renderer are most of it.
  build: { outDir: "dist", target: "safari18", chunkSizeWarningLimit: 1500 },
});
