import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import manifest from "./package.json" with { type: "json" };

export default defineConfig({
  plugins: [react(), tailwindcss()],
  base: "./",
  define: { __APP_VERSION__: JSON.stringify(manifest.version) },
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  preview: { host: "127.0.0.1", port: 1421, strictPort: true },
  build: { target: "es2022", sourcemap: false },
});
