import React, { useState } from "react";
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

// Where the bearer key + base URL live between visits. The key must reach the
// browser (there is no main process to hold it, unlike the desktop shell); the
// gateway's api channel authenticates it and is loopback/key-scoped.
const KEY_STORE = "komo.key";
const BASE_STORE = "komo.base";

/** Pull `?key=`/`?token=` and `?base=` into localStorage on first load, then
 *  strip them from the address bar so the key isn't left in history. */
function consumeQueryParams(): void {
  const url = new URL(location.href);
  let changed = false;
  const key = url.searchParams.get("key") ?? url.searchParams.get("token");
  if (key) {
    localStorage.setItem(KEY_STORE, key);
    url.searchParams.delete("key");
    url.searchParams.delete("token");
    changed = true;
  }
  const base = url.searchParams.get("base");
  if (base) {
    localStorage.setItem(BASE_STORE, base);
    url.searchParams.delete("base");
    changed = true;
  }
  if (changed) history.replaceState(null, "", url.toString());
}

/** The web GatewayResolver: same-origin by default (the gateway serves this
 *  build), overridable via a stored base; null until a key is known. */
function currentGateway(): Gateway | null {
  const key = localStorage.getItem(KEY_STORE);
  if (!key) return null;
  const base = localStorage.getItem(BASE_STORE) || location.origin;
  return { base, key };
}

consumeQueryParams();
setClient(new HttpKomoClient(async () => currentGateway()));
applyTheme(initialTheme());

/** Gate the app behind a key: without one there is nothing to authenticate
 *  with, so prompt for it (and an optional base for cross-origin/dev use). */
function ConnectGate({ onSaved }: { onSaved: () => void }) {
  const [base, setBase] = useState(() => localStorage.getItem(BASE_STORE) ?? "");
  const [key, setKey] = useState("");
  const save = () => {
    const k = key.trim();
    if (!k) return;
    localStorage.setItem(KEY_STORE, k);
    if (base.trim()) localStorage.setItem(BASE_STORE, base.trim());
    else localStorage.removeItem(BASE_STORE);
    onSaved();
  };
  return (
    <div className="w-screen h-screen grid place-items-center bg-(--mc-bg) text-(--mc-fg)">
      <div className="w-[min(92vw,420px)] flex flex-col gap-3 p-6 rounded-2xl border border-(--mc-border) bg-(--mc-surface-strong) shadow-(--mc-shadow-card)">
        <div className="flex items-center gap-2.5">
          <span
            className="w-7 h-7 rounded-lg shrink-0 shadow-(--mc-shadow-glow)"
            style={{ background: "var(--mc-accent-grad)" }}
          />
          <div className="font-bold tracking-wide text-lg">连接 komo</div>
        </div>
        <p className="text-[13px] text-(--mc-fg-muted)">
          输入 gateway 的访问密钥（见 <code>~/.komo/gateway.json</code> 的 <code>key</code>）。
          留空 base 表示与本页同源。
        </p>
        <label className="flex flex-col gap-1 text-[13px]">
          <span className="text-(--mc-fg-muted)">Base URL（可选）</span>
          <input
            className="h-10 px-3 rounded-[12px] border border-(--mc-border) bg-(--mc-surface) text-(--mc-fg) outline-none focus:border-(--mc-accent)"
            placeholder={location.origin}
            value={base}
            onChange={(e) => setBase(e.target.value)}
          />
        </label>
        <label className="flex flex-col gap-1 text-[13px]">
          <span className="text-(--mc-fg-muted)">访问密钥</span>
          <input
            className="h-10 px-3 rounded-[12px] border border-(--mc-border) bg-(--mc-surface) text-(--mc-fg) outline-none focus:border-(--mc-accent)"
            type="password"
            autoFocus
            value={key}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") save();
            }}
          />
        </label>
        <button
          className="h-10 rounded-[12px] font-medium text-white shadow-(--mc-shadow-glow) disabled:opacity-50"
          style={{ background: "var(--mc-accent-grad)" }}
          disabled={!key.trim()}
          onClick={save}
        >
          连接
        </button>
      </div>
    </div>
  );
}

function Root() {
  const [ready, setReady] = useState(() => currentGateway() !== null);
  if (!ready) return <ConnectGate onSaved={() => setReady(true)} />;
  return <App />;
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <Root />
    </QueryClientProvider>
  </React.StrictMode>,
);
