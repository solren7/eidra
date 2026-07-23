import React from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";

import {
  App,
  HttpKomoClient,
  applyTheme,
  initialTheme,
  queryClient,
  setClient,
  type Gateway,
} from "@komo/app";
import "@komo/app/styles/main.css";

// Desktop gateway resolver: read ~/.komo/gateway.json over the preload bridge
// each time, so a gateway restart's new port/key is picked up on the next
// connection-poll tick. All HTTP then goes straight from here to loopback.
const resolveGateway = async (): Promise<Gateway | null> => {
  return (await window.komoBridge.gateway()) ?? null;
};

setClient(new HttpKomoClient(resolveGateway));

// Apply the persisted theme before the first paint to avoid a light→dark flash.
applyTheme(initialTheme());

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
