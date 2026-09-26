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
    /// Allow this call again, with identical arguments, for the rest of the
    /// session. It does not approve the same tool with other arguments.
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
/// so the user isn't asked again for the same call.
///
/// Decisions are keyed by tool name *and* arguments: approving
/// `bash {"command":"ls"}` for the session does not approve
/// `bash {"command":"rm -rf ~"}`. JSON arguments are canonicalized (object
/// keys sorted, whitespace dropped) so a re-ordered but identical call still
/// hits the cache.
pub struct ApprovalCache {
    decisions: Mutex<HashMap<(String, String), ApprovalDecision>>,
}

impl ApprovalCache {
    pub fn new() -> Self {
        Self {
            decisions: Mutex::new(HashMap::new()),
        }
    }

    /// Check if this exact call has a cached approval decision.
    pub fn get(&self, tool_name: &str, arguments: &str) -> Option<ApprovalDecision> {
        let key = cache_key(tool_name, arguments);
        self.decisions
            .lock()
            .ok()
            .and_then(|d| d.get(&key).cloned())
    }

    /// Cache an "approved for session" decision for this exact call.
    pub fn insert(&self, tool_name: &str, arguments: &str, decision: ApprovalDecision) {
        if let Ok(mut d) = self.decisions.lock() {
            d.insert(cache_key(tool_name, arguments), decision);
        }
    }
}

fn cache_key(tool_name: &str, arguments: &str) -> (String, String) {
    let args = match serde_json::from_str::<serde_json::Value>(arguments) {
        Ok(value) => canonical_json(&value),
        Err(_) => arguments.to_string(),
    };
    (tool_name.to_string(), args)
}

/// Serialize JSON with object keys sorted at every level, independent of
/// whether serde_json's `preserve_order` feature is enabled.
fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let fields: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::Value::String(k.clone()),
                        canonical_json(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", fields.join(","))
        }
        serde_json::Value::Array(items) => {
            let items: Vec<String> = items.iter().map(canonical_json).collect();
            format!("[{}]", items.join(","))
        }
        other => other.to_string(),
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
        let ls = r#"{"command":"ls"}"#;
        assert!(cache.get("bash", ls).is_none());

        cache.insert("bash", ls, ApprovalDecision::ApprovedForSession);
        assert_eq!(
            cache.get("bash", ls),
            Some(ApprovalDecision::ApprovedForSession)
        );
    }

    #[test]
    fn test_approval_cache_is_per_call_not_per_tool() {
        let cache = ApprovalCache::new();
        cache.insert(
            "bash",
            r#"{"command":"ls"}"#,
            ApprovalDecision::ApprovedForSession,
        );
        assert!(cache.get("bash", r#"{"command":"rm -rf ~"}"#).is_none());
        assert!(cache.get("write_file", r#"{"command":"ls"}"#).is_none());
    }

    #[test]
    fn test_approval_cache_ignores_key_order_and_whitespace() {
        let cache = ApprovalCache::new();
        cache.insert(
            "write_file",
            r#"{"path":"a.txt","content":"hi"}"#,
            ApprovalDecision::ApprovedForSession,
        );
        assert_eq!(
            cache.get("write_file", r#"{ "content": "hi", "path": "a.txt" }"#),
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
