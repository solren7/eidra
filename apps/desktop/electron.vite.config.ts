import { fileURLToPath, URL } from "node:url";

import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "electron-vite";

// Path to the shared renderer package's source. The renderer bundles it
// directly (it's a source-only workspace package), so both `@komo/app` (the
// barrel) and `@` (the app's internal alias) resolve here.
const appSrc = fileURLToPath(new URL("../app/src", import.meta.url));

// Three-part build (main / preload / renderer). Main and preload bundle to
// CommonJS (`.cjs`) so the sandboxed preload and the Electron main entry load
// without ESM friction. The renderer is a thin host that mounts @komo/app.
export default defineConfig({
  main: {
    build: {
      outDir: "dist/main",
      lib: { entry: "src/main/index.ts" },
      rollupOptions: {
        external: ["electron"],
        output: { format: "cjs", entryFileNames: "[name].cjs", inlineDynamicImports: true },
      },
    },
  },
  preload: {
    build: {
      outDir: "dist/preload",
      lib: { entry: "src/preload/index.ts" },
      rollupOptions: {
        external: ["electron"],
        output: { format: "cjs", entryFileNames: "[name].cjs", inlineDynamicImports: true },
      },
    },
  },
  renderer: {
    root: fileURLToPath(new URL("./src/renderer", import.meta.url)),
    plugins: [tailwindcss(), react()],
    resolve: {
      alias: {
        "@komo/app": appSrc,
        "@": appSrc,
      },
    },
    build: {
      outDir: fileURLToPath(new URL("./dist/renderer", import.meta.url)),
      emptyOutDir: true,
    },
    server: { host: "127.0.0.1", port: 5273, strictPort: true },
  },
});
