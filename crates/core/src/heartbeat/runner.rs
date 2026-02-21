//! Heartbeat runner for continuous autonomous operation

use anyhow::Result;
use chrono::{Local, NaiveTime};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};
use tokio::time::interval_at;
use tracing::{debug, info, warn};

use super::events::{HeartbeatEvent, HeartbeatStatus, emit_heartbeat_event, now_ms};
use crate::agent::{
    Agent, AgentConfig, HEARTBEAT_OK_TOKEN, SessionStore, build_heartbeat_prompt, is_heartbeat_ok,
    tools::Tool,
};
use crate::concurrency::{TurnGate, WorkspaceLock};
use crate::config::{Config, parse_duration, parse_time};
use crate::memory::MemoryManager;

/// Factory function type for creating additional tools for the heartbeat agent.
/// This allows the caller (e.g., CLI daemon) to inject dangerous tools like bash, file I/O.
pub type ToolFactory = Box<dyn Fn(&Config) -> Result<Vec<Box<dyn Tool>>> + Send + Sync>;

pub struct HeartbeatRunner {
    config: Config,
    interval: Duration,
    /// Maximum wall-clock time for a single heartbeat run.
    /// Defaults to half the interval when not explicitly configured.
    run_timeout: Duration,
    active_hours: Option<(NaiveTime, NaiveTime)>,
    workspace: PathBuf,
    agent_id: String,
    /// Cached MemoryManager to avoid reinitializing embedding provider on every heartbeat
    memory: MemoryManager,
    /// In-process turn gate (shared with HTTP server when running in daemon)
    turn_gate: Option<TurnGate>,
    /// Cross-process workspace lock
    workspace_lock: WorkspaceLock,
    /// Optional tool factory for injecting additional tools (e.g., CLI tools from daemon)
    tool_factory: Option<ToolFactory>,
}

impl HeartbeatRunner {
    /// Create a new HeartbeatRunner with the default agent ID ("main")
    pub fn new(config: &Config) -> Result<Self> {
        Self::new_with_agent(config, "main")
    }

    /// Create a new HeartbeatRunner for a specific agent ID
    pub fn new_with_agent(config: &Config, agent_id: &str) -> Result<Self> {
        Self::new_with_gate(config, agent_id, None)
    }

    /// Create a new HeartbeatRunner with an optional in-process TurnGate.
    ///
    /// When running inside the daemon alongside the HTTP server, pass a
    /// shared `TurnGate` so heartbeat skips when an HTTP agent turn is active.
    pub fn new_with_gate(
        config: &Config,
        agent_id: &str,
        turn_gate: Option<TurnGate>,
    ) -> Result<Self> {
        Self::new_with_gate_and_tools(config, agent_id, turn_gate, None)
    }

    /// Create a new HeartbeatRunner with optional TurnGate and tool factory.
    ///
    /// The tool_factory allows injecting additional tools (e.g., bash, file I/O from CLI)
    /// into the heartbeat agent. This enables the heartbeat to perform filesystem operations
    /// and execute commands when running in a CLI/daemon context.
    pub fn new_with_gate_and_tools(
        config: &Config,
        agent_id: &str,
        turn_gate: Option<TurnGate>,
        tool_factory: Option<ToolFactory>,
    ) -> Result<Self> {
        let interval = parse_duration(&config.heartbeat.interval)
            .map_err(|e| anyhow::anyhow!("Invalid heartbeat interval: {}", e))?;

        // Resolve run timeout: explicit config value or default to half the interval.
        let run_timeout = if let Some(ref t) = config.heartbeat.timeout {
            parse_duration(t).map_err(|e| anyhow::anyhow!("Invalid heartbeat timeout: {}", e))?
        } else {
            interval / 2
        };

        let active_hours = if let Some(ref hours) = config.heartbeat.active_hours {
            let (start_h, start_m) = parse_time(&hours.start)
                .map_err(|e| anyhow::anyhow!("Invalid start time: {}", e))?;
            let (end_h, end_m) =
                parse_time(&hours.end).map_err(|e| anyhow::anyhow!("Invalid end time: {}", e))?;

            Some((
                NaiveTime::from_hms_opt(start_h as u32, start_m as u32, 0).unwrap(),
                NaiveTime::from_hms_opt(end_h as u32, end_m as u32, 0).unwrap(),
            ))
        } else {
            None
        };

        let workspace = config.workspace_path();

        // Create MemoryManager once and reuse it to avoid reinitializing embedding provider
        let memory = MemoryManager::new_with_full_config(&config.memory, Some(config), agent_id)?;
        let workspace_lock = WorkspaceLock::new()?;

        Ok(Self {
            config: config.clone(),
            interval,
            run_timeout,
            active_hours,
            workspace,
            agent_id: agent_id.to_string(),
            memory,
            turn_gate,
            workspace_lock,
            tool_factory,
        })
    }

    async fn first_delay(&self) -> Duration {
        // Read last heartbeat event to calibrate first tick time
        if let Ok(json) = fs::read_to_string(self.config.paths.last_heartbeat())
            && let Ok(event) = serde_json::from_str::<HeartbeatEvent>(&json)
        {
            let last_tick_end = std::time::UNIX_EPOCH + Duration::from_millis(event.ts);
            let last_tick_elapsed = Duration::from_millis(event.duration_ms);
            let last_tick = last_tick_end - last_tick_elapsed;
            debug!(
                name: "Heartbeat",
                "loaded last_tick: {:?} (ts: {}, duration_ms: {})",
                last_tick, event.ts, event.duration_ms
            );

            let next_tick = last_tick + self.interval;
            let now = SystemTime::now();
            if now < next_tick {
                return next_tick.duration_since(now).unwrap_or(Duration::ZERO);
            }
        }

        // heartbeat is overdue
        parse_duration(&self.config.heartbeat.overdue_delay).unwrap_or_else(|e| {
            warn!(name: "Heartbeat", "invalid overdue_delay: {}, falling back to zero", e);
            Duration::ZERO
        })
    }

    /// Run the heartbeat loop continuously
    pub async fn run(&self) -> Result<()> {
        info!(name: "Heartbeat", "starting runner with interval: {:?}", self.interval);
        info!(name: "Heartbeat", "run timeout: {:?}", self.run_timeout);

        // Schedule first tick at next interval from last tick
        let first_after = self.first_delay().await;
        let first_at = tokio::time::Instant::now() + first_after;
        let mut interval = interval_at(first_at, self.interval);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        info!(
            name: "Heartbeat",
            "first tick scheduled after: {:?} at: {:?}",
            first_after,
            first_at,
        );

        // Exponential backoff for SkippedMayTry retries
        let mut skips_since_last = 0;
        let skip_retry_base = Duration::from_millis(1000);
        let skip_retry_max = self.interval / 2;

        loop {
            interval.tick().await; // Sleep until next interval

            // Check active hours
            if !self.in_active_hours() {
                info!(name: "Heartbeat", "skipping: outside active hours");
                emit_heartbeat_event(HeartbeatEvent {
                    ts: now_ms(),
                    status: HeartbeatStatus::Skipped,
                    duration_ms: 0,
                    preview: None,
                    reason: Some("outside active hours".to_string()),
                });
                continue;
            }

            // Run heartbeat with timing, enforcing the configured deadline
            let start = Instant::now();
            info!(name: "Heartbeat", "tick starting at: {:?}", start);

            let timed_result =
                tokio::time::timeout(self.run_timeout, self.run_once_internal()).await;
            let elapsed = start.elapsed();

            let res = match timed_result {
                Ok(inner) => inner,
                Err(_) => {
                    warn!(
                        name: "Heartbeat",
                        "tick timed out after {:?} (deadline: {:?})", elapsed, self.run_timeout
                    );
                    let event = HeartbeatEvent {
                        ts: now_ms(),
                        status: HeartbeatStatus::TimedOut,
                        duration_ms: elapsed.as_millis() as u64,
                        preview: None,
                        reason: Some(format!(
                            "exceeded deadline of {}",
                            &self
                                .config
                                .heartbeat
                                .timeout
                                .as_deref()
                                .unwrap_or("half the interval")
                        )),
                    };
                    if let Err(e) = serde_json::to_writer_pretty(
                        fs::File::create(self.config.paths.last_heartbeat())?,
                        &event,
                    ) {
                        warn!(name: "Heartbeat", "failed to write event: {}", e);
                    }
                    emit_heartbeat_event(event);
                    skips_since_last = 0;
                    info!(name: "Heartbeat", "waiting for next tick");
                    continue;
                }
            };

            info!(name: "Heartbeat", "tick done elapsed: {:?}", elapsed);

            let event = match res {
                Ok((response, status)) => {
                    let preview = if response.len() > 200 {
                        Some(format!("{}...", &response[..200]))
                    } else {
                        Some(response.clone())
                    };

                    if is_heartbeat_ok(&response) {
                        debug!(name: "Heartbeat", "OK");
                    } else {
                        warn!(name: "Heartbeat", "response not OK: {}", response);
                    }

                    if status == HeartbeatStatus::SkippedMayTry {
                        skips_since_last += 1;
                        let retry_after =
                            (skip_retry_base * 2_u32.pow(skips_since_last)).min(skip_retry_max);
                        interval.reset_after(retry_after);
                        info!(name: "Heartbeat", "transient skip, retry quickly after: {:?}", retry_after);
                    } else {
                        skips_since_last = 0;
                    }

                    HeartbeatEvent {
                        ts: now_ms(),
                        status: status.clone(),
                        duration_ms: elapsed.as_millis() as u64,
                        preview,
                        reason: None,
                    }
                }
                Err(e) => {
                    warn!(name: "Heartbeat", "error: {}", e);
                    HeartbeatEvent {
                        ts: now_ms(),
                        status: HeartbeatStatus::Failed,
                        duration_ms: elapsed.as_millis() as u64,
                        preview: None,
                        reason: Some(e.to_string()),
                    }
                }
            };

            // Persist any non-transient heartbeat event to disk
            if event.status != HeartbeatStatus::SkippedMayTry
                && let Err(e) = serde_json::to_writer_pretty(
                    fs::File::create(self.config.paths.last_heartbeat())?,
                    &event,
                )
            {
                warn!(name: "Heartbeat", "failed to write event: {}", e);
            }

            emit_heartbeat_event(event);

            info!(name: "Heartbeat", "waiting for next tick");
        }
    }

    /// Run a single heartbeat cycle (public API, emits events)
    pub async fn run_once(&self) -> Result<String> {
        let start = Instant::now();

        match self.run_once_internal().await {
            Ok((response, status)) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                let preview = if response.len() > 200 {
                    Some(format!("{}...", &response[..200]))
                } else {
                    Some(response.clone())
                };

                emit_heartbeat_event(HeartbeatEvent {
                    ts: now_ms(),
                    status,
                    duration_ms,
                    preview,
                    reason: None,
                });

                Ok(response)
            }
            Err(e) => {
                let duration_ms = start.elapsed().as_millis() as u64;
                emit_heartbeat_event(HeartbeatEvent {
                    ts: now_ms(),
                    status: HeartbeatStatus::Failed,
                    duration_ms,
                    preview: None,
                    reason: Some(e.to_string()),
                });
                Err(e)
            }
        }
    }

    /// Internal heartbeat execution (returns response and status)
    async fn run_once_internal(&self) -> Result<(String, HeartbeatStatus)> {
        // Skip if an in-process agent turn is already in flight
        if let Some(ref gate) = self.turn_gate
            && gate.is_busy()
        {
            info!(name: "Heartbeat", "skipping: agent turn in flight (TurnGate busy)");
            return Ok((
                HEARTBEAT_OK_TOKEN.to_string(),
                HeartbeatStatus::SkippedMayTry,
            ));
        }

        // Try to acquire the cross-process workspace lock (non-blocking)
        let _ws_guard = match self.workspace_lock.try_acquire()? {
            Some(guard) => guard,
            None => {
                info!(name: "Heartbeat", "skipping: workspace locked by another process");
                return Ok((
                    HEARTBEAT_OK_TOKEN.to_string(),
                    HeartbeatStatus::SkippedMayTry,
                ));
            }
        };

        // Try to acquire the in-process turn gate (non-blocking, race between
        // the is_busy check above and now)
        let _gate_permit = if let Some(ref gate) = self.turn_gate {
            match gate.try_acquire() {
                Some(permit) => Some(permit),
                None => {
                    info!(name: "Heartbeat", "skipping: agent turn started between check and acquire");
                    return Ok((
                        HEARTBEAT_OK_TOKEN.to_string(),
                        HeartbeatStatus::SkippedMayTry,
                    ));
                }
            }
        } else {
            None
        };

        // Check if HEARTBEAT.md exists and has content
        let heartbeat_path = self.workspace.join("HEARTBEAT.md");

        if !heartbeat_path.exists() {
            info!(name: "Heartbeat", "skipping: no HEARTBEAT.md");
            return Ok((HEARTBEAT_OK_TOKEN.to_string(), HeartbeatStatus::Skipped));
        }

        let content = fs::read_to_string(&heartbeat_path)?;
        if content.trim().is_empty() {
            info!(name: "Heartbeat", "skipping: empty HEARTBEAT.md");
            return Ok((HEARTBEAT_OK_TOKEN.to_string(), HeartbeatStatus::Skipped));
        }

        // Create agent for heartbeat (clone the cached MemoryManager to share the embedding provider)
        let agent_config = AgentConfig {
            model: self.config.agent.default_model.clone(),
            context_window: self.config.agent.context_window,
            reserve_tokens: self.config.agent.reserve_tokens,
        };

        let mut agent = Agent::new(agent_config, &self.config, self.memory.clone()).await?;

        // Extend agent with additional tools from factory if provided (e.g., CLI tools from daemon)
        if let Some(ref factory) = self.tool_factory {
            let extra_tools = factory(&self.config)?;
            agent.extend_tools(extra_tools);
        }

        agent.new_session().await?;

        info!(name: "Heartbeat", "Running HEARTBEAT.md");

        // Check if workspace is a git repo
        let workspace_is_git = self.workspace.join(".git").exists();

        // Send heartbeat prompt; save session after each tool call round so the log
        // is visible while the heartbeat is still running.
        let heartbeat_prompt = build_heartbeat_prompt(workspace_is_git);
        let response = agent
            .chat_saving_session(&heartbeat_prompt, &self.agent_id)
            .await?;

        // Save final session log
        match agent.save_session_for_agent(&self.agent_id).await {
            Ok(path) => {
                info!(name: "Heartbeat", "saved session: {:?}", path.to_str().unwrap_or("<Unknown>"));
            }
            Err(error) => {
                warn!(name: "Heartbeat", "failed to save session: {:?}", error);
            }
        }

        // Determine status based on response
        if is_heartbeat_ok(&response) {
            return Ok((response, HeartbeatStatus::Ok));
        }

        // For actual alerts, check for deduplication
        let session_key = "heartbeat"; // Use dedicated session key for heartbeat state

        // Load session store to check for duplicates
        if let Ok(mut store) = SessionStore::load_for_agent(&self.agent_id) {
            if let Some(entry) = store.get(session_key)
                && entry.is_duplicate_heartbeat(&response)
            {
                info!(name: "Heartbeat",
                    "skipping: duplicate (same text within 24h): {}",
                    &response[..response.len().min(100)]
                );
                return Ok((response, HeartbeatStatus::Skipped));
            }

            // Record the heartbeat (re-read from disk to avoid clobbering)
            let session_id = agent.session_status().id;
            if let Err(e) = store.load_and_update(session_key, &session_id, |entry| {
                entry.record_heartbeat(&response);
            }) {
                warn!(name: "Heartbeat", "Failed to record in session store: {}", e);
            }
        }

        Ok((response, HeartbeatStatus::Sent))
    }

    fn in_active_hours(&self) -> bool {
        let Some((start, end)) = self.active_hours else {
            return true; // No active hours configured, always active
        };

        let now = Local::now().time();

        if start <= end {
            // Normal range (e.g., 09:00 to 22:00)
            now >= start && now <= end
        } else {
            // Overnight range (e.g., 22:00 to 06:00)
            now >= start || now <= end
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_active_hours_normal_range() {
        // This test would require mocking Local::now()
        // For now, just verify the logic pattern
        let start = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        let end = NaiveTime::from_hms_opt(22, 0, 0).unwrap();

        let noon = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();

        assert!(noon >= start && noon <= end);
        assert!(!(midnight >= start && midnight <= end));
    }

    #[test]
    fn test_tool_factory_type_signature() {
        // Verify that the ToolFactory type can be constructed with a closure
        // that matches create_cli_tools signature
        let _factory: ToolFactory = Box::new(|_config: &Config| {
            // Simulate returning CLI tools
            Ok(Vec::new())
        });

        // This test just verifies the type signature compiles correctly
        // Actual tool creation is tested in integration tests
    }

    #[test]
    fn test_run_timeout_defaults_to_half_interval() {
        // Default HeartbeatConfig has interval="30m", no explicit timeout
        let config = Config::default();
        assert_eq!(config.heartbeat.timeout, None);

        // Verify that interval / 2 is the default timeout (30m / 2 = 15m)
        let interval =
            parse_duration(&config.heartbeat.interval).expect("default interval is valid");
        let expected_timeout = interval / 2;
        assert_eq!(expected_timeout, Duration::from_secs(15 * 60));
    }

    #[test]
    fn test_explicit_timeout_config() {
        use crate::config::HeartbeatConfig;
        let cfg = HeartbeatConfig {
            timeout: Some("5m".to_string()),
            ..HeartbeatConfig::default()
        };
        let timeout = parse_duration(cfg.timeout.as_ref().unwrap()).unwrap();
        assert_eq!(timeout, Duration::from_secs(5 * 60));
    }
}
