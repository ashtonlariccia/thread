import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  root: "ui",
  plugins: [svelte()],

  // Tauri owns the terminal output; don't let Vite wipe it.
  clearScreen: false,

  server: {
    // Not 5173: that is Shaman's, and the two are developed side by side.
    port: 5174,
    strictPort: true,
  },

  build: {
    outDir: "../dist",
    emptyOutDir: true,
    // WebView2 is evergreen Chromium; no need to down-level far.
    target: "chrome110",
    sourcemap: false,
  },
});
