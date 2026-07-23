# komo — web build

The standalone browser build of the shared [`@komo/app`](../app) renderer — the
same React app the [desktop shell](../desktop) hosts, served as a static SPA.
This mirrors opencode's pattern (`packages/app` served both as the Electron
renderer and a web SPA), unified by the gateway's HTTP api channel.

## How it connects

The app talks to the gateway only through a `KomoClient`; the web entry
(`src/entry.tsx`) builds an `HttpKomoClient` with a browser gateway resolver:

- **Base URL**: same-origin by default (`location.origin`) — the intended
  production shape is the gateway serving this build itself (a `ServeDir`
  fallback on the api channel, so no CORS is involved). Overridable with a
  stored base for cross-origin/dev use.
- **Bearer key**: from a `?key=` (or `?token=`) query param — read once, saved
  to `localStorage`, then stripped from the address bar — or entered on the
  connect screen. It comes from `~/.komo/gateway.json`'s `key`.

## Run (dev)

The gateway has no CORS layer, so a cross-origin dev browser must go through the
Vite proxy. Run a gateway with `[channels.api] enabled = true` on a fixed port,
then point the dev server at it:

```bash
cd apps && bun install
```

```bash
cd apps/web && KOMO_DEV_GATEWAY=http://127.0.0.1:8787 bun run dev
```

Open the printed URL, leave **Base URL** blank (same-origin → proxied), and
paste the gateway key. `bun run build` emits a static `dist/` for the gateway
to serve.

## Status

The web target is scaffolded and builds; same-origin serving from the gateway
(the `ServeDir` route) and relaxing the loopback-gated interactive/approval
endpoints for keyed remote callers are follow-ups — until then a non-loopback
browser gets read-only panels + chat, with approval/clarify and session writes
unavailable.
