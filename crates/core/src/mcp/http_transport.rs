//! MCP Streamable HTTP transport — serves MCP tools over HTTP.
//!
//! Implements the MCP streamable HTTP transport specification:
//!
//! - `POST /mcp` — accepts a JSON-RPC request body, dispatches via
//!   [`McpHandler`], returns a JSON-RPC response.
//! - `GET /mcp` — opens an SSE stream for server-initiated notifications
//!   (stub: sends a keepalive every 30 s).
//! - `DELETE /mcp` — terminates the session.
//!
//! Session tracking uses the `Mcp-Session-Id` header. CORS is enabled for
//! browser-based MCP clients.
//!
//! Requires the `mcp-http` feature flag.

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

use super::server::McpHandler;

// ---------------------------------------------------------------------------
// State shared across all HTTP requests
// ---------------------------------------------------------------------------

struct McpHttpState {
    handler: Arc<dyn McpHandler>,
    /// Active sessions: session_id -> creation timestamp.
    sessions: RwLock<HashMap<String, std::time::Instant>>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build an axum [`Router`] that serves the MCP streamable HTTP transport.
///
/// Mount this at whatever path prefix you like (the routes use `/mcp`
/// internally). Example:
///
/// ```ignore
/// let app = mcp_http_router(handler);
/// let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;
/// axum::serve(listener, app).await?;
/// ```
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

/// Start the MCP HTTP server on the given address.
///
/// This is a convenience wrapper that binds, logs, and runs the server.
/// It blocks until the server shuts down.
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

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// POST /mcp — JSON-RPC request/response.
async fn handle_post(
    State(state): State<Arc<McpHttpState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // Resolve or create session
    let session_id = resolve_or_create_session(&state, &headers).await;

    debug!(
        "MCP HTTP POST session={} method={}",
        session_id,
        body.get("method").and_then(|m| m.as_str()).unwrap_or("?")
    );

    // Dispatch the JSON-RPC message
    let response = state.handler.dispatch(&body).await;

    let mut headers = HeaderMap::new();
    headers.insert("Mcp-Session-Id", session_header_value(&session_id));

    match response {
        Some(resp) => (StatusCode::OK, headers, Json(resp)).into_response(),
        // Notification — no response body needed
        None => (StatusCode::ACCEPTED, headers).into_response(),
    }
}

/// GET /mcp — SSE stream for server-initiated notifications.
///
/// Currently a stub that sends keepalive pings. Full notification
/// support can be added when server push is needed.
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

/// DELETE /mcp — terminate the session.
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

// ---------------------------------------------------------------------------
// Session helpers
// ---------------------------------------------------------------------------

/// Read the `Mcp-Session-Id` header, or create a new session if absent/unknown.
async fn resolve_or_create_session(state: &McpHttpState, headers: &HeaderMap) -> String {
    if let Some(sid) = headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok()) {
        let sessions = state.sessions.read().await;
        if sessions.contains_key(sid) {
            return sid.to_string();
        }
    }

    // Create a new session
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

// ---------------------------------------------------------------------------
// SSE stream
// ---------------------------------------------------------------------------

/// Produce a keepalive SSE stream. Sends a comment ping every 30 seconds
/// so the connection stays alive through proxies.
fn sse_keepalive_stream(session_id: String) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
        // Initial event: session info
        yield Ok(Event::default()
            .event("session")
            .data(serde_json::to_string(&json!({
                "sessionId": session_id,
            })).unwrap_or_default()));

        // Keepalive loop
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
    use axum::response::IntoResponse;

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

    #[tokio::test]
    async fn get_without_session_header_returns_created_session_id() {
        let state = test_state();

        let response = handle_get(State(state.clone()), HeaderMap::new())
            .await
            .into_response();

        let session_id = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|h| h.to_str().ok())
            .expect("response should include a session id");
        assert_ne!(session_id, "unknown");
        assert!(state.sessions.read().await.contains_key(session_id));
    }

    #[tokio::test]
    async fn get_with_unknown_session_header_returns_created_session_id() {
        let state = test_state();
        let mut headers = HeaderMap::new();
        headers.insert(
            "Mcp-Session-Id",
            HeaderValue::from_static("missing-session"),
        );

        let response = handle_get(State(state.clone()), headers)
            .await
            .into_response();

        let session_id = response
            .headers()
            .get("Mcp-Session-Id")
            .and_then(|h| h.to_str().ok())
            .expect("response should include a session id");
        assert_ne!(session_id, "missing-session");
        assert!(state.sessions.read().await.contains_key(session_id));
    }

    #[tokio::test]
    async fn get_with_known_session_header_returns_existing_session_id() {
        let state = test_state();
        let created = resolve_or_create_session(&state, &HeaderMap::new()).await;
        let mut headers = HeaderMap::new();
        headers.insert("Mcp-Session-Id", session_header_value(&created));

        let response = handle_get(State(state), headers).await.into_response();

        assert_eq!(
            response
                .headers()
                .get("Mcp-Session-Id")
                .and_then(|h| h.to_str().ok()),
            Some(created.as_str())
        );
    }
}
