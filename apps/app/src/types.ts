// Mirror DTOs for the gateway's `/api/*` responses. Kept loose (enum fields as
// plain strings) — the GUI only displays them, so exact variant typing isn't
// worth coupling to the Rust definitions.

export interface StatusSnapshot {
  ok: boolean;
  version: string;
  channels: string[];
  home_chat: string | null;
  open_tasks: number;
  sessions: number;
}

export interface Task {
  id: string;
  title: string;
  note: string;
  status: string;
  board: string;
  due_at: number | null;
  created_at: number;
}

export interface Memory {
  id: string;
  kind: string;
  content: string;
  status: string;
  confidence: string;
  pinned: boolean;
}

export interface Run {
  id: string;
  session_id: string;
  input: string;
  plan: string;
  status: string;
  recoverable: boolean;
  started_at: number;
  ended_at: number | null;
  final_output: string;
  error: string;
}

export interface RunStep {
  seq: number;
  tool_name: string;
  args: string;
  result: string;
  error: string;
  ok: boolean;
}

export interface SessionMessage {
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  timestamp: number;
}

export interface RunDetail {
  run: Run;
  steps: RunStep[];
}

export interface SessionSummary {
  id: string;
  created_at: number;
  messages: number;
  user_turns: number;
  title?: string;
  /** "active" | "archive" (deleted sessions are omitted from the list). */
  status?: string;
}

export interface PendingApproval {
  summary: string;
  detail: string | null;
  risk: string;
}

export interface Interactions {
  approval: PendingApproval | null;
  question: string | null;
}

// ── Client seam ─────────────────────────────────────────────────────────────
// The renderer is platform-agnostic: it talks to the gateway only through a
// `KomoClient`. Each host (Electron, browser) constructs one over HTTP and
// injects it via `client/runtime.ts`. This replaces the old `window.komo` IPC
// bridge — the desktop shell now only *resolves* the gateway address/key and
// hands it to the same `HttpKomoClient` the web build uses.

/** A resolved gateway endpoint: base URL + bearer key. */
export interface Gateway {
  base: string;
  key: string;
}

/** Yields the current gateway endpoint, or null when none is reachable/known.
 *  Desktop reads `~/.komo/gateway.json` (over IPC, re-read each call so a
 *  restart's new port/key is picked up); web derives it from the location +
 *  a stored key. */
export type GatewayResolver = () => Promise<Gateway | null>;

export interface KomoApiRequest {
  path: string;
  method?: "GET" | "POST";
  body?: unknown;
}
export interface KomoApiResponse<T = unknown> {
  ok: boolean;
  status: number;
  data?: T;
  error?: string;
}
export interface KomoChatRequest {
  header: string;
  message: string;
  mode: "interactive" | "trusted";
}
export interface KomoChatResponse {
  ok: boolean;
  reply?: string;
  error?: string;
}
export interface KomoConnectResponse {
  connected: boolean;
  base?: string;
  error?: string;
}

/** A live tool-call event streamed during a turn (mirrors komo's `TurnEvent`). */
export type TurnEvent =
  | { type: "tool_started"; seq: number; name: string; args: string }
  | { type: "tool_finished"; seq: number; name: string; ok: boolean; summary: string };

/** The renderer's entire data plane. One HTTP implementation
 *  (`client/http.ts`) backs both hosts. */
export interface KomoClient {
  /** Probe reachability and (re)bind to the current gateway endpoint. */
  connect(): Promise<KomoConnectResponse>;
  /** One authenticated `/api/*` or `/v1/*` request. */
  api<T = unknown>(req: KomoApiRequest): Promise<KomoApiResponse<T>>;
  /** One chat turn over the SSE stream; `onToolEvent` fires per live tool
   *  frame, the final assistant text is returned. */
  chat(req: KomoChatRequest, onToolEvent?: (event: TurnEvent) => void): Promise<KomoChatResponse>;
}
