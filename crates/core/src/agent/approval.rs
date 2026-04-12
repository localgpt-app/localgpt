//! Approval gate for elevated tool execution.
//!
//! When a tool requires a permission level above the session's configured level,
//! the agent asks the approval gate for permission before executing.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

use super::tools::PermissionLevel;

/// Request for permission to execute an elevated tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub tool_name: String,
    pub arguments: String,
    pub level: PermissionLevel,
}

/// Decision from the approval gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalDecision {
    /// Allow this single execution.
    Approved,
    /// Allow all future executions of this tool in this session.
    ApprovedForSession,
    /// Deny execution.
    Denied { reason: String },
}

/// Trait for pluggable approval mechanisms (CLI prompt, HTTP modal, auto-approve, etc.).
#[async_trait]
pub trait ApprovalGate: Send + Sync {
    async fn request_approval(&self, request: &ApprovalRequest) -> Result<ApprovalDecision>;
}

/// Auto-approve gate — approves all requests without prompting.
/// Used when `auto_approve_loopback = true` for localhost connections.
pub struct AutoApproveGate;

#[async_trait]
impl ApprovalGate for AutoApproveGate {
    async fn request_approval(&self, _request: &ApprovalRequest) -> Result<ApprovalDecision> {
        Ok(ApprovalDecision::Approved)
    }
}

/// Auto-deny gate — denies all elevated requests.
/// Used for server/mobile agents that should never run dangerous tools.
pub struct AutoDenyGate;

#[async_trait]
impl ApprovalGate for AutoDenyGate {
    async fn request_approval(&self, request: &ApprovalRequest) -> Result<ApprovalDecision> {
        Ok(ApprovalDecision::Denied {
            reason: format!(
                "Tool '{}' requires {} permission (auto-denied)",
                request.tool_name, request.level
            ),
        })
    }
}

/// Session-scoped approval cache. Remembers "approved for session" decisions
/// so the user isn't asked repeatedly for the same tool.
pub struct ApprovalCache {
    decisions: Mutex<HashMap<String, ApprovalDecision>>,
}

impl ApprovalCache {
    pub fn new() -> Self {
        Self {
            decisions: Mutex::new(HashMap::new()),
        }
    }

    /// Check if a tool has a cached approval decision.
    pub fn get(&self, tool_name: &str) -> Option<ApprovalDecision> {
        self.decisions
            .lock()
            .ok()
            .and_then(|d| d.get(tool_name).cloned())
    }

    /// Cache an "approved for session" decision.
    pub fn insert(&self, tool_name: &str, decision: ApprovalDecision) {
        if let Ok(mut d) = self.decisions.lock() {
            d.insert(tool_name.to_string(), decision);
        }
    }
}

impl Default for ApprovalCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_approval_cache() {
        let cache = ApprovalCache::new();
        assert!(cache.get("bash").is_none());

        cache.insert("bash", ApprovalDecision::ApprovedForSession);
        assert_eq!(
            cache.get("bash"),
            Some(ApprovalDecision::ApprovedForSession)
        );
    }

    #[tokio::test]
    async fn test_auto_approve_gate() {
        let gate = AutoApproveGate;
        let request = ApprovalRequest {
            tool_name: "bash".to_string(),
            arguments: "ls".to_string(),
            level: PermissionLevel::Elevated,
        };
        let decision = gate.request_approval(&request).await.unwrap();
        assert_eq!(decision, ApprovalDecision::Approved);
    }

    #[tokio::test]
    async fn test_auto_deny_gate() {
        let gate = AutoDenyGate;
        let request = ApprovalRequest {
            tool_name: "bash".to_string(),
            arguments: "rm -rf /".to_string(),
            level: PermissionLevel::Elevated,
        };
        let decision = gate.request_approval(&request).await.unwrap();
        assert!(matches!(decision, ApprovalDecision::Denied { .. }));
    }
}
