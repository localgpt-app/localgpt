//! Agent write deny list for security-critical files.
//!
//! Blocks the agent from modifying policy files, the integrity manifest,
//! the device key, and the audit log via `write_file`, `edit_file`, or
//! `bash` tools.
//!
//! The `bash` tool check is heuristic and bypassable — full enforcement
//! requires OS-level sandboxing (Landlock/seccomp, separate RFC). The
//! tool-level check catches casual/accidental modifications and raises
//! the bar for injection attacks.

use std::path::Path;

/// Files in the workspace that the agent must not write to.
///
/// These are security-critical files whose integrity must be maintained
/// by the user (via CLI) or the security system itself — never by the
/// agent's tool calls.
pub const PROTECTED_FILES: &[&str] = &["LocalGPT.md", ".localgpt_manifest.json", "IDENTITY.md"];

/// Files outside the workspace (in the state directory) that the agent
/// must not access.
///
/// The device key and audit log live in `~/.local/state/localgpt/` (the state
/// directory), which is outside the workspace and not indexed by memory.
/// These paths are checked as filename suffixes for defense in depth.
pub const PROTECTED_EXTERNAL_PATHS: &[&str] = &["localgpt.device.key", "localgpt.audit.jsonl"];

/// Check if a workspace-relative filename is protected from agent writes.
///
/// Compares the final path component (filename) against the protected
/// files list. Case-sensitive match.
///
/// # Examples
///
/// ```
/// use localgpt_core::security::is_workspace_file_protected;
///
/// assert!(is_workspace_file_protected("LocalGPT.md"));
/// assert!(is_workspace_file_protected(".localgpt_manifest.json"));
/// assert!(!is_workspace_file_protected("MEMORY.md"));
/// ```
pub fn is_workspace_file_protected(filename: &str) -> bool {
    // Extract just the filename component if a path was passed
    let name = Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(filename);

    PROTECTED_FILES.contains(&name)
}

/// Check if an arbitrary path resolves to a protected file.
///
/// Handles absolute paths, relative paths, paths with `~` expansion,
/// and checks against both workspace-internal and external protected
/// paths.
pub fn is_path_protected(path: &str, workspace: &Path, state_dir: &Path) -> bool {
    let expanded = shellexpand::tilde(path);
    let expanded_path = Path::new(expanded.as_ref());

    // Check workspace-internal protected files
    if let Ok(canonical_workspace) = workspace.canonicalize()
        && let Ok(canonical_path) = expanded_path.canonicalize()
    {
        for &protected in PROTECTED_FILES {
            let protected_full = canonical_workspace.join(protected);
            if canonical_path == protected_full {
                return true;
            }
        }
    }

    // Fallback: check filename component directly
    if is_workspace_file_protected(path) {
        return true;
    }

    // Check external protected paths (state directory)
    if let Ok(canonical_state) = state_dir.canonicalize()
        && let Ok(canonical_path) = expanded_path.canonicalize()
    {
        for &protected in PROTECTED_EXTERNAL_PATHS {
            let protected_full = canonical_state.join(protected);
            if canonical_path == protected_full {
                return true;
            }
        }
    }

    // Fallback: check if path ends with any external protected filename
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path);
    PROTECTED_EXTERNAL_PATHS.contains(&name)
}

/// Best-effort check for bash commands that might write to protected files.
///
/// Scans the command string for protected filenames. This is a heuristic
/// that catches common patterns (`echo > LocalGPT.md`, `cp x LocalGPT.md`,
/// `sed -i ... LocalGPT.md`) but can be bypassed by obfuscation.
///
/// Returns a list of protected filenames found in the command.
pub fn check_bash_command(command: &str) -> Vec<&'static str> {
    let mut found = Vec::new();

    for &name in PROTECTED_FILES {
        if command.contains(name) {
            found.push(name);
        }
    }

    for &name in PROTECTED_EXTERNAL_PATHS {
        if command.contains(name) {
            found.push(name);
        }
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn workspace_files_protected() {
        assert!(is_workspace_file_protected("LocalGPT.md"));
        assert!(is_workspace_file_protected(".localgpt_manifest.json"));
        assert!(is_workspace_file_protected("IDENTITY.md"));
    }

    #[test]
    fn regular_files_not_protected() {
        assert!(!is_workspace_file_protected("MEMORY.md"));
        assert!(!is_workspace_file_protected("HEARTBEAT.md"));
        assert!(!is_workspace_file_protected("SOUL.md"));
        assert!(!is_workspace_file_protected("config.toml"));
        assert!(!is_workspace_file_protected("memory/2024-01-15.md"));
    }

    #[test]
    fn path_with_directory_checks_filename() {
        assert!(is_workspace_file_protected("workspace/LocalGPT.md"));
        assert!(is_workspace_file_protected(
            "/home/user/.local/share/localgpt/workspace/IDENTITY.md"
        ));
    }

    #[test]
    fn bash_command_detection() {
        let hits = check_bash_command("echo 'new rules' > LocalGPT.md");
        assert!(hits.contains(&"LocalGPT.md"));

        let hits = check_bash_command("cat localgpt.device.key");
        assert!(hits.contains(&"localgpt.device.key"));

        let hits = check_bash_command("ls -la");
        assert!(hits.is_empty());
    }

    #[test]
    fn path_protected_with_real_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().join("workspace");
        let state_dir = tmp.path().join("state");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&state_dir).unwrap();

        // Create the protected file so canonicalize works
        let policy = workspace.join("LocalGPT.md");
        fs::write(&policy, "test").unwrap();

        assert!(is_path_protected(
            policy.to_str().unwrap(),
            &workspace,
            &state_dir
        ));

        // Non-protected file
        let memory = workspace.join("MEMORY.md");
        fs::write(&memory, "test").unwrap();
        assert!(!is_path_protected(
            memory.to_str().unwrap(),
            &workspace,
            &state_dir
        ));
    }

    /// Verify the agent write-deny boundary: LocalGPT.md and manifest
    /// are always protected from agent writes, while user-editable files
    /// (MEMORY.md, SOUL.md, HEARTBEAT.md) remain accessible.
    ///
    /// This test documents that the mobile-ffi's file editor API does NOT
    /// weaken the agent protection — only the user (via mobile UI or CLI)
    /// can write to security-critical files.
    #[test]
    fn agent_protection_boundary_enforced() {
        // Security-critical files the agent must NEVER write to
        let agent_blocked = &["LocalGPT.md", ".localgpt_manifest.json", "IDENTITY.md"];
        for &file in agent_blocked {
            assert!(
                is_workspace_file_protected(file),
                "Agent must be blocked from writing to {}",
                file
            );
        }

        // User-editable files the agent CAN write to via tools
        let agent_allowed = &["MEMORY.md", "SOUL.md", "HEARTBEAT.md"];
        for &file in agent_allowed {
            assert!(
                !is_workspace_file_protected(file),
                "Agent should be allowed to write to {}",
                file
            );
        }

        // External security files must also be protected
        let external_blocked = &["localgpt.device.key", "localgpt.audit.jsonl"];
        for &file in external_blocked {
            assert!(
                PROTECTED_EXTERNAL_PATHS.contains(&file),
                "External file {} must be protected",
                file
            );
        }
    }
}
