//! CLI-only tools: bash, read_file, write_file, edit_file.
//!
//! These tools are not included in `localgpt-core` because they have
//! platform-specific dependencies (sandbox) and security implications
//! that make them unsuitable for mobile.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;
use tracing::debug;

use localgpt_core::agent::hardcoded_filters;
use localgpt_core::agent::path_utils::{check_path_allowed, resolve_real_path};
use localgpt_core::agent::providers::ToolSchema;
use localgpt_core::agent::tool_filters::CompiledToolFilter;
use localgpt_core::agent::tools::Tool;
use localgpt_core::config::Config;
use localgpt_core::security;
use localgpt_sandbox::{self, SandboxPolicy};

/// Compile a tool filter from config (if present), then merge hardcoded defaults.
fn compile_filter_for(
    config: &Config,
    tool_name: &str,
    hardcoded_subs: &[&str],
    hardcoded_pats: &[&str],
) -> Result<CompiledToolFilter> {
    let base = config
        .tools
        .filters
        .get(tool_name)
        .map(CompiledToolFilter::compile)
        .unwrap_or_else(|| Ok(CompiledToolFilter::permissive()))?;
    base.merge_hardcoded(hardcoded_subs, hardcoded_pats)
}

/// Canonicalize configured allowed_directories into absolute paths.
fn resolve_allowed_directories(config: &Config) -> Vec<PathBuf> {
    config
        .security
        .allowed_directories
        .iter()
        .filter_map(|d| {
            let expanded = shellexpand::tilde(d).to_string();
            match fs::canonicalize(&expanded) {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::warn!("Ignoring non-existent allowed_directory '{}': {}", d, e);
                    None
                }
            }
        })
        .collect()
}

/// Create just the CLI-specific dangerous tools (bash, read_file, write_file, edit_file).
///
/// Use with `agent.extend_tools()` after `Agent::new()` to add these to an
/// agent that already has safe tools.
pub fn create_cli_tools(config: &Config) -> Result<Vec<Box<dyn Tool>>> {
    let workspace = config.workspace_path();
    let state_dir = config.paths.state_dir.clone();

    // Build sandbox policy if enabled
    let sandbox_policy = if config.sandbox.enabled {
        let caps = localgpt_sandbox::detect_capabilities();
        let effective = caps.effective_level(&config.sandbox.level);
        if effective > localgpt_sandbox::SandboxLevel::None {
            Some(localgpt_sandbox::build_policy(
                &config.sandbox,
                &workspace,
                effective,
            ))
        } else {
            tracing::warn!(
                "Sandbox enabled but no kernel support detected (level: {:?}). \
                 Commands will run without sandbox enforcement.",
                caps.level
            );
            None
        }
    } else {
        None
    };

    // Compile per-tool filters with hardcoded defaults
    let bash_filter = compile_filter_for(
        config,
        "bash",
        hardcoded_filters::BASH_DENY_SUBSTRINGS,
        hardcoded_filters::BASH_DENY_PATTERNS,
    )?;

    // File tools get no hardcoded filters (path scoping handles security)
    let file_filter = compile_filter_for(config, "file", &[], &[])?;
    let allowed_dirs = resolve_allowed_directories(config);
    let strict_policy = config.security.strict_policy;

    Ok(vec![
        Box::new(BashTool::new(
            config.tools.bash_timeout_ms,
            state_dir.clone(),
            sandbox_policy.clone(),
            bash_filter,
            strict_policy,
        )),
        Box::new(ReadFileTool::new(
            sandbox_policy.clone(),
            file_filter.clone(),
            allowed_dirs.clone(),
            state_dir.clone(),
        )),
        Box::new(WriteFileTool::new(
            state_dir.clone(),
            sandbox_policy.clone(),
            file_filter.clone(),
            allowed_dirs.clone(),
        )),
        Box::new(EditFileTool::new(
            state_dir,
            sandbox_policy,
            file_filter,
            allowed_dirs,
        )),
    ])
}

// Bash Tool
pub struct BashTool {
    default_timeout_ms: u64,
    state_dir: PathBuf,
    sandbox_policy: Option<SandboxPolicy>,
    filter: CompiledToolFilter,
    strict_policy: bool,
}

impl BashTool {
    pub fn new(
        default_timeout_ms: u64,
        state_dir: PathBuf,
        sandbox_policy: Option<SandboxPolicy>,
        filter: CompiledToolFilter,
        strict_policy: bool,
    ) -> Self {
        Self {
            default_timeout_ms,
            state_dir,
            sandbox_policy,
            filter,
            strict_policy,
        }
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "bash".to_string(),
            description: "Execute a bash command and return the output".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": format!("Optional timeout in milliseconds (default: {})", self.default_timeout_ms)
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let command = args["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing command"))?;

        let timeout_ms = args["timeout_ms"]
            .as_u64()
            .unwrap_or(self.default_timeout_ms);

        // Check command against deny/allow filters
        self.filter.check(command, "bash", "command")?;

        // Best-effort protected file check for bash commands
        let suspicious = security::check_bash_command(command);
        if !suspicious.is_empty() {
            let detail = format!(
                "Bash command references protected files: {:?} (cmd: {})",
                suspicious,
                &command[..command.floor_char_boundary(command.len().min(200))]
            );
            let _ = security::append_audit_entry_with_detail(
                &self.state_dir,
                security::AuditAction::WriteBlocked,
                "",
                "tool:bash",
                Some(&detail),
            );
            if self.strict_policy {
                anyhow::bail!(
                    "Blocked: command references protected files: {:?}",
                    suspicious
                );
            }
            tracing::warn!("Bash command may modify protected files: {:?}", suspicious);
        }

        debug!(
            "Executing bash command (timeout: {}ms): {}",
            timeout_ms, command
        );

        // Use sandbox if policy is configured
        if let Some(ref policy) = self.sandbox_policy {
            let (output, exit_code) =
                localgpt_sandbox::run_sandboxed(command, policy, timeout_ms).await?;

            if output.is_empty() {
                return Ok(format!("Command completed with exit code: {}", exit_code));
            }

            return Ok(output);
        }

        // Fallback: run command directly without sandbox
        let timeout_duration = std::time::Duration::from_millis(timeout_ms);
        let output = tokio::time::timeout(
            timeout_duration,
            tokio::process::Command::new("bash")
                .arg("-c")
                .arg(command)
                .output(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Command timed out after {}ms", timeout_ms))??;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();

        if !stdout.is_empty() {
            result.push_str(&stdout);
        }

        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n\nSTDERR:\n");
            }
            result.push_str(&stderr);
        }

        if result.is_empty() {
            result = format!(
                "Command completed with exit code: {}",
                output.status.code().unwrap_or(-1)
            );
        }

        Ok(result)
    }
}

// Read File Tool
pub struct ReadFileTool {
    sandbox_policy: Option<SandboxPolicy>,
    filter: CompiledToolFilter,
    allowed_directories: Vec<PathBuf>,
    state_dir: PathBuf,
}

impl ReadFileTool {
    pub fn new(
        sandbox_policy: Option<SandboxPolicy>,
        filter: CompiledToolFilter,
        allowed_directories: Vec<PathBuf>,
        state_dir: PathBuf,
    ) -> Self {
        Self {
            sandbox_policy,
            filter,
            allowed_directories,
            state_dir,
        }
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read_file".to_string(),
            description: "Read the contents of a file".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to read"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Line number to start reading from (0-indexed)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of lines to read"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;

        // Resolve symlinks and check path scoping
        let real_path = resolve_real_path(path)?;
        let real_path_str = real_path.to_string_lossy();
        self.filter.check(&real_path_str, "read_file", "path")?;
        if let Err(e) = check_path_allowed(&real_path, &self.allowed_directories) {
            let detail = format!("read_file denied: {}", real_path.display());
            let _ = security::append_audit_entry_with_detail(
                &self.state_dir,
                security::AuditAction::PathDenied,
                "",
                "tool:read_file",
                Some(&detail),
            );
            return Err(e);
        }

        // Check credential directory access
        if let Some(ref policy) = self.sandbox_policy
            && localgpt_sandbox::policy::is_path_denied(&real_path, policy)
        {
            anyhow::bail!(
                "Cannot read file in denied directory: {}. \
                     This path is blocked by sandbox policy.",
                real_path.display()
            );
        }

        debug!("Reading file: {}", real_path.display());

        let content = fs::read_to_string(&real_path)?;

        // Handle offset and limit
        let offset = args["offset"].as_u64().unwrap_or(0) as usize;
        let limit = args["limit"].as_u64().map(|l| l as usize);

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start = offset.min(total_lines);
        let end = limit
            .map(|l| (start + l).min(total_lines))
            .unwrap_or(total_lines);

        let selected: Vec<String> = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:4}\t{}", start + i + 1, line))
            .collect();

        Ok(selected.join("\n"))
    }
}

// Write File Tool
pub struct WriteFileTool {
    state_dir: PathBuf,
    sandbox_policy: Option<SandboxPolicy>,
    filter: CompiledToolFilter,
    allowed_directories: Vec<PathBuf>,
}

impl WriteFileTool {
    pub fn new(
        state_dir: PathBuf,
        sandbox_policy: Option<SandboxPolicy>,
        filter: CompiledToolFilter,
        allowed_directories: Vec<PathBuf>,
    ) -> Self {
        Self {
            state_dir,
            sandbox_policy,
            filter,
            allowed_directories,
        }
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write_file".to_string(),
            description: "Write content to a file (creates or overwrites)".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
        let content = args["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing content"))?;

        // Resolve symlinks and check path scoping
        let real_path = resolve_real_path(path)?;
        let real_path_str = real_path.to_string_lossy();
        self.filter.check(&real_path_str, "write_file", "path")?;
        if let Err(e) = check_path_allowed(&real_path, &self.allowed_directories) {
            let detail = format!("write_file denied: {}", real_path.display());
            let _ = security::append_audit_entry_with_detail(
                &self.state_dir,
                security::AuditAction::PathDenied,
                "",
                "tool:write_file",
                Some(&detail),
            );
            return Err(e);
        }

        // Check credential directory access
        if let Some(ref policy) = self.sandbox_policy
            && localgpt_sandbox::policy::is_path_denied(&real_path, policy)
        {
            anyhow::bail!(
                "Cannot write to denied directory: {}. \
                     This path is blocked by sandbox policy.",
                real_path.display()
            );
        }

        // Check protected files
        if let Some(name) = real_path.file_name().and_then(|n| n.to_str())
            && security::is_workspace_file_protected(name)
        {
            let detail = format!("Agent attempted write to {}", real_path.display());
            let _ = security::append_audit_entry_with_detail(
                &self.state_dir,
                security::AuditAction::WriteBlocked,
                "",
                "tool:write_file",
                Some(&detail),
            );
            anyhow::bail!(
                "Cannot write to protected file: {}. This file is managed by the security system. \
                     Use `localgpt md sign` to update the security policy.",
                real_path.display()
            );
        }

        debug!("Writing file: {}", real_path.display());

        // Create parent directories if needed
        if let Some(parent) = real_path.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&real_path, content)?;

        Ok(format!(
            "Successfully wrote {} bytes to {}",
            content.len(),
            real_path.display()
        ))
    }
}

// Edit File Tool
pub struct EditFileTool {
    state_dir: PathBuf,
    sandbox_policy: Option<SandboxPolicy>,
    filter: CompiledToolFilter,
    allowed_directories: Vec<PathBuf>,
}

impl EditFileTool {
    pub fn new(
        state_dir: PathBuf,
        sandbox_policy: Option<SandboxPolicy>,
        filter: CompiledToolFilter,
        allowed_directories: Vec<PathBuf>,
    ) -> Self {
        Self {
            state_dir,
            sandbox_policy,
            filter,
            allowed_directories,
        }
    }
}

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "edit_file".to_string(),
            description: "Edit a file by replacing old_string with new_string".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The path to the file to edit"
                    },
                    "old_string": {
                        "type": "string",
                        "description": "The text to replace"
                    },
                    "new_string": {
                        "type": "string",
                        "description": "The replacement text"
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "Replace all occurrences (default: false)"
                    }
                },
                "required": ["path", "old_string", "new_string"]
            }),
        }
    }

    async fn execute(&self, arguments: &str) -> Result<String> {
        let args: Value = serde_json::from_str(arguments)?;
        let path = args["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
        let old_string = args["old_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing old_string"))?;
        let new_string = args["new_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing new_string"))?;
        let replace_all = args["replace_all"].as_bool().unwrap_or(false);

        // Resolve symlinks and check path scoping
        let real_path = resolve_real_path(path)?;
        let real_path_str = real_path.to_string_lossy();
        self.filter.check(&real_path_str, "edit_file", "path")?;
        if let Err(e) = check_path_allowed(&real_path, &self.allowed_directories) {
            let detail = format!("edit_file denied: {}", real_path.display());
            let _ = security::append_audit_entry_with_detail(
                &self.state_dir,
                security::AuditAction::PathDenied,
                "",
                "tool:edit_file",
                Some(&detail),
            );
            return Err(e);
        }

        // Check credential directory access
        if let Some(ref policy) = self.sandbox_policy
            && localgpt_sandbox::policy::is_path_denied(&real_path, policy)
        {
            anyhow::bail!(
                "Cannot edit file in denied directory: {}. \
                     This path is blocked by sandbox policy.",
                real_path.display()
            );
        }

        // Check protected files
        if let Some(name) = real_path.file_name().and_then(|n| n.to_str())
            && security::is_workspace_file_protected(name)
        {
            let detail = format!("Agent attempted edit to {}", real_path.display());
            let _ = security::append_audit_entry_with_detail(
                &self.state_dir,
                security::AuditAction::WriteBlocked,
                "",
                "tool:edit_file",
                Some(&detail),
            );
            anyhow::bail!(
                "Cannot edit protected file: {}. This file is managed by the security system.",
                real_path.display()
            );
        }

        debug!("Editing file: {}", real_path.display());

        let content = fs::read_to_string(&real_path)?;

        let (new_content, count) = if replace_all {
            let count = content.matches(old_string).count();
            (content.replace(old_string, new_string), count)
        } else if content.contains(old_string) {
            (content.replacen(old_string, new_string, 1), 1)
        } else {
            return Err(anyhow::anyhow!("old_string not found in file"));
        };

        fs::write(&real_path, &new_content)?;

        Ok(format!(
            "Replaced {} occurrence(s) in {}",
            count,
            real_path.display()
        ))
    }
}
