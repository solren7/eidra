// Public surface of the shared komo renderer. Each host (Electron renderer,
// web bootstrap) builds an `HttpKomoClient` over its own gateway resolver,
// installs it with `setClient`, then mounts `<App/>` under the query provider.
// The host also imports the stylesheet: `import "@komo/app/styles/main.css"`.

export { App } from "./App";
export { HttpKomoClient } from "./client/http";
export { setClient, getClient } from "./client/runtime";
export { queryClient } from "./lib/query-client";
export { applyTheme, initialTheme } from "./lib/theme";
export type { Theme } from "./lib/theme";
export type {
  Gateway,
  GatewayResolver,
  KomoClient,
  KomoConnectResponse,
} from "./types";
