import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { viteSingleFile } from "vite-plugin-singlefile";

// Set by `tauri ios dev` so a real iPhone can reach this dev server over Wi-Fi.
const host = process.env.TAURI_DEV_HOST;

// `npm run dev`        -> Tauri dev server (port 1420)
// `npm run build`      -> assets for the Tauri bundle (dist/)
// `npm run build:preview` -> one self-contained HTML file for the browser preview

export default defineConfig(({ mode }) => ({
  plugins: [react(), ...(mode === "preview" ? [viteSingleFile()] : [])],

  clearScreen: false,

  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      ignored: ["**/src-tauri/target/**"],
    },
  },

  envPrefix: ["VITE_", "TAURI_ENV_"],

  build: {
    target: "es2021",
    outDir: mode === "preview" ? "dist-preview" : "dist",
    sourcemap: false,
  },
}));
