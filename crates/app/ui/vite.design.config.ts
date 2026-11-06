import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

// The design sheet in design/: a paired phone's page, drawn with its own
// components and fixture data, reviewed before behaviour is written (`npm
// run design`, then /phone.html). Nothing here reaches the app's bundle.
export default defineConfig({
  root: "design",
  plugins: [react(), tailwindcss()],
  server: { port: 5175, strictPort: true },
  build: {
    outDir: "../design-dist",
    emptyOutDir: true,
    target: "safari18",
    chunkSizeWarningLimit: 1500,
    rollupOptions: { input: { phone: "design/phone.html" } },
  },
});
