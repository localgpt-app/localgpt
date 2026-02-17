//! HTTP server for LocalGPT
//!
//! Supports multiple sessions with session ID-based routing.
//! Sessions are created on demand and cached for reuse.

use anyhow::Result;
use axum::{
    Router,
    extract::{
        Path, Query, State,
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{
        IntoResponse, Json, Response,
        sse::{Event, Sse},
    },
    routing::{delete, get, post},
};
use futures::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, info};

use localgpt_core::agent::{Agent, AgentConfig, StreamEvent, extract_tool_detail};
use localgpt_core::concurrency::{TurnGate, WorkspaceLock};
use localgpt_core::config::Config;
use localgpt_core::heartbeat::{HeartbeatStatus, get_last_heartbeat_event};
use localgpt_core::memory::MemoryManager;

/// Embedded UI assets
#[derive(RustEmbed)]
#[folder = "ui/"]
struct UiAssets;

/// Session timeout (30 minutes of inactivity)
const SESSION_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Maximum number of concurrent sessions
const MAX_SESSIONS: usize = 100;

/// Agent ID for HTTP sessions
const HTTP_AGENT_ID: &str = "http";

pub struct Server {
    config: Config,
    turn_gate: TurnGate,
}

struct SessionEntry {
    agent: Agent,
    last_accessed: Instant,
    /// Whether session has unsaved changes
    dirty: bool,
}

struct AppState {
    config: Config,
    sessions: Mutex<HashMap<String, SessionEntry>>,
    /// Shared MemoryManager to avoid reinitializing embedding provider
    memory: MemoryManager,
    /// In-process turn gate shared with heartbeat runner
    turn_gate: TurnGate,
    /// Cross-process workspace lock
    workspace_lock: WorkspaceLock,
}

impl Server {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            turn_gate: TurnGate::new(),
        })
    }

    /// Create a server with a shared TurnGate (for daemon mode where
    /// heartbeat and HTTP share concurrency control).
    pub fn new_with_gate(config: &Config, turn_gate: TurnGate) -> Result<Self> {
        Ok(Self {
            config: config.clone(),
            turn_gate,
        })
    }

    pub async fn run(&self) -> Result<()> {
        // Create shared MemoryManager once to avoid reinitializing embedding provider
        let memory =
            MemoryManager::new_with_full_config(&self.config.memory, Some(&self.config), "main")?;

        let workspace_lock = WorkspaceLock::new()?;

        let state = Arc::new(AppState {
            config: self.config.clone(),
            sessions: Mutex::new(HashMap::new()),
            memory,
            turn_gate: self.turn_gate.clone(),
            workspace_lock,
        });

        // Load persisted sessions on startup
        if let Err(e) = load_persisted_sessions(&state).await {
            info!("Could not load persisted sessions: {}", e);
        }

        // Spawn session cleanup task
        let cleanup_state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                cleanup_expired_sessions(&cleanup_state).await;
            }
        });

        // Spawn session save task (save every 5 minutes)
        let save_state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                save_dirty_sessions(&save_state).await;
            }
        });

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let app = Router::new()
            // Web UI routes
            .route("/", get(serve_ui_index))
            .route("/ui/{*path}", get(serve_ui_file))
            // Egui web UI routes (PoC)
            .route("/egui", get(serve_egui_index))
            .route("/egui/{*path}", get(serve_egui_file))
            // API routes
            .route("/health", get(health_check))
            .route("/api/sessions", post(create_session))
            .route("/api/sessions", get(list_sessions))
            .route("/api/sessions/{session_id}", delete(delete_session))
            .route("/api/sessions/{session_id}", get(get_session_status))
            .route(
                "/api/sessions/{session_id}/messages",
                get(get_session_messages),
            )
            .route("/api/sessions/{session_id}/compact", post(compact_session))
            .route("/api/sessions/{session_id}/clear", post(clear_session))
            .route("/api/sessions/{session_id}/model", post(set_session_model))
            .route("/api/chat", post(chat))
            .route("/api/chat/stream", post(chat_stream))
            .route("/api/ws", get(websocket_handler))
            .route("/api/memory/search", get(memory_search))
            .route("/api/memory/stats", get(memory_stats))
            .route("/api/memory/reindex", post(memory_reindex))
            .route("/api/status", get(status))
            .route("/api/config", get(get_config))
            .route("/api/heartbeat/status", get(heartbeat_status))
            .route("/api/saved-sessions", get(list_saved_sessions))
            .route("/api/saved-sessions/{session_id}", get(get_saved_session))
            .route("/api/logs/daemon", get(get_daemon_logs))
            .layer(cors)
            .with_state(state);

        let addr: SocketAddr =
            format!("{}:{}", self.config.server.bind, self.config.server.port).parse()?;

        info!("Starting HTTP server on http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

// Error response type
struct AppError(StatusCode, String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

// Session cleanup task
async fn cleanup_expired_sessions(state: &Arc<AppState>) {
    let mut sessions = state.sessions.lock().await;
    let before_count = sessions.len();

    sessions.retain(|id, entry| {
        let expired = entry.last_accessed.elapsed() > SESSION_TIMEOUT;
        if expired {
            debug!("Expiring session: {}", id);
        }
        !expired
    });

    let removed = before_count - sessions.len();
    if removed > 0 {
        info!("Cleaned up {} expired sessions", removed);
    }
}

// Load persisted sessions from disk
async fn load_persisted_sessions(state: &Arc<AppState>) -> Result<(), anyhow::Error> {
    use localgpt_core::agent::list_sessions_for_agent;

    let sessions_list = list_sessions_for_agent(HTTP_AGENT_ID)?;
    let mut loaded = 0;

    for session_info in sessions_list.into_iter().take(MAX_SESSIONS) {
        let agent_config = AgentConfig {
            model: state.config.agent.default_model.clone(),
            context_window: state.config.agent.context_window,
            reserve_tokens: state.config.agent.reserve_tokens,
        };

        let mut agent = Agent::new(agent_config, &state.config, state.memory.clone()).await?;

        // Try to resume the session
        if agent.resume_session(&session_info.id).await.is_ok() {
            let mut sessions = state.sessions.lock().await;
            sessions.insert(
                session_info.id.clone(),
                SessionEntry {
                    agent,
                    last_accessed: Instant::now(),
                    dirty: false,
                },
            );
            loaded += 1;
        }
    }

    if loaded > 0 {
        info!("Loaded {} persisted HTTP sessions", loaded);
    }

    Ok(())
}

// Save dirty sessions to disk
async fn save_dirty_sessions(state: &Arc<AppState>) {
    let mut sessions = state.sessions.lock().await;
    let mut saved = 0;

    for (id, entry) in sessions.iter_mut() {
        if entry.dirty {
            if let Err(e) = entry.agent.save_session_for_agent(HTTP_AGENT_ID).await {
                debug!("Failed to save session {}: {}", id, e);
            } else {
                entry.dirty = false;
                saved += 1;
            }
        }
    }

    if saved > 0 {
        info!("Saved {} HTTP sessions to disk", saved);
    }
}

// Get or create a session
async fn get_or_create_session(
    state: &Arc<AppState>,
    session_id: Option<String>,
) -> Result<String, AppError> {
    let mut sessions = state.sessions.lock().await;

    // If session_id provided, try to use existing session
    if let Some(ref id) = session_id
        && sessions.contains_key(id)
    {
        // Update last accessed time
        if let Some(entry) = sessions.get_mut(id) {
            entry.last_accessed = Instant::now();
        }
        return Ok(id.clone());
    }

    // Check session limit
    if sessions.len() >= MAX_SESSIONS {
        // Try to remove oldest session
        if let Some(oldest_id) = sessions
            .iter()
            .min_by_key(|(_, e)| e.last_accessed)
            .map(|(id, _)| id.clone())
        {
            sessions.remove(&oldest_id);
            info!("Removed oldest session {} to make room", oldest_id);
        }
    }

    // Create new session
    let new_id = session_id.unwrap_or_else(|| {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        format!("{:x}-{:x}", ts.as_secs(), ts.subsec_nanos())
    });

    let agent_config = AgentConfig {
        model: state.config.agent.default_model.clone(),
        context_window: state.config.agent.context_window,
        reserve_tokens: state.config.agent.reserve_tokens,
    };

    let mut agent = Agent::new(agent_config, &state.config, state.memory.clone())
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    agent
        .new_session()
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    sessions.insert(
        new_id.clone(),
        SessionEntry {
            agent,
            last_accessed: Instant::now(),
            dirty: true, // New sessions should be saved
        },
    );

    info!("Created new session: {}", new_id);
    Ok(new_id)
}

// Health check endpoint
async fn health_check() -> &'static str {
    "OK"
}

// Serve UI index.html at root
async fn serve_ui_index() -> Response {
    serve_ui_asset("index.html")
}

// Serve UI static files
async fn serve_ui_file(Path(path): Path<String>) -> Response {
    serve_ui_asset(&path)
}

// Helper to serve embedded UI assets
fn serve_ui_asset(path: &str) -> Response {
    match UiAssets::get(path) {
        Some(content) => {
            let mime = match path.rsplit('.').next() {
                Some("js") => "application/javascript".to_string(),
                Some("wasm") => "application/wasm".to_string(),
                _ => mime_guess::from_path(path)
                    .first_or_octet_stream()
                    .to_string(),
            };
            ([(header::CONTENT_TYPE, mime)], content.data.to_vec()).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

// Serve egui web UI index
async fn serve_egui_index() -> Response {
    serve_ui_asset("egui.html")
}

// Serve egui web UI files (WASM, JS, etc.)
async fn serve_egui_file(Path(path): Path<String>) -> Response {
    // Serve WASM artifacts from ui/egui/ directory
    let egui_path = format!("egui/{}", path);
    serve_ui_asset(&egui_path)
}

// Status endpoint
#[derive(Serialize)]
struct StatusResponse {
    version: String,
    model: String,
    memory_chunks: usize,
    active_sessions: usize,
}

async fn status(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let sessions = state.sessions.lock().await;

    Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        model: state.config.agent.default_model.clone(),
        memory_chunks: state.memory.chunk_count().unwrap_or(0),
        active_sessions: sessions.len(),
    })
}

// Session management endpoints
#[derive(Deserialize)]
struct CreateSessionRequest {
    session_id: Option<String>,
}

#[derive(Serialize)]
struct SessionResponse {
    session_id: String,
    model: String,
}

async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateSessionRequest>,
) -> Response {
    match get_or_create_session(&state, request.session_id).await {
        Ok(session_id) => Json(SessionResponse {
            session_id,
            model: state.config.agent.default_model.clone(),
        })
        .into_response(),
        Err(e) => e.into_response(),
    }
}

#[derive(Serialize)]
struct SessionInfo {
    session_id: String,
    idle_seconds: u64,
}

#[derive(Serialize)]
struct ListSessionsResponse {
    sessions: Vec<SessionInfo>,
}

async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<ListSessionsResponse> {
    let sessions = state.sessions.lock().await;

    let session_list: Vec<SessionInfo> = sessions
        .iter()
        .map(|(id, entry)| SessionInfo {
            session_id: id.clone(),
            idle_seconds: entry.last_accessed.elapsed().as_secs(),
        })
        .collect();

    Json(ListSessionsResponse {
        sessions: session_list,
    })
}

// Delete a session
async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Response {
    let mut sessions = state.sessions.lock().await;

    if sessions.remove(&session_id).is_some() {
        info!("Deleted session: {}", session_id);
        Json(json!({"deleted": true, "session_id": session_id})).into_response()
    } else {
        AppError(StatusCode::NOT_FOUND, "Session not found".to_string()).into_response()
    }
}

// Get session status
#[derive(Serialize)]
struct SessionStatusResponse {
    session_id: String,
    model: String,
    message_count: usize,
    token_count: usize,
    idle_seconds: u64,
    api_input_tokens: u64,
    api_output_tokens: u64,
    search_queries: u64,
    search_cached_hits: u64,
    search_cost_usd: f64,
}

async fn get_session_status(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Response {
    let sessions = state.sessions.lock().await;

    match sessions.get(&session_id) {
        Some(entry) => {
            let status = entry.agent.session_status();
            Json(SessionStatusResponse {
                session_id,
                model: entry.agent.model().to_string(),
                message_count: status.message_count,
                token_count: status.token_count,
                idle_seconds: entry.last_accessed.elapsed().as_secs(),
                api_input_tokens: status.api_input_tokens,
                api_output_tokens: status.api_output_tokens,
                search_queries: status.search_queries,
                search_cached_hits: status.search_cached_hits,
                search_cost_usd: status.search_cost_usd,
            })
            .into_response()
        }
        None => AppError(StatusCode::NOT_FOUND, "Session not found".to_string()).into_response(),
    }
}

// Get session messages - returns message history for an active session
#[derive(Serialize)]
struct ActiveSessionMessage {
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<serde_json::Value>>,
    tool_call_id: Option<String>,
    timestamp: u64,
}

#[derive(Serialize)]
struct SessionMessagesResponse {
    session_id: String,
    messages: Vec<ActiveSessionMessage>,
}

async fn get_session_messages(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Response {
    let mut sessions = state.sessions.lock().await;

    match sessions.get_mut(&session_id) {
        Some(entry) => {
            entry.last_accessed = Instant::now();

            let messages: Vec<ActiveSessionMessage> = entry
                .agent
                .raw_session_messages()
                .iter()
                .map(|sm| {
                    let role = match sm.message.role {
                        localgpt_core::agent::Role::User => "user",
                        localgpt_core::agent::Role::Assistant => "assistant",
                        localgpt_core::agent::Role::System => "system",
                        localgpt_core::agent::Role::Tool => "toolResult",
                    };

                    // Convert tool calls to JSON
                    let tool_calls = sm.message.tool_calls.as_ref().map(|tcs| {
                        tcs.iter()
                            .map(|tc| {
                                json!({
                                    "id": tc.id,
                                    "name": tc.name,
                                    "arguments": tc.arguments
                                })
                            })
                            .collect()
                    });

                    ActiveSessionMessage {
                        role: role.to_string(),
                        content: if sm.message.content.is_empty() {
                            None
                        } else {
                            Some(sm.message.content.clone())
                        },
                        tool_calls,
                        tool_call_id: sm.message.tool_call_id.clone(),
                        timestamp: sm.timestamp,
                    }
                })
                .collect();

            Json(SessionMessagesResponse {
                session_id,
                messages,
            })
            .into_response()
        }
        None => AppError(StatusCode::NOT_FOUND, "Session not found".to_string()).into_response(),
    }
}

// Compact session history
async fn compact_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Response {
    let mut sessions = state.sessions.lock().await;

    match sessions.get_mut(&session_id) {
        Some(entry) => {
            entry.last_accessed = Instant::now();

            match entry.agent.compact_session().await {
                Ok((before, after)) => Json(json!({
                    "session_id": session_id,
                    "token_count_before": before,
                    "token_count_after": after,
                }))
                .into_response(),
                Err(e) => {
                    AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                }
            }
        }
        None => AppError(StatusCode::NOT_FOUND, "Session not found".to_string()).into_response(),
    }
}

// Clear session history
async fn clear_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Response {
    let mut sessions = state.sessions.lock().await;

    match sessions.get_mut(&session_id) {
        Some(entry) => {
            entry.last_accessed = Instant::now();
            entry.agent.clear_session();
            Json(json!({"session_id": session_id, "cleared": true})).into_response()
        }
        None => AppError(StatusCode::NOT_FOUND, "Session not found".to_string()).into_response(),
    }
}

// Set session model
#[derive(Deserialize)]
struct SetModelRequest {
    model: String,
}

async fn set_session_model(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(request): Json<SetModelRequest>,
) -> Response {
    let mut sessions = state.sessions.lock().await;

    match sessions.get_mut(&session_id) {
        Some(entry) => {
            entry.last_accessed = Instant::now();

            match entry.agent.set_model(&request.model) {
                Ok(()) => Json(json!({
                    "session_id": session_id,
                    "model": request.model,
                }))
                .into_response(),
                Err(e) => AppError(StatusCode::BAD_REQUEST, e.to_string()).into_response(),
            }
        }
        None => AppError(StatusCode::NOT_FOUND, "Session not found".to_string()).into_response(),
    }
}

// Chat endpoint
#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    session_id: Option<String>,
    /// Optional model to use for this request (switches session model)
    model: Option<String>,
}

#[derive(Serialize)]
struct ChatResponse {
    response: String,
    session_id: String,
    model: String,
}

async fn chat(State(state): State<Arc<AppState>>, Json(request): Json<ChatRequest>) -> Response {
    // Get or create session
    let session_id = match get_or_create_session(&state, request.session_id).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    // Acquire in-process turn gate (waits for other turns to finish)
    let _gate_permit = state.turn_gate.acquire().await;

    // Acquire cross-process workspace lock (blocking, so use spawn_blocking)
    let ws_lock_path = state.workspace_lock.clone();
    let ws_guard = match tokio::task::spawn_blocking(move || ws_lock_path.acquire()).await {
        Ok(Ok(guard)) => guard,
        Ok(Err(e)) => {
            return AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to acquire workspace lock: {}", e),
            )
            .into_response();
        }
        Err(e) => {
            return AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Lock task error: {}", e),
            )
            .into_response();
        }
    };

    // Get agent from session
    let mut sessions = state.sessions.lock().await;
    let entry = match sessions.get_mut(&session_id) {
        Some(e) => e,
        None => {
            return AppError(StatusCode::NOT_FOUND, "Session not found".to_string())
                .into_response();
        }
    };

    entry.last_accessed = Instant::now();

    // Switch model if requested
    if let Some(ref model) = request.model
        && let Err(e) = entry.agent.set_model(model)
    {
        return AppError(StatusCode::BAD_REQUEST, format!("Invalid model: {}", e)).into_response();
    }

    let result = entry.agent.chat(&request.message).await;

    // Release workspace lock explicitly before returning
    drop(ws_guard);

    match result {
        Ok(response) => {
            entry.dirty = true;
            Json(ChatResponse {
                response,
                session_id,
                model: entry.agent.model().to_string(),
            })
            .into_response()
        }
        Err(e) => AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// Streaming chat endpoint (SSE) with tool support
async fn chat_stream(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> Response {
    // Get or create session first (outside the stream)
    let session_id = match get_or_create_session(&state, request.session_id).await {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    let state_clone = state.clone();
    let message = request.message.clone();

    let stream = async_stream::stream! {
        // Send session_id first
        yield Ok::<Event, Infallible>(Event::default().data(json!({"type": "session", "session_id": session_id}).to_string()));

        // Acquire in-process turn gate
        let _gate_permit = state_clone.turn_gate.acquire().await;

        // Acquire cross-process workspace lock
        let ws_lock = state_clone.workspace_lock.clone();
        let _ws_guard = match tokio::task::spawn_blocking(move || ws_lock.acquire()).await {
            Ok(Ok(guard)) => Some(guard),
            Ok(Err(e)) => {
                yield Ok(Event::default().data(json!({"error": format!("Workspace lock error: {}", e)}).to_string()));
                return;
            }
            Err(e) => {
                yield Ok(Event::default().data(json!({"error": format!("Lock task error: {}", e)}).to_string()));
                return;
            }
        };

        let mut sessions = state_clone.sessions.lock().await;
        let entry = match sessions.get_mut(&session_id) {
            Some(e) => e,
            None => {
                yield Ok(Event::default().data(json!({"error": "Session not found"}).to_string()));
                return;
            }
        };

        entry.last_accessed = Instant::now();
        entry.dirty = true;

        // Use streaming with tools
        match entry.agent.chat_stream_with_tools(&message).await {
            Ok(event_stream) => {
                use futures::StreamExt;

                // Pin the stream to iterate over it
                let mut pinned_stream = std::pin::pin!(event_stream);

                while let Some(event) = pinned_stream.next().await {
                    match event {
                        Ok(StreamEvent::Content(content)) => {
                            let data = json!({"type": "content", "delta": content});
                            yield Ok(Event::default().data(data.to_string()));
                        }
                        Ok(StreamEvent::ToolCallStart { name, id, arguments }) => {
                            let detail = extract_tool_detail(&name, &arguments);
                            let data = json!({"type": "tool_start", "name": name, "id": id, "detail": detail});
                            yield Ok(Event::default().data(data.to_string()));
                        }
                        Ok(StreamEvent::ToolCallEnd { name, id, output, warnings }) => {
                            let data = json!({
                                "type": "tool_end",
                                "name": name,
                                "id": id,
                                "output": output.chars().take(500).collect::<String>(),
                                "warnings": warnings
                            });
                            yield Ok(Event::default().data(data.to_string()));
                        }
                        Ok(StreamEvent::Done) => {
                            let data = json!({"type": "done"});
                            yield Ok(Event::default().data(data.to_string()));
                        }
                        Err(e) => {
                            yield Ok(Event::default().data(json!({"error": e.to_string()}).to_string()));
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                yield Ok(Event::default().data(json!({"error": e.to_string()}).to_string()));
            }
        }

        yield Ok(Event::default().data("[DONE]"));
    };

    Sse::new(stream).into_response()
}

// Memory search endpoint
#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct SearchResult {
    file: String,
    line_start: i32,
    line_end: i32,
    content: String,
    score: f64,
}

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<SearchResult>,
    query: String,
}

async fn memory_search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Response {
    match memory_search_inner(&state.memory, &query.q, query.limit) {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn memory_search_inner(
    memory: &MemoryManager,
    query: &str,
    limit: Option<usize>,
) -> Result<SearchResponse, anyhow::Error> {
    let limit = limit.unwrap_or(10);
    let results = memory.search(query, limit)?;

    let results: Vec<SearchResult> = results
        .into_iter()
        .map(|r| SearchResult {
            file: r.file,
            line_start: r.line_start,
            line_end: r.line_end,
            content: r.content,
            score: r.score,
        })
        .collect();

    Ok(SearchResponse {
        results,
        query: query.to_string(),
    })
}

// Memory stats endpoint
#[derive(Serialize)]
struct StatsResponse {
    workspace: String,
    total_files: usize,
    total_chunks: usize,
    index_size_kb: u64,
}

async fn memory_stats(State(state): State<Arc<AppState>>) -> Response {
    match memory_stats_inner(&state.memory) {
        Ok(response) => Json(response).into_response(),
        Err(e) => AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn memory_stats_inner(memory: &MemoryManager) -> Result<StatsResponse, anyhow::Error> {
    let stats = memory.stats()?;

    Ok(StatsResponse {
        workspace: stats.workspace,
        total_files: stats.total_files,
        total_chunks: stats.total_chunks,
        index_size_kb: stats.index_size_kb,
    })
}

// Memory reindex endpoint
#[derive(Deserialize)]
struct ReindexRequest {
    #[serde(default)]
    force: bool,
}

#[derive(Serialize)]
struct ReindexResponse {
    files_processed: usize,
    files_updated: usize,
    chunks_indexed: usize,
    duration_ms: u128,
}

async fn memory_reindex(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ReindexRequest>,
) -> Response {
    // Run reindex in blocking task since it uses sqlite
    let memory = state.memory.clone();
    let force = request.force;

    match tokio::task::spawn_blocking(move || memory_reindex_inner(&memory, force)).await {
        Ok(Ok(response)) => Json(response).into_response(),
        Ok(Err(e)) => AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        Err(e) => AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Task error: {}", e),
        )
        .into_response(),
    }
}

fn memory_reindex_inner(
    memory: &MemoryManager,
    force: bool,
) -> Result<ReindexResponse, anyhow::Error> {
    let stats = memory.reindex(force)?;

    Ok(ReindexResponse {
        files_processed: stats.files_processed,
        files_updated: stats.files_updated,
        chunks_indexed: stats.chunks_indexed,
        duration_ms: stats.duration.as_millis(),
    })
}

// Config endpoint - show current configuration (safe subset)
#[derive(Serialize)]
struct ConfigResponse {
    agent: AgentConfigInfo,
    server: ServerConfigInfo,
    memory: MemoryConfigInfo,
    heartbeat: HeartbeatConfigInfo,
}

#[derive(Serialize)]
struct AgentConfigInfo {
    default_model: String,
    context_window: usize,
    reserve_tokens: usize,
}

#[derive(Serialize)]
struct ServerConfigInfo {
    port: u16,
    bind: String,
}

#[derive(Serialize)]
struct MemoryConfigInfo {
    workspace: String,
    embedding_model: String,
    chunk_size: usize,
    chunk_overlap: usize,
}

#[derive(Serialize)]
struct HeartbeatConfigInfo {
    enabled: bool,
    interval: String,
}

async fn get_config(State(state): State<Arc<AppState>>) -> Json<ConfigResponse> {
    Json(ConfigResponse {
        agent: AgentConfigInfo {
            default_model: state.config.agent.default_model.clone(),
            context_window: state.config.agent.context_window,
            reserve_tokens: state.config.agent.reserve_tokens,
        },
        server: ServerConfigInfo {
            port: state.config.server.port,
            bind: state.config.server.bind.clone(),
        },
        memory: MemoryConfigInfo {
            workspace: state.config.memory.workspace.clone(),
            embedding_model: state.config.memory.embedding_model.clone(),
            chunk_size: state.config.memory.chunk_size,
            chunk_overlap: state.config.memory.chunk_overlap,
        },
        heartbeat: HeartbeatConfigInfo {
            enabled: state.config.heartbeat.enabled,
            interval: state.config.heartbeat.interval.clone(),
        },
    })
}

// Heartbeat status endpoint
#[derive(Serialize)]
struct HeartbeatStatusResponse {
    enabled: bool,
    interval: String,
    last_event: Option<HeartbeatEventInfo>,
}

#[derive(Serialize)]
struct HeartbeatEventInfo {
    ts: u64,
    status: String,
    duration_ms: u64,
    preview: Option<String>,
    reason: Option<String>,
    age_seconds: u64,
}

async fn heartbeat_status(State(state): State<Arc<AppState>>) -> Json<HeartbeatStatusResponse> {
    let last_event = get_last_heartbeat_event().map(|event| {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let age_seconds = (now_ms.saturating_sub(event.ts)) / 1000;

        let status = match event.status {
            HeartbeatStatus::Sent => "sent",
            HeartbeatStatus::Ok => "ok",
            HeartbeatStatus::Skipped => "skipped",
            HeartbeatStatus::SkippedMayTry => "skipped",
            HeartbeatStatus::Failed => "failed",
        };

        HeartbeatEventInfo {
            ts: event.ts,
            status: status.to_string(),
            duration_ms: event.duration_ms,
            preview: event.preview,
            reason: event.reason,
            age_seconds,
        }
    });

    Json(HeartbeatStatusResponse {
        enabled: state.config.heartbeat.enabled,
        interval: state.config.heartbeat.interval.clone(),
        last_event,
    })
}

// Saved sessions endpoint - list sessions from file store
#[derive(Serialize)]
struct SavedSessionInfo {
    id: String,
    message_count: usize,
    created_at: String,
}

#[derive(Serialize)]
struct SavedSessionsResponse {
    sessions: Vec<SavedSessionInfo>,
}

async fn list_saved_sessions(State(_state): State<Arc<AppState>>) -> Response {
    use localgpt_core::agent::list_sessions_for_agent;

    match list_sessions_for_agent(HTTP_AGENT_ID) {
        Ok(sessions) => {
            let session_list: Vec<SavedSessionInfo> = sessions
                .into_iter()
                .map(|s| SavedSessionInfo {
                    id: s.id,
                    message_count: s.message_count,
                    created_at: s.created_at.format("%Y-%m-%dT%H:%M:%S").to_string(),
                })
                .collect();

            Json(SavedSessionsResponse {
                sessions: session_list,
            })
            .into_response()
        }
        Err(e) => AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// Get saved session detail - read and parse JSONL session file
#[derive(Serialize)]
struct SavedSessionMessage {
    role: String,
    content: Option<String>,
    tool_calls: Option<Vec<serde_json::Value>>,
    tool_call_id: Option<String>,
    timestamp: Option<u64>,
}

#[derive(Serialize)]
struct SavedSessionDetail {
    session_id: String,
    created_at: String,
    messages: Vec<SavedSessionMessage>,
}

async fn get_saved_session(Path(session_id): Path<String>) -> Response {
    use localgpt_core::agent::get_sessions_dir_for_agent;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let sessions_dir = match get_sessions_dir_for_agent(HTTP_AGENT_ID) {
        Ok(dir) => dir,
        Err(e) => {
            return AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let session_path = sessions_dir.join(format!("{}.jsonl", session_id));

    if !session_path.exists() {
        return AppError(StatusCode::NOT_FOUND, "Session not found".to_string()).into_response();
    }

    let file = match File::open(&session_path) {
        Ok(f) => f,
        Err(e) => {
            return AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let reader = BufReader::new(file);
    let mut messages = Vec::new();
    let mut created_at = String::new();

    for (i, line) in reader.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        let parsed: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // First line is session header
        if i == 0 && parsed["type"].as_str() == Some("session") {
            created_at = parsed["timestamp"].as_str().unwrap_or("").to_string();
            continue;
        }

        // Parse message entries
        if parsed["type"].as_str() == Some("message")
            && let Some(msg) = parsed.get("message")
        {
            let role = msg["role"].as_str().unwrap_or("unknown").to_string();

            // Extract text content
            let content = if let Some(content_arr) = msg["content"].as_array() {
                content_arr
                    .iter()
                    .filter_map(|c| {
                        if c["type"].as_str() == Some("text") {
                            c["text"].as_str().map(String::from)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else if let Some(text) = msg["content"].as_str() {
                text.to_string()
            } else {
                String::new()
            };

            // Extract tool calls
            let tool_calls = msg["toolCalls"].as_array().cloned();

            // Extract tool result ID
            let tool_call_id = msg["toolCallId"].as_str().map(String::from);

            let timestamp = msg["timestamp"].as_u64();

            messages.push(SavedSessionMessage {
                role,
                content: if content.is_empty() {
                    None
                } else {
                    Some(content)
                },
                tool_calls,
                tool_call_id,
                timestamp,
            });
        }
    }

    Json(SavedSessionDetail {
        session_id,
        created_at,
        messages,
    })
    .into_response()
}

// Daemon logs endpoint - read log file
#[derive(Deserialize)]
struct LogsQuery {
    lines: Option<usize>,
}

#[derive(Serialize)]
struct DaemonLogsResponse {
    lines: Vec<String>,
    total_lines: usize,
    file_size_bytes: u64,
}

async fn get_daemon_logs(Query(query): Query<LogsQuery>) -> Response {
    use localgpt_core::agent::get_state_dir;
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let lines_requested = query.lines.unwrap_or(200).min(1000);

    let state_dir = match get_state_dir() {
        Ok(dir) => dir,
        Err(e) => {
            return AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    // Use date-based log file (matches daemon.rs)
    let date = chrono::Local::now().format("%Y-%m-%d");
    let log_path = state_dir
        .join("logs")
        .join(format!("localgpt-{}.log", date));

    if !log_path.exists() {
        return Json(DaemonLogsResponse {
            lines: vec![],
            total_lines: 0,
            file_size_bytes: 0,
        })
        .into_response();
    }

    let metadata = match std::fs::metadata(&log_path) {
        Ok(m) => m,
        Err(e) => {
            return AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let file = match File::open(&log_path) {
        Ok(f) => f,
        Err(e) => {
            return AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let reader = BufReader::new(file);
    let all_lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
    let total_lines = all_lines.len();

    // Get last N lines
    let lines: Vec<String> = if total_lines > lines_requested {
        all_lines[(total_lines - lines_requested)..].to_vec()
    } else {
        all_lines
    };

    Json(DaemonLogsResponse {
        lines,
        total_lines,
        file_size_bytes: metadata.len(),
    })
    .into_response()
}

// WebSocket handler
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

/// WebSocket message types
#[derive(Deserialize)]
#[serde(tag = "type")]
enum WsIncoming {
    /// Start or resume a session
    #[serde(rename = "session")]
    Session { session_id: Option<String> },
    /// Chat message (uses tool loop, returns complete response)
    /// For streaming, use the SSE endpoint at /api/chat/stream
    #[serde(rename = "chat")]
    Chat { message: String },
    /// Ping for keepalive
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)] // ToolStart/ToolEnd reserved for streaming with tools
enum WsOutgoing {
    /// Connection established
    #[serde(rename = "connected")]
    Connected { session_id: String },
    /// Text content chunk
    #[serde(rename = "content")]
    Content { delta: String },
    /// Tool call started
    #[serde(rename = "tool_start")]
    ToolStart { name: String, id: String },
    /// Tool call completed
    #[serde(rename = "tool_end")]
    ToolEnd {
        name: String,
        id: String,
        output: String,
    },
    /// Message complete
    #[serde(rename = "done")]
    Done,
    /// Pong response
    #[serde(rename = "pong")]
    Pong,
    /// Error
    #[serde(rename = "error")]
    Error { message: String },
}

async fn handle_websocket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    debug!("WebSocket client connected");

    // Track current session for this connection
    let mut current_session_id: Option<String> = None;

    // Process incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(WsMessage::Text(text)) => {
                // Parse incoming message
                match serde_json::from_str::<WsIncoming>(&text) {
                    Ok(WsIncoming::Session { session_id }) => {
                        // Create or resume session
                        match get_or_create_session(&state, session_id).await {
                            Ok(id) => {
                                current_session_id = Some(id.clone());
                                let connected = WsOutgoing::Connected { session_id: id };
                                if let Ok(json) = serde_json::to_string(&connected) {
                                    let _ = sender.send(WsMessage::Text(json.into())).await;
                                }
                            }
                            Err(e) => {
                                let error = WsOutgoing::Error {
                                    message: format!("Failed to create session: {}", e.1),
                                };
                                if let Ok(json) = serde_json::to_string(&error) {
                                    let _ = sender.send(WsMessage::Text(json.into())).await;
                                }
                            }
                        }
                    }
                    Ok(WsIncoming::Chat { message }) => {
                        // Ensure we have a session
                        let session_id = match &current_session_id {
                            Some(id) => id.clone(),
                            None => {
                                // Auto-create session if none exists
                                match get_or_create_session(&state, None).await {
                                    Ok(id) => {
                                        current_session_id = Some(id.clone());
                                        // Notify client of new session
                                        let connected = WsOutgoing::Connected {
                                            session_id: id.clone(),
                                        };
                                        if let Ok(json) = serde_json::to_string(&connected) {
                                            let _ = sender.send(WsMessage::Text(json.into())).await;
                                        }
                                        id
                                    }
                                    Err(e) => {
                                        let error = WsOutgoing::Error {
                                            message: format!("Failed to create session: {}", e.1),
                                        };
                                        if let Ok(json) = serde_json::to_string(&error) {
                                            let _ = sender.send(WsMessage::Text(json.into())).await;
                                        }
                                        continue;
                                    }
                                }
                            }
                        };

                        debug!("WebSocket chat [{}]: {}", session_id, message);

                        // Acquire in-process turn gate
                        let _gate_permit = state.turn_gate.acquire().await;

                        // Acquire cross-process workspace lock
                        let ws_lock = state.workspace_lock.clone();
                        let _ws_guard =
                            match tokio::task::spawn_blocking(move || ws_lock.acquire()).await {
                                Ok(Ok(guard)) => guard,
                                Ok(Err(e)) => {
                                    let error = WsOutgoing::Error {
                                        message: format!("Workspace lock error: {}", e),
                                    };
                                    if let Ok(json) = serde_json::to_string(&error) {
                                        let _ = sender.send(WsMessage::Text(json.into())).await;
                                    }
                                    continue;
                                }
                                Err(e) => {
                                    let error = WsOutgoing::Error {
                                        message: format!("Lock task error: {}", e),
                                    };
                                    if let Ok(json) = serde_json::to_string(&error) {
                                        let _ = sender.send(WsMessage::Text(json.into())).await;
                                    }
                                    continue;
                                }
                            };

                        // Process chat
                        let mut sessions = state.sessions.lock().await;
                        let entry = match sessions.get_mut(&session_id) {
                            Some(e) => e,
                            None => {
                                let error = WsOutgoing::Error {
                                    message: "Session not found".to_string(),
                                };
                                if let Ok(json) = serde_json::to_string(&error) {
                                    let _ = sender.send(WsMessage::Text(json.into())).await;
                                }
                                current_session_id = None;
                                continue;
                            }
                        };

                        entry.last_accessed = Instant::now();

                        match entry.agent.chat(&message).await {
                            Ok(response) => {
                                // Send response as content
                                let content = WsOutgoing::Content { delta: response };
                                if let Ok(json) = serde_json::to_string(&content) {
                                    let _ = sender.send(WsMessage::Text(json.into())).await;
                                }

                                // Send done
                                let done = WsOutgoing::Done;
                                if let Ok(json) = serde_json::to_string(&done) {
                                    let _ = sender.send(WsMessage::Text(json.into())).await;
                                }
                            }
                            Err(e) => {
                                let error = WsOutgoing::Error {
                                    message: e.to_string(),
                                };
                                if let Ok(json) = serde_json::to_string(&error) {
                                    let _ = sender.send(WsMessage::Text(json.into())).await;
                                }
                            }
                        }
                    }
                    Ok(WsIncoming::Ping) => {
                        let pong = WsOutgoing::Pong;
                        if let Ok(json) = serde_json::to_string(&pong) {
                            let _ = sender.send(WsMessage::Text(json.into())).await;
                        }
                    }
                    Err(e) => {
                        let error = WsOutgoing::Error {
                            message: format!("Invalid message format: {}", e),
                        };
                        if let Ok(json) = serde_json::to_string(&error) {
                            let _ = sender.send(WsMessage::Text(json.into())).await;
                        }
                    }
                }
            }
            Ok(WsMessage::Ping(data)) => {
                let _ = sender.send(WsMessage::Pong(data)).await;
            }
            Ok(WsMessage::Close(_)) => {
                debug!("WebSocket client disconnected");
                break;
            }
            Err(e) => {
                debug!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    debug!("WebSocket connection closed");
}
