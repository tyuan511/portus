import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

// Dev backend override: SSHIELD_DEV_BACKEND=http://127.0.0.1:8788 pnpm dev
const backend = process.env.SSHIELD_DEV_BACKEND ?? "http://127.0.0.1:8787";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": path.resolve(__dirname, "./src") },
  },
  server: {
    port: 5173,
    proxy: {
      // The backend accepts same-origin browser requests only (allowed_origins
      // is empty by default), so rewrite Origin to the backend host as well —
      // changeOrigin alone only rewrites Host.
      "/api": { target: backend, changeOrigin: true, headers: { origin: backend } },
      "/ws": { target: backend.replace(/^http/, "ws"), ws: true, headers: { origin: backend } },
      "/mcp": { target: backend, changeOrigin: true, headers: { origin: backend } },
    },
  },
  build: { outDir: "dist", sourcemap: false },
});
