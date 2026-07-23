import { fileURLToPath, URL } from "node:url";

import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig, loadEnv } from "vite";

// The shared renderer package's source — bundled directly. `@komo/app` (barrel)
// and `@` (the app's internal alias) both resolve here.
const appSrc = fileURLToPath(new URL("../app/src", import.meta.url));

// In production the gateway serves this build same-origin, so requests to
// /api, /v1, /health hit the same host with no CORS. In dev, set
// KOMO_DEV_GATEWAY=http://127.0.0.1:<port> (a gateway with `[channels.api]`
// bound to a fixed port) and Vite proxies those paths there, so the browser
// stays same-origin against the dev server.
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "KOMO_");
  const target = env.KOMO_DEV_GATEWAY;
  const proxy = target
    ? Object.fromEntries(
        ["/api", "/v1", "/health"].map((p) => [p, { target, changeOrigin: true }]),
      )
    : undefined;
  return {
    plugins: [tailwindcss(), react()],
    resolve: { alias: { "@komo/app": appSrc, "@": appSrc } },
    server: { host: "127.0.0.1", port: 5274, strictPort: true, proxy },
    build: { outDir: "dist", emptyOutDir: true },
  };
});
