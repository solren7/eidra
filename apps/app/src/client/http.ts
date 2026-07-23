// The one data-plane implementation, shared by every host. It speaks plain
// HTTP + SSE to the gateway's api channel — the exact logic that used to live
// in the Electron main process, now running in the renderer/browser directly
// (browsers have `fetch`, `AbortController`, and `ReadableStream`, so nothing
// here needs Node). Each host differs only in the `GatewayResolver` it passes:
// Electron reads `~/.komo/gateway.json` over IPC, web derives base+key from the
// page location + a stored key.
//
// The bearer key therefore now lives in the renderer (unlike the old REST-over
// -IPC design that kept it in main). That is the deliberate trade for a single
// client shared with the web build, where the key must reach the browser
// anyway; the renderer is sandboxed and the key is loopback-scoped.

import type {
  Gateway,
  GatewayResolver,
  KomoApiRequest,
  KomoApiResponse,
  KomoChatRequest,
  KomoChatResponse,
  KomoClient,
  KomoConnectResponse,
  TurnEvent,
} from "../types";

const PROBE_TIMEOUT_MS = 2000;
// Longer than a plain request: an interactive turn can block server-side while
// a human approves a tool (up to the gateway's 5-min approval timeout).
const REQUEST_TIMEOUT_MS = 600_000;

async function fetchWithTimeout(
  url: string,
  options: RequestInit,
  timeoutMs: number,
): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(url, { ...options, signal: controller.signal });
  } finally {
    clearTimeout(timer);
  }
}

async function healthOk(base: string): Promise<boolean> {
  try {
    const res = await fetchWithTimeout(`${base}/health`, {}, PROBE_TIMEOUT_MS);
    return res.ok;
  } catch {
    return false;
  }
}

export class HttpKomoClient implements KomoClient {
  /** The endpoint the last `connect()` bound to; api/chat use it until the next
   *  `connect()` re-resolves (so a gateway restart's new port/key is picked
   *  up on the app's connection-poll tick). */
  private gateway: Gateway | null = null;

  constructor(private readonly resolve: GatewayResolver) {}

  async connect(): Promise<KomoConnectResponse> {
    const found = await this.resolve();
    if (!found) {
      this.gateway = null;
      return {
        connected: false,
        error: "未发现运行中的 komo gateway（启动 `komo gateway` 后自动连接）",
      };
    }
    if (!(await healthOk(found.base))) {
      this.gateway = null;
      return { connected: false, error: "gateway 无响应（rendezvous 可能过期）" };
    }
    this.gateway = found;
    return { connected: true, base: found.base };
  }

  async api<T = unknown>(req: KomoApiRequest): Promise<KomoApiResponse<T>> {
    if (!this.gateway) return { ok: false, status: 0, error: "未连接" };
    const { path, method = "GET", body } = req ?? {};
    try {
      const res = await fetchWithTimeout(
        `${this.gateway.base}${path}`,
        {
          method,
          headers: {
            Authorization: `Bearer ${this.gateway.key}`,
            ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
          },
          body: body !== undefined ? JSON.stringify(body) : undefined,
        },
        REQUEST_TIMEOUT_MS,
      );
      const text = await res.text();
      const data = text ? JSON.parse(text) : null;
      if (!res.ok) {
        const msg = (data && data.error) || `HTTP ${res.status}`;
        return { ok: false, status: res.status, error: msg, data };
      }
      return { ok: true, status: res.status, data };
    } catch (err) {
      return { ok: false, status: 0, error: errMsg(err) };
    }
  }

  // One chat turn over the SSE stream. `mode` picks the loopback session
  // context: interactive (approval/clarify suspend the turn, resolved
  // out-of-band) or trusted (side-effecting tools auto-approve, like
  // `komo chat`). Tool-call frames (`event: tool`) fire `onToolEvent` live; the
  // assistant text deltas are accumulated and returned.
  async chat(
    req: KomoChatRequest,
    onToolEvent?: (event: TurnEvent) => void,
  ): Promise<KomoChatResponse> {
    if (!this.gateway) return { ok: false, error: "未连接" };
    const { header, message, mode } = req ?? {};
    const headers: Record<string, string> = {
      Authorization: `Bearer ${this.gateway.key}`,
      "Content-Type": "application/json",
      "X-Komo-Session-Id": header,
    };
    if (mode === "trusted") headers["X-Komo-Trusted"] = "1";
    else headers["X-Komo-Interactive"] = "1";
    try {
      const res = await fetchWithTimeout(
        `${this.gateway.base}/v1/chat/completions`,
        {
          method: "POST",
          headers,
          body: JSON.stringify({
            model: "komo",
            stream: true,
            messages: [{ role: "user", content: message }],
          }),
        },
        REQUEST_TIMEOUT_MS,
      );
      if (!res.ok || !res.body) {
        const text = await res.text().catch(() => "");
        let msg = `HTTP ${res.status}`;
        try {
          const j = JSON.parse(text);
          if (j?.error) msg = j.error;
        } catch {
          /* keep default */
        }
        return { ok: false, error: msg };
      }

      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buf = "";
      let reply = "";
      const flushFrame = (frame: string) => {
        let event = "message";
        const dataLines: string[] = [];
        for (const line of frame.split("\n")) {
          if (line.startsWith("event:")) event = line.slice(6).trim();
          else if (line.startsWith("data:")) dataLines.push(line.slice(5).replace(/^ /, ""));
        }
        const data = dataLines.join("\n");
        if (!data || data === "[DONE]") return;
        if (event === "tool") {
          try {
            onToolEvent?.(JSON.parse(data) as TurnEvent);
          } catch {
            /* ignore malformed frame */
          }
        } else {
          try {
            const chunk = JSON.parse(data);
            const piece = chunk?.choices?.[0]?.delta?.content;
            if (piece) reply += piece;
          } catch {
            /* ignore malformed frame */
          }
        }
      };

      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        let idx: number;
        while ((idx = buf.indexOf("\n\n")) >= 0) {
          flushFrame(buf.slice(0, idx));
          buf = buf.slice(idx + 2);
        }
      }
      if (buf.trim()) flushFrame(buf);
      return { ok: true, reply };
    } catch (err) {
      return { ok: false, error: errMsg(err) };
    }
  }
}

function errMsg(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
