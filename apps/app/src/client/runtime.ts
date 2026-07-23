// The active `KomoClient`, installed once by the host entry (Electron renderer
// or web bootstrap) before the app renders. A module singleton mirrors the old
// `window.komo` global: there is one gateway connection per window, and the
// helpers in `lib/ipc.ts` plus the chat view read it through `getClient()`
// rather than threading it through every component.

import type { KomoClient } from "../types";

let current: KomoClient | null = null;

export function setClient(client: KomoClient): void {
  current = client;
}

export function getClient(): KomoClient {
  if (!current) throw new Error("KomoClient not initialized — call setClient() before render");
  return current;
}
