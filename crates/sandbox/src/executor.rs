use anyhow::Result;
use std::time::Duration;

use crate::policy::SandboxPolicy;

/// Run a shell command inside the sandbox.
///
/// This is the parent-side function. It:
/// 1. Serializes the policy to JSON
/// 2. Re-execs the current binary with argv[0]="localgpt-sandbox"
/// 3. Passes policy + command as arguments
/// 4. Collects output and enforces timeout
pub async fn run_sandboxed(
    command: &str,
    policy: &SandboxPolicy,
    timeout_ms: u64,
) -> Result<(String, i32)> {
    let policy_json = serde_json::to_string(policy)?;

    // Get path to current executable for re-exec
    let exe_path = std::env::current_exe()?;

    // Build the child command:
    // argv[0] = "localgpt-sandbox" (sentinel for dispatch)
    // argv[1] = policy JSON
    // argv[2] = shell command to execute
    let timeout_duration = Duration::from_millis(timeout_ms);

    let output = tokio::time::timeout(
        timeout_duration,
        tokio::process::Command::new(&exe_path)
            .arg0("localgpt-sandbox")
            .arg(&policy_json)
            .arg(command)
            .current_dir(&policy.workspace_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("Sandboxed command timed out after {}ms", timeout_ms))??;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let max_bytes = policy.max_output_bytes as usize;

    let mut result = String::new();

    if !stdout.is_empty() {
        if stdout.len() > max_bytes {
            result.push_str(&stdout[..stdout.floor_char_boundary(max_bytes)]);
            result.push_str(&format!(
                "\n\n[Output truncated, {} bytes total]",
                stdout.len()
            ));
        } else {
            result.push_str(&stdout);
        }
    }

    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push_str("\n\nSTDERR:\n");
        }
        let remaining = max_bytes.saturating_sub(result.len());
        if stderr.len() > remaining && remaining > 0 {
            result.push_str(&stderr[..stderr.floor_char_boundary(remaining)]);
            result.push_str("\n[stderr truncated]");
        } else {
            result.push_str(&stderr);
        }
    }

    let exit_code = output.status.code().unwrap_or(-1);

    Ok((result, exit_code))
}

/// Trait extension for Command to set argv[0].
#[allow(dead_code)]
trait CommandExt {
    fn arg0(&mut self, arg0: &str) -> &mut Self;
}

#[cfg(unix)]
impl CommandExt for tokio::process::Command {
    fn arg0(&mut self, arg0: &str) -> &mut Self {
        use std::os::unix::process::CommandExt;
        self.as_std_mut().arg0(arg0);
        self
    }
}

#[cfg(not(unix))]
impl CommandExt for tokio::process::Command {
    fn arg0(&mut self, _arg0: &str) -> &mut Self {
        // argv[0] dispatch not supported on non-unix platforms
        self
    }
}
