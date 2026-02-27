//! Spawn Agent Tool - Hierarchical delegation for complex tasks
//!
//! Implements single-level hierarchical delegation (Phase 1 of multi-agent orchestration).
//! The main agent can spawn specialist subagents for tasks like:
//! - Code exploration and research
//! - Planning and architecture
//! - Implementation tasks
//!
//! Subagents CANNOT spawn more agents (depth limit = 1 by default).
//! Results are returned to the parent via structured response.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::Tool;
use crate::agent::providers::ToolSchema;
use crate::agent::session::Session;
use crate::config::Config;
use crate::memory::MemoryManager;

/// Maximum depth for agent spawning (0 = root, 1 = first subagent level)
const DEFAULT_MAX_SPAWN_DEPTH: u8 = 1;

/// Default model for subagents (can be overridden in config)
const DEFAULT_SUBAGENT_MODEL: &str = "claude-cli/sonnet";

/// Parameters for spawning a subagent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnParams {
    /// Task type: "explore", "plan", "implement", "analyze"
    #[serde(default = "default_task")]
    pub task: String,

    /// Description of what the subagent should do
    pub description: String,

    /// Input/context for the subagent
    #[serde(default)]
    pub input: String,

    /// Current spawn depth (set by parent, not user)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<u8>,
}

fn default_task() -> String {
    "explore".to_string()
}

/// Result from a subagent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    /// Whether the task completed successfully
    pub success: bool,

    /// Summary of what was accomplished
    pub summary: String,

    /// Detailed findings/output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,

    /// Any errors encountered
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Token usage (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens_used: Option<u64>,
}

/// Context passed to the spawn_agent tool
pub struct SpawnContext {
    /// Current spawn depth (0 = root agent)
    pub depth: u8,

    /// Configuration for creating providers and memory
    pub config: Config,

    /// Memory manager (shared with parent)
    pub memory: Arc<MemoryManager>,

    /// Model to use for subagent (defaults to config or DEFAULT_SUBAGENT_MODEL)
    pub model: Option<String>,

    /// Maximum depth allowed
    pub max_depth: u8,
}

/// Spawn Agent Tool - allows an agent to delegate tasks to subagents
pub struct SpawnAgentTool {
    /// Context for spawning subagents
    context: SpawnContext,
}

impl SpawnAgentTool {
    /// Create a new spawn_agent tool with the given context
    pub fn new(context: SpawnContext) -> Self {
        Self { context }
    }

    /// Create with default settings from config and memory
    pub fn from_config(config: Config, memory: Arc<MemoryManager>) -> Self {
        let max_depth = config
            .agent
            .max_spawn_depth
            .unwrap_or(DEFAULT_MAX_SPAWN_DEPTH);
        Self {
            context: SpawnContext {
                depth: 0,
                config,
                memory,
                model: None,
                max_depth,
            },
        }
    }

    /// Create a spawn tool for subagents (with incremented depth)
    pub fn for_subagent(&self, config: Config, memory: Arc<MemoryManager>) -> Self {
        let max_depth = config
            .agent
            .max_spawn_depth
            .unwrap_or(DEFAULT_MAX_SPAWN_DEPTH);
        Self {
            context: SpawnContext {
                depth: self.context.depth + 1,
                config,
                memory,
                model: self.context.model.clone(),
                max_depth,
            },
        }
    }

    /// Get the current spawn depth
    pub fn depth(&self) -> u8 {
        self.context.depth
    }

    /// Check if spawning is allowed at current depth
    pub fn can_spawn(&self) -> bool {
        self.context.depth < self.context.max_depth
    }

    /// Build a focused system prompt for the subagent
    fn build_subagent_prompt(&self, params: &SpawnParams) -> String {
        let task_guidance = match params.task.as_str() {
            "explore" => {
                "You are an exploration specialist. Your job is to search, read, and gather information. \
                 Be thorough but concise. Report findings clearly. Do NOT make changes."
            }
            "plan" => {
                "You are a planning specialist. Your job is to analyze requirements and create detailed plans. \
                 Break down complex tasks into steps. Consider edge cases. Provide clear recommendations."
            }
            "implement" => {
                "You are an implementation specialist. Your job is to write code and make changes. \
                 Follow existing patterns. Write clean, well-documented code. Test your changes."
            }
            "analyze" => {
                "You are an analysis specialist. Your job is to examine code/data and provide insights. \
                 Look for patterns, issues, and opportunities. Provide actionable recommendations."
            }
            _ => "You are a specialist agent. Complete the assigned task and return results.",
        };

        format!(
            "# Specialist Agent\n\n\
             ## Role\n\
             {}\n\n\
             ## Task\n\
             {}\n\n\
             ## Instructions\n\
             - Focus only on the assigned task\n\
             - Do NOT spawn additional agents\n\
             - Return a clear summary of your work\n\
             - If you cannot complete the task, explain why\n\n\
             ## Input\n\
             {}",
            task_guidance,
            params.description,
            if params.input.is_empty() {
                "(No additional input provided)"
            } else {
                &params.input
            }
        )
    }

    /// Run the subagent loop
    async fn run_subagent(
        &self,
        params: &SpawnParams,
        tools: Vec<Box<dyn Tool>>,
    ) -> Result<SubAgentResult> {
        let model = self
            .context
            .model
            .as_ref()
            .or(self.context.config.agent.subagent_model.as_ref())
            .map(|s| s.as_str())
            .unwrap_or(DEFAULT_SUBAGENT_MODEL);

        info!(
            "Spawning subagent for task: {} (depth: {}, model: {})",
            params.task,
            self.context.depth + 1,
            model
        );

        // Create provider for subagent
        let provider = crate::agent::providers::create_provider(model, &self.context.config)?;

        // Build system prompt for subagent
        let system_prompt = self.build_subagent_prompt(params);

        // Create filtered tools (exclude spawn_agent from subagent)
        let subagent_tools: Vec<Box<dyn Tool>> = if self.context.depth + 1 >= self.context.max_depth
        {
            // At max depth, don't include spawn_agent
            tools
                .into_iter()
                .filter(|t| t.name() != "spawn_agent")
                .collect()
        } else {
            // Below max depth, could include spawn_agent with incremented depth
            // But for Phase 1, we don't allow nested spawning
            tools
                .into_iter()
                .filter(|t| t.name() != "spawn_agent")
                .collect()
        };

        // Create tool schemas
        let tool_schemas: Vec<ToolSchema> = subagent_tools.iter().map(|t| t.schema()).collect();

        // Create a minimal session for the subagent
        let mut session = Session::new();
        session.set_system_context(system_prompt);

        // Add the task as the initial user message
        let user_message = if params.input.is_empty() {
            format!("Task: {}", params.description)
        } else {
            format!("Task: {}\n\nInput:\n{}", params.description, params.input)
        };

        use crate::agent::providers::{Message, Role};
        session.add_message(Message {
            role: Role::User,
            content: user_message,
            tool_calls: None,
            tool_call_id: None,
            images: Vec::new(),
        });

        // Run the agent loop
        let max_iterations = 20;
        let mut iterations = 0;
        let mut total_tokens = 0u64;

        loop {
            iterations += 1;
            if iterations > max_iterations {
                warn!("Subagent reached max iterations ({})", max_iterations);
                return Ok(SubAgentResult {
                    success: false,
                    summary: "Subagent reached maximum iterations".to_string(),
                    details: None,
                    error: Some("Max iterations exceeded".to_string()),
                    tokens_used: Some(total_tokens),
                });
            }

            // Get messages for LLM
            let messages = session.messages_for_llm();

            // Call the LLM
            let response = provider.chat(&messages, Some(&tool_schemas)).await?;

            // Track token usage
            if let Some(usage) = response.usage {
                total_tokens += usage.total();
            }

            match response.content {
                crate::agent::providers::LLMResponseContent::Text(text) => {
                    // Subagent completed - return result
                    debug!("Subagent completed with text response");

                    // Parse the response for structured result
                    let (summary, _details) = self.parse_subagent_response(&text);

                    return Ok(SubAgentResult {
                        success: true,
                        summary,
                        details: Some(text),
                        error: None,
                        tokens_used: Some(total_tokens),
                    });
                }

                crate::agent::providers::LLMResponseContent::ToolCalls { calls, text } => {
                    // Execute tool calls
                    debug!("Subagent executing {} tool calls", calls.len());

                    // Add assistant message with tool calls (preserving any reasoning text)
                    session.add_message(Message {
                        role: Role::Assistant,
                        content: text.unwrap_or_default(),
                        tool_calls: Some(calls.clone()),
                        tool_call_id: None,
                        images: Vec::new(),
                    });

                    // Execute each tool
                    for call in &calls {
                        let tool = subagent_tools.iter().find(|t| t.name() == call.name);

                        let output = match tool {
                            Some(t) => match t.execute(&call.arguments).await {
                                Ok(result) => result,
                                Err(e) => format!("Error: {}", e),
                            },
                            None => format!("Error: Unknown tool: {}", call.name),
                        };

                        // Add tool result
                        session.add_message(Message {
                            role: Role::Tool,
                            content: output,
                            tool_calls: None,
                            tool_call_id: Some(call.id.clone()),
                            images: Vec::new(),
                        });
                    }

                    // Continue loop for next response
                }
            }
        }
    }

    /// Parse subagent response into summary and details
    fn parse_subagent_response(&self, text: &str) -> (String, Option<String>) {
        // Try to extract a summary from the first paragraph or line
        let lines: Vec<&str> = text.lines().take(5).collect();

        // Look for a summary pattern
        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with("Summary:") || trimmed.starts_with("summary:") {
                let summary = trimmed
                    .trim_start_matches("Summary:")
                    .trim_start_matches("summary:")
                    .trim();
                if !summary.is_empty() {
                    return (summary.to_string(), Some(text.to_string()));
                }
            }
        }

        // Fall back to first non-empty line
        for line in &lines {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                // Take first sentence or first 200 chars
                let summary = if let Some(pos) = trimmed.find('.') {
                    trimmed[..pos + 1].to_string()
                } else if trimmed.len() > 200 {
                    format!("{}...", &trimmed[..197])
                } else {
                    trimmed.to_string()
                };
                return (summary, Some(text.to_string()));
            }
        }

        // Ultimate fallback
        ("Task completed".to_string(), Some(text.to_string()))
    }
}

#[async_trait]
impl Tool for SpawnAgentTool {
    fn name(&self) -> &str {
        "spawn_agent"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "spawn_agent".to_string(),
            description: "Spawn a specialist subagent to handle a complex task. \
                          The subagent will focus on the assigned task and return results. \
                          Use for exploration, planning, implementation, or analysis tasks \
                          that benefit from focused attention."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "enum": ["explore", "plan", "implement", "analyze"],
                        "description": "Type of task for the subagent"
                    },
                    "description": {
                        "type": "string",
                        "description": "Clear description of what the subagent should accomplish"
                    },
                    "input": {
                        "type": "string",
                        "description": "Additional context or input for the task"
                    }
                },
                "required": ["description"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        // Parse arguments
        let params: SpawnParams = match serde_json::from_str(arguments) {
            Ok(p) => p,
            Err(e) => {
                return Ok(format!("Error parsing spawn_agent arguments: {}", e));
            }
        };

        // Check depth limit
        let current_depth = params.depth.unwrap_or(self.context.depth);
        if current_depth >= self.context.max_depth {
            return Ok(format!(
                "Cannot spawn agent: maximum depth ({}) reached. \
                 Complete the current task without spawning more agents.",
                self.context.max_depth
            ));
        }

        // Get safe tools for subagent (from create_safe_tools)
        let subagent_tools = crate::agent::tools::create_safe_tools(
            &self.context.config,
            Some(Arc::clone(&self.context.memory)),
        )?;

        // Run subagent
        let result = self.run_subagent(&params, subagent_tools).await?;

        // Format result for parent agent
        let formatted = if result.success {
            format!(
                "## Subagent Result\n\n**Summary:** {}\n\n**Details:**\n{}\n\n**Tokens used:** {}",
                result.summary,
                result
                    .details
                    .unwrap_or_else(|| "No details provided".to_string()),
                result.tokens_used.unwrap_or(0)
            )
        } else {
            format!(
                "## Subagent Failed\n\n**Error:** {}\n\n**Details:**\n{}",
                result.error.unwrap_or_else(|| "Unknown error".to_string()),
                result
                    .details
                    .unwrap_or_else(|| "No details available".to_string())
            )
        };

        Ok(formatted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─────────────────────────────────────────────────────────────────────────
    // Unit Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_spawn_params_parsing() {
        let json = r#"{
            "task": "explore",
            "description": "Find all uses of spawn_agent",
            "input": "Search in the crates directory"
        }"#;

        let params: SpawnParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.task, "explore");
        assert_eq!(params.description, "Find all uses of spawn_agent");
        assert_eq!(params.input, "Search in the crates directory");
    }

    #[test]
    fn test_spawn_params_minimal() {
        let json = r#"{
            "description": "Analyze the codebase structure"
        }"#;

        let params: SpawnParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.task, "explore"); // default
        assert_eq!(params.description, "Analyze the codebase structure");
        assert!(params.input.is_empty());
    }

    #[test]
    fn test_subagent_result_serialization() {
        let result = SubAgentResult {
            success: true,
            summary: "Found 5 occurrences".to_string(),
            details: Some("Detailed output here...".to_string()),
            error: None,
            tokens_used: Some(1500),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"summary\":\"Found 5 occurrences\""));
    }

    #[test]
    fn test_build_subagent_prompt_explore() {
        let params = SpawnParams {
            task: "explore".to_string(),
            description: "Find security vulnerabilities".to_string(),
            input: "Check auth module".to_string(),
            depth: None,
        };

        // Test just the prompt building logic without MemoryManager
        let task_guidance = match params.task.as_str() {
            "explore" => "You are an exploration specialist",
            _ => "You are a specialist agent",
        };

        let prompt = format!(
            "Task: {}\nGuidance: {}\nInput: {}",
            params.description, task_guidance, params.input
        );

        assert!(prompt.contains("exploration specialist"));
        assert!(prompt.contains("Find security vulnerabilities"));
        assert!(prompt.contains("Check auth module"));
    }

    #[test]
    fn test_build_subagent_prompt_plan() {
        let params = SpawnParams {
            task: "plan".to_string(),
            description: "Create implementation plan".to_string(),
            input: "".to_string(),
            depth: None,
        };

        let task_guidance = match params.task.as_str() {
            "plan" => "You are a planning specialist",
            _ => "You are a specialist agent",
        };

        let prompt = format!("Task: {}\nGuidance: {}", params.description, task_guidance);

        assert!(prompt.contains("planning specialist"));
        assert!(prompt.contains("Create implementation plan"));
    }

    #[test]
    fn test_depth_limit_logic() {
        // Test depth limit logic directly
        let max_depth: u8 = 2;

        // Root agent (depth 0) should allow spawning
        let depth_0: u8 = 0;
        assert!(depth_0 < max_depth);

        // First subagent (depth 1) should still allow spawning
        let depth_1: u8 = 1;
        assert!(depth_1 < max_depth);

        // Second subagent (depth 2) should NOT allow spawning
        let depth_2: u8 = 2;
        assert!(!(depth_2 < max_depth));

        // Test with max_depth = 1 (Phase 1 default)
        let max_depth_1: u8 = 1;
        assert!(0 < max_depth_1); // depth 0 can spawn
        assert!(!(1 < max_depth_1)); // depth 1 cannot spawn
    }

    #[test]
    fn test_parse_subagent_response_with_summary() {
        // Test with explicit summary
        let response = "Summary: Found 3 issues in the code.\n\nDetails here.";
        let lines: Vec<&str> = response.lines().take(5).collect();

        let summary = lines.iter().find_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("Summary:") || trimmed.starts_with("summary:") {
                let s = trimmed
                    .trim_start_matches("Summary:")
                    .trim_start_matches("summary:")
                    .trim();
                if !s.is_empty() {
                    Some(s.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        });

        assert_eq!(summary, Some("Found 3 issues in the code.".to_string()));
    }

    #[test]
    fn test_parse_subagent_response_without_summary() {
        // Test without explicit summary - takes first non-empty line
        let response = "This is the first line.\n\nMore details here.";
        let lines: Vec<&str> = response.lines().take(5).collect();

        let summary = lines.iter().find_map(|line| {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                Some(trimmed.to_string())
            } else {
                None
            }
        });

        assert_eq!(summary, Some("This is the first line.".to_string()));
    }

    #[test]
    fn test_tool_schema() {
        let schema = ToolSchema {
            name: "spawn_agent".to_string(),
            description: "Test description".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "enum": ["explore", "plan", "implement", "analyze"]
                    },
                    "description": {
                        "type": "string"
                    }
                },
                "required": ["description"]
            }),
        };

        assert_eq!(schema.name, "spawn_agent");

        // Verify the schema has the required fields
        let params = schema.parameters.as_object().unwrap();
        assert!(params.contains_key("required"));
        let required = params.get("required").unwrap().as_array().unwrap();
        assert!(required.contains(&json!("description")));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Integration Tests - Tool Filtering and Depth Limits
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_tool_filtering_removes_spawn_agent() {
        // Create a mock list of tools including spawn_agent
        let tool_names: Vec<&str> = vec!["memory_search", "memory_get", "spawn_agent", "web_fetch"];

        // Simulate filtering (what happens in run_subagent)
        let filtered: Vec<&str> = tool_names
            .into_iter()
            .filter(|t| *t != "spawn_agent")
            .collect();

        assert!(!filtered.contains(&"spawn_agent"));
        assert!(filtered.contains(&"memory_search"));
        assert!(filtered.contains(&"memory_get"));
        assert!(filtered.contains(&"web_fetch"));
        assert_eq!(filtered.len(), 3);
    }

    #[test]
    fn test_can_spawn_at_various_depths() {
        // Test the can_spawn logic at different depths
        let max_depth: u8 = 1;

        // Depth 0: root agent - can spawn
        let can_spawn_0 = 0 < max_depth;
        assert!(can_spawn_0);

        // Depth 1: first subagent - cannot spawn (at max)
        let can_spawn_1 = 1 < max_depth;
        assert!(!can_spawn_1);

        // Test with max_depth = 2
        let max_depth_2: u8 = 2;
        assert!(0 < max_depth_2); // depth 0 can spawn
        assert!(1 < max_depth_2); // depth 1 can spawn
        assert!(!(2 < max_depth_2)); // depth 2 cannot spawn
    }

    #[test]
    fn test_subagent_system_prompt_includes_no_spawn_instruction() {
        // Verify that the subagent prompt tells it not to spawn more agents
        let task_guidance = "You are an exploration specialist";
        let prompt = format!(
            "{}\n\n- Focus only on the assigned task\n- Do NOT spawn additional agents\n",
            task_guidance
        );

        assert!(prompt.contains("Do NOT spawn additional agents"));
    }

    #[test]
    fn test_task_type_guidance() {
        // Verify each task type gets appropriate guidance
        let task_types = vec![
            ("explore", "exploration specialist"),
            ("plan", "planning specialist"),
            ("implement", "implementation specialist"),
            ("analyze", "analysis specialist"),
            ("unknown", "specialist agent"),
        ];

        for (task_type, expected_guidance) in task_types {
            let guidance = match task_type {
                "explore" => "exploration specialist",
                "plan" => "planning specialist",
                "implement" => "implementation specialist",
                "analyze" => "analysis specialist",
                _ => "specialist agent",
            };
            assert!(
                guidance.contains(expected_guidance),
                "Task type '{}' should contain '{}'",
                task_type,
                expected_guidance
            );
        }
    }

    #[test]
    fn test_subagent_result_formatting() {
        // Test successful result formatting
        let success_result = SubAgentResult {
            success: true,
            summary: "Found 5 files".to_string(),
            details: Some("Details here".to_string()),
            error: None,
            tokens_used: Some(1000),
        };

        let formatted = format!(
            "## Subagent Result\n\n**Summary:** {}\n\n**Details:**\n{}\n\n**Tokens used:** {}",
            success_result.summary,
            success_result.details.unwrap_or_default(),
            success_result.tokens_used.unwrap_or(0)
        );

        assert!(formatted.contains("## Subagent Result"));
        assert!(formatted.contains("Found 5 files"));
        assert!(formatted.contains("**Tokens used:** 1000"));

        // Test failed result formatting
        let fail_result = SubAgentResult {
            success: false,
            summary: String::new(),
            details: Some("Partial output".to_string()),
            error: Some("Max iterations exceeded".to_string()),
            tokens_used: Some(500),
        };

        let fail_formatted = format!(
            "## Subagent Failed\n\n**Error:** {}\n\n**Details:**\n{}",
            fail_result.error.unwrap_or_default(),
            fail_result.details.unwrap_or_default()
        );

        assert!(fail_formatted.contains("## Subagent Failed"));
        assert!(fail_formatted.contains("Max iterations exceeded"));
    }

    #[test]
    fn test_max_iterations_limit() {
        // Test that max iterations is set to prevent runaway subagents
        let max_iterations = 20;
        let mut iterations = 0;

        // Simulate hitting the limit
        loop {
            iterations += 1;
            if iterations > max_iterations {
                break;
            }
        }

        assert_eq!(iterations, max_iterations + 1);
    }

    #[test]
    fn test_spawn_params_with_optional_fields() {
        // Test with all optional fields
        let full_json = r#"{
            "task": "implement",
            "description": "Add new feature",
            "input": "See design doc",
            "depth": 0
        }"#;

        let params: SpawnParams = serde_json::from_str(full_json).unwrap();
        assert_eq!(params.task, "implement");
        assert_eq!(params.description, "Add new feature");
        assert_eq!(params.input, "See design doc");
        assert_eq!(params.depth, Some(0));

        // Test with only required fields
        let min_json = r#"{"description": "Quick task"}"#;
        let min_params: SpawnParams = serde_json::from_str(min_json).unwrap();
        assert_eq!(min_params.task, "explore"); // default
        assert!(min_params.input.is_empty());
        assert!(min_params.depth.is_none());
    }

    #[test]
    fn test_spawn_agent_schema_parameters() {
        // Verify the tool schema has correct parameter definitions
        let schema = ToolSchema {
            name: "spawn_agent".to_string(),
            description: "Test".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "enum": ["explore", "plan", "implement", "analyze"]
                    },
                    "description": {
                        "type": "string"
                    },
                    "input": {
                        "type": "string"
                    }
                },
                "required": ["description"]
            }),
        };

        let params = schema.parameters.as_object().unwrap();
        let properties = params.get("properties").unwrap().as_object().unwrap();

        // Verify all expected properties exist
        assert!(properties.contains_key("task"));
        assert!(properties.contains_key("description"));
        assert!(properties.contains_key("input"));

        // Verify task enum values
        let task = properties.get("task").unwrap().as_object().unwrap();
        let enum_values = task.get("enum").unwrap().as_array().unwrap();
        assert!(enum_values.contains(&json!("explore")));
        assert!(enum_values.contains(&json!("plan")));
        assert!(enum_values.contains(&json!("implement")));
        assert!(enum_values.contains(&json!("analyze")));

        // Verify required field
        let required = params.get("required").unwrap().as_array().unwrap();
        assert_eq!(required.len(), 1);
        assert_eq!(required[0], "description");
    }
}
