//! HTTP API ingress channel.
//!
//! Exposes the agent over a loopback HTTP port for local UIs (the Tauri
//! dashboard) and any OpenAI-compatible client (Open WebUI, LobeChat, …). Two
//! families of endpoints:
//!
//!   - **OpenAI-compatible** (`/v1/*`): `chat/completions` (streaming and not)
//!     and `models`, so third-party chat frontends connect by pointing at
//!     `http://127.0.0.1:8765/v1` with the bearer key.
//!   - **dashboard** (`/api/*`): read views over the same repositories the
//!     `shion` CLI uses — sessions, tasks, memories, runs, plus a `status`
//!     aggregate. These back the desktop control panel (roadmap §9).
//!
//! Unlike the chat channels, an HTTP request is synchronous request/response,
//! so it calls the [`MessageHandler`] directly and awaits the reply rather than
//! going through the spawn-and-return [`GatewayDispatcher`]. The turn runs in a
//! **non-interactive** session context ([`SessionContext::detached`]), so a tool
//! that needs approval is denied immediately — there is no human on an HTTP
//! request to answer a `/approve` prompt.
//!
//! Auth is a single bearer key (`API_SERVER_KEY`); the listener binds loopback
//! by default. `/health` is unauthenticated so a probe can check liveness.

use std::convert::Infallible;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Path, Query, Request, State},
    http::{StatusCode, header::AUTHORIZATION},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::watch;
use tracing::{info, warn};

use crate::{
    agent::{gateway::Channel, interaction::GatewayDispatcher},
    config::ApiConfig,
    domain::{
        gateway::MessageHandler,
        memory::{MemoryRepository, MemoryStatus, parse_memory_status},
        repository::{MessageRepository, SessionRepository},
        run::RunRepository,
        task::TaskRepository,
    },
    services::tool_registry::{SessionContext, with_session},
};

/// Everything the HTTP handlers need, cheaply cloned per request (all `Arc`).
#[derive(Clone)]
struct AppState {
    api_key: Arc<String>,
    handler: Arc<dyn MessageHandler>,
    sessions: Arc<dyn SessionRepository>,
    messages: Arc<dyn MessageRepository>,
    tasks: Arc<dyn TaskRepository>,
    memories: Arc<dyn MemoryRepository>,
    runs: Arc<dyn RunRepository>,
    /// Channel names enabled on this gateway (for `/api/status`).
    channels: Arc<Vec<String>>,
    /// Resolved config `home_chat` fallback, if any (for `/api/status`).
    home: Option<String>,
}

/// The HTTP API channel. Holds the listen config and the shared handler state.
pub struct ApiChannel {
    bind: String,
    port: u16,
    state: AppState,
}

impl ApiChannel {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: &ApiConfig,
        handler: Arc<dyn MessageHandler>,
        sessions: Arc<dyn SessionRepository>,
        messages: Arc<dyn MessageRepository>,
        tasks: Arc<dyn TaskRepository>,
        memories: Arc<dyn MemoryRepository>,
        runs: Arc<dyn RunRepository>,
        channels: Vec<String>,
        home: Option<String>,
    ) -> Self {
        Self {
            bind: config.bind.clone(),
            port: config.port,
            state: AppState {
                api_key: Arc::new(config.server_key.clone()),
                handler,
                sessions,
                messages,
                tasks,
                memories,
                runs,
                channels: Arc::new(channels),
                home,
            },
        }
    }
}

#[async_trait]
impl Channel for ApiChannel {
    fn name(&self) -> &str {
        "api"
    }

    async fn serve(
        &self,
        _dispatcher: Arc<GatewayDispatcher>,
        mut shutdown: watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let addr = format!("{}:{}", self.bind, self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        info!(%addr, "api channel listening");
        let app = build_router(self.state.clone());
        let graceful = async move {
            let _ = shutdown.changed().await;
        };
        axum::serve(listener, app)
            .with_graceful_shutdown(graceful)
            .await?;
        info!("api channel stopped");
        Ok(())
    }
}

/// Build the router: `/health` is public, everything else sits behind the
/// bearer-key middleware.
fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/api/status", get(status))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/{id}/messages", get(session_messages))
        .route("/api/tasks", get(list_tasks))
        .route("/api/memories", get(list_memories))
        .route("/api/runs", get(list_runs))
        .route("/api/runs/{id}", get(get_run))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .with_state(state)
}

/// Reject any request whose `Authorization: Bearer <key>` does not match.
async fn require_auth(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let presented = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match presented {
        Some(token) if token == state.api_key.as_str() => Ok(next.run(req).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Maps any handler error to a 500 with a JSON body.
struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        warn!(error = %self.0, "api request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": self.0.to_string() })),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

// ---- OpenAI-compatible endpoints -------------------------------------------

#[derive(Deserialize)]
struct ChatCompletionRequest {
    #[serde(default)]
    model: String,
    #[serde(default)]
    messages: Vec<ChatMessage>,
    #[serde(default)]
    stream: bool,
}

#[derive(Deserialize)]
struct ChatMessage {
    #[serde(default)]
    role: String,
    #[serde(default)]
    content: String,
}

async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

async fn list_models() -> impl IntoResponse {
    Json(json!({
        "object": "list",
        "data": [{
            "id": "shion",
            "object": "model",
            "created": 0,
            "owned_by": "shion",
        }],
    }))
}

async fn chat_completions(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, ApiError> {
    let (session_id, stateful) = resolve_session(&headers);
    let input = build_input(&req.messages, stateful);
    let model = if req.model.is_empty() {
        "shion".to_string()
    } else {
        req.model.clone()
    };

    // Synchronous: drive the turn directly and await the reply. A detached
    // (non-interactive) context auto-denies any approval-needing tool call.
    let reply = with_session(
        SessionContext::detached(&session_id),
        state.handler.handle(&session_id, input),
    )
    .await?;

    let id = format!("chatcmpl-{}", uuid::Uuid::now_v7());
    let created = now();

    if req.stream {
        Ok(stream_completion(id, created, model, reply).into_response())
    } else {
        Ok(Json(json!({
            "id": id,
            "object": "chat.completion",
            "created": created,
            "model": model,
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": reply },
                "finish_reason": "stop",
            }],
        }))
        .into_response())
    }
}

/// SSE rendering of a completed reply. The turn already produced the full text
/// (the tool loop lives inside rig — no token stream yet), so we emit it as one
/// delta chunk followed by the stop chunk and `[DONE]`. Streaming clients see a
/// normal stream; it just isn't token-incremental.
fn stream_completion(
    id: String,
    created: i64,
    model: String,
    reply: String,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let content_chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "delta": { "role": "assistant", "content": reply },
            "finish_reason": Value::Null,
        }],
    });
    let stop_chunk = json!({
        "id": id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }],
    });
    let events = vec![
        Ok(Event::default().data(content_chunk.to_string())),
        Ok(Event::default().data(stop_chunk.to_string())),
        Ok(Event::default().data("[DONE]")),
    ];
    Sse::new(futures_util::stream::iter(events))
}

/// Continue an existing conversation only when the client opts in with
/// `X-Shion-Session-Id`. Without it, mint an ephemeral session so no server-side
/// history accrues — the client manages its own context.
fn resolve_session(headers: &axum::http::HeaderMap) -> (String, bool) {
    if let Some(id) = headers
        .get("x-shion-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        (format!("api:{id}"), true)
    } else {
        (format!("api:{}", uuid::Uuid::now_v7()), false)
    }
}

/// Reduce the OpenAI `messages` array to one input string for the turn.
///
/// Stateful (header given): the agent already has its history in the db, so we
/// pass only the latest user message. Stateless: the client owns the history,
/// so we flatten the whole exchange into the single ephemeral turn.
fn build_input(messages: &[ChatMessage], stateful: bool) -> String {
    if stateful {
        messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .or_else(|| messages.last())
            .map(|m| m.content.clone())
            .unwrap_or_default()
    } else {
        messages
            .iter()
            .filter(|m| !m.content.trim().is_empty())
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

// ---- dashboard endpoints ---------------------------------------------------

async fn status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let open_tasks = state.tasks.list_open().await?.len();
    let sessions = state.sessions.list().await?.len();
    Ok(Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "channels": state.channels.as_ref(),
        "home_chat": state.home,
        "open_tasks": open_tasks,
        "sessions": sessions,
    })))
}

async fn list_sessions(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    // Summaries only — never dump every transcript in a list view.
    let sessions: Vec<Value> = state
        .sessions
        .list()
        .await?
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "created_at": s.created_at,
                "messages": s.messages.len(),
            })
        })
        .collect();
    Ok(Json(json!({ "sessions": sessions })))
}

async fn session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let messages = state.messages.list_by_session(&id).await?;
    Ok(Json(json!({ "session_id": id, "messages": messages })))
}

async fn list_tasks(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let tasks: Vec<Value> = state
        .tasks
        .list_open()
        .await?
        .into_iter()
        .map(|t| {
            json!({
                "id": t.id,
                "title": t.title,
                "note": t.note,
                "status": t.status.as_str(),
                "waiting_on": t.waiting_on,
                "due_at": t.due_at,
                "board": t.board,
                "source": t.source,
                "created_at": t.created_at,
                "completed_at": t.completed_at,
            })
        })
        .collect();
    Ok(Json(json!({ "tasks": tasks })))
}

#[derive(Deserialize)]
struct MemoryQueryParams {
    status: Option<String>,
}

async fn list_memories(
    State(state): State<AppState>,
    Query(params): Query<MemoryQueryParams>,
) -> Result<Json<Value>, ApiError> {
    let mut memories = state.memories.list().await?;
    if let Some(status) = params.status.as_deref().filter(|s| !s.is_empty()) {
        let want: MemoryStatus = parse_memory_status(status);
        memories.retain(|m| m.status == want);
    }
    // Memory derives Serialize, so it serializes verbatim.
    Ok(Json(json!({ "memories": memories })))
}

#[derive(Deserialize)]
struct RunsQueryParams {
    limit: Option<usize>,
}

async fn list_runs(
    State(state): State<AppState>,
    Query(params): Query<RunsQueryParams>,
) -> Result<Json<Value>, ApiError> {
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let runs: Vec<Value> = state
        .runs
        .list(limit)
        .await?
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "session_id": r.session_id,
                "input": r.input,
                "plan": r.plan,
                "status": r.status.as_str(),
                "started_at": r.started_at,
                "ended_at": r.ended_at,
            })
        })
        .collect();
    Ok(Json(json!({ "runs": runs })))
}

async fn get_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let Some(run) = state.runs.get(&id).await? else {
        return Ok((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "run not found" })),
        )
            .into_response());
    };
    let steps: Vec<Value> = state
        .runs
        .steps(&id)
        .await?
        .into_iter()
        .map(|s| {
            json!({
                "seq": s.seq,
                "tool_name": s.tool_name,
                "args": s.args,
                "result": s.result,
                "error": s.error,
                "ok": s.ok,
                "started_at": s.started_at,
                "ended_at": s.ended_at,
            })
        })
        .collect();
    Ok(Json(json!({
        "id": run.id,
        "session_id": run.session_id,
        "input": run.input,
        "plan": run.plan,
        "status": run.status.as_str(),
        "final_output": run.final_output,
        "error": run.error,
        "started_at": run.started_at,
        "ended_at": run.ended_at,
        "steps": steps,
    }))
    .into_response())
}

/// Unix seconds, for OpenAI `created` fields.
fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stateful_input_takes_last_user_message() {
        let messages = vec![
            ChatMessage {
                role: "user".into(),
                content: "first".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "reply".into(),
            },
            ChatMessage {
                role: "user".into(),
                content: "second".into(),
            },
        ];
        assert_eq!(build_input(&messages, true), "second");
    }

    #[test]
    fn stateless_input_flattens_conversation() {
        let messages = vec![
            ChatMessage {
                role: "user".into(),
                content: "hi".into(),
            },
            ChatMessage {
                role: "assistant".into(),
                content: "hello".into(),
            },
        ];
        assert_eq!(
            build_input(&messages, false),
            "user: hi\n\nassistant: hello"
        );
    }

    #[test]
    fn resolve_session_is_ephemeral_without_header() {
        let headers = axum::http::HeaderMap::new();
        let (id, stateful) = resolve_session(&headers);
        assert!(id.starts_with("api:"));
        assert!(!stateful);
    }

    #[test]
    fn resolve_session_uses_header_when_present() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-shion-session-id", "panel-1".parse().unwrap());
        let (id, stateful) = resolve_session(&headers);
        assert_eq!(id, "api:panel-1");
        assert!(stateful);
    }
}
