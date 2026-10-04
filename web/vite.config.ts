// The owner's dashboard. `pnpm dev:web` serves it with hot reload and proxies
// /api to `pnpm dev:server`, adding the identity header the Realm gate would
// inject in production. `pnpm build:web` writes web/dist, which the server
// serves (ANALYTICS_WEB_DIR) and the image copies to /app/web.

import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const server = process.env.ANALYTICS_URL ?? "http://127.0.0.1:3000";
const devUser = process.env.ANALYTICS_DEV_USER ?? "dev";

export default defineConfig({
  root: import.meta.dirname,
  plugins: [react(), tailwindcss()],
  // Recharts is most of the bundle; one chunk is fine for a dashboard.
  build: { outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 800 },
  server: {
    port: 5173,
    proxy: {
      "/api": { target: server, headers: { "X-Analytics-User": devUser } },
      "/script.js": { target: server },
    },
  },
});
