//! Failover provider that tries multiple LLM providers in sequence
//!
//! When a provider fails with a retryable error (rate limit, timeout, server error),
//! the FailoverProvider automatically tries the next provider in the chain.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use tracing::warn;

use super::providers::{LLMProvider, LLMResponse, Message, StreamResult, ToolSchema};

/// Duration to cooldown a failed provider before retrying
const COOLDOWN_SECS: u64 = 60;

/// Provider that wraps multiple LLM providers and tries them in sequence on failure.
///
/// Retryable errors include:
/// - HTTP 429 (rate limit)
/// - HTTP 500, 502, 503 (server errors)
/// - Timeouts
/// - Connection refused/reset
///
/// Non-retryable errors (e.g., 401 unauthorized, 400 bad request) fail immediately.
pub struct FailoverProvider {
    providers: Vec<Box<dyn LLMProvider>>,
    /// Cooldown expiry timestamps as seconds since start (AtomicU64 for thread safety)
    /// 0 means not in cooldown
    cooldowns: Vec<AtomicU64>,
    /// Reference time for cooldown calculations
    start_instant: Instant,
}

impl FailoverProvider {
    /// Create a new FailoverProvider with the given providers.
    /// The first provider is the primary, followed by fallbacks in order.
    pub fn new(providers: Vec<Box<dyn LLMProvider>>) -> Self {
        let count = providers.len();
        Self {
            providers,
            cooldowns: (0..count).map(|_| AtomicU64::new(0)).collect(),
            start_instant: Instant::now(),
        }
    }

    /// Check if an error is retryable (should try next provider)
    fn is_retryable(err: &anyhow::Error) -> bool {
        let msg = err.to_string().to_lowercase();
        // Rate limits
        msg.contains("429") || msg.contains("rate limit") || msg.contains("ratelimit") ||
        // Server errors
        msg.contains("500") || msg.contains("502") || msg.contains("503") ||
        // Network issues
        msg.contains("timeout") ||
        msg.contains("connection refused") ||
        msg.contains("connection reset") ||
        msg.contains("connection closed") ||
        msg.contains("timed out")
    }

    /// Check if a provider is in cooldown
    fn is_in_cooldown(&self, index: usize) -> bool {
        let expiry_secs = self.cooldowns[index].load(Ordering::Relaxed);
        if expiry_secs == 0 {
            return false;
        }
        let elapsed = self.start_instant.elapsed().as_secs();
        elapsed < expiry_secs
    }

    /// Put a provider into cooldown
    fn set_cooldown(&self, index: usize) {
        let expiry = self.start_instant.elapsed().as_secs() + COOLDOWN_SECS;
        self.cooldowns[index].store(expiry, Ordering::Relaxed);
    }
}

#[async_trait]
impl LLMProvider for FailoverProvider {
    fn name(&self) -> String {
        let names: Vec<_> = self.providers.iter().map(|p| p.name()).collect();
        format!("failover({})", names.join(" → "))
    }

    async fn chat(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<LLMResponse> {
        let mut last_err = None;

        for (i, provider) in self.providers.iter().enumerate() {
            // Skip if in cooldown
            if self.is_in_cooldown(i) {
                warn!("Provider {} ({}) in cooldown, skipping", i, provider.name());
                continue;
            }

            match provider.chat(messages, tools).await {
                Ok(result) => return Ok(result),
                Err(e) if Self::is_retryable(&e) => {
                    warn!(
                        "Provider {} ({}) failed (retryable): {}, trying next",
                        i,
                        provider.name(),
                        e
                    );
                    self.set_cooldown(i);
                    last_err = Some(e);
                }
                Err(e) => {
                    // Non-retryable error, fail immediately
                    warn!(
                        "Provider {} ({}) failed (non-retryable): {}",
                        i,
                        provider.name(),
                        e
                    );
                    return Err(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!(
                "All {} providers in cooldown or unavailable",
                self.providers.len()
            )
        }))
    }

    async fn summarize(&self, text: &str) -> Result<String> {
        let mut last_err = None;

        for (i, provider) in self.providers.iter().enumerate() {
            if self.is_in_cooldown(i) {
                warn!("Provider {} ({}) in cooldown, skipping", i, provider.name());
                continue;
            }

            match provider.summarize(text).await {
                Ok(result) => return Ok(result),
                Err(e) if Self::is_retryable(&e) => {
                    warn!(
                        "Provider {} ({}) failed (retryable): {}, trying next",
                        i,
                        provider.name(),
                        e
                    );
                    self.set_cooldown(i);
                    last_err = Some(e);
                }
                Err(e) => {
                    warn!(
                        "Provider {} ({}) failed (non-retryable): {}",
                        i,
                        provider.name(),
                        e
                    );
                    return Err(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!(
                "All {} providers in cooldown or unavailable",
                self.providers.len()
            )
        }))
    }

    async fn chat_stream(
        &self,
        messages: &[Message],
        tools: Option<&[ToolSchema]>,
    ) -> Result<StreamResult> {
        let mut last_err = None;

        for (i, provider) in self.providers.iter().enumerate() {
            if self.is_in_cooldown(i) {
                warn!("Provider {} ({}) in cooldown, skipping", i, provider.name());
                continue;
            }

            match provider.chat_stream(messages, tools).await {
                Ok(result) => return Ok(result),
                Err(e) if Self::is_retryable(&e) => {
                    warn!(
                        "Provider {} ({}) failed (retryable): {}, trying next",
                        i,
                        provider.name(),
                        e
                    );
                    self.set_cooldown(i);
                    last_err = Some(e);
                }
                Err(e) => {
                    warn!(
                        "Provider {} ({}) failed (non-retryable): {}",
                        i,
                        provider.name(),
                        e
                    );
                    return Err(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            anyhow::anyhow!(
                "All {} providers in cooldown or unavailable",
                self.providers.len()
            )
        }))
    }

    fn reset_session(&self) {
        // Reset all providers
        for provider in &self.providers {
            provider.reset_session();
        }
    }

    // Both reach every provider: a fallback that answers a turn must not
    // resume a conversation the primary was told to leave alone.
    fn make_ephemeral(&self) {
        for provider in &self.providers {
            provider.make_ephemeral();
        }
    }

    fn set_session_owner(&self, agent_id: &str) {
        for provider in &self.providers {
            provider.set_session_owner(agent_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_retryable_rate_limit() {
        let err = anyhow::anyhow!("Error: 429 Too Many Requests");
        assert!(FailoverProvider::is_retryable(&err));

        let err = anyhow::anyhow!("Rate limit exceeded");
        assert!(FailoverProvider::is_retryable(&err));
    }

    #[test]
    fn test_is_retryable_server_error() {
        let err = anyhow::anyhow!("Error: 500 Internal Server Error");
        assert!(FailoverProvider::is_retryable(&err));

        let err = anyhow::anyhow!("Error: 502 Bad Gateway");
        assert!(FailoverProvider::is_retryable(&err));

        let err = anyhow::anyhow!("Error: 503 Service Unavailable");
        assert!(FailoverProvider::is_retryable(&err));
    }

    #[test]
    fn test_is_retryable_network_error() {
        let err = anyhow::anyhow!("Connection refused");
        assert!(FailoverProvider::is_retryable(&err));

        let err = anyhow::anyhow!("Request timed out");
        assert!(FailoverProvider::is_retryable(&err));

        let err = anyhow::anyhow!("Connection reset by peer");
        assert!(FailoverProvider::is_retryable(&err));
    }

    #[test]
    fn test_is_not_retryable_auth_error() {
        let err = anyhow::anyhow!("Error: 401 Unauthorized");
        assert!(!FailoverProvider::is_retryable(&err));

        let err = anyhow::anyhow!("Error: 400 Bad Request");
        assert!(!FailoverProvider::is_retryable(&err));
    }

    /// What a CLI provider is told about its session reaches every provider
    /// in the chain, fallbacks included.
    #[test]
    fn session_calls_reach_every_provider() {
        use std::sync::{Arc, Mutex};

        struct Recorder(Arc<Mutex<Vec<String>>>);
        #[async_trait]
        impl LLMProvider for Recorder {
            fn name(&self) -> String {
                "recorder".into()
            }
            async fn chat(&self, _: &[Message], _: Option<&[ToolSchema]>) -> Result<LLMResponse> {
                Ok(LLMResponse::text(String::new()))
            }
            async fn summarize(&self, _: &str) -> Result<String> {
                Ok(String::new())
            }
            fn make_ephemeral(&self) {
                self.0.lock().unwrap().push("ephemeral".into());
            }
            fn set_session_owner(&self, agent_id: &str) {
                self.0.lock().unwrap().push(format!("owner {agent_id}"));
            }
        }

        let (a, b) = (Arc::default(), Arc::default());
        let chain = FailoverProvider::new(vec![
            Box::new(Recorder(Arc::clone(&a))),
            Box::new(Recorder(Arc::clone(&b))),
        ]);
        chain.set_session_owner("gen");
        chain.make_ephemeral();
        for calls in [a, b] {
            assert_eq!(*calls.lock().unwrap(), ["owner gen", "ephemeral"]);
        }
    }
}
