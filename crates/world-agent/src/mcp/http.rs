//! MCP streamable HTTP transport — the same tools over HTTP.
//!
//! - `POST /mcp` — a JSON-RPC request in, dispatched through [`McpHandler`],
//!   a JSON-RPC response out.
//! - `GET /mcp` — an SSE stream for server-initiated notifications (a
//!   keepalive every 30 s; nothing is pushed yet).
//! - `DELETE /mcp` — ends the session.
//!
//! Sessions are tracked by the `Mcp-Session-Id` header. CORS is open, for
//! browser-based MCP clients. Behind the `mcp-http` feature.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Json};
use axum::routing::{delete, get, post};
use axum::{Router, extract::State};
use futures::stream::Stream;
use serde_json::{Value, json};
use tokio::sync::RwLock;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tracing::{debug, info, warn};

use super::McpHandler;

struct McpHttpState {
    handler: Arc<dyn McpHandler>,
    /// Live sessions, by id, with when each began.
    sessions: RwLock<HashMap<String, std::time::Instant>>,
}

/// An axum [`Router`] serving the transport at `/mcp`.
pub fn mcp_http_router(handler: Arc<dyn McpHandler>) -> Router {
    let state = Arc::new(McpHttpState {
        handler,
        sessions: RwLock::new(HashMap::new()),
    });

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::any())
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers(Any);

    Router::new()
        .route("/mcp", post(handle_post))
        .route("/mcp", get(handle_get))
        .route("/mcp", delete(handle_delete))
        .with_state(state)
        .layer(cors)
}

/// Bind `addr` and serve until the server stops.
pub async fn run_mcp_http_server(
    handler: Arc<dyn McpHandler>,
    addr: std::net::SocketAddr,
) -> anyhow::Result<()> {
    let router = mcp_http_router(handler.clone());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(
        "MCP HTTP server '{}' listening on http://{}",
        handler.server_name(),
        addr,
    );
    axum::serve(listener, router).await?;
    Ok(())
}

async fn handle_post(
    State(state): State<Arc<McpHttpState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let session_id = resolve_or_create_session(&state, &headers).await;
    debug!(
        "MCP HTTP POST session={} method={}",
        session_id,
        body.get("method").and_then(|m| m.as_str()).unwrap_or("?")
    );
    let response = state.handler.dispatch(&body).await;
    let mut headers = HeaderMap::new();
    headers.insert("Mcp-Session-Id", session_header_value(&session_id));
    match response {
        Some(resp) => (StatusCode::OK, headers, Json(resp)).into_response(),
        // A notification: nothing to answer.
        None => (StatusCode::ACCEPTED, headers).into_response(),
    }
}

async fn handle_get(
    State(state): State<Arc<McpHttpState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let session_id = resolve_or_create_session(&state, &headers).await;
    info!("MCP HTTP SSE stream opened for session={}", session_id);
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert("Mcp-Session-Id", session_header_value(&session_id));
    let stream = sse_keepalive_stream(session_id);
    (resp_headers, Sse::new(stream)).into_response()
}

async fn handle_delete(
    State(state): State<Arc<McpHttpState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(sid) = headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok()) {
        let removed = state.sessions.write().await.remove(sid).is_some();
        if removed {
            info!("MCP HTTP session terminated: {}", sid);
            StatusCode::OK
        } else {
            warn!("MCP HTTP DELETE for unknown session: {}", sid);
            StatusCode::NOT_FOUND
        }
    } else {
        StatusCode::BAD_REQUEST
    }
}

/// The session the `Mcp-Session-Id` header names, or a new one.
async fn resolve_or_create_session(state: &McpHttpState, headers: &HeaderMap) -> String {
    if let Some(sid) = headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok()) {
        let sessions = state.sessions.read().await;
        if sessions.contains_key(sid) {
            return sid.to_string();
        }
    }
    let session_id = uuid::Uuid::new_v4().to_string();
    state
        .sessions
        .write()
        .await
        .insert(session_id.clone(), std::time::Instant::now());
    debug!("MCP HTTP new session: {}", session_id);
    session_id
}

fn session_header_value(session_id: &str) -> HeaderValue {
    HeaderValue::from_str(session_id).unwrap_or_else(|_| HeaderValue::from_static("unknown"))
}

/// A keepalive stream: the session's id first, then a comment every 30 s so
/// proxies keep the connection.
fn sse_keepalive_stream(session_id: String) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        yield Ok(Event::default()
            .event("session")
            .data(serde_json::to_string(&json!({
                "sessionId": session_id,
            })).unwrap_or_default()));
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            yield Ok(Event::default().comment("keepalive"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct TestHandler;

    #[async_trait]
    impl McpHandler for TestHandler {
        fn server_name(&self) -> &str {
            "test"
        }

        fn handle_tools_list(&self) -> Result<Value, Value> {
            Ok(json!({ "tools": [] }))
        }

        async fn handle_tools_call(&self, _params: &Value) -> Result<Value, Value> {
            Ok(json!({}))
        }
    }

    fn test_state() -> Arc<McpHttpState> {
        Arc::new(McpHttpState {
            handler: Arc::new(TestHandler),
            sessions: RwLock::new(HashMap::new()),
        })
    }

    fn session_of(response: &axum::response::Response) -> String {
        response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|h| h.to_str().ok())
            .expect("a session id")
            .to_string()
    }

    #[tokio::test]
    async fn a_get_without_a_session_makes_one() {
        let state = test_state();
        let response = handle_get(State(state.clone()), HeaderMap::new())
            .await
            .into_response();
        let session_id = session_of(&response);
        assert_ne!(session_id, "unknown");
        assert!(state.sessions.read().await.contains_key(&session_id));
    }

    #[tokio::test]
    async fn an_unknown_session_header_is_replaced() {
        let state = test_state();
        let mut headers = HeaderMap::new();
        headers.insert(
            "Mcp-Session-Id",
            HeaderValue::from_static("missing-session"),
        );
        let response = handle_get(State(state.clone()), headers)
            .await
            .into_response();
        let session_id = session_of(&response);
        assert_ne!(session_id, "missing-session");
        assert!(state.sessions.read().await.contains_key(&session_id));
    }

    #[tokio::test]
    async fn a_known_session_header_is_kept() {
        let state = test_state();
        let created = resolve_or_create_session(&state, &HeaderMap::new()).await;
        let mut headers = HeaderMap::new();
        headers.insert("Mcp-Session-Id", session_header_value(&created));
        let response = handle_get(State(state), headers).await.into_response();
        assert_eq!(session_of(&response), created);
    }
}
