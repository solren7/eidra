// The preload bridge exposed on `window.komoBridge`. Gateway discovery only —
// the renderer's HttpKomoClient (from @komo/app) does all HTTP itself.

import type { Gateway } from "@komo/app";

declare global {
  interface Window {
    komoBridge: {
      /** The current gateway endpoint, or null when none is running. */
      gateway(): Promise<Gateway | null>;
    };
  }
}

export {};
