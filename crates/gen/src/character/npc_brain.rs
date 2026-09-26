//! AI-driven NPC brain system (AI2.1).
//!
//! Attaches a local SLM-driven brain to NPCs for autonomous behavior.
//! Brain runs as background tasks, issuing commands at configurable tick rates.
//!
//! ## Architecture
//!
//! The brain loop runs as a Bevy system that periodically ticks each NPC entity
//! with an `NpcBrainState` component. On each tick:
//!
//! 1. Gather perception (nearby entities, recent events)
//! 2. Build prompt with personality, goals, knowledge, and memory context
//! 3. Spawn async Ollama HTTP call via `BrainTaskChannel`
//! 4. Parse response into `NpcAction` and apply
//!
//! Async calls are spawned via `std::thread::spawn` + a dedicated tokio runtime
//! (same pattern as ws_server.rs), with results returned through a crossbeam channel
//! so Bevy systems can poll results without blocking.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::mpsc as std_mpsc;

/// NPC actions the brain can decide to take.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum NpcAction {
    MoveTo { position: [f32; 3] },
    LookAt { entity: String },
    Speak { text: String },
    Emote { emote_type: EmoteType },
    Interact { entity: String },
    Wait,
    Wander,
}

/// Emote types for NPC expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmoteType {
    Wave,
    Nod,
    Shrug,
    Point,
    Laugh,
}

/// An event the NPC perceived.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcEvent {
    pub timestamp: f64,
    pub description: String,
}

/// Configuration for an NPC brain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpcBrainConfig {
    pub personality: String,
    pub model: String,
    pub tick_rate: f32,
    pub perception_radius: f32,
    pub goals: Vec<String>,
    pub knowledge: Vec<String>,
}

impl Default for NpcBrainConfig {
    fn default() -> Self {
        Self {
            personality: "a friendly villager".to_string(),
            model: "llama3.2:3b".to_string(),
            tick_rate: 2.0,
            perception_radius: 15.0,
            goals: Vec::new(),
            knowledge: Vec::new(),
        }
    }
}

/// State of an NPC brain (Bevy Component).
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub struct NpcBrainState {
    pub config: NpcBrainConfig,
    pub active: bool,
    pub current_action: NpcAction,
    pub last_tick: f64,
    pub recent_events: VecDeque<NpcEvent>,
    pub tick_count: u64,
    /// Whether an async Ollama request is currently in-flight for this NPC.
    #[serde(skip)]
    pub pending_request: bool,
}

impl NpcBrainState {
    pub fn new(config: NpcBrainConfig) -> Self {
        Self {
            config,
            active: true,
            current_action: NpcAction::Wait,
            last_tick: 0.0,
            recent_events: VecDeque::with_capacity(20),
            tick_count: 0,
            pending_request: false,
        }
    }

    /// Add an event to the recent events buffer (ring buffer, max 20).
    pub fn push_event(&mut self, description: String, timestamp: f64) {
        if self.recent_events.len() >= 20 {
            self.recent_events.pop_front();
        }
        self.recent_events.push_back(NpcEvent {
            timestamp,
            description,
        });
    }

    /// Check if enough time has passed for next tick.
    pub fn should_tick(&self, current_time: f64) -> bool {
        self.active && (current_time - self.last_tick) >= self.config.tick_rate as f64
    }
}

/// A nearby entity perceived by the NPC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceivedEntity {
    pub name: String,
    pub entity_type: String,
    pub position: [f32; 3],
    pub distance: f32,
    pub direction: String,
}

/// Perception snapshot for building brain context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptionSnapshot {
    pub npc_position: [f32; 3],
    pub current_action: NpcAction,
    pub nearby_entities: Vec<PerceivedEntity>,
    pub player_info: Option<PerceivedEntity>,
    pub recent_events: Vec<NpcEvent>,
}

/// Build the context prompt for an NPC brain tick.
pub fn build_brain_context(
    brain: &NpcBrainState,
    perception: &PerceptionSnapshot,
    memory_context: Option<&str>,
) -> String {
    let mut ctx = format!("You are {}.\n\n", brain.config.personality);

    if !brain.config.goals.is_empty() {
        ctx.push_str("Your goals:\n");
        for goal in &brain.config.goals {
            ctx.push_str(&format!("- {}\n", goal));
        }
        ctx.push('\n');
    }

    if !brain.config.knowledge.is_empty() {
        ctx.push_str("You know:\n");
        for fact in &brain.config.knowledge {
            ctx.push_str(&format!("- {}\n", fact));
        }
        ctx.push('\n');
    }

    if let Some(mem) = memory_context {
        ctx.push_str(&format!("## Your memories\n{}\n\n", mem));
    }

    ctx.push_str(&format!(
        "## Current situation\nPosition: ({:.1}, {:.1}, {:.1})\nCurrently: {:?}\n\n",
        perception.npc_position[0],
        perception.npc_position[1],
        perception.npc_position[2],
        perception.current_action
    ));

    if !perception.nearby_entities.is_empty() {
        ctx.push_str(&format!(
            "## Nearby ({} entities within {}m)\n",
            perception.nearby_entities.len(),
            brain.config.perception_radius
        ));
        for entity in &perception.nearby_entities {
            ctx.push_str(&format!(
                "- \"{}\" ({}) at {:.1}m {}\n",
                entity.name, entity.entity_type, entity.distance, entity.direction
            ));
        }
        ctx.push('\n');
    }

    if let Some(player) = &perception.player_info {
        ctx.push_str(&format!(
            "Player at {:.1}m {}\n\n",
            player.distance, player.direction
        ));
    }

    if !perception.recent_events.is_empty() {
        ctx.push_str("## Recent events\n");
        for event in &perception.recent_events {
            ctx.push_str(&format!("- {}\n", event.description));
        }
        ctx.push('\n');
    }

    ctx.push_str("## Available actions (choose exactly ONE)\n");
    ctx.push_str("- move_to(x, y, z) — walk to position\n");
    ctx.push_str("- look_at(\"entity_name\") — turn toward entity\n");
    ctx.push_str("- speak(\"text\") — say aloud\n");
    ctx.push_str("- emote(wave|nod|shrug|point|laugh)\n");
    ctx.push_str("- interact(\"entity_name\") — interact with nearby entity\n");
    ctx.push_str("- wait — stay still, observe\n");
    ctx.push_str("- wander — walk to random nearby point\n\n");
    ctx.push_str("Respond with ONLY the action. Example: speak(\"Welcome, traveler!\")");

    ctx
}

/// Parse an NPC action from SLM response text.
pub fn parse_npc_action(response: &str) -> NpcAction {
    let trimmed = response.trim();

    // Try move_to(x, y, z)
    if let Some(inner) = extract_parens(trimmed, "move_to") {
        let parts: Vec<f32> = inner
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if parts.len() >= 3 {
            return NpcAction::MoveTo {
                position: [parts[0], parts[1], parts[2]],
            };
        }
    }

    // Try look_at("entity")
    if let Some(inner) = extract_parens(trimmed, "look_at") {
        let entity = inner.trim_matches('"').trim_matches('\'').to_string();
        if !entity.is_empty() {
            return NpcAction::LookAt { entity };
        }
    }

    // Try speak("text")
    if let Some(inner) = extract_parens(trimmed, "speak") {
        let text = inner.trim_matches('"').trim_matches('\'').to_string();
        if !text.is_empty() {
            return NpcAction::Speak { text };
        }
    }

    // Try emote(type)
    if let Some(inner) = extract_parens(trimmed, "emote") {
        let emote_str = inner.trim().to_lowercase();
        let emote_type = match emote_str.as_str() {
            "wave" => Some(EmoteType::Wave),
            "nod" => Some(EmoteType::Nod),
            "shrug" => Some(EmoteType::Shrug),
            "point" => Some(EmoteType::Point),
            "laugh" => Some(EmoteType::Laugh),
            _ => None,
        };
        if let Some(et) = emote_type {
            return NpcAction::Emote { emote_type: et };
        }
    }

    // Try interact("entity")
    if let Some(inner) = extract_parens(trimmed, "interact") {
        let entity = inner.trim_matches('"').trim_matches('\'').to_string();
        if !entity.is_empty() {
            return NpcAction::Interact { entity };
        }
    }

    // Simple keywords
    if trimmed.starts_with("wait") {
        return NpcAction::Wait;
    }
    if trimmed.starts_with("wander") {
        return NpcAction::Wander;
    }

    // Fallback
    NpcAction::Wait
}

/// Helper to extract content inside parentheses after a function name.
fn extract_parens<'a>(text: &'a str, func_name: &str) -> Option<&'a str> {
    let lower = text.to_lowercase();
    if let Some(start) = lower.find(&format!("{}(", func_name)) {
        let after_paren = start + func_name.len() + 1;
        if after_paren < text.len() {
            // Find matching closing paren
            if let Some(end) = text[after_paren..].rfind(')') {
                return Some(&text[after_paren..after_paren + end]);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perception_is_the_view_cone_within_the_radius() {
        // Looking down -Z from the origin.
        let eye = Transform::default();
        let others = [
            ("ahead", Vec3::new(0.0, 0.0, -5.0)),
            ("right", Vec3::new(3.0, 0.0, -3.0)),
            ("behind", Vec3::new(0.0, 0.0, 5.0)),
            ("far", Vec3::new(0.0, 0.0, -50.0)),
        ]
        .map(|(n, p)| (n.to_string(), p));
        let seen = perceive(&eye, 120.0, 20.0, others.clone().into_iter());
        let names: Vec<_> = seen.iter().map(|v| v["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            ["right", "ahead"],
            "nearest first; behind and far unseen"
        );
        assert_eq!(seen[1]["bearing_degrees"], 0.0);
        assert_eq!(seen[0]["bearing_degrees"], 45.0);
        let all_round = perceive(&eye, 360.0, 20.0, others.into_iter());
        assert_eq!(all_round.len(), 3);
    }

    async fn mock_ollama() -> String {
        use axum::routing::{get, post};
        let app = axum::Router::new()
            .route(
                "/api/tags",
                get(|| async {
                    axum::Json(serde_json::json!({"models": [{"name": "llama3.2:3b"}, {"name": "mistral:latest"}]}))
                }),
            )
            .route(
                "/api/chat",
                post(|axum::Json(body): axum::Json<serde_json::Value>| async move {
                    let asked = body["messages"][1]["content"].as_str().unwrap_or("").contains("well");
                    axum::Json(serde_json::json!({"message": {"content": if asked { "The well is ahead." } else { "?" }}}))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        url
    }

    #[tokio::test]
    async fn a_brain_needs_ollama_and_its_model() {
        let url = mock_ollama().await;
        check_ollama_model(&url, "llama3.2:3b").await.unwrap();
        check_ollama_model(&url, "mistral").await.unwrap();
        let missing = check_ollama_model(&url, "qwen3:8b").await.unwrap_err();
        assert!(
            missing.to_string().contains("ollama pull qwen3:8b"),
            "{missing}"
        );
        let down = check_ollama_model("http://127.0.0.1:9", "llama3.2:3b")
            .await
            .unwrap_err();
        assert!(down.to_string().contains("not reachable"), "{down}");
        let answer = ask_ollama(&url, "llama3.2:3b", "sys", "Where is the well?")
            .await
            .unwrap();
        assert_eq!(answer, "The well is ahead.");
    }

    #[test]
    fn test_brain_state_new() {
        let config = NpcBrainConfig::default();
        let state = NpcBrainState::new(config);
        assert!(state.active);
        assert!(matches!(state.current_action, NpcAction::Wait));
        assert_eq!(state.last_tick, 0.0);
        assert_eq!(state.tick_count, 0);
        assert!(state.recent_events.is_empty());
    }

    #[test]
    fn test_push_event_ring_buffer() {
        let config = NpcBrainConfig::default();
        let mut state = NpcBrainState::new(config);

        // Fill beyond capacity
        for i in 0..25 {
            state.push_event(format!("event_{}", i), i as f64);
        }

        // Should only keep last 20
        assert_eq!(state.recent_events.len(), 20);
        assert_eq!(state.recent_events.front().unwrap().description, "event_5");
        assert_eq!(state.recent_events.back().unwrap().description, "event_24");
    }

    #[test]
    fn test_should_tick() {
        let config = NpcBrainConfig {
            tick_rate: 2.0,
            ..Default::default()
        };
        let mut state = NpcBrainState::new(config);
        state.last_tick = 10.0;

        // Not enough time
        assert!(!state.should_tick(11.0));
        // Enough time
        assert!(state.should_tick(12.0));
        assert!(state.should_tick(13.0));

        // Inactive brain should not tick
        state.active = false;
        assert!(!state.should_tick(13.0));
    }

    #[test]
    fn test_parse_speak() {
        let action = parse_npc_action("speak(\"Welcome, traveler!\")");
        match action {
            NpcAction::Speak { text } => assert_eq!(text, "Welcome, traveler!"),
            other => panic!("Expected Speak, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_move_to() {
        let action = parse_npc_action("move_to(1.0, 2.5, 3.0)");
        match action {
            NpcAction::MoveTo { position } => {
                assert_eq!(position, [1.0, 2.5, 3.0]);
            }
            other => panic!("Expected MoveTo, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_emote() {
        let action = parse_npc_action("emote(wave)");
        match action {
            NpcAction::Emote { emote_type } => assert_eq!(emote_type, EmoteType::Wave),
            other => panic!("Expected Emote, got {:?}", other),
        }

        let action = parse_npc_action("emote(laugh)");
        match action {
            NpcAction::Emote { emote_type } => assert_eq!(emote_type, EmoteType::Laugh),
            other => panic!("Expected Emote, got {:?}", other),
        }
    }

    #[test]
    fn test_parse_fallback() {
        let action = parse_npc_action("some garbage text");
        assert!(matches!(action, NpcAction::Wait));

        let action = parse_npc_action("wait");
        assert!(matches!(action, NpcAction::Wait));

        let action = parse_npc_action("wander around");
        assert!(matches!(action, NpcAction::Wander));
    }

    #[test]
    fn test_build_context() {
        let config = NpcBrainConfig {
            personality: "a wise elder".to_string(),
            goals: vec!["protect the village".to_string()],
            knowledge: vec!["dragons roam the north".to_string()],
            ..Default::default()
        };
        let brain = NpcBrainState::new(config);
        let perception = PerceptionSnapshot {
            npc_position: [1.0, 0.0, 2.0],
            current_action: NpcAction::Wait,
            nearby_entities: vec![PerceivedEntity {
                name: "oak_tree".to_string(),
                entity_type: "prop".to_string(),
                position: [5.0, 0.0, 2.0],
                distance: 4.0,
                direction: "east".to_string(),
            }],
            player_info: Some(PerceivedEntity {
                name: "player".to_string(),
                entity_type: "player".to_string(),
                position: [3.0, 0.0, 2.0],
                distance: 2.0,
                direction: "east".to_string(),
            }),
            recent_events: vec![],
        };

        let ctx = build_brain_context(&brain, &perception, Some("Met a traveler yesterday"));

        assert!(ctx.contains("You are a wise elder."));
        assert!(ctx.contains("protect the village"));
        assert!(ctx.contains("dragons roam the north"));
        assert!(ctx.contains("Met a traveler yesterday"));
        assert!(ctx.contains("oak_tree"));
        assert!(ctx.contains("Player at 2.0m east"));
        assert!(ctx.contains("Available actions"));
    }
}

// ===========================================================================
// Brain Task Loop — Bevy systems + async Ollama integration
// ===========================================================================

/// Result of an async brain tick returned from the Ollama worker thread.
pub struct BrainTickResult {
    /// The Bevy entity this result is for.
    pub entity: Entity,
    /// The NPC's chosen action.
    pub action: NpcAction,
    /// Optional memory to record (if the NPC said something notable).
    pub memory: Option<String>,
}

/// Bevy Resource: channel for receiving brain tick results from async Ollama calls.
///
/// Uses `Mutex<Receiver>` because `std::sync::mpsc::Receiver` is not `Sync`,
/// which Bevy requires for Resources. The Mutex is only locked briefly during
/// `try_recv` polls each frame.
#[derive(Resource)]
pub struct BrainTaskChannel {
    pub sender: std_mpsc::Sender<BrainTickResult>,
    pub receiver: std::sync::Mutex<std_mpsc::Receiver<BrainTickResult>>,
}

impl Default for BrainTaskChannel {
    fn default() -> Self {
        let (sender, receiver) = std_mpsc::channel();
        Self {
            sender,
            receiver: std::sync::Mutex::new(receiver),
        }
    }
}

/// Bevy Resource: Ollama endpoint configuration.
#[derive(Resource, Clone)]
pub struct OllamaConfig {
    pub endpoint: String,
    /// If true, Ollama availability has been checked and failed. Skip until retry.
    pub unavailable: bool,
    /// Timestamp of last availability check (seconds since startup).
    pub last_check: f64,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            endpoint: ollama_url(),
            unavailable: false,
            last_check: 0.0,
        }
    }
}

/// Ollama's base URL (`LOCALGPT_GEN_OLLAMA_URL`, default `http://localhost:11434`).
pub fn ollama_url() -> String {
    std::env::var(localgpt_core::env::LOCALGPT_GEN_OLLAMA_URL)
        .unwrap_or_else(|_| "http://localhost:11434".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Check that Ollama answers at `url` and has `model` pulled, so a brain
/// isn't attached that could never think.
pub async fn check_ollama_model(url: &str, model: &str) -> anyhow::Result<()> {
    let tags: serde_json::Value = reqwest::Client::new()
        .get(format!("{url}/api/tags"))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Ollama is not reachable at {url} ({e}); NPC brains need it running \
                 (`ollama serve`, or set {})",
                localgpt_core::env::LOCALGPT_GEN_OLLAMA_URL
            )
        })?
        .error_for_status()?
        .json()
        .await?;
    let bare = |name: &str| name.strip_suffix(":latest").unwrap_or(name).to_string();
    let pulled: Vec<String> = tags["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["name"].as_str().map(bare))
        .collect();
    if pulled.contains(&bare(model)) {
        Ok(())
    } else {
        anyhow::bail!(
            "Ollama at {url} doesn't have '{model}' (pulled: {}); run `ollama pull {model}`",
            if pulled.is_empty() {
                "none".to_string()
            } else {
                pulled.join(", ")
            }
        )
    }
}

/// Ask Ollama's chat endpoint one question (async; for tools).
pub async fn ask_ollama(
    url: &str,
    model: &str,
    system: &str,
    prompt: &str,
) -> anyhow::Result<String> {
    let json: serde_json::Value = reqwest::Client::new()
        .post(format!("{url}/api/chat"))
        .timeout(std::time::Duration::from_secs(60))
        .json(&serde_json::json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": prompt }
            ],
            "stream": false,
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    json["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("Ollama returned no message"))
}

/// Call the Ollama /api/chat endpoint synchronously (runs inside a tokio task).
///
/// Returns the assistant message content, or an error string.
fn call_ollama_blocking(endpoint: &str, model: &str, prompt: &str) -> Result<String, String> {
    // Build a single-use tokio runtime for this blocking call.
    // This is called from a std::thread::spawn context, not from Bevy's main thread.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("Failed to create tokio runtime: {}", e))?;

    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("HTTP client error: {}", e))?;

        let body = serde_json::json!({
            "model": model,
            "messages": [
                { "role": "system", "content": "You are an NPC in a 3D world. Respond with exactly ONE action." },
                { "role": "user", "content": prompt }
            ],
            "stream": false,
            "options": {
                "temperature": 0.7,
                "num_predict": 64
            }
        });

        let resp = client
            .post(format!("{}/api/chat", endpoint))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Ollama request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("Ollama returned status {}", resp.status()));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

        json["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "No content in Ollama response".to_string())
    })
}

/// Named things `eye` can perceive: within `radius` and inside a `fov`-degree
/// cone around its forward, nearest first (at most 30), each with distance
/// and bearing (degrees, 0 ahead, positive to the right).
pub fn perceive(
    eye: &Transform,
    fov: f32,
    radius: f32,
    others: impl Iterator<Item = (String, Vec3)>,
) -> Vec<serde_json::Value> {
    let half_fov = (fov.clamp(1.0, 360.0) / 2.0).to_radians();
    let forward = eye.forward().as_vec3();
    let right = eye.right().as_vec3();
    let mut seen: Vec<(f32, serde_json::Value)> = others
        .filter_map(|(name, pos)| {
            let delta = pos - eye.translation;
            let distance = delta.length();
            if distance > radius {
                return None;
            }
            if distance > 1e-3 && forward.angle_between(delta) > half_fov + 1e-4 {
                return None;
            }
            let bearing = delta.dot(right).atan2(delta.dot(forward)).to_degrees();
            Some((
                distance,
                serde_json::json!({
                    "name": name,
                    "distance": (distance * 10.0).round() / 10.0,
                    "bearing_degrees": bearing.round(),
                    "position": pos.to_array(),
                }),
            ))
        })
        .collect();
    seen.sort_by(|a, b| a.0.total_cmp(&b.0));
    seen.into_iter().map(|(_, v)| v).take(30).collect()
}

/// Compute cardinal direction from one position to another.
fn direction_label(from: Vec3, to: Vec3) -> String {
    let delta = to - from;
    let angle = delta.z.atan2(delta.x).to_degrees();
    match angle {
        a if (-45.0..45.0).contains(&a) => "east".to_string(),
        a if (45.0..135.0).contains(&a) => "north".to_string(),
        a if !(-135.0..135.0).contains(&a) => "west".to_string(),
        _ => "south".to_string(),
    }
}

/// Bevy system: gather perception and dispatch async Ollama calls for NPC brains.
///
/// This system runs every frame but only dispatches work when an NPC's tick
/// interval has elapsed and no request is already in-flight.
#[allow(clippy::type_complexity)]
pub fn brain_tick_system(
    time: Res<Time>,
    ollama_config: Res<OllamaConfig>,
    brain_channel: Res<BrainTaskChannel>,
    mut brain_query: Query<(
        Entity,
        &mut NpcBrainState,
        &Transform,
        &Name,
        Option<&super::npc_memory::NpcMemory>,
    )>,
    player_query: Query<&Transform, With<super::player::Player>>,
    all_named: Query<(&Name, &Transform), Without<super::player::Player>>,
) {
    if ollama_config.unavailable {
        return;
    }

    let current_time = time.elapsed_secs_f64();
    let player_transform = player_query.single().ok();

    for (entity, mut brain, npc_transform, npc_name, memory) in brain_query.iter_mut() {
        // Skip if not time to tick or already waiting for a response
        if !brain.should_tick(current_time) || brain.pending_request {
            continue;
        }

        // Gather perception snapshot
        let npc_pos = npc_transform.translation;
        let radius = brain.config.perception_radius;

        let mut nearby = Vec::new();
        for (name, transform) in all_named.iter() {
            if name.as_str() == npc_name.as_str() {
                continue; // skip self
            }
            let dist = npc_pos.distance(transform.translation);
            if dist <= radius {
                nearby.push(PerceivedEntity {
                    name: name.as_str().to_string(),
                    entity_type: "entity".to_string(),
                    position: transform.translation.to_array(),
                    distance: dist,
                    direction: direction_label(npc_pos, transform.translation),
                });
            }
        }

        let player_info = player_transform.map(|pt| {
            let dist = npc_pos.distance(pt.translation);
            PerceivedEntity {
                name: "player".to_string(),
                entity_type: "player".to_string(),
                position: pt.translation.to_array(),
                distance: dist,
                direction: direction_label(npc_pos, pt.translation),
            }
        });

        let perception = PerceptionSnapshot {
            npc_position: npc_pos.to_array(),
            current_action: brain.current_action.clone(),
            nearby_entities: nearby,
            player_info,
            recent_events: brain.recent_events.iter().cloned().collect(),
        };

        // Build memory context
        let memory_ctx = memory.and_then(|m| m.format_for_context(5, current_time));

        // Build prompt
        let prompt = build_brain_context(&brain, &perception, memory_ctx.as_deref());
        let model = brain.config.model.clone();
        let endpoint = ollama_config.endpoint.clone();
        let sender = brain_channel.sender.clone();

        // Mark as pending before spawning
        brain.pending_request = true;
        brain.last_tick = current_time;
        brain.tick_count += 1;

        // Spawn blocking Ollama call on a background thread
        std::thread::spawn(move || {
            let action = match call_ollama_blocking(&endpoint, &model, &prompt) {
                Ok(response) => {
                    let parsed = parse_npc_action(&response);
                    tracing::debug!("NPC brain response: {:?} -> {:?}", response, parsed);
                    parsed
                }
                Err(e) => {
                    tracing::warn!("NPC brain Ollama error: {}", e);
                    NpcAction::Wait
                }
            };

            // Extract memory from speak actions
            let memory = match &action {
                NpcAction::Speak { text } => Some(format!("I said: {}", text)),
                _ => None,
            };

            let _ = sender.send(BrainTickResult {
                entity,
                action,
                memory,
            });
        });
    }
}

/// Bevy system: receive completed brain tick results and apply actions to NPCs.
pub fn brain_apply_results_system(
    time: Res<Time>,
    brain_channel: Res<BrainTaskChannel>,
    mut brain_query: Query<(&mut NpcBrainState, &mut Transform, &Name)>,
    mut memory_query: Query<&mut super::npc_memory::NpcMemory>,
) {
    let current_time = time.elapsed_secs_f64();

    // Drain all pending results (non-blocking)
    let Ok(receiver) = brain_channel.receiver.lock() else {
        return;
    };
    while let Ok(result) = receiver.try_recv() {
        let Ok((mut brain, mut transform, name)) = brain_query.get_mut(result.entity) else {
            continue;
        };

        brain.pending_request = false;
        brain.current_action = result.action.clone();

        // Record event for the NPC's recent events buffer
        let action_desc = match &result.action {
            NpcAction::MoveTo { position } => {
                format!(
                    "Moving to ({:.1}, {:.1}, {:.1})",
                    position[0], position[1], position[2]
                )
            }
            NpcAction::LookAt { entity } => format!("Looking at {}", entity),
            NpcAction::Speak { text } => format!("Said: \"{}\"", text),
            NpcAction::Emote { emote_type } => format!("Emoted: {:?}", emote_type),
            NpcAction::Interact { entity } => format!("Interacting with {}", entity),
            NpcAction::Wait => "Waiting".to_string(),
            NpcAction::Wander => "Wandering".to_string(),
        };
        brain.push_event(action_desc, current_time);

        // Apply immediate actions
        match &result.action {
            NpcAction::MoveTo { position } => {
                // Set target — the NPC wander/patrol system will handle movement.
                // For simplicity, directly update the position for now.
                transform.translation = Vec3::from_array(*position);
            }
            NpcAction::LookAt {
                entity: target_name,
            } => {
                // Look toward the named entity (best-effort)
                tracing::debug!("NPC '{}' looking at '{}'", name.as_str(), target_name);
            }
            NpcAction::Speak { text } => {
                tracing::info!("NPC '{}' says: {}", name.as_str(), text);
            }
            NpcAction::Emote { emote_type } => {
                tracing::info!("NPC '{}' emotes: {:?}", name.as_str(), emote_type);
            }
            NpcAction::Interact { entity } => {
                tracing::info!("NPC '{}' interacts with '{}'", name.as_str(), entity);
            }
            NpcAction::Wander => {
                // Add a small random offset to position
                use rand::RngExt;
                let mut rng = rand::rng();
                let dx: f32 = rng.random_range(-3.0..3.0);
                let dz: f32 = rng.random_range(-3.0..3.0);
                transform.translation.x += dx;
                transform.translation.z += dz;
            }
            NpcAction::Wait => {}
        }

        // Record memory if auto_memorize is enabled
        if let Some(memory_text) = result.memory
            && let Ok(mut mem) = memory_query.get_mut(result.entity)
            && mem.auto_memorize
        {
            mem.add_memory(memory_text, 0.5, current_time);
        }
    }
}

/// Bevy system: periodically check if Ollama is available (every 30 seconds when marked unavailable).
pub fn ollama_health_check_system(time: Res<Time>, mut ollama_config: ResMut<OllamaConfig>) {
    let current_time = time.elapsed_secs_f64();

    // Only check periodically
    if current_time - ollama_config.last_check < 30.0 {
        return;
    }
    ollama_config.last_check = current_time;

    // Quick non-blocking check: try to connect to Ollama
    let endpoint = ollama_config.endpoint.clone();
    let was_unavailable = ollama_config.unavailable;

    // Use a very short timeout for the health check
    use std::net::ToSocketAddrs;
    let addr = reqwest::Url::parse(&endpoint).ok().and_then(|url| {
        let host = url.host_str()?.to_string();
        let port = url.port_or_known_default()?;
        (host.as_str(), port).to_socket_addrs().ok()?.next()
    });
    let reachable = addr.is_some_and(|addr| {
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(500)).is_ok()
    });
    match reachable {
        true => {
            if was_unavailable {
                tracing::info!("Ollama is now available at {}", endpoint);
            }
            ollama_config.unavailable = false;
        }
        false => {
            if !was_unavailable {
                tracing::warn!(
                    "Ollama unavailable at {} — NPC brains will be paused until it comes online",
                    endpoint
                );
            }
            ollama_config.unavailable = true;
        }
    }
}

/// Plugin for the NPC brain system.
pub struct NpcBrainPlugin;

impl Plugin for NpcBrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BrainTaskChannel>()
            .init_resource::<OllamaConfig>()
            .add_systems(Update, brain_tick_system)
            .add_systems(Update, brain_apply_results_system)
            .add_systems(Update, ollama_health_check_system);
    }
}
